//! Brush-slider bookmarks and asynchronous contact previews.
use crate::*;
use layer_core::{BrushSnapshot, Stroke, StrokeId, StrokePoint, StrokeTool, Point};
use layer_render::{CanvasRenderer, BrushStampRequest, DabStyle};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SliderBookmarks {
    size: Vec<f32>,
    strength: Vec<f32>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SliderBookmark {
    pub value: f32,
    pub fill: f64,
    pub selected: bool,
}
impl SliderBookmarks {
    fn values(&self, binding: &ToolbarNumericBinding) -> &[f32] {
        match binding {
            ToolbarNumericBinding::BrushSize => &self.size,
            ToolbarNumericBinding::BrushStrength => &self.strength,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), WorkspaceValidationError> {
        for binding in [
            ToolbarNumericBinding::BrushSize,
            ToolbarNumericBinding::BrushStrength,
        ] {
            let values = self.values(&binding);
            if values.len() > 32 {
                return Err("Too many slider bookmarks".into());
            }
            for &value in values {
                binding.numeric().validate(value, binding.label())?;
            }
            if values.windows(2).any(|v| v[0] >= v[1]) {
                return Err("Slider bookmarks must be sorted and unique".into());
            }
        }
        Ok(())
    }
    pub(crate) fn toggle(
        &mut self,
        binding: &ToolbarNumericBinding,
        value: f32,
        localizer: &Localizer,
    ) -> Result<(), String> {
        let value = binding
            .numeric()
            .resolve(
                value as f64,
                NumericOperation::Value {
                    value: value as f64,
                },
            ).map_err(|reason| reason.message(localizer))?
            .value as f32;
        let values = match binding {
            ToolbarNumericBinding::BrushSize => &mut self.size,
            ToolbarNumericBinding::BrushStrength => &mut self.strength,
        };
        if let Some(i) = values.iter().position(|&v| v == value) {
            values.remove(i);
        } else {
            if values.len() >= 32 {
                return Err("Remove a slider bookmark before adding another".into());
            }
            values.push(value);
            values.sort_by(f32::total_cmp);
        }
        Ok(())
    }
    pub(crate) fn view(
        &self,
        binding: &ToolbarNumericBinding,
        current: Option<f32>,
    ) -> Vec<SliderBookmark> {
        self.values(binding)
            .iter()
            .map(|&value| SliderBookmark {
                value,
                fill: binding
                    .numeric()
                    .resolve(value as f64, NumericOperation::Format)
                    .unwrap()
                    .fill,
                selected: current.is_some_and(|v| (v - value).abs() < 0.00001),
            })
            .collect()
    }
}

/// Only taps snap to nearby marks, within 18 logical pixels (at most 15% of
/// travel). Hosts supply measured thumb travel; drags pass no bookmarks.
pub fn slider_bookmark_value(
    control: ToolbarControl,
    values: &[f32],
    position: f64,
    travel: f64,
    localizer: &Localizer,
) -> Result<f64, String> {
    let numeric = control.slider().ok_or("Not a slider")?.numeric();
    if !position.is_finite() || !travel.is_finite() || travel <= 0. {
        return Err(NumericError::InvalidPosition.message(localizer));
    }
    let tolerance = (18. / travel).min(0.15);
    let nearest = values
        .iter()
        .filter_map(|&value| {
            numeric.validate(value, "Bookmark").ok()?;
            let fill = numeric
                .resolve(value as f64, NumericOperation::Format)
                .ok()?
                .fill;
            Some(((fill - position).abs(), value))
        })
        .filter(|(distance, _)| *distance <= tolerance)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    Ok(if let Some((_, value)) = nearest {
        value as f64
    } else {
        numeric
            .resolve(0., NumericOperation::Position { position }).map_err(|reason| reason.message(localizer))?
            .value
    })
}

#[derive(Serialize)]
pub struct SliderPreviewLayout {
    pub side: f32,
    pub radius: f32,
    pub bookmark: Bounds,
    pub icon: f32,
    pub caption: Bounds,
    pub stamp: Bounds,
    pub viewport: Bounds,
    pub header_fade: f32,
    pub header_fade_opacity: f32,
    pub opacity: f32,
    pub text: String,
}
pub fn slider_preview_layout(
    control: ToolbarControl,
    style: TileStyle,
    value: f32,
    length: f32,
    extent: f32,
    localizer: &Localizer,
) -> Result<SliderPreviewLayout, String> {
    let binding = control.slider().ok_or("Not a slider")?;
    if !extent.is_finite() || extent <= 0. {
        return Err("Invalid stamp extent".into());
    }
    binding.numeric().validate(value, binding.label()).map_err(|reason| reason.message(localizer))?;
    let tile = style.size()[1];
    let icon = tile * 4. / 9.;
    let inset = (tile - icon) / 2.;
    let grown = tile - TILE_SIZE;
    let side = if length.is_finite() {
        length.clamp(160. + grown, 240. + grown)
    } else {
        180. + grown
    };
    let available = side - tile - 16.;
    let fitted = binding != ToolbarNumericBinding::BrushSize;
    // Size remains in document pixels. Oversized tips are clipped by the preview
    // viewport instead of making the largest sizes all look identical.
    let diameter = if fitted {
        available * 0.8
    } else {
        value * extent.clamp(0.02, 50.)
    };
    Ok(SliderPreviewLayout {
        side,
        radius: style.corner_radius(),
        bookmark: Bounds {
            x: side - tile,
            y: 0.,
            width: tile,
            height: tile,
        },
        icon,
        caption: Bounds {
            x: inset,
            y: 0.,
            width: side - tile - inset,
            height: tile,
        },
        stamp: Bounds {
            x: (side - diameter) / 2.,
            y: if fitted {
                tile + (available - diameter) / 2.
            } else {
                (side - diameter) / 2.
            },
            width: diameter,
            height: diameter,
        },
        viewport: if fitted {
            Bounds {
                x: 8.,
                y: tile,
                width: side - 16.,
                height: side - tile - 8.,
            }
        } else {
            Bounds {
                x: 0.,
                y: 0.,
                width: side,
                height: side,
            }
        },
        header_fade: if fitted { 0. } else { 65. * tile / TileStyle::Medium.size()[1] },
        header_fade_opacity: 0.5,
        opacity: 1.,
        text: format!(
            "{}: {}",
            localizer.text(if binding == ToolbarNumericBinding::BrushSize { MessageId::TOOL_CONTROL_GROUP_SIZE } else { binding.label() }),
            binding.numeric().compact_text(value as f64)
        ),
    })
}

#[derive(Clone, Serialize)]
pub struct BrushStamp {
    pub request_id: u64,
    pub pending: bool,
    pub size: u32,
    /// The square footprint's diameter relative to the nominal brush diameter.
    pub extent: f32,
    #[serde(serialize_with = "stamp_alpha")]
    pub alpha: Vec<u8>,
}
fn stamp_alpha<S:serde::Serializer>(alpha:&[u8],serializer:S)->Result<S::Ok,S::Error> {
    serializer.serialize_bytes(alpha)
}
#[derive(Default)]
pub(crate) struct StampRequests {
    next: u64,
    flight: Option<(u64, ToolbarContext, PreviewKey, f32)>,
    ready: Option<(ToolbarContext, PreviewKey, BrushStamp)>,
}
impl StampRequests {
    pub(crate) fn invalidate(&mut self) { self.flight=None; self.ready=None; }
}
type PreviewKey = (BrushSnapshot, Option<std::sync::Arc<layer_core::paper::PaperSurface>>);

impl<R: CanvasRenderer> UiSession<R> {
    fn preview_key(&self) -> PreviewKey {
        let brush = self.engine().configured_brush().clone();
        let document = self.engine().document();
        let paper = brush.grain.is_some().then(|| std::sync::Arc::new(document.paper(document.working.target)));
        (brush, paper)
    }
    pub(crate) fn stamp_request(&self) -> Result<(BrushStampRequest, f32), String> {
        let (brush, paper) = self.preview_key();
        let tilt = if brush.contact.is_some_and(|c| c.dry.is_some()) {
            layer_core::BrushDryMedia::WRITING_TILT_RADIANS
        } else { std::f32::consts::FRAC_PI_4 };
        let point = StrokePoint { position: Point {x:0.,y:0.}, pressure: 1.,
            tilt: [tilt, 0.], twist: 0., elapsed_micros: 0 };
        let target = self.engine().document().working.target.unwrap_or(layer_core::SourceTarget::Paint(layer_core::authored::PaintHandle::INVALID));
        let resolve = |brush: BrushSnapshot| -> Result<Vec<layer_render::Dab>, String> {
            if brush.execution == layer_core::BrushExecution::Liquify {
                return Ok(layer_engine::DabGenerator::new(self.engine().document().composition().color.space)
                    .cursor_contacts(point, &brush));
            }
            let stroke = Stroke::new(StrokeId(1), target, StrokeTool::Brush, brush,
                std::sync::Arc::<[StrokePoint]>::from([point])).map_err(|e| e.to_string())?;
            let mut dabs = Vec::new();
            layer_engine::DabGenerator::generate(&stroke,
                self.engine().document().composition().color.space, &mut dabs);
            Ok(dabs)
        };
        let dabs = resolve(brush.clone())?;
        let mut geometry = brush.clone(); geometry.flow = 1.; geometry.opacity = 1.;
        geometry.color_rgba_linear[3] = 1.;
        let geometry = resolve(geometry)?;
        let radius = geometry.iter().fold((brush.diameter*0.5).max(0.005), |radius, dab| {
            let bounds = dab.bounds();
            radius.max(bounds.min.x.abs()).max(bounds.min.y.abs())
                .max(bounds.max.x.abs()).max(bounds.max.y.abs())
        });
        let bounds = layer_core::Rect { min: Point {x:-radius,y:-radius}, max: Point {x:radius,y:radius} };
        let shape_only = self.state().brush.tool == Tool::Eraser
            || brush.moves_paint_only()
            || crate::tools::is_retouching(self.state().brush.tool);
        let shape_strength = shape_only.then(|| ToolbarNumericBinding::BrushStrength.field(self.state()).map_or(1., |f| f.value));
        let mut style = DabStyle::for_brush(&brush, StrokeTool::Brush);
        style.paper = paper.map(|surface| layer_core::paper::PaperBinding { surface, target_offset: [0; 2] });
        Ok((BrushStampRequest { style, dabs, bounds, shape_strength }, 2.*radius/brush.diameter.max(0.01)))
    }

    pub fn toolbar_stamp(&mut self, context: ToolbarContext, known_request: Option<u64>) -> Result<Option<BrushStamp>, String> {
        if context != self.state().toolbar_context() { return Err("Obsolete brush preview".into()); }
        let key = self.preview_key();
        if self.brush_stamps.ready.as_ref().is_some_and(|(owner, _, _)| *owner != context) {
            self.brush_stamps.ready = None;
        }
        let mut delivered = None;
        while let Some(result) = self.renderer_mut().take_brush_stamp() {
            let image = result.map_err(|e| { self.brush_stamps.flight=None; e.to_string() })?;
            if self.brush_stamps.flight.as_ref().is_some_and(|(id,_,_,_)|*id==image.request_id) {
                let (id,owner,request_key,extent)=self.brush_stamps.flight.take().unwrap();
                if owner == context {
                    let metadata = BrushStamp { request_id:id, pending:false,
                        size:image.width, extent, alpha:Vec::new() };
                    self.brush_stamps.ready = Some((owner, request_key, metadata.clone()));
                    delivered = Some(BrushStamp { alpha:image.bytes, ..metadata });
                }
            }
        }
        let current = self.brush_stamps.ready.as_ref().is_some_and(|(owner, k, stamp)|
            *owner == context && *k == key &&
            (known_request == Some(stamp.request_id) || delivered.as_ref().is_some_and(|d| d.request_id == stamp.request_id)));
        if self.brush_stamps.flight.is_none() && !current {
            let (request, extent) = self.stamp_request()?;
            let id = self.brush_stamps.next.wrapping_add(1);
            if self.renderer_mut().request_brush_stamp(id, &request).map_err(|e|e.to_string())? {
                self.brush_stamps.next = id;
                self.brush_stamps.flight = Some((id, context, key, extent));
            }
        }
        Ok(delivered.or_else(|| self.brush_stamps.ready.as_ref()
            .filter(|(owner, _, _)| *owner == context).map(|(_, _, stamp)| stamp.clone()))
            .map(|mut stamp| { stamp.pending = !current; stamp }))
    }
}
