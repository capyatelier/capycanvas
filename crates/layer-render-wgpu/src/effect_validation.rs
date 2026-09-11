//! Candidate shader compilation never replaces working state on failure.
//! Error scopes are popped immediately and polled later; there is no GPU wait.
use super::*;
use layer_render::{EffectValidationRequest, EffectValidationResult};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

#[cfg(not(target_arch = "wasm32"))]
type ValidationFuture = Pin<Box<dyn Future<Output = Option<String>> + Send>>;
#[cfg(target_arch = "wasm32")]
type ValidationFuture = Pin<Box<dyn Future<Output = Option<String>>>>;

pub(super) struct Pending {
    value: Option<Validation>,
    background: Option<mpsc::Receiver<Validation>>,
}
struct BackgroundValidation {
    effects: Option<effects::Effects>,
    errors: Vec<ValidationFuture>,
    error: Option<String>,
}
struct Validation {
    request_id: u64,
    effects: effects::Effects,
    error: Option<String>,
    result: ValidationFuture,
    namespace: Vec<Arc<layer_core::EffectProgram>>,
}
impl WgpuRasterizer {
    pub fn effect_validation_pending(&self) -> bool {
        self.effect_validation.is_some()
    }
    pub(super) fn start_effect_validation(
        &mut self,
        request: EffectValidationRequest,
    ) -> Result<bool, GpuRasterError> {
        if self.effect_validation.is_some() {
            return Ok(false);
        }
        if request.programs.len() > 1024 || request.namespace.len() > 2048 {
            return Err(GpuRasterError::Effect(
                "Oversized validation request".into(),
            ));
        }
        effects::validate_namespace(&request.namespace)?;
        // Reuse the established ABI layouts and compiled programs, but keep
        // candidate buffers/pipelines isolated until all device scopes resolve.
        let candidate = if let Some(scene) = &self.scene {
            scene.effects.fork()
        } else if let Some(cache) = &self.validated_effects {
            cache.fork()
        } else {
            self.scene_pipelines.effects(self)
        };
        if let Some(startup) = &self.startup {
            // Cold startup also warms unchanged catalog programs. Each program
            // is its own background job so newly needed document/brush work can
            // overtake speculative compilation between driver calls.
            let programs = if !startup.finished {
                request.namespace.clone()
            } else {
                request.programs.clone()
            };
            let state = Arc::new(std::sync::Mutex::new(BackgroundValidation {
                effects: Some(candidate),
                errors: Vec::new(),
                error: None,
            }));
            for program in programs {
                let gpu = effects::Context {
                    device: self.device.clone(),
                    queue: self.queue.clone(),
                };
                let state = state.clone();
                startup.compiler.enqueue(startup::OTHER, move || {
                    let mut state = state.lock().unwrap();
                    let candidate = state.effects.take().unwrap();
                    let value = compile_candidate(
                        &gpu,
                        candidate,
                        EffectValidationRequest {
                            request_id: 0,
                            programs: vec![program],
                            namespace: Vec::new(),
                        },
                    );
                    state.effects = Some(value.effects);
                    state.errors.push(value.result);
                    if state.error.is_none() {
                        state.error = value.error;
                    }
                    Ok(())
                });
            }
            let (tx, rx) = mpsc::channel();
            startup.compiler.enqueue(startup::OTHER, move || {
                let mut state = state.lock().unwrap();
                let errors = std::mem::take(&mut state.errors);
                let value = Validation {
                    request_id: request.request_id,
                    namespace: request.namespace,
                    effects: state.effects.take().unwrap(),
                    error: state.error.take(),
                    result: Box::pin(async move {
                        let mut error = None;
                        for result in errors {
                            let next = result.await;
                            if error.is_none() {
                                error = next;
                            }
                        }
                        error
                    }),
                };
                let _ = tx.send(value);
                Ok(())
            });
            self.effect_validation = Some(Pending {
                value: None,
                background: Some(rx),
            });
            return Ok(true);
        }
        self.effect_validation = Some(Pending {
            value: Some(compile_candidate(self, candidate, request)),
            background: None,
        });
        Ok(true)
    }
    pub(super) fn poll_effect_validation(&mut self) -> Option<EffectValidationResult> {
        let pending = self.effect_validation.as_mut()?;
        if let Some(rx) = &pending.background {
            match rx.try_recv() {
                Ok(value) => {
                    pending.value = Some(value);
                    pending.background = None;
                }
                Err(_) => return None,
            }
        }
        let pending = pending.value.as_mut()?;
        let _ = self.device.poll(wgpu::PollType::Poll);
        let Poll::Ready(error) = pending
            .result
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        else {
            return None;
        };
        let pending = self.effect_validation.take().unwrap().value.unwrap();
        let error = pending.error.or(error);
        if error.is_none() {
            // fork discards temporary parameter buffers and unexecuted lookup
            // dispatches. Keep compilation caches even on a plain paint canvas.
            let mut cache = pending.effects.fork();
            cache.retain_compilations(&pending.namespace);
            if let Some(scene) = &mut self.scene {
                scene.effects.merge_validated(cache.fork());
                scene.effects.retain_compilations(&pending.namespace);
            }
            self.validated_effects = Some(cache);
        }
        Some(EffectValidationResult {
            request_id: pending.request_id,
            result: error.map_or(Ok(()), Err),
        })
    }
}

fn compile_candidate(
    gpu: &impl effects::Gpu,
    mut candidate: effects::Effects,
    request: EffectValidationRequest,
) -> Validation {
    let internal = gpu.device().push_error_scope(wgpu::ErrorFilter::Internal);
    let memory = gpu
        .device()
        .push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = gpu.device().push_error_scope(wgpu::ErrorFilter::Validation);
    let result = (|| {
        for (i, program) in request.programs.iter().enumerate() {
            let mut layer = Layer::paint(LayerId(u64::MAX - i as u64), "");
            layer.effect = Some(Arc::new(layer_core::EffectInstance::new(program.clone())));
            candidate.prepare(gpu, &[&layer], effects::Execution::Preview, 0.)?;
            if program.image_boundary() {
                for pass in 0..program.passes.len().max(1) {
                    candidate.prepare(gpu, &[&layer], effects::Execution::Image(pass), 0.)?;
                }
            } else {
                candidate.prepare(gpu, &[&layer], effects::Execution::Fused, 0.)?;
            }
        }
        Ok::<_, GpuRasterError>(())
    })();
    // Drop !Send scope guards here, on the thread which pushed them.
    let validation = validation.pop();
    let memory = memory.pop();
    let internal = internal.pop();
    Validation {
        request_id: request.request_id,
        effects: candidate,
        error: result.err().map(|e| e.to_string()),
        result: Box::pin(async move {
            let a = validation.await;
            let b = memory.await;
            let c = internal.await;
            a.or(b).or(c).map(|e| e.to_string())
        }),
        namespace: request.namespace,
    }
}
