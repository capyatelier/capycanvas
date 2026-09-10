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
        let mut candidate = if let Some(scene) = &self.scene {
            scene.effects.fork()
        } else if let Some(cache) = &self.validated_effects {
            cache.fork()
        } else {
            scene::Scene::new(self).effects
        };
        let internal = self.device.push_error_scope(wgpu::ErrorFilter::Internal);
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let result = (|| {
            for (i, program) in request.programs.iter().enumerate() {
                let mut layer = Layer::paint(LayerId(u64::MAX - i as u64), "");
                layer.effect = Some(Arc::new(layer_core::EffectInstance::new(program.clone())));
                candidate.prepare(self, &[&layer], effects::Execution::Preview, 0.)?;
                if program.image_boundary() {
                    for pass in 0..program.passes.len().max(1) {
                        candidate.prepare(self, &[&layer], effects::Execution::Image(pass), 0.)?;
                    }
                } else {
                    candidate.prepare(self, &[&layer], effects::Execution::Fused, 0.)?;
                }
            }
            Ok::<_, GpuRasterError>(())
        })();
        // Drop !Send scope guards here, on the thread which pushed them.
        let validation = validation.pop();
        let memory = memory.pop();
        let internal = internal.pop();
        self.effect_validation = Some(Pending {
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
        });
        Ok(true)
    }
    pub(super) fn poll_effect_validation(&mut self) -> Option<EffectValidationResult> {
        let pending = self.effect_validation.as_mut()?;
        let _ = self.device.poll(wgpu::PollType::Poll);
        let Poll::Ready(error) = pending
            .result
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        else {
            return None;
        };
        let pending = self.effect_validation.take().unwrap();
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
