use super::*;
use wgpu::util::DeviceExt;

const PIXELS_PER_END:u64=256;
const MIN_END_CAPACITY:u64=16384;
const END_WORDS:u64=20;
const FRONTIER_NODES:u64=256;
const VISIT_ENTRIES:u64=512;
const SEAL_STATE_WORDS:u64=40;
const SLOT_BYTES:u64=FRONTIER_NODES*8+VISIT_ENTRIES*2+SEAL_STATE_WORDS*4;
const SEAL_SLOTS:u64=(16<<20)/SLOT_BYTES/64*64;
const HEADER_WORDS:u32=6;
const UNFINISHED_SEALS:u32=5;
const SEAL_STEP_NANOSECONDS:f64=33.;
const SEAL_ROUND_WORK:[f64;2]=[64.,4096.];
const SEAL_PARALLEL_ENDS:f64=64.;
const MAX_SEAL_ROUNDS:u64=64;
const GAP_REACH:u32=64;
pub(super) const WINDOW_HALO:u32=GAP_REACH+2*MAX_LINE_WIDTH;
const MAX_LINE_WIDTH:u32=16;
const THIN_CUTOFF:u32=MAX_LINE_WIDTH*3/2;
const RIM_HALF_PIXELS:u32=3;
const BAND_ROWS:u32=256;
pub(crate) const UNDERFILL_HALF_PIXELS:u32=48;
const DISTANCE_LIMIT_HALF_PIXELS:u32=UNDERFILL_HALF_PIXELS+1;
const DISTANCE_HALO:u32=64;
const SKELETON_HALO:u32=1+32+2*(32+63+1);
const END_HALO:u32=33;
const FIT_DISPLACEMENT:u32=24;
const NEIGHBOUR_REACH:u32=64;
const PARAMS: u64 = 128;
const PARAMETER_BATCH_BYTES:u64=64*1024;
pub(crate) const CHUNK_NANOSECONDS:f64=20e6;
pub(super) const SCOPE_BLOCK:u32=128;
const SCOPE_BLOCKS:i64=3;
type Step = (&'static str,u64,Vec<u32>);
type Readback<T> = Option<mpsc::Receiver<Result<T,GpuRasterError>>>;
const SNAP_AREA:usize=33*33;
const MIDDLE_CLIMB:u32=16;
pub(crate) struct ScopeProbe {pub stale:u32, pub flagged:Vec<[u32;2]>, region:Vec<u32>}
#[derive(Clone, PartialEq, Debug)]
enum Built {Grain,Blocks(std::collections::BTreeSet<[u32;2]>),Full}
struct Sealing { params:Vec<u32>, active:u64, started:bool, unfinished:Readback<[u32;1]> }
impl Sealing {
    fn rounds(&mut self,budget:f64)->(Vec<Step>,f64) {
        let ends=(self.active as f64).max(SEAL_PARALLEL_ENDS);
        let work=(budget/ends).clamp(SEAL_ROUND_WORK[0],SEAL_ROUND_WORK[1]);
        let rounds=((budget/(ends*work)) as u64).clamp(1,MAX_SEAL_ROUNDS);
        let count=u64::from(self.params[18]-self.params[24]);
        let steps=(0..rounds).map(|round| {
            let mut params=self.params.clone();
            params[19]=(work as u32)<<2|u32::from(round==0 && !self.started)|u32::from(round+1==rounds && self.unfinished.is_none())<<1;
            ("seal_candidates",count,params)
        }).collect();
        self.started=true;
        (steps,rounds as f64*ends*work)
    }
}
struct Build { steps:std::collections::VecDeque<Step>, submission:Option<crate::submission::Submission>,in_flight:std::sync::Arc<std::sync::atomic::AtomicU32>, ends:Readback<[u32;2]>, ranked:bool, incremental:bool,
    open:Readback<[u32;1]>, reads_open:bool, lines:bool, target:Built, grain:paper::Grain, scoped:bool, sealing:Option<Sealing> }
impl Build {
    fn new(steps:Vec<Step>,target:Built,grain:paper::Grain)->Self {
        Self {steps:steps.into(),submission:None,in_flight:Default::default(),ends:None,ranked:false,incremental:false,open:None,reads_open:false,lines:false,target,grain,scoped:false,sealing:None}
    }
    fn available(&self,demand:bool)->bool {
        let count=self.in_flight.load(std::sync::atomic::Ordering::Acquire);
        self.submission.as_ref().is_none_or(|owner|owner.submitted())
            && (self.ends.is_none() && self.open.is_none() || count==0)
            && (count<if demand {crate::selection_refine::CHUNKS_IN_FLIGHT} else {1} || demand && self.finished())
    }
    fn finished(&self)->bool {self.ranked && self.steps.is_empty() && self.sealing.is_none()}
}
const STAGES: [&str; 25] = ["grain_bits", "distance_seeds", "jump_distance", "store_distance", "skeleton_seeds", "skeleton_ridges", "skeleton_prune", "line_ends", "refine_ends", "seal_candidates", "scope_region", "scope_snap", "open_bits", "apply_seals", "region_seeds", "assign_region", "hold_barriers", "grow_region", "intersect_bits", "remember_thin", "check_seeds", "barrier_plane", "snap_seed", "reset_ends", "assign_tokens"];

pub(crate) struct Input<'a> {pub device:&'a PipelineDevice,pub mask:&'a wgpu::Buffer,pub paper:&'a wgpu::TextureView}
pub(super) struct Lines {
    layout: wgpu::BindGroupLayout,
    colour_layout:wgpu::BindGroupLayout,
    colour:Deferred<wgpu::ComputePipeline>,
    pipelines: std::collections::HashMap<&'static str, Deferred<wgpu::ComputePipeline>>,
    scratch: Option<[wgpu::Buffer; 2]>,
    tokens:Option<wgpu::Buffer>,
    frontier:Option<wgpu::Buffer>,
    data: Option<wgpu::Buffer>,
    extent: [u32; 2],
    offsets: [u32; 4],
    windows: Vec<([u32; 4], [u32; 4])>,
    building:Option<Build>,
    ends:u32,
    overflow:bool,
    built:Built,
    open:bool,
}
impl Lines {
    pub fn new(device: &PipelineDevice) -> Self {
        let mut entries: Vec<_> = (1..=5).map(|binding| crate::bindings::buffer(binding, wgpu::ShaderStages::COMPUTE,
            if binding == 1 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only:false } },
            binding == 1, (binding == 1).then(|| wgpu::BufferSize::new(PARAMS).unwrap()))).collect();
        entries.push(crate::bindings::texture(6, wgpu::ShaderStages::COMPUTE, false));
        let layout = crate::bindings::layout(device, "region line stages", &entries);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:Some("region line stages"),bind_group_layouts:&[Some(&layout)],immediate_size:0,
        });
        let shader=Deferred::wgsl(device,"region line stages",compose_wgsl(&[&working_color::shader(device),include_str!("region_lines.wgsl")]));
        let pipelines=STAGES.into_iter().map(|entry|(entry,Deferred::compute(device,entry,&pipeline_layout,&shader,entry))).collect();
        let colour_layout=crate::bindings::layout(device,"area colours",&[
            crate::bindings::buffer(1,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Uniform,false,wgpu::BufferSize::new(PARAMS)),
            crate::bindings::buffer(2,wgpu::ShaderStages::COMPUTE,wgpu::BufferBindingType::Storage {read_only:false},false,None),
            crate::bindings::storage_texture(7,wgpu::ShaderStages::COMPUTE,wgpu::TextureFormat::Rgba32Float,wgpu::StorageTextureAccess::WriteOnly),
        ]);
        let colour_pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:Some("area colours"),bind_group_layouts:&[Some(&colour_layout)],immediate_size:0,
        });
        let colour=Deferred::compute(device,"area colours",&colour_pipeline_layout,&shader,"colour_tokens");
        Self {layout,colour_layout,colour,pipelines,scratch:None,tokens:None,frontier:None,data:None,extent:[0;2],offsets:[0;4],windows:Vec::new(),building:None,ends:0,overflow:false,built:Built::Grain,open:true}
    }
    pub fn storage_bytes(&self)->u64 {
        self.tokens.as_ref().map_or(0,wgpu::Buffer::size)+self.frontier.as_ref().map_or(0,wgpu::Buffer::size)+self.data.as_ref().map_or(0,wgpu::Buffer::size)+self.scratch.as_ref().map_or(0,|buffers|buffers.iter().map(wgpu::Buffer::size).sum())
    }
    pub fn cache_pipelines(&self)->impl Iterator<Item=&Deferred<wgpu::ComputePipeline>> {
        STAGES.iter().filter(|entry|self.building.as_ref().map_or_else(
            ||STAGES[..8].contains(entry) || **entry=="reset_ends",
            |work|work.steps.iter().any(|(pending,_,_)|pending==*entry))).map(|entry|&self.pipelines[entry])
    }
    pub fn prepare_cache(&self,compiler:&compile::Scope,priority:startup::Class)->bool {
        if self.building.as_ref().is_none_or(|work|!work.ranked) {
            compiler.require(["refine_ends","seal_candidates"].map(|entry|&self.pipelines[entry]),priority);
        }
        compiler.require(self.cache_pipelines(),priority)
    }
    pub fn pipelines(&self,refinement:RegionRefinement,prepare_ends:bool)->impl Iterator<Item=&Deferred<wgpu::ComputePipeline>> {
        self.cache_pipelines().chain(STAGES[8..].iter().filter(move |entry|match **entry {
            "assign_tokens"=>false,
            "refine_ends"|"seal_candidates"=>!self.overflow,
            "grow_region"=>matches!(refinement.edges,layer_render::RegionEdges::GrowShrink(_)),
            "assign_region"=>matches!(refinement.edges,layer_render::RegionEdges::LineMiddle),
            "region_seeds"=>!matches!(refinement.edges,layer_render::RegionEdges::Exact),
            "apply_seals"=>refinement.close_gaps && !self.overflow && (prepare_ends || self.ends>0),
            _=>true,
        }).map(|entry|&self.pipelines[entry]))
    }
    pub fn area_pipelines(&self)->impl Iterator<Item=&Deferred<wgpu::ComputePipeline>> {
        self.cache_pipelines().chain(["open_bits","apply_seals","hold_barriers","reset_ends","region_seeds","jump_distance","assign_tokens"].map(|entry|&self.pipelines[entry])).chain([&self.colour])
    }
    pub fn token_buffer(&self)->&wgpu::Buffer {self.tokens.as_ref().unwrap()}
    pub fn begin_areas(&mut self,device:&PipelineDevice) {
        let size=32+self.scratch.as_ref().unwrap()[1].size();
        if self.tokens.as_ref().is_none_or(|buffer|buffer.size()<size) {
            self.tokens=Some(device.create_buffer(&wgpu::BufferDescriptor {label:Some("region window tokens"),size,
                usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false}));
        }
    }
    pub fn hold_barriers(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder) {
        self.encode(input,encoder,None,None,vec![("hold_barriers",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],paper::Grain::FLAT))]);
    }
    pub fn assign_tokens(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,core:[u32;4],window:[u32;4]) {
        let mut steps=vec![("reset_ends",1,self.params(core,window,[2,0,0,0],paper::Grain::FLAT))];
        let words=(window[0]+window[2]).div_ceil(32)-window[0]/32;
        steps.push(("region_seeds",u64::from(words)*u64::from(window[3]),self.params(core,window,[0,5,0,0],paper::Grain::FLAT)));
        let nearest=self.jump_steps(&mut steps,core,window,"region_seeds",4,UNDERFILL_HALF_PIXELS.div_ceil(2));
        self.encode(input,encoder,None,None,steps);
        let scratch=self.scratch.as_ref().unwrap();let tokens=self.token_buffer();
        let (first,second)=if nearest==0 {(&scratch[0],tokens)}else{(tokens,&scratch[1])};
        self.encode(input,encoder,Some(first),Some(second),vec![("assign_tokens",word_count(core),self.params(core,window,[UNDERFILL_HALF_PIXELS,4,0,nearest],paper::Grain::FLAT))]);
    }
    pub fn colour_tokens(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,core:[u32;4],window:[u32;4],page:[u32;2],output:&wgpu::TextureView) {
        let mut params=self.params(core,window,[0;4],paper::Grain::FLAT);
        params[24..28].copy_from_slice(&[page[0],page[1],256,0]);
        let bytes:Vec<_>=params.into_iter().flat_map(u32::to_ne_bytes).collect();
        let uniform=device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("area colour parameters"),contents:&bytes,usage:wgpu::BufferUsages::UNIFORM});
        let group=device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("area colours"),layout:&self.colour_layout,entries:&[
            wgpu::BindGroupEntry {binding:1,resource:uniform.as_entire_binding()},
            wgpu::BindGroupEntry {binding:2,resource:self.token_buffer().as_entire_binding()},
            wgpu::BindGroupEntry {binding:7,resource:wgpu::BindingResource::TextureView(output)},
        ]});
        let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("area colours"),timestamp_writes:None});
        pass.set_pipeline(&self.colour);pass.set_bind_group(0,&group,&[]);pass.dispatch_workgroups(256,4,1);
    }
    fn layout(&mut self,extent:[u32;2],limits:&wgpu::Limits,windows:&[(u32,[u32;4])])->Result<u64,GpuRasterError> {
        let (offsets,size)=storage_layout(extent,limits).ok_or(GpuRasterError::SizeOverflow)?;
        self.extent=extent;self.offsets=offsets;
        self.windows=windows.iter().map(|(_,core)|(*core,analysis_window(*core,extent))).collect();
        Ok(size)
    }
    pub fn allocate(&mut self,device:&PipelineDevice,extent:[u32;2],side:u32,windows:&[(u32,[u32;4])])->Result<(),GpuRasterError> {
        let size=self.layout(extent,&device.limits(),windows)?;
        let offsets=self.offsets;
        if self.data.as_ref().is_none_or(|buffer|buffer.size()<size) {
            self.data=Some(device.create_buffer(&wgpu::BufferDescriptor {label:Some("cached region lines"),size,
                usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false}));
        }
        let scratch_bytes=u64::from(side+2*WINDOW_HALO).pow(2)*4;
        let compact_bytes=scratch_bytes.max(u64::from(offsets[3])*END_WORDS*4);
        if self.scratch.as_ref().is_none_or(|buffers|buffers[0].size()<compact_bytes || buffers[1].size()<scratch_bytes) {
            self.scratch=Some(std::array::from_fn(|index|device.create_buffer(&wgpu::BufferDescriptor {label:Some("region window coordinates"),size:if index==0 {compact_bytes}else{scratch_bytes},
                usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false})));
        }
        if self.frontier.is_none() {
            let limits=device.limits();
            let slots=(limits.max_storage_buffer_binding_size.min(limits.max_buffer_size)/SLOT_BYTES).clamp(64,SEAL_SLOTS)/64*64;
            self.frontier=Some(device.create_buffer(&wgpu::BufferDescriptor {label:Some("region seal search frontier"),size:slots*SLOT_BYTES,
                usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false}));
        }
        Ok(())
    }
    fn params(&self,core:[u32;4],window:[u32;4],options:[u32;4],paper:paper::Grain)->Vec<u32> {
        let [w,h]=self.extent;
        [vec![w,h,w.div_ceil(32),w.div_ceil(4)],window.to_vec(),core.to_vec(),self.offsets.to_vec(),options.to_vec(),paper.tile.map(f32::to_bits).to_vec(),vec![0;4],paper.axes.map(f32::to_bits).to_vec()].concat()
    }
    fn encode(&self,input:&Input<'_>,encoder:&mut impl Record,first:Option<&wgpu::Buffer>,second:Option<&wgpu::Buffer>,steps:Vec<Step>)->f64 {
        let dispatched=steps.iter().map(|(entry,count,params)|*count as f64*nanoseconds(entry,params,1)).sum();
        let data=self.data.as_ref().unwrap();
        let Input {device,mask,paper}=input;
        let alignment=u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let stride=PARAMS.next_multiple_of(alignment);
        let scratch=self.scratch.as_ref().unwrap();
        for steps in steps.chunks((PARAMETER_BATCH_BYTES/stride) as usize) {
            let mut values=vec![0u8;steps.len()*stride as usize];
            for ((_,_,params),block) in steps.iter().zip(values.chunks_mut(stride as usize)) {
                for (word,value) in block.as_chunks_mut::<4>().0.iter_mut().zip(params){word.copy_from_slice(&value.to_ne_bytes());}
            }
            let uniform=device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("region line parameters"),contents:&values,usage:wgpu::BufferUsages::UNIFORM});
            let group=device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("region line stages"),layout:&self.layout,entries:&[
                wgpu::BindGroupEntry {binding:1,resource:wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer:&uniform,offset:0,size:wgpu::BufferSize::new(PARAMS)})},
                wgpu::BindGroupEntry {binding:2,resource:first.unwrap_or(&scratch[0]).as_entire_binding()},
                wgpu::BindGroupEntry {binding:3,resource:second.unwrap_or(&scratch[1]).as_entire_binding()},
                wgpu::BindGroupEntry {binding:4,resource:data.as_entire_binding()},
                wgpu::BindGroupEntry {binding:5,resource:mask.as_entire_binding()},
                wgpu::BindGroupEntry {binding:6,resource:wgpu::BindingResource::TextureView(paper)},
            ]});
            let dispatches:Vec<_>=steps.iter().enumerate().filter(|(_,(_,count,_))|*count!=0).map(|(i,(entry,count,_))| {
                let groups=count.div_ceil(64) as u32;let maximum=device.limits().max_compute_workgroups_per_dimension;
                let rows=groups.div_ceil(maximum);
                ((*self.pipelines[entry]).clone(),(i as u64*stride) as u32,[groups.div_ceil(rows),rows])
            }).collect();
            encoder.record(move|encoder| {
                let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("region line stages"),timestamp_writes:None});
                for (pipeline,offset,[x,y]) in &dispatches {pass.set_pipeline(pipeline);pass.set_bind_group(0,&group,&[*offset]);pass.dispatch_workgroups(*x,*y,1);}
            });
        }
        dispatched
    }
    fn jump_steps(&self,steps:&mut Vec<Step>,core:[u32;4],window:[u32;4],seeds:&'static str,kind:u32,reach:u32)->u32 {
        let count=u64::from(window[2])*u64::from(window[3]);
        steps.push((seeds,count,self.params(core,window,[0,kind,0,0],paper::Grain::FLAT)));
        let mut input=0;
        for step in (0..=reach.max(1).next_power_of_two().ilog2()).rev().map(|n|1<<n).chain([1]) {
            steps.push(("jump_distance",if kind==0 || kind==4 {word_count(window)}else{count},self.params(core,window,[0,kind,step,input],paper::Grain::FLAT)));input^=1;
        }
        input
    }
    fn windows_in(&self,regions:&[PixelRect],context:u32)->Vec<([u32;4],[u32;4])> {
        self.windows.iter().filter_map(|&(core,window)| {
            let core=rect(core);let window=rect(window);
            let core=regions.iter().map(|region|core.intersect(*region)).filter(|core|!core.is_empty()).reduce(PixelRect::union)?;
            Some((bounds(core),bounds(core.expand(context,self.extent).intersect(window))))
        }).collect()
    }
    fn grain_steps(&self,grain:paper::Grain,radius:u32,has_paper:bool,dirty:&std::collections::BTreeSet<[u32;2]>)->Vec<Step> {
        let windows=if dirty.is_empty() {self.windows.clone()}else{self.windows_in(&changed_regions(dirty,radius.max(1),self.extent),0)};
        windows.into_iter().map(|(core,window)|("grain_bits",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[radius,u32::from(has_paper),0,0],grain))).collect()
    }
    fn line_steps(&self,grain:paper::Grain,radius:u32,dirty:&std::collections::BTreeSet<[u32;2]>)->Vec<Step> {
        let mut steps=Vec::new();
        let changed=|halo|changed_regions(dirty,halo,self.extent);
        let distance_windows=if dirty.is_empty() {self.windows.clone()}else{self.windows_in(&changed(radius.max(1)+DISTANCE_HALO),DISTANCE_HALO)};
        self.distance_steps(&mut steps,distance_windows,grain);
        let end_halo=radius.max(1)+DISTANCE_HALO+SKELETON_HALO+END_HALO;
        let end_windows=if dirty.is_empty() {self.windows.clone()}else{self.windows_in(&changed(end_halo),SKELETON_HALO+END_HALO)};
        if !dirty.is_empty() {
            steps.push(("reset_ends",u64::from(self.ends),self.params([0;4],[0;4],[3,self.ends,0,0],grain)));
            for &(core,_) in &end_windows {steps.push(("reset_ends",u64::from(self.ends),self.params(core,[0;4],[4,self.ends,0,0],grain)));}
            let candidate_halo=end_halo+2*(NEIGHBOUR_REACH+2*FIT_DISPLACEMENT);
            for (core,_) in self.windows_in(&changed(candidate_halo),0) {steps.push(("reset_ends",u64::from(self.ends),self.params(core,[0;4],[5,self.ends,0,0],grain)));}
            steps.push(self.clear_ends(grain));
            steps.push(("reset_ends",u64::from(self.ends),self.params([0;4],[0;4],[6,self.ends,0,0],grain)));
            steps.push(("reset_ends",u64::from(self.offsets[0]),self.params([0;4],[0;4],[7,0,0,0],grain)));
        }
        for (core,window) in end_windows {self.end_steps(&mut steps,core,window,grain);}
        steps
    }
    fn distance_steps(&self,steps:&mut Vec<Step>,windows:Vec<([u32;4],[u32;4])>,grain:paper::Grain) {
        for (core,window) in windows {
            let input=self.jump_steps(steps,core,window,"distance_seeds",0,DISTANCE_LIMIT_HALF_PIXELS.div_ceil(2));
            steps.push(("store_distance",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[DISTANCE_LIMIT_HALF_PIXELS,0,0,input],grain)));
        }
    }
    fn clear_ends(&self,grain:paper::Grain)->Step {("reset_ends",u64::from(self.offsets[2]-self.offsets[1]-HEADER_WORDS).max(1),self.params([0;4],[0;4],[0;4],grain))}
    fn scope_steps(&self,grain:paper::Grain,target:&Built)->Vec<Step> {
        let mut steps=vec![self.clear_ends(grain)];
        let Built::Blocks(blocks)=target else {
            steps.extend(self.line_steps(grain,1,&Default::default()));
            return steps;
        };
        let cores:Vec<_>=blocks.iter().map(|&[x,y]|PixelRect::new(x*SCOPE_BLOCK,y*SCOPE_BLOCK,(x+1)*SCOPE_BLOCK,(y+1)*SCOPE_BLOCK).intersect(PixelRect::full(self.extent))).filter(|core|!core.is_empty()).collect();
        let reach:Vec<_>=cores.iter().map(|core|core.expand(SKELETON_HALO+END_HALO,self.extent)).collect();
        steps.push(("reset_ends",u64::from(self.offsets[0]),self.params([0;4],[0;4],[7,0,0,0],grain)));
        self.distance_steps(&mut steps,self.windows_in(&reach,DISTANCE_HALO),grain);
        for (core,window) in self.windows_in(&cores,SKELETON_HALO+END_HALO) {self.end_steps(&mut steps,core,window,grain);}
        steps
    }
    fn end_steps(&self,steps:&mut Vec<Step>,core:[u32;4],window:[u32;4],grain:paper::Grain) {
        let count=u64::from(window[2])*u64::from(window[3]);
        steps.push(("skeleton_seeds",count,self.params(core,window,[0;4],grain)));
        let sparse_count=word_count(window);
        for distance in (2..=16).step_by(2) {for phase in 0..4 {
            steps.push(("skeleton_ridges",sparse_count,self.params(core,window,[distance,0,phase,0],grain)));
        }}
        let mut input=0;
        for _ in 0..2 {for mode in 0..2 {steps.push(("skeleton_prune",count,self.params(core,window,[mode,0,0,input],grain)));}input^=1;}
        steps.push(("line_ends",word_count(core),self.params(core,window,[0,MAX_LINE_WIDTH,0,input],grain)));
    }
    pub fn building(&self)->bool {self.building.is_some()}
    #[cfg(test)]
    pub fn compaction_pending(&self)->Option<bool> {
        self.building.as_ref().filter(|work|work.incremental).map(|work|
            work.steps.iter().any(|(entry,_,params)|*entry=="reset_ends" && params[16]==6))
    }
    fn available(&self,demand:bool)->bool {
        self.building.as_ref().is_none_or(|work|work.available(demand))
    }
    pub fn ready(&mut self,demand:bool)->Result<bool,GpuRasterError> {
        if !self.available(demand) {return Ok(false);}
        let Some(mut work)=self.building.take() else {return Ok(true);};
        if let Some(rx)=&work.open {
            let [open]=match rx.try_recv() {Ok(result)=>result?,Err(mpsc::TryRecvError::Empty)=>{self.building=Some(work);return Ok(false);},Err(error)=>return Err(GpuRasterError::MapFailed(error.to_string()))};
            self.open=open!=0 || work.incremental && self.open;work.open=None;
            if work.lines && self.open {work.steps=self.scope_steps(work.grain,&Built::Full).into();work.target=Built::Full;}else{work.ranked=true;}
        }
        if let Some(rx)=&work.ends {
            let metadata=match rx.try_recv() {Ok(result)=>result?,Err(mpsc::TryRecvError::Empty)=>{self.building=Some(work);return Ok(false);},Err(error)=>return Err(GpuRasterError::MapFailed(error.to_string()))};
            self.overflow=metadata[1]!=0;self.ends=metadata[0].min(self.offsets[3]);work.ends=None;work.ranked=true;
            if !self.overflow && self.ends>0 {for entry in ["refine_ends","seal_candidates"] {work.steps.push_back((entry,u64::from(self.ends),self.params([0;4],[0;4],[THIN_CUTOFF,u32::from(work.incremental),0,0],paper::Grain::FLAT)));}}
        }
        if let Some(sealing)=&mut work.sealing && let Some(rx)=&sealing.unfinished {
            match rx.try_recv() {
                Ok(result)=>{let [unfinished]=result?;sealing.active=u64::from(unfinished);sealing.unfinished=None;},
                Err(mpsc::TryRecvError::Empty)=>{},
                Err(error)=>return Err(GpuRasterError::MapFailed(error.to_string())),
            }
        }
        if work.sealing.as_ref().is_some_and(|sealing|sealing.active==0) {work.sealing=None;}
        self.building=Some(work);Ok(true)
    }
    pub fn discarded(&self)->bool {self.building.as_ref().is_some_and(|work|work.submission.as_ref().is_some_and(|owner|owner.discarded()))}
    pub fn submission(&self)->Option<crate::submission::Submission> {self.building.as_ref().and_then(|work|work.submission.clone())}
    pub fn cancel(&mut self) {self.building=None;}
    #[allow(clippy::too_many_arguments)]
    pub fn build(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,paper:(paper::Grain,u32,bool),budget:f64,demand:bool,dirty:&std::collections::BTreeSet<[u32;2]>,(lines,reads_open):(bool,bool))->Result<(bool,f64),GpuRasterError> {
        let (grain,radius,has_paper)=paper;
        if self.building.as_ref().is_some_and(|work|work.scoped) {self.building=None;}
        if self.building.is_none() {
            let incremental=!dirty.is_empty() && !self.overflow;
            self.building=Some(if incremental && self.built==Built::Full {
                Build {incremental,..Build::new([self.grain_steps(grain,radius,has_paper,dirty),self.line_steps(grain,radius,dirty)].concat(),Built::Full,grain)}
            } else {
                if incremental {encoder.clear_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+2)*4,Some(4));}
                else {encoder.clear_buffer(self.data.as_ref().unwrap(),0,None);self.overflow=false;}
                self.ends=0;
                let empty=std::collections::BTreeSet::new();
                let steps=[if incremental {vec![self.clear_ends(grain)]} else {Vec::new()},self.grain_steps(grain,radius,has_paper,if incremental {dirty} else {&empty})].concat();
                Build {incremental,reads_open:reads_open || lines,ranked:!reads_open && !lines,lines,..Build::new(steps,Built::Grain,grain)}
            });
            self.built=Built::Grain;
        }
        self.advance(input,encoder,radius,budget,demand)
    }
    pub fn build_scope(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,paper:(paper::Grain,u32),budget:f64,blocks:Option<std::collections::BTreeSet<[u32;2]>>)->Result<(bool,f64),GpuRasterError> {
        let target=blocks.map_or(Built::Full,Built::Blocks);
        if self.covers(&target) {return Ok((true,0.));}
        if self.building.as_ref().is_some_and(|work|work.target!=target) {self.building=None;}
        if self.building.is_none() {self.building=Some(Build {scoped:true,..Build::new(self.scope_steps(paper.0,&target),target.clone(),paper.0)});}
        let (ready,spent)=self.advance(input,encoder,paper.1,budget,true)?;
        Ok((ready && self.covers(&target),spent))
    }
    fn covers(&self,target:&Built)->bool {
        match (&self.built,target) {(Built::Full,_)=>true,(Built::Blocks(built),Built::Blocks(blocks))=>blocks.is_subset(built),_=>false}
    }
    pub fn full(&self)->bool {self.built==Built::Full}
    #[cfg(test)]
    pub fn built_blocks(&self)->Option<usize> {match &self.built {Built::Full=>None,Built::Grain=>Some(0),Built::Blocks(blocks)=>Some(blocks.len())}}
    pub fn open(&self)->bool {self.open}
    pub fn scope_blocks(&self,seed:[u32;2],flagged:impl Iterator<Item=[u32;2]>)->std::collections::BTreeSet<[u32;2]> {
        let blocks=self.extent.map(|n|i64::from(n.div_ceil(SCOPE_BLOCK)));
        flagged.chain([seed.map(|n|n/SCOPE_BLOCK)]).flat_map(|[x,y]|(-SCOPE_BLOCKS..=SCOPE_BLOCKS).flat_map(move |dy|(-SCOPE_BLOCKS..=SCOPE_BLOCKS).map(move |dx|[i64::from(x)+dx,i64::from(y)+dy])))
            .filter(|[x,y]|(0..blocks[0]).contains(x) && (0..blocks[1]).contains(y)).map(|[x,y]|[x as u32,y as u32]).collect()
    }
    pub fn tap_distance(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,tap:[u32;2],grain:paper::Grain) {
        let reach=MIDDLE_CLIMB+1;
        let around=PixelRect::new(tap[0].saturating_sub(reach),tap[1].saturating_sub(reach),(tap[0]+reach+1).min(self.extent[0]),(tap[1]+reach+1).min(self.extent[1]));
        let mut steps=Vec::new();
        self.distance_steps(&mut steps,self.windows_in(&[around],DISTANCE_HALO),grain);
        self.encode(input,encoder,None,None,steps);
    }
    pub fn probe_scope(&self,input:&Input<'_>,encoder:&mut impl Record,tap:[u32;2])->mpsc::Receiver<Result<ScopeProbe,GpuRasterError>> {
        let columns=self.extent[0].div_ceil(SCOPE_BLOCK);let blocks=columns as usize*self.extent[1].div_ceil(SCOPE_BLOCK) as usize;
        let scope=&self.scratch.as_ref().unwrap()[0];let bytes=((1+blocks+SNAP_AREA)*4) as u64;
        encoder.clear(scope,0,Some(bytes));
        self.encode(input,encoder,None,None,vec![("scope_region",u64::from(self.offsets[0]),self.params([0;4],[0;4],[tap[0],tap[1],0,0],paper::Grain::FLAT))]);
        let readback=input.device.create_buffer(&wgpu::BufferDescriptor {label:Some("region scope"),size:bytes,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
        encoder.copy(scope,0,&readback,0,bytes);
        let (tx,rx)=mpsc::channel();
        let device=input.device.clone();
        encoder.record(move|encoder|crate::raster::map_then(&device,Some(encoder),&readback,bytes,move|bytes|{
            let words:Vec<u32>=bytes.as_chunks::<4>().0.iter().map(|word|u32::from_le_bytes(*word)).collect();
            let flagged=words[1..1+blocks].iter().enumerate().filter(|(_,flag)|**flag!=0).map(|(index,_)|[index as u32%columns,index as u32/columns]).collect();
            Ok(ScopeProbe {stale:words[0],flagged,region:words[1+blocks..].to_vec()})
        },move|result|{let _=tx.send(result);}));
        rx
    }
    pub fn read_stale(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder)->mpsc::Receiver<Result<[u32;1],GpuRasterError>> {
        self.read_header(device,encoder,2,"region seed check")
    }
    fn read_header<const N:usize>(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,word:u32,label:&'static str)->mpsc::Receiver<Result<[u32;N],GpuRasterError>> {
        let size=4*N as u64;
        let readback=device.create_buffer(&wgpu::BufferDescriptor {label:Some(label),size,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
        encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+word)*4,&readback,0,size);
        let (tx,rx)=mpsc::channel();
        crate::raster::map_then(device,Some(encoder),&readback,size,|bytes|Ok(std::array::from_fn(|i|u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap()))),move|result|{let _=tx.send(result);});
        rx
    }
    pub fn check_snap(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,tap:[u32;2],probe:&ScopeProbe,stop:Option<&wgpu::Buffer>) {
        let marks=input.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("region snap marks"),
            contents:&[vec![0;4*SNAP_AREA],probe.region.clone()].concat().iter().flat_map(|word|word.to_ne_bytes()).collect::<Vec<_>>(),usage:wgpu::BufferUsages::STORAGE});
        let checks=[0,1].map(|mode|("scope_snap",if mode==0 {u64::from(self.ends)}else{1},self.params([0;4],[0;4],[tap[0],tap[1],u32::from(stop.is_some()),mode],paper::Grain::FLAT)));
        self.encode(input,encoder,stop,Some(&marks),checks.to_vec());
    }
    fn advance(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,radius:u32,budget:f64,demand:bool)->Result<(bool,f64),GpuRasterError> {
        let device=input.device;
        use std::sync::atomic::Ordering;
        if !self.ready(demand)? {return Ok((false,0.));}
        let mut work=self.building.take().unwrap();
        if work.finished() {self.built=work.target;return Ok((true,0.));}
        let (steps,mut spent)=if work.sealing.is_some() {(Vec::new(),0.)} else {plan_chunk(&mut work.steps,budget,radius)};
        let mut encoded=!steps.is_empty();
        self.encode(input,encoder,None,None,steps);
        let room=(budget-spent).max(0.)/SEAL_STEP_NANOSECONDS;
        if work.sealing.is_none() && (spent==0. || room>=SEAL_PARALLEL_ENDS*SEAL_ROUND_WORK[0]) && let Some((_,remaining,params))=work.steps.pop_front_if(|(entry,_,_)|*entry=="seal_candidates") {
            let count=remaining.min(self.frontier.as_ref().unwrap().size()/SLOT_BYTES).min(((room/SEAL_ROUND_WORK[0]) as u64).max(SEAL_PARALLEL_ENDS as u64));
            let mut sealed=params.clone();sealed[18]=params[24]+count as u32;
            if count<remaining {let mut rest=params;rest[24]+=count as u32;work.steps.push_front(("seal_candidates",remaining-count,rest));}
            work.sealing=Some(Sealing {params:sealed,active:count,started:false,unfinished:None});
        }
        if let Some(sealing)=&mut work.sealing {
            let counted=sealing.unfinished.is_none();
            let (rounds,steps)=sealing.rounds(room);
            spent+=steps*SEAL_STEP_NANOSECONDS;
            if counted {encoder.clear_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+UNFINISHED_SEALS)*4,Some(4));}
            self.encode(input,encoder,None,self.frontier.as_ref(),rounds);
            if counted {sealing.unfinished=Some(self.read_header(device,encoder,UNFINISHED_SEALS,"region unfinished seal searches"));}
            encoded=true;
        }
        if work.steps.is_empty() && work.sealing.is_none() && work.reads_open {
            work.open=Some(self.read_header(device,encoder,2,"region open pixels"));work.reads_open=false;encoded=true;
        } else if work.steps.is_empty() && work.sealing.is_none() && !work.ranked {
            work.ends=Some(self.read_header(device,encoder,0,"region line end count"));encoded=true;
        }
        if encoded {
            work.submission=Some(encoder.submission());work.in_flight.fetch_add(1,Ordering::AcqRel);
            let in_flight=crate::InFlightCompletion(work.in_flight.clone());encoder.hold(in_flight);
        }
        if !encoded && work.finished() {self.built=work.target;return Ok((true,spent));}
        self.building=Some(work);Ok((false,spent))
    }
    pub fn has_seals(&self)->bool {!self.overflow && self.ends!=0}
    pub fn exclude(&self,input:&Input<'_>,encoder:&mut impl Record,excluded:&[u32])->f64 {
        if !self.has_seals(){return 0.;}
        let mut ids=excluded.to_vec();ids.sort_unstable();ids.dedup();
        let bytes=ids.iter().flat_map(|id|id.to_ne_bytes()).collect::<Vec<_>>();
        let buffer=(!ids.is_empty()).then(||input.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:Some("removed fill seals"),contents:&bytes,usage:wgpu::BufferUsages::STORAGE}));
        self.encode(input,encoder,None,buffer.as_ref(),vec![("reset_ends",u64::from(self.ends),
            self.params([0;4],[0;4],[8,self.ends,0,ids.len() as u32],paper::Grain::FLAT))])
    }
    pub fn seal_metadata(&self,device:&PipelineDevice)->Option<wgpu::Buffer> {
        self.has_seals().then(||device.create_buffer(&wgpu::BufferDescriptor {label:Some("fill seal metadata"),
            size:u64::from(self.ends)*20,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false}))
    }
    pub fn capture_seals(&self,input:&Input<'_>,encoder:&mut impl Record,output:&wgpu::Buffer,parents:&wgpu::Buffer,slot:u32)->f64 {
        self.encode(input,encoder,Some(output),Some(parents),vec![("apply_seals",u64::from(self.ends),
            self.params([0;4],[0;4],[0,slot,0,1],paper::Grain::FLAT))])
    }

    pub fn barriers(&self,input:&Input<'_>,encoder:&mut impl Record,close:bool,reach:u32,job:[u32;4],parents:&wgpu::Buffer)->f64 {
        let params=|mut steps:Vec<Step>| {
            for (_,_,params) in &mut steps {params[24..28].copy_from_slice(&job);}
            steps
        };
        self.encode(input,encoder,Some(parents),None,params(vec![("open_bits",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],paper::Grain::FLAT))]))
            +if close && self.has_seals() {
                self.encode(input,encoder,Some(parents),None,params(vec![("apply_seals",u64::from(self.ends),self.params([0;4],[0;4],[0,0,reach/16-1,0],paper::Grain::FLAT))]))
            }else{0.}
            +self.encode(input,encoder,Some(parents),None,params(vec![("hold_barriers",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],paper::Grain::FLAT))]))
    }

    pub fn band_strips(&self,area:Option<PixelRect>)->Vec<([u32;4],[u32;4])> {
        area.map_or_else(||self.windows.clone(),|area|self.windows_in(&[area],WINDOW_HALO)).into_iter().flat_map(|([x,y,w,h],window)| {
            (y..y+h).step_by(BAND_ROWS as usize).map(move |top| {
                let core=[x,top,w,BAND_ROWS.min(y+h-top)];
                (core,bounds(rect(core).expand(WINDOW_HALO,self.extent).intersect(rect(window))))
            })
        }).collect()
    }
    pub fn assign_steps(&self,fill:bool,(core,window):([u32;4],[u32;4]))->Vec<Step> {
        let mut steps=vec![("reset_ends",1,self.params(core,window,[2,0,0,0],paper::Grain::FLAT))];
        let words=(window[0]+window[2]).div_ceil(32)-window[0]/32;
        steps.push(("region_seeds",u64::from(words)*u64::from(window[3]),self.params(core,window,[0,5,0,0],paper::Grain::FLAT)));
        let nearest=self.jump_steps(&mut steps,core,window,"region_seeds",4,UNDERFILL_HALF_PIXELS.div_ceil(2));
        let mut params=self.params(core,window,[UNDERFILL_HALF_PIXELS,RIM_HALF_PIXELS,THIN_CUTOFF,nearest],paper::Grain::FLAT);params[25]=u32::from(fill);
        steps.push(("assign_region",word_count(core),params));
        steps
    }
    pub fn run(&self,input:&Input<'_>,encoder:&mut impl Record,steps:Vec<Step>)->f64 {
        self.encode(input,encoder,None,None,steps)
    }
    pub fn grow_steps(&self,value:i32)->Vec<Step> {
        let kind=if value<0 {3} else {2};
        let mut steps=vec![("hold_barriers",u64::from(self.offsets[0]),self.params([0;4],[0;4],[1,0,0,0],paper::Grain::FLAT))];
        for &(core,window) in &self.windows {
            let input=self.jump_steps(&mut steps,core,window,"region_seeds",kind,value.unsigned_abs());
            steps.push(("grow_region",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[value.unsigned_abs(),kind,0,input],paper::Grain::FLAT)));
        }
        steps
    }

    pub fn intersect(&self,input:&Input<'_>,encoder:&mut impl Record,stop:&wgpu::Buffer)->f64 {
        self.encode(input,encoder,Some(stop),None,vec![("intersect_bits",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],paper::Grain::FLAT))])
    }

    pub fn thin(&self,input:&Input<'_>,encoder:&mut impl Record,coverage:&wgpu::Buffer,plane:u32)->f64 {
        self.encode(input,encoder,Some(coverage),None,vec![("remember_thin",u64::from(self.offsets[0]),self.params([0;4],[0;4],[plane,0,0,0],paper::Grain::FLAT))])
    }

    pub fn barrier_plane(&self,input:&Input<'_>,encoder:&mut impl Record,coverage:&wgpu::Buffer,parents:&wgpu::Buffer,bucket:u32,mode:u32)->f64 {
        let params=self.params([0;4],[0;4],[mode,bucket,0,0],paper::Grain::FLAT);
        self.encode(input,encoder,Some(coverage),Some(parents),vec![("barrier_plane",u64::from(self.offsets[0]),params)])
    }

    pub fn boundary(&self,input:&Input<'_>,encoder:&mut impl Record,coverage:&wgpu::Buffer)->f64 {
        let dispatched=self.encode(input,encoder,Some(coverage),None,vec![
            ("reset_ends",1,self.params([0;4],[0;4],[2,0,0,0],paper::Grain::FLAT)),
            ("barrier_plane",u64::from(self.offsets[0]),self.params([0;4],[0;4],[3,0,THIN_CUTOFF,0],paper::Grain::FLAT)),
        ]);
        encoder.copy(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+3)*4,coverage,28,4);
        dispatched
    }
    pub fn union_trail(&self,input:&Input<'_>,encoder:&mut impl Record,trail:&wgpu::Buffer)->f64 {
        self.encode(input,encoder,Some(trail),None,vec![("barrier_plane",u64::from(self.offsets[0]),self.params([0;4],[0;4],[5,0,0,0],paper::Grain::FLAT))])
    }
    pub fn next_trail(&self,input:&Input<'_>,encoder:&mut impl Record,trail:&wgpu::Buffer,points:&[[f32;2]],limit:Option<&wgpu::Buffer>)->f64 {
        let mut count=0u32;let mut data=Vec::new();
        for segment in points.windows(2) {
            let steps=((segment[1][0]-segment[0][0]).abs().max((segment[1][1]-segment[0][1]).abs())*2.).ceil().max(1.) as u32;
            count=count.saturating_add(steps+1);
            data.extend([segment[0][0].to_bits(),segment[0][1].to_bits(),segment[1][0].to_bits(),segment[1][1].to_bits(),count,steps]);
        }
        if data.is_empty(){data.extend([0;6]);}
        let bytes=data.into_iter().flat_map(u32::to_ne_bytes).collect::<Vec<_>>();
        let path=input.device.create_buffer(&wgpu::BufferDescriptor {label:Some("fill input path"),size:bytes.len() as u64+limit.map_or(0,wgpu::Buffer::size),
            usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let input_path=input.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("fill input segments"),contents:&bytes,usage:wgpu::BufferUsages::COPY_SRC});
        encoder.copy(&input_path,0,&path,0,bytes.len() as u64);
        if let Some(limit)=limit {encoder.copy(limit,0,&path,bytes.len() as u64,limit.size());}
        self.encode(input,encoder,Some(trail),Some(&path),vec![
            ("reset_ends",1,self.params([0;4],[0;4],[2,0,1,0],paper::Grain::FLAT)),
            ("barrier_plane",u64::from(count.max(1)),self.params([bytes.len() as u32/4,u32::from(limit.is_some()),0,0],[0;4],[6,count,points.len().saturating_sub(1) as u32,THIN_CUTOFF],paper::Grain::FLAT)),
            ("barrier_plane",1,self.params([0;4],[0;4],[7,count,points.len().saturating_sub(1) as u32,0],paper::Grain::FLAT)),
        ])
    }

    #[allow(clippy::too_many_arguments)]
    pub fn check_seeds(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,old:&wgpu::Buffer,new:&wgpu::Buffer,position:[u32;2],tolerance:f32,seeded:bool) {
        encoder.clear_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+2)*4,Some(4));
        let mut params=self.params([0;4],[0;4],[position[0],position[1],THIN_CUTOFF,0],paper::Grain::FLAT);params[25]=u32::from(seeded);params[26]=tolerance.to_bits();
        self.encode(input,encoder,Some(old),Some(new),vec![("check_seeds",old.size()/4,params)]);
    }
    pub fn colour_mode(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,mode:layer_render::RegionColourMode,seed:Option<&wgpu::Buffer>) {
        self.encode(input,encoder,seed,None,vec![("check_seeds",1,self.params([0;4],[0;4],
            [u32::from(matches!(mode,layer_render::RegionColourMode::ColourOnSolid)),u32::from(seed.is_some()),0,1],paper::Grain::FLAT))]);
    }
    pub fn snap_seed(&self,input:&Input<'_>,encoder:&mut impl Record,parents:&wgpu::Buffer,seed:u32,position:[u32;2])->f64 {
        self.encode(input,encoder,Some(parents),None,vec![("snap_seed",1,self.params([0;4],[0;4],[position[0],position[1],seed,0],paper::Grain::FLAT))])
    }
    pub fn copy_colour(&self,encoder:&mut impl Record,output:&wgpu::Buffer,offset:u64) {
        encoder.copy(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+4)*4,output,offset,4);
    }
    pub fn copy_stale(&self,encoder:&mut crate::submission::CommandEncoder,output:&wgpu::Buffer,offset:u64) {
        encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+2)*4,output,offset,4);
    }

}

pub(super) fn analysis_window(core:[u32;4],extent:[u32;2])->[u32;4] {
    let x=core[0].saturating_sub(WINDOW_HALO);let y=core[1].saturating_sub(WINDOW_HALO);
    [x,y,(core[0]+core[2]+WINDOW_HALO).min(extent[0])-x,(core[1]+core[3]+WINDOW_HALO).min(extent[1])-y]
}
fn word_count(rect:[u32;4])->u64 {u64::from((rect[0]+rect[2]).div_ceil(32)-rect[0]/32)*u64::from(rect[3])}
fn bounds(rect:PixelRect)->[u32;4] {[rect.min_x(),rect.min_y(),rect.width(),rect.height()]}
fn rect(bounds:[u32;4])->PixelRect {PixelRect::new(bounds[0],bounds[1],bounds[0]+bounds[2],bounds[1]+bounds[3])}
fn changed_regions(pages:&std::collections::BTreeSet<[u32;2]>,halo:u32,extent:[u32;2])->Vec<PixelRect> {
    pages.iter().map(|&page|page_rect(page).intersect(PixelRect::full(extent)).expand(halo,extent)).filter(|region|!region.is_empty()).collect()
}

pub(super) fn fits(extent:[u32;2],limits:&wgpu::Limits)->bool {storage_layout(extent,limits).is_some()}
fn nanoseconds(entry:&str,params:&[u32],radius:u32)->f64 {
    match entry {
        "grain_bits"=>f64::from(radius*16+32)*0.1,"distance_seeds"|"store_distance"|"skeleton_seeds"=>10.,
        "jump_distance"=>if matches!(params[17],0|4) {640.}else{20.},"skeleton_ridges"=>120.,"skeleton_prune"=>150.,"line_ends"=>1000.,
        "refine_ends"=>10e3,"seal_candidates"=>f64::from(params[19]>>2)*SEAL_STEP_NANOSECONDS,"apply_seals"=>128.,"assign_region"=>64.,_=>2.,
    }
}
fn plan_chunk(queue:&mut std::collections::VecDeque<Step>,budget:f64,radius:u32)->(Vec<Step>,f64) {
    let mut steps=Vec::new();let mut spent=0.;
    while let Some((entry,remaining,mut params))=queue.pop_front_if(|(entry,_,_)|*entry!="seal_candidates") {
        let ends=entry=="refine_ends";
        let cost=nanoseconds(entry,&params,radius);
        let group=if ends {1} else {64};
        let fits=((budget-spent).max(0.)/cost) as u64/group*group;
        if fits==0 && !steps.is_empty() {queue.push_front((entry,remaining,params));break;}
        let count=fits.max(group).min(remaining);
        let mut planned=params.clone();
        if ends {planned[18]=params[24]+count as u32;}
        steps.push((entry,count,planned));spent+=count as f64*cost;
        if count<remaining {params[24]+=count as u32;queue.push_front((entry,remaining-count,params));}
        if spent>=budget {break;}
    }
    (steps,spent)
}
fn storage_layout(extent:[u32;2],limits:&wgpu::Limits)->Option<([u32;4],u64)> {
    let [w,h]=extent;if w==0 || h==0 || w>=65535 || h>=65535 {return None;}
        let bits=u64::from(w.div_ceil(32))*u64::from(h);
        let distance=u64::from(w.div_ceil(4))*u64::from(h);
        let summary=bits+distance;
        let sparse=summary+u64::from(HEADER_WORDS)+u64::from(w.div_ceil(128))*u64::from(h.div_ceil(128));
        let dense=(sparse+bits*2)*4;
        let area=u64::from(w)*u64::from(h);
        let capacity=area.div_ceil(PIXELS_PER_END).max(MIN_END_CAPACITY).min(area)
            .min(limits.max_storage_buffer_binding_size.min(limits.max_buffer_size).saturating_sub(dense)/(END_WORDS*4));
        let size=dense+capacity*END_WORDS*4;
        if capacity==0 || size>limits.max_storage_buffer_binding_size || size>limits.max_buffer_size || w>=65535 || h>=65535 {return None;}
    Some(([bits as u32,summary as u32,sparse as u32,capacity as u32],size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn cache_build_requests_tail_pipelines_before_reading_the_end_count() {
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        for before_build in [true,false] {
            let compiler=r.device.compilation().unwrap();
            let mut lines=Lines::new(&r.device);
            pollster::block_on(compiler.prepare_all(lines.cache_pipelines())).unwrap();
            let held=compiler.batch();
            lines.allocate(&r.device,[64;2],64,&[(0,[0,0,64,64])]).unwrap();
            let mask=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label:Some("open reference"),contents:&[255;512],usage:wgpu::BufferUsages::STORAGE,
            });
            let input=Input {device:&r.device,mask:&mask,paper:&r.empty_paper_view};
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            if before_build {assert!(lines.prepare_cache(&compiler,startup::BRUSH));}
            assert!(!lines.build(&input,&mut encoder,(paper::Grain::FLAT,1,false),1.,true,&Default::default(),(true,true)).unwrap().0);
            assert!(!lines.building.as_ref().unwrap().ranked);
            assert!(lines.prepare_cache(&compiler,startup::BRUSH));
            for entry in ["refine_ends","seal_candidates"] {
                assert!(!lines.pipelines[entry].ready());
                assert!(compile::tests::requested(&compiler,&lines.pipelines[entry],startup::BRUSH),"{entry} was not requested");
            }
            drop(held);
            r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
        }
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn every_line_chunk_fits_the_chunk_time() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let limits=r.device.limits();
        for extent in [[4248,2832],[9504,6336]] {
            let labels=super::super::Labels::new(extent,&limits).unwrap();
            let windows:Vec<_>=labels.windows(extent).collect();
            let mut lines=Lines::new(&r.device);
            lines.layout(extent,&limits,&windows).unwrap();
            let ranking=("refine_ends",u64::from(lines.offsets[3]),lines.params([0;4],[0;4],[THIN_CUTOFF,0,0,0],paper::Grain::FLAT));
            let mut queue:std::collections::VecDeque<Step>=[lines.grain_steps(paper::Grain::FLAT,1,false,&Default::default()),lines.scope_steps(paper::Grain::FLAT,&Built::Full),vec![ranking]].concat().into();
            let mut planned=0.;
            while !queue.is_empty() {
                let (steps,spent)=plan_chunk(&mut queue,CHUNK_NANOSECONDS,1);
                assert!(!steps.is_empty());
                assert!(spent<=CHUNK_NANOSECONDS || steps.len()==1 && steps[0].1<=64,"{extent:?}: one line chunk plans {:.1} ms",spent*1e-6);
                planned+=spent;
            }
            assert!(planned>10.*CHUNK_NANOSECONDS,"{extent:?}: {planned:e}");
        }
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn seal_rounds_bound_each_search_and_fit_the_chunk_time() {
        let budget=CHUNK_NANOSECONDS/SEAL_STEP_NANOSECONDS;
        for active in [5440,1024,64,1] {
            let mut params=vec![0;28];params[18]=5440;
            let (steps,taps)=Sealing {params,active,started:false,unfinished:None}.rounds(budget);
            let flags:Vec<_>=steps.iter().map(|(_,_,params)|params[19]&3).collect();
            assert!(flags[0]&1!=0 && flags[1..].iter().all(|flag|flag&1==0) && flags[..flags.len()-1].iter().all(|flag|flag&2==0) && flags[flags.len()-1]&2!=0,"{flags:?}");
            assert!(steps.iter().all(|(_,count,params)|*count==5440 && f64::from(params[19]>>2)<=SEAL_ROUND_WORK[1]),"a round searches past its bound");
            assert!(taps<=budget*1.0001,"{} rounds for {active} searching ends take {:.1} ms",steps.len(),taps*SEAL_STEP_NANOSECONDS*1e-6);
            assert!(taps>=budget*0.5 || f64::from(steps[0].2[19]>>2)==SEAL_ROUND_WORK[1] && steps.len() as u64==MAX_SEAL_ROUNDS,"{} rounds leave a chunk half idle",steps.len());
        }
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn seal_searches_cut_at_each_rounds_work_resume_to_the_same_seals() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let extent=[512u32,512];
        let labels=super::super::Labels::new(extent,&r.device.limits()).unwrap();
        let windows:Vec<_>=labels.windows(extent).collect();
        let bits:Vec<u32>=(0..extent[1]).flat_map(|y|(0..extent[0].div_ceil(32)).map(move |word| (0..32).fold(u32::MAX,|bits,bit| {
            let x=(word*32+bit) as i32;let y=y as i32;
            if (x+y)%10<2 && (x-y).rem_euclid(30)<20 {bits&!(1<<bit)}else{bits}
        }))).collect();
        let mask=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("dashes"),contents:&bits.iter().flat_map(|word|word.to_ne_bytes()).collect::<Vec<_>>(),usage:wgpu::BufferUsages::STORAGE});
        let compiler=r.device.compilation().unwrap();
        let seals=|budget:f64| {
            let mut lines=Lines::new(&r.device);
            pollster::block_on(compiler.prepare_all(lines.pipelines.values())).unwrap();
            lines.allocate(&r.device,extent,labels.side,&windows).unwrap();
            let input=Input {device:&r.device,mask:&mask,paper:&r.empty_paper_view};
            let mut rounds=0;
            loop {
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                let ready=lines.build(&input,&mut encoder,(paper::Grain::FLAT,1,false),budget,true,&Default::default(),(true,true)).unwrap().0;
                rounds+=lines.building.as_ref().and_then(|work|work.sealing.as_ref()).map_or(0,|sealing|u32::from(sealing.started));
                encoder.submit(&r.queue);crate::test_support::complete(&r);
                if ready {break;}
            }
            let words:Vec<u32>=crate::test_support::buffer_bytes(&r,lines.data.as_ref().unwrap()).as_chunks::<4>().0.iter().map(|word|u32::from_ne_bytes(*word)).collect();
            let ends=&words[lines.offsets[2] as usize..][..lines.ends as usize*END_WORDS as usize];
            let mut seals:Vec<Vec<u32>>=ends.chunks(END_WORDS as usize).map(|end|[0,4,5,6,8,9,10,12,13,14,16,17,18].map(|word|end[word]).to_vec()).collect();
            seals.sort();
            (seals,rounds,lines.ends)
        };
        let (cut,rounds,ends)=seals(65_536.);
        let (whole,_,_)=seals(f64::INFINITY);
        assert!(ends>500 && rounds>=8,"{ends} line ends finished their seal searches in {rounds} submissions of the least round work");
        assert!(cut.iter().any(|seal|seal[1]!=u32::MAX),"no end found a seal");
        assert!(cut==whole,"seal searches cut at each round's work found other seals");
    }


    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn region_line_function_arrays_fit_small_gpu_stacks() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let source=compose_wgsl(&[&working_color::shader(&r.device),include_str!("region_lines.wgsl")]);
        let module=naga::front::wgsl::parse_str(&source).unwrap();
        let mut sizes=naga::proc::Layouter::default();sizes.update(module.to_ctx()).unwrap();
        let functions=module.functions.iter().map(|(_,function)|function).chain(module.entry_points.iter().map(|entry|&entry.function));
        let largest=functions.flat_map(|function|function.local_variables.iter().map(|(_,local)|sizes[local.ty].size)).max().unwrap();
        assert!(largest<=512,"a {largest}-byte function array does not fit the A10 GPU stack");
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn seal_searches_submit_one_frontier_dispatch_at_a_time_at_61mp() {
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut lines=Lines::new(&r.device);
        pollster::block_on(r.device.compilation().unwrap().prepare_all(["refine_ends","seal_candidates"].map(|entry|&lines.pipelines[entry]))).unwrap();
        lines.allocate(&r.device,[9504,6336],64,&[(0,[0,0,64,64])]).unwrap();
        let slots=lines.frontier.as_ref().unwrap().size()/SLOT_BYTES;
        assert!(slots*SLOT_BYTES<=16<<20);
        let mask=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("open reference"),size:u64::from(lines.offsets[0])*4,
            usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false});
        let input=Input {device:&r.device,mask:&mask,paper:&r.empty_paper_view};
        let pending=|lines:&Lines|lines.building.as_ref().map_or(0,|work|work.steps.iter().filter(|(entry,_,_)|*entry=="seal_candidates").map(|(_,count,_)|*count).sum::<u64>());
        for ends in [26932,lines.offsets[3]] {
            let (tx,rx)=mpsc::channel();tx.send(Ok([ends,0])).unwrap();
            lines.building=Some(Build {ends:Some(rx),..Build::new(Vec::new(),Built::Full,paper::Grain::FLAT)});
            let mut left=u64::from(ends);
            for _ in 0..=u64::from(ends).div_ceil(slots) {
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                let ready=lines.advance(&input,&mut encoder,1,f64::INFINITY,true).unwrap().0;
                r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
                let searched=left-pending(&lines);
                assert!(searched<=slots,"{searched} of {ends} seal searches share one submission; one frontier holds {slots}");
                left-=searched;
                if ready {break;}
            }
            assert_eq!((left,lines.building.is_none()),(0,true));
        }
    }

    #[test]
    fn end_storage_scales_with_canvas_area_and_fits_61mp() {
        let limits=wgpu::Limits {max_storage_buffer_binding_size:128*1024*1024,max_buffer_size:128*1024*1024,..Default::default()};
        for (extent,ends,bytes) in [([4248,2832],46994,20312880),([6000,4000],93750,40530040),([9504,6336],235224,101631792)] {
            let (offsets,size)=storage_layout(extent,&limits).unwrap();
            assert_eq!((offsets[3],size),(ends,bytes));
            assert!(size<=limits.max_storage_buffer_binding_size);
        }
        assert!(storage_layout([9504,6336],&wgpu::Limits {max_storage_buffer_binding_size:64,..limits}).is_none());
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn turned_paper_grain_finds_valleys_at_document_points_and_one_true_period_away() {
        use crate::layer_tests::paper_shade::{TURNS,turned_surface,periods};
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut lines=Lines::new(&r.device);
        pollster::block_on(r.device.compilation().unwrap().prepare_all([&lines.pipelines["grain_bits"]])).unwrap();
        lines.allocate(&r.device,[256;2],256,&[(0,[0,0,256,256])]).unwrap();
        let hole=|x:u32,y:u32|x%5==0 && y%5==0;
        let words:Vec<u32>=(0..256u32).flat_map(|y|(0..8u32).map(move|word|(0..32u32).filter(|bit|hole(word*32+bit,y)).fold(0,|bits,bit|bits|1<<bit))).collect();
        let mask=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("holes in ink"),
            contents:&words.iter().chain(std::iter::repeat(&0).take(lines.offsets[0] as usize-words.len())).flat_map(|word|word.to_ne_bytes()).collect::<Vec<_>>(),usage:wgpu::BufferUsages::STORAGE});
        for turn in TURNS {
            let mut valleys=|target_offset:[i64;2]| {
                let binding=layer_core::paper::PaperBinding {surface:Arc::new(turned_surface(turn)),target_offset};
                r.prepare_paper_binding(&binding,startup::BRUSH).unwrap();
                let request=layer_render::RegionRequest {fill_areas:None,colour_mode:Default::default(),fill:None,reference_count:None,paper:Some(binding),
                    enclosure:None,contiguous:true,selection:None,request_id:0,source:layer_render::RegionSource::Composite,position:[0;2],
                    tolerance:0.,refinement:Default::default(),limit:None};
                let (view,grain,_)=crate::region_requests::RegionRequests::paper(&r,&request).unwrap();
                let input=Input {device:&r.device,mask:&mask,paper:&view};
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                encoder.clear_buffer(lines.data.as_ref().unwrap(),0,None);
                lines.encode(&input,&mut encoder,None,None,lines.grain_steps(grain,1,true,&Default::default()));
                r.uploads.finish(&encoder);encoder.submit(&r.queue);crate::test_support::complete(&r);
                let bytes=pollster::block_on(crate::local_tone::read_buffer_async(&r.device,&r.queue,lines.data.as_ref().unwrap())).unwrap();
                (0..256*256).map(|i:i32|u32::from_ne_bytes(bytes[((i/256*8+i%256/32)*4) as usize..((i/256*8+i%256/32)*4+4) as usize].try_into().unwrap())>>(i%256%32)&1==1).collect::<Vec<_>>()
            };
            let moved=valleys([1,0]);
            let grain=|x:i32,y:i32|moved[(y*256+x) as usize];
            let count=(0..256).flat_map(|y|(0..256).map(move|x|(x,y))).filter(|&(x,y)|hole(x as u32,y as u32) && grain(x,y)).count();
            assert!(count>50 && count<2500,"paper turned by {turn:?} must make some holes valleys: {count}");
            assert!(moved==valleys([0,0]),"Enclose Fill on a moved layer must find the valleys of paper turned by {turn:?} at the document points brushes and fills use");
            for shift in periods(turn,1.) {
                let (mut differ,mut compared)=(0,0);
                for y in 0..256 {for x in 0..256 {
                    let [u,v]=[x+shift[0],y+shift[1]];
                    if hole(x as u32,y as u32) && [x,y,u,v].iter().all(|c|(1..255).contains(c)) {compared+=1;differ+=usize::from(grain(x,y)!=grain(u,v));}
                }}
                assert!(differ*100<=compared,"paper turned by {turn:?} must find the same valleys one period {shift:?} away: {differ} of {compared} differ");
            }
        }
    }
}
