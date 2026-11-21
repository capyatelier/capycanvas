use crate::test_support::float_pixels as pixels;
use super::*;
use crate::test_support::{dab_batch, packet};
use layer_core::color::{SampleDepth, source::*};
use layer_core::{DefaultBrushPreset, Document, CoverageSnapshot};
use layer_core::authored::*;
use super::Image;

pub(crate) fn queue_unencoded_clear(scene:&mut Scene,r:&WgpuRasterizer)->Result<(),GpuRasterError> {
    let slot=scene.reserve(r)?;
    scene.jobs.push(Job::Clear(scene.pool[slot].view.clone(),wgpu::Color::TRANSPARENT));
    scene.source_jobs.push(0..1);
    Ok(())
}

pub(crate) fn unencoded_job_counts(scene:&Scene)->[usize;2] {[scene.jobs.len(),scene.source_jobs.len()]}

pub(super) fn prepare_fixture_frame(r:&WgpuRasterizer,packet:FramePacket<'_>,scene:&Scene) {
    scene.scale_pages.lock().unwrap().set_expected(Arc::new(SignatureFrame::capture(r,packet,Declared::live(r)).unwrap()));
}

pub(super) fn evict_composite_pages(cache:&Cache,areas:&[PixelRect]) {pages::evict_composite_pages(cache,areas)}
pub(crate) fn evict_view(r:&WgpuRasterizer) {let cache=r.scale_display.as_ref().unwrap();evict_composite_pages(cache,&[cache.plan.bounds]);cache.evict_window(cache.plan.bounds);}

fn source_chunks(cache:&Cache,id:SourceTarget,level:u32)->BTreeSet<[u32;2]> {
    cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.source==Some(id) && page.key.level==level)
        .flat_map(|page|page.current.iter().copied().collect::<Vec<_>>()).collect()
}

fn source_levels(cache:&Cache,id:SourceTarget)->BTreeMap<u32,BTreeSet<[u32;2]>> {
    let mut levels=BTreeMap::<u32,BTreeSet<[u32;2]>>::new();
    for page in cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.source==Some(id) && !page.current.is_empty()) {
        levels.entry(page.key.level).or_default().extend(page.current.iter().copied());
    }
    levels
}

pub(crate) fn source_textures(r:&WgpuRasterizer,id:SourceTarget,level:Option<u32>)->Vec<wgpu::Texture> {
    let Some(cache)=r.scale_display.as_ref() else {return Vec::new();};
    source_textures_in(cache,id,level)
}

fn source_textures_in(cache:&Cache,id:SourceTarget,level:Option<u32>)->Vec<wgpu::Texture> {
    let mut pages=cache.pages.lock().unwrap().retained_pages().into_iter()
        .filter(|page|page.key.source==Some(id) && level.is_none_or(|level|page.key.level==level) && !page.current.is_empty()).collect::<Vec<_>>();
    pages.sort_by_key(|page|(page.key.level,page.key.coordinate));
    pages.into_iter().map(|page|page.texture).collect()
}

pub(super) fn prepare_layer_pages(r:&mut WgpuRasterizer,frame:FramePacket<'_>) {
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
    loop {
        let (mut scene,mut cache)=(r.scene.take().unwrap(),r.scale_display.take().unwrap());
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        scene.begin_frame();
        let requests=cache.region_sources(&scene,r,frame,cache.plan.bounds,false);
        let ready=requests.iter().all(|(id,request)|cache.source_ready(&scene,r,*id,request));
        for (id,request) in requests {{let mut work=crate::test_support::work(false);cache.prepare_source(&mut scene,r,frame,&mut encoder,id,request,Class::Visible,&mut work).unwrap();}}
        r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(r);
        r.scene=Some(scene);r.scale_display=Some(cache);
        if ready {return;}
        assert!(std::time::Instant::now()<deadline,"the reduced path's layer pages must prepare");
        r.poll_pending(frame.view).unwrap();std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn source_cache_info(r:&WgpuRasterizer,id:SourceTarget,level:Option<u32>)->Option<(Vec<wgpu::Texture>,u64,u32)> {
    let cache=r.scale_display.as_ref()?;
    let source=r.scene.as_ref()?.scale_sources.entries.get(&id)?;
    let level=level.or_else(||source_levels(cache,id).first_key_value().map(|(level,_)|*level))?;
    let textures=source_textures_in(cache,id,Some(level));
    (!textures.is_empty()).then_some((textures,source.updates,level))
}

fn source_image(r:&WgpuRasterizer,cache:&Cache,source:&sources::Source,id:SourceTarget,plan:display_mips::Plan)->Image {
    let mut store=cache.pages.lock().unwrap();
    let inputs=page_coordinates(plan.bounds).map(|coordinate| {
        let region=page_rect(coordinate).intersect(plan.bounds);
        let key=source.key(r,id,plan.level,coordinate.map(|value|value>>plan.level));
        let image=store.get(&key,region,pages::Accuracy::Display,key.level,crate::residency::Class::Visible).expect("every sampled source chunk must be committed and current");
        (key,image,region)
    }).collect::<Vec<_>>();
    let output=Image::new(r,plan,"committed source test window");
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    for (_,image,region) in inputs {
        let [sx,sy,width,height]=paint_transform::texel_rect(region.window_local(image.plan.bounds),1<<plan.level);
        let [dx,dy,_,_]=paint_transform::texel_rect(region.window_local(plan.bounds),1<<plan.level);
        encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:sx,y:sy,z:0},..image.texture.as_image_copy()},
            wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:dx,y:dy,z:0},..output.texture.as_image_copy()},wgpu::Extent3d {width,height,depth_or_array_layers:1});
        image.track(&encoder);
    }
    encoder.submit(&r.queue);output
}

pub(crate) fn source_copy_ready(r:&WgpuRasterizer,scene:SceneView<'_>,id:SourceTarget,extent:[u32;2],level:u32)->bool {
    let output=Image::new(r,display_mips::Plan::at(extent,level),"raw transform source eligibility");
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    pages::coordinates(PixelRect::full(extent),level).all(|coordinate|
        r.scene.as_ref().unwrap().copy_reduced_source(r,scene,id,extent,level,pages::bounds(coordinate,level,extent),&output.texture,&mut encoder))
}

pub(crate) fn publication_pending(r: &WgpuRasterizer) -> bool {
    r.scale_display.as_ref().is_some_and(|cache| cache.presented_matches_expected()!=Some(true) || !cache.coverage_complete())
}

pub(super) fn covered_document_pages(cache:&Cache,accuracy:pages::Accuracy)->impl Iterator<Item=[u32;2]> {
    let coverage=cache.coverage_snapshot();let bounds=cache.plan.bounds;
    page_coordinates(bounds).filter(move|coordinate|coverage.ready(page_rect(*coordinate).intersect(bounds),accuracy))
}

fn reset_page_store(cache:&mut Cache) {
    let expected=cache.pages.lock().unwrap().expected().cloned();
    let mut store=pages::Store::default();if let Some(expected)=expected {store.set_expected(expected);}
    let pages=Arc::new(std::sync::Mutex::new(store));
    let mut current=Some(cache);
    while let Some(cache)=current {cache.pixels=Default::default();cache.cells=Default::default();cache.pages=pages.clone();current=cache.overview.as_deref_mut();}
}

fn assert_coverage_eq(before:&pages::Coverage,after:&pages::Coverage) {
    assert_eq!(before.written,after.written);
    assert_eq!(before.matching,after.matching);
    assert_eq!(before.exact,after.exact);
}

fn working_scene_frame(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    crate::memory::memory_tests::legacy_scene_frame(r, frame);
}

#[test]
fn nearest_work_is_bounded_and_uses_accepted_inputs_only_to_break_ties() {
    let document=Document::new(PortableId::random(),32768,30518,layer_core::DocumentNames {paint:"Pixels".into(),paper:"Paper".into()});
    let mut frame=packet(document.scene(),document.composition().size);
    frame.view.width_px=1024;frame.view.height_px=768;
    frame.view.document_to_surface=[1.,0.,0.,1.,-4096.,-2048.];
    let wanted=view_plan(frame,0).unwrap().bounds;
    let smaller=FramePacket {document_extent:[9504,6336],..frame};
    for level in 0..=1 {
        assert_eq!(view_plan(frame,level).unwrap().bounds,view_plan(smaller,level).unwrap().bounds,
            "per-attempt output planning depends on the window, not the document extent");
    }
    let held=[wanted.min_x()/PAGE_SIZE,wanted.min_y()/PAGE_SIZE];
    let candidates=||page_coordinates(wanted);
    let selected=nearest_pages(wanted,frame.view,7,[held],|_|true);
    assert_eq!(selected.len(),7);assert_ne!(selected[0],held);
    assert!(selected.iter().all(|coordinate|!page_rect(*coordinate).intersect(wanted).is_empty()));
    assert_eq!(selected.iter().copied().collect::<BTreeSet<_>>().len(),selected.len());
    let mut expected=candidates().collect::<Vec<_>>();
    let center=[4608.,2432.];
    expected.sort_by_key(|coordinate| {
        let rect=page_rect(*coordinate);
        let dx=f64::from(rect.min_x()+rect.max_x())*0.5-center[0];
        let dy=f64::from(rect.min_y()+rect.max_y())*0.5-center[1];
        ((dx*dx+dy*dy).to_bits(),*coordinate!=held,*coordinate)
    });
    assert_eq!(selected,expected[..7]);
    let visited=std::cell::Cell::new(0);
    assert!(nearest_pages(wanted,frame.view,0,std::iter::empty(),|_|{visited.set(visited.get()+1);true}).is_empty());
    assert_eq!(visited.get(),0,"exhausted frames must not enumerate source work");
}

#[test]
fn authored_work_excludes_source_arrivals_camera_moves_and_refinement() {
    fn authored(frame:FramePacket<'_>)->layer_core::raster::ContentStamp {
        let preview=crate::scene::PreviewDeclaration::from_scene(frame,Default::default(),Default::default(),crate::scene::PreviewQuality::default(),None,None).unwrap().map(|declaration| {
            assert!(frame.dab_batches.iter().filter(|batch|batch.kind==DabBatchKind::Preview).all(|batch|batch.stroke_start));
            let seeds=declaration.required_seed_inputs().into_iter().map(|(plane,coordinate,pass)| {
                let versions=match pass {
                    crate::scene::PreviewPass::Coverage|crate::scene::PreviewPass::Reservoir=>Vec::new(),
                    crate::scene::PreviewPass::StrokeStart=>vec![declaration.version(declaration.target(),plane,coordinate,crate::scene::PreviewStage::Seed).unwrap()],
                    _=>panic!("fresh-stroke fixture has an unexpected seed pass"),
                };
                crate::scene::PreviewSeed {plane,coordinate,pass,versions,committed:false}
            }).collect();
            Arc::new(declaration.with_seed_inputs(seeds).unwrap())
        });
        let prepared=SignatureFrame::from_scene(frame,Declared {preview,..Default::default()}).unwrap();
        let root=graph::compose(frame,None,None,0,frame.scene.composition().color.space,None,None).unwrap();
        prepared.authored_revision(&root).unwrap()
    }
    let doc=document_at([512;2]);let frame=packet(doc.scene(),doc.composition().size);let baseline=authored(frame);
    assert_eq!(authored(frame),baseline);
    let mut moved=frame;moved.view.document_to_surface[4]+=128.;
    assert_eq!(authored(moved),baseline);
    let target=source_at(&doc,0);
    let restored=FramePacket {composite_all:true,..moved};
    assert_eq!(authored(restored),baseline,"physical restoration alone is not an authored edit");
    let dab=crate::tests::test_dab([48.,48.],[1.,0.,0.,1.],1.);
    let batch=dab_batch(target,crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
    let mut changed=doc.clone();
    paint_mut(&mut changed,0).raster=layer_core::raster::RasterRevision::backed(layer_core::raster::RasterData {
        tiles:[(layer_core::raster::TileKey {plane:layer_core::raster::RasterPlane::Color,coordinate:[0;2]},
            layer_core::raster::RasterTile::pending(layer_core::raster::RasterPlane::Color.descriptor(changed.composition().color)))].into(),..Default::default()
    });
    assert_ne!(authored(FramePacket {scene:changed.scene(),dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),..moved}),baseline);
    let mut changed=doc.clone();occurrence_mut(&mut changed,0).opacity=0.5;
    assert_ne!(authored(FramePacket {scene:changed.scene(),..moved}),baseline);
    let mut preview=batch.clone();preview.kind=DabBatchKind::Preview;
    let preview_frame=FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&preview),..frame};
    let current=authored(preview_frame);
    assert_eq!(authored(FramePacket {view:moved.view,..preview_frame}),current,
        "resubmitting an unchanged preview while moving the camera is not an authored edit");
    assert_ne!(authored(frame),current,"removing authored preview pixels requires in-frame recomposition");
    let mut replacement=preview.clone();replacement.damage=layer_core::Rect {
        min:layer_core::Point {x:64.,y:64.},max:layer_core::Point {x:96.,y:96.}};
    let replacement_dab=crate::tests::test_dab([80.,80.],[1.,0.,0.,1.],1.);
    assert_ne!(authored(FramePacket {dabs:std::slice::from_ref(&replacement_dab),dab_batches:std::slice::from_ref(&replacement),..preview_frame}),current);
}

#[test]
fn identity_source_window_clips_signed_bounds_without_interpolation_padding() {
    let extent = [2048; 2];
    let mut output = display_mips::Plan::window(extent, 2, PixelRect::full([579, 190]));
    output.doc_bounds = DocRect { min: [-100, 289], max: [479, 479] };
    let identity = SourceFrame::at([0; 2], [0; 2]);
    let plan = source_plan(output, identity, extent, false, 0);
    assert_eq!(plan.level, 2);
    assert_eq!(plan.bounds, PixelRect::new(0, 256, 512, 512));
    output.doc_bounds = DocRect { min: [-400, 289], max: [-100, 479] };
    assert!(source_plan(output, identity, extent, false, 0).bounds.is_empty());
}

#[path = "presentation_tests.rs"]
mod presentation;

#[path = "commit_tests.rs"]
mod commit;

#[path = "cell_key_tests.rs"]
pub(super) mod cell_keys;

#[path = "contact_tests.rs"]
pub(crate) mod contact;

#[path = "scratch_tests.rs"]
mod scratch_ownership;

#[path = "source_progress_tests.rs"]
pub(crate) mod source_progress;

#[path = "effect_tests.rs"]
mod effects;
#[path = "color_effect_tests.rs"]
mod color_effects;

#[path = "refinement_tests.rs"]
mod refinement;

#[path = "native_alpha_reuse_tests.rs"]
mod native_alpha_reuse;

#[path = "native_composite_tests.rs"]
mod native_composite;

#[path = "native_signed_tests.rs"]
mod native_signed;

#[path = "root_fragment_tests.rs"]
mod root_fragment;

#[path = "moved_paint_tests.rs"]
mod moved_paint;

pub(super) use crate::test_support::submit_published;

fn retained_root_pages(cache: &Cache) -> Vec<pages::RetainedPage> {
    let root=cache.graph.content.as_ref().expect("prepared root");
    cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.expression==*root).collect()
}

fn retained_level_pixels(r: &WgpuRasterizer, cache: &Cache, level: u32) -> Vec<[f32;4]> {
    let plan=display_mips::Plan::at(cache.plan.extent,level);
    let mut output=vec![[f32::NAN;4];(plan.size[0]*plan.size[1]) as usize];
    for page in retained_root_pages(cache).into_iter().filter(|page|page.key.level==level) {
        let local=page.key.plan();let data=pixels(r,&page.texture);
        for region in page.exact.regions(local.bounds) {
            let [sx,sy,width,height]=paint_transform::texel_rect(region.window_local(local.bounds),1<<level);
            let [x,y,_,_]=paint_transform::texel_rect(region,1<<level);
            for row in 0..height {
                let from=((sy+row)*local.size[0]+sx) as usize;
                let to=((y+row)*plan.size[0]+x) as usize;
                output[to..to+width as usize].copy_from_slice(&data[from..from+width as usize]);
            }
        }
    }
    assert!(output.iter().all(|pixel|pixel.iter().all(|value|value.is_finite())),"the requested exact level must be completely retained");
    output
}

fn assert_settled(mut r: &mut WgpuRasterizer, frame: FramePacket<'_>, reference: &[[f32; 4]]) {
    let revision = r.artwork_revision;
    let pages = page_coordinates(PixelRect::full(frame.document_extent)).count();
    let idle = FramePacket { composite_all: false, reset_layers: false, dabs: &[], dab_batches: &[], restore_rasters: &[], ..frame };
    let check_work=|r:&WgpuRasterizer,work:u64| {
        let bounded = frame.scene.order().iter().any(|h|frame.scene.visible(*h)
            && frame.scene.effect(*h).is_some_and(|effect|effect.program.image_boundary()));
        let limit = if bounded {4 * u64::from(PAGE_SIZE).pow(2)} else {
            r.scale_display.as_ref().and_then(|cache|cache.exact_tile.as_ref()).map_or(
                SOURCE_SLOTS as u64 * u64::from(PAGE_SIZE).pow(2), |image|
                    (u64::from(image.texture.width()) * u64::from(image.texture.height()))
                        .min(SOURCE_SLOTS as u64 * u64::from(PAGE_SIZE).pow(2)))
        };
        assert!(r.metrics.composited_pixels - work <= limit);
        assert_eq!(r.artwork_revision, revision, "idle refinement does not edit artwork");
    };
    let installed=r.preparation_wake.clone();let forward=installed.clone();
    let (signal,woken)=std::sync::mpsc::channel();
    r.set_preparation_wake(Arc::new(move||{if let Some(wake)=&forward {wake();}let _=signal.send(());}));
    let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
    let mut visits = 0;
    'frames: for _ in 0..pages + 3 {
        let work = r.metrics.composited_pixels;
        let navigator = r.navigator.revision;
        loop {
            r.poll_startup().unwrap();r.poll_pending(frame.view).unwrap();
            match r.next_frame() {
                layer_render::FrameDemand::Work=>break,
                _ if r.settled()=>{check_work(r,work);break 'frames;},
                _=>assert!(woken.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).is_ok(),
                    "refinement needs a registered completion edge: {:?}",r.outstanding_terms()),
            }
        }
        assert!(visits < pages + 1, "refinement must finish within one visit per native page");
        r.wait_idle().unwrap();
        crate::test_support::submit_ready(&mut r, idle).unwrap();
        if r.navigator.revision == navigator { visits += 1; }
        check_work(r,work);
    }
    assert!(r.settled() && !publication_pending(r), "refinement and its Navigator refresh must settle: {:?}",r.outstanding_terms());
    match installed {Some(wake)=>r.set_preparation_wake(wake),None=>r.preparation_wake=None}
    let cache = r.scale_display.as_ref().unwrap();
    assert_reduction(&display_pixels(r), reference, cache.plan, cache.texture().format(), 2e-5, format_args!("settled display"));
    if let Some(overview) = &cache.overview {
        assert_reduction(&window_pixels(r, overview.pixels.root().unwrap(), overview.plan), reference, overview.plan, overview.pixels.root().unwrap().texture.format(), 2e-5, format_args!("settled overview"));
    }
    assert_presentation_mip(r);

    let work = r.metrics.composited_pixels;
    crate::test_support::submit_ready(&mut r, idle).unwrap();
    assert_eq!(r.metrics.composited_pixels, work, "completed refinement has no further work");
}

fn document() -> Document {
    document_at([517, 259])
}

#[test]
fn an_untouched_drawing_at_fit_refines_empty_paint_without_missing_inputs() {
    let extent=[512,256];
    let document=Document::new(PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let paint=document.scene().paint_source(document.scene().order()[0]).unwrap();
    assert!(paint.base.is_none() && paint.raster.is_empty());
    assert!(document.bottom_paper().is_some());
    let mut renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let mut frame=packet(document.scene(),extent);
    frame.view=layer_render::ViewState {width_px:128,height_px:64,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
    let before=crate::test_support::PassCounts::snapshot();
    crate::test_support::submit_settled(&mut renderer,frame);
    let cache=renderer.scale_display.as_ref().unwrap();
    assert!(cache.pixels.committed_root().is_some());
    assert!(cache.coverage_snapshot().ready(cache.plan.bounds,pages::Accuracy::Exact));
    assert!(renderer.scene.as_ref().unwrap().valid.load(std::sync::atomic::Ordering::Acquire));
    let pixels=renderer.readback_srgb_rgba8().unwrap();
    assert_eq!(pixels.len(),extent[0] as usize*extent[1] as usize*4);
    assert!(pixels.as_chunks::<4>().0.iter().all(|pixel|*pixel==[255;4]),"untouched paper remains opaque white");
    assert_eq!(crate::test_support::PassCounts::snapshot().delta(before).source_input_misses,0);
}

fn plain_raster_stroke_with_shared_overview(minified:bool) {
    let extent=[2304;2];let document=document_at(extent);
    let mut renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    renderer.test.exact_display=true;
    let mut frame=packet(document.scene(),extent);
    frame.view.width_px=512;frame.view.height_px=512;
    frame.view.document_to_surface=[1.,0.,0.,1.,-896.,-896.];
    crate::test_support::submit_settled(&mut renderer,frame);
    let cache=renderer.scale_display.as_ref().unwrap();
    let overview=cache.overview.as_ref().expect("plain raster fixture has an overview");
    assert!(Arc::ptr_eq(&cache.pages,&overview.pages));
    assert_ne!(cache.plan,overview.plan);
    let shared=cache.pages.clone();
    if minified {
        renderer.test.exact_display=false;
        frame.view.document_to_surface=[1./64.,0.,0.,1./64.,0.,0.];
        crate::test_support::submit_settled(&mut renderer,FramePacket {composite_all:false,..frame});
        assert!(Arc::ptr_eq(&shared,&renderer.scale_display.as_ref().unwrap().pages));
    }
    let mut dab=crate::tests::test_dab([1152.;2],[1.,0.,0.,1.],1.);dab.radii=[32.;2];
    let batch=dab_batch(source_at(&document,0),crate::layer_tests::preset_style(DefaultBrushPreset::GPen),dab.bounds());
    crate::test_support::submit_settled(&mut renderer,FramePacket {composite_all:false,dabs:std::slice::from_ref(&dab),
        dab_batches:std::slice::from_ref(&batch),..frame});
    let cache=renderer.scale_display.as_ref().unwrap();
    assert!(!cache.coverage_snapshot().written.is_empty());
    assert!(cache.retained_ready(),"the stroke must publish current raster pages across cache roles");
}

#[test]
fn a_view_without_an_overview_admits_its_root_window_beyond_a_quarter_of_the_scene_budget() {
    let extent=[2048,1024];let document=document_at(extent);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
    let frame=FramePacket {composite_all:false,..packet(document.scene(),extent)};
    crate::test_support::submit_settled(&mut r,frame);
    let mut cache=r.scale_display.take().unwrap();
    assert!(cache.overview.is_none() && cache.plan.bounds==PixelRect::full(extent),"the view covers the document");
    drop(cache.pixels.take());cache.pages=Default::default();
    let bytes=cache.plan.size.map(u64::from).into_iter().product::<u64>()*u64::from(cache.precision.format().block_copy_size(None).unwrap());
    let used=r.scene_tiers.display.used();
    let _budget=crate::memory::memory_tests::display_frame_with_budget(&mut r,frame,used+2*bytes);
    let grant=r.memory.display_grant();
    assert!(grant/4<r.scene_tiers.display.used()+bytes && r.scene_tiers.display.used()+bytes<=grant,"the window fits the display grant but not a quarter of it: window={bytes} grant={grant}");
    assert!(cache.reserve_window(&r,r.scene.as_ref().unwrap(),bytes).is_ok(),"the view's required root window is admitted within the scene budget");
}

#[test]
fn a_page_claim_charges_its_page_once_on_the_display_tier_and_publishes_no_report() {
    let extent=[2304;2];let document=document_at(extent);
    let mut renderer=WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let mut frame=packet(document.scene(),extent);
    frame.view.width_px=512;frame.view.height_px=512;
    frame.view.document_to_surface=[1.,0.,0.,1.,-896.,-896.];
    crate::test_support::submit_settled(&mut renderer,frame);
    let r=&renderer;let cache=r.scale_display.as_ref().unwrap();let scene=r.scene.as_deref().unwrap();
    for coordinate in [[8,8],[0,8],[8,0]] {
        let key=cache.page_key(cache.graph.root.clone().unwrap(),pages::bounds(coordinate,cache.plan.level,cache.plan.extent));
        assert!(cache.pages.lock().unwrap().allocated(&key,crate::residency::Class::Visible).is_none(),"the fixture page {coordinate:?} must be new");
        let [before,reported]=[r.scene_tiers.display.used(),r.memory.display_owner().reported().gpu_bytes];
        let image=cache.claim_page(scene,r,key,crate::residency::Class::Visible).unwrap().expect("an off-view page fits the display grant");
        assert_eq!(r.scene_tiers.display.used()-before,image.bytes(),"claim {coordinate:?} charges its page once");
        assert_eq!(r.memory.display_owner().reported().gpu_bytes,reported,"claim {coordinate:?} publishes no report");
        drop(image);
    }
}

#[test]
fn plain_raster_overview_stroke_reconciles_finer_inputs() {
    plain_raster_stroke_with_shared_overview(false);
}

#[test]
fn plain_raster_64x_role_swap_reconciles_finer_inputs() {
    plain_raster_stroke_with_shared_overview(true);
}

pub(crate) fn document_at(extent: [u32; 2]) -> Document {
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let ink = doc.scene().order()[0];
    set_root_entries(&mut doc, vec![ink]);
    let paint = primary_paint(&doc);
    doc.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new((rgba8_source(extent, |x, y| [(x / 3) as u8, (y / 2) as u8, 80, 255])).into()));
    doc
}

fn composition_mut(doc: &mut Document) -> &mut layer_core::authored::Composition { let root = doc.artwork.root; doc.artwork.compositions.get_mut(root).unwrap() }
fn set_attachment(occurrence: &mut Occurrence, enabled: bool) {
    occurrence.attachment = if !enabled { Attachment::None }
        else if occurrence.kind() == LayerKind::Effect { Attachment::Effect } else { Attachment::Clip };
}
fn set_attachment_at(doc: &mut Document, index: usize, enabled: bool) {
    set_attachment(occurrence_mut(doc, index), enabled);
    reindex(doc);
}
fn occurrence_at(doc: &Document, index: usize) -> &Occurrence { doc.artwork.occurrences.get(doc.scene().order()[index]).unwrap() }
fn occurrence_mut(doc: &mut Document, index: usize) -> &mut Occurrence { let handle = doc.scene().order()[index]; doc.artwork.occurrences.get_mut(handle).unwrap() }
fn source_at(doc: &Document, index: usize) -> SourceTarget { match occurrence_at(doc, index).content { OccurrenceContent::Paint(paint) => SourceTarget::Paint(paint), _ => unreachable!() } }
fn paint_at(doc: &Document, index: usize) -> &PaintSource { let SourceTarget::Paint(paint) = source_at(doc,index) else { unreachable!() }; doc.artwork.paint.get(paint).unwrap() }
pub(super) fn paint_mut(doc: &mut Document, index: usize) -> &mut PaintSource { let SourceTarget::Paint(paint) = source_at(doc,index) else { unreachable!() }; doc.artwork.paint.get_mut(paint).unwrap() }
fn copy_paint(doc: &mut Document, index: usize) -> OccurrenceHandle {
    let mut occurrence = occurrence_at(doc,index).clone();
    let source=paint_at(doc,index).clone();
    let paint = doc.artwork.paint.insert(PortableId::random(), source).unwrap();
    if let Some(mask)=&mut occurrence.mask {
        let source=doc.artwork.coverage.get(mask.source).unwrap().clone();
        mask.source=doc.artwork.coverage.insert(PortableId::random(),source).unwrap();
    }
    occurrence.content = OccurrenceContent::Paint(paint);
    doc.artwork.occurrences.insert(PortableId::random(), occurrence).unwrap()
}
pub(super) fn paint_occurrence(doc: &mut Document, name: &str, original: Option<Arc<SourceImage>>) -> OccurrenceHandle {
    let paint = doc.artwork.paint.insert(PortableId::random(), PaintSource { color_mode: Default::default(), domain: doc.composition().size, raster: Default::default(), base: original.map(|source| layer_core::authored::PaintBase::new(source.into())), operations: Arc::default() }).unwrap();
    doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(paint), name)).unwrap()
}
fn stack_occurrence(doc: &mut Document, name: &str, entries: Vec<OccurrenceHandle>) -> OccurrenceHandle {
    let stack = doc.artwork.stacks.insert(PortableId::random(), Stack { entries }).unwrap();
    doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Stack(stack), name)).unwrap()
}
fn set_root_entries(doc: &mut Document, entries: Vec<OccurrenceHandle>) { let stack=doc.composition().result; doc.artwork.stacks.get_mut(stack).unwrap().entries=entries; reindex(doc); }

fn swap_entries(doc:&mut Document,a:usize,b:usize){ let stack=doc.composition().result;doc.artwork.stacks.get_mut(stack).unwrap().entries.swap(a,b);reindex(doc); }

fn primary_paint(doc: &Document) -> PaintHandle { doc.artwork.paint.iter().next().unwrap().0 }
fn primary_target(doc: &Document) -> SourceTarget { SourceTarget::Paint(primary_paint(doc)) }
fn reindex(doc: &mut Document) {
    let handle = doc.composition().result;
    let edit = layer_core::Edit::Stack(RecordChange { handle, id: doc.artwork.stacks.id(handle).unwrap(), value: doc.artwork.stacks.get(handle).cloned() });
    doc.apply(edit).unwrap();
}
pub(super) fn insert_occurrence(doc: &mut Document, handle: OccurrenceHandle, position: usize) {
    let stack = doc.composition().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(position, handle);
    reindex(doc);
}
fn remove_occurrence(doc: &mut Document, handle: OccurrenceHandle) {
    let stack = doc.composition().result;
    doc.artwork.stacks.get_mut(stack).unwrap().entries.retain(|entry| *entry != handle);
    doc.artwork.occurrences.get_mut(handle).unwrap().attachment = Attachment::None;
    reindex(doc);
}
pub(super) fn effect_occurrence(doc: &mut Document, effect: layer_core::EffectInstance, name: &str) -> OccurrenceHandle {
    let size=doc.composition().size;
    let application=doc.artwork.effects.insert(PortableId::random(),EffectApplication::new(effect.program,effect.values,size)).unwrap();
    doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(application), name)).unwrap()
}
pub(super) fn add_fill(doc: &mut Document, color: layer_core::color::RgbColor) -> OccurrenceHandle {
    let mut effect = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("solid_color").unwrap().program());
    effect.set("color", layer_core::EffectValue::Color(color)).unwrap();
    let fill = effect_occurrence(doc, effect, "Fill");
    let stack = doc.composition().result;
    let position = doc.artwork.stacks.get(stack).unwrap().entries.len();
    insert_occurrence(doc, fill, position);
    fill
}
fn effect_handle(doc: &Document, handle: OccurrenceHandle) -> EffectHandle {
    match doc.artwork.occurrences.get(handle).unwrap().content { OccurrenceContent::Effect(effect) => effect, _ => unreachable!() }
}
pub(super) fn set_effect_value(doc: &mut Document, handle: OccurrenceHandle, key: &str, value: layer_core::EffectValue) {
    let effect = effect_handle(doc, handle);
    let application = doc.artwork.effects.get(effect).unwrap();
    let program = &application.program;
    let parameter = program.parameters.iter().position(|parameter| parameter.key.as_ref() == key).unwrap();
    doc.artwork.effects.get_mut(effect).unwrap().values[parameter] = value;
}
pub(super) fn coverage_mask(doc: &mut Document, handle: OccurrenceHandle, offset: [i64; 2], initial: Option<layer_core::Selection>) -> CoverageHandle {
    let mut source = CoverageSource { domain: doc.scene().local_extent(handle), raster: Default::default(), default_coverage: 1., operations: Arc::default() };
    if let Some(selection) = initial { crate::test_support::materialize_mask(&mut source, selection, doc.composition().color); }
    let source = doc.artwork.coverage.insert(PortableId::random(), source).unwrap();
    doc.artwork.occurrences.get_mut(handle).unwrap().mask = Some(MaskUse { source, enabled: true, linked: true, inverted: false, offset });
    reindex(doc);
    source
}

#[test]
fn native_material_reduction_preserves_small_hdr_corrections() {
    let mut doc = document_at([256, 256]);
    composition_mut(&mut doc).color.depth = SampleDepth::F32;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let base = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
    let source = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
    let output = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
    let mut pigment = vec![[0., 0., 0., 1.]; 256 * 256];
    for (i, value) in [1048576., f32::from_bits(0x4980_0001), f32::from_bits(0x4980_0002), f32::from_bits(0x4980_0003)].into_iter().enumerate() {
        pigment[(i / 2) * 256 + i % 2][0] = value;
    }
    let mut appearance = pigment.clone();
    appearance[0][0] += 0.125;
    let expected = [0, 1, 256, 257].into_iter().map(|i|
        f64::from(appearance[i][0]) - f64::from(pigment[i][0])).sum::<f64>() / 4.;
    let bytes = |values: &[[f32; 4]]| values.iter().flatten().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    crate::test_support::upload_page(&r, &base, &bytes(&pigment));
    crate::test_support::upload_page(&r, &source, &bytes(&appearance));
    let binding = RegionRecords::binding(&r, &source.create_view(&Default::default()),
        &base.create_view(&Default::default()), &output.create_view(&Default::default()));
    pollster::block_on(r.compilation.prepare_all([&r.scene_pipelines.scale.full.reduce])).unwrap();
    let mut commands = RegionRecords::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let mut values = [0; 20];
    values[..8].copy_from_slice(&[0, 0, 1, 1, 256, 256, 2, 128]);
    commands.reduce(&mut r, &mut encoder, values, &binding, "HDR material correction oracle",wgpu::TextureFormat::Rgba32Float).unwrap();
    r.uploads.finish(&encoder); encoder.submit(&r.queue);
    let actual = pixels(&r, &output)[0][0];
    assert!((f64::from(actual) - expected).abs() < 1e-6, "HDR correction {actual}, expected {expected}");
}

#[test]
fn idle_display_converges_to_exact_composition_after_edits() {
    for space in layer_core::BlendSpace::ALL {
        let mut doc = document_at([1537, 1025]);
        let extent = doc.composition().size;
        paint_mut(&mut doc,0).base = Some(layer_core::authored::PaintBase::new((layer_core::color::source::rgba8_source(extent, |x, y|
            [if (x / 3 + y / 2) % 2 == 0 { 40 } else { 220 }, 128, 70, 255])).into()));
        let top = paint_occurrence(&mut doc, "correlated coverage", Some(layer_core::color::source::rgba8_source(extent, |x, y|
            [180, 20, 100, if (x / 3 + y / 2) % 2 == 0 { 40 } else { 220 }])));
        insert_occurrence(&mut doc, top, 0);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        for (step, opacity) in [0.8, 0.3, 0.9].into_iter().enumerate() {
            occurrence_mut(&mut doc,0).opacity = opacity;
            let mut frame = packet(doc.scene(), extent);
            frame.blend_space = space;
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            submit_published(&mut r, frame);
            submit_published(&mut exact, frame);
            let reference = pixels(&exact, crate::test_support::document_texture(&exact));
            let before = quality(&display_pixels(&r), &reference, r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
            assert!(before[2] > 1e-4, "fixture must need refinement: {before:?}");
            if step == 0 {
                crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..frame }).unwrap();
                assert!(r.next_frame()==layer_render::FrameDemand::Work, "the next edit interrupts partial refinement");
                continue;
            }
            assert_settled(&mut r, frame, &reference);
        }
    }
}

#[test]
fn blend_space_changes_refresh_branches_and_source_representations() {
    use layer_core::BlendSpace;
    let mut doc = document();
    let extent = doc.composition().size;
    let entries=(0..12).map(|i| { let handle=copy_paint(&mut doc,0); doc.artwork.occurrences.get_mut(handle).unwrap().opacity=0.15+i as f32*0.04; handle }).collect();
    set_root_entries(&mut doc,entries);
    occurrence_mut(&mut doc,0).blend = layer_core::LayerBlend::SoftLight;
    let color = layer_core::color::RgbColor::from_linear(doc.composition().color.space, [0.17, 0.39, 0.81, 0.7]).unwrap();
    add_fill(&mut doc, color);
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut fresh = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for (step, (space, offset, level)) in [
        (BlendSpace::Linear, [0, 0], 3),
        (BlendSpace::Perceptual, [0, 0], 3),
        (BlendSpace::Linear, [0, 0], 3),
        (BlendSpace::Perceptual, [8, -4], 3),
        (BlendSpace::Perceptual, [0, 0], 3),
        (BlendSpace::Perceptual, [0, 0], 0),
        (BlendSpace::Linear, [0, 0], 0),
    ].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).offset = offset;
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = space;
        frame.composite_all = matches!(step, 3 | 4);
        let scale = 1. / (1 << level) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);
        crate::memory::memory_tests::legacy_scene_frame(&mut fresh, FramePacket {reset_layers:true,..frame});
        submit_published(&mut r, frame);
        submit_published(&mut fresh, FramePacket { reset_layers: true, ..frame });
        submit_published(&mut exact, FramePacket { composite_all: true, ..frame });
        let actual = display_pixels(&r);
        let expected = display_pixels(&fresh);
        let error = crate::test_support::max_error(&actual, &expected);
        assert!(error < 2e-5, "step={step} {space:?} {offset:?} level={level}: {error}");
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.pages.lock().unwrap().bytes() > 0);
        let sources = &r.scene.as_ref().unwrap().scale_sources;
        assert_eq!(sources.entries[&source_at(&doc,0)].blend_space, space);
        if space == BlendSpace::Perceptual {
            assert!(!source_copy_ready(&r,doc.scene(),source_at(&doc,0),extent,level));
        }
        if level == 0 {
            let reference = pixels(&exact, crate::test_support::document_texture(&exact));
            let error = crate::test_support::max_error(&actual, &reference);
            assert!(error < 2e-5, "native {space:?}: {error}");
        }
        let actual = r.readback_srgb_rgba8().unwrap();
        let expected = exact.readback_srgb_rgba8().unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn clipped_contacts_do_not_require_unmaterialized_source_levels() {
    let doc = Document::new(PortableId::random(), 517, 259, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    let expected = display_pixels(&r);
    let dab = crate::tests::test_dab([10000., 10000.], [0.8, 0.2, 0.1, 1.], 1.);
    let batch = dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    for kind in [DabBatchKind::Persistent, DabBatchKind::Preview] {
        let batch = DabBatch { kind, ..batch.clone() };
        crate::test_support::submit_ready(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        assert_eq!(display_pixels(&r), expected);
        assert!(r.paint_layers.iter().all(|layer| layer.pages.is_empty()));
        assert!(r.preview_pages.is_empty());

    }
}

#[test]
fn native_graph_admits_a_full_4k_view_with_bounded_scratch() {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size[0] = 4096;
    composition_mut(&mut doc).size[1] = 4096;
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    for count in [1, 32] {
        while doc.scene().order().len() <= count {
            let layer = copy_paint(&mut doc,0);
            doc.artwork.occurrences.get_mut(layer).unwrap().opacity = 0.35;
            insert_occurrence(&mut doc, layer, 0);
        }
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.view.width_px = doc.composition().size[0];
        frame.view.height_px = doc.composition().size[1];
        crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);
        assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(0));
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        let cache = r.scale_display.as_ref().unwrap();
        let scene = r.scene.as_ref().unwrap();
        assert_eq!(cache.plan.level, 0);
        assert!(cache.graph.root.is_some());
        assert!(cache.pages.lock().unwrap().retained_pages().iter().all(|page|page.key.source.is_none() || page.key.level!=0));
        let bytes = resident_bytes_with_pool(cache, scene);
        assert!(bytes <= r.memory.display_owner().grant().gpu_bytes, "layers={count}, bytes={bytes}");
        assert!(scene.used.iter().all(|used| !used.occupied()));
    }
}

#[test]
fn reduced_photo_stacks_use_bounded_working_tiles() {
    let doc = reduced_photo_stack();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 1500;
    frame.view.height_px = 1000;
    frame.view.document_to_surface = [0.1578, 0., 0., 0.1578, 0., 0.];
    r.begin_frame_memory(frame);
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(2));
    crate::test_support::submit_settled(&mut r, frame);
    let cache = r.scale_display.as_ref().unwrap();
    let scene = r.scene.as_ref().unwrap();
    assert!(cache.output.is_empty() && cache.pixels.root().is_some(), "working images stay tile-sized");
    assert!(scene.pool.iter().all(|p| p.texture.width() == PAGE_SIZE && p.texture.height() == PAGE_SIZE));
    let stored = cache.pages.lock().unwrap().bytes();
    let optional = stored - cache.pinned_page_bytes();
    let bytes = resident_bytes_with_pool(cache, scene) - optional;
    assert!(bytes <= crate::memory::CACHE_BYTES, "resident bytes beside {optional} optional exact bytes={bytes}");

}

#[test]
fn partial_reduced_views_admit_the_visible_window() {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size[0] = 32768;
    composition_mut(&mut doc).size[1] = 16384;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for detail in [1, 2, 4] {
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.view.width_px = 960;
        frame.view.height_px = 640;
        let scale = 1. / (1 << detail) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, -1600. * scale, -900. * scale];
        r.begin_frame_memory(frame);
        assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(detail));
        let plan = view_plan(frame, detail).unwrap();
        assert!(plan.bounds.area() < PixelRect::full(plan.extent).area());
    }
}

#[test]
fn rotated_reduced_stacks_admit_bounded_source_working_storage() {
    let doc = reduced_photo_stack();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 1600; frame.view.height_px = 1000;
    let (s, c) = std::f32::consts::FRAC_PI_4.sin_cos();
    let scale = 0.307;
    frame.view.document_to_surface = [scale * c, scale * s, -scale * s, scale * c,
        800. - scale * (c * doc.composition().size[0] as f32 - s * doc.composition().size[1] as f32) * 0.5,
        500. - scale * (s * doc.composition().size[0] as f32 + c * doc.composition().size[1] as f32) * 0.5];
    r.begin_frame_memory(frame);
    assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(1));
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    let cache = r.scale_display.as_ref().unwrap();
    let scene = r.scene.as_ref().unwrap();
    let bytes = resident_bytes_with_pool(cache, scene);
    assert!(bytes <= crate::memory::CACHE_BYTES, "resident bytes={bytes}");

    assert!(scene.used.iter().all(|used| !used.occupied()));
}

#[test]
fn streamed_sources_match_cached_pixels_through_masks_paint_and_admission_changes() {
    std::thread::Builder::new().stack_size(8 << 20).spawn(streamed_sources_match_cached_pixels).unwrap().join().unwrap();
}

fn streamed_sources_match_cached_pixels() {
    let mut doc = document_at([2053, 1541]);
    let extent = doc.composition().size;
    for _ in 0..63 {
        let layer = copy_paint(&mut doc,0);
        doc.artwork.occurrences.get_mut(layer).unwrap().opacity = 0.35;
        doc.artwork.occurrences.get_mut(layer).unwrap().blend = layer_core::LayerBlend::SoftLight;
        insert_occurrence(&mut doc, layer, 0);
    }
    let owner=doc.scene().order()[0];
    coverage_mask(&mut doc,owner, [0, 0], Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 280., y: 210. }, layer_core::Point { x: 1600., y: 260. },
        layer_core::Point { x: 1700., y: 1200. }, layer_core::Point { x: 330., y: 1100. },
    ]).unwrap()));
    let mut streamed = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut cached = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    for state in 0..6 {
        for i in 0..doc.scene().order().len() { occurrence_mut(&mut doc,i).visible = state != 3 || i < 2; }
        occurrence_mut(&mut doc,0).mask.as_mut().unwrap().inverted = state == 2;
        let dab = crate::tests::test_dab([420. + state as f32 * 7., 310.], [0.8, 0.2, 0.1, 1.], 0.7);
        let mut batch = dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        if state < 4 { batch.kind = DabBatchKind::Preview; }
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = (2..=4).contains(&state);
        frame.view.width_px = extent[0]; frame.view.height_px = extent[1];
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
        if state == 1 || state >= 4 {
            frame.dabs = std::slice::from_ref(&dab);
            frame.dab_batches = std::slice::from_ref(&batch);
        }
        crate::memory::memory_tests::legacy_scene_frame(&mut streamed, frame);
        submit_published(&mut streamed, frame);
        let full = streamed.scale_display.as_ref().unwrap();
        let full_plan = full.plan;
        let complete = display_pixels(&streamed);
        frame.view.width_px = 192; frame.view.height_px = 128;
        let [x, y] = if state == 2 { [-910., -660.] } else { [-160., -110.] };
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, x, y];
        crate::memory::memory_tests::legacy_scene_frame(&mut cached, frame);
        submit_published(&mut cached, frame);
        let window = cached.scale_display.as_ref().unwrap();
        let actual = display_pixels(&cached);
        let origin = [window.plan.bounds.min_x() >> 1, window.plan.bounds.min_y() >> 1];
        for (i, pixel) in actual.iter().enumerate() {
            let x = origin[0] + i as u32 % window.plan.size[0];
            let y = origin[1] + i as u32 / window.plan.size[0];
            let expected = complete[(y * full_plan.size[0] + x) as usize];
            assert!(pixel.iter().zip(expected).all(|(a, b)| (a - b).abs() < 2e-5),
                "state {state}, [{x}, {y}]: {pixel:?} != {expected:?}");
        }
        let scene = streamed.scene.as_ref().unwrap();
        let bytes = resident_bytes_with_pool(full, scene);
        assert!(bytes <= crate::memory::CACHE_BYTES, "state {state}, bytes {bytes}");
        assert!(scene.used.iter().all(|used| !used.occupied()));

        assert_presentation_mip(&streamed);
    }
}

#[test]
fn streamed_sources_reuse_valid_finer_pages_without_native_decoding() {
    let doc = document_at([1027, 773]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, frame);
    let expected = display_pixels(&r);
    let mut cache = r.scale_display.take().unwrap();
    let mut scene = r.scene.take().unwrap();
    let mut commands = scene.scale_commands.take().unwrap();
    let id=source_at(&doc,0);
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
    scene.begin_frame();
    {let mut work=crate::test_support::work(false);cache.prepare_source(&mut scene,&mut r,frame,&mut encoder,id,SourceRequest {plan:display_mips::Plan::at(frame.document_extent,2),required:PixelRect::full(frame.document_extent).into(),covered:PixelRect::EMPTY},Class::Prefetch,&mut work).unwrap();}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    assert!(!source_chunks(&cache,id,2).is_empty());
    cache.pages.lock().unwrap().remove_source_level(id,3);
    for missing in [None, Some([4, 3])] {
        if let Some(tile) = missing {
            assert!(source_chunks(&cache,id,2).contains(&tile));
            cache.pages.lock().unwrap().discard_source_chunks(id,2,page_rect(tile));
        }
        let updates = scene.scale_sources.entries[&id].updates;
        cache.graph = Default::default();
        evict_composite_pages(&cache,&[cache.plan.bounds]);
        scene.begin_frame();
        commands.begin();
        crate::scene::scale::tests::prepare_fixture_frame(&r, frame, &scene);
        cache.prepare_graph(&r, frame, &scene).unwrap();
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        {let mut work=crate::test_support::work(false);cache.render(&mut scene, &mut r, frame,
            &mut Encoding { encoder: &mut encoder, commands: &mut commands, retained:0, budget:&mut work }).unwrap();}
        scene.encode_jobs(&mut r, &mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        assert_eq!(scene.scale_sources.entries[&source_at(&doc,0)].updates, updates + u64::from(missing.is_some()),
            "only gaps in retained finer pages may require native reduction");
        let actual = pixels(&r, cache.texture());
        assert!(actual.iter().zip(&expected).all(|(a, b)| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-5)));
    }
    scene.scale_commands = Some(commands);
    r.scene = Some(scene);
    r.scale_display = Some(cache);
    assert_presentation_mip(&r);
}

#[test]
fn source_window_growth_remains_admitted_during_rotated_pans() {
    let doc = reduced_photo_stack();
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    for step in 0..24 {
        let angle = step as f32 / 23. * std::f32::consts::TAU;
        let (s, c) = angle.sin_cos();
        let [x, y] = [(0.5 + 0.4 * c) * doc.composition().size[0] as f32, (0.5 + 0.4 * s) * doc.composition().size[1] as f32];
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.view.width_px = 1600; frame.view.height_px = 1000;
        frame.view.document_to_surface = [0.5 * c, 0.5 * s, -0.5 * s, 0.5 * c,
            800. - 0.5 * (c * x - s * y), 500. - 0.5 * (s * x + c * y)];
        r.begin_frame_memory(frame);
        assert_eq!(request(&r, frame).ok().map(|q| q.plan.level), Some(1), "step {step}");
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(r.scale_display.is_some());
        let cache = r.scale_display.as_ref().unwrap();
        let scene = r.scene.as_ref().unwrap();
        let bytes = resident_bytes_with_pool(cache, scene);
        assert!(bytes <= crate::memory::CACHE_BYTES, "step {step}: {bytes}");
    }
}

#[test]
fn view_windows_reuse_overlap_and_preserve_global_sampling() {
    let doc = document_at([2053, 1541]);
    let extent = doc.composition().size;
    for detail in [0, 1, 2, 4] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut exact = crate::memory::memory_tests::renderer(doc.composition().color,
            layer_core::DeviceMemoryFacts { budget_bytes: Some(256 << 20), ..Default::default() }).unwrap();
        let mut whole = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        exact.test.reference = true;
        let mut frame = packet(doc.scene(), extent);
        submit_published(&mut exact, frame);
        let oracle = pixels(&exact, crate::test_support::document_texture(&exact));
        frame.composite_all = false;
        frame.view.width_px = 192;
        frame.view.height_px = 128;
        let mut presenter = crate::test_support::float_presenter(&r);
        let mut reference_presenter = crate::test_support::float_presenter(&whole);
        let surface = |renderer: &WgpuRasterizer, presenter: &mut crate::present::ViewportPresenter, view| {
            let (texture, target) = create_color_target(&renderer.device, [192, 128], "composition viewport oracle");
            presenter.present(renderer, &target, view, [0.2; 4]).unwrap();
            pixels(renderer, &texture)
        };
        let views = [
            [1., 0., 0., 1., -300., -200.],
            [1., 0., 0., 1., -310., -210.],
            [1., 0., 0., 1., -750., -410.],
            [0., 1., -1., 0., 900., -510.],
            [-1.5, 0., 0., 0.75, 1700., -300.],
            [1., 0., 0., 1., -1900., -1430.],
        ];
        let mut work = 0;
        let mut revision = None;
        for (i, transform) in views.into_iter().enumerate() {
            let transform = transform.map(|n| n / (1 << detail) as f32);
            frame.view.document_to_surface = transform;
            submit_published(&mut r, frame);
            let current = r.canvas_preview_revision();
            assert!(revision.is_none_or(|previous| previous == current), "camera motion preserves artwork revision");
            revision = Some(current);
            let cache = r.scale_display.as_ref().unwrap();
            assert_eq!(cache.plan.level, display_mips::view_level(transform, 4).unwrap());
            assert!(cache.graph.root.is_some());
            if detail == 0 { assert!(cache.plan.bounds.area() < PixelRect::full(extent).area() / 2); }
            if let Some(overview) = &cache.overview { assert!(overview.plan.level > cache.plan.level); }

            assert!(quality(&display_pixels(&r), &oracle, cache.plan, cache.texture().format())[2] < 2e-5, "view={i}");
            assert_presentation_mip(&r);
            if i == 1 { assert_eq!(r.metrics.composited_pixels, work); }
            if i == 2 { assert!(r.metrics.composited_pixels - work < cache.plan.size.map(u64::from).into_iter().product()); }
            work = r.metrics.composited_pixels;
            let actual = surface(&r, &mut presenter, frame.view);
            let scale = 1. / (1 << cache.plan.level) as f32;
            let mut full = packet(doc.scene(), extent);
            full.view.width_px = extent[0];
            full.view.height_px = extent[1];
            full.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            submit_published(&mut whole, full);
            let complete = whole.scale_display.as_ref().unwrap();
            assert_eq!(complete.plan.bounds, PixelRect::full(extent));
            assert_eq!(complete.plan.level, cache.plan.level);
            assert!(quality(&display_pixels(&whole), &oracle, complete.plan, complete.texture().format())[2] < 2e-5);
            let reference = surface(&whole, &mut reference_presenter, frame.view);
            let error = crate::test_support::max_error(&actual, &reference);
            assert!(error < 0.0001, "window presentation detail={detail} view={i} plan={:?} error={error}", cache.plan);
        }
        let dab = crate::tests::test_dab([1980., 1480.], [0.9, 0.2, 0.1, 1.], 0.7);
        let batch = dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        frame.dabs = std::slice::from_ref(&dab);
        frame.dab_batches = std::slice::from_ref(&batch);
        submit_published(&mut r, frame);
        assert!(r.canvas_preview_revision() > revision.unwrap(), "painting advances artwork revision");
        submit_published(&mut exact, frame);
        let cache = r.scale_display.as_ref().unwrap();
        let oracle = pixels(&exact, crate::test_support::document_texture(&exact));
        assert!(quality(&display_pixels(&r), &oracle, cache.plan, cache.texture().format())[2] < 2e-5);
        if let Some(overview) = &cache.overview {
            assert!(quality(&pixels(&r, overview.texture()), &oracle, overview.plan, overview.texture().format())[2] < 2e-5);
        }
        assert_presentation_mip(&r);
    }
}

#[test]
fn small_offset_source_remains_visible_beyond_its_local_extent() {
    let mut doc = document_at([256, 256]);
    let extent = [2048, 1536];
    composition_mut(&mut doc).size[0] = extent[0]; composition_mut(&mut doc).size[1] = extent[1];
    occurrence_mut(&mut doc,0).offset = [896, 640];
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.385, 0., 0., 0.385, 0., 0.];
    submit_published(&mut r, frame);
    let display = display_pixels(&r);
    assert!(display.iter().any(|p| p[3] > 0.99 && p[2] > 0.05), "offset source contributes to presentation");
    let exact = r.readback_srgb_rgba8().unwrap();
    let center = ((768 * extent[0] + 1024) * 4) as usize;
    assert_eq!(&exact[center..center + 4], &[42, 64, 80, 255]);
}

#[test]
fn offset_sources_and_masks_compose_in_document_scale_and_keep_exact_queries() {
    let source_extent = [517, 259];
    let extent = [389, 277];
    let mut doc = document_at(source_extent);
    composition_mut(&mut doc).size[0] = extent[0];
    composition_mut(&mut doc).size[1] = extent[1];
    occurrence_mut(&mut doc,0).opacity = 0.71;
    let owner=doc.scene().order()[0];
    let mask_source=coverage_mask(&mut doc,owner, [17, -11], Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 30., y: 10. }, layer_core::Point { x: 390., y: 10. },
        layer_core::Point { x: 390., y: 200. }, layer_core::Point { x: 30., y: 200. },
    ]).unwrap()));
    doc.artwork.coverage.get_mut(mask_source).unwrap().default_coverage=0.63;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for (pose, offset) in [[16, 32], [-120, 32], [30, 4], [-17, 21], [37, 0], [0, 101]].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).offset = offset;
        occurrence_mut(&mut doc,0).mask.as_mut().unwrap().inverted = pose % 2 != 0;
        for enabled in [true, false] {
            occurrence_mut(&mut doc,0).mask.as_mut().unwrap().enabled = enabled;
            for level in [0, 1, 2, 3] {
                let scale = 1. / (1 << level) as f32;
                let mut frame = packet(doc.scene(), extent);
                frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
                submit_published(&mut r, frame);
                submit_published(&mut exact, frame);
                let cache = r.scale_display.as_ref().unwrap();
                assert_eq!(cache.plan.level, level);

                let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
                assert!(error[0] < 0.004 && error[1] < 0.04, "pose={pose} level={level}: {error:?}");
                assert_presentation_mip(&r);
                let actual = r.readback_srgb_rgba8().unwrap();
                let expected = exact.readback_srgb_rgba8().unwrap();
                assert!(actual == expected, "exact offset export pose={pose} level={level} mask={enabled}");
            }
        }
    }
}

#[test]
fn a_coarser_source_level_averages_the_transparent_rows_a_finer_level_ends_before() {
    let extent = [389, 277];
    let mut doc = document_at([517, 259]);
    composition_mut(&mut doc).size[0] = extent[0];
    composition_mut(&mut doc).size[1] = extent[1];
    occurrence_mut(&mut doc,0).offset = [37, 0];
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for level in [2, 3] {
        let scale = 1. / (1 << level) as f32;
        let mut frame = packet(doc.scene(), extent);
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        submit_published(&mut r, frame);
        submit_published(&mut exact, frame);
        let cache = r.scale_display.as_ref().unwrap();
        assert_eq!(cache.plan.level, level);
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
        assert!(error[2] < 2e-3, "level={level}: {error:?}");
    }
}

#[test]
fn source_windows_keep_overlap_and_sample_global_coordinates() {
    let source_extent = [4099, 2053];
    let mut doc = document_at(source_extent);
    let extent = [513, 257];
    composition_mut(&mut doc).size[0] = extent[0]; composition_mut(&mut doc).size[1] = extent[1];
    let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    r.prepare_moving_layer(Some(doc.scene().order()[0]));
    let mut updates = 0;
    let mut previous = None;
    for (step, offset) in [[-2050, -1000], [-2100, -1000], [-2350, -1000], [-2100, -1000], [-3575, -1790], [700, 400]].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).offset = offset;
        let mut frame = packet(doc.scene(), extent);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);
        submit_published(&mut r, frame); submit_published(&mut exact, frame);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while crate::scene::scale::tests::publication_pending(&r) || r.moving_decode.as_ref().is_some_and(|(_, queue)| queue.pending()) {
            assert!(std::time::Instant::now() < deadline, "moving source tiles must decode and publish");
            r.wait_idle().unwrap();
            crate::test_support::submit_ready(&mut r, frame).unwrap();
        }
        let cache = r.scale_display.as_ref().unwrap();

        let sources = &r.scene.as_ref().unwrap().scale_sources;
        let bytes=sources.entries.keys().flat_map(|id|source_textures(&r,*id,None)).map(|texture|texture_bytes(&texture)).sum::<u64>();
        assert!(bytes < 8 << 20, "small output retains bounded source windows: {bytes}");
        let source = &sources.entries[&source_at(&doc,0)];
        let images = source_textures(&r,source_at(&doc,0),None);
        if step == 1 || step == 3 {
            assert_eq!(source.updates, updates, "motion inside the retained window reuses source pixels");
            assert_eq!(previous.as_ref().unwrap(), &images, "covered motion retains the same textures");
        }
        previous = Some(images);
        updates = source.updates;
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
        assert!(error[0] < 0.004 && error[1] < 0.04, "step={step}: {error:?}");
        assert_presentation_mip(&r);
    }
    r.prepare_moving_layer(None);
    occurrence_mut(&mut doc,0).offset = [-100, -100];
    let frame = packet(doc.scene(), extent);
    submit_published(&mut r, frame); submit_published(&mut exact, frame);
    let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
    assert!(error[0] < 0.004 && error[1] < 0.04, "native offset after a partial source: {error:?}");
}

#[test]
fn cold_identity_sources_prepare_adjacent_detail_without_redecoding() {
    let doc = document_at([1025, 769]);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), doc.composition().size);
    frame.view.width_px = 512; frame.view.height_px = 512;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, frame);
    let source = &r.scene.as_ref().unwrap().scale_sources.entries[&source_at(&doc,0)];
    let updates = source.updates;
    assert_eq!(updates, 20);
    assert!(source_levels(r.scale_display.as_ref().unwrap(),source_at(&doc,0)).contains_key(&3));
    let mut cache=r.scale_display.take().unwrap();let mut scene=r.scene.take().unwrap();
    let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());scene.begin_frame();
    {let mut work=crate::test_support::work(false);cache.prepare_source(&mut scene,&mut r,frame,&mut encoder,source_at(&doc,0),SourceRequest {plan:display_mips::Plan::at(frame.document_extent,2),required:PixelRect::full(frame.document_extent).into(),covered:PixelRect::EMPTY},Class::Prefetch,&mut work).unwrap();}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
    let updates=scene.scale_sources.entries[&source_at(&doc,0)].updates;
    let levels=source_levels(&cache,source_at(&doc,0)).keys().copied().collect::<BTreeSet<_>>();
    let finer = source_textures_in(&cache,source_at(&doc,0),Some(2));
    assert!(!finer.is_empty());r.scale_display=Some(cache);r.scene=Some(scene);
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_published(&mut r, frame);
    let source = &r.scene.as_ref().unwrap().scale_sources.entries[&source_at(&doc,0)];
    assert_eq!(source.updates, updates);
    assert_eq!(source_textures(&r,source_at(&doc,0),Some(2)), finer);
    assert_eq!(source_levels(r.scale_display.as_ref().unwrap(),source_at(&doc,0)).keys().copied().collect::<BTreeSet<_>>(),levels,"scale changes must not repeatedly promote optional detail");
}

#[test]
fn source_retention_reserves_images_that_composition_allocates_later() {
    for material in [false, true] {
        let mut doc = document_at([33, 17]);
        composition_mut(&mut doc).size[0] = 4096; composition_mut(&mut doc).size[1] = 4096;
        if material {
            use layer_core::raster::*;
            let plane = RasterPlane::WatercolorWetness;
            let tile = RasterTile::backed(TileBlob::encode(plane.descriptor(doc.composition().color), &vec![255; 256 * 256]).unwrap());
            let mut data = RasterData { watercolor: Some(RasterWatercolor {
                wet_edge: 0.9, burnt_edge: 0.6, edge_width: 8.,
            }), ..Default::default() };
            for y in 0..16 {
                for x in 0..16 {
                    data.tiles.insert(TileKey { plane, coordinate: [x, y] }, tile.clone());
                }
            }
            paint_mut(&mut doc,0).domain=doc.composition().size;
            paint_mut(&mut doc,0).raster = RasterRevision::backed(data);
            occurrence_mut(&mut doc,0).offset = [32, 32];
        }
        let front = copy_paint(&mut doc,0);
        doc.artwork.occurrences.get_mut(front).unwrap().opacity = 0.5;
        insert_occurrence(&mut doc, front, 0);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        if material {
            submit_published(&mut r, frame);
            r.wait_idle().unwrap();
            r.scale_display = None;
            r.scene = None;
            frame.reset_layers = false;
        }
        let cache = Cache::new(&r, Request { plan: display_mips::Plan::at(frame.document_extent, 2), evaluation: Evaluation::Display }, doc.scene().order().len());
        assert!(cache.output.is_empty() && cache.pixels.next().is_none());
        r.scale_display = Some(cache);
        submit_published(&mut r, frame);
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.pixels.root().is_some() && cache.pixels.next().is_some());
        let commands = r.scene.as_ref().unwrap().scale_commands.as_ref().unwrap();
        let scene = r.scene.as_ref().unwrap();
        let material_bytes = scene.pool.iter().map(PageSurface::storage_bytes).sum::<u64>();
        let total = cache.storage_bytes() + commands.storage_bytes() + material_bytes;
        assert!(total <= crate::memory::CACHE_BYTES, "prospective sources and later outputs/material must stay admitted: {total}");
    }
}

#[test]
fn source_windows_derive_across_origins_and_fill_only_missing_pages() {
    let doc = document_at([1027, 773]);
    let extent = doc.composition().size;
    let id = source_at(&doc,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    submit_published(&mut r, frame);
    let mut exact=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();exact.test.reference=true;
    submit_published(&mut exact,frame);
    let reference=pixels(&exact,crate::test_support::document_texture(&exact));
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, frame);
    let mut scene = r.scene.take().unwrap();let mut cache=r.scale_display.take().unwrap();
    let full = PixelRect::full(extent);
    let window = PixelRect::new(256, 256, extent[0], extent[1]);
    for (fine, coarse) in [(full, window), (window, full)] {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        cache.pages.lock().unwrap().remove_source_level(id,1);
        scene.begin_frame();
        {let mut work=crate::test_support::work(false);cache.prepare_source(&mut scene,&mut r,frame,&mut encoder,id,SourceRequest {
            plan:display_mips::Plan::window(extent,1,fine),required:fine.into(),covered:PixelRect::EMPTY,
        },Class::Prefetch,&mut work).unwrap();}
        cache.pages.lock().unwrap().remove_source_level(id,2);
        let plan=display_mips::Plan::window(extent,2,coarse);
        let updates=scene.scale_sources.entries[&id].updates;
        scene.begin_frame();
        {let mut work=crate::test_support::work(false);cache.prepare_source(&mut scene,&mut r,frame,&mut encoder,id,SourceRequest {plan,required:fine.intersect(coarse).into(),covered:PixelRect::EMPTY},Class::Prefetch,&mut work).unwrap();}
        let complete=source_chunks(&cache,id,2).len();
        assert_eq!(complete,page_coordinates(fine.intersect(coarse)).count());
        assert_eq!(scene.scale_sources.entries[&id].updates,updates);
        scene.begin_frame();
        {let mut work=crate::test_support::work(false);cache.prepare_source(&mut scene,&mut r,frame,&mut encoder,id,SourceRequest {plan,required:coarse.into(),covered:PixelRect::EMPTY},Class::Prefetch,&mut work).unwrap();}
        assert_eq!(scene.scale_sources.entries[&id].updates-updates,(page_coordinates(coarse).count()-complete) as u64);
        r.uploads.finish(&encoder);encoder.submit(&r.queue);
        let image=source_image(&r,&cache,&scene.scale_sources.entries[&id],id,plan);
        let actual=pixels(&r,&image.texture);
        let error=quality(&actual,&reference,plan,image.texture.format());
        let first=(error[2]>=1e-6).then(||actual.iter().enumerate().find_map(|(index,actual)| {
            let side=1<<plan.level;let x=plan.bounds.min_x()+index as u32%plan.size[0]*side;
            let y=plan.bounds.min_y()+index as u32/plan.size[0]*side;let mut expected=[0.;4];let mut count=0.;
            for yy in y..(y+side).min(plan.bounds.max_y()) {for xx in x..(x+side).min(plan.bounds.max_x()) {
                for c in 0..4 {expected[c]+=reference[(yy*extent[0]+xx) as usize][c];}count+=1.;
            }}
            let expected=committed_color(image.texture.format(),expected.map(|value|value/count));
            actual.iter().zip(expected).any(|(a,b)|(*a-b).abs()>=1e-6).then_some(([x,y],*actual,expected))
        }));
        assert!(error[2]<1e-6,"source window fine={fine:?} coarse={coarse:?} error={error:?} first={first:?}");
        assert_eq!(source_chunks(&cache,id,2).len()==page_coordinates(full).count(),coarse==full);
    }
}

#[test]
fn moving_layer_crossing_identity_keeps_the_prepared_source() {
    let extent = [1024, 512];
    let mut doc = document_at(extent);
    let id = source_at(&doc,0);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.prepare_moving_layer(Some(doc.scene().order()[0]));
    let mut prepared = None;
    let mut prepared_level = None;
    for (step, x) in [10, 0, 0, -10, 0].into_iter().enumerate() {
        occurrence_mut(&mut doc,0).offset = [x, 0];
        let mut frame = packet(doc.scene(), extent);
        frame.blend_space = layer_core::BlendSpace::Perceptual;
        frame.time_seconds = step as f32 * 0.1;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        submit_published(&mut r, frame);
        let source = &r.scene.as_ref().unwrap().scale_sources.entries[&id];
        let level = *prepared_level.get_or_insert_with(|| source_level(r.scale_display.as_ref().unwrap().plan.level,
            source_frame(Some(&r.scene.as_ref().unwrap().scale_sources), doc.scene(), id), false, 0));
        let current = (source.updates,source_textures(&r,id,Some(level)));
        assert_eq!(prepared.get_or_insert_with(|| current.clone()), &current,
            "moving an unchanged photo through its original position must reuse its pixels");
        assert!(r.next_frame()!=layer_render::FrameDemand::Work, "an unfinished move must not schedule exact refinement");
        let work = r.metrics.composited_pixels;
        frame.composite_all = false;
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert_eq!(r.metrics.composited_pixels, work, "an unchanged position must not refine");
    }
    r.prepare_moving_layer(None);
    occurrence_mut(&mut doc,0).offset = [0, 0];
    let mut frame = packet(doc.scene(), extent);
    frame.blend_space = layer_core::BlendSpace::Perceptual;
    frame.composite_all = false;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert!(r.next_frame()==layer_render::FrameDemand::Work, "finishing a move must allow exact refinement");
    let mut fresh = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_published(&mut fresh, frame);
    let actual = display_pixels(&r);
    let expected = display_pixels(&fresh);
    let describe = |r:&WgpuRasterizer|source_levels(r.scale_display.as_ref().unwrap(),id).into_iter().map(|(level,chunks)|(level,chunks.len())).collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    let error = actual.iter().zip(&expected).flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
        .fold(0f32, f32::max);
    eprintln!("display reduction histories max={error}; cached {:?}; fresh {:?}", describe(&r), describe(&fresh));
    assert!(error < 1e-6, "cached versus fresh display reduction: {error}");
}

#[test]
fn deferred_transform_preview_samples_the_final_surface_without_a_canvas_image() {
    let mut doc = document_at([1025, 513]);
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    let extent = [641, 385];
    composition_mut(&mut doc).size[0] = extent[0]; composition_mut(&mut doc).size[1] = extent[1];
    occurrence_mut(&mut doc,0).opacity = 0.71;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut exact_presenter = crate::test_support::float_presenter(&exact);
    let mut presenter = crate::test_support::float_presenter(&r);
    let render = |r: &WgpuRasterizer, presenter: &mut crate::ViewportPresenter, view: layer_render::ViewState| {
        let (texture, target) = create_color_target(&r.device, [view.width_px, view.height_px], "deferred presentation oracle");
        presenter.present(r, &target, view, [1.; 4]).unwrap();
        pixels(r, &texture)
    };
    for placement in [[1., 0., 0., 1., -153.25, -51.5], [0.5, 0.1, -0.15, 0.6, 30., 5.], [-0.6, 0.1, 0.15, 0.5, 570., 7.]] {
        let preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc, 0), moving: false, selection: None,
            transform: layer_core::ImageTransform::affine(layer_core::Affine(placement)) };
        for renderer in [&mut r, &mut exact] { renderer.set_transform_preview(Some(&preview)).unwrap(); }
        for camera in [[0.25, 0., 0., 0.25, 8.25, 7.5], [0.19, 0., 0., 0.19, 8.25, 7.5], [0.13, 0., 0., 0.13, 8.25, 7.5],
            [0.17, 0.075, -0.075, 0.17, 37.5, 6.25], [0.14, -0.06, 0.02, 0.24, 8.25, 42.5]] {
            let mut frame = packet(doc.scene(), extent);
            frame.view.width_px = 192; frame.view.height_px = 128;
            frame.view.document_to_surface = camera;
            let inverse = layer_core::Affine(camera).inverse().unwrap();
            let inside = |x, y| {
                let p = inverse.map(layer_core::Point { x, y });
                p.x >= 0. && p.y >= 0. && p.x < extent[0] as f32 && p.y < extent[1] as f32
            };
            crate::test_support::submit_ready(&mut r, frame).unwrap();
            crate::test_support::submit_settled(&mut exact, frame);
            assert!(r.scale_display.as_ref().unwrap().output.is_empty());
            assert!(r.scale_display.as_ref().unwrap().pixels.next().is_none());
            let actual = render(&r, &mut presenter, frame.view);
            let mut high = frame.view;
            high.width_px *= 8; high.height_px *= 8;
            high.document_to_surface = high.document_to_surface.map(|n| n * 8.);
            let reference = render(&exact, &mut exact_presenter, high);
            let point = render(&exact, &mut exact_presenter, frame.view);
            let expected: Vec<[f32; 4]> = (0..frame.view.height_px).flat_map(|y| (0..frame.view.width_px).map(move |x| (x, y))).map(|(x, y)| {
                if !inside(x as f32 + 0.5, y as f32 + 0.5) {
                    return point[(y * frame.view.width_px + x) as usize];
                }
                let mut value = [0.; 4];
                let mut count = 0.;
                for yy in y * 8..(y + 1) * 8 { for xx in x * 8..(x + 1) * 8 {
                    if !inside((xx as f32 + 0.5) / 8., (yy as f32 + 0.5) / 8.) { continue; }
                    for c in 0..4 { value[c] += reference[(yy * high.width_px + xx) as usize][c]; }
                    count += 1.;
                }}
                value.map(|c| c / count)
            }).collect();
            let mut errors: Vec<_> = actual.iter().zip(&expected).map(|(a,b)| a.iter().zip(b).map(|(a,b)| (a-b).abs()).fold(0., f32::max)).collect();
            errors.sort_by(f32::total_cmp);
            let mean = errors.iter().sum::<f32>() / errors.len() as f32;
            let p99 = errors[errors.len() * 99 / 100];
            assert!(mean < 0.004 && p99 < 0.04);
        }
    }
}

#[test]
fn deferred_transform_preview_navigator_preserves_coarse_artwork() {
    let mut doc = document_at([1025, 513]);
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    composition_mut(&mut doc).size[0] = 641; composition_mut(&mut doc).size[1] = 385;
    occurrence_mut(&mut doc,0).opacity = 0.71;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let overview = |r: &WgpuRasterizer, size: [u32; 2]| {
        let mut presenter = crate::ViewportPresenter::for_overview_surface(
            r, wgpu::TextureFormat::Rgba32Float, crate::SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
        presenter.set_overviews(r, &[crate::OverviewPlacement {
            bounds: [0., 0., size[0] as f32, size[1] as f32], clip: None,
            work_area: [[-1000.; 2]; 4], outline_linear: [0.; 3], scale: 1., opacity: 1.,
        }]);
        let (texture, target) = create_color_target(&r.device, size, "placement navigator oracle");
        presenter.present_overviews(r, &target, size).unwrap();
        pixels(r, &texture)
    };
    for (step, placement) in [[1., 0., 0., 1., -153.25, -51.5], [0.5, 0.1, -0.15, 0.6, 30., 5.], [-0.6, 0.1, 0.15, 0.5, 570., 7.]].into_iter().enumerate() {
        let preview = layer_render::TransformPreview { transaction: 1, target: source_at(&doc, 0), moving: false, selection: None,
            transform: layer_core::ImageTransform::affine(layer_core::Affine(placement)) };
        for renderer in [&mut r, &mut exact] { renderer.set_transform_preview(Some(&preview)).unwrap(); }
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.time_seconds = step as f32 * 0.1;
        frame.view.document_to_surface = [0.25, 0., 0., 0.25, 8.25, 7.5];
        crate::test_support::submit_ready(&mut r, frame).unwrap(); crate::test_support::submit_settled(&mut exact, frame);
        let display = r.scale_display.as_ref().unwrap();
        assert!(display.coverage_complete() && display.pixels.committed_root().is_some(), "the display presents the transform preview");
        for size in [[128, 77], [64, 39], [32, 19], [16, 10], [7, 4]] {
            let actual = overview(&r, size);
            let high = size.map(|n| n * 8);
            let reference = overview(&exact, high);
            let mut errors = Vec::new();
            for y in 0..size[1] { for x in 0..size[0] {
                let mut expected = [0.; 4];
                for yy in y * 8..(y + 1) * 8 { for xx in x * 8..(x + 1) * 8 {
                    for c in 0..4 { expected[c] += reference[(yy * high[0] + xx) as usize][c] / 64.; }
                }}
                errors.push(actual[(y * size[0] + x) as usize].iter().zip(expected).map(|(a,b)| (a-b).abs()).fold(0., f32::max));
            }}
            errors.sort_by(f32::total_cmp);
            let mean = errors.iter().sum::<f32>() / errors.len() as f32;
            let p99 = errors[errors.len() * 99 / 100];
            assert!(mean < 0.03 && p99 < 0.25, "navigator placement={placement:?} size={size:?}: mean={mean} p99={p99}");
        }
    }
}

#[test]
fn deriving_partial_sources_preserves_completed_texels_between_refreshed_regions() {
    let doc = document_at([769, 769]);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, frame);
    prepare_layer_pages(&mut r, frame);
    let mut scene = r.scene.take().unwrap();
    let id = source_at(&doc,0);
    let mut cache=r.scale_display.take().unwrap();
    let plan=display_mips::Plan::at(extent,3);
    let expected=pixels(&r,&source_image(&r,&cache,&scene.scale_sources.entries[&id],id,plan).texture);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    for tile in [[0,0],[3,3]] {
        cache.pages.lock().unwrap().discard_source_chunks(id,3,page_rect(tile));
        scene.begin_frame();
        {let mut work=crate::test_support::work(false);cache.prepare_source(&mut scene,&mut r,frame,&mut encoder,id,SourceRequest {plan:display_mips::Plan::at(extent,1),required:page_rect(tile).into(),covered:PixelRect::EMPTY},Class::Prefetch,&mut work).unwrap();}
    }
    scene.begin_frame();
    {let mut work=crate::test_support::work(false);cache.prepare_source(&mut scene,&mut r,frame,&mut encoder,id,SourceRequest {plan,required:PixelRect::full(extent).into(),covered:PixelRect::EMPTY},Class::Prefetch,&mut work).unwrap();}
    r.uploads.finish(&encoder);encoder.submit(&r.queue);
    let actual=pixels(&r,&source_image(&r,&cache,&scene.scale_sources.entries[&id],id,plan).texture);
    assert!(actual.iter().flatten().zip(expected.iter().flatten()).all(|(a,b)|(a-b).abs()<1e-6));
    assert_eq!(source_chunks(&cache,id,3).len(),16);
}

#[test]
fn groups_clipping_and_all_blends_share_exact_stack_semantics() {
    let extent = [33, 19];
    let mut doc = document_at(extent);
    let paper=*doc.scene().order().last().unwrap();
    let base=paint_occurrence(&mut doc,"solid",Some(rgba8_source(extent,|_,_|[50,170,80,117])));
    doc.artwork.occurrences.get_mut(base).unwrap().opacity=0.81;
    let clipped=paint_occurrence(&mut doc,"solid",Some(rgba8_source(extent,|_,_|[230,30,120,193])));
    doc.artwork.occurrences.get_mut(clipped).unwrap().opacity=0.54;
    let clip_mask=coverage_mask(&mut doc,clipped,[0, 0],None);
    doc.artwork.coverage.get_mut(clip_mask).unwrap().default_coverage=0.42;
    doc.artwork.occurrences.get_mut(clipped).unwrap().mask.as_mut().unwrap().inverted=true;
    let inner=stack_occurrence(&mut doc,"inner",vec![clipped,base]);
    set_attachment(doc.artwork.occurrences.get_mut(clipped).unwrap(),true);
    let occurrence=doc.artwork.occurrences.get_mut(inner).unwrap(); occurrence.opacity=0.71; occurrence.blend=layer_core::LayerBlend::Multiply;
    let outer=stack_occurrence(&mut doc,"outer",vec![inner]);
    doc.artwork.occurrences.get_mut(outer).unwrap().opacity=0.63;
    let group_mask=coverage_mask(&mut doc,outer,[0, 0],None);
    doc.artwork.coverage.get_mut(group_mask).unwrap().default_coverage=0.61;
    let behind=paint_occurrence(&mut doc,"solid",Some(rgba8_source(extent,|_,_|[170,210,70,230])));
    doc.artwork.occurrences.get_mut(behind).unwrap().opacity=0.79;
    set_root_entries(&mut doc,vec![outer,behind,paper]);
    set_attachment(doc.artwork.occurrences.get_mut(clipped).unwrap(), true); reindex(&mut doc);
    let mut reduced = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut reads=Vec::new();
    for mode in layer_core::LayerBlend::ALL {
        occurrence_mut(&mut doc,0).blend = mode;
        occurrence_mut(&mut doc,2).blend = mode;
        for state in 0..4 {
            occurrence_mut(&mut doc,1).visible = state != 1;
            occurrence_mut(&mut doc,2).visible = state != 2;
            occurrence_mut(&mut doc,3).visible = state != 3;
            occurrence_mut(&mut doc,0).mask.as_mut().unwrap().enabled = state != 2;

            let mut frame = packet(doc.scene(), extent);
            frame.inspect_mask=(state==1).then_some(clipped);
            frame.composite_all = false;
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            crate::scene::sources::Inputs::begin_accepted_reads();
            submit_published(&mut reduced, frame);
            reads.extend(crate::scene::sources::Inputs::take_accepted_read_results());
            submit_published(&mut exact, frame);
            let cache = reduced.scale_display.as_ref().unwrap();
            let error = quality(&display_pixels(&reduced), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
            assert!(error[2] < 2e-5, "{mode:?} state={state}: {error:?}");
            assert_presentation_mip(&reduced);

            crate::scene::sources::Inputs::begin_accepted_reads();
            assert_settled(&mut reduced, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
            reads.extend(crate::scene::sources::Inputs::take_accepted_read_results());
        }
    }
    assert!(reads.iter().map(|(count,_)|count).sum::<usize>()>0,"groups and pass-through must consume authored sources");
    assert!(reads.iter().all(|(_,unexpected)|unexpected.is_empty()),"{reads:?}");
}

#[test]
fn pivot_halves_bound_edit_recomposition_independently_of_layer_count_and_preserve_untouched_regions() {
    assert_eq!(pivot_edit_work(8), pivot_edit_work(32), "an edit's records per frame and committed pages must not depend on the layer count");
}

fn pivot_edit_work(layers: usize) -> Vec<(Vec<u32>, u64)> {
    const PARTS: usize = 2;
    const WINDOW_REDUCTION: u32 = 1;
    let mut doc = document();
    let photo_source=paint_at(&doc,0).base.as_ref().unwrap().image.storage().clone();
    let paper=*doc.scene().order().last().unwrap();
    let mut entries:Vec<_>=(0..layers).map(|i| { let handle=copy_paint(&mut doc,0); doc.artwork.occurrences.get_mut(handle).unwrap().opacity=0.17+i as f32*0.02; handle }).collect();
    entries.push(paper);
    let middle=layers/2-1;
    let empty=paint_occurrence(&mut doc,"empty paint target",None); entries.insert(middle,empty); set_root_entries(&mut doc,entries);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    fn frame(scene: SceneView<'_>, extent: [u32; 2]) -> FramePacket<'_> {
        let mut p = packet(scene, extent);
        p.composite_all = false;
        p.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        p
    }
    crate::test_support::submit_published(&mut r, frame(doc.scene(), extent));
    crate::test_support::submit_published(&mut exact, frame(doc.scene(), extent));
    assert!(!r.scene.as_ref().unwrap().scale_sources.entries.contains_key(&source_at(&doc,middle)));
    let plan = r.scale_display.as_ref().unwrap().plan;
    let pages_under = |cell: PixelRect, levels: std::ops::Range<u32>| levels.map(|level| pages::coordinates(cell, level).count()).sum::<usize>();
    let mut work = Vec::new();
    let mut pivot = None;
    for (step, index) in [middle, 0, layers, middle, 0].into_iter().flat_map(|index| [index; 2]).enumerate() {
        let switched = pivot.replace(index) != Some(index);
        let mut dab = crate::tests::test_dab([30. + step as f32 * 47., 97. + (step % 2) as f32 * 60.], [0.9, 0.02, 0.1, 0.7], 1.);
        dab.radii = [21.; 2];
        let batch = DabBatch { stroke_id: layer_core::StrokeId(step as u64 + 1),
            ..dab_batch(source_at(&doc,index), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds()) };
        let cells: Vec<_> = pages::coordinates(pixel_rect(dab.bounds(), extent), plan.level)
            .map(|coordinate| pages::bounds(coordinate, plan.level, extent).intersect(plan.bounds)).collect();
        let half_reductions = cells.iter().map(|cell| PARTS * pages_under(*cell, 0..plan.level)).sum::<usize>() as u32;
        let half_pages = cells.iter().map(|cell| PARTS * pages_under(*cell, 0..plan.level + 1)).sum::<usize>() as u64;
        for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
            let batch = DabBatch { kind, ..batch.clone() };
            let p = FramePacket { dabs: &[dab], dab_batches: &[batch], ..frame(doc.scene(), extent) };
            let before = crate::test_support::PassCounts::snapshot();
            let mut records = Vec::new();
            crate::test_support::submit_published_observing(&mut r, p, |r| records.push(r.scene.as_ref().unwrap().scale_commands.as_ref().unwrap().cursor));
            let commits = crate::test_support::PassCounts::snapshot().delta(before).composite_commits;
            crate::test_support::submit_published(&mut exact, p);
            let building = switched && kind == DabBatchKind::Preview;
            let (record_bound, commit_bound) = if building { (WINDOW_REDUCTION + half_reductions, cells.len() as u64 + half_pages) } else { (WINDOW_REDUCTION, cells.len() as u64) };
            assert!(records.iter().all(|records| *records <= record_bound),
                "{layers} layers, step={step} layer={index} {kind:?}: records per frame {records:?} exceed {record_bound} for {} touched cells (building halves: {building})", cells.len());
            assert!(commits <= commit_bound,
                "{layers} layers, step={step} layer={index} {kind:?}: {commits} committed pages exceed {commit_bound} for {} touched cells (building halves: {building})", cells.len());
            let scene = r.scene.as_ref().unwrap();
            let cache = r.scale_display.as_ref().unwrap();
            assert_eq!(cache.graph.active(), Some(doc.scene().order()[index]), "step={step}: the edited layer is the pivot");
            assert!(cells.iter().all(|cell| cache.half_cells(*cell) == [*cell]), "step={step} {kind:?}: the touched cells hold the pivot's halves");
            let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
            assert!(error[0] < 0.003 && error[1] < 0.025, "{layers} layers, step={step} kind={kind:?}: {error:?}");
            assert_presentation_mip(&r);
            assert!(resident_bytes(cache, scene) <= crate::memory::CACHE_BYTES);
            work.push((records, commits));
        }
    }
    for step in 0..4 {
        match step {
            0 => occurrence_mut(&mut doc,middle).opacity = 0.91,
            1 => occurrence_mut(&mut doc,0).visible = false,
            2 => swap_entries(&mut doc,middle,layers-1),
            _ => paint_mut(&mut doc,middle).base = Some(layer_core::authored::PaintBase::new((Arc::new((*photo_source).clone())).into())),
        }
        crate::test_support::submit_published(&mut r, frame(doc.scene(), extent));
        crate::test_support::submit_published(&mut exact, frame(doc.scene(), extent));
        let cache = r.scale_display.as_ref().unwrap();
        let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
        assert!(error[0] < 0.003 && error[1] < 0.025, "{layers} layers, metadata step={step}: {error:?}");
    }
    work
}

#[test]
fn layer_edits_reuse_balanced_branches_across_the_stack() {
    fn blends(node: &graph::Node, target: SourceTarget) -> Option<u32> {
        use graph::Expression;
        match node.as_ref() {
            Expression::Source { id, .. } => (*id == target).then_some(0),
            Expression::Opacity { input, .. } => blends(input, target),
            Expression::Combine { front, back, .. } => blends(front, target)
                .or_else(|| blends(back, target)).map(|n| n + 1),
            _ => None,
        }
    }
    let mut doc = document();
    let photo_source=paint_at(&doc,0).base.as_ref().map(|base|base.image.storage().clone());
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for count in [2usize, 3, 4, 7, 8, 15, 16] {
        let mut entries:Vec<_>=(0..count).map(|i| { let handle=paint_occurrence(&mut doc,"photo",photo_source.clone()); doc.artwork.occurrences.get_mut(handle).unwrap().opacity=0.17+i as f32*0.02; handle }).collect();
        let empty=paint_occurrence(&mut doc,"paint",None); entries.insert(0,empty); set_root_entries(&mut doc,entries);
        let mut dab = crate::tests::test_dab([97., 97.], [0.9, 0.02, 0.1, 0.7], 1.);
        dab.radii = [21.; 2];
        let fill = add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
        for (space, paper) in layer_core::BlendSpace::ALL.into_iter().flat_map(|space| [0., 0.7].map(|paper| (space, paper))) {
            let color = layer_core::color::RgbColor::from_linear(doc.composition().color.space, [0.17, 0.39, 0.81, paper]).unwrap();
            set_effect_value(&mut doc, fill, "color", layer_core::EffectValue::Color(color));
            let mut frame = packet(doc.scene(), extent);
            frame.blend_space = space;
            frame.composite_all = false;
            frame.reset_layers = true;
            frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
            crate::test_support::submit_published(&mut r, frame);
            crate::test_support::submit_published(&mut exact, frame);
            frame.reset_layers = false;
            let bound = (count as u32 + 1 + u32::from(paper > 0.)).next_power_of_two().ilog2();
            for index in [0, count / 2, count] {
                let batch = dab_batch(source_at(&doc,index), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
                let stroke = FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame };
                crate::test_support::submit_published(&mut r, stroke);
                crate::test_support::submit_published(&mut exact, stroke);
                let cache = r.scale_display.as_ref().unwrap();
                for other in 0..=count {
                    let target=source_at(&doc,other);
                    let work = blends(cache.graph.root.as_ref().unwrap(), target).unwrap();
                    let limit = if other == index { 2 } else { bound + 2 };
                    assert!(work <= limit, "{count} static layers, {space:?}, paper={paper}, layer={:?}: {work} > {limit}", target);
                }
                let error = quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
                assert!(error[0] < 0.003 && error[1] < 0.025, "{count} static layers, {space:?}, index={index}: {error:?}");
                assert_presentation_mip(&r);
            }
        }
    }
}

#[test]
fn identity_edits_recompose_only_damaged_regions() {
    let doc = document_at([1024, 768]);
    let dabs: Vec<_> = [[125., 125.], [893., 125.], [893., 637.], [125., 637.]].into_iter().map(|p| {
        let mut dab = crate::tests::test_dab(p, [1., 0., 0., 1.], 1.);
        dab.radii = [16.; 2];
        dab
    }).collect();
    let damage = dabs.iter().fold(layer_core::Rect::default(), |r, d| r.union(d.bounds()));
    let batch = DabBatch { dab_count: dabs.len() as u32,
        ..dab_batch(source_at(&doc,0), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), damage) };
    for level in [0, 2] {
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), doc.composition().size);
        frame.composite_all = false;
        frame.view.width_px = doc.composition().size[0];
        frame.view.height_px = doc.composition().size[1];
        let scale = 1. / (1 << level) as f32;
        frame.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        submit_published(&mut r, frame);
        frame.dabs = &dabs;
        frame.dab_batches = std::slice::from_ref(&batch);
        let work = r.metrics.composited_pixels;
        crate::test_support::submit_ready(&mut r, frame).unwrap();
        let work = r.metrics.composited_pixels - work;
        let cells = dabs.iter().flat_map(|dab| pages::coordinates(pixel_rect(dab.bounds(), doc.composition().size), level)).collect::<BTreeSet<_>>().len() as u64;
        assert!(work > 0 && work <= cells * u64::from(PAGE_SIZE).pow(2), "warm identity contacts compose only the cells under their dabs: {work} for {cells} cells");
        let incremental = display_pixels(&r);
        r.scale_display = None;
        submit_published(&mut r, FramePacket { dabs: &[], dab_batches: &[], composite_all: true, ..frame });
        assert_eq!(display_pixels(&r), incremental);
    }
}

#[test]
fn offset_page_edge_edits_match_rebuilding_the_entire_display() {
    let mut doc = document_at([513, 513]);
    let moving = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(moving).unwrap().opacity = 0.71;
    doc.artwork.occurrences.get_mut(moving).unwrap().offset = [77, 78];
    let front = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(front).unwrap().opacity = 0.3;
    insert_occurrence(&mut doc, moving, 0);
    insert_occurrence(&mut doc, front, 0);
    let extent = doc.composition().size;
    let mut incremental = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut incremental, Some(doc.scene().order()[1]));
    let mut rebuilt = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut rebuilt, Some(doc.scene().order()[1]));
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    submit_published(&mut incremental, frame);
    submit_published(&mut rebuilt, frame);
    for renderer in [&mut incremental, &mut rebuilt] { crate::test_support::edit_working_layer(renderer, frame, source_at(&doc,1)); }
    let original = display_pixels(&incremental);
    let mut dab = crate::tests::test_dab([254.5, 0.5], [1., 0., 0., 1.], 1.);
    dab.radii = [0.45; 2];
    let batch = dab_batch(source_at(&doc,1), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
    frame.dabs = std::slice::from_ref(&dab);
    frame.dab_batches = std::slice::from_ref(&batch);
    rebuilt.scale_display = None;
    crate::test_support::submit_ready(&mut incremental, frame).unwrap();
    submit_published(&mut rebuilt, frame);
    let expected = display_pixels(&rebuilt);
    let actual = display_pixels(&incremental);
    assert_ne!(actual, original);
    let error = crate::test_support::max_error(&actual, &expected);
    assert!(error < 1e-6, "a filtered source page reaches pixels beyond its mapped bounds: {error}");
}

#[test]
fn masks_refresh_coverage_properties_and_paint_without_exact_display() {
    let mut doc = document();
    let extent = doc.composition().size;
    let owner=doc.scene().order()[0];
    let mask_source=coverage_mask(&mut doc,owner, [0, 0], Some(layer_core::Selection::polygon(vec![
        layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 258., y: 0. },
        layer_core::Point { x: 258., y: 259. }, layer_core::Point { x: 0., y: 259. },
    ]).unwrap()));
    doc.artwork.coverage.get_mut(mask_source).unwrap().default_coverage=0.;
    let mut reduced = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for state in 0..7 {
        let mask = occurrence_mut(&mut doc,0).mask.as_mut().unwrap();
        mask.enabled = state != 3;
        mask.inverted = state == 1;

        let mut frame = packet(doc.scene(), extent);
        frame.inspect_mask=(state==2).then_some(owner);
        frame.composite_all = false;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        let dab = crate::tests::test_dab([370., 129.], [1.; 4], 1.);
        let batch = dab_batch(SourceTarget::Coverage(mask_source), crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        if state == 5 {
            frame.dabs = std::slice::from_ref(&dab);
            frame.dab_batches = std::slice::from_ref(&batch);
        }
        submit_published(&mut reduced, frame);
        submit_published(&mut exact, FramePacket { composite_all: true, ..frame });
        let cache = reduced.scale_display.as_ref().unwrap();
        let error = quality(&display_pixels(&reduced), &pixels(&exact, crate::test_support::document_texture(&exact)), cache.plan, cache.texture().format());
        assert!(error[0] < 0.001 && error[1] < 0.015, "moving mask state={state}: {error:?}");
        assert_presentation_mip(&reduced);
        assert_settled(&mut reduced, frame, &pixels(&exact, crate::test_support::document_texture(&exact)));
        let a = reduced.readback_srgb_rgba8().unwrap();
        let b = exact.readback_srgb_rgba8().unwrap();
        assert_eq!(a, b, "mask state={state} exact query");

    }
}
pub(crate) fn display_pixels(r: &WgpuRasterizer) -> Vec<[f32; 4]> {
    pixels(r, &materialized_display(r).texture)
}
fn assert_keyed_presentation(r:&WgpuRasterizer) {
    let cache=r.scale_display.as_ref().unwrap();
    assert!(cache.coverage_complete());
    assert!(cache.pixels.root().is_some());
    let pages=cache.pages.lock().unwrap().retained_pages();
    assert!(pages.iter().any(|page|!page.current.is_empty()),"presentation must have committed current keyed pixels");
}
fn materialized_display(r: &WgpuRasterizer) -> Image {
    let cache = r.scale_display.as_ref().unwrap();
    let image = cache.pixels.root().unwrap();
    if image.plan == cache.plan { return image.clone(); }
    let output = Image::new(r, cache.plan, "display window query");
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {
        origin: wgpu::Origin3d { x: cache.plan.bounds.min_x() >> cache.plan.level, y: cache.plan.bounds.min_y() >> cache.plan.level, z: 0 },
        ..image.texture.as_image_copy()
    }, output.texture.as_image_copy(), wgpu::Extent3d { width: cache.plan.size[0], height: cache.plan.size[1], depth_or_array_layers: 1 });
    encoder.submit(&r.queue);
    output
}

pub(super) fn window_pixels(r: &WgpuRasterizer, image: &Image, plan: display_mips::Plan) -> Vec<[f32; 4]> {
    let pixels = pixels(r, &image.texture);
    let [x, y, width, height] = paint_transform::texel_rect(plan.bounds.window_local(image.plan.bounds), 1 << plan.level);
    (y..y + height).flat_map(|y| pixels[(y * image.plan.size[0] + x) as usize..(y * image.plan.size[0] + x + width) as usize].iter().copied()).collect()
}

pub(super) fn assert_presentation_mip(r: &WgpuRasterizer) {
    let cache = r.scale_display.as_ref().unwrap();
    let (input, actual, plan) = (display_pixels(r), window_pixels(r, cache.pixels.next().unwrap(),
        display_mips::Plan::window(cache.plan.extent, cache.plan.level + 1, cache.plan.bounds)), cache.plan);
    let size = plan.level_size(plan.level + 1);
    let side = 1 << plan.level;
    for y in 0..size[1] {
        for x in 0..size[0] {
            let mut sum = [0.; 4];
            let mut area = 0.;
            for yy in y * 2..((y + 1) * 2).min(plan.size[1]) {
                for xx in x * 2..((x + 1) * 2).min(plan.size[0]) {
                    let weight = (side.min(plan.bounds.width() - xx * side)
                        * side.min(plan.bounds.height() - yy * side))
                        as f32;
                    for c in 0..4 {
                        sum[c] += input[(yy * plan.size[0] + xx) as usize][c] * weight;
                    }
                    area += weight;
                }
            }
            let expected=sum.map(|value|value/area);
            let format=cache.pixels.next().unwrap().texture.format();
            for c in 0..4 {
                let value=actual[(y * size[0] + x) as usize][c];
                let allowed=match format {
                    wgpu::TextureFormat::Rgba16Float=>half_storage_bounds(expected[c]),
                    wgpu::TextureFormat::Rgba32Float=>[expected[c];2],
                    format=>panic!("unexpected committed page format: {format:?}"),
                };
                let matches=if format==wgpu::TextureFormat::Rgba16Float {allowed.contains(&value)}
                    else {(value-expected[c]).abs()<1e-5};
                assert!(matches,
                    "adjacent output must weight partial cells; WGSL half storage permits either bracketing value: level={} at={:?} channel={c} actual={value} exact_mean={} allowed={allowed:?} max_adjacent={} format={format:?}",
                    plan.level,[x,y],expected[c],allowed[1]-allowed[0]
                );
            }
        }
    }
}

fn half_storage_bounds(value:f32)->[f32;2] {
    assert!(value.is_finite(),"storage mean must be finite: {value}");
    let rounded=layer_core::color::f16::from_f32(value);
    let nearest=rounded.to_f32();
    if nearest==value {return [value;2];}
    let upward=value>nearest;
    let step=if upward==nearest.is_sign_positive() {1}else {-1};
    let adjacent=layer_core::color::f16::from_bits(rounded.to_bits().wrapping_add_signed(step)).to_f32();
    [nearest.min(adjacent),nearest.max(adjacent)]
}

fn committed_color(format:wgpu::TextureFormat,pixel:[f32;4])->[f32;4] {
    match format {
        wgpu::TextureFormat::Rgba16Float=>pixel.map(|value|layer_core::color::f16::from_f32(value).to_f32()),
        wgpu::TextureFormat::Rgba32Float=>pixel,
        format=>panic!("unexpected committed page format: {format:?}"),
    }
}

pub(crate) fn quality(actual: &[[f32; 4]], exact: &[[f32; 4]], plan: display_mips::Plan, format: wgpu::TextureFormat) -> [f32; 3] {
    quality_linear(actual, exact, plan, format, |color| color)
}

fn within_one_half_float_step(actual: &[[f32; 4]], exact: &[[f32; 4]], plan: display_mips::Plan) -> bool {
    let ordinals = |color: [f32; 4]| color.map(|value| f32::from(layer_core::color::f16::from_f32(value).to_bits()));
    quality_linear(actual, exact, plan, wgpu::TextureFormat::Rgba16Float, ordinals)[2] <= 1.
}

fn assert_reduction(actual: &[[f32; 4]], exact: &[[f32; 4]], plan: display_mips::Plan, format: wgpu::TextureFormat, bound: f32, context: std::fmt::Arguments<'_>) {
    let error = quality(actual, exact, plan, format);
    let reduced_into_half_float = plan.level > 0 && format == wgpu::TextureFormat::Rgba16Float;
    assert!(error[2] < bound || reduced_into_half_float && within_one_half_float_step(actual, exact, plan), "{context}: {error:?}");
}

fn linear_color(color: [f32; 4], space: layer_core::BlendSpace, rgb: layer_core::color::RgbSpace) -> [f32; 4] {
    let a = color[3];
    if space == layer_core::BlendSpace::Linear || a <= 0. { return color; }
    [rgb.decode(f64::from(color[0] / a)) as f32 * a,
     rgb.decode(f64::from(color[1] / a)) as f32 * a,
     rgb.decode(f64::from(color[2] / a)) as f32 * a, a]
}

fn quality_linear(actual: &[[f32; 4]], exact: &[[f32; 4]], plan: display_mips::Plan, format: wgpu::TextureFormat, linear: impl Fn([f32; 4]) -> [f32; 4]) -> [f32; 3] {
    let side = 1 << plan.level;
    let mut errors = Vec::new();
    for y in 0..plan.size[1] {
        for x in 0..plan.size[0] {
            let mut sum = [0.; 4];
            let mut count = 0.;
            for yy in plan.bounds.min_y() + y * side..(plan.bounds.min_y() + (y + 1) * side).min(plan.bounds.max_y()) {
                for xx in plan.bounds.min_x() + x * side..(plan.bounds.min_x() + (x + 1) * side).min(plan.bounds.max_x()) {
                    for c in 0..4 {
                        sum[c] += exact[(yy * plan.extent[0] + xx) as usize][c];
                    }
                    count += 1.;
                }
            }
            let a = linear(actual[(y * plan.size[0] + x) as usize]);
            let b = linear(committed_color(format, sum.map(|v| v / count)));
            for c in 0..4 { errors.push((a[c] - b[c]).abs()); }
        }
    }
    errors.sort_by(f32::total_cmp);
    [
        errors.iter().sum::<f32>() / errors.len() as f32,
        errors[errors.len() * 99 / 100],
        *errors.last().unwrap(),
    ]
}

#[test]
fn scaled_composition_preserves_exact_paint_and_replaces_full_display() {
    let mut doc = document();
    let paint = copy_paint(&mut doc,0);
    let OccurrenceContent::Paint(source)=doc.artwork.occurrences.get(paint).unwrap().content else {unreachable!()}; doc.artwork.paint.get_mut(source).unwrap().base=None;
    insert_occurrence(&mut doc, paint, 0);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut p = packet(doc.scene(), extent);
    p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, p);
    submit_published(&mut exact, p);

    assert!(!r.scene.as_ref().unwrap().scale_sources.entries.contains_key(&source_at(&doc,0)));
    assert!(r.scale_display.as_ref().unwrap().storage_bytes() < 1 << 20);
    let original = display_pixels(&r);
    let reference = pixels(&exact, crate::test_support::document_texture(&exact));
    let plan = r.scale_display.as_ref().unwrap().plan;
    assert_eq!(plan.level, 3);
    assert_eq!(plan.bounds, PixelRect::full(extent));
    assert_reduction(&original, &reference, plan, r.scale_display.as_ref().unwrap().texture().format(), 1e-5, format_args!("first display"));
    let mut dab = crate::tests::test_dab([255., 129.], [0.9, 0.02, 0.1, 0.7], 1.);
    dab.radii = [45.; 2];
    let batch = dab_batch(
        source_at(&doc,0),
        crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
        dab.bounds(),
    );
    for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
        let batch = DabBatch {
            kind,
            ..batch.clone()
        };
        let stroke = FramePacket {
            dabs: &[dab],
            dab_batches: &[batch],
            composite_all: false,
            ..p
        };
        crate::test_support::submit_ready(&mut r, stroke).unwrap();
        crate::test_support::submit_ready(&mut exact, stroke).unwrap();
        assert_presentation_mip(&r);
        if kind == DabBatchKind::Preview {
            assert_eq!(r.preview_level, 2);
            assert!(
                r.preview_pages
                    .iter()
                    .all(|p| p.primary.size().width == 64)
            );
            assert!(r.preview_coverage_pages.is_empty());
        }
        let coarse = r.scale_display.as_ref().unwrap();
        let error = quality(
            &pixels(&r, coarse.texture()),
            &pixels(&exact, crate::test_support::document_texture(&exact)),
            coarse.plan, coarse.texture().format(),
        );
        assert!(
            error[0] < 0.003,
            "large brush preview must remain close to exact reduction"
        );
        let a = r.readback_srgb_rgba8().unwrap();
        let b = exact.readback_srgb_rgba8().unwrap();
        assert_eq!(a.len(), b.len());
        for (index, (actual, expected)) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()).enumerate() {
            assert_eq!(actual, expected, "{kind:?} native output pixel {:?}", [index % extent[0] as usize, index / extent[0] as usize]);
        }
    }
    crate::test_support::submit_ready(&mut r, FramePacket {
        composite_all: false,
        ..p
    }).unwrap();
    let final_pixels = display_pixels(&r);
    assert_ne!(original, final_pixels);
    assert_settled(&mut r, p, &pixels(&exact, crate::test_support::document_texture(&exact)));
    p.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    crate::test_support::submit_ready(&mut r, p).unwrap();
    assert_eq!(r.scale_display.as_ref().unwrap().plan.level, 0);

    assert!(quality(&display_pixels(&r), &pixels(&exact, crate::test_support::document_texture(&exact)), r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format())[2] < 1e-6);
}

#[test]
fn native_predictions_match_exact_composition_before_layer_opacity_and_blending() {
    let mut doc = document_at([257, 259]);
    let front = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(front).unwrap().opacity = 0.37;
    occurrence_mut(&mut doc,0).opacity = 0.63;
    insert_occurrence(&mut doc, front, 0);
    let extent = doc.composition().size;
    for preset in [DefaultBrushPreset::GPen, DefaultBrushPreset::Pencil, DefaultBrushPreset::PointyPencil,
        DefaultBrushPreset::ShadingPencil, DefaultBrushPreset::Charcoal, DefaultBrushPreset::Chalk,
        DefaultBrushPreset::PastelBlock, DefaultBrushPreset::Crayon] {
    let targets = [source_at(&doc,0), source_at(&doc,1)];
    for target in targets {
        for mode in [layer_render::DabMode::Paint, layer_render::DabMode::Erase] {
            let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            exact.test.reference = true;
            let mut frame = packet(doc.scene(), extent);
            frame.view.width_px = extent[0];
            frame.view.height_px = extent[1];
            submit_published(&mut r, frame);
            submit_published(&mut exact, frame);
            let original = display_pixels(&r);
            let mut dab = crate::tests::test_dab([253., 254.], [0.9, 0.02, 0.1, 0.7], 1.);
            dab.radii = [35.; 2];
            let mut style = crate::layer_tests::preset_style(preset);
            if let Some(dry) = style.contact.and_then(|c| c.dry) {
                let relief = dry.pose(0.5).relief;
                dab.radii = [35., 19.];
                dab.previous = [35., 19., 1., 0.];
                dab.motion = [20., -8.];
                dab.contact = [0.7, relief[0], relief[1], relief[2]];
                dab.previous_contact = dab.contact;
            }
            style.mode = mode;
            let batch = DabBatch { kind: DabBatchKind::Preview,
                ..dab_batch(target, style, dab.bounds()) };
            let mut oracle_batch = batch.clone();
            if preset != DefaultBrushPreset::GPen { oracle_batch.style.selection = Some(Arc::new(layer_core::Selection::full())); }
            for prediction in [true, false] {
                let stroke = FramePacket {
                    dabs: if prediction { std::slice::from_ref(&dab) } else { &[] },
                    dab_batches: if prediction { std::slice::from_ref(&batch) } else { &[] },
                    composite_all: false, ..frame
                };
                crate::test_support::submit_ready(&mut r, stroke).unwrap();
                crate::test_support::submit_ready(&mut exact, FramePacket { dab_batches: if prediction { std::slice::from_ref(&oracle_batch) } else { &[] }, ..stroke }).unwrap();
                let actual = display_pixels(&r);
                let expected = pixels(&exact, crate::test_support::document_texture(&exact));
                let error = quality(&actual, &expected, r.scale_display.as_ref().unwrap().plan, r.scale_display.as_ref().unwrap().texture().format());
                assert!(error[2] < 1e-6, "{preset:?} target={target:?}, mode={mode:?}, preview={prediction}: {error:?}");
                if prediction {
                    assert_ne!(actual, original);
                    assert_eq!(r.preview_level, 0);
                    assert!(!r.preview_contribution, "{preset:?}/{mode:?} plain scene uses native underpaint");
                    assert!(!exact.preview_contribution);
                    assert!(r.preview_pages.iter().all(|p| p.primary.size().width == PAGE_SIZE));
                }
                else { assert_eq!(actual, original); }
            }
        }
    }}
}

#[test]
fn compact_preview_weights_partial_edge_texels_and_retires_corrections() {
    let doc = document();
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut p = packet(doc.scene(), extent);
    p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
    submit_published(&mut r, p);
    let original = display_pixels(&r);
    for position in [[516., 258.], [100., 100.], [514., 256.]] {
        let mut dab = crate::tests::test_dab(position, [0.9, 0.02, 0.1, 0.7], 1.);
        dab.radii = [12.; 2];
        let batch = DabBatch {
            kind: DabBatchKind::Preview,
            ..dab_batch(
                source_at(&doc,0),
                crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
                dab.bounds(),
            )
        };
        crate::test_support::submit_ready(&mut r, FramePacket {
            dabs: &[dab],
            dab_batches: &[batch],
            composite_all: false,
            ..p
        }).unwrap();
        if let Some(page) = r.preview_page([2, 1]) {
            let wanted=r.preview_version(source_at(&doc,0),RasterPlane::Color,page.coordinate,scene::PreviewStage::Final).unwrap();
            let read=page.active().read(wanted.version(),None).unwrap();
            let compact = pixels(&r, &read.value().texture);
            let cache = r.scale_display.as_ref().unwrap();
            let id=source_at(&doc,0);
            let layer = pixels(&r, &source_image(&r,cache,&r.scene.as_ref().unwrap().scale_sources.entries[&id],id,display_mips::Plan::at(doc.composition().size,cache.plan.level)).texture);
            // The 517 × 259 document ends with a 5 × 3 block: these two
            // compact texels cover 4 × 3 and 1 × 3 original pixels.
            let last = layer.last().unwrap();
            for c in 0..4 {
                assert!((last[c] - (compact[0][c] * 4. + compact[1][c]) / 5.).abs() < 1e-6);
            }
        }
    }
    crate::test_support::submit_ready(&mut r, FramePacket {
        composite_all: false,
        ..p
    }).unwrap();
    assert_eq!(
        display_pixels(&r),
        original,
        "discarding a corrected tail restores every old footprint"
    );
    assert!(r.preview_pages.is_empty());
}

#[test]
fn scaled_layer_cache_tracks_stack_changes_and_odd_edges_at_each_level() {
    let mut doc = document();
    let foreground = copy_paint(&mut doc,0);
    doc.artwork.occurrences.get_mut(foreground).unwrap().opacity = 0.37;
    insert_occurrence(&mut doc, foreground, 0);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for level in [1, 3, 2, 4] {
        let scale = 1. / (1u32 << level) as f32;
        for change in 0..5 {
            match change {
                1 => occurrence_mut(&mut doc,0).opacity = 0.12,
                2 => swap_entries(&mut doc,0,1),
                3 => paint_mut(&mut doc,0).base = None,
                4 => paint_mut(&mut doc,0).base = paint_at(&doc,1).base.clone(),
                _ => (),
            }
            let mut p = packet(doc.scene(), extent);
            p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
            submit_published(&mut r, p);
            submit_published(&mut exact, p);
            let cache = r.scale_display.as_ref().unwrap();
            assert_reduction(
                &pixels(&r, cache.texture()),
                &pixels(&exact, crate::test_support::document_texture(&exact)),
                cache.plan, cache.texture().format(), 2e-5,
                format_args!("constant-alpha stack, level {level}, change {change}"),
            );
        }
    }
}

#[test]
fn display_and_native_evaluation_share_the_display_cache() {
    let mut doc = document();
    let extent = doc.composition().size;
    let mut draft = crate::tests::image_windows::program(false, false);
    Arc::make_mut(&mut draft.program).resolution = layer_core::EffectResolution::Native;
    let effect=effect_occurrence(&mut doc,draft,"native windows");
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    for filtered in [true, false, true, false] {
        let mut entries=doc.scene().children(None).iter().copied().filter(|handle|*handle!=effect).collect::<Vec<_>>();
        if filtered {entries.insert(0,effect);}
        set_root_entries(&mut doc,entries);
        let mut p = packet(doc.scene(), extent);
        p.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        submit_published(&mut r, p);
        submit_published(&mut exact, p);
        assert!(r.scale_display.is_some());

        assert_eq!(r.scale_display.as_ref().unwrap().evaluation == Evaluation::Native, filtered);
        assert_settled(&mut r, p, &pixels(&exact, crate::test_support::document_texture(&exact)));
        if !filtered {
            assert_eq!(r.scene.as_ref().unwrap().image_cache_bytes(), 0);
        }
        let a = r.readback_srgb_rgba8().unwrap();
        let b = exact.readback_srgb_rgba8().unwrap();
        assert_eq!(a, b);
    }
}

/// Optional photographic oracle, without putting a licensed photo in the repo.
/// Supply a 2048 × 1536, row-major, RGBA8 sRGB crop in LAYER_DISPLAY_PHOTO_RGBA.
#[test]
#[ignore = "requires a local photographic RGBA fixture in LAYER_DISPLAY_PHOTO_RGBA"]
fn photographic_preview_and_committed_display_quality() {
    let extent = [2048, 1536];
    let bytes = std::fs::read(std::env::var("LAYER_DISPLAY_PHOTO_RGBA").unwrap()).unwrap();
    assert_eq!(bytes.len(), (extent[0] * extent[1] * 4) as usize);
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: Default::default(),
            profile_assumed: false,
        },
        32 << 20,
    )
    .unwrap();
    for row in bytes.chunks_exact(extent[0] as usize * 4) {
        builder.push_row(row).unwrap();
    }
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let photo = copy_paint(&mut doc,0);
    let OccurrenceContent::Paint(source)=doc.artwork.occurrences.get(photo).unwrap().content else {unreachable!()}; doc.artwork.paint.get_mut(source).unwrap().base=Some(layer_core::authored::PaintBase::new((Arc::new(builder.finish().unwrap())).into()));
    insert_occurrence(&mut doc, photo, 1);
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut p = packet(doc.scene(), extent);
    for zoom in [0.071382575, 0.16, 0.3, 0.6] {
    p.view.document_to_surface = [zoom, 0., 0., zoom, 0., 0.];
    for space in layer_core::BlendSpace::ALL {
      p.blend_space = space;
      for diameter in [50., 460., 1000., 2000.] {
        crate::test_support::submit_ready(&mut r, FramePacket {
            reset_layers: true,
            ..p
        }).unwrap();
        crate::test_support::submit_ready(&mut exact, FramePacket {
                reset_layers: true,
                ..p
            }).unwrap();
        let mut dab = crate::tests::test_dab([1055., 780.], [0.01, 0.02, 0.03, 0.7], 1.);
        dab.radii = [diameter / 2.; 2];
        let batch = dab_batch(
            source_at(&doc,0),
            crate::layer_tests::preset_style(DefaultBrushPreset::GPen),
            dab.bounds(),
        );
        for kind in [DabBatchKind::Preview, DabBatchKind::Persistent] {
            let batch = DabBatch {
                kind,
                ..batch.clone()
            };
            let stroke = FramePacket {
                dabs: &[dab],
                dab_batches: &[batch],
                composite_all: false,
                ..p
            };
            crate::test_support::submit_ready(&mut r, stroke).unwrap();
            crate::test_support::submit_ready(&mut exact, stroke).unwrap();
            let cache = r.scale_display.as_ref().unwrap();
            let errors = quality_linear(
                &display_pixels(&r),
                &pixels(&exact, crate::test_support::document_texture(&exact)),
                cache.plan, cache.texture().format(),
                |color| linear_color(color, space, doc.composition().color.space),
            );
            assert!(
                errors[0] < 0.003 && errors[1] < 0.03,
                "photographic display quality regressed"
            );
            let actual = r.readback_srgb_rgba8().unwrap();
            let expected = exact.readback_srgb_rgba8().unwrap();
            assert_eq!(
                actual, expected,
                "photographic exact output at {space:?} {diameter}px {kind:?}"
            );
        }
    }
    }
    }
}

#[test]
fn unchanged_navigation_derives_and_reuses_a_bounded_neighbor_level() {
    let mut doc = document();
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut exact = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    exact.test.reference = true;
    let mut misses = None;
    let mut composed = 0;
    for (index, level) in [2, 3, 2, 3, 2].into_iter().enumerate() {
        let scale = 1. / (1u32 << level) as f32;
        let mut p = packet(doc.scene(), extent);
        p.composite_all = index == 0;
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        submit_published(&mut r, p);
        submit_published(&mut exact, p);
        let work = r.source_cache_work()[1];
        assert_eq!(
            *misses.get_or_insert(work),
            work,
            "zooming must reuse already-decoded content"
        );
        if index >= 2 {
            assert_eq!(
                r.metrics.composited_pixels, composed,
                "returning to a cached level needs no composition"
            );
        }
        composed = r.metrics.composited_pixels;
        assert_presentation_mip(&r);
        let cache = r.scale_display.as_ref().unwrap();
        assert!(cache.storage_bytes() <= crate::memory::CACHE_BYTES);
        assert_reduction(
            &display_pixels(&r),
            &pixels(&exact, crate::test_support::document_texture(&exact)),
            cache.plan, cache.texture().format(), 1e-5,
            format_args!("derived level must include correct partial-edge weights"),
        );
    }
    assert!(retained_root_pages(r.scale_display.as_ref().unwrap()).iter().map(|page|page.key.level).collect::<BTreeSet<_>>().len()>=2);
    let previous_root=r.scale_display.as_ref().unwrap().graph.root.clone();
    occurrence_mut(&mut doc,0).opacity = 0.3;
    for (index, scale) in [0.25, 0.125].into_iter().enumerate() {
        let mut p = packet(doc.scene(), extent);
        p.composite_all = index == 0;
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        submit_published(&mut r, p);
        submit_published(&mut exact, p);
        if scale == 0.25 {
            assert!(r.scale_display.as_ref().unwrap().graph.root!=previous_root);
        }
        let cache = r.scale_display.as_ref().unwrap();
        assert_reduction(
            &display_pixels(&r),
            &pixels(&exact, crate::test_support::document_texture(&exact)),
            cache.plan, cache.texture().format(), 1e-5,
            format_args!("changed artwork must replace the retired neighbor"),
        );
    }
    occurrence_mut(&mut doc,0).visible = false;
    for (index, scale) in [0.25, 0.125, 0.25, 0.125, 0.125].into_iter().enumerate() {
        if index == 4 {
            occurrence_mut(&mut doc,0).visible = true;
        }
        let mut p = packet(doc.scene(), extent);
        p.composite_all = index == 0 || index == 4;
        p.view.document_to_surface = [scale, 0., 0., scale, 0., 0.];
        submit_published(&mut r, p);
        submit_published(&mut exact, p);
        let cache = r.scale_display.as_ref().unwrap();
        assert_reduction(
            &display_pixels(&r),
            &pixels(&exact, crate::test_support::document_texture(&exact)),
            cache.plan, cache.texture().format(), 1e-5,
            format_args!("an empty retained level must remain reusable after layer visibility changes"),
        );
    }
}

#[path = "transform_tests.rs"]
mod transforms;

fn reduced_photo_stack() -> Document {
    let mut doc = document_at([65, 33]);
    composition_mut(&mut doc).size[0] = 9504; composition_mut(&mut doc).size[1] = 6336;
    for _ in 0..4 {
        let layer = copy_paint(&mut doc,0);
        doc.artwork.occurrences.get_mut(layer).unwrap().opacity = 0.35;
        doc.artwork.occurrences.get_mut(layer).unwrap().blend = layer_core::LayerBlend::SoftLight;
        insert_occurrence(&mut doc, layer, 0);
    }
    doc
}

fn resident_bytes(cache: &crate::scene::scale::Cache, scene: &crate::scene::Scene) -> u64 {
    cache.storage_bytes() + scene.scale_commands.as_ref().unwrap().storage_bytes()
}

fn resident_bytes_with_pool(cache: &crate::scene::scale::Cache, scene: &crate::scene::Scene) -> u64 {
    resident_bytes(cache, scene) + scene.pool.iter().map(|p| texture_bytes(&p.texture)).sum::<u64>()
}

#[test]
fn projective_display_matches_analytic_bilinear_pigment_and_composition() {
    analytic_display_resample(layer_core::Projective([0.9,0.07,-16.,-0.05,1.1,-12.,0.001,-0.0002,1.]));
}

#[test]
fn affine_display_matches_analytic_bilinear_pigment_and_composition() {
    analytic_display_resample(layer_core::Projective([0.9,0.07,-16.,-0.05,1.1,-12.,0.,0.,1.]));
}

fn analytic_display_resample(transform: layer_core::Projective) {
    use layer_core::{ImageTransform,LayerPlacement};
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let source=crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float);
    let colors:Vec<_>=(0..256*256).map(|i| {let a=if (i/256/5+i%256/7)%2==0 {0.25}else{0.75};
        [a*0.8,a*0.2,a*0.6,a]}).collect();
    crate::test_support::upload_page(&r,&source,&colors.iter().flatten().flat_map(|v|f32::to_le_bytes(*v)).collect::<Vec<_>>());
    let extent=[256;2];let plan=display_mips::Plan::at(extent,0);let texels=[3,5,113,83];
    let resample=r.scene_pipelines.resample.clone();
    for encode in [false,true] {
        let moved=ImageTransform {placement:LayerPlacement::from_projective(transform),..Default::default()};
        let values=crate::scene::resample::Resample::values(crate::scene::resample::Request {moved:&moved,kept:&ImageTransform::default(),
            clip:layer_core::Affine::IDENTITY,extent,texels,
            display:pixel_transform::DisplayLevel {side:1,extent,opacity:0.65,backdrop:[0.1,0.05,0.15,0.5],encode},
            target:plan,source:plan,max_lod:0,outside:0.,keep_source:false,identity:false}).unwrap();
        let uniforms=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:None,contents:&values,usage:wgpu::BufferUsages::UNIFORM});
        let compute=(transform.0[6..]==[0.,0.,1.]).then(||crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float));
        let fragment=crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let tiled=crate::test_support::page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let source=source.create_view(&Default::default());
        let fragment_view=fragment.create_view(&Default::default());
        let binding=compute.as_ref().map(|output|resample.binding(&r.device,&uniforms,0,[&source,&output.create_view(&Default::default()),&r.empty_view]));
        let reference=resample.mesh_binding(&r.device,&uniforms,&[source.clone(),r.empty_view.clone()]);
        let mut fine=crate::paint_transform::fine::FineInputs::new(&r,1,false,0,Default::default()).unwrap();
        fine.require([[0;2]].into_iter().collect()).unwrap();
        let slot=fine.assign([0;2]).unwrap();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        encoder.copy_texture_to_texture(source.texture().as_image_copy(),fine.texture.as_image_copy(),source.texture().size());
        fine.written(slot,&encoder);
        fine.write_directory(&mut r,&mut encoder,[0;2]).unwrap();
        let fine_pass=r.scene_pipelines.fine_resample.clone();
        pollster::block_on(r.compilation.prepare_all([&resample.area])).unwrap();
        pollster::block_on(r.compilation.prepare_all([&resample.mesh[4], &fine_pass.mesh[4]])).unwrap();
        let tiled_binding=fine_pass.fine_binding(&r.device,&uniforms,&[source.clone(),r.empty_view.clone()],&fine.view,&fine.directory);
        fine_pass.encode_mesh(&mut encoder,&tiled_binding,&tiled.create_view(&Default::default()),texels,None,false);
        if let Some(binding)=&binding {resample.encode(&mut encoder,binding,texels);}
        resample.encode_mesh(&mut encoder,&reference,&fragment_view,texels,None,false);
        r.uploads.finish(&encoder);encoder.submit(&r.queue);
        let mut sampled=vec![pixels(&r,&fragment),pixels(&r,&tiled)];
        if let Some(compute)=&compute {sampled.push(pixels(&r,compute));}
        for image in &sampled {assert!(image.iter().any(|p|p[3]>0.5),"display samples actual semitransparent pigment");}
        let rows=pixel_transform::inverse_rows(&moved).unwrap().map(|row|row.map(f64::from));
        let mut maximum=vec![0f64;sampled.len()];
        for y in 0..256 {for x in 0..256 {
            let index=y*256+x;
            if x<texels[0] as usize || y<texels[1] as usize || x>=(texels[0]+texels[2]) as usize || y>=(texels[1]+texels[3]) as usize {
                for image in &sampled {assert_eq!(image[index],[0.;4]);}continue;
            }
            let [u,v]=crate::test_support::preimage(moved.placement.outer.0.map(f64::from),[x as f64+0.5,y as f64+0.5]).unwrap();
            let h=[x as f64+0.5,y as f64+0.5,1.];
            let dot=|row:usize| rows[row].into_iter().zip(h).map(|(a,b)|a*b).sum::<f64>();
            let size=|row:usize| rows[row].into_iter().zip(h).map(|(a,b)|(a*b).abs()).sum::<f64>();
            let w=dot(2);let round=4.*f64::from(f32::EPSILON);let minimum_w=w-round*size(2);
            assert!(minimum_w>0.);
            let coordinate_error=[u,v].into_iter().enumerate().map(|(axis,q)|
                (dot(axis)/w-q).abs()+round*(size(axis)+q.abs()*size(2))/minimum_w+q.abs()*f64::from(f32::EPSILON)).sum::<f64>();
            let [u,v]=[u-0.5,v-0.5];let [i,j]=[u.floor() as usize,v.floor() as usize];let [fx,fy]=[u.fract(),v.fract()];
            assert!(i+1<256 && j+1<256);
            let alpha=f64::from(colors[j*256+i][3])*(1.-fx)*(1.-fy)+f64::from(colors[j*256+i+1][3])*fx*(1.-fy)
                +f64::from(colors[(j+1)*256+i][3])*(1.-fx)*fy+f64::from(colors[(j+1)*256+i+1][3])*fx*fy;
            let opacity=alpha*0.65;let mut reference=[0f64;4];
            for (channel,value) in [0.8,0.2,0.6].into_iter().enumerate() {
                let value=if encode {r.device.working_space().encode(value)}else{value};
                reference[channel]=value*opacity+[0.1,0.05,0.15][channel]*(1.-opacity);
            }
            reference[3]=opacity+0.5*(1.-opacity);
            for (path,image) in sampled.iter().enumerate() {
                let pixel=image[index];
                for channel in 0..4 {
                    let error=(f64::from(pixel[channel])-reference[channel]).abs();maximum[path]=maximum[path].max(error);
                    let color=if channel==3 {1.}else {let value=[0.8,0.2,0.6][channel];if encode {r.device.working_space().encode(value)}else{value}};
                    let paper=[0.1,0.05,0.15,0.5][channel];
                    let bound=0.5*0.65*(2./256.+coordinate_error)*(color-paper).abs()+2e-5;
                    assert!(error<bound,"sampler rounding at {x},{y} channel{channel}, path{path}: {error} exceeds {bound}");
                }
            }
        }}
        eprintln!("analytic display resample max error {maximum:?}, encode {encode}");
        assert!(maximum.iter().all(|&error|error<1./255.),"reduced U8 display is within one channel code of analytic bilinear pigment: {maximum:?}");
    }
}

#[test]
fn reduction_samplers_live_with_records_not_page_views() {
    let ownership = |source: &str| {
        let module = naga::front::wgsl::parse_str(source).unwrap();
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module).unwrap();
        let mut records = BTreeSet::new();
        let mut images = BTreeSet::new();
        let mut samplers = Vec::new();
        for (_, global) in module.global_variables.iter() {
            let Some(binding) = global.binding.as_ref() else { continue; };
            if global.space == naga::AddressSpace::Uniform { records.insert(binding.group); }
            match module.types[global.ty].inner {
                naga::TypeInner::Image { .. } => { images.insert(binding.group); }
                naga::TypeInner::Sampler { .. } => samplers.push(*binding),
                _ => (),
            }
        }
        assert!(!records.is_empty() && !images.is_empty() && !samplers.is_empty());
        let owned = samplers.iter().all(|binding| records.contains(&binding.group) && !images.contains(&binding.group));
        (owned, module, samplers)
    };
    for filterable in [true, false] {
        let source = shader_source(Default::default(), crate::pipeline_device::float_sampling_wgsl(filterable));
        let (owned, module, samplers) = ownership(&source);
        assert!(owned, "page input bindings must not allocate sampler-index storage");
        let page_group = module.global_variables.iter().find_map(|(_, global)|
            matches!(module.types[global.ty].inner, naga::TypeInner::Image { .. }).then(|| global.binding.as_ref().unwrap().group)).unwrap();
        let last = module.global_variables.iter().filter_map(|(_, global)| global.binding.as_ref())
            .filter(|binding| binding.group == page_group).map(|binding| binding.binding).max().unwrap() + 1;
        let sampler = &samplers[0];
        let previous = source.replace(&format!("@group({}) @binding({}) var area_sampler", sampler.group, sampler.binding),
            &format!("@group({page_group}) @binding({last}) var area_sampler"));
        assert!(!ownership(&previous).0, "the previous per-page sampler ownership must be rejected");
    }
}

#[test]
fn first_tiled_transform_draw_waits_for_its_fine_resample_entries() {
    let extent = [6000, 4000];
    let mut doc = Document::new(PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames { paint: "Fine transform".into(), paper: "Paper".into() });
    crate::layer_tests::placement::set_source(&mut doc, rgba8_source(extent, |_, _| [127, 63, 191, 255]));
    let reference = crate::memory::memory_tests::legacy_scene_renderer(Default::default());
    let mut r = *reference.factory.rasterizer(Default::default()).unwrap();
    r.finish_startup_catalog();
    let brush = layer_core::default_brush(DefaultBrushPreset::GPen);
    r.prepare_startup(&doc, &brush, true).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    crate::test_support::wait_startup(&mut r, deadline, |progress| progress.brush_ready,
        format_args!("Fine transform startup timed out"));
    let mut frame = packet(doc.scene(), extent);
    frame.view = layer_render::ViewState { width_px: 1920, height_px: 1080,
        document_to_surface: [0.27, 0., 0., 0.27, 0., 0.] };
    let preview = layer_render::TransformPreview { transaction: 1, moving: true,
        target: crate::layer_tests::placement::target(&doc), selection: None,
        transform: layer_core::ImageTransform { placement: layer_core::LayerPlacement::from_projective(
            layer_core::Projective::rect_to_quad(layer_core::Rect::from_extent(extent),
                [[900., 150.], [5140., 100.], [5900., 3920.], [150., 3940.]]
                    .map(|[x, y]| layer_core::Point { x, y })).unwrap()), ..Default::default() } };
    crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);
    assert!(r.scene_pipelines.fine_resample.mesh.iter().all(|p| !p.ready()));
    assert!(!r.raster_dependencies_ready(frame, Some(&preview)), "the first fine draw must wait for its cold pipelines");
    while !r.raster_dependencies_ready(frame, Some(&preview)) {
        r.poll_startup().unwrap();
        assert!(std::time::Instant::now() < deadline, "Fine transform preparation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(r.scene_pipelines.fine_resample.mesh[4..].iter().all(|p| p.ready()), "admission prepares what the admitted preview binds");
    assert!(r.scene_pipelines.fine_resample.mesh[..4].iter().all(|p| !p.ready()));
    assert!(r.scene_pipelines.resample.mesh[..4].iter().all(|p| !p.ready()));
    r.set_transform_preview(Some(&preview)).unwrap();
    let selected = request(&r, frame).unwrap();
    assert_eq!(selected.plan.level, 1);
    assert!(matches!(selected.evaluation, Evaluation::Tiled(_)));
    crate::test_support::submit_published(&mut r, frame);
    assert!(matches!(r.scale_display.as_ref().unwrap().evaluation, Evaluation::Tiled(_)));
    assert!(r.scene_pipelines.fine_resample.mesh[..4].iter().all(|p| !p.ready()));
}

#[test]
fn fitted_projective_preview_reuses_coarse_pixels_and_bounds_fine_source_ownership() {
    let extent=[6000,4000];
    let mut doc=layer_core::Document::new(layer_core::PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"bounded projective source".into(),paper:"Paper".into()});
    crate::layer_tests::placement::set_source(&mut doc,rgba8_source(extent,|_,_|[127,63,191,255]));
    let id=crate::layer_tests::placement::target(&doc);
    let mut r=crate::memory::memory_tests::legacy_scene_renderer(Default::default());
    let mut frame=packet(doc.scene(),extent);
    frame.view.width_px=1920;frame.view.height_px=1080;
    frame.view.document_to_surface=[0.27,0.,0.,0.27,0.,0.];
    crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);crate::test_support::submit_ready(&mut r, frame).unwrap();r.wait_idle().unwrap();
    frame.composite_all=false;
    let mut preview=layer_render::TransformPreview {transaction:1,moving:true,target:id,selection:None,
        transform:layer_core::ImageTransform::affine(layer_core::Affine([0.9,0.,0.,0.9,0.,0.]))};
    r.set_transform_preview(Some(&preview)).unwrap();frame.time_seconds+=1./60.;crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);crate::test_support::submit_ready(&mut r, frame).unwrap();r.wait_idle().unwrap();
    preview.transform.placement=layer_core::LayerPlacement::from_projective(layer_core::Projective::rect_to_quad(
        layer_core::Rect {min:Default::default(),max:layer_core::Point {x:6000.,y:4000.}},
        [[900.,150.],[5140.,100.],[5900.,3920.],[150.,3940.]].map(|[x,y]|layer_core::Point {x,y})).unwrap());
    let bounded_owned=|r:&WgpuRasterizer| {
        let cache=r.scale_display.as_ref().unwrap();let scene=r.scene.as_ref().unwrap();
        resident_bytes_with_pool(cache,scene).saturating_sub(cache.resident_bytes())+r.navigator.storage_bytes()
            +r.transforms.as_ref().unwrap().storage_bytes()
            +r.retired_display_bytes.used()
    };
    let mut preparation_trace=Vec::new();
    for _ in 0..65 {
        r.set_transform_preview(Some(&preview)).unwrap();frame.time_seconds+=1./60.;crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(bounded_owned(&r)<=crate::memory::CACHE_BYTES,"pending composition/fine owners stay within608MiB: {}",bounded_owned(&r));
        r.wait_idle().unwrap();
        preparation_trace.push((r.scale_display.as_ref().and_then(|cache|cache.evaluation.fine_capacity()),crate::scene::scale::tests::publication_pending(&r),
            r.transforms.as_ref().unwrap().fine_progress_bytes(),r.navigator.pending()));
        if r.next_frame()!=layer_render::FrameDemand::Work {break;}
    }
    assert!(r.next_frame()!=layer_render::FrameDemand::Work,"immutable fine inputs finish without canceling current-pose progress: {preparation_trace:?}");
    for _ in 0..20 {
        preview.transform.placement=preview.transform.placement.post(layer_core::Projective::from_affine(layer_core::Affine([1.,0.,0.,1.,1.25,0.5]))).unwrap();
        let previous=r.composite_revision;
        r.set_transform_preview(Some(&preview)).unwrap();frame.time_seconds+=1./60.;crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(bounded_owned(&r)<=crate::memory::CACHE_BYTES,"pending composition/fine owners stay within608MiB: {}",bounded_owned(&r));
        r.wait_idle().unwrap();
        assert!(!crate::scene::scale::tests::publication_pending(&r) && r.composite_revision!=previous,"each moving pose must publish fresh pixels instead of submitting its old display backup");
        assert!(matches!(r.scale_display.as_ref().unwrap().evaluation,Evaluation::Tiled(_)));
    }
    let cache=r.scale_display.as_ref().unwrap();
    assert!(matches!(cache.evaluation,Evaluation::Tiled(_)),"a fitted projective drag with its Paper backdrop must use bounded fine pages");
    assert_eq!(cache.plan.level,1);
    let scene=r.scene.as_ref().unwrap();
    let owned=resident_bytes(cache,scene).saturating_sub(cache.resident_bytes())+r.navigator.storage_bytes()+r.transforms.as_ref().unwrap().storage_bytes()
        +r.retired_display_bytes.used();
    assert!(owned<=crate::memory::CACHE_BYTES,"bounded composition/fine-source owners stay within608MiB: {owned}");
    let accepted=r.composite_revision;
    preview.transform.placement=layer_core::LayerPlacement::from_projective(layer_core::Projective::rect_to_quad(
        layer_core::Rect {min:Default::default(),max:layer_core::Point {x:6000.,y:4000.}},
        [[2900.,100.],[3100.,100.],[5990.,3990.],[10.,3990.]].map(|[x,y]|layer_core::Point {x,y})).unwrap());
    r.set_transform_preview(Some(&preview)).unwrap();frame.time_seconds+=1./60.;crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);crate::test_support::submit_ready(&mut r, frame).unwrap();
    assert!(crate::scene::scale::tests::publication_pending(&r) && r.transforms.as_ref().unwrap().fine_progress_bytes()>0,"deep projective planning retains a partial private output");
    assert_eq!(r.composite_revision,accepted,"partial output must not replace the accepted display");
    assert!(bounded_owned(&r)<=crate::memory::CACHE_BYTES);
    r.set_transform_preview(None).unwrap();frame.time_seconds+=1./60.;crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);crate::test_support::submit_ready(&mut r, frame).unwrap();r.wait_idle().unwrap();
    assert!(!r.transforms.as_ref().unwrap().has_preview());
    assert_eq!(r.transforms.as_ref().unwrap().fine_progress_bytes(),0);
    assert_eq!(r.retired_display_bytes.used(),0);
    preview.transaction+=1;
    for _ in 0..65 {
        r.set_transform_preview(Some(&preview)).unwrap();frame.time_seconds+=1./60.;crate::memory::memory_tests::legacy_scene_frame(&mut r, frame);crate::test_support::submit_ready(&mut r, frame).unwrap();
        assert!(bounded_owned(&r)<=crate::memory::CACHE_BYTES);r.wait_idle().unwrap();
        if r.next_frame()!=layer_render::FrameDemand::Work {break;}
    }
    assert!(r.next_frame()!=layer_render::FrameDemand::Work,"a canceled partial output cannot prevent the next transaction from finishing");
    assert!(matches!(r.scale_display.as_ref().unwrap().evaluation,Evaluation::Tiled(_)));
}

#[test]
fn fine_memory_admission_cannot_override_a_visible_native_effect() {
    let mut doc=Document::new(PortableId::random(),6000,4000,layer_core::DocumentNames {paint:"paint".into(),paper:"Paper".into()});
    assert!(!requires_native_effects(doc.scene(),1),"ordinary Paper does not require native filter evaluation");
    let target=crate::layer_tests::placement::target(&doc);
    let preview=layer_render::TransformPreview {transaction:1,moving:true,target,selection:None,transform:Default::default()};
    assert!(fine_source(Evaluation::Tiled(48),Some(&preview),target));
    assert!(!fine_source(Evaluation::Display,Some(&preview),target));
    assert!(!fine_source(Evaluation::Native,Some(&preview),target));
    assert!(!fine_source(Evaluation::Tiled(48),None,target));
    assert!(!fine_source(Evaluation::Tiled(48),Some(&preview),SourceTarget::Paint(layer_core::PaintHandle::from_index(99))));
    let mut draft=crate::tests::image_windows::program(false,false);
    Arc::make_mut(&mut draft.program).resolution=layer_core::EffectResolution::Native;
    let effect=effect_occurrence(&mut doc,draft,"native image windows");
    insert_occurrence(&mut doc,effect,0);
    assert!(requires_native_effects(doc.scene(),1));
    remove_occurrence(&mut doc,effect);
    assert!(!requires_native_effects(doc.scene(),1));
}

#[path = "batch_failure_tests.rs"]
mod batch_failure;

#[test]
fn each_traversal_reads_only_the_inputs_its_declaration_recorded() {
    let extent=[600,300];
    let photo=|width:u32|rgba8_source([width,extent[1]],|x,y|[(x*7) as u8,(y*5) as u8,90,201]);
    for name in ["decoded","absent","placed-integer","group","pass-through","mask","effect","resident-preview","brush-dry","brush-wet","brush-watercolor","brush-preview","brush-watercolor-preview"] {
        let mut doc=document_at(extent);
        let ink=doc.scene().order()[0];
        match name {
            "absent"=>paint_mut(&mut doc,0).base=Some(layer_core::authored::PaintBase::new(photo(256).into())),
            "placed-integer"=>doc.artwork.occurrences.get_mut(ink).unwrap().offset=[273,31],
            "group"|"pass-through"=>{
                let under=paint_occurrence(&mut doc,"under",Some(photo(extent[0])));
                let group=stack_occurrence(&mut doc,"group",vec![ink,under]);
                set_root_entries(&mut doc,vec![group]);
                let mask=coverage_mask(&mut doc,group,[0;2],None);doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.6;
                let occurrence=doc.artwork.occurrences.get_mut(group).unwrap();occurrence.opacity=0.5;
                occurrence.blend=if name=="group" {layer_core::LayerBlend::Multiply}else {layer_core::LayerBlend::PassThrough};
                reindex(&mut doc);
            },
            "mask"=>{
                let mask=coverage_mask(&mut doc,ink,[17,0],None);doc.artwork.coverage.get_mut(mask).unwrap().default_coverage=0.42;
                doc.artwork.occurrences.get_mut(ink).unwrap().mask.as_mut().unwrap().inverted=true;reindex(&mut doc);
            },
            "effect"=>{let filter=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("exposure").program()),"exposure");insert_occurrence(&mut doc,filter,0);},
            _=>{},
        }
        let target=primary_target(&doc);
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame=packet(doc.scene(),extent);
        let mut reads=Vec::new();
        if name.starts_with("brush-") {submit_published(&mut r,frame);}
        let preset=match name {
            "brush-dry"=>layer_core::DefaultBrushPreset::Paintbrush,
            "brush-wet"|"brush-preview"=>layer_core::DefaultBrushPreset::WetRound,
            "brush-watercolor"|"brush-watercolor-preview"=>layer_core::DefaultBrushPreset::WatercolorWash,
            _=>layer_core::DefaultBrushPreset::GPen,
        };
        let style=crate::layer_tests::preset_style(preset);
        let mut dab=crate::tests::test_dab([150.,150.],[0.02,0.07,0.15,0.63],0.9);
        if let Some(dry)=style.contact.and_then(|contact|contact.dry) {
            let relief=dry.pose(0.5).relief;dab.contact=[0.7,relief[0],relief[1],relief[2]];dab.previous_contact=dab.contact;
        }
        if name=="resident-preview" {
            let stroke=crate::test_support::dab_batch(target,style.clone(),dab.bounds());
            crate::test_support::submit_settled(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&stroke),..frame});
        }
        let mut preview=crate::test_support::dab_batch(target,style.clone(),dab.bounds());
        preview.kind=layer_render::DabBatchKind::Preview;preview.stroke_end=false;
        for (step,scale) in [0.5,1.].into_iter().enumerate() {
            frame.view.document_to_surface=[scale,0.,0.,scale,0.,0.];
            crate::scene::sources::Inputs::begin_accepted_reads();
            if name.starts_with("brush-") {
                let mut contacts=[dab;2];contacts[1].center.x+=8.;
                if matches!(name,"brush-preview"|"brush-watercolor-preview") {
                    let persistent=layer_render::DabBatch {stroke_id:layer_core::StrokeId(step as u64+1),stroke_end:false,
                        ..crate::test_support::dab_batch(target,style.clone(),dab.bounds())};
                    crate::test_support::submit_ready(&mut r,FramePacket {dabs:&[dab],dab_batches:&[persistent],..frame})
                        .unwrap_or_else(|error|panic!("{name} persistent prefix at scale {scale}: {error:?}"));
                }
                let batches:Vec<_>=contacts.iter().enumerate().map(|(index,dab)|layer_render::DabBatch {
                    first_dab:index as u32,stroke_id:layer_core::StrokeId(step as u64+1),
                    stroke_start:index==0 && !matches!(name,"brush-preview"|"brush-watercolor-preview"),stroke_end:false,
                    kind:if matches!(name,"brush-preview"|"brush-watercolor-preview") {layer_render::DabBatchKind::Preview}else {layer_render::DabBatchKind::Persistent},
                    ..crate::test_support::dab_batch(target,style.clone(),dab.bounds())}).collect();
                crate::test_support::submit_ready(&mut r,FramePacket {dabs:&contacts,dab_batches:&batches,..frame})
                    .unwrap_or_else(|error|panic!("{name} at scale {scale}: {error:?}"));
            }else if name=="resident-preview" {
                crate::test_support::submit_ready(&mut r,FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&preview),..frame}).unwrap();
            }else {submit_published(&mut r,frame);}
            reads.extend(crate::scene::sources::Inputs::take_accepted_read_results());
        }
        submit_published(&mut r,frame);
        let captured=r.artwork_frame.clone().unwrap();
        let mut capture=crate::artwork::Capture::default();
        crate::scene::sources::Inputs::begin_accepted_reads();
        for coordinate in page_coordinates(PixelRect::full(extent)) {
            let region=page_rect(coordinate).intersect(PixelRect::full(extent));
            let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
            crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|capture.region(r,captured.packet(extent),region,[PAGE_SIZE;2],encoder));
            crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|capture.layer_tile(r,target,coordinate,encoder));
            crate::test_support::query_ready(&mut r,&mut encoder,|r,encoder|r.read_raw_neighborhood(target,coordinate,[Some([-1,0]),Some([0,0]),Some([1,0])],encoder));
            r.uploads.finish(&encoder);encoder.submit(&r.queue);
        }
        crate::test_support::complete(&r);
        reads.extend(crate::scene::sources::Inputs::take_accepted_read_results());
        assert!(reads.iter().map(|(count,_)|count).sum::<usize>()>0,"{name}: every traversal family must consume authored sources");
        assert!(reads.iter().all(|(_,unexpected)|unexpected.is_empty()),"{name}: {reads:?}");
    }
}

fn submit_unpinned(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        r.poll_pending(frame.view).unwrap();
        if r.can_submit() && let std::task::Poll::Ready(result) = r.submit_when_ready(frame, None) { return result.unwrap(); }
        assert!(std::time::Instant::now() < deadline, "frame dependencies must become ready");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn pan_frame(prepare: impl FnOnce(&mut WgpuRasterizer, FramePacket<'_>)) -> WgpuRasterizer {
    let mut doc = Document::new(PortableId::random(), 8192, 4096, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.width_px = 2048; frame.view.height_px = 2048;
    frame.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    crate::test_support::submit_published(&mut r, frame);
    frame.reset_layers = false;
    crate::test_support::complete(&r);
    crate::test_support::pace(&mut r);
    prepare(&mut r, frame);
    frame.view.document_to_surface[4] = -6144.;
    submit_unpinned(&mut r, frame);
    r
}

#[test]
fn a_pan_frame_spends_only_the_interactive_budget_on_entering_pages() {
    let r = pan_frame(|_, _| {});
    let pages = r.metrics.frame_composited_pages.len() as u64;
    assert!(pages > 0 && pages <= r.test.frame_work[0] / object_cache::PASS_TAPS + 1, "a pan frame composed {pages} entering pages");
}

#[test]
fn a_pan_frame_behind_a_frame_in_flight_longer_than_its_interval_composes_one_entering_page() {
    let r = pan_frame(|r, frame| {
        let [(first, ..), (second, ..)] = [0; 2].map(|_| crate::test_support::bounded_outstanding_gpu_work(r));
        first.submit(&r.queue);
        r.submit(frame).unwrap();
        crate::test_support::complete(r);
        second.submit(&r.queue);
        r.submit(frame).unwrap();
    });
    assert!(r.metrics.frame_composited_pages.len() <= 1, "a pan frame behind a frame in flight longer than its interval composed {} entering pages", r.metrics.frame_composited_pages.len());
}

fn pan_behind_a_presentation(presentation_ms: f64, interval_ms: f64, steps: usize) -> (Vec<usize>, Vec<u64>) {
    let mut doc = Document::new(PortableId::random(), 8192, 4096, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    add_fill(&mut doc, layer_core::color::RgbColor::WHITE);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.width_px = 2048; frame.view.height_px = 2048;
    frame.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    crate::test_support::submit_published(&mut r, frame);
    frame.reset_layers = false;
    let mut presentations: Vec<_> = {
        let (_, calibrated, raw) = crate::test_support::bounded_outstanding_gpu_work(&r);
        let divisor = (calibrated() / presentation_ms).round().max(1.) as u32;
        (0..256).map(|_| raw(divisor)).collect()
    };
    let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
    crate::test_support::prepare_lit_presenter(&r, &mut presenter);
    let (_texture, target) = crate::create_target(&r.device, [2048, 2048], wgpu::TextureFormat::Rgba8UnormSrgb, "paced pan presentation");
    let interval = std::time::Duration::from_secs_f64(interval_ms / 1000.);
    let mut started = std::time::Instant::now();
    r.frame_pace.pin(interval_ms, [crate::frame_pace::MOVING_TAPS, crate::frame_pace::IDLE_TAPS]);
    let mut tick = |r: &mut WgpuRasterizer, frame: FramePacket<'_>| {
        loop {
            std::thread::sleep(interval.saturating_sub(started.elapsed()));
            started = std::time::Instant::now();
            r.poll_pending(frame.view).unwrap();
            if r.can_submit() && let std::task::Poll::Ready(result) = r.submit_when_ready(frame, None) { result.unwrap(); break; }
        }
        if presenter.needs_present(r, frame.view, [0.; 4]) {
            presenter.present(r, &target, frame.view, [0.; 4]).unwrap();
            r.device.submit(&r.queue, [presentations.pop().expect("enough presentations")]).unwrap();
        }
    };
    let mut entering = Vec::new();
    for step in 1..=steps {
        frame.view.document_to_surface[4] = -256. * step as f32;
        tick(&mut r, frame);
        entering.push(r.metrics.frame_composited_pages.len());
    }
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    let mut fill = Vec::new();
    while publication_pending(&r) {
        assert!(std::time::Instant::now() < deadline, "the panned view fills");
        tick(&mut r, frame);
        fill.push(r.test.frame_work[0]);
    }
    (entering, fill)
}

#[test]
fn a_pan_behind_a_presentation_longer_than_its_interval_composes_entering_pages_steadily_and_fills_in_whole_idle_frames() {
    let (entering, fill) = pan_behind_a_presentation(18., 1000. / 90., 8);
    assert!(entering[1..].iter().all(|pages| *pages == 1), "moving frames behind a presentation longer than their window compose one entering page each: {entering:?}");
    assert!(!fill.is_empty() && fill.iter().all(|taps| *taps >= crate::frame_pace::IDLE_TAPS), "frames filling the view after the pan admit less than the fixed idle budget: {fill:?}");
}

#[test]
fn entering_visible_pages_decode_as_visible_work_while_the_view_moves() {
    let doc = document_at([8192, 1024]);
    let extent = doc.composition().size;
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut frame = packet(doc.scene(), extent);
    frame.composite_all = false;
    frame.view.width_px = 1024; frame.view.height_px = 1024;
    frame.view.document_to_surface = [1., 0., 0., 1., 0., 0.];
    crate::test_support::submit_published(&mut r, frame);
    frame.view.document_to_surface[4] = -6144.;
    crate::test_support::submit_ready(&mut r, frame).unwrap();
    let mut classes = r.moving_decode.as_ref().unwrap().1.outstanding_classes();
    classes.extend(r.source_results.iter().map(|(_, result, _)| result.class()));
    let composed = !r.metrics.frame_composited_pages.is_empty();
    assert!((classes.contains(&Class::Visible) || composed) && !classes.contains(&Class::Refinement), "entering pages neither composed nor decode as visible work: {classes:?}");
}

fn stroke_composites(node: &super::graph::Node, target: SourceTarget) -> Option<usize> {
    match node.as_ref() {
        super::graph::Expression::Source { id, .. } => (*id == target).then_some(0),
        super::graph::Expression::Opacity { input, .. } => stroke_composites(input, target),
        super::graph::Expression::Combine { front, back, .. } => [front, back].into_iter().filter_map(|node| stroke_composites(node, target)).min().map(|path| path + 1),
        super::graph::Expression::Effect { input, .. } => stroke_composites(input, target).map(|path| path + 1),
        _ => None,
    }
}

#[test]
fn a_stroke_on_the_bottom_layer_recomposes_a_constant_number_of_planes_whatever_the_layer_count() {
    let composites = |layers: usize| {
        let mut doc = document_at([512, 512]);
        let bottom = primary_target(&doc);
        for index in 1..layers {
            let layer = paint_occurrence(&mut doc, &format!("Layer {index}"), Some(rgba8_source([512; 2], move |x, y| [(x / 2) as u8, index as u8, (y / 2) as u8, 160]).into()));
            insert_occurrence(&mut doc, layer, 0);
        }
        let extent = doc.composition().size;
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let mut frame = packet(doc.scene(), extent);
        frame.composite_all = false;
        crate::test_support::submit_published(&mut r, frame);
        let dab = crate::tests::test_dab([200., 200.], [0.9, 0.1, 0.3, 1.], 0.8);
        let batch = dab_batch(bottom, crate::layer_tests::preset_style(DefaultBrushPreset::GPen), dab.bounds());
        crate::test_support::submit_ready(&mut r, FramePacket { dabs: std::slice::from_ref(&dab), dab_batches: std::slice::from_ref(&batch), ..frame }).unwrap();
        let root = r.scale_display.as_ref().unwrap().graph.root.clone().unwrap();
        stroke_composites(&root, bottom).expect("the stroke target is composed")
    };
    assert_eq!(composites(28), composites(4), "a stroke unit's layer composites grow with the layers above it");
}

fn noisy_photo_project(extent:[u32;2])->Document {
    let source=rgba8_source(extent,|x,y| {
        let mut h=x.wrapping_mul(0x9E37_79B1)^y.wrapping_mul(0x85EB_CA77);h^=h>>15;h=h.wrapping_mul(0x2C1B_3C6D);h^=h>>12;
        let noise=((h&255) as i32+((h>>8)&255) as i32+((h>>16)&255) as i32-384)/4;
        let base=[90+(x*80/extent[0]) as i32,120-(y*60/extent[1]) as i32,110+((x+y)*50/(extent[0]+extent[1])) as i32];
        let v=base.map(|b|(b+noise).clamp(0,255) as u8);[v[0],v[1],v[2],255]
    });
    layer_color::photo_project(Arc::unwrap_or_clone(source),Default::default(),"Ink".into(),SampleDepth::U8).unwrap()
}
fn renderer_on_device(doc:&Document,device_bytes:u64)->WgpuRasterizer {
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(32<<30),override_bytes:Some(32<<30),..Default::default()});
    crate::memory::memory_tests::renderer_in_budget(doc.composition().color,
        layer_core::DeviceMemoryFacts {budget_bytes:Some(device_bytes),heap_bytes:Some(device_bytes/3*4),unified:false,..Default::default()},budget).unwrap()
}
fn fit_frame(doc:&Document,divisor:u32)->FramePacket<'_> {
    let extent=doc.composition().size;
    let mut frame=packet(doc.scene(),extent);frame.composite_all=false;frame.blend_space=doc.composition().blend;
    frame.view=crate::test_support::view([extent[0]/divisor,extent[1]/divisor]);frame.view.document_to_surface=[1./divisor as f32,0.,0.,1./divisor as f32,0.,0.];
    frame
}
fn settle_with_room(r:&mut WgpuRasterizer,frame:FramePacket<'_>) {
    use crate::frame_demand::{Input,Outstanding};
    let idle=FramePacket {dabs:&[],dab_batches:&[],restore_rasters:&[],reset_layers:false,composite_all:false,..frame};
    let (mut first,mut refused)=(true,0);
    for _ in 0..4000 {
        r.poll_pending(frame.view).unwrap();crate::test_support::settle_decodes(r);
        if r.can_submit() && let std::task::Poll::Ready(result)=r.submit_when_ready(if first {frame}else {idle},None) {result.unwrap();first=false;}
        r.wait_idle().unwrap();
        if !first && r.next_frame()!=layer_render::FrameDemand::Work && r.settled() {return;}
        refused=if !first && r.outstanding_terms()==[("display",Outstanding::Blocked(Input::Residency))] {refused+1}else {0};
        let display=&r.scene_tiers.display;
        assert!(refused<=200,"the display stayed refused with device room free: used {} grant {} floor {}",display.used(),display.grant(),display.floor());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the view did not settle: {:?}",r.outstanding_terms());
}

#[test]
fn a_noisy_photo_view_settles_when_the_device_has_room() {
    let doc=noisy_photo_project([8192,8192]);
    for divisor in [8,4] {
        let mut r=renderer_on_device(&doc,72<<30);
        settle_with_room(&mut r,fit_frame(&doc,divisor));
    }
}

#[test]
fn a_large_display_prefetch_refusal_never_shrinks_the_raster_grant() {
    let doc=document_at([2048,2048]);
    let mut r=renderer_on_device(&doc,4<<30);
    let frame=fit_frame(&doc,4);
    settle_with_room(&mut r,frame);
    r.frame_room(frame);
    let budget=r.factory.memory_budget().clone();
    let snapshot=budget.snapshot();
    let display=r.memory.display_owner().reported();
    let others=snapshot.devices[0].pinned_bytes.saturating_sub(display.pinned_gpu_bytes);
    let painting=snapshot.devices[0].limit_bytes.saturating_sub(display.demand_gpu_bytes.max(display.gpu_bytes)+others+(64<<20));
    let _painting=r.raster_tiers.raster.charge(painting);
    r.frame_room(frame);
    let before=r.memory.raster_grant();
    assert!(before>=painting,"the raster's resident paint fits beside the display's demand: grant {before} paint {painting}");
    let cache=r.scale_display.as_ref().unwrap();
    cache.evict_window(cache.plan.bounds);
    let refusal=r.scene_tiers.display.blocked(crate::residency::Class::Prefetch,2<<30).expect("the prefetch does not fit");
    cache.refuse(None,cache.plan.bounds,&refusal);
    r.frame_room(frame);
    assert!(r.memory.raster_grant()>=before,"a display prefetch refusal took the raster's room: {} -> {}",before,r.memory.raster_grant());
}

fn settle_to_wait(r:&mut WgpuRasterizer,frame:FramePacket<'_>)->usize {
    let mut submitted=0;
    for _ in 0..64 {
        r.poll_startup().unwrap();r.poll_pending(frame.view).unwrap();
        if r.can_submit() && let std::task::Poll::Ready(result)=r.submit_when_ready(if submitted==0 {frame}else {FramePacket {composite_all:false,..frame}},None) {
            result.unwrap();submitted+=1;
        }
        r.wait_idle().unwrap();
        if submitted>0 && r.next_frame()!=layer_render::FrameDemand::Work {break;}
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    submitted
}

fn pending_color_tile(doc:&mut Document,coordinate:[u32;2])->layer_core::raster::RasterTile {
    use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileKey};
    let tile=RasterTile::pending(doc.composition().color.paint_descriptor());
    let mut data=RasterData::default();
    data.tiles.insert(TileKey {plane:RasterPlane::Color,coordinate},tile.clone());
    paint_mut(doc,0).raster=RasterRevision::backed(data);
    tile
}

fn publish_clear(r:&WgpuRasterizer,doc:&Document,tile:&layer_core::raster::RasterTile) {
    let descriptor=doc.composition().color.paint_descriptor();
    tile.publish(Ok(layer_core::raster::TileBlob::encode(descriptor,&vec![0;256*256*descriptor.bytes_per_pixel().unwrap()]).unwrap())).unwrap();
    r.raster_name_waker().unwrap().wake_by_ref();
}

#[test]
fn display_units_reading_an_unpublished_page_wait_for_its_wake_instead_of_spinning() {
    use crate::frame_demand::{Input,Outstanding};
    let extent=[512,512];
    let mut doc=document_at(extent);
    let tile=pending_color_tile(&mut doc,[0,0]);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.set_preparation_wake(Arc::new(||{}));
    let mut frame=packet(doc.scene(),extent);
    frame.view.document_to_surface=[0.5,0.,0.,0.5,0.,0.];
    let submitted=settle_to_wait(&mut r,frame);
    assert!(submitted>0 && r.next_frame()!=layer_render::FrameDemand::Work,"units whose page is unpublished must wait, not demand frames: {:?}",r.outstanding_terms());
    assert!(r.outstanding_terms().contains(&("display",Outstanding::Blocked(Input::Name))),"{:?}",r.outstanding_terms());
    assert!(publication_pending(&r));
    publish_clear(&r,&doc,&tile);
    assert!(r.next_frame()==layer_render::FrameDemand::Work,"the page's publication resumes its units");
    submit_published(&mut r,FramePacket {composite_all:false,..frame});
}

#[test]
fn a_transition_held_for_an_unpublished_page_waits_for_it_instead_of_spinning() {
    use crate::frame_demand::{Input,Outstanding};
    let extent=[1024,512];
    let doc=document_at(extent);
    let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    r.set_preparation_wake(Arc::new(||{}));
    let view=[0.5,0.,0.,0.5,0.,0.];
    let mut frame=packet(doc.scene(),extent);frame.view.document_to_surface=view;
    submit_published(&mut r,frame);
    let mut changed=doc.clone();
    occurrence_mut(&mut changed,0).opacity=0.5;
    let tile=pending_color_tile(&mut changed,[0,0]);
    let mut frame=packet(changed.scene(),extent);frame.view.document_to_surface=view;
    let submitted=settle_to_wait(&mut r,frame);
    assert!(submitted>0 && r.next_frame()!=layer_render::FrameDemand::Work,"a transition held for an unpublished page must wait, not demand frames: {:?}",r.outstanding_terms());
    assert!(r.outstanding_terms().contains(&("display",Outstanding::Blocked(Input::Name))),"{:?}",r.outstanding_terms());
    publish_clear(&r,&changed,&tile);
    assert!(r.next_frame()==layer_render::FrameDemand::Work,"the page's publication resumes the transition");
    submit_published(&mut r,FramePacket {composite_all:false,..frame});
}

fn cold_photo_at_fit(display_grant:u64,frames:u32,done:impl Fn(&WgpuRasterizer)->bool)->Option<u32> {
    let extent=[12000,8000];
    let doc=document_at(extent);
    let ram=2<<30;
    let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(ram),override_bytes:Some(ram),..Default::default()});
    let mut r=crate::memory::memory_tests::renderer_in_budget(doc.composition().color,layer_core::DeviceMemoryFacts {budget_bytes:Some(4<<30),..Default::default()},budget).unwrap();
    let zoom=0.14505f32;
    let view=layer_render::ViewState {width_px:(extent[0] as f32*zoom).ceil() as u32,height_px:(extent[1] as f32*zoom).ceil() as u32,document_to_surface:[zoom,0.,0.,zoom,0.,0.]};
    let frame=FramePacket {view,..packet(doc.scene(),extent)};
    let _pressure=crate::memory::memory_tests::display_frame_with_budget(&mut r,frame,display_grant);
    (1..=frames).find(|&submitted| {
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(30);
        while r.moving_decode.as_ref().is_some_and(|(_,queue)|!queue.landed()) {
            assert!(std::time::Instant::now()<deadline,"requested decodes land before the next frame");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        r.poll_startup().unwrap();
        crate::test_support::submit_ready(&mut r,if submitted==1 {frame}else {FramePacket {composite_all:false,..frame}}).unwrap();
        r.wait_idle().unwrap();
        r.device.retirement_handle().flush().unwrap();
        done(&r)
    })
}

fn display_written(r:&WgpuRasterizer)->bool {r.scale_display.as_ref().is_some_and(|cache|cache.coverage_complete() && cache.output_complete())}

#[test]
fn a_cold_photo_at_fit_writes_every_display_cell_when_its_exact_cells_fill_the_display_grant() {
    let written=cold_photo_at_fit(200<<20,200,display_written);
    assert!(written.is_some(),"every display cell of a cold 12000x8000 photo at Fit is written within 200 frames on a 200 MiB display grant");
}

#[test]
fn a_cold_photo_at_fit_settles_across_display_grants() {
    for grant in (120..=400).step_by(20) {
        let settled=cold_photo_at_fit(grant<<20,300,|r|display_written(r) && r.next_frame()==layer_render::FrameDemand::Wait);
        assert!(settled.is_some(),"a cold 12000x8000 photo at Fit settles with every display cell written on a {grant} MiB display grant");
    }
}
