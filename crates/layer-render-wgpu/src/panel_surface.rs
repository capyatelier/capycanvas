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
    pub mask_pipeline: Deferred<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    profile: wgpu::Buffer,
    surfaces: Vec<PanelSurface>,
    data: Vec<[f32; 4]>,
    uploaded: Arc<AtomicBool>,
    buffer: wgpu::Buffer,
    mask_layout: wgpu::BindGroupLayout,
    mask_group: Option<wgpu::BindGroup>,
    masks: Vec<Option<PanelMask>>,
    mask_view: Option<([u32; 2], u32)>,
    mask_valid: Arc<AtomicBool>,
    blur: wgpu::Texture,
    empty: wgpu::Texture,
    sampler: wgpu::Sampler,
    spacer: wgpu::BindGroup,
    camera: wgpu::BindGroup,
}

struct PanelMask {
    texture: wgpu::Texture,
    group: Option<wgpu::BindGroup>,
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
            crate::bindings::texture(4,wgpu::ShaderStages::FRAGMENT,false),
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
        let mask_layout=crate::bindings::layout(device,"panel coverage",&[
            storage(0,wgpu::ShaderStages::VERTEX_FRAGMENT,true),storage(1,wgpu::ShaderStages::FRAGMENT,true),
        ]);
        let mask_pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label:Some("panel coverage"),bind_group_layouts:&[Some(&empty_layout),Some(&empty_layout),Some(&mask_layout)],immediate_size:0,
        });
        let mask_pipeline=crate::present::surface_pipeline(device,"panel coverage",&mask_pipeline_layout,shader,
            ["panel_mask_vertex","panel_mask_fragment"],None,wgpu::TextureFormat::Rg16Float,None);
        let bytes=layer_render::panel_geometry::PANEL_PROFILE_BYTES;
        let profile=device.create_buffer(&wgpu::BufferDescriptor {label:Some("panel boundary profiles"),
            size:bytes.len() as u64,usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:true});
        profile.slice(..).get_mapped_range_mut().expect("new panel profile buffer is mapped").copy_from_slice(bytes);
        profile.unmap();
        let empty = device.create_texture(&wgpu::TextureDescriptor { label:Some("opaque panel backdrop"),
            size:wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1},mip_level_count:1,sample_count:1,
            dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba16Float,
            usage:wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[], });
        Self { pipeline,mask_pipeline,layout,profile,surfaces:Vec::new(),data:Vec::new(),
            uploaded:Arc::new(AtomicBool::new(false)),
            buffer:device.create_buffer(&wgpu::BufferDescriptor {label:Some("panel surface parameters"),size:64,
                usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false}),
            mask_layout,mask_group:None,masks:Vec::new(),mask_view:None,mask_valid:Arc::new(AtomicBool::new(false)),
            blur:empty.clone(),empty,spacer,camera,
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
        self.mask_valid.store(false,Ordering::Release);
        true
    }

    pub fn ready(&self) -> bool {
        self.pipeline.ready() && self.mask_pipeline.ready()
    }

    pub fn needs_refresh(&self) -> bool {
        !self.surfaces.is_empty() && !self.mask_valid.load(Ordering::Acquire)
    }

    pub fn prepare(&mut self, device: &wgpu::Device, uploads: &mut Uploads, encoder: &mut wgpu::CommandEncoder,
        blur: Option<(&wgpu::Texture,layer_core::Affine)>, extent: [u32; 2], turns: u32) -> Result<(),GpuRasterError> {
        if self.surfaces.is_empty() { self.masks.clear();return Ok(()); }
        if self.mask_view!=Some((extent,turns)) {
            self.mask_view=Some((extent,turns));self.mask_valid.store(false,Ordering::Release);
        }
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
                self.mask_group=None;
                for mask in self.masks.iter_mut().flatten() {mask.group=None;}
            }
            uploads.write(encoder,&self.buffer,bytes)?;
            self.uploaded.store(true,Ordering::Release);
            crate::submission::CacheWrite::shared(self.uploaded.clone()).track(encoder);
        }
        let texture=blur.map_or(&self.empty,|(texture,_)| texture);
        if &self.blur!=texture {
            self.blur=texture.clone();for mask in self.masks.iter_mut().flatten() {mask.group=None;}
        }
        self.masks.resize_with(self.surfaces.len(),||None);
        for (i,mask) in self.masks.iter_mut().enumerate() {
            let area=self.data[3+i*6];
            let size:[u32;2]=std::array::from_fn(|axis| {
                let low=area[axis].floor().clamp(0.,extent[axis] as f32);
                let high=(area[axis]+area[axis+2]).ceil().clamp(0.,extent[axis] as f32);
                (high-low).max(0.) as u32
            });
            if size.contains(&0) {*mask=None;continue;}
            if mask.as_ref().is_none_or(|m|[m.texture.width(),m.texture.height()]!=size) {
                *mask=Some(PanelMask {texture:device.create_texture(&wgpu::TextureDescriptor {
                    label:Some("panel coverage"),size:wgpu::Extent3d {width:size[0],height:size[1],depth_or_array_layers:1},
                    mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rg16Float,
                    usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[],
                }),group:None});
                self.mask_valid.store(false,Ordering::Release);
            }
            let mask=mask.as_mut().unwrap();
            if mask.group.is_none() {
                mask.group=Some(crate::bindings::group(device,"connected panel surfaces",&self.layout,[
                    self.buffer.as_entire_binding(),self.profile.as_entire_binding(),
                    wgpu::BindingResource::TextureView(&self.blur.create_view(&Default::default())),wgpu::BindingResource::Sampler(&self.sampler),
                    wgpu::BindingResource::TextureView(&mask.texture.create_view(&Default::default())),
                ]));
            }
        }
        if !self.mask_valid.load(Ordering::Acquire) {
            let group=self.mask_group.get_or_insert_with(||crate::bindings::group(device,"panel coverage",&self.mask_layout,[
                self.buffer.as_entire_binding(),self.profile.as_entire_binding(),
            ]));
            for (i,mask) in self.masks.iter().enumerate().filter_map(|(i,m)|m.as_ref().map(|m|(i,m))) {
                let target=mask.texture.create_view(&Default::default());
                let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label:Some("panel coverage"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {
                        view:&target,depth_slice:None,resolve_target:None,
                        ops:wgpu::Operations {load:wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),store:wgpu::StoreOp::Store},
                    })],depth_stencil_attachment:None,timestamp_writes:None,occlusion_query_set:None,multiview_mask:None,
                });
                pass.set_pipeline(&self.mask_pipeline);
                pass.set_bind_group(0,&self.spacer,&[]);pass.set_bind_group(1,&self.spacer,&[]);pass.set_bind_group(2,&*group,&[]);
                pass.draw(0..6,i as u32..i as u32+1);
            }
            self.mask_valid.store(true,Ordering::Release);
            crate::submission::CacheWrite::shared(self.mask_valid.clone()).track(encoder);
        }
        Ok(())
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        if self.surfaces.is_empty() { return; }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0,&self.camera,&[0]);
        pass.set_bind_group(1,&self.spacer,&[]);
        for (i,mask) in self.masks.iter().enumerate().filter_map(|(i,m)|m.as_ref().map(|m|(i,m))) {
            pass.set_bind_group(2,mask.group.as_ref().unwrap(),&[]);
            pass.draw(0..6,i as u32..i as u32+1);
        }
    }
}
