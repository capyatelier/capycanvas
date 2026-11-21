use super::*;
use crate::IdMap;

pub(super) struct Contact {
    id:u64,
    stroke:layer_core::StrokeId,
    target:SourceTarget,
    plan:display_mips::Plan,
    window:wgpu::Texture,
    root:graph::Node,
    tiled:bool,
    frame:Arc<SignatureFrame>,
    composed:crate::raster::contents::Versions,
    predicted:BTreeSet<[u32;2]>,
    previewed:BTreeSet<[u32;2]>,
    next:Option<(crate::raster::contents::Versions,BTreeSet<[u32;2]>,BTreeSet<[u32;2]>,BTreeSet<[u32;2]>)>,
    footprints:Vec<(PixelRect,Vec<usize>)>,
    tainted:crate::IdSet<usize>,
    guard:Guard,
    generation:std::sync::Mutex<[u64;3]>,
    exit:std::sync::OnceLock<Option<Arc<SignatureFrame>>>,
    preview_level:std::sync::OnceLock<u32>,
    cells:Vec<(PixelRect,crate::scene::sources::Inputs)>,
    inputs:crate::scene::sources::AcceptedInputs,
    pub(super) pages:IdMap<pages::Key,pages::Held>,
    level:Option<u32>,
    stale:BTreeMap<[u32;2],PixelRect>,
    written:IdMap<[u32;2],(pages::Key,PixelRect,submission::Submission)>,
    submissions:Vec<submission::Submission>,
    stamps:Arc<PreviewStamps>,
    reduced:BTreeSet<[u32;2]>,
    baselines:BTreeMap<[u32;2],Baseline>,
    drawn_directly:bool,
}

type Description=(crate::resident::Input,PixelRect);
enum Stroked {Tile(Vec<Description>),Level(PixelRect)}
struct Baseline {submission:submission::Submission,stroked:Stroked,reads:Vec<(usize,u64)>}
impl Baseline {
    fn changed(&self,now:&Stroked,region:PixelRect)->Option<PixelRect> {
        match (&self.stroked,now) {
            (Stroked::Tile(before),Stroked::Tile(now))=>changed_since(now,before).map(|changed|changed.translated([region.min_x(),region.min_y()])),
            (Stroked::Level(rewritten),Stroked::Level(_))=>Some(*rewritten),
            _=>None,
        }
    }
}
#[derive(Clone,Copy)]
enum Patch {Tile,Level}
impl Patch {
    fn of(contact:&Contact,tiled:bool,frame:SourceFrame)->Option<Self> {
        if !tiled || contact.guard.watercolor.is_some() || !contact.drawn_directly {return None;}
        match contact.level {
            None=>(contact.plan.level==0).then_some(Self::Tile),
            Some(level)=>(level==contact.plan.level && frame.origin()==[0;2]).then_some(Self::Level),
        }
    }
    fn stroked(self,unit:&Unit,jobs:&[crate::scene::Job],target:SourceTarget,cell:[u32;2],texels:[u32;2])->Option<Stroked> {
        if !unit.pages.is_empty() || !whole_cell_composes(jobs,texels) {return None;}
        match self {
            Self::Tile=>unit.sources.sole_color(target,cell).filter(|witness|witness.live()).map(|witness|Stroked::Tile(descriptions(witness))),
            Self::Level=>Some(Stroked::Level(PixelRect::EMPTY)),
        }
    }
}
fn drawn_directly(node:&graph::Node,tainted:&crate::IdSet<usize>)->bool {
    if !tainted.contains(&(Arc::as_ptr(node) as *const () as usize)) {return true;}
    match node.as_ref() {
        graph::Expression::Source {..}=>true,
        graph::Expression::Opacity {input,..}=>drawn_directly(input,tainted),
        graph::Expression::Combine {front,back,flags,..}=>flags&64==0 && drawn_directly(front,tainted) && drawn_directly(back,tainted),
        _=>false,
    }
}
fn descriptions(witness:&crate::resident::Input)->Vec<Description> {
    let mut known=vec![(witness.clone(),PixelRect::EMPTY)];
    while known.len()<3 && let Some((page,outside))=known.last() && let Some((from,rect))=page.seed() {known.push((from,outside.union(rect)));}
    known
}
fn changed_since(now:&[Description],before:&[Description])->Option<PixelRect> {
    now.iter().find_map(|(page,rect)|before.iter().find_map(|(earlier,outside)|Some(page.changed_since(earlier)?.union(*rect).union(*outside))))
}
fn whole_cell_composes(jobs:&[crate::scene::Job],texels:[u32;2])->bool {
    jobs.iter().all(|job|match job {
        crate::scene::Job::Compose {values,..}=>[values[2],values[3]]==texels,
        crate::scene::Job::DecodedTile(_)=>true,
        _=>false,
    })
}
fn crop(jobs:&mut [crate::scene::Job],[x,y,width,height]:[u32;4]) {
    for job in jobs {
        if let crate::scene::Job::Compose {values,..}=job {values[0]+=x;values[1]+=y;values[2]=width;values[3]=height;}
    }
}

#[derive(Default)]
struct Declared {cells:Vec<(PixelRect,crate::scene::sources::Inputs)>,inputs:crate::scene::sources::AcceptedInputs,pages:IdMap<pages::Key,pages::Held>,level:Option<u32>}

#[derive(PartialEq)]
struct Guard {inspect_mask:Option<OccurrenceHandle>,clipping:[bool;2],blend_space:layer_core::BlendSpace,watercolor:Option<WatercolorLayerStyle>}
impl Guard {
    fn capture(r:&WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,level:&std::sync::OnceLock<u32>)->Option<Self> {
        if let Some(preview)=&r.preview_declaration && (preview.target()!=target || preview.contact_outputs().is_none() || *level.get_or_init(||preview.level())!=preview.level()) {return None;}
        (r.transform_preview.is_none() && r.moving_layer.is_none() && r.vector_preview.is_none() && r.vector_motion.is_none() && r.fill_preview.is_none())
            .then(||Self {inspect_mask:packet.inspect_mask,clipping:r.clipping_preview,blend_space:packet.blend_space,watercolor:r.watercolor_style(target,packet.dab_batches)})
    }
}
fn reads(node:&graph::Node,target:SourceTarget,tainted:&mut crate::IdSet<usize>)->bool {
    let read=match node.as_ref() {
        graph::Expression::Source {id,..}=>*id==target,
        graph::Expression::Opacity {input,..}=>reads(input,target,tainted),
        graph::Expression::Combine {front,back,..}=>reads(front,target,tainted) | reads(back,target,tainted),
        graph::Expression::Effect {input,masks,..}=>masks.iter().flatten().fold(reads(input,target,tainted),|read,mask|reads(mask,target,tainted) | read),
        _=>false,
    };
    if read {tainted.insert(Arc::as_ptr(node) as *const () as usize);}
    read
}

fn source_phase(scene:&Scene,target:SourceTarget)->[u32;2] {scene.scale_sources.entries.get(&target).map_or([0;2],|source|source.phase)}
fn phased_tiles(tile:[u32;2],phase:[u32;2])->impl Iterator<Item=[u32;2]> {page_coordinates(page_rect(tile).translated(phase))}
fn local_reach(packet:FramePacket<'_>,target:SourceTarget,phase:[u32;2])->Vec<PixelRect> {
    let extent=packet.scene.target_extent(target);
    packet.dab_batches.iter().map(|batch|crate::pixel_rect::pixel_rect(batch.damage,extent)).filter(|reach|!reach.is_empty()).map(|reach|reach.translated(phase)).collect()
}
fn stroke(packet:FramePacket<'_>)->Option<(layer_core::StrokeId,SourceTarget)> {
    let first=packet.dab_batches.first()?;
    packet.dab_batches.iter().all(|batch|batch.stroke_id==first.stroke_id && batch.target==first.target
        && matches!(batch.kind,DabBatchKind::Preview|DabBatchKind::Persistent)).then_some((first.stroke_id,first.target))
}

impl Contact {
    fn reduced_tiles(&self,phase:[u32;2],reach:impl Iterator<Item=[u32;2]>)->BTreeSet<[u32;2]> {
        let changed=self.next.iter().flat_map(|(_,changed,..)|changed.iter().flat_map(|tile|phased_tiles(*tile,phase)));
        let tiles=reach.chain(changed).chain(self.stale.keys().copied());
        if self.frame.watercolor(self.target) {tiles.flat_map(crate::scene::watercolor_stencil).flatten().collect()}else {tiles.collect()}
    }
}

impl Cache {
    fn tiled(&self,r:&WgpuRasterizer,packet:FramePacket<'_>,scene:&Scene,root:&graph::Node)->bool {
        use_tiles(r,self.plan,packet,Some(scene)) && !(self.plan.level>0 && root.fused_transform(r))
    }
    pub(super) fn contact_id(&self)->Option<u64> {self.gesture.as_ref().map(|contact|contact.id)}
    pub(super) fn stroke_scope(&self,packet:FramePacket<'_>,touched:&[PixelRect],from:Option<[f32;2]>)->Vec<PixelRect> {
        let (level,extent,bounds)=(self.plan.level,self.plan.extent,self.plan.bounds);let span=PAGE_SIZE<<level;
        let near=|reach:PixelRect|PixelRect::new(reach.min_x().saturating_sub(span),reach.min_y().saturating_sub(span),
            reach.max_x().saturating_add(span),reach.max_y().saturating_add(span)).intersect(bounds);
        let swept=|reach:PixelRect|{
            let centre=[(reach.min_x()+reach.max_x()) as f32*0.5,(reach.min_y()+reach.max_y()) as f32*0.5];
            let [dx,dy]=from.map_or([0.;2],|[x,y]|[x-centre[0],y-centre[1]]);
            let steps=(dx.abs().max(dy.abs())/span as f32).ceil() as u32;
            (0..=steps).map(move|step|{let t=step as f32/steps.max(1) as f32;let [x,y]=[(dx*t).round() as i64,(dy*t).round() as i64];
                let shift=|value:u32,by:i64|(i64::from(value)+by).clamp(0,i64::from(u32::MAX)) as u32;
                PixelRect::new(shift(reach.min_x(),x),shift(reach.min_y(),y),shift(reach.max_x(),x),shift(reach.max_y(),y))})
        };
        let coordinates:BTreeSet<[u32;2]>=dab_reach(packet,extent).flat_map(swept).map(near).chain(touched.iter().map(|cell|cell.intersect(bounds)))
            .flat_map(|reach|pages::coordinates(reach,level)).collect();
        coordinates.into_iter().map(|coordinate|pages::bounds(coordinate,level,extent)).collect()
    }
    pub(super) fn dab_cells(&self,packet:FramePacket<'_>)->Vec<PixelRect> {
        if stroke(packet).is_none() {return Vec::new();}
        let (level,extent,bounds)=(self.plan.level,self.plan.extent,self.plan.bounds);
        dab_reach(packet,extent).flat_map(|reach|pages::coordinates(reach.intersect(bounds),level)).collect::<BTreeSet<_>>()
            .into_iter().map(|coordinate|pages::bounds(coordinate,level,extent)).collect()
    }
    pub(super) fn contact_composes(&self)->Vec<PixelRect> {
        let Some(contact)=self.gesture.as_ref() else {return Vec::new();};
        self.contact_cells().into_iter().filter(|region|contact.cells.iter().any(|(cell,_)|cell==region)).collect()
    }
    pub(super) fn contact_reads(&self,node:&graph::Node)->bool {
        self.gesture.as_ref().is_some_and(|contact|contact.tainted.contains(&(Arc::as_ptr(node) as *const () as usize)))
    }
    pub(in crate::scene) fn holds_contact_frame(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
        let Some(contact)=self.gesture.as_ref().filter(|contact|contact.exit.get().is_none()) else {return false;};
        if self.ends_contact(packet) || !self.contact_holds(r,packet) || !contact.frame.holds_scene(packet.scene)
            || Guard::capture(r,packet,contact.target,&contact.preview_level).as_ref()!=Some(&contact.guard) {return false;}
        let Some(now)=r.raster_generation(contact.target) else {return false;};
        let mut base=contact.generation.lock().unwrap();
        let delta=|index:usize|now[index].checked_sub(base[index]);
        if delta(2).is_none() || delta(0)!=delta(2) || delta(1)!=delta(2) {return false;}
        *base=now;true
    }
    pub(super) fn end_held_frame(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->Result<(),GpuRasterError> {
        if self.gesture.as_ref().is_none_or(|contact|contact.exit.get().is_some()) {return Ok(());}
        self.exit_contact();
        self.pages.lock().unwrap().set_expected(SignatureFrame::prepare(r,packet)?);
        Ok(())
    }
    pub(in crate::scene) fn exit_contact(&self) {
        let Some(contact)=self.gesture.as_ref().filter(|contact|contact.exit.get().is_none()) else {return;};
        let kept=!contact.submissions.iter().chain(contact.written.values().map(|(_,_,submission)|submission)).any(submission::Submission::discarded);
        let frame=kept.then(||Arc::new(contact.frame.composed(contact.target,contact.composed.clone())));
        if let Some(frame)=&frame {
            self.pages.lock().unwrap().set_expected(frame.clone());
            self.key_contact_cells(frame,&contact.predicted);
        }
        drop(contact.exit.set(frame));
    }
    pub(crate) fn contact_stamps(&self,packet:FramePacket<'_>)->Option<Arc<PreviewStamps>> {
        let contact=self.gesture.as_ref()?;
        (!packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty() && packet.dab_batches.iter().all(|batch|batch.stroke_id==contact.stroke
            && batch.target==contact.target && matches!(batch.kind,DabBatchKind::Preview|DabBatchKind::Persistent))).then(||contact.stamps.clone())
    }
    pub(in crate::scene) fn contact_holds(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
        let Some(contact)=self.gesture.as_ref() else {return false;};
        let tier=self.pages.lock().unwrap().tier();
        packet.dab_batches.iter().all(|batch|batch.stroke_id==contact.stroke && batch.target==contact.target && matches!(batch.kind,DabBatchKind::Preview|DabBatchKind::Persistent))
            && !packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty() && r.transform_preview.is_none()
            && self.plan==contact.plan && self.pixels.root().map(|image|&image.texture)==Some(&contact.window) && tier.used()<=tier.limit()
            && !contact.submissions.iter().any(submission::Submission::discarded)
    }
    pub(super) fn ends_contact(&self,packet:FramePacket<'_>)->bool {
        self.gesture.as_ref().is_some_and(|contact|packet.dab_batches.iter().any(|batch|batch.stroke_end && batch.stroke_id==contact.stroke))
    }
    pub(super) fn stroke_only(&mut self,packet:FramePacket<'_>)->bool {
        let frame=self.pages.lock().unwrap().expected().cloned();
        let previous=std::mem::replace(&mut self.stroked,frame.clone());
        self.gesture.is_some() || stroke(packet).zip(previous.zip(frame)).is_some_and(|((_,target),(previous,frame))|frame.contact_continues(&previous,target).is_some())
    }
    pub(super) fn edit_halves(&self,packet:FramePacket<'_>,budget:&WorkBudget)->Vec<pages::Key> {
        if !packet.dab_batches.is_empty() || budget.motion || !self.pivot_edited() {Vec::new()}else {self.wanted_halves(self.plan.bounds,packet.view)}
    }
    pub(super) fn prepare_edit_sources(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,budget:&mut WorkBudget)
        ->Result<(),GpuRasterError> {
        let (Some([x,y]),Some(root))=(self.pointer,self.graph.root.clone()) else {return Ok(());};
        let span=(PAGE_SIZE<<self.plan.level) as f32;
        let near=PixelRect::new((x-span).max(0.) as u32,(y-span).max(0.) as u32,(x+span).max(0.) as u32+1,(y+span).max(0.) as u32+1).intersect(self.plan.bounds);
        let tiled=self.tiled(r,packet,scene,&root);
        let inside:BTreeSet<SourceTarget>=self.pivot_halves().iter().flat_map(|node|node.reads().0).collect();
        let mut prepared=false;
        for coordinate in pages::coordinates(near,self.plan.level) {
            let cell=pages::bounds(coordinate,self.plan.level,self.plan.extent);
            if self.pivot_missing(cell) {continue;}
            for (id,request) in self.region_sources(scene,r,packet,cell,tiled) {
                if inside.contains(&id) || self.source_ready(scene,r,id,&request) {continue;}
                if prepared && budget.remaining()<object_cache::PASS_TAPS {return Ok(());}
                self.prepare_source(scene,r,packet,encoder,id,request,Class::Refinement,budget)?;
                prepared=true;
            }
        }
        Ok(())
    }
    #[expect(clippy::too_many_arguments,reason="A contact begins from the frame's composed cells, encoding and budget")]
    pub(super) fn begin_contact(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,retained:u64,budget:&mut WorkBudget,
        touched:&[PixelRect],
    )->Result<(),GpuRasterError> {
        let Some((stroke,target))=stroke(packet).filter(|(_,target)|!target.is_coverage() && !packet.dab_batches.iter().any(|batch|batch.stroke_end)) else {return Ok(());};
        let Some(root)=self.graph.root.clone() else {return Ok(());};
        let tier=self.pages.lock().unwrap().tier();
        let scope=self.stroke_scope(packet,touched,None);
        if scope.is_empty() || tier.used()>tier.limit() || r.transform_preview.is_some() || r.moving_layer.is_some() || refinement::exact_at_view_level(packet.scene)
            || root.native_capture(packet.scene,self.plan.level) || !self.contact_ready(&scope)
            || packet.scene.order().iter().any(|handle|packet.scene.effect(*handle).is_some_and(|effect|effect.animated())) {return Ok(());}
        let (Some(window),Some(frame))=(self.pixels.root().map(|image|image.texture.clone()),self.pages.lock().unwrap().expected().cloned()) else {return Ok(());};
        let preview_level=std::sync::OnceLock::new();
        let (Some(guard),Some(composed),Some(generation))=(Guard::capture(r,packet,target,&preview_level),frame.target_versions(target),r.raster_generation(target)) else {return Ok(());};
        let tiled=self.tiled(r,packet,scene,&root);
        let plan=self.plan;
        let required=self.dab_cells(packet).into_iter().chain(touched.iter().filter(|cell|!cell.intersect(plan.bounds).is_empty()).copied()).collect::<Vec<_>>();
        let ready=scope.into_iter().filter(|cell|!self.pivot_missing(*cell)).collect::<Vec<_>>();
        if required.iter().any(|cell|!ready.contains(cell)) {return Ok(());}
        let reach=local_reach(packet,target,source_phase(scene,target));
        let reduced=reach.iter().copied().flat_map(page_coordinates).collect();
        let Some(Declared {cells,inputs,pages,level})=self.declare_contact(scene,r,packet,encoder,retained,budget,&frame,&root,target,tiled,None,ready,&required,reach)? else {return Ok(());};
        if required.iter().any(|region|!cells.iter().any(|(cell,_)|cell==region)) {return Ok(());}
        let footprints=frame.reader_regions(target,self.plan.level).into_iter().map(|(local,regions)|(local,regions.into_iter().flat_map(|region|self.reached_cells(region)).collect())).collect();
        let mut tainted=crate::IdSet::default();reads(&root,target,&mut tainted);
        let drawn_directly=drawn_directly(&root,&tainted);
        let predicted=frame.preview_for(target).and_then(|preview|preview.contact_outputs().map(Iterator::collect)).unwrap_or_default();
        static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
        self.pixels.hold_current(&frame);
        let previewed=r.preview_input_tiles(target);let stale=previewed.iter().map(|tile|(*tile,page_rect(*tile))).collect();
        self.gesture=Some(Contact {id:NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed),stroke,target,plan:self.plan,window,root,tiled,frame,composed,predicted,
            previewed,next:None,
            footprints,tainted,guard,generation:std::sync::Mutex::new(generation),exit:std::sync::OnceLock::new(),preview_level,cells,inputs,pages,level,
            stale,written:IdMap::default(),submissions:Vec::new(),stamps:Arc::default(),reduced,baselines:BTreeMap::new(),drawn_directly});
        Ok(())
    }
    #[expect(clippy::too_many_arguments,reason="A contact grows with the frame's scene, encoding and budget into the cells its stroke reaches")]
    pub(super) fn grow_contact(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,retained:u64,budget:&mut WorkBudget,
        reached:&[PixelRect],from:Option<[f32;2]>,
    )->Result<bool,GpuRasterError> {
        let Some(contact)=self.gesture.as_ref() else {return Ok(false);};
        let (root,target,tiled,level)=(contact.root.clone(),contact.target,contact.tiled,contact.level);
        let Some(current)=contact.next.as_ref().map(|(current,..)|current.clone()) else {return Ok(true);};
        let frame=Arc::new(contact.frame.advanced(target,current,r.preview_declaration.clone()));
        let known=contact.cells.iter().map(|(cell,_)|*cell).collect::<Vec<_>>();
        let phase=source_phase(scene,target);
        let tiles=contact.reduced_tiles(phase,local_reach(packet,target,phase).into_iter().flat_map(page_coordinates))
            .into_iter().filter(|tile|!contact.reduced.contains(tile)).collect::<Vec<_>>();
        let reach=tiles.iter().copied().map(page_rect).collect::<Vec<_>>();
        let scope=self.stroke_scope(packet,reached,from);
        let view=work_view(packet,0);
        let mut waiting=self.contact_cells().into_iter().filter(|cell|!known.contains(cell) && !scope.contains(cell) && self.keyless(*cell)).collect::<Vec<_>>();
        waiting.sort_by_key(|cell|centre_distance(view,*cell));
        let mut cells=scope.into_iter().filter(|cell|!known.contains(cell)).collect::<Vec<_>>();
        cells.extend(waiting.into_iter().take(1));
        cells.retain(|cell|!self.pivot_missing(*cell));
        let declared=if cells.is_empty() && reach.is_empty() {Some(Declared::default())}
            else {self.declare_contact(scene,r,packet,encoder,retained,budget,&frame,&root,target,tiled,level,cells.clone(),&cells,reach)?};
        let Some(Declared {cells,inputs,pages,level})=declared else {return Ok(false);};
        let contact=self.gesture.as_mut().expect("live contact");
        contact.cells.extend(cells);
        if level.is_some() {contact.level=level;contact.reduced.extend(tiles);}
        contact.inputs.adopt(inputs)?;
        for (key,page) in pages {contact.pages.entry(key).or_insert(page);}
        Ok(reached.iter().all(|cell|contact.cells.iter().any(|(declared,_)|declared==cell)))
    }
    pub(super) fn lane_halves(&self,packet:FramePacket<'_>)->Vec<pages::Key> {
        self.dab_cells(packet).into_iter().filter(|cell|self.pivot_missing(*cell)).flat_map(|cell|self.wanted_halves(cell,packet.view)).collect()
    }
    #[expect(clippy::too_many_arguments,reason="A contact declares its cells with the frame's scene, encoding and budget, around the pages it already holds")]
    fn declare_contact(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,retained:u64,budget:&mut WorkBudget,
        frame:&Arc<SignatureFrame>,root:&graph::Node,target:SourceTarget,tiled:bool,level:Option<u32>,cells:Vec<PixelRect>,prepared:&[PixelRect],reach:Vec<PixelRect>,
    )->Result<Option<Declared>,GpuRasterError> {
        let wet=frame.watercolor(target);
        let inside:BTreeSet<SourceTarget>=self.pivot_halves().iter().flat_map(|node|node.reads().0).collect();
        let held:std::collections::HashSet<pages::Key>=self.gesture.iter().flat_map(|contact|contact.pages.keys().cloned()).collect();
        let (mut ready,mut requests)=(Vec::new(),Vec::<(SourceTarget,SourceRequest)>::new());
        for cell in cells {
            let mut wanted=self.region_sources(scene,r,packet,cell,tiled);
            wanted.retain(|(id,_)|!inside.contains(id));
            for (id,request) in &mut wanted {
                let (source,level)=(&scene.scale_sources.entries[id],request.plan.level);
                let mut unheld=Vec::new();
                for area in &request.required.regions {for coordinate in pages::coordinates(*area,level) {
                    let key=source.key(r,*id,level,coordinate);
                    if !held.contains(&key) {unheld.push(key.plan().bounds.intersect(*area));}
                }}
                request.required=Damage::from_regions(unheld);
            }
            if prepared.contains(&cell) {
                for (id,request) in wanted.iter().filter(|(id,request)|!self.source_ready(scene,r,*id,request)).cloned().collect::<Vec<_>>() {
                    self.prepare_source(scene,r,packet,encoder,id,request,Class::Visible,budget)?;
                }
            }
            if wanted.iter().all(|(id,request)|self.source_ready(scene,r,*id,request)) {ready.push(cell);requests.extend(wanted);}
        }
        let bounds=ready.iter().fold(PixelRect::EMPTY,|bounds,cell|bounds.union(*cell)).intersect(self.plan.bounds);
        let mut areas=IdMap::<pages::Key,(PixelRect,u32)>::default();
        for (id,request) in &requests {
            let source=&scene.scale_sources.entries[id];
            for area in &request.required.regions {for coordinate in pages::coordinates(*area,request.plan.level) {
                let key=source.key(r,*id,request.plan.level,coordinate);
                let region=key.plan().bounds.intersect(*area).intersect(PixelRect::full(key.extent));
                if region.is_empty() {continue;}
                let held=areas.entry(key).or_insert((region,request.plan.level));held.0=held.0.union(region);
            }}
        }
        let mut pages=IdMap::default();
        for (key,_,page) in self.held_halves(bounds) {pages.insert(key,page);}
        {
            let mut store=self.pages.lock().unwrap();
            for (key,(region,level)) in areas {
                let Some(page)=store.get(&key,region,pages::Accuracy::Display,level,Class::Visible) else {return Ok(None);};
                pages.insert(key,page);
            }
        }
        if let Some(contact)=self.gesture.as_mut() {for (key,page) in &pages {contact.pages.entry(key.clone()).or_insert_with(||page.clone());}}
        let input=input_plan(self.plan,packet.scene,Some(self.graph.objects()));
        let mut cells=Vec::new();
        for cell in ready {
            let unit=unit_plan(input,packet.scene,cell,tiled,Some(self.graph.objects()));
            let declared=self.declare_unit(scene,r,packet,frame.clone(),cell,unit,tiled,false,retained,Class::Visible,encoder,|compositor| {
                compositor.evaluate_root(root,Root::Store)?;
                if wet {compositor.accepted.declare_watercolor_stencils(compositor.r,compositor.packet.scene,target)?;}
                Ok(())
            });
            let Some(declared)=declared.ok().filter(|inputs|inputs.objects().is_empty()) else {continue;};
            cells.push((cell,declared));
        }
        let level=level.or_else(||requests.iter().find(|(id,_)|*id==target).map(|(_,request)|request.plan.level));
        let request=level.filter(|_|!reach.is_empty()).map(|level|SourceRequest {plan:display_mips::Plan::at(self.plan.extent,level),required:Damage::from_regions(reach),covered:PixelRect::EMPTY});
        let reductions=match &request {Some(request)=>self.declare_reductions(scene,r,packet,frame,target,request,encoder),None=>Ok(Default::default())};
        let Some(inputs)=reductions.ok().and_then(|inputs|inputs.accept_query(r,Some(scene)).ok()) else {return Ok(None);};
        self.touch_cell_keys(bounds);
        Ok(Some(Declared {cells,inputs,pages,level}))
    }
    fn declare_reductions(&self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,frame:&Arc<SignatureFrame>,target:SourceTarget,request:&SourceRequest,
        encoder:&mut submission::CommandEncoder,
    )->Result<crate::scene::sources::Inputs,GpuRasterError> {
        let level=request.plan.level;
        let handle=packet.scene.source_owner(target).ok_or(GpuRasterError::NotReady)?;
        let source=&scene.scale_sources.entries[&target];let valid=PixelRect::full(source.valid_extent(level));
        let pages=request.required.regions.iter().flat_map(|area|pages::coordinates(*area,level))
            .map(|coordinate|(source.key(r,target,level,coordinate).plan(),coordinate)).collect::<Vec<_>>();
        let tiles=request.required.regions.iter().flat_map(|area|page_coordinates(area.intersect(valid))).collect::<BTreeSet<_>>();
        let wet=frame.watercolor(target);
        let placements=scene.source_placements(packet,scene::Output::Display)?;
        scene.declare(r,placements,crate::scene::sources::Declaration::scene(frame.clone(),Some(encoder.submission())),|scene,r,accepted| {
            let output=scene.reserve(r)?;let output=scene.pool[output].view.clone();
            for tile in &tiles {
                let plan=pages.iter().find(|(plan,_)|!plan.bounds.intersect(page_rect(*tile)).is_empty()).map(|(plan,_)|*plan).ok_or(GpuRasterError::NotReady)?;
                scene.reduce_color_pages(r,packet,handle,plan,&output,&[*tile],None,[0;2],&BTreeMap::new(),accepted)?;
            }
            if wet {accepted.declare_watercolor_stencils(r,packet.scene,target)?;}
            Ok(())
        })
    }
    pub(super) fn contact_continues(&mut self,scene:&Scene,r:&WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder)->Option<Vec<PixelRect>> {
        let contact=self.gesture.as_mut()?;
        contact.submissions.retain(|submission|submission.pending() || submission.discarded());
        if contact.exit.get().is_some() || !self.contact_holds(r,packet) || self.graph.root.as_ref()!=self.gesture.as_ref().map(|contact|&contact.root) {return None;}
        let contact=self.gesture.as_mut()?;
        let target=contact.target;let wet=contact.frame.watercolor(target);
        let current=r.current_raster_versions(target).ok()?;
        let mut changed=BTreeSet::new();
        if !contact.composed.changes(&current,|coordinate|{changed.insert(coordinate);}) {return None;}
        let predicted:BTreeSet<_>=match &r.preview_declaration {
            Some(preview) if preview.target()==target=>preview.contact_outputs()?.collect(),
            Some(_)=>return None,
            None=>BTreeSet::new(),
        };
        let persistent=changed.clone();
        changed.extend(contact.predicted.iter().chain(&predicted).copied());
        let previewed=r.preview_input_tiles(target);
        let stale=contact.footprints.iter().filter(|(local,_)|changed.iter().any(|tile|!page_rect(*tile).intersect(*local).is_empty()))
            .flat_map(|(_,cells)|cells.iter().copied()).collect::<Vec<_>>();
        let all=pages::coordinates(contact.plan.bounds,contact.plan.level).collect::<Vec<_>>();
        let reached=stale.iter().map(|index|pages::bounds(all[*index],contact.plan.level,contact.plan.extent)).collect();
        let refreshed:BTreeSet<_>=changed.iter().chain(&contact.previewed).chain(&previewed).copied().collect();
        let planes:&[layer_core::raster::RasterPlane]=if wet {&[layer_core::raster::RasterPlane::Color,layer_core::raster::RasterPlane::WatercolorWetness]}else {&[layer_core::raster::RasterPlane::Color]};
        if !contact.inputs.refresh_contact(r,packet.scene,target,planes,refreshed,&persistent,encoder) {return None;}
        for region in scene.scale_sources.entries.get(&target).into_iter().flat_map(|source|&source.damage.regions) {for tile in page_coordinates(*region) {
            let rect=contact.stale.entry(tile).or_insert(PixelRect::EMPTY);*rect=rect.union(region.intersect(page_rect(tile)));
        }}
        contact.next=Some((current,changed,predicted,previewed));
        self.invalidate_cells(stale);
        Some(reached)
    }
    #[cfg(test)]
    pub(super) fn contact_decisions_differ_from_a_declaration(&self,r:&WgpuRasterizer,scene:SceneView<'_>)->Result<(usize,Vec<String>),GpuRasterError> {
        let contact=self.gesture.as_ref().ok_or(GpuRasterError::NotReady)?;
        let wet=contact.frame.watercolor(contact.target);
        let tiles:BTreeSet<_>=contact.predicted.iter().chain(&contact.previewed).copied().collect();
        let declared=crate::scene::sources::declare(Default::default(),crate::scene::sources::Declaration::current(None),|accepted| {
            for coordinate in &tiles {
                accepted.color(r,scene,contact.target,*coordinate)?;
                if wet {accepted.scalar(r,scene,contact.target,layer_core::raster::RasterPlane::WatercolorWetness,*coordinate)?;}
            }
            Ok(())
        })?;
        Ok(contact.inputs.differing(&declared))
    }
    pub(super) fn compose_contact(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder,
        composes:Vec<PixelRect>,retained:u64,
    )->Result<(Vec<PixelRect>,Vec<PixelRect>),GpuRasterError> {
        let drawn=self.unnamed_writes(&composes);
        let contact=self.gesture.as_mut().expect("live contact");
        let (root,tiled,target)=(contact.root.clone(),contact.tiled,contact.target);
        let (plan,frame)=(contact.plan,source_frame(Some(&scene.scale_sources),packet.scene,target));
        let patch=Patch::of(contact,tiled,frame);
        let (side,span)=(1u32<<plan.level,PAGE_SIZE<<plan.level);
        let reduced=contact.reduced_tiles(source_phase(scene,target),std::iter::empty());
        let mut inputs=std::mem::take(&mut contact.inputs);
        let cells=std::mem::take(&mut contact.cells);
        let stale=std::mem::take(&mut contact.stale);
        let mut baselines=std::mem::take(&mut contact.baselines);
        let mut reductions=Vec::new();
        if let Some(level)=contact.level {
            let source=&scene.scale_sources.entries[&target];let valid=PixelRect::full(source.valid_extent(level));
            for tile in &reduced {
                let key=source.key(r,target,level,tile.map(|value|value>>level));
                let region=page_rect(*tile).intersect(valid);
                if let Some(page)=contact.pages.get(&key).filter(|_|!region.is_empty()) {reductions.push((key,region,*tile,page.clone()));}
            }
        }
        let start=scene.jobs.len();
        let result=(|| {
            let handle=packet.scene.source_owner(target).ok_or(GpuRasterError::NotReady)?;
            let mut reads=Unit::default();
            for (_,_,tile,page) in &reductions {
                let rewritten=frame.document(scene.reduce_color_pages(r,packet,handle,page.plan,&page.view,&[*tile],None,[0;2],&stale,&mut inputs)?).in_frame(plan.extent);
                reads.reads.push(page.clone());
                for cell in pages::coordinates(rewritten,plan.level) {
                    if let Some(Baseline {stroked:Stroked::Level(area),..})=baselines.get_mut(&cell) {*area=area.union(rewritten);}
                }
            }
            if !reductions.is_empty() {reads.updates.push((target,reductions.len()));}
            if !reads.ready(scene,r,encoder,start,Class::Visible)? {return Err(GpuRasterError::NotReady);}
            let input=input_plan(self.plan,packet.scene,Some(self.graph.objects()));
            let mut units=vec![reads];let mut composed=Vec::new();let mut blocked=Vec::new();let mut fresh=Vec::new();
            for (region,drawn) in composes.into_iter().zip(drawn) {
                let start=scene.jobs.len();let mut unit=Unit::default();
                let cell=[region.min_x()/span,region.min_y()/span];
                let before=baselines.remove(&cell);
                let result=(|| {
                    let (_,declared)=cells.iter().find(|(cell,_)|*cell==region).ok_or(GpuRasterError::Invariant("a contact cell has no declared inputs"))?;
                    let mut accepted=declared.accept_query(r,Some(scene))?;
                    accepted.overlay(&inputs);
                    let input=unit_plan(input,packet.scene,region,tiled,Some(self.graph.objects()));
                    let mut compositor=Evaluator {cache:&mut *self,scene:&mut *scene,packet,r:&mut *r,encoder:&mut *encoder,region:region.into(),input,tiled,
                        unit:&mut unit,accepted:&mut accepted,previous_motion:None,retained,approximate:false,preview:false,class:Class::Visible};
                    compositor.evaluate_root(&root,Root::Window)?;
                    if !compositor.unit.ready(compositor.scene,compositor.r,compositor.encoder,start,Class::Visible)? {return Err(GpuRasterError::NotReady);}
                    unit.sources=accepted;
                    Ok(())
                })();
                match result {
                    Ok(())=>{
                        let [_,_,width,height]=paint_transform::texel_rect(PixelRect::full([region.width(),region.height()]),side);
                        let now=patch.and_then(|patch|patch.stroked(&unit,&scene.jobs[start..],target,cell,[width,height]))
                            .map(|stroked|(stroked,unit.reads.iter().map(pages::Held::stamp).collect::<Vec<_>>()));
                        let texels=now.as_ref().zip(before)
                            .filter(|((_,reads),before)|drawn.as_ref().is_some_and(|shown|shown.same(&before.submission)) && *reads==before.reads)
                            .and_then(|((now,_),before)|before.changed(now,region))
                            .map(|changed|paint_transform::texel_rect(changed.window_local(region),side)).filter(|[_,_,width,height]|width*height!=0);
                        if let Some(texels)=texels {crop(&mut scene.jobs[start..],texels);}
                        if let Some((stroked,reads))=now {fresh.push((cell,stroked,reads));}
                        unit.work=scene.planned_work(start);units.push(unit);
                        composed.push(texels.map_or(region,|[x,y,width,height]|PixelRect::new(x*side,y*side,(x+width)*side,(y+height)*side).translated([region.min_x(),region.min_y()]).intersect(region)));
                    },
                    Err(GpuRasterError::NotReady|GpuRasterError::NotPublished|GpuRasterError::SourceWorkingSetExceeded)=>{scene.discard_jobs_from(start);blocked.push(region);},
                    Err(error)=>return Err(error),
                }
            }
            self.encode_units(scene,r,encoder,units)?;
            Ok((composed,blocked,fresh))
        })();
        let contact=self.gesture.as_mut().expect("live contact");
        contact.inputs=inputs;contact.cells=cells;contact.stale=stale;contact.baselines=baselines;
        let (composed,blocked,fresh)=match result {Ok(done)=>done,Err(error)=>{scene.discard_jobs_from(start);return Err(error);}};
        let submission=encoder.submission();
        contact.baselines.extend(fresh.into_iter().map(|(cell,stroked,reads)|(cell,Baseline {submission:submission.clone(),stroked,reads})));
        if let Some((versions,_,predicted,previewed))=contact.next.take() {contact.composed=versions;contact.predicted=predicted;contact.previewed=previewed;}
        {
            let mut store=self.pages.lock().unwrap();
            for (key,region,tile,_) in reductions {
                if !contact.previewed.contains(&tile) {contact.stale.remove(&tile);}
                store.forget(&key,region);contact.written.insert(tile,(key,region,submission.clone()));
            }
        }
        contact.submissions.push(submission);
        Ok((composed,blocked))
    }
    pub(super) fn end_contact(&mut self,encoder:&mut submission::CommandEncoder) {
        let Some(contact)=self.gesture.take() else {return;};
        if let Some(Some(frame))=contact.exit.get() && !contact.written.values().any(|(_,_,submission)|submission.discarded()) {
            let mut store=self.pages.lock().unwrap();
            for (_,(key,region,_)) in contact.written.iter().filter(|(tile,_)|!contact.predicted.contains(*tile)) {store.materialize_source(key,*region,key.level,frame);}
        }
        self.requeue_keyless_cells();
        let Contact {inputs,pages,..}=contact;
        inputs.track(encoder);
        for page in pages.into_values() {page.track(encoder);}
    }
}
