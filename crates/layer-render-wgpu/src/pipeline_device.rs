//! Keep the optional startup cache attached to every pipeline recipe, including
//! recipes sent to the compiler. Ordinary GPU resource creation dereferences to
//! wgpu unchanged. Other hosts use an uncached device.
#[cfg(target_os = "windows")]
struct CompileTrace<'a>(Option<(std::time::Instant, Option<&'a str>)>);
#[cfg(target_os = "windows")]
impl<'a> CompileTrace<'a> {
    fn new(label: Option<&'a str>) -> Self {
        Self(std::env::var_os("CAPY_TRACE_SHADER_JOBS").map(|_| {
            eprintln!("pipeline begin label={label:?}");
            (std::time::Instant::now(), label)
        }))
    }
}
#[cfg(target_os = "windows")]
impl Drop for CompileTrace<'_> {
    fn drop(&mut self) {
        if let Some((start, label)) = self.0 {
            eprintln!(
                "pipeline end label={label:?} elapsed_ms={:.3}",
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
}
#[derive(Clone)]
pub(crate) struct PipelineDevice {
    pub demand_shaders: bool,
    device: wgpu::Device,
    working_format: wgpu::TextureFormat,
    working_space: layer_core::color::RgbSpace,
    hdr: bool,
    pub source_samples: std::sync::Arc<layer_core::raster::DecodedTileCache>,
    pub blend_pipelines: std::sync::Arc<std::sync::Mutex<std::sync::Weak<super::portable_blend::Pipelines>>>,
    pub tone_pipelines: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::local_tone::Pipelines>>>,
    #[cfg(not(target_arch = "wasm32"))]
    cache: Option<std::sync::Arc<super::shader_cache::Cache>>,
}
impl From<wgpu::Device> for PipelineDevice {
    fn from(device: wgpu::Device) -> Self {
        Self {
            demand_shaders: cfg!(target_arch = "wasm32"),
            device,
            tone_pipelines: Default::default(),
            blend_pipelines: Default::default(),
            working_format: super::SRGB8_FORMAT,
            working_space: Default::default(),
            hdr: false,
            source_samples: std::sync::Arc::new(layer_core::raster::DecodedTileCache::new(
                512 * 1024 * 1024,
            )),
            #[cfg(not(target_arch = "wasm32"))]
            cache: None,
        }
    }
}
impl std::ops::Deref for PipelineDevice {
    type Target = wgpu::Device;
    fn deref(&self) -> &Self::Target {
        &self.device
    }
}
impl PipelineDevice {
    /// A working attachment choice, independent of native integer backing and
    /// document primaries. All deferred recipes retain this same choice.
    pub fn working_format(&self) -> wgpu::TextureFormat {
        self.working_format
    }
    pub fn hdr(&self) -> bool { self.hdr }
    pub fn with_hdr(mut self, hdr: bool) -> Self { self.hdr = hdr; self }
    pub fn working_space(&self) -> layer_core::color::RgbSpace {
        self.working_space
    }
    pub fn with_working_space(mut self, space: layer_core::color::RgbSpace) -> Self {
        self.working_space = space;
        self
    }
    pub fn portable_blend(&self) -> bool {
        self.working_format == wgpu::TextureFormat::Rgba32Float
            && !self.features().contains(wgpu::Features::FLOAT32_BLENDABLE)
    }
    pub fn attachment_blend(&self, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>) -> Option<wgpu::BlendState> {
        if matches!(format, wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::Rg32Float | wgpu::TextureFormat::R32Float)
            && !self.features().contains(wgpu::Features::FLOAT32_BLENDABLE) { None } else { blend }
    }
    pub fn scalar_format(&self) -> wgpu::TextureFormat {
        if self.working_format == wgpu::TextureFormat::Rgba32Float {
            wgpu::TextureFormat::R32Float
        } else {
            wgpu::TextureFormat::R8Unorm
        }
    }
    pub fn with_working_format(
        mut self,
        format: wgpu::TextureFormat,
    ) -> Result<Self, super::GpuRasterError> {
        let required = match format {
            wgpu::TextureFormat::Rgba8UnormSrgb => wgpu::Features::empty(),
            wgpu::TextureFormat::Rgba32Float => {
                wgpu::Features::FLOAT32_FILTERABLE
            }
            _ => {
                return Err(super::GpuRasterError::Color(
                    "Unsupported working texture format".into(),
                ));
            }
        };
        if !self.features().contains(required) {
            return Err(super::GpuRasterError::Color(
                "This device cannot sample Float32 working tiles".into(),
            ));
        }
        self.working_format = format;
        Ok(self)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cached(
        device: wgpu::Device,
        adapter: &wgpu::Adapter,
        directory: &std::path::Path,
    ) -> Self {
        let cache = super::shader_cache::Cache::open(&device, &adapter.get_info(), directory)
            .map(std::sync::Arc::new);
        Self {
            device,
            cache,
            demand_shaders: false,
            tone_pipelines: Default::default(),
            blend_pipelines: Default::default(),
            working_format: super::SRGB8_FORMAT,
            working_space: Default::default(),
            hdr: false,
            source_samples: std::sync::Arc::new(layer_core::raster::DecodedTileCache::new(
                512 * 1024 * 1024,
            )),
        }
    }
    pub fn create_texture(&self, descriptor: &wgpu::TextureDescriptor<'_>) -> wgpu::Texture {
        let _trace = crate::performance_trace::Span::new(c"capy.allocate_texture");
        self.device.create_texture(descriptor)
    }
    pub fn create_buffer(&self, descriptor: &wgpu::BufferDescriptor<'_>) -> wgpu::Buffer {
        let _trace = crate::performance_trace::Span::new(c"capy.allocate_buffer");
        self.device.create_buffer(descriptor)
    }
    pub fn create_bind_group(&self, descriptor: &wgpu::BindGroupDescriptor<'_>) -> wgpu::BindGroup {
        let _trace = crate::performance_trace::Span::new(c"capy.allocate_binding");
        self.device.create_bind_group(descriptor)
    }
    pub fn create_render_pipeline(
        &self,
        descriptor: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> wgpu::RenderPipeline {
        #[cfg(target_os = "windows")]
        let _trace = CompileTrace::new(descriptor.label);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(cache) = self.cache.as_ref().and_then(|c| c.pipeline()) {
            return self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    cache: Some(&cache),
                    ..descriptor.clone()
                });
        }
        self.device.create_render_pipeline(descriptor)
    }
    pub fn create_compute_pipeline(
        &self,
        descriptor: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> wgpu::ComputePipeline {
        #[cfg(target_os = "windows")]
        let _trace = CompileTrace::new(descriptor.label);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(cache) = self.cache.as_ref().and_then(|c| c.pipeline()) {
            return self
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    cache: Some(&cache),
                    ..descriptor.clone()
                });
        }
        self.device.create_compute_pipeline(descriptor)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish_cache(&self) {
        if let Some(cache) = &self.cache {
            cache.finish();
        }
    }
}
