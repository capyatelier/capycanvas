use super::*;

fn fixture(name:&str)->(Document,OccurrenceHandle,OccurrenceHandle,OccurrenceHandle) {
    let extent=[517,259];
    let mut doc=document_at(extent);
    paint_mut(&mut doc,0).base = (Some(rgba8_source(extent,|x,y| {
        let v=if (x/3+y/2)%2==0 {126} else {130};
        [v,(x*17) as u8,(y*23) as u8,if (x/7+y/11)%3==0 {0}else{97+(x%159) as u8}]
    }))).map(|source|layer_core::authored::PaintBase::new(source.into()));
    let owner=doc.scene().order()[0];
    let filter=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture(name).program()),name);
    doc.artwork.occurrences.get_mut(filter).unwrap().attachment=Attachment::Effect;
    insert_occurrence(&mut doc,filter,0);
    let photo=paint_occurrence(&mut doc,"photo",Some(rgba8_source(extent,|x,y|[(x*29) as u8,(y*31) as u8,173,255])));
    insert_occurrence(&mut doc,photo,2);
    (doc,owner,filter,photo)
}

fn capture(r:&mut WgpuRasterizer,frame:FramePacket<'_>,handles:[OccurrenceHandle;3],tile:[u32;2],eligible:bool,colors:Option<[[f32;4];2]>)->(Vec<[f32;4]>,[u64;2]) {
    let [owner,filter,photo]=handles;
    let mut scene=r.scene.take().unwrap_or_else(||Scene::new(r));
    scene.begin_frame();scene.used.fill(false);scene.jobs.clear();scene.source_jobs.clear();
    let (front,back)=if let Some([ink,photo])=colors {
        let color=|[red,green,blue,alpha]:[f32;4]|wgpu::Color {r:f64::from(red),g:f64::from(green),b:f64::from(blue),a:f64::from(alpha)};
        let back=scene.alloc(r,color(photo));
        (scene.alloc(r,color(ink)),back)
    } else {
        let back=scene.layer(r,frame,photo,tile).unwrap();
        (scene.layer(r,frame,owner,tile).unwrap(),back)
    };
    let front=scene.effect(r,frame,&[filter],tile,front).unwrap();
    if !eligible {
        let Job::Effect {prepared,..}=scene.jobs.last_mut().unwrap() else {panic!("final effect")};
        prepared.original_independent=false;
    }
    let output=scene.combine(r,front,back,frame.scene.occurrence(owner).unwrap().opacity,
        frame.scene.occurrence(owner).unwrap().blend,false,frame.blend_space);
    let work=scene.jobs.iter().filter_map(|job|match job {
        Job::Draw {data,..} if data[8]==4.=>Some((1,(data[2]*data[3]) as u64)),_=>None,
    }).fold([0,0],|[jobs,pixels],(n,area)|[jobs+n,pixels+area]);
    let (texture,view)=create_color_target(&r.device,[PAGE_SIZE;2],"native composite oracle");
    let region=page_rect(tile).intersect(PixelRect::full(frame.document_extent));
    let destination=Image {texture:texture.clone(),view,plan:display_mips::Plan::window(frame.document_extent,0,region)};
    scene.copy_window_tile(output,&destination,tile);
    let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
    scene.encode_jobs(r,&mut encoder).unwrap();r.uploads.finish(&encoder);encoder.submit(&r.queue);
    r.scene=Some(scene);
    (pixels(r,&texture),work)
}

#[test]
fn native_pixel_backdrop_fusion_removes_intermediate_and_preserves_float_pixels() {
    for name in ["threshold","brightness_to_opacity"] {for blend in layer_core::BlendSpace::ALL {
        let (mut doc,owner,filter,photo)=fixture(name);
        doc.artwork.occurrences.get_mut(filter).unwrap().opacity=0.79;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        for opacity in [0.,0.61,1.] {
            doc.artwork.occurrences.get_mut(owner).unwrap().opacity=opacity;
            let mut frame=packet(doc.scene(),doc.composition().size);frame.blend_space=blend;
            r.submit(frame).unwrap();
            for tile in [[0,0],[2,1]] {
                let (fused,work)=capture(&mut r,frame,[owner,filter,photo],tile,true,None);
                let (unfused,reference_work)=capture(&mut r,frame,[owner,filter,photo],tile,false,None);
                assert_eq!(work,[0,0],"{name} {blend:?} opacity={opacity} tile={tile:?}: intermediate composite must disappear");
                assert_eq!(reference_work,[1,u64::from(PAGE_SIZE).pow(2)]);
                for (i,(actual,expected)) in fused.iter().zip(&unfused).enumerate() {
                    assert_eq!(actual.map(f32::to_bits),expected.map(f32::to_bits),"{name} {blend:?} opacity={opacity} tile={tile:?} pixel={i}: {actual:?} != {expected:?}");
                }
            }
        }
    }}
}

#[test]
fn native_pixel_backdrop_fusion_keeps_masks_and_compact_contributions_separate() {
    for name in ["threshold","brightness_to_opacity"] {for blend in layer_core::BlendSpace::ALL {
        let (mut doc,owner,filter,photo)=fixture(name);
        coverage_mask(&mut doc,filter,Default::default(),Some(layer_core::Selection::polygon(vec![
            layer_core::Point{x:11.,y:7.},layer_core::Point{x:223.,y:17.},layer_core::Point{x:127.,y:243.},
        ]).unwrap()));
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=packet(doc.scene(),doc.composition().size);frame.blend_space=blend;
        r.submit(frame).unwrap();
        let (actual,work)=capture(&mut r,frame,[owner,filter,photo],[0,0],true,None);
        let (reference,reference_work)=capture(&mut r,frame,[owner,filter,photo],[0,0],false,None);
        assert_eq!(work,reference_work);assert_eq!(work[0],1);assert_eq!(actual,reference);
        doc.artwork.occurrences.get_mut(filter).unwrap().mask=None;
        let dab=crate::tests::test_dab([125.5,126.5],[0.02,0.07,0.15,0.63],0.9);
        let target=doc.scene().source_target(owner).unwrap();
        let mut preview=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
        preview.kind=DabBatchKind::Preview;preview.stroke_end=false;preview.style.blend_space=blend;
        let mut frame=packet(doc.scene(),doc.composition().size);frame.blend_space=blend;
        frame.view.document_to_surface=[0.25,0.,0.,0.25,0.,0.];
        r.submit(frame).unwrap();
        r.submit(FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&preview),..frame}).unwrap();
        assert!(r.preview_contribution && r.preview_level>0);
        let (actual,work)=capture(&mut r,frame,[owner,filter,photo],[0,0],true,None);
        let (reference,reference_work)=capture(&mut r,frame,[owner,filter,photo],[0,0],false,None);
        assert_eq!(work,reference_work);assert_eq!(work[0],1);assert_eq!(actual,reference);
    }}
}

#[test]
fn native_pixel_backdrop_fusion_preserves_tiny_alpha_and_extended_values() {
    for name in ["threshold","brightness_to_opacity"] {for blend in layer_core::BlendSpace::ALL {
        let (mut doc,owner,filter,photo)=fixture(name);
        composition_mut(&mut doc).color.depth=SampleDepth::F32;
        doc.artwork.occurrences.get_mut(owner).unwrap().opacity=0.61;
        doc.artwork.occurrences.get_mut(filter).unwrap().opacity=0.79;
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=packet(doc.scene(),doc.composition().size);frame.blend_space=blend;
        for alpha in [0.,1e-30,1e-12,0.37,1.] {for rgb in [[0.2,0.7,0.9],[-2.,3.,1e5]] {for backdrop_alpha in [0.,0.53,1.] {
            let colors=Some([[rgb[0]*alpha,rgb[1]*alpha,rgb[2]*alpha,alpha],
                [0.17*backdrop_alpha,0.31*backdrop_alpha,0.73*backdrop_alpha,backdrop_alpha]]);
            let (actual,work)=capture(&mut r,frame,[owner,filter,photo],[0,0],true,colors);
            let (expected,reference_work)=capture(&mut r,frame,[owner,filter,photo],[0,0],false,colors);
            assert_eq!(work,[0,0]);assert_eq!(reference_work[0],1);
            assert_eq!(actual[0].map(f32::to_bits),expected[0].map(f32::to_bits),
                "{name} {blend:?} alpha={alpha} rgb={rgb:?} backdrop_alpha={backdrop_alpha}: {:?} != {:?}",actual[0],expected[0]);
            assert!(actual.iter().all(|p|p.map(f32::to_bits)==actual[0].map(f32::to_bits)));
            assert!(expected.iter().all(|p|p.map(f32::to_bits)==expected[0].map(f32::to_bits)));
        }}}
    }}
}
