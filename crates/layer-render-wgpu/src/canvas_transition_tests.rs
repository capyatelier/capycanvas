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
fn a_frame_result_in_flight_asks_for_no_frame_that_cannot_deliver_it() {
    use std::time::{Duration,Instant};
    let extent=[64;2];
    let document=paint_document(extent,"Result in flight");
    let mut r=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let (wake,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{let _=wake.send(());}));
    let frame=packet(document.scene(),extent);
    submit_settled(&mut r,frame);
    r.wait_idle().unwrap();
    r.fill_preview_done=Some(Arc::new(std::sync::atomic::AtomicBool::new(false)));
    assert!(r.settled() && r.frame_results_pending());
    let idle=FramePacket {composite_all:false,..frame};
    let (started,mut frames)=(Instant::now(),0);
    while started.elapsed()<Duration::from_secs(2) {
        r.poll_pending(frame.view).unwrap();
        if r.next_frame()==layer_render::FrameDemand::Wait {let _=woken.recv_timeout(Duration::from_millis(20));continue;}
        frames+=1;
        if r.can_submit() {let _=r.submit_when_ready(idle,None);}
    }
    assert!(frames<=2,"a result still in flight drove {frames} frames that could not deliver it");
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

const PAPER: [u32; 2] = [128; 2];
const SURFACE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn gray_paper() -> layer_core::Document {
    let mut document = crate::layer_tests::paper_shade::rough_gray_paper();
    crate::layer_tests::paper_shade::set_paper(&mut document, &[(layer_core::paper::RELIEF, layer_core::EffectValue::Number(0.))]);
    document
}

fn canvas(r: &WgpuRasterizer) -> Vec<u8> {
    let mut presenter = ViewportPresenter::for_surface(r, SURFACE, SdrSurfaceColor::Srgb).unwrap();
    pollster::block_on(presenter.prepare(r)).unwrap();
    let (texture, target) = crate::create_target(&r.device, PAPER, SURFACE, "presented canvas");
    presenter.present(r, &target, crate::test_support::view(PAPER), [0.2, 0.3, 0.4, 1.]).unwrap();
    crate::layer_tests::page_bytes(r, &texture)
}

fn navigator(r: &WgpuRasterizer) -> Vec<u8> {
    let mut presenter = ViewportPresenter::for_overview_surface(r, SURFACE, SdrSurfaceColor::Srgb).unwrap();
    presenter.set_overviews(r, &[crate::OverviewPlacement { bounds: [0., 0., 128., 128.], clip: None, work_area: [[-1000.; 2]; 4], outline_linear: [0.; 3], scale: 1., opacity: 1. }]);
    pollster::block_on(presenter.prepare(r)).unwrap();
    let (texture, target) = crate::create_target(&r.device, PAPER, SURFACE, "presented Navigator");
    presenter.present_overviews(r, &target, PAPER).unwrap();
    crate::layer_tests::page_bytes(r, &texture)
}

#[test]
fn a_second_drawing_with_equal_pixels_shows_from_the_committed_windows_without_composing() {
    let (first, second) = (gray_paper(), gray_paper());
    assert_ne!((first.owner, first.artwork.id), (second.owner, second.artwork.id));
    let mut r = WgpuRasterizer::new_native_headless(first.composition().color).unwrap();
    submit_settled(&mut r, packet(first.scene(), PAPER));
    assert!(r.showing_drawing() && r.showing_navigator());
    let before = (r.composite_revision, r.navigator.revision, r.metrics.composited_pixels);
    let counts = crate::test_support::PassCounts::snapshot();
    submit_settled(&mut r, packet(second.scene(), PAPER));
    let counts = crate::test_support::PassCounts::snapshot().delta(counts);
    assert_eq!((r.composite_revision, r.navigator.revision, r.metrics.composited_pixels), before, "an equal picture needs no pixel operation");
    assert_eq!((counts.composed_texels, counts.destination_units, counts.projected_copies, counts.composite_commits), (0, 0, 0, 0), "an equal picture needs no pixel operation");
    assert!(r.showing_drawing() && r.showing_navigator(), "the committed windows already hold the second drawing");
    let mut fresh = WgpuRasterizer::new_native_headless(second.composition().color).unwrap();
    submit_settled(&mut fresh, packet(second.scene(), PAPER));
    assert!(fresh.showing_drawing() && fresh.showing_navigator());
    assert_eq!(canvas(&r), canvas(&fresh), "the reused renderer presents the second drawing like a fresh one");
    assert_eq!(navigator(&r), navigator(&fresh), "the reused Navigator presents the second drawing like a fresh one");
}

#[test]
fn a_second_drawing_with_other_pixels_stays_hidden_until_its_own_picture_publishes() {
    let (first, mut second) = (gray_paper(), gray_paper());
    let layer_core::SourceTarget::Paint(paint) = second.working.target.unwrap() else { unreachable!() };
    second.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(
        layer_core::color::source::rgba8_source(PAPER, |_, _| [0, 0, 255, 255]).into()));
    let mut r = WgpuRasterizer::new_native_headless(first.composition().color).unwrap();
    submit_settled(&mut r, packet(first.scene(), PAPER));
    assert!(r.showing_drawing());
    let old = canvas(&r);
    let revision = r.composite_revision;
    let mut hidden = 0;
    crate::test_support::submit_published_observing(&mut r, packet(second.scene(), PAPER), |r| {
        assert!(canvas(r).chunks_exact(4).zip(old.chunks_exact(4)).all(|(shown, old)| shown != old), "the first drawing's window never presents as the second drawing");
        if !r.showing_drawing() { hidden += 1; }
    });
    assert!(hidden > 0, "the second drawing waits for its source while the first drawing's window is still committed");
    submit_settled(&mut r, FramePacket { composite_all: false, ..packet(second.scene(), PAPER) });
    assert!(r.showing_drawing() && r.composite_revision != revision);
    let mut fresh = WgpuRasterizer::new_native_headless(second.composition().color).unwrap();
    submit_settled(&mut fresh, packet(second.scene(), PAPER));
    assert_eq!(canvas(&r), canvas(&fresh));
}
