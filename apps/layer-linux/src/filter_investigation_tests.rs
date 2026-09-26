//! Opt-in real photo / production GTK render-owner measurements.
use super::*;
use layer_render::CanvasRenderer;
use layer_ui::EffectAction;

fn settled(w: &Rc<Workspace>) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        pump(20);
        let ready = w.gpu.borrow().as_ref().is_some_and(|g| {
            g.session.engine().backend().startup.complete
                && !g.session.engine().has_pending_document_edits()
                && g.session.engine().backend().frames_idle()
                && w.frame_timer.borrow().is_none()
        });
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "settle: {}", w.status.text());
    }
    pump(100);
}

#[test]
#[ignore = "private Wayland display; CAPY_FILTER_PHOTO is the supplied JPEG"]
fn native_photo_filter_investigation() {
    let path = std::path::PathBuf::from(std::env::var("CAPY_FILTER_PHOTO").unwrap());
    let (project, location) = crate::files::open::read(
        &path,
        layer_ui::DocumentLocation {
            uri: path.to_string_lossy().into(),
            name: "Water photo.jpg".into(),
        },
        Default::default(),
        Default::default(),
    )
    .unwrap();
    eprintln!(
        "photo={}x{} color={:?}",
        project.document.width, project.document.height, project.document.color
    );
    let app = native_test_app("art.capycanvas.FilterInvestigation");
    let w = Workspace::with_project(&app, Some((project, location)));
    w.window.set_default_size(1400, 950);
    w.window.present();
    settled(&w);
    let stats = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .stats
        .clone();
    w.gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .renderer_mut()
        .set_telemetry_enabled(true);
    *stats.lock().unwrap() = Default::default();
    let start = Instant::now();
    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "gaussian_blur".into(),
        },
    });
    settled(&w);
    eprintln!(
        "insert_wall_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    let layer = state(&w).layer_properties.layer.unwrap();
    for sigma in [3., 4., 12., 21., 3.] {
        *stats.lock().unwrap() = Default::default();
        let start = Instant::now();
        w.dispatch(UiAction::Effect {
            action: EffectAction::Set {
                layer,
                key: "sigma".into(),
                value: layer_core::EffectValue::Number(sigma),
            },
        });
        settled(&w);
        let s = stats.lock().unwrap();
        eprintln!(
            "sigma={sigma} wall_ms={:.3} cpu={:?} thread_cpu={:?} gpu={:?} phases={:?}",
            start.elapsed().as_secs_f64() * 1000.,
            s.cpu,
            s.thread_cpu,
            s.gpu,
            s.renderer_phases
        );
        eprintln!(
            "camera_work={:?} source_transfers={:?}",
            s.camera_work, s.source_transfers
        );
    }
    crate::capture(&w, "../../artifacts/filter-investigation/gtk-gaussian.png");
    w.window.destroy();
    pump(100);
}
