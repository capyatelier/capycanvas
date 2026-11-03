//! Real GApplication argument forwarding on the private session bus. The sender
//! runs production application setup; the receiver inspects native documents.
use super::*;
use gtk::gio;
use layer_core::color::{ColorProfile, SampleDepth, source::*};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

#[test]
#[ignore = "secondary process for native_application_file_launch"]
fn application_file_launch_sender() {
    let Ok(id) = std::env::var("LAYER_FILE_LAUNCH_APP_ID") else {
        return;
    };
    let files: Vec<String> =
        serde_json::from_str(&std::env::var("LAYER_FILE_LAUNCH_ARGS").unwrap()).unwrap();
    let (app, _) = crate::application(&id);
    let args = std::iter::once("capycanvas".to_owned())
        .chain(files)
        .collect::<Vec<_>>();
    assert_eq!(app.run_with_args(&args), glib::ExitCode::SUCCESS);
}

struct Sender(Child);
impl Sender {
    fn new(id: &str, files: &[&Path]) -> Self {
        let files: Vec<_> = files.iter().map(|p| p.to_str().unwrap()).collect();
        Self(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "application_file_launch_sender",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("LAYER_FILE_LAUNCH_APP_ID", id)
                .env(
                    "LAYER_FILE_LAUNCH_ARGS",
                    serde_json::to_string(&files).unwrap(),
                )
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn finish(&mut self) {
        until(
            || self.0.try_wait().unwrap().is_some(),
            "secondary application completed",
        );
        assert!(self.0.wait().unwrap().success());
    }
}
impl Drop for Sender {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn launch_window(app: &adw::Application) -> Option<adw::ApplicationWindow> {
    app.windows()
        .into_iter()
        .find(|w| w.widget_name() == "file-launch-window")
        .and_then(|w| w.downcast().ok())
}

fn admission_input(w: &Rc<Workspace>) {
    new_photo::invoke(w, CommandId::Pen);
    w.dispatch(UiAction::SetBrushSize { value: 16. });
    new_photo::ready(w);
    let before = place_source::authored_snapshot(w);
    let strokes = ui_session(w).engine().metrics().committed_strokes;
    w.documents.changing.set(true);
    w.dispatch(UiAction::Invoke { command: CommandId::AddLayer });
    native_pen_path(w, &[[32., 80.], [80., 80.], [128., 80.]]);
    w.documents.changing.set(false);
    new_photo::ready(w);
    assert_eq!(place_source::authored_snapshot(w), before);
    assert_eq!(ui_session(w).engine().metrics().committed_strokes, strokes);
    native_pen_path(w, &[[32., 80.], [80., 80.], [128., 80.]]);
    new_photo::ready(w);
    assert_eq!(ui_session(w).engine().metrics().committed_strokes, strokes + 1);
    assert_ne!(place_source::authored_snapshot(w), before);
    for finish in [PenPhase::Up, PenPhase::Cancel] {
        let camera = state(w).camera;
        for (phase, point) in [(PenPhase::Down, [32., 112.]), (PenPhase::Move, [80., 112.])] {
            w.input.send(w, PenEvent { device_id: 92,
                ..pen_event(&camera, point, phase, glib::monotonic_time() as u64 * 1000) });
            pump(20);
        }
        assert!(!ui_session(w).can_park_document());
        w.documents.changing.set(true);
        w.input.send(w, PenEvent { device_id: 92,
            ..pen_event(&camera, [128., 112.], finish, glib::monotonic_time() as u64 * 1000) });
        w.documents.changing.set(false);
        until(|| ui_session(w).can_park_document(), "finishing pen record retires contact during admission");
    }
}

#[test]
#[ignore = "isolated Wayland/session bus and hardware GPU"]
fn native_application_file_launch() {
    adw::init().unwrap();
    let id = format!("art.capycanvas.FileLaunch.p{}", std::process::id());
    let (app, windows) = crate::application(&id);
    let app = NativeTestApp(app);
    app.register(None::<&gio::Cancellable>).unwrap();
    assert!(!app.is_remote());
    assert!(windows.borrow().is_empty() && app.windows().is_empty());
    let settings_path = crate::storage::roots().unwrap().settings();
    assert!(settings_path.starts_with(std::env::temp_dir()));
    std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    let mut settings = layer_ui::Settings {
        theme: match std::env::var("CAPY_NATIVE_TEST_THEME").as_deref() {
            Ok("light") => Some(Theme::Light),
            Ok("dark") => Some(Theme::Dark),
            Ok(_) => panic!("CAPY_NATIVE_TEST_THEME must be light or dark"),
            Err(_) => None,
        },
        ..Default::default()
    };
    settings.photo_open.promote_to_16 = true;
    settings.photo_open.missing_profile = layer_ui::MissingProfilePolicy::Ask;
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let activated = Rc::new(Cell::new(false));
    app.connect_window_added(glib::clone!(
        #[strong]
        activated,
        move |app, window| {
            let application = app.downgrade();
            let activated = activated.clone();
            window.connect_map(move |window| {
                if window.widget_name() == "file-launch-window" && !activated.replace(true) {
                    application.upgrade().unwrap().activate();
                }
            });
        }
    ));
    let directory = PathBuf::from(std::env::var_os("LAYER_FILE_LAUNCH_OUTPUT").unwrap_or_else(
        || {
            std::env::temp_dir()
                .join(format!("capy-file-launch-{}", std::process::id()))
                .into()
        },
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let photo = directory.join("Photograph with spaces.jpg");
    let mut source = SourceBuilder::new(
        [96, 64],
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: ColorProfile::default(),
            profile_assumed: false,
        },
        1 << 20,
    )
    .unwrap();
    for y in 0..64 {
        source
            .push_row(
                &(0..96)
                    .flat_map(|x| [x as u8 * 2, y as u8 * 3, 155])
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    layer_color::photo::write_jpeg(
        std::fs::File::create(&photo).unwrap(),
        &source.finish().unwrap(),
        layer_color::photo::JpegEncodeOptions::from_memory_budget(95, layer_color::photo::PhotoMemoryBudget::from_limit(layer_core::MemoryBudget::process().import_headroom_bytes())),
    )
    .unwrap();
    let expected = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(&photo).unwrap()),
        layer_ui::photo_decode_limits(),
    )
    .unwrap();
    let master = directory.join("Master drawing.capy");
    let project = new_drawing(321, 217, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    write_document(&project, &mut std::fs::File::create(&master).unwrap()).unwrap();
    let photo_before = std::fs::read(&photo).unwrap();
    let master_before = std::fs::read(&master).unwrap();

    // Cold multi-file launch creates one drawing window and no blank tab.
    let mut sender = Sender::new(&id, &[&photo, &master]);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if windows.borrow().first().is_some_and(|w| {
            w.documents.len() == 2 && !w.documents.changing.get() && !w.documents.loading.get()
        }) && launch_window(&app).is_none() { break; }
        assert!(Instant::now() < deadline, "cold launch creates tabs: documents {:?}; native windows {:?}",
            windows.borrow().iter().map(|w| (w.documents.len(), w.documents.selected(),
                w.documents.changing.get(), w.documents.loading.get(), w.servicing.get(),
                w.gpu.borrow().as_ref().map(|g| (g.session.state().host_error.clone(), g.session.state().document_file.clone())))).collect::<Vec<_>>(),
            app.windows().iter().map(|w| (w.widget_name(), w.is_visible(), w.downcast_ref::<adw::ApplicationWindow>()
                .and_then(|w| w.visible_dialog()).map(|d| d.widget_name()))).collect::<Vec<_>>());
        pump(10);
    }
    sender.finish();
    let w = windows.borrow()[0].clone();
    new_photo::ready(&w);
    assert_eq!(app.windows().len(), 1);
    assert_eq!(windows.borrow().len(), 1);
    assert!(activated.get());
    let second = w.documents.selected();
    assert_live_artwork_eq(ui_session(&w).engine().document(), &project);
    assert_eq!(
        state(&w).document_file.location.unwrap().uri,
        gio::File::for_path(&master).uri()
    );
    admission_input(&w);
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, 1))
        .unwrap();
    new_photo::ready(&w);
    {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().unwrap().session;
        let doc = session.engine().document();
        assert_eq!([doc.composition().size[0], doc.composition().size[1]], expected.extent);
        assert_eq!(doc.composition().color.depth, SampleDepth::U16);
        assert_source_samples(paint_at(doc, 0).base.as_ref().map(|base|base.image.as_ref()).unwrap(), &expected);
        assert!(session.state().document_file.location.is_none());
    }
    new_photo::invoke(&w, CommandId::AddLayer);
    let edited = place_source::authored_snapshot(&w);
    let mut sender = Sender::new(&id, &[&photo]);
    until(
        || w.documents.len() == 3 && !w.documents.changing.get() && !w.documents.loading.get(),
        "warm file launch",
    );
    sender.finish();
    assert_eq!(app.windows().len(), 1);
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, 1))
        .unwrap();
    new_photo::ready(&w);
    assert_eq!(place_source::authored_snapshot(&w), edited);
    // Error acknowledgement continues a batch in the receiving window.
    let bad = directory.join("Broken picture.jpg");
    std::fs::write(&bad, b"not an image").unwrap();
    let mut sender = Sender::new(&id, &[&bad, &master]);
    until(
        || {
            w.window
                .visible_dialog()
                .is_some_and(|d| d.widget_name() == "file-launch-error")
        },
        "failed file explanation",
    );
    assert_eq!(app.windows().len(), 1);
    new_photo::response(&w, "ok");
    until(
        || w.documents.len() == 4 && !w.documents.changing.get() && !w.documents.loading.get(),
        "batch continues",
    );
    sender.finish();
    // A second explicit window remains possible; its opens target that window.
    app.activate_action("new-window", None);
    until(|| windows.borrow().len() == 2, "explicit new window");
    let other = windows.borrow()[1].clone();
    new_photo::ready(&other);
    let blank = other.documents.selected();
    let blank_document = place_source::authored_snapshot(&other);
    crate::files::launch::open_in(&other, vec![gio::File::for_path(&master)]);
    until(
        || other.documents.len() == 2 && !other.documents.changing.get(),
        "pinned launch target",
    );
    assert_eq!(w.documents.len(), 4);
    assert_eq!(app.windows().len(), 2);
    glib::MainContext::default().block_on(other.documents.activate(&other, blank)).unwrap();
    new_photo::ready(&other);
    assert_eq!(place_source::authored_snapshot(&other), blank_document);
    // Closing during preparation cancels the batch; late results never create a window.
    let progress_seen = Rc::new(Cell::new(false));
    let observed = progress_seen.clone();
    let closed_while_loading = Rc::new(Cell::new(false));
    let closed = closed_while_loading.clone();
    let workspace = Rc::downgrade(&other);
    let signal = other
        .window
        .connect_notify_local(Some("visible-dialog"), move |window, _| {
            if window
                .visible_dialog()
                .is_some_and(|d| d.widget_name() == "document-open-progress")
            {
                observed.set(true);
                let workspace = workspace.clone();
                let closed = closed.clone();
                glib::idle_add_local_full(glib::Priority::HIGH, move || {
                    if let Some(workspace) = workspace.upgrade() {
                        closed.set(workspace.documents.loading.get());
                        workspace.window.close();
                    }
                    glib::ControlFlow::Break
                });
            }
        });
    crate::files::launch::open_in(
        &other,
        vec![gio::File::for_path(&photo), gio::File::for_path(&master)],
    );
    until(|| progress_seen.get(), "incoming batch begins preparation");
    let deadline = Instant::now() + Duration::from_secs(30);
    while other.documents.loading.get() || other.documents.changing.get() || other.window.visible_dialog().is_some() {
        assert!(Instant::now() < deadline,
            "close cancels incoming batch: progress {}, loading {}, changing {}, close pending {}, closing {}, cancelled {}, dialog {:?}, document {:?}, error {:?}",
            progress_seen.get(), other.documents.loading.get(), other.documents.changing.get(),
            other.documents.close_pending.get(), other.documents.closing_window.get(), other.documents.cancel_open.get(),
            other.window.visible_dialog().map(|dialog| dialog.widget_name()), state(&other).document_file, state(&other).host_error);
        pump(10);
    }
    assert!(progress_seen.get() && closed_while_loading.get());
    pump(100);
    assert!(other.window.is_visible());
    assert_eq!(other.documents.len(), 2);
    assert_eq!(other.documents.selected(), blank);
    assert_eq!(place_source::authored_snapshot(&other), blank_document);
    other.window.disconnect(signal);
    other.window.close();
    until(|| !other.window.is_visible(), "close after cancellation");
    assert_eq!(windows.borrow().len(), 1);
    assert_eq!(std::fs::read(&photo).unwrap(), photo_before);
    assert_eq!(std::fs::read(&master).unwrap(), master_before);
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, second))
        .unwrap();
    new_photo::ready(&w);
    crate::capture(&w, directory.join("opened-tabs.png").to_str().unwrap());
    w.window.destroy();
    windows.borrow_mut().clear();
    pump(100);
}
