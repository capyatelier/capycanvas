//! One cancellable color candidate, prepared on the existing GPU owner/device.
use super::*;
use layer_core::{BrushSnapshot, Document, color::DocumentColor};
use std::time::Instant;

pub(super) struct Request {
    id: u64,
    project: Document,
    context: EvaluationContext,
    brush: BrushSnapshot,
    view: ViewState,
    cancelled: Arc<AtomicBool>,
    reply: mpsc::Sender<Result<(), String>>,
}
pub(super) struct Pending {
    id: u64,
    color: DocumentColor,
    cancelled: Arc<AtomicBool>,
    reply: mpsc::Receiver<Result<(), String>>,
    ready: bool,
}
pub(super) struct Prepared {
    id: u64,
    renderer: WgpuRasterizer,
    presenter: ViewportPresenter,
}

impl RenderWorker {
    pub(crate) fn prepare_color(
        &mut self,
        project: Document,
        mut context: EvaluationContext,
        brush: BrushSnapshot,
        view: ViewState,
        ) -> Result<(), String> {
        if self.pending_color.is_some() {
            return Err("A color renderer is already being prepared".into());
        }
        self.next_color_request = self
            .next_color_request
            .checked_add(1)
            .ok_or("Color requests exhausted")?;
        let id = self.next_color_request;
        let color = project.composition().color;
        context.retain_effects(&project.artwork);
        let cancelled = Arc::new(AtomicBool::new(false));
        let (reply, receiver) = mpsc::channel();
        self.send(Command::PrepareColor(Box::new(Request {
            id,
            project,
            context,
            brush,
            view,
            cancelled: cancelled.clone(),
            reply,
        })))
        .map_err(error)?;
        self.pending_color = Some(Pending {
            id,
            color,
            cancelled,
            reply: receiver,
            ready: false,
        });
        Ok(())
    }

    pub(crate) fn poll_prepared_color(&mut self) -> Option<Result<(), String>> {
        let pending = self.pending_color.as_mut()?;
        if pending.ready {
            return None;
        }
        let result = match pending.reply.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Color renderer preparation stopped".into())
            }
        };
        if result.is_ok() {
            pending.ready = true;
        } else {
            self.pending_color = None;
        }
        Some(result)
    }

    /// The acknowledgement follows candidate destruction, including when the
    /// prepare command is still running. Keep the document reserved until then.
    pub(crate) fn discard_prepared_color(&mut self) -> Result<mpsc::Receiver<()>, String> {
        let (reply, receiver) = mpsc::channel();
        if let Some(pending) = self.pending_color.take() {
            pending.cancelled.store(true, Ordering::Release);
            self.send(Command::DiscardColor(pending.id, reply))
                .map_err(error)?;
        } else {
            let _ = reply.send(());
        }
        Ok(receiver)
    }

    pub(super) fn adopt_color(&mut self, color: DocumentColor) -> Result<bool, BackendError> {
        let Some(pending) = &self.pending_color else {
            return Ok(false);
        };
        if !pending.ready || pending.color != color || pending.cancelled.load(Ordering::Acquire) {
            return Ok(false);
        }
        let id = pending.id;
        // Drain old interpretation-dependent replies before invalidating them.
        self.ready()
            .map_err(|_| BackendError("GPU worker stopped before color adoption"))?;
        self.send(Command::AdoptColor(id))?;
        self.awaiting_color_adoption = Some(id);
        self.pending_color = None;
        self.color = color;
        self.hdr_view = None;
        self.startup_generation += 1;
        self.startup = layer_render_wgpu::StartupProgress::COMPLETE;
        self.startup_key = None;
        let overlays = Arc::make_mut(&mut self.overlays);
        overlays.selection = None;
        overlays.paper_generation += 1;
        self.transform_preview = None;
        self.region = None;
        self.region_pending = false;
        self.color_sample = None;
        self.color_sample_pending = false;
        self.filter_previews.clear();
        self.filter_previews_pending = false;
        self.thumbnails.clear();
        self.effect_validation = None;
        self.effect_validation_pending = false;
        self.brush_sources.clear();
        Ok(true)
    }
}

impl Worker {
    pub(super) fn prepare_color(&mut self, request: Request, startup_busy: bool) {
        #[cfg(test)]
        crate::workspace::tests::providers::enter(crate::workspace::tests::providers::Provider::Color);
        let result = if startup_busy || self.prepared_color.is_some() {
            Err("The canvas renderer is still preparing another operation".into())
        } else {
            self.color_candidate(&request)
        };
        let result = result.map(|candidate| {
            self.prepared_color = Some(candidate);
        });
        if request.reply.send(result).is_err() {
            self.prepared_color = None;
        }
    }

    fn color_candidate(&self, request: &Request) -> Result<Prepared, String> {
        let deadline = Instant::now() + Duration::from_secs(60);
        let check = || {
            if request.cancelled.load(Ordering::Acquire) {
                Err("Color preparation cancelled".to_string())
            } else if Instant::now() >= deadline {
                Err("Color renderer preparation timed out".to_string())
            } else {
                Ok(())
            }
        };
        check()?;
        request.project.validate(Default::default())?;
        let mut renderer = self.renderer.color_candidate_staged(
            request.project.composition().color,
        )
        .map_err(error)?;
        renderer.configure_ui_previews(self.view_color.space()).map_err(error)?;
        renderer
            .resize_surface(request.view.width_px, request.view.height_px)
            .map_err(error)?;
        renderer
            .prepare_startup(&request.project, &request.brush, false)
            .map_err(error)?;
        renderer.finish_startup_catalog();
        while !renderer.poll_startup().map_err(error)?.brush_ready {
            check()?;
            std::thread::sleep(Duration::from_millis(2));
        }
        check()?;
        let document = &request.project;
        renderer.seed_evaluation_context(request.context.clone());
        let scene = document.scene();
        let restored: Vec<_> = scene.targets().filter_map(|t| scene.raster(t).map(|r| (t,r.clone()))).collect();
        let packet = FramePacket {
            commit_rasters: true,
            time_seconds: request.context.elapsed,
            view: request.view,
            document_extent: document.composition().size,
            scene,
            inspect_mask: None,
            selection_overlays: Some(&document.working.selection_overlays),
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &restored,
            reset_layers: true,
            composite_all: true,
            blend_space: document.composition().blend,
        };
        loop {
            match renderer.submit_when_ready(packet, None) {
                std::task::Poll::Ready(result) => { result.map_err(error)?; break; }
                std::task::Poll::Pending => {},
            }
            check()?;
            std::thread::sleep(Duration::from_millis(2));
        }
        let (completed,finished)=mpsc::channel();
        renderer.on_gpu_done(move || {let _=completed.send(());});
        loop {
            match finished.try_recv() {
                Ok(())=>break,
                Err(mpsc::TryRecvError::Disconnected)=>return Err("The operation stopped".into()),
                Err(mpsc::TryRecvError::Empty)=>{},
            }
            check()?;
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut presenter = workspace_presenter(
            &renderer,
            self.config.format,
            self.hdr_encoding.unwrap_or_else(|| self.view_color.surface()),
        )?;
        presenter.set_hdr_view(&renderer, request.project.composition().color.depth.is_float().then_some(request.project.output().sdr), if self.preview_sdr { 1. } else { self.display_headroom }).map_err(error)?;
        while !presenter.prepare_chrome(&renderer) {
            check()?;
            renderer.poll_startup().map_err(error)?;
            std::thread::sleep(Duration::from_millis(2));
        }
        check()?;
        Ok(Prepared {
            id: request.id,
            renderer,
            presenter,
        })
    }

    pub(super) fn adopt_color(&mut self, id: u64, telemetry: bool) -> Result<(), String> {
        if self.prepared_color.as_ref().is_none_or(|p| p.id != id) {
            return Err("Prepared color renderer is missing".into());
        }
        let candidate = self.prepared_color.take().unwrap();
        self.renderer = candidate.renderer;
        self.overlays = None;
        self.presenter = candidate.presenter;
        self.renderer.set_telemetry_enabled(telemetry);
        self.last_view = None;
        self.pending_present = false;
        Ok(())
    }

    pub(super) fn discard_color(&mut self, id: u64) {
        if self.prepared_color.as_ref().is_some_and(|p| p.id == id) {
            self.prepared_color = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_adoption_rejects_paper_receipts_from_the_previous_renderer() {
        let (mut renderer, _commands, _chrome, _reply) = RenderWorker::channels(Default::default());
        let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let paper = document.nearest_paper_binding(None).unwrap();
        renderer.set_paper_overlay(Some(layer_render::PaperOverlay { paper: paper.clone(), opacity: 0.5, moving: true }));
        let previous = renderer.overlays.clone();
        renderer.report.paper_drawn.store(previous.paper_generation, Ordering::Release);
        assert!(renderer.paper_overlay_ready(&paper));
        renderer.pending_color = Some(Pending { id: 1, color: renderer.color, cancelled: Arc::default(), reply: mpsc::channel().1, ready: true });
        assert!(renderer.adopt_color(renderer.color).unwrap());
        renderer.report.paper_drawn.store(previous.paper_generation, Ordering::Release);
        assert!(!renderer.paper_overlay_ready(&paper));
        renderer.report.paper_drawn.store(renderer.overlays.paper_generation, Ordering::Release);
        assert!(renderer.paper_overlay_ready(&paper));
    }
}
