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
    error: OnceLock<String>,
    #[cfg(target_arch = "wasm32")]
    validating: std::cell::Cell<u32>,
    #[cfg(target_arch = "wasm32")]
    preparation: std::cell::OnceCell<futures_util::future::Shared<futures_util::future::LocalBoxFuture<'static, Result<(), String>>>>,
    async_pipeline: bool,
    factory: Mutex<Option<Factory<T>>>,
}
pub(super) struct Deferred<T>(Arc<Inner<T>>);
#[cfg(target_arch = "wasm32")]
pub(super) struct Validation<T> { pipeline: Deferred<T>, active: std::cell::Cell<bool> }
#[cfg(target_arch = "wasm32")]
impl<T> Validation<T> {
    pub fn begin(&self) {
        if !self.active.replace(true) { self.pipeline.0.validating.set(self.pipeline.0.validating.get() + 1); }
    }
    pub fn finish(&self, result: &Result<(),String>) {
        if let Err(error)=result {self.pipeline.fail(error);}
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
            error: OnceLock::new(),
            #[cfg(target_arch = "wasm32")]
            validating: std::cell::Cell::new(0),
            #[cfg(target_arch = "wasm32")]
            preparation: Default::default(),
            async_pipeline,
            factory: Mutex::new(Some(factory)),
        }))
    }
    pub fn async_pipeline(&self) -> bool {
        self.0.async_pipeline
    }
    pub fn compile(&self) -> &T {
        if !self.ready() { crate::compile::assert_execution(); }
        self.0.value.get_or_init(|| {
            #[cfg(target_arch = "wasm32")]
            assert!(!self.0.async_pipeline, "Pipeline used before preparation");
            #[cfg(target_arch = "wasm32")]
            let mode = CompileMode::Async;
            #[cfg(not(target_arch = "wasm32"))]
            let mode = CompileMode::Immediate;
            #[cfg(not(target_arch = "wasm32"))]
            {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(||self.take_factory()(mode).immediate())) {
                    Ok(value)=>value,
                    Err(payload)=>{self.fail(&crate::panic_message(&*payload));std::panic::resume_unwind(payload)}
                }
            }
            #[cfg(target_arch = "wasm32")]
            self.take_factory()(mode).immediate()
        })
    }
    fn take_factory(&self) -> Factory<T> {
        let factory = self.0.factory.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        factory.unwrap_or_else(||panic!("{}",self.error().unwrap_or("resource recipe")))
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
        if let Some(error)=self.error() {let error=error.to_string();return Box::pin(async move {Err(error)});}
        if self.0.value.get().is_some() {
            return Box::pin(async { Ok(()) });
        }
        use futures_util::FutureExt;
        Box::pin(self.0.preparation.get_or_init(|| {
            crate::compile::assert_execution();
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
    pub fn ready(&self) -> bool {
        if self.error().is_some() {return false;}
        #[cfg(target_arch = "wasm32")]
        if self.0.validating.get() != 0 {
            return false;
        }
        self.0.value.get().is_some()
    }
    #[cfg(target_arch = "wasm32")]
    pub fn validation(&self) -> Validation<T> { Validation { pipeline: self.clone(), active: std::cell::Cell::new(false) } }
    pub(crate) fn identity(&self) -> usize { Arc::as_ptr(&self.0) as usize }
    pub(crate) fn error(&self)->Option<&str> {self.0.error.get().map(String::as_str)}
    fn fail(&self,error:&str) {let _=self.0.error.set(error.into());}
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

#[cfg(all(test, debug_assertions, not(target_arch = "wasm32")))]
mod permit_tests {
    use super::*;
    #[test]
    #[should_panic(expected = "Cold recipe compiled outside the compilation coordinator")]
    fn a_cold_recipe_requires_the_executor_permit() { Deferred::new(|| ()).compile(); }
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
        crate::compile::execute(|| recipe.compile());
        assert!(!recipe.ready());
        drop(first);
        assert!(!recipe.ready());
        drop(second);
        assert!(recipe.ready());
    }
}
