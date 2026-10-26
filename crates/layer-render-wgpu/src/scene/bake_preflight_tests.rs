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
    while engine.has_pending_document_edits() { engine.render_frame_at(0).unwrap(); }
    engine.backend_mut().native_edit.as_mut().unwrap().image_pixel_bytes = Some(u64::MAX);
    let plan = engine.document().merge_plan(layer_core::MergeKind::Down).unwrap();
    engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], None).unwrap();
    let mut shared = 0;
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
    }
    assert!(shared > 1, "a step's filter evaluation served at most {shared} page");
}
