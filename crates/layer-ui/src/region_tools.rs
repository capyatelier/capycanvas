//! Shared click-to-select/fill policy. Hosts forward input and render controls;
//! source choice, cancellation, stale-result handling and edits stay here.
use super::*;
use layer_core::{Edit, Point, Selection};
use layer_render::RegionRequest;

pub(super) struct RegionTools {
    pub tolerance: f32,
    pub source: [RegionSource; 2],
    contact: Option<Point>,
    generation: u64,
    queued: Option<RegionRequest>,
    pending: bool,
    target: Option<Target>,
    failure: Option<&'static str>,
}
struct Target {
    generation: u64,
    revision: u64,
    layer: LayerId,
    operation: Option<layer_core::LayerOperationKind>,
    offset: Point,
}
impl Default for RegionTools {
    fn default() -> Self {
        Self {
            tolerance: 0.1,
            source: [RegionSource::Visible; 2],
            contact: None,
            generation: 0,
            queued: None,
            pending: false,
            target: None,
            failure: None,
        }
    }
}
impl RegionTools {
    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.contact = None;
        self.queued = None;
        self.target = None;
        self.failure = None;
    }
    pub fn busy(&self) -> bool {
        self.pending || self.queued.is_some() || self.failure.is_some()
    }
    pub fn cancellable(&self) -> bool {
        self.contact.is_some() || self.target.is_some()
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn region_pen(&mut self, event: PenEvent) {
        match event.phase {
            PenPhase::Down => {
                self.region_tools.cancel();
                self.region_tools.contact = Some(
                    self.state
                        .camera
                        .input_transform()
                        .map(event.surface_position),
                );
            }
            PenPhase::Up => {
                let Some(mut point) = self.region_tools.contact.take() else {
                    return;
                };
                let LayerCanvasTool::Region { fill, source } = self.layer_interaction.tool else {
                    return;
                };
                let doc = self.engine.document();
                let Some(target) = doc.layer(doc.active_layer) else {
                    return;
                };
                if fill && (!LayerControls::for_layer(doc, target).fill || doc.active_mask) {
                    return;
                }
                let offset = if source == RegionSource::Editing {
                    doc.layer_offset(doc.active_layer)
                } else {
                    Point::default()
                };
                point.x -= offset.x;
                point.y -= offset.y;
                if !point.x.is_finite()
                    || !point.y.is_finite()
                    || point.x < 0.
                    || point.y < 0.
                    || point.x >= doc.width as f32
                    || point.y >= doc.height as f32
                {
                    return;
                }
                self.region_tools.queued = Some(RegionRequest {
                    request_id: self.region_tools.generation,
                    source: match source {
                        RegionSource::Visible => layer_render::RegionSource::Composite,
                        RegionSource::Editing => {
                            layer_render::RegionSource::Layer(doc.active_layer)
                        }
                        RegionSource::Reference => {
                            if doc.reference_layers.is_empty() {
                                self.region_tools.failure =
                                    Some("Mark a reference layer with the lighthouse button.");
                                return;
                            }
                            layer_render::RegionSource::Layers(doc.reference_snapshot())
                        }
                    },
                    position: [point.x as u32, point.y as u32],
                    tolerance: self.region_tools.tolerance,
                    limit: if fill {
                        doc.selection.as_ref().map(|s| {
                            std::sync::Arc::new(s.translated(Point {
                                x: -offset.x,
                                y: -offset.y,
                            }))
                        })
                    } else {
                        None
                    },
                });
                self.region_tools.target = Some(Target {
                    generation: self.region_tools.generation,
                    revision: doc.revision,
                    layer: doc.active_layer,
                    operation: fill.then(|| self.fill_operation()),
                    offset,
                });
            }
            PenPhase::Cancel => self.region_tools.cancel(),
            _ => (),
        }
    }
    pub(super) fn poll_region_tool(&mut self) -> Result<(), String> {
        if let Some(error) = self.region_tools.failure.take() {
            return Err(error.into());
        }
        if self.region_tools.pending
            && let Some(result) = self.engine.backend_mut().take_region()
        {
            self.region_tools.pending = false;
            let result = result.map_err(error)?;
            if self
                .region_tools
                .target
                .as_ref()
                .is_some_and(|t| t.generation == result.request_id)
            {
                let target = self.region_tools.target.take().unwrap();
                let doc = self.engine.document();
                if doc.revision == target.revision && doc.active_layer == target.layer {
                    if target.operation.is_some() && result.pixels.bounds() == [0; 4] {
                        return Ok(()); // No paint and no empty undo entry.
                    }
                    let selection = Selection::pixels(result.pixels).translated(target.offset);
                    if let Some(operation) = target.operation {
                        self.paint_operation(Some(selection), operation)?;
                    } else {
                        self.layer_edit(Edit::SetSelection(Some(selection)))?;
                    }
                }
            }
        }
        if !self.region_tools.pending
            && let Some(request) = self.region_tools.queued.as_ref()
            && self
                .engine
                .backend_mut()
                .request_region(request.clone())
                .map_err(error)?
        {
            self.region_tools.queued = None;
            self.region_tools.pending = true;
        }
        Ok(())
    }
}
