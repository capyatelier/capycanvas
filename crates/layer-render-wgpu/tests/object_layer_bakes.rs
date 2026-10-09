//! Stamp Visible, Merge Group and applying an attached filter bake image
//! layers into paint with the composite they replace, keep what they don't
//! consume, and undo in one step.
mod support;
use layer_core::*;
use support::*;

const TOLERANCE: u8 = 1;

fn edit(engine: &mut Engine, handle: OccurrenceHandle, change: impl FnOnce(&mut Occurrence)) {
    let edit = occurrence_edit(engine.document(), handle, change);
    engine.apply_edit(edit).unwrap();
}

/// The ink layer below an image layer with a smooth rotated image and a
/// Nearest one beyond the frame's left edge.
fn drawing(space: BlendSpace) -> (Document, OccurrenceHandle) {
    let mut doc = named_document(&["Ink"], SIZE, space);
    let picture = photo([120, 90], |_, _| 255);
    let layer = images(&mut doc, vec![
        placed(&picture, [0.8, 0.45, -0.45, 0.8, 140., 30.], false),
        placed(&picture, [1.5, 0., 0., 1.5, -70.25, 110.5], true),
    ]);
    (doc, layer)
}

fn grown() -> CanvasGeometry {
    CanvasGeometry::crop(CanvasRect { origin: [-200, 0], size: [SIZE[0] + 200, SIZE[1]] })
}

#[test]
fn stamp_visible_keeps_image_layers_and_bakes_their_off_frame_pixels() {
    for space in BlendSpace::ALL {
        let (doc, layer) = drawing(space);
        let (mut engine, mut input) = engine(doc);
        let paper = named_occurrence(engine.document(), "Paper");
        let ink = named_occurrence(engine.document(), "Ink");
        edit(&mut engine, paper, |o| o.visible = false);
        engine.set_active_layer(ink).unwrap();
        draw(&mut engine, &mut input, [0.2, 0.3, 0.9, 1.], Point { x: 20., y: 200. }, Point { x: 360., y: 60. }, 1_000_000_000);
        edit(&mut engine, layer, |o| o.opacity = 0.7);
        let original = image(&mut engine, 2_000_000_000);
        engine.apply_canvas_geometry(&grown()).unwrap();
        let revealed = image(&mut engine, 3_000_000_000);
        assert!(engine.undo().unwrap());
        let objects = engine.document().artwork.objects.clone();
        let layers = engine.document().scene().order().iter().copied().filter(|h|engine.document().scene().object_layer(*h).is_some()).collect::<Vec<_>>();
        let plan = engine.document().merge_plan(MergeKind::Stamp).unwrap();
        let stamp = bake(&mut engine, plan);
        image(&mut engine, 4_000_000_000);
        let scene = engine.document().scene();
        assert_eq!(scene.order()[0], stamp, "{space:?}");
        assert_eq!(engine.document().artwork.objects, objects, "{space:?}: Stamp Visible keeps its sources");
        for hidden in layers.into_iter().chain([ink]) { edit(&mut engine, hidden, |o| o.visible = false); }
        image(&mut engine, 5_000_000_000).assert_near(&original, TOLERANCE, &format!("{space:?}: the stamp alone"));
        engine.apply_canvas_geometry(&grown()).unwrap();
        image(&mut engine, 6_000_000_000).assert_near(&revealed, TOLERANCE, &format!("{space:?}: the stamp keeps pixels beyond the frame"));
    }
}

#[test]
fn merge_group_bakes_image_descendants_and_keeps_outer_properties_once() {
    for space in BlendSpace::ALL {
        let mut doc = named_document(&["Group", "Ink"], SIZE, space);
        let picture = photo([140, 100], |x, y| if (x + y) % 13 == 0 { 0 } else { 255 });
        let layer = images(&mut doc, vec![placed(&picture, [1.25, 0., 0., 1.25, 60.5, 40.25], false)]);
        let group = named_occurrence(&doc, "Group");
        let inner = add_paint(&mut doc, "Inner", 0);
        let edit_group = convert_group(&doc, group, &[layer, inner]);
        doc.apply(edit_group).unwrap();
        let mut mask = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), SIZE, [0; 2]);
        mask.source.default_coverage = 0.6;
        let edit_mask = mask_edit(&doc, group, mask);
        doc.apply(edit_mask).unwrap();
        let (mut engine, mut input) = engine(doc);
        engine.set_active_layer(inner).unwrap();
        draw(&mut engine, &mut input, [0.9, 0.2, 0.1, 0.8], Point { x: 30., y: 120. }, Point { x: 340., y: 140. }, 1_000_000_000);
        edit(&mut engine, group, |o| { o.opacity = 0.5; o.blend = LayerBlend::Screen; });
        let original = image(&mut engine, 2_000_000_000);
        engine.set_active_layer(group).unwrap();
        let plan = engine.document().merge_plan(MergeKind::Group).unwrap();
        let RasterOperationKind::Bake { scene, .. } = &plan.operation.kind else { panic!("bake") };
        let baked = scene.view().occurrence(group).unwrap();
        assert_eq!((baked.opacity, baked.blend, baked.mask.is_some()), (1., LayerBlend::Normal, true), "{space:?}: the bake applies the mask, not opacity or blend");
        let result = bake(&mut engine, plan);
        image(&mut engine, 3_000_000_000).assert_near(&original, TOLERANCE, &format!("{space:?}: merged group"));
        let occurrence = engine.document().scene().occurrence(result).unwrap();
        assert_eq!((occurrence.kind(), occurrence.opacity, occurrence.blend), (LayerKind::Paint, 0.5, LayerBlend::Screen), "{space:?}");
        assert!(engine.document().artwork.objects.is_empty(), "{space:?}: the group's images are consumed");
        assert!(engine.undo().unwrap());
        image(&mut engine, 4_000_000_000).assert_eq(&original, &format!("{space:?}: undo"));
        assert!(engine.document().scene().object_layer(layer).is_some());
    }
}

/// The 16-bit drawing stores a blurred edge's translucent pixels without an
/// 8-bit rounding step of their own.
#[test]
fn applying_a_filter_attached_to_an_image_layer_bakes_it_into_the_layer() {
    for space in BlendSpace::ALL {
        for filter in ["black_white", "gaussian_blur"] {
            let what = format!("{space:?} {filter}");
            let mut doc = named_document(&["Effect", "Ink"], SIZE, space);
            doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color.depth = color::SampleDepth::U16;
            let picture = photo([140, 100], |_, _| 255);
            let layer = images(&mut doc, vec![placed(&picture, [1., 0., 0., 1., -40.5, 70.], false)]);
            let effect = named_occurrence(&doc, "Effect");
            let stack = doc.composition().result;
            let mut entries = doc.artwork.stacks.get(stack).unwrap().clone();
            entries.entries.retain(|h| ![effect, layer].contains(h));
            entries.entries.splice(0..0, [effect, layer]);
            doc.apply(Edit::Stack(RecordChange::replace(&doc.artwork.stacks, stack, Some(entries)).unwrap())).unwrap();
            let apply = effect_edit(&doc, effect, EffectInstance::new(bundled_effect_catalog().get(filter).unwrap().program()));
            doc.apply(apply).unwrap();
            let attach = occurrence_edit(&doc, effect, |o| o.attachment = Attachment::Effect);
            doc.apply(attach).unwrap();
            let (mut engine, _input) = engine(doc);
            let original = image(&mut engine, 1_000_000_000);
            engine.set_active_layer(effect).unwrap();
            assert_eq!(engine.document().merge_down(), MergeDown::ApplyEffect, "{what}");
            let plan = engine.document().merge_plan(MergeKind::Down).unwrap();
            let result = bake(&mut engine, plan);
            image(&mut engine, 2_000_000_000).assert_near(&original, TOLERANCE, &format!("{what}: applied"));
            let scene = engine.document().scene();
            assert!(scene.occurrence(effect).is_none() && scene.occurrence(layer).is_none(), "{what}: the filter and its image layer are consumed");
            assert_eq!(scene.occurrence(result).unwrap().kind(), LayerKind::Paint, "{what}");
            assert!(engine.undo().unwrap());
            image(&mut engine, 3_000_000_000).assert_eq(&original, &format!("{what}: undo"));
            assert_eq!(engine.document().scene().attached_effects(layer), [effect], "{what}");
        }
    }
}
