//! One cancellable color candidate, prepared on the existing GPU owner/device.
use super::*;
use layer_core::{BrushSnapshot, Project, color::DocumentColor};
use std::time::Instant;

pub(super) struct Request {
    id: u64,
    project: Project,
    brush: BrushSnapshot,
    view: ViewState,
    time: f32,
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
        project: Project,
        brush: BrushSnapshot,
        view: ViewState,
        time: f32,
    ) -> Result<(), String> {
        if self.pending_color.is_some() {
            return Err("A color renderer is already being prepared".into());
        }
        self.next_color_request = self
            .next_color_request
            .checked_add(1)
            .ok_or("Color requests exhausted")?;
        let id = self.next_color_request;
        let color = project.document.color;
        let cancelled = Arc::new(AtomicBool::new(false));
        let (reply, receiver) = mpsc::channel();
        self.send(Command::PrepareColor(Box::new(Request {
            id,
            project,
            brush,
            view,
            time,
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
        self.selection = None;
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
        let cache = gtk::glib::user_cache_dir()
            .join("capycanvas")
            .join("shaders");
        let mut renderer = self.renderer.color_candidate_staged_cached(
            &cache,
            request.project.document.color,
        )
        .map_err(error)?;
        renderer.configure_ui_previews(self.view_color.space()).map_err(error)?;
        for (id, asset) in &request.project.assets {
            check()?;
            renderer.prepare_owned_asset(id, asset).map_err(error)?;
        }
        renderer
            .resize_surface(request.view.width_px, request.view.height_px)
            .map_err(error)?;
        renderer
            .prepare_startup(&request.project.document, &request.brush, false)
            .map_err(error)?;
        renderer.finish_startup_cache();
        while !renderer.poll_startup().map_err(error)?.brush_ready {
            check()?;
            renderer
                .device()
                .poll(wgpu::PollType::Poll)
                .map_err(error)?;
            std::thread::sleep(Duration::from_millis(2));
        }
        check()?;
        let document = &request.project.document;
        let restored: Vec<_> = document
            .layers
            .iter()
            .flat_map(|l| {
                std::iter::once((l.id, l.raster.clone()))
                    .chain(l.masks().map(|m| (m.id, m.raster.clone())))
            })
            .collect();
        let packet = FramePacket {
            time_seconds: request.time,
            view: request.view,
            document_extent: [document.width, document.height],
            layers: &document.layers,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &restored,
            reset_layers: true,
            composite_all: true,
        };
        while !renderer.raster_dependencies_ready(packet) {
            check()?;
            renderer
                .device()
                .poll(wgpu::PollType::Poll)
                .map_err(error)?;
            std::thread::sleep(Duration::from_millis(2));
        }
        renderer.submit(packet).map_err(error)?;
        let finished = Arc::new(AtomicBool::new(false));
        let completed = finished.clone();
        renderer
            .queue()
            .on_submitted_work_done(move || completed.store(true, Ordering::Release));
        while !finished.load(Ordering::Acquire) {
            check()?;
            renderer
                .device()
                .poll(wgpu::PollType::Poll)
                .map_err(error)?;
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut presenter = ViewportPresenter::for_surface(
            &renderer,
            self.config.format,
            self.hdr_encoding.unwrap_or_else(|| self.view_color.surface()),
        )
        .map_err(error)?;
        presenter.set_hdr_view(&renderer, request.project.document.color.depth.is_float().then_some(request.project.document.sdr_rendition), if self.preview_sdr { 1. } else { self.display_headroom }).map_err(error)?;
        presenter.prepare_overviews(&renderer);
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
