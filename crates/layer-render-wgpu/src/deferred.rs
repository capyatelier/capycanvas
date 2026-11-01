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
    validating: std::cell::Cell<u32>,
    #[cfg(target_arch = "wasm32")]
    preparation: std::cell::OnceCell<futures_util::future::Shared<futures_util::future::LocalBoxFuture<'static, Result<(), String>>>>,
    async_pipeline: bool,
    factory: Mutex<Option<Factory<T>>>,
    priority: std::sync::atomic::AtomicU8,
}
pub(super) struct Deferred<T>(Arc<Inner<T>>);
pub(super) struct Promotion<T> { pipeline: Deferred<T>, priority: u8 }
impl<T> Drop for Promotion<T> {
    fn drop(&mut self) {
        let _ = self.pipeline.0.priority.compare_exchange(self.priority, u8::MAX,
            std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed);
    }
}
#[cfg(target_arch = "wasm32")]
pub(super) struct Validation<T> { pipeline: Deferred<T>, active: std::cell::Cell<bool> }
#[cfg(target_arch = "wasm32")]
impl<T> Validation<T> {
    pub fn begin(&self) {
        if !self.active.replace(true) { self.pipeline.0.validating.set(self.pipeline.0.validating.get() + 1); }
    }
}
#[cfg(target_arch = "wasm32")]
impl<T> Drop for Validation<T> {
    fn drop(&mut self) {
        if self.active.get() { self.pipeline.0.validating.set(self.pipeline.0.validating.get() - 1); }
    }
}
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
    fn boxed(factory: Factory<T>, async_pipeline: bool) -> Self {
        Self(Arc::new(Inner {
            value: OnceLock::new(),
            #[cfg(target_arch = "wasm32")]
            validating: std::cell::Cell::new(0),
            #[cfg(target_arch = "wasm32")]
            preparation: Default::default(),
            async_pipeline,
            factory: Mutex::new(Some(factory)),
            priority: std::sync::atomic::AtomicU8::new(u8::MAX),
        }))
    }
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
            self.take_factory()(mode).immediate()
        })
    }
    fn take_factory(&self) -> Factory<T> {
        let factory = self.0.factory.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        factory.expect("resource recipe")
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
            let compilation = self.take_factory()(CompileMode::Async);
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
        if self.0.validating.get() != 0 {
            return false;
        }
        self.0.value.get().is_some()
    }
    #[cfg(target_arch = "wasm32")]
    pub fn validation(&self) -> Validation<T> { Validation { pipeline: self.clone(), active: std::cell::Cell::new(false) } }
    pub fn promote(&self, priority: u8) -> Option<Promotion<T>> {
        (!self.ready() && self.0.priority.fetch_min(priority, std::sync::atomic::Ordering::Relaxed) > priority)
            .then(|| Promotion { pipeline: self.clone(), priority })
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
        label: impl Into<std::borrow::Cow<'static, str>>,
        layout: &wgpu::PipelineLayout,
        module: &Deferred<wgpu::ShaderModule>,
        entry: &'static str,
    ) -> Self {
        let (device, layout, module, label) = (device.for_recipe(), layout.clone(), module.clone(), label.into());
        Self::pipeline(move |mode| mode.compute(&device, &wgpu::ComputePipelineDescriptor {
            label: Some(&label),
            layout: Some(&layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        }))
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    #[test]
    fn canceled_validation_releases_only_its_shared_recipe_owner() {
        let recipe = Deferred::new(|| 17);
        let first = recipe.validation();
        let second = recipe.validation();
        first.begin(); second.begin();
        recipe.compile();
        assert!(!recipe.ready());
        drop(first);
        assert!(!recipe.ready());
        drop(second);
        assert!(recipe.ready());
    }
}
