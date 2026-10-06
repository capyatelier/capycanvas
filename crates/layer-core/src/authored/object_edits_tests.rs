use super::*;
use crate::{color::source::rgba8_source, operation_test_support as fixture, Image};

fn image(extent: [u32; 2]) -> Image { rgba8_source(extent, |x, y| [x as u8, y as u8, 9, 255]).into() }
fn layer(doc: &mut Document, name: &str, parent: Option<OccurrenceHandle>) -> OccurrenceHandle {
    let (handle, edit) = doc.create_object_layer_edit(name, parent, 0).unwrap();
    doc.apply(edit).unwrap();
    handle
}
fn object(doc: &mut Document, layer: OccurrenceHandle, name: &str, affine: Affine64, at: usize) -> ImageObjectHandle {
    let mut value = ImageObject::new(image([10, 10]), name);
    value.affine = affine;
    let (handle, edit) = doc.add_image_object_edit(layer, value, at).unwrap();
    doc.apply(edit).unwrap();
    handle
}
fn translate(x: f64, y: f64) -> Affine64 { Affine64([1., 0., 0., 1., x, y]) }
fn select(doc: &mut Document, layer: OccurrenceHandle, objects: &[ImageObjectHandle]) {
    let mut working = doc.working.clone();
    working.occurrence = Some(layer);
    working.objects = objects.iter().copied().collect();
    doc.apply(Edit::Working(working)).unwrap();
}

#[test]
fn picking_is_front_to_back_through_offsets_rotation_and_excludes_hidden_or_locked_ancestors() {
    let mut doc = fixture::document([200, 200], &["Group", "Paint"]);
    let group = fixture::nest(&mut doc, "Group", &[]);
    let nested = layer(&mut doc, "Nested", Some(group));
    let top = layer(&mut doc, "Top", None);
    let back = object(&mut doc, top, "Back", translate(0., 0.), 0);
    let front = object(&mut doc, top, "Front", Affine64([0., 2., -2., 0., 30., 0.]), 0);
    let deep = object(&mut doc, nested, "Deep", translate(40., 40.), 0);
    fixture::occurrence_mut(&mut doc, "Group").offset = [100, 0];
    fixture::refresh(&mut doc);
    assert_eq!(doc.pick_image_object([5., 5.]), Some((top, back)));
    assert_eq!(doc.pick_image_object([15., 5.]), Some((top, front)));
    assert_eq!(doc.pick_image_object([145., 45.]), Some((nested, deep)));
    assert_eq!(doc.pick_image_object([45., 45.]), None);
    assert_eq!(doc.pick_image_object([150., 50.]), None);
    let mut hidden = doc.artwork.objects.get(front).unwrap().clone();
    hidden.visible = false;
    doc.apply(Edit::ImageObject(RecordChange::replace(&doc.artwork.objects, front, Some(hidden)).unwrap())).unwrap();
    assert_eq!(doc.pick_image_object([5., 5.]), Some((top, back)));
    assert_eq!(doc.pick_image_object([15., 5.]), None);
    fixture::occurrence_mut(&mut doc, "Group").locked = true;
    assert_eq!(doc.pick_image_object([145., 45.]), None);
    fixture::occurrence_mut(&mut doc, "Group").locked = false;
    fixture::occurrence_mut(&mut doc, "Group").visible = false;
    assert_eq!(doc.pick_image_object([145., 45.]), None);
}

#[test]
fn delete_and_duplicate_keep_identities_shared_images_and_selection_through_undo() {
    let mut doc = fixture::document([64, 64], &["Paint"]);
    let images = layer(&mut doc, "Images", None);
    let a = object(&mut doc, images, "A", translate(1., 2.), 0);
    let b = object(&mut doc, images, "B", translate(3., 4.), 1);
    select(&mut doc, images, &[a]);
    let (copies, edit) = doc.duplicate_image_objects_edit(&[a].into()).unwrap();
    let undo = doc.apply(edit).unwrap();
    assert_eq!(doc.object_layer_children(images).unwrap(), &[copies[0], a, b]);
    assert_eq!(doc.working.objects, [copies[0]].into());
    let copy = doc.scene().object(copies[0]).unwrap();
    assert!(copy.image.same_owner(&doc.scene().object(a).unwrap().image));
    assert_ne!(doc.artwork.objects.id(copies[0]), doc.artwork.objects.id(a));
    assert_eq!(copy.affine, translate(1., 2.));
    doc.apply(undo).unwrap();
    assert_eq!(doc.object_layer_children(images).unwrap(), &[a, b]);
    assert_eq!(doc.working.objects, [a].into());
    let before = doc.clone();
    let undo = doc.apply(doc.delete_image_objects_edit(&[a, b].into()).unwrap()).unwrap();
    assert!(doc.object_layer_children(images).unwrap().is_empty());
    assert!(doc.artwork.objects.is_empty());
    assert!(doc.working.objects.is_empty());
    assert!(doc.artwork.images().unwrap().is_empty());
    doc.apply(undo).unwrap();
    assert_eq!(doc.artwork, before.artwork);
    assert_eq!(doc.working.objects, before.working.objects);
    assert!(doc.delete_image_objects_edit(&BTreeSet::new()).is_err());
}

#[test]
fn ordering_moves_selected_objects_as_a_group_and_refuses_no_change() {
    let mut doc = fixture::document([64, 64], &["Paint"]);
    let images = layer(&mut doc, "Images", None);
    let handles: Vec<_> = (0..4).map(|index| object(&mut doc, images, &index.to_string(), translate(0., 0.), index)).collect();
    let [a, b, c, d] = handles[..] else { unreachable!() };
    let selected: BTreeSet<_> = [b, d].into();
    assert_eq!(ordered_objects(&handles, &selected, ObjectOrder::Front), vec![b, d, a, c]);
    assert_eq!(ordered_objects(&handles, &selected, ObjectOrder::Back), vec![a, c, b, d]);
    assert_eq!(ordered_objects(&handles, &selected, ObjectOrder::Forward), vec![b, a, d, c]);
    assert_eq!(ordered_objects(&handles, &selected, ObjectOrder::Backward), vec![a, c, b, d]);
    assert_eq!(ordered_objects(&handles, &[a, b].into(), ObjectOrder::Forward), handles);
    assert!(doc.reorder_image_objects_edit(&[a].into(), ObjectOrder::Front).is_err());
    doc.apply(doc.reorder_image_objects_edit(&[c].into(), ObjectOrder::Front).unwrap()).unwrap();
    assert_eq!(doc.object_layer_children(images).unwrap(), &[c, a, b, d]);
    doc.apply(doc.move_image_object_edit(c, 3).unwrap()).unwrap();
    assert_eq!(doc.object_layer_children(images).unwrap(), &[a, b, d, c]);
}

#[test]
fn object_selection_stays_inside_the_active_object_layer_and_survives_session_records() {
    let mut doc = fixture::document([64, 64], &["Paint"]);
    let first = layer(&mut doc, "First", None);
    let second = layer(&mut doc, "Second", None);
    let a = object(&mut doc, first, "A", translate(0., 0.), 0);
    let b = object(&mut doc, second, "B", translate(0., 0.), 0);
    select(&mut doc, first, &[a, b]);
    assert_eq!(doc.working.objects, [a].into());
    let mut working = doc.working.clone();
    working.occurrence = Some(fixture::id(&doc, "Paint"));
    doc.apply(Edit::Working(working)).unwrap();
    assert!(doc.working.objects.is_empty());
    select(&mut doc, second, &[b]);
    let mut inventory = crate::package::resources::ResourceInventory::default();
    let record = crate::package::session::WorkingRecord::capture(&doc.working, &mut inventory).unwrap();
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["objects"], serde_json::json!([b]));
    let interpolation = doc.set_image_objects_interpolation_edit(&[b].into(), ImageInterpolation::Nearest).unwrap();
    doc.apply(interpolation).unwrap();
    assert_eq!(doc.scene().object(b).unwrap().interpolation, ImageInterpolation::Nearest);
    assert!(doc.set_image_object_affines_edit(&[(b, Affine64([1., 2., 2., 4., 0., 0.]))]).is_err());
    fixture::occurrence_mut(&mut doc, "Second").locked = true;
    assert!(doc.set_image_object_affines_edit(&[(b, translate(1., 1.))]).is_err());
}
