use super::*;
use layer_core::{AssetId, Point};
use layer_engine::{SampleFlags, ToolKind};
use layer_render::{BackendError, FilterPreviewImage, FilterPreviewRequest, FramePacket, HostImage};

/// Protocol recorder only: no canvas storage or software rasterization.
#[derive(Default)]
pub(crate) struct Recorder {
    pub(crate) settling: bool,
    pub(crate) color: layer_core::color::DocumentColor,
    pub(crate) prepared_color: Option<layer_core::color::DocumentColor>,
    pub(crate) tiled_sources: bool,
    pub(crate) telemetry_enabled: bool,
    pub(crate) pending_operations: Vec<(layer_core::LayerId, layer_core::LayerOperation)>,
    pub(crate) last_style: Option<layer_render::DabStyle>,
    pub(crate) recorded_dabs: Vec<layer_render::Dab>,
    pub(crate) dabs: usize,
    pub(crate) composites: usize,
    pub(crate) validation: Option<layer_render::EffectValidationRequest>,
    pub(crate) validation_result: Option<layer_render::EffectValidationResult>,
    pub(crate) sample_requests: Vec<layer_render::ColorSampleRequest>,
    pub(crate) sample_reply: Option<layer_render::ColorSample>,
    pub(crate) selection_updates: Vec<layer_render::SelectionPaint>,
    pub(crate) selection_reply: Option<layer_render::SelectionPaintResult>,
    pub(crate) selection_wait: bool,
    pub(crate) region_requests: Vec<layer_render::RegionRequest>,
    pub(crate) region_reply: Option<layer_render::RegionResult>,
    pub(crate) region_fails: bool,
    pub(crate) region_cancels: usize,
    pub(crate) bounds_requests: Vec<layer_core::ContentBoundsRequest>,
    pub(crate) bounds_reply: Option<Result<layer_core::Rect, BackendError>>,
    pub(crate) bounds_cancels: usize,
    pub(crate) bounds_wait: bool,
    pub(crate) snapshot_requests: Vec<layer_render::SnapshotRequest>,
    pub(crate) snapshot_reply: Option<Result<layer_render::SnapshotResult, BackendError>>,
    pub(crate) snapshot_cancels: usize,
    pub(crate) snapshot_wait: bool,
    pub(crate) snapshot_fails: bool,
    pub(crate) transform: Option<layer_render::TransformPreview>,
    pub(crate) moving_layer: Option<layer_core::LayerId>,
    pub(crate) moving_pixels: Option<(layer_core::LayerId, layer_core::Selection)>,
    pub(crate) overlay: Option<layer_render::SelectionOverlay>,
    pub(crate) filter_preview: Option<FilterPreviewRequest>,
    pub(crate) filter_preview_ready: Option<Result<FilterPreviewImage, BackendError>>,
    pub(crate) filter_preview_requests: usize,
    pub(crate) filter_preview_takes: usize,
    pub(crate) filter_preview_cancels: usize,
    pub(crate) reject_filter_previews: bool,
    pub(crate) max_dimension: Option<u32>,
    pub(crate) crop_overlay: Option<layer_render::CropOverlay>,
    pub(crate) frame_layers: Vec<layer_core::Layer>,
}
impl CanvasRenderer for Recorder {
    type Error = BackendError;
    fn has_pending_submission(&self) -> bool { self.settling }
    fn can_submit(&self) -> bool { !self.settling }
    fn set_selection_overlay(&mut self, overlay: Option<layer_render::SelectionOverlay>) {self.overlay=overlay;}
    fn document_color(&self) -> layer_core::color::DocumentColor { self.color }
    fn adopt_prepared_color(&mut self, color: layer_core::color::DocumentColor) -> Result<bool, Self::Error> {
        if self.prepared_color != Some(color) { return Ok(false); }
        self.prepared_color = None;
        self.color = color;
        Ok(true)
    }
    fn supports_tiled_sources(&self) -> bool { self.tiled_sources }
    fn max_document_dimension(&self) -> u32 { self.max_dimension.unwrap_or(u32::MAX) }
    fn set_crop_overlay(&mut self, overlay: Option<layer_render::CropOverlay>) { self.crop_overlay = overlay; }
    fn set_telemetry_enabled(&mut self, enabled: bool) {
        self.telemetry_enabled = enabled;
    }
    fn set_transform_preview(
        &mut self,
        preview: Option<&layer_render::TransformPreview>,
    ) -> Result<(), Self::Error> {
        self.transform = preview.cloned();
        Ok(())
    }
    fn prepare_moving_layer(&mut self, layer: Option<layer_core::LayerId>) {
        self.moving_layer = layer;
    }
    fn prepare_moving_pixels(&mut self, pixels: Option<(layer_core::LayerId, layer_core::Selection)>) {
        self.moving_pixels = pixels;
    }
    fn paint_selection(&mut self,update:&layer_render::SelectionPaint)->Result<bool,Self::Error> {
        self.selection_updates.push(update.clone()); Ok(!self.selection_wait)
    }
    fn take_selection_paint(&mut self)->Option<Result<layer_render::SelectionPaintResult,Self::Error>> {
        self.selection_reply.take().map(Ok)
    }
    fn request_region(
        &mut self,
        request: layer_render::RegionRequest,
    ) -> Result<bool, Self::Error> {
        self.region_requests.push(request);
        Ok(true)
    }
    fn take_region(&mut self) -> Option<Result<layer_render::RegionResult, Self::Error>> {
        let fails = self.region_fails;
        self.region_reply.take().map(|reply| if fails { Err(BackendError("readback failed")) } else { Ok(reply) })
    }
    fn cancel_region(&mut self) {
        self.region_cancels += 1;
    }
    fn request_snapshot(&mut self, request: layer_render::SnapshotRequest) -> Result<bool, Self::Error> {
        if self.snapshot_fails { return Err(BackendError("snapshot request failed")); }
        if self.snapshot_wait { return Ok(false); }
        self.snapshot_requests.push(request);
        Ok(true)
    }
    fn take_snapshot(&mut self) -> Option<Result<layer_render::SnapshotResult, Self::Error>> {
        self.snapshot_reply.take()
    }
    fn cancel_snapshot(&mut self) {
        self.snapshot_cancels += 1;
        self.snapshot_reply = None;
    }
    fn request_content_bounds(&mut self, request: layer_core::ContentBoundsRequest) -> Result<bool, Self::Error> {
        if self.bounds_wait { return Ok(false); }
        self.bounds_requests.push(request);
        Ok(true)
    }
    fn take_content_bounds(&mut self) -> Option<Result<layer_core::Rect, Self::Error>> {
        self.bounds_reply.take()
    }
    fn cancel_content_bounds(&mut self) {
        self.bounds_cancels += 1;
        self.bounds_reply = None;
        self.cancel_snapshot();
    }
    fn request_color_sample(
        &mut self,
        request: layer_render::ColorSampleRequest,
    ) -> Result<bool, Self::Error> {
        self.sample_requests.push(request);
        Ok(true)
    }
    fn take_color_sample(&mut self) -> Option<Result<layer_render::ColorSample, Self::Error>> {
        self.sample_reply.take().map(Ok)
    }
    fn request_effect_validation(
        &mut self,
        request: layer_render::EffectValidationRequest,
    ) -> Result<bool, Self::Error> {
        self.validation = Some(request);
        Ok(true)
    }
    fn take_effect_validation(&mut self) -> Option<layer_render::EffectValidationResult> {
        self.validation_result.take()
    }
    fn request_filter_previews(&mut self, request: FilterPreviewRequest) -> Result<bool, Self::Error> {
        if self.reject_filter_previews {
            return Err(BackendError("preview unavailable"));
        }
        assert!(self.filter_preview.is_none(), "only one request may be in flight");
        self.filter_preview_requests += 1;
        self.filter_preview = Some(request);
        Ok(true)
    }
    fn take_filter_previews(&mut self) -> Option<Result<FilterPreviewImage, Self::Error>> {
        self.filter_preview_takes += 1;
        let ready = self.filter_preview_ready.take()?;
        self.filter_preview = None;
        Some(ready)
    }
    fn cancel_filter_previews(&mut self) {
        self.filter_preview_cancels += 1;
        self.filter_preview = None;
        self.filter_preview_ready = None;
    }
    fn tip_outline(&self, asset: &AssetId) -> Option<&layer_render::TipOutline> {
        static OUTLINE: std::sync::OnceLock<layer_render::TipOutline> =
            std::sync::OnceLock::new();
        (asset == &AssetId::from("cursor-test")).then(|| {
            OUTLINE.get_or_init(|| {
                vec![vec![[-1.0, -0.5], [0.5, -0.5], [-1.0, 0.5], [-1.0, -0.5]]]
            })
        })
    }
    fn tip_mask(&self, asset: &AssetId) -> Option<HostImage<'_>> {
        (asset == &AssetId::from("preview-test")).then_some(HostImage {
            width: 4, height: 4, stride: 4, format: layer_render::PixelFormat::R8Unorm,
            bytes: &[255,255,255,255,255,0,0,255,255,0,0,255,255,255,255,255],
        })
    }
    fn resize_surface(&mut self, _: u32, _: u32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn prepare_asset(&mut self, _: &AssetId, _: HostImage<'_>) -> Result<(), Self::Error> {
        Ok(())
    }
    fn release_asset(&mut self, _: &AssetId) {}
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
        self.frame_layers = packet.layers.to_vec();
        self.pending_operations.clear();
        for batch in packet.dab_batches {
            if let layer_render::DabBatchKind::LayerOperation(index) = batch.kind {
                let layer = packet
                    .layers
                    .iter()
                    .find(|l| l.target_operations(batch.layer_id).is_some())
                    .unwrap();
                self.pending_operations.push((
                    batch.layer_id,
                    layer.target_operations(batch.layer_id).unwrap()[index as usize].clone(),
                ));
            }
        }
        self.dabs += packet.dabs.len();
        if let Some(batch) = packet.dab_batches.iter().rev().find(|b| b.dab_count > 0) {
            self.last_style = Some(batch.style.clone());
        }
        self.recorded_dabs.extend_from_slice(packet.dabs);
        for layer in packet.layers.iter().filter(|_| packet.commit_rasters) {
            for (mask, revision) in std::iter::once((false, &layer.raster))
                .chain(layer.mask.iter().map(|m| (true, &m.raster)))
            {
                if revision.try_data().is_none() {
                    use layer_core::raster::*;
                    let plane = if mask {
                        RasterPlane::Mask
                    } else {
                        RasterPlane::Color
                    };
                    let bytes = vec![128; plane.descriptor(self.document_color()).byte_len([TILE_SIZE; 2]).unwrap()];
                    let mut data = RasterData::default();
                    data.tiles.insert(
                        TileKey {
                            plane,
                            coordinate: [0, 0],
                        },
                        RasterTile::backed(
                            TileBlob::encode(plane.descriptor(self.document_color()), &bytes).unwrap(),
                        ),
                    );
                    revision.publish(Ok(data)).unwrap();
                }
            }
        }
        self.composites += usize::from(packet.composite_all);
        Ok(())
    }
}
pub(crate) fn session(platform: Platform) -> UiSession<Recorder> {
    UiSession::new(
        Recorder::default(),
        Document::new("test", 1000, 1000, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }),
        [1000, 1000],
        platform,
    )
    .unwrap()
}
pub(crate) fn key(
    s: &mut UiSession<Recorder>,
    name: &str,
    pressed: bool,
    command: bool,
    editing: bool,
) -> InputReply {
    s.input(UiInput::Key {
        key: name.into(),
        pressed,
        repeat: false,
        modifiers: Modifiers {
            command,
            ..Modifiers::default()
        },
        editing,
        divider: None,
    })
    .unwrap()
}
pub(crate) fn pointer_input(id: u64, phase: ContactPhase, kind: PointerKind, button: PointerButton, position: [f32; 2], time_ns: u64) -> UiInput {
    UiInput::Pointer { id, phase, kind, button, position, time_ns }
}
pub(crate) fn pointer(
    s: &mut UiSession<Recorder>,
    id: u64,
    phase: ContactPhase,
    position: [f32; 2],
    button: PointerButton,
) -> InputReply {
    s.input(pointer_input(id, phase, PointerKind::Pen, button, position, 0))
    .unwrap()
}
pub(crate) fn chrome(s: &mut UiSession<Recorder>, event: ChromeEvent, facts: ChromeFacts) -> InputReply {
    s.input(UiInput::Chrome {
        event,
        facts,
        viewport: [1200.0, 900.0],
    })
    .unwrap()
}
pub(crate) fn invoke(session: &mut UiSession<Recorder>, command: CommandId) -> UiChange {
    let mut change = session.dispatch(UiAction::Invoke { command }).unwrap();
    let prepare_move = command == CommandId::Move || (matches!(command, CommandId::Undo | CommandId::Redo)
        && session.layer_interaction.tool == LayerCanvasTool::Move);
    if prepare_move { change.regions |= session.frame(1, 1).unwrap().regions; }
    if command == CommandId::ScaleRotate || prepare_move {
        for tick in 1..=4 {
            if !session.content_bounds.busy() { break; }
            change.regions |= session.frame(tick, tick).unwrap().regions;
            let request = session.engine.backend().bounds_requests.last().unwrap();
            let layer_core::ContentScope::Target(target) = request.scope else { panic!("fixture target bounds"); };
            let document = &request.document;
            let bounds = document.selection.as_ref().map_or_else(
                || layer_core::Rect::from_extent(document.target_extent(target)),
                |selection| document.affine_edit_transform(target).unwrap().inverse().unwrap().bounds(selection.coverage_bounds()),
            );
            session.engine.backend_mut().bounds_reply = Some(Ok(bounds));
            change.regions |= session.frame(tick + 1, tick + 1).unwrap().regions;
        }
        assert!(!session.content_bounds.busy(), "fixture bounds did not finish");
    }
    change
}
pub(crate) fn layer(s: &mut UiSession<Recorder>, action: LayerAction) {
    s.dispatch(UiAction::Layer { action }).unwrap();
}
pub(crate) fn customize(s: &mut UiSession<Recorder>, action: CustomizationAction) -> UiChange {
    s.dispatch(UiAction::Customize { action }).unwrap()
}
pub(crate) fn drag(
    s: &mut UiSession<Recorder>,
    item: DockItem,
    phase: ContactPhase,
    position: [f32; 2],
    viewport: [f32; 2],
) -> UiChange {
    s.dispatch(UiAction::DragWorkspace { item, phase, position, viewport, tabs: vec![] }).unwrap()
}
pub(crate) fn find_group(
    s: &UiSession<Recorder>,
    viewport: [f32; 2],
    find: impl Fn(&GroupPlacement) -> bool,
) -> GroupPlacement {
    s.layout(viewport).groups.into_iter().find(|g| find(g)).unwrap()
}
pub(crate) fn event(
    session: &UiSession<Recorder>,
    sequence: u64,
    phase: PenPhase,
    pressure: f32,
) -> PenEvent {
    PenEvent {
        device_id: 1,
        sequence,
        timestamp_ns: sequence * 10_000_000,
        view_revision: session.state.camera.revision,
        surface_position: Point {
            x: 200.0 + sequence as f32 * 25.0,
            y: 300.0,
        },
        pressure,
        tilt_radians: [0.0; 2],
        twist_radians: 0.0,
        distance: 0.0,
        phase,
        tool: ToolKind::Pen,
        flags: SampleFlags::PRIMARY,
    }
}
pub(crate) fn preference(s: &mut UiSession<Recorder>, action: PreferenceAction) {
    s.dispatch(UiAction::Preferences { action }).unwrap();
}
pub(crate) fn edit_preference(s: &mut UiSession<Recorder>, id: PreferenceId, value: PreferenceValue) {
    preference(s, PreferenceAction::Edit { id, value });
}
pub(crate) fn record_shortcut(s: &mut UiSession<Recorder>, id: &str, name: &str, command: bool) {
    preference(s, PreferenceAction::BeginShortcut { id: id.into() });
    assert!(key(s, name, true, command, true).handled);
    key(s, name, false, command, true);
}

impl UiSession<Recorder> {
    pub(crate) fn set_platform(&mut self, platform: Platform) {
        if self.state.platform != platform {
            self.platform_prediction_available = None;
        }
        self.state.platform = platform;
        self.refresh_feedback_config();
        self.state.palette = self
            .state
            .settings
            .palette(self.state.theme, platform, self.system_accent);
        self.refresh_commands();
        self.refresh_shortcuts(true);
    }
}

pub(crate) fn on_surface(s: &UiSession<Recorder>, p: Point) -> Point {
    let m = s.state.camera.document_to_surface();
    Point { x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5] }
}

pub(crate) fn pen_at(s: &mut UiSession<Recorder>, sequence: u64, phase: PenPhase, p: [f32; 2]) {
    let mut e = event(s, sequence, phase, 1.);
    e.surface_position = on_surface(s, Point { x: p[0], y: p[1] });
    s.pen(e).unwrap();
}

pub(crate) fn tile_ids(layout: &DockLayout, panel: Panel) -> Vec<u32> {
    layout.panel(panel).unwrap().tiles().iter().map(|tile| tile.id).collect()
}

pub(crate) fn rectangle([x0, y0, x1, y1]: [f32; 4]) -> layer_core::Selection {
    layer_core::Selection::polygon(vec![
        Point { x: x0, y: y0 }, Point { x: x1, y: y0 },
        Point { x: x1, y: y1 }, Point { x: x0, y: y1 },
    ]).unwrap()
}

pub(crate) fn select(s: &mut UiSession<Recorder>, selection: impl Into<Option<layer_core::Selection>>) {
    s.layer_edit(layer_core::Edit::SetSelection(selection.into())).unwrap();
    s.frame(1, 1).unwrap();
}

pub(crate) fn notice_text(s: &UiSession<Recorder>) -> Option<&str> {
    s.state.notice.as_ref().map(|n| n.text.as_str())
}

pub(crate) fn package_json(categories: Vec<layer_core::EffectCategory>, filters: Vec<layer_core::EffectDefinition>) -> String {
    serde_json::to_string(&layer_core::EffectPackage { format: 2, categories, filters }).unwrap()
}

pub(crate) fn insert_effect(s: &mut UiSession<Recorder>, effect: &str) {
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: effect.into() } }).unwrap();
}

pub(crate) fn stage_candidate_library(s: &mut UiSession<Recorder>, catalog: &layer_core::EffectCatalog, label: &str, missing: &str) {
    let mut filters = catalog.filters().to_vec();
    std::sync::Arc::make_mut(&mut filters[0].program).label = label.into();
    s.load_effect_library(&package_json(catalog.categories().to_vec(), filters),
        |_| panic!("{missing}"), layer_core::EffectInstallMode::Merge).unwrap();
}

pub(crate) fn tile_anchor(layout: &DockLayout, control: ToolbarControl) -> TileAnchor {
    layout.panels.iter().find_map(|panel| panel.tiles().iter()
        .find(|tile| tile.control == control)
        .map(|tile| TileAnchor { panel: panel.id, tile: tile.id })).unwrap()
}

pub(crate) fn assert_operation_undo_redo(s: &mut UiSession<Recorder>, layer: LayerId, queued: usize, committed: u64, frames: [u64; 2]) {
    invoke(s, CommandId::Undo);
    s.frame(frames[0], frames[0]).unwrap();
    assert_eq!(s.engine.document().layer(layer).unwrap().pending_operations.len(), queued);
    invoke(s, CommandId::Redo);
    s.frame(frames[1], frames[1]).unwrap();
    assert_eq!(s.engine.document().layer(layer).unwrap().raster.identity(), committed);
}

pub(crate) fn abandon_layer_drag(s: &mut UiSession<Recorder>, cancel: u8) -> bool {
    pen_at(s, 1, PenPhase::Down, [30., 40.]);
    pen_at(s, 1, PenPhase::Move, [90., 100.]);
    let handled = match cancel {
        0 => { pen_at(s, 1, PenPhase::Cancel, [90., 100.]); true }
        1 => {
            let handled = key(s, "escape", true, false, false).handled;
            key(s, "escape", false, false, false);
            handled
        }
        _ => { s.input(UiInput::Blur).unwrap(); true }
    };
    pen_at(s, 1, PenPhase::Up, [90., 100.]);
    handled
}
