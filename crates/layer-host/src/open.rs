//! Worker-side project preparation shared by the native hosts: limits, import,
//! admission, renderer construction, embedded effect validation and staged startup.
use crate::{GpuContext, Renderer, RendererOptions};
use layer_core::{Document, ProjectLimits};
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_ui::UiSession;
use std::io::{Read, Seek};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub const PREPARE_DEADLINE: Duration = Duration::from_secs(120);

pub struct OpenEnvironment {
    pub admission: layer_ui::DocumentAdmission,
    pub platform: layer_ui::Platform,
    pub localization: std::sync::Arc<layer_ui::Localizer>,
    pub gpu: GpuContext,
    pub options: RendererOptions,
    pub viewport: [u32; 2],
    pub brush: layer_core::BrushSnapshot,
    pub new_drawing: Option<layer_ui::NewDrawingOrder>,
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
            localization: session.localization().clone(),
            gpu: GpuContext::of(gpu),
            options,
            viewport: session.state().camera.viewport,
            brush: session.engine().configured_brush().clone(),
            new_drawing: session.new_drawing_order().cloned(),
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

    pub fn admit_sessions<'a>(&self, editors: impl IntoIterator<Item = &'a layer_core::Editor>) -> Result<(), String> {
        self.admission.admit_sessions(editors).map_err(|reason| reason.message(&self.localization))
    }

    pub fn read(
        &self,
        input: impl Read + Seek,
        intent: layer_ui::ImportIntent,
        name: &str,
        cancel: &AtomicBool,
        decode: crate::photo_reader::Decoder,
    ) -> Result<layer_ui::ImportOutcome, String> {
        layer_ui::read_import_with_decoder(
            input,
            intent,
            self.photo_policy,
            layer_ui::photo_document_names(name, &self.localization),
            self.limits(),
            Default::default(),
            cancel,
            |mut input, limits, cancelled| decode(&mut input, limits, cancelled),
        )
    }

    /// The drawing New chose, on its paper from `papers` when it is stored there.
    pub fn new_drawing(&self, papers: Option<&std::path::Path>) -> Result<(Document, Option<String>), String> {
        let order = self.new_drawing.as_ref().ok_or("Choose New drawing first")?;
        let paper = crate::papers::resolve(&order.paper, papers, &self.localization);
        Ok((order.project(paper.texture, &self.localization)?, paper.warning))
    }

    /// Returns a candidate whose canvas and configured brush are ready to draw.
    pub fn prepare(
        &self,
        project: Document,
        cancelled: impl Fn() -> bool,
    ) -> Result<Box<UiSession<Renderer>>, String> {
        project.validate(self.limits())?;
        self.admission.admit(&project).map_err(|reason| reason.message(&self.localization))?;
        let check = || {
            if cancelled() {
                Err("Document operation cancelled".to_string())
            } else {
                Ok(())
            }
        };
        check()?;
        layer_color::validate_document_color(&project)?;
        let mut gpu = self
            .gpu
            .rasterizer(project.composition().color, &self.options, true)?;
        let mut programs = Vec::new();
        for (_, _, effect) in project.artwork.effects.iter() {
            if !programs.contains(&effect.program) { programs.push(effect.program.clone()); }
        }
        let mut validating = !programs.is_empty();
        if validating {
            gpu.request_effect_validation(EffectValidationRequest {
                request_id: 1,
                retained_programs: programs.clone(),
                programs,
            })
            .map_err(|e| e.to_string())?;
        }
        // Preparing startup starts the native compiler. Waiting for validation
        // before this call leaves all embedded shader jobs permanently queued.
        gpu.prepare_startup(&project, &self.brush, false)
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
            UiSession::from_project_localized(Renderer(Some(gpu)), project, None, self.viewport, self.platform, self.localization.clone())?;
        loop {
            check()?;
            candidate.frame(0, 0)?;
            if candidate.can_park_document() {break;}
            let gpu=candidate.renderer_mut().0.as_mut().ok_or("Canvas unavailable")?;
            gpu.device().poll(wgpu::PollType::Poll).map_err(|error|error.to_string())?;
            gpu.poll_startup().map_err(|error|error.to_string())?;
            if Instant::now()>=deadline {return Err("Project canvas preparation timed out".into());}
            std::thread::sleep(Duration::from_millis(2));
        }
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
        let mut project = layer_ui::NewDocumentOptions::default().project(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let layer_core::SourceTarget::Paint(target)=project.working.target.unwrap() else {panic!()};
        let descriptor=project.composition().color.paint_descriptor();
        let pixels=[23,47,83,255].repeat((layer_core::raster::TILE_SIZE*layer_core::raster::TILE_SIZE) as usize);
        let tile=layer_core::raster::RasterTile::backed(layer_core::raster::TileBlob::encode(descriptor,&pixels).unwrap());
        project.artwork.paint.get_mut(target).unwrap().raster=layer_core::raster::RasterRevision::backed(layer_core::raster::RasterData {
            tiles:[(layer_core::raster::TileKey {plane:layer_core::raster::RasterPlane::Color,coordinate:[0;2]},tile)].into(),
            ..Default::default()
        });
        let gpu = WgpuRasterizer::new_native_headless(project.composition().color).unwrap();
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let session = UiSession::from_project_localized(
            Renderer(Some(gpu.into())),
            layer_ui::new_drawing(64, 48, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),
            None,
            [640, 480],
            layer_ui::Platform::Mac,
            localization.clone(),
        )
        .unwrap();
        let admission = layer_ui::DocumentSessions::<()>::default()
            .admission(&session.retained_document_tiles());
        let environment =
            OpenEnvironment::capture(&session, admission, RendererOptions::default()).unwrap();
        assert!(std::sync::Arc::ptr_eq(&environment.localization, &localization));
        assert_eq!(environment.viewport, session.state().camera.viewport);
        let mut candidate = environment.prepare(project.clone(), || false).unwrap();
        assert!(candidate.can_park_document());
        assert!(std::sync::Arc::ptr_eq(candidate.localization(), &localization));
        let gpu = candidate.renderer_mut().0.as_mut().unwrap();
        assert!(gpu.device() == &environment.gpu.device);
        let ready = gpu.poll_startup().unwrap();
        assert!(ready.canvas_ready && ready.brush_ready);
        assert_eq!(
            environment.prepare(project, || true).err().as_deref(),
            Some("Document operation cancelled")
        );
        drop((candidate, environment, session));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
