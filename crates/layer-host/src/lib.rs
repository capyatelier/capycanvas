//! Shared transport facade for native hosts. No UI toolkit or surface ownership.
//! Call from one engine/render owner; platform callbacks enqueue owned batches.
pub mod clipboard;
pub mod export;
pub mod gpu;
mod header;
mod snapshot;
mod model_update;
pub mod open;
pub mod scene;
pub mod storage;
pub mod tasks;
pub mod tone;
pub mod window;
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_render::CanvasRenderer;
use layer_ui::{ContactPhase, PointerButton, PointerKind, UiAction, UiInput, UiSession};
pub use gpu::{DeviceWatch, GpuContext, RendererOptions, UiColor};
pub use storage::StorageRoots;
pub use layer_render_wgpu::AttachedRenderer as Renderer;
pub use snapshot::extend_update;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, PartialEq)]
struct SnapshotKey {
    revision: u64,
    localization_generation: u64,
    command_search_revision: u64,
    logical: [f32; 2],
    chrome_hidden: bool,
    keep_zen_button: bool,
    navigation_cursor: Option<layer_ui::NavigationMode>,
    gpu_ready: bool,
    startup: layer_render_wgpu::StartupProgress,
    error: Option<String>,
}

/// Owned by the caller for this call. The camera revision is captured with the
/// samples, not looked up after they have waited in a platform work queue.
pub struct PointerBatch<'a> {
    pub id: u64,
    pub tool: u8,
    pub button: u8,
    /// Records: x/y, pressure, tilt x/y, twist, distance, monotonic ns, phase.
    /// Phase 0 hover, 1 down, 2 move, 3 up, 4 cancel. Tool 0 pen, 1 mouse,
    /// 2 eraser, 3 touch. Pointer routing is decided by the shared core.
    pub records: &'a [f64],
    pub predicted: bool,
    pub view_revision: u64,
    /// Twist is a measured barrel rotation rather than an absent axis.
    pub barrel_twist: bool,
}

pub struct NativeHost {
    pub session: UiSession<Renderer>,
    /// Number of drawings owned by the window, for shared header geometry.
    pub document_count: usize,
    /// Configure before publishing UI; hosts tag these preview values to match.
    pub ui_color: UiColor,
    pub logical: [f32; 2],
    pub dirty: bool,
    pub chrome_hidden: bool,
    keep_zen_button: bool,
    pub error: Option<String>,
    pub sequence: u64,
    pub startup: layer_render_wgpu::StartupProgress,
    pub(crate) document_close_prepared: bool,
    pub proof: layer_ui::proof_workflow::ProofView,
    deferred_contacts: layer_engine::DeferredContacts,
    last_pen: Option<PenEvent>,
    paint_start_sequence: u64,
    last_snapshot: Option<SnapshotKey>,
    last_model_snapshot: Option<model_update::ModelBaseline>,
    model_transport: bool,
    last_workspace_model_revision: Option<u64>,
    last_workspace_content_revision: Option<u64>,
    last_camera_revision: Option<u64>,
    document_view_revision: u64,
    service_changes: u32,
    header_drag: Option<layer_ui::HeaderDrag>,
    preview_clock: std::time::Instant,
    filter_preview_image: Option<layer_render::FilterPreviewImage>,
    catalog: Box<layer_ui::UiCatalog>,
    localization_generation: u64,
    thumbnails: layer_ui::ThumbnailRequests,
}

impl NativeHost {
    fn require_document_owner(&self) -> Result<(), String> {
        if self.document_close_prepared {
            return Err(layer_ui::DocumentTransportRefusal::ChangeInProgress.message(self.session.localization()).to_string());
        }
        Ok(())
    }
    pub fn document_close_prepared(&self) -> bool { self.document_close_prepared }

    fn prepare_ui_previews(&mut self) -> Result<(), String> {
        let rendition = self.session.engine().document().composition().color.depth.is_float()
            .then(|| self.session.effective_sdr_rendition());
        if let Some(gpu) = self.session.renderer_mut().0.as_mut() {
            gpu.set_ui_rendition(rendition).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    /// Native adapters transfer only metadata and the completed packed atlas.
    pub fn poll_filter_previews(
        &mut self,
        filters: Vec<std::sync::Arc<str>>,
        size: [u32; 2],
        cache: layer_ui::FilterPreviewCache,
    ) -> Result<layer_ui::FilterPreviewUpdate, String> {
        self.prepare_ui_previews()?;
        let ready = self.session.engine().backend().0.is_some();
        self.session.poll_filter_previews(
            self.preview_clock.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            if ready { filters } else { Vec::new() },
            size,
            cache,
        )
    }
    pub fn take_filter_preview_image(&mut self) -> Option<layer_render::FilterPreviewImage> {
        self.filter_preview_image.take()
    }
    pub fn launch(platform: layer_ui::Platform, saved: &str, preferred_tags: &[&str]) -> Result<Self, String> {
        Self::launch_localized(platform, saved, layer_ui::launch_localization(saved, preferred_tags))
    }
    pub fn launch_localized(platform: layer_ui::Platform, saved: &str, localization: std::sync::Arc<layer_ui::Localizer>) -> Result<Self, String> {
        let mut host = Self::new_localized(platform, localization)?;
        if !saved.is_empty() {
            host.dispatch(UiAction::RestoreSavedSettings { saved: saved.to_owned() })?;
        }
        Ok(host)
    }
    pub fn bootstrap_view(&self) -> layer_ui::BootstrapView {
        layer_ui::bootstrap_view(self.session.localization())
    }
    pub fn localization_generation(&self) -> u64 {
        self.localization_generation
    }
    pub fn set_localization(&mut self, localization: std::sync::Arc<layer_ui::Localizer>) -> bool {
        if !self.session.set_localization(localization) {
            return false;
        }
        *self.catalog = layer_ui::ui_catalog_localized(self.session.localization());
        self.localization_generation = self.localization_generation.saturating_add(1);
        self.last_snapshot = None;
        self.last_model_snapshot = None;
        self.last_workspace_model_revision = None;
        self.last_workspace_content_revision = None;
        true
    }
    pub fn new(platform: layer_ui::Platform) -> Result<Self, String> {
        Self::new_localized(platform, layer_ui::Localizer::shared(layer_ui::UiLanguage::English))
    }
    pub fn new_localized(
        platform: layer_ui::Platform,
        localization: std::sync::Arc<layer_ui::Localizer>,
    ) -> Result<Self, String> {
        let catalog = Box::new(layer_ui::ui_catalog_localized(&localization));
        let session = UiSession::blank_localized(Renderer::default(), [1, 1], platform, localization)?;
        Ok(Self {
            session,
            ui_color: UiColor::Mapped,
            logical: [1.0, 1.0],
            dirty: true,
            chrome_hidden: false,
            keep_zen_button: false,
            error: None,
            sequence: 0,
            proof: Default::default(),
            // Eager hosts are ready on GPU attachment; staged hosts reset this.
            startup: layer_render_wgpu::StartupProgress::COMPLETE,
            document_close_prepared: false,
            deferred_contacts: Default::default(),
            last_pen: None,
            paint_start_sequence: 0,
            last_snapshot: None,
            last_model_snapshot: None,
            model_transport: false,
            last_workspace_model_revision: None,
            last_workspace_content_revision: None,
            last_camera_revision: None,
            document_count: 1,
            document_view_revision: 0,
            service_changes: 0,
            header_drag: None,
            preview_clock: std::time::Instant::now(),
            filter_preview_image: None,
            catalog,
            localization_generation: 0,
            thumbnails: Default::default(),
        })
    }
    /// Regions changed since the platform service last observed accepted input.
    /// Independent of snapshot publication, so taking a UI update cannot erase
    /// a pending persistence observation. Call only from the exclusive owner.
    pub fn take_service_changes(&mut self) -> u32 {
        std::mem::take(&mut self.service_changes)
    }
    pub fn renderer_options(&self, cache: Option<std::path::PathBuf>) -> RendererOptions {
        RendererOptions {
            cache,
            ui_color: self.ui_color,
        }
    }
    /// Republish host-owned service state without changing the shared document.
    pub fn invalidate_snapshot(&mut self) {
        self.last_snapshot = None;
    }
    /// Invalidate host input and snapshot caches after shared document adoption.
    pub fn document_adopted(&mut self) {
        self.header_drag = None;
        self.deferred_contacts.clear();
        self.session.set_input_held(false);
        self.last_pen = None;
        self.last_snapshot = None;
        self.last_camera_revision = None;
        self.document_view_revision = self.session.state().camera.revision;
        self.dirty = true;
    }
    pub fn resize(&mut self, width: u32, height: u32, density: f32) -> Result<(), String> {
        if width == 0 || height == 0 || !density.is_finite() || density <= 0.0 {
            return Err("Invalid native surface dimensions".into());
        }
        let logical = [width as f32 / density, height as f32 / density];
        if self.logical != logical {
            self.header_drag = None;
        }
        self.logical = logical;
        self.session.set_viewport(self.logical, [width, height])?;
        self.dirty = true;
        Ok(())
    }
    /// Shared staged GPU lifecycle. Presenters submit the first constant-fill frame
    /// before passing `has_presented = true`; it must not consume document replay.
    pub fn prepare_canvas_frame(
        &mut self,
        now: u64,
        presentation: u64,
        has_presented: bool,
    ) -> Result<(), String> {
        if has_presented || self.session.renderer_mut().0.as_mut()
            .ok_or("Missing native renderer")?.poll_startup().map_err(|e| e.to_string())?.canvas_ready
        {
            let engine = self.session.engine();
            let gpu = engine
                .backend()
                .0
                .as_ref()
                .ok_or("Missing native renderer")?;
            let transform = engine.transform_preview().is_some();
            if gpu.startup_needs_update(engine.document(), engine.brush(), transform) {
                let (document, brush) = (engine.document().clone(), engine.brush().clone());
                self.session
                    .renderer_mut()
                    .0
                    .as_mut()
                    .unwrap()
                    .prepare_startup(&document, &brush, transform)
                    .map_err(|e| e.to_string())?;
            }
            self.startup = self
                .session
                .renderer_mut()
                .0
                .as_mut()
                .unwrap()
                .poll_startup()
                .map_err(|e| e.to_string())?;
            let released = self.deferred_contacts.release(self.paint_ready(), std::time::Instant::now());
            self.session.set_input_held(self.deferred_contacts.holding());
            for event in released {
                self.deliver(event)?;
            }
            if self.startup.canvas_ready {
                let previous = self.session.state().revision;
                let change = self.session.frame(now, presentation)?;
                self.dirty = change.canvas_wake;
                self.apply_change(previous, change);
            }
        } else {
            self.session.submit_backdrop_frame()?;
        }
        self.dirty |= !self.startup.complete || !self.deferred_contacts.is_empty();
        Ok(())
    }
    pub fn dispatch(&mut self, action: UiAction) -> Result<(), String> {
        self.require_document_owner()?;
        if matches!(action, UiAction::RestoreWorkspace { .. }) {
            self.header_drag = None;
        }
        let previous = self.session.state().revision;
        let change = self.session.dispatch(action)?;
        self.apply_change(previous, change);
        Ok(())
    }

    /// Idle-time layer thumbnails with owned pixels, also used by JSON adapters.
    /// Native binary bridges can keep pixel arrays out of their metadata format.
    pub fn layer_thumbnails(
        &mut self,
        requests: impl IntoIterator<Item = (u64, u64)>,
    ) -> Result<(Vec<u64>, Vec<layer_render::ReadbackImage>), String> {
        self.prepare_ui_previews()?;
        let retry = self.thumbnails.retry(&self.session);
        let mut accepted = Vec::new();
        // Background brush/filter warmup keeps the canvas dirty after document
        // pixels settle. It must not starve visible thumbnails. Still yield to
        // active input, pending edits and shared editor background operations.
        if self.startup.canvas_ready
            && self.session.background_readback_idle()
            && self.session.engine().backend().0.as_ref().is_some_and(|gpu| gpu.ui_readback_ready())
        {
            let requests = requests.into_iter().map(|(request, target)| layer_render::ThumbnailTarget::from_wire_id(target)
                .map(|target| (request, target, false)).ok_or("Invalid thumbnail identity"));
            for next in retry.map(|(request, target)| Ok((request, target, true))).into_iter().chain(requests).take(1) {
                let (request, target, retry) = next?;
                // Match GTK's bounded cold-photo work. The UI retries requests
                // that are not yet accepted, leaving input/frame opportunities
                // between batches instead of scanning an entire photo here.
                let prepared = self.session.renderer_mut().0.as_mut().unwrap().prepare_thumbnail_batch(target);
                let submitted = match prepared {
                    Ok(true) => self.session.renderer_mut().request_thumbnail(request, target).map(|()| true),
                    Ok(false) => continue,
                    Err(error) => Err(error),
                };
                match submitted {
                    Ok(_) => {
                        if !retry { accepted.push(request); }
                        self.thumbnails.submitted(request, target, retry);
                    }
                    Err(error) if retry => self.thumbnails.refused(&self.session, matches!(error, layer_render_wgpu::GpuRasterError::ThumbnailUnavailable(_))),
                    Err(layer_render_wgpu::GpuRasterError::ThumbnailUnavailable(_)) => (),
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
        let mut images = Vec::new();
        if let Some(gpu) = &self.session.renderer_mut().0 {
            gpu.device()
                .poll(wgpu::PollType::Poll)
                .map_err(|e| e.to_string())?;
        }
        while let Some(image) = self.session.renderer_mut().take_thumbnail() {
            let image = image.map_err(|e| e.to_string())?;
            self.thumbnails.completed(image.request_id);
            images.push(image);
        }
        self.dirty |= self.session.engine().wants_continuous_frames();
        Ok((accepted, images))
    }
    /// Camera motion changes the core revision without changing the workspace
    /// models. Only acknowledge it if no unpublished structural change precedes
    /// it; otherwise the next snapshot must still include that pending change.
    pub fn apply_change(&mut self, previous: u64, change: layer_ui::UiChange) {
        self.service_changes |= change.regions;
        self.dirty |= change.canvas_wake;
        if change.regions == layer_ui::regions::CAMERA
            && let Some(key) = &mut self.last_snapshot
            && key.revision == previous
        {
            key.revision = change.revision;
        }
    }
    pub fn input(&mut self, input: UiInput) -> Result<layer_ui::InputReply, String> {
        if self.document_close_prepared {
            return Ok(layer_ui::InputReply {change:layer_ui::UiChange {revision:self.session.state().revision,..Default::default()},
                handled:true,chrome_hidden:self.chrome_hidden,keep_zen_button:self.keep_zen_button,pan_cursor:self.session.navigation_mode()==Some(layer_ui::NavigationMode::Pan),navigation_cursor:self.session.navigation_mode(),..Default::default()});
        }
        if matches!(input, UiInput::Blur) {
            self.header_drag = None;
        }
        let previous = self.session.state().revision;
        let reply = self.session.input(input)?;
        self.chrome_hidden = reply.chrome_hidden;
        self.keep_zen_button = reply.keep_zen_button;
        self.apply_change(previous, reply.change);
        if reply.cancel_paint {
            self.cancel_pen()?;
        }
        Ok(reply)
    }
    pub fn scroll(
        &mut self,
        anchor: [f32; 2],
        delta: [f32; 2],
        density: f32,
        zoom: bool,
        horizontal: bool,
    ) -> Result<(), String> {
        if self.document_close_prepared { return Ok(()); }
        let previous = self.session.state().revision;
        let change = self
            .session
            .scroll(anchor, delta, density, zoom, horizontal)?;
        self.apply_change(previous, change);
        Ok(())
    }
    pub fn gesture(&mut self, anchor: [f32; 2], scale: f32, rotation: f32, began: bool) -> Result<(), String> {
        if !anchor
            .into_iter()
            .chain([scale, rotation])
            .all(f32::is_finite)
            || scale <= 0.0
        {
            return Err("Invalid native gesture".into());
        }
        if self.document_close_prepared { return Ok(()); }
        let previous = self.session.state().revision;
        if began { self.session.begin_view_gesture(); }
        let change = self.session.gesture(anchor, anchor, scale, rotation)?;
        self.apply_change(previous, change);
        Ok(())
    }
    fn cancel_pen(&mut self) -> Result<(), String> {
        if let Some(mut event) = self.last_pen.take() {
            event.phase = PenPhase::Cancel;
            self.sequence += 1;
            event.sequence = self.sequence;
            self.enqueue(event)?;
        }
        Ok(())
    }
    fn enqueue(&mut self, event: PenEvent) -> Result<(), String> {
        if let Err(event) = self.session.pen(event) {
            // Relieve queue pressure through an actual frame boundary. Raster
            // commits require GPU submission; CPU-only draining cannot save them.
            let previous = self.session.state().revision;
            let change = self.session.frame(event.timestamp_ns, event.timestamp_ns)?;
            self.apply_change(previous, change);
            self.session
                .pen(event)
                .map_err(|_| "Pen queue remained full")?;
        }
        self.dirty = true;
        Ok(())
    }

    pub fn accepts_pointer_input(&self, view_revision: u64) -> bool {
        view_revision >= self.document_view_revision
            && !self.session.state().document_file.close_ready
    }
    /// Latest admitted real paint contact, before a frame consumes its input.
    /// Surface hosts can prepare low-latency presentation before issuing ink.
    pub fn paint_start_sequence(&self) -> u64 {
        self.paint_start_sequence
    }
    pub fn pointer_batch(&mut self, batch: PointerBatch<'_>) -> Result<(), String> {
        self.pointer_batch_updates(batch, &[], false)
    }
    /// Optional pairs per sample: contact-local estimate token and whether
    /// further corrections are expected (0/1). Corrections bypass UI routing
    /// and pointer ownership: they refer to previously admitted paint only.
    pub fn pointer_batch_updates(
        &mut self,
        batch: PointerBatch<'_>,
        updates: &[u64],
        correction: bool,
    ) -> Result<(), String> {
        let PointerBatch {
            id,
            tool,
            button,
            records,
            predicted,
            view_revision,
            barrel_twist,
        } = batch;
        if records.is_empty()
            || !records.len().is_multiple_of(9)
            || !records.iter().all(|n| n.is_finite())
            || records
                .as_chunks::<9>()
                .0
                .iter()
                .any(|r| r[7] < 0.0 || r[8] < 0.0 || r[8] > 4.0 || r[8].fract() != 0.0)
        {
            return Err("Invalid native pointer batch".into());
        }
        if (!updates.is_empty() && updates.len() != records.len() / 9 * 2)
            || updates
                .as_chunks::<2>()
                .0
                .iter()
                .any(|u| u[1] > 1 || (u[1] != 0 && u[0] == 0))
            || (correction
                && (predicted
                    || updates.is_empty()
                    || updates.as_chunks::<2>().0.iter().any(|u| u[0] == 0)))
            || (predicted && !updates.is_empty())
        {
            return Err("Invalid native input estimates".into());
        }
        if !self.accepts_pointer_input(view_revision) {
            return Ok(());
        }
        for (index, sample) in records.as_chunks::<9>().0.iter().enumerate() {
            let update = updates.get(index * 2..index * 2 + 2).unwrap_or(&[0, 0]);
            let phase = match sample[8] as u8 {
                0 => PenPhase::Hover,
                1 => PenPhase::Down,
                2 => PenPhase::Move,
                3 => PenPhase::Up,
                _ => PenPhase::Cancel,
            };
            let position = [sample[0] as f32, sample[1] as f32];
            let event = PenEvent {
                device_id: id,
                sequence: if update[0] != 0 {
                    update[0]
                } else {
                    self.sequence + 1
                },
                timestamp_ns: sample[7] as u64,
                view_revision,
                surface_position: Point {
                    x: position[0],
                    y: position[1],
                },
                pressure: sample[2] as f32,
                tilt_radians: [sample[3] as f32, sample[4] as f32],
                twist_radians: sample[5] as f32,
                distance: sample[6] as f32,
                phase,
                tool: match tool {
                    1 => ToolKind::Mouse,
                    2 => ToolKind::Eraser,
                    3 => ToolKind::Finger,
                    _ => ToolKind::Pen,
                },
                flags: SampleFlags(
                    SampleFlags::PRIMARY.0
                        | if correction {
                            SampleFlags::CORRECTION.0
                        } else {
                            0
                        }
                        | if update[1] != 0 {
                            SampleFlags::ESTIMATED.0
                        } else {
                            0
                        }
                        | if predicted {
                            SampleFlags::PREDICTED.0
                        } else {
                            0
                        }
                        | if barrel_twist {
                            SampleFlags::BARREL_TWIST.0
                        } else {
                            0
                        },
                ),
            };
            if correction {
                // Corrections refer to previously admitted sample tokens and never
                // enter UI pointer ownership or replace the current cursor.
                self.sequence += 1;
                self.admit(event)?;
                continue;
            }
            self.pointer_event_inner(
                event,
                match button {
                    0 => PointerButton::Primary,
                    1 => PointerButton::Pan,
                    _ => PointerButton::Other,
                },
                update[0] != 0,
            )?;
        }
        Ok(())
    }
    /// Route a validated typed platform sample through shared input policy.
    /// Capture-time coordinates, axes, timestamps and flags remain unchanged.
    /// The host assigns engine sequence numbers, including inserted cancellations.
    pub fn pointer_event(&mut self, event: PenEvent, button: PointerButton) -> Result<(), String> {
        if !self.accepts_pointer_input(event.view_revision) {
            return Ok(());
        }
        self.pointer_event_inner(event, button, false)
    }
    pub fn suspend_renderer(&mut self) -> Result<(), String> {
        self.last_pen = None;
        self.deferred_contacts.clear();
        self.session.set_input_held(false);
        let previous = self.session.state().revision;
        let change = self.session.suspend_renderer()?;
        self.apply_change(previous, change);
        self.startup = Default::default();
        Ok(())
    }
    fn pointer_event_inner(
        &mut self,
        mut event: PenEvent,
        button: PointerButton,
        preserve_token: bool,
    ) -> Result<(), String> {
        let id = event.device_id;
        let phase = event.phase;
        let predicted = event.flags.0 & SampleFlags::PREDICTED.0 != 0;
        let touch = event.tool == ToolKind::Finger;
        let contact = match phase {
            PenPhase::Hover => None,
            PenPhase::Down => Some(ContactPhase::Down),
            PenPhase::Move => Some(ContactPhase::Move),
            PenPhase::Up => Some(ContactPhase::Up),
            PenPhase::Cancel => Some(ContactPhase::Cancel),
        };
        let paint = if predicted {
            self.last_pen.is_some_and(|p| p.device_id == id) && !touch
        } else if let Some(phase) = contact {
            self.input(UiInput::Pointer {
                id,
                phase,
                kind: if touch {
                    PointerKind::Touch
                } else if event.tool == ToolKind::Mouse {
                    PointerKind::Mouse
                } else {
                    PointerKind::Pen
                },
                button,
                position: [event.surface_position.x, event.surface_position.y],
                time_ns: event.timestamp_ns,
            })?
            .paint
        } else {
            false
        };
        if !preserve_token {
            event.sequence = self.sequence + 1;
        }
        if !touch && !predicted {
            self.session.cursor_input(if phase == PenPhase::Cancel {
                None
            } else {
                Some(event)
            });
            self.dirty = true;
        }
        if paint && self.session.engine().backend().0.is_some() {
            self.sequence += 1;
            self.admit(event)?;
        }
        Ok(())
    }
    /// Deliver a paint sample to the engine, or hold its contact until
    /// painting is ready and then deliver every sample it held.
    fn admit(&mut self, event: PenEvent) -> Result<(), String> {
        let admitted = self.deferred_contacts.admit(event, self.paint_ready(), std::time::Instant::now());
        self.session.set_input_held(self.deferred_contacts.holding());
        for event in admitted {
            self.deliver(event)?;
        }
        Ok(())
    }
    fn deliver(&mut self, event: PenEvent) -> Result<(), String> {
        self.enqueue(event)?;
        if event.flags.contains(SampleFlags::PREDICTED) || event.flags.contains(SampleFlags::CORRECTION) {
            return Ok(());
        }
        if event.phase == PenPhase::Down {
            self.paint_start_sequence = event.sequence;
        }
        self.last_pen = if matches!(event.phase, PenPhase::Up | PenPhase::Cancel) {
            None
        } else {
            Some(event)
        };
        Ok(())
    }
    fn paint_ready(&self) -> bool {
        let engine = self.session.engine();
        engine.backend().0.as_ref().is_some_and(|gpu| {
            self.startup.brush_ready
                && !gpu.startup_needs_update(
                    engine.document(),
                    engine.brush(),
                    engine.transform_preview().is_some(),
                )
        })
    }
    fn export_validation(&self, recipe: &layer_ui::ExportRecipe) -> Result<(), layer_ui::ColorFeatureError> {
        recipe.validate()?;
        let document = self.session.engine().document();
        recipe.output_extent(document.composition().size)?;
        Ok(())
    }
    pub fn query(&mut self, query: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum StrokeRecordingAction {
            Start,
            Stop,
            Saved,
        }
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Query {
            ToolbarStamp { context: layer_ui::ToolbarContext },
            ImageLayerDrop { target: u64, fraction: f32 },
            DocumentColor,
            Requests,
            ProofPanel { action: Option<layer_ui::proof_panel::ProofAction> },
            ProofForm,
            ProofCopy,
            ProofStatus,
            ExportForm,
            ExportValidate { recipe: layer_ui::ExportRecipe },
            ExportValidation { recipe: layer_ui::ExportRecipe },
            ExportDraft { recipe: layer_ui::ExportRecipe, action: layer_ui::ExportDraftAction },
            Header { request: header::HeaderRequest },
            Catalog,
            NativeCaption { caption: layer_ui::NativeCaption },
            DocumentDeliveryMessage { message: layer_ui::DocumentDeliveryMessage },
            ProfileEntriesCopy { entries: Vec<layer_ui::profile_library::ProfileEntry> },
            ProfileNameCopy { name: Option<String> },
            ExportProfileNameCopy { name: String },
            ColorFeatureErrorCopy { reason: layer_ui::ColorFeatureError, profile: Option<bool>, proof: Option<bool> },
            ExportPresetCopy { names: Vec<String> },
            ExportMetadataCopy { format: layer_ui::ExportFormat, keep: layer_ui::MetadataKeep },
            ExportProfileCaptionsCopy { captions: Vec<layer_ui::ExportProfileCaption> },
            ApplicationMenu {
                menu: layer_ui::ApplicationMenu,
            },
            ApplicationMenuHover {
                open: Option<layer_ui::ApplicationMenu>,
                hovered: layer_ui::ApplicationMenu,
            },
            ApplicationLink {
                link: layer_ui::ApplicationLink,
            },
            RendererStats,
            CommandReason { command: layer_ui::CommandId },
            FilterPreviews {
                filters: Vec<std::sync::Arc<str>>,
                size: [u32; 2],
                #[serde(default)]
                cache: layer_ui::FilterPreviewCache,
            },
            ActionTooltip {
                label: String,
                action: UiAction,
            },
            SelectionMenu {kind:layer_ui::SelectionMenu},
            CanvasBarLayout {
                measure: layer_ui::CanvasBarMeasure,
            },
            CanvasBarMenu {
                context: layer_ui::CanvasBarContext,
                shown: usize,
            },
            CanvasBarChoiceMenu {
                context: layer_ui::CanvasBarContext,
                id: String,
            },
            ZoomMenu,
            LayerMenu {
                id: u64,
                mask: bool,
            },
            LayerBlendMenu {
                id: u64,
            },
            PaletteMenu {
                target: layer_ui::PaletteMenuTarget,
            },
            SwatchSheet {
                query: String,
                current: layer_core::color::RgbColor,
            },
            StrokeRecording {
                #[serde(default)]
                action: Option<StrokeRecordingAction>,
            },
            RevealPanel {
                panel: layer_ui::Panel,
            },
            PaletteReorderPreview {
                palette: u64,
                id: u64,
                slot: usize,
            },
            PaletteAction {
                action: layer_ui::ColorLibraryAction,
                #[serde(default)]
                dry_run: bool,
            },
            LayerDrop {
                epoch: u64,
                id: u64,
                target: u64,
                fraction: f32,
                #[serde(default)]
                surface: layer_ui::LayerDropSurface,
            },
            LayerThumbnails {
                requests: Vec<(u64, u64)>,
            },
            Context {
                target: layer_ui::ContextTarget,
            },
            PanelHandleTarget {
                item: layer_ui::DockItem,
            },
            WorkspaceDragPreview {
                position: [f32; 2],
                tabs: Vec<layer_ui::TabHit>,
                item: layer_ui::DockItem,
                #[serde(default)]
                expansion: Option<layer_ui::PanelExpansion>,
            },
            Drop {
                position: [f32; 2],
                tabs: Vec<layer_ui::TabHit>,
                item: layer_ui::DockItem,
                #[serde(default)]
                expansion: Option<layer_ui::PanelExpansion>,
            },
            Drawer {
                column: Option<u32>,
                heights: Vec<f32>,
                progress: f32,
                from: Option<layer_ui::DrawerPlacement>,
                #[serde(default)]
                closing: bool,
            },
            DrawerToolbar {
                panel: layer_ui::Panel,
                width: f32,
                height: f32,
            },
            Navigator {
                viewport: [f32; 2],
            },
            Expansion {
                panel: layer_ui::Panel,
                heights: [f32; 2],
                progress: f32,
                #[serde(default)]
                from: Option<layer_ui::PanelExpansion>,
                #[serde(default)]
                closing: bool,
            },
        }
        let result = match serde_json::from_value(query).map_err(|e| e.to_string())? {
            Query::Header { request } => self.header_request(request),
            Query::Catalog => json!(self.catalog),
            Query::NativeCaption { caption } => json!(caption.message(self.session.localization())),
            Query::DocumentDeliveryMessage { message } => json!(message.message(self.session.localization())),
            Query::ProfileEntriesCopy { entries } => {
                if entries.len() > layer_ui::profile_library::PROFILE_LIBRARY_ENTRIES { return Err(layer_ui::ColorFeatureError::ProfileLibraryLimit.message(self.session.localization())); }
                json!(entries.into_iter().map(|mut entry| { entry.profile = None; entry.localized_view(self.session.localization()) }).collect::<Vec<_>>())
            },
            Query::ExportProfileNameCopy { name } => json!(layer_ui::ExportProfileCaption::for_name(name).message(self.session.localization())),
            Query::ProfileNameCopy { name } => json!(layer_ui::profile_library::profile_description_name(name, self.session.localization())),
            Query::ColorFeatureErrorCopy { reason, profile, proof } => json!(if proof == Some(true) { reason.proof_message(self.session.localization()) } else { match profile {
                Some(true) => reason.profile_message(self.session.localization()),
                Some(false) => reason.preset_message(self.session.localization()),
                None => reason.message(self.session.localization()),
            }}),
            Query::ExportPresetCopy { names } => {
                let mut view = layer_ui::ExportPresetView { names, index: None, recipe: None, changed: false };
                view.localize_names(self.session.engine().document().composition().color, self.session.localization());
                json!(view.names)
            },
            Query::ExportProfileCaptionsCopy { captions } => json!(captions.iter().map(|caption| caption.message(self.session.localization())).collect::<Vec<_>>()),
            Query::ExportMetadataCopy { format, keep } => json!(layer_ui::ExportMetadataView::localized_for(format, keep, self.session.localization())),
            Query::ToolbarStamp { context } => json!(self.session.toolbar_stamp(context)?),
            Query::ApplicationMenu { menu } => json!(self.session.application_menu(menu)),
            Query::ApplicationMenuHover { open, hovered } => json!(layer_ui::ApplicationMenu::switches_on_hover(open, hovered)),
            Query::ApplicationLink { link } => json!(link.url()),
            Query::ProofForm => layer_ui::proof_workflow::proof_form(&self.session),
            Query::ProofCopy => json!(layer_ui::proof_workflow::proof_copy(&self.session)),
            Query::ProofStatus => json!(self.proof.observe(&self.session)),
            Query::ProofPanel {action} => {
                if let Some(action)=action {
                    let previous=self.session.state().revision;
                    let change=layer_ui::proof_panel::apply(&mut self.session,action).map_err(|reason|reason.proof_message(self.session.localization()))?;
                    self.apply_change(previous,change);
                }
                layer_ui::color_management::proof_view(&self.session)
            },
            Query::DocumentColor => json!(self.session.engine().document().composition().color),
            Query::Requests => json!(self.session.state().requests),
            Query::ExportForm => json!(layer_ui::ExportForm::new_localized(self.session.engine().document(), self.session.localization())),
            Query::ExportDraft { recipe, action } => json!(recipe.draft_localized(action, self.session.localization())),
            Query::ExportValidate { recipe } => {
                self.export_validation(&recipe).map_err(|reason|reason.message(self.session.localization()))?;
                json!(recipe)
            }
            Query::ExportValidation { recipe } => json!(self.export_validation(&recipe).err()),
            Query::RendererStats => json!(self.session.renderer_stats()),
            Query::CommandReason { command } => json!(self.session.command_disabled_reason(command)),
            Query::FilterPreviews {
                filters,
                size,
                cache,
            } => {
                let update = self.poll_filter_previews(filters, size, cache)?;
                self.filter_preview_image = update.image;
                json!(update.status)
            }
            Query::ActionTooltip { label, action } => {
                let state = self.session.state();
                json!(
                    state
                        .settings
                        .action_tooltip_localized(&label, &action, state.platform, self.session.localization())
                )
            }
            Query::ImageLayerDrop { target, fraction } => json!({"position": self.session.image_layer_drop_hint(target, fraction)}),
            Query::SelectionMenu {kind} => json!(self.session.selection_menu(kind)),
            Query::CanvasBarLayout { measure } => json!(self.session.canvas_bar_layout(&measure)),
            Query::CanvasBarMenu { context, shown } => json!(self.session.canvas_bar_menu(context, shown)),
            Query::CanvasBarChoiceMenu { context, id } => json!(self.session.canvas_bar_choice_menu(context, &id)),
            Query::ZoomMenu => json!(self.session.zoom_menu()),
            Query::LayerMenu { id, mask } => json!(self.session.layer_menu(id, mask)?),
            Query::LayerBlendMenu { id } => json!(self.session.layer_blend_menu(id)?),
            Query::StrokeRecording { action } => {
                let platform = json!(self.session.state().platform);
                let mut recorder = self.session.stroke_recording();
                match action {
                    Some(StrokeRecordingAction::Start) => {
                        recorder.start(platform.as_str().unwrap_or_default())?
                    }
                    Some(StrokeRecordingAction::Stop) => {
                        recorder.stop(layer_engine::recording::StopReason::Manual)
                    }
                    Some(StrokeRecordingAction::Saved) => recorder.saved(),
                    None => {}
                }
                json!(recorder.status())
            }
            Query::PaletteMenu { target } => {
                json!(self.session.state().color_library.menu(target, self.session.localization())?)
            }
            Query::SwatchSheet { query, current } => {
                let state = self.session.state();
                let colors = state.display_colors();
                json!(layer_ui::SwatchSheetView::new(&state.color_library, &query, current, |color| self.swatch_preview(colors, color), self.session.localization()))
            }
            Query::RevealPanel { panel } => {
                let previous = self.session.state().revision;
                let change = self.session.reveal_panel(panel)?;
                self.apply_change(previous, change);
                json!(null)
            }
            Query::PaletteReorderPreview { palette, id, slot } => json!(
                self.session
                    .state()
                    .color_library
                    .preview_reorder(palette, id, slot)
            ),
            Query::PaletteAction { action, dry_run } => {
                let result = if dry_run {
                    self.session.state().color_library.check(action, self.session.localization())
                } else {
                    self.dispatch(UiAction::Color {
                        action: layer_ui::ColorAction::Library { action },
                    })
                };
                json!({"error": result.err()})
            }
            Query::LayerDrop {
                epoch,
                id,
                target,
                fraction,
                surface,
            } => {
                let current = self.session.state().document_file.epoch;
                let hint = (epoch == current)
                    .then(|| self.session.layer_drop_preview(id, target, fraction, surface))
                    .flatten();
                json!({ "epoch": current, "target": hint.map(|h| h.target), "position": hint.map(|h| h.position), "effect_owner": hint.and_then(|h| h.effect_owner) })
            }
            Query::LayerThumbnails { requests } => {
                let (accepted, images) = self.layer_thumbnails(requests)?;
                let images: Vec<_> = images
                    .into_iter()
                    .map(|image| (image.request_id, image.width, image.height, image.bytes))
                    .collect();
                json!({ "accepted": accepted, "images": images })
            }
            Query::Context { target } => json!(self.session.context_menu(target)?),
            Query::PanelHandleTarget { item } => {
                json!(
                    self.session
                        .state()
                        .workspace
                        .layout
                        .panel_handle_target(item)
                )
            }
            Query::WorkspaceDragPreview {
                position,
                tabs,
                item,
                expansion,
            } => {
                let drop = self.workspace_drop(position, &tabs, item, expansion);
                json!({"drop": drop, "tab": self.session.tab_drag_preview(position)})
            }
            Query::Drop {
                position,
                tabs,
                item,
                expansion,
            } => self.workspace_drop(position, &tabs, item, expansion),
            Query::Drawer {
                column,
                heights,
                progress,
                from,
                closing,
            } => {
                let state = self.session.state();
                let drawer = match column {
                    None => state.customization.drawer.as_ref(),
                    Some(id) => state.customization.column_drawers.iter().find(|d|
                        matches!(d.anchor, layer_ui::DrawerAnchor::Column { column, .. } if column == id)),
                };
                let end = if closing {
                    from.as_ref().map(|p| p.closed())
                } else {
                    drawer.and_then(|d| {
                        state.customization.drawer_placement(
                            d,
                            &state.workspace.layout,
                            self.logical,
                            &heights,
                        )
                    })
                };
                json!(end.map(|end| {
                    let from = from.unwrap_or_else(|| end.closed());
                    let placement = end.interpolate_from(&from, progress);
                    json!({"connection": placement.connection(), "placement": placement})
                }))
            }
            Query::DrawerToolbar {
                panel,
                width,
                height,
            } => {
                if ![width, height].into_iter().all(|v| v.is_finite() && v > 0.) {
                    return Err("Invalid drawer toolbar size".into());
                }
                let config = self.session.state().workspace.layout.panel(panel)?;
                let geometry = layer_ui::toolbar_tile_layout(
                    width,
                    layer_ui::toolbar_content_height(width, config.tiles(), config.tile_style),
                    layer_ui::Axis::Vertical,
                    config.tiles(),
                    false,
                    config.tile_style,
                );
                let content_height = geometry
                    .tiles
                    .iter()
                    .map(|b| b.y + b.height + 4.)
                    .fold(0., f32::max);
                let mut value = json!(geometry);
                value["content_height"] = json!(content_height);
                value
            }
            Query::Navigator { viewport } => {
                let state = self.session.state();
                let doc = self.session.engine().document();
                json!(layer_ui::NavigatorGeometry::new(
                    &state.camera,
                    [doc.composition().size[0], doc.composition().size[1]],
                    viewport
                ))
            }
            Query::Expansion {
                panel,
                heights,
                progress,
                from,
                closing,
            } => {
                let layout = &self.session.state().workspace.layout;
                let end = layout.expanded_panel(
                    self.logical,
                    panel,
                    heights,
                    if closing { 0.0 } else { 1.0 },
                );
                let from =
                    from.or_else(|| layout.expanded_panel(self.logical, panel, heights, 0.0));
                let resolved = self.session.layout(self.logical);
                end.zip(from).map_or(Value::Null, |(end, from)| {
                    let placement = end.interpolate_from(from, progress);
                    let mut value = json!(placement);
                    if panel.kind() == layer_ui::PanelKind::Tiles {
                        let config = layout.panel(panel).expect("expanded panel exists");
                        let group = resolved
                            .groups
                            .iter()
                            .find(|g| g.panels.contains(&panel))
                            .expect("expanded group exists");
                        value["tiles"] = json!(layer_ui::toolbar_tile_layout(
                            placement.preview.width,
                            (placement.preview.height - placement.configuration.y).max(0.0),
                            group.axis,
                            config.tiles(),
                            !group.tabs_visible,
                            group
                                .tiles
                                .as_ref()
                                .filter(|_| group.active == panel)
                                .map_or(config.tile_style, |t| t.presentation.tile_style),
                        ));
                    }
                    value
                })
            }
        };
        Ok(result)
    }

    fn workspace_drop(
        &self,
        position: [f32; 2],
        tabs: &[layer_ui::TabHit],
        item: layer_ui::DockItem,
        expansion: Option<layer_ui::PanelExpansion>,
    ) -> Value {
        self.session
            .drop_hint(self.logical, position, tabs, item, expansion)
            .map_or(Value::Null, |hint| {
                let action = item.move_action(hint.target.clone(), self.logical);
                let mut value = json!(hint);
                // Panel/group gestures commit through DragWorkspace, preserving
                // grab offsets and one history transaction. Tiles apply an action.
                if matches!(item, layer_ui::DockItem::Tile { .. }) {
                    value["action"] = json!(action);
                }
                value
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_restores_settings_before_gpu_or_view_publication() {
        let saved = r#"{"language":{"Explicit":"ja"},"theme":"dark"}"#;
        let mut host = NativeHost::launch(layer_ui::Platform::Android, saved, &["ko-KR", "en-GB"]).unwrap();
        assert!(host.session.renderer_mut().0.is_none());
        assert_eq!(host.session.localization().language(), layer_ui::UiLanguage::Japanese);
        assert_eq!(host.session.state().settings.theme, Some(layer_ui::Theme::Dark));
        assert_eq!(host.bootstrap_view().active_tag, "ja");
        assert_eq!(host.bootstrap_view().shipped_tags, layer_ui::UiLanguage::ALL.map(layer_ui::UiLanguage::tag));
        assert!(std::sync::Arc::ptr_eq(host.session.localization(), &layer_ui::launch_localization(saved, &["en"])));
    }

    #[test]
    fn prepared_bootstrap_context_is_adopted_without_changing_active_language() {
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let bootstrap = layer_ui::bootstrap_view(&localization);
        let host = NativeHost::launch_localized(layer_ui::Platform::Mac, r#"{"theme":"dark"}"#, localization.clone()).unwrap();
        assert!(std::sync::Arc::ptr_eq(host.session.localization(), &localization));
        assert_eq!(host.bootstrap_view().active_tag, bootstrap.active_tag);
        assert_eq!(host.session.state().settings.theme, Some(layer_ui::Theme::Dark));
    }

    #[test]
    fn hosts_keep_independent_launch_localization() {
        let japanese = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let korean = layer_ui::Localizer::shared(layer_ui::UiLanguage::Korean);
        let first = NativeHost::new_localized(layer_ui::Platform::Android, japanese.clone()).unwrap();
        let second = NativeHost::new_localized(layer_ui::Platform::Android, korean.clone()).unwrap();
        assert!(std::sync::Arc::ptr_eq(first.session.localization(), &japanese));
        assert!(std::sync::Arc::ptr_eq(second.session.localization(), &korean));
        assert!(!std::sync::Arc::ptr_eq(first.session.localization(), second.session.localization()));
    }

    #[test]
    fn typed_copy_queries_use_the_published_context_and_preserve_literal_names() {
        let name = "İı Tiếng Việt Tie\u{302}\u{301}ng Vie\u{323}\u{302}t ไทย 🎨 {draft} \"quoted\"";
        let mut host = NativeHost::new(layer_ui::Platform::Android).unwrap();
        for language in layer_ui::UiLanguage::ALL {
            let localization = layer_ui::Localizer::shared(language);
            host.set_localization(localization.clone());
            let catalog = host.query(json!({"type":"catalog"})).unwrap();
            assert_eq!(catalog["profile_copy"]["library_title"], localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_LIBRARY_TITLE).as_ref());
            assert_eq!(catalog["export_copy"]["title"], localization.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_TITLE).as_ref());
            assert_eq!(catalog["document_color_copy"]["correct_profile"], localization.text(layer_ui::MessageId::COLOR_FEATURES_COLOR_CORRECT_PROFILE).as_ref());
            assert_eq!(catalog["proof_copy"]["title"], localization.text(layer_ui::MessageId::COLOR_FEATURES_PROOF_TITLE).as_ref());
            assert_eq!(catalog["document_delivery_copy"]["separate_copy"], localization.text(layer_ui::MessageId::DOCUMENTS_ERROR_SEPARATE_COPY).as_ref());
            assert_eq!(catalog["document_delivery_copy"]["untitled"], localization.text(layer_ui::MessageId::DOCUMENTS_UNTITLED).as_ref());
            let caption = host.query(json!({"type":"native_caption","caption":{"type":"drawing_title","title":name,"width":12,"height":34}})).unwrap();
            assert_eq!(caption, layer_ui::NativeCaption::DrawingTitle { title:name.into(), width:12, height:34 }.message(&localization));
            let filename = host.query(json!({"type":"document_delivery_message","message":{"type":"converted_name","name":name}})).unwrap();
            assert_eq!(filename, layer_ui::DocumentDeliveryMessage::ConvertedName { name:name.into() }.message(&localization));
            assert!(filename.as_str().unwrap().contains(name));
            assert!(std::sync::Arc::ptr_eq(host.session.localization(), &localization));
            assert!(host.session.renderer_mut().0.is_none());
        }
    }

    #[test]
    fn retained_color_metadata_queries_follow_context_without_profile_buffers_or_literal_changes() {
        let literal = "İı ไทย Tie\u{302}\u{301}ng Vie\u{323}\u{302}t { $name } 🎨";
        let metadata = json!({"id":"0123456789abcdef","bytes":1234,"name":"","channels":"Rgb","issue":"ProfileChanged"});
        let mut host = NativeHost::new(layer_ui::Platform::Android).unwrap();
        for language in layer_ui::UiLanguage::ALL {
            let localization = layer_ui::Localizer::shared(language); host.set_localization(localization.clone());
            let views = host.query(json!({"type":"profile_entries_copy","entries":[metadata]})).unwrap(); let view = &views[0];
            assert_eq!(view["name"], localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).as_ref());
            assert_eq!(view["issue"], localization.text(layer_ui::MessageId::COLOR_PROFILE_CHANGED_STORAGE).as_ref());
            assert!(view["details"].as_str().unwrap().contains("1234")); assert!(view["details"].as_str().unwrap().contains("0123456789ab"));
            assert!(view.get("profile").is_none()); assert_eq!(metadata["name"], ""); assert_eq!(metadata["issue"], "ProfileChanged");
            for name in [Some(literal),Some("")] { assert_eq!(host.query(json!({"type":"profile_name_copy","name":name})).unwrap(), name.unwrap()); }
            assert_eq!(host.query(json!({"type":"profile_name_copy","name":null})).unwrap(), localization.text(layer_ui::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).as_ref());
            let names = host.query(json!({"type":"export_preset_copy","names":["","","","",literal]})).unwrap();
            assert_eq!(names[0], localization.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_WEB_SHARE).as_ref()); assert_eq!(names[4], literal);
            let refusal = host.query(json!({"type":"color_feature_error_copy","reason":"WebpLimit"})).unwrap();
            assert_eq!(refusal, localization.text(layer_ui::MessageId::COLOR_EXPORT_WEBP_LIMIT).as_ref());
            let metadata_view = host.query(json!({"type":"export_metadata_copy","format":"Exr","keep":"All"})).unwrap();
            assert_eq!(metadata_view["available"], false); assert_eq!(metadata_view["note"], localization.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_METADATA_EXR).as_ref());
            let mut invalid = layer_ui::ExportRecipe::web_share(); invalid.jpeg_quality = 0;
            let raw = host.query(json!({"type":"export_validation","recipe":invalid})).unwrap();
            assert_eq!(raw, "ExportQuality");
            assert_eq!(host.query(json!({"type":"color_feature_error_copy","reason":raw})).unwrap(), localization.text(layer_ui::MessageId::COLOR_EXPORT_QUALITY_RANGE).as_ref());
            assert!(host.query(json!({"type":"profile_entries_copy","entries":vec![metadata.clone();layer_ui::profile_library::PROFILE_LIBRARY_ENTRIES+1]})).is_err());
            assert!(host.session.renderer_mut().0.is_none());
        }
    }

    fn gpu_host(platform: layer_ui::Platform, size: [u32; 2]) -> (layer_render_wgpu::WgpuRasterizer, NativeHost) {
        gpu_document_host(platform, layer_ui::new_drawing(size[0], size[1], &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap())
    }

    fn gpu_document_host(platform: layer_ui::Platform, document: layer_core::Document) -> (layer_render_wgpu::WgpuRasterizer, NativeHost) {
        let reference = layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let gpu = GpuContext::of(&reference).rasterizer(Default::default(), &RendererOptions::default(), true).unwrap();
        let mut host = NativeHost::new(platform).unwrap();
        host.session = UiSession::from_project(Renderer(Some(gpu.into())), document, None, [640, 480], platform).unwrap();
        host.startup = Default::default();
        host.resize(640, 480, 1.).unwrap();
        (reference, host)
    }

    fn frame_step(host: &mut NativeHost, clock: &std::cell::Cell<u64>, deadline: std::time::Instant) {
        clock.set(clock.get() + 8_000_000);
        host.prepare_canvas_frame(clock.get(), clock.get(), true).unwrap();
        assert!(std::time::Instant::now() < deadline, "startup timed out");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    #[test]
    fn a_maskless_fill_thumbnail_is_admitted_and_tracks_color() {
        let (_reference, mut host) = gpu_host(layer_ui::Platform::Android, [64; 2]);
        let clock = std::cell::Cell::new(0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !host.startup.brush_ready { frame_step(&mut host, &clock, deadline); }
        let fill = layer_ui::occurrence_token(*host.session.engine().document().scene().constant_backdrop().first().unwrap());
        for action in [
            json!({"type":"select_layer","id":fill}),
            json!({"type":"effect","action":{"op":"set","layer":fill,"key":"color","value":{"kind":"color","value":{"space":"Srgb","rgba":[1.,0.,0.,1.]}}}}),
        ] { host.dispatch(serde_json::from_value(action).unwrap()).unwrap(); }
        frame_step(&mut host, &clock, deadline);
        assert!(host.session.background_readback_idle());
        loop {
            let (_, images) = host.layer_thumbnails([(1, fill)]).unwrap();
            if let Some(image) = images.iter().find(|image| image.request_id == 1) {
                assert!(image.bytes.as_chunks::<4>().0.iter().all(|pixel| *pixel == [255, 0, 0, 255]));
                break;
            }
            frame_step(&mut host, &clock, deadline);
        }
        host.dispatch(serde_json::from_value(json!({"type":"layer","action":{"op":"add_mask","id":fill,"replace":false}})).unwrap()).unwrap();
        let mask = host.session.state().layers.iter().find(|layer| layer.id == fill).unwrap().mask_id.unwrap();
        loop {
            let (_, images) = host.layer_thumbnails([(2, mask)]).unwrap();
            if let Some(image) = images.iter().find(|image| image.request_id == 2) {
                assert!(image.bytes.as_chunks::<4>().0.iter().all(|pixel| *pixel == [255; 4]));
                break;
            }
            frame_step(&mut host, &clock, deadline);
        }
    }
    #[test]
    fn the_first_eraser_stroke_on_a_new_image_layer_mask_waits_for_its_shaders_and_is_stored() {
        let mut document = layer_ui::new_drawing(640, 480, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let image = layer_core::color::source::rgba8_source([960, 720], |x, y| [(x % 256) as u8, (y % 256) as u8, 40, 255]);
        let mut object = layer_core::authored::ImageObject::new(image.into());
        object.affine = layer_core::Affine64([1., 0., 0., 1., -160., -120.]);
        let (layer, edit) = document.create_object_layer_edit("Photo", object, None, 0).unwrap();
        document.apply(edit).unwrap();
        let (_reference, mut host) = gpu_document_host(layer_ui::Platform::Android, document);
        let clock = std::cell::Cell::new(0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !host.startup.brush_ready { frame_step(&mut host, &clock, deadline); }
        host.dispatch(UiAction::Layer { action: layer_ui::LayerAction::AddMask { id: layer_ui::occurrence_token(layer), replace: false } }).unwrap();
        host.dispatch(UiAction::Invoke { command: layer_ui::CommandId::Eraser }).unwrap();
        assert!(!host.paint_ready(), "the first mask needs shaders the drawing did not use before");
        let surface = layer_core::Affine(host.session.state().camera.document_to_surface());
        let mut records = Vec::new();
        for (step, x) in [100., 160., 220.].into_iter().enumerate() {
            let at = surface.map(Point { x, y: 200. });
            records.extend([f64::from(at.x), f64::from(at.y), 1., 0., 0., 0., 0., (clock.get() + step as u64 * 1_000_000) as f64, step as f64 + 1.]);
        }
        pointer(&mut host, 7, 0, 0, &records, false).unwrap();
        assert!(!host.deferred_contacts.is_empty(), "the stroke waits instead of being dropped");
        while !host.deferred_contacts.is_empty() || host.session.engine().has_active_stroke() { frame_step(&mut host, &clock, deadline); }
        for _ in 0..4 { frame_step(&mut host, &clock, deadline); }
        let doc = host.session.engine().document();
        let mask = doc.scene().occurrence(layer).unwrap().mask.clone().unwrap().source;
        assert_eq!(doc.working.target, Some(layer_core::SourceTarget::Coverage(mask)));
        assert_eq!(host.session.engine().metrics().committed_strokes, 1);
        assert!(doc.artwork.coverage.get(mask).unwrap().raster.try_data().is_some_and(|data| data.is_ok_and(|data| !data.tiles.is_empty())), "the erased coverage is stored");
        assert!(host.session.engine().can_undo());
    }
    #[test]
    fn input_held_for_shaders_keeps_saving_waiting_until_it_is_delivered_or_cancelled() {
        let mut document = layer_ui::new_drawing(640, 480, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let mut object = layer_core::authored::ImageObject::new(layer_core::color::source::rgba8_source([200, 150], |_, _| [200, 40, 30, 255]).into());
        object.affine = layer_core::Affine64([1., 0., 0., 1., 100., 100.]);
        let (layer, edit) = document.create_object_layer_edit("Photo", object, None, 0).unwrap();
        document.apply(edit).unwrap();
        let image = document.scene().object_handle(layer).unwrap();
        let (_reference, mut host) = gpu_document_host(layer_ui::Platform::Android, document);
        let clock = std::cell::Cell::new(0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !host.startup.brush_ready { frame_step(&mut host, &clock, deadline); }
        let choose = UiAction::Layer { action: layer_ui::LayerAction::Select { id: layer_ui::occurrence_token(layer), mask: false } };
        host.dispatch(UiAction::Layer { action: layer_ui::LayerAction::AddMask { id: layer_ui::occurrence_token(layer), replace: false } }).unwrap();
        host.dispatch(choose).unwrap();
        host.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Tool { tool: layer_ui::LayerCanvasTool::Move } }).unwrap();
        assert!(!host.paint_ready(), "the first mask needs shaders the drawing did not use before");
        let surface = layer_core::Affine(host.session.state().camera.document_to_surface());
        let contact = |host: &mut NativeHost, id: u64, path: &[([f64; 2], f64)]| {
            let records: Vec<f64> = path.iter().enumerate().flat_map(|(step, ([x, y], phase))| {
                let at = surface.map(Point { x: *x as f32, y: *y as f32 });
                [f64::from(at.x), f64::from(at.y), 0.5, 0., 0., 0., 0., (clock.get() + step as u64 * 1_000_000) as f64, *phase]
            }).collect();
            pointer(host, id, 1, 0, &records, false).unwrap();
        };
        let saving = |host: &NativeHost| host.session.command_disabled_reason(layer_ui::CommandId::SaveDocumentAs).is_none();
        contact(&mut host, 6, &[([140., 140.], 1.)]);
        assert!(host.deferred_contacts.holding() && !saving(&host), "a held contact keeps saving waiting");
        contact(&mut host, 6, &[([140., 140.], 4.)]);
        assert!(!host.deferred_contacts.holding() && saving(&host), "a cancelled held contact releases saving");
        contact(&mut host, 7, &[([140., 140.], 1.), ([155., 150.], 2.), ([170., 160.], 3.)]);
        assert!(host.deferred_contacts.holding() && !saving(&host), "a later save waits for the held drag");
        while host.deferred_contacts.holding() { frame_step(&mut host, &clock, deadline); }
        assert!(saving(&host), "delivering the drag releases saving");
        for _ in 0..4 { frame_step(&mut host, &clock, deadline); }
        let [.., x, y] = host.session.engine().document().scene().object(image).unwrap().affine.0;
        assert!((x - 130.).abs() < 0.5 && (y - 120.).abs() < 0.5, "the held drag moved the image: {x}, {y}");
        assert!(host.paint_ready());
        contact(&mut host, 8, &[([500., 400.], 1.), ([500., 400.], 3.)]);
        assert!(!host.deferred_contacts.holding() && saving(&host), "a contact while painting is ready is never held");
    }
    #[test]
    fn thumbnails_in_flight_during_a_renderer_replacement_still_arrive() {
        let mut document = layer_ui::new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let image = layer_core::color::source::rgba8_source([64; 2], |_, _| [20, 200, 40, 255]);
        let (layer, edit) = document.create_object_layer_edit("Photo", layer_core::authored::ImageObject::new(image.into()), None, 0).unwrap();
        document.apply(edit).unwrap();
        let (reference, mut host) = gpu_document_host(layer_ui::Platform::Android, document);
        let clock = std::cell::Cell::new(0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !host.startup.brush_ready { frame_step(&mut host, &clock, deadline); }
        let target = layer_ui::occurrence_token(layer);
        let mut request = 40;
        let request = loop {
            request += 1;
            let (accepted, images) = host.layer_thumbnails([(request, target)]).unwrap();
            if accepted == [request] && images.iter().all(|image| image.request_id != request) { break request; }
            frame_step(&mut host, &clock, deadline);
        };
        let replacement = GpuContext::of(&reference).rasterizer(Default::default(), &RendererOptions::default(), true).unwrap();
        host.session.replace_renderer(Renderer(Some(replacement.into()))).unwrap();
        host.startup = Default::default();
        while !(host.startup.canvas_ready && host.session.background_readback_idle()
            && host.session.engine().backend().0.as_ref().is_some_and(|gpu| gpu.ui_readback_ready())) { frame_step(&mut host, &clock, deadline); }
        host.session.renderer_mut().prepare_moving_layer(Some(layer));
        let (accepted, images) = host.layer_thumbnails(std::iter::empty()).unwrap();
        assert!(accepted.is_empty() && images.is_empty(), "a renderer without a drawn frame cannot preview the image yet");
        host.session.renderer_mut().prepare_moving_layer(None);
        let image = loop {
            frame_step(&mut host, &clock, deadline);
            let (accepted, images) = host.layer_thumbnails(std::iter::empty()).unwrap();
            assert!(accepted.is_empty(), "the resubmitted request is not reported as a new acceptance");
            if let Some(image) = images.into_iter().find(|image| image.request_id == request) { break image; }
        };
        assert_eq!(image.bytes.as_chunks::<4>().0[16 * 32 + 16], [20, 200, 40, 255]);
    }
    #[test]
    fn thumbnail_work_left_for_canvas_frames_wakes_the_host() {
        let mut document = layer_ui::new_drawing(2048, 1024, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let image = layer_core::color::source::rgba8_source([1024; 2], |x, y| [x as u8, y as u8, 90, 255]);
        let (layer, edit) = document.create_object_layer_edit("Photo", layer_core::authored::ImageObject::new(image.into()), None, 0).unwrap();
        document.apply(edit).unwrap();
        document.artwork.occurrences.get_mut(layer).unwrap().visible = false;
        let (_reference, mut host) = gpu_document_host(layer_ui::Platform::Android, document);
        let clock = std::cell::Cell::new(0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !host.startup.brush_ready || host.session.engine().wants_continuous_frames() { frame_step(&mut host, &clock, deadline); }
        let mut woke = false;
        loop {
            host.dirty = false;
            let (_, images) = host.layer_thumbnails([(1, layer_ui::occurrence_token(layer))]).unwrap();
            woke |= host.dirty;
            assert!(host.dirty || !host.session.engine().wants_continuous_frames(), "work left for frames schedules one");
            if images.iter().any(|image| image.request_id == 1) { break; }
            while host.dirty { frame_step(&mut host, &clock, deadline); }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        assert!(woke, "the thumbnail leaves decoding for canvas frames");
    }
    fn thumbnail(host: &mut NativeHost, clock: &std::cell::Cell<u64>, request: u64, target: u64) -> Vec<u8> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            while host.session.engine().wants_continuous_frames() || host.session.engine().has_pending_document_edits() { frame_step(host, clock, deadline); }
            let (_, images) = host.layer_thumbnails([(request, target)]).unwrap();
            if let Some(image) = images.into_iter().find(|image| image.request_id == request) { return image.bytes; }
            frame_step(host, clock, deadline);
        }
    }
    #[test]
    fn a_layer_thumbnail_returns_the_same_bytes_after_a_stroke_is_undone() {
        let (_reference, mut host) = gpu_host(layer_ui::Platform::Windows, [2048, 1536]);
        let clock = std::cell::Cell::new(0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !host.startup.brush_ready || host.session.engine().wants_continuous_frames() { frame_step(&mut host, &clock, deadline); }
        let layer = layer_ui::occurrence_token(host.session.engine().document().working.occurrence.unwrap());
        let original = thumbnail(&mut host, &clock, 1, layer);
        for attempt in 0..4u64 {
            let start = clock.get();
            let records: Vec<f64> = (0..42u64).flat_map(|i| {
                let phase = if i == 0 { 1. } else if i == 41 { 3. } else { 2. };
                [256. + i as f64 * 5., 240. + 24. * (i as f64 / 6.).sin(), 1., 0., 0., 0., 0., (start + i * 1_000_000) as f64, phase]
            }).collect();
            pointer(&mut host, 77, 1, 0, &records, false).unwrap();
            let painted = thumbnail(&mut host, &clock, 10 + attempt * 2, layer);
            assert_ne!(painted, original, "the stroke changes the thumbnail");
            host.dispatch(UiAction::Invoke { command: layer_ui::CommandId::Undo }).unwrap();
            let undone = thumbnail(&mut host, &clock, 11 + attempt * 2, layer);
            let differing: Vec<_> = original.iter().zip(&undone).enumerate().filter(|(_, (a, b))| a != b).take(8).collect();
            assert!(differing.is_empty(), "attempt {attempt}: Undo restores the thumbnail bytes: {differing:?}");
        }
    }
    fn pointer(host: &mut NativeHost, id: u64, tool: u8, button: u8, records: &[f64], predicted: bool) -> Result<(), String> {
        host.pointer_batch(PointerBatch {
            id, tool, button, records, predicted,
            view_revision: host.session.state().camera.revision,
            barrel_twist: false,
        })
    }

    #[test]
    fn command_reason_queries_current_input_while_published_commands_stay_stable() {
        let mut host = NativeHost::new(layer_ui::Platform::Android).unwrap();
        host.session.renderer_mut().0 = Some(layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).unwrap().into());
        let query = json!({"type": "command_reason", "command": "add_layer"});
        assert!(host.query(query.clone()).unwrap().is_null());
        let commands = host.session.state().commands.clone();
        let mut event = PenEvent {
            device_id: 1, sequence: 1, timestamp_ns: 1,
            view_revision: host.session.state().camera.revision,
            surface_position: layer_core::Point { x: 50., y: 50. }, pressure: 1.,
            tilt_radians: [0.; 2], twist_radians: 0., distance: 0.,
            phase: PenPhase::Down, tool: ToolKind::Pen, flags: SampleFlags::PRIMARY,
        };
        host.session.pen(event).unwrap();
        assert_eq!(host.session.state().commands, commands);
        assert_eq!(host.query(query.clone()).unwrap(), json!("Finish the canvas interaction first"));
        event.phase = PenPhase::Up; event.sequence = 2; event.timestamp_ns = 2; event.pressure = 0.;
        host.session.pen(event).unwrap();
        assert_eq!(host.query(query.clone()).unwrap(), json!("Finish the canvas interaction first"));
        host.session.frame(3, 3).unwrap();
        assert!(host.query(query).unwrap().is_null());
    }
    #[test]
    fn layer_drop_query_preserves_surface_normalized_target_and_epoch() {
        use layer_ui::{EffectAction,LayerAction,Platform};
        let mut host=NativeHost::new(Platform::Android).unwrap();
        host.session=UiSession::new(Renderer::default(),layer_ui::new_drawing(32,32,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),[1,1],Platform::Android).unwrap();
        let rows=&host.session.state().layers;let ink=rows[0].id;let paper=rows[1].id;
        host.dispatch(UiAction::Layer{action:LayerAction::New{group:false,clipped:false}}).unwrap();
        let id=host.session.state().layers.iter().find(|row|row.editing).unwrap().id;
        let epoch=host.session.state().document_file.epoch;let revision=host.session.engine().document().revision;
        for surface in [None,Some("row")] {
            let mut query=json!({"type":"layer_drop","epoch":epoch,"id":id,"target":ink,"fraction":1.});
            if let Some(surface)=surface{query["surface"]=json!(surface);}
            assert_eq!(host.query(query).unwrap(),json!({"epoch":epoch,"target":paper,"position":"above","effect_owner":null}));
        }
        assert_eq!(host.session.engine().document().revision,revision);
        host.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
        let effect=host.session.state().layers.iter().find(|row|row.editing).unwrap().id;
        let revision=host.session.engine().document().revision;
        let query=json!({"type":"layer_drop","epoch":epoch,"id":effect,"target":ink,"fraction":0.5,"surface":"thumbnail"});
        assert_eq!(host.query(query.clone()).unwrap(),json!({"epoch":epoch,"target":ink,"position":"attach","effect_owner":ink}));
        let mut stale=query.clone();stale["epoch"]=json!(epoch+1);
        assert_eq!(host.query(stale).unwrap(),json!({"epoch":epoch,"target":null,"position":null,"effect_owner":null}));
        let mut invalid=query;invalid["surface"]=json!("unknown");assert!(host.query(invalid).is_err());
        assert_eq!(host.session.engine().document().revision,revision);
        host.dispatch(UiAction::Layer{action:LayerAction::Clip{id,value:true}}).unwrap();
        let revision=host.session.engine().document().revision;
        for (target,fraction) in [(id,1.),(ink,0.)] {
            assert_eq!(host.query(json!({"type":"layer_drop","epoch":epoch,"id":effect,"target":target,"fraction":fraction,"surface":"row"})).unwrap(),json!({"epoch":epoch,"target":ink,"position":"above","effect_owner":ink}));
        }
        assert_eq!(host.session.engine().document().revision,revision);
    }

    #[test]
    fn object_layer_menu_query_keeps_layer_context_and_selection() {
        use layer_core::color::source::{SourceBuilder,SourceChannels,SourceInterpretation};
        let localizer=layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        let mut document=layer_ui::new_drawing(32,32,&localizer).unwrap();
        let mut builder=SourceBuilder::new([4;2],SourceInterpretation{channels:SourceChannels::Rgba,depth:layer_core::color::SampleDepth::U8,profile:Default::default(),profile_assumed:false},1<<20).unwrap();
        for _ in 0..4 { builder.push_row(&[255;16]).unwrap(); }
        let image=layer_core::authored::Image::new(std::sync::Arc::new(builder.finish().unwrap()));
        let (layer,edit)=document.create_object_layer_edit("Photo",layer_core::ImageObject::new(image),None,0).unwrap();document.apply(edit).unwrap();
        let object=document.scene().object_handle(layer).unwrap();
        let reference=layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let gpu=GpuContext::of(&reference).rasterizer(Default::default(),&RendererOptions::default(),true).unwrap();
        let mut host=NativeHost::new(layer_ui::Platform::Android).unwrap();
        host.session=UiSession::from_project(Renderer(Some(gpu.into())),document,None,[640,480],layer_ui::Platform::Android).unwrap();
        let id=layer_ui::occurrence_token(layer);
        let before=host.session.engine().document().working.clone();
        let menu=host.query(json!({"type":"layer_menu","id":id,"mask":false})).unwrap();
        assert_eq!(menu,json!(host.session.layer_menu(id,false).unwrap()));
        assert!(menu["sections"].as_array().is_some_and(|sections|!sections.is_empty()));
        assert_eq!(host.session.engine().document().working,before,"querying a menu does not change selection");
        host.dispatch(UiAction::Layer{action:layer_ui::LayerAction::Context{id,mask:false}}).unwrap();
        let working=&host.session.engine().document().working;
        assert_eq!(working.occurrence,Some(layer));
        assert_eq!(working.layer_selection,[layer].into());
        assert_eq!(host.session.engine().document().selected_objects(),[object].into());
        assert!(host.query(json!({"type":"layer_menu","id":layer_ui::object_token(object),"mask":false})).is_err());
    }

    #[test]
    fn requests_query_does_not_consume_publications() {
        let mut host = NativeHost::new(layer_ui::Platform::Android).unwrap();
        host.take_model_update_bytes().unwrap();
        host.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::ImportImage,
        })
        .unwrap();
        let query = json!({"type": "requests"});
        let requests = host.query(query.clone()).unwrap();
        assert_eq!(host.query(query).unwrap(), requests);
        assert!(
            requests
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["kind"]["type"] == "document")
        );
        let update: Value =
            serde_json::from_slice(&host.take_model_update_bytes().unwrap().unwrap()).unwrap();
        assert!(update.to_string().contains("requests"), "{update}");
    }
    #[test]
    fn application_menus_follow_actions_without_entering_camera_patches() {
        use layer_ui::{ApplicationMenu, CommandId, Platform};
        for platform in [Platform::Ios, Platform::Mac] {
            let mut host = NativeHost::new(platform).unwrap();
            host.resize(1200, 900, 1.).unwrap();
            let menus = host.take_value().unwrap()["application_menus"].clone();
            assert_eq!(menus.as_array().unwrap().len(), ApplicationMenu::ALL.len());
            for (id, menu) in ApplicationMenu::ALL
                .into_iter()
                .zip(menus.as_array().unwrap())
            {
                let expected =
                    json!({"id":id,"label":id.canonical_label(),"model":host.session.application_menu(id)});
                assert_eq!(
                    host.query(json!({"type":"application_menu","menu":id}))
                        .unwrap(),
                    expected["model"]
                );
                assert_eq!(*menu, expected);
            }
            let help = menus
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == "help")
                .unwrap();
            assert_eq!(
                help["model"]["sections"][1][0]["action"]["command"],
                "website"
            );
            assert_eq!(help["model"]["sections"][1][0]["enabled"], true);
            assert!(host.take_value().is_none());
            host.dispatch(UiAction::Invoke {
                command: CommandId::SelectAll,
            })
            .unwrap();
            let next = host.take_value().unwrap();
            let select = next["application_menus"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == "select")
                .unwrap();
            assert!(
                select["model"]["sections"][0][1]["enabled"]
                    .as_bool()
                    .unwrap()
            );
            host.dispatch(UiAction::Invoke { command: CommandId::ZoomOut }).unwrap();
            let first_navigation = host.take_value().unwrap();
            assert!(first_navigation["state"]["camera"].is_object());
            assert!(first_navigation.get("application_menus").is_some());
            assert!(host.session.command(CommandId::PreviousView).enabled);
            host.dispatch(UiAction::Invoke { command: CommandId::ZoomIn }).unwrap();
            let camera = host.take_value().unwrap();
            assert!(camera.get("camera").is_some());
            assert!(
                camera.get("application_menus").is_none(),
                "No menu rebuild at camera input rate"
            );
            for link in [
                layer_ui::ApplicationLink::Website,
                layer_ui::ApplicationLink::SourceCode,
            ] {
                assert_eq!(
                    host.query(json!({"type":"application_link", "link":link}))
                        .unwrap(),
                    link.url()
                );
            }
        }
    }
    #[test]
    fn drawer_queries_measure_and_close_after_the_model_is_removed() {
        use layer_ui::{CustomizationAction, Panel, ToolbarControl};
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.resize(1600, 1000, 1.).unwrap();
        let tile = host.session.state().workspace.layout.panel(Panel::Toolbar).unwrap().tiles()
            .iter().find(|t| t.control == ToolbarControl::Color).unwrap().id;
        host.dispatch(UiAction::ActivateTile { panel: Panel::Toolbar, tile }).unwrap();
        assert!(host.query(json!({"type":"drawer","heights":[],"progress":1})).unwrap().is_null());
        let open = host.query(json!({"type":"drawer","heights":[360],"progress":1})).unwrap();
        let placement = open["placement"].clone();
        assert_eq!(placement["bounds"]["width"], 280.);
        assert_eq!(placement["bounds"]["height"], 360.);
        assert!(open["connection"].is_object());
        let revision = host.session.engine().document().revision;
        host.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded }).unwrap();
        assert!(host.session.state().customization.drawer.is_none());
        for progress in [0., 1.] {
            let closed = host.query(json!({"type":"drawer","heights":[360],"progress":progress,"from":placement,"closing":true})).unwrap();
            if progress == 0. { assert_eq!(closed["placement"]["bounds"], placement["bounds"]); }
            else { assert_eq!(closed["placement"]["bounds"]["height"], 0.); }
        }
        assert_eq!(host.session.engine().document().revision, revision);
    }

    #[test]
    fn expansion_retains_presented_geometry_on_resize_and_close() {
        use layer_ui::{CustomizationAction, Panel};
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.resize(1600, 1000, 1.).unwrap();
        let revision = host.session.engine().document().revision;
        for panel in [Panel::Sizes, Panel::Toolbar] {
            host.dispatch(UiAction::Customize { action: CustomizationAction::ShowAllControls { panel } }).unwrap();
            let placement = host.query(json!({"type":"expansion","panel":panel,"heights":[0,420],"progress":1})).unwrap();
            assert_eq!(placement["configuration"]["width"], 380.);
            if panel == Panel::Toolbar {
                let layout = host.session.layout([1600., 1000.]);
                let group = layout.groups.iter().find(|g| g.panels.contains(&panel)).unwrap();
                let config = host.session.state().workspace.layout.panel(panel).unwrap();
                let expected = layer_ui::toolbar_tile_layout(placement["preview"]["width"].as_f64().unwrap() as f32,
                    (placement["preview"]["height"].as_f64().unwrap() - placement["configuration"]["y"].as_f64().unwrap()) as f32,
                    group.axis, config.tiles(), !group.tabs_visible, config.tile_style);
                assert_eq!(placement["tiles"], json!(expected));
            }
            host.resize(1000, 700, 1.).unwrap();
            let resized = host.query(json!({"type":"expansion","panel":panel,"heights":[0,420],"from":placement,"progress":0})).unwrap();
            assert_eq!(resized["bounds"], placement["bounds"]);
            assert_eq!(resized["preview"], placement["preview"]);
            host.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded }).unwrap();
            assert!(host.session.state().customization.expanded.is_none());
            for progress in [0., 1.] {
                let result = host.query(json!({"type":"expansion","panel":panel,"heights":[0,420],"from":placement,"progress":progress,"closing":true})).unwrap();
                if progress == 0. { assert_eq!(result["bounds"], placement["bounds"]); }
                else {
                    let layout = host.session.layout([1000., 700.]);
                    assert_eq!(result["bounds"], json!(layout.groups.iter().find(|g| g.panels.contains(&panel)).unwrap().bounds));
                }
            }
            host.resize(1600, 1000, 1.).unwrap();
        }
        assert_eq!(host.session.engine().document().revision, revision);
    }

    #[test]
    fn android_drawer_queries_follow_collapsed_toolbar_measurements() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2880, 1800, 1.75).unwrap();
        let group = app
            .session
            .state()
            .workspace
            .layout
            .panel_group(layer_ui::Panel::Brushes)
            .unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"move_panel", "panel":"toolbar", "target":{"kind":"tab","group":group}, "viewport":app.logical})).unwrap()).unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"customize", "action":{"type":"set_column_collapsed","group":group,"collapsed":true}})).unwrap()).unwrap();
        let column = app
            .session
            .state()
            .workspace
            .layout
            .collapsed_column_for_group(group)
            .unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"customize", "action":{"type":"set_column_drawers","column":column,"drawers":true}})).unwrap()).unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"customize", "action":{"type":"toggle_column_drawer","group":group,"panel":"toolbar"}})).unwrap()).unwrap();
        let tile = app
            .session
            .state()
            .workspace
            .layout
            .panel(layer_ui::Panel::Toolbar)
            .unwrap()
            .tiles()[0]
            .id;
        app.dispatch(serde_json::from_value(json!({"type":"measure_drawer_tiles", "measurements":[{"column":column,"anchor":{"panel":"toolbar","tile":tile},"bounds":{"x":80.,"y":100.,"width":36.,"height":36.}}]})).unwrap()).unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"customize", "action":{"type":"toggle_tool_drawer","anchor":{"panel":"toolbar","tile":tile}}})).unwrap()).unwrap();
        let query = json!({"type":"drawer","heights":[900.,200.],"progress":1.});
        let first = app.query(query.clone()).unwrap();
        assert_eq!(first["placement"]["anchor"]["y"], 100.);
        assert!(first["connection"].is_object());
        app.dispatch(serde_json::from_value(json!({"type":"measure_drawer_tiles", "measurements":[{"column":column,"anchor":{"panel":"toolbar","tile":tile},"bounds":{"x":80.,"y":60.,"width":36.,"height":20.}}]})).unwrap()).unwrap();
        assert_eq!(
            app.query(query.clone()).unwrap()["placement"]["anchor"]["height"],
            20.
        );
        app.dispatch(
            serde_json::from_value(json!({"type":"measure_drawer_tiles", "measurements":[]}))
                .unwrap(),
        )
        .unwrap();
        assert!(
            app.query(query).unwrap().is_null(),
            "A clipped-out tile has no child drawer"
        );
    }

    #[test]
    fn service_observation_survives_native_snapshot_publication() {
        let mut app = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        app.take_service_changes();
        app.dispatch(UiAction::SetBrushSize { value: 42. }).unwrap();
        let revision = app.session.state().revision;
        app.take_update_bytes().unwrap();
        let changes = app.take_service_changes();
        assert_ne!(changes & layer_ui::regions::BRUSH, 0);
        assert_eq!(app.take_service_changes(), 0);
        assert_eq!(app.session.state().revision, revision);
        app.dispatch(UiAction::SetBrushSize { value: 63. }).unwrap();
        assert_ne!(app.take_service_changes() & layer_ui::regions::BRUSH, 0);
    }

    #[test]
    fn native_navigation_preserves_camera_patches_and_rejects_nonfinite_input() {
        let mut app = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        app.resize(2400, 1800, 2.0).unwrap();
        app.take_value().unwrap();
        let anchor = [1200., 900.];
        let before = app.session.state().camera.clone();
        app.scroll(anchor, [30., -20.], 2., false, false).unwrap();
        let patch = app.take_value().unwrap();
        assert!(
            patch.get("state").is_none(),
            "Wheel pan must not rebuild all editor models"
        );
        assert_ne!(patch["camera"], json!(before));
        let zoom = app.session.state().camera.zoom;
        app.gesture(anchor, 1.5, 0.2, true).unwrap();
        assert!((app.session.state().camera.zoom - zoom * 1.5).abs() < 0.0001);
        assert!(app.take_value().unwrap().get("state").is_none());
        let camera = json!(app.session.state().camera);
        assert!(
            app.scroll(anchor, [f32::NAN, 0.], 2., false, false)
                .is_err()
        );
        assert!(app.gesture(anchor, 0., 0., true).is_err());
        assert_eq!(json!(app.session.state().camera), camera);
        app.dispatch(UiAction::SetBrushSize { value: 42. }).unwrap();
        app.scroll(anchor, [0., 1.], 2., false, false).unwrap();
        assert_eq!(
            app.take_value().unwrap()["state"]["brush"]["diameter"],
            42.
        );
    }
    #[test]
    fn filter_preview_geometry_before_gpu_attachment_is_optional() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        let response = app
            .query(json!({"type":"filter_previews",
            "filters":["curves"], "size":[240,40]}))
            .unwrap();
        assert_eq!(response["pending"], false);
        assert_eq!(response["requests"], 0);
        assert!(response["error"].is_null());
        assert!(app.take_filter_preview_image().is_none());
        assert!(app.session.renderer_mut().0.is_none());
    }
    #[test]
    fn workspace_views_expose_shared_actions_and_transient_measurements() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        app.dispatch(UiAction::MeasurePanels {
            measurements: vec![layer_ui::PanelMeasurement {
                panel: layer_ui::Panel::Brushes,
                tab_width: 76.0,
                content_height: 480.0,
                scroll: None,
            }],
        })
        .unwrap();
        let snapshot = app.snapshot();
        assert_eq!(snapshot["panel_measurements"][0]["tab_width"], 76.0);
        assert!(
            snapshot["state"]["workspace"]["layout"]
                .get("measurements")
                .is_none()
        );
        assert_eq!(
            snapshot["workspace_menu"]["sections"][0][0]["action"]["type"],
            "invoke"
        );
        app.dispatch(
            serde_json::from_value(json!({
                "type": "move_panel", "panel": "toolbar", "viewport": [1280, 800],
                "target": {"kind": "float", "position": [500, 300]}
            }))
            .unwrap(),
        )
        .unwrap();
        let target = app.query(json!({"type": "panel_handle_target", "item": {"kind": "panel", "panel": "toolbar"}})).unwrap();
        assert!(target.is_u64());
        app.dispatch(serde_json::from_value(json!({"type": "customize", "action": {"type": "duplicate_toolbar", "panel": "toolbar"}})).unwrap()).unwrap();
        assert_eq!(
            app.snapshot()["toolbar_prompt"]["title"],
            "Duplicate Toolbar"
        );
    }

    #[test]
    fn palette_queries_preview_validate_and_apply_through_the_session() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        let view = app.snapshot()["palette_panel"].clone();
        let palette = view["palette"].as_u64().unwrap();
        let ids: Vec<u64> = view["swatches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_u64().unwrap())
            .collect();
        assert!(ids.len() > 3);
        let preview = app
            .query(json!({"type":"palette_reorder_preview","palette":palette,"id":ids[0],"slot":2}))
            .unwrap();
        assert_eq!(preview["order"][2], ids[0]);
        assert_eq!(
            app.snapshot()["palette_panel"]["swatches"][0]["id"],
            ids[0],
            "previews never edit"
        );
        let action = preview["action"].clone();
        let name = view["swatches"][1]["name"].clone();
        let duplicate = json!({"op":"rename","id":ids[0],"name":name});
        for dry_run in [true, false] {
            let result = app
                .query(json!({"type":"palette_action","action":duplicate,"dry_run":dry_run}))
                .unwrap();
            assert!(result["error"].is_string());
        }
        assert!(
            app.query(json!({"type":"palette_action","action":action,"dry_run":true}))
                .unwrap()["error"]
                .is_null()
        );
        assert_eq!(
            app.snapshot()["palette_panel"]["swatches"][0]["id"],
            ids[0],
            "dry runs never edit"
        );
        assert!(
            app.query(json!({"type":"palette_action","action":action}))
                .unwrap()["error"]
                .is_null()
        );
        let view = app.snapshot()["palette_panel"].clone();
        assert_eq!(view["swatches"][2]["id"], ids[0]);
        assert_eq!(view["can_undo"], true);
        let menu = app
            .query(json!({"type":"palette_menu","target":{"kind":"color","id":ids[0]}}))
            .unwrap();
        assert_eq!(menu[1][0]["enabled"], true);
        let panel = layer_ui::Panel::Palettes;
        assert_ne!(
            app.session.state().workspace.layout.active_panel(panel),
            Some(panel)
        );
        app.query(json!({"type":"reveal_panel","panel":"palettes"}))
            .unwrap();
        assert_eq!(
            app.session.state().workspace.layout.active_panel(panel),
            Some(panel)
        );
    }

    #[test]
    fn ui_is_available_without_a_gpu() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        app.dispatch(UiAction::OpenSettings {
            page: layer_ui::SettingsPage::Input,
        })
        .unwrap();
        let snapshot = app.snapshot();
        assert_eq!(snapshot["state"]["platform"], "android");
        assert_eq!(snapshot["gpu_ready"], false);
        assert_eq!(snapshot["preferences"]["page"], "input");
        assert!(!snapshot["layout"]["groups"].as_array().unwrap().is_empty());
        assert_eq!(
            app.query(json!({"type":"catalog"})).unwrap()["app_name"],
            "Capy Canvas"
        );
    }
    #[test]
    fn navigation_cursor_snapshots_follow_tool_commands_without_pointer_input() {
        use layer_ui::{CommandId, Modifiers, Platform};
        for platform in [Platform::Mac, Platform::Ios, Platform::Android, Platform::Windows] {
            let mut host = NativeHost::new(platform).unwrap();
            host.resize(1200, 900, 1.).unwrap();
            host.take_value().unwrap();
            for (command, cursor) in [(CommandId::Hand, Some("pan")), (CommandId::Zoom, Some("zoom")),
                (CommandId::RotateView, Some("rotate")), (CommandId::Brush, None)]
            {
                host.dispatch(UiAction::Invoke { command }).unwrap();
                assert!(host.session.command(command).selected, "{platform:?} {command:?}");
                let snapshot = host.take_value().unwrap();
                assert_eq!(snapshot["navigation_cursor"], json!(cursor), "{platform:?} {command:?}");
                assert_eq!(snapshot["pan_cursor"], cursor == Some("pan"), "{platform:?} {command:?}");
                assert!(host.take_value().is_none(), "{platform:?} {command:?}");
            }
            host.dispatch(UiAction::Invoke { command: CommandId::Zoom }).unwrap();
            assert_eq!(host.take_value().unwrap()["navigation_cursor"], "zoom");
            let alt_key = |pressed| UiInput::Key { key: "Alt_L".into(), pressed, repeat: false,
                modifiers: Modifiers { alt: !pressed, ..Modifiers::default() }, editing: false, divider: None };
            for (input, cursor) in [(alt_key(true), "zoom_out"), (alt_key(false), "zoom"),
                (alt_key(true), "zoom_out"), (UiInput::Blur, "zoom")]
            {
                host.input(input).unwrap();
                let snapshot = host.take_value().expect("changed cursor must publish without a frame");
                assert!(snapshot["state"].is_object(), "the native owner must receive the cursor with a full publication");
                assert_eq!(snapshot["navigation_cursor"], cursor, "{platform:?}");
                assert_eq!(snapshot["pan_cursor"], false, "{platform:?}");
                assert!(host.take_value().is_none(), "{platform:?} {cursor}");
            }
        }
    }

    #[test]
    fn navigation_cursor_snapshots_follow_the_adopted_document_without_input() {
        use layer_ui::{CommandId, Modifiers, Platform};
        for platform in [Platform::Mac, Platform::Ios, Platform::Android, Platform::Windows] {
            for (command, cursor) in [(CommandId::Hand, Some("pan")), (CommandId::Zoom, Some("zoom")),
                (CommandId::RotateView, Some("rotate")), (CommandId::Brush, None)]
            {
                let mut host = NativeHost::new(platform).unwrap();
                host.resize(1200, 900, 1.).unwrap();
                host.take_value().unwrap();
                let reply = host.input(UiInput::Key { key: " ".into(), pressed: true, repeat: false,
                    modifiers: Modifiers::default(), editing: false, divider: None }).unwrap();
                assert_eq!(reply.navigation_cursor, Some(layer_ui::NavigationMode::Pan));
                assert_eq!(host.take_value().unwrap()["navigation_cursor"], "pan");
                let mut adopted = NativeHost::new(platform).unwrap();
                adopted.resize(1200, 900, 1.).unwrap();
                adopted.dispatch(UiAction::Invoke { command }).unwrap();
                assert!(adopted.session.command(command).selected, "{platform:?} {command:?}");
                host.session = adopted.session;
                host.document_adopted();
                let snapshot = host.take_value().unwrap();
                assert_eq!(snapshot["navigation_cursor"], json!(cursor), "{platform:?} {command:?}");
                assert_eq!(snapshot["pan_cursor"], cursor == Some("pan"), "{platform:?} {command:?}");
                assert!(host.take_value().is_none(), "{platform:?} {command:?}");
            }
        }
    }

    #[test]
    fn snapshots_skip_unchanged_input_but_publish_state_and_chrome() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        assert!(app.take_value().is_some());
        for i in 0..1000 {
            pointer(&mut app,
                1,
                0,
                0,
                &[100. + i as f64, 200., 1., 0., 0., 0., 0., i as f64, 0.],
                false,
            )
            .unwrap();
            assert!(app.take_value().is_none());
        }
        app.dispatch(UiAction::SetBrushSize { value: 42.0 })
            .unwrap();
        assert_eq!(
            app.take_value().unwrap()["state"]["brush"]["diameter"],
            42.0
        );
        assert!(app.take_value().is_none());
        app.chrome_hidden = true;
        assert_eq!(app.take_value().unwrap()["chrome_hidden"], true);
        app.input(UiInput::Key { key: " ".into(), pressed: true, repeat: false,
            modifiers: layer_ui::Modifiers::default(), editing: false, divider: None }).unwrap();
        app.keep_zen_button = true;
        let snapshot = app.take_value().unwrap();
        assert_eq!(snapshot["keep_zen_button"], true);
        assert_eq!(snapshot["pan_cursor"], true);
        assert!(app.take_value().is_none());
        app.error = Some("test surface error".into());
        assert_eq!(app.take_value().unwrap()["error"], "test surface error");
        assert!(app.take_value().is_none());
        app.resize(1600, 2560, 2.0).unwrap();
        assert!(app.take_value().is_some());
    }
    #[test]
    fn canvas_bar_queries_answer_for_the_current_bar() {
        use layer_ui::CommandId;
        let (_reference, mut app) = gpu_host(layer_ui::Platform::Android, [256, 192]);
        app.resize(2560, 1600, 2.0).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let clock = std::cell::Cell::new(0);
        while !app.startup.complete { frame_step(&mut app, &clock, deadline); }
        for command in [CommandId::RectangleSelect, CommandId::SelectAll, CommandId::FillSelection, CommandId::ScaleRotate] {
            app.dispatch(UiAction::Invoke { command }).unwrap();
            frame_step(&mut app, &clock, deadline);
        }
        while app.session.engine().transform_preview().is_none() { frame_step(&mut app, &clock, deadline); }
        let bar = app.session.state().canvas_bar.clone().expect("transform bar");
        let measure = json!({"context": bar.context, "label": 0, "items": vec![90.; bar.items.len()],
            "completion": vec![70.; bar.completion.len()], "more": 32, "height": 44, "gap": 4, "padding": 6});
        let layout = app.query(json!({"type": "canvas_bar_layout", "measure": measure})).unwrap();
        assert!(layout["bounds"].is_object());
        assert_eq!(layout, json!(app.session.canvas_bar_layout(&serde_json::from_value(measure).unwrap())));
        let menu = app.query(json!({"type": "canvas_bar_menu", "context": bar.context, "shown": 0})).unwrap();
        assert!(menu["sections"].is_array());
        assert_eq!(menu, json!(app.session.canvas_bar_menu(bar.context, 0)));
        let choice = app
            .query(json!({"type": "canvas_bar_choice_menu", "context": bar.context, "id": "transform-interpolation"}))
            .unwrap();
        let nearest = choice["sections"][0].as_array().unwrap().iter().find(|item| item["label"] == "Nearest").unwrap();
        app.dispatch(serde_json::from_value(nearest["action"].clone()).unwrap()).unwrap();
        assert!(app.session.state().commands.iter().any(|c| c.id == CommandId::TransformNearest && c.selected));
    }

    #[test]
    fn zoom_menu_query_serves_the_readout_menu_and_its_actions() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        let menu = app.query(json!({"type": "zoom_menu"})).unwrap();
        assert_eq!(menu, json!(app.session.zoom_menu()));
        let double = menu["sections"][1].as_array().unwrap().iter().find(|item| item["label"] == "200%").unwrap();
        assert_eq!(double["action"], json!({"type": "set_zoom", "zoom": 2.0}));
        app.dispatch(serde_json::from_value(double["action"].clone()).unwrap()).unwrap();
        assert_eq!(app.session.state().camera.zoom, 2.0);
        let actual = menu["sections"][0].as_array().unwrap().iter().find(|item| item["label"] == "Actual Pixels").unwrap();
        app.dispatch(serde_json::from_value(actual["action"].clone()).unwrap()).unwrap();
        assert_eq!(app.session.state().camera.zoom, 1.0);
        assert_eq!(app.query(json!({"type": "catalog"})).unwrap()["zoom"], json!(layer_ui::NumericControl::zoom()));
        app.take_value().unwrap();
        for action in [layer_ui::UiAction::SetZoomLocked { locked: true }, layer_ui::UiAction::SetRotationLocked { locked: true }] {
            app.dispatch(action).unwrap();
            let update = app.take_value().unwrap();
            assert_eq!(update["camera"], json!(app.session.state().camera));
        }
        let menu = app.query(json!({"type": "zoom_menu"})).unwrap();
        assert_eq!(menu["rotation_section"], 3);
        assert_eq!(menu["sections"][2][0]["selected"], true);
        assert_eq!(menu["sections"][3][1]["selected"], true);
        assert_eq!(menu["buttons"].as_array().unwrap().len(), layer_ui::NAVIGATOR_COMMANDS.len());
    }

    #[test]
    fn export_validation_refuses_webp_beyond_its_encoder_limit_before_rendering() {
        use layer_ui::{ExportDraftAction, ExportFormat, ExportRecipe, ExportSize};
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        let webp = ExportRecipe::web_share().draft_canonical(ExportDraftAction::Format(ExportFormat::Webp)).recipe;
        assert_eq!(app.query(json!({"type": "export_validate", "recipe": webp})).unwrap(), json!(webp));
        let enlarged = ExportRecipe { size: ExportSize::Fit { bounds: [20000, 20000], enlarge: true }, ..webp.clone() };
        let error = app.query(json!({"type": "export_validate", "recipe": enlarged})).unwrap_err();
        assert!(error.starts_with("WebP export is limited to 16,384 pixels per side."), "{error}");
        let png = ExportRecipe { format: ExportFormat::Png, ..enlarged };
        assert!(app.query(json!({"type": "export_validate", "recipe": png})).is_ok());
    }

    #[test]
    fn camera_patches_preserve_pending_structural_updates() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        app.take_value().unwrap();
        let finger = |app: &mut NativeHost, id, x, phase| {
            pointer(app,
                id,
                3,
                0,
                &[x, 300., 1., 0., 0., 0., 0., 1_000_000., phase],
                false,
            )
            .unwrap();
        };
        finger(&mut app, 1, 400., 1.);
        finger(&mut app, 2, 600., 1.);
        for i in 1..100 {
            finger(&mut app, 2, 600. + i as f64, 2.);
            let patch = app.take_value().unwrap();
            assert_eq!(patch["camera"], json!(app.session.state().camera));
            assert_eq!(patch["revision"], app.session.state().revision);
            assert!(
                patch.get("state").is_none(),
                "Camera motion must not build full UI models"
            );
            assert!(app.take_value().is_none());
        }
        // A camera update must not acknowledge an unpublished brush change.
        app.dispatch(UiAction::SetBrushSize { value: 42.0 })
            .unwrap();
        finger(&mut app, 2, 710., 2.);
        let full = app.take_value().unwrap();
        assert_eq!(full["state"]["brush"]["diameter"], 42.0);
        assert_eq!(full["state"]["camera"], json!(app.session.state().camera));
        // Also cover changes that bypass the host's dispatch wrapper.
        app.session
            .dispatch(UiAction::SetBrushSize { value: 52.0 })
            .unwrap();
        finger(&mut app, 2, 720., 2.);
        assert_eq!(
            app.take_value().unwrap()["state"]["brush"]["diameter"],
            52.0
        );
        app.error = Some("surface lost".into());
        finger(&mut app, 2, 730., 2.);
        assert_eq!(app.take_value().unwrap()["error"], "surface lost");
        app.resize(1600, 2560, 2.0).unwrap();
        assert!(app.take_value().unwrap().get("layout").is_some());
    }

    #[test]
    fn canvas_bar_queries_place_the_bar_and_reject_stale_contexts() {
        let mut app = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        app.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState::for_platform(layer_ui::Platform::Windows)),
        })
        .unwrap();
        app.resize(1600, 1000, 1.0).unwrap();
        app.dispatch(UiAction::Invoke { command: layer_ui::CommandId::SelectAll }).unwrap();
        app.dispatch(UiAction::Invoke { command: layer_ui::CommandId::Move }).unwrap();
        let bar = app.session.state().canvas_bar.clone().expect("selection bar");
        let measure = |context: layer_ui::CanvasBarContext| {
            json!({"type": "canvas_bar_layout", "measure": {
                "context": context, "items": vec![90.; bar.items.len()], "completion": [],
                "more": 40., "height": 52., "gap": 4., "padding": 6.
            }})
        };
        let layout = app.query(measure(bar.context)).unwrap();
        assert!(layout["bounds"]["width"].as_f64().unwrap() > 0., "{layout}");
        assert!(layout["items"].as_u64().unwrap() > 0);
        let stale = layer_ui::CanvasBarContext { generation: bar.context.generation + 1, ..bar.context };
        assert!(app.query(measure(stale)).unwrap().is_null());
        let menu = app.query(json!({"type": "canvas_bar_menu", "context": bar.context, "shown": 0})).unwrap();
        assert!(menu.to_string().contains("show_canvas_action_bar"), "{menu}");
        assert!(app.query(json!({"type": "canvas_bar_menu", "context": stale, "shown": 0})).unwrap().is_null());
    }

    #[test]
    fn warp_opens_over_a_large_filled_selection() {
        let (_reference, mut host) = gpu_host(layer_ui::Platform::Android, [6000, 4000]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let clock = std::cell::Cell::new(0);
        let frame = |host: &mut NativeHost| {
            frame_step(host, &clock, deadline);
        };
        while !host.startup.complete {
            frame(&mut host);
        }
        let mut accepted = None;
        for command in [
            layer_ui::CommandId::SelectAll,
            layer_ui::CommandId::FillSelection,
            layer_ui::CommandId::ScaleRotate,
            layer_ui::CommandId::TransformWarp,
        ] {
            host.dispatch(UiAction::Invoke { command }).unwrap();
            if command == layer_ui::CommandId::ScaleRotate {
                while host.session.engine().transform_preview().is_none() { frame(&mut host); }
            }
            for _ in 0..8 {
                frame(&mut host);
            }
            if command == layer_ui::CommandId::ScaleRotate {
                accepted = host.session.engine().transform_preview().map(|preview| preview.transform.placement.clone());
            }
        }
        let selected = |command| host.session.state().commands.iter().any(|c| c.id == command && c.selected);
        assert!(selected(layer_ui::CommandId::TransformWarp) && selected(layer_ui::CommandId::WarpGridThree), "Warp offers its grid");
        let bar = host.session.state().canvas_bar.clone().expect("Warp bar");
        assert!(bar.items.iter().any(|item| matches!(item.option, layer_ui::ToolOption::Choice { id: "transform-warp-grid", .. })));
        let bounds = host.session.engine().document().working.selection.as_ref().unwrap().bounds();
        let anchor = bar.anchor.expect("the bar anchors to the warped pixels");
        for (actual, expected) in anchor.into_iter().zip([bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]) {
            assert!((actual - expected).abs() <= 1., "the grid covers the selection: {anchor:?} against {bounds:?}");
        }
        let accepted = accepted.unwrap();
        let preview = host.session.engine().transform_preview().unwrap();
        assert_eq!(preview.transform.placement.outer, accepted.outer, "Warp keeps the accepted pose");
        assert!(preview.transform.placement.mesh.is_none(), "the untouched grid leaves the transform unchanged");
    }

    #[test]
    fn a_contact_begun_as_a_transform_opens_is_replayed_whole_once_it_is_prepared() {
        let (_reference, mut host) = gpu_host(layer_ui::Platform::Android, [256, 192]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let clock = std::cell::Cell::new(0);
        let frame = |host: &mut NativeHost| {
            frame_step(host, &clock, deadline);
        };
        while !host.startup.complete {
            frame(&mut host);
        }
        for command in [layer_ui::CommandId::SelectAll, layer_ui::CommandId::FillSelection] {
            host.dispatch(UiAction::Invoke { command }).unwrap();
            frame(&mut host);
        }
        host.dispatch(UiAction::Invoke { command: layer_ui::CommandId::ScaleRotate }).unwrap();
        while host.session.engine().transform_preview().is_none() { frame(&mut host); }
        host.dispatch(UiAction::Invoke { command: layer_ui::CommandId::TransformDistort }).unwrap();
        let surface = layer_core::Affine(host.session.state().camera.document_to_surface());
        let view_revision = host.session.state().camera.revision;
        let [corner, target] = [Point { x: 256., y: 0. }, Point { x: 216., y: 30. }].map(|p| surface.map(p));
        assert!(!host.paint_ready(), "the transform is not prepared before its first frame");
        for (step, (phase, at)) in
            [(PenPhase::Down, corner), (PenPhase::Move, target), (PenPhase::Up, target)].into_iter().enumerate()
        {
            let event = PenEvent {
                device_id: 1,
                sequence: 0,
                timestamp_ns: clock.get() + step as u64 * 1_000_000,
                view_revision,
                surface_position: at,
                pressure: 0.5,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase,
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            host.pointer_event(event, PointerButton::Primary).unwrap();
        }
        assert!(!host.deferred_contacts.is_empty(), "the contact waits instead of being dropped");
        while !host.deferred_contacts.is_empty() {
            frame(&mut host);
        }
        frame(&mut host);
        let map = host.session.engine().transform_preview().unwrap().transform.clone();
        let projective = map.projective().expect("the corner drag retains a homography");
        let moved = projective.map(Point { x: 256., y: 0. }).unwrap();
        assert!(
            (moved.x - 216.).abs() < 0.5 && (moved.y - 30.).abs() < 0.5,
            "the replayed drag moves the corner it grabbed once: {moved:?}"
        );
    }

    #[test]
    fn a_stroke_once_painting_is_reported_ready_paints_while_background_shaders_compile() {
        let (_reference, mut host) = gpu_host(layer_ui::Platform::Windows, [256, 192]);
        host.session.set_workspace_read_only(true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let clock = std::cell::Cell::new(0);
        let frame = |host: &mut NativeHost, busy: bool| {
            if busy {
                host.session.renderer_mut().0.as_mut().unwrap().shader_input();
            }
            frame_step(host, &clock, deadline);
        };
        let published = |host: &mut NativeHost| {
            host.invalidate_snapshot();
            let model = host.take_value().unwrap();
            (model["brush_ready"] == true, model["shaders_ready"] == true)
        };
        let mut brush_frames = 0;
        while !published(&mut host).0 {
            frame(&mut host, true);
            brush_frames += usize::from(host.paint_ready());
            if brush_frames == 3 {
                host.session.set_workspace_read_only(false);
            }
        }
        assert!(!published(&mut host).1, "background compiles are still pending");
        let view_revision = host.session.state().camera.revision;
        let records: Vec<f64> = (0..42u32)
            .flat_map(|i| {
                let phase = match i { 0 => 1., 41 => 3., _ => 2. };
                [250. + f64::from(i) * 3., 240. + 20. * (f64::from(i) / 6.).sin(), 1., 0., 0., 0., 0.,
                    (clock.get() + u64::from(i) * 1_000_000) as f64, phase]
            })
            .collect();
        host.pointer_batch(PointerBatch { id: 77, tool: 1, button: 0, records: &records, predicted: false, view_revision, barrel_twist: false })
            .unwrap();
        for _ in 0..8 {
            frame(&mut host, true);
        }
        assert!(!published(&mut host).1, "the stroke waits for no background compilation");
        assert_eq!(host.session.engine().metrics().committed_strokes, 1, "the stroke paints");
        assert!(host.session.state().document_file.modified);
        while !published(&mut host).1 {
            frame(&mut host, false);
        }
    }

    #[test]
    fn typed_touch_uses_the_same_shared_navigation_as_packed_input() {
        for (platform, size, samples) in [
            (layer_ui::Platform::Windows, [1600, 1000], [
                (1, 400., 300., 1_000_000, 1), (2, 600., 300., 1_000_000, 1),
                (2, 750., 300., 1_000_000, 2), (2, 750., 300., 1_000_000, 3),
            ]),
            (layer_ui::Platform::Android, [2560, 1600], [
                (1, 100., 100., 1_000_000, 1), (2, 200., 100., 1_000_000, 1),
                (1, 120., 120., 2_000_000, 2), (1, 120., 120., 3_000_000, 3),
            ]),
        ] {
            let mut typed = NativeHost::new(platform).unwrap();
            let mut packed = NativeHost::new(platform).unwrap();
            for app in [&mut typed, &mut packed] {
                app.resize(size[0], size[1], 2.0).unwrap();
            }
            let before = packed.session.state().camera.revision;
            for (id, x, y, timestamp_ns, phase) in samples {
                typed.pointer_event(PenEvent {
                    device_id: id, sequence: 1, timestamp_ns,
                    view_revision: typed.session.state().camera.revision,
                    surface_position: Point { x, y }, pressure: 1.,
                    tilt_radians: [0., 0.], twist_radians: 0., distance: 0.,
                    phase: match phase { 1 => PenPhase::Down, 2 => PenPhase::Move, _ => PenPhase::Up },
                    tool: ToolKind::Finger, flags: SampleFlags::PRIMARY,
                }, PointerButton::Primary).unwrap();
                pointer(&mut packed, id, 3, 0,
                    &[x as f64, y as f64, 1., 0., 0., 0., 0., timestamp_ns as f64, phase as f64], false).unwrap();
            }
            assert_eq!(json!(typed.session.state().camera), json!(packed.session.state().camera));
            assert_eq!(typed.sequence, 0);
            assert_eq!(packed.sequence, 0);
            assert_eq!(packed.paint_start_sequence(), 0);
            assert!(packed.session.state().camera.revision > before);
        }
    }

    #[test]
    fn typed_samples_from_retired_or_closed_documents_do_not_acquire_contacts() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.resize(800, 600, 1.).unwrap();
        let old_revision = host.session.state().camera.revision;
        host.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::ZoomIn,
        })
        .unwrap();
        host.document_adopted();
        assert!(host.session.state().camera.revision > old_revision);
        let mut event = PenEvent {
            device_id: 1,
            sequence: 1,
            timestamp_ns: 1_000_000,
            view_revision: old_revision,
            surface_position: Point { x: 400., y: 300. },
            pressure: 0.5,
            tilt_radians: [0.; 2],
            twist_radians: 0.,
            distance: 0.,
            phase: PenPhase::Down,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        };
        host.pointer_event(event, PointerButton::Primary).unwrap();
        assert!(host.deferred_contacts.is_empty());
        host.session.require_document_idle().unwrap();
        host.session.request_document_close().unwrap();
        assert!(host.session.state().document_file.close_ready);
        event.view_revision = host.session.state().camera.revision;
        host.pointer_event(event, PointerButton::Primary).unwrap();
        assert!(host.deferred_contacts.is_empty());
        host.session.require_document_idle().unwrap();
    }

    #[test]
    #[ignore = "Requires an explicitly selected hardware GPU"]
    fn estimated_input_tokens_reach_committed_stroke_corrections() {
        let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(gpu.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
        let mut host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        host.session = UiSession::from_project(
            Renderer(Some(gpu.into())),
            layer_ui::new_drawing(64, 48, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),
            None,
            [64, 48],
            layer_ui::Platform::Mac,
        )
        .unwrap();
        host.session.frame(0, 0).unwrap();
        assert!(host.paint_ready());
        assert_eq!(host.paint_start_sequence(), 0);
        let revision = host.session.state().camera.revision;
        for (token, pending, x, phase, time) in [
            (9001, 1, 20., 1., 10_000_000.),
            (9002, 0, 40., 2., 20_000_000.),
            (9003, 0, 40., 3., 21_000_000.),
        ] {
            host.pointer_batch_updates(
                PointerBatch {
                    id: 7,
                    tool: 0,
                    button: 0,
                    predicted: false,
                    view_revision: revision,
                    barrel_twist: false,
                    records: &[x, 24., 0.25, 0., 0., 0., 0., time, phase],
                },
                &[token, pending],
                false,
            )
            .unwrap();
            if phase == 1. {
                assert!(host.paint_start_sequence() > 0, "pen-down is visible before rendering ink");
                assert!(!host.session.engine().has_active_stroke(), "the frame has not consumed pen-down yet");
            }
            host.session.frame(time as u64, time as u64).unwrap();
        }
        assert!(host.last_pen.is_none());
        let checkpoint = host.session.engine().checkpoint();
        let before = crate::test_support::active_source(host.session.engine().document()).raster.clone();
        let before_pixels = host.session
            .renderer_mut()
            .0
            .as_mut()
            .unwrap()
            .readback_srgb_rgba8()
            .unwrap();
        let camera = json!(host.session.state().camera);
        let paint_start = host.paint_start_sequence();
        host.pointer_batch_updates(
            PointerBatch {
                id: 7,
                tool: 0,
                button: 0,
                predicted: false,
                view_revision: revision,
                barrel_twist: false,
                records: &[24., 24., 0.9, 0.2, -0.3, 1.7, 0., 10_000_000., 1.],
            },
            &[9001, 0],
            true,
        )
        .unwrap();
        assert_eq!(host.paint_start_sequence(), paint_start, "corrected down samples do not start another contact");
        host.session.frame(30_000_000, 30_000_000).unwrap();
        let after = &crate::test_support::active_source(host.session.engine().document()).raster;
        assert_ne!(
            after, &before,
            "late correction publishes a replacement raster root"
        );
        assert_ne!(host.session.engine().checkpoint(), checkpoint, "a prior save remains dirty after correction");
        assert_eq!(host.session.engine().metrics().committed_strokes, 1);
        let after_pixels = host.session
            .renderer_mut()
            .0
            .as_mut()
            .unwrap()
            .readback_srgb_rgba8()
            .unwrap();
        assert_ne!(
            after_pixels, before_pixels,
            "corrected pressure/position changes pixels"
        );
        assert!(host.last_pen.is_none());
        assert!(host.deferred_contacts.is_empty());
        assert_eq!(json!(host.session.state().camera), camera);
        host.session.require_document_idle().unwrap();
        host.dispatch(UiAction::Invoke { command: layer_ui::CommandId::Undo }).unwrap();
        host.session.frame(40_000_000, 40_000_000).unwrap();
        assert!(crate::test_support::active_source(host.session.engine().document()).raster.is_empty());
        assert!(!host.session.engine().can_undo(), "correction adds no undo entry");
    }

    #[test]
    fn malformed_input_is_rejected_before_changing_state() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        for sample in [
            vec![],
            vec![0.0; 8],
            vec![f64::NAN; 9],
            vec![0., 0., 1., 0., 0., 0., 0., -1., 1.],
        ] {
            assert!(pointer(&mut app, 1, 0, 0, &sample, false).is_err());
        }
        assert_eq!(app.sequence, 0);
        assert!(app.resize(0, 100, 1.0).is_err());
        assert!(app.resize(100, 100, 0.0).is_err());
    }
}

#[cfg(test)]
mod test_support {
    use layer_core::{Document, SourceTarget, authored::{PaintSource, Composition}};
    pub fn active_source(document: &Document) -> &PaintSource {
        let SourceTarget::Paint(handle) = document.working.target.unwrap() else { panic!("paint target"); };
        document.artwork.paint.get(handle).unwrap()
    }
    pub fn active_source_mut(document: &mut Document) -> &mut PaintSource {
        let SourceTarget::Paint(handle) = document.working.target.unwrap() else { panic!("paint target"); };
        document.artwork.paint.get_mut(handle).unwrap()
    }
    pub fn composition_mut(document: &mut Document) -> &mut Composition {
        document.artwork.compositions.get_mut(document.artwork.root).unwrap()
    }
    pub fn hide_paper(document: &mut Document) {
        let handles = document.scene().constant_backdrop().to_vec();
        for handle in handles { document.artwork.occurrences.get_mut(handle).unwrap().visible = false; }
    }
}
