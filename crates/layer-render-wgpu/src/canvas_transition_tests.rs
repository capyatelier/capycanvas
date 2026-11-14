use super::*;
use crate::layer_tests::placement::{paint_document, paint_mut, target};
use crate::test_support::{packet, submit_ready, submit_settled};

fn refining(r: &WgpuRasterizer) -> bool {
    crate::scene::scale::tests::publication_pending(&r) || r.scale_display.as_ref().is_some_and(|cache| cache.has_pending_work(r))
}

#[test]
fn navigator_waits_for_a_full_refresh_before_pausing_transition_refinement() {
    let extent=[64;2];
    let mut document=paint_document(extent,"Navigator readiness");
    paint_mut(&mut document).base=Some(layer_core::authored::PaintBase::new(
        layer_core::color::source::rgba8_source(extent,|_,_|[255,0,0,255]).into()));
    let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let backdrop=layer_core::SceneScope::Members(document.scene().constant_backdrop().into());
    submit_settled(&mut r,packet(document.scene().with_scope(&backdrop),extent));
    assert!(!r.showing_drawing() && !r.showing_navigator());
    let revision=r.navigator.revision;
    assert!(revision>0);
    r.set_canvas_transition(layer_render::CanvasFade::WAITING);
    let frame=FramePacket {time_seconds:0.01,..packet(document.scene(),extent)};
    let idle=FramePacket {composite_all:false,..frame};
    let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
    let mut first=true;
    while !r.showing_drawing() || !r.showing_navigator() {
        assert!(!r.retained_transition_frame(idle,None),"a transition cannot pause before both views publish");
        submit_ready(&mut r,if std::mem::take(&mut first) {frame}else {idle}).unwrap();
        r.wait_idle().unwrap();
        assert!(std::time::Instant::now()<deadline,"the full Navigator composition must publish");
    }
    assert!(r.navigator.revision>revision);
    assert!(!r.navigator.pending(),"a presented Navigator must match its expected content");
    let pixels=crate::test_support::float_pixels(&r,r.navigator.view().unwrap().texture());
    assert!(!pixels.is_empty() && pixels.iter().all(|pixel|*pixel==[1.,0.,0.,1.]),
        "the Navigator must contain the full current source before the transition pauses");
    assert!(r.retained_transition_frame(idle,None));
}

#[test]
fn retained_fade_preserves_pending_refinement_and_accepts_camera_and_paint() {
    let extent = [2560, 2048];
    let mut document = paint_document(extent, "Retained fade");
    paint_mut(&mut document).base = Some(layer_core::authored::PaintBase::new(
        layer_core::color::source::rgba8_source(extent, |x, y| [
            30 + ((x / 32) % 160) as u8, 40 + ((y / 32) % 150) as u8, 80, 255,
        ]).into(),
    ));
    let mut r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    assert_eq!(r.canvas_transition, layer_render::CanvasFade::default());
    let frame = FramePacket {
        view: layer_render::ViewState { width_px: 512, height_px: 384,
            document_to_surface: [0.5, 0., 0., 0.5, 0., 0.] },
        ..packet(document.scene(), extent)
    };
    let idle = FramePacket { composite_all: false, ..frame };
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    let mut first = true;
    while !r.showing_drawing() || !r.showing_navigator() || !refining(&r) {
        submit_ready(&mut r, if first { frame } else { idle }).unwrap();
        first = false;
        r.wait_idle().unwrap();
        assert!(std::time::Instant::now() < deadline, "the source-backed drawing must expose a meaningful partial result");
    }
    let submissions = r.metrics.submissions;
    let preparation = r.preparation_progress();
    let damage = r.document_damage.clone();
    let deferred = crate::scene::scale::tests::publication_pending(&r);
    for alpha in [1., 0.5, 0.125] {
        r.set_canvas_transition(layer_render::CanvasFade {drawing:Some(alpha),navigator:None});
        assert!(r.retained_transition_frame(idle, None));
        assert!(r.raster_dependencies_ready(idle, None));
        r.submit(idle).unwrap();
        assert_eq!(r.metrics.submissions, submissions);
        assert_eq!(r.preparation_progress(), preparation);
        assert_eq!(r.document_damage, damage);
        assert_eq!(crate::scene::scale::tests::publication_pending(&r), deferred);
        assert!(refining(&r));
    }

    let camera = FramePacket { view: layer_render::ViewState {
        document_to_surface: [0.5, 0., 0., 0.5, -64., -32.], ..idle.view
    }, ..idle };
    assert!(!r.retained_transition_frame(camera, None));
    submit_ready(&mut r, camera).unwrap();
    r.wait_idle().unwrap();
    assert_eq!(r.artwork_frame.as_ref().unwrap().view, camera.view);

    let mut dab = crate::layer_tests::dab([0.8, 0.1, 0.2, 1.]);
    dab.center = layer_core::Point { x: 256., y: 256. };
    let style = crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
    let batch = crate::test_support::dab_batch(target(&document), style, layer_core::Rect {
        min: layer_core::Point { x: 196., y: 196. }, max: layer_core::Point { x: 316., y: 316. },
    });
    let dabs = [dab];
    let batches = [batch];
    let paint = FramePacket { dabs: &dabs, dab_batches: &batches, ..camera };
    assert!(!r.retained_transition_frame(paint, None));
    let painted = r.metrics.dabs;
    submit_ready(&mut r, paint).unwrap();
    r.wait_idle().unwrap();
    assert!(r.metrics.dabs > painted);

    r.set_canvas_transition(Default::default());
    let submissions = r.metrics.submissions;
    submit_settled(&mut r, camera);
    assert!(r.metrics.submissions > submissions);
    assert!(!refining(&r));
}

#[test]
fn a_settled_canvas_demands_no_frame_and_wakes_its_host_for_five_seconds() {
    use std::time::{Duration,Instant};
    let extent=[64;2];
    let document=paint_document(extent,"Idle canvas");
    let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let (wake,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=wake.send(());}));
    submit_settled(&mut r,packet(document.scene(),extent));
    r.wait_idle().unwrap();
    let deadline=Instant::now()+Duration::from_secs(2);
    while woken.recv_timeout(Duration::from_millis(200)).is_ok() {assert!(Instant::now()<deadline,"a settled canvas keeps waking its host");}
    assert!(!r.has_pending_submission() && r.next_frame()!=layer_render::FrameDemand::Work);
    assert_eq!(r.next_frame(),layer_render::FrameDemand::Wait);
    assert!(woken.recv_timeout(Duration::from_secs(5)).is_err(),"a settled canvas woke its host");
}
