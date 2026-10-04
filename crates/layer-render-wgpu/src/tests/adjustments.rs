use super::*;
use layer_core::EffectInstance;
use super::native_effects::{empty_document,insert_effect,mask,refresh};
use layer_core::authored::{PortableId,Occurrence,OccurrenceContent,Stack,PaintSource};

// Keep the established ten-filter baseline stable as the catalog grows.
fn pointwise_baseline() -> [&'static layer_core::EffectDefinition; 10] {
    [
        fixture("curves"),
        fixture("levels"),
        fixture("brightness_contrast"),
        fixture("hue_saturation"),
        fixture("color_balance"),
        fixture("exposure"),
        fixture("vibrance"),
        fixture("black_white"),
        fixture("gradient_map"),
        fixture("posterize"),
    ]
}

#[test]
fn all_effects_incremental_masks_groups_and_clipping_match_full_recomposition() {
    let mut r =
        WgpuRasterizer::new_native_headless(Default::default()).expect("physical GPU required");
    let extent=[333,291];
    let mut document=empty_document(extent,Default::default());
    for (i,kind) in pointwise_baseline().into_iter().enumerate() {
        let h=insert_effect(&mut document,EffectInstance::new(kind.program()));
        document.artwork.occurrences.get_mut(h).unwrap().opacity=0.7;
        if i%2==0 {mask(&mut document,h,0.4);}
    }
    let source=document.artwork.paint.insert(PortableId::random(),PaintSource {domain:extent,original:None,raster:Default::default(),operations:Default::default()}).unwrap();
    let base=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(source),"Translucent paint")).unwrap();
    let root=document.composition().result;
    document.artwork.stacks.get_mut(root).unwrap().entries.push(base);
    for &h in document.artwork.stacks.get(root).unwrap().entries.iter().filter(|&&h|h!=base) { document.artwork.occurrences.get_mut(h).unwrap().attachment=layer_core::Attachment::Effect; }
    let children=std::mem::take(&mut document.artwork.stacks.get_mut(root).unwrap().entries);
    let stack=document.artwork.stacks.insert(PortableId::random(),Stack {entries:children}).unwrap();
    let group=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Stack(stack),"Isolated group")).unwrap();
    document.artwork.stacks.get_mut(root).unwrap().entries.push(group);refresh(&mut document);
    let mut view = test_view();
    view.width_px = 333;
    view.height_px = 291;
    let mut dab = test_dab([255., 150.], [0.8, 0.2, 0.1, 0.65], 1.);
    dab.radii = [45.; 2];
    let batch = crate::test_support::dab_batch(
        layer_core::authored::SourceTarget::Paint(source),
        test_style(BrushExecution::Dry),
        Rect { min: Point { x: 209., y: 104. }, max: Point { x: 301., y: 196. } },
    );
    let render = |r: &mut WgpuRasterizer, all, dabs: &[Dab], batches: &[DabBatch]| {
        r.submit(FramePacket {
            view,
            dabs,
            dab_batches: batches,
            composite_all: all,
            ..packet(document.scene(), [333, 291])
        })
        .unwrap();
        r.readback_srgb_rgba8().unwrap()
    };
    render(&mut r, true, &[], &[]);
    let incremental = render(&mut r, false, &[dab], &[batch]);
    let full = render(&mut r, true, &[], &[]);
    assert_eq!(
        incremental, full,
        "damage crossing a tile boundary must match full composite"
    );
    let p = &full[(150 * 333 + 255) * 4..][..4];
    assert!(
        (160..=170).contains(&p[3]),
        "adjustments and masks preserve base alpha: {p:?}"
    );
    assert_eq!(
        &full[..4],
        &[0; 4],
        "pointwise effects preserve empty owner coverage"
    );
}

#[test]
fn clipping_filter_drops_render_in_owner_order_through_undo_and_reopen() {
    use layer_core::{Attachment,Document,EffectValue,OccurrenceDropPosition};
    use layer_core::color::{DocumentColor,SampleDepth,RgbSpace};
    use super::native_effects::{insert_source,set_effect,roundtrip};
    let extent=[32;2];let color=DocumentColor{depth:SampleDepth::F32,..Default::default()};
    let source=|rgba|crate::test_support::depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,1024*1024,|_,_|rgba);
    let mut doc=empty_document(extent,color);
    let top=insert_source(&mut doc,"Top",source([0.;4]));
    let member=insert_source(&mut doc,"Member",source([0.2,0.2,0.2,1.]));
    let base=insert_source(&mut doc,"Base",source([1.,0.,0.,0.25]));
    let gain=insert_effect(&mut doc,EffectInstance::new(fixture("exposure").program()));
    let offset=insert_effect(&mut doc,EffectInstance::new(fixture("exposure").program()));
    set_effect(&mut doc,gain,"exposure",EffectValue::Number(1.));
    set_effect(&mut doc,offset,"offset",EffectValue::Number(0.1));mask(&mut doc,gain,0.5);
    for id in [top,member] {doc.artwork.occurrences.get_mut(id).unwrap().attachment=Attachment::Clip;}
    refresh(&mut doc);
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();
    let render=|r:&mut WgpuRasterizer,doc:&Document,reset| {
        r.submit(FramePacket{reset_layers:reset,composite_all:false,..packet(doc.scene(),extent)}).unwrap();
        let bytes=crate::layer_tests::page_bytes(r,crate::test_support::document_texture(r));
        std::array::from_fn::<_,4,_>(|c|f32::from_le_bytes(bytes[c*4..c*4+4].try_into().unwrap()))
    };
    let check=|pixel:[f32;4],gray:f32| {
        for value in &pixel[..3] {assert!((*value-gray*0.25).abs()<1e-6,"{pixel:?}, expected gray {gray}");}
        assert_eq!(pixel[3],0.25);
    };
    check(render(&mut r,&doc,true),0.2);
    let undo=doc.apply(doc.drop_layers_edit(&[offset,gain],member,OccurrenceDropPosition::Above).unwrap().edit).unwrap();
    assert_eq!(doc.scene().attached_effects(member),[offset,gain]);assert_eq!(doc.scene().clipping_base(member),Some(base));
    check(render(&mut r,&doc,false),0.45);
    let reorder=doc.apply(doc.drop_occurrence_edit(gain,member,OccurrenceDropPosition::Above).unwrap().edit).unwrap();
    check(render(&mut r,&doc,false),0.4);doc.apply(reorder).unwrap();check(render(&mut r,&doc,false),0.45);
    let redo=doc.apply(undo).unwrap();check(render(&mut r,&doc,false),0.2);
    doc.apply(redo).unwrap();check(render(&mut r,&doc,false),0.45);
    check(render(&mut r,&roundtrip(doc),true),0.45);
}

#[test]
fn attached_spatial_effects_expand_owner_alpha_before_common_base_clipping() {
    use layer_core::{Attachment, EffectAlpha, EffectPass, EffectSampling};
    use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
    use super::native_effects::insert_source;
    let extent = [128;2];
    let color = DocumentColor { depth: SampleDepth::F32, ..Default::default() };
    let source = |color: [f32;4], half: bool| crate::test_support::depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,16*1024*1024,
        |x,_|if half && x>=64 {[0.;4]}else{color});
    let mut program = (*fixture("exposure").program()).clone();
    program.alpha = EffectAlpha::Filter;
    program.entry = "owner_average".into();
    program.wgsl = "fn owner_average(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p-vec2<f32>(1.,0.))+fx_sample(p+vec2<f32>(1.,0.)))*.5;}".into();
    program.passes = vec![EffectPass { entry: program.entry.clone(), sampling: EffectSampling::Neighborhood { radius:1 } }].into();
    let mut document=empty_document(extent,color);
    let base=insert_source(&mut document,"Base",source([1.,0.,0.,1.],true));
    let blur=insert_effect(&mut document,EffectInstance::new(Arc::new(program.clone())));
    let paint=insert_source(&mut document,"Paint",source([0.,0.,1.,1.],false));
    document.artwork.stacks.get_mut(document.composition().result).unwrap().entries=vec![paint,blur,base];
    document.artwork.occurrences.get_mut(paint).unwrap().attachment=Attachment::Clip;
    document.artwork.occurrences.get_mut(blur).unwrap().attachment=Attachment::Effect;
    refresh(&mut document);
    let mut r=WgpuRasterizer::new_native_headless(color).unwrap();
    let mut frame=packet(document.scene(),extent);frame.composite_all=false;
    r.submit(frame).unwrap();
    let pixels=crate::layer_tests::page_bytes(&r,crate::test_support::document_texture(&r));
    let pixel=|bytes:&[u8],x:usize|std::array::from_fn::<_,4,_>(|c|f32::from_le_bytes(bytes[(64*128+x)*16+c*4..][..4].try_into().unwrap()));
    assert_eq!(pixel(&pixels,64),[0.,0.,0.5,0.5]);
    assert_eq!(pixel(&pixels,65),[0.;4]);
    let work=r.scene.as_ref().unwrap().image_work()[1];
    let ink=crate::tests::test_dab([64.,64.],[0.,1.,0.,1.],1.);
    let target=document.scene().source_target(paint).unwrap();
    let batch=crate::test_support::dab_batch(target,test_style(BrushExecution::Dry),ink.bounds());
    r.submit(FramePacket {dabs:&[ink],dab_batches:&[batch],..frame}).unwrap();
    assert_eq!(r.scene.as_ref().unwrap().image_work()[1],work,"painting above a filtered base reuses its spatial output");
    let edited=crate::layer_tests::page_bytes(&r,crate::test_support::document_texture(&r));
    assert_eq!(pixel(&edited,64),[0.,0.5,0.,0.5]);
    document.artwork.occurrences.get_mut(base).unwrap().opacity=0.5;
    refresh(&mut document);
    r.submit(FramePacket {composite_all:true,..packet(document.scene(),extent)}).unwrap();
    assert_eq!(r.scene.as_ref().unwrap().image_work()[1],work,"owner contribution opacity does not change local filter input");
    let faded=crate::layer_tests::page_bytes(&r,crate::test_support::document_texture(&r));
    assert_eq!(pixel(&faded,64),[0.,0.25,0.,0.25]);
    for opacity in [0.,0.5,1.] {
        document.artwork.occurrences.get_mut(blur).unwrap().opacity=opacity;
        document.artwork.occurrences.get_mut(base).unwrap().opacity=1.;
        refresh(&mut document);
        r.submit(packet(document.scene(),extent)).unwrap();
        let result=crate::layer_tests::page_bytes(&r,crate::test_support::document_texture(&r));
        let expected=[0.,0.5*opacity,0.,0.5*opacity];
        assert_eq!(pixel(&result,64),expected,"effect strength interpolates alpha");
    }
    let mut member=empty_document(extent,color);
    let base=insert_source(&mut member,"Base",source([1.,0.,0.,0.25],false));
    let shadows=insert_source(&mut member,"Shadows",source([0.,0.,1.,1.],true));
    let blur=insert_effect(&mut member,EffectInstance::new(Arc::new(program)));
    member.artwork.stacks.get_mut(member.composition().result).unwrap().entries=vec![blur,shadows,base];
    member.artwork.occurrences.get_mut(shadows).unwrap().attachment=Attachment::Clip;
    member.artwork.occurrences.get_mut(blur).unwrap().attachment=Attachment::Effect;
    refresh(&mut member);
    r.submit(FramePacket {reset_layers:true,..packet(member.scene(),extent)}).unwrap();
    let result=crate::layer_tests::page_bytes(&r,crate::test_support::document_texture(&r));
    assert_eq!(pixel(&result,64),[0.125,0.,0.125,0.25],"member effects process only the member before outer clipping");
    let work=r.scene.as_ref().unwrap().image_work()[1];
    member.artwork.occurrences.get_mut(shadows).unwrap().attachment=Attachment::None;
    refresh(&mut member);
    r.submit(packet(member.scene(),extent)).unwrap();
    assert_eq!(r.scene.as_ref().unwrap().image_work()[1],work,"owner clipping does not change local filter input");
    let result=crate::layer_tests::page_bytes(&r,crate::test_support::document_texture(&r));
    assert_eq!(pixel(&result,64),[0.125,0.,0.5,0.625]);
    member.artwork.occurrences.get_mut(shadows).unwrap().attachment=Attachment::Clip;
    refresh(&mut member);
    let request=layer_core::ArtworkSampleRequest::new(&member,layer_core::ArtworkSource::EffectInput(blur),[63.5,64.5],1);
    let sample=pollster::block_on(r.snapshot_gpu().artwork_sample(request,Default::default())).unwrap();
    assert_eq!(sample,layer_core::ArtworkSample::Color([0.,0.,1.,1.]),"exact effect input excludes the base and outer clipping");

}

#[test]
fn independent_owner_counts_and_local_chain_lengths_bound_resident_work() {
    use layer_core::{Attachment,EffectAlpha,EffectPass,EffectSampling};
    use layer_core::color::{DocumentColor,SampleDepth,RgbSpace};
    use layer_core::authored::{Definition,EffectApplication,SourceTarget};
    let extent=[32;2];
    let color=DocumentColor {depth:SampleDepth::F32,..Default::default()};
    let backing=crate::test_support::depth_source(extent,SampleDepth::F32,RgbSpace::Srgb,1024*1024,|_,_|[0.02,0.04,0.06,0.1]);
    let mut program=(*fixture("exposure").program()).clone();
    program.alpha=EffectAlpha::Filter;program.entry="resident_average".into();
    program.wgsl="fn resident_average(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p-vec2<f32>(1.,0.))+fx_sample(p+vec2<f32>(1.,0.)))*.5;}".into();
    program.passes=vec![EffectPass {entry:program.entry.clone(),sampling:EffectSampling::Neighborhood {radius:1}}].into();
    let program=Arc::new(program);
    for (owners,length) in [(1,1),(10,1),(100,1),(10,2),(10,4)] {
        let mut document=empty_document(extent,color);
        let definition=document.artwork.definitions.insert(PortableId::random(),Definition {program:program.clone()}).unwrap();
        let root=document.composition().result;
        let mut entries=Vec::new();let mut groups=Vec::new();let mut paints=Vec::new();
        for _ in 0..owners {
            let paint=document.artwork.paint.insert(PortableId::random(),PaintSource {domain:extent,original:Some(backing.clone()),raster:Default::default(),operations:Default::default()}).unwrap();
            let content=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(paint),"content")).unwrap();
            let stack=document.artwork.stacks.insert(PortableId::random(),Stack {entries:vec![content]}).unwrap();
            let group=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Stack(stack),"owner")).unwrap();
            let mut chain=Vec::new();
            for _ in 0..length {
                let application=document.artwork.effects.insert(PortableId::random(),EffectApplication {definition,values:EffectInstance::new(program.clone()).values}).unwrap();
                let mut occurrence=Occurrence::new(OccurrenceContent::Effect(application),"average");occurrence.attachment=Attachment::Effect;
                chain.push(document.artwork.occurrences.insert(PortableId::random(),occurrence).unwrap());
            }
            entries.extend(chain.into_iter().rev());entries.push(group);groups.push(group);paints.push(paint);
        }
        document.artwork.stacks.get_mut(root).unwrap().entries=entries;refresh(&mut document);
        let mut r=WgpuRasterizer::new_native_headless(color).unwrap();
        let mut full=WgpuRasterizer::new_native_headless(color).unwrap();
        fn frame(document:&layer_core::Document)->FramePacket<'_> {FramePacket {blend_space:layer_core::BlendSpace::Linear,composite_all:false,..packet(document.scene(),document.composition().size)}}
        for renderer in [&mut r,&mut full] {renderer.submit(frame(&document)).unwrap();}
        let work=r.scene.as_ref().unwrap().image_work();let composed=r.metrics.composited_pixels;
        r.submit(frame(&document)).unwrap();
        document.working.occurrence=Some(*groups.last().unwrap());document.working.target=None;
        r.submit(frame(&document)).unwrap();
        document.artwork.occurrences.get_mut(groups[0]).unwrap().name="renamed".into();refresh(&mut document);
        r.submit(frame(&document)).unwrap();
        assert_eq!(r.scene.as_ref().unwrap().image_work(),work,"presentation changes dispatch no effects");
        assert_eq!(r.metrics.composited_pixels,composed,"presentation changes do not recompose artwork");
        let pixels=r.scene.as_ref().unwrap().image_pass_pixels();
        let mut dab=test_dab([16.;2],[0.9,0.1,0.3,1.],1.);dab.radii=[64.;2];
        let batch=crate::test_support::dab_batch(SourceTarget::Paint(paints[0]),test_style(BrushExecution::Dry),dab.bounds());
        for renderer in [&mut r,&mut full] {renderer.submit(FramePacket {dabs:&[dab],dab_batches:std::slice::from_ref(&batch),..frame(&document)}).unwrap();}
        let after=r.scene.as_ref().unwrap().image_work();
        assert_eq!([after[0]-work[0],after[1]-work[1]],[1,length],"only the edited owner chain executes: {owners} owners, {length} effects");
        assert_eq!(r.scene.as_ref().unwrap().image_pass_pixels()-pixels,length*32*32);
        assert_eq!(r.metrics.composited_pixels-composed,32*32);
        assert_eq!(r.metrics.frame_composited_pages,vec![(0,[0,0])]);
        let incremental=crate::layer_tests::page_bytes(&r,crate::test_support::document_texture(&r));
        full.scene=None;
        full.submit(FramePacket {composite_all:true,..frame(&document)}).unwrap();
        assert_eq!(incremental,crate::layer_tests::page_bytes(&full,crate::test_support::document_texture(&full)),"resident incremental output matches independently rebuilt composition");
    }
}
