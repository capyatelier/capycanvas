//! In-process pipeline recipes. No disk cache; each device starts uncompiled.
use std::sync::{Arc, Mutex, OnceLock};

/// Recipes choose the native/immediate API or a real browser compilation
/// promise. Descriptor borrows end before the owned future is returned.
#[derive(Clone, Copy)]
pub(super) enum CompileMode {
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
            Self::Immediate => Compilation::Ready(device.create_compute_pipeline(desc)),
            #[cfg(target_arch = "wasm32")]
            Self::Async => {
                Compilation::Pending(Box::pin(device.create_compute_pipeline_async(desc)))
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Factory<T> = Box<dyn FnMut(CompileMode) -> Compilation<T> + Send>;
#[cfg(target_arch = "wasm32")]
type Factory<T> = Box<dyn FnMut(CompileMode) -> Compilation<T>>;

struct Inner<T> {
    value: OnceLock<T>,
    #[cfg(target_arch = "wasm32")]
    validating: std::cell::Cell<bool>,
    #[cfg(target_arch = "wasm32")]
    failure: OnceLock<String>,
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
        let mut factory = Some(factory);
        Self::boxed(Box::new(move |_| Compilation::Ready(factory.take().expect("recipe runs once")())), false)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn new(factory: impl FnOnce() -> T + 'static) -> Self {
        let mut factory = Some(factory);
        Self::boxed(Box::new(move |_| Compilation::Ready(factory.take().expect("recipe runs once")())), false)
    }
    /// The recipe can run twice; see [`Deferred::compile`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn pipeline(factory: impl Fn(CompileMode) -> Compilation<T> + Send + 'static) -> Self {
        Self::boxed(Box::new(factory), true)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn pipeline(factory: impl Fn(CompileMode) -> Compilation<T> + 'static) -> Self {
        Self::boxed(Box::new(factory), true)
    }
    fn boxed(factory: Factory<T>, _async_pipeline: bool) -> Self {
        Self(Arc::new(Inner {
            value: OnceLock::new(),
            #[cfg(target_arch = "wasm32")]
            validating: std::cell::Cell::new(false),
            #[cfg(target_arch = "wasm32")]
            failure: OnceLock::new(),
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
    /// Returns the value, compiling it now if no compilation has published one.
    /// Natively this waits for a compilation already running on another thread.
    /// The browser cannot wait for an asynchronous one, so this compiles the
    /// recipe again and the asynchronous result is dropped when it arrives.
    pub fn compile(&self) -> &T {
        self.0.value.get_or_init(|| {
            let mut factory = self.0.factory.lock().unwrap();
            let value = factory.as_mut().expect("pipeline recipe")(CompileMode::Immediate).immediate();
            *factory = None;
            value
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
        if let Some(error) = self.0.failure.get() {
            return Box::pin(std::future::ready(Err(error.clone())));
        }
        let compilation =
            self.0.factory.lock().unwrap().as_mut().expect("pipeline recipe")(CompileMode::Async);
        let this = self.clone();
        let publish = move |value| {
            let _ = this.0.value.set(value);
            this.0.factory.lock().unwrap().take();
        };
        match compilation {
            Compilation::Ready(value) => {
                publish(value);
                Box::pin(async { Ok(()) })
            }
            Compilation::Pending(future) => {
                let this = self.clone();
                Box::pin(async move {
                    match future.await {
                        Ok(value) => {
                            publish(value);
                            Ok(())
                        }
                        Err(error) => {
                            let _ = this.0.failure.set(error.clone());
                            Err(error)
                        }
                    }
                })
            }
        }
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
        let (device, source) = (device.clone(), source.into());
        Self::new(move || device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source) }))
    }
}
impl Deferred<wgpu::ComputePipeline> {
    pub fn compute(
        device: &super::PipelineDevice,
        label: &'static str,
        layout: &wgpu::PipelineLayout,
        module: &Deferred<wgpu::ShaderModule>,
        entry: &'static str,
    ) -> Self {
        let (device, layout, module) = (device.clone(), layout.clone(), module.clone());
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
