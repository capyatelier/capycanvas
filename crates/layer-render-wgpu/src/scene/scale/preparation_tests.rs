#![cfg(not(target_arch="wasm32"))]
use super::*;

fn gather(scene:&mut Scene,r:&WgpuRasterizer,doc:&Document,effect:OccurrenceHandle,destination:&wgpu::Texture)
    ->(std::sync::Weak<wgpu::Buffer>,crate::effects::Pipeline) {
    let input=scene.alloc(r,wgpu::Color {r:0.65,g:0.2,b:0.1,a:1.}).unwrap();
    let output=scene.effect(r,frame(doc,0),&[effect],[0;2],input,&mut Default::default()).unwrap();
    let (parameters,pipeline)=scene.jobs.iter().find_map(|job|match job {
        Job::Effect {prepared,..}=>Some((Arc::downgrade(&prepared._parameters),prepared.pipeline.clone())),
        _=>None,
    }).unwrap();
    scene.jobs.push(Job::Copy {source:scene.pool[output].texture.clone(),source_origin:[0;2],
        destination:destination.clone(),origin:[0;2],width:PAGE_SIZE,height:PAGE_SIZE});
    (parameters,pipeline)
}

fn prepare(r:&WgpuRasterizer,pipeline:&crate::effects::Pipeline) {
    match pipeline {
        crate::effects::Pipeline::Render(pipeline)=>pollster::block_on(r.compilation.prepare_all([pipeline])).unwrap(),
        crate::effects::Pipeline::Compute(pipeline)=>pollster::block_on(r.compilation.prepare_all([pipeline])).unwrap(),
    }
}

#[test]
fn a_retained_cold_lookup_does_not_block_an_independent_ready_unit() {
    let mut doc=document_at([PAGE_SIZE;2]);
    composition_mut(&mut doc).blend=layer_core::BlendSpace::Linear;
    let mut program=(*crate::tests::fixture("selective_color").program()).clone();
    program.id="independent_lookup".into();
    let mut lookups=program.lookups.to_vec();
    assert_eq!(lookups.len(),1);
    lookups[0].wgsl=format!("{}\n\n",lookups[0].wgsl.sources().unwrap().join("\n")).into();
    program.lookups=lookups.into();
    let mut instance=layer_core::EffectInstance::new(Arc::new(program));
    instance.set("reds_cyan",layer_core::EffectValue::Number(-30.)).unwrap();
    let a=effect_occurrence(&mut doc,instance,"Cold lookup");
    let mut instance=layer_core::EffectInstance::new(crate::tests::fixture("exposure").program());
    instance.set("exposure",layer_core::EffectValue::Number(0.5)).unwrap();
    let b=effect_occurrence(&mut doc,instance,"Ready exposure");
    let paint=doc.scene().order()[0];
    set_root_entries(&mut doc,vec![a,b,paint]);
    let reference=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut r=crate::test_support::staged_renderer(&reference,doc.composition().color);
    for renderer in [&r,&reference] {
        pollster::block_on(renderer.compilation.prepare_all([&renderer.scene_pipelines.constant.2,&renderer.scene_pipelines.fold.pipeline])).unwrap();
    }
    let mut scene=Scene::new(&r).unwrap();
    let image=||crate::test_support::page_texture(&r,r.device.working_format());
    let warm=image();let pending=image();let ready=image();let expected=image();
    let (_,pipeline)=gather(&mut scene,&r,&doc,b,&warm);prepare(&r,&pipeline);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    scene.encode_jobs(&mut r,&mut encoder).unwrap();
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    scene.discard_unencoded_jobs();
    let expected_b=pixels(&r,&warm);
    assert!(expected_b.iter().all(|pixel|pixel[3]==1. && pixel[0]>0.));
    let (parameters,pipeline)=gather(&mut scene,&r,&doc,a,&pending);prepare(&r,&pipeline);
    assert!(pipeline.ready());
    let compiler=r.startup.as_ref().unwrap().compiler.clone();
    let (entered,entry)=std::sync::mpsc::channel();
    let (release,held)=std::sync::mpsc::channel();
    compiler.enqueue(startup::BRUSH,move||{let _=entered.send(());let _=held.recv();Ok(())});
    entry.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    assert!(!scene.prepare_jobs(&mut r,&mut encoder,0,Class::Visible).unwrap(),"only A's lookup preparation shader must remain unavailable");
    assert_eq!(encoder.pass_count(),0);
    assert!(compiler.pending()>1,"A must queue its actual lookup behind the held compiler job");
    scene.discard_unencoded_jobs();
    assert!(parameters.upgrade().is_some(),"discarding A's job must retain its cached parameter Instance");
    let prepared=scene.effects.preparation_count();
    let (_,pipeline)=gather(&mut scene,&r,&doc,b,&ready);
    assert!(pipeline.ready());
    assert!(scene.prepare_jobs(&mut r,&mut encoder,0,Class::Visible).unwrap(),
        "an excluded unit's retained lookup preparation must not block B's ready jobs");
    scene.encode_jobs(&mut r,&mut encoder).unwrap();
    assert!(encoder.pass_count()>0);
    assert_eq!(scene.effects.preparation_count(),prepared,"B must not encode A's unready lookup");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    scene.discard_unencoded_jobs();
    assert_eq!(pixels(&r,&ready),expected_b);
    assert!(pixels(&r,&pending).iter().all(|pixel|*pixel==[0.;4]),"A's discarded unit must not publish pixels");
    release.send(()).unwrap();
    let (finished,done)=std::sync::mpsc::channel();
    compiler.enqueue(startup::DOCUMENT,move||{let _=finished.send(());Ok(())});
    done.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    gather(&mut scene,&r,&doc,a,&pending);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    scene.encode_jobs(&mut r,&mut encoder).unwrap();
    assert_eq!(scene.effects.preparation_count(),prepared+1,"A must prepare its retained lookup exactly once after release");
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let mut oracle=Scene::new(&reference).unwrap();
    gather(&mut oracle,&reference,&doc,a,&expected);pollster::block_on(oracle.effects.needs().prepare(&reference.compilation)).unwrap();
    let mut reference=reference;
    let mut encoder=submission::CommandEncoder::new(&reference.device,&Default::default());
    oracle.encode_jobs(&mut reference,&mut encoder).unwrap();
    reference.uploads.finish(&encoder);encoder.submit(&reference.queue);crate::test_support::complete(&reference);
    assert_eq!(pixels(&r,&pending),pixels(&reference,&expected));
}

#[test]
fn a_prepared_drawing_shows_its_first_stroke_on_the_first_admitted_frame_and_compiles_only_its_display_kernels() {
    for (case,depth,zoom,offset,layers,kind) in [
        ("half reduced preview",SampleDepth::U8,0.125,[0;2],1,DabBatchKind::Preview),
        ("half reduced commit",SampleDepth::U8,0.125,[0;2],1,DabBatchKind::Persistent),
        ("full reduced preview",SampleDepth::F32,0.125,[0;2],1,DabBatchKind::Preview),
        ("half full-size preview",SampleDepth::U8,1.,[0;2],1,DabBatchKind::Preview),
        ("half offset layer preview",SampleDepth::U8,0.125,[3,5],1,DabBatchKind::Preview),
        ("half pivot commit",SampleDepth::U8,0.25,[0;2],3,DabBatchKind::Persistent),
    ] {
        let mut doc=document_at([517,259]);
        composition_mut(&mut doc).color.depth=depth;
        let photo=doc.scene().order()[0];
        let mut entries:Vec<_>=(1..layers).map(|index| {
            let copy=copy_paint(&mut doc,0);
            doc.artwork.occurrences.get_mut(copy).unwrap().opacity=0.17+index as f32*0.02;
            copy
        }).collect();
        entries.push(photo);
        let stroked=(layers-1)/2;
        let ink=paint_occurrence(&mut doc,"Ink",None);
        doc.artwork.occurrences.get_mut(ink).unwrap().offset=offset;
        entries.insert(stroked,ink);set_root_entries(&mut doc,entries);
        let extent=doc.composition().size;
        let mut exact=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut r=crate::test_support::staged_renderer(&exact,doc.composition().color);
        exact.test.reference=true;
        r.shader_idle(false,false);
        r.prepare_startup(&doc,&layer_core::default_brush(DefaultBrushPreset::GPen),false).unwrap();
        crate::test_support::wait_startup(&mut r,std::time::Instant::now()+std::time::Duration::from_secs(60),|progress|progress.brush_ready,format_args!("{case}: startup"));
        let mut frame=FramePacket {blend_space:doc.composition().blend,..packet(doc.scene(),extent)};
        frame.view.document_to_surface=[zoom,0.,0.,zoom,0.,0.];
        crate::test_support::submit_settled(&mut r,frame);
        crate::test_support::submit_published(&mut exact,frame);
        let original=display_pixels(&r);
        let mut dab=crate::tests::test_dab([255.,129.],[0.9,0.02,0.1,0.7],1.);
        dab.radii=[45.;2];
        let style=layer_render::DabStyle {blend_space:doc.composition().blend,..crate::layer_tests::preset_style(DefaultBrushPreset::GPen)};
        let batch=DabBatch {kind,..dab_batch(source_at(&doc,stroked),style,dab.bounds())};
        let stroke=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),composite_all:false,..frame};
        let _held=r.compilation.batch();
        r.poll_pending(stroke.view).unwrap();
        crate::test_support::pace(&mut r);
        assert!(matches!(r.submit_when_ready(stroke,None),std::task::Poll::Ready(Ok(()))),"{case}: a prepared drawing admits its first stroke at once");
        let shown=display_pixels(&r);
        assert!(shown.iter().zip(&original).any(|(shown,original)|shown!=original),"{case}: the first admitted stroke frame shows ink");
        assert!(!publication_pending(&r),"{case}: the first admitted stroke frame presents the stroked artwork: {:?}",r.outstanding_terms());
        crate::test_support::submit_published(&mut exact,stroke);
        let cache=r.scale_display.as_ref().unwrap();
        let (plan,format)=(cache.plan,cache.texture().format());
        let error=quality(&shown,&pixels(&exact,crate::test_support::document_texture(&exact)),plan,format);
        assert!(error[0]<0.003,"{case}: the first stroke frame stays close to the exact reduction: {error:?}");
        crate::test_support::submit_settled(&mut r,FramePacket {composite_all:false,..frame});
        assert_eq!(crate::compile::tests::queued_recipes(&r.compilation),0,"{case}: the stroke and its refinement wait on no compilation");
        let half=format==wgpu::TextureFormat::Rgba16Float;
        assert_eq!(half,depth==SampleDepth::U8,"{case}");
        let scale=&r.scene_pipelines.scale;
        let compiled:Vec<_>=[("full",&scale.full),("half",&scale.half)].into_iter().flat_map(|(precision,kernels)|
            [("compose",&kernels.compose),("reduce",&kernels.reduce),("reduce_phased",&kernels.reduce_phased),("reduce_pair",&kernels.reduce_pair),("project",&kernels.project)]
                .into_iter().filter(|(_,kernel)|kernel.ready()).map(move|(name,_)|format!("{precision}.{name}"))).collect();
        let expected:&[&str]=if half {&["full.compose","full.reduce","full.reduce_phased","half.compose","half.reduce","half.reduce_pair","half.project"]}
            else {&["full.compose","full.reduce","full.reduce_phased","full.reduce_pair"]};
        assert_eq!(compiled,expected,"{case}: a display compiles the kernels it can run and no others");
    }
}

const FIRST_DAB:[f32;2]=[243.,129.];

#[derive(Clone,Copy,PartialEq,Debug)]
enum FirstContact {OnTop,Beneath,PhotoSelectedAtPenDown,PhotoSelectedAtPenDownAfterAStroke}

fn overlaid_photo(depth:SampleDepth,contact:FirstContact)->(Document,[OccurrenceHandle;3]) {
    let mut doc=document_at([517,259]);
    composition_mut(&mut doc).color.depth=depth;
    let photo=doc.scene().order()[0];
    let above=copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(above).unwrap().opacity=0.35;
    let ink=paint_occurrence(&mut doc,"Ink",None);
    set_root_entries(&mut doc,if contact==FirstContact::OnTop {vec![ink,above,photo]}else {vec![above,ink,photo]});
    (doc,[above,ink,photo])
}

fn at_quarter_size(doc:&Document)->FramePacket<'_> {
    let mut frame=FramePacket {blend_space:doc.composition().blend,..packet(doc.scene(),doc.composition().size)};
    frame.view=layer_render::ViewState {width_px:130,height_px:65,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    frame
}

fn warm_and_fresh(doc:&Document,working:OccurrenceHandle)->[WgpuRasterizer;2] {
    let color=doc.composition().color;
    let mut fresh=WgpuRasterizer::new_native_headless(color).unwrap();
    let mut warm=crate::test_support::staged_renderer(&fresh,color);
    for r in [&mut warm,&mut fresh] {layer_render::CanvasRenderer::set_working_layer(r,Some(working));}
    warm.shader_idle(false,false);
    warm.prepare_startup(doc,&layer_core::default_brush(DefaultBrushPreset::GPen),false).unwrap();
    crate::test_support::wait_startup(&mut warm,std::time::Instant::now()+std::time::Duration::from_secs(60),
        |progress|progress.brush_ready,format_args!("brush readiness"));
    let scale=&warm.scene_pipelines.scale;
    let kernels=[&scale.full,&scale.half].into_iter().flat_map(|set|[&set.compose,&set.reduce,&set.reduce_phased,&set.reduce_pair,&set.project]);
    pollster::block_on(warm.compilation.prepare_all(kernels)).unwrap();
    [warm,fresh]
}

fn ink_dab(x:f32)->layer_render::Dab {
    let mut dab=crate::tests::test_dab([x,FIRST_DAB[1]],[0.9,0.02,0.1,0.7],1.);
    dab.radii=[25.;2];dab
}

fn stroke_packet<'a>(frame:FramePacket<'a>,dab:&'a layer_render::Dab,batch:&'a DabBatch,time:f32)->FramePacket<'a> {
    FramePacket {time_seconds:time,composite_all:false,dabs:std::slice::from_ref(dab),dab_batches:std::slice::from_ref(batch),..frame}
}

fn commit_stroke(r:&mut WgpuRasterizer,frame:FramePacket<'_>,target:SourceTarget,style:&layer_render::DabStyle,stroke:u64,xs:[f32;2]) {
    for (step,x) in xs.into_iter().enumerate() {
        let dab=ink_dab(x);
        let batch=DabBatch {stroke_id:layer_core::StrokeId(stroke),stroke_start:step==0,stroke_end:step==1,..dab_batch(target,style.clone(),dab.bounds())};
        crate::test_support::submit_ready(r,stroke_packet(frame,&dab,&batch,0.016*(step+2) as f32)).unwrap();
    }
    crate::test_support::submit_settled(r,FramePacket {composite_all:false,..frame});
}

fn committed(r:&mut WgpuRasterizer,doc:&Document)->Vec<Vec<u8>> {
    let scene=doc.scene();
    let pages=scene.order().iter().filter_map(|owner|scene.source_target(*owner))
        .flat_map(|target|page_coordinates(PixelRect::full(scene.target_extent(target))).map(move|coordinate|(target,coordinate))).collect::<Vec<_>>();
    pages.into_iter().map(|(target,coordinate)| {
        let tile=crate::test_support::submit_encoded(r,|r,encoder|crate::test_support::query_ready(r,encoder,|r,encoder|r.raw_layer_tile(target,coordinate,crate::residency::Class::Visible,crate::frame_demand::Allowance::Fresh,encoder).map(|tile|tile.map(crate::source_access::Sampled::uncertified))));
        tile.map_or_else(Vec::new,|tile|crate::layer_tests::page_bytes(r,&tile.texture))
    }).collect()
}

fn the_first_preview_frame_shows_its_ink_and_commits_like_a_fresh_renderer(contact:FirstContact) {
    for (depth,format) in [(SampleDepth::U8,wgpu::TextureFormat::Rgba16Float),(SampleDepth::F32,wgpu::TextureFormat::Rgba32Float)] {
        let (doc,[_,ink,photo])=overlaid_photo(depth,contact);
        let frame=at_quarter_size(&doc);
        let style=layer_render::DabStyle {blend_space:doc.composition().blend,..crate::layer_tests::preset_style(DefaultBrushPreset::GPen)};
        let on_photo=matches!(contact,FirstContact::PhotoSelectedAtPenDown|FirstContact::PhotoSelectedAtPenDownAfterAStroke);
        let target=doc.scene().source_target(if on_photo {photo}else {ink}).unwrap();
        let [mut r,mut fresh]=warm_and_fresh(&doc,ink);
        for r in [&mut r,&mut fresh] {
            crate::test_support::submit_settled(r,frame);
            if contact==FirstContact::PhotoSelectedAtPenDownAfterAStroke {commit_stroke(r,frame,doc.scene().source_target(ink).unwrap(),&style,1,[120.,124.]);}
            if on_photo {layer_render::CanvasRenderer::set_working_layer(r,Some(photo));}
        }
        let cache=r.scale_display.as_ref().unwrap();
        assert_eq!((cache.texture().format(),cache.plan.size,cache.plan.level),(format,[130,65],2));
        let centre=(FIRST_DAB[1] as usize/4)*130+FIRST_DAB[0] as usize/4;
        let before=display_pixels(&r)[centre][0];

        let dab=ink_dab(FIRST_DAB[0]);
        let batch=DabBatch {kind:DabBatchKind::Preview,stroke_id:layer_core::StrokeId(2),stroke_end:false,..dab_batch(target,style.clone(),dab.bounds())};
        let preview=stroke_packet(frame,&dab,&batch,0.016);
        r.poll_pending(preview.view).unwrap();
        crate::test_support::pace(&mut r);
        let compiling=r.compilation.batch();
        let before_halves=r.scale_display.as_ref().unwrap().half_pages();
        assert!(matches!(r.submit_when_ready(preview,None),std::task::Poll::Ready(Ok(()))),"{format:?} {contact:?}: the first preview frame is accepted without waiting");
        assert_eq!(crate::compile::tests::queued_recipes(&r.compilation),0,"{format:?} {contact:?}: the first preview frame waits for no pipeline");
        assert!(!publication_pending(&r),"{format:?} {contact:?}: the first preview frame covers the view");
        let shown=display_pixels(&r)[centre][0]-before;
        assert!(shown>0.3,"{format:?} {contact:?}: the first preview frame adds {shown} red at its dab centre");
        assert!(r.preview_level>0,"{format:?} {contact:?}: the stroke keeps its preview below full size");
        if contact!=FirstContact::OnTop {
            let cache=r.scale_display.as_ref().unwrap();
            let damage=pixel_rect(dab.bounds(),doc.composition().size);
            let reached=cache.wanted_cells(damage,preview.view);
            let (declared,owed)=cache.contact_declared_and_owed().unwrap_or_default();
            assert!(!reached.is_empty() && reached.iter().all(|cell|declared.contains(cell) && !owed.contains(cell)),
                "{format:?} {contact:?}: the contact begun by the first preview frame declares every reached cell and owes none: {reached:?} in {declared:?}, owed {owed:?}");
            let built:Vec<_>=cache.half_pages().into_iter().filter(|page|!before_halves.contains(page)).collect();
            assert!(built.iter().all(|(level,region)|*level==cache.plan.level && reached.contains(region))
                && built.len()<=cache.pivot_halves().len()*reached.len(),"{format:?} {contact:?}: the first preview only builds missing reached-cell halves: {built:?}");
        }
        drop(compiling);
        let moved=ink_dab(FIRST_DAB[0]+4.);
        let batch=DabBatch {damage:moved.bounds(),..batch};
        crate::test_support::submit_ready(&mut r,stroke_packet(frame,&moved,&batch,0.032)).unwrap();
        let shown=display_pixels(&r)[centre][0]-before;
        assert!(!publication_pending(&r) && shown>0.3,"{format:?} {contact:?}: the second preview frame shows {shown} red where the first dab was");
        assert!(committed(&mut r,&doc)==committed(&mut fresh,&doc),"{format:?} {contact:?}: a preview changes no committed layer");

        for r in [&mut r,&mut fresh] {commit_stroke(r,frame,target,&style,2,[FIRST_DAB[0],FIRST_DAB[0]+4.]);}
        assert!(committed(&mut r,&doc)==committed(&mut fresh,&doc),"{format:?} {contact:?}: the stroke commits what a fresh renderer commits");
        let error=crate::test_support::max_error(&display_pixels(&r),&display_pixels(&fresh));
        assert!(error<1e-4,"{format:?} {contact:?}: the settled view differs from a fresh renderer's by {error}");
    }
}

#[test]
fn the_first_preview_frame_on_the_top_layer_shows_its_ink_and_commits_like_a_fresh_renderer() {
    the_first_preview_frame_shows_its_ink_and_commits_like_a_fresh_renderer(FirstContact::OnTop);
}

#[test]
fn the_first_preview_frame_on_a_layer_beneath_another_shows_its_ink_and_commits_like_a_fresh_renderer() {
    the_first_preview_frame_shows_its_ink_and_commits_like_a_fresh_renderer(FirstContact::Beneath);
}

#[test]
fn the_first_preview_frame_on_a_photo_selected_at_pen_down_shows_its_ink_and_commits_like_a_fresh_renderer() {
    the_first_preview_frame_shows_its_ink_and_commits_like_a_fresh_renderer(FirstContact::PhotoSelectedAtPenDown);
}

#[test]
fn the_first_preview_frame_on_a_photo_selected_at_pen_down_after_a_stroke_on_another_layer_shows_its_ink_and_commits_like_a_fresh_renderer() {
    the_first_preview_frame_shows_its_ink_and_commits_like_a_fresh_renderer(FirstContact::PhotoSelectedAtPenDownAfterAStroke);
}

#[test]
fn an_exact_query_replays_a_reduced_preview_only_when_it_draws_the_stroked_layer() {
    let (doc,[_,ink,photo])=overlaid_photo(SampleDepth::U8,FirstContact::Beneath);
    let frame=at_quarter_size(&doc);
    let style=layer_render::DabStyle {blend_space:doc.composition().blend,..crate::layer_tests::preset_style(DefaultBrushPreset::GPen)};
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r,Some(ink));
    crate::test_support::submit_settled(&mut r,frame);
    let dab=ink_dab(FIRST_DAB[0]);
    let batch=DabBatch {kind:DabBatchKind::Preview,stroke_end:false,..dab_batch(doc.scene().source_target(ink).unwrap(),style,dab.bounds())};
    crate::test_support::submit_ready(&mut r,stroke_packet(frame,&dab,&batch,0.016)).unwrap();
    let reduced=r.preview_level;
    assert!(reduced>0,"the stroke previews below full size");

    let centre=FIRST_DAB[1] as usize*PAGE_SIZE as usize+FIRST_DAB[0] as usize;
    let query=|r:&mut WgpuRasterizer,member:OccurrenceHandle| {
        let scope=SceneScope::Members([member].into());
        let accepted=r.artwork_frame.clone().unwrap();
        let mut capture=crate::artwork::Capture::default();
        let tile=crate::test_support::submit_encoded(r,|r,encoder|crate::test_support::query_ready(r,encoder,|r,encoder|
            capture.region(r,FramePacket {scene:accepted.scene.view().with_scope(&scope),..accepted.packet(r.document_extent)},PixelRect::full([PAGE_SIZE;2]),[PAGE_SIZE;2],encoder)));
        pixels(r,&tile.texture)[centre]
    };
    assert!(query(&mut r,photo)[3]>0.99,"the photo alone is opaque under the dab");
    assert_eq!(r.preview_level,reduced,"a query of a layer that does not draw the stroked one keeps the reduced preview");
    assert!(query(&mut r,ink)[3]>0.5,"a query of the stroked layer shows its previewed dab");
    assert_eq!(r.preview_level,0,"a query that draws the stroked layer replays the preview at full size");
}
