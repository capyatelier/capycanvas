//! A bounded GPU loupe. It reads the presenter's existing artwork bindings.
use crate::{GpuRasterError, Uploads, WgpuRasterizer};
use layer_render::ColorPickerOverlay;

#[derive(Default)]
pub(super) struct Picker {
    pipeline: Option<wgpu::RenderPipeline>,
    buffer: Option<wgpu::Buffer>,
    data: Option<[f32; 16]>,
    dirty: bool,
}
impl Picker {
    pub fn set(
        &mut self,
        overlay: Option<ColorPickerOverlay>,
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        format: wgpu::TextureFormat,
    ) {
        let data = overlay.map(|o| {
            [
                o.center[0],
                o.center[1],
                o.scale,
                f32::from(o.classic),
                o.sample[0],
                o.sample[1],
                f32::from(o.layer),
                0.,
                o.original[0],
                o.original[1],
                o.original[2],
                1.,
                o.candidate[0],
                o.candidate[1],
                o.candidate[2],
                1.,
            ]
        });
        if self.data == data {
            return;
        }
        self.data = data;
        self.dirty = true;
        if data.is_none() || self.pipeline.is_some() {
            return;
        }
        self.pipeline = Some(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("color picker glass"), layout: Some(layout),
            vertex: wgpu::VertexState { module: shader, entry_point: Some("picker_vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 64, step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4], })], },
            fragment: Some(wgpu::FragmentState { module: shader, entry_point: Some("picker_fragment"),
                compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })], }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None,
        }));
        self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("color picker geometry"),
            size: 64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    pub fn bounds(&self) -> Option<[f32; 4]> {
        self.data.map(|d| {
            let radius = 48. * d[2];
            [d[0] - radius, d[1] - radius, radius * 2., radius * 2.]
        })
    }
    pub fn upload(
        &mut self,
        uploads: &mut Uploads,
        renderer: &WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if self.dirty
            && let Some(data) = self.data
        {
            let mut bytes = [0u8; 64];
            for (chunk, value) in bytes.chunks_exact_mut(4).zip(data) {
                chunk.copy_from_slice(&value.to_ne_bytes());
            }
            uploads.write(
                encoder,
                renderer.queue(),
                self.buffer.as_ref().unwrap(),
                &bytes,
            )?;
        }
        self.dirty = false;
        Ok(())
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.data.is_none() {
            return;
        }
        pass.set_pipeline(self.pipeline.as_ref().unwrap());
        pass.set_vertex_buffer(0, self.buffer.as_ref().unwrap().slice(..));
        pass.draw(0..6, 0..1);
    }
}
