mod support;
use layer_core::*;
use layer_core::authored::{PaintHandle,SourceTarget};
use layer_render::{BrushStampRequest,CanvasRenderer,DabBatch,DabBatchKind,DabStyle,FramePacket,HostImage,PixelFormat,ViewState};
use layer_render_wgpu::WgpuRasterizer;
use std::{sync::Arc,time::{Duration,Instant}};

const SIZE:[u32;2]=[192;2];
fn packet(doc:&Document)->FramePacket<'_> {
    FramePacket {commit_rasters:true,restore_rasters:&[],time_seconds:0.,
        view:ViewState {width_px:192,height_px:192,document_to_surface:[1.,0.,0.,1.,0.,0.]},
        document_extent:SIZE,scene:doc.scene(),selection_overlays:None,inspect_mask:None,
        dabs:&[],dab_batches:&[],reset_layers:false,composite_all:true,blend_space:BlendSpace::Perceptual}
}
fn tap(brush:&BrushSnapshot)->Vec<layer_render::Dab> {
    let stroke=Stroke::new(StrokeId(1),SourceTarget::Paint(PaintHandle::from_index(1)),StrokeTool::Brush,brush.clone(),vec![StrokePoint {
        position:Point {x:96.,y:96.},pressure:1.,tilt:[std::f32::consts::FRAC_PI_4,0.],twist:0.,elapsed_micros:0,
    }]).unwrap();
    let mut dabs=Vec::new();layer_engine::DabGenerator::generate(&stroke,color::RgbSpace::Srgb,&mut dabs);dabs
}
fn preview(r:&mut WgpuRasterizer,id:u64,request:&BrushStampRequest)->Vec<u8> {
    let deadline=Instant::now()+Duration::from_secs(15);
    while !r.request_brush_stamp(id,request).unwrap() {
        assert!(Instant::now()<deadline,"stamp{id}: pipeline readiness");std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!r.request_brush_stamp(id+10_000,request).unwrap(),"only one stamp may be in flight");
    loop {
        if let Some(image)=r.take_brush_stamp() {
            let image=image.unwrap();assert_eq!((image.request_id,image.width,image.height,image.stride),(id,192,192,192));
            assert_eq!(image.bytes.len(),192*192);assert!(r.take_brush_stamp().is_none());return image.bytes;
        }
        assert!(Instant::now()<deadline,"stamp{id}: asynchronous completion");std::thread::sleep(Duration::from_millis(1));
    }
}
fn reference(r:&mut WgpuRasterizer,doc:&Document,id:u64,style:DabStyle,dabs:&[layer_render::Dab])->Vec<u8> {
    let batch = DabBatch {
        preview_end: None,
        material_update: 0,
        stroke_id: StrokeId(id),
        target: doc.working.target.unwrap(),
        kind: DabBatchKind::Persistent,
        stroke_start:true,stroke_end:true,first_dab:0,dab_count:dabs.len() as u32,style,damage:Rect::from_extent(SIZE)};
    support::submit_settled(r,FramePacket {reset_layers:true,dabs,dab_batches:std::slice::from_ref(&batch),..packet(doc)});
    r.readback_srgb_rgba8().unwrap()
}
fn check(r:&mut WgpuRasterizer,doc:&Document,id:u64,name:&str,request:BrushStampRequest,style:DabStyle,dabs:&[layer_render::Dab]) {
    let before=reference(r,doc,id,style,dabs);
    let stamp=preview(r,id,&request);
    let after=r.readback_srgb_rgba8().unwrap();assert_eq!(after,before,"{name}: preview must not change live artwork");
    let maximum=stamp.iter().zip(before.as_chunks::<4>().0.iter()).map(|(a,p)|a.abs_diff(p[3])).max().unwrap();
    if let Ok(path)=std::env::var("LAYER_TEST_ARTIFACTS") {
        let path=std::path::Path::new(&path);std::fs::create_dir_all(path).unwrap();
        let file=std::fs::File::create(path.join(format!("{name}.png"))).unwrap();let mut encoder=png::Encoder::new(file,192,192);
        encoder.set_color(png::ColorType::Grayscale);encoder.set_depth(png::BitDepth::Eight);encoder.write_header().unwrap().write_image_data(&stamp).unwrap();
        let alpha:Vec<u8>=before.as_chunks::<4>().0.iter().map(|p|p[3]).collect();
        let file=std::fs::File::create(path.join(format!("{name}.reference.png"))).unwrap();let mut encoder=png::Encoder::new(file,192,192);
        encoder.set_color(png::ColorType::Grayscale);encoder.set_depth(png::BitDepth::Eight);encoder.write_header().unwrap().write_image_data(&alpha).unwrap();
    }
    println!("{name}: max_alpha_error={maximum} nonzero={}",stamp.iter().filter(|a|**a>0).count());
    assert!(maximum<=2,"{name}: preview versus completed production tap differs by{maximum}U8");
}
#[test]
fn asynchronous_brush_stamp_matches_completed_production_taps_and_preserves_artwork() {
    let mut doc=support::named_document(&["Ink"],SIZE,BlendSpace::Perceptual);
    let paper=support::named_occurrence(&doc,"Paper");doc.artwork.occurrences.get_mut(paper).unwrap().visible=false;
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).expect("hardware GPU required");
    let bounds=Rect::from_extent(SIZE);let mut id=0;
    for preset in [DefaultBrushPreset::Pencil,DefaultBrushPreset::PointyPencil,DefaultBrushPreset::ShadingPencil,
        DefaultBrushPreset::Charcoal,DefaultBrushPreset::Chalk,DefaultBrushPreset::PastelBlock,DefaultBrushPreset::Crayon] {
        for strength in [0.,0.25,1.] {for hardness in [0.,0.5,1.] {
            let mut brush=default_brush(preset);brush.diameter=64.;brush.flow=strength;brush.opacity=0.7;
            brush.color_rgba_linear=[0.,0.,0.,1.];brush.contact.as_mut().unwrap().dry.as_mut().unwrap().loading=hardness;
            let dabs=tap(&brush);let style=DabStyle::for_brush(&brush,StrokeTool::Brush);id+=1;
            check(&mut r,&doc,id,&format!("{preset:?}-s{strength}-h{hardness}"),BrushStampRequest {style:style.clone(),dabs:dabs.clone(),bounds,shape_strength:None},style,&dabs);
        }}
    }
    for preset in [DefaultBrushPreset::Pencil,DefaultBrushPreset::PointyPencil] {
        let mut brush=default_brush(preset);brush.flow=1.;brush.opacity=0.7;brush.color_rgba_linear=[0.,0.,0.,1.];
        brush.contact.as_mut().unwrap().dry.as_mut().unwrap().loading=1.;
        let dabs=tap(&brush);let style=DabStyle::for_brush(&brush,StrokeTool::Brush);id+=1;
        check(&mut r,&doc,id,&format!("{preset:?}-default-small"),BrushStampRequest {style:style.clone(),dabs:dabs.clone(),bounds,shape_strength:None},style,&dabs);
    }
    for preset in [DefaultBrushPreset::GPen,DefaultBrushPreset::Paintbrush] {
        for strength in [0.25,1.] {
            let mut brush=default_brush(preset);brush.diameter=80.;brush.flow=strength;brush.opacity=0.6;
            brush.color_rgba_linear=[0.,0.,0.,1.];let dabs=tap(&brush);let style=DabStyle::for_brush(&brush,StrokeTool::Brush);id+=1;
            check(&mut r,&doc,id,&format!("{preset:?}-s{strength}"),BrushStampRequest {style:style.clone(),dabs:dabs.clone(),bounds,shape_strength:None},style,&dabs);
        }
    }
    let mut brush=default_brush(DefaultBrushPreset::GPen);brush.diameter=80.;brush.hardness=0.35;brush.mappings=Arc::from([]);
    brush.grain=None;brush.contact=None;brush.color_rgba_linear=[0.,0.,0.,1.];brush.opacity=1.;
    let mask=AssetId::from("test:stamp-hole");let bytes:Vec<u8>=(0..64).flat_map(|y|(0..64).map(move|x|if (24..40).contains(&x)&&(24..40).contains(&y) {0}else{255})).collect();
    r.prepare_asset(&mask,HostImage {width:64,height:64,stride:64,format:PixelFormat::R8Unorm,bytes:&bytes}).unwrap();
    brush.tip=BrushTip::Mask(mask);brush.flow=0.4;let dabs=tap(&brush);let style=DabStyle::for_brush(&brush,StrokeTool::Brush);id+=1;
    check(&mut r,&doc,id,"mask-hole",BrushStampRequest {style:style.clone(),dabs:dabs.clone(),bounds,shape_strength:None},style.clone(),&dabs);
    let hole=preview(&mut r,id+1,&BrushStampRequest {style,dabs,bounds,shape_strength:None});id+=1;
    assert_eq!(hole[96*192+96],0);assert!(hole.iter().any(|a|*a>0));
    brush.tip=BrushTip::AnalyticEllipse;brush.flow=1.;let shape_contacts=tap(&brush);
    for execution in [BrushExecution::Dry,BrushExecution::Smudge,BrushExecution::Liquify] {for strength in [0.,0.25,1.] {
        let mut source=brush.clone();source.flow=strength;let dabs=tap(&source);let style=DabStyle::for_brush(&source,StrokeTool::Brush);
        let mut shape_style=DabStyle::for_brush(&brush,StrokeTool::Eraser);shape_style.execution=execution;id+=1;
        check(&mut r,&doc,id,&format!("shape-{execution:?}-s{strength}"),BrushStampRequest {style:shape_style,dabs:shape_contacts.clone(),bounds,shape_strength:Some(strength)},style,&dabs);
    }}
    let style=DabStyle::for_brush(&brush,StrokeTool::Brush);let empty=preview(&mut r,id+1,&BrushStampRequest {style,dabs:Vec::new(),bounds,shape_strength:None});
    assert!(empty.iter().all(|a|*a==0),"zero input clears every output byte after a populated stamp");
}
