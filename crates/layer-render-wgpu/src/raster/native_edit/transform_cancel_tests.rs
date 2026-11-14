use super::*;
use layer_core::{Affine,Figure,FigurePaint,FigureShape,ImageTransform,Point,RasterOperation,RasterOperationKind,Selection};

fn state(r:&WgpuRasterizer,id:SourceTarget)->(u64,u64,u64,Vec<[u32;2]>) {
    (r.memory.raster_grant(),r.raster_tiers.raster.used(),r.paint_memory_bytes(),
        r.paint_layers.iter().filter(|layer|layer.id==id).flat_map(|layer|layer.pages.iter().map(|page|page.coordinate)).collect())
}

struct Pixels {plan:display_mips::Plan,size:[u32;2],bytes:Vec<u8>}

fn capture(live:&mut CanvasEngine<WgpuRasterizer>)->Pixels {
    let snapshot=live.document().snapshot();let view=live.view();
    let blend_space=live.document().composition().blend;
    let extent=live.document().composition().size;
    let r=live.backend_mut();
    r.shader_idle(true,true);
    crate::test_support::submit_settled(r,FramePacket {view,blend_space,composite_all:false,
        ..crate::test_support::packet(snapshot.view(),extent)});
    let cache=r.scale_display.as_ref().unwrap();let texture=cache.texture();
    assert_eq!(texture.format(),wgpu::TextureFormat::Rgba32Float);
    let pixels=Pixels {plan:cache.plan,size:[texture.width(),texture.height()],bytes:crate::layer_tests::page_bytes(r,texture)};
    r.after_present();crate::test_support::complete(r);
    pixels
}

fn patch(pixels:&Pixels,center:[u32;2])->Vec<[f32;4]> {
    (center[1]-8..center[1]+8).flat_map(|y|(center[0]-8..center[0]+8)
        .map(move|x| {
            assert!((pixels.plan.bounds.min_x()..pixels.plan.bounds.max_x()).contains(&x)
                && (pixels.plan.bounds.min_y()..pixels.plan.bounds.max_y()).contains(&y));
            let x=(x-pixels.plan.bounds.min_x())>>pixels.plan.level;
            let y=(y-pixels.plan.bounds.min_y())>>pixels.plan.level;
            let offset=((y*pixels.size[0]+x)*16) as usize;
            std::array::from_fn(|channel|f32::from_le_bytes(pixels.bytes[offset+channel*4..offset+channel*4+4].try_into().unwrap()))
        })).collect()
}

#[test]
fn cancelling_a_persistent_rectangle_transform_preserves_unselected_ink_across_raster_grants() {
    let extent=[2133,1600];
    let mut document=Document::new(PortableId::random(),extent[0],extent[1],
        layer_core::DocumentNames {paint:"Current ink".into(),paper:"Paper".into()});
    let composition=document.artwork.compositions.get_mut(document.artwork.root).unwrap();
    composition.color=DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U8};
    composition.blend=layer_core::BlendSpace::Perceptual;
    composition.resolution=Some(layer_core::ImageResolution::ppi(300));
    document.set_paper_relief(0.);
    let id=target(&document);
    let r=crate::memory::memory_tests::renderer(document.composition().color,
        layer_core::DeviceMemoryFacts {budget_bytes:Some(4<<30),..Default::default()}).unwrap();
    let (_,mut live)=engine_on(document,r);
    let zoom=0.77849996;
    let translation=[664.229797,204.200012];
    live.set_view(ViewState {width_px:2560,height_px:1528,
        document_to_surface:[zoom,0.,0.,zoom,translation[0],translation[1]]},ViewTransform {
        revision:1,surface_to_document:[1./zoom,0.,0.,1./zoom,-translation[0]/zoom,-translation[1]/zoom],
    });
    flush(&mut live);
    live.backend_mut().after_present();crate::test_support::complete(live.backend());
    live.append_raster_operation(id,RasterOperation {placement:Affine::IDENTITY,coverage:reveal_all(extent,[0;2]),
        kind:RasterOperationKind::Figure(Figure {shape:FigureShape::Rectangle,paint:FigurePaint::Fill,
            start:Point {x:911.,y:697.},end:Point {x:1219.,y:902.},width:4.,
            colors:[[RgbSpace::Srgb.decode(46./255.) as f32,RgbSpace::Srgb.decode(46./255.) as f32,RgbSpace::Srgb.decode(46./255.) as f32,1.];2],alpha_locked:false,erase:false})}).unwrap();
    flush(&mut live);crate::test_support::complete(live.backend());
    let saved=backing(&paint(live.document()).raster);
    assert!(!saved.is_empty(),"the rectangle must have real persistent raster backing");
    let _=capture(&mut live);
    let selection=Selection::polygon(vec![Point {x:937.,y:723.},Point {x:1040.,y:723.},
        Point {x:1040.,y:877.},Point {x:937.,y:877.}]).unwrap();
    let mut working=live.document().working.clone();working.selection=Some(selection.clone());
    live.apply_edit(layer_core::Edit::Working(working)).unwrap();flush(&mut live);
    let baseline=capture(&mut live);
    let before=state(live.backend(),id);
    let selected=patch(&baseline,[989,832]);
    let destination=patch(&baseline,[1374,832]);
    let unselected=patch(&baseline,[1156,832]);
    assert!(selected.iter().chain(&unselected).all(|pixel|pixel[..3].iter().all(|channel|*channel<0.5) && pixel[3]==1.),
        "selected and unselected rectangle samples must contain ink: {before:?}");
    assert!(destination.iter().all(|pixel|pixel[3]==0. || pixel.iter().all(|channel|*channel>0.99)),"destination must begin empty: {before:?}");
    let artwork=live.document().artwork.clone();
    let working=live.document().working.clone();
    let revision=live.document().scene().revision();
    for (step,moving) in std::iter::once((0,false)).chain((1..=12).map(|step|(step,true))).chain([(12,false)]) {
        live.set_transform_preview(Some(layer_render::TransformPreview {transaction:1,moving,target:id,
            selection:Some(selection.clone()),transform:ImageTransform::affine(Affine::translation(Point {x:25.*step as f32/zoom,y:0.}))})).unwrap();
        flush(&mut live);
        live.backend_mut().after_present();
    }
    let preview_state=state(live.backend(),id);
    let preview=capture(&mut live);
    assert!(patch(&preview,[1156,832])==unselected,"move preview lost unselected ink: before={before:?}, preview={preview_state:?}");
    assert!(patch(&preview,[989,832])==destination,"move must clear selected source ink: before={before:?}, preview={preview_state:?}");
    assert!(patch(&preview,[1374,832])==selected,"move must fill its destination: before={before:?}, preview={preview_state:?}");
    assert!(live.document().artwork==artwork && live.document().working==working,"preview changed authored state");
    assert!(backing(&paint(live.document()).raster)==saved,"preview changed persistent raster backing");
    live.set_transform_preview(None).unwrap();flush(&mut live);
    let cancelled_state=state(live.backend(),id);
    let cancelled=capture(&mut live);
    assert!(patch(&cancelled,[989,832])==selected,"Cancel must restore selected ink: {cancelled_state:?}");
    assert!(patch(&cancelled,[1374,832])==destination,"Cancel must clear the moved destination: {cancelled_state:?}");
    assert!(patch(&cancelled,[1156,832])==unselected,"Cancel must preserve unselected ink: {cancelled_state:?}");
    assert_eq!(cancelled.plan,baseline.plan);assert_eq!(cancelled.size,baseline.size);
    let first=baseline.bytes.iter().zip(&cancelled.bytes).position(|(before,after)|before!=after);
    assert!(first.is_none(),"Cancel must restore all artwork pixels; first={first:?}, before={before:?}, preview={preview_state:?}, cancelled={cancelled_state:?}");
    assert_eq!(live.document().scene().revision(),revision);
    assert!(live.document().artwork==artwork && live.document().working==working,"Cancel changed authored state");
    assert!(backing(&paint(live.document()).raster)==saved,"Cancel changed persistent raster backing");
}
