//! Rasterize Layer, Convert to Image Layer and merges of image layers keep the
//! composite they replace, in both blend spaces, and one undo step restores
//! the records they consumed.
mod support;
use layer_core::*;
use support::*;

const TOLERANCE: u8 = 1;

/// Two overlapping images, one rotated and smooth, one off the frame's left
/// edge and nearest, in a half-opaque layer with a half-revealing mask.
fn scene(space: BlendSpace) -> (Document, OccurrenceHandle) {
    let mut doc = named_document(&["Ink"], SIZE, space);
    let image = photo([120, 90], |x, y| if (x + y) % 11 == 0 { 40 } else { 255 });
    let layer = images(&mut doc, vec![
        placed(&image, [0.8, 0.45, -0.45, 0.8, 120., 20.], false),
        placed(&image, [1.5, 0., 0., 1.5, -60.25, 100.5], true),
    ]);
    let mut mask = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), SIZE, [0; 2]);
    mask.source.default_coverage = 0.5;
    let edit = mask_edit(&doc, layer, mask);
    doc.apply(edit).unwrap();
    let edit = occurrence_edit(&doc, layer, |o| o.opacity = 0.6);
    doc.apply(edit).unwrap();
    (doc, layer)
}

fn rasterize(engine: &mut Engine, layer: OccurrenceHandle, apply_mask: bool) {
    let plan = engine.document().rasterize_plan(layer, apply_mask).unwrap();
    bake(engine, plan);
}

fn convert(engine: &mut Engine, layer: OccurrenceHandle) {
    let edit = match engine.document().convert_to_object(layer).unwrap() {
        ObjectConversion::Ready(edit) => edit,
        ObjectConversion::Capture(capture) => {
            let image = captured(engine, capture);
            engine.document().object_conversion_edit(layer, image).unwrap()
        }
    };
    engine.apply_edit(edit).unwrap();
}

#[test]
fn rasterize_layer_keeps_the_composite_and_one_undo_restores_the_images() {
    for space in BlendSpace::ALL {
        for apply_mask in [false, true] {
            let what = format!("{space:?}, apply mask {apply_mask}");
            let (doc, layer) = scene(space);
            let before = doc.clone();
            let (mut engine, _input) = engine(doc);
            let original = image(&mut engine, 0);
            rasterize(&mut engine, layer, apply_mask);
            image(&mut engine, 1).assert_near(&original, TOLERANCE, &format!("{what}: rasterized"));
            let document = engine.document();
            let occurrence = document.scene().occurrence(layer).unwrap();
            assert_eq!(occurrence.kind(), LayerKind::Paint);
            assert_eq!((occurrence.opacity, occurrence.mask.is_some()), (0.6, !apply_mask), "{what}");
            assert!(document.artwork.objects.is_empty(), "{what}");
            let base = paint(document, layer).base.as_ref().unwrap();
            assert!(base.offset[0] < 256 && occurrence.offset[0] < 0, "{what}: the off-frame image is baked left of the frame");
            assert!(engine.undo().unwrap());
            image(&mut engine, 2).assert_eq(&original, &format!("{what}: undo"));
            assert_eq!(engine.document().artwork.objects.len(), before.artwork.objects.len());
            assert_eq!(engine.document().scene().occurrence(layer), before.scene().occurrence(layer));
        }
    }
}

#[test]
fn a_hidden_image_layer_rasterizes_its_content_for_when_it_is_shown() {
    let (mut doc, layer) = scene(BlendSpace::Linear);
    let (mut shown, _input) = engine(doc.clone());
    let expected = image(&mut shown, 0);
    let edit = occurrence_edit(&doc, layer, |o| o.visible = false);
    doc.apply(edit).unwrap();
    let (mut engine, _input) = engine(doc);
    rasterize(&mut engine, layer, false);
    image(&mut engine, 1);
    let edit = occurrence_edit(engine.document(), layer, |o| o.visible = true);
    engine.apply_edit(edit).unwrap();
    image(&mut engine, 2).assert_near(&expected, TOLERANCE, "shown after rasterizing");
}

#[test]
fn convert_to_image_layer_keeps_the_composite_of_painted_and_untouched_layers() {
    for space in BlendSpace::ALL {
        let mut doc = named_document(&["Ink", "Photo"], SIZE, space);
        let photo_layer = named_occurrence(&doc, "Photo");
        paint_mut(&mut doc, photo_layer).base = Some(layer_core::authored::PaintBase { offset: [13, 29], ..layer_core::authored::PaintBase::new(photo([200, 150], |_, _| 255)) });
        let edit = occurrence_edit(&doc, photo_layer, |o| { o.opacity = 0.7; o.offset = [-40, 6]; });
        doc.apply(edit).unwrap();
        let (mut engine, mut input) = engine(doc);
        stroke_on(&mut engine, &mut input, "Ink", [0.9, 0.2, 0.1, 0.8], Point { x: 30., y: 40. }, Point { x: 300., y: 200. }, 1_000_000_000);
        let original = image(&mut engine, 2_000_000_000);
        for name in ["Photo", "Ink"] {
            let layer = named_occurrence(engine.document(), name);
            convert(&mut engine, layer);
            let what = format!("{space:?} {name}");
            image(&mut engine, 3_000_000_000).assert_near(&original, TOLERANCE, &format!("{what}: converted"));
            let document = engine.document();
            let children = &document.scene().object_layer(layer).unwrap().children;
            assert_eq!(children.len(), 1, "{what}");
            let object = document.scene().object(children[0]).unwrap();
            assert!(object.affine.0[..4] == [1., 0., 0., 1.] && object.affine.0[4..].iter().all(|v| v.fract() == 0.), "{what}: pixels stay on the grid");
        }
        assert!(engine.undo().unwrap() && engine.undo().unwrap());
        image(&mut engine, 4_000_000_000).assert_eq(&original, &format!("{space:?}: undo"));
        assert_eq!(engine.document().scene().occurrence(named_occurrence(engine.document(), "Ink")).unwrap().kind(), LayerKind::Paint);
    }
}

fn stroke_on(engine: &mut Engine, input: &mut layer_engine::InputProducer<layer_engine::PenEvent>, name: &str, color: [f32; 4], from: Point, to: Point, start: u64) {
    engine.set_active_layer(named_occurrence(engine.document(), name)).unwrap();
    draw(engine, input, color, from, to, start);
}

#[test]
fn merges_bake_image_layers_and_keep_their_off_frame_pixels() {
    for space in BlendSpace::ALL {
        let (mut doc, layer) = scene(space);
        let edit = occurrence_edit(&doc, named_occurrence(&doc, "Paper"), |o| o.visible = false);
        doc.apply(edit).unwrap();
        let shifted = |engine: &mut Engine, handle: OccurrenceHandle, time: u64| {
            let edit = occurrence_edit(engine.document(), handle, |o| o.offset = [o.offset[0] + 80, o.offset[1]]);
            engine.apply_edit(edit).unwrap();
            let shown = image(engine, time);
            assert!(engine.undo().unwrap());
            shown
        };
        let (mut engine, _input) = engine(doc);
        let original = image(&mut engine, 0);
        let revealed = shifted(&mut engine, layer, 1);
        for kind in [MergeKind::Down, MergeKind::Visible] {
            let what = format!("{space:?} {kind:?}");
            let plan = engine.document().merge_plan(kind).unwrap();
            let result = bake(&mut engine, plan);
            image(&mut engine, 2).assert_near(&original, TOLERANCE, &format!("{what}: merged"));
            if kind == MergeKind::Down {
                assert!(engine.document().scene().occurrence(layer).is_none() && engine.document().artwork.objects.is_empty(), "{what}");
            } else {
                let scene = engine.document().scene();
                assert!(scene.children(None).iter().filter(|h| **h != result).all(|h| !scene.occurrence(*h).unwrap().visible), "{what}: Merge Visible consumes every visible root");
            }
            assert!(engine.undo().unwrap());
            image(&mut engine, 4).assert_eq(&original, &format!("{what}: undo"));
        }
        let plan = engine.document().merge_plan(MergeKind::Visible).unwrap();
        let result = bake(&mut engine, plan);
        image(&mut engine, 5);
        let edit = occurrence_edit(engine.document(), result, |o| o.offset = [o.offset[0] + 80, o.offset[1]]);
        engine.apply_edit(edit).unwrap();
        let moved = image(&mut engine, 6);
        moved.crop([0, 0], [80, SIZE[1]]).assert_near(&revealed.crop([0, 0], [80, SIZE[1]]), TOLERANCE, &format!("{space:?}: off-frame image pixels survive Merge Visible"));
    }
}

/// An image layer as a clipping base with a blur attached: the blur applies to
/// the base alone, and a clipping-stack merge or an off-frame Merge Visible
/// keeps that composite, in a 16-bit drawing.
#[test]
fn clipping_runs_on_blurred_image_layers_merge_to_the_same_composite() {
    for space in BlendSpace::ALL {
        let mut doc = named_document(&["Clipped", "Blur", "Ink"], SIZE, space);
        doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color.depth = color::SampleDepth::U16;
        let picture = photo([140, 110], |x, _| if x < 70 { 255 } else { 120 });
        let layer = images(&mut doc, vec![placed(&picture, [1., 0., 0., 1., -40., 60.5], false)]);
        let stack = doc.composition().result;
        let mut entries = doc.artwork.stacks.get(stack).unwrap().clone();
        let [clipped, blur] = ["Clipped", "Blur"].map(|name| named_occurrence(&doc, name));
        entries.entries.retain(|h| ![clipped, blur, layer].contains(h));
        entries.entries.splice(0..0, [clipped, blur, layer]);
        doc.apply(Edit::Stack(RecordChange::replace(&doc.artwork.stacks, stack, Some(entries)).unwrap())).unwrap();
        let mut gaussian = EffectInstance::new(bundled_effect_catalog().get(SeparationFilters::BLUR).unwrap().program());
        gaussian.set("sigma", EffectValue::Number(5.)).unwrap();
        let edit = effect_edit(&doc, blur, gaussian);
        doc.apply(edit).unwrap();
        for (handle, attachment) in [(blur, Attachment::Effect), (clipped, Attachment::Clip)] {
            let edit = occurrence_edit(&doc, handle, |o| o.attachment = attachment);
            doc.apply(edit).unwrap();
        }
        let (mut engine, mut input) = engine(doc);
        engine.set_active_layer(clipped).unwrap();
        draw(&mut engine, &mut input, [0.1, 0.8, 0.2, 1.], Point { x: 0., y: 80. }, Point { x: 150., y: 150. }, 1_000_000_000);
        let original = image(&mut engine, 2_000_000_000);
        engine.set_active_layer(layer).unwrap();
        assert_eq!(engine.document().merge_down(), MergeDown::ClippingStack);
        for kind in [MergeKind::Down, MergeKind::Visible] {
            let what = format!("{space:?} {kind:?}");
            let plan = engine.document().merge_plan(kind).unwrap();
            bake(&mut engine, plan);
            image(&mut engine, 3_000_000_000).assert_near(&original, TOLERANCE, &format!("{what}: merged"));
            assert!(engine.undo().unwrap());
            image(&mut engine, 4_000_000_000).assert_eq(&original, &format!("{what}: undo"));
            engine.set_active_layer(layer).unwrap();
        }
    }
}

/// An image far beyond the canvas rasterizes into its own pages; moving the
/// result back shows the same pixels as the image placed there, in a 16-bit
/// drawing.
#[test]
fn far_off_canvas_images_rasterize_into_their_own_pages() {
    let picture = photo([90, 70], |x, y| if (x + y) % 9 == 0 { 30 } else { 255 });
    let near = |doc: &mut Document, x: f64| {
        doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color.depth = color::SampleDepth::U16;
        images(doc, vec![placed(&picture, [1.25, 0., 0., 1.25, x, 60.5], false)])
    };
    let mut reference = named_document(&["Ink"], SIZE, BlendSpace::Linear);
    near(&mut reference, 100.25);
    let (mut shown, _input) = engine(reference);
    let expected = image(&mut shown, 0);
    let mut doc = named_document(&["Ink"], SIZE, BlendSpace::Linear);
    let layer = near(&mut doc, 40_100.25);
    let (mut engine, _input) = engine(doc);
    rasterize(&mut engine, layer, false);
    let domain = paint(engine.document(), layer).domain;
    assert!(domain[0] < 512, "only the image's pages: {domain:?}");
    let edit = occurrence_edit(engine.document(), layer, |o| o.offset = [o.offset[0] - 40_000, o.offset[1]]);
    engine.apply_edit(edit).unwrap();
    image(&mut engine, 1).assert_near(&expected, TOLERANCE, "moved back into view");
}
