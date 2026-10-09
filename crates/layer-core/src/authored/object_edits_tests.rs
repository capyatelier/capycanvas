use super::*;
use crate::{color::source::rgba8_source, operation_test_support as fixture, Image, OccurrenceDropPosition};

fn layer(doc:&mut Document,name:&str,parent:Option<OccurrenceHandle>,affine:Affine64,at:usize)->OccurrenceHandle {
    let mut object=ImageObject::new(Image::new(rgba8_source([10,10],|x,y|[x as u8,y as u8,9,255])));object.affine=affine;
    let (handle,edit)=doc.create_object_layer_edit(name,object,parent,at).unwrap();doc.apply(edit).unwrap();handle
}
fn translate(x:f64,y:f64)->Affine64 {Affine64([1.,0.,0.,1.,x,y])}

#[test]
fn picking_is_front_to_back_through_offsets_rotation_and_excludes_hidden_or_locked_ancestors() {
    let mut doc=fixture::document([200,200],&["Group","Paint"]);let group=fixture::nest(&mut doc,"Group",&[]);
    let back=layer(&mut doc,"Back",None,translate(0.,0.),0);
    let front=layer(&mut doc,"Front",None,Affine64([0.,2.,-2.,0.,30.,0.]),0);
    let deep=layer(&mut doc,"Deep",Some(group),translate(40.,40.),0);
    fixture::occurrence_mut(&mut doc,"Group").offset=[100,0];fixture::refresh(&mut doc);
    let picked=|doc:&Document,owner|Some((owner,doc.scene().object_handle(owner).unwrap()));
    assert_eq!(doc.pick_image_object([5.,5.]),picked(&doc,back));assert_eq!(doc.pick_image_object([15.,5.]),picked(&doc,front));
    assert_eq!(doc.pick_image_object([145.,45.]),picked(&doc,deep));assert_eq!(doc.pick_image_object([45.,45.]),None);assert_eq!(doc.pick_image_object([150.,50.]),None);
    let mut hidden=doc.scene().occurrence(front).unwrap().clone();hidden.visible=false;
    doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,front,Some(hidden)).unwrap())).unwrap();
    assert_eq!(doc.pick_image_object([5.,5.]),picked(&doc,back));assert_eq!(doc.pick_image_object([15.,5.]),None);
    fixture::occurrence_mut(&mut doc,"Group").locked=true;assert_eq!(doc.pick_image_object([145.,45.]),None);
    fixture::occurrence_mut(&mut doc,"Group").locked=false;fixture::occurrence_mut(&mut doc,"Group").visible=false;assert_eq!(doc.pick_image_object([145.,45.]),None);
}
#[test]
fn delete_and_duplicate_keep_identities_shared_images_and_selection_through_undo() {
    let mut doc=fixture::document([64,64],&["Paint"]);let a=layer(&mut doc,"A",None,translate(1.,2.),0);let b=layer(&mut doc,"B",None,translate(3.,4.),1);
    let mut working=doc.working.clone();working.occurrence=Some(a);working.layer_selection=[a].into();doc.apply(Edit::Working(working)).unwrap();
    let (edit,copies)=doc.duplicate_layers_edit(&[a]).unwrap();let undo=doc.apply(edit).unwrap();
    let original=doc.scene().object_handle(a).unwrap();let copy=doc.scene().object_handle(copies[0]).unwrap();
    assert!(doc.scene().object(copy).unwrap().image.same_owner(&doc.scene().object(original).unwrap().image));assert_ne!(doc.artwork.objects.id(copy),doc.artwork.objects.id(original));assert_eq!(doc.scene().object(copy).unwrap().affine,translate(1.,2.));
    doc.apply(undo).unwrap();let before=doc.clone();let undo=doc.apply(doc.delete_layers_edit(&[a,b]).unwrap()).unwrap();
    assert!(doc.artwork.objects.is_empty());assert!(doc.selected_objects().is_empty());assert!(doc.artwork.images().unwrap().is_empty());
    doc.apply(undo).unwrap();assert_eq!(doc.artwork,before.artwork);assert_eq!(doc.working.layer_selection,before.working.layer_selection);assert_eq!(doc.working.occurrence,before.working.occurrence);assert_eq!(doc.selected_objects(),before.selected_objects());assert!(doc.delete_layers_edit(&[]).is_err());
}
#[test]
fn object_selection_is_derived_from_selected_occurrences_and_saved_as_layers() {
    let mut doc=fixture::document([64,64],&["Paint"]);let a=layer(&mut doc,"A",None,translate(0.,0.),0);let b=layer(&mut doc,"B",None,translate(0.,0.),1);
    let mut working=doc.working.clone();working.occurrence=Some(a);working.layer_selection=[a,b].into();doc.apply(Edit::Working(working)).unwrap();
    let handles=[doc.scene().object_handle(a).unwrap(),doc.scene().object_handle(b).unwrap()].into();assert_eq!(doc.selected_objects(),handles);
    let mut inventory=crate::package::resources::ResourceInventory::default();let record=crate::package::session::WorkingRecord::capture(&doc.working,&mut inventory).unwrap();let json=serde_json::to_value(record).unwrap();assert!(json.get("objects").is_none());
    let object=doc.scene().object_handle(b).unwrap();doc.apply(doc.set_image_objects_interpolation_edit(&[object].into(),ImageInterpolation::Nearest).unwrap()).unwrap();assert_eq!(doc.scene().object(object).unwrap().interpolation,ImageInterpolation::Nearest);
    assert!(doc.set_image_object_affines_edit(&[(object,Affine64([1.,2.,2.,4.,0.,0.]))]).is_err());fixture::occurrence_mut(&mut doc,"B").locked=true;assert!(doc.set_image_object_affines_edit(&[(object,translate(1.,1.))]).is_err());
}

#[test]
fn ordering_moves_selected_object_layers_as_a_group_without_changing_sources() {
    let mut doc=fixture::document([64,64],&["Paint"]);
    let layers:Vec<_>=(0..4).map(|i|layer(&mut doc,&i.to_string(),None,translate(i as f64,0.),i)).collect();
    let [a,b,c,d]=layers[..] else {unreachable!()};let objects:Vec<_>=layers.iter().map(|h|doc.scene().object_layer(*h).unwrap().clone()).collect();
    let plan=doc.drop_layers_edit(&[b,d],a,OccurrenceDropPosition::Above).unwrap();let undo=doc.apply(plan.edit).unwrap();
    assert_eq!(&doc.scene().order()[..4],&[b,d,a,c]);
    for (h,object) in layers.iter().zip(&objects) {assert_eq!(doc.scene().object_layer(*h).unwrap(),object);}
    doc.apply(undo).unwrap();assert_eq!(&doc.scene().order()[..4],&layers);
    let plan=doc.drop_layers_edit(&[c],d,OccurrenceDropPosition::Below).unwrap();doc.apply(plan.edit).unwrap();assert_eq!(&doc.scene().order()[..4],&[a,b,d,c]);
}
