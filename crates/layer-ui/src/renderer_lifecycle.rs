//! Renderer replacement keeps CPU document/session ownership in place.
use super::*;

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
    pub fn rendering_diagnostics(&self) -> serde_json::Value {
        let telemetry = self.engine.backend().telemetry();
        serde_json::json!({"canvas": self.engine.document().composition().size,
            "color": self.engine.document().composition().color,
            "revision": self.engine.document().revision, "epoch": self.state.document_file.epoch,
            "tool": self.layer_interaction.tool, "transform_open": self.operation.transforming(),
            "transform_pixels_pending": self.content_bounds.baking(),
            "transform_selection_pending": self.region_tools.applying_transform(),
            "transform_tool_requested": self.operation.next_tool.is_some(),
            "raster_backing_pending": self.engine.raster_backing_pending(),
            "suspended": self.rendering_suspended, "tracked_renderer_bytes": telemetry.resident_bytes,
            "submitted_frames": telemetry.submissions})
    }
    /// The host has no modal work and must wait for this boundary before normal
    /// parking. Failed renderers remain navigable/saveable/closeable.
    pub(super) fn document_park_interaction_idle(&self)->bool {
        self.require_workspace_idle().is_ok() && !self.workspace_transition && !self.state.customization.header_editing
            && self.deferred_edits.is_empty() && self.state.requests.is_empty() && !self.state.document_file.busy
    }
    pub fn can_park_document(&self) -> bool {
        (self.rendering_suspended || (self.require_workspace_idle().is_ok()
            && (if self.state.document_file.close_ready { self.require_document_snapshot_idle() } else { self.require_document_idle() }).is_ok()
            && self.engine.can_park()))
            && !self.workspace_transition && !self.state.customization.header_editing
            && self.deferred_edits.is_empty() && self.state.requests.is_empty() && !self.state.document_file.busy
    }

    pub(super) fn discard_render_requests(&mut self) {
        self.operation.next_tool = None;
        self.cancel_content_bounds();
        self.content_bounds = Default::default();
        self.resubmit_conversion();
        self.cancel_picker();
        self.eyedropper.renderer_replaced();
        self.cancel_tonal();
        self.region_tools.renderer_replaced();
        self.painted_selections.renderer_replaced();
        self.deferred_edits.clear();
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
        self.return_to_artwork()?;
        self.release_idle_document_buffers();
        self.cancel_auto_levels();self.cancel_histogram();self.cancel_effect_analyses();
        self.rendering_suspended = true;
        self.refresh_commands();
        Ok(tiles)
    }

    /// Present constant fills before restoring the document's committed raster.
    pub fn submit_backdrop_frame(&mut self) -> Result<(), String> {
        let view = self.state.camera.view();
        let document = self.engine.document();
        let scene = document.scene();
        let backdrop = scene.constant_backdrop();
        let snapshot = document.snapshot().as_ref().clone().with_scope(layer_core::authored::SceneScope::Members(backdrop.to_vec().into()));
        self.engine.backend_mut().submit(layer_render::FramePacket {
            commit_rasters: true,
            time_seconds: 0.,
            view,
            scene: snapshot.view(),
            inspect_mask: None,
            selection_overlays: None,
            document_extent: snapshot.view().composition().size,
            blend_space: snapshot.view().composition().blend,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: true,
            composite_all: true,
        }).map_err(error)
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
        let navigation=self.camera_navigation_revision();
        let camera=(!self.initial_fit).then(||super::session_recovery::SessionCamera::capture(&self.state.camera));
        self.set_localization(previous.localization().clone());
        self.localization_generation = previous.localization_generation;
        self.preferences_revision = self.preferences_revision.max(previous.preferences_revision);
        // A recording is window state, shared by parked and active documents.
        self.engine.recording = previous.engine.recording.clone();
        self.state.platform = previous.state.platform;
        self.platform_prediction_available = previous.platform_prediction_available;
        self.interaction.touch_policy = previous.interaction.touch_policy;
        self.system_theme = previous.system_theme;
        self.system_accent = previous.system_accent;
        self.state.workspace = previous.state.workspace.clone();
        self.state.tool_slots = previous.state.tool_slots.clone();
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
        self.apply_settings(previous.state.settings.clone())?;
        // View orientation/zoom belongs to this drawing; display size and scale
        // belong to the window and may have changed while this editor slept.
        if let Some(logical) = previous.logical_viewport {
            self.set_viewport(logical, previous.state.camera.viewport)?;
        }
        self.sync_work_area();
        if let Some(camera)=camera {self.state.camera=camera.restore(&self.state.camera)?;}
        // A window can still have queued native input from the previous tab.
        // Preserve each drawing's camera while retiring that input generation.
        self.state.camera.revision = self.state.camera.revision.max(previous.state.camera.revision)
            .checked_add(1).ok_or("Camera generation exhausted")?;
        self.automatic_camera_revision=self.state.camera.revision.checked_sub(navigation).ok_or("Camera revision moved backwards")?;
        self.sync_camera();
        self.refresh_layer_presentation();
        self.refresh_commands();
        Ok(self.changed(regions::ALL, true))
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
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS, true))
    }

    pub(super) fn command_without_renderer(command: CommandId) -> bool {
        matches!(
            command,
            CommandId::CancelTransform
                | CommandId::SaveDocument
                | CommandId::SaveDocumentAs
                | CommandId::CloseDocument
                | CommandId::Settings
                | CommandId::KeyboardShortcuts
                | CommandId::ToggleTheme
                | CommandId::Fullscreen
                | CommandId::NewWindow
                | CommandId::Drawings
                | CommandId::NextDrawing
                | CommandId::PreviousDrawing
                | CommandId::About
                | CommandId::Website
                | CommandId::SourceCode
        )
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
        let recovered = self.engine.recover_failed_rasters().map_err(error);
        self.refresh_document();
        self.poll_document_close();
        self.refresh_commands();
        if recovered? > 0 || interrupted_selection {
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
        self.cancel_effect_analyses();
        let previous = self.engine.replace_backend(renderer).map_err(error)?;
        self.renderer_generation = self.renderer_generation.wrapping_add(1);
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
        let change = self.changed(regions::DOCUMENT | regions::BRUSH | regions::COMMANDS, true);
        Ok((previous, change))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::*;

    fn sources() -> Recorder {
        Recorder { tiled_sources: true, ..Default::default() }
    }

    fn source(pixel: [u8; 4]) -> layer_core::color::source::SourceImage {
        std::sync::Arc::unwrap_or_clone(layer_core::color::source::rgba8_source([1, 1], |_, _| pixel))
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
            let mut archive = Vec::new();
            parked.capture_session().unwrap().prepare(&cancel).unwrap().write(&mut archive, &cancel).unwrap();
            let restored = SessionRestore::open(backing(&archive), ProjectLimits::default(), &cancel).unwrap();
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
