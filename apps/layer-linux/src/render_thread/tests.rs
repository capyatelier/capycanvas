#[test]
fn captured_context_publication_shares_source_roots_and_preserves_authored_output() {
    let document = layer_ui::new_drawing(32,24,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut capture = layer_host::tasks::capture_document(&document);
    let value = Arc::new(std::sync::OnceLock::new());
    let request = ContextRequest {value:value.clone()};
    let context = EvaluationContext {elapsed:17.,phases:Vec::new().into()};
    request.value.set(Ok(context.clone())).unwrap();
    drop(request);
    ContextCapture(value).install(&mut capture).unwrap();
    assert_eq!(capture.output().context,context);
    assert_eq!(document.output().context.elapsed,0.);
    assert_eq!(capture.artwork.paint,document.artwork.paint);
    assert_eq!(capture.checkpoint.owner,document.owner);
}

#[test]
fn retired_context_capture_resolves_failure_without_changing_sources() {
    let document = layer_ui::new_drawing(32,24,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut capture = layer_host::tasks::capture_document(&document);
    let original = capture.artwork.clone();
    let value = Arc::new(std::sync::OnceLock::new());
    drop(ContextRequest {value:value.clone()});
    assert!(ContextCapture(value).install(&mut capture).is_err());
    assert!(Arc::ptr_eq(&capture.artwork,&original));
}

use super::*;
use gtk::prelude::*;

fn pump_worker_window() {
    let context = gtk::glib::MainContext::default();
    while context.pending() { context.iteration(false); }
    std::thread::sleep(Duration::from_millis(5));
}

fn native_worker_window(id: &str) -> (crate::workspace::tests::NativeTestApp,
    gtk::ApplicationWindow, Parent, gtk::glib::SendWeakRef<gtk::Picture>) {
    let app = crate::workspace::tests::native_test_app(id);
    let area = gtk::Picture::new();
    let window = gtk::ApplicationWindow::builder()
        .application(&*app)
        .child(&area)
        .default_width(320)
        .default_height(240)
        .build();
    window.present();
    while !window.is_mapped() {
        pump_worker_window();
    }
    let parent = Parent::new(&window.surface().unwrap()).unwrap();
    (app, window, parent, area.downgrade().into())
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_hdr_display_hint_before_first_frame() {
    let (_app, window, parent, area) = native_worker_window("art.capycanvas.EarlyHdrDisplayHint");
    let (commands, receiver) = mpsc::channel();
    let (reply, replies) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut worker = Worker::new(
            parent,
            area,
            Arc::default(),
            layer_core::color::DocumentColor {
                depth: layer_core::color::SampleDepth::F16,
                ..Default::default()
            },
        )?;
        // Exercise the display-event path before ANY frame/geometry is sent.
        // An idle acquire here used to panic on an unconfigured swapchain.
        worker.display_headroom = 4.;
        worker.update_hdr_view()?;
        worker.run(
            &receiver,
            &reply,
            &Default::default(),
            &AtomicUsize::new(0),
            &Default::default(),
            Arc::default(),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !matches!(replies.try_recv(), Ok(Reply::Initialized(..))) {
        assert!(!worker.is_finished(), "GPU owner stopped during initialization");
        assert!(std::time::Instant::now() < deadline, "GPU owner did not initialize");
        pump_worker_window();
    }
    let idle_until = std::time::Instant::now() + Duration::from_millis(200);
    while std::time::Instant::now() < idle_until {
        pump_worker_window();
    }
    let sent = commands.send(Command::Stop);
    let result = worker.join();
    window.destroy();
    assert!(sent.is_ok(), "GPU owner failed while waiting for its first frame");
    result.expect("GPU owner panicked before its first frame").unwrap();
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_late_shader_failure_after_startup() {
    const CAUSE: &str = "late required compiler cause after startup";
    let (_app, window, parent, area) = native_worker_window("art.capycanvas.LateShaderFailure");
    let (commands, receiver) = mpsc::channel();
    let (reply, _replies) = mpsc::channel();
    let (prepared, ready) = mpsc::channel();
    let worker = std::thread::spawn(move || -> Result<(), String> {
        let mut worker = Worker::new(parent, area, Arc::default(), Default::default())?;
        let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English))?;
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        worker.renderer.prepare_startup(&document, &brush, false).map_err(error)?;
        worker.renderer.finish_startup_cache();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !worker.renderer.poll_startup().map_err(error)?.complete {
            if std::time::Instant::now() >= deadline { return Err("Renderer startup did not complete".into()); }
            std::thread::sleep(Duration::from_millis(1));
        }
        layer_render_wgpu::enqueue_compiler_failure(&worker.renderer, CAUSE)
            .recv_timeout(Duration::from_secs(5)).map_err(error)?;
        prepared.send(()).map_err(error)?;
        worker.run(&receiver, &reply, &Default::default(), &AtomicUsize::new(0),
            &Default::default(), Arc::default())
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    let startup_complete = loop {
        if ready.try_recv().is_ok() { break true; }
        if worker.is_finished() || std::time::Instant::now() >= deadline { break false; }
        pump_worker_window();
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while startup_complete && !worker.is_finished() && std::time::Instant::now() < deadline {
        let _ = commands.send(Command::FinishStartupCache);
        pump_worker_window();
    }
    let failed_before_stop = worker.is_finished();
    let _ = commands.send(Command::Stop);
    let result = worker.join();
    window.destroy();
    assert!(startup_complete, "Startup preparation failed: {result:?}");
    assert!(failed_before_stop, "A completed startup must still poll late compiler failures");
    let cause = result.expect("GPU owner panicked").expect_err("GPU owner ignored the compiler failure");
    assert!(cause.contains(CAUSE), "GPU owner lost the recorded cause: {cause}");
}
