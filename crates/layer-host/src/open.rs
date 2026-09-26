//! Worker-side project preparation shared by the native hosts: limits, import,
//! admission, renderer construction, embedded effect validation and staged startup.
use crate::{GpuContext, Renderer, RendererOptions};
use layer_core::{Project, ProjectLimits};
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_ui::UiSession;
use std::io::{Read, Seek};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub const PREPARE_DEADLINE: Duration = Duration::from_secs(120);

pub struct OpenEnvironment {
    pub admission: layer_ui::DocumentAdmission,
    pub platform: layer_ui::Platform,
    pub gpu: GpuContext,
    pub options: RendererOptions,
    pub viewport: [u32; 2],
    pub brush: layer_core::BrushSnapshot,
    pub new_options: layer_ui::NewDocumentOptions,
    pub photo_policy: layer_ui::PhotoOpenPolicy,
}

impl OpenEnvironment {
    pub fn capture(
        session: &UiSession<Renderer>,
        admission: layer_ui::DocumentAdmission,
        options: RendererOptions,
    ) -> Result<Self, String> {
        let gpu = session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Wait for the canvas to finish starting")?;
        Ok(Self {
            admission,
            platform: session.state().platform,
            gpu: GpuContext::of(gpu),
            options,
            viewport: session.state().camera.viewport,
            brush: session.engine().configured_brush().clone(),
            new_options: session.state().settings.new_document.defaults,
            photo_policy: session.state().settings.photo_open,
        })
    }

    pub fn limits(&self) -> ProjectLimits {
        ProjectLimits {
            dimension: self
                .gpu
                .device
                .limits()
                .max_texture_dimension_2d
                .min(ProjectLimits::default().dimension),
            ..Default::default()
        }
    }

    pub fn read(
        &self,
        input: impl Read + Seek,
        intent: layer_ui::ImportIntent,
        name: &str,
        cancel: &AtomicBool,
    ) -> Result<layer_ui::ImportedDocument, String> {
        layer_ui::read_import(
            input,
            intent,
            self.photo_policy,
            name,
            self.limits(),
            Default::default(),
            cancel,
        )
    }

    /// Returns a candidate whose canvas and configured brush are ready to draw.
    pub fn prepare(
        &self,
        project: Project,
        cancelled: impl Fn() -> bool,
    ) -> Result<Box<UiSession<Renderer>>, String> {
        project.validate(self.limits())?;
        self.admission.admit(&project)?;
        let check = || {
            if cancelled() {
                Err("Document operation cancelled".to_string())
            } else {
                Ok(())
            }
        };
        check()?;
        let mut gpu = self
            .gpu
            .rasterizer(project.document.color, &self.options, true)?;
        let mut programs = Vec::new();
        for effect in project
            .document
            .layers
            .iter()
            .filter_map(|l| l.effect.as_ref())
        {
            if !programs.contains(&effect.program) {
                programs.push(effect.program.clone());
            }
        }
        let mut validating = !programs.is_empty();
        if validating {
            gpu.request_effect_validation(EffectValidationRequest {
                request_id: 1,
                namespace: programs.clone(),
                programs,
            })
            .map_err(|e| e.to_string())?;
        }
        // Preparing startup starts the native compiler. Waiting for validation
        // before this call leaves all embedded shader jobs permanently queued.
        gpu.prepare_startup(&project.document, &self.brush, false)
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + PREPARE_DEADLINE;
        loop {
            check()?;
            gpu.device()
                .poll(wgpu::PollType::Poll)
                .map_err(|e| e.to_string())?;
            if validating && let Some(result) = gpu.take_effect_validation() {
                result.result?;
                validating = false;
            }
            let ready = gpu.poll_startup().map_err(|e| e.to_string())?;
            if !validating && ready.canvas_ready && ready.brush_ready {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Project canvas preparation timed out".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut candidate =
            UiSession::from_project(Renderer(Some(gpu.into())), project, None, self.viewport, self.platform)?;
        candidate.frame(0, 0)?;
        check()?;
        Ok(Box::new(candidate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_render_wgpu::WgpuRasterizer;

    #[test]
    fn open_prepares_a_ready_candidate_and_honours_cancellation() {
        let project = layer_ui::NewDocumentOptions::default().project().unwrap();
        let gpu = WgpuRasterizer::new_native_headless(project.document.color).unwrap();
        let session = UiSession::from_project(
            Renderer(Some(gpu.into())),
            layer_ui::new_drawing(64, 48).unwrap(),
            None,
            [640, 480],
            layer_ui::Platform::Mac,
        )
        .unwrap();
        let admission = layer_ui::DocumentSessions::<()>::default()
            .admission(&session.retained_document_tiles());
        let environment =
            OpenEnvironment::capture(&session, admission, RendererOptions::default()).unwrap();
        assert_eq!(environment.viewport, session.state().camera.viewport);
        let mut candidate = environment.prepare(project.clone(), || false).unwrap();
        let gpu = candidate.renderer_mut().0.as_mut().unwrap();
        assert!(gpu.device() == &environment.gpu.device);
        let ready = gpu.poll_startup().unwrap();
        assert!(ready.canvas_ready && ready.brush_ready);
        assert_eq!(
            environment.prepare(project, || true).err().as_deref(),
            Some("Document operation cancelled")
        );
    }
}
