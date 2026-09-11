//! Keep the optional startup cache attached to every pipeline recipe, including
//! recipes sent to the compiler. Ordinary GPU resource creation dereferences to
//! wgpu unchanged. Other hosts use an uncached device.
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
