use crate::{WgpuRasterizer, snapshot::SnapshotGpu};
use crate::tests::native_effects::{empty_document, insert_source, insert_effect, refresh};
use layer_core::{SceneScope, Document, EffectInstance, LayerBlend,
    color::{DocumentColor, RgbSpace, SampleDepth}, authored::{Occurrence, OccurrenceContent, PortableId, Stack},
    raster::{RasterData, RasterPlane, RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey}};
use std::{collections::BTreeMap, sync::Arc};

const EXTENT: [u32;2] = [32,24];
const FIXTURE: &[u8] = include_bytes!("../../layer-core/src/package/codec/fixtures/authored-filters.capy");

fn base(depth: SampleDepth, reverse: bool) -> Document {
    let mut document=empty_document(EXTENT,DocumentColor {space:RgbSpace::Srgb,depth});
    let source=crate::test_support::depth_source(EXTENT,depth,RgbSpace::Srgb,1<<20,|x,y| {
        let (x,y)=if reverse {(EXTENT[0]-1-x,EXTENT[1]-1-y)} else {(x,y)};
        let u=x as f32/(EXTENT[0]-1) as f32;let v=y as f32/(EXTENT[1]-1) as f32;
        let alpha=if x==0 || y==0 {0.} else if (x+y)%7==0 {0.25} else {0.85};
        let rgb=[0.03+0.85*u,0.07+0.7*v,0.04+0.8*((x*13+y*7)%31) as f32/30.];
        let gain=if depth.is_float() && x>22 && y<12 {8.} else {1.};
        [rgb[0]*gain,rgb[1]*gain,rgb[2]*gain,alpha]
    });
    insert_source(&mut document,"Reference source",source);document
}
fn pixels(gpu: &SnapshotGpu, document: &Document, sdr: bool) -> Vec<[f32;4]> {
    let mut reader=gpu.capture_scene(document.snapshot(),SceneScope::All,Default::default()).unwrap();
    if sdr {reader.preview_document(EXTENT,RgbSpace::Srgb).unwrap().pixels}
    else {reader.read_region([0,0,EXTENT[0],EXTENT[1]]).unwrap()}
}
fn filtered(mut document: Document, effect: EffectInstance, phase: Option<f32>) -> Document {
    let h=insert_effect(&mut document,effect);
    let root=document.composition().result;
    let entries=&mut document.artwork.stacks.get_mut(root).unwrap().entries;
    entries.pop();entries.insert(0,h);
    if let Some(phase)=phase {
        let OccurrenceContent::Effect(effect)=document.artwork.occurrences.get(h).unwrap().content else {unreachable!()};
        document.artwork.outputs.get_mut(document.artwork.default_output).unwrap().context.phases=vec![(effect,phase)].into();
    }
    refresh(&mut document);document
}
fn cases(saved: &Document) -> BTreeMap<String,(Document,bool)> {
    let mut cases=BTreeMap::new();
    let context=&saved.artwork.outputs.get(saved.artwork.default_output).unwrap().context;
    let mut filters=BTreeMap::new();
    for (handle,_,application) in saved.artwork.effects.iter() {
        let program=application.program.clone();
        let phase=context.phases.iter().find(|(effect,_)|*effect==handle).map(|(_,phase)|*phase);
        filters.entry(program.id.to_string()).or_insert((EffectInstance {program,values:application.values.clone()},phase));
    }
    assert_eq!(filters.len(),52);
    for (name,(effect,phase)) in &filters {
        for depth in [SampleDepth::U8,SampleDepth::F32] {
            cases.insert(format!("filter/{name}/{depth:?}"),(filtered(base(depth,false),effect.clone(),*phase),false));
        }
    }
    let blends=[LayerBlend::Normal,LayerBlend::Multiply,LayerBlend::Screen,LayerBlend::Add,LayerBlend::Overlay,
        LayerBlend::SoftLight,LayerBlend::Color,LayerBlend::Darken,LayerBlend::Lighten,LayerBlend::ColorBurn,
        LayerBlend::LinearBurn,LayerBlend::ColorDodge,LayerBlend::HardLight,LayerBlend::VividLight,LayerBlend::LinearLight,
        LayerBlend::PinLight,LayerBlend::HardMix,LayerBlend::Difference,LayerBlend::Exclusion,LayerBlend::Subtract,
        LayerBlend::Divide,LayerBlend::Hue,LayerBlend::Saturation,LayerBlend::Luminosity,LayerBlend::PassThrough];
    for blend in blends {
        let mut document=base(SampleDepth::F32,false);
        let source=base(SampleDepth::F32,true).artwork.paint.iter().next().unwrap().2.base.as_ref().unwrap().image.storage().clone();
        let bottom=insert_source(&mut document,"Backdrop",source);
        let root=document.composition().result;
        let top=document.artwork.stacks.get(root).unwrap().entries[0];
        let child=document.artwork.stacks.insert(PortableId::random(),Stack {entries:vec![top]}).unwrap();
        let mut group=Occurrence::new(OccurrenceContent::Stack(child),"Blended group");group.blend=blend;group.opacity=0.63;
        let group=document.artwork.occurrences.insert(PortableId::random(),group).unwrap();
        document.artwork.stacks.get_mut(root).unwrap().entries=vec![group,bottom];
        refresh(&mut document);cases.insert(format!("blend/{blend:?}"),(document,false));
    }
    let mut watercolor=base(SampleDepth::U8,false);
    let material=saved.artwork.paint.iter().find_map(|(_,_,p)|p.raster.try_data()?.ok()?.watercolor).unwrap();
    let color=watercolor.composition().color;
    let mut pigment=vec![0u8;256*256*4];let mut wetness=vec![0u8;256*256];
    for y in 0..EXTENT[1] {for x in 0..EXTENT[0] {
        let distance=(x as f32-16.).hypot((y as f32-12.)*1.3);
        if distance<10. {let i=(y*256+x) as usize;let a=((10.-distance).min(1.)*220.) as u8;
            pigment[i*4..i*4+4].copy_from_slice(&[a/4,a/2,(u16::from(a)*3/4) as u8,a]);wetness[i]=if x<10 {1} else if x<16 {2} else {177};}
    }}
    let raster=RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0;2]},RasterTile::backed(TileBlob::encode(color.paint_descriptor(),&pigment).unwrap())),
        (TileKey {plane:RasterPlane::WatercolorWetness,coordinate:[0;2]},RasterTile::backed(TileBlob::encode(color.coverage_descriptor(),&wetness).unwrap()))].into(),
        watercolor:Some(RasterWatercolor {..material})};
    let h=watercolor.artwork.paint.iter().next().unwrap().0;
    let paint=watercolor.artwork.paint.get_mut(h).unwrap();paint.base=None;paint.raster=RasterRevision::backed(raster);
    refresh(&mut watercolor);cases.insert("watercolor".into(),(watercolor,false));
    let mut sdr=base(SampleDepth::F32,false);
    sdr.artwork.outputs.get_mut(sdr.artwork.default_output).unwrap().sdr=saved.artwork.outputs.get(saved.artwork.default_output).unwrap().sdr;
    refresh(&mut sdr);cases.insert("output/sdr".into(),(sdr,true));
    let grain=filters["film_grain"].0.program.clone();let mut sources=grain.wgsl.sources().unwrap().to_vec();
    sources.push("fn saved_fbm(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(fx_fbm(p*.37,5u,17u),fx_fbm(p*.73,3u,101u),fx_fbm(p*.19,7u,3u),1.);}".into());
    let program=Arc::new(layer_core::EffectProgram {id:"saved_fbm".into(),label:"Saved FBM".into(),wgsl:layer_core::EffectShader::Linked {sources:sources.into()},
        entry:"saved_fbm".into(),time:false,parameters:Default::default(),pages:Default::default(),constraints:Default::default(),..(*grain).clone()});
    cases.insert("procedural/fbm".into(),(filtered(base(SampleDepth::F32,false),EffectInstance::new(program),None),false));
    cases
}

#[test]
fn saved_artwork_render_contracts() {
    let rasterizer=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    assert_ne!(rasterizer.adapter().get_info().device_type,wgpu::DeviceType::Cpu,"Saved render contracts require a hardware GPU");
    eprintln!("Saved render contract GPU: {:?}",rasterizer.adapter().get_info());
    let gpu=rasterizer.snapshot_gpu();
    let backing=layer_core::package::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(FIXTURE))).unwrap();
    let layer_core::package::codec::OpenOutcome::Candidate {artwork,..}=layer_core::package::codec::open(backing,Default::default(),&std::sync::atomic::AtomicBool::new(false)).unwrap() else {panic!("Saved authored values must reopen")};
    let saved=Document::from_artwork(artwork).unwrap();
    let cases=cases(&saved);
    let baseline=include_bytes!("fixtures/authored-renders.rgba32f");
    let index=include_str!("fixtures/authored-renders.tsv");
    assert_eq!(index.lines().count(),cases.len());
    let mut compared=0;
    for line in index.lines() {
        let columns:Vec<_>=line.split_whitespace().collect();assert_eq!(columns.len(),5);
        let name=columns[0];let offset:usize=columns[1].parse().unwrap();let length:usize=columns[2].parse().unwrap();
        let max_abs:f64=columns[3].parse().unwrap();let rms:f64=columns[4].parse().unwrap();
        assert_eq!(offset,compared);compared+=length;
        let (document,sdr)=cases.get(name).unwrap_or_else(||panic!("Missing render case {name}"));
        let output=pixels(&gpu,document,*sdr);
        assert_eq!(output.len(),(EXTENT[0]*EXTENT[1]) as usize);assert_eq!(length,output.len()*16);
        let (mut peak,mut square)=(0f64,0f64);
        for (channel,(actual,expected)) in output.iter().flatten().zip(baseline[offset..offset+length].chunks_exact(4)).enumerate() {
            let expected=f32::from_le_bytes(expected.try_into().unwrap());
            assert!(actual.is_finite() && expected.is_finite(),"{name}: nonfinite channel {channel}");
            let error=f64::from(*actual)-f64::from(expected);peak=peak.max(error.abs());square+=error*error;
        }
        let actual_rms=(square/(output.len()*4) as f64).sqrt();
        assert!(peak<=max_abs && actual_rms<=rms,"{name}: linear-float drift: maximum {peak} (limit {max_abs}), RMS {actual_rms} (limit {rms}). Preserve the released baseline; fix the regression or version the authored meaning.");
    }
    assert_eq!(compared,baseline.len());
}
