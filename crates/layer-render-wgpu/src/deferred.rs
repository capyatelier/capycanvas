//! In-process pipeline recipes. No disk cache; each device starts uncompiled.
use std::sync::{Arc, Mutex, OnceLock};

/// Recipes choose the native/immediate API or a real browser compilation
/// promise. Descriptor borrows end before the owned future is returned.
#[derive(Clone, Copy)]
pub(super) enum CompileMode {
    #[cfg(not(target_arch = "wasm32"))]
    Immediate,
    #[cfg(target_arch = "wasm32")]
    Async,
}
pub(super) enum Compilation<T> {
    Ready(T),
    #[cfg(target_arch = "wasm32")]
    Pending(std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>>>>),
}
impl<T> Compilation<T> {
    pub fn immediate(self) -> T {
        match self {
            Self::Ready(value) => value,
            #[cfg(target_arch = "wasm32")]
            Self::Pending(_) => panic!("Asynchronous pipeline used before compilation completed"),
        }
    }
}
impl CompileMode {
    pub fn render(
        self,
        device: &super::PipelineDevice,
        desc: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> Compilation<wgpu::RenderPipeline> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Immediate => Compilation::Ready(device.create_render_pipeline(desc)),
            #[cfg(target_arch = "wasm32")]
            Self::Async => {
                Compilation::Pending(Box::pin(device.create_render_pipeline_async(desc)))
            }
        }
    }
    pub fn compute(
        self,
        device: &super::PipelineDevice,
        desc: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> Compilation<wgpu::ComputePipeline> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Immediate => Compilation::Ready(device.create_compute_pipeline(desc)),
            #[cfg(target_arch = "wasm32")]
            Self::Async => {
                Compilation::Pending(Box::pin(device.create_compute_pipeline_async(desc)))
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Factory<T> = Box<dyn FnOnce(CompileMode) -> Compilation<T> + Send>;
#[cfg(target_arch = "wasm32")]
type Factory<T> = Box<dyn FnOnce(CompileMode) -> Compilation<T>>;

struct Inner<T> {
    value: OnceLock<T>,
    #[cfg(target_arch = "wasm32")]
    validating: std::cell::Cell<bool>,
    #[cfg(target_arch = "wasm32")]
    preparation: std::cell::OnceCell<futures_util::future::Shared<futures_util::future::LocalBoxFuture<'static, Result<(), String>>>>,
    #[cfg(target_arch = "wasm32")]
    async_pipeline: bool,
    factory: Mutex<Option<Factory<T>>>,
    priority: std::sync::atomic::AtomicU8,
}
pub(super) struct Deferred<T>(Arc<Inner<T>>);
impl<T> Clone for Deferred<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Deferred<T> {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(factory: impl FnOnce() -> T + Send + 'static) -> Self {
        Self::boxed(Box::new(move |_| Compilation::Ready(factory())), false)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn new(factory: impl FnOnce() -> T + 'static) -> Self {
        Self::boxed(Box::new(move |_| Compilation::Ready(factory())), false)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn pipeline(factory: impl FnOnce(CompileMode) -> Compilation<T> + Send + 'static) -> Self {
        Self::boxed(Box::new(factory), true)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn pipeline(factory: impl FnOnce(CompileMode) -> Compilation<T> + 'static) -> Self {
        Self::boxed(Box::new(factory), true)
    }
    fn boxed(factory: Factory<T>, _async_pipeline: bool) -> Self {
        Self(Arc::new(Inner {
            value: OnceLock::new(),
            #[cfg(target_arch = "wasm32")]
            validating: std::cell::Cell::new(false),
            #[cfg(target_arch = "wasm32")]
            preparation: Default::default(),
            #[cfg(target_arch = "wasm32")]
            async_pipeline: _async_pipeline,
            factory: Mutex::new(Some(factory)),
            priority: std::sync::atomic::AtomicU8::new(u8::MAX),
        }))
    }
    #[cfg(target_arch = "wasm32")]
    pub fn async_pipeline(&self) -> bool {
        self.0.async_pipeline
    }
    pub fn compile(&self) -> &T {
        self.0.value.get_or_init(|| {
            #[cfg(target_arch = "wasm32")]
            assert!(!self.0.async_pipeline, "Pipeline used before preparation");
            #[cfg(target_arch = "wasm32")]
            let mode = CompileMode::Async;
            #[cfg(not(target_arch = "wasm32"))]
            let mode = CompileMode::Immediate;
            self.0.factory.lock().unwrap().take().expect("resource recipe")(mode).immediate()
        })
    }
    /// Start inside the caller's error scopes, then await without borrowing the
    /// renderer. Only successful compilation publishes a typed pipeline handle.
    #[cfg(target_arch = "wasm32")]
    pub fn compile_async(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>
    where
        T: 'static,
    {
        if self.0.value.get().is_some() {
            return Box::pin(async { Ok(()) });
        }
        use futures_util::FutureExt;
        Box::pin(self.0.preparation.get_or_init(|| {
            let compilation = self.0.factory.lock().unwrap().take().expect("resource recipe")(CompileMode::Async);
            let owner = Arc::downgrade(&self.0);
            async move {
                let value = match compilation {
                    Compilation::Ready(value) => value,
                    Compilation::Pending(future) => future.await?,
                };
                if let Some(owner) = owner.upgrade() { let _ = owner.value.set(value); }
                Ok(())
            }.boxed_local().shared()
        }).clone())
    }
    pub async fn prepare(&self) -> Result<(), String> where T: 'static {
        #[cfg(target_arch = "wasm32")]
        { self.compile_async().await }
        #[cfg(not(target_arch = "wasm32"))]
        { self.compile(); Ok(()) }
    }
    pub async fn prepare_all<'a>(pipelines: impl IntoIterator<Item = &'a Self>) -> Result<(), String> where T: 'static {
        #[cfg(target_arch = "wasm32")]
        { futures_util::future::try_join_all(pipelines.into_iter().map(Self::prepare)).await.map(|_| ()) }
        #[cfg(not(target_arch = "wasm32"))]
        { for pipeline in pipelines { pipeline.prepare().await?; } Ok(()) }
    }
    pub fn ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.0.validating.get() {
            return false;
        }
        self.0.value.get().is_some()
    }
    #[cfg(target_arch = "wasm32")]
    pub fn validating(&self, value: bool) {
        self.0.validating.set(value);
    }
    pub fn promote(&self, priority: u8) -> bool {
        !self.ready()
            && self
                .0
                .priority
                .fetch_min(priority, std::sync::atomic::Ordering::Relaxed)
                > priority
    }
}
impl<T> std::ops::Deref for Deferred<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.compile()
    }
}
impl Deferred<wgpu::ShaderModule> {
    pub fn wgsl(device: &super::PipelineDevice, label: &'static str, source: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        let (device, source) = (device.for_recipe(), source.into());
        Self::new(move || device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source) }))
    }
}
impl Deferred<wgpu::ComputePipeline> {
    pub fn compute_module(device: &super::PipelineDevice, label: &'static str, layout: &wgpu::PipelineLayout,
        module: &wgpu::ShaderModule, entry: &'static str) -> Self {
        let module = module.clone();
        Self::compute(device, label, layout, &Deferred::new(move || module), entry)
    }
    pub fn compute(
        device: &super::PipelineDevice,
        label: &'static str,
        layout: &wgpu::PipelineLayout,
        module: &Deferred<wgpu::ShaderModule>,
        entry: &'static str,
    ) -> Self {
        let (device, layout, module) = (device.for_recipe(), layout.clone(), module.clone());
        Self::pipeline(move |mode| mode.compute(&device, &wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        }))
    }
}
