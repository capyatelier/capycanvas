//! Renderer replacement keeps CPU document/session ownership in place.
use super::*;

pub const LAUNCH_CAPTION_DELAY_MS: u64 = 5000;
pub const LAUNCH_REVEAL_MS: u64 = 500;
pub const LAUNCH_MARK_SIZE: f32 = 7. * crate::layout::TILE_SIZE;
pub const LAUNCH_MARK_MAX_FRACTION: f32 = 0.4;
pub const LAUNCH_CAPTION_GAP: f32 = crate::layout::TILE_SIZE * 0.5;
pub const LAUNCH_CAPTION_SIZE: f32 = crate::UI_TEXT_PT as f32 * 96. / 72.;
pub const LAUNCH_CAPTION_COLOR_ROLE: &str = "settings_secondary";

pub fn launch_mark_size(viewport: [f32; 2]) -> f32 {
    LAUNCH_MARK_SIZE.min(viewport[0].min(viewport[1]).max(0.) * LAUNCH_MARK_MAX_FRACTION)
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct LaunchMark {
    pub icon: &'static str,
    pub ink_bottom: f32,
}

struct LaunchInk { view_top: f32, view_size: f32, bottom: f32, circle: [f32; 3] }

impl crate::ZenIcon {
    fn launch_ink(self) -> LaunchInk {
        match self {
            Self::LookingUp => LaunchInk { view_top: 4.84, view_size: 428., bottom: 400., circle: [180.24, 214.49, 232.] },
            Self::FacingForward => LaunchInk { view_top: 31., view_size: 364., bottom: 378., circle: [136.86, 223.94, 184.] },
            Self::Bathing => LaunchInk { view_top: -21., view_size: 460., bottom: 370.76, circle: [245.21, 254.61, 211.1] },
            Self::Sleeping => LaunchInk { view_top: -73., view_size: 428., bottom: 244., circle: [210.45, 173.55, 193.5] },
        }
    }
    pub fn launch_system_circle(self) -> [f32; 3] {
        self.launch_ink().circle
    }
    pub fn launch_mark(self) -> LaunchMark {
        let ink = self.launch_ink();
        LaunchMark {
            icon: self.icon(),
            ink_bottom: (ink.bottom - ink.view_top) / ink.view_size,
        }
    }
}

pub(super) const PAINTING_UNAVAILABLE: &str = "Painting is unavailable. Save the drawing and reopen it.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasNotice {
    Canvas,
    Brush,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaunchReadiness {
    pub canvas: bool,
    pub brush: bool,
    pub chrome: bool,
    pub failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LaunchView {
    pub visible: bool,
    pub finished: bool,
    pub frame: Option<u64>,
}

/// Accepted layer-preview requests of one drawing. After a renderer
/// replacement, unfinished requests are submitted again under their original
/// ids before new ones.
#[derive(Default)]
pub struct ThumbnailRequests {
    drawing: (u64, u64),
    renderer: u64,
    in_flight: Vec<(u64, layer_render::ThumbnailTarget)>,
    resubmit: std::collections::VecDeque<(u64, layer_render::ThumbnailTarget)>,
}
impl ThumbnailRequests {
    pub fn retry<R: CanvasRenderer>(&mut self, session: &UiSession<R>) -> Option<(u64, layer_render::ThumbnailTarget)> {
        let (drawing, renderer) = ((session.engine.document().owner, session.state.document_file.epoch), session.renderer_generation);
        if self.drawing != drawing {
            *self = Self { drawing, renderer, ..Default::default() };
        } else if self.renderer != renderer {
            self.renderer = renderer;
            let lost = std::mem::take(&mut self.in_flight);
            self.resubmit.extend(lost);
        }
        self.resubmit.front().copied()
    }
    pub fn submitted(&mut self, request: u64, target: layer_render::ThumbnailTarget, retry: bool) {
        if retry { self.resubmit.pop_front(); }
        self.in_flight.push((request, target));
    }
    pub fn refused<R: CanvasRenderer>(&mut self, session: &UiSession<R>, unavailable: bool) {
        let Some(&(_, target)) = self.resubmit.front() else { return };
        let scene = session.engine.document().scene();
        let exists = match target {
            layer_render::ThumbnailTarget::Occurrence(handle) => scene.occurrence(handle).is_some(),
            layer_render::ThumbnailTarget::Source(SourceTarget::Paint(handle)) => scene.paint(handle).is_some(),
            layer_render::ThumbnailTarget::Source(SourceTarget::Coverage(handle)) => scene.coverage(handle).is_some(),
            layer_render::ThumbnailTarget::Source(SourceTarget::Selection(handle)) => scene.artwork().selections.get(handle).is_some(),
            layer_render::ThumbnailTarget::QuickMask => true,
        };
        if !unavailable || !exists { self.resubmit.pop_front(); }
    }
    pub fn completed(&mut self, request: u64) { self.in_flight.retain(|(id, _)| *id != request); }
}

impl<R: CanvasRenderer> UiSession<R> {
    fn launch_blocked(&self) -> bool {
        self.rendering_suspended || self.workspace_attention || self.host_attention || self.state.host_error.is_some()
            || self.state.settings_open || self.state.new_drawing.is_some()
            || self.state.requests.iter().any(|request| !matches!(request.kind,
                HostRequestKind::SaveSettings { .. } | HostRequestKind::SetFullscreen { .. }
                    | HostRequestKind::NewWindow | HostRequestKind::OpenLink { .. }))
    }

    pub(super) fn launch_covering(&self) -> bool {
        !self.launch_finished && !self.launch_blocked()
    }

    pub fn set_workspace_attention(&mut self, attention: bool) -> UiChange {
        let changed = std::mem::replace(&mut self.workspace_attention, attention) != attention;
        self.launch_attention_changed(changed)
    }

    pub fn set_host_attention(&mut self, attention: bool) -> UiChange {
        let changed = std::mem::replace(&mut self.host_attention, attention) != attention;
        self.launch_attention_changed(changed)
    }

    fn launch_attention_changed(&mut self, changed: bool) -> UiChange {
        if changed && !self.launch_finished { self.changed(regions::HOST, false) } else { UiChange::default() }
    }

    pub fn launch_state(&self, readiness: LaunchReadiness) -> LaunchView {
        let blocked = readiness.failed || self.launch_blocked();
        let workspace = self.state.workspace_ready && !self.workspace_read_only;
        let canvas = readiness.canvas && self.engine.backend().showing_drawing();
        let ready = workspace && canvas && readiness.brush && readiness.chrome;
        let identity = [self.engine.document().owner, self.state.document_file.epoch,
            self.renderer_generation, self.workspace_model_revision(), self.state.camera.revision,
            self.engine.document().revision];
        let frame = (!self.launch_finished && !blocked && ready).then(|| {
            use sha2::Digest;
            const MAX_EXACT_JSON_INTEGER: u64 = (1 << 53) - 1;
            let mut digest = sha2::Sha256::new();
            for field in identity { digest.update(field.to_le_bytes()); }
            digest.update([u8::from(workspace), u8::from(canvas), u8::from(readiness.brush),
                u8::from(readiness.chrome), u8::from(readiness.failed)]);
            (u64::from_le_bytes(digest.finalize()[..8].try_into().unwrap()) & MAX_EXACT_JSON_INTEGER).max(1)
        });
        LaunchView {
            visible: !self.launch_finished && !blocked,
            finished: self.launch_finished,
            frame,
        }
    }

    pub fn launch_presented(&mut self, frame: u64, readiness: LaunchReadiness) -> bool {
        let view = self.launch_state(readiness);
        if view.frame != Some(frame) { return false; }
        self.launch_finished = true;
        self.release_held_notice();
        true
    }

    pub fn canvas_notice(&self, canvas_ready: bool, brush_ready: bool) -> Option<CanvasNotice> {
        if self.drawing_switch_pending { None }
        else if !canvas_ready { Some(CanvasNotice::Canvas) }
        else if !brush_ready { Some(CanvasNotice::Brush) }
        else { None }
    }

    pub fn rendering_diagnostics(&self) -> serde_json::Value {
        let telemetry = self.engine.backend().telemetry();
        serde_json::json!({"canvas": self.engine.document().composition().size,
            "color": self.engine.document().composition().color,
            "revision": self.engine.document().revision, "epoch": self.state.document_file.epoch,
            "tool": self.layer_interaction.tool, "transform_open": self.operation.transforming(),
            "transform_pixels_pending": self.content_bounds.baking(),
            "transform_selection_pending": self.region_tools.applying_transform(),
            "raster_backing_pending": self.engine.raster_backing_pending(),
            "content_bounds": self.content_bounds.diagnostics(),
            "input_pending": self.engine.has_pending_input(), "stroke_active": self.engine.has_active_stroke(),
            "deferred_edits": self.deferred_edits.len(), "file_pending": self.files.pending.is_some(),
            "painted_selection_pending": self.painted_selections.busy(),
            "selection_edit_pending": self.region_tools.publishing_edit(), "notice": self.state.notice,
            "suspended": self.rendering_suspended, "tracked_renderer_bytes": telemetry.resident_bytes,
            "submitted_frames": telemetry.submissions})
    }
    /// The host has no modal work and must wait for this boundary before normal
    /// parking. Failed renderers remain navigable/saveable/closeable.
    pub(super) fn document_switch_interaction_idle(&self, suspended: bool, retired: &[u32])->bool {
        (suspended || self.workspace_gestures_idle() && !self.painted_selections.busy() && self.document_interaction_idle_reason_for_switch(false,true).is_none()) && !self.workspace_transition && !self.state.customization.header_editing
            && self.state.requests.iter().all(|request| retired.contains(&request.id)) && self.files.pending.as_ref().is_none_or(|(id, _)| retired.contains(id))
    }
    pub fn can_switch_document(&self) -> bool {
        self.document_switch_interaction_idle(self.rendering_suspended, &[])
            && (self.rendering_suspended || self.pending_filters.is_none() && self.document_interaction_idle_reason_for_switch(true,true).is_none())
    }
    pub fn prepare_document_switch(&mut self) -> Result<(), String> {
        self.prepare_document_action()?;
        if self.can_switch_document() { Ok(()) }
        else { Err(DocumentTransportRefusal::SwitchOperation.message(self.localization()).to_string()) }
    }
    pub fn can_park_document(&self) -> bool {
        self.can_park_document_after(&[])
    }
    pub fn can_park_document_after(&self, retired: &[u32]) -> bool {
        let idle = if self.state.document_file.close_ready { self.require_document_snapshot_idle() } else { self.require_document_idle() };
        self.document_switch_interaction_idle(self.rendering_suspended, retired) && self.deferred_edits.is_empty()
            && (self.rendering_suspended || (idle.is_ok() && self.engine.can_park()))
    }

    pub(super) fn discard_render_requests(&mut self) {
        self.engine.clear_liquify_preview();
        self.operation.liquify = None;
        self.cancel_content_bounds();
        self.content_bounds = Default::default();
        self.resubmit_conversion();
        self.cancel_picker();
        self.eyedropper.renderer_replaced();
        self.cancel_tonal();
        self.region_tools.renderer_replaced();
        self.painted_selections.renderer_replaced();
        self.discard_mode_actions();
        self.reset_filter_previews();
    }

    pub fn release_idle_document_buffers(&mut self) {
        self.engine.release_idle_buffers();
        self.filter_previews.renderer_replaced();
    }

    /// Retire an idle drawing after input submission and backing publication.
    /// Its renderer can later be replaced on this same session.
    pub fn park_document(&mut self) -> Result<layer_core::raster_storage::RetainedTiles, String> {
        if !self.can_park_document() {
            return Err("Finish the current operation before switching drawings".into());
        }
        let tiles = self.retained_document_tiles();
        if !self.rendering_suspended && !self.state.document_file.close_ready
            && tiles.try_blobs()?.is_none()
        {
            return Err("Wait for drawing capture before switching drawings".into());
        }
        self.input(UiInput::Blur)?;
        self.end_color_contact();
        self.return_to_artwork()?;
        self.release_idle_document_buffers();
        self.cancel_auto_levels();self.cancel_histogram();self.cancel_effect_analyses();
        self.rendering_suspended = true;
        self.storage_activity.set_busy(false);self.storage_activity.touch();
        self.drawing_switch_pending = true;
        self.refresh_commands();
        Ok(tiles)
    }

    pub fn retained_document_tiles(&self) -> layer_core::raster_storage::RetainedTiles {
        let mut tiles = self.engine.retained_tiles();
        // Include the fixed native input queue/session structures. This is
        // admission accounting, not process RSS.
        tiles.metadata_bytes = tiles.metadata_bytes.saturating_add(2 * 1024 * 1024);
        tiles
    }

    /// A host with multiple drawings retains one workspace owner. Bring its
    /// layout/history/settings forward without overwriting this drawing's view,
    /// tools, history, dirty checkpoint or color interpretation.
    pub fn inherit_window_state(&mut self, previous: &Self) -> Result<UiChange, String> {
        self.launch_finished = previous.launch_finished;
        self.workspace_attention = previous.workspace_attention;
        self.host_attention = previous.host_attention;
        let navigation=self.camera_navigation_revision();
        let camera=(!self.initial_fit).then(||self.state.camera.clone());
        let restored_view=self.restored_view.take();
        self.set_localization(previous.localization().clone());
        self.localization_generation = previous.localization_generation;
        self.preferences_revision = self.preferences_revision.max(previous.preferences_revision);
        // A recording is window state, shared by parked and active documents.
        self.engine.recording = previous.engine.recording.clone();
        self.state.platform = previous.state.platform;
        self.platform_prediction_available = previous.platform_prediction_available;
        self.interaction.touch_policy = previous.interaction.touch_policy;
        self.finger.device = previous.finger.device;
        self.system_theme = previous.system_theme;
        self.system_accent = previous.system_accent;
        self.state.workspace = previous.state.workspace.clone();
        self.state.phone = PhoneState { menu: false, menu_navigation: Default::default(), capy_drag: false, slider: None, ..previous.state.phone.clone() };
        self.state.workspace_ready = previous.state.workspace_ready;
        self.state.tool_slots = previous.state.tool_slots.clone();
        self.region_tools.last_fill = previous.region_tools.last_fill.or(self.region_tools.last_fill);
        self.workspace_history = previous.workspace_history.clone();
        self.workspace_read_only = previous.workspace_read_only;
        self.managed_workspace = previous.managed_workspace.clone();
        self.state.fullscreen = previous.state.fullscreen;
        self.state.screen = ScreenState {
            report: previous.state.screen.report.clone(),
            assessment: previous.state.screen.assessment,
            show_clipped: previous.state.screen.show_clipped,
            ..Default::default()
        };
        self.state.customization = Default::default();
        self.state.document_file.epoch = previous.state.document_file.epoch.checked_add(1)
            .ok_or("Document activation generation exhausted")?;
        if self.rendering_suspended { self.cancel_effect_analyses(); } else { self.clear_effect_analyses()?; }
        self.state.revision = self.state.revision.max(previous.state.revision);
        self.next_request = self.next_request.max(previous.next_request);
        self.inherit_notice_ids(previous);
        self.papers.inherit(&previous.papers);
        self.apply_settings(previous.state.settings.clone())?;
        // View orientation/zoom belongs to this drawing; display size and scale
        // belong to the window and may have changed while this editor slept.
        if let Some(logical) = previous.logical_viewport {
            self.set_viewport(logical, previous.state.camera.viewport)?;
        }
        self.sync_work_area();
        if let Some(camera)=camera.filter(|c|(c.viewport,c.work_area)!=(self.state.camera.viewport,self.state.camera.work_area)) {
            self.state.camera=super::session_recovery::SessionCamera::capture(&camera).restore(&self.state.camera)?;
        }
        // A window can still have queued native input from the previous tab.
        // Preserve each drawing's camera while retiring that input generation.
        self.state.camera.revision = self.state.camera.revision.max(previous.state.camera.revision)
            .checked_add(1).ok_or("Camera generation exhausted")?;
        self.automatic_camera_revision=self.state.camera.revision.checked_sub(navigation).ok_or("Camera revision moved backwards")?;
        self.restored_view=restored_view;
        self.sync_camera();
        self.refresh_layer_presentation();
        self.refresh_commands();
        self.drawing_switch_pending = true;
        self.begin_canvas_transition();
        Ok(self.changed(regions::ALL, true))
    }

    pub fn inherit_restored_window_state(&mut self, previous: &Self) -> Result<UiChange, String> {
        self.restore_editing(previous.editing_state())?;
        self.inherit_window_state(previous)
    }

    /// Initialize a newly opened drawing from the current painting tools. Do
    /// not call when reactivating a parked drawing: it owns its existing tools.
    pub fn inherit_initial_drawing_tools(&mut self, previous: &Self) -> Result<(), String> {
        let destination = self.engine.document().composition().color.space;
        let transform = previous.engine.document().composition().color.space.linear_transform(destination);
        let mut brush = previous.engine.configured_brush().clone();
        previous.state.colors.load_paint(&mut brush, destination)?;
        let secondary = &mut brush.color_dynamics.secondary_color_rgba_linear;
        let rgb = layer_core::color::rgb::apply(transform, [secondary[0],secondary[1],secondary[2]].map(f64::from));
        secondary[..3].copy_from_slice(&rgb.map(|v|v as f32));
        self.engine.set_brush(brush).map_err(error)?;
        self.state.brush = previous.state.brush.clone();
        self.state.colors = previous.state.colors.clone();
        self.state.color_library = previous.state.color_library.clone();
        self.state.colors.set_rgb_space(destination)?;
        self.state.colors.set_document_depth(self.engine.document().composition().color.depth)?;
        self.apply_brush()
    }

    pub fn rendering_suspended(&self) -> bool {
        self.rendering_suspended
    }

    pub fn inherit_parked_viewport(&mut self, previous: &Self) -> UiChange {
        assert!(self.rendering_suspended, "Viewport adoption requires a parked drawing");
        let navigation = self.camera_navigation_revision();
        if let Some(logical) = previous.logical_viewport {
            let physical = previous.state.camera.viewport;
            let old_scale = self.logical_viewport.map_or(1.0, |v| self.state.camera.viewport[0] as f32 / v[0]);
            let new_scale = physical[0] as f32 / logical[0];
            if self.state.camera.viewport != physical {self.restored_view=None;}
            self.state.camera.resize(physical);
            self.logical_viewport = Some(logical);
            if let Some(event) = self.cursor.event.as_mut() {
                event.surface_position.x *= new_scale / old_scale;
                event.surface_position.y *= new_scale / old_scale;
            }
            self.sync_work_area();
        }
        self.state.camera.revision = self.state.camera.revision.max(previous.state.camera.revision)
            .checked_add(1).expect("Camera generation exhausted");
        self.automatic_camera_revision = self.state.camera.revision - navigation;
        self.sync_camera();
        self.changed(regions::CAMERA, true)
    }

    pub fn cancel_document_park(&mut self) -> Result<UiChange, String> {
        if !self.rendering_suspended {
            return Err("The drawing is not parked".into());
        }
        self.rendering_suspended = false;
        self.drawing_switch_pending = false;
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS, true))
    }

    pub(super) fn command_without_document(command: CommandId) -> bool {
        matches!(
            command,
            CommandId::Settings
                | CommandId::KeyboardShortcuts
                | CommandId::ToggleTheme
                | CommandId::Fullscreen
                | CommandId::NewWindow
                | CommandId::About
                | CommandId::Website
                | CommandId::SourceCode
        )
    }

    pub(super) fn command_without_renderer(command: CommandId) -> bool {
        Self::command_without_document(command) || matches!(command,
            CommandId::CancelTransform | CommandId::SaveDocument | CommandId::SaveDocumentAs
                | CommandId::CloseDocument | CommandId::Drawings | CommandId::NextDrawing | CommandId::PreviousDrawing)
    }

    pub(super) fn action_without_renderer(action: &UiAction) -> bool {
        match action {
            UiAction::Invoke { command } => Self::command_without_renderer(*command),
            UiAction::OpenSettings { .. } | UiAction::Preferences { .. } | UiAction::SetTheme { .. } => true,
            UiAction::CanvasSize { action } => *action == CanvasSizeAction::Cancel,
            UiAction::ImageSize { action } => *action == ImageSizeAction::Cancel,
            UiAction::FrequencySeparation { action } => *action == FrequencySeparationAction::Cancel,
            _ => action.is_host_report(),
        }
    }

    /// Preserve saveable source state after rendering cannot resume. The host
    /// has stopped new canvas input. Unsubmitted samples are discarded; only
    /// completed raster captures can supply recovery pixels.
    pub fn suspend_renderer(&mut self) -> Result<UiChange, String> {
        let interrupted_selection = self.painted_selections.busy();
        let retired_regions = self.input(UiInput::Blur)?.change.regions;
        self.cancel_transform()?;
        self.engine.discard_unsubmitted_input();
        self.input_pending = false;
        self.pen_contact = false;
        self.discard_render_requests();
        if self.pending_filters.take().is_some() {
            self.state.filter_load.pending = false;
            self.state.filter_load.error =
                Some("Filter validation stopped because painting is unavailable".into());
        }
        self.cancel_auto_levels();self.cancel_histogram();self.cancel_effect_analyses();
        self.rendering_suspended = true;
        self.storage_activity.set_busy(false);self.storage_activity.touch();
        let recovered = self.engine.recover_failed_rasters().map_err(error);
        self.refresh_document();
        self.poll_document_close();
        self.refresh_commands();
        if self.engine.document().memory().package_failure().is_some() {
            self.raise_message_notice(MessageId::DOCUMENTS_PACKAGE_DAMAGED_BODY);
            self.set_host_error(Some(self.localization().text(MessageId::DOCUMENTS_PACKAGE_DAMAGED_BODY).to_string()));
        }else if recovered? > 0 || interrupted_selection {
            self.set_host_error(Some("Painting stopped. Edits whose pixels could not be recovered were canceled; earlier edits are retained.".into()));
        }
        Ok(self.changed(
            retired_regions
                | regions::DOCUMENT
                | regions::BRUSH
                | regions::COMMANDS
                | regions::HOST,
            false,
        ))
    }

    pub(super) fn require_painting(&self) -> Result<(), String> {
        (!self.rendering_suspended).then_some(()).ok_or_else(|| PAINTING_UNAVAILABLE.into())
    }
    /// Retain document history, tool settings, camera and workspace. GPU-only
    /// readbacks are cancelled; pending filter validation resumes from retained
    /// source bytes before it can publish a catalog or edit.
    pub fn replace_renderer(&mut self, mut renderer: R) -> Result<(R, UiChange), String> {
        if (!self.engine.document().artwork.objects.is_empty() || self.engine.document().artwork.paint.iter().any(|(_, _, source)| source.base.is_some()))
            && !renderer.supports_tiled_sources()
        {
            return Err("The replacement renderer does not support tiled photo documents".into());
        }
        if let Some(pending) = &self.pending_filters
            && !renderer
                .request_effect_validation(pending.validation.clone())
                .map_err(error)?
        {
            return Err("The replacement GPU could not resume filter validation".into());
        }
        // Capture failures can arrive after suspension, while the old GPU is
        // being retired. Recheck before allowing its history onto a new device.
        if self.rendering_suspended {
            self.engine.recover_failed_rasters().map_err(error)?;
        }
        self.discard_render_requests();
        self.cancel_auto_levels();self.cancel_histogram();self.cancel_effect_analyses();
        let previous = self.engine.replace_backend(renderer).map_err(error)?;
        self.paper_preview.invalidate();
        self.renderer_generation = self.renderer_generation.wrapping_add(1);
        self.brush_stamps.invalidate();
        self.input_pending = self.engine.has_pending_input();
        self.refresh_file_state();
        self.sync_renderer_telemetry();
        if self.rendering_suspended {
            self.set_host_error(None);
        }
        self.rendering_suspended = false;
        if let Some(pending) = &mut self.pending_filters {
            pending.validated = false;
        }
        self.refresh_commands();
        self.begin_canvas_transition();
        self.preparation_context = None;
        let change = self.changed(regions::DOCUMENT | regions::BRUSH | regions::COMMANDS, true);
        Ok((previous, change))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::*;

    const LAUNCH_READY: LaunchReadiness = LaunchReadiness { canvas: true, brush: true, chrome: true, failed: false };

    fn launching() -> UiSession<Recorder> {
        let mut app = launching_session(Platform::Windows);
        app.finish_workspace_startup();
        app.renderer_mut().showing_drawing = true;
        app
    }

    #[test]
    fn launch_waits_for_each_dependency_and_then_a_ready_presentation() {
        for missing in 0..4 {
            let mut app = launching();
            let mut readiness = LAUNCH_READY;
            match missing {
                0 => app.set_workspace_read_only(true),
                1 => readiness.canvas = false,
                2 => readiness.brush = false,
                _ => readiness.chrome = false,
            }
            let pending = app.launch_state(readiness);
            assert!(pending.visible && !pending.finished);
            assert_eq!(pending.frame, None);
            assert!(serde_json::to_value(pending).unwrap().get("notice").is_none());
            app.set_workspace_read_only(false);
            let ready = app.launch_state(LAUNCH_READY);
            assert!(ready.visible && !ready.finished);
            assert!(ready.frame.is_some());
            assert_eq!(app.launch_state(LAUNCH_READY).frame, ready.frame);
            assert!(!app.launch_presented(0, LAUNCH_READY));
            assert!(app.launch_presented(ready.frame.unwrap(), LAUNCH_READY));
            assert!(!app.launch_state(LAUNCH_READY).visible);
        }
    }

    #[test]
    fn launch_queries_are_pure_and_reject_completion_while_unready() {
        let mut app = launching();
        let old = app.launch_state(LAUNCH_READY).frame.unwrap();
        assert_eq!(old as f64 as u64, old);
        let unready = LaunchReadiness { brush: false, ..LAUNCH_READY };
        assert!(!app.launch_presented(old, unready));
        let current = app.launch_state(LAUNCH_READY).frame.unwrap();
        assert_eq!(old, current);
        assert!(!app.launch_finished);
        app.set_workspace_read_only(true);
        assert!(!app.launch_presented(current, LAUNCH_READY));
        app.set_workspace_read_only(false);
        let current = app.launch_state(LAUNCH_READY).frame.unwrap();
        assert!(app.launch_presented(current, LAUNCH_READY));
    }

    #[test]
    fn launch_rejects_old_renderer_and_document_completions() {
        let mut app = launching();
        let renderer_frame = app.launch_state(LAUNCH_READY).frame.unwrap();
        app.replace_renderer(Recorder::default()).unwrap();
        assert!(!app.launch_presented(renderer_frame, LAUNCH_READY));
        app.renderer_mut().showing_drawing = true;
        let document_frame = app.launch_state(LAUNCH_READY).frame.unwrap();
        let mut incoming = launching();
        incoming.inherit_window_state(&app).unwrap();
        assert!(!incoming.launch_presented(document_frame, LAUNCH_READY));
        let current = incoming.launch_state(LAUNCH_READY).frame.unwrap();
        assert!(incoming.launch_presented(current, LAUNCH_READY));
    }

    #[test]
    fn launch_rejects_presentation_from_before_the_final_layout() {
        let mut app = launching();
        let old = app.launch_state(LAUNCH_READY).frame.unwrap();
        app.set_viewport([720., 480.], [1440, 960]).unwrap();
        assert!(!app.launch_presented(old, LAUNCH_READY));
        let current = app.launch_state(LAUNCH_READY).frame.unwrap();
        assert!(app.launch_presented(current, LAUNCH_READY));
    }

    #[test]
    fn launch_waits_for_restored_workspace_to_accept_input() {
        let mut app = launching_session(Platform::Gtk);
        app.renderer_mut().showing_drawing = true;
        assert!(!app.state.workspace_ready);
        assert_eq!(app.launch_state(LAUNCH_READY).frame, None);
        app.set_workspace_read_only(true);
        app.finish_workspace_startup();
        assert_eq!(app.launch_state(LAUNCH_READY).frame, None);
        app.set_workspace_read_only(false);
        assert!(app.launch_state(LAUNCH_READY).frame.is_some());
    }

    #[test]
    fn launch_does_not_reveal_a_surround_only_frame_with_ready_shaders() {
        let mut app = launching();
        app.renderer_mut().showing_drawing = false;
        let surround = app.launch_state(LAUNCH_READY);
        assert!(surround.visible && !surround.finished);
        assert_eq!(surround.frame, None);
        assert!(!app.launch_presented(1, LAUNCH_READY));
        app.renderer_mut().showing_drawing = true;
        let drawing = app.launch_state(LAUNCH_READY).frame.unwrap();
        assert!(app.launch_presented(drawing, LAUNCH_READY));
    }

    #[test]
    fn launch_never_covers_failures_or_required_interaction() {
        let mut app = launching();
        for condition in 0..7 {
            let old = app.launch_state(LAUNCH_READY).frame.unwrap();
            let readiness = LaunchReadiness { failed: condition == 0, ..LAUNCH_READY };
            app.set_host_attention(condition == 5);
            app.set_workspace_attention(condition == 6);
            app.state.host_error = (condition == 1).then(|| "Cannot initialize canvas".into());
            app.state.settings_open = condition == 2;
            app.rendering_suspended = condition == 3;
            if condition == 4 {
                app.state.requests.push(HostRequest { id: 999, kind: HostRequestKind::ImportKeymap });
            }
            let view = app.launch_state(readiness);
            assert!(!view.visible && !view.finished);
            assert_eq!(view.frame, None);
            assert!(!app.launch_presented(old, readiness));
            app.state.host_error = None;
            app.state.settings_open = false;
            app.rendering_suspended = false;
            app.state.requests.retain(|request| request.id != 999);
            app.set_host_attention(false);
            app.set_workspace_attention(false);
            assert!(app.launch_state(LAUNCH_READY).visible);
        }
    }

    #[test]
    fn notices_raised_under_the_launch_cover_wait_for_its_reveal() {
        let mut app = launching();
        app.notify_window("Workspace storage was restored");
        let id = app.last_notice_id();
        assert!(app.state.notice.is_none());
        assert!(app.update_notice_text(id, "Restored".into()));
        app.changed(regions::HOST, false);
        assert!(app.state.notice.is_none(), "a covered launch keeps the notice for its reveal");
        let frame = app.launch_state(LAUNCH_READY).frame.unwrap();
        assert!(app.launch_presented(frame, LAUNCH_READY));
        assert_eq!(app.state.notice.as_ref().map(|notice| (notice.id, notice.text.as_str())), Some((id, "Restored")));
        app.notify("Visible after launch");
        assert_eq!(app.state.notice.as_ref().map(|notice| notice.id), Some(app.last_notice_id()));
    }

    #[test]
    fn a_blocked_launch_shows_held_notices_with_the_workspace() {
        let mut app = launching();
        app.notify_window("Workspace storage was restored");
        let id = app.last_notice_id();
        assert!(app.state.notice.is_none());
        let change = app.set_workspace_attention(true);
        assert_ne!(change.regions & regions::HOST, 0);
        assert!(!app.launch_state(LAUNCH_READY).visible);
        assert_eq!(app.state.notice.as_ref().map(|notice| notice.id), Some(id));
        assert_eq!(app.set_workspace_attention(true).regions, 0);
        app.set_workspace_attention(false);
        app.notify("Raised while a host dialog is open");
        assert!(app.state.notice.is_none());
        assert_ne!(app.set_host_attention(true).regions & regions::HOST, 0);
        assert_eq!(app.state.notice.as_ref().map(|notice| notice.text.as_str()), Some("Raised while a host dialog is open"));
        assert!(app.set_host_attention(false).regions != 0 && app.launch_state(LAUNCH_READY).visible);
        let frame = app.launch_state(LAUNCH_READY).frame.unwrap();
        assert!(app.launch_presented(frame, LAUNCH_READY));
        assert_eq!(app.set_host_attention(true).regions, 0, "a revealed window republishes nothing for later dialogs");
    }

    #[test]
    fn launch_marks_match_the_shared_capy_artwork() {
        let icons = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/layer-web/icons");
        for (icon, _) in crate::ZenIcon::CHOICES {
            let svg = std::fs::read_to_string(icons.join(format!("layer-{}-symbolic.svg", icon.icon()))).unwrap();
            let view = svg.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap()
                .split(' ').map(|value| value.parse::<f32>().unwrap()).collect::<Vec<_>>();
            let ink = icon.launch_ink();
            assert_eq!([view[1], view[3]], [ink.view_top, ink.view_size], "{icon:?}");
            assert!(ink.view_top < ink.bottom && ink.bottom <= ink.view_top + ink.view_size, "{icon:?}");
        }
    }

    #[test]
    fn launch_ignores_background_settings_save_requests() {
        let mut app = launching();
        let settings = Box::new(app.state.settings.clone());
        app.state.requests.push(HostRequest { id: 999, kind: HostRequestKind::SaveSettings { settings } });
        let ready = app.launch_state(LAUNCH_READY);
        assert!(ready.visible && ready.frame.is_some());
    }

    #[test]
    fn launch_finishes_once_per_window_across_switches_and_restarts() {
        let mut app = launching();
        let frame = app.launch_state(LAUNCH_READY).frame.unwrap();
        assert!(app.launch_presented(frame, LAUNCH_READY));
        assert!(!app.launch_presented(frame, LAUNCH_READY));
        app.replace_renderer(Recorder::default()).unwrap();
        assert!(app.launch_state(LaunchReadiness::default()).finished);
        let mut incoming = launching();
        incoming.inherit_window_state(&app).unwrap();
        let inherited = incoming.launch_state(LaunchReadiness::default());
        assert!(inherited.finished && !inherited.visible && inherited.frame.is_none());
        let new_window = launching();
        assert!(new_window.launch_state(LaunchReadiness::default()).visible);
    }

    fn sources() -> Recorder {
        Recorder { tiled_sources: true, ..Default::default() }
    }

    fn source(pixel: [u8; 4]) -> layer_core::color::source::SourceImage {
        std::sync::Arc::unwrap_or_clone(layer_core::color::source::rgba8_source([1, 1], |_, _| pixel))
    }

    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn an_unread_package_failure_after_a_picture_preserves_roots_and_reports_damage() {
        use layer_core::{SourceTarget,raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey,TILE_SIZE},package::{ImmutableBacking,codec::{self,OpenOutcome}}};
        use std::sync::{Arc,atomic::AtomicBool};
        for malformed in [false,true] {
            let mut document=Document::new(layer_core::PortableId::random(),2*TILE_SIZE,TILE_SIZE,
                layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
            let descriptor=document.composition().color.paint_descriptor();
            let samples=[12,34,56,255].repeat((TILE_SIZE*TILE_SIZE) as usize);
            let good=Arc::new(TileBlob::encode(descriptor,&samples).unwrap());
            let bad=Arc::new(if malformed {TileBlob::from_verified_resource(layer_core::PortableId::random(),descriptor,Arc::from([0u8])).unwrap()}
                else {TileBlob::encode(descriptor,&[78,90,123,255].repeat((TILE_SIZE*TILE_SIZE) as usize)).unwrap()});
            let key=|x|TileKey {plane:RasterPlane::Color,coordinate:[x,0]};
            let SourceTarget::Paint(paint)=document.working.target.unwrap() else {unreachable!()};
            document.artwork.paint.get_mut(paint).unwrap().raster=RasterRevision::backed(RasterData {
                tiles:[(key(0),RasterTile::backed_shared(good)),(key(1),RasterTile::backed_shared(bad.clone()))].into(),..Default::default()
            });
            let capture=layer_core::Editor::new(document).capture(0,Default::default()).unwrap();
            let mut bytes=package_bytes(&capture);
            if !malformed {
                let encoded=bad.memory_compressed().unwrap().unwrap();
                let positions=bytes.windows(encoded.len()).enumerate().filter_map(|(at,bytes)|(bytes==&*encoded).then_some(at)).collect::<Vec<_>>();
                assert_eq!(positions.len(),1);bytes[positions[0]+encoded.len()/2]^=1;
            }
            let bytes:Arc<[u8]>=bytes.into();let backing=ImmutableBacking::new(Arc::new(bytes.clone())).unwrap();
            let cancelled=AtomicBool::new(false);
            let OpenOutcome::Candidate {artwork,memory,..}=codec::open(backing.clone(),Default::default(),&cancelled).unwrap() else {panic!("lazy package must open before its unread tile is inspected")};
            let document=Document::from_artwork(artwork).unwrap().with_memory(memory);
            let raster=document.artwork.paint.iter().find(|(_,_,paint)|!paint.raster.is_empty()).unwrap().2.raster.wait_data().unwrap();
            let good=raster.tiles[&key(0)].wait_backing().unwrap();let bad=raster.tiles[&key(1)].wait_backing().unwrap();
            assert_eq!(good.resident_bytes(),0);assert_eq!(bad.resident_bytes(),0);
            assert_eq!(good.decode_on_worker().unwrap(),samples);
            let mut drawing=UiSession::new(Recorder {showing_drawing:true,..sources()},document,[256,256],Platform::Gtk).unwrap();
            drawing.frame(0,0).unwrap();assert!(drawing.engine.backend().composites>0);assert!(drawing.engine.showing_drawing());
            assert!(drawing.engine.document().memory().package_failure().is_none());assert!(notice_text(&drawing).is_none());
            let before=drawing.engine.document().clone();let checkpoint=drawing.engine.checkpoint();
            let unread=bad.clone();let failure=std::thread::spawn(move ||unread.decode_on_worker()).join().unwrap().unwrap_err();
            if !malformed {assert!(failure.contains("checksum"),"{failure}");}
            assert!(bad.poll_compressed().is_err());assert!(backing.poll(0,1).is_err());
            drawing.suspend_renderer().unwrap();
            assert!(drawing.rendering_suspended());
            assert_eq!(notice_text(&drawing),Some(drawing.localization().text(MessageId::DOCUMENTS_PACKAGE_DAMAGED_BODY).as_ref()));
            assert_live_artwork_eq(drawing.engine.document(),&before);assert_eq!(drawing.engine.checkpoint(),checkpoint);
            let retained=drawing.engine.document().artwork.paint.iter().find(|(_,_,paint)|!paint.raster.is_empty()).unwrap().2.raster.wait_data().unwrap();
            assert!(Arc::ptr_eq(&retained.tiles[&key(0)].wait_backing().unwrap(),&good));
            assert!(Arc::ptr_eq(&retained.tiles[&key(1)].wait_backing().unwrap(),&bad));
            assert_eq!(backing.byte_len(),bytes.len() as u64);assert_eq!(bad.resident_bytes(),0);
            let capture=layer_core::Editor::new(before).capture(0,Default::default()).unwrap();
            assert!(codec::PreparedPackage::prepare(&capture,None,&cancelled).is_err());
        }
    }

    #[test]
    fn a_released_queued_stroke_allows_switch_preparation_but_not_parking() {
        for settling in [false,true] {
            let mut drawing=session(Platform::Gtk);drawing.frame(0,0).unwrap();
            drawing.engine.backend_mut().settling=settling;
            pen_at(&mut drawing,1,PenPhase::Down,[40.,40.]);
            assert!(drawing.prepare_document_switch().is_err());
            pen_at(&mut drawing,2,PenPhase::Up,[80.,40.]);
            assert!(drawing.prepare_document_switch().is_ok(),"released input must not depend on an unrelated submission: {settling}");
            assert!(!drawing.can_park_document());assert_eq!(drawing.engine.metrics().committed_strokes,0);
            drawing.engine.backend_mut().settling=false;
            for frame in 1..=8 {drawing.frame(frame*8_000_000,frame*8_000_000).unwrap();}
            assert_eq!(drawing.engine.metrics().committed_strokes,1);
            drawing.state.requests.clear();
            assert!(drawing.can_park_document());
        }
    }
    #[test]
    fn parking_waits_for_an_open_crop_to_finish() {
        let mut drawing = session(Platform::Gtk);
        drawing.frame(0, 0).unwrap();
        assert!(drawing.can_park_document());
        invoke(&mut drawing, CommandId::Crop);
        assert!(!drawing.can_park_document());
        assert!(drawing.park_document().is_err());
        invoke(&mut drawing, CommandId::CancelTransform);
        assert!(drawing.can_park_document());
    }

    #[test]
    fn closing_can_park_while_private_filter_validation_blocks_switching() {
        let mut drawing = session(Platform::Gtk);
        drawing.frame(0, 0).unwrap();
        let package = package_json(drawing.effect_catalog.categories().to_vec(), vec![custom_filter("unsharp_mask")]);
        drawing.load_effect_package(&package, |_| panic!("inline package"), layer_core::EffectInstallMode::Merge).unwrap();
        assert!(drawing.pending_filters.is_some());
        assert!(!drawing.can_switch_document());
        assert!(!drawing.can_park_document());
        drawing.request_session_close().unwrap();
        assert!(!drawing.can_switch_document());
        assert!(drawing.can_park_document());
        drawing.park_document().unwrap();
        assert!(drawing.rendering_suspended());
    }

    #[test]
    fn reactivation_keeps_the_drawing_camera_exactly_until_the_window_changes() {
        let mut drawing = session(Platform::Gtk);
        drawing.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
        drawing.dispatch(UiAction::SetRotation { rotation: 0.47 }).unwrap();
        invoke(&mut drawing, CommandId::ZoomIn);
        let mut other = session(Platform::Gtk);
        other.set_viewport([1600., 1000.], [1600, 1000]).unwrap();
        let camera = drawing.state.camera.clone();
        for _ in 0..3 {
            other.inherit_window_state(&drawing).unwrap();
            drawing.inherit_window_state(&other).unwrap();
            assert!(drawing.state.camera.revision > camera.revision);
            assert_eq!(Camera { revision: camera.revision, ..drawing.state.camera.clone() }, camera);
        }
        let center = |s: &UiSession<Recorder>| s.state.camera.input_transform().map(layer_core::Point {
            x: s.state.camera.work_area_center()[0], y: s.state.camera.work_area_center()[1],
        });
        let before = center(&drawing);
        other.set_viewport([1200., 900.], [1200, 900]).unwrap();
        drawing.inherit_window_state(&other).unwrap();
        assert_eq!(drawing.state.camera.viewport, [1200, 900]);
        let after = center(&drawing);
        assert!((after.x - before.x).abs() < 1e-3 && (after.y - before.y).abs() < 1e-3, "{before:?} {after:?}");
    }

    #[test]
    fn thumbnail_requests_lost_to_a_replaced_renderer_are_retried_in_order_once() {
        let mut session = UiSession::blank(sources(), [800, 600], Platform::Web).unwrap();
        let target = |id| layer_render::ThumbnailTarget::Occurrence(occurrence_handle(id).unwrap());
        let ink = session.state().layers[0].id;
        let mut requests = ThumbnailRequests::default();
        assert_eq!(requests.retry(&session), None);
        for (request, retry) in [(7, false), (8, false), (9, false)] { requests.submitted(request, target(ink), retry); }
        requests.completed(8);
        assert_eq!(requests.retry(&session), None, "the same renderer still owns its requests");
        session.replace_renderer(sources()).unwrap();
        assert_eq!(requests.retry(&session), Some((7, target(ink))));
        requests.submitted(7, target(ink), true);
        assert_eq!(requests.retry(&session), Some((9, target(ink))));
        requests.refused(&session, true);
        assert_eq!(requests.retry(&session), Some((9, target(ink))), "an unavailable renderer keeps the request queued");
        requests.refused(&session, false);
        assert_eq!(requests.retry(&session), None, "each lost request is retried once");
        session.replace_renderer(sources()).unwrap();
        assert_eq!(requests.retry(&session), Some((7, target(ink))), "a resubmitted request is retried again after another loss");
        requests.submitted(7, target(ink), true);
        requests.completed(7);
        session.replace_renderer(sources()).unwrap();
        assert_eq!(requests.retry(&session), None, "completed requests are not retried");
        requests.submitted(10, target(ink), false);
        session.state.document_file.epoch += 1;
        session.replace_renderer(sources()).unwrap();
        assert_eq!(requests.retry(&session), None, "another drawing discards the old requests");
    }

    #[test]
    fn header_layout_remains_publishable_after_final_document_retirement() {
        let mut session = UiSession::blank(Recorder::default(), [800, 600], Platform::Android).unwrap();
        session.frame(0, 0).unwrap();
        session.dispatch(UiAction::Invoke { command: CommandId::CloseDocument }).unwrap();
        assert!(session.state().document_file.close_ready);
        session.park_document().unwrap();
        let document = session.engine().document().clone();
        session.dispatch(UiAction::MeasureHeader { height: 48., items: vec![] }).unwrap();
        assert_eq!(session.state().workspace.layout.header_presentation.height, 48.);
        assert_eq!(session.engine().document(), &document);
        assert!(session.state().document_file.close_ready);
        assert!(session.rendering_suspended());
        assert!(session.dispatch(UiAction::Invoke { command: CommandId::AddLayer }).is_err());
    }

    #[test]
    fn stroke_recording_follows_the_window_when_switching_drawings() {
        let mut active = UiSession::blank(Recorder::default(), [800, 600], Platform::Gtk).unwrap();
        let mut parked = UiSession::blank(Recorder::default(), [800, 600], Platform::Gtk).unwrap();
        active.stroke_recording().start("test").unwrap();
        let event = layer_engine::PenEvent {
            device_id: 1,
            sequence: 1,
            timestamp_ns: 1,
            view_revision: 0,
            surface_position: layer_core::Point { x: 5., y: 6. },
            pressure: 0.,
            tilt_radians: [0.; 2],
            twist_radians: 0.,
            distance: 0.2,
            phase: layer_engine::PenPhase::Hover,
            tool: layer_engine::ToolKind::Pen,
            flags: layer_engine::SampleFlags::NONE,
        };
        active.cursor_input(Some(event));
        parked.inherit_window_state(&active).unwrap();
        assert!(parked.stroke_recording().status().recording);
        drop(active);
        parked.cursor_input(Some(layer_engine::PenEvent {
            sequence: 2,
            timestamp_ns: 2,
            ..event
        }));
        assert_eq!(parked.stroke_recording().status().raw_events, 2);
        parked
            .stroke_recording()
            .stop(layer_engine::recording::StopReason::Manual);
        let data = parked.stroke_recording().bytes().unwrap();
        assert_eq!(
            layer_engine::recording::read(data.as_slice())
                .unwrap()
                .iter()
                .filter(|r| matches!(r, layer_engine::recording::Record::Raw { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn drawing_activation_keeps_native_dialog_request_ids_monotonic() {
        let mut active = UiSession::blank(Recorder::default(), [800, 600], Platform::Gtk).unwrap();
        let mut parked = UiSession::blank(Recorder::default(), [800, 600], Platform::Gtk).unwrap();
        active.set_platform(Platform::Windows);
        let mut last = 0;
        for _ in 0..3 {
            active.dispatch(UiAction::Invoke { command: CommandId::OpenDocument }).unwrap();
            last = active.state.requests.last().unwrap().id;
            active.complete_document_request(last, Ok(false)).unwrap();
        }
        parked.inherit_window_state(&active).unwrap();
        parked.dispatch(UiAction::Invoke { command: CommandId::OpenDocument }).unwrap();
        assert!(parked.state.requests.last().unwrap().id > last);
    }

    #[test]
    fn parked_editor_inherits_window_viewport_without_losing_its_history() {
        let mut active = UiSession::blank(Recorder::default(), [800, 600], Platform::Gtk).unwrap();
        let mut parked = UiSession::blank(Recorder::default(), [800, 600], Platform::Gtk).unwrap();
        let layers = parked.engine().document().scene().order().len();
        parked.dispatch(UiAction::Invoke { command: CommandId::AddLayer }).unwrap();
        let revision = parked.engine().document().revision;
        active.set_viewport([1000., 700.], [2000, 1400]).unwrap();
        parked.inherit_window_state(&active).unwrap();
        assert_eq!(parked.state().camera.viewport, [2000, 1400]);
        assert_eq!(parked.logical_viewport, Some([1000., 700.]));
        assert_eq!(parked.engine().document().revision, revision);
        parked.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        assert_eq!(parked.engine().document().scene().order().len(), layers);
    }

    #[test]
    fn retired_renderer_analysis_does_not_block_drawing_activation_or_replacement() {
        for failed in [false, true] {
            let mut active = UiSession::blank(Recorder::default(), [800, 600], Platform::Gtk).unwrap();
            let mut parked = UiSession::blank(Recorder::default(), [800, 600], Platform::Gtk).unwrap();
            active.dispatch(UiAction::SetTheme { theme: Some(Theme::Dark) }).unwrap();
            let layers = parked.engine().document().scene().order().len();
            parked.dispatch(UiAction::Invoke { command: CommandId::AddLayer }).unwrap();
            parked.frame(0, 0).unwrap();
            let document = parked.engine().document().clone();
            let checkpoint = parked.engine().checkpoint();
            if failed { parked.suspend_renderer().unwrap(); }
            else { parked.park_document().unwrap(); }
            parked.renderer_mut().analysis_retain_fails = true;
            let retained = parked.engine().backend().analysis_retained.len();
            let cancelled = parked.engine().backend().analysis_cancels;
            parked.inherit_window_state(&active).unwrap();
            assert_eq!(parked.engine().backend().analysis_retained.len(), retained);
            assert!(parked.engine().backend().analysis_cancels > cancelled);
            assert!(parked.rendering_suspended());
            assert!(!parked.effect_analyses.busy());
            assert_eq!(parked.state().settings, active.state().settings);
            assert_eq!(parked.capture_workspace().unwrap(), active.capture_workspace().unwrap());
            let (previous, change) = parked.replace_renderer(Recorder::default()).unwrap();
            assert_eq!(previous.analysis_retained.len(), retained);
            assert!(change.canvas_wake);
            assert!(!parked.rendering_suspended());
            assert_eq!(parked.engine().document(), &document);
            assert_eq!(parked.engine().checkpoint(), checkpoint);
            parked.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
            assert_eq!(parked.engine().document().scene().order().len(), layers);
            parked.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
            assert_live_artwork_eq(parked.engine().document(), &document);
            parked.frame(1, 1).unwrap();
        }
    }

    #[test]
    fn parked_fixed_image_objects_keep_shared_owners_and_private_redo_history() {
        use layer_core::{Affine64, PortableId, ProjectLimits, package::{ImmutableBacking, codec::{self, OpenOutcome}}};
        use crate::session_recovery::SessionRestore;
        use std::sync::{Arc, atomic::AtomicBool};
        let cancel = AtomicBool::new(false);
        let backing = |bytes: &[u8]| ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
        let bytes = include_bytes!("../../layer-core/src/package/codec/fixtures/shared-image-objects.capy");
        let OpenOutcome::Candidate { artwork, .. } = codec::open(backing(bytes), Default::default(), &cancel).unwrap() else { panic!("Fixed objects must be editable") };
        let document = Document::from_artwork(artwork).unwrap();
        let renderer = || Recorder {color:document.composition().color, ..sources()};
        let identity = |value:u128| PortableId::from_bytes(value.to_be_bytes());
        let object = document.artwork.objects.resolve(identity(9)).unwrap();
        let original = document.artwork.objects.get(object).unwrap().clone();
        let changed_affine = Affine64([1., 0.25, -0.5, 1., -7.125, 3.0000000000000004]);
        let check = |document: &Document, expected: &layer_core::ImageObject| {
            let first = document.artwork.objects.get(document.artwork.objects.resolve(identity(9)).unwrap()).unwrap();
            let second = document.artwork.objects.get(document.artwork.objects.resolve(identity(10)).unwrap()).unwrap();
            assert_eq!(first, expected);
            assert_eq!(first.affine.0.map(f64::to_bits), expected.affine.0.map(f64::to_bits));
            assert_eq!(first.image.id(), identity(11));
            assert!(first.image.same_owner(&second.image));
            let second_handle = document.artwork.objects.resolve(identity(10)).unwrap();
            let second_owner = document.scene().object_owner(second_handle).unwrap();
            assert!(!document.scene().occurrence(second_owner).unwrap().visible);
            assert_eq!(second.interpolation, layer_core::ImageInterpolation::Nearest);
            let source = document.artwork.paint.get(document.artwork.paint.resolve(identity(7)).unwrap()).unwrap();
            assert!(first.image.same_owner(&source.base.as_ref().unwrap().image));
        };
        for failed in [false, true] {
            let mut parked = UiSession::from_project(renderer(), document.clone(), None, [500, 400], Platform::Gtk).unwrap();
            parked.frame(0, 0).unwrap();
            let edit = parked.engine.document().set_image_object_affine_edit(object, changed_affine).unwrap();
            parked.engine.apply_edit(edit).unwrap();
            let changed = parked.engine.document().artwork.objects.get(object).unwrap().clone();
            invoke(&mut parked, CommandId::Undo);
            parked.frame(1, 1).unwrap();
            check(parked.engine.document(), &original);
            if failed { parked.suspend_renderer().unwrap(); }
            else { assert!(parked.park_document().unwrap().try_blobs().unwrap().is_some()); }
            let stamp = parked.session_stamp();
            let path = std::env::temp_dir().join(format!("capy-ui-parked-{}", PortableId::random()));
            let mut store = pollster::block_on(layer_core::package::session_store::SessionStore::open(&path)).unwrap();
            pollster::block_on(parked.capture_session().unwrap().checkpoint(&mut store, &cancel)).unwrap();
            let restored = SessionRestore::from_core(pollster::block_on(store.load(ProjectLimits::default(), &cancel)).unwrap().unwrap()).unwrap();
            check(restored.document(), &original);
            let mut recovered = UiSession::from_project(renderer(), restored.document().clone(), None, [500, 400], Platform::Gtk).unwrap();
            recovered.frame(0, 0).unwrap();
            recovered.restore_session(restored, false, None).unwrap();
            invoke(&mut recovered, CommandId::Redo);
            check(recovered.engine.document(), &changed);
            invoke(&mut recovered, CommandId::Undo);
            check(recovered.engine.document(), &original);
            parked.replace_renderer(renderer()).unwrap();
            assert_eq!(parked.session_stamp(), stamp);
            check(parked.engine.document(), &original);
            invoke(&mut parked, CommandId::Redo);
            check(parked.engine.document(), &changed);
            invoke(&mut parked, CommandId::Undo);
            check(parked.engine.document(), &original);
            drop((recovered, parked, store));
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn replacement_retains_sources_undo_redo_workspace_and_pending_save() {
        let mut s = UiSession::new(
            sources(),
            Document::new(layer_core::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
            [256, 256],
            Platform::Windows,
        )
        .unwrap();
        s.import_layer_source("Imported source", source([180, 20, 75, 128])).unwrap();
        s.frame(0, 0).unwrap();
        let imported = s.engine.document().clone();
        invoke(&mut s, CommandId::Undo);
        s.frame(1, 1).unwrap();
        let document = s.engine.document().clone();
        let workspace = s.capture_workspace().unwrap();
        let camera = s.state.camera.clone();
        let checkpoint = s.engine.checkpoint();
        s.suspend_renderer().unwrap();
        assert!(s.rendering_suspended());
        assert!(s.capture_artwork().is_ok());
        assert!(!s.command(CommandId::Redo).enabled);
        invoke(&mut s, CommandId::SaveDocument);
        let request = s.files.pending.as_ref().unwrap().0;
        let (_, change) = s.replace_renderer(sources()).unwrap();
        assert!(change.canvas_wake);
        assert!(!s.rendering_suspended());
        assert_eq!(s.engine.document(), &document);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.capture_workspace().unwrap(), workspace);
        assert_eq!(s.state.camera, camera);
        assert_eq!(s.files.pending.as_ref().unwrap().0, request);
        s.complete_document_request(request, Ok(false)).unwrap();
        invoke(&mut s, CommandId::Redo);
        s.frame(2, 2).unwrap();
        assert_live_artwork_eq(s.engine.document(), &imported);
    }

    #[test]
    fn replacement_clears_gpu_waits_and_resumes_filter_validation() {
        let mut s = UiSession::new(
            Recorder::default(),
            Document::new(layer_core::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
            [128, 128],
            Platform::Gtk,
        )
        .unwrap();
        s.frame(0, 0).unwrap();
        s.eyedropper
            .queue(layer_render::ColorSampleSource::Composite, [10, 10]);
        s.eyedropper.poll(s.engine.backend_mut(), layer_core::color::RgbSpace::Srgb).unwrap();
        assert!(s.eyedropper.busy());
        let original = s.effect_catalog.clone();
        stage_candidate_library(&mut s, &original, "Recovered candidate", "resolved sources");
        assert!(s.state.filter_load.pending);
        let original_validation = s.engine.backend().validation.as_ref().unwrap().clone();
        assert_eq!(original_validation.programs.len(), 1);
        s.replace_renderer(Recorder::default()).unwrap();
        assert!(s.state.filter_load.pending);
        let resumed = s.engine.backend().validation.as_ref().unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &resumed.programs[0],
            &original_validation.programs[0]
        ));
        let request = resumed.request_id;
        assert_eq!(request, s.state.filter_load.request_id);
        s.renderer_mut().validation_result = Some(layer_render::EffectValidationResult {
            request_id: request,
            result: Ok(()),
        });
        s.frame(1, 1).unwrap();
        assert!(
            s.state.filter_load.pending,
            "publication waits for reconstruction replay"
        );
        assert_eq!(s.effect_catalog.filters(), original.filters());
        s.frame(2, 2).unwrap();
        assert!(!s.state.filter_load.pending);
        assert!(s.state.filter_load.error.is_none());
        assert_eq!(
            s.effect_catalog
                .get(&original_validation.programs[0].id)
                .unwrap()
                .program
                .label,
            layer_core::ResourceLabel::from("Recovered candidate")
        );
        assert!(!s.eyedropper.busy());
        s.eyedropper
            .queue(layer_render::ColorSampleSource::Composite, [10, 10]);
        s.eyedropper.poll(s.engine.backend_mut(), layer_core::color::RgbSpace::Srgb).unwrap();
        assert_eq!(s.engine.backend().sample_requests.len(), 1);
    }

    #[test]
    fn suspension_cancels_transform_and_filter_candidate_without_changing_sources() {
        let mut s = UiSession::new(
            sources(),
            Document::new(layer_core::PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
            [128, 128],
            Platform::Windows,
        )
        .unwrap();
        s.import_layer_source("Source", source([20, 30, 40, 255])).unwrap();
        s.frame(0, 0).unwrap();
        let document = s.engine.document().clone();
        let imported = document.working.occurrence.unwrap();
        let catalog = s.effect_catalog.clone();
        stage_candidate_library(&mut s, &catalog, "Unpublished candidate", "owned sources");
        invoke(&mut s, CommandId::ScaleRotate);
        assert!(s.operation.active());
        assert!(s.state.filter_load.pending);
        s.suspend_renderer().unwrap();
        assert!(!s.operation.active());
        assert!(!s.state.filter_load.pending);
        assert!(s.state.filter_load.error.is_some());
        assert_eq!(s.effect_catalog.filters(), catalog.filters());
        assert_eq!(s.engine.document(), &document);
        let recovered = s.capture_artwork().unwrap();
        let paint = match document.scene().occurrence(imported).unwrap().content { OccurrenceContent::Paint(paint) => paint, _ => unreachable!() };
        let retained = document.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage();
        let recovered = recovered.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage();
        assert!(std::sync::Arc::ptr_eq(recovered, retained));
        assert!(s.command(CommandId::SaveDocumentAs).enabled);
        assert!(
            s.dispatch(UiAction::Color {
                action: ColorAction::Shape {
                    shape: ColorShape::Triangle
                }
            })
            .is_err()
        );
        assert_eq!(s.engine.document(), &document);
    }
}
