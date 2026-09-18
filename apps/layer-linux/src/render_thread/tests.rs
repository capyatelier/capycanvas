use super::*;
use gtk::prelude::*;

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_hdr_display_hint_before_first_frame() {
    adw::init().unwrap();
    let app = adw::Application::builder()
        .application_id("art.capycanvas.EarlyHdrDisplayHint")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let area = gtk::Picture::new();
    let window = gtk::ApplicationWindow::builder()
        .application(&app)
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
