//! GPU-only viewport presentation shared by toolkit surfaces and WebGPU.

use crate::{BackdropBlurStyle, BackdropRegion, GpuRasterError, SdrSurfaceColor, Uploads, WgpuRasterizer};
use layer_render::{CanvasRenderer, CursorSegment, ViewState};

const CAMERA_SIZE: u64 = 512;
const SOURCE_CAMERA: u32 = 512;

/// A native UI's document overview, sampled from the existing GPU image.
/// Bounds and work-area corners use physical target-surface pixels. Hosts can
/// place it in their main viewport or a retained native Navigator surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverviewPlacement {
    pub bounds: [f32; 4],
    /// Visible part of the image after native scrolling/overflow clipping.
    /// None uses the complete image bounds without changing its UV mapping.
    pub clip: Option<[f32; 4]>,
    pub work_area: [[f32; 2]; 4],
    pub outline_linear: [f32; 3],
    pub background_linear: [f32; 3],
    pub scale: f32,
    pub opacity: f32,
}

impl OverviewPlacement {
    fn packed(self) -> Option<[f32; 24]> {
        let [x, y, w, h] = self.bounds;
        let [[ax, ay], [bx, by], [cx, cy], [dx, dy]] = self.work_area;
        let [r, g, b] = self.outline_linear;
        let [br, bg, bb] = self.background_linear;
        let opacity = self.opacity.clamp(0., 1.);
        let [clip_x, clip_y, clip_w, clip_h] = self.clip.unwrap_or(self.bounds);
        let data = [
            x, y, w, h, ax, ay, bx, by, cx, cy, dx, dy, r, g, b, opacity, br, bg, bb, self.scale,
            clip_x, clip_y, clip_w, clip_h,
        ];
        (w > 0.
            && h > 0.
            && self.scale > 0.
            && self.opacity > 0.
            && clip_w > 0.
            && clip_h > 0.
            && data.iter().all(|v| v.is_finite()))
        .then_some(data)
    }
}

pub struct ViewportPresenter {
    timing: Option<crate::frame_timing::GpuFrameTimer>,
    pipeline: [wgpu::RenderPipeline; 3],
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    proof_buffer: wgpu::Buffer,
    proof_uniform: wgpu::Buffer,
    hdr_uniform: wgpu::Buffer,
    hdr_options: [f32; 8],
    screen_uniform: wgpu::Buffer,
    screen_options: [f32; 16],
    screen_counter: Option<crate::present_screen::ScreenCounter>,
    local_buffer: wgpu::Buffer,
    disabled_local_buffer: wgpu::Buffer,
    gpu_local_guide: Option<std::sync::Arc<crate::local_tone::GpuToneGuide>>,
    proof_options: [u32; 4],
    proof_lut: Option<std::sync::Arc<layer_color::ProofLut>>,
    bind_group: Option<wgpu::BindGroup>,
    selection_buffer: Option<wgpu::Buffer>,
    saved_selection_buffer: Option<wgpu::TextureView>,
    empty_saved_selection:wgpu::TextureView,
    composite_view: Option<wgpu::TextureView>,
    coarse_view: Option<wgpu::TextureView>,
    next_view: Option<wgpu::TextureView>,
    display_geometry: Option<wgpu::Buffer>,
    document_extent: [u32; 2],
    encode_srgb: bool,
    corner_radius: f32,
    picker: crate::present_picker::Picker,
    cursor_pipeline: wgpu::RenderPipeline,
    cursor_buffer: wgpu::Buffer,
    cursor_vertices: Vec<CursorSegment>,
    uploads: Uploads,
    camera_data: Option<[f32; 128]>,
    quarter_turns: u32,
    retained: bool,
    history: crate::present_damage::Retained,
    presented_view: Option<(ViewState, [f32; 4], u32, f32)>,
    overlays_changed: bool,
    backdrop: Option<crate::backdrop_blur::BackdropBlur>,
    source_camera: Option<[f32; 128]>,
    presented_area: u64,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    color: SdrSurfaceColor,
    overview_pipeline: Option<wgpu::RenderPipeline>,
    navigator_view: Option<wgpu::TextureView>,
    overview_buffer: Option<wgpu::Buffer>,
    overviews: Vec<[f32; 24]>,
    overviews_changed: bool,
    standalone_overview: bool,
}

impl ViewportPresenter {
    /// Bind an immutable guide built on this device without staging any image
    /// pixels through the host. The previous buffer remains valid until here.
    pub fn set_gpu_local_tone_guide(
        &mut self,
        renderer: &WgpuRasterizer,
        guide: Option<std::sync::Arc<crate::local_tone::GpuToneGuide>>,
    ) -> Result<(), GpuRasterError> {
        if let Some(g) = &guide
            && (g.device != *renderer.device || g.space != renderer.document_color.space) {
                return Err(GpuRasterError::Color("Local tone guide belongs to a different device or color space".into()));
            }
        if match (&self.gpu_local_guide, &guide) {
            (None, None) => true,
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
            _ => false,
        } {
            return Ok(());
        }
        self.local_buffer = guide.as_ref().map_or_else(|| self.disabled_local_buffer.clone(), |g| g.buffer.clone());
        self.gpu_local_guide = guide;
        self.bind_group = None;
        Ok(())
    }
    pub fn set_hdr_view(
        &mut self,
        renderer: &WgpuRasterizer,
        rendition: Option<layer_core::color::hdr::SdrRendition>,
        headroom: f32,
    ) -> Result<(), GpuRasterError> {
        self.set_hdr_options(renderer, rendition, headroom, false)
    }

    /// Submit HDR to the host's tone mapper without the display-headroom shoulder.
    /// PQ is absolute and bounded to BT.2020 / 10,000 cd/m²; WebGPU extended sRGB
    /// is SDR-relative. Explicit print/gamut proof still takes precedence.
    pub fn set_compositor_hdr_view(
        &mut self,
        renderer: &WgpuRasterizer,
        rendition: layer_core::color::hdr::SdrRendition,
    ) -> Result<(), GpuRasterError> {
        if !matches!(self.color, SdrSurfaceColor::Bt2100Pq | SdrSurfaceColor::ExtendedSrgb) {
            return Err(GpuRasterError::Color("Compositor HDR requires a PQ or extended sRGB surface".into()));
        }
        self.set_hdr_options(renderer, Some(rendition), 1., true)
    }

    fn set_hdr_options(
        &mut self,
        renderer: &WgpuRasterizer,
        rendition: Option<layer_core::color::hdr::SdrRendition>,
        headroom: f32,
        compositor: bool,
    ) -> Result<(), GpuRasterError> {
        if !headroom.is_finite() || !(1. ..=100.).contains(&headroom) {
            return Err(GpuRasterError::Color("Invalid display HDR headroom".into()));
        }
        if let Some(r) = rendition {
            r.validate().map_err(|e| GpuRasterError::Color(e.into()))?;
        }
        let options = rendition.map_or([0.; 8], |r| {
            let p = r.parameters();
            [
                p[0], p[1], p[2], p[3], headroom, p[4], p[5], u32::from(compositor) as f32,
            ]
        });
        if options != self.hdr_options {
            renderer.queue.write_buffer(
                &self.hdr_uniform,
                0,
                options.map(f32::to_ne_bytes).as_flattened(),
            );
            self.hdr_options = options;
        }
        Ok(())
    }

    /// Opt-in, bounded and nonblocking pass timings for benchmarks using
    /// `present` / `present_overviews`. Leave disabled when submitting `encode`
    /// directly: those callers cannot notify this timer of their submission.
    pub fn gpu_timings(
        &mut self,
        renderer: &WgpuRasterizer,
        enabled: bool,
    ) -> Vec<crate::frame_timing::GpuFrameSample> {
        if !enabled {
            self.timing = None;
            return Vec::new();
        }
        let timer = self.timing.get_or_insert_with(|| {
            crate::frame_timing::GpuFrameTimer::new(renderer.device(), renderer.queue())
        });
        timer.poll(renderer.device(), renderer.queue());
        let mut samples = vec![crate::frame_timing::GpuFrameSample::default(); 256];
        let count = timer.take_into(&mut samples);
        samples.truncate(count);
        samples
    }

    pub fn proof_storage_bytes(&self) -> u64 {
        (if self.gpu_local_guide.is_some() { self.local_buffer.size() } else { 0 })
            + if self.proof_lut.is_some() {
                self.proof_buffer.size()
            } else {
                0
            }
    }

    /// Explicit viewport captures share immutable samples and the same viewing
    /// options; they do not allocate another LUT. Export never calls this path.
    pub fn inherit_proof(&mut self, renderer: &WgpuRasterizer, source: &Self) {
        if self.local_buffer != source.local_buffer {
            self.local_buffer = source.local_buffer.clone();
            self.gpu_local_guide = source.gpu_local_guide.clone();
            self.bind_group = None;
        }
        if self.hdr_options != source.hdr_options {
            renderer.queue.write_buffer(
                &self.hdr_uniform,
                0,
                source.hdr_options.map(f32::to_ne_bytes).as_flattened(),
            );
            self.hdr_options = source.hdr_options;
        }
        if self.proof_buffer == source.proof_buffer && self.proof_uniform == source.proof_uniform {
            self.proof_options = source.proof_options;
            return;
        }
        self.proof_buffer = source.proof_buffer.clone();
        self.proof_uniform = source.proof_uniform.clone();
        self.proof_options = source.proof_options;
        self.proof_lut = source.proof_lut.clone();
        self.bind_group = None;
    }

    /// Publish a complete viewing derivative without changing artwork caches or
    /// recompiling shaders. Hosts prepare the LUT on a separate CPU worker.
    pub fn set_proof(
        &mut self,
        renderer: &WgpuRasterizer,
        lut: Option<std::sync::Arc<layer_color::ProofLut>>,
        enabled: bool,
        gamut: bool,
    ) -> Result<(), GpuRasterError> {
        if lut
            .as_ref()
            .is_some_and(|l| l.space() != renderer.device.working_space())
        {
            return Err(GpuRasterError::Color(
                "Proof preview working space is stale".into(),
            ));
        }
        let same = match (&self.proof_lut, &lut) {
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            let size = lut.as_ref().map_or(20, |l| l.byte_len()) as u64;
            if size > renderer.device.limits().max_storage_buffer_binding_size {
                return Err(GpuRasterError::Color(
                    "Proof preview exceeds the GPU buffer limit".into(),
                ));
            }
            let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("proof viewing samples"),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: true,
            });
            if let Some(lut) = &lut {
                // Nested f32 arrays have no padding or uninitialized bytes.
                let bytes = unsafe {
                    std::slice::from_raw_parts(lut.samples().as_ptr().cast::<u8>(), lut.byte_len())
                };
                buffer
                    .slice(..)
                    .get_mapped_range_mut()
                    .map_err(|e| GpuRasterError::Color(e.to_string()))?
                    .copy_from_slice(bytes);
            }
            buffer.unmap();
            self.proof_buffer = buffer;
            self.proof_lut = lut;
            self.bind_group = None;
        }
        let options = self.proof_lut.as_ref().map_or([0; 4], |lut| {
            [
                lut.edge(),
                layer_core::color::RgbSpace::ALL
                    .iter()
                    .position(|s| *s == lut.space())
                    .unwrap() as u32
                    | (u32::from(lut.dark_grid()) << 8),
                u32::from(enabled),
                u32::from(gamut),
            ]
        });
        if self.proof_options != options {
            let bytes = unsafe { std::slice::from_raw_parts(options.as_ptr().cast::<u8>(), 16) };
            renderer.queue.write_buffer(&self.proof_uniform, 0, bytes);
            self.proof_options = options;
        }
        Ok(())
    }

    pub fn set_screen_check(&mut self, renderer: &WgpuRasterizer, check: Option<crate::present_screen::ScreenCheck>) {
        let options = crate::present_screen::ScreenCheck::uniform(check);
        if options != self.screen_options {
            renderer.queue.write_buffer(&self.screen_uniform, 0, options.map(f32::to_ne_bytes).as_flattened());
            self.screen_options = options;
        }
    }

    pub fn screen_check_busy(&self) -> bool {
        self.screen_counter.as_ref().is_some_and(|c| c.busy())
    }

    pub fn check_screen(&mut self, renderer: &WgpuRasterizer) -> bool {
        let (Some(group), Some(camera)) = (&self.bind_group, self.camera_data) else { return false };
        if self.standalone_overview || !crate::present_screen::ScreenCheck::counts(&self.screen_options) {
            return false;
        }
        let signature: Vec<u32> = camera[..32]
            .iter()
            .chain(&self.hdr_options)
            .chain(&self.screen_options)
            .map(|v| v.to_bits())
            .chain(self.proof_options)
            .chain([renderer.composite_revision as u32, (renderer.composite_revision >> 32) as u32])
            .collect();
        let counter = self.screen_counter.get_or_insert_with(|| {
            crate::present_screen::ScreenCounter::new(&renderer.device, &self.layout, &self.shader)
        });
        if counter.busy() || counter.current(&signature) || !renderer.background_pipeline_ready(&counter.pipeline) {
            return false;
        }
        let viewport = [camera[8] as u32, camera[9] as u32];
        counter.start(&renderer.device, &renderer.queue, group, viewport, signature);
        true
    }

    pub fn screen_check_result(&mut self) -> Option<Result<bool, GpuRasterError>> {
        self.screen_counter.as_mut()?.finish()
    }

    /// Shares the renderer's optional startup cache with presentation shaders.
    pub fn for_renderer(renderer: &WgpuRasterizer, format: wgpu::TextureFormat) -> Self {
        Self::with_device(&renderer.device, format, SdrSurfaceColor::Srgb)
    }

    /// The host configures its surface with the matching, advertised color space.
    /// UI overlays are sRGB; artwork is in the renderer's document coordinates.
    pub fn for_surface(
        renderer: &WgpuRasterizer,
        format: wgpu::TextureFormat,
        color: SdrSurfaceColor,
    ) -> Result<Self, GpuRasterError> {
        color.shader_encoding(format)?;
        let mut presenter = Self::with_device(&renderer.device, format, color);
        presenter.set_hdr_view(
            renderer,
            renderer
                .document_color()
                .depth
                .is_float()
                .then_some(Default::default()),
            1.,
        )?;
        Ok(presenter)
    }

    /// Standalone Navigator with the same explicit encoding as its main canvas.
    pub fn for_overview_surface(
        renderer: &WgpuRasterizer,
        format: wgpu::TextureFormat,
        color: SdrSurfaceColor,
    ) -> Result<Self, GpuRasterError> {
        Ok(Self { standalone_overview: true, ..Self::for_surface(renderer, format, color)? })
    }

    fn with_device(
        device: &crate::PipelineDevice,
        format: wgpu::TextureFormat,
        color: SdrSurfaceColor,
    ) -> Self {
        let layout = crate::bindings::layout(device, "viewport bindings", &[
            crate::bindings::buffer(
                10,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Storage { read_only: true },
                false,
                std::num::NonZeroU64::new(32),
            ),
            crate::bindings::texture_of(
                11,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::TextureSampleType::Uint,
                wgpu::TextureViewDimension::D2,
            ),
            crate::bindings::buffer(
                0,
                wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform,
                true,
                wgpu::BufferSize::new(CAMERA_SIZE),
            ),
            crate::bindings::texture(1, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, true),
            crate::bindings::sampler(
                2,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::SamplerBindingType::Filtering,
            ),
            crate::bindings::buffer(
                3,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BufferBindingType::Storage { read_only: true },
                false,
                None,
            ),
            crate::bindings::texture(4, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, true),
            crate::bindings::buffer(
                5,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Storage { read_only: true },
                false,
                std::num::NonZeroU64::new(32),
            ),
            crate::bindings::texture(6, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, true),
            crate::bindings::texture(13, wgpu::ShaderStages::FRAGMENT, true),
            crate::bindings::buffer(
                7,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Storage { read_only: true },
                false,
                std::num::NonZeroU64::new(20),
            ),
            crate::bindings::buffer(
                9,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform,
                false,
                std::num::NonZeroU64::new(32),
            ),
            crate::bindings::buffer(
                8,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform,
                false,
                std::num::NonZeroU64::new(16),
            ),
            crate::bindings::buffer(
                12,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::BufferBindingType::Uniform,
                false,
                std::num::NonZeroU64::new(crate::present_screen::UNIFORM_SIZE),
            ),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport shader"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "const VIEW_FLOAT16:bool={};\nconst VIEW_WHITE_SCALE:f32={};\nconst VIEW_PQ:bool={};\nconst VIEW_EXTENDED_SRGB:bool={};\nconst SCREEN_SAMPLE_STRIDE:u32={}u;\nconst CANVAS_SPACE:u32={}u;\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
                    format == wgpu::TextureFormat::Rgba16Float,
                    if color == SdrSurfaceColor::WindowsScrgb { 2.5375 } else { 1. },
                    color == SdrSurfaceColor::Bt2100Pq,
                    color == SdrSurfaceColor::ExtendedSrgb,
                    crate::present_screen::SAMPLE_STRIDE,
                    crate::working_color::space_id(device.working_space()),
                    crate::view_color::matrix_shader("view_bt2020", layer_core::color::hdr::srgb_to_bt2020()),
                    crate::view_color::shader(device.working_space(), color.primaries()),
                    include_str!("sdr_color.wgsl"),
                    crate::view_color::hdr_shader(device.working_space(), color.primaries()),
                    include_str!("hdr_view.wgsl"),
                    concat!(include_str!("tetrahedron.wgsl"), "\n", include_str!("proof_view.wgsl")),
                    concat!(include_str!("area_sample.wgsl"), "\n", include_str!("mapped_sample.wgsl"), "\n", include_str!("present.wgsl")).replace("resample.", "camera.mapped."),
                    include_str!("present_screen.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = ["fs_main", "fs_placed", "fs_mapped"].map(|entry|
            surface_pipeline(device, "viewport presentation", &pipeline_layout, &shader, ["vs_main", entry], None, format, None));
        let cursor_pipeline = surface_pipeline(device, "display-only cursor", &pipeline_layout, &shader, ["cursor_vertex", "cursor_fragment"],
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<CursorSegment>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32, 3 => Float32, 4 => Float32],
            }),
            format, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));
        let cursor_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cursor segments"),
            size: 256 * std::mem::size_of::<CursorSegment>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport camera"),
            size: SOURCE_CAMERA as u64 + CAMERA_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let disabled_local_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("disabled local tone guide"),
            size: 32,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            uniform,
            proof_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("disabled proof samples"),
                size: 20,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            proof_uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("proof viewing options"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            hdr_uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("HDR viewing options"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            hdr_options: [0.; 8],
            screen_uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("screen gamut check"),
                size: crate::present_screen::UNIFORM_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            screen_options: [0.; 16],
            screen_counter: None,
            local_buffer: disabled_local_buffer.clone(),
            disabled_local_buffer,
            gpu_local_guide: None,
            proof_options: [0; 4],
            timing: None,
            proof_lut: None,
            bind_group: None,
            selection_buffer: None,
            saved_selection_buffer: None,
            empty_saved_selection: device.create_texture(&wgpu::TextureDescriptor {label:Some("empty saved overlay"),size:wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::R32Uint,usage:wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[]}).create_view(&Default::default()),
            composite_view: None,
            coarse_view: None,
            next_view: None,
            display_geometry: None,

            document_extent: [0; 2],
            encode_srgb: color
                .shader_encoding(format)
                .expect("view encoding was validated"),
            corner_radius: 0.0,
            picker: Default::default(),
            cursor_pipeline,
            cursor_buffer,
            cursor_vertices: Vec::with_capacity(256),
            uploads: Uploads::new(device, 16 * 1024),
            camera_data: None,
            quarter_turns: 0,
            retained: false,
            history: Default::default(),
            presented_view: None,
            overlays_changed: false,
            backdrop: None,
            source_camera: None,
            presented_area: 0,
            shader,
            pipeline_layout,
            format,
            color,
            overview_pipeline: None,
            navigator_view: None,
            overview_buffer: None,
            overviews: Vec::new(),
            overviews_changed: false,
            standalone_overview: false,
        }
    }

    /// Opt-in startup preparation. Hosts without overviews do not
    /// compile this pipeline or allocate overview buffers.
    pub fn prepare_overviews(&mut self, renderer: &WgpuRasterizer) {
        if self.overview_pipeline.is_some() {
            return;
        }
        let device = &renderer.device;
        let instance = wgpu::VertexBufferLayout {
            array_stride: 96,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4],
        };
        let blend = wgpu::BlendState {
            color: wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.color,
            // The canvas already owns window coverage. Reapplying
            // alpha blending here would thicken antialiased corners.
            alpha: if self.standalone_overview {
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.alpha
            } else {
                wgpu::BlendComponent { src_factor:wgpu::BlendFactor::Zero, dst_factor:wgpu::BlendFactor::One, operation:wgpu::BlendOperation::Add }
            },
        };
        self.overview_pipeline = Some(surface_pipeline(device, "in-surface document overviews", &self.pipeline_layout, &self.shader,
            ["overview_vertex", "overview_fragment"], Some(instance), self.format, Some(blend)));
    }

    /// Reuses the composition, bindings and current presentation pass. An
    /// unchanged placement uploads nothing; painting never exports preview pixels.
    pub fn set_overviews(&mut self, renderer: &WgpuRasterizer, placements: &[OverviewPlacement]) {
        let data = placements.iter().filter_map(|p| p.packed());
        if self.overviews.iter().copied().eq(data.clone()) {
            return;
        }
        self.overviews.clear();
        self.overviews.extend(data);
        self.overviews_changed = true;
        if !self.overviews.is_empty() {
            self.prepare_overviews(renderer);
        }
        let device = &renderer.device;
        let size = std::mem::size_of_val(self.overviews.as_slice()) as u64;
        if size > 0
            && self
                .overview_buffer
                .as_ref()
                .is_none_or(|b| b.size() < size)
        {
            self.overview_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overview placements"),
                size: size.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }

    /// Native child surfaces do not inherit the parent window's rounded clip.
    /// Zero (the default on web) keeps an opaque rectangular viewport.
    pub fn set_corner_radius(&mut self, physical_pixels: f32) {
        self.corner_radius = physical_pixels.max(0.0);
    }

    pub fn retains_target(&self) -> bool { self.retained }

    pub fn set_backdrop(&mut self, renderer: &WgpuRasterizer, regions: &[BackdropRegion], style: BackdropBlurStyle, hold: bool) {
        if regions.is_empty() && self.backdrop.is_none() {
            return;
        }
        let backdrop = self
            .backdrop
            .get_or_insert_with(|| crate::backdrop_blur::BackdropBlur::new(&renderer.device, self.format));
        self.overlays_changed |= backdrop.regions() != regions || backdrop.style() != style;
        backdrop.set_style(style);
        backdrop.set_regions(regions);
        backdrop.set_hold(hold);
    }

    pub fn inherit_backdrop(&mut self, renderer: &WgpuRasterizer, other: &Self) {
        if let Some(backdrop) = &other.backdrop {
            self.set_backdrop(renderer, backdrop.regions(), backdrop.style(), false);
        }
    }

    pub fn backdrop_frames(&self) -> [u64; 2] {
        self.backdrop.as_ref().map_or([0; 2], |b| b.frames())
    }

    pub fn damage_area_pixels(&self) -> u64 { self.presented_area }

    /// Reset destination history on every swapchain reconfiguration. Retention
    /// requires the same image to survive presentations; buffered images redraw fully.
    pub fn set_target_retention(&mut self, retained: bool) {
        self.retained = retained;
        self.history = Default::default();
    }

    pub fn set_color_picker(&mut self, renderer: &WgpuRasterizer, overlay: Option<layer_render::ColorPickerOverlay>) {
        self.picker.set(overlay, renderer.device(), &self.shader, &self.pipeline_layout, self.format);
    }

    pub fn set_cursor(&mut self, device: &wgpu::Device, segments: &[CursorSegment], scale: f32) {
        let vertices = segments.iter().map(|s| CursorSegment {
                from: s.from.map(|v| v * scale),
                to: s.to.map(|v| v * scale),
                distance: s.distance * scale,
                marker: s.marker,
                scale,
            });
        if self.cursor_vertices.iter().copied().eq(vertices.clone()) { return; }
        self.overlays_changed = true;
        self.cursor_vertices.clear();
        self.cursor_vertices.extend(vertices);
        let size = std::mem::size_of_val(self.cursor_vertices.as_slice()) as u64;
        if self.cursor_buffer.size() < size {
            self.cursor_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cursor segments"),
                size: size.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
    }

    pub fn needs_present(&self, renderer: &WgpuRasterizer, view: ViewState, surround_linear: [f32; 4]) -> bool {
        let Some(cache) = &renderer.scale_display else { return false; };
        let previous = &self.history;
        !previous.valid
            || self.presented_view != Some((view, surround_linear, self.quarter_turns, self.corner_radius))
            || (previous.revision != renderer.composite_revision
                && (previous.artwork_revision != renderer.artwork_revision || !cache.has_pending_work(renderer)))
            || previous.selection_revision != renderer.selection_paint_revision
            || previous.outline_revision != renderer.display_selection_revision
            || previous.hdr != self.hdr_options
            || previous.proof != self.proof_options
            || previous.screen != self.screen_options
            || self.overlays_changed
            || self.picker.changed()
            || self.camera_data.is_none_or(|old| old[62..64] != renderer.clipping_preview.map(f32::from))
            || (!self.overviews.is_empty() && previous.navigator_revision != renderer.navigator.revision)
            || previous.overviews != self.overviews
            || self.backdrop.as_ref().is_some_and(|b| b.needs_refresh())
            || self.bind_group.is_none()
            || self.document_extent != renderer.document_extent
            || self.composite_view.as_ref() != Some(cache.view())
            || self.coarse_view.as_ref() != Some(cache.coarse_view())
            || self.next_view.as_ref() != Some(cache.next_view())
            || self.display_geometry.as_ref() != Some(&cache.geometry)
            || self.selection_buffer.as_ref() != Some(renderer.display_selection.as_ref().map_or(&renderer.unclipped, |(_, b)| b))
            || self.saved_selection_buffer.as_ref() != Some(renderer.selection_previews.texture.as_ref().unwrap_or(&self.empty_saved_selection))
    }

    /// The target is a toolkit-owned framebuffer or acquired surface texture.
    /// No pixel readback, GPU completion wait, or document re-rasterization.
    pub fn present(
        &mut self,
        renderer: &WgpuRasterizer,
        target: &wgpu::TextureView,
        view: ViewState,
        surround_linear: [f32; 4],
    ) -> Result<(), GpuRasterError> {
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewport presentation"),
            });
        self.encode(renderer, &mut encoder, target, view, surround_linear)?;
        renderer.queue.submit([encoder.finish()]);
        if let Some(timer) = &mut self.timing {
            timer.submitted(renderer.queue());
        }
        Ok(())
    }

    /// Rotate the logical viewport into a display's native buffer orientation.
    /// Cursor, selection and Navigator coordinates remain in logical pixels.
    pub fn set_surface_rotation(&mut self, clockwise_quarter_turns: u32) {
        self.quarter_turns = clockwise_quarter_turns % 4;
    }

    /// Encode into the host's submission, allowing native GPU interop barriers
    /// and completion signals to surround the same viewport pass.
    pub fn encode(
        &mut self,
        renderer: &WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: ViewState,
        surround_linear: [f32; 4],
    ) -> Result<(), GpuRasterError> {
        self.encode_content(renderer, encoder, target, view, surround_linear, false)
    }

    /// Render a retained native Navigator surface from the current GPU image.
    /// Native placement and clipping can then move it without another GPU pass.
    pub fn present_overviews(
        &mut self,
        renderer: &WgpuRasterizer,
        target: &wgpu::TextureView,
        extent: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        assert!(
            self.standalone_overview,
            "use ViewportPresenter::for_overview_surface"
        );
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        self.encode_content(
            renderer,
            &mut encoder,
            target,
            ViewState {
                width_px: extent[0],
                height_px: extent[1],
                document_to_surface: [1., 0., 0., 1., 0., 0.],
                background_rgba_linear: [0.; 4],
            },
            [0.; 4],
            true,
        )?;
        renderer.queue.submit([encoder.finish()]);
        if let Some(timer) = &mut self.timing {
            timer.submitted(renderer.queue());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_content(
        &mut self,
        renderer: &WgpuRasterizer,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: ViewState,
        surround_linear: [f32; 4],
        overview_only: bool,
    ) -> Result<(), GpuRasterError> {
        let Some(cache) = &renderer.scale_display else { return Ok(()); };
        let navigator = renderer.navigator.view().unwrap_or(&renderer.empty_view);
        let composite = cache.view();
        let coarse = cache.coarse_view();
        let next = cache.next_view();
        let geometry = &cache.geometry;
        let device = &renderer.device;
        let selection = renderer.display_selection.as_ref();
        let coverage = selection.map_or(&renderer.unclipped, |(_, buffer)| buffer);
        let saved = renderer.selection_previews.texture.as_ref().unwrap_or(&self.empty_saved_selection);
        let selection_changed = self.selection_buffer.as_ref() != Some(coverage);
        let bindings_changed = self.bind_group.is_none()
            || self.navigator_view.as_ref() != Some(navigator)
            || self.saved_selection_buffer.as_ref() != Some(saved)
            || self.document_extent != renderer.document_extent
            || self.composite_view.as_ref() != Some(composite)
            || self.coarse_view.as_ref() != Some(coarse)
            || self.next_view.as_ref() != Some(next)
            || self.display_geometry.as_ref() != Some(geometry);
        if bindings_changed || selection_changed {
            self.bind_group = Some(crate::bindings::group(device, "viewport composite", &self.layout, [
                wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &self.uniform,
                    offset: 0,
                    size: wgpu::BufferSize::new(CAMERA_SIZE),
                }),
                wgpu::BindingResource::TextureView(composite),
                wgpu::BindingResource::Sampler(&renderer.sampler),
                coverage.as_entire_binding(),
                wgpu::BindingResource::TextureView(coarse),
                geometry.as_entire_binding(),
                wgpu::BindingResource::TextureView(next),
                self.proof_buffer.as_entire_binding(),
                self.proof_uniform.as_entire_binding(),
                self.hdr_uniform.as_entire_binding(),
                self.local_buffer.as_entire_binding(),
                wgpu::BindingResource::TextureView(saved),
                self.screen_uniform.as_entire_binding(),
                wgpu::BindingResource::TextureView(navigator),
            ]));
            self.navigator_view = Some(navigator.clone());
            self.document_extent = renderer.document_extent;
            self.selection_buffer = Some(coverage.clone());
            self.saved_selection_buffer = Some(saved.clone());
            self.composite_view = Some(composite.clone());
            self.coarse_view = Some(coarse.clone());
            self.next_view = Some(next.clone());
            self.display_geometry = Some(geometry.clone());
        }
        let [a, b, c, d, tx, ty] = view.document_to_surface;
        let det = a * d - b * c;
        if !det.is_finite() || det.abs() < 1.0e-12 {
            return Ok(());
        }
        let inverse = selection
            .map_or(layer_core::Affine::IDENTITY, |(s, _)| {
                if matches!(s.shape, layer_core::SelectionShape::Pixels(_)) {
                    s.affine.inverse().expect("selection placement validated")
                } else { layer_core::Affine::IDENTITY }
            })
            .0;
        let overlay = renderer.selection_overlay;
        let overlay_color = overlay.map_or([0.;4],|o| o.color);
        let crop = renderer.crop_overlay.filter(|c| c.to_crop.inverse().is_some());
        let [ca, cb, cc, cd, cx, cy] = crop.map_or([0.; 6], |c| c.to_crop.0);
        let mut data = [0.; 128];
        data[..40].copy_from_slice(&[
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * ty - d * tx) / det,
            (b * tx - a * ty) / det,
            self.document_extent[0] as f32,
            self.document_extent[1] as f32,
            view.width_px as f32,
            view.height_px as f32,
            f32::from(self.encode_srgb),
            self.corner_radius,
            surround_linear[0],
            surround_linear[1],
            surround_linear[2],
            surround_linear[3],
            inverse[4],
            inverse[5],
            f32::from(selection.is_some()),
            selection.map_or(0., |(s, _)| f32::from(s.inverted)),
            inverse[0],
            inverse[1],
            inverse[2],
            inverse[3],
            self.quarter_turns as f32, overlay.filter(|o|o.active).map_or(0.,|o| if o.protected { 2. } else { 1. }), f32::from(renderer.selection_previews.buffer.is_some()), 1.,
            overlay_color[0], overlay_color[1], overlay_color[2], overlay_color[3],
            ca, cb, cc, cd,
            cx, cy, crop.map_or(0., |c| c.dim.clamp(0., 1.)), f32::from(crop.is_some()),
        ]);
        data[40..60].copy_from_slice(&cache.placement_values());
        for (value, bytes) in data[64..].iter_mut().zip(cache.resample_values().chunks_exact(4)) {
            *value = f32::from_le_bytes(bytes.try_into().unwrap());
        }
        data[60] = f32::from(renderer.blend_space == layer_core::BlendSpace::Perceptual);
        data[61] = renderer.navigator.scale();
        data[62..64].copy_from_slice(&renderer.clipping_preview.map(f32::from));
        // A fixed f32 array has no padding or uninitialized bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), std::mem::size_of_val(&data))
        };
        let placement = 16..24;
        let camera_changed = self.camera_data.is_none_or(|old| {
            old[..placement.start] != data[..placement.start] || old[placement.end..40] != data[placement.end..40]
        });
        let selection_changed = selection_changed
            || self.camera_data.is_some_and(|old| old[placement.clone()] != data[placement]);
        let artwork_changed = self.camera_data.is_none_or(|old| old[40..] != data[40..]);
        if camera_changed || selection_changed || artwork_changed {
            self.uploads
                .write(encoder, &self.uniform, bytes)?;
            self.camera_data = Some(data);
        }
        let source_camera = (self.backdrop.is_some() && !overview_only).then(|| {
            let mut source = data;
            source[27] = 4.;
            source
        });
        if let Some(source) = source_camera.filter(|s| self.source_camera != Some(*s)) {
            let bytes = unsafe {
                std::slice::from_raw_parts(source.as_ptr().cast::<u8>(), std::mem::size_of_val(&source))
            };
            self.uploads
                .write_at(encoder, &self.uniform, SOURCE_CAMERA.into(), bytes)?;
            self.source_camera = Some(source);
        }
        self.picker.upload(&mut self.uploads, encoder)?;
        if !self.cursor_vertices.is_empty() {
            // repr(C) contains only initialized f32s, without padding.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    self.cursor_vertices.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(self.cursor_vertices.as_slice()),
                )
            };
            self.uploads
                .write(encoder, &self.cursor_buffer, bytes)?;
        }
        if self.overviews_changed && !self.overviews.is_empty() {
            // Fixed initialized f32 arrays, with no struct padding.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    self.overviews.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(self.overviews.as_slice()),
                )
            };
            self.uploads.write(
                encoder,
                self.overview_buffer.as_ref().unwrap(),
                bytes,
            )?;
            self.overviews_changed = false;
        }
        let extent = if self.quarter_turns.is_multiple_of(2) {
            [view.width_px, view.height_px]
        } else {
            [view.height_px, view.width_px]
        };
        // A retained target may be scanned out while this pass runs. Even a
        // full camera redraw must preserve its old pixels until the fullscreen
        // shader replaces them; a fast clear can otherwise flash on screen.
        let preserve_target = !overview_only && self.retained && self.history.valid;
        let (mut regions, full, content_damage, moved) = if !overview_only {
            let previous = &self.history;
            // A displayed selection replaced once since the last present
            // repaints where either one has coverage or an outline.
            let outline = (previous.outline_revision != renderer.display_selection_revision)
                .then_some(renderer.display_selection_damage)
                .filter(|_| previous.outline_revision.wrapping_add(1) == renderer.display_selection_revision);
            let content = !previous.valid
                || previous.hdr != self.hdr_options
                || previous.proof != self.proof_options
                || previous.screen != self.screen_options
                || (previous.selection_revision != renderer.selection_paint_revision
                    && previous.selection_revision.wrapping_add(1) != renderer.selection_paint_revision)
                || (previous.revision != renderer.composite_revision
                    && previous.revision.wrapping_add(1) != renderer.composite_revision);
            let full = content || bindings_changed || camera_changed || (selection_changed && outline.is_none());
            let cursor =
                crate::present_damage::cursor_bounds(&self.cursor_vertices, view, self.quarter_turns);
            let repaint = if full {
                crate::pixel_rect::PixelRect::full(extent)
            } else if previous.revision != renderer.composite_revision {
                crate::present_damage::damage(renderer.composite_damage, view, self.quarter_turns)
            } else {
                crate::pixel_rect::PixelRect::EMPTY
            };
            if crate::performance_trace::enabled() {
                crate::performance_trace::counter(c"Capy viewport full redraw", u64::from(full));
                crate::performance_trace::counter(c"Capy viewport bindings changed", u64::from(bindings_changed));
                crate::performance_trace::counter(c"Capy viewport artwork pixels", repaint.area());
                crate::performance_trace::counter(c"Capy viewport cursor pixels", cursor.area());
            }
            let mut regions = Vec::with_capacity(5 + 2 * self.overviews.len());
            for bounds in previous.picker.into_iter().chain(self.picker.bounds()) {
                crate::present_damage::add_region(&mut regions, crate::present_damage::surface_bounds(bounds, view, self.quarter_turns));
            }
            crate::present_damage::add_region(&mut regions, repaint);
            if !full && previous.selection_revision != renderer.selection_paint_revision {
                crate::present_damage::add_region(&mut regions,
                    crate::present_damage::damage(renderer.selection_paint_damage, view, self.quarter_turns));
            }
            let outline = outline.filter(|_| !full).map(|area| crate::present_damage::damage(area, view, self.quarter_turns));
            crate::performance_trace::counter(c"Capy viewport outline pixels", outline.map_or(0, |r| r.area()));
            if let Some(area) = outline {
                crate::present_damage::add_region(&mut regions, area);
            }
            crate::present_damage::add_region(&mut regions, previous.cursor);
            crate::present_damage::add_region(&mut regions, cursor);
            let content_damage = (!full).then(|| {
                let selection = (previous.selection_revision != renderer.selection_paint_revision)
                    .then(|| crate::present_damage::damage(renderer.selection_paint_damage, view, self.quarter_turns));
                [repaint].into_iter().chain(selection).chain(outline).filter(|r| !r.is_empty()).collect::<Vec<_>>()
            });
            // A distant Navigator must not turn a short stroke into a nearly
            // full-screen render area. Only merge intersecting damage regions.
            if previous.overviews != self.overviews {
                for o in previous.overviews.iter().chain(self.overviews.iter()) {
                    let pad = o[19] * 3. + 2.;
                    crate::present_damage::add_region(
                        &mut regions,
                        crate::present_damage::surface_bounds(
                            [o[0] - pad, o[1] - pad, o[2] + 2. * pad, o[3] + 2. * pad],
                            view,
                            self.quarter_turns,
                        ),
                    );
                }
            } else if previous.navigator_revision != renderer.navigator.revision {
                for o in &self.overviews {
                    crate::present_damage::add_region(&mut regions,
                        crate::present_damage::surface_bounds([o[0], o[1], o[2], o[3]], view, self.quarter_turns)
                            .intersect(crate::present_damage::surface_bounds([o[20], o[21], o[22], o[23]], view, self.quarter_turns)));
                }
            }
            let previous = &mut self.history;
            previous.valid = true;
            previous.revision = renderer.composite_revision;
            previous.artwork_revision = renderer.artwork_revision;
            previous.navigator_revision = renderer.navigator.revision;
            previous.selection_revision = renderer.selection_paint_revision;
            previous.outline_revision = renderer.display_selection_revision;
            previous.hdr = self.hdr_options;
            previous.proof = self.proof_options;
            previous.screen = self.screen_options;
            previous.cursor = cursor;
            previous.picker = self.picker.bounds();
            previous.overviews.clone_from(&self.overviews);
            (regions, full, content_damage, camera_changed && !bindings_changed && !content)
        } else {
            (vec![crate::pixel_rect::PixelRect::full(extent)], true, None, false)
        };
        let timestamp_writes = if regions.is_empty() && self.retained {
            None
        } else {
            self.timing.as_mut().and_then(|timer| {
                timer.poll(renderer.device(), renderer.queue());
                timer.begin_render_pass(timer.stats().requested)
            })
        };
        let mut began = false;
        let pipeline = &self.pipeline[data[53] as usize];
        if let Some(backdrop) = self.backdrop.as_mut().filter(|_| !overview_only) {
            let group = self.bind_group.as_ref().unwrap();
            let mut glass = Vec::new();
            began = backdrop.encode(
                renderer,
                encoder,
                [view.width_px, view.height_px],
                self.quarter_turns,
                layer_core::Affine(view.document_to_surface),
                moved,
                content_damage.as_deref(),
                |pass| {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, group, &[SOURCE_CAMERA]);
                    pass.draw(0..3, 0..1);
                },
                timestamp_writes.as_ref().map(|t| wgpu::RenderPassTimestampWrites { end_of_pass_write_index: None, ..t.clone() }),
                &mut glass,
            );
            crate::performance_trace::counter(c"Capy viewport glass pixels", glass.iter().map(|r| r.area()).sum());
            if !full {
                for area in glass {
                    crate::present_damage::add_region(&mut regions, area);
                }
            }
        }
        let (regions, full) = if self.retained || overview_only {
            (regions, full)
        } else {
            (vec![crate::pixel_rect::PixelRect::full(extent)], true)
        };
        let size = target.texture().size();
        let bounds = crate::pixel_rect::PixelRect::full([size.width, size.height]);
        let regions: Vec<_> = regions.into_iter().map(|r| r.intersect(bounds)).filter(|r| !r.is_empty()).collect();
        self.presented_area = regions.iter().map(|r| r.area()).sum();
        crate::performance_trace::counter(c"Capy viewport regions", regions.len() as u64);
        for (index, repaint) in regions.iter().enumerate() {
            // Each pass needs its own view: wgpu can defer encoding until
            // finish(), so mutating one view would reuse the last area.
            let region_view = (!full).then(|| target.texture().create_view(&Default::default()));
            let pass_target = region_view.as_ref().unwrap_or(target);
            // Scissoring alone does not limit attachment loads/stores on a
            // tile renderer. Narrow the native render area as well.
            #[cfg(any(target_os = "android", target_os = "linux"))]
            if !full
                && let Some(target) = unsafe { pass_target.as_hal::<wgpu::hal::api::Vulkan>() } {
                    unsafe {
                        target.set_retained_render_area([
                            repaint.min_x(),
                            repaint.min_y(),
                            repaint.width(),
                            repaint.height(),
                        ]);
                    }
                }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: pass_target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if preserve_target {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(if overview_only { wgpu::Color::TRANSPARENT } else { wgpu::Color::BLACK })
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timestamp_writes.as_ref()
                    .filter(|_| (index == 0 && !began) || index + 1 == regions.len()).map(|t| {
                    wgpu::RenderPassTimestampWrites {
                        query_set: t.query_set,
                        beginning_of_pass_write_index: if index == 0 && !began {
                            t.beginning_of_pass_write_index
                        } else {
                            None
                        },
                        end_of_pass_write_index: if index + 1 == regions.len() {
                            t.end_of_pass_write_index
                        } else {
                            None
                        },
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(
                repaint.min_x(),
                repaint.min_y(),
                repaint.width(),
                repaint.height(),
            );
            pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[0]);
            if !overview_only {
                pass.set_pipeline(pipeline);
                let mut visible = vec![*repaint];
                for hole in self.backdrop.iter().flat_map(|b| b.interiors()) {
                    visible = visible.into_iter().flat_map(|r| r.subtract(*hole)).filter(|r| !r.is_empty()).collect();
                }
                for area in visible {
                    pass.set_scissor_rect(area.min_x(), area.min_y(), area.width(), area.height());
                    pass.draw(0..3, 0..1);
                }
                pass.set_scissor_rect(repaint.min_x(), repaint.min_y(), repaint.width(), repaint.height());
                self.picker.draw(&mut pass);
                if !self.cursor_vertices.is_empty() {
                    pass.set_pipeline(&self.cursor_pipeline);
                    pass.set_vertex_buffer(0, self.cursor_buffer.slice(..));
                    pass.draw(0..6, 0..self.cursor_vertices.len() as u32);
                }
                if let Some(backdrop) = &self.backdrop {
                    backdrop.draw(&mut pass, *repaint);
                    pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[0]);
                }
            }
            if !self.overviews.is_empty() {
                pass.set_pipeline(self.overview_pipeline.as_ref().unwrap());
                pass.set_vertex_buffer(0, self.overview_buffer.as_ref().unwrap().slice(..));
                pass.draw(0..6, 0..self.overviews.len() as u32);
            }
        }
        // Return upload chunks only after this encoder's GPU work completes.
        // Works both for present() and hosts submitting encode() themselves.
        self.uploads.finish(encoder);
        self.presented_view = Some((view, surround_linear, self.quarter_turns, self.corner_radius));
        self.overlays_changed = false;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn surface_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entries: [&str; 2],
    instance: Option<wgpu::VertexBufferLayout<'_>>,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some(entries[0]), compilation_options: Default::default(), buffers: &[instance] },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entries[1]),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overview_records_reject_invalid_geometry() {
        let p = OverviewPlacement {
            bounds: [0., 0., 100., 75.],
            clip: None,
            work_area: [[0.; 2]; 4],
            outline_linear: [0.; 3],
            background_linear: [0.; 3],
            scale: 1.,
            opacity: 1.,
        };
        assert!(p.packed().is_some());
        assert!(
            OverviewPlacement {
                bounds: [0., 0., 0., 75.],
                ..p
            }
            .packed()
            .is_none()
        );
        assert!(
            OverviewPlacement {
                scale: f32::NAN,
                ..p
            }
            .packed()
            .is_none()
        );
        assert!(
            OverviewPlacement {
                work_area: [[f32::INFINITY, 0.]; 4],
                ..p
            }
            .packed()
            .is_none()
        );
    }
}
