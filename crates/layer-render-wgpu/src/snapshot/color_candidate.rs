//! Private, fully rendered color candidates. Hosts may replace their renderer
//! only after `take_ready`, then commit the matching shared color transaction.
use super::*;
use layer_core::BrushSnapshot;
use layer_render::{EffectValidationRequest, ViewState};

pub struct ColorCanvas {
    renderer: Option<WgpuRasterizer>,
    project: Project,
    view: ViewState,
    time: f32,
    control: CaptureControl,
    validating: bool,
    submitted: bool,
    finished: Arc<AtomicBool>,
}
impl SnapshotGpu {
    pub fn color_canvas(
        &self,
        project: Project,
        brush: &BrushSnapshot,
        view: ViewState,
        time: f32,
        control: CaptureControl,
    ) -> Result<ColorCanvas, GpuRasterError> {
        control.check()?;
        project
            .validate(Default::default())
            .map_err(GpuRasterError::Color)?;
        let mut renderer = WgpuRasterizer::native_staged_on_device(
            self.adapter.clone(),
            self.device.clone(),
            self.queue.clone(),
            project.document.color,
        )?;
        #[cfg(target_arch = "wasm32")]
        if let Some(encoder) = self.encoder.clone() {
            renderer.set_browser_raster_encoder(encoder);
        }
        for (id, asset) in &project.assets {
            control.check()?;
            renderer.prepare_owned_asset(id, asset)?;
        }
        renderer.resize_surface(view.width_px, view.height_px)?;
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
        let validating = !programs.is_empty();
        if validating {
            renderer.request_effect_validation(EffectValidationRequest {
                request_id: 1,
                namespace: programs.clone(),
                programs,
            })?;
        }
        renderer.prepare_startup(&project.document, brush, false)?;
        #[cfg(target_arch = "wasm32")]
        renderer.startup_catalog_submitted();
        #[cfg(not(target_arch = "wasm32"))]
        renderer.finish_startup_cache();
        Ok(ColorCanvas {
            renderer: Some(renderer),
            project,
            view,
            time,
            control,
            validating,
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
        self.renderer.as_mut().unwrap().compile_startup_step().await
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
        let document = &self.project.document;
        let restored: Vec<_> = document
            .layers
            .iter()
            .flat_map(|l| {
                std::iter::once((l.id, l.raster.clone()))
                    .chain(l.masks().map(|m| (m.id, m.raster.clone())))
            })
            .collect();
        let packet = FramePacket {
            time_seconds: self.time,
            view: self.view,
            document_extent: [document.width, document.height],
            layers: &document.layers,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &restored,
            reset_layers: true,
            composite_all: true,
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
