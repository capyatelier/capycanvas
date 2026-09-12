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
    #[cfg(not(target_arch = "wasm32"))]
    cache: Option<std::sync::Arc<super::shader_cache::Cache>>,
}
impl From<wgpu::Device> for PipelineDevice {
    fn from(device: wgpu::Device) -> Self {
        Self {
            device,
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
    #[cfg(not(target_arch = "wasm32"))]
    pub fn cached(
        device: wgpu::Device,
        adapter: &wgpu::Adapter,
        directory: &std::path::Path,
    ) -> Self {
        let cache = super::shader_cache::Cache::open(&device, &adapter.get_info(), directory)
            .map(std::sync::Arc::new);
        Self { device, cache }
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
