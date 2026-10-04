//! Private, fully rendered color candidates. Hosts may replace their renderer
//! only after `take_ready`, then commit the matching shared color transaction.
use super::*;
use layer_core::BrushSnapshot;
use layer_render::{EffectValidationRequest, ViewState};

pub struct ColorCanvas {
    renderer: Option<WgpuRasterizer>,
    document: layer_core::Document,
    view: ViewState,
    context: EvaluationContext,
    control: CaptureControl,
    validating: bool,
    analysis: Option<crate::effect_analysis::Job>,
    analysed: bool,
    submitted: bool,
    finished: Arc<AtomicBool>,
}
impl SnapshotGpu {
    pub fn color_canvas(
        &self,
        document: layer_core::Document,
        context: EvaluationContext,
        brush: &BrushSnapshot,
        view: ViewState,
        control: CaptureControl,
    ) -> Result<ColorCanvas, GpuRasterError> {
        control.check()?;
        document.validate(Default::default()).map_err(GpuRasterError::Color)?;
        let mut renderer = WgpuRasterizer::native_staged_on_device(
            self.adapter.clone(),
            self.device.clone(),
            self.queue.clone(),
            document.composition().color,
        )?;
        #[cfg(target_arch = "wasm32")]
        if let Some(encoder) = self.encoder.clone() {
            renderer.set_browser_raster_encoder(encoder);
        }
        renderer.seed_evaluation_context(context.clone());
        #[cfg(target_arch = "wasm32")]
        { renderer.analysis_backing_waiter = self.analysis_backing_waiter.clone(); }
        renderer.resize_surface(view.width_px, view.height_px)?;
        let mut programs = Vec::new();
        for (_, _, definition) in document.artwork.definitions.iter() {
            if !programs.contains(&definition.program) { programs.push(definition.program.clone()); }
        }
        let validating = !programs.is_empty();
        if validating {
            renderer.request_effect_validation(EffectValidationRequest {
                request_id: 1,
                retained_programs: programs.clone(),
                programs,
            })?;
        }
        renderer.prepare_startup(&document, brush, false)?;
        #[cfg(not(target_arch = "wasm32"))]
        renderer.finish_startup_cache();
        Ok(ColorCanvas {
            renderer: Some(renderer),
            context,
            document,
            view,
            control,
            validating,
            analysis: None,
            analysed: false,
            submitted: false,
            finished: Default::default(),
        })
    }
}
impl ColorCanvas {
    /// Browser pipeline compilation must yield rather than block its event loop.
    #[cfg(target_arch = "wasm32")]
    pub async fn compile_step(&mut self) -> Result<(), GpuRasterError> {
        self.control.check()?;
        self.renderer.as_mut().unwrap().compile_startup_step(false).await
    }
    pub fn poll(&mut self) -> Result<bool, GpuRasterError> {
        self.control.check()?;
        let renderer = self.renderer.as_mut().unwrap();
        #[cfg(not(target_arch = "wasm32"))]
        renderer
            .device()
            .poll(wgpu::PollType::Poll)
            .map_err(|e| GpuRasterError::Color(e.to_string()))?;
        if self.submitted {
            return Ok(self.finished.load(Ordering::Acquire));
        }
        if self.validating
            && let Some(result) = renderer.take_effect_validation()
        {
            result.result.map_err(GpuRasterError::Color)?;
            self.validating = false;
        }
        let ready = renderer.poll_startup()?;
        if self.validating || !ready.canvas_ready || !ready.brush_ready {
            return Ok(false);
        }
        let document = &self.document;
        let scene = document.scene();
        if !self.analysed && scene.order().iter().any(|&h| scene.visible(h) && scene.effect(h).is_some_and(|effect| effect.program.analysis().is_some())) {
            if let Some(job) = &mut self.analysis {
                let Some(candidate) = job.take() else { return Ok(false); };
                renderer.apply_effect_analysis(candidate.map_err(GpuRasterError::Effect)?);
                self.analysis = None; self.analysed = true;
            } else {
                self.analysis = Some(crate::effect_analysis::Job::frame(renderer.snapshot_gpu(), crate::effect_analysis::BakeInput {
                    scene: document.snapshot_with_context(self.context.clone()), scope: SceneScope::All, offset: layer_core::Point::default(), extent: document.composition().size,
                    color: document.composition().color, blend: document.composition().blend, time: self.context.elapsed,
                }).map_err(GpuRasterError::Effect)?);
                return Ok(false);
            }
        }

        let restored: Vec<_> = scene.targets().filter_map(|t| scene.raster(t).map(|r| (t, r.clone()))).collect();
        let packet = FramePacket {
            commit_rasters: true,
            time_seconds: self.context.elapsed,
            view: self.view,
            document_extent: document.composition().size,
            scene,
            selection_visibility: None,
            inspect_mask: None,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &restored,
            reset_layers: true,
            composite_all: true,
            blend_space: document.composition().blend,
        };
        if !renderer.raster_dependencies_ready(packet) {
            return Ok(false);
        }
        renderer.submit(packet)?;
        let finished = self.finished.clone();
        renderer
            .queue()
            .on_submitted_work_done(move || finished.store(true, Ordering::Release));
        self.submitted = true;
        Ok(false)
    }
    pub fn take_ready(mut self) -> Result<WgpuRasterizer, GpuRasterError> {
        self.control.check()?;
        if !self.submitted || !self.finished.load(Ordering::Acquire) {
            return Err(GpuRasterError::Color("Color canvas is not ready".into()));
        }
        Ok(self.renderer.take().unwrap())
    }
}
