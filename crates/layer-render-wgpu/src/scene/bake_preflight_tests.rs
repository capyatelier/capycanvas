use super::*;
use layer_engine::{CanvasEngine, ViewTransform, input_queue};

#[test]
fn a_bake_within_its_image_budget_filters_each_step_once() {
    let extent = [1100, 700];
    let (doc, _) = crate::tests::image_windows::blurred_photo(extent, layer_core::color::DocumentColor::default(), layer_core::BlendSpace::Perceptual);
    let passes = doc.scene().effect(doc.working.occurrence.unwrap()).unwrap().program.passes.len() as u64;
    let gpu = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut engine = CanvasEngine::new(gpu, doc, input_queue(8).1, crate::test_support::view([128; 2]), ViewTransform::IDENTITY).unwrap();
    engine.render_frame_at(0).unwrap();
    let initial_deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    while engine.has_pending_document_edits() {
        assert!(std::time::Instant::now() < initial_deadline, "initial document edits must finish");
        engine.render_frame_at(0).unwrap();
    }
    engine.backend_mut().native_edit.as_mut().unwrap().image_pixel_bytes = Some(u64::MAX);
    crate::scene::sources::Inputs::begin_accepted_reads();
    let plan = engine.document().merge_plan(layer_core::MergeKind::Down).unwrap();
    engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
    let mut shared = 0;
    let bake_deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        engine.render_frame_at(0).unwrap();
        if let Some(state) = &engine.backend().bake_preflight && let Some(scene) = &state.scene {
            let pages = state.pages.iter().filter(|(input, _, _)| *input == state.next).count();
            let [inputs, evaluated] = scene.image_work();
            assert!(!state.windowed);
            assert!(inputs <= 1 && evaluated <= passes, "{pages} prepared pages of one step ran {inputs} filter inputs and {evaluated} passes");
            if evaluated == passes { shared = shared.max(pages); }
        }
        if !engine.has_pending_document_edits() { break; }
        assert!(std::time::Instant::now() < bake_deadline, "baked document edits must finish");
    }
    assert!(shared > 1, "a step's filter evaluation served at most {shared} page");
    let reads=crate::scene::sources::Inputs::take_accepted_read_results();
    assert!(reads.iter().map(|(count,_)|count).sum::<usize>()>0,"effect-input preparation must consume decoded sources");
    assert!(reads.iter().all(|(_,unexpected)|unexpected.is_empty()),"{reads:?}");
}

fn merge_visible(document: &mut layer_core::Document) -> (SourceTarget, DabBatch) {
    let plan = document.merge_plan(layer_core::MergeKind::Visible).unwrap();
    let SourceTarget::Paint(handle) = plan.target else { unreachable!() };
    document.apply(layer_core::Edit::Batch(plan.edits)).unwrap();
    let mut source = document.artwork.paint.get(handle).unwrap().clone();
    source.operations = Arc::new(vec![plan.operation]);
    document.apply(layer_core::Edit::Paint(layer_core::authored::RecordChange::replace(&document.artwork.paint, handle, Some(source)).unwrap())).unwrap();
    (plan.target, DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
        ..crate::test_support::dab_batch(plan.target, crate::tests::test_style(BrushExecution::Dry), layer_core::Rect::from_extent(document.composition().size)) })
}

#[test]
fn a_bake_waiting_for_memory_is_not_ready_and_completes_once_room_is_made() {
    let (mut document, mut r, _) = crate::memory::memory_tests::durable_color_renderer();
    let extent = document.composition().size;
    let (target, batch) = merge_visible(&mut document);
    let frame = FramePacket { dab_batches: std::slice::from_ref(&batch), view: crate::test_support::view([512; 2]), ..crate::test_support::packet(document.scene(), extent) };
    r.ensure_document_metadata(extent, document.scene()).unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.publish_memory();
    budget.flush_reports();
    let pressure = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    budget.flush_reports();
    assert!(2 * r.color_memory_bytes() > r.memory.display_grant() + r.memory.raster_grant(), "the bake must not fit beside the resident color it reads");
    assert!(!r.prepare_bakes(frame), "a bake that waits for memory is not ready");
    assert!(r.take_bake_failure().is_none(), "a wait for memory is not a failure");
    assert_eq!(r.color_memory_bytes(), 0, "the wait makes room by releasing backed color");
    crate::test_support::prepare_raster(&mut r, |r| if r.prepare_bakes(frame) { Ok(()) } else { Err(GpuRasterError::NotReady) }).unwrap();
    assert!(r.take_bake_failure().is_none());
    assert!(page_coordinates(PixelRect::full(extent)).all(|coordinate| r.prepared_bake(target, 0, coordinate).is_some()));
    drop(pressure);
}

#[test]
fn a_bake_larger_than_the_memory_limit_stays_a_failure() {
    let extent = [PAGE_SIZE * 12; 2];
    let mut document = layer_core::Document::new(layer_core::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let mut r = crate::memory::memory_tests::renderer(document.composition().color,
        layer_core::DeviceMemoryFacts { unified: true, ..Default::default() }).unwrap();
    let view = crate::test_support::view([512; 2]);
    crate::test_support::submit_ready(&mut r, FramePacket { reset_layers: true, view, ..crate::test_support::packet(document.scene(), extent) }).unwrap();
    let (_, batch) = merge_visible(&mut document);
    let frame = FramePacket { dab_batches: std::slice::from_ref(&batch), view, ..crate::test_support::packet(document.scene(), extent) };
    r.ensure_document_metadata(extent, document.scene()).unwrap();
    let budget = r.factory.memory_budget().clone();
    budget.set_ram_override(Some(384 << 20));
    r.publish_memory();
    budget.flush_reports();
    r.refresh_memory_limits();
    let pressure = budget.admit_import(budget.import_headroom_bytes()).unwrap();
    r.refresh_memory_limits();
    assert!(r.prepare_bakes(frame));
    assert!(matches!(r.take_bake_failure(), Some(GpuRasterError::SourceWorkingSetExceeded)));
    drop(pressure);
}
