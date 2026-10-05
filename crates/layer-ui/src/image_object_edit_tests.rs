use super::{*, test_support::*};
use layer_core::authored::{Affine64, Image, ImageObject};
use layer_core::color::source::rgba8_source;

#[test]
fn object_only_layer_accepts_local_filters_and_keeps_paint_color_modes_read_only() {
    let mut doc = Document::from_artwork(layer_core::authored::Artwork::new([64;2]).unwrap()).unwrap();
    let (owner, edit) = doc.create_object_layer_edit("Images",None,0).unwrap();
    doc.apply(edit).unwrap();
    let mut image = ImageObject::new(rgba8_source([8;2],|_,_|[17,33,65,255]).into(),"Photo");
    image.affine = Affine64([1.,0.,0.,1.,17.125,9.]);
    let (object, edit) = doc.add_image_object_edit(owner,image,0).unwrap();
    doc.apply(edit).unwrap();
    let mut s = UiSession::new(Recorder { tiled_sources: true, ..Default::default() },doc,[64;2],Platform::Gtk).unwrap();
    s.dispatch(UiAction::SelectLayer { id: occurrence_token(owner) }).unwrap();
    assert!(s.engine.document().artwork.paint.is_empty());
    assert!(s.state.layer_tools.add_filter.is_some());
    assert_eq!(s.state.layer_properties.add_filter,s.state.layer_tools.add_filter);
    assert!(s.state.layer_properties.controls.iter().all(|control|control.key != "color_mode"));
    let before = s.engine.document().clone();
    let checkpoint = s.engine.checkpoint();
    assert!(s.dispatch(UiAction::Layer { action: LayerAction::ColorMode {
        id: occurrence_token(owner), epoch: s.state.document_file.epoch,
        mode: layer_core::color::LayerColorMode::Grayscale,
    } }).is_err());
    assert_eq!(s.engine.checkpoint(),checkpoint);
    assert_live_artwork_eq(s.engine.document(),&before);
    s.dispatch(UiAction::Effect { action: EffectAction::InsertAttached {
        effect: "gaussian_blur".into(), owner: occurrence_token(owner), epoch: s.state.document_file.epoch,
    } }).unwrap();
    let attached = s.engine.document().working.occurrence.unwrap();
    let scene = s.engine.document().scene();
    assert_eq!(scene.effect_owner(attached),Some(owner));
    assert_eq!(scene.attached_effects(owner),&[attached]);
    assert_eq!(scene.order(),&[attached,owner]);
    let application = scene.effect_application(attached).unwrap();
    assert_eq!(application.program.id.as_ref(),"gaussian_blur");
    assert_eq!(application.spatial.unwrap().mapping,Affine64::default());
    assert_eq!(application.spatial.unwrap().extent,[64.;2]);
    assert_eq!(scene.object(object),before.scene().object(object));
    assert!(s.state.layer_properties.controls.iter().all(|control|control.key != "color_mode"));
    let accepted = s.engine.document().clone();
    invoke(&mut s,CommandId::Undo);
    assert_live_artwork_eq(s.engine.document(),&before);
    invoke(&mut s,CommandId::Redo);
    assert_live_artwork_eq(s.engine.document(),&accepted);
}

#[test]
fn affine_image_edit_publishes_canvas_damage_and_atomic_history() {
    let mut session = session(Platform::Android);
    let source = rgba8_source([1,1],|_,_|[17,33,65,255]);
    let (layer, edit) = session.engine.document().create_object_layer_edit("Images",None,0).unwrap();
    session.engine.apply_edit(edit).unwrap();
    let (object, edit) = session.engine.document().add_image_object_edit(layer,
        ImageObject::new(Image::new(source),"Photo"),0).unwrap();
    session.engine.apply_edit(edit).unwrap();
    let before = session.engine.document().clone();
    session.set_image_object_motion(Some(object)).unwrap();
    assert_eq!(session.engine.backend().moving_layer,Some(layer));
    let affine = Affine64([0.5,0.1,-0.1,0.5,12.,9.]);
    let change = session.set_image_object_affine(object,affine).unwrap();
    assert!(change.canvas_wake);
    assert_ne!(change.regions & regions::DOCUMENT,0);
    assert_eq!(session.engine.document().scene().object(object).unwrap().affine,affine);
    let accepted = session.engine.document().clone();
    session.engine.undo().unwrap();
    assert_live_artwork_eq(session.engine.document(),&before);
    session.engine.redo().unwrap();
    assert_live_artwork_eq(session.engine.document(),&accepted);
    assert!(session.set_image_object_affine(object,Affine64([0.;6])).is_err());
    assert_live_artwork_eq(session.engine.document(),&accepted);
    session.set_image_object_motion(None).unwrap();
    assert_eq!(session.engine.backend().moving_layer,None);
}

#[test]
fn unsupported_image_sampling_leaves_document_checkpoint_and_redo_intact() {
    let mut session=session(Platform::Android);
    let (layer,edit)=session.engine.document().create_object_layer_edit("Images",None,0).unwrap();session.engine.apply_edit(edit).unwrap();
    let (object,edit)=session.engine.document().add_image_object_edit(layer,ImageObject::new(rgba8_source([1,1],|_,_|[17,33,65,255]).into(),"Photo"),0).unwrap();session.engine.apply_edit(edit).unwrap();
    session.set_image_object_affine(object,Affine64([1.,0.,0.,1.,12.,9.])).unwrap();let accepted=session.engine.document().clone();
    session.engine.undo().unwrap();let before=session.engine.document().clone();let checkpoint=session.engine.checkpoint();
    let history=(session.engine.can_undo(),session.engine.can_redo());assert!(history.1);
    session.engine.backend_mut().image_affine_requests.borrow_mut().clear();session.engine.backend_mut().reject_image_affines=true;
    let affine=Affine64([0.001,0.,0.,0.001,12.,9.]);let view=session.engine.view();
    assert_eq!(session.set_image_object_affine(object,affine).unwrap_err(),"Image sampling request is unsupported");
    assert_eq!(session.engine.document(),&before);assert_eq!(session.engine.checkpoint(),checkpoint);
    assert_eq!((session.engine.can_undo(),session.engine.can_redo()),history);
    assert_eq!(*session.engine.backend().image_affine_requests.borrow(),vec![(object,affine,view)]);
    session.engine.redo().unwrap();assert_live_artwork_eq(session.engine.document(),&accepted);
    session.engine.undo().unwrap();assert_live_artwork_eq(session.engine.document(),&before);
}
