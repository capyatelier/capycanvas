use super::*;
use crate::test_support::{dab_batch, StoredTiles, tile_read_cache};
use layer_core::{MemoryBudget, MemoryFacts, DeviceMemoryFacts, DefaultBrushPreset};

#[test]
fn a_cold_navigator_jump_presents_each_photo_stroke_frame() {
    let extent=[4216,3016];let viewport=[2560,1600];
    let mut doc=layer_ui::new_drawing(extent[0],extent[1],&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let target=doc.working.target.unwrap();let paint=doc.working.occurrence.unwrap();
    let photo=AuthoredImage::new(rgba8_source(extent,|x,y|[(x%256) as u8,(y%256) as u8,128,255]));
    let (_,edit)=doc.create_object_layer_edit("Photo",ImageObject::new(photo),None,0).unwrap();doc.apply(edit).unwrap();
    let stack=doc.composition().result;
    let entries=&mut doc.artwork.stacks.get_mut(stack).unwrap().entries;
    entries.retain(|entry|*entry!=paint);entries.insert(0,paint);
    let doc=layer_core::Document::from_artwork(doc.artwork).unwrap();
    let _store=StoredTiles::new(&layer_core::Editor::new(doc.clone()).retained_tiles(),tile_read_cache());
    let budget=MemoryBudget::with_facts(MemoryFacts {host_default_bytes:Some(1200<<20),override_bytes:Some(1200<<20),..Default::default()});
    let mut r=crate::memory::memory_tests::renderer_in_budget(doc.composition().color,
        DeviceMemoryFacts {unified:true,..Default::default()},budget).unwrap();
    let fit=layer_render::ViewState {width_px:viewport[0],height_px:viewport[1],document_to_surface:[0.40132827,0.,0.,0.40132827,234.25,256.422]};
    crate::test_support::submit_settled(&mut r,FramePacket {view:fit,blend_space:doc.composition().blend,..packet(doc.scene(),extent)});
    let mut presenter=crate::test_support::float_presenter(&r);
    presenter.set_overviews(&r,&[crate::OverviewPlacement {bounds:[2100.,200.,320.,230.],clip:None,
        work_area:[[0.;2];4],outline_linear:[1.;3],scale:1.,opacity:1.}]);
    let output=create_color_target(&r.device,viewport,"cold Navigator stroke");
    presenter.present(&mut r,&output.1,fit,[0.;4]).unwrap();
    r.wait_idle().unwrap();
    crate::test_support::settle_decodes(&mut r);
    r.moving_decode=None;r.source_tiles.borrow_mut().invalidate();
    let samples=r.device.source_samples.clone();let limit=samples.stats().limit_bytes;
    samples.set_limit(0);samples.trim_unused(usize::MAX);samples.set_limit(limit);
    let view=layer_render::ViewState {document_to_surface:[1.,0.,0.,1.,447.85,409.225],..fit};
    let style=crate::layer_tests::preset_style(DefaultBrushPreset::GPen);
    let mut outcomes=Vec::new();
    for frame in 0..96 {
        let mut dab=crate::layer_tests::dab([1.,0.,1.,1.]);
        let time=frame as f32/12.;
        dab.center=layer_core::Point {x:632.4+464.4*(time*3.2).sin(),y:452.4+301.86*(time*4.7+0.31).sin()};
        dab.radii=[512.;2];dab.contact=[1.,0.,0.,0.];
        let mut batch=dab_batch(target,style.clone(),dab.bounds());
        batch.stroke_start=frame==0;batch.stroke_end=frame==95;
        let before=r.metrics.submissions;
        let result=crate::test_support::submit_ready(&mut r,FramePacket {view,composite_all:false,
            blend_space:doc.composition().blend,dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..packet(doc.scene(),extent)});
        r.wait_idle().unwrap();
        let needs_present=presenter.needs_present(&r,view,[0.;4]);
        outcomes.push((frame,result.as_ref().map_err(ToString::to_string).map(|_|r.metrics.submissions>before),
            r.has_presentable_frame(),needs_present,crate::scene::scale::tests::publication_pending(&r),r.composite_revision,r.artwork_revision));
        if needs_present {presenter.present(&mut r,&output.1,view,[0.;4]).unwrap();}
    }
    let queued=outcomes.iter().filter(|row|row.1==Ok(true)).count();
    let presented=outcomes.iter().filter(|row|row.3).count();
    assert_eq!(queued,outcomes.len(),"each dab must reach queue submission: {outcomes:?}");
    assert!(presented>=90,"cold source preparation must not hide stroke frames: {outcomes:?}");
}
