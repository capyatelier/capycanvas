use super::*;
use crate::layer_tests::placement::{paint_document, reveal_all, set_source, target};
use crate::test_support::{flush_engine, packet, submit_settled, view};
use layer_core::{Affine, Point, RasterOperation, RasterOperationKind, Selection};
use layer_engine::{CanvasEngine, ViewTransform, input_queue};
use layer_render::{FillPreview, FillReveal};

#[test]
fn fill_reveal_damage_covers_changed_pixels_in_every_native_orientation() {
    let view=layer_render::ViewState {width_px:64,height_px:48,document_to_surface:[1.,0.,0.,1.,0.,0.]};
    for turns in 0..4 {for center in [[32.5,24.5],[0.2,13.7],[63.8,47.9]] {for (from,to) in [(0.,8.),(8.,24.),(24.,128.),(128.,24.),(8.,8.)] {
        let old=FillReveal {transaction:1,center,radius:from};let new=FillReveal {radius:to,..old};
        let damage=crate::present_damage::fill_reveal(Some(old),Some(new),view,turns).unwrap();
        let [width,height]=if turns%2==0 {[64,48]}else{[48,64]};
        for y in 0..height {for x in 0..width {
            let [px,py]=[x as f32+0.5,y as f32+0.5];
            let [sx,sy]=match turns {1=>[py,48.-px],2=>[64.-px,48.-py],3=>[64.-py,px],_=>[px,py]};
            let distance=(sx-center[0]).hypot(sy-center[1]);
            if (distance<from)!=(distance<to) {assert!(damage.iter().any(|area|x>=area.min_x()&&x<area.max_x()&&y>=area.min_y()&&y<area.max_y()),"turns {turns}, {from}->{to}, {center:?}, pixel {x},{y}");}
        }}
    }}}
}

#[test]
fn fill_reveal_presents_before_after_and_circles_without_raster_work() {
    let extent=[64;2];
    let mut document=paint_document(extent,"Fill reveal");
    set_source(&mut document,layer_core::color::source::rgba8_source(extent,|_,_|[0,0,255,255]));
    let target=target(&document);
    let renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let mut engine=CanvasEngine::new(renderer,document,input_queue(8).1,view(extent),ViewTransform::IDENTITY).unwrap();
    flush_engine(&mut engine);
    let snapshot=engine.document().clone();
    submit_settled(engine.backend_mut(),packet(snapshot.scene(),extent));
    let authored=engine.document().artwork.clone();
    let mut presenter=ViewportPresenter::for_renderer(engine.backend(),wgpu::TextureFormat::Rgba8UnormSrgb);
    presenter.set_target_retention(true);
    pollster::block_on(presenter.prepare(engine.backend())).unwrap();
    let (texture,surface)=crate::create_target(engine.backend().device(),extent,wgpu::TextureFormat::Rgba8UnormSrgb,"fill reveal viewport");
    let capture=|presenter:&mut ViewportPresenter,engine:&CanvasEngine<WgpuRasterizer>| {
        presenter.present(engine.backend(),&surface,view(extent),[1.;4]).unwrap();
        crate::layer_tests::page_bytes(engine.backend(),&texture)
    };
    let work=|engine:&CanvasEngine<WgpuRasterizer>| {
        let renderer=engine.backend();let metrics=renderer.metrics();
        (metrics.command_passes,metrics.dabs,metrics.paint_pages,metrics.preview_pages,
            metrics.raster_candidate_pixels,metrics.native_restore_submissions,renderer.artwork_revision,renderer.composite_revision)
    };
    let before=capture(&mut presenter,&engine);
    assert!(before.chunks_exact(4).all(|pixel|pixel==[0,0,255,255]));
    let mut coverage=reveal_all(extent,[0;2]);coverage.source.default_coverage=0.;
    coverage.selection=Some(Selection::polygon(vec![Point{x:8.,y:8.},Point{x:56.,y:8.},Point{x:56.,y:56.},Point{x:8.,y:56.}]).unwrap());
    let operation=RasterOperation {placement:Affine::IDENTITY,coverage,
        kind:RasterOperationKind::Fill {color:[1.,0.,0.,1.],texture:Default::default(),alpha_locked:false,edges: None, underfill:None}};
    let reveal=FillReveal {transaction:7,center:[32.5,32.5],radius:-1.};
    engine.set_fill_reveal(Some(reveal));engine.render_frame().unwrap();
    assert_eq!(capture(&mut presenter,&engine),before);
    assert_eq!(presenter.fill_before.as_ref().map(|(id,_)|*id),Some(7));
    engine.set_fill_preview(Some(FillPreview {transaction:7,revision:0,target,operation:operation.clone()}));
    flush_engine(&mut engine);
    engine.set_fill_reveal(Some(FillReveal {radius:128.,..reveal}));engine.render_frame().unwrap();
    let after=capture(&mut presenter,&engine);
    assert_eq!(&after[(32*64+32)*4..][..4],&[255,0,0,255]);
    assert_eq!(&before[(32*64+32)*4..][..4],&[0,0,255,255]);
    let raster_work=work(&engine);
    for radius in [0.,8.,24.,128.] {
        engine.set_fill_reveal(Some(FillReveal {radius,..reveal}));engine.render_frame().unwrap();
        assert!(presenter.needs_present(engine.backend(),view(extent),[1.;4]));
        let shown=capture(&mut presenter,&engine);
        if radius==8. {assert!(presenter.presented_area<64*64,"the moving reveal preserves static canvas pixels");}
        for y in 0..64 {for x in 0..64 {
            let distance=((x as f32+0.5-32.5).powi(2)+(y as f32+0.5-32.5).powi(2)).sqrt();
            let at=(y*64+x)*4;let expected=if distance<radius {&after}else{&before};
            assert_eq!(&shown[at..at+4],&expected[at..at+4],"radius {radius} at {x},{y}");
        }}
        assert_eq!(work(&engine),raster_work);
        assert_eq!(engine.document().artwork,authored);assert!(!engine.can_undo());
    }
    engine.set_fill_reveal(None);engine.render_frame().unwrap();
    assert_eq!(capture(&mut presenter,&engine),after);
    assert!(presenter.fill_before.is_none());assert_eq!(work(&engine),raster_work);
    engine.set_fill_preview(None);flush_engine(&mut engine);
    assert_eq!(capture(&mut presenter,&engine),before);
    engine.set_fill_preview(Some(FillPreview {transaction:8,revision:0,target,operation}));flush_engine(&mut engine);
    assert_eq!(capture(&mut presenter,&engine),after);
    assert!(presenter.fill_before.is_none());
    engine.set_fill_preview(None);flush_engine(&mut engine);
    assert_eq!(capture(&mut presenter,&engine),before);
    assert_eq!(engine.document().artwork,authored);assert!(!engine.can_undo());
}
