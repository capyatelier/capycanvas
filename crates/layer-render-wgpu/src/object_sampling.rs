use crate::{Deferred, GpuRasterError, PipelineDevice, PixelRect};

const THREAD_TAPS: u32 = 4096;
pub(crate) const DISPATCH_TAPS: u64 = 1 << 20;
const DISPATCH_THREADS: u32 = 1 << 16;
const MAX_RADIUS: f64 = 2047.;
const MAX_SOURCE_COORDINATE:f64=268435456.;

#[derive(Clone,Copy)]
pub(crate) struct SamplingRequest {
    pub inverse:[f64;6],
    pub size:[u32;2],
    pub nearest:bool,
}

pub fn prepare_nearest_coordinates(inverse:[f64;6],size:[u32;2],first:u32,count:u32)->Result<std::sync::Arc<Vec<u8>>,String> {
    admission(inverse,size,true,u64::MAX).map_err(|error|error.to_string())?;
    let end=u64::from(first)+u64::from(count);
    if count==0 || count>65536 || end>u64::from(size[0])*u64::from(size[1]) {return Err("Invalid nearest coordinate chunk".into());}
    let [a,b,c,d,tx,ty]=inverse;
    let mut bytes=Vec::with_capacity(count as usize*8);
    for index in u64::from(first)..end {
        let x=(index%u64::from(size[0])) as f64+0.5;
        let y=(index/u64::from(size[0])) as f64+0.5;
        for value in [a*x+c*y+tx,b*x+d*y+ty] {
            if !value.is_finite() || value.floor()<f64::from(i32::MIN) || value.floor()>f64::from(i32::MAX) {return Err("Nearest source coordinate is unsupported".into());}
            bytes.extend_from_slice(&(value.floor() as i32).to_le_bytes());
        }
    }
    Ok(std::sync::Arc::new(bytes))
}

#[derive(Clone)]
pub(crate) struct ObjectSampler {
    layout: wgpu::BindGroupLayout,
    empty_coordinates:wgpu::Buffer,
    preview_layout:wgpu::BindGroupLayout,
    preview_uniform:wgpu::Buffer,
    pub denominator: Deferred<wgpu::ComputePipeline>,
    pub contribute: Deferred<wgpu::ComputePipeline>,
    pub normalize: Deferred<wgpu::ComputePipeline>,
    pub preview: Deferred<wgpu::ComputePipeline>,
}
pub(crate) const PREVIEW_SOURCES: usize = 8;
const PREVIEW_OBJECTS: usize = 16;
const PREVIEW_UNIFORM: u64 = 16 + PREVIEW_OBJECTS as u64 * 64;
#[derive(Clone)]
pub(crate) struct PreviewSource { pub source: usize, pub inverse: [f64; 6], pub extent: [u32; 2], pub nearest: bool }
#[derive(Clone)]
pub(crate) struct CollectionPreview {
    pub sources: Vec<wgpu::TextureView>,
    pub objects: Vec<PreviewSource>,
    pub size: [u32; 2],
    pub output: wgpu::TextureView,
    pub scratch: Option<wgpu::TextureView>,
    pub encode: bool,
}
impl CollectionPreview {
    pub(crate) fn passes(objects: &[PreviewSource]) -> usize {
        let mut passes = 0;
        let mut index = 0;
        while index < objects.len() { index += Self::batch(&objects[index..]); passes += 1; }
        passes
    }
    fn batch(objects: &[PreviewSource]) -> usize {
        let mut sources = Vec::new();
        objects.iter().take(PREVIEW_OBJECTS).take_while(|object| {
            if !sources.contains(&object.source) { if sources.len() == PREVIEW_SOURCES { return false; } sources.push(object.source); }
            true
        }).count()
    }
}
pub(crate) struct ObjectAccumulator {
    buffer: wgpu::Buffer,
    coordinates:Option<wgpu::Buffer>,
    coordinates_uploaded:std::cell::Cell<u32>,
    uniform:wgpu::Buffer,
    binding:crate::bindings::CachedBinding<wgpu::TextureView>,
    contribution:std::cell::RefCell<Option<ContributionSchedule>>,
    output: wgpu::TextureView,
    empty: wgpu::TextureView,
    inverse: [f64; 6],
    size: [u32; 2],
    stretch: [f64; 3],
    data: [u8; 112],
    nearest: bool,
}
struct ContributionWindow {work:[u32;4],first:u32,end:u64}
struct ContributionSchedule {kind:u32,origin:[i32;2],extent:[u32;2],windows:Vec<ContributionWindow>}
struct PhaseGeometry {side:u32,columns:u32,lines:u32,height:u32,rows:u32,taps:u32}
impl PhaseGeometry {
    fn work(&self,size:[u32;2],index:u64)->[u32;4] {
        let [x,y]=[(index%u64::from(self.columns)) as u32*self.side,(index/u64::from(self.columns)) as u32*self.side];
        [x,y,self.side.min(size[0]-x),self.side.min(size[1]-y)]
    }
    fn windows(&self)->u64 {u64::from(self.columns)*u64::from(self.lines)}
    fn count(&self)->u64 {self.windows().saturating_mul(u64::from(self.height.div_ceil(self.rows.max(1))))}
}
fn phase_geometry(size:[u32;2],stretch:[f64;3],nearest:bool,phase:u32,extent:[u32;2])->PhaseGeometry {
    let radius=[stretch[0]+stretch[1].abs(),stretch[2]+stretch[1].abs()];
    let bilinear=stretch==[1.,0.,1.];
    let height=if phase==2 || nearest || bilinear {1} else {(radius[1]*2.).ceil() as u32+3};
    let width=if nearest || phase==2 {1} else if bilinear {if phase==1 {4} else {1}} else {((radius[0]*2.+3.).ceil().min(if phase==1 {f64::from(extent[0])} else {f64::INFINITY})) as u32};
    let rows=(THREAD_TAPS/width.max(1)).clamp(1,height);
    let taps=width.max(1)*rows;
    let side=(((DISPATCH_TAPS/u64::from(taps)) as u32).clamp(1,DISPATCH_THREADS) as f64).sqrt().floor().max(1.) as u32;
    PhaseGeometry {side,columns:size[0].div_ceil(side),lines:size[1].div_ceil(side),height,rows,taps}
}
fn tent_bounds(inverse:[f64;6],radius:[f64;2],nearest:bool,work:[u32;4])->([f64;2],[f64;2],[f64;2],[f64;2]) {
    let [a,b,c,d,tx,ty]=inverse;
    let mut qlow=[f64::INFINITY;2];let mut qhigh=[f64::NEG_INFINITY;2];
    for y in [f64::from(work[1])+0.5,f64::from(work[1]+work[3])-0.5] {for x in [f64::from(work[0])+0.5,f64::from(work[0]+work[2])-0.5] {
        for (axis,q) in [a*x+c*y+tx,b*x+d*y+ty].into_iter().enumerate() {qlow[axis]=qlow[axis].min(q);qhigh[axis]=qhigh[axis].max(q);}
    }}
    let low=std::array::from_fn(|axis|if nearest {qlow[axis].floor()} else {(qlow[axis]-radius[axis]-0.5).floor()});
    let high=std::array::from_fn(|axis|if nearest {qhigh[axis].floor()+1.} else {(qhigh[axis]+radius[axis]-0.5).ceil()+1.});
    (low,high,qlow,qhigh)
}
fn contribution_schedule(request:SamplingRequest,stretch:[f64;3],kind:u32,origin:[i32;2],extent:[u32;2],geometry:&PhaseGeometry,limit:u64)->ContributionSchedule {
    let SamplingRequest {inverse,size,nearest,..}=request;
    let [a,b,c,d,tx,ty]=inverse;
    let determinant=a*d-b*c;
    let radius=if nearest {[0.;2]} else {[stretch[0]+stretch[1].abs(),stretch[2]+stretch[1].abs()]};
    let mut bounds=[[f64::INFINITY;2],[f64::NEG_INFINITY;2]];
    for y in [f64::from(origin[1])-radius[1]-0.5,f64::from(origin[1])+f64::from(extent[1])+radius[1]+0.5] {
        for x in [f64::from(origin[0])-radius[0]-0.5,f64::from(origin[0])+f64::from(extent[0])+radius[0]+0.5] {
            let p=[(d*(x-tx)-c*(y-ty))/determinant,(-b*(x-tx)+a*(y-ty))/determinant];
            for axis in 0..2 {bounds[0][axis]=bounds[0][axis].min(p[axis]);bounds[1][axis]=bounds[1][axis].max(p[axis]);}
        }
    }
    let low:[u32;2]=std::array::from_fn(|axis|(bounds[0][axis]-0.5).floor().max(0.).min(f64::from(size[axis])) as u32);
    let high:[u32;2]=std::array::from_fn(|axis|((bounds[1][axis]-0.5).ceil()+1.).max(0.).min(f64::from(size[axis])) as u32);
    let span=std::array::from_fn::<u32,2,_>(|axis|high[axis].saturating_sub(low[axis]).div_ceil(geometry.side));
    let capacity=(u64::from(span[0])*u64::from(span[1])).min(limit.saturating_add(1));
    let mut windows=Vec::with_capacity(capacity as usize);let mut total=0u64;
    let chunk=geometry.rows.max(1);let chunks=geometry.height.div_ceil(chunk);
    for line in 0..span[1] {for column in 0..span[0] {
        let [x,y]=[low[0]+column*geometry.side,low[1]+line*geometry.side];
        let work=[x,y,geometry.side.min(high[0]-x),geometry.side.min(high[1]-y)];
        let (kernel_low,kernel_high,qlow,qhigh)=tent_bounds(inverse,radius,nearest,work);
        if (0..2).any(|axis|kernel_high[axis]<=f64::from(origin[axis]) || kernel_low[axis]>=f64::from(origin[axis])+f64::from(extent[axis])) {continue;}
        let (first,end)=if kind==0 {
            if (0..2).all(|axis|kernel_low[axis]>=f64::from(origin[axis]) && kernel_high[axis]<=f64::from(origin[axis])+f64::from(extent[axis])) {continue;}
            (0,chunks)
        } else if nearest || stretch==[1.,0.,1.] {(0,1)} else {
            let first_row=[(qlow[1]-radius[1]-0.5).floor()-1.,(qhigh[1]-radius[1]-0.5).floor()+1.];
            (((f64::from(origin[1])-first_row[1]).max(0.) as u32/chunk).min(chunks),
                ((f64::from(origin[1])+f64::from(extent[1])-first_row[0]).min(f64::from(geometry.height)).max(0.) as u32).div_ceil(chunk).min(chunks))
        };
        if first==end {continue;}
        let count=end-first;total=total.saturating_add(u64::from(count));
        windows.push(ContributionWindow {work,first,end:total});
        if total>limit {return ContributionSchedule {kind,origin,extent,windows};}
    }}
    ContributionSchedule {kind,origin,extent,windows}
}

pub(crate) enum SamplingPhase<'a> {
    Denominator { extent:[u32;2] },
    Contribute { source:&'a wgpu::TextureView, origin:[i32;2], extent:[u32;2] },
    Normalize,
}
fn sampling_uniform(device:&PipelineDevice)->wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {label:Some("image-object sampling parameters"),size:112,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false})
}

fn stretch(inverse: [f64; 6]) -> Result<[f64; 3], GpuRasterError> {
    if inverse.iter().any(|v| !v.is_finite()) { return Err(GpuRasterError::InvalidTransform("Nonfinite image mapping")); }
    let [a,b,c,d,_,_] = inverse;
    let [xx,xy,yy] = [a*a+c*c,a*b+c*d,b*b+d*d];
    let discriminant = (xx-yy).hypot(2.*xy);
    let major2 = (xx+yy+discriminant)*0.5;
    let determinant = (a*d-b*c).abs();
    if determinant==0. || !major2.is_finite() { return Err(GpuRasterError::InvalidTransform("Singular image mapping")); }
    let minor2 = determinant*determinant/major2;
    let major = major2.sqrt();
    let minor = minor2.sqrt();
    let result = if major<=1. { [1.,0.,1.] }
    else if minor>=1. {
        let scale = (xx+yy+2.*determinant).sqrt();
        [(xx+determinant)/scale,xy/scale,(yy+determinant)/scale]
    } else {
        let scale = (major-1.)/(major2-minor2);
        [1.+scale*(xx-minor2),scale*xy,1.+scale*(yy-minor2)]
    };
    Ok(result)
}

pub(crate) fn mapping(inverse:[f64;6],size:[u32;2])->Result<(),GpuRasterError> {
    if size.contains(&0) {return Err(GpuRasterError::InvalidExtent);}
    if inverse.iter().any(|v| !v.is_finite()) {return Err(GpuRasterError::InvalidTransform("Nonfinite image mapping"));}
    if inverse[..4].iter().any(|&v|v.abs()>MAX_SOURCE_COORDINATE || !(v as f32).is_finite()) {return Err(GpuRasterError::ExtentUnsupported);}
    let [a,b,c,d,tx,ty]=inverse;
    if a*d-b*c==0. {return Err(GpuRasterError::InvalidTransform("Singular image mapping"));}
    for y in [0.5,f64::from(size[1])-0.5] {for x in [0.5,f64::from(size[0])-0.5] {
        if [a*x+c*y+tx,b*x+d*y+ty].iter().any(|v|!v.is_finite() || v.abs()>MAX_SOURCE_COORDINATE) {return Err(GpuRasterError::ExtentUnsupported);}
    }}
    Ok(())
}
fn admission(inverse:[f64;6],size:[u32;2],nearest:bool,limit:u64)->Result<([f64;3],u64),GpuRasterError> {
    mapping(inverse,size)?;
    let stretch=if nearest {[1.,0.,1.]} else {stretch(inverse)?};
    let radius=[stretch[0]+stretch[1].abs(),stretch[2]+stretch[1].abs()];
    if !nearest && radius.iter().any(|&v|!v.is_finite() || v>MAX_RADIUS) {return Err(GpuRasterError::ExtentUnsupported);}
    let bytes=u64::from(size[0]).checked_mul(u64::from(size[1])).and_then(|v|v.checked_mul(32)).ok_or(GpuRasterError::SizeOverflow)?;
    if bytes>limit {return Err(GpuRasterError::SourceWorkingSetExceeded);}
    Ok((stretch,bytes))
}

impl crate::WgpuRasterizer {
    pub fn preflight_image_sampling(&self,inverse:[f64;6],size:[u32;2],nearest:bool)->Result<(),GpuRasterError> {
        admission(inverse,size,nearest,self.device.limits().max_storage_buffer_binding_size).map(|_|())
    }
}

pub(crate) fn preflight_object_affine(scene: layer_core::SceneView<'_>, object: layer_core::authored::ImageObjectHandle,
    affine: layer_core::authored::Affine64, view: layer_render::ViewState, storage_limit: u64,
) -> Result<(), GpuRasterError> {
    use layer_core::authored::{Affine64, ImageInterpolation};
    let source = scene.object(object).ok_or(GpuRasterError::InvalidExtent)?;
    let owner = scene.object_owner(object).ok_or(GpuRasterError::InvalidExtent)?;
    let inverse = affine.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid image object affine"))?;
    let nearest = source.interpolation == ImageInterpolation::Nearest;
    let mut native = inverse.0; native[4] = 0.; native[5] = 0.;
    admission(native, [crate::PAGE_SIZE; 2], nearest, storage_limit)?;
    let offset = scene.occurrence_offset64(owner);
    let placed = Affine64([1., 0., 0., 1., offset[0], offset[1]]).compose(affine);
    let [min, max] = placed.bounds(source.image.extent);
    let visible = crate::display_mips::view_bounds(view, scene.composition().size, 0)?;
    if scene.visible(owner) && !visible.is_empty() && max[0] > f64::from(visible.min_x()) && max[1] > f64::from(visible.min_y())
        && min[0] < f64::from(visible.max_x()) && min[1] < f64::from(visible.max_y()) {
        let level = crate::display_mips::view_level(view.document_to_surface, crate::display_mips::MAX_LEVEL).ok_or(GpuRasterError::InvalidExtent)?;
        let side = f64::from(1u32 << level);
        admission(native.map(|value| value * side), [crate::PAGE_SIZE; 2], nearest, storage_limit)?;
    }
    Ok(())
}

impl ObjectAccumulator {
    pub(crate) fn storage_bytes(&self)->u64 {self.buffer.size()+self.uniform.size()+self.coordinates.as_ref().map_or(0,wgpu::Buffer::size)+self.contribution.borrow().as_ref().map_or(0,|schedule|schedule.windows.capacity() as u64*std::mem::size_of::<ContributionWindow>() as u64)}
    pub(crate) fn nearest_coordinates_ready(&self)->bool {!self.nearest || u64::from(self.coordinates_uploaded.get())==u64::from(self.size[0])*u64::from(self.size[1])}
}
fn source_bounds(inverse:[f64;6],size:[u32;2],nearest:bool,stretch:[f64;3],extent:[u32;2])->PixelRect {
        let [a,b,c,d,tx,ty] = inverse;
        let [rx,ry] = if nearest { [0.,0.] } else { [stretch[0]+stretch[1].abs(),stretch[2]+stretch[1].abs()] };
        let mut low = [f64::INFINITY;2];
        let mut high = [f64::NEG_INFINITY;2];
        for y in [0.5,size[1] as f64-0.5] { for x in [0.5,size[0] as f64-0.5] {
            for (axis,value) in [a*x+c*y+tx,b*x+d*y+ty].into_iter().enumerate() { low[axis]=low[axis].min(value);high[axis]=high[axis].max(value); }
        }}
        PixelRect::new((low[0]-rx-0.5).floor().max(0.).min(extent[0] as f64) as u32,
            (low[1]-ry-0.5).floor().max(0.).min(extent[1] as f64) as u32,
            (high[0]+rx+0.5).ceil().max(0.).min(extent[0] as f64) as u32,
            (high[1]+ry+0.5).ceil().max(0.).min(extent[1] as f64) as u32)
}
impl ObjectSampler {
    pub(crate) fn working_bytes(inverse:[f64;6],size:[u32;2],nearest:bool)->Result<u64,GpuRasterError> {
        let (stretch,bytes)=admission(inverse,size,nearest,u64::MAX)?;
        let geometry=phase_geometry(size,stretch,nearest,1,[u32::MAX;2]);
        let schedules=geometry.windows().checked_mul(std::mem::size_of::<ContributionWindow>() as u64).ok_or(GpuRasterError::SizeOverflow)?;
        bytes.checked_add(if nearest {bytes/4} else {0}).and_then(|value|value.checked_add(112)).and_then(|value|value.checked_add(schedules)).ok_or(GpuRasterError::SizeOverflow)
    }
    pub(crate) fn preview(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,uploads:&mut crate::Uploads,job:&CollectionPreview,empty:&wgpu::TextureView)->Result<(),GpuRasterError> {
        let passes=CollectionPreview::passes(&job.objects);
        let mut first=0;
        let mut back:Option<wgpu::TextureView>=None;
        for pass in 0..passes {
            let count=CollectionPreview::batch(&job.objects[first..]);
            let objects=&job.objects[first..first+count];
            let mut slots:Vec<usize>=Vec::new();
            let mut data=vec![0u8;PREVIEW_UNIFORM as usize];
            for (i,value) in [job.size[0],job.size[1],count as u32,u32::from(back.is_some())|if job.encode {2} else {0}].into_iter().enumerate() {data[i*4..i*4+4].copy_from_slice(&value.to_le_bytes());}
            for (index,object) in objects.iter().enumerate() {
                mapping(object.inverse,job.size)?;
                let [a,b,c,d,tx,ty]=object.inverse;
                let centre=[f64::from(job.size[0])*0.5,f64::from(job.size[1])*0.5];
                let mapped=[a*centre[0]+c*centre[1]+tx,b*centre[0]+d*centre[1]+ty];
                let anchor=mapped.map(f64::floor);
                let slot=slots.iter().position(|source|*source==object.source).unwrap_or_else(||{slots.push(object.source);slots.len()-1});
                let base=16+index*64;
                for (i,value) in [a,c,mapped[0]-anchor[0],0.,b,d,mapped[1]-anchor[1],0.].into_iter().enumerate() {data[base+i*4..base+i*4+4].copy_from_slice(&(value as f32).to_le_bytes());}
                for (i,value) in [-anchor[0] as i32,-anchor[1] as i32,object.extent[0] as i32,object.extent[1] as i32].into_iter().enumerate() {data[base+32+i*4..base+36+i*4].copy_from_slice(&value.to_le_bytes());}
                data[base+48..base+52].copy_from_slice(&(slot as u32).to_le_bytes());
                data[base+52..base+56].copy_from_slice(&u32::from(object.nearest).to_le_bytes());
            }
            uploads.write_at(encoder,&self.preview_uniform,0,&data)?;
            let target=if (passes-1-pass).is_multiple_of(2) {&job.output} else {job.scratch.as_ref().ok_or(GpuRasterError::InvalidExtent)?};
            let sources:Vec<_>=(0..PREVIEW_SOURCES).map(|slot|slots.get(slot).map_or(empty,|source|&job.sources[*source])).collect();
            let binding=crate::bindings::group(device,"image-object collection preview",&self.preview_layout,std::iter::once(self.preview_uniform.as_entire_binding())
                .chain(std::iter::once(wgpu::BindingResource::TextureView(back.as_ref().unwrap_or(empty))))
                .chain(std::iter::once(wgpu::BindingResource::TextureView(target)))
                .chain(sources.into_iter().map(wgpu::BindingResource::TextureView)).collect::<Vec<_>>());
            let mut compute=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("image-object collection preview"),timestamp_writes:None});
            compute.set_pipeline(&self.preview);compute.set_bind_group(0,&binding,&[]);compute.dispatch_workgroups(job.size[0].div_ceil(8),job.size[1].div_ceil(8),1);
            drop(compute);
            back=Some(target.clone());
            first+=count;
        }
        Ok(())
    }
    pub(crate) fn pipelines(&self)->[&Deferred<wgpu::ComputePipeline>;4] {
        [&self.denominator,&self.contribute,&self.normalize,&self.preview]
    }
    pub(crate) fn source_bounds(inverse:[f64;6],size:[u32;2],nearest:bool,extent:[u32;2])->Result<PixelRect,GpuRasterError> {
        let (stretch,_)=admission(inverse,size,nearest,u64::MAX)?;
        Ok(source_bounds(inverse,size,nearest,stretch,extent))
    }
    pub(crate) fn new(device: &PipelineDevice) -> Self {
        let stage = wgpu::ShaderStages::COMPUTE;
        let layout = crate::bindings::layout(device,"image-object tent sampling",&[
            crate::bindings::buffer(0,stage,wgpu::BufferBindingType::Uniform,false,wgpu::BufferSize::new(112)),
            crate::bindings::texture(1,stage,false),
            crate::bindings::buffer(2,stage,wgpu::BufferBindingType::Storage { read_only:false },false,None),
            crate::bindings::storage_texture(3,stage,wgpu::TextureFormat::Rgba32Float,wgpu::StorageTextureAccess::WriteOnly),
            crate::bindings::buffer(4,stage,wgpu::BufferBindingType::Storage {read_only:true},false,wgpu::BufferSize::new(8)),
        ]);
        let shader = { let device=device.clone(); Deferred::new(move || device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:Some("image-object stretched tent"),source:wgpu::ShaderSource::Wgsl(include_str!("object_sampling.wgsl").into()),
        })) };
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:Some("image-object stretched tent"),bind_group_layouts:&[Some(&layout)],immediate_size:0,
        });
        let pipeline = |entry| Deferred::compute(device,"image-object stretched tent",&pipeline_layout,&shader,entry);
        let empty_coordinates=device.create_buffer(&wgpu::BufferDescriptor {label:Some("empty image-object coordinates"),size:8,usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false});
        let preview_layout = crate::bindings::layout(device,"image-object collection preview",&std::iter::once(crate::bindings::buffer(0,stage,wgpu::BufferBindingType::Uniform,false,wgpu::BufferSize::new(PREVIEW_UNIFORM)))
            .chain(std::iter::once(crate::bindings::texture(1,stage,false)))
            .chain(std::iter::once(crate::bindings::storage_texture(2,stage,wgpu::TextureFormat::Rgba32Float,wgpu::StorageTextureAccess::WriteOnly)))
            .chain((0..PREVIEW_SOURCES as u32).map(|slot|crate::bindings::texture(3+slot,stage,false))).collect::<Vec<_>>());
        let preview_shader = Deferred::wgsl(device,"image-object collection preview",crate::compose_wgsl(&[&crate::working_color::shader(device),include_str!("object_preview.wgsl")]));
        let preview_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:Some("image-object collection preview"),bind_group_layouts:&[Some(&preview_layout)],immediate_size:0,
        });
        let preview = Deferred::compute(device,"image-object collection preview",&preview_pipeline_layout,&preview_shader,"main");
        let preview_uniform = device.create_buffer(&wgpu::BufferDescriptor {label:Some("image-object collection preview parameters"),size:PREVIEW_UNIFORM,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        Self { layout,empty_coordinates,preview_layout,preview_uniform,denominator:pipeline("denominator"),contribute:pipeline("contribute"),normalize:pipeline("normalize"),preview }
    }
    #[cfg(test)]
    #[expect(clippy::too_many_arguments, reason = "Test sampling keeps the image extent explicit")]
    fn begin(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,uploads:&mut crate::Uploads,request:SamplingRequest,extent:[u32;2],output:&wgpu::TextureView,empty:&wgpu::TextureView) -> Result<ObjectAccumulator,GpuRasterError> {
        let SamplingRequest {inverse,size,nearest,..}=request;
        let acc=self.create(device,request,output,empty,&sampling_uniform(device))?;
        if nearest {
            let length=size[0]*size[1];
            for first in (0..length).step_by(65536) {
                let bytes=prepare_nearest_coordinates(inverse,size,first,(length-first).min(65536)).unwrap();
                self.set_nearest_coordinates(encoder,uploads,&acc,first,&bytes)?;
            }
        }
        self.initialize(encoder,&acc);
        self.encode_phase(device,encoder,uploads,&acc,SamplingPhase::Denominator {extent})?;
        Ok(acc)
    }
    pub(crate) fn uniform(device:&PipelineDevice)->wgpu::Buffer {sampling_uniform(device)}
    pub(crate) fn create(&self,device:&PipelineDevice,request:SamplingRequest,output:&wgpu::TextureView,empty:&wgpu::TextureView,uniform:&wgpu::Buffer) -> Result<ObjectAccumulator,GpuRasterError> {
        let SamplingRequest {inverse,size,nearest}=request;
        let (stretch,bytes)=admission(inverse,size,nearest,device.limits().max_storage_buffer_binding_size)?;
        let radius=[stretch[0]+stretch[1].abs(),stretch[2]+stretch[1].abs()];
        let [a,b,c,d,tx,ty] = inverse;
        let centre = [size[0] as f64*0.5,size[1] as f64*0.5];
        let anchor = if nearest {[0;2]} else {[(a*centre[0]+c*centre[1]+tx).floor() as i64,(b*centre[0]+d*centre[1]+ty).floor() as i64]};
        let determinant = stretch[0]*stretch[2]-stretch[1]*stretch[1];
        let mut data = [0u8;112];
        for (i,value) in [a,c,tx-anchor[0] as f64,0.,b,d,ty-anchor[1] as f64,0.,stretch[2]/determinant,-stretch[1]/determinant,stretch[0]/determinant,0.,radius[0],radius[1],f64::from(nearest),f64::from(stretch==[1.,0.,1.])].into_iter().enumerate() { let value=if nearest && i<8 {0.} else {value};data[i*4..i*4+4].copy_from_slice(&(value as f32).to_le_bytes()); }
        for (i,value) in size.into_iter().enumerate() { data[80+i*4..84+i*4].copy_from_slice(&value.to_le_bytes()); }
        let buffer=device.create_buffer(&wgpu::BufferDescriptor {label:Some("image-object sampling accumulation"),size:bytes,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let uniform=uniform.clone();
        let coordinates=nearest.then(||device.create_buffer(&wgpu::BufferDescriptor {label:Some("image-object nearest coordinates"),size:bytes/4,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false}));
        Ok(ObjectAccumulator {buffer,coordinates,coordinates_uploaded:Default::default(),uniform,binding:Default::default(),contribution:Default::default(),output:output.clone(),empty:empty.clone(),inverse,size,stretch,data,nearest})
    }
    pub(crate) fn set_nearest_coordinates(&self,encoder:&mut crate::submission::CommandEncoder,uploads:&mut crate::Uploads,acc:&ObjectAccumulator,first:u32,bytes:&[u8])->Result<(),GpuRasterError> {
        let coordinates=acc.coordinates.as_ref().ok_or(GpuRasterError::InvalidExtent)?;
        let count=u32::try_from(bytes.len()/8).map_err(|_|GpuRasterError::SizeOverflow)?;
        let end=u64::from(first)+u64::from(count);
        if !bytes.len().is_multiple_of(8) || count==0 || count>65536 || first!=acc.coordinates_uploaded.get() || end>u64::from(acc.size[0])*u64::from(acc.size[1]) {return Err(GpuRasterError::InvalidExtent);}
        uploads.write_at(encoder,coordinates,u64::from(first)*8,bytes)?;
        acc.coordinates_uploaded.set(end as u32);
        Ok(())
    }
    pub(crate) fn initialize(&self,encoder:&mut crate::submission::CommandEncoder,acc:&ObjectAccumulator) { encoder.clear_buffer(&acc.buffer,0,Some(u64::from(acc.size[0])*u64::from(acc.size[1])*32)); }
    #[cfg(test)]
    fn encode_phase(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,uploads:&mut crate::Uploads,acc:&ObjectAccumulator,phase:SamplingPhase<'_>)->Result<(),GpuRasterError> {
        for step in 0.. {if self.encode_step(device,encoder,uploads,acc,&phase,step)?.is_none() {break;}}
        Ok(())
    }
    pub(crate) fn encode_step(&self,device:&PipelineDevice,encoder:&mut crate::submission::CommandEncoder,uploads:&mut crate::Uploads,acc:&ObjectAccumulator,phase:&SamplingPhase<'_>,step:u64)->Result<Option<u64>,GpuRasterError> {
        if acc.nearest && !acc.nearest_coordinates_ready() {return Err(GpuRasterError::DeferredObjectWork);}
        let (source,origin,extent,kind,pipeline)=match phase {
            SamplingPhase::Denominator {extent}=>(&acc.empty,[0;2],*extent,0,&self.denominator),
            SamplingPhase::Contribute {source,origin,extent}=>(*source,*origin,*extent,1,&self.contribute),
            SamplingPhase::Normalize=>(&acc.empty,[0;2],[0;2],2,&self.normalize),
        };
        let geometry=phase_geometry(acc.size,acc.stretch,acc.nearest,kind,extent);
        let (work,row)=if kind==1 || (kind==0 && !acc.nearest && acc.stretch!=[1.,0.,1.]) {
            let mut cached=acc.contribution.borrow_mut();
            if cached.as_ref().is_none_or(|schedule|schedule.kind!=kind || schedule.origin!=origin || schedule.extent!=extent) {
                *cached=None;
                *cached=Some(contribution_schedule(SamplingRequest {inverse:acc.inverse,size:acc.size,nearest:acc.nearest},acc.stretch,kind,origin,extent,&geometry,u64::MAX));
            }
            let windows=&cached.as_ref().unwrap().windows;
            let index=windows.partition_point(|window|window.end<=step);
            let Some(window)=windows.get(index) else {return Ok(None);};
            let previous=if index==0 {0} else {windows[index-1].end};
            (window.work,u64::from(window.first)+step-previous)
        } else {
            if step>=geometry.count() {return Ok(None);}
            (geometry.work(acc.size,step%geometry.windows()),step/geometry.windows())
        };
        let [a,b,c,d,tx,ty]=acc.inverse;
        let centre=[f64::from(work[0])+f64::from(work[2])*0.5,f64::from(work[1])+f64::from(work[3])*0.5];
        let anchor=if acc.nearest {[0;2]} else {[(a*centre[0]+c*centre[1]+tx).floor() as i64,(b*centre[0]+d*centre[1]+ty).floor() as i64]};
        let mut data=acc.data;
        let translation=if acc.nearest {[0.;2]} else {[a*centre[0]+c*centre[1]+tx-anchor[0] as f64,b*centre[0]+d*centre[1]+ty-anchor[1] as f64]};
        for (offset,value) in [(8,translation[0]),(24,translation[1])] {data[offset..offset+4].copy_from_slice(&(value as f32).to_le_bytes());}
        let rectangle=if acc.nearest {[origin[0],origin[1],extent[0] as i32,extent[1] as i32]} else {[(i64::from(origin[0])-anchor[0]) as i32,(i64::from(origin[1])-anchor[1]) as i32,extent[0] as i32,extent[1] as i32]};
        for (i,value) in rectangle.into_iter().enumerate() {data[64+i*4..68+i*4].copy_from_slice(&value.to_le_bytes());}
        data[88..92].copy_from_slice(&(row as u32*geometry.rows.max(1)).to_le_bytes());data[92..96].copy_from_slice(&geometry.rows.to_le_bytes());
        for (i,value) in work.into_iter().enumerate() {data[96+i*4..100+i*4].copy_from_slice(&value.to_le_bytes());}
        uploads.write_at(encoder,&acc.uniform,0,&data)?;
        let binding=acc.binding.get(source.clone(),||crate::bindings::group(device,"image-object sampling",&self.layout,[acc.uniform.as_entire_binding(),wgpu::BindingResource::TextureView(source),acc.buffer.as_entire_binding(),wgpu::BindingResource::TextureView(&acc.output),acc.coordinates.as_ref().unwrap_or(&self.empty_coordinates).as_entire_binding()]));
        let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("image-object stretched tent"),timestamp_writes:None});
        pass.set_pipeline(pipeline);pass.set_bind_group(0,&binding,&[]);pass.dispatch_workgroups(work[2].div_ceil(8),work[3].div_ceil(8),1);
        Ok(Some(u64::from(work[2])*u64::from(work[3])*u64::from(geometry.taps)))
    }

}

#[cfg(test)]
#[path="object_sampling_reference.rs"]
mod reference;

#[cfg(test)]
mod tests {
    use super::*;
    fn dispatches(inverse:[f64;6],size:[u32;2],nearest:bool,extent:[u32;2],limit:u64)->Result<u64,GpuRasterError> {
        let (stretch,_)=admission(inverse,size,nearest,u64::MAX)?;
        let geometry=phase_geometry(size,stretch,nearest,0,extent);
        let mut total=if nearest || stretch==[1.,0.,1.] {geometry.count()} else {contribution_schedule(SamplingRequest {inverse,size,nearest},stretch,0,[0;2],extent,&geometry,limit).windows.last().map_or(0,|window|window.end)}
            .saturating_add(phase_geometry(size,stretch,nearest,2,[0;2]).count());
        if total>limit {return Ok(limit.saturating_add(1));}
        for tile in crate::page_coordinates(source_bounds(inverse,size,nearest,stretch,extent)) {
            let origin=tile.map(|v|v*crate::PAGE_SIZE);
            let tile_extent=std::array::from_fn(|axis|(extent[axis]-origin[axis]).min(crate::PAGE_SIZE));
            let geometry=phase_geometry(size,stretch,nearest,1,tile_extent);
            let schedule=contribution_schedule(SamplingRequest {inverse,size,nearest},stretch,1,origin.map(|v|v as i32),tile_extent,&geometry,limit-total);
            total=total.saturating_add(schedule.windows.last().map_or(0,|window|window.end));
            if total>limit {return Ok(limit.saturating_add(1));}
        }
        Ok(total)
    }
    use crate::test_support::{page_texture,upload_page};
    fn pixel(x: i64,y: i64)->[f32;4] {
        if !(0..256).contains(&x) || !(0..256).contains(&y) { return [0.;4]; }
        signal(x,y)
    }
    fn signal(x:i64,y:i64)->[f32;4] {
        let alpha=if (x/7+y/11)%5==0 {0.} else {0.75};
        let checker=((x+y)%2) as f32;
        let plate=(((x-128).pow(2)+(y-128).pow(2)) as f64*0.03).sin() as f32;
        [checker*alpha,plate*alpha*4.,if x%13==0 {-2.*alpha} else {0.},alpha]
    }
    #[test]
    fn private_working_reservation_bounds_source_window_metadata_before_allocation() {
        for size in [[1,1],[256,256],[513,257]] {
            for inverse in [[1.,0.,0.,1.,0.,0.],[16.,0.,0.,16.,-0.25,0.75],[2047.,0.,0.,2047.,0.,0.],[57.06,28.98,-0.45,0.89,256.25,256.75]] {
                for nearest in [false,true] {
                    let (stretch,bytes)=admission(inverse,size,nearest,u64::MAX).unwrap();
                    let reserve=ObjectSampler::working_bytes(inverse,size,nearest).unwrap();
                    let fixed=bytes+112+if nearest {bytes/4} else {0};
                    for extent in [[1,1],[256,256],[8192,4096]] {
                        let geometry=phase_geometry(size,stretch,nearest,1,extent);
                        for origin in [[0,0],[256,256]] {
                            let schedule=contribution_schedule(SamplingRequest {inverse,size,nearest},stretch,1,origin,extent,&geometry,u64::MAX);
                            let actual=fixed+schedule.windows.capacity() as u64*std::mem::size_of::<ContributionWindow>() as u64;
                            assert!(actual<=reserve,"{inverse:?} {size:?} {extent:?}: {actual}>{reserve}");
                            let short=contribution_schedule(SamplingRequest {inverse,size,nearest},stretch,1,origin,extent,&geometry,8);
                            assert!(short.windows.capacity()<=9);
                        }
                    }
                }
            }
        }
        assert!(ObjectSampler::working_bytes([1.,0.,0.,1.,0.,0.],[u32::MAX;2],false).is_err());
    }
    #[test]
    fn photo_size_sampling_culls_empty_work_and_reuses_one_parameter_allocation() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let sampler=ObjectSampler::new(&r.device);
        let mut uploads=crate::Uploads::new(&r.device,64*1024);
        let inverse=[16.,0.,0.,16.,0.,0.];let extent:[u32;2]=[4000,3000];let size=[256;2];
        let exact=dispatches(inverse,size,false,extent,u64::MAX-1).unwrap();
        let geometry=phase_geometry(size,[16.,0.,16.],false,1,[256;2]);
        let cartesian=geometry.count()*u64::from(extent[0].div_ceil(256))*u64::from(extent[1].div_ceil(256));
        assert!(exact<cartesian/8,"useful={exact}, cartesian={cartesian}");
        assert_eq!(dispatches(inverse,size,false,extent,8).unwrap(),9);
        let source=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<_>=(0..256).flat_map(|y|(0..256).flat_map(move|x|signal(x,y))).flat_map(f32::to_le_bytes).collect();
        upload_page(&r,&source,&bytes);
        let source_view=source.create_view(&Default::default());
        let output=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let view=output.create_view(&Default::default());
        let acc=sampler.create(&r.device,SamplingRequest {inverse,size,nearest:false},&view,&r.empty_view,&sampling_uniform(&r.device)).unwrap();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        sampler.initialize(&mut encoder,&acc);
        let mut encoded=0;let mut peak_parameters=0;
        let tiles=crate::page_coordinates(source_bounds(acc.inverse,acc.size,false,acc.stretch,extent)).count() as u64;
        let phases=std::iter::once(SamplingPhase::Denominator {extent}).chain(crate::page_coordinates(source_bounds(acc.inverse,acc.size,false,acc.stretch,extent)).map(|tile| {
            let origin=tile.map(|v|v*256);let tile_extent=std::array::from_fn(|axis|(extent[axis]-origin[axis]).min(256));
            SamplingPhase::Contribute {source:&source_view,origin:origin.map(|v|v as i32),extent:tile_extent}
        })).chain(std::iter::once(SamplingPhase::Normalize));
        for phase in phases {for step in 0.. {
            if sampler.encode_step(&r.device,&mut encoder,&mut uploads,&acc,&phase,step).unwrap().is_none() {break;}
            encoded+=1;
            if let Some(report)=r.device.generate_allocator_report() {
                let parameters=report.allocations.iter().filter(|allocation|allocation.name=="image-object sampling parameters").count();
                peak_parameters=peak_parameters.max(parameters);
                assert_eq!(parameters,1);
            }
            if encoded%8==0 {
                assert!(encoder.pass_count()<=8);
                uploads.finish(&encoder);
                let submission=encoder.submit(&r.queue);
                r.device.poll(wgpu::PollType::Wait {submission_index:Some(submission),timeout:Some(std::time::Duration::from_secs(10))}).unwrap();
                encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            }
        }}
        uploads.finish(&encoder);encoder.submit(&r.queue);
        assert_eq!(encoded,exact);
        let border=phase_geometry(size,acc.stretch,false,0,extent);
        assert!(exact<=tiles+2*u64::from(border.columns+border.lines)+1,"one tent dispatch per source tile plus boundary denominators: {exact} for {tiles} tiles");
        assert!(acc.storage_bytes()<3*1024*1024);
        assert!(acc.storage_bytes()<=ObjectSampler::working_bytes(inverse,size,false).unwrap());
        eprintln!("12MP sampling useful={exact}, cartesian={cartesian}, completed passes={encoded}, peak parameter allocations={peak_parameters}, retained bytes={}",acc.storage_bytes());
    }
    #[test]
    fn unminified_full_tile_uses_three_bounded_passes_and_preserves_exact_samples() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let sampler=ObjectSampler::new(&r.device);
        let mut uploads=crate::Uploads::new(&r.device,64*1024);
        let source=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<_>=(0..256).flat_map(|y|(0..256).flat_map(move|x|signal(x,y))).flat_map(f32::to_le_bytes).collect();
        upload_page(&r,&source,&bytes);
        let source_view=source.create_view(&Default::default());
        let output=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let view=output.create_view(&Default::default());
        let acc=sampler.create(&r.device,SamplingRequest {inverse:[1.,0.,0.,1.,0.,0.],size:[256;2],nearest:false},&view,&r.empty_view,&sampling_uniform(&r.device)).unwrap();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        sampler.initialize(&mut encoder,&acc);
        let mut passes=0;
        for phase in [SamplingPhase::Denominator {extent:[256;2]},SamplingPhase::Contribute {source:&source_view,origin:[0;2],extent:[256;2]},SamplingPhase::Normalize] {
            for step in 0.. {if sampler.encode_step(&r.device,&mut encoder,&mut uploads,&acc,&phase,step).unwrap().is_none() {break;}passes+=1;}
        }
        assert_eq!(passes,3);
        uploads.finish(&encoder);
                encoder.submit(&r.queue);
        let actual=crate::layer_tests::page_bytes(&r,&output);
        for (actual,expected) in actual.chunks_exact(4).zip(bytes.chunks_exact(4)) {assert_eq!(f32::from_le_bytes(actual.try_into().unwrap()),f32::from_le_bytes(expected.try_into().unwrap()));}
    }
    #[test]
    fn gpu_tent_accumulates_partial_source_tiles_against_uncapped_reference() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let sampler=ObjectSampler::new(&r.device);
        let mut uploads=crate::Uploads::new(&r.device,64*1024);
        let extent=[513i64,257i64];
        let color=|x,y|if (0..extent[0]).contains(&x) && (0..extent[1]).contains(&y) {signal(x,y)} else {[0.;4]};
        let scale=(0..extent[1]).flat_map(|y|(0..extent[0]).flat_map(move|x|color(x,y).into_iter().take(3))).map(|v|f64::from(v.abs())).fold(1.,f64::max);
        let mut sources=Vec::new();
        for y in 0..2 {for x in 0..3 {
            let source=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
            let bytes:Vec<u8>=(0..256).flat_map(|dy|(0..256).flat_map(move|dx|color(x*256+dx,y*256+dy))).flat_map(f32::to_le_bytes).collect();
            upload_page(&r,&source,&bytes);
            sources.push((source.create_view(&Default::default()),[(x*256) as i32,(y*256) as i32],[(extent[0]-x*256).min(256) as u32,(extent[1]-y*256).min(256) as u32]));
        }}
        let output=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let view=output.create_view(&Default::default());
        let (s,c)=0.47f64.sin_cos();
        for (linear,nearest) in [([64.*c,64.*s,-s,c],false),([-32.*c,-32.*s,-8.*s,8.*c],false),([1.,0.,0.,1.],false),([32.*c,32.*s,-8.*s,8.*c],true)] {
            let [a,b,c,d]=linear;
            for centre in [[256.25,256.75],[512.25,256.25]] {
                let inverse=[a,b,c,d,centre[0]-a*2.-c*2.,centre[1]-b*2.-d*2.];
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                let acc=sampler.begin(&r.device,&mut encoder,&mut uploads,SamplingRequest {inverse,size:[4;2],nearest},extent.map(|v|v as u32),&view,&r.empty_view).unwrap();
                for (source,origin,extent) in &sources {sampler.encode_phase(&r.device,&mut encoder,&mut uploads,&acc,SamplingPhase::Contribute {source,origin:*origin,extent:*extent}).unwrap();}
                sampler.encode_phase(&r.device,&mut encoder,&mut uploads,&acc,SamplingPhase::Normalize).unwrap();
                uploads.finish(&encoder);
                encoder.submit(&r.queue);
                let bytes=crate::layer_tests::page_bytes(&r,&output);
                let mut max=[0f64;4];let mut mean=[0f64;4];
                for y in 0..4 {for x in 0..4 {
                    let q=[a*(x as f64+0.5)+c*(y as f64+0.5)+inverse[4],b*(x as f64+0.5)+d*(y as f64+0.5)+inverse[5]];
                    let expected=reference::sample(q,[a,c,b,d],nearest,|x,y|color(x,y).map(f64::from));
                    for channel in 0..4 {
                        let offset=(y*256+x)*16+channel*4;
                        let actual=f64::from(f32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()));
                        let error=(actual-expected[channel]).abs();
                        max[channel]=max[channel].max(error);mean[channel]+=error/16.;
                        if nearest {assert_eq!(actual,expected[channel]);}
                    }
                }}
                for channel in 0..4 {let bound=if channel==3 {1.} else {scale};assert!(max[channel]<=2e-3*bound && mean[channel]<=2e-4*bound,"{linear:?} centre={centre:?} channel={channel}: max={max:?} mean={mean:?}");}
                eprintln!("cross-tile tent {linear:?} centre={centre:?} nearest={nearest}: max={max:?} mean={mean:?}");
            }
        }
    }
    #[test]
    fn gpu_matches_independent_uncapped_affine_reference() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let sampler=ObjectSampler::new(&r.device);
        let mut uploads=crate::Uploads::new(&r.device,64*1024);
        for hdr in [false,true] {
        let color=move |x,y| {
            let [red,green,blue,alpha]=pixel(x,y);
            if hdr {[red,green,blue,alpha]} else {[red,(green*0.25+alpha)*0.5,-blue*0.5,alpha]}
        };
        let rgb_scale=(0..256).flat_map(|y|(0..256).flat_map(move |x|color(x,y).into_iter().take(3))).map(|v|f64::from(v.abs())).fold(1.,f64::max);
        let source=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<u8>=(0..256).flat_map(|y|(0..256).flat_map(move|x|color(x,y))).flat_map(f32::to_le_bytes).collect();
        upload_page(&r,&source,&bytes);
        let source_view=source.create_view(&Default::default());
        let output=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let output_view=output.create_view(&Default::default());
        let (s,c)=0.47f64.sin_cos();
        let cases=[
            ([1.,0.,0.,1.],false),([0.,1.,-1.,0.],false),([-1.,0.,0.,1.],false),
            ([0.37*c,0.37*s,-0.37*s,0.37*c],false),
            ([8.,0.,0.,8.],false),([16.,0.,0.,16.],false),([32.,0.,0.,32.],false),([64.,0.,0.,64.],false),
            ([19.7,0.,0.,19.7],false),([64.*c,64.*s,-s,c],false),([-64.*c,-64.*s,-s,c],false),
            ([64.,0.,0.,64.],true),
        ];
        for (linear,nearest) in cases {
            let [a,b,c,d]=linear;
            let permutation=!nearest && linear.iter().all(|v|[0.,1.,-1.].contains(v));
            let centre=if permutation {[128.,128.]} else {[128.25,128.75]};
            let inverse=[a,b,c,d,centre[0]-a*4.-c*4.,centre[1]-b*4.-d*4.];
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            if linear==[64.,0.,0.,64.] && !nearest {
                let acc=sampler.create(&r.device,SamplingRequest {inverse,size:[8;2],nearest},&output_view,&r.empty_view,&sampling_uniform(&r.device)).unwrap();
                sampler.initialize(&mut encoder,&acc);
                for phase in [SamplingPhase::Denominator {extent:[256;2]},SamplingPhase::Contribute {source:&source_view,origin:[0;2],extent:[256;2]},SamplingPhase::Normalize] {
                    for step in 0.. {
                        if sampler.encode_step(&r.device,&mut encoder,&mut uploads,&acc,&phase,step).unwrap().is_none() {break;}
                        if step%2==1 {
                            uploads.finish(&encoder);
                encoder.submit(&r.queue);
                            encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                        }
                    }
                }
            } else {
                let acc=sampler.begin(&r.device,&mut encoder,&mut uploads,SamplingRequest {inverse,size:[8;2],nearest},[256;2],&output_view,&r.empty_view).unwrap();
                sampler.encode_phase(&r.device,&mut encoder,&mut uploads,&acc,SamplingPhase::Contribute {source:&source_view,origin:[0;2],extent:[256;2]}).unwrap();
                sampler.encode_phase(&r.device,&mut encoder,&mut uploads,&acc,SamplingPhase::Normalize).unwrap();
            }
            uploads.finish(&encoder);
                encoder.submit(&r.queue);
            let bytes=crate::layer_tests::page_bytes(&r,&output);
            let mut max=[0f64;4];let mut mean=[0f64;4];
            for y in 0..8 { for x in 0..8 {
                let q=[a*(x as f64+0.5)+c*(y as f64+0.5)+inverse[4],b*(x as f64+0.5)+d*(y as f64+0.5)+inverse[5]];
                let reference=reference::sample(q,[a,c,b,d],nearest,|x,y|color(x,y).map(f64::from));
                let offset=(y*256+x)*16;
                for channel in 0..4 {
                    let actual=f64::from(f32::from_le_bytes(bytes[offset+channel*4..offset+channel*4+4].try_into().unwrap()));
                    let error=(actual-reference[channel]).abs();max[channel]=max[channel].max(error);mean[channel]+=error/64.;
                    if permutation || nearest { assert_eq!(actual,reference[channel],"{linear:?}: {x},{y},{channel}"); }
                }
            }}
            for channel in 0..4 {
                let scale=if channel==3 {1.} else {rgb_scale};
                assert!(max[channel]<=2e-3*scale && mean[channel]<=2e-4*scale,"{linear:?} nearest={nearest} channel={channel}: max {} mean {}",max[channel],mean[channel]);
            }
            eprintln!("object tent hdr={hdr} {linear:?} nearest={nearest}: max={max:?} mean={mean:?}");
        }
        }
    }
    #[test]
    fn preflight_refuses_finite_mappings_that_cannot_form_gpu_source_coordinates() {
        for translation in [1e40,1e300,f64::from(i32::MAX)+1.] {
            for nearest in [false,true] {assert!(matches!(admission([1.,0.,0.,1.,translation,0.],[1;2],nearest,u64::MAX),Err(GpuRasterError::ExtentUnsupported)));}
        }
    }
    #[test]
    fn nearest_coordinate_chunks_preserve_f64_boundaries_and_reject_unbounded_requests() {
        let inverse=[2./0.55,0.,0.,1.,-80./0.55,0.4999999999];
        let size=[513,257];
        let length=size[0]*size[1];
        for first in (0..length).step_by(65536) {
            let count=(length-first).min(65536);
            let bytes=prepare_nearest_coordinates(inverse,size,first,count).unwrap();
            assert_eq!(bytes.len(),count as usize*8);
            for (offset,pair) in bytes.chunks_exact(8).enumerate() {
                let index=first+offset as u32;
                let x=f64::from(index%size[0])+0.5;let y=f64::from(index/size[0])+0.5;
                let expected=reference::sample([inverse[0]*x+inverse[4],y+inverse[5]],[inverse[0],0.,0.,1.],true,|x,y|[x as f64,y as f64,0.,1.]);
                assert_eq!(i32::from_le_bytes(pair[..4].try_into().unwrap()) as f64,expected[0]);
                assert_eq!(i32::from_le_bytes(pair[4..].try_into().unwrap()) as f64,expected[1]);
            }
        }
        assert!(prepare_nearest_coordinates(inverse,size,0,65537).is_err());
        assert!(prepare_nearest_coordinates(inverse,size,length,1).is_err());
        assert!(prepare_nearest_coordinates(inverse,size,0,0).is_err());
    }
    #[test]
    fn nearest_uses_f64_source_floor_after_large_coefficient_cancellation() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let sampler=ObjectSampler::new(&r.device);
        let mut uploads=crate::Uploads::new(&r.device,64*1024);
        let source=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let bytes:Vec<_>=(0..256).flat_map(|y|(0..256).flat_map(move|x|[x as f32,y as f32,-(x as f32),1.])).flat_map(f32::to_le_bytes).collect();
        upload_page(&r,&source,&bytes);
        let source_view=source.create_view(&Default::default());
        let output=page_texture(&r,wgpu::TextureFormat::Rgba32Float);
        let output_view=output.create_view(&Default::default());
        for (inverse,size) in [([1e7,0.,0.,1.,0.75-0.5e7,128.],[1,1]),([1e7,0.,0.,1.,128.75-1.5e7,127.],[3,3]),([0.,-1e7,1e7,0.,128.75-1.5e7,128.75+1.5e7],[3,3]),([1.,0.,0.,1.,0.4999999999,0.],[8,8]),([-1e-30,0.,0.,1.,0.,0.],[8,8]),([1./0.55,0.,0.,1.,-80./0.55,0.],[256,8]),([2./0.55,0.,0.,1.,-80./0.55,0.],[256,8])] {
            let [a,b,c,d,_,_]=inverse;
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            let acc=sampler.begin(&r.device,&mut encoder,&mut uploads,SamplingRequest {inverse,size,nearest:true},[256;2],&output_view,&r.empty_view).unwrap();
            sampler.encode_phase(&r.device,&mut encoder,&mut uploads,&acc,SamplingPhase::Contribute {source:&source_view,origin:[0;2],extent:[256;2]}).unwrap();
            sampler.encode_phase(&r.device,&mut encoder,&mut uploads,&acc,SamplingPhase::Normalize).unwrap();
            uploads.finish(&encoder);
                encoder.submit(&r.queue);
            let bytes=crate::layer_tests::page_bytes(&r,&output);
            for y in 0..size[1] {for x in 0..size[0] {
                let q=[a*(f64::from(x)+0.5)+c*(f64::from(y)+0.5)+inverse[4],b*(f64::from(x)+0.5)+d*(f64::from(y)+0.5)+inverse[5]];
                let expected=reference::sample(q,[a,c,b,d],true,|x,y|if (0..256).contains(&x) && (0..256).contains(&y) {[x as f64,y as f64,-(x as f64),1.]} else {[0.;4]});
                for (channel,expected) in expected.into_iter().enumerate() {let offset=((y*256+x)*16) as usize+channel*4;let actual=f64::from(f32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()));assert_eq!(actual,expected,"{inverse:?} {x},{y},{channel}");}
            }}
        }
    }
    #[test]
    fn nearest_has_no_footprint_cap_but_checks_finite_linear_precision() {
        assert!(admission([1e6,0.,0.,1e6,0.,0.],[1;2],true,32).is_ok());
        assert!(matches!(admission([8192.,0.,0.,8192.,0.,0.],[1;2],false,32),Err(GpuRasterError::ExtentUnsupported)));
        assert!(matches!(admission([1.,0.,1.,0.,0.,0.],[1;2],true,32),Err(GpuRasterError::InvalidTransform(_))));
        assert!(matches!(admission([1e30,0.,0.,1e30,-5e29,-5e29],[1;2],true,u64::MAX),Err(GpuRasterError::ExtentUnsupported)));
        assert!(matches!(admission([f64::MAX,0.,0.,1.,0.,0.],[2;2],true,u64::MAX),Err(GpuRasterError::ExtentUnsupported)));
    }
    #[test]
    fn reference_stretch_agrees_with_independent_eigenvectors() {
        for angle in [0.13f64,0.47,1.19] {let (s,c)=angle.sin_cos();
            for scales in [[64.,1.],[0.5,0.3],[3.,7.],[1.00001,0.8]] {
                let [major,minor]=scales;
                let inverse=[major*c,major*s,-minor*s,minor*c,0.,0.];
                let canonical=stretch(inverse).unwrap();
                let expected=[major.max(1.)*c*c+minor.max(1.)*s*s,(major.max(1.)-minor.max(1.))*s*c,major.max(1.)*s*s+minor.max(1.)*c*c];
                for (a,b) in canonical.into_iter().zip(expected) {assert!((a-b).abs()<1e-10);}
            }
        }
    }
}
