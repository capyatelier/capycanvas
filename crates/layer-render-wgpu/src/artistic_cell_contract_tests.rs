use super::*;
use layer_core::EffectValue;

fn effect(name: &str, parameters: &[(&str,f32)]) -> EffectInstance {
    let mut effect=layer_core::bundled_effect_catalog().get(name).unwrap().preview().unwrap();
    for &(id,value) in parameters {effect.set(id,EffectValue::Number(value)).unwrap();}
    effect
}
fn source(extent: [u32;2], depth: SampleDepth) -> Document {
    let mut document=empty_document(extent,DocumentColor {space:RgbSpace::Srgb,depth});
    let image=crate::test_support::depth_source(extent,depth,RgbSpace::Srgb,64<<20,|x,y| {
        match (x+3*y)%7 {
            0=>[0.;4],1=>[0.8,0.02,0.1,1.],2=>[0.03,0.14,0.9,0.35],
            3=>[0.1,0.75,0.08,0.65],4 if depth.is_float()=>[-0.5,2.,7.,0.75],
            4=>[0.01,0.03,0.02,1.],5=>[0.,0.,0.,1.],_=>[1.;4],
        }
    });
    insert_source(&mut document,"Native cell palette",image);document
}
fn read(gpu: &SnapshotGpu, document: &Document) -> Vec<[f32;4]> {
    let [w,h]=document.composition().size;
    gpu.capture_scene(document.snapshot(),SceneScope::All,Default::default()).unwrap().read_region([0,0,w,h]).unwrap()
}
fn close(actual: &[[f32;4]], expected: &[[f32;4]], context: &str) {
    assert_eq!(actual.len(),expected.len());
    for (i,(a,b)) in actual.iter().zip(expected).enumerate() {
        assert!((0..4).all(|c|(a[c]-b[c]).abs()<=0.000002*(1.+b[c].abs())),"{context} pixel{i}: {a:?} != {b:?}");
    }
}
fn palette(input: &[[f32;4]], output: &[[f32;4]], context: &str) {
    let colors:Vec<_>=input.iter().copied().fold(Vec::new(),|mut colors,p| {if !colors.contains(&p) {colors.push(p);}colors});
    for (i,p) in output.iter().enumerate() {
        assert!(p.iter().all(|v|v.is_finite()),"{context}: nonfinite pixel{i}: {p:?}");
        assert!(colors.iter().any(|s|(0..4).all(|c|(p[c]-s[c]).abs()<=0.000002*(1.+s[c].abs()))),"{context}: invented RGBA at{i}: {p:?}");
    }
}
fn maximum(name: &str) -> EffectInstance {
    if name=="crystallize" {effect(name,&[("cell_size",128.),("stretch",100.),("seed",65535.)])}
    else {
        let mut parameters=vec![("spread",256.),("cell_size",50.),("density",100.)];
        if name=="scatter" {parameters.push(("seed",65535.));}
        effect(name,&parameters)
    }
}

#[test]
fn artistic_cells_copy_native_rgba_through_alpha_hdr_borders_and_maximum_parameters() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let gpu=r.snapshot_gpu();
    assert_ne!(r.adapter().get_info().device_type,wgpu::DeviceType::Cpu);
    for depth in [SampleDepth::U8,SampleDepth::F32] {
        let document=source([269,139],depth);let input=read(&gpu,&document);
        for name in ["dither_spread","scatter","crystallize"] {
            for operation in [effect(name,&[]),maximum(name)] {
                let output=read(&gpu,&filtered(document.clone(),operation,None));
                palette(&input,&output,&format!("{name}/{depth:?}"));
                assert!(input.iter().zip(&output).any(|(a,b)|a!=b),"{name}/{depth:?}: fixture must change paint");
            }
        }
    }
}

#[test]
fn artistic_cells_zero_density_and_spread_are_identity_without_partial_rgb_blending() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let gpu=r.snapshot_gpu();
    let document=source([91,73],SampleDepth::F32);let input=read(&gpu,&document);
    for name in ["dither_spread","scatter"] {
        for parameters in [vec![("density",0.)],vec![("spread",0.)]] {
            close(&read(&gpu,&filtered(document.clone(),effect(name,&parameters),None)),&input,name);
        }
        let output=read(&gpu,&filtered(document.clone(),effect(name,&[("density",37.)]),None));
        palette(&input,&output,name);
        assert!(input.iter().zip(&output).any(|(a,b)|a!=b),"{name}: partial density must activate cells");
        assert!(input.iter().zip(&output).any(|(a,b)|a==b),"{name}: partial density must preserve native pixels");
    }
}

#[test]
fn artistic_cells_are_repeatable_across_independent_capture_windows_and_seeds() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let gpu=r.snapshot_gpu();
    let original=source([521,271],SampleDepth::F32);
    for (name,operation) in [("dither_spread",maximum("dither_spread")),
        ("dither_spread",effect("dither_spread",&[("spread",40.),("cell_size",50.)])),
        ("scatter",maximum("scatter")),("crystallize",maximum("crystallize"))] {
        let document=filtered(original.clone(),operation,None);let full=read(&gpu,&document);
        close(&read(&gpu,&document),&full,&format!("{name} repeated"));
        for region in [[251,5,19,259],[1,251,519,19],[509,259,12,12]] {
            let actual=gpu.capture_scene(document.snapshot(),SceneScope::All,Default::default()).unwrap().read_region(region).unwrap();
            let expected:Vec<_>=(region[1]..region[1]+region[3]).flat_map(|y|(region[0]..region[0]+region[2]).map(move|x|(y*521+x) as usize)).map(|i|full[i]).collect();
            close(&actual,&expected,&format!("{name} region{region:?}"));
        }
        if name!="dither_spread" {
            let mut operation=maximum(name);operation.set("seed",EffectValue::Number(17.)).unwrap();
            let different=read(&gpu,&filtered(original.clone(),operation,None));
            assert_ne!(full,different,"{name}: authored seed must change native donor choices");
        }
    }
}

#[test]
fn artistic_cells_crystallize_both_stretch_axes_work_at_low_workgroup_storage_limits() {
    let instance=WgpuRasterizer::headless_instance();
    let adapter=pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {power_preference:wgpu::PowerPreference::HighPerformance,..Default::default()})).unwrap();
    assert_ne!(adapter.get_info().device_type,wgpu::DeviceType::Cpu);
    let limits=wgpu::Limits {max_compute_workgroup_storage_size:16352,max_storage_buffers_per_shader_stage:4,..crate::rendering_limits(adapter.limits())};
    let (device,queue)=pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {required_limits:limits,required_features:crate::rendering_features(&adapter),..Default::default()})).unwrap();
    let r=WgpuRasterizer::native_capture_on_gpu(adapter,device.into(),queue,Default::default()).unwrap();let gpu=r.snapshot_gpu();
    let document=source([263,131],SampleDepth::F32);let input=read(&gpu,&document);
    let mut outputs=Vec::new();
    for stretch in [-100f32,0.,100.] {
        let output=read(&gpu,&filtered(document.clone(),effect("crystallize",&[("cell_size",128.),("stretch",stretch)]),None));
        palette(&input,&output,&format!("crystallize stretch{stretch}"));outputs.push(output);
    }
    assert_ne!(outputs[0],outputs[2],"Stretch must transform the normalized Voronoi metric axes");
    for name in ["dither_spread","scatter"] {palette(&input,&read(&gpu,&filtered(document.clone(),maximum(name),None)),name);}
}

#[test]
#[ignore]
fn artistic_cells_native_capture_completion_observations() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let gpu=r.snapshot_gpu();
    eprintln!("ARTISTIC_CELLS_ADAPTER {:?}",r.adapter().get_info());
    for name in ["dither_spread","scatter","crystallize"] {
        let original=source([512;2],SampleDepth::F32);
        for maximum_parameters in [false,true] {
            let operation=if maximum_parameters {maximum(name)} else {effect(name,&[])};
            let document=filtered(original.clone(),operation,None);
            for repetition in 0..4 {
                let start=std::time::Instant::now();
                let mut reader=gpu.capture_scene(document.snapshot(),SceneScope::All,Default::default()).unwrap();
                let setup=start.elapsed();let output=reader.read_region([0,0,512,512]).unwrap();let complete=start.elapsed();
                assert_eq!(output.len(),512*512);
                eprintln!("ARTISTIC_CELLS_CAPTURE name={name} maximum={maximum_parameters} repetition={repetition} capture_setup_us={} completed_with_readback_us={}",setup.as_micros(),complete.as_micros());
            }
        }
    }
}

fn hash(mut value:u32)->u32 {
    value=value.wrapping_mul(747796405).wrapping_add(2891336453);
    value=((value>>((value>>28)+4))^value).wrapping_mul(277803737);(value>>22)^value
}
fn pair(x:i32,y:i32,seed:u32)->[f64;2] {
    let key=hash(x as u32^hash(y as u32^hash(seed)));
    [0x243f6a88,0x85a308d3].map(|salt|f64::from(hash(key^salt)>>8)/16777216.)
}
#[test]
fn artistic_cells_crystallize_matches_independent_seven_by_seven_native_donor_oracle() {
    let extent=[269,271];let mut document=empty_document(extent,DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::F32});
    let image=crate::test_support::depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,8<<20,|x,y|[x as f32/269.,y as f32/271.,(x+269*y) as f32/72899.,1.]);
    insert_source(&mut document,"Unique donor addresses",image);
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let gpu=r.snapshot_gpu();let input=read(&gpu,&document);
    for stretch in [-100f32,0.,100.] {
        let sides=[7.*(1.+f64::from(stretch.max(0.))/100.),7.*(1.+f64::from((-stretch).max(0.))/100.)];
        let output=read(&gpu,&filtered(document.clone(),effect("crystallize",&[("cell_size",7.),("stretch",stretch),("seed",65535.)]),None));
        for y in (40..230).step_by(11) {for x in (40..230).step_by(13) {
            let query=[(x as f64+0.5)/sides[0],(y as f64+0.5)/sides[1]];
            let base=query.map(|v|v.floor() as i32);let mut nearest=f64::INFINITY;let mut donor=[0.;2];
            for sy in base[1]-3..=base[1]+3 {for sx in base[0]-3..=base[0]+3 {
                let site=pair(sx,sy,65535);let position=[f64::from(sx)+0.25+site[0]*0.5,f64::from(sy)+0.25+site[1]*0.5];
                let distance=(position[0]-query[0]).powi(2)+(position[1]-query[1]).powi(2);
                if distance<nearest {nearest=distance;donor=[position[0]*sides[0],position[1]*sides[1]];}
            }}
            let sx=donor[0].floor() as usize;let sy=donor[1].floor() as usize;
            close(&[output[y*269+x]],&[input[sy*269+sx]],&format!("Voronoi stretch{stretch} query{x},{y} donor{sx},{sy}"));
        }}
    }
}

#[test]
#[ignore]
fn artistic_cells_capture_review_images() {
    let directory=std::path::PathBuf::from(std::env::var_os("CAPY_ARTISTIC_CELLS_CAPTURE_DIR").unwrap());
    std::fs::create_dir_all(&directory).unwrap();let extent=[1024,512];
    let mut document=empty_document(extent,DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::F32});
    let image=crate::test_support::depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,32<<20,|x,y| {
        let u=x as f32/1024.;let v=y as f32/512.;let circle=(x as f32-730.).hypot(y as f32-260.);
        let mut color=if x<340 {if x<169 {[0.9,0.025,0.03,1.]} else {[0.025,0.1,0.85,1.]}}
            else if x<640 {[u,v,0.1+0.5*(u*8.).sin().abs(),1.]}
            else if circle<130. {[0.8,0.25+0.5*v,0.035,1.]} else {[0.03,0.08+0.3*v,0.15+0.2*u,1.]};
        if x==169||y==255 {color=[0.05,0.85,0.12,1.];}
        if x>640 && y>340 && ((x+y*2)/23)%3==0 {color=[0.85,0.08,0.2,0.7];}
        color
    });
    insert_source(&mut document,"Synthetic artistic review",image);
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let gpu=r.snapshot_gpu();
    let mut cases=vec![("original",read(&gpu,&document))];
    for name in ["dither_spread","scatter","crystallize"] {cases.push((name,read(&gpu,&filtered(document.clone(),effect(name,&[]),None))));}
    let encode=|pixels:&[[f32;4]]|->Vec<u8> {pixels.iter().flat_map(|p| {
        let alpha=p[3].clamp(0.,1.);std::array::from_fn::<_,3,_>(|c|(RgbSpace::Srgb.encode(f64::from(p[c]+0.35*(1.-alpha))).clamp(0.,1.)*255.).round() as u8)
    }).collect()};
    let save=|name:&str,size:[u32;2],bytes:&[u8]| {
        let file=std::fs::File::create(directory.join(format!("{name}.png"))).unwrap();let mut encoder=png::Encoder::new(file,size[0],size[1]);
        encoder.set_color(png::ColorType::Rgb);encoder.set_depth(png::BitDepth::Eight);encoder.write_header().unwrap().write_image_data(bytes).unwrap();
    };
    for (name,pixels) in &cases {save(name,extent,&encode(pixels));}
    let mut sheet=Vec::new();
    for y in 0..256 {for (_,pixels) in &cases {for x in 0..512 {sheet.extend(encode(&[pixels[(2*y*1024+2*x) as usize]]));}}}
    save("contact-sheet-original-dither-scatter-crystallize",[2048,256],&sheet);
}

#[test]
#[ignore]
fn artistic_cells_live_parameter_frame_diagnostics() {
    use layer_render::CanvasRenderer;
    for name in ["dither_spread","scatter","crystallize"] {
        for max_parameters in [false,true] {
            let operation=if max_parameters {maximum(name)} else {effect(name,&[])};
            let configured=filtered(source([512;2],SampleDepth::F32),operation,None);
            for enabled in [false,true] {
            let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            r.set_telemetry_enabled(true);
            eprintln!("ARTISTIC_CELLS_LIVE_ADAPTER name={name} maximum={max_parameters} enabled={enabled} {:?}",r.adapter().get_info());
            let mut document=configured.clone();
            let occurrence=document.artwork.occurrences.iter().find(|(_,_,o)|matches!(o.content,OccurrenceContent::Effect(_))).unwrap().0;
            document.artwork.occurrences.get_mut(occurrence).unwrap().visible=enabled;refresh(&mut document);
            let start=std::time::Instant::now();
            crate::test_support::submit_settled(&mut r,layer_render::FramePacket {reset_layers:true,composite_all:true,
                ..crate::test_support::packet(document.scene(),[512;2])});
            r.wait_idle().unwrap();
            eprintln!("ARTISTIC_CELLS_PREPARE_FIRST name={name} maximum={max_parameters} enabled={enabled} completed_us={}",start.elapsed().as_micros());
            let handle=document.artwork.effects.iter().next().unwrap().0;
            for frame in 0..4 {
                let application=document.artwork.effects.get_mut(handle).unwrap();
                let mut updated=EffectInstance {program:application.program.clone(),values:application.values.clone()};
                let parameter=if name=="crystallize" {"stretch"} else {"density"};
                updated.set(parameter,EffectValue::Number(if frame%2==0 {99.}else{100.})).unwrap();application.values=updated.values;
                document.revision+=1;refresh(&mut document);
                let before=r.metrics();let gpu_before=r.telemetry.completed_snapshot(&r.device,&r.queue).gpu.count;
                let start=std::time::Instant::now();
                crate::test_support::submit_settled(&mut r,layer_render::FramePacket {composite_all:true,..crate::test_support::packet(document.scene(),[512;2])});
                let cpu=start.elapsed();r.wait_idle().unwrap();let completed=start.elapsed();
                assert!(r.metrics().command_passes>before.command_passes,"a parameter observation must execute rendering work");
                let telemetry=r.telemetry.completed_snapshot(&r.device,&r.queue);
                let samples=telemetry.gpu.ordered();let gpu_submissions=(telemetry.gpu.count-gpu_before) as usize;
                assert!(gpu_submissions>0 && gpu_submissions<=samples.len(),"every measured boundary must contain completed GPU timestamps");
                let gpu: f32=samples.iter().rev().take(gpu_submissions).sum();
                eprintln!("ARTISTIC_CELLS_LIVE name={name} maximum={max_parameters} enabled={enabled} frame={frame} settled_work_us={} completed_us={} gpu_sum_ms={gpu} gpu_submissions={gpu_submissions} command_passes={}",cpu.as_micros(),completed.as_micros(),r.metrics().command_passes-before.command_passes);
            }
            }
        }
    }
}

#[test]
fn artistic_cells_transformed_fractional_reference_and_huge_finite_uniform_paint() {
    let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();let gpu=r.snapshot_gpu();
    let original=source([269,139],SampleDepth::F32);let input=read(&gpu,&original);
    for name in ["dither_spread","scatter","crystallize"] {
        let mut document=filtered(original.clone(),effect(name,&[]),None);
        let handle=document.artwork.effects.iter().next().unwrap().0;
        document.artwork.effects.get_mut(handle).unwrap().spatial=Some(layer_core::authored::EffectSpatialReference {
            mapping:layer_core::authored::Affine64([1.25,0.2,-0.15,0.8,40.25,15.75]),extent:[269.,139.],
        });refresh(&mut document);
        let output=read(&gpu,&document);palette(&input,&output,&format!("{name} fractional reference and negative coordinates"));
        if name!="crystallize" {
            for (key,value) in [("density",37.),("density",0.),("spread",0.)] {
                let mut probe=document.clone();let application=probe.artwork.effects.get_mut(handle).unwrap();
                let mut updated=EffectInstance {program:application.program.clone(),values:application.values.clone()};
                updated.set(key,EffectValue::Number(value)).unwrap();application.values=updated.values;refresh(&mut probe);
                let pixels=read(&gpu,&probe);palette(&input,&pixels,&format!("{name} transformed {key}={value}"));
                if value==0. {close(&pixels,&input,&format!("{name} transformed identity {key}"));}
            }
        }
        let region=[251,5,18,129];let cropped=gpu.capture_scene(document.snapshot(),SceneScope::All,Default::default()).unwrap().read_region(region).unwrap();
        let expected:Vec<_>=(5..134).flat_map(|y|(251..269).map(move|x|(y*269+x) as usize)).map(|i|output[i]).collect();
        close(&cropped,&expected,&format!("{name} transformed capture phase"));
    }
    for alpha in [1e-8,1e-30] {
        let mut document=empty_document([17,13],DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::F32});
        let image=crate::test_support::depth_source([17,13],SampleDepth::F32,RgbSpace::Srgb,1<<20,|_,_|[1e30,-1e29,2e30,alpha]);
        insert_source(&mut document,"Huge finite low alpha uniform",image);let input=read(&gpu,&document);
        assert!(input.iter().flatten().all(|v|v.is_finite()));
        for name in ["dither_spread","scatter","crystallize"] {
            close(&read(&gpu,&filtered(document.clone(),maximum(name),None)),&input,&format!("{name} huge finite uniform alpha{alpha}"));
        }
    }
}
