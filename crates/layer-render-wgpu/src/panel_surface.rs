use crate::{Deferred, GpuRasterError, Uploads};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

#[cfg(test)]
#[path = "panel_surface_tests.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq)]
pub struct PanelSurfaceGroup {
    pub bounds: [f32; 4],
    pub radii: [f32; 4],
    pub tab_bar: [f32; 4],
    pub active_tab: [f32; 4],
    pub tab_clip: [f32; 4],
    pub color: [f32; 4],
    pub strip_color: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelSurface {
    pub sidebar: [f32; 4],
    pub sidebar_radii: [f32; 4],
    pub plane: f32,
    pub bridge_width: f32,
    pub direction: f32,
    pub scale: f32,
    pub color: [f32; 4],
    pub groups: Vec<PanelSurfaceGroup>,
    pub anchors: Vec<[f32; 4]>,
}

impl PanelSurface {
    pub fn bounds(&self) -> [f32; 4] {
        let first=if self.sidebar[2]>0. && self.sidebar[3]>0. {self.sidebar}
            else {self.groups.first().map_or([0.;4],|g|g.bounds)};
        self.groups.iter().fold(first, |a, g| {
            let [x,y] = [a[0].min(g.bounds[0]),a[1].min(g.bounds[1])];
            [x,y,(a[0]+a[2]).max(g.bounds[0]+g.bounds[2])-x,(a[1]+a[3]).max(g.bounds[1]+g.bounds[3])-y]
        })
    }

    pub fn scaled(mut self, scale: f32) -> Self {
        self.sidebar = self.sidebar.map(|v| v*scale);
        self.sidebar_radii = self.sidebar_radii.map(|v| v*scale);
        self.plane *= scale;
        self.bridge_width *= scale;
        self.scale *= scale;
        for anchor in &mut self.anchors { *anchor = anchor.map(|v| v*scale); }
        for group in &mut self.groups {
            group.bounds = group.bounds.map(|v| v*scale);
            group.radii = group.radii.map(|v| v*scale);
            group.tab_bar = group.tab_bar.map(|v| v*scale);
            group.active_tab = group.active_tab.map(|v| v*scale);
            group.tab_clip = group.tab_clip.map(|v| v*scale);
        }
        self
    }
}

pub(crate) struct PanelSurfaces {
    pub pipeline: Deferred<wgpu::RenderPipeline>,
    pub profile_pipeline: Deferred<wgpu::ComputePipeline>,
    layout: wgpu::BindGroupLayout,
    profile_group: wgpu::BindGroup,
    profile: wgpu::Buffer,
    profile_ready: Arc<AtomicBool>,
    surfaces: Vec<PanelSurface>,
    data: Vec<[f32; 4]>,
    uploaded: Arc<AtomicBool>,
    buffer: wgpu::Buffer,
    group: Option<wgpu::BindGroup>,
    blur: wgpu::Texture,
    empty: wgpu::Texture,
    sampler: wgpu::Sampler,
    spacer: wgpu::BindGroup,
    camera: wgpu::BindGroup,
}

impl PanelSurfaces {
    pub fn new(device: &crate::PipelineDevice, uniform: &wgpu::Buffer, shader: &wgpu::ShaderModule, format: wgpu::TextureFormat) -> Self {
        let storage = |binding, visibility, read_only| crate::bindings::buffer(binding, visibility,
            wgpu::BufferBindingType::Storage { read_only }, false, None);
        let layout = crate::bindings::layout(device, "connected panel surfaces", &[
            storage(0,wgpu::ShaderStages::VERTEX_FRAGMENT,true),
            storage(1,wgpu::ShaderStages::FRAGMENT,true),
            crate::bindings::texture(2,wgpu::ShaderStages::FRAGMENT,true),
            crate::bindings::sampler(3,wgpu::ShaderStages::FRAGMENT,wgpu::SamplerBindingType::Filtering),
        ]);
        let empty_layout = crate::bindings::layout(device, "panel surface spacer", &[]);
        let spacer = crate::bindings::group(device,"panel surface spacer",&empty_layout,[]);
        let viewport=crate::bindings::layout(device,"panel viewport",&[crate::bindings::buffer(0,wgpu::ShaderStages::VERTEX_FRAGMENT,
            wgpu::BufferBindingType::Uniform,true,wgpu::BufferSize::new(crate::present::CAMERA_SIZE))]);
        let camera=crate::bindings::group(device,"panel viewport",&viewport,[wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer:uniform,offset:0,size:wgpu::BufferSize::new(crate::present::CAMERA_SIZE),
        })]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("connected panel surfaces"), bind_group_layouts: &[Some(&viewport),Some(&empty_layout),Some(&layout)], immediate_size: 0,
        });
        let pipeline = crate::present::surface_pipeline(device, "connected panel surfaces", &pipeline_layout, shader,
            ["panel_vertex","panel_fragment"], None, format, Some(wgpu::BlendState {
                color: wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.color,
                alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            }));
        let profile_layout = crate::bindings::layout(device,"panel boundary profiles",&[storage(0,wgpu::ShaderStages::COMPUTE,false)]);
        let profile = device.create_buffer(&wgpu::BufferDescriptor { label:Some("panel boundary profiles"),
            size:512*257*8, usage:wgpu::BufferUsages::STORAGE, mapped_at_creation:false });
        let profile_group = crate::bindings::group(device,"panel boundary profiles",&profile_layout,[profile.as_entire_binding()]);
        let profile_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label:Some("panel boundary profiles"),
            source:wgpu::ShaderSource::Wgsl(include_str!("panel_profile.wgsl").into()) });
        let profile_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:Some("panel boundary profiles"),bind_group_layouts:&[Some(&profile_layout)],immediate_size:0,
        });
        let compiler = device.clone();
        let profile_pipeline = Deferred::pipeline(move |mode| mode.compute(&compiler,&wgpu::ComputePipelineDescriptor {
            label:Some("panel boundary profiles"),layout:Some(&profile_pipeline_layout),module:&profile_shader,
            entry_point:Some("panel_profile"),compilation_options:Default::default(),cache:None,
        }));
        let empty = device.create_texture(&wgpu::TextureDescriptor { label:Some("opaque panel backdrop"),
            size:wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1},mip_level_count:1,sample_count:1,
            dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba16Float,
            usage:wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[], });
        Self { pipeline,profile_pipeline,layout,profile_group,profile,
            profile_ready:Arc::new(AtomicBool::new(false)),surfaces:Vec::new(),data:Vec::new(),
            uploaded:Arc::new(AtomicBool::new(false)),
            buffer:device.create_buffer(&wgpu::BufferDescriptor {label:Some("panel surface parameters"),size:64,
                usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false}),
            group:None,blur:empty.clone(),empty,spacer,camera,
            sampler:device.create_sampler(&wgpu::SamplerDescriptor {label:Some("panel backdrop"),mag_filter:wgpu::FilterMode::Linear,min_filter:wgpu::FilterMode::Linear,..Default::default()}),
        }
    }

    pub fn surfaces(&self) -> &[PanelSurface] { &self.surfaces }
    pub fn set(&mut self, surfaces: &[PanelSurface]) -> bool {
        if self.surfaces==surfaces { return false; }
        self.surfaces.clear();
        self.surfaces.extend_from_slice(surfaces);
        self.data.clear();
        self.data.resize(3+self.surfaces.len()*6,[0.;4]);
        for (i,surface) in self.surfaces.iter().enumerate() {
            let b=surface.bounds();
            let pad=32.*surface.scale;
            let first=self.data.len();
            for g in &surface.groups { self.data.extend([g.bounds,g.radii,g.tab_bar,g.active_tab,g.tab_clip,g.color,g.strip_color]); }
            let anchors=self.data.len();
            self.data.extend_from_slice(&surface.anchors);
            self.data[3+i*6..3+(i+1)*6].copy_from_slice(&[
                [b[0]-pad,b[1]-pad,b[2]+2.*pad,b[3]+2.*pad],surface.sidebar,surface.sidebar_radii,
                [surface.plane,surface.bridge_width,surface.direction,surface.scale],surface.color,
                [first as f32,surface.groups.len() as f32,anchors as f32,surface.anchors.len() as f32],
            ]);
        }
        self.uploaded.store(false,Ordering::Release);
        true
    }

    pub fn ready(&self) -> bool {
        self.pipeline.ready() && self.profile_pipeline.ready() && self.profile_ready.load(Ordering::Acquire)
    }

    pub fn warm_profile(&self,device: &wgpu::Device,queue: &wgpu::Queue) -> bool {
        if !self.pipeline.ready() || !self.profile_pipeline.ready() {return false;}
        if !self.profile_ready.load(Ordering::Acquire) {
            let mut encoder=device.create_command_encoder(&wgpu::CommandEncoderDescriptor {label:Some("prepare panel boundary profiles")});
            self.encode_profile(&mut encoder);
            queue.submit([encoder.finish()]);
        }
        true
    }

    fn encode_profile(&self,encoder: &mut wgpu::CommandEncoder) {
        if !self.profile_ready.load(Ordering::Acquire) {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("panel boundary profiles"),timestamp_writes:None});
            pass.set_pipeline(&self.profile_pipeline);
            pass.set_bind_group(0,&self.profile_group,&[]);
            pass.dispatch_workgroups(64,33,1);
            drop(pass);
            self.profile_ready.store(true,Ordering::Release);
            crate::submission::CacheWrite::shared(self.profile_ready.clone()).track(encoder);
        }
    }

    pub fn prepare(&mut self, device: &wgpu::Device, uploads: &mut Uploads, encoder: &mut wgpu::CommandEncoder,
        blur: Option<(&wgpu::Texture,layer_core::Affine)>) -> Result<(),GpuRasterError> {
        self.encode_profile(encoder);
        if self.surfaces.is_empty() { return Ok(()); }
        let mut header=[[0.;4],[1.,0.,0.,1.],[0.;4]];
        if let Some((texture,affine))=blur {
            header[0]=[0.25/texture.width() as f32,0.25/texture.height() as f32,1.,0.];
            header[1].copy_from_slice(&affine.0[..4]);
            header[2][..2].copy_from_slice(&affine.0[4..]);
        }
        if self.data[..3]!=header {
            self.data[..3].copy_from_slice(&header);
            self.uploaded.store(false,Ordering::Release);
        }
        if !self.uploaded.load(Ordering::Acquire) {
            let bytes=unsafe { std::slice::from_raw_parts(self.data.as_ptr().cast::<u8>(),std::mem::size_of_val(self.data.as_slice())) };
            if self.buffer.size()<bytes.len() as u64 {
                self.buffer=device.create_buffer(&wgpu::BufferDescriptor {label:Some("panel surface parameters"),size:(bytes.len() as u64).next_power_of_two(),
                    usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
                self.group=None;
            }
            uploads.write(encoder,&self.buffer,bytes)?;
            self.uploaded.store(true,Ordering::Release);
            crate::submission::CacheWrite::shared(self.uploaded.clone()).track(encoder);
        }
        let texture=blur.map_or(&self.empty,|(texture,_)| texture);
        if &self.blur!=texture { self.blur=texture.clone();self.group=None; }
        if self.group.is_none() {
            self.group=Some(crate::bindings::group(device,"connected panel surfaces",&self.layout,[
                self.buffer.as_entire_binding(),self.profile.as_entire_binding(),
                wgpu::BindingResource::TextureView(&self.blur.create_view(&Default::default())),wgpu::BindingResource::Sampler(&self.sampler),
            ]));
        }
        Ok(())
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        if self.surfaces.is_empty() { return; }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0,&self.camera,&[0]);
        pass.set_bind_group(1,&self.spacer,&[]);
        pass.set_bind_group(2,self.group.as_ref().unwrap(),&[]);
        pass.draw(0..6,0..self.surfaces.len() as u32);
    }
}
