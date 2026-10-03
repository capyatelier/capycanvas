//! Actual input, worker and presenter ownership, on an isolated GTK display.
use super::*;

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_gpu_tone_retains_preview_cancels_and_refreshes_after_drawing() {
    let app = native_test_app("art.capycanvas.GpuToneDrawing");
    let p = new_drawing_at(4096, 2160, SampleDepth::F32);
    let w = Workspace::with_project(&app, Some((p, None)));
    w.window.present();
    ready(&w);
    let change = ui_session_mut(&w)
        .set_proof_mode(layer_ui::ProofMode::Sdr).unwrap();
    w.changed(Ok(change));
    w.dispatch(UiAction::SetBrushSize { value: 180. });
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Definition {
            color: layer_core::color::RgbColor::from_linear(RgbSpace::Srgb, [8., 2., 0.25, 1.])
                .unwrap(),
        },
    });
    let wait_ready = || {
        let start = Instant::now();
        while w.local_tone.ready_count().is_none() {
            pump(5);
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "{}",
                w.local_tone.label.text()
            );
        }
        start.elapsed().as_secs_f64() * 1000.
    };
    let initial_ms = wait_ready();
    let original = w.local_tone.preview_count().unwrap();
    let camera = state(&w).camera;
    let send = |phase, x, y| {
        w.input.send(
            &w,
            PenEvent {
                device_id: 91,
                pressure: 0.8,
                ..pen_event(&camera, [x, y], phase, 0)
            },
        )
    };
    let capture = || {
        ui_session(&w)
            .engine()
            .backend()
            .capture()
            .unwrap()
    };
    let before = capture();
    send(PenPhase::Down, 1000., 1000.);
    pump(20);
    for i in 0..30 {
        send(PenPhase::Move, 1000. + i as f32 * 30., 1000.);
        pump(8);
        assert_eq!(w.local_tone.preview_count(), Some(original));
    }
    let drawing = capture();
    assert_ne!(
        before.bytes, drawing.bytes,
        "new artwork must appear while retaining illumination"
    );
    send(PenPhase::Up, 2000., 1000.);
    let deadline = Instant::now() + Duration::from_secs(30);
    while w.local_tone.worker_state().1 != Some(false) {
        pump(1);
        assert!(Instant::now() < deadline, "refresh worker never started");
    }
    // Interrupt real in-flight analysis with a fresh contact. Even if compute
    // finishes before cancellation is observed, publication must reject it.
    send(PenPhase::Down, 1000., 1400.);
    pump(20);
    let retained = w.local_tone.preview_count();
    assert_eq!(retained, Some(original));
    for i in 0..40 {
        send(PenPhase::Move, 1000. + i as f32 * 25., 1400.);
        pump(8);
        assert_eq!(
            w.local_tone.preview_count(),
            retained,
            "no guide swap during drawing"
        );
    }
    send(PenPhase::Up, 2000., 1400.);
    let refresh_ms = wait_ready();
    assert!(w.local_tone.ready_count().unwrap() > original);
    let refreshed = w.local_tone.ready_count().unwrap();
    invoke(&w, CommandId::Undo);
    wait_ready();
    assert!(
        w.local_tone.ready_count().unwrap() > refreshed,
        "undo must refresh illumination"
    );
    // Same geometry/color and initial file epoch, but a different document and
    // device owner: an in-flight candidate must not follow a tab switch.
    let first = w.documents.selected();
    w.dispatch(UiAction::Invoke {
        command: CommandId::AddLayer,
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while w.local_tone.worker_state().1 != Some(false) {
        pump(1);
        assert!(Instant::now() < deadline);
    }
    let next = new_drawing_at(4096, 2160, SampleDepth::F32);
    glib::MainContext::default()
        .block_on(w.documents.open(&w, (next, None, None)))
        .unwrap();
    assert!(
        w.local_tone.preview_count().is_none(),
        "new tab cannot inherit stale illumination"
    );
    ready(&w);
    wait_ready();
    glib::MainContext::default()
        .block_on(w.documents.activate(&w, first))
        .unwrap();
    assert!(
        w.local_tone.preview_count().is_none(),
        "reattached device must rebuild its guide"
    );
    ready(&w);
    wait_ready();
    eprintln!(
        "GPU_TONE_DRAWING initial_ms={initial_ms:.2} refresh_ms={refresh_ms:.2} cancelled_inflight=true retained_during_contact=true"
    );
    w.window.destroy();
    until(|| !w.local_tone.worker_state().0, "local tone worker stop");
    pump(50);
}
