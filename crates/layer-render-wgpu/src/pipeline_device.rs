//! Shared device resources and working color choices retained by pipeline recipes.
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
#[cfg(target_arch="wasm32")]
thread_local! {static COMPLETION:std::cell::RefCell<(u64,Vec<std::sync::Arc<dyn Fn()+Send+Sync>>)>=Default::default();}
#[derive(Clone)]
pub(crate) struct DeviceRecipes {
    #[cfg(not(target_arch="wasm32"))]
    poll: crate::device_poll::Binding,
    compilation: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<crate::compile::Coordinator>>>>,
    #[cfg(not(target_arch = "wasm32"))]
    saved_pipelines: Option<std::sync::Arc<crate::SavedPipelines>>,
    pub transient_display: std::sync::Arc<crate::residency::Tier>,
    pub transient_memory: Option<layer_core::MemoryOwner>,
    pub bounds_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::thumbnails::BoundsPipeline>>>,
    pub levels_pipeline:std::sync::Arc<std::sync::OnceLock<std::sync::Arc<crate::snapshot::levels::LevelsPipeline>>>,
    pub statistics_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<crate::snapshot::statistics::StatisticsPipeline>>>,
    pub sample_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::snapshot::sample::SamplePipeline>>>,
    pub native_publication: std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<super::raster::native_edit::Recipes>>>>,
    pub native_transfers: std::sync::Arc<std::sync::Mutex<super::native_tiles::transfer::Tables>>,
}
#[cfg(not(target_arch="wasm32"))]
impl DeviceRecipes {
    pub(crate) fn poll_binding(&self) -> crate::device_poll::Binding { self.poll.owned() }
}
impl From<&PipelineDevice> for DeviceRecipes {
    fn from(device: &PipelineDevice) -> Self {
        Self {
            #[cfg(not(target_arch="wasm32"))]
            poll: device.poll.clone(),
            compilation: device.compilation.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            saved_pipelines: device.saved_pipelines.clone(),
            transient_display: device.transient_display.clone(),
            transient_memory: device.transient_memory.clone(),
            bounds_pipeline: device.bounds_pipeline.clone(),
            levels_pipeline: device.levels_pipeline.clone(),
            statistics_pipeline: device.statistics_pipeline.clone(),
            sample_pipeline: device.sample_pipeline.clone(),
            native_publication: device.native_publication.clone(),
            native_transfers: device.native_transfers.clone(),
        }
    }
}
#[derive(Clone)]
pub(crate) struct PipelineDevice {
    #[cfg(not(target_arch="wasm32"))]
    poll: crate::device_poll::Binding,
    compilation: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<crate::compile::Coordinator>>>>,
    #[cfg(not(target_arch = "wasm32"))]
    saved_pipelines: Option<std::sync::Arc<crate::SavedPipelines>>,
    pub transient_display: std::sync::Arc<crate::residency::Tier>,
    pub transient_memory: Option<layer_core::MemoryOwner>,
    device: wgpu::Device,
    working_space: layer_core::color::RgbSpace,
    depth: layer_core::color::SampleDepth,
    pub source_samples: std::sync::Arc<layer_core::raster::DecodedTileCache>,
    pub sample_memory: Option<layer_core::MemoryOwner>,
    pub resource_limit: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub blend_pipelines: std::sync::Arc<std::sync::Mutex<std::sync::Weak<super::portable_blend::Pipelines>>>,
    pub tone_pipelines: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::analysis_compute::Pipelines>>>,
    pub dehaze_pipelines: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::analysis_compute::Pipelines>>>,
    pub bounds_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::thumbnails::BoundsPipeline>>>,
    pub levels_pipeline:std::sync::Arc<std::sync::OnceLock<std::sync::Arc<crate::snapshot::levels::LevelsPipeline>>>,
    pub statistics_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<crate::snapshot::statistics::StatisticsPipeline>>>,
    pub sample_pipeline: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<super::snapshot::sample::SamplePipeline>>>,
    pub analysis_memory: std::sync::Arc<crate::residency::Tier>,
    pub effect_resources: std::sync::Arc<std::sync::Mutex<super::effects::resources::Cache>>,
    pub native_publication: std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<super::raster::native_edit::Recipes>>>>,
    pub native_transfers: std::sync::Arc<std::sync::Mutex<super::native_tiles::transfer::Tables>>,
}
impl From<wgpu::Device> for PipelineDevice {
    fn from(device: wgpu::Device) -> Self {
        Self {
            #[cfg(not(target_arch="wasm32"))]
            poll: Default::default(),
            compilation: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            saved_pipelines: None,
            device,
            transient_display: Default::default(),
            transient_memory: None,
            sample_memory: None,
            resource_limit: Default::default(),
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
                0,
            )),
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
    pub fn with_device_recipes(mut self, recipes: &DeviceRecipes) -> Self {
        #[cfg(not(target_arch="wasm32"))]
        {self.poll=recipes.poll.clone();}
        self.compilation = recipes.compilation.clone();
        #[cfg(not(target_arch = "wasm32"))]
        { self.saved_pipelines = recipes.saved_pipelines.clone(); }
        self.transient_display = recipes.transient_display.clone();
        self.transient_memory = recipes.transient_memory.clone();
        self.bounds_pipeline = recipes.bounds_pipeline.clone();
        self.levels_pipeline = recipes.levels_pipeline.clone();
        self.statistics_pipeline = recipes.statistics_pipeline.clone();
        self.sample_pipeline = recipes.sample_pipeline.clone();
        self.native_publication = recipes.native_publication.clone();
        self.native_transfers = recipes.native_transfers.clone();
        self
    }
    pub fn with_recipes(&self, recipes: &Self) -> Self {
        Self {
            compilation: recipes.compilation.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            saved_pipelines: recipes.saved_pipelines.clone(),
            blend_pipelines: recipes.blend_pipelines.clone(),
            tone_pipelines: recipes.tone_pipelines.clone(),
            dehaze_pipelines: recipes.dehaze_pipelines.clone(),
            ..self.clone()
        }
    }
    pub fn for_recipe(&self) -> Self {
        Self { working_space: self.working_space, depth: self.depth, transient_display: self.transient_display.clone(), transient_memory: self.transient_memory.clone(),
            #[cfg(not(target_arch="wasm32"))]
            poll: self.poll.recipe(),
            #[cfg(not(target_arch = "wasm32"))]
            saved_pipelines: self.saved_pipelines.clone(),
            ..Self::from(self.device.clone()) }
    }
    pub(crate) fn for_compilation(&self) -> Self {
        Self { compilation: Default::default(),
            #[cfg(not(target_arch="wasm32"))]
            poll:self.poll.recipe(),
            ..self.clone() }
    }
    pub(crate) fn for_resource(&self)->Self {
        Self { compilation:Default::default(),..self.clone() }
    }
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn poll_binding(&self)->crate::device_poll::Binding {self.poll.owned()}

    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn bind_queue(&self,queue:&wgpu::Queue) {self.poll.bind_queue(queue);}
    #[cfg(all(test,not(target_arch="wasm32")))]
    pub(crate) fn initialize_poll_owner(&self)->Result<(),crate::GpuRasterError> {self.poll.handle(&self.device);Ok(())}
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn retirement_handle(&self)->crate::device_poll::RetirementHandle {self.poll.handle(&self.device)}
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn retirement<T:Send+'static>(&self,value:T)->crate::device_poll::Retirement<T> {self.retirement_handle().retirement(value)}
    pub(crate) fn request_poll(&self) {
        #[cfg(not(target_arch="wasm32"))]
        self.poll.request(&self.device);
    }
    pub(crate) fn on_queue_done(&self,queue:&wgpu::Queue,callback:impl FnOnce()+Send+'static) {
        let callback=self.callback(move|()|callback());
        queue.on_submitted_work_done(move||callback(()));self.request_poll();
    }
    pub(crate) fn on_encoder_done(&self,encoder:&wgpu::CommandEncoder,callback:impl FnOnce()+Send+'static) {
        let callback=self.callback(move|()|callback());encoder.on_submitted_work_done(move||callback(()));
    }
    pub(crate) fn callback<A:Send+'static>(&self,callback:impl FnOnce(A)+Send+'static)->impl FnOnce(A)+Send+'static {
        #[cfg(not(target_arch="wasm32"))]
        {self.poll.callback(&self.device,callback)}
        #[cfg(target_arch="wasm32")]
        {move|argument| {
            callback(argument);
            let wakes=COMPLETION.with_borrow_mut(|(count,wakes)| {*count+=1;std::mem::take(wakes)});
            for wake in wakes {wake();}
        }}
    }
    pub(crate) fn on_next_completion(&self,wake:&std::sync::Arc<dyn Fn()+Send+Sync>)->u64 {
        #[cfg(not(target_arch="wasm32"))]
        {self.retirement_handle().on_next_completion(wake)}
        #[cfg(target_arch="wasm32")]
        {COMPLETION.with_borrow_mut(|(count,wakes)| {
            if !wakes.iter().any(|old|std::sync::Arc::ptr_eq(old,wake)) {wakes.push(wake.clone());}
            *count
        })}
    }
    pub(crate) fn submit(&self,queue:&wgpu::Queue,commands:impl IntoIterator<Item=wgpu::CommandBuffer>)->Option<wgpu::SubmissionIndex> {
        self.queue_submit(||queue.submit(commands))
    }
    pub(crate) fn submit_for_present(&self,queue:&wgpu::Queue,commands:impl IntoIterator<Item=wgpu::CommandBuffer>)->Option<wgpu::SubmissionIndex> {
        self.queue_submit(||queue.submit_for_present(commands))
    }
    fn queue_submit(&self,work:impl FnOnce()->wgpu::SubmissionIndex)->Option<wgpu::SubmissionIndex> {
        #[cfg(not(target_arch="wasm32"))]
        self.poll.handle(&self.device);
        #[cfg(not(target_arch="wasm32"))]
        let submitted=self.poll.submit(work);
        #[cfg(target_arch="wasm32")]
        let submitted=Some(work());
        self.request_poll();submitted
    }
    #[cfg(not(target_arch="wasm32"))]
    pub(crate) fn wait_queue(&self)->Result<(),String> {self.retirement_handle().flush()}
    pub(crate) fn compilation(&self) -> Result<crate::compile::Scope, crate::GpuRasterError> {
        let mut shared = self.compilation.lock().unwrap();
        if let Some(scope)=shared.as_ref().and_then(|coordinator|coordinator.scope()) {return Ok(scope);}
        let coordinator=crate::compile::Coordinator::new(self)?;
        let scope=coordinator.scope().ok_or_else(||crate::GpuRasterError::WaitFailed("GPU device service retired".into()))?;
        *shared=Some(coordinator);
        Ok(scope)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn with_saved_pipelines(mut self, saved: Option<std::sync::Arc<crate::SavedPipelines>>) -> Self {
        self.saved_pipelines = saved;
        self
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn saved_pipelines(&self) -> Option<&std::sync::Arc<crate::SavedPipelines>> { self.saved_pipelines.as_ref() }
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
    pub fn float32_filterable(&self) -> bool {
        self.features().contains(wgpu::Features::FLOAT32_FILTERABLE)
    }
    pub fn float_sampling_wgsl(&self) -> &'static str {
        float_sampling_wgsl(self.float32_filterable())
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
        if descriptor.cache.is_none() && let Some(store) = self.saved_pipelines() {
            let mut descriptor = descriptor.clone();
            descriptor.cache = Some(store.cache());
            let pipeline = self.device.create_render_pipeline(&descriptor);
            store.compiled();
            return pipeline;
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
        if descriptor.cache.is_none() && let Some(store) = self.saved_pipelines() {
            let mut descriptor = descriptor.clone();
            descriptor.cache = Some(store.cache());
            let pipeline = self.device.create_compute_pipeline(&descriptor);
            store.compiled();
            return pipeline;
        }
        self.device.create_compute_pipeline(descriptor)
    }
}
pub(crate) fn float_sampling_wgsl(filterable: bool) -> &'static str {
    if filterable { include_str!("float_sample_filtered.wgsl") } else { include_str!("float_sample_texels.wgsl") }
}
