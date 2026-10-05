use super::*;
use crate::test_support::{packet,float_pixels,max_error};
use layer_core::{Document,EffectInstance,EffectValue,authored::*};
use layer_core::color::{RgbSpace,source::*};

#[test]
fn cold_late_object_window_preserves_accepted_pixels_and_pose_until_complete() {
    let extent=[1024,256];
    let mut artwork=Artwork::new(extent).unwrap();
    let mut builder=SourceBuilder::new([1024;2],SourceInterpretation {channels:SourceChannels::Rgba,
        depth:layer_core::color::SampleDepth::U8,profile:Default::default(),profile_assumed:false},8<<20).unwrap();
    for _ in 0..1024 {builder.push_row(&[0,0,255,255].repeat(1024)).unwrap();}
    let mut object=ImageObject::new(layer_core::authored::Image::new(Arc::new(builder.finish().unwrap())),"Cold image");
    object.affine=Affine64([0.25,0.,0.,0.25,0.,0.]);object.interpolation=ImageInterpolation::Nearest;
    let child=artwork.objects.insert(PortableId::random(),object).unwrap();
    let collection=artwork.object_layers.insert(PortableId::random(),ObjectLayer {children:vec![child]}).unwrap();
    let mut owner=Occurrence::new(OccurrenceContent::Objects(collection),"Cold images");owner.visible=false;
    let owner=artwork.occurrences.insert(PortableId::random(),owner).unwrap();
    let mut fill=EffectInstance::new(layer_core::bundled_effect_catalog().get("solid_color").unwrap().program());
    fill.set("color",EffectValue::Color(layer_core::color::RgbColor::from_linear(RgbSpace::Srgb,[1.,0.,0.,1.]).unwrap())).unwrap();
    let fill_handle=artwork.effects.insert(PortableId::random(),EffectApplication::new(fill.program,fill.values,extent)).unwrap();
    let fill=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(fill_handle),"Red")).unwrap();
    let mut blur=EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
    blur.set("sigma",EffectValue::Number(2.)).unwrap();
    let blur=artwork.effects.insert(PortableId::random(),EffectApplication::new(blur.program,blur.values,extent)).unwrap();
    let blur=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(blur),"Blur")).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries=vec![blur,owner,fill];
    let mut doc=Document::from_artwork(artwork).unwrap();
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.test.reference=true;r.native_edit.as_mut().unwrap().image_pixel_bytes=Some(8<<20);
    assert!(windows::Plan::new(doc.scene(),extent,8<<20,r.device.limits().max_texture_dimension_2d).unwrap().is_some());
    r.submit(packet(doc.scene(),extent)).unwrap();r.wait_idle().unwrap();
    let expected=float_pixels(&r,r.display_views().unwrap()[0].texture());
    let accepted=r.evaluated_object_revision();let windows=r.metrics.image_window_submissions;
    doc.artwork.occurrences.get_mut(owner).unwrap().visible=true;
    let application=doc.artwork.effects.get_mut(fill_handle).unwrap();
    let mut fill=EffectInstance {program:application.program.clone(),values:application.values.clone()};
    fill.set("color",EffectValue::Color(layer_core::color::RgbColor::from_linear(RgbSpace::Srgb,[0.,1.,0.,1.]).unwrap())).unwrap();
    application.values=fill.values;
    r.submit(packet(doc.scene(),extent)).unwrap();r.wait_idle().unwrap();
    assert!(r.object_deferred);
    assert!(r.has_pending_work(),"native hosts keep scheduled work while a cold image prepares");
    assert!(r.metrics.image_window_submissions>windows+1,"an earlier filter window submitted before the later cold tile");
    assert_eq!(r.evaluated_object_revision(),accepted);
    assert!(max_error(&expected,&float_pixels(&r,r.display_views().unwrap()[0].texture()))<3e-5);
    assert!(r.metrics.composite_storage_bytes>=r.display_backup.as_ref().unwrap().bytes);
    for _ in 0..2000 {
        if !r.object_deferred {break;}
        std::thread::sleep(std::time::Duration::from_millis(1));
        r.submit(FramePacket {composite_all:false,reset_layers:false,..packet(doc.scene(),extent)}).unwrap();r.wait_idle().unwrap();
        if r.object_deferred {
            assert_eq!(r.evaluated_object_revision(),accepted);
            assert!(max_error(&expected,&float_pixels(&r,r.display_views().unwrap()[0].texture()))<3e-5);
        }
    }
    assert!(!r.object_deferred,"bounded worker preparation and private sampling complete");
    assert!(r.display_backup.is_none());
    assert_eq!(r.evaluated_object_revision(),Some(doc.scene().revision()));
    assert!(max_error(&expected,&float_pixels(&r,r.display_views().unwrap()[0].texture()))>0.5);
}
