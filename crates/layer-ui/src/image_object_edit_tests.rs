use super::{*, test_support::*};
use layer_core::authored::{Affine64, Image, ImageObject};
use layer_core::color::source::rgba8_source;

#[test]
fn object_only_layer_accepts_local_filters_and_keeps_paint_color_modes_read_only() {
    let mut doc = Document::from_artwork(layer_core::authored::Artwork::new([64;2]).unwrap()).unwrap();
    let mut image = ImageObject::new(rgba8_source([8;2],|_,_|[17,33,65,255]).into());
    image.affine = Affine64([1.,0.,0.,1.,17.125,9.]);
    let (owner, edit) = doc.create_object_layer_edit("Photo",image,None,0).unwrap();
    doc.apply(edit).unwrap();
    let object = doc.scene().object_handle(owner).unwrap();
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
fn object_motion_previews_without_history_and_commits_one_atomic_edit() {
    let mut session = session(Platform::Android);
    let source = rgba8_source([1,1],|_,_|[17,33,65,255]);
    let (layer, edit) = session.engine.document().create_object_layer_edit("Photo",
        ImageObject::new(Image::new(source.clone())),None,0).unwrap();
    session.engine.apply_edit(edit).unwrap();
    let first = session.engine.document().scene().object_handle(layer).unwrap();
    let mut second = ImageObject::new(Image::new(source));second.affine = Affine64([2.,0.,0.,2.,5.,7.]);
    let (second_layer, edit) = session.engine.document().create_object_layer_edit("Second",second,None,1).unwrap();
    session.engine.apply_edit(edit).unwrap();
    let second = session.engine.document().scene().object_handle(second_layer).unwrap();
    for (owner, position) in [(layer, [3, -4]), (second_layer, [-8, 6])] {
        let mut offset = session.engine.document().artwork.occurrences.get(owner).unwrap().clone();offset.offset = position;
        session.engine.apply_edit(layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&session.engine.document().artwork.occurrences,owner,Some(offset)).unwrap())).unwrap();
    }
    let before = session.engine.document().clone();
    let checkpoint = session.engine.checkpoint();
    session.begin_object_motion(&[first,second]).unwrap();
    assert_eq!(session.engine.backend().moving_layer,Some(layer));
    for step in 1..=12 {
        let change = session.preview_object_motion(Affine64([1.,0.,0.,1.,f64::from(step),0.5])).unwrap();
        assert!(change.canvas_wake);
        assert_eq!(change.regions & regions::DOCUMENT,0,"motion updates do not republish panels");
    }
    assert_eq!(session.engine.checkpoint(),checkpoint,"previews never enter history");
    let turn = Affine64([0.,1.,-1.,0.,40.,2.]);
    session.preview_object_motion(turn).unwrap();
    let posed = session.engine.document().clone();
    for singular in [Affine64([0.;6]), Affine64([2.,1.,4.,2.,0.,0.]), Affine64([1.,1.,1.,1.,0.,0.])] {
        assert!(session.preview_object_motion(singular).is_err(), "a singular pose is refused");
        assert_live_artwork_eq(session.engine.document(),&posed);
        assert_eq!(session.engine.backend().moving_layer,Some(layer));
    }
    let change = session.commit_object_motion().unwrap();
    assert_ne!(change.regions & regions::DOCUMENT,0);
    assert_eq!(session.engine.backend().moving_layer,None);
    let local = Affine64([1.,0.,0.,1.,-3.,4.]).compose(turn).compose(Affine64([1.,0.,0.,1.,3.,-4.]));
    assert_eq!(session.engine.document().scene().object(first).unwrap().affine,local);
    let second_local = Affine64([1.,0.,0.,1.,8.,-6.]).compose(turn).compose(Affine64([1.,0.,0.,1.,-8.,6.]));
    assert_eq!(session.engine.document().scene().object(second).unwrap().affine,second_local.compose(Affine64([2.,0.,0.,2.,5.,7.])));
    let accepted = session.engine.document().clone();
    session.engine.undo().unwrap();
    assert_live_artwork_eq(session.engine.document(),&before);
    session.engine.redo().unwrap();
    assert_live_artwork_eq(session.engine.document(),&accepted);
    session.begin_object_motion(&[second]).unwrap();
    session.preview_object_motion(Affine64([1.,0.,0.,1.,9.,9.])).unwrap();
    session.cancel_object_motion().unwrap();
    assert_live_artwork_eq(session.engine.document(),&accepted);
    assert_eq!(session.engine.backend().moving_layer,None);
    assert!(session.commit_object_motion().is_err());
}

#[test]
fn unsupported_image_sampling_leaves_document_checkpoint_and_redo_intact() {
    let mut session=session(Platform::Android);
    let (layer,edit)=session.engine.document().create_object_layer_edit("Photo",ImageObject::new(rgba8_source([1,1],|_,_|[17,33,65,255]).into()),None,0).unwrap();session.engine.apply_edit(edit).unwrap();
    let object=session.engine.document().scene().object_handle(layer).unwrap();
    session.begin_object_motion(&[object]).unwrap();session.preview_object_motion(Affine64([1.,0.,0.,1.,12.,9.])).unwrap();session.commit_object_motion().unwrap();
    let accepted=session.engine.document().clone();
    session.engine.undo().unwrap();let before=session.engine.document().clone();let checkpoint=session.engine.checkpoint();
    let history=(session.engine.can_undo(),session.engine.can_redo());assert!(history.1);
    session.engine.backend_mut().image_affine_requests.borrow_mut().clear();session.engine.backend_mut().reject_image_affines=true;
    let delta=Affine64([0.001,0.,0.,0.001,12.,9.]);let view=session.engine.view();
    session.begin_object_motion(&[object]).unwrap();
    assert_eq!(session.preview_object_motion(delta).unwrap_err(),"Image sampling request is unsupported");
    session.cancel_object_motion().unwrap();
    assert_eq!(session.engine.document(),&before);assert_eq!(session.engine.checkpoint(),checkpoint);
    assert_eq!((session.engine.can_undo(),session.engine.can_redo()),history);
    assert_eq!(*session.engine.backend().image_affine_requests.borrow(),vec![(object,delta,view)]);
    session.engine.redo().unwrap();assert_live_artwork_eq(session.engine.document(),&accepted);
    session.engine.undo().unwrap();assert_live_artwork_eq(session.engine.document(),&before);
}
