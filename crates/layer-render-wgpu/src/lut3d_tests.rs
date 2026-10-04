use crate::{WgpuRasterizer, PixelRect, scene, tests::fixture, test_support::{packet, floats, document_texture}};
use layer_core::{EffectInstance, EffectKind, EffectValue, Document, OccurrenceHandle, Lut3d, Point};
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth, rgb};
use layer_render::{CanvasRenderer, FramePacket};
use std::sync::Arc;
use crate::tests::native_effects::{effect_document,empty_document,insert_effect,insert_source,set_effect,mask,roundtrip};

fn set(effect:&mut EffectInstance,key:&str,value:EffectValue) {effect.set(key,value).unwrap();}
fn lookup(resource:Arc<Lut3d>,space:usize,intensity:f32)->EffectInstance {
    let mut effect=EffectInstance::new(fixture("color_lookup").program());
    set(&mut effect,"color_space",EffectValue::Choice(space as u32));set(&mut effect,"resource",EffectValue::Lut3d(Some(resource)));set(&mut effect,"intensity",EffectValue::Number(intensity));effect
}
fn pattern(colors:&[[f32;4]])->EffectInstance {
    let mut program=(*fixture("exposure").program()).clone();program.kind=EffectKind::Generator;program.entry="lut_input_pattern".into();
    let values=colors.iter().map(|c|format!("vec4<f32>({:?},{:?},{:?},{:?})",c[0],c[1],c[2],c[3])).collect::<Vec<_>>().join(",");
    program.wgsl=format!("fn lut_input_pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{let values=array<vec4<f32>,{}>({values});return values[u32(p.x)%{}u];}}",colors.len(),colors.len()).into();
    EffectInstance::new(Arc::new(program))
}
fn render_document(r:&mut WgpuRasterizer,document:&Document)->Vec<[f32;4]> {
    r.submit(packet(document.scene().with_owner(0,0),document.composition().size)).unwrap();floats(&crate::layer_tests::page_bytes(r,document_texture(r)))
}
fn render(r:&mut WgpuRasterizer,effects:&[EffectInstance])->Vec<[f32;4]> {
    let document=effect_document(effects,[256;2],r.document_color);render_document(r,&document)
}
fn program_mut(document:&mut Document,h:OccurrenceHandle)->&mut layer_core::EffectProgram {
    let definition=document.scene().effect_application(h).unwrap().definition;Arc::make_mut(&mut document.artwork.definitions.get_mut(definition).unwrap().program)
}
fn transfer(v:f64,space:usize,decode:bool)->f64 {
    let x=v.abs();let y=match (space,decode) {
        (0|1,true)=>if x<=0.04045 {x/12.92}else{((x+0.055)/1.055).powf(2.4)},
        (0|1,false)=>if x<=0.0031308 {x*12.92}else{1.055*x.powf(1./2.4)-0.055},
        (2,true)=>x.powf(563./256.),(2,false)=>x.powf(256./563.),
        (3,true)=>if x<=1./32. {x/16.}else{x.powf(1.8)},
        (3,false)=>if x<=1./512. {x*16.}else{x.powf(1./1.8)},_=>unreachable!()
    };y.copysign(v)
}
fn convert(v:[f64;3],source:usize,destination:usize)->[f64;3] {
    if source==destination {v}else{rgb::apply(RgbSpace::ALL[source].linear_transform(RgbSpace::ALL[destination]),v)}
}
fn tetra(resource:&Lut3d,e:[f64;3])->[f64;3] {
    let domain=resource.domain();let n=resource.size() as usize;
    let coordinate:[f64;3]=std::array::from_fn(|i|(e[i].clamp(domain[0][i] as f64,domain[1][i] as f64)-domain[0][i] as f64)/(domain[1][i] as f64-domain[0][i] as f64)*(n-1) as f64);
    let mut point=coordinate.map(|x|(x.floor() as usize).min(n-2));
    let fractional:[f64;3]=std::array::from_fn(|i|coordinate[i]-point[i] as f64);
    let mut axes=[0usize,1,2];axes.sort_by(|a,b|fractional[*b].total_cmp(&fractional[*a]));
    let t=axes.map(|i|fractional[i]);let weights=[1.-t[0],t[0]-t[1],t[1]-t[2],t[2]];
    let mut output=[0.;3];
    for (vertex,weight) in weights.into_iter().enumerate(){if vertex>0{point[axes[vertex-1]]+=1;}let sample=resource.samples().unwrap().nth((point[2]*n+point[1])*n+point[0]).unwrap();for i in 0..3{output[i]+=weight*f64::from(sample[i]);}}
    output
}
fn reference(resource:&Lut3d,c:[f32;4],working:usize,selected:usize,intensity:f32)->[f64;4] {
    if c[3]<=0. || intensity==0. {return c.map(f64::from);}
    let alpha=f64::from(c[3]);let straight=std::array::from_fn(|i|f64::from(c[i])/alpha);
    let encoded=convert(straight,working,selected).map(|v|transfer(v,selected,false));
    let domain=resource.domain();let mapped=tetra(resource,encoded);
    let edited=std::array::from_fn(|i|encoded[i]+f64::from(intensity)/100.*(mapped[i]-encoded[i].clamp(f64::from(domain[0][i]),f64::from(domain[1][i]))));
    let linear=convert(edited.map(|v|transfer(v,selected,true)),selected,working);
    if linear.iter().any(|v|!(*v as f32).is_finite()){return c.map(f64::from);}
    [linear[0]*alpha,linear[1]*alpha,linear[2]*alpha,alpha]
}
fn close(actual:[f32;4],expected:[f64;4],context:&str)->[f64;2] {
    let mut maximum=[0_f64;2];
    assert_eq!(actual[3],expected[3] as f32,"{context}: alpha");
    for i in 0..3{let scale=expected[i].abs().max(expected[3].max(1e-30));let error=(f64::from(actual[i])-expected[i]).abs();maximum[0]=maximum[0].max(error);maximum[1]=maximum[1].max(error/scale);assert!(actual[i].is_finite()&&error<=2e-5*scale,"{context}: {actual:?} != {expected:?}");}
    maximum
}
fn table(n:u32,domain:[[f32;3];2],identity:bool)->Arc<Lut3d> {
    let samples=(0..n).flat_map(|z|(0..n).flat_map(move|y|(0..n).map(move|x|[x,y,z]))).map(|p|{
        let t=p.map(|i|f64::from(i)/f64::from(n-1));
        let v=if identity{std::array::from_fn(|i|f64::from(domain[0][i])+t[i]*(f64::from(domain[1][i])-f64::from(domain[0][i])))}
        else{[0.1+0.7*t[0]*t[0]+0.2*t[1]*t[2],-0.15+0.8*t[1]+0.3*t[0]*t[2],0.05+1.1*t[2]*t[2]-0.2*t[0]*t[1]]};v.map(|v|v as f32)
    }).collect::<Vec<_>>();Arc::new(Lut3d::from_samples(n,domain,"Independent LUT".into(),samples.into()).unwrap())
}
fn encoded_input(e:[f64;3],working:usize,selected:usize,alpha:f32)->[f32;4] {
    let linear=convert(e.map(|v|transfer(v,selected,true)),selected,working);
    [(linear[0]*f64::from(alpha)) as f32,(linear[1]*f64::from(alpha)) as f32,(linear[2]*f64::from(alpha)) as f32,alpha]
}
fn residency(r:&WgpuRasterizer)->(u64,u64) {
    let cache=r.device.effect_resources.lock().unwrap();(cache.uploads,cache.bytes())
}

#[test]
fn builtin_looks_preserve_alpha_hdr_and_match_known_srgb_knots_in_every_working_profile() {
    use layer_core::lut3d::Look;
    let points=[[0.25,0.5,0.75],[0.;3],[1.;3],[-1.;3],[3.;3]];
    for working in 0..4 {
        let mut renderer=WgpuRasterizer::new_native_headless(DocumentColor{space:RgbSpace::ALL[working],depth:SampleDepth::F32}).unwrap();
        let inputs=points.into_iter().flat_map(|p|[0.,8e-8,0.37,1.].map(|alpha|encoded_input(p,working,0,alpha))).collect::<Vec<_>>();
        let original=pattern(&inputs);
        for look in Look::ALL {
            let generation=std::time::Instant::now();
            let resource=look.resource();
            if working==0 {println!("builtin {look:?} first-resource={:?} bytes={}",generation.elapsed(),resource.bytes());}
            let started=std::time::Instant::now();
            let first=render(&mut renderer,&[lookup(resource.clone(),0,100.),original.clone()]);
            println!("builtin {look:?} working={working} first-render={:?}",started.elapsed());
            for (index,input) in inputs.iter().copied().enumerate() {
                let encoded=points[index/4];
                let alpha=f64::from(input[3]);
                let expected=if alpha==0. { input.map(f64::from) } else {
                    let clamped=encoded.map(|value|value.clamp(0.,1.));
                    let mapped=match look {
                        Look::Warm=>[clamped[0]+0.16*clamped[0]*(1.-clamped[0]),clamped[1]+0.02*clamped[1]*(1.-clamped[1]),clamped[2]-0.16*clamped[2]*(1.-clamped[2])],
                        Look::Cool=>[clamped[0]-0.16*clamped[0]*(1.-clamped[0]),clamped[1]-0.02*clamped[1]*(1.-clamped[1]),clamped[2]+0.16*clamped[2]*(1.-clamped[2])],
                        Look::Monochrome=>{
                            let gray=transfer(0.21263900587151036*transfer(clamped[0],0,true)+0.715168678767756*transfer(clamped[1],0,true)+0.07219231536073371*transfer(clamped[2],0,true),0,false);
                            [gray;3]
                        }
                    };
                    let edited=std::array::from_fn(|axis|encoded[axis]+mapped[axis]-clamped[axis]);
                    let linear=convert(edited.map(|value|transfer(value,0,true)),0,working);
                    [linear[0]*alpha,linear[1]*alpha,linear[2]*alpha,alpha]
                };
                close(first[index],expected,&format!("builtin {look:?} profile={working} sample={index}"));
            }
            let neutral=render(&mut renderer,&[lookup(resource,0,0.),original.clone()]);
            assert_eq!(&neutral[..inputs.len()],inputs.as_slice());
        }
    }
}
fn capture(r:&mut WgpuRasterizer,scene:&mut scene::Scene,document:&Document,output:scene::Output,crop:PixelRect)->Vec<[f32;4]> {
    let (target,_)=crate::create_color_target(&r.device,[crop.width(),crop.height()],"LUT checkpoint oracle");
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    scene.capture_region(r,packet(document.scene().with_owner(0,0),[256;2]),&target,crop,output,&mut encoder).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);floats(&crate::layer_tests::page_bytes(r,&target))
}

#[test]
fn tetrahedra_ties_profiles_depths_alpha_and_intensity_match_independent_reference() {
    let points=[[0.8,0.5,0.2],[0.8,0.2,0.5],[0.5,0.8,0.2],[0.2,0.8,0.5],[0.5,0.2,0.8],[0.2,0.5,0.8],[0.3;3],[0.3,0.3,0.1],[0.3,0.1,0.3],[0.1,0.3,0.3],[0.;3],[1.;3],[-1.,0.3,3.],[2.,-0.6,0.4]];
    let resources=[table(2,[[0.;3],[1.;3]],false),table(3,[[-0.25,-0.5,-0.75],[1.25,1.5,1.75]],false)];
    let mut maximum=[0_f64;2];
    for working in 0..4{for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32]{
        let mut r=WgpuRasterizer::new_native_headless(DocumentColor{space:RgbSpace::ALL[working],depth}).unwrap();
        for selected in 0..4{
            let inputs=points.into_iter().flat_map(|p|[0.,8e-8,0.37,1.].map(|a|encoded_input(p,working,selected,a))).collect::<Vec<_>>();let original=pattern(&inputs);
            for resource in &resources{for intensity in [0.,50.,100.]{
                let layers=[lookup(resource.clone(),selected,intensity),original.clone()];let pixels=render(&mut r,&layers);
                for (i,input) in inputs.iter().copied().enumerate(){let error=close(pixels[i],reference(resource,input,working,selected,intensity),&format!("{working}/{selected}/{depth:?}/N{}/i{intensity}/pixel{i}",resource.size()));for j in 0..2{maximum[j]=maximum[j].max(error[j]);}if intensity==0. || input[3]==0.{assert_eq!(pixels[i],input);}}
            }}
        }
    }}
    assert!(maximum[1]<2e-6,"standard LUT oracle scaled maximum={}",maximum[1]);
    println!("LUT oracle maximum absolute={} scaled={}",maximum[0],maximum[1]);
}

#[test]
fn identity_empty_resources_and_extended_residuals_preserve_original_pixels() {
    let resource=table(2,[[-0.5;3],[1.5;3]],true);
    for working in 0..4{
        let mut r=WgpuRasterizer::new_native_headless(DocumentColor{space:RgbSpace::ALL[working],depth:SampleDepth::F32}).unwrap();
        for selected in 0..4{
            let inputs=[[-1.,0.25,3.],[0.,0.5,1.],[2.,-0.5,0.125]].into_iter().flat_map(|p|[8e-8,0.37,1.].map(|a|encoded_input(p,working,selected,a))).collect::<Vec<_>>();let original=pattern(&inputs);
            let mut layer=lookup(resource.clone(),selected,100.);
            for input in inputs.iter().copied().zip(render(&mut r,&[layer.clone(),original.clone()])){close(input.1,input.0.map(f64::from),"identity residual");}
            set(&mut layer,"resource",EffectValue::Lut3d(None));let empty=render(&mut r,&[layer,original]);assert_eq!(&empty[..inputs.len()],inputs.as_slice());
        }
    }
}

#[test]
fn stacked_resources_masks_clipping_and_cropped_checkpoints_keep_distinct_inputs() {
    let first=table(3,[[0.;3],[1.;3]],false);let second=table(2,[[-0.25;3],[1.25;3]],false);
    let colors=[[0.24,0.41,0.69,0.37],[0.12,0.31,0.19,0.8],[0.,0.,0.,0.]];
    for working in 0..4{
        let mut r=WgpuRasterizer::new_native_headless(DocumentColor{space:RgbSpace::ALL[working],depth:SampleDepth::F32}).unwrap();
        let bottom=lookup(first.clone(),0,65.);let top=lookup(second.clone(),3,40.);

        for clipped in [false,true]{let mut document=effect_document(&[top.clone(),bottom.clone(),pattern(&colors)],[256;2],r.document_color);
            let handles=document.scene().order().to_vec();let top_h=handles[0];let bottom_h=handles[1];
            document.artwork.occurrences.get_mut(top_h).unwrap().opacity=0.8;document.artwork.occurrences.get_mut(top_h).unwrap().clipped=clipped;
            document.artwork.occurrences.get_mut(bottom_h).unwrap().opacity=0.6;document.artwork.occurrences.get_mut(bottom_h).unwrap().clipped=clipped;mask(&mut document,bottom_h,0.25);
            let full=render_document(&mut r,&document);
            let mut expected_mid=Vec::new();let mut expected_final=Vec::new();for input in colors{
                let corrected=reference(&first,input,working,0,65.);let mid=std::array::from_fn::<_,4,_>(|i|if i==3{f64::from(input[i])}else{f64::from(input[i])+0.15*(corrected[i]-f64::from(input[i]))});
                let second_input=mid.map(|v|v as f32);let corrected=reference(&second,second_input,working,3,40.);let output=std::array::from_fn(|i|if i==3{mid[i]}else{mid[i]+0.8*(corrected[i]-mid[i])});expected_mid.push(mid);expected_final.push(output);
            }
            for i in 0..3{close(full[i],expected_final[i],"stacked LUTs");}
            let mut scene=scene::Scene::new(&r);let all=capture(&mut r,&mut scene,&document,scene::Output::Artwork(None),PixelRect::full([256;2]));
            for (i,p) in all.iter().enumerate(){close(*p,expected_final[(i%256)%3],"retained checkpoint full");}
            let crop=PixelRect::new(131,7,149,19);let before_top=capture(&mut r,&mut scene,&document,scene::Output::EffectInput(top_h),crop);
            for (i,p) in before_top.iter().enumerate(){close(*p,expected_mid[((i%crop.width() as usize)+131)%3],"checkpoint effect input");}
            let cropped=capture(&mut r,&mut scene,&document,scene::Output::Artwork(None),crop);for (i,p) in cropped.iter().enumerate(){close(*p,expected_final[((i%crop.width() as usize)+131)%3],"checkpoint crop");}
        }
    }
}

#[test]
fn hundred_intensity_edits_aliases_and_resource_removal_keep_uploads_bounded() {
    let resource=table(17,[[0.;3],[1.;3]],false);let bytes=(96+resource.bytes() as u64).next_multiple_of(16);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let cache=r.device.effect_resources.clone();
    let source=pattern(&[[0.24,0.41,0.69,0.37]]);let layer=lookup(resource.clone(),0,100.);
    let mut document=effect_document(&[layer,source],[256;2],r.document_color);
    let handles=document.scene().order().to_vec();let target=handles[0];let source=handles[1];let stack=document.composition().result;
    document.artwork.stacks.get_mut(stack).unwrap().entries=vec![source];crate::tests::native_effects::refresh(&mut document);
    render_document(&mut r,&document);let baseline=residency(&r);assert_eq!(baseline,(0,16));
    document.artwork.stacks.get_mut(stack).unwrap().entries=handles;crate::tests::native_effects::refresh(&mut document);
    render_document(&mut r,&document);let initial=residency(&r);assert_eq!(initial,(1,bytes+baseline.1));
    for intensity in 0..100{set_effect(&mut document,target,"intensity",EffectValue::Number(intensity as f32));let pixels=render_document(&mut r,&document);close(pixels[0],reference(&resource,[0.24,0.41,0.69,0.37],0,0,intensity as f32),"intensity edit original input");assert_eq!(residency(&r),initial);}
    for selected in 0..4{set_effect(&mut document,target,"color_space",EffectValue::Choice(selected));render_document(&mut r,&document);assert_eq!(residency(&r),initial);}
    set_effect(&mut document,target,"resource",EffectValue::Lut3d(Some(Arc::new(resource.with_title("Retitled use".into()).unwrap()))));
    render_document(&mut r,&document);assert_eq!(residency(&r),initial);
    let alias=Arc::new(Lut3d::from_samples(resource.size(),resource.domain(),"Alias".into(),resource.samples().unwrap().collect::<Vec<_>>().into()).unwrap());assert_eq!(alias.digest(),resource.digest());
    let duplicate=insert_effect(&mut document,lookup(alias,0,100.));
    document.artwork.stacks.get_mut(stack).unwrap().entries=vec![duplicate,target,source];crate::tests::native_effects::refresh(&mut document);
    render_document(&mut r,&document);assert_eq!(residency(&r),initial);
    r.submit(FramePacket{composite_all:false,..packet(document.scene().with_owner(0,0),[256;2])}).unwrap();assert_eq!(residency(&r),initial);
    document.artwork.stacks.get_mut(stack).unwrap().entries=vec![source];crate::tests::native_effects::refresh(&mut document);
    assert!(document.scene().position(target).is_none());assert!(document.scene().position(duplicate).is_none());
    assert_eq!(document.scene().effect(target).unwrap().lut3d().unwrap().digest(),resource.digest());
    assert_eq!(document.scene().effect(duplicate).unwrap().lut3d().unwrap().digest(),resource.digest());
    for _ in 0..3{render_document(&mut r,&document);}assert_eq!(residency(&r),(initial.0,baseline.1));
    drop(document);drop(resource);drop(r);assert_eq!(cache.lock().unwrap().bytes(),baseline.1);
}

#[test]
fn archive_reopen_owns_resource_without_original_file_and_restores_editable_pixels() {
    let resource=table(3,[[0.;3],[1.;3]],false);let color=DocumentColor{space:RgbSpace::ProPhoto,depth:SampleDepth::F32};
    let mut document=empty_document([256;2],color);insert_effect(&mut document,lookup(resource.clone(),0,67.));
    let source=insert_source(&mut document,"Retained pixels",crate::test_support::depth_source([256;2],SampleDepth::F32,color.space,4*1024*1024,|x,y|[(x as f32+1.)/257.,(y as f32+1.)/257.,0.24,0.37]));
    document.working.occurrence=Some(source);document.working.target=document.scene().source_target(source);
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();let original=render_document(&mut r,&document);
    let mut reopened=roundtrip(document);drop(resource);drop(r);let target=reopened.scene().order()[0];let ready=reopened.scene().effect(target).unwrap().lut3d().unwrap().clone();assert!(ready.accepts(RgbSpace::Srgb));
    let mut fresh=WgpuRasterizer::new_native_headless(color).unwrap();assert_eq!(render_document(&mut fresh,&reopened),original);
    set_effect(&mut reopened,target,"intensity",EffectValue::Number(0.));let neutral=render_document(&mut fresh,&reopened);assert_ne!(neutral,original);
    set_effect(&mut reopened,target,"intensity",EffectValue::Number(67.));assert_eq!(render_document(&mut fresh,&reopened),original);assert_eq!(residency(&fresh),(1,(96+ready.bytes() as u64).next_multiple_of(16)));
}

#[test]
fn unsafe_selected_space_is_refused_and_unrepresentable_residual_returns_source() {
    let unsafe_resource=Arc::new(Lut3d::from_samples(2,[[0.;3],[1.;3]],"Near maximum".into(),vec![[1.1972514e16;3];8].into()).unwrap());assert!(!unsafe_resource.accepts(RgbSpace::Srgb));
    let mut layer=lookup(table(2,[[0.;3],[1.;3]],false),0,100.);let before=layer.clone();assert!(layer.set("resource",EffectValue::Lut3d(Some(unsafe_resource))).is_err());assert_eq!(layer,before);
    let encoded=RgbSpace::Srgb.encode(f64::from(f32::MAX)*0.1) as f32;let resource=Arc::new(Lut3d::from_samples(2,[[0.;3],[1.;3]],"Large but admitted".into(),vec![[encoded;3];8].into()).unwrap());assert!(resource.accepts(RgbSpace::Srgb));
    let source=[f32::MAX*0.9,f32::MAX*0.9,f32::MAX*0.9,1.];let expected=reference(&resource,source,0,0,100.);assert_eq!(expected,source.map(f64::from));
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let original=pattern(&[source]);assert_eq!(render(&mut r,std::slice::from_ref(&original))[0],source,"finite original reaches LUT stage unchanged");
    let result=render(&mut r,&[lookup(resource,0,100.),original]);assert_eq!(result[0],source,"unrepresentable extended encoded result returns original pixel");
}

#[test]
fn literal_domain_coordinates_preserve_adjacent_normal_interiors_and_tetra_ties() {
    let mut r=WgpuRasterizer::new_native_headless(DocumentColor{space:RgbSpace::Srgb,depth:SampleDepth::F32}).unwrap();
    for minimum in [f32::MIN_POSITIVE,f32::MIN_POSITIVE*2.,1e-30,1.,1e30,-1e-30]{
        let maximum=f32::from_bits(if minimum.is_sign_negative(){minimum.to_bits()-8}else{minimum.to_bits()+8});
        let resource=Arc::new(Lut3d::from_samples(2,[[minimum;3],[maximum;3]],"Normal precision probe".into(),vec![[0.;3];8].into()).unwrap());
        let mut layer=lookup(resource,0,100.);let program=Arc::make_mut(&mut layer.program);
        let code=program.wgsl.sources().unwrap().iter().map(|v|v.as_ref()).collect::<Vec<_>>().join("\n");
        program.entry="cube_coordinate_probe".into();program.wgsl=format!("{code}\nfn cube_coordinate_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{return vec4<f32>(fx_cube_coordinate(c.r,0u),fx_cube_coordinate(c.g,1u),fx_cube_coordinate(c.b,2u),1.);}}").into();
        let inputs=(0..=8).map(|i|{let v=f32::from_bits(if minimum.is_sign_negative(){minimum.to_bits()-i}else{minimum.to_bits()+i});[v,v,v,1.]}).collect::<Vec<_>>();
        let actual=render(&mut r,&[layer,pattern(&inputs)]);for i in 0..9{assert_eq!(actual[i],[i as f32/8.,i as f32/8.,i as f32/8.,1.],"normal coordinate {minimum:e}/{maximum:e}/step{i}");}
    }
    let mut layer=lookup(table(2,[[0.;3],[1.;3]],false),0,100.);let program=Arc::make_mut(&mut layer.program);
    let code=program.wgsl.sources().unwrap().iter().map(|v|v.as_ref()).collect::<Vec<_>>().join("\n");program.entry="tetra_offset_probe".into();program.wgsl=format!("{code}\nfn tetra_offset_probe(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{let t=tetrahedron(c.rgb);return vec4<f32>(vec3<f32>(select(t.first,t.second,fx_parameter(b,2u).x<50.)),1.);}}").into();
    let inputs=[[0.8,0.5,0.2,1.],[0.8,0.2,0.5,1.],[0.5,0.8,0.2,1.],[0.2,0.8,0.5,1.],[0.5,0.2,0.8,1.],[0.2,0.5,0.8,1.],[0.3,0.3,0.3,1.],[0.3,0.3,0.1,1.],[0.3,0.1,0.3,1.],[0.1,0.3,0.3,1.]];
    for (intensity,offset) in [(100.,0),(0.,1)]{set(&mut layer,"intensity",EffectValue::Number(intensity));let actual=render(&mut r,&[layer.clone(),pattern(&inputs)]);for (i,c) in inputs.iter().enumerate(){let mut axes=[0usize,1,2];axes.sort_by(|a,b|c[*b].total_cmp(&c[*a]));let mut expected=[0.,0.,0.,1.];expected[axes[offset]]=1.;assert_eq!(actual[i],expected,"tetra offset {offset}, {c:?}");}}
}

#[test]
fn discontinuous_n65_vertices_require_native_graph_evaluation() {
    use layer_core::EffectResolution;
    use crate::scene::scale::{Evaluation,tests::{display_pixels,quality}};
    assert_eq!(fixture("color_lookup").program().resolution,EffectResolution::Native);
    let extent=[1033,517];
    let n=65;
    let samples=(0..n).flat_map(|z|(0..n).flat_map(move|y|(0..n).map(move|x|[x,y,z]))).map(|p|[if p[0]>=32{1.}else{0.};3]).collect::<Vec<_>>();
    let resource=Arc::new(Lut3d::from_samples(n,[[0.;3],[1.;3]],"Adjacent vertex edge".into(),samples.into()).unwrap());
    let mut worst=[0_f32;3];
    for working in RgbSpace::ALL {
        let color=DocumentColor{space:working,depth:SampleDepth::F32};
        let mut window=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut exact=WgpuRasterizer::new_native_headless(color).unwrap();exact.test.reference=true;
        for hdr in [false,true] {
            let original=crate::test_support::depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,16*1024*1024,|x,y|{
                let gray=if hdr && (x/67+y/43)%5==0 {if x%2==0{-0.125}else{4.}} else {RgbSpace::Srgb.decode(if (x/3+y/2)%2==0{126./255.}else{130./255.}) as f32};
                let alpha=if hdr{match (x/67+y/43)%3{0=>8e-8,1=>0.37,_=>1.}}else{1.};[gray,gray,gray,alpha]
            });
            let mut adjustment=lookup(resource.clone(),0,100.);Arc::make_mut(&mut adjustment.program).resolution=EffectResolution::Display;
            let mut document=empty_document(extent,color);let target=insert_effect(&mut document,adjustment);insert_source(&mut document,"Correlated LUT edges",original);
            if !hdr {
                mask(&mut document,target,1.);let coverage=document.scene().mask(target).unwrap().0.source;
                document.artwork.coverage.get_mut(coverage).unwrap().initial=Some(layer_core::Selection::polygon(vec![Point{x:573.,y:237.},Point{x:1001.,y:257.},Point{x:987.,y:507.},Point{x:587.,y:479.}]).unwrap());
                let occurrence=document.artwork.occurrences.get_mut(target).unwrap();occurrence.opacity=0.7;occurrence.clipped=true;
            }
            for level in [1,2,3] {
                let scale=1./(1<<level) as f32;let mut frame=packet(document.scene().with_owner(0,0),extent);
                frame.view.width_px=43;frame.view.height_px=25;
                frame.view.document_to_surface=[scale,0.,0.,scale,-610.*scale,-270.*scale];
                window.submit(frame).unwrap();exact.submit(frame).unwrap();
                let cache=window.scale_display.as_ref().unwrap();assert_eq!(cache.plan.level,level);assert!(cache.evaluation==Evaluation::Display);
                let error=quality(&display_pixels(&window),&crate::test_support::float_pixels(&exact,document_texture(&exact)),cache.plan);
                assert!(error.iter().all(|v|v.is_finite()));for i in 0..3{worst[i]=worst[i].max(error[i]);}
                println!("Color Lookup adjacent N65 {working:?} hdr={hdr} level={level} error={error:?}");
                assert_eq!(window.readback_srgb_rgba8().unwrap(),exact.readback_srgb_rgba8().unwrap());
            }
            program_mut(&mut document,target).resolution=EffectResolution::Native;
            let scale=0.25;let mut frame=packet(document.scene().with_owner(0,0),extent);frame.view.width_px=43;frame.view.height_px=25;
            frame.view.document_to_surface=[scale,0.,0.,scale,-610.*scale,-270.*scale];
            window.submit(frame).unwrap();exact.submit(frame).unwrap();
            let cache=window.scale_display.as_ref().unwrap();assert!(cache.evaluation==Evaluation::Native);
            let error=quality(&display_pixels(&window),&crate::test_support::float_pixels(&exact,document_texture(&exact)),cache.plan);
            assert!(error[2]<2e-5,"Native LUT preserves exact reduced artwork: {working:?} hdr={hdr} {error:?}");
        }
    }
    println!("Color Lookup adjacent N65 worst={worst:?}");
    assert!(worst[0]>=0.002 || worst[1]>=0.015,"admitted sharp LUT must reject generic reduced evaluation: {worst:?}");
}

#[test]
fn near_maximum_admitted_tables_and_physical_profile_transforms_remain_finite() {
    let mut maximum=[0_f64;2];
    for selected in 0..4 {
        let encoded=[0.001,0.002,0.003].map(|v|transfer(v*f64::from(f32::MAX),selected,false) as f32);
        let resource=Arc::new(Lut3d::from_samples(2,[[0.;3],[1.;3]],"Admitted physical HDR".into(),vec![encoded;8].into()).unwrap());
        assert!(resource.accepts(RgbSpace::ALL[selected]));
        for working in 0..4 {
            let mut r=WgpuRasterizer::new_native_headless(DocumentColor{space:RgbSpace::ALL[working],depth:SampleDepth::F32}).unwrap();
            let inputs=[[0.01,0.025,0.05],[0.15,0.05,0.02],[-0.001,0.0005,0.002]].into_iter().flat_map(|p|[8e-8,0.37,1.].map(|alpha|{
                let linear=convert(p.map(|v|v*f64::from(f32::MAX)),selected,working);
                [(linear[0]*f64::from(alpha)) as f32,(linear[1]*f64::from(alpha)) as f32,(linear[2]*f64::from(alpha)) as f32,alpha]
            })).collect::<Vec<_>>();let original=pattern(&inputs);
            for intensity in [0.,50.,100.] {
                let actual=render(&mut r,&[lookup(resource.clone(),selected,intensity),original.clone()]);
                for (i,input) in inputs.iter().copied().enumerate() {
                    let expected=reference(&resource,input,working,selected,intensity);
                    let scale=expected[..3].iter().map(|v|v.abs()).fold(1_f64,f64::max);
                    assert_eq!(actual[i][3],expected[3] as f32);
                    for c in 0..3 {
                        let error=(f64::from(actual[i][c])-expected[c]).abs();
                        maximum[0]=maximum[0].max(error);maximum[1]=maximum[1].max(error/scale);
                        assert!(actual[i][c].is_finite()&&error/scale<2e-5,"physical HDR {working}/{selected}/i{intensity}/pixel{i}: {:?} != {expected:?}",actual[i]);
                    }
                    if intensity==0.{assert_eq!(actual[i],input);}
                }
            }
        }
    }
    println!("Physical HDR LUT oracle maximum absolute={} scaled={}",maximum[0],maximum[1]);
}

#[test]
fn auxiliary_lookup_is_pointwise_and_stays_out_of_neighbor_fusion_chains() {
    let resource=table(2,[[0.;3],[1.;3]],false);let lut=lookup(resource,0,100.);
    assert!(!lut.program.image_boundary());assert!(lut.program.fusion_boundary());
    let exposure=EffectInstance::new(fixture("exposure").program());let invert=EffectInstance::new(fixture("invert").program());let source=pattern(&[[0.2,0.4,0.6,1.]]);
    let document=effect_document(&[exposure.clone(),lut.clone(),invert.clone(),source.clone()],[256;2],Default::default());let lut_h=document.scene().order()[1];
    let chains=scene::startup_effect_chains(document.scene());
    assert!(chains.iter().any(|(chain,execution)|chain[0]==lut_h && chain.len()==1 && *execution==crate::effects::Execution::Fused));
    assert!(chains.iter().all(|(chain,_)|chain.len()==1 || chain.iter().all(|h|*h!=lut_h)));
    let ordinary=effect_document(&[exposure.clone(),invert,source],[256;2],Default::default());assert!(scene::startup_effect_chains(ordinary.scene()).iter().any(|(chain,_)|chain.len()==2));
    let invalid=effect_document(&[lut,exposure],[256;2],Default::default());let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let mut scene=scene::Scene::new(&r);
    assert!(scene.effects.prepare(&r,invalid.scene(),invalid.scene().order(),crate::effects::Execution::Fused,0.,0,Default::default()).is_err());
}

#[test]
fn resident_lookup_tiles_batch_with_immutable_auxiliary_and_bounded_sources() {
    use crate::scene::scale::tests::{display_pixels,quality};
    let extent=[1795,773];let color=DocumentColor{space:RgbSpace::Srgb,depth:SampleDepth::F32};
    let resource=table(3,[[0.;3],[1.;3]],false);
    let codes=|id:u32|[20+id*5,40+id*3,220-id*4];
    let original=layer_core::color::source::rgba8_source(extent,|x,y|{let c=codes(y/256*8+x/256);[c[0] as u8,c[1] as u8,c[2] as u8,255]});
    let mut document=empty_document(extent,color);let target=insert_effect(&mut document,lookup(resource.clone(),0,100.));insert_source(&mut document,"Distinct cold lookup tiles",original);
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();r.native_edit.as_mut().unwrap().display_complete_bytes=u64::MAX;
    drop(r.device.effect_resources.lock().unwrap().get(&r.device,&r.queue,None).unwrap());
    let baseline=residency(&r);assert_eq!(baseline,(0,16));
    let mut source_bytes=None;
    for (state,intensity) in [35.,70.,100.].into_iter().enumerate() {
        set_effect(&mut document,target,"intensity",EffectValue::Number(intensity));
        let coverage=if state==2 {mask(&mut document,target,0.25);let occurrence=document.artwork.occurrences.get_mut(target).unwrap();occurrence.opacity=0.6;occurrence.clipped=true;0.15}else{1.};
        let mut frame=packet(document.scene().with_owner(0,0),extent);frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];r.submit(frame).unwrap();
        let cache=r.scale_display.as_ref().unwrap();assert!(cache.resident_bytes()>0);assert!(!cache.has_pending_work(&r));
        let passes=r.scene.as_ref().unwrap().effect_passes;assert!(passes<=2,"32 cold/warm pointwise LUT tiles must share at most two effect passes: state={state} passes={passes}");
        let expected=(0..32).map(|id|{
            let c=codes(id).map(|v|RgbSpace::Srgb.decode(f64::from(v)/255.) as f32);let input=[c[0],c[1],c[2],1.];let adjusted=reference(&resource,input,0,0,intensity);
            std::array::from_fn::<_,4,_>(|i|if i==3{1.}else{(f64::from(input[i])+coverage*(adjusted[i]-f64::from(input[i]))) as f32})
        }).collect::<Vec<_>>();
        let independent=(0..extent[0]*extent[1]).map(|i|expected[((i/extent[0])/256*8+(i%extent[0])/256) as usize]).collect::<Vec<_>>();
        let error=quality(&display_pixels(&r),&independent,cache.plan);assert!(error[2]<2e-5,"batched LUT independent odd-edge/mask/clip pixels {error:?}");
        println!("Resident LUT state={state} pages=32 effect_passes={passes} error={error:?}");
        let bytes=r.source_tiles.borrow().gpu_bytes();if let Some(expected)=source_bytes{assert_eq!(bytes,expected);}else{source_bytes=Some(bytes);}
        assert_eq!(residency(&r),(1,(96+resource.bytes() as u64).next_multiple_of(16)+baseline.1));
    }
}

#[test]
fn shared_samples_with_different_domains_have_distinct_gpu_headers() {
    let first=table(3,[[0.;3],[1.;3]],false);
    let second=Lut3d::from_samples(3,[[0.;3],[2.;3]],"Different domain".into(),first.samples().unwrap().collect::<Vec<_>>().into()).unwrap();
    assert_eq!(first.digest(),second.digest());
    let renderer=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut cache=renderer.device.effect_resources.lock().unwrap();
    let a=cache.get(&renderer.device,&renderer.queue,Some(&first)).unwrap();
    let b=cache.get(&renderer.device,&renderer.queue,Some(&second)).unwrap();
    assert!(!Arc::ptr_eq(&a,&b));
    assert!(Arc::ptr_eq(&a,&cache.get(&renderer.device,&renderer.queue,Some(&first)).unwrap()));
}
