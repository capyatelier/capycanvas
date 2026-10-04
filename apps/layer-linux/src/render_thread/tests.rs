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

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_hdr_display_hint_before_first_frame() {
    let app = crate::workspace::tests::native_test_app("art.capycanvas.EarlyHdrDisplayHint");
    let area = gtk::Picture::new();
    let window = gtk::ApplicationWindow::builder()
        .application(&*app)
        .child(&area)
        .default_width(320)
        .default_height(240)
        .build();
    window.present();
    let pump = || {
        let context = gtk::glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    while !window.is_mapped() {
        pump();
    }
    let parent = Parent::new(&window.surface().unwrap()).unwrap();
    let area = area.downgrade().into();
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
        pump();
    }
    let idle_until = std::time::Instant::now() + Duration::from_millis(200);
    while std::time::Instant::now() < idle_until {
        pump();
    }
    let sent = commands.send(Command::Stop);
    let result = worker.join();
    window.destroy();
    assert!(sent.is_ok(), "GPU owner failed while waiting for its first frame");
    result.expect("GPU owner panicked before its first frame").unwrap();
}
