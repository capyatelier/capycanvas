use super::*;
use crate::d0_release::{KINDS, Released, Switch};

fn kind_bytes(r: &WgpuRasterizer) -> [[u64; 4]; 2] {
    let cache = r.scale_display.as_ref().unwrap();
    let mut kept = cache.release_settled(&Switch::new([false; 4], true, Default::default()), &r.navigator).0.kept;
    let total = Released::bytes(&kept);
    kept.remove(&cache.plan.level);
    [total, Released::bytes(&kept)]
}

fn settle(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        crate::test_support::submit_ready(r, frame).unwrap();
        r.wait_idle().unwrap();
        r.poll_startup().unwrap();
        r.poll_pending(frame.view).unwrap();
        match r.next_frame() {
            layer_render::FrameDemand::Wait if r.settled() && !publication_pending(r) => return,
            layer_render::FrameDemand::WaitUntil(due) => std::thread::sleep(due.saturating_duration_since(std::time::Instant::now())),
            _ => {},
        }
        assert!(std::time::Instant::now() < deadline, "the canvas never settled: {:?}", r.outstanding_terms());
    }
}

fn released_kind_keeps_the_settled_picture(kind: Option<usize>, keep_view: bool) -> ([u64; 3], Vec<[f32; 4]>) {
    let mut doc = document_at([1537, 1025]);
    let (above, below) = (copy_paint(&mut doc, 0), copy_paint(&mut doc, 0));
    let ink = doc.scene().order()[0];
    for handle in [above, ink] { doc.artwork.occurrences.get_mut(handle).unwrap().opacity = 0.6; }
    set_root_entries(&mut doc, vec![above, ink, below]);
    let extent = doc.composition().size;
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut frame = FramePacket { composite_all: false, ..packet(doc.scene(), extent) };
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    let view = frame.view;
    submit_published(&mut r, frame);
    let target = source_at(&doc, 1);
    let dab = crate::tests::test_dab([700., 500.], [0.9, 0.1, 0.3, 1.], 0.8);
    let batch = dab_batch(target, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    doc.apply(layer_core::Edit::SetRaster { target, revision: layer_core::raster::RasterRevision::pending() }).unwrap();
    frame = FramePacket { view, composite_all: false, ..packet(doc.scene(), extent) };
    crate::test_support::submit_ready(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    submit_published(&mut exact, frame);
    let reference = pixels(&exact, crate::test_support::document_texture(&exact));
    assert_settled(&mut r, frame, &reference);

    let [before, off_view] = kind_bytes(&r);
    let shown = display_pixels(&r);
    let name = kind.map_or("no", |kind| KINDS[kind]);
    r.d0_release = kind.map(|kind| Switch::new(std::array::from_fn(|index| index == kind), keep_view, Default::default()));
    let work = r.metrics.composited_pixels;
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert!(r.settled(), "releasing {name} pages left work outstanding: {:?}", r.outstanding_terms());
    settle(&mut r, frame);
    assert_eq!(r.metrics.composited_pixels, work, "still frames composed after {name} pages were released");
    assert!(display_pixels(&r) == shown, "releasing {name} pages changed the settled picture");
    let [after, _] = kind_bytes(&r);
    for other in (0..4).filter(|other| Some(*other) != kind) { assert_eq!(after[other], before[other], "only {name} pages are released"); }

    frame.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    let work = r.metrics.composited_pixels;
    settle(&mut r, frame);
    let kind = kind.unwrap_or(0);
    ([if keep_view { off_view[kind] } else { before[kind] }, before[kind] - after[kind], r.metrics.composited_pixels - work], display_pixels(&r))
}

#[test]
fn each_released_kind_keeps_the_settled_and_the_zoomed_in_picture() {
    let ([.., control], expected) = released_kind_keeps_the_settled_picture(None, true);
    for keep_view in [true, false] {
        for (kind, name) in KINDS.into_iter().enumerate() {
            let ([releasable, released, zoom], zoomed) = released_kind_keeps_the_settled_picture(Some(kind), keep_view);
            let error = crate::test_support::max_error(&zoomed, &expected);
            eprintln!("D0 test {name} keep_view={keep_view}: released {released} of {releasable} bytes, zoom to 100% composed {zoom} texels against {control}, error {error}");
            assert!(released <= releasable && (released > 0 || releasable == 0), "{name} pages off the kept view stayed with keep_view={keep_view}");
            assert!(zoomed.len() == expected.len() && error < 2e-5, "zoomed in after releasing {name} pages with keep_view={keep_view}: {error}");
        }
    }
}
