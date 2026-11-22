use super::*;
use wgpu::util::DeviceExt;

const PIXELS_PER_END:u64=256;
const MIN_END_CAPACITY:u64=16384;
const END_WORDS:u64=20;
const FRONTIER_NODES:u64=256;
const VISIT_ENTRIES:u64=512;
const SLOT_BYTES:u64=FRONTIER_NODES*8+VISIT_ENTRIES*2;
const SEAL_SLOTS:u64=(16<<20)/SLOT_BYTES/64*64;
const HEADER_WORDS:u32=5;
const GAP_REACH:u32=64;
pub(super) const WINDOW_HALO:u32=GAP_REACH+2*MAX_LINE_WIDTH;
const MAX_LINE_WIDTH:u32=16;
const THIN_CUTOFF:u32=MAX_LINE_WIDTH*3/2;
const RIM_HALF_PIXELS:u32=3;
pub(crate) const UNDERFILL_HALF_PIXELS:u32=48;
const DISTANCE_LIMIT_HALF_PIXELS:u32=UNDERFILL_HALF_PIXELS+1;
const DISTANCE_HALO:u32=64;
const SKELETON_HALO:u32=1+32+2*(32+63+1);
const END_HALO:u32=33;
const FIT_DISPLACEMENT:u32=24;
const NEIGHBOUR_REACH:u32=64;
const PARAMS: u64 = 112;
const PARAMETER_BATCH_BYTES:u64=64*1024;
const MAX_CHUNK_TAPS:f64=5e8;
pub(super) const SCOPE_BLOCK:u32=128;
const SCOPE_BLOCKS:i64=3;
type Step = (&'static str,u64,Vec<u32>);
type Readback<T> = Option<mpsc::Receiver<Result<T,GpuRasterError>>>;
const SNAP_AREA:usize=33*33;
const MIDDLE_CLIMB:u32=16;
pub(crate) struct ScopeProbe {pub stale:u32, pub flagged:Vec<[u32;2]>, region:Vec<u32>}
#[derive(Clone, PartialEq, Debug)]
enum Built {Grain,Blocks(std::collections::BTreeSet<[u32;2]>),Full}
struct Build { steps:std::collections::VecDeque<Step>, submission:Option<crate::submission::Submission>,in_flight:std::sync::Arc<std::sync::atomic::AtomicU32>, ends:Readback<[u32;2]>, ranked:bool, incremental:bool,
    open:Readback<u32>, reads_open:bool, lines:bool, target:Built, grain:[f32;4], scoped:bool }
impl Build {
    fn new(steps:Vec<Step>,target:Built,grain:[f32;4])->Self {
        Self {steps:steps.into(),submission:None,in_flight:Default::default(),ends:None,ranked:false,incremental:false,open:None,reads_open:false,lines:false,target,grain,scoped:false}
    }
    fn available(&self,demand:bool)->bool {
        let count=self.in_flight.load(std::sync::atomic::Ordering::Acquire);
        self.submission.as_ref().is_none_or(|owner|owner.submitted())
            && (self.ends.is_none() && self.open.is_none() || count==0)
            && (count<u32::from(demand)+1 || demand && self.ranked && self.steps.is_empty())
    }
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
    fn cache_pipelines(&self)->impl Iterator<Item=&Deferred<wgpu::ComputePipeline>> {
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
    pub fn prepare_areas(&self,compiler:&compile::Scope,priority:startup::Class)->bool {
        self.prepare_cache(compiler,priority)
            & compiler.require(["open_bits","apply_seals","hold_barriers","reset_ends","region_seeds","jump_distance","assign_tokens"].map(|entry|&self.pipelines[entry]),priority)
            & compiler.require([&self.colour],priority)
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
        self.encode(input,encoder,None,None,vec![("hold_barriers",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],[1.,1.,0.,0.]))]);
    }
    pub fn assign_tokens(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,core:[u32;4],window:[u32;4]) {
        let mut steps=vec![("reset_ends",1,self.params(core,window,[2,0,0,0],[1.,1.,0.,0.]))];
        let words=(window[0]+window[2]).div_ceil(32)-window[0]/32;
        steps.push(("region_seeds",u64::from(words)*u64::from(window[3]),self.params(core,window,[0,5,0,0],[1.,1.,0.,0.])));
        let nearest=self.jump_steps(&mut steps,core,window,"region_seeds",4,UNDERFILL_HALF_PIXELS.div_ceil(2));
        self.encode(input,encoder,None,None,steps);
        let scratch=self.scratch.as_ref().unwrap();let tokens=self.token_buffer();
        let (first,second)=if nearest==0 {(&scratch[0],tokens)}else{(tokens,&scratch[1])};
        self.encode(input,encoder,Some(first),Some(second),vec![("assign_tokens",word_count(core),self.params(core,window,[UNDERFILL_HALF_PIXELS,4,0,nearest],[1.,1.,0.,0.]))]);
    }
    pub fn colour_tokens(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,core:[u32;4],window:[u32;4],page:[u32;2],output:&wgpu::TextureView) {
        let mut params=self.params(core,window,[0;4],[1.,1.,0.,0.]);
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
    fn params(&self,core:[u32;4],window:[u32;4],options:[u32;4],paper:[f32;4])->Vec<u32> {
        let [w,h]=self.extent;
        [vec![w,h,w.div_ceil(32),w.div_ceil(4)],window.to_vec(),core.to_vec(),self.offsets.to_vec(),options.to_vec(),paper.map(f32::to_bits).to_vec(),vec![0;4]].concat()
    }
    fn encode(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,first:Option<&wgpu::Buffer>,second:Option<&wgpu::Buffer>,steps:Vec<Step>) {
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
            let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("region line stages"),timestamp_writes:None});
            for (i,(entry,count,_)) in steps.iter().enumerate() {
                if *count==0 {continue;}
                pass.set_pipeline(&self.pipelines[entry]);pass.set_bind_group(0,&group,&[(i as u64*stride) as u32]);
                let groups=count.div_ceil(64) as u32;let maximum=device.limits().max_compute_workgroups_per_dimension;
                let rows=groups.div_ceil(maximum);
                pass.dispatch_workgroups(groups.div_ceil(rows),rows,1);
            }
        }
    }
    fn jump_steps(&self,steps:&mut Vec<Step>,core:[u32;4],window:[u32;4],seeds:&'static str,kind:u32,reach:u32)->u32 {
        let count=u64::from(window[2])*u64::from(window[3]);
        steps.push((seeds,count,self.params(core,window,[0,kind,0,0],[1.,1.,0.,0.])));
        let mut input=0;
        for step in (0..=reach.max(1).next_power_of_two().ilog2()).rev().map(|n|1<<n).chain([1]) {
            steps.push(("jump_distance",if kind==0 || kind==4 {word_count(window)}else{count},self.params(core,window,[0,kind,step,input],[1.,1.,0.,0.])));input^=1;
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
    fn grain_steps(&self,grain:[f32;4],radius:u32,has_paper:bool,dirty:&std::collections::BTreeSet<[u32;2]>)->Vec<Step> {
        let windows=if dirty.is_empty() {self.windows.clone()}else{self.windows_in(&changed_regions(dirty,radius.max(1),self.extent),0)};
        windows.into_iter().map(|(core,window)|("grain_bits",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[radius,u32::from(has_paper),0,0],grain))).collect()
    }
    fn line_steps(&self,grain:[f32;4],radius:u32,dirty:&std::collections::BTreeSet<[u32;2]>)->Vec<Step> {
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
    fn distance_steps(&self,steps:&mut Vec<Step>,windows:Vec<([u32;4],[u32;4])>,grain:[f32;4]) {
        for (core,window) in windows {
            let input=self.jump_steps(steps,core,window,"distance_seeds",0,DISTANCE_LIMIT_HALF_PIXELS.div_ceil(2));
            steps.push(("store_distance",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[DISTANCE_LIMIT_HALF_PIXELS,0,0,input],grain)));
        }
    }
    fn clear_ends(&self,grain:[f32;4])->Step {("reset_ends",u64::from(self.offsets[2]-self.offsets[1]-HEADER_WORDS).max(1),self.params([0;4],[0;4],[0;4],grain))}
    fn scope_steps(&self,grain:[f32;4],target:&Built)->Vec<Step> {
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
    fn end_steps(&self,steps:&mut Vec<Step>,core:[u32;4],window:[u32;4],grain:[f32;4]) {
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
            let open=match rx.try_recv() {Ok(result)=>result?,Err(mpsc::TryRecvError::Empty)=>{self.building=Some(work);return Ok(false);},Err(error)=>return Err(GpuRasterError::MapFailed(error.to_string()))};
            self.open=open!=0 || work.incremental && self.open;work.open=None;
            if work.lines && self.open {work.steps=self.scope_steps(work.grain,&Built::Full).into();work.target=Built::Full;}else{work.ranked=true;}
        }
        if let Some(rx)=&work.ends {
            let metadata=match rx.try_recv() {Ok(result)=>result?,Err(mpsc::TryRecvError::Empty)=>{self.building=Some(work);return Ok(false);},Err(error)=>return Err(GpuRasterError::MapFailed(error.to_string()))};
            self.overflow=metadata[1]!=0;self.ends=metadata[0].min(self.offsets[3]);work.ends=None;work.ranked=true;
            if !self.overflow && self.ends>0 {for entry in ["refine_ends","seal_candidates"] {work.steps.push_back((entry,u64::from(self.ends),self.params([0;4],[0;4],[THIN_CUTOFF,u32::from(work.incremental),0,0],[1.,1.,0.,0.])));}}
        }
        self.building=Some(work);Ok(true)
    }
    pub fn submitted(&self)->bool {self.building.as_ref().is_none_or(|work|work.submission.as_ref().is_none_or(|owner|owner.submitted()))}
    pub fn discarded(&self)->bool {self.building.as_ref().is_some_and(|work|work.submission.as_ref().is_some_and(|owner|owner.discarded()))}
    pub fn cancel(&mut self) {self.building=None;}
    #[allow(clippy::too_many_arguments)]
    pub fn build(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,paper:([f32;4],u32,bool),budget:f64,demand:bool,dirty:&std::collections::BTreeSet<[u32;2]>,(lines,reads_open):(bool,bool))->Result<(bool,f64),GpuRasterError> {
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
    pub fn build_scope(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,paper:([f32;4],u32),budget:f64,blocks:Option<std::collections::BTreeSet<[u32;2]>>)->Result<bool,GpuRasterError> {
        let target=blocks.map_or(Built::Full,Built::Blocks);
        if self.covers(&target) {return Ok(true);}
        if self.building.as_ref().is_some_and(|work|work.target!=target) {self.building=None;}
        if self.building.is_none() {self.building=Some(Build {scoped:true,..Build::new(self.scope_steps(paper.0,&target),target.clone(),paper.0)});}
        Ok(self.advance(input,encoder,paper.1,budget,true)?.0 && self.covers(&target))
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
    pub fn tap_distance(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,tap:[u32;2],grain:[f32;4]) {
        let reach=MIDDLE_CLIMB+1;
        let around=PixelRect::new(tap[0].saturating_sub(reach),tap[1].saturating_sub(reach),(tap[0]+reach+1).min(self.extent[0]),(tap[1]+reach+1).min(self.extent[1]));
        let mut steps=Vec::new();
        self.distance_steps(&mut steps,self.windows_in(&[around],DISTANCE_HALO),grain);
        self.encode(input,encoder,None,None,steps);
    }
    pub fn probe_scope(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,tap:[u32;2])->mpsc::Receiver<Result<ScopeProbe,GpuRasterError>> {
        let columns=self.extent[0].div_ceil(SCOPE_BLOCK);let blocks=columns as usize*self.extent[1].div_ceil(SCOPE_BLOCK) as usize;
        let scope=&self.scratch.as_ref().unwrap()[0];let bytes=((1+blocks+SNAP_AREA)*4) as u64;
        encoder.clear_buffer(scope,0,Some(bytes));
        self.encode(input,encoder,None,None,vec![("scope_region",u64::from(self.offsets[0]),self.params([0;4],[0;4],[tap[0],tap[1],0,0],[1.,1.,0.,0.]))]);
        let readback=input.device.create_buffer(&wgpu::BufferDescriptor {label:Some("region scope"),size:bytes,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
        encoder.copy_buffer_to_buffer(scope,0,&readback,0,bytes);
        let (tx,rx)=mpsc::channel();
        crate::raster::map_then(input.device,Some(encoder),&readback,bytes,move|bytes|{
            let words:Vec<u32>=bytes.as_chunks::<4>().0.iter().map(|word|u32::from_le_bytes(*word)).collect();
            let flagged=words[1..1+blocks].iter().enumerate().filter(|(_,flag)|**flag!=0).map(|(index,_)|[index as u32%columns,index as u32/columns]).collect();
            Ok(ScopeProbe {stale:words[0],flagged,region:words[1+blocks..].to_vec()})
        },move|result|{let _=tx.send(result);});
        rx
    }
    pub fn read_stale(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder)->mpsc::Receiver<Result<u32,GpuRasterError>> {
        let readback=device.create_buffer(&wgpu::BufferDescriptor {label:Some("region seed check"),size:4,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
        encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+2)*4,&readback,0,4);
        let (tx,rx)=mpsc::channel();
        crate::raster::map_then(device,Some(encoder),&readback,4,|bytes|Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap())),move|result|{let _=tx.send(result);});
        rx
    }
    pub fn check_snap(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,tap:[u32;2],probe:&ScopeProbe,stop:Option<&wgpu::Buffer>) {
        let marks=input.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("region snap marks"),
            contents:&[vec![0;4*SNAP_AREA],probe.region.clone()].concat().iter().flat_map(|word|word.to_ne_bytes()).collect::<Vec<_>>(),usage:wgpu::BufferUsages::STORAGE});
        let checks=[0,1].map(|mode|("scope_snap",if mode==0 {u64::from(self.ends)}else{1},self.params([0;4],[0;4],[tap[0],tap[1],u32::from(stop.is_some()),mode],[1.,1.,0.,0.])));
        self.encode(input,encoder,stop,Some(&marks),checks.to_vec());
    }
    fn advance(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,radius:u32,budget:f64,demand:bool)->Result<(bool,f64),GpuRasterError> {
        let device=input.device;
        use std::sync::atomic::Ordering;
        if !self.ready(demand)? {return Ok((false,0.));}
        let mut work=self.building.take().unwrap();
        if work.steps.is_empty() && work.ranked {self.built=work.target;return Ok((true,0.));}
        let (steps,spent)=plan_chunk(&mut work.steps,budget,radius,self.frontier.as_ref().unwrap().size()/SLOT_BYTES);
        let mut encoded=!steps.is_empty();
        for run in steps.chunk_by(|a,b|(a.0=="seal_candidates")==(b.0=="seal_candidates")) {
            self.encode(input,encoder,None,(run[0].0=="seal_candidates").then(||self.frontier.as_ref().unwrap()),run.to_vec());
        }
        if work.steps.is_empty() && work.reads_open {
            let readback=device.create_buffer(&wgpu::BufferDescriptor {label:Some("region open pixels"),size:4,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
            encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+2)*4,&readback,0,4);
            let (tx,rx)=mpsc::channel();
            crate::raster::map_then(device,Some(encoder),&readback,4,|bytes|Ok(u32::from_le_bytes(bytes[..4].try_into().unwrap())),move|result|{let _=tx.send(result);});
            work.open=Some(rx);work.reads_open=false;encoded=true;
        } else if work.steps.is_empty() && !work.ranked {
            let readback=device.create_buffer(&wgpu::BufferDescriptor {label:Some("region line end count"),size:8,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
            encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1])*4,&readback,0,8);
            let (tx,rx)=mpsc::channel();
            crate::raster::map_then(device,Some(encoder),&readback,8,|bytes|Ok(std::array::from_fn(|i|u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap()))),move|result|{let _=tx.send(result);});
            work.ends=Some(rx);encoded=true;
        }
        if encoded {
            work.submission=Some(encoder.submission());work.in_flight.fetch_add(1,Ordering::AcqRel);
            let in_flight=crate::InFlightCompletion(work.in_flight.clone());encoder.hold(in_flight);
        }
        if !encoded && work.steps.is_empty() && work.ranked {self.built=work.target;return Ok((true,spent));}
        self.building=Some(work);Ok((false,spent))
    }
    pub fn has_seals(&self)->bool {!self.overflow && self.ends!=0}
    pub fn exclude(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,excluded:&[u32]) {
        if !self.has_seals(){return;}
        let mut ids=excluded.to_vec();ids.sort_unstable();ids.dedup();
        let bytes=ids.iter().flat_map(|id|id.to_ne_bytes()).collect::<Vec<_>>();
        let buffer=(!ids.is_empty()).then(||input.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label:Some("removed fill seals"),contents:&bytes,usage:wgpu::BufferUsages::STORAGE}));
        self.encode(input,encoder,None,buffer.as_ref(),vec![("reset_ends",u64::from(self.ends),
            self.params([0;4],[0;4],[8,self.ends,0,ids.len() as u32],[1.,1.,0.,0.]))]);
    }
    pub fn seal_metadata(&self,device:&PipelineDevice)->Option<wgpu::Buffer> {
        self.has_seals().then(||device.create_buffer(&wgpu::BufferDescriptor {label:Some("fill seal metadata"),
            size:u64::from(self.ends)*20,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false}))
    }
    pub fn capture_seals(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,output:&wgpu::Buffer,parents:&wgpu::Buffer,slot:u32) {
        self.encode(input,encoder,Some(output),Some(parents),vec![("apply_seals",u64::from(self.ends),
            self.params([0;4],[0;4],[0,slot,0,1],[1.,1.,0.,0.]))]);
    }

    pub fn barriers(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,close:bool,reach:u32,job:[u32;4],parents:&wgpu::Buffer) {
        let params=|mut steps:Vec<Step>| {
            for (_,_,params) in &mut steps {params[24..28].copy_from_slice(&job);}
            steps
        };
        self.encode(input,encoder,Some(parents),None,params(vec![("open_bits",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],[1.,1.,0.,0.]))]));
        if close && self.has_seals() {
            self.encode(input,encoder,Some(parents),None,params(vec![("apply_seals",u64::from(self.ends),self.params([0;4],[0;4],[0,0,reach/16-1,0],[1.,1.,0.,0.]))]));
        }
        self.encode(input,encoder,Some(parents),None,params(vec![("hold_barriers",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],[1.,1.,0.,0.]))]));
    }

    pub fn assign(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,fill:bool,included:Option<&[bool]>) {
        let mut steps=Vec::new();
        for (at,&(core,window)) in self.windows.iter().enumerate() {
            if included.is_some_and(|included|!included[at]) {continue;}
            steps.push(("reset_ends",1,self.params(core,window,[2,0,0,0],[1.,1.,0.,0.])));
            let words=(window[0]+window[2]).div_ceil(32)-window[0]/32;
            steps.push(("region_seeds",u64::from(words)*u64::from(window[3]),self.params(core,window,[0,5,0,0],[1.,1.,0.,0.])));
            let nearest=self.jump_steps(&mut steps,core,window,"region_seeds",4,UNDERFILL_HALF_PIXELS.div_ceil(2));
            let mut params=self.params(core,window,[UNDERFILL_HALF_PIXELS,RIM_HALF_PIXELS,THIN_CUTOFF,nearest],[1.,1.,0.,0.]);params[25]=u32::from(fill);
            steps.push(("assign_region",word_count(core),params));
        }
        self.encode(input,encoder,None,None,steps);
    }
    pub fn grow(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,value:i32) {
        let kind=if value<0 {3} else {2};
        let mut steps=vec![("hold_barriers",u64::from(self.offsets[0]),self.params([0;4],[0;4],[1,0,0,0],[1.,1.,0.,0.]))];
        for &(core,window) in &self.windows {
            let input=self.jump_steps(&mut steps,core,window,"region_seeds",kind,value.unsigned_abs());
            steps.push(("grow_region",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[value.unsigned_abs(),kind,0,input],[1.,1.,0.,0.])));
        }
        self.encode(input,encoder,None,None,steps);
    }

    pub fn intersect(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,stop:&wgpu::Buffer) {
        self.encode(input,encoder,Some(stop),None,vec![("intersect_bits",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],[1.,1.,0.,0.]))]);
    }

    pub fn thin(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,coverage:&wgpu::Buffer,plane:u32) {
        self.encode(input,encoder,Some(coverage),None,vec![("remember_thin",u64::from(self.offsets[0]),self.params([0;4],[0;4],[plane,0,0,0],[1.,1.,0.,0.]))]);
    }

    pub fn barrier_plane(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,coverage:&wgpu::Buffer,parents:&wgpu::Buffer,bucket:u32,mode:u32) {
        let params=self.params([0;4],[0;4],[mode,bucket,0,0],[1.,1.,0.,0.]);
        self.encode(input,encoder,Some(coverage),Some(parents),vec![("barrier_plane",u64::from(self.offsets[0]),params)]);
    }

    pub fn boundary(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,coverage:&wgpu::Buffer) {
        self.encode(input,encoder,Some(coverage),None,vec![
            ("reset_ends",1,self.params([0;4],[0;4],[2,0,0,0],[1.,1.,0.,0.])),
            ("barrier_plane",u64::from(self.offsets[0]),self.params([0;4],[0;4],[3,0,THIN_CUTOFF,0],[1.,1.,0.,0.])),
        ]);
        encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+3)*4,coverage,28,4);
    }
    pub fn union_trail(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,trail:&wgpu::Buffer) {
        self.encode(input,encoder,Some(trail),None,vec![("barrier_plane",u64::from(self.offsets[0]),self.params([0;4],[0;4],[5,0,0,0],[1.,1.,0.,0.]))]);
    }
    pub fn next_trail(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,trail:&wgpu::Buffer,points:&[[f32;2]],limit:Option<&wgpu::Buffer>) {
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
        encoder.copy_buffer_to_buffer(&input_path,0,&path,0,bytes.len() as u64);
        if let Some(limit)=limit {encoder.copy_buffer_to_buffer(limit,0,&path,bytes.len() as u64,limit.size());}
        self.encode(input,encoder,Some(trail),Some(&path),vec![
            ("reset_ends",1,self.params([0;4],[0;4],[2,0,1,0],[1.,1.,0.,0.])),
            ("barrier_plane",u64::from(count.max(1)),self.params([bytes.len() as u32/4,u32::from(limit.is_some()),0,0],[0;4],[6,count,points.len().saturating_sub(1) as u32,THIN_CUTOFF],[1.,1.,0.,0.])),
            ("barrier_plane",1,self.params([0;4],[0;4],[7,count,points.len().saturating_sub(1) as u32,0],[1.,1.,0.,0.])),
        ]);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn check_seeds(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,old:&wgpu::Buffer,new:&wgpu::Buffer,position:[u32;2],tolerance:f32,seeded:bool) {
        encoder.clear_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+2)*4,Some(4));
        let mut params=self.params([0;4],[0;4],[position[0],position[1],THIN_CUTOFF,0],[1.,1.,0.,0.]);params[25]=u32::from(seeded);params[26]=tolerance.to_bits();
        self.encode(input,encoder,Some(old),Some(new),vec![("check_seeds",old.size()/4,params)]);
    }
    pub fn colour_mode(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,mode:layer_render::RegionColourMode,seed:Option<&wgpu::Buffer>) {
        self.encode(input,encoder,seed,None,vec![("check_seeds",1,self.params([0;4],[0;4],
            [u32::from(matches!(mode,layer_render::RegionColourMode::ColourOnSolid)),u32::from(seed.is_some()),0,1],[1.,1.,0.,0.]))]);
    }
    pub fn snap_seed(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,parents:&wgpu::Buffer,seed:u32,position:[u32;2]) {
        self.encode(input,encoder,Some(parents),None,vec![("snap_seed",1,self.params([0;4],[0;4],[position[0],position[1],seed,0],[1.,1.,0.,0.]))]);
    }
    pub fn copy_colour(&self,encoder:&mut crate::submission::CommandEncoder,output:&wgpu::Buffer,offset:u64) {
        encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+4)*4,output,offset,4);
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
fn plan_chunk(queue:&mut std::collections::VecDeque<Step>,budget:f64,radius:u32,slots:u64)->(Vec<Step>,f64) {
    let budget=budget.min(MAX_CHUNK_TAPS);
    let mut steps=Vec::new();let mut spent=0.;
    while let Some((entry,remaining,mut params))=queue.pop_front() {
        let weight=match entry {"grain_bits"=>f64::from(radius*16+32),"jump_distance"=>9.,"store_distance"=>16.,"skeleton_ridges"=>32.,"skeleton_prune"=>if params[16]==0 {1.}else{8.},"line_ends"=>32.,"refine_ends"=>1024.,"seal_candidates"=>4096.,_=>1.};
        let sparse=entry=="skeleton_ridges" || entry=="jump_distance" && matches!(params[17],0|4);
        let weight=weight*if sparse {32.}else{1.};
        let fits=((budget-spent).max(0.)/weight) as u64/64*64;
        if fits==0 && !steps.is_empty() {queue.push_front((entry,remaining,params));break;}
        let count=fits.max(64).min(remaining).min(if entry=="seal_candidates" {slots}else{u64::MAX});
        steps.push((entry,count,params.clone()));spent+=count as f64*weight;
        if count<remaining {params[24]+=count as u32;queue.push_front((entry,remaining-count,params));}
        if entry=="seal_candidates" || spent>=budget {break;}
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
            assert!(!lines.build(&input,&mut encoder,([1.,1.,0.,0.],1,false),1.,true,&Default::default(),(true,true)).unwrap().0);
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
    fn line_build_chunks_stay_bounded_however_fast_the_gpu_measures() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let limits=r.device.limits();
        for extent in [[4248,2832],[9504,6336]] {
            let labels=super::super::Labels::new(extent,&limits).unwrap();
            let windows:Vec<_>=labels.windows(extent).collect();
            let mut lines=Lines::new(&r.device);
            lines.layout(extent,&limits,&windows).unwrap();
            let ranking=["refine_ends","seal_candidates"].map(|entry|(entry,u64::from(lines.offsets[3]),lines.params([0;4],[0;4],[THIN_CUTOFF,0,0,0],[1.,1.,0.,0.])));
            let mut queue:std::collections::VecDeque<Step>=[lines.grain_steps([1.,1.,0.,0.],1,false,&Default::default()),lines.scope_steps([1.,1.,0.,0.],&Built::Full),ranking.to_vec()].concat().into();
            let mut planned=0.;
            while !queue.is_empty() {
                let (steps,spent)=plan_chunk(&mut queue,f64::INFINITY,1,SEAL_SLOTS);
                assert!(!steps.is_empty());
                assert!(spent<=MAX_CHUNK_TAPS,"{extent:?}: one line chunk plans {spent:e} taps");
                planned+=spent;
            }
            assert!(planned>10.*MAX_CHUNK_TAPS,"{extent:?}: {planned:e}");
        }
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
            lines.building=Some(Build {ends:Some(rx),..Build::new(Vec::new(),Built::Full,[1.,1.,0.,0.])});
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
        for (extent,ends,bytes) in [([4248,2832],46994,20312876),([6000,4000],93750,40530036),([9504,6336],235224,101631788)] {
            let (offsets,size)=storage_layout(extent,&limits).unwrap();
            assert_eq!((offsets[3],size),(ends,bytes));
            assert!(size<=limits.max_storage_buffer_binding_size);
        }
        assert!(storage_layout([9504,6336],&wgpu::Limits {max_storage_buffer_binding_size:64,..limits}).is_none());
    }
}
