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
    device: wgpu::Device,
    working_space: layer_core::color::RgbSpace,
    depth: layer_core::color::SampleDepth,
    pub source_samples: std::sync::Arc<layer_core::raster::DecodedTileCache>,
    pub blend_pipelines: std::sync::Arc<std::sync::Mutex<std::sync::Weak<super::portable_blend::Pipelines>>>,
    pub tone_pipelines: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::analysis_compute::Pipelines>>>,
    pub dehaze_pipelines: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::analysis_compute::Pipelines>>>,
    pub bounds_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::thumbnails::BoundsPipeline>>>,
    pub levels_pipeline:std::sync::Arc<std::sync::OnceLock<std::sync::Arc<crate::snapshot::levels::LevelsPipeline>>>,
    pub statistics_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<crate::snapshot::statistics::StatisticsPipeline>>>,
    pub sample_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::snapshot::sample::SamplePipeline>>>,
    pub analysis_memory: std::sync::Arc<std::sync::Mutex<u64>>,
    pub effect_resources: std::sync::Arc<std::sync::Mutex<super::effects::resources::Cache>>,
    pub native_publication: std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<super::raster::native_edit::Recipes>>>>,
    pub native_transfers: std::sync::Arc<std::sync::Mutex<super::native_tiles::transfer::Tables>>,
    #[cfg(not(target_arch = "wasm32"))]
    cache: Option<std::sync::Arc<super::shader_cache::Cache>>,
}
impl From<wgpu::Device> for PipelineDevice {
    fn from(device: wgpu::Device) -> Self {
        Self {
            device,
            tone_pipelines: Default::default(),
            dehaze_pipelines: Default::default(),
            bounds_pipeline: Default::default(),
            sample_pipeline: Default::default(),
            statistics_pipeline: Default::default(),
            levels_pipeline:Default::default(),
            native_transfers: Default::default(),
            native_publication: Default::default(),
            effect_resources: Default::default(),
            analysis_memory: Default::default(),
            blend_pipelines: Default::default(),
            working_space: Default::default(),
            depth: Default::default(),
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
    pub fn for_recipe(&self) -> Self {
        Self { working_space: self.working_space, depth: self.depth,
            #[cfg(not(target_arch = "wasm32"))]
            cache: self.cache.clone(), ..Self::from(self.device.clone()) }
    }
    /// A working attachment choice, independent of native integer backing and
    /// document primaries. All deferred recipes retain this same choice.
    pub fn working_format(&self) -> wgpu::TextureFormat {
        wgpu::TextureFormat::Rgba32Float
    }
    pub fn hdr(&self) -> bool { self.depth.is_float() }
    pub fn depth(&self) -> layer_core::color::SampleDepth {self.depth}
    pub fn with_depth(mut self, depth:layer_core::color::SampleDepth) -> Self {self.depth=depth;self}
    pub fn working_space(&self) -> layer_core::color::RgbSpace {
        self.working_space
    }
    pub fn with_working_space(mut self, space: layer_core::color::RgbSpace) -> Self {
        self.working_space = space;
        self
    }
    pub fn portable_blend(&self) -> bool {
        !self.features().contains(wgpu::Features::FLOAT32_BLENDABLE)
    }
    pub fn attachment_blend(&self, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>) -> Option<wgpu::BlendState> {
        if matches!(format, wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::Rg32Float | wgpu::TextureFormat::R32Float)
            && !self.features().contains(wgpu::Features::FLOAT32_BLENDABLE) { None } else { blend }
    }
    pub fn scalar_format(&self) -> wgpu::TextureFormat {
        wgpu::TextureFormat::R32Float
    }
    pub fn require_float32(self) -> Result<Self, super::GpuRasterError> {
        if !self.features().contains(wgpu::Features::FLOAT32_FILTERABLE) {
            return Err(super::GpuRasterError::Color(
                "This device cannot sample Float32 working tiles".into(),
            ));
        }
        Ok(self)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cached(
        device: wgpu::Device,
        adapter: &wgpu::Adapter,
        directory: Option<&std::path::Path>,
    ) -> Self {
        let cache = directory
            .and_then(|directory| super::shader_cache::Cache::open(&device, &adapter.get_info(), directory))
            .map(std::sync::Arc::new);
        Self { cache, ..Self::from(device) }
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
    #[cfg(not(target_arch = "wasm32"))]
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
    #[cfg(not(target_arch = "wasm32"))]
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
