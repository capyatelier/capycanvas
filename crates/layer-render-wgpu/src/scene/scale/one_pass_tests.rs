use super::*;

fn two_layers()->layer_core::Document {
    let mut document=blank_document([512,256]);
    let layer=paint_occurrence(&mut document,"Wash",Some(rgba8_source([512,256],|x,y|[(x/2) as u8,90,(y) as u8,140]).into()));
    insert_occurrence(&mut document,layer,1);document
}
fn store_matches_window(harness:&Harness,label:&str,required:bool) {
    let r=harness.engine.backend();let cache=r.scale_display.as_ref().unwrap();let plan=cache.plan;
    let window=pixels(r,cache.texture());let side=1<<plan.level;
    for coordinate in pages::coordinates(plan.bounds,plan.level) {
        let region=pages::bounds(coordinate,plan.level,plan.extent).intersect(plan.bounds);
        let key=cache.page_key(cache.graph.root.clone().unwrap(),region);
        let image=cache.pages.lock().unwrap().get(&key,region,pages::Accuracy::Display,cache.required_input_level(),Class::Visible);
        assert!(image.is_some() || !required,"{label}: cell {coordinate:?} is a Store page under its current key");
        let Some(image)=image else {continue;};
        let page=pixels(r,&image.texture);
        let [x,y,width,height]=paint_transform::texel_rect(region.window_local(plan.bounds),side);
        let [px,py,_,_]=paint_transform::texel_rect(region.window_local(image.plan.bounds),side);
        for row in 0..height {
            let from=((py+row)*image.plan.size[0]+px) as usize;let to=((y+row)*plan.size[0]+x) as usize;
            close(&page[from..from+width as usize],&window[to..to+width as usize]);
        }
        let _=label;
    }
}

fn same_picture(actual:&mut Harness,reference:&mut Harness) {
    close(&actual.present(),&reference.present());
    let [a,b]=[&*actual,&*reference].map(|harness|harness.engine.backend());
    let [ac,bc]=[a,b].map(|r|r.scale_display.as_ref().unwrap());assert_eq!(ac.plan,bc.plan);
    close(&pixels(a,ac.texture()),&pixels(b,bc.texture()));
}

#[test]
fn after_contact_ends_every_cell_is_a_current_store_page_equal_to_the_window() {
    let mut actual=Harness::new(two_layers());actual.settle();
    actual.gpen(1,96.,[1.,0.,0.,1.]);actual.settle();store_matches_window(&actual,"stroke",true);
    assert!(actual.engine.undo().unwrap());actual.settle();store_matches_window(&actual,"undo",false);
    assert!(actual.engine.redo().unwrap());actual.settle();store_matches_window(&actual,"redo",false);
    let mut reference=Harness::new(actual.engine.document().clone());reference.settle();
    same_picture(&mut actual,&mut reference);
}

#[test]
fn a_zoom_during_contact_replans_and_both_levels_match_a_fresh_renderer_after_the_gesture() {
    let base=two_layers();let extent=base.composition().size;let target=base.working.target.unwrap();
    let frame=FramePacket {composite_all:false,..crate::test_support::packet(base.scene(),extent)};
    let zoomed=FramePacket {view:layer_render::ViewState {width_px:extent[0]/4,height_px:extent[1]/4,document_to_surface:[0.25,0.,0.,0.25,0.,0.]},..frame};
    let current=|r:&WgpuRasterizer|r.scale_display.as_ref().unwrap().pages.lock().unwrap().retained_pages().into_iter()
        .filter(|page|page.key.source.is_none() && page.key.level==0 && !page.current.is_empty()).count();
    let picture=|zoom_in_contact:bool| {
        let mut r=WgpuRasterizer::new_native_headless(base.composition().color).unwrap();
        crate::test_support::submit_settled(&mut r,frame);
        let mut dab=crate::tests::test_dab([112.,96.],[0.9,0.02,0.1,1.],1.);dab.radii=[20.;2];
        for (step,x) in [112.,200.,300.].into_iter().enumerate() {
            dab.center.x=x;
            let batch=layer_render::DabBatch {stroke_start:step==0,stroke_end:false,..crate::test_support::dab_batch(target,
                crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen),dab.bounds())};
            crate::test_support::submit_ready(&mut r,FramePacket {dabs:&[dab],dab_batches:&[batch],time_seconds:0.01*(step+1) as f32,..frame}).unwrap();
        }
        assert!(r.scale_display.as_ref().unwrap().has_direct_cells(),"contact composes its cells straight into the window");
        if !zoom_in_contact {crate::test_support::submit_ready(&mut r,frame).unwrap();}
        let before=current(&r);
        crate::test_support::submit_ready(&mut r,zoomed).unwrap();
        assert!(r.scale_display.as_ref().unwrap().plan.level>0,"the zoom replans the view");
        if zoom_in_contact {assert!(current(&r)>before,"the zoom writes the contact's level-0 cells back into the Store: {before} -> {}",current(&r));}
        crate::test_support::submit_settled(&mut r,zoomed);let far=crate::scene::scale::tests::display_pixels(&r);
        crate::test_support::submit_settled(&mut r,frame);let near=crate::scene::scale::tests::display_pixels(&r);
        (far,near)
    };
    let [(far,near),(reference_far,reference_near)]=[true,false].map(picture);
    for (label,actual,expected) in [("zoomed",far,reference_far),("zoomed back",near,reference_near)] {
        let error=actual.iter().zip(&expected).flat_map(|(a,b)|a.iter().zip(b).map(|(a,b)|(a-b).abs())).fold(0f32,f32::max);
        assert!(error<5e-6,"{label}: a zoom during contact presents the picture of a zoom after contact: {error}");
    }
}

#[test]
fn zooming_back_to_fit_presents_the_held_revision_for_cells_it_has_not_composed() {
    let extent=[2048,1024];
    let mut document=blank_document(extent);
    let layer=paint_occurrence(&mut document,"Wash",Some(rgba8_source(extent,|x,y|[(x/8) as u8,90,(y/4) as u8,255]).into()));
    insert_occurrence(&mut document,layer,1);
    let mut harness=Harness::new(document);
    harness.frame([0.25,0.,0.,0.25,0.,0.]);harness.settle();let fit=harness.present();
    harness.frame([1.,0.,0.,1.,-512.,-256.]);harness.settle();
    evict_composite_pages(harness.engine.backend().scale_display.as_ref().unwrap(),&[PixelRect::full(extent)]);
    harness.frame([0.25,0.,0.,0.25,0.,0.]);
    for step in 0..4 {
        harness.step();let shown=harness.present();
        let error=shown.iter().zip(&fit).flat_map(|(a,b)|a.iter().zip(b).map(|(a,b)|(a-b).abs())).fold(0f32,f32::max);
        assert!(shown.iter().all(|pixel|pixel[3]>0.99),"frame {step} after zooming back to Fit shows no surround or blank cell");
        assert!(error<0.05,"frame {step} after zooming back to Fit shows the held revision or the composed picture: {error}");
    }
    harness.settle();close(&harness.present(),&fit);
}
