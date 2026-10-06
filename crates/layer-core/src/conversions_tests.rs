use super::*;
use crate::color::source::rgba8_source;
use crate::operation_test_support as fixture;
use fixture::*;

fn images(doc: &mut Document, affines: &[Affine64]) -> OccurrenceHandle {
    let (layer, edit) = doc.create_object_layer_edit("Images", None, 0).unwrap();
    doc.apply(edit).unwrap();
    let image: Image = rgba8_source([40, 30], |x, y| [x as u8, y as u8, 9, 255]).into();
    for (index, affine) in affines.iter().enumerate() {
        let mut object = ImageObject::new(image.clone(), format!("Image {index}"));
        object.affine = *affine;
        let (_, edit) = doc.add_image_object_edit(layer, object, index).unwrap();
        doc.apply(edit).unwrap();
    }
    doc.working.occurrence = Some(layer);
    doc.working.target = None;
    layer
}
fn translation(x: f64, y: f64) -> Affine64 { Affine64([1., 0., 0., 1., x, y]) }
fn changed<T>(edits: &[Edit], pick: impl Fn(&Edit) -> Option<T>) -> T {
    edits.iter().find_map(pick).unwrap()
}
fn occurrence_after(edits: &[Edit], h: OccurrenceHandle) -> Occurrence {
    changed(edits, |e| match e { Edit::Occurrence(c) if c.handle == h => c.value.clone(), _ => None })
}
fn edited(doc: &mut Document, h: OccurrenceHandle, update: impl FnOnce(&mut Occurrence)) {
    update(doc.artwork.occurrences.get_mut(h).unwrap());
    fixture::refresh(doc);
}

#[test]
fn rasterize_keeps_the_occurrence_and_bakes_off_frame_images_into_one_undoable_paint_layer() {
    let mut doc = fixture::document([600, 400], &["Ink"]);
    let layer = images(&mut doc, &[translation(-300., 10.), translation(500., 380.)]);
    edited(&mut doc, layer, |o| { o.opacity = 0.5; o.blend = LayerBlend::Multiply; });
    let before = doc.clone();
    let plan = doc.rasterize_plan(layer, false).unwrap();
    assert_eq!(plan.result, layer);
    let occurrence = occurrence_after(&plan.edits, layer);
    assert_eq!((occurrence.opacity, occurrence.blend), (0.5, LayerBlend::Multiply), "presentation stays on the layer and applies once");
    assert_eq!(occurrence.offset, [-512, 0], "whole pages reach the off-frame image");
    let OccurrenceContent::Paint(paint) = occurrence.content else { panic!("paint") };
    let source = changed(&plan.edits, |e| match e { Edit::Paint(c) if c.handle == paint => c.value.clone(), _ => None });
    assert_eq!(source.domain, [1112, 410]);
    let RasterOperationKind::Bake { scene, scope, offset } = &plan.operation.kind else { panic!("bake") };
    assert_eq!((scope, *offset), (&SceneScope::RawObjects(layer), Point { x: 512., y: 0. }));
    let raw = scene.view().occurrence(layer).unwrap();
    assert_eq!((raw.opacity, raw.blend, raw.mask.is_none()), (1., LayerBlend::Normal, true), "the bake reads raw content");
    let mut edits = plan.edits.clone();
    for edit in &mut edits {
        if let Edit::Paint(change) = edit { Arc::make_mut(&mut change.value.as_mut().unwrap().operations).push(plan.operation.clone()); }
    }
    let undo = doc.apply(Edit::Batch(edits)).unwrap();
    assert!(doc.artwork.objects.is_empty() && doc.artwork.object_layers.is_empty());
    assert_eq!(doc.working.target, Some(plan.target));
    assert!(!doc.artwork.topology().unwrap().objects.values().any(|shape| matches!(shape, crate::authored::Shape::Image)), "a portable save omits the image's last use");
    doc.apply(undo).unwrap();
    restored(&before, &doc);
    assert_eq!(doc.artwork.objects.len(), 2);
}

#[test]
fn rasterize_keeps_a_linked_mask_where_it_was_and_apply_mask_removes_it() {
    let mut doc = fixture::document([600, 400], &["Ink"]);
    let layer = images(&mut doc, &[translation(-300., 10.)]);
    edited(&mut doc, layer, |o| o.offset = [7, 3]);
    let mask = add_mask(&mut doc, layer, [600, 400], [5, -2]);
    let world = doc.scene().mask_origin(layer).unwrap();
    let plan = doc.rasterize_plan(layer, false).unwrap();
    let mut after = doc.clone();
    after.apply(Edit::Batch(plan.edits)).unwrap();
    assert_ne!(after.scene().occurrence(layer).unwrap().offset, [7, 3]);
    assert_eq!(after.scene().mask_origin(layer), Some(world), "a linked mask stays in place");
    edited(&mut doc, layer, |o| o.mask.as_mut().unwrap().linked = false);
    let plan = doc.rasterize_plan(layer, false).unwrap();
    let mut after = doc.clone();
    after.apply(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(after.scene().mask_origin(layer), doc.scene().mask_origin(layer), "an unlinked mask doesn't follow storage origins");
    let plan = doc.rasterize_plan(layer, true).unwrap();
    let RasterOperationKind::Bake { scene, scope, .. } = &plan.operation.kind else { panic!("bake") };
    assert!(scene.view().occurrence(layer).unwrap().mask.is_some(), "the bake applies the mask");
    assert_eq!(scope, &SceneScope::Members(vec![layer].into()), "the layer's own content and mask, without its effects");
    let mut after = doc.clone();
    after.apply(Edit::Batch(plan.edits)).unwrap();
    assert!(after.scene().occurrence(layer).unwrap().mask.is_none());
    assert!(after.artwork.coverage.get(mask).is_none());
    edited(&mut doc, layer, |o| o.mask.as_mut().unwrap().enabled = false);
    assert_eq!(doc.rasterize_plan(layer, true).err(), Some(ConversionRefusal::MaskDisabled));
    edited(&mut doc, layer, |o| o.mask = None);
    assert_eq!(doc.rasterize_plan(layer, true).err(), Some(ConversionRefusal::NoMask));
}

#[test]
fn rasterize_refuses_other_layers_locks_and_unaddressable_content_without_changes() {
    let mut doc = fixture::document([600, 400], &["Ink"]);
    assert_eq!(doc.rasterize_refusal(id(&doc, "Ink"), false), Some(ConversionRefusal::NotObjects));
    let layer = images(&mut doc, &[translation(0., 0.)]);
    edited(&mut doc, layer, |o| o.visible = false);
    assert!(doc.rasterize_plan(layer, false).is_ok(), "hidden layers rasterize their raw content");
    edited(&mut doc, layer, |o| o.locked = true);
    assert_eq!(doc.rasterize_plan(layer, false).err(), Some(ConversionRefusal::Locked));
    let far = images(&mut doc, &[translation(0., 0.), translation(100_000., 0.)]);
    let before = doc.clone();
    assert_eq!(doc.rasterize_plan(far, false).err(), Some(ConversionRefusal::TooLarge));
    assert_eq!(doc, before);
}

#[test]
fn a_small_image_far_beyond_the_canvas_rasterizes_without_spanning_to_it() {
    let mut doc = fixture::document([600, 400], &["Ink"]);
    let far = images(&mut doc, &[translation(100_000.5, -50_000.)]);
    let plan = doc.rasterize_plan(far, false).unwrap();
    let occurrence = occurrence_after(&plan.edits, far);
    let OccurrenceContent::Paint(paint) = occurrence.content else { panic!("paint") };
    let source = changed(&plan.edits, |e| match e { Edit::Paint(c) if c.handle == paint => c.value.clone(), _ => None });
    assert_eq!((occurrence.offset, source.domain), ([99_840, -50_176], [203, 209]), "only the image's pages, not the span to the canvas");
    let near = images(&mut doc, &[translation(50., 60.)]);
    let plan = doc.rasterize_plan(near, false).unwrap();
    let occurrence = occurrence_after(&plan.edits, near);
    let OccurrenceContent::Paint(paint) = occurrence.content else { panic!("paint") };
    let source = changed(&plan.edits, |e| match e { Edit::Paint(c) if c.handle == paint => c.value.clone(), _ => None });
    assert_eq!((occurrence.offset, source.domain), ([0, 0], [600, 400]), "nearby content still covers the canvas so it can be painted across");
}

#[test]
fn a_filter_whose_reach_overflows_refuses_every_bake_without_changes() {
    let mut doc = fixture::document([600, 400], &["Blur", "Ink"]);
    effect(&mut doc, "Blur", "gaussian_blur");
    let blur = id(&doc, "Blur");
    let OccurrenceContent::Effect(application) = doc.scene().occurrence(blur).unwrap().content else { panic!("effect") };
    let program = doc.artwork.effects.get(application).unwrap().program.clone();
    let sigma = program.parameters.iter().position(|p| p.key.as_ref() == "sigma").unwrap();
    doc.artwork.effects.get_mut(application).unwrap().values[sigma] = EffectValue::Number(1e30);
    fixture::refresh(&mut doc);
    assert_eq!(doc.scene().effect(blur).unwrap().support_radius(), None);
    let before = doc.clone();
    for kind in [MergeKind::Visible, MergeKind::Flatten, MergeKind::Stamp] {
        assert_eq!(doc.merge_plan(kind).err(), Some(MergeRefusal::UnboundedSupport), "{kind:?}");
    }
    fixture::activate(&mut doc, "Blur");
    assert_eq!(doc.merge_plan(MergeKind::Down).err(), Some(MergeRefusal::UnboundedSupport));
    assert_eq!(doc, before, "a refused bake changes nothing");
}

#[test]
fn smooth_fractional_images_reach_beyond_their_rectangle_but_exact_placements_do_not() {
    let image = ImageObject::new(rgba8_source([10, 10], |_, _| [0, 0, 0, 255]).into(), "");
    assert_eq!(object_support(translation(3., 4.), &image), SupportBounds { min: [3., 4.], max: [13., 14.] });
    assert_eq!(object_support(Affine64([0., 1., -1., 0., 10., 0.]), &image), SupportBounds { min: [0., 0.], max: [10., 10.] });
    let smooth = object_support(translation(3.5, 4.), &image);
    assert!(smooth.min[0] < 3.5 && smooth.max[0] > 13.5);
    let mut nearest = image.clone();
    nearest.interpolation = ImageInterpolation::Nearest;
    assert_eq!(object_support(translation(3.5, 4.), &nearest), SupportBounds { min: [3.5, 4.], max: [13.5, 14.] });
}

#[test]
fn merges_include_object_layers_and_keep_their_off_frame_images() {
    let mut doc = fixture::document([600, 400], &["Ink"]);
    let layer = images(&mut doc, &[translation(-300., 10.)]);
    let plan = doc.merge_plan(MergeKind::Visible).unwrap();
    assert_eq!(occurrence_after(&plan.edits, plan.result).offset, [-512, 0]);
    let RasterOperationKind::Bake { scope: SceneScope::Members(members), .. } = &plan.operation.kind else { panic!("bake") };
    assert!(members.contains(&layer));
    let plan = doc.merge_plan(MergeKind::Down).unwrap();
    let RasterOperationKind::Bake { scope: SceneScope::Members(members), .. } = &plan.operation.kind else { panic!("bake") };
    assert!(members.contains(&layer) && members.contains(&id(&doc, "Ink")));
}

#[test]
fn whole_drawing_filters_resample_within_their_input_and_keep_off_frame_content() {
    let mut doc = fixture::document([600, 400], &["Swirl", "Ink"]);
    effect(&mut doc, "Swirl", "swirl");
    let ink = id(&doc, "Ink");
    edited(&mut doc, ink, |o| o.offset = [-700, 0]);
    let plan = doc.merge_plan(MergeKind::Visible).unwrap();
    let result = occurrence_after(&plan.edits, plan.result);
    let OccurrenceContent::Paint(paint) = result.content else { panic!("paint") };
    let source = changed(&plan.edits, |e| match e { Edit::Paint(c) if c.handle == paint => c.value.clone(), _ => None });
    assert_eq!((result.offset, source.domain), ([-768, 0], [1368, 400]), "the off-frame paint survives a whole-drawing filter");
}

#[test]
fn convert_to_object_reuses_an_untouched_photo_and_captures_edited_paint() {
    let mut doc = fixture::document([600, 400], &["Photo"]);
    let image: Image = rgba8_source([40, 30], |x, y| [x as u8, y as u8, 9, 255]).into();
    paint_mut(&mut doc, "Photo").base = Some(PaintBase { image: image.clone(), offset: [13, 29], policy: PaintBasePolicy::SourceProfile });
    let photo = id(&doc, "Photo");
    occurrence_mut(&mut doc, "Photo").offset = [-4, 6];
    occurrence_mut(&mut doc, "Photo").alpha_locked = true;
    let mask = add_mask(&mut doc, photo, [600, 400], [1, 1]);
    let world = doc.scene().mask_origin(photo);
    let OccurrenceContent::Paint(paint) = doc.scene().occurrence(photo).unwrap().content else { panic!("paint") };
    let before = doc.clone();
    let ObjectConversion::Ready(edit) = doc.convert_to_object(photo).unwrap() else { panic!("an untouched photo needs no capture") };
    let undo = doc.apply(edit).unwrap();
    let occurrence = doc.scene().occurrence(photo).unwrap();
    assert_eq!((occurrence.kind(), occurrence.offset, occurrence.alpha_locked), (LayerKind::Object, [-4, 6], false));
    assert_eq!(occurrence.mask.as_ref().map(|m| m.source), Some(mask));
    assert_eq!(doc.scene().mask_origin(photo), world);
    let [object] = doc.scene().object_layer(photo).unwrap().children[..] else { panic!("one image") };
    let object = doc.scene().object(object).unwrap();
    assert!(object.image.same_owner(&image), "the image is shared, not copied");
    assert_eq!(object.affine, translation(13., 29.));
    assert!(doc.artwork.paint.get(paint).is_none());
    doc.apply(undo).unwrap();
    restored(&before, &doc);

    paint_mut(&mut doc, "Photo").raster = crate::raster::RasterRevision::backed(crate::raster::RasterData {
        tiles: [(crate::raster::TileKey { plane: crate::raster::RasterPlane::Color, coordinate: [1, 0] },
            crate::raster::RasterTile::pending(crate::raster::RasterPlane::Color.descriptor(Default::default())))].into(),
        watercolor: None });
    let ObjectConversion::Capture(capture) = doc.convert_to_object(photo).unwrap() else { panic!("edited paint is captured") };
    let target = fixture::target(&doc, "Photo");
    assert_eq!((&capture.scope, capture.trim, capture.offset, capture.extent, capture.window), (&SceneScope::Raw(target), Some(target), Point { x: 4., y: -6. }, [600, 400], [13, 0, 499, 256]));
    let mut empty = doc.clone();
    empty.apply(doc.object_conversion_edit(photo, None).unwrap()).unwrap();
    assert!(empty.scene().object_layer(photo).unwrap().children.is_empty(), "a layer without pixels becomes an empty image layer");
    occurrence_mut(&mut doc, "Photo").locked = true;
    assert_eq!(doc.convert_to_object(photo).err(), Some(ConversionRefusal::Locked));
    let empty = images(&mut doc, &[]);
    assert_eq!(doc.convert_to_object_refusal(empty), Some(ConversionRefusal::NotPaint));
}

#[test]
fn grouping_ungrouping_and_duplicating_keep_image_layer_membership_and_placement() {
    let mut doc = fixture::document([600, 400], &["Ink"]);
    let layer = images(&mut doc, &[translation(-30.5, 12.25), translation(100., 40.)]);
    edited(&mut doc, layer, |o| o.offset = [7, -3]);
    let children = doc.scene().object_layer(layer).unwrap().children.clone();
    let world = |doc: &Document, h: OccurrenceHandle| doc.scene().layer_origin(Some(h));
    let before = world(&doc, layer);
    doc.apply(doc.group_layers_edit(&[layer], LayerBlend::Normal, "Group").unwrap()).unwrap();
    let group = doc.scene().parent(layer).unwrap();
    assert_eq!(doc.scene().object_layer(layer).unwrap().children, children, "grouping keeps the image list");
    assert_eq!(world(&doc, layer), before);
    edited(&mut doc, group, |o| o.offset = [40, 5]);
    let moved = world(&doc, layer);
    doc.apply(doc.ungroup_layer_edit(group).unwrap()).unwrap();
    assert_eq!((world(&doc, layer), doc.scene().parent(layer)), (moved, None), "ungrouping keeps the document position");
    assert_eq!(doc.scene().object_layer(layer).unwrap().children, children);
    let (edit, copies) = doc.duplicate_layers_edit(&[layer]).unwrap();
    doc.apply(edit).unwrap();
    let copied = &doc.scene().object_layer(copies[0]).unwrap().children;
    assert_eq!(copied.len(), 2);
    assert!(copied.iter().all(|h| !children.contains(h)), "duplicates get their own image identities");
    for (copy, original) in copied.iter().zip(&children) {
        let [copy, original] = [copy, original].map(|h| doc.scene().object(*h).unwrap());
        assert!(copy.image.same_owner(&original.image) && copy.affine == original.affine);
    }
}
