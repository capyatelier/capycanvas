use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use crate::{PipelineDevice, SdrSurfaceColor, Uploads, GpuRasterError, Deferred};

pub(crate) struct Charge { bytes: u64, total: Arc<AtomicU64> }
impl Drop for Charge {
    fn drop(&mut self) { self.total.fetch_sub(self.bytes, Ordering::AcqRel); }
}
#[derive(Clone)]
pub struct HeldCanvas {
    pub(crate) _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) size: [u32; 2],
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) encoding: SdrSurfaceColor,
    pub(crate) device: wgpu::Device,
    pub(crate) _charge: Arc<Charge>,
}
impl HeldCanvas {
    pub(crate) fn capture(device: &PipelineDevice, encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2], format: wgpu::TextureFormat, encoding: SdrSurfaceColor,
        pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("held canvas"), size: wgpu::Extent3d {width:size[0],height:size[1],depth_or_array_layers:1},
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,
            usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[],
        });
        let view = texture.create_view(&Default::default());
        let bytes = crate::texture_bytes(&texture);
        device.transient_display.fetch_add(bytes, Ordering::AcqRel);
        let held = Self {_texture:texture,view,size,format,encoding,device:(**device).clone(),
            _charge:Arc::new(Charge {bytes,total:device.transient_display.clone()})};
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label:Some("capture held canvas"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {
                    view:&held.view,depth_slice:None,resolve_target:None,ops:wgpu::Operations {
                        load:wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),store:wgpu::StoreOp::Store}})],
                depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None,multiview_mask:None});
            pass.set_pipeline(pipeline);pass.set_bind_group(0,group,&[0]);pass.draw(0..3,0..1);
        }
        held.retain_until_complete(encoder);
        held
    }
    pub(crate) fn compatible(&self, device:&PipelineDevice,size:[u32;2],format:wgpu::TextureFormat,encoding:SdrSurfaceColor)->bool {
        self.device == **device && self.size == size && self.format == format && self.encoding == encoding
    }
    pub(crate) fn retain_until_complete(&self, encoder:&wgpu::CommandEncoder) {
        #[cfg(not(target_arch = "wasm32"))]
        let retained = self.clone();
        #[cfg(target_arch = "wasm32")]
        let retained = self._charge.clone();
        encoder.on_submitted_work_done(move || drop(retained));
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
    group:wgpu::BindGroup,
    uniform:wgpu::Buffer,
    alpha:Option<f32>,
}
impl TransitionDraw {
    pub fn new(device:&PipelineDevice,recipes:&TransitionRecipes,held:&HeldCanvas)->Self {
        let uniform=device.create_buffer(&wgpu::BufferDescriptor {label:Some("held canvas fade"),size:272,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let group=device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("held canvas"),layout:&recipes.layout,entries:&[
            wgpu::BindGroupEntry {binding:0,resource:wgpu::BindingResource::TextureView(&held.view)},
            wgpu::BindGroupEntry {binding:1,resource:wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer:&uniform,offset:0,size:wgpu::BufferSize::new(16)})},
        ]});
        Self {group,uniform,alpha:None}
    }
    pub fn update(&mut self,uploads:&mut Uploads,encoder:&mut wgpu::CommandEncoder,alpha:f32)->Result<(),GpuRasterError> {
        let alpha=alpha.clamp(0.,1.);
        if self.alpha==Some(alpha) {return Ok(());}
        for (offset,footprint) in [(0,1.),(256,4.)] {
            let bytes: [u8;16] = std::array::from_fn(|i| [alpha,footprint,0.,0.][i/4].to_ne_bytes()[i%4]);
            uploads.write_at(encoder,&self.uniform,offset,&bytes)?;
        }
        self.alpha=Some(alpha);Ok(())
    }
    pub fn draw(&self,recipes:&TransitionRecipes,pass:&mut wgpu::RenderPass<'_>,source:bool) {
        pass.set_pipeline(&recipes.pipeline);pass.set_bind_group(0,&self.group,&[if source {256}else{0}]);pass.draw(0..3,0..1);
    }
}
const SHADER:&str=r#"
struct Uniform {alpha:f32, footprint:f32, pad:vec2<f32>}
@group(0) @binding(0) var held:texture_2d<f32>;
@group(0) @binding(1) var<uniform> fade:Uniform;
@vertex fn vs(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));
    return vec4(p[index],0.,1.);
}
@fragment fn fs(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
    let pixel=min(vec2<i32>(position.xy*fade.footprint),vec2<i32>(textureDimensions(held))-vec2(1));
    return textureLoad(held,pixel,0)*fade.alpha;
}
"#;

#[cfg(test)]
#[path = "present_transition_tests.rs"]
mod tests;
