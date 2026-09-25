//! Shared click-to-select/fill policy. Hosts forward input and render controls;
//! source choice, cancellation, stale-result handling and edits stay here.
use super::*;
use layer_core::{Edit, Point, Selection};
use layer_render::RegionRequest;

pub(super) struct RegionTools {
    pub tolerance: f32,
    pub refinement: layer_render::RegionRefinement,
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
    color: Option<layer_core::color::RgbColor>,
    basis: layer_core::Affine,
}
impl Default for RegionTools {
    fn default() -> Self {
        Self {
            tolerance: 0.1,
            refinement: layer_render::RegionRefinement {
                smoothing: 1.,
                ..Default::default()
            },
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
    pub fn controls(&self) -> Vec<ToolSetting> {
        let distance = layer_render::RegionRefinement::MAX_DISTANCE as f64;
        [
            (
                "tolerance",
                "Tolerance",
                "",
                NumericControl::percent(),
                self.tolerance,
            ),
            (
                "gap_closing",
                "Close gaps",
                "Edges",
                NumericControl::number(0., distance, 1., 0).unit("px"),
                self.refinement.gap_closing as f32,
            ),
            (
                "expansion",
                "Expansion",
                "Edges",
                NumericControl::number(-distance, distance, 1., 0).unit("px"),
                self.refinement.expansion as f32,
            ),
            (
                "smoothing",
                "Edge smoothing",
                "Edges",
                NumericControl::percent(),
                self.refinement.smoothing,
            ),
        ]
        .into_iter()
        .map(|(id, label, group, numeric, value)| ToolSetting {
            id,
            label,
            group,
            numeric,
            value,
        })
        .collect()
    }
    pub fn edit(&mut self, id: &str, value: f32) -> Result<(), String> {
        let control = self
            .controls()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or("Unknown region setting")?;
        control.numeric.validate(value, control.label)?;
        if matches!(id, "gap_closing" | "expansion") && value.fract() != 0. {
            return Err(format!("{} needs a whole number of pixels", control.label));
        }
        match id {
            "tolerance" => self.tolerance = value,
            "gap_closing" => self.refinement.gap_closing = value as u32,
            "expansion" => self.refinement.expansion = value as i32,
            "smoothing" => self.refinement.smoothing = value,
            _ => unreachable!(),
        }
        self.cancel();
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.contact = None;
        self.queued = None;
        self.target = None;
        self.failure = None;
    }
    pub fn renderer_replaced(&mut self) {
        self.cancel();
        self.pending = false;
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
                let Some((fill, source, contiguous)) = self.layer_interaction.tool.region() else {
                    return;
                };
                let doc = self.engine.document();
                let mask_target = self.selection_masks.target().filter(|_| fill);
                if fill && mask_target.is_none() && doc.drawing_content().is_none() {
                    return;
                }
                let source_layer = if mask_target.is_some() { self.selection_masks.artwork().unwrap_or(doc.active_layer) }
                    else { doc.drawing_target().unwrap_or(doc.active_target()) };
                let (basis, extent) = if source == RegionSource::Editing {
                    (doc.layer_transform(source_layer), doc.target_extent(source_layer))
                } else { (layer_core::Affine::IDENTITY, [doc.width, doc.height]) };
                let Some(inverse) = basis.inverse() else { return; };
                point = inverse.map(point);
                if !point.x.is_finite()
                    || !point.y.is_finite()
                    || point.x < 0.
                    || point.y < 0.
                    || point.x >= extent[0] as f32
                    || point.y >= extent[1] as f32
                {
                    return;
                }
                let request = RegionRequest {
                    request_id: self.region_tools.generation,
                    contiguous,
                    selection: if fill { None } else { self.selection_refinement(basis) },
                    source: match source {
                        RegionSource::Visible => layer_render::RegionSource::Composite,
                        RegionSource::Editing => {
                            layer_render::RegionSource::Layer(source_layer)
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
                    refinement: layer_render::RegionRefinement {
                        gap_closing: if contiguous { self.region_tools.refinement.gap_closing } else { 0 },
                        smoothing: if !fill && CommandId::Select.available_on(self.state.platform) && !self.selection_tools.options.antialias { 0. } else { self.region_tools.refinement.smoothing },
                        ..self.region_tools.refinement
                    },
                    limit: if fill && mask_target.is_none() {
                        doc.selection.as_ref().and_then(|s| s.transformed(inverse).ok()).map(std::sync::Arc::new)
                    } else {
                        None
                    },
                };
                if let Some(target) = mask_target {
                    if let Err(error) = self.queue_mask_region(target, request, basis, true) { self.state.host_error = Some(error); }
                    return;
                }
                self.region_tools.queued = Some(request);
                self.region_tools.target = Some(Target {
                    generation: self.region_tools.generation,
                    revision: doc.revision,
                    layer: doc.active_layer,
                    operation: fill.then(|| self.fill_operation()),
                    color: (fill
                        && !self.state.colors.transparent()
                        && self.state.brush.opacity > 0.)
                        .then(|| self.state.colors.definition()),
                    basis: if !fill && self.selection_refinement(basis).is_some() { layer_core::Affine::IDENTITY } else { basis },
                });
            }
            PenPhase::Cancel => self.region_tools.cancel(),
            _ => (),
        }
    }
    pub(super) fn queue_selection(&mut self, selection: Selection, options: layer_render::SelectionRefinement) {
        self.region_tools.cancel();
        let doc = self.engine.document();
        self.region_tools.queued = Some(RegionRequest {
            request_id: self.region_tools.generation, contiguous: false, selection: Some(options),
            source: layer_render::RegionSource::Selection(std::sync::Arc::new(selection)),
            position: [0,0], tolerance: 0., refinement: Default::default(), limit: None,
        });
        self.region_tools.target = Some(Target {
            generation: self.region_tools.generation,
            revision: doc.revision,
            layer: doc.active_layer,
            operation: None,
            color: None,
            basis: layer_core::Affine::IDENTITY,
        });
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
                    let selection = Selection::pixels(result.pixels).transformed(target.basis).map_err(error)?;
                    if let Some(operation) = target.operation {
                        self.paint_operation(Some(selection), operation, target.color.as_slice())?;
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
