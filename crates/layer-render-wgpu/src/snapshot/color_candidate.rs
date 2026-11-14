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
    finished: Option<Arc<std::sync::atomic::AtomicU8>>,
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
        let mut renderer = WgpuRasterizer::native_staged_on_factory(
            self.factory.clone(), self.device.clone(), document.composition().color,
        )?;
        #[cfg(target_arch = "wasm32")]
        self.install_browser_workers(&mut renderer);
        if let Some(wake)=&self.preparation_wake {renderer.set_preparation_wake(wake.clone());}
        renderer.seed_evaluation_context(context.clone());
        renderer.resize_surface(view.width_px, view.height_px)?;
        let mut programs = Vec::new();
        for (_, _, application) in document.artwork.effects.iter() {
            if !programs.contains(&application.program) { programs.push(application.program.clone()); }
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
        renderer.finish_startup_catalog();
        Ok(ColorCanvas {
            renderer: Some(*renderer),
            context,
            document,
            view,
            control,
            validating,
            analysis: None,
            analysed: false,
            submitted: false,
            finished: None,
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
        if let Some(finished) = &self.finished {
            return match finished.load(Ordering::Acquire) {0=>Ok(false),1=>Ok(true),_=>Err(GpuRasterError::Color("The operation stopped".into()))};
        }
        let renderer = self.renderer.as_mut().unwrap();
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

        let restored: Vec<_> = scene.targets().filter(|_| !self.submitted).filter_map(|t| scene.raster(t).map(|r| (t, r.clone()))).collect();
        let packet = FramePacket {
            commit_rasters: true,
            time_seconds: self.context.elapsed,
            view: self.view,
            document_extent: document.composition().size,
            scene,
            selection_overlays: None,
            inspect_mask: None,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &restored,
            reset_layers: !self.submitted,
            composite_all: !self.submitted,
            blend_space: document.composition().blend,
        };
        if !renderer.can_submit() || !renderer.raster_dependencies_ready(packet, None) {
            return Ok(false);
        }
        renderer.submit(packet)?;
        self.submitted = true;
        if renderer.has_presentable_frame() {
            let finished = Arc::new(std::sync::atomic::AtomicU8::new(0));
            let done = Completion(finished.clone());
            renderer.device.on_queue_done(renderer.queue(),move || {done.0.store(1,Ordering::Release);drop(done);});
            self.finished = Some(finished);
        }
        Ok(false)
    }
    pub fn take_ready(mut self) -> Result<WgpuRasterizer, GpuRasterError> {
        self.control.check()?;
        if self.finished.as_ref().is_some_and(|finished|finished.load(Ordering::Acquire)==2) {return Err(GpuRasterError::Color("The operation stopped".into()));}
        if !self.finished.as_ref().is_some_and(|finished| finished.load(Ordering::Acquire)==1) {
            return Err(GpuRasterError::Color("Color canvas is not ready".into()));
        }
        Ok(self.renderer.take().unwrap())
    }
}

struct Completion(Arc<std::sync::atomic::AtomicU8>);
impl Drop for Completion {fn drop(&mut self) {let _=self.0.compare_exchange(0,2,Ordering::AcqRel,Ordering::Acquire);}}
