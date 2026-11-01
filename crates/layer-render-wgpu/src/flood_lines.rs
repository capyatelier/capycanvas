use super::*;
use wgpu::util::DeviceExt;

const ENDS_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LINE_WIDTH:u32=16;
const UNDERFILL_HALF_PIXELS:u32=48;
const DISTANCE_LIMIT_HALF_PIXELS:u32=UNDERFILL_HALF_PIXELS+1;
const PARAMS: u64 = 112;
type Step = (&'static str,u64,Vec<u32>);
struct Build { steps:std::collections::VecDeque<Step>, submission:Option<crate::submission::Submission>,in_flight:std::sync::Arc<std::sync::atomic::AtomicU32>, ends:Option<mpsc::Receiver<Result<[u32;2],GpuRasterError>>>, ranked:bool, sealing:bool }
impl Build {
    fn available(&self,demand:bool)->bool {
        let count=self.in_flight.load(std::sync::atomic::Ordering::Acquire);
        self.submission.as_ref().is_none_or(|owner|owner.submitted())
            && (self.ends.is_none() || count==0)
            && (count<u32::from(demand)+1 || demand && self.ranked && self.steps.is_empty())
    }
}
const STAGES: [&str; 23] = ["grain_bits", "distance_seeds", "jump_distance", "store_distance", "skeleton_seeds", "skeleton_ridges", "skeleton_prune", "line_ends", "refine_ends", "seal_candidates", "open_bits", "apply_seals", "region_seeds", "assign_region", "hold_barriers", "grow_region", "intersect_bits", "remember_thin", "check_seeds", "barrier_plane", "snap_seed", "reset_ends", "apply_seal_plane"];

pub(crate) struct Input<'a> {pub device:&'a PipelineDevice,pub mask:&'a wgpu::Buffer,pub paper:&'a wgpu::TextureView}
pub(super) struct Lines {
    layout: wgpu::BindGroupLayout,
    pipelines: std::collections::HashMap<&'static str, Deferred<wgpu::ComputePipeline>>,
    scratch: Option<[wgpu::Buffer; 2]>,
    data: Option<wgpu::Buffer>,
    extent: [u32; 2],
    offsets: [u32; 4],
    windows: Vec<([u32; 4], [u32; 4])>,
    building:Option<Build>,
    ends:u32,
    overflow:bool,
    seals:Option<wgpu::Buffer>,
    seals_ready:bool,
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
        let shader=Deferred::wgsl(device,"region line stages",include_str!("region_lines.wgsl"));
        let pipelines=STAGES.into_iter().map(|entry|(entry,Deferred::compute(device,entry,&pipeline_layout,&shader,entry))).collect();
        Self {layout,pipelines,scratch:None,data:None,extent:[0;2],offsets:[0;4],windows:Vec::new(),building:None,ends:0,overflow:false,seals:None,seals_ready:false}
    }
    pub fn storage_bytes(&self)->u64 {
        self.seals.as_ref().map_or(0,wgpu::Buffer::size)+self.data.as_ref().map_or(0,wgpu::Buffer::size)+self.scratch.as_ref().map_or(0,|buffers|buffers.iter().map(wgpu::Buffer::size).sum())
    }
    pub fn prepare(&self,compiler:&startup::Compiler,priority:u8)->bool {
        compiler.require(STAGES.iter().map(|entry|&self.pipelines[entry]),priority)
    }
    pub fn allocate(&mut self,device:&PipelineDevice,extent:[u32;2],side:u32,windows:&[(u32,[u32;4])])->Result<(),GpuRasterError> {
        let (offsets,size)=storage_layout(extent,&device.limits()).ok_or(GpuRasterError::SizeOverflow)?;
        if self.data.as_ref().is_none_or(|buffer|buffer.size()<size) {
            self.data=Some(device.create_buffer(&wgpu::BufferDescriptor {label:Some("cached region lines"),size,
                usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false}));
        }
        let scratch_bytes=u64::from(side+160).pow(2)*4;
        if self.scratch.as_ref().is_none_or(|buffers|buffers[0].size()<scratch_bytes) {
            self.scratch=Some(std::array::from_fn(|_|device.create_buffer(&wgpu::BufferDescriptor {label:Some("region window coordinates"),size:scratch_bytes,
                usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false})));
        }
        self.extent=extent;self.offsets=offsets;
        self.windows=windows.iter().map(|(_,core)|(*core,analysis_window(*core,extent))).collect();
        Ok(())
    }
    fn params(&self,core:[u32;4],window:[u32;4],options:[u32;4],paper:[f32;4])->Vec<u32> {
        let [w,h]=self.extent;
        [vec![w,h,w.div_ceil(32),w.div_ceil(4)],window.to_vec(),core.to_vec(),self.offsets.to_vec(),options.to_vec(),paper.map(f32::to_bits).to_vec(),vec![0;4]].concat()
    }
    fn encode(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,first:Option<&wgpu::Buffer>,second:Option<&wgpu::Buffer>,steps:Vec<Step>) {
        let Input {device,mask,paper}=input;
        let alignment=u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let stride=PARAMS.next_multiple_of(alignment);
        let mut values=vec![0u8;steps.len()*stride as usize];
        for ((_,_,params),block) in steps.iter().zip(values.chunks_mut(stride as usize)) {
            for (word,value) in block.as_chunks_mut::<4>().0.iter_mut().zip(params){word.copy_from_slice(&value.to_ne_bytes());}
        }
        let uniform=device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("region line parameters"),contents:&values,usage:wgpu::BufferUsages::UNIFORM});
        let scratch=self.scratch.as_ref().unwrap();
        let group=device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("region line stages"),layout:&self.layout,entries:&[
            wgpu::BindGroupEntry {binding:1,resource:wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer:&uniform,offset:0,size:wgpu::BufferSize::new(PARAMS)})},
            wgpu::BindGroupEntry {binding:2,resource:first.unwrap_or(&scratch[0]).as_entire_binding()},
            wgpu::BindGroupEntry {binding:3,resource:second.unwrap_or(&scratch[1]).as_entire_binding()},
            wgpu::BindGroupEntry {binding:4,resource:self.data.as_ref().unwrap().as_entire_binding()},
            wgpu::BindGroupEntry {binding:5,resource:mask.as_entire_binding()},
            wgpu::BindGroupEntry {binding:6,resource:wgpu::BindingResource::TextureView(paper)},
        ]});
        for (i,(entry,count,_)) in steps.iter().enumerate() {
            let pass=encoder.compute_pass();
            pass.set_pipeline(&self.pipelines[entry]);pass.set_bind_group(0,&group,&[(i as u64*stride) as u32]);
            let groups=count.div_ceil(64) as u32;let maximum=device.limits().max_compute_workgroups_per_dimension;
            pass.dispatch_workgroups(groups.min(maximum),groups.div_ceil(maximum).max(1),1);
        }
    }
    fn jump_steps(&self,steps:&mut Vec<Step>,core:[u32;4],window:[u32;4],seeds:&'static str,kind:u32,reach:u32)->u32 {
        let count=u64::from(window[2])*u64::from(window[3]);
        steps.push((seeds,count,self.params(core,window,[0,kind,0,0],[1.,1.,0.,0.])));
        let mut input=0;
        for step in (0..=reach.max(1).next_power_of_two().ilog2()).rev().map(|n|1<<n).chain([1]) {
            steps.push(("jump_distance",count,self.params(core,window,[0,kind,step,input],[1.,1.,0.,0.])));input^=1;
        }
        input
    }
    fn build_steps(&self,grain:[f32;4],radius:u32,has_paper:bool)->Vec<Step> {
        let mut steps=Vec::new();
        for &(core,window) in &self.windows {
            steps.push(("grain_bits",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[radius,u32::from(has_paper),0,0],grain)));
        }
        for &(core,window) in &self.windows {
            let input=self.jump_steps(&mut steps,core,window,"distance_seeds",0,DISTANCE_LIMIT_HALF_PIXELS.div_ceil(2));
            steps.push(("store_distance",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[DISTANCE_LIMIT_HALF_PIXELS,0,0,input],grain)));
        }
        for &(core,window) in &self.windows {self.end_steps(&mut steps,core,window,grain);}
        steps
    }
    fn end_steps(&self,steps:&mut Vec<Step>,core:[u32;4],window:[u32;4],grain:[f32;4]) {
        let count=u64::from(window[2])*u64::from(window[3]);
        steps.push(("skeleton_seeds",count,self.params(core,window,[0;4],grain)));
        let mut input=0;
        for distance in (2..=16).step_by(2) {for phase in 0..4 {
            steps.push(("skeleton_ridges",count,self.params(core,window,[distance,0,phase,input],grain)));input^=1;
        }}
        for _ in 0..2 {steps.push(("skeleton_prune",count,self.params(core,window,[0,0,0,input],grain)));input^=1;}
        steps.push(("line_ends",count,self.params(core,window,[0,MAX_LINE_WIDTH,0,input],grain)));
    }
    pub fn building(&self)->bool {self.building.is_some()}
    fn available(&self,demand:bool)->bool {
        self.building.as_ref().is_none_or(|work|work.available(demand))
    }
    pub fn ready(&mut self,demand:bool)->Result<bool,GpuRasterError> {
        if !self.available(demand) {return Ok(false);}
        let Some(mut work)=self.building.take() else {return Ok(true);};
        if let Some(rx)=&work.ends {
            let metadata=match rx.try_recv() {Ok(result)=>result?,Err(mpsc::TryRecvError::Empty)=>{self.building=Some(work);return Ok(false);},Err(error)=>return Err(GpuRasterError::MapFailed(error.to_string()))};
            self.overflow=metadata[1]!=0;self.ends=metadata[0].min(self.offsets[3]);work.ends=None;work.ranked=true;
            if !self.overflow && self.ends>0 {for entry in ["refine_ends","seal_candidates"] {work.steps.push_back((entry,u64::from(self.ends),self.params([0;4],[0;4],[MAX_LINE_WIDTH*3/2,0,0,0],[1.,1.,0.,0.])));}}
        }
        self.building=Some(work);Ok(true)
    }
    pub fn submitted(&self)->bool {self.building.as_ref().is_none_or(|work|work.submission.as_ref().is_none_or(|owner|owner.submitted()))}
    pub fn discarded(&self)->bool {self.building.as_ref().is_some_and(|work|work.submission.as_ref().is_some_and(|owner|owner.discarded()))}
    pub fn cancel(&mut self) {self.building=None;self.seals_ready=false;}
    pub fn begin_tap(&mut self) {if self.building.as_ref().is_some_and(|work|work.sealing){self.building=None;}self.seals_ready=false;}
    pub fn build(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,paper:([f32;4],u32,bool),budget:f64,demand:bool)->Result<(bool,f64),GpuRasterError> {
        let (grain,radius,has_paper)=paper;
        use std::sync::{Arc,atomic::AtomicU32};
        if self.building.is_none() {
            encoder.clear_buffer(self.data.as_ref().unwrap(),0,None);self.ends=0;self.overflow=false;self.seals_ready=false;
            self.building=Some(Build {steps:self.build_steps(grain,radius,has_paper).into(),submission:None,in_flight:Arc::new(AtomicU32::new(0)),ends:None,ranked:false,sealing:false});
        }
        self.advance(input,encoder,radius,budget,demand)
    }
    fn advance(&mut self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,radius:u32,budget:f64,demand:bool)->Result<(bool,f64),GpuRasterError> {
        let device=input.device;
        use std::sync::atomic::Ordering;
        if !self.ready(demand)? {return Ok((false,0.));}
        let mut work=self.building.take().unwrap();
        if work.steps.is_empty() && work.ranked {return Ok((true,0.));}
        let mut steps=Vec::new();let mut spent=0.;
        while spent<budget {
            let Some((entry,remaining,mut params))=work.steps.pop_front() else {break;};
            let weight=match entry {"grain_bits"=>f64::from(radius*16+32),"jump_distance"=>9.,"store_distance"=>16.,"skeleton_ridges"=>32.,"skeleton_prune"=>128.,"line_ends"=>128.,"refine_ends"=>1024.,"seal_candidates"=>4096.,_=>1.};
            let count=((((budget-spent)/weight) as u64).clamp(64,65536)/64*64).min(remaining);
            steps.push((entry,count,params.clone()));spent+=count as f64*weight;
            if count<remaining {params[24]+=count as u32;work.steps.push_front((entry,remaining-count,params));}
        }
        let mut encoded=!steps.is_empty();
        if encoded {self.encode(input,encoder,None,None,steps);}
        if work.steps.is_empty() && !work.ranked {
            let readback=device.create_buffer(&wgpu::BufferDescriptor {label:Some("region line end count"),size:8,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
            encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1])*4,&readback,0,8);
            let (tx,rx)=mpsc::channel();
            crate::raster::map_then(Some(encoder),&readback,8,|bytes|Ok(std::array::from_fn(|i|u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap()))),move|result|{let _=tx.send(result);});
            work.ends=Some(rx);encoded=true;
        }
        if encoded {
            work.submission=Some(encoder.submission());work.in_flight.fetch_add(1,Ordering::AcqRel);
            let in_flight=work.in_flight.clone();encoder.on_submitted_work_done(move||{in_flight.fetch_sub(1,Ordering::AcqRel);});
        }
        if !encoded && work.steps.is_empty() && work.ranked {return Ok((true,spent));}
        self.building=Some(work);Ok((false,spent))
    }
    pub fn needs_seals(&self,close:bool)->bool {close && self.overflow && !self.seals_ready}
    pub fn has_seals(&self)->bool {self.overflow || self.ends!=0}
    pub fn prepare_seals(&mut self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,paper:&wgpu::TextureView,close:bool,budget:f64)->Result<(bool,f64),GpuRasterError> {
        if !close || !self.overflow || self.seals_ready {return Ok((true,0.));}
        if self.building.is_none() {
            let bytes=u64::from(self.offsets[0])*12;
            if bytes>device.limits().max_storage_buffer_binding_size.min(device.limits().max_buffer_size) {return Err(GpuRasterError::SizeOverflow);}
            if self.seals.as_ref().is_none_or(|buffer|buffer.size()<bytes) {self.seals=Some(device.create_buffer(&wgpu::BufferDescriptor {label:Some("uncached region seals"),size:bytes,usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false}));}
            let mut steps=vec![("reset_ends",u64::from(self.offsets[0])*3,self.params([0;4],[0;4],[1,0,0,0],[1.,1.,0.,0.]))];
            let side=(0..=8).rev().map(|power|1u32<<power).find(|side|
                u64::from(self.extent[0].min(side+160))*u64::from(self.extent[1].min(side+160))<=u64::from(self.offsets[3])).ok_or(GpuRasterError::SizeOverflow)?;
            for &(core,_) in &self.windows {
                for y in (core[1]..core[1]+core[3]).step_by(side as usize) {for x in (core[0]..core[0]+core[2]).step_by(side as usize) {
                    let block=[x,y,side.min(core[0]+core[2]-x),side.min(core[1]+core[3]-y)];
                    let window=analysis_window(block,self.extent);let count=u64::from(window[2])*u64::from(window[3]);
                    steps.push(("reset_ends",u64::from(self.offsets[2]-self.offsets[1]-4).max(1),self.params(block,window,[0;4],[1.,1.,0.,0.])));
                    self.end_steps(&mut steps,window,window,[1.,1.,0.,0.]);
                    for entry in ["refine_ends","seal_candidates"] {steps.push((entry,count,self.params(block,window,[MAX_LINE_WIDTH*3/2,0,0,0],[1.,1.,0.,0.])));}
                    for bucket in 0..3 {steps.push(("apply_seals",count,self.params(block,window,[0,0,bucket,bucket*self.offsets[0]],[1.,1.,0.,0.])));}
                }}
            }
            self.building=Some(Build {steps:steps.into(),submission:None,in_flight:std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),ends:None,ranked:true,sealing:true});
        }
        let mask=self.seals.as_ref().unwrap().clone();let result=self.advance(&Input {device,mask:&mask,paper},encoder,1,budget,true)?;
        self.seals_ready=result.0;Ok(result)
    }
    pub fn barriers(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,close:bool,reach:u32,job:[u32;4],parents:&wgpu::Buffer) {
        let params=|mut steps:Vec<Step>| {
            for (_,_,params) in &mut steps {params[24..28].copy_from_slice(&job);}
            steps
        };
        self.encode(input,encoder,Some(parents),None,params(vec![("open_bits",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],[1.,1.,0.,0.]))]));
        if close && self.overflow {
            self.encode(input,encoder,Some(parents),self.seals.as_ref(),params(vec![("apply_seal_plane",u64::from(self.offsets[0]),self.params([0;4],[0;4],[(reach/16-1)*self.offsets[0],0,0,0],[1.,1.,0.,0.]))]));
        } else if close {
            self.encode(input,encoder,Some(parents),None,params(vec![("apply_seals",u64::from(self.ends),self.params([0;4],[0;4],[0,0,reach/16-1,0],[1.,1.,0.,0.]))]));
        }
        self.encode(input,encoder,Some(parents),None,params(vec![("hold_barriers",u64::from(self.offsets[0]),self.params([0;4],[0;4],[0;4],[1.,1.,0.,0.]))]));
    }

    pub fn assign(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder) {
        let mut steps=Vec::new();
        for &(core,window) in &self.windows {
            steps.push(("reset_ends",1,self.params(core,window,[2,0,0,0],[1.,1.,0.,0.])));
            let words=(window[0]+window[2]).div_ceil(32)-window[0]/32;
            steps.push(("region_seeds",u64::from(words)*u64::from(window[3]),self.params(core,window,[0,5,0,0],[1.,1.,0.,0.])));
            let input=self.jump_steps(&mut steps,core,window,"region_seeds",4,UNDERFILL_HALF_PIXELS.div_ceil(2));
            steps.push(("assign_region",u64::from(core[2])*u64::from(core[3]),self.params(core,window,[UNDERFILL_HALF_PIXELS,4,0,input],[1.,1.,0.,0.])));
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

    pub fn window_state(&self)->&wgpu::Buffer {&self.scratch.as_ref().unwrap()[0]}
    pub fn barrier_plane(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,coverage:&wgpu::Buffer,parents:&wgpu::Buffer,bucket:u32,mode:u32) {
        let params=self.params([0;4],[0;4],[mode,bucket,0,0],[1.,1.,0.,0.]);
        self.encode(input,encoder,Some(coverage),Some(parents),vec![("barrier_plane",u64::from(self.offsets[0]),params)]);
    }

    pub fn check_seeds(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,old:&wgpu::Buffer,new:&wgpu::Buffer,position:[u32;2]) {
        encoder.clear_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+2)*4,Some(4));
        self.encode(input,encoder,Some(old),Some(new),vec![("check_seeds",old.size()/4,self.params([0;4],[0;4],[position[0],position[1],MAX_LINE_WIDTH*3/2,0],[1.,1.,0.,0.]))]);
    }
    pub fn snap_seed(&self,input:&Input<'_>,encoder:&mut crate::submission::CommandEncoder,parents:&wgpu::Buffer,seed:u32,position:[u32;2]) {
        self.encode(input,encoder,Some(parents),None,vec![("snap_seed",1,self.params([0;4],[0;4],[position[0],position[1],seed,0],[1.,1.,0.,0.]))]);
    }
    pub fn copy_stale(&self,encoder:&mut crate::submission::CommandEncoder,output:&wgpu::Buffer,offset:u64) {
        encoder.copy_buffer_to_buffer(self.data.as_ref().unwrap(),u64::from(self.offsets[1]+2)*4,output,offset,4);
    }

}

fn analysis_window(core:[u32;4],extent:[u32;2])->[u32;4] {
    let x=core[0].saturating_sub(80);let y=core[1].saturating_sub(80);
    [x,y,(core[0]+core[2]+80).min(extent[0])-x,(core[1]+core[3]+80).min(extent[1])-y]
}

pub(super) fn fits(extent:[u32;2],limits:&wgpu::Limits)->bool {storage_layout(extent,limits).is_some()}
fn storage_layout(extent:[u32;2],limits:&wgpu::Limits)->Option<([u32;4],u64)> {
    let [w,h]=extent;if w==0 || h==0 || w>=65535 || h>=65535 {return None;}
        let bits=u64::from(w.div_ceil(32))*u64::from(h);
        let distance=u64::from(w.div_ceil(4))*u64::from(h);
        let summary=bits+distance;
        let sparse=summary+4+u64::from(w.div_ceil(128))*u64::from(h.div_ceil(128));
        let dense=(sparse+bits)*4;
        let capacity=ENDS_BYTES.min(limits.max_storage_buffer_binding_size.min(limits.max_buffer_size).saturating_sub(dense))
            .min(u64::from(w)*u64::from(h)*64)/64;
        let size=dense+capacity*64;
        if capacity<u64::from(w.min(161))*u64::from(h.min(161)) || size>limits.max_storage_buffer_binding_size || size>limits.max_buffer_size || w>=65535 || h>=65535 {return None;}
    Some(([bits as u32,summary as u32,sparse as u32,capacity as u32],size))
}
