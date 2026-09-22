use super::*;

#[test]
#[ignore = "native GTK save dialog and GPU; run on an isolated Wayland display"]
#[allow(deprecated)]
fn native_stroke_recording() {
    use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
    glib::set_prgname(Some("capy-stroke-recording-test"));
    let app = native_test_app("art.capycanvas.StrokeRecordingTest");
    let w = Workspace::with_project(&app, Some((new_drawing(384, 256).unwrap(), None)));
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !w
        .gpu
        .borrow()
        .as_ref()
        .is_some_and(|g| g.session.engine().backend().startup.complete)
    {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    let button = find_named(w.effects.stats.upcast_ref(), "stroke-recording")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    assert_eq!(
        w.effects.stats.last_child().as_ref(),
        Some(button.upcast_ref())
    );
    button.emit_clicked();
    assert_eq!(button.label().as_deref(), Some("Stop stroke recording"));
    for i in 0..22 {
        let now = glib::monotonic_time() as u64 * 1000;
        {
            let mut gpu = w.gpu.borrow_mut();
            let g = gpu.as_mut().unwrap();
            let camera = &g.session.state().camera;
            let event = PenEvent {
                device_id: 7,
                sequence: i,
                timestamp_ns: now,
                view_revision: camera.revision,
                surface_position: layer_core::Point {
                    x: 500. + i as f32 * 4.,
                    y: 450.,
                },
                pressure: 0.6,
                tilt_radians: [0.2, 0.3],
                twist_radians: 0.4,
                distance: 0.,
                phase: if i == 0 {
                    PenPhase::Down
                } else if i == 21 {
                    PenPhase::Up
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            g.session.pen(event).unwrap();
        }
        w.wake();
        pump(12);
    }
    let chooser = || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            pump(30);
            if let Some(d) = gtk::Window::list_toplevels()
                .into_iter()
                .find_map(|w| w.downcast::<gtk::FileChooserDialog>().ok())
                .filter(|d| d.is_visible())
            {
                pump(300);
                return d;
            }
            assert!(Instant::now() < deadline, "save chooser did not open");
        }
    };
    button.emit_clicked();
    chooser().response(gtk::ResponseType::Cancel);
    pump(300);
    assert_eq!(button.label().as_deref(), Some("Save stroke recording"));
    assert!(
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .stroke_recording()
            .status()
            .ready
    );
    button.emit_clicked();
    let dialog = chooser();
    let folder = std::env::temp_dir().join(format!("capy-recording-test-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    dialog
        .set_current_folder(Some(&gtk::gio::File::for_path(&folder)))
        .unwrap();
    dialog.set_current_name("gtk.capystrokes");
    pump(300);
    dialog.response(gtk::ResponseType::Accept);
    let deadline = Instant::now() + Duration::from_secs(10);
    while button.label().as_deref() != Some("Start stroke recording") {
        pump(30);
        assert!(Instant::now() < deadline);
    }
    let records =
        layer_engine::recording::read(std::fs::File::open(folder.join("gtk.capystrokes")).unwrap())
            .unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(r, layer_engine::recording::Record::Raw { .. }))
            .count(),
        22
    );
    assert!(records.iter().any(|r| matches!(
        r,
        layer_engine::recording::Record::Predictor(layer_engine::recording::Event::Query(..))
    )));
    println!(
        "STROKE_RECORDING_FILE={}",
        folder.join("gtk.capystrokes").display()
    );
    w.window.close();
    pump(100);
}
