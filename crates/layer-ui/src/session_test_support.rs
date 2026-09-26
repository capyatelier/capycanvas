use super::*;
use layer_core::{AssetId, Point, ProjectAsset};
use layer_engine::{SampleFlags, ToolKind};
use layer_render::{BackendError, FilterPreviewImage, FilterPreviewRequest, FramePacket, HostImage};
use std::collections::BTreeMap;

/// Protocol recorder only: no canvas storage or software rasterization.
#[derive(Default)]
pub(crate) struct Recorder {
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
    pub(crate) transform: Option<layer_render::TransformPreview>,
    pub(crate) overlay: Option<layer_render::SelectionOverlay>,
    pub(crate) assets: BTreeMap<AssetId, ProjectAsset>,
    pub(crate) reject_assets: bool,
    pub(crate) filter_preview: Option<FilterPreviewRequest>,
    pub(crate) filter_preview_ready: Option<Result<FilterPreviewImage, BackendError>>,
    pub(crate) filter_preview_requests: usize,
    pub(crate) filter_preview_takes: usize,
    pub(crate) filter_preview_cancels: usize,
    pub(crate) reject_filter_previews: bool,
}
impl CanvasRenderer for Recorder {
    type Error = BackendError;
    fn set_selection_overlay(&mut self, overlay: Option<layer_render::SelectionOverlay>) {self.overlay=overlay;}
    fn document_color(&self) -> layer_core::color::DocumentColor { self.color }
    fn adopt_prepared_color(&mut self, color: layer_core::color::DocumentColor) -> Result<bool, Self::Error> {
        if self.prepared_color != Some(color) { return Ok(false); }
        self.prepared_color = None;
        self.color = color;
        Ok(true)
    }
    fn supports_tiled_sources(&self) -> bool { self.tiled_sources }
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
        self.region_reply.take().map(Ok)
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
    fn prepare_owned_asset(&mut self, id: &AssetId, asset: &ProjectAsset) -> Result<(), Self::Error> {
        if self.reject_assets {
            return Err(BackendError("asset preparation failed"));
        }
        self.assets.insert(id.clone(), asset.clone());
        Ok(())
    }
    fn release_asset(&mut self, _: &AssetId) {}
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
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
        for layer in packet.layers {
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
pub(crate) fn session() -> UiSession<Recorder> {
    UiSession::new(
        Recorder::default(),
        Document::new("test", 1000, 1000),
        [1000, 1000],
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
pub(crate) fn pointer(
    s: &mut UiSession<Recorder>,
    id: u64,
    phase: ContactPhase,
    position: [f32; 2],
    button: PointerButton,
) -> InputReply {
    s.input(UiInput::Pointer {
        id,
        phase,
        position,
        button,
        kind: PointerKind::Pen,
    })
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
    session.dispatch(UiAction::Invoke { command }).unwrap()
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
