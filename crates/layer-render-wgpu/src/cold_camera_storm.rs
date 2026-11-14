use super::*;
use layer_core::{Document,MemoryBudget,MemoryFacts,DeviceMemoryFacts,Point,PortableId};
use layer_core::authored::*;
use layer_core::color::{ColorProfile,RgbSpace,SampleDepth,source::{SourceImage,SourceInterpretation,SourceChannels}};
use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
use layer_engine::{CanvasEngine,PenEvent,PenPhase,SampleFlags,ViewTransform,input_queue};
use layer_render::{CanvasRenderer,ViewState};
use std::sync::{Condvar,Mutex};
use std::collections::BTreeMap;
use std::time::{Duration,Instant};

const QUIET:Duration=Duration::from_secs(1);

const SEEDS:[u64;3]=[0x139a,0x84e7,0xc25d];
const FRAMES:usize=300;
const CPU_MAX:Duration=Duration::from_nanos(33_333_332);
const CPU_P99:Duration=Duration::from_micros(8300);

#[derive(Clone,Copy,Debug)]
enum Fixture {Object,PaintBase,Paint28,Effects,Mixed,Gigapixel,GigapixelPaint,Panorama,MultiGigapixel}
#[derive(Clone,Copy,Debug)]
enum Grant {Desktop,Edge,Tablet}
impl Grant {
    fn renderer(self,color:layer_core::color::DocumentColor)->WgpuRasterizer {
        let (ram,gpu,unified)=match self {Self::Desktop=>(8<<30,8<<30,false),Self::Edge=>(2<<30,768<<20,false),Self::Tablet=>(1<<30,1<<30,true)};
        crate::memory::memory_tests::renderer_in_budget(color,DeviceMemoryFacts {budget_bytes:Some(gpu),unified,..Default::default()},
            MemoryBudget::with_facts(MemoryFacts {host_default_bytes:Some(ram),override_bytes:Some(ram),..Default::default()})).unwrap()
    }
}

fn mix(mut value:u64)->u64 {value^=value>>30;value=value.wrapping_mul(0xbf58476d1ce4e5b9);value^=value>>27;value=value.wrapping_mul(0x94d049bb133111eb);value^(value>>31)}

#[derive(Clone,Copy,Debug)]
enum IoProfile {Adversarial,S7Virtual3ms,HuionVirtual5ms}
impl IoProfile {
    fn frame_ns(self)->u64 {match self {Self::Adversarial|Self::S7Virtual3ms=>8_333_333,Self::HuionVirtual5ms=>11_111_111}}
    fn delay(self,phase:u64)->usize {
        match self {
            Self::Adversarial=>1+(phase%7) as usize,
            Self::S7Virtual3ms|Self::HuionVirtual5ms=>{
                let latency=match self {Self::S7Virtual3ms=>3_000_000,Self::HuionVirtual5ms=>5_000_000,_=>unreachable!()};
                usize::from(phase%self.frame_ns()+latency>=self.frame_ns())
            }
        }
    }
}
#[derive(Default,Debug)]
struct CoverageProgress {
    previous:Option<([u64;3],f64)>,first_visible:Option<usize>,zero:usize,partial:usize,full:usize,
}
impl CoverageProgress {
    fn observe(&mut self,frame:usize,context:[u64;3],covered:f64,visible:f64,artwork:bool) {
        if let Some((previous,area))=self.previous && previous==context {
            let projection_roundoff=visible.max(1.)*f64::EPSILON*128.;
            assert!(covered+projection_roundoff>=area,"compatible view lost committed coverage: frame={frame}, previous={area}, covered={covered}, context={context:?}");
        }
        self.previous=Some((context,covered));
        if self.first_visible.is_none() && artwork && covered>0. {self.first_visible=Some(frame);}
        if visible>0. && self.first_visible.is_some() {
            if covered==0. {self.zero+=1;}else if covered+visible*f64::EPSILON*128.<visible {self.partial+=1;}else{self.full+=1;}
        }
    }
}

#[derive(Default)]
struct EvictionLag {over:[usize;2],worst:[usize;2]}
impl EvictionLag {
    fn observe(&mut self,r:&WgpuRasterizer,context:std::fmt::Arguments<'_>) {
        let resident=[r.scene_tiers.display.used().saturating_sub(r.scene_tiers.retiring.used()),r.raster_tiers.raster.used().saturating_sub(r.raster_buffers.transfer.used())];
        let grants=[r.memory.display_owner().grant().gpu_bytes,r.memory.raster_owner().grant().gpu_bytes];
        for owner in 0..2 {
            self.over[owner]=if resident[owner]>grants[owner] {self.over[owner]+1}else {0};
            self.worst[owner]=self.worst[owner].max(self.over[owner]);
            assert!(self.over[owner]<=1,"{} stayed above its grant past the next frame: {context}, resident={}, grant={}",["display","raster"][owner],resident[owner],grants[owner]);
        }
    }
}

struct Reads {state:Arc<(Mutex<(usize,bool,usize)>,Condvar)>,seed:u64,profile:IoProfile}
impl Reads {
    fn new(seed:u64,profile:IoProfile)->Self {Self {state:Arc::new((Mutex::new((0,false,0)),Condvar::new())),seed,profile}}
    fn observe(&self)->Arc<dyn Fn(u64,usize)+Send+Sync> {
        let state=self.state.clone();let seed=self.seed;let profile=self.profile;
        Arc::new(move |offset,length| {
            let mut step=state.0.lock().unwrap();
            let ready=step.0+profile.delay(mix(seed^offset^((length as u64)<<40)));
            step.2+=1;
            while !step.1 && step.0<ready {step=state.1.wait(step).unwrap();}
            step.2-=1;
        })
    }
    fn gated(&self)->bool {self.state.0.lock().unwrap().2!=0}
    fn advance(&self,frame:usize) {self.state.0.lock().unwrap().0=frame;self.state.1.notify_all();}
    fn release(&self) {self.state.0.lock().unwrap().1=true;self.state.1.notify_all();}
}
impl Drop for Reads {fn drop(&mut self) {self.release();}}

fn image(extent:[u32;2],profile:ColorProfile,seed:u64)->layer_core::Image {
    let interpretation=SourceInterpretation {channels:SourceChannels::Rgb,depth:SampleDepth::U8,profile,profile_assumed:false};
    let descriptor=interpretation.descriptor();let mut tiles=BTreeMap::new();
    for y in 0..extent[1].div_ceil(PAGE_SIZE) {for x in 0..extent[0].div_ceil(PAGE_SIZE) {
        let tag=mix(seed^u64::from(x)^(u64::from(y)<<32));
        let mut bytes=vec![0;PAGE_SIZE.pow(2) as usize*3];
        for row in 0..(extent[1]-y*PAGE_SIZE).min(PAGE_SIZE) {for column in 0..(extent[0]-x*PAGE_SIZE).min(PAGE_SIZE) {
            let at=((row*PAGE_SIZE+column)*3) as usize;
            bytes[at..at+3].copy_from_slice(&[(column as u8).wrapping_add(tag as u8),(row as u8).wrapping_add((tag>>8) as u8),96]);
        }}
        tiles.insert([x,y],Arc::new(TileBlob::encode(descriptor,&bytes).unwrap()));
    }}
    layer_core::Image::new(Arc::new(SourceImage {extent,resolution:None,interpretation,tiles}))
}

fn document(fixture:Fixture,seed:u64)->Document {
    let extent=match fixture {Fixture::Paint28=>[4216,3016],Fixture::Gigapixel|Fixture::GigapixelPaint|Fixture::MultiGigapixel=>[32768,30518],Fixture::Panorama=>[65536,16384],_=>[9504,6336]};
    let admitted=extent.map(|n|n.min(layer_core::MAX_EXTENT));
    let mut artwork=Artwork::new(admitted).unwrap();
    let (owner,target)=crate::test_support::add_paint(&mut artwork,"Ink",admitted);
    let color=layer_core::color::DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U8};
    artwork.compositions.get_mut(artwork.root).unwrap().color=color;
    if matches!(fixture,Fixture::Paint28|Fixture::GigapixelPaint) {
        for layer in 0..if matches!(fixture,Fixture::Paint28) {28}else {1} {
            let id=if layer==0 {target}else {crate::test_support::add_paint(&mut artwork,"Raster",extent).1};
            let SourceTarget::Paint(handle)=id else {unreachable!()};
            let descriptor=color.paint_descriptor();let mut tiles=BTreeMap::new();
            for y in 0..extent[1].div_ceil(PAGE_SIZE) {for x in 0..extent[0].div_ceil(PAGE_SIZE) {
                let value=mix(seed^layer^u64::from(x)^(u64::from(y)<<32));
                let sample=[16+(value as u8)%80,24+((value>>8) as u8)%72,32+((value>>16) as u8)%64,128];
                let mut bytes=sample.repeat(PAGE_SIZE.pow(2) as usize);
                for (index,part) in value.to_le_bytes().into_iter().enumerate() {bytes[index*4]=part%128;}
                let width=(extent[0]-x*PAGE_SIZE).min(PAGE_SIZE) as usize;
                let height=(extent[1]-y*PAGE_SIZE).min(PAGE_SIZE) as usize;
                for row in bytes.chunks_exact_mut(PAGE_SIZE as usize*4).take(height) {row[width*4..].fill(0);}
                bytes[height*PAGE_SIZE as usize*4..].fill(0);
                tiles.insert(TileKey {plane:RasterPlane::Color,coordinate:[x,y]},RasterTile::backed(TileBlob::encode(descriptor,&bytes).unwrap()));
            }}
            artwork.paint.get_mut(handle).unwrap().raster=RasterRevision::backed(RasterData {tiles:tiles.into(),watercolor:None});
        }
    } else {
        let source=image([9504,6336],ColorProfile::Builtin(RgbSpace::Srgb),seed);
        if matches!(fixture,Fixture::PaintBase) {
            let SourceTarget::Paint(handle)=target else {unreachable!()};
            artwork.paint.get_mut(handle).unwrap().base=Some(PaintBase::new(source));
        } else {
            let count=match fixture {Fixture::Mixed=>3,Fixture::MultiGigapixel=>2,_=>1};
            for index in 0..count {
                let mut source=source.clone();
                if index!=0 && matches!(fixture,Fixture::Mixed) {
                    let mut storage=(**source.storage()).clone();
                    storage.interpretation.profile=if index==1 {ColorProfile::Builtin(RgbSpace::DisplayP3)}else {
                        ColorProfile::Icc(layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb)).unwrap().into())};
                    source=layer_core::Image::new(Arc::new(storage));
                }
                let mut object=ImageObject::new(source);
                object.affine=Affine64([extent[0] as f64/9504.,0.,0.,extent[1] as f64/6336.,0.,0.]);
                let handle=artwork.objects.insert(PortableId::random(),object).unwrap();
                let entry=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(handle),"Photo")).unwrap();
                let stack=artwork.compositions.get(artwork.root).unwrap().result;
                artwork.stacks.get_mut(stack).unwrap().entries.push(entry);
            }
        }
    }
    if matches!(fixture,Fixture::Effects) {
        let instance=layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program());
        let effect=artwork.effects.insert(PortableId::random(),EffectApplication::new(instance.program,instance.values,extent)).unwrap();
        let entry=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(effect),"Blur")).unwrap();
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries.insert(0,entry);
    }
    let mut document=Document::from_artwork(artwork).unwrap();
    document.working.occurrence=Some(owner);document.working.target=Some(target);
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().size=extent;
    document
}

fn camera(seed:u64,frame:usize,extent:[u32;2])->(ViewState,ViewTransform,Point) {
    let phase=frame%12;
    let zoom=if frame==0 {(3840./extent[0] as f32).min(2160./extent[1] as f32)}else {
        match phase {0|2|5=>0.01,1|3|6=>32.,4=>0.38,7=>0.63,8=>0.64,9=>1.41,10=>1.42,_=>1.}};
    let angle=match frame%8 {0|1=>std::f32::consts::FRAC_PI_4,2|3=>std::f32::consts::FRAC_PI_2,_=>0.};
    let (sin,cos)=angle.sin_cos();
    let viewport=match (frame/17)%3 {0=>[3840,2160],1=>[2560,1600],_=>[1920,1200]};
    let random=mix(seed^frame as u64);
    let fractions=[0.01,0.99,0.5,0.25,0.75];
    let center=Point {x:extent[0] as f32*fractions[(random%5) as usize],y:extent[1] as f32*fractions[((random>>8)%5) as usize]};
    let [a,b,c,d]=[zoom*cos,zoom*sin,-zoom*sin,zoom*cos];
    let translation=[viewport[0] as f32/2.-a*center.x-c*center.y,viewport[1] as f32/2.-b*center.x-d*center.y];
    let determinant=a*d-b*c;
    (ViewState {width_px:viewport[0],height_px:viewport[1],document_to_surface:[a,b,c,d,translation[0],translation[1]]},
        ViewTransform {revision:frame as u64+1,surface_to_document:[d/determinant,-b/determinant,-c/determinant,a/determinant,
            (c*translation[1]-d*translation[0])/determinant,(b*translation[0]-a*translation[1])/determinant]},center)
}

fn engine(document:Document,renderer:WgpuRasterizer,view:ViewState,transform:ViewTransform)->(layer_engine::InputProducer<PenEvent>,CanvasEngine<WgpuRasterizer>) {
    let (input,consumer)=input_queue(1024);
    let mut engine=CanvasEngine::new(renderer,document,consumer,view,transform).unwrap();
    engine.set_brush(layer_core::BrushSnapshot {diameter:24.,color_rgba_linear:[1.,0.,1.,1.],mappings:Arc::from([]),
        ..layer_core::default_brush(layer_core::DefaultBrushPreset::GPen)}).unwrap();
    (input,engine)
}

fn storm(fixture:Fixture,grant:Grant,profile:IoProfile) {
    for seed in SEEDS {
        let _artwork=crate::test_support::SubmittedArtworkObserver::new(format!("{profile:?}/{fixture:?}/{grant:?}, seed={seed:x}"));
        let document=document(fixture,seed);let extent=document.composition().size;
        let reads=Reads::new(seed,profile);
        let retained=layer_core::Editor::new(document.clone()).retained_tiles();
        let _store=crate::test_support::StoredTiles::with_reads(&retained,crate::test_support::tile_read_cache(),Some(reads.observe()));
        let (initial,transform,_)=camera(seed,0,extent);
        let (woken,wakes)=std::sync::mpsc::channel::<()>();
        let wake:Arc<dyn Fn()+Send+Sync>=Arc::new(move||{let _=woken.send(());});
        let mut renderer=grant.renderer(document.composition().color);renderer.set_preparation_wake(wake.clone());
        let (mut input,mut live)=engine(document,renderer,initial,transform);
        let mut presenter=crate::ViewportPresenter::for_surface(live.backend(),wgpu::TextureFormat::Rgba8UnormSrgb,crate::SdrSurfaceColor::Srgb).unwrap();
        presenter.set_overviews(live.backend(),&[crate::OverviewPlacement {bounds:[3500.,24.,312.,192.],clip:None,
            work_area:[[0.;2];4],outline_linear:[1.;3],scale:1.,opacity:1.}]);
        let mut output=create_target(&live.backend().device,[initial.width_px,initial.height_px],wgpu::TextureFormat::Rgba8UnormSrgb,"cold storm presentation");
        let mut lag=EvictionLag::default();
        let mut timings=Vec::new();let mut pictured=false;let mut sequence=0;let mut coverage=CoverageProgress::default();let mut requested_view=initial;
        for frame in 0..FRAMES {
            while wakes.try_recv().is_ok() {}
            reads.advance(frame);
            let cold_stroke=(151..=171).contains(&frame);
            let (view,mut transform,center)=camera(seed,if cold_stroke {4}else {frame},extent);
            transform.revision=frame as u64+1;
            if frame==FRAMES/2 {
                let factory=live.backend().factory.clone();let color=live.document().composition().color;
                let mut renderer=WgpuRasterizer::from_factory_inner(factory.clone(),factory.pipeline_device(color),Initialization::Headless).unwrap();
                renderer.initialize_native(color).unwrap();renderer.set_preparation_wake(wake.clone());
                drop(live.replace_backend(*renderer).unwrap());
            }
            live.set_view(view,transform);requested_view=view;
            if output.0.width()!=view.width_px || output.0.height()!=view.height_px {
                output=create_target(&live.backend().device,[view.width_px,view.height_px],wgpu::TextureFormat::Rgba8UnormSrgb,"resized cold storm presentation");
                presenter.set_overviews(live.backend(),&[crate::OverviewPlacement {bounds:[view.width_px as f32-336.,24.,312.,192.],clip:None,
                    work_area:[[0.;2];4],outline_linear:[1.;3],scale:1.,opacity:1.}]);
            }
            let stroke=cold_stroke || frame>171 && frame%19<3;
            if stroke {
                let target=live.document().active_target().unwrap();let domain=live.document().target_extent(target);
                let point=Point {x:center.x.min(domain[0] as f32-32.),y:center.y.min(domain[1] as f32-32.)};
                let [a,b,c,d,x,y]=view.document_to_surface;sequence+=1;
                input.push(PenEvent {sequence,timestamp_ns:(frame as u64+1)*profile.frame_ns(),
                    phase:if cold_stroke {match frame {151=>PenPhase::Down,171=>PenPhase::Up,_=>PenPhase::Move}}
                        else {match frame%19 {0=>PenPhase::Down,1=>PenPhase::Move,_=>PenPhase::Up}},
                    surface_position:Point {x:a*point.x+c*point.y+x,y:b*point.x+d*point.y+y},view_revision:transform.revision,
                    ..crate::test_support::pen(sequence,PenPhase::Move,[0.;2],SampleFlags::PRIMARY)}).unwrap();
            }
            let device=live.backend().device.clone();let validation=device.push_error_scope(wgpu::ErrorFilter::Validation);
            let start=owner_cpu();
            let counts=crate::test_support::PassCounts::snapshot();
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||live.render_frame()));
            assert_eq!(crate::test_support::PassCounts::snapshot().delta(counts).source_input_misses,0,"accepted input miss: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}");
            assert_eq!(crate::test_support::PassCounts::snapshot().delta(counts).raster_output_misses,0,"accepted output miss: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}");
            assert!(result.is_ok(),"storm panic: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}");
            let result=result.unwrap();
            assert!(result.is_ok(),"storm renderer error: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}: {result:?}");
            let revision=live.document().revision;let owner=live.document().owner;
            let r=live.backend_mut();
            let requested=presenter.needs_present(r,view,[0.18,0.18,0.18,1.]);
            if requested {presenter.present(r,&output.1,view,[0.18,0.18,0.18,1.]).unwrap();}
            assert!(!pictured || presenter.presented_camera()==Some(view),"I2 withheld/current camera: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}, presented={:?}, requested={view:?}",presenter.presented_camera());
            assert!(!pictured || presenter.canvas_revision().is_some(),"I2 withheld artwork: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}");
            timings.push(owner_cpu().saturating_sub(start));
            let [covered,visible,_,known]=r.scale_display.as_ref().map_or([0.,crate::test_support::storm_surface_area(PixelRect::full(extent),view),0.,1.],|cache|cache.storm_visible_coverage(view));
            assert_eq!(known,1.,"committed root has no matching publication witness");
            let camera=crate::test_support::coverage_hash(&(view.document_to_surface.map(f32::to_bits),[view.width_px,view.height_px]));
            coverage.observe(frame,[owner,revision,camera],covered,visible,presenter.canvas_revision().is_some() && presenter.presented_camera()==Some(view));
            pictured=coverage.first_visible.is_some();
            if !cfg!(debug_assertions) && matches!(grant,Grant::Desktop) {assert!(timings.last().unwrap()<=&CPU_MAX,"CPU max: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}, cpu={:?}",timings.last());}
            let physical=r.telemetry().resident_bytes;
            let limit=r.factory.memory_budget().snapshot().devices.iter().map(|device|device.limit_bytes).sum::<u64>();
            assert!(physical<=limit,"device memory above its limit: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}, physical={physical}, limit={limit}");
            lag.observe(r,format_args!("{profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}"));
            let _=device.poll(wgpu::PollType::Poll);
            assert!(pollster::block_on(validation.pop()).is_none(),"validation: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}");
            if matches!(live.frame_demand(),layer_render::FrameDemand::Wait|layer_render::FrameDemand::WaitUntil(_)) && !reads.gated() {let _=wakes.recv_timeout(QUIET);}
        }
        reads.release();
        assert!(pictured,"no visible first picture: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, coverage={coverage:?}");
        timings.sort();
        if !cfg!(debug_assertions) && matches!(grant,Grant::Desktop) {assert!(timings[timings.len()*99/100]<=CPU_P99,"CPU p99: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, p99={:?}, max={:?}",timings[timings.len()*99/100],timings.last());}
        let deadline=Instant::now()+Duration::from_secs(30);let mut settled=false;let mut audit=crate::frame_demand::DemandAudit::default();
        crate::test_support::count_source_preparations();
        for frame in 0..4000 {
            while wakes.try_recv().is_ok() {}
            let counts=crate::test_support::PassCounts::snapshot();
            audit.begin(live.backend());
            live.render_frame().unwrap();live.backend_mut().wait_idle().unwrap();
            audit.end(live.backend(),format_args!("{profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, settle frame={frame}"));
            let [prepared,pages]=crate::test_support::source_preparations();
            assert!(prepared<=3*pages.max(64),"settling prepares source pages again: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, settle frame={frame}, {prepared} preparations of {pages} pages");
            assert_eq!(crate::test_support::PassCounts::snapshot().delta(counts).source_input_misses,0,"accepted input miss after storm: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}");
            assert_eq!(crate::test_support::PassCounts::snapshot().delta(counts).raster_output_misses,0,"accepted output miss after storm: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}");
            let revision=live.document().revision;let owner=live.document().owner;let r=live.backend_mut();
            let view=requested_view;
            if presenter.needs_present(r,view,[0.18,0.18,0.18,1.]) {presenter.present(r,&output.1,view,[0.18,0.18,0.18,1.]).unwrap();}
            let [covered,visible,_,known]=r.scale_display.as_ref().map_or([0.,0.,0.,1.],|cache|cache.storm_visible_coverage(view));
            assert_eq!(known,1.,"settled root has no matching publication witness");
            let camera=crate::test_support::coverage_hash(&(view.document_to_surface.map(f32::to_bits),[view.width_px,view.height_px]));
            coverage.observe(FRAMES+frame,[owner,revision,camera],covered,visible,presenter.canvas_revision().is_some());
            let waiting=!live.has_pending_input() && !live.has_pending_document_edits() && !live.raster_backing_pending()
                && live.frame_demand() == layer_render::FrameDemand::Wait;
            if waiting && wakes.recv_timeout(QUIET).is_err() {
                let r=live.backend();
                assert!(r.has_presentable_frame());
                assert!(covered>0.,"idle renderer has no visible committed content: {coverage:?}");
                if let Some(cache)=&r.scale_display && let Err(cell)=cache.storm_view_settled(view) {
                    panic!("settled with a required visible cell incomplete: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}: {cell}");
                }
                settled=true;break;
            }
            assert!(Instant::now()<deadline,"settle timeout: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, frame={frame}, first_visible={:?}",coverage.first_visible);
            if !waiting {std::thread::sleep(Duration::from_millis(1));}
        }
        eprintln!("STORM profile={profile:?} fixture={fixture:?} grant={grant:?} seed={seed:x} first_visible={:?} zero={} partial={} full={} cpu_p99={:?} cpu_max={:?} eviction_lag={:?}",coverage.first_visible,coverage.zero,coverage.partial,coverage.full,timings[timings.len()*99/100],timings.last(),lag.worst);
        assert!(settled,"settle frame bound: {profile:?}/{fixture:?}/{grant:?}, seed={seed:x}, first_visible={:?}",coverage.first_visible);
    }
}

#[cfg(target_os="linux")]
fn owner_cpu()->Duration {
    let mut time=libc::timespec {tv_sec:0,tv_nsec:0};
    assert_eq!(unsafe {libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID,&mut time)},0);
    Duration::new(time.tv_sec as u64,time.tv_nsec as u32)
}
#[cfg(not(target_os="linux"))]
fn owner_cpu()->Duration {static START:std::sync::OnceLock<Instant>=std::sync::OnceLock::new();START.get_or_init(Instant::now).elapsed()}

macro_rules! fixture_tests {($profile:expr;$($name:ident:$fixture:ident),*)=>{$(
    mod $name {
        use super::*;
        #[test] fn desktop() {storm(Fixture::$fixture,Grant::Desktop,$profile);}
        #[test] fn budget_edge() {storm(Fixture::$fixture,Grant::Edge,$profile);}
        #[test] fn unified_tablet() {storm(Fixture::$fixture,Grant::Tablet,$profile);}
    }
)*};}
macro_rules! profile_tests {($name:ident:$profile:ident)=>{mod $name {use super::*;
    fixture_tests! {IoProfile::$profile;object:Object,paint_base:PaintBase,paint28:Paint28,effects:Effects,mixed:Mixed,gigapixel:Gigapixel,gigapixel_paint:GigapixelPaint,panorama:Panorama,multi_gigapixel:MultiGigapixel}
}};}
profile_tests! {adversarial:Adversarial}
profile_tests! {s7_virtual_3ms:S7Virtual3ms}
profile_tests! {huion_virtual_5ms:HuionVirtual5ms}

#[test]
fn device_io_profiles_use_phase_weighted_latency() {
    for (profile,latency) in [(IoProfile::S7Virtual3ms,3_000_000),(IoProfile::HuionVirtual5ms,5_000_000)] {
        let boundary=profile.frame_ns()-latency;
        assert_eq!(profile.delay(boundary-1),0);assert_eq!(profile.delay(boundary),1);
        assert_eq!(profile.delay(0),0);assert_eq!(profile.delay(profile.frame_ns()-1),1);
    }
    for phase in 0..64 {assert!((1..=7).contains(&IoProfile::Adversarial.delay(mix(phase))));}
}

#[test]
fn recreation_with_pen_down_commits_the_stroke_and_presents_immediately() {
    let mut results=Vec::new();
    for replace in [false,true] {
        let extent=[128;2];let mut artwork=Artwork::new(extent).unwrap();
        let (owner,target)=crate::test_support::add_paint(&mut artwork,"Live stroke",extent);
        let mut document=Document::from_artwork(artwork).unwrap();
        document.working.occurrence=Some(owner);document.working.target=Some(target);
        let color=document.composition().color;
        let renderer=Grant::Desktop.renderer(color);
        let view=ViewState {width_px:128,height_px:128,document_to_surface:[1.,0.,0.,1.,0.,0.]};
        let (mut input,mut live)=engine(document,renderer,view,ViewTransform::IDENTITY);
        crate::test_support::flush_engine(&mut live);
        let mut presenter=crate::test_support::float_presenter(live.backend());
        let output=create_color_target(&live.backend().device,[128;2],"mid-stroke recreation");
        for (sequence,phase,x) in [(1,PenPhase::Down,16.),(2,PenPhase::Move,32.),(3,PenPhase::Move,64.)] {
            input.push(crate::test_support::pen(sequence,phase,[x,64.],SampleFlags::PRIMARY)).unwrap();
            crate::test_support::flush_engine(&mut live);
            crate::test_support::complete(live.backend());
        }
        assert!(live.has_active_stroke());assert_eq!(live.metrics().committed_strokes,0);
        presenter.present(live.backend_mut(),&output.1,view,[0.;4]).unwrap();
        if replace {
            let factory=live.backend().factory.clone();
            let mut renderer=WgpuRasterizer::from_factory_inner(factory.clone(),factory.pipeline_device(color),Initialization::Headless).unwrap();
            renderer.initialize_native(color).unwrap();
            drop(live.replace_backend(*renderer).unwrap());
            assert!(live.has_active_stroke(),"replacement must retain the real contact");
            live.render_frame().unwrap();
            assert!(presenter.needs_present(live.backend(),view,[0.;4]),"incoming renderer must request its first presentation");
            presenter.present(live.backend_mut(),&output.1,view,[0.;4]).unwrap();
            assert_eq!(presenter.presented_camera(),Some(view));
            assert!(presenter.canvas_revision().is_some(),"incoming renderer must present artwork immediately");
        }
        for (sequence,phase,x) in [(4,PenPhase::Move,96.),(5,PenPhase::Up,112.)] {
            input.push(crate::test_support::pen(sequence,phase,[x,64.],SampleFlags::PRIMARY)).unwrap();
            crate::test_support::flush_engine(&mut live);
        }
        assert_eq!(live.metrics().committed_strokes,1,"one real contact must commit once");
        assert!(!live.has_active_stroke());
        let data=live.document().target_raster(target).unwrap().wait_data().unwrap();
        assert!(!data.tiles.is_empty(),"the stroke must have durable authored pixels");
        results.push(data.tiles.iter().map(|(key,tile)|(*key,tile.wait_backing().unwrap().decode_on_worker().unwrap())).collect::<BTreeMap<_,_>>());
    }
    assert_eq!(results[0],results[1],"renderer recreation must not truncate or change the accepted stroke");
}

#[test]
fn recreated_tablet_renderer_under_real_pressure_keeps_frames_resumable() {
    let seed=SEEDS[0];let document=document(Fixture::Paint28,seed);let extent=document.composition().size;
    let reads=Reads::new(seed,IoProfile::S7Virtual3ms);
    let retained=layer_core::Editor::new(document.clone()).retained_tiles();
    let _store=crate::test_support::StoredTiles::with_reads(&retained,crate::test_support::tile_read_cache(),Some(reads.observe()));
    let (initial,transform,_)=camera(seed,0,extent);
    let renderer=Grant::Tablet.renderer(document.composition().color);
    let (_input,mut live)=engine(document,renderer,initial,transform);
    let mut presenter=crate::ViewportPresenter::for_surface(live.backend(),wgpu::TextureFormat::Rgba8UnormSrgb,crate::SdrSurfaceColor::Srgb).unwrap();
    presenter.set_overviews(live.backend(),&[crate::OverviewPlacement {bounds:[3500.,24.,312.,192.],clip:None,
        work_area:[[0.;2];4],outline_linear:[1.;3],scale:1.,opacity:1.}]);
    let mut output=create_target(&live.backend().device,[initial.width_px,initial.height_px],wgpu::TextureFormat::Rgba8UnormSrgb,"cold recreation presentation");
    let mut pressure=None;
    for frame in 0..=FRAMES/2 {
        reads.advance(frame);
        let (view,mut transform,_)=camera(seed,frame,extent);transform.revision=frame as u64+1;
        if frame==FRAMES/2 {
            let factory=live.backend().factory.clone();let color=live.document().composition().color;
            let mut renderer=WgpuRasterizer::from_factory_inner(factory.clone(),factory.pipeline_device(color),Initialization::Headless).unwrap();
            renderer.initialize_native(color).unwrap();
            drop(live.replace_backend(*renderer).unwrap());
            let _=live.backend().device.poll(wgpu::PollType::Poll);
            let budget=live.backend().factory.memory_budget();budget.flush_reports();
            let bytes=budget.import_headroom_bytes();
            assert!(bytes>0,"replacement must leave positive real headroom before the external import");
            pressure=Some(budget.admit_import(bytes).expect("the ordinary import reservation must be admitted"));
            assert!(live.backend().memory.display_owner().admit_working(layer_core::MemoryGrant {ram_bytes:0,gpu_bytes:1}).is_none(),
                "the held import must cause a real unified working-admission refusal");
        }
        live.set_view(view,transform);
        if output.0.width()!=view.width_px || output.0.height()!=view.height_px {
            output=create_target(&live.backend().device,[view.width_px,view.height_px],wgpu::TextureFormat::Rgba8UnormSrgb,"resized cold recreation presentation");
            presenter.set_overviews(live.backend(),&[crate::OverviewPlacement {bounds:[view.width_px as f32-336.,24.,312.,192.],clip:None,
                work_area:[[0.;2];4],outline_linear:[1.;3],scale:1.,opacity:1.}]);
        }
        let device=live.backend().device.clone();let validation=device.push_error_scope(wgpu::ErrorFilter::Validation);
        let result=live.render_frame();
        assert!(result.is_ok(),"cold frame must remain resumable through renderer replacement: frame={frame}, result={result:?}");
        if presenter.needs_present(live.backend(),view,[0.18,0.18,0.18,1.]) {
            presenter.present(live.backend_mut(),&output.1,view,[0.18,0.18,0.18,1.]).unwrap();
        }
        let _=device.poll(wgpu::PollType::Poll);
        assert!(pollster::block_on(validation.pop()).is_none(),"cold replacement validation: frame={frame}");
    }
    assert!(live.backend().navigator.pending(),"a refused Navigator allocation must remain resumable");
    drop(pressure.take());reads.release();
    let mut published=false;
    for _ in 0..256 {
        live.render_frame().unwrap();live.backend_mut().wait_idle().unwrap();
        if live.backend().navigator.view().is_some() {published=true;break;}
    }
    assert!(published,"releasing real import pressure must let the Navigator publish");
}

#[test]
fn an_effects_storm_holds_composition_scratch_to_one_unit() {
    let seed=SEEDS[0];let document=document(Fixture::Effects,seed);let extent=document.composition().size;
    let reads=Reads::new(seed,IoProfile::Adversarial);
    let retained=layer_core::Editor::new(document.clone()).retained_tiles();
    let _store=crate::test_support::StoredTiles::with_reads(&retained,crate::test_support::tile_read_cache(),Some(reads.observe()));
    let (initial,transform,_)=camera(seed,0,extent);
    let renderer=Grant::Desktop.renderer(document.composition().color);
    let (_input,mut live)=engine(document,renderer,initial,transform);
    let mut presenter=crate::ViewportPresenter::for_surface(live.backend(),wgpu::TextureFormat::Rgba8UnormSrgb,crate::SdrSurfaceColor::Srgb).unwrap();
    let mut output=create_target(&live.backend().device,[initial.width_px,initial.height_px],wgpu::TextureFormat::Rgba8UnormSrgb,"effects storm presentation");
    for frame in 0..96 {
        reads.advance(frame);
        let (view,mut transform,_)=camera(seed,frame,extent);transform.revision=frame as u64+1;
        live.set_view(view,transform);
        if output.0.width()!=view.width_px || output.0.height()!=view.height_px {
            output=create_target(&live.backend().device,[view.width_px,view.height_px],wgpu::TextureFormat::Rgba8UnormSrgb,"resized effects storm presentation");
        }
        live.render_frame().unwrap();
        let r=live.backend_mut();
        if presenter.needs_present(r,view,[0.18,0.18,0.18,1.]) {presenter.present(r,&output.1,view,[0.18,0.18,0.18,1.]).unwrap();}
        let scratch=r.scene.as_ref().map_or(0,|scene|scene.pool_bytes())+r.scale_display.as_ref().map_or(0,|cache|cache.charged_scratch_bytes());
        assert!(scratch<=r.memory.unit(),"composition scratch above one unit's working term: frame={frame}, scratch={scratch}, unit={}",r.memory.unit());
    }
}
