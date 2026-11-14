use std::sync::Arc;
use crate::{PipelineDevice, SdrSurfaceColor, Uploads, GpuRasterError, Deferred, residency::Tier};

pub(crate) struct Charge { charge: Option<crate::residency::Charge>, memory: Option<layer_core::MemoryOwner> }
impl Charge {
    pub(crate) fn new(bytes:u64,tier:&Arc<Tier>,memory:Option<layer_core::MemoryOwner>)->Self {
        let charge=tier.charge(bytes);publish(memory.as_ref(),tier);Self {charge:Some(charge),memory}
    }
}
fn publish(memory:Option<&layer_core::MemoryOwner>,tier:&Tier) {
    if let Some(memory)=memory {
        let bytes=tier.used();
        memory.publish(layer_core::MemoryReport {gpu_bytes: bytes, pinned_gpu_bytes: bytes, ..Default::default()});
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        let charge=self.charge.take().unwrap();let tier=charge.tier().clone();drop(charge);publish(self.memory.as_ref(),&tier);
    }
}
#[derive(Clone)]
pub struct HeldCanvas {
    pub(crate) canvas: Option<wgpu::TextureView>,
    pub(crate) navigator: Option<(wgpu::TextureView, crate::pixel_rect::PixelRect)>,
    pub(crate) size: [u32; 2],
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) encoding: SdrSurfaceColor,
    pub(crate) device: wgpu::Device,
    pub(crate) _charge: Arc<Charge>,
}
impl HeldCanvas {
    pub(crate) fn capture(encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, draw: impl FnOnce(&mut wgpu::RenderPass<'_>)) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label:Some("capture held canvas"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {
                view:target,depth_slice:None,resolve_target:None,ops:wgpu::Operations {
                    load:wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),store:wgpu::StoreOp::Store}})],
            depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None,multiview_mask:None});
        draw(&mut pass);
    }
    pub(crate) fn texture(device: &PipelineDevice, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::TextureView {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("held canvas"), size: wgpu::Extent3d {width:size[0],height:size[1],depth_or_array_layers:1},
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,
            usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_SRC|wgpu::TextureUsages::COPY_DST,view_formats:&[],
        }).create_view(&Default::default())
    }
    pub(crate) fn new(device: &PipelineDevice, size: [u32; 2], format: wgpu::TextureFormat, encoding: SdrSurfaceColor,
        canvas: Option<wgpu::TextureView>, navigator: Option<(wgpu::TextureView, crate::pixel_rect::PixelRect)>) -> Self {
        let bytes = canvas.iter().chain(navigator.iter().map(|(view,_)| view)).map(|view| crate::texture_bytes(view.texture())).sum();
        Self {canvas,navigator,size,format,encoding,device:(**device).clone(),
            _charge:Arc::new(Charge::new(bytes,&device.transient_display,device.transient_memory.clone()))}
    }
    pub(crate) fn compatible(&self, device:&PipelineDevice,size:[u32;2],format:wgpu::TextureFormat,encoding:SdrSurfaceColor)->bool {
        self.device == **device && self.size == size && self.format == format && self.encoding == encoding
    }
    pub(crate) fn retain_until_complete(&self, device:&PipelineDevice, encoder:&wgpu::CommandEncoder) {
        #[cfg(not(target_arch = "wasm32"))]
        let retained = device.retirement(self.clone());
        #[cfg(target_arch = "wasm32")]
        let retained = self._charge.clone();
        device.on_encoder_done(encoder,move || {#[cfg(not(target_arch="wasm32"))] retained.complete();#[cfg(target_arch="wasm32")] drop(retained);});
    }
}

pub(crate) struct TransitionRecipes {
    pub layout:wgpu::BindGroupLayout,
    pub pipeline:Deferred<wgpu::RenderPipeline>,
}
impl TransitionRecipes {
    pub fn new(device:&PipelineDevice,format:wgpu::TextureFormat)->Self {
        let layout=crate::bindings::layout(device,"held canvas bindings",&[
            crate::bindings::texture_of(0,wgpu::ShaderStages::FRAGMENT,wgpu::TextureSampleType::Float {filterable:false},wgpu::TextureViewDimension::D2),
            crate::bindings::buffer(1,wgpu::ShaderStages::FRAGMENT,wgpu::BufferBindingType::Uniform,true,wgpu::BufferSize::new(16)),
        ]);
        let pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {label:Some("held canvas"),bind_group_layouts:&[Some(&layout)],immediate_size:0});
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("held canvas"),source:wgpu::ShaderSource::Wgsl(SHADER.into())});
        let pipeline=crate::present::surface_pipeline(device,"held canvas",&pipeline_layout,&shader,["vs","fs"],None,format,Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));
        Self {layout,pipeline}
    }
}
pub(crate) struct TransitionDraw {
    canvas: Option<wgpu::BindGroup>,
    navigator: Option<wgpu::BindGroup>,
    origin: [f32; 2],
    uniform: wgpu::Buffer,
    fade: Option<layer_render::CanvasFade>,
}
impl TransitionDraw {
    pub fn new(device:&PipelineDevice,recipes:&TransitionRecipes,held:&HeldCanvas)->Self {
        let uniform=device.create_buffer(&wgpu::BufferDescriptor {label:Some("held canvas fade"),size:528,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let group=|view: &wgpu::TextureView| device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("held canvas"),layout:&recipes.layout,entries:&[
            wgpu::BindGroupEntry {binding:0,resource:wgpu::BindingResource::TextureView(view)},
            wgpu::BindGroupEntry {binding:1,resource:wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer:&uniform,offset:0,size:wgpu::BufferSize::new(16)})},
        ]});
        Self {canvas:held.canvas.as_ref().map(group),navigator:held.navigator.as_ref().map(|(view,_)|group(view)),
            origin:held.navigator.as_ref().map_or([0.;2],|(_,rect)|[rect.min_x() as f32,rect.min_y() as f32]),uniform,fade:None}
    }
    pub fn update(&mut self,uploads:&mut Uploads,encoder:&mut wgpu::CommandEncoder,fade:layer_render::CanvasFade)->Result<(),GpuRasterError> {
        if self.fade==Some(fade) {return Ok(());}
        for (offset,alpha,footprint,origin) in [(0,fade.drawing,1.,[0.;2]),(256,fade.drawing,4.,[0.;2]),(512,fade.navigator,1.,self.origin)] {
            let values=[alpha.unwrap_or(0.).clamp(0.,1.),footprint,origin[0],origin[1]];
            let bytes: [u8;16] = std::array::from_fn(|i| values[i/4].to_ne_bytes()[i%4]);
            uploads.write_at(encoder,&self.uniform,offset,&bytes)?;
        }
        self.fade=Some(fade);Ok(())
    }
    pub fn draw(&self,recipes:&TransitionRecipes,pass:&mut wgpu::RenderPass<'_>,slot:u32) {
        let Some(group)=(if slot==2 {&self.navigator}else{&self.canvas}) else {return;};
        pass.set_pipeline(&recipes.pipeline);pass.set_bind_group(0,group,&[slot*256]);pass.draw(0..3,0..1);
    }
}
const SHADER:&str=r#"
struct Uniform {alpha:f32, footprint:f32, origin:vec2<f32>}
@group(0) @binding(0) var held:texture_2d<f32>;
@group(0) @binding(1) var<uniform> fade:Uniform;
@vertex fn vs(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));
    return vec4(p[index],0.,1.);
}
@fragment fn fs(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
    let pixel=clamp(vec2<i32>(position.xy*fade.footprint)-vec2<i32>(fade.origin),vec2(0),vec2<i32>(textureDimensions(held))-vec2(1));
    return textureLoad(held,pixel,0)*fade.alpha;
}
"#;

#[cfg(test)]
#[path = "present_transition_tests.rs"]
mod tests;
