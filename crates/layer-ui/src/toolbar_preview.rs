//! Brush-slider bookmarks and small, on-demand tip previews. The raster is
//! generated once when an editor opens; motion only changes scale and alpha.
use crate::*;
use layer_core::{BrushGrain, BrushGrainBehavior, BrushTip, DualCombineMode};
use layer_render::{CanvasRenderer, HostImage};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SliderBookmarks {
    size: Vec<f32>,
    opacity: Vec<f32>,
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
            ToolbarNumericBinding::BrushOpacity => &self.opacity,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        for binding in [
            ToolbarNumericBinding::BrushSize,
            ToolbarNumericBinding::BrushOpacity,
        ] {
            let values = self.values(&binding);
            if values.len() > 32 {
                return Err("Too many slider bookmarks".into());
            }
            for &value in values {
                binding.numeric().validate(value, "Slider bookmark")?;
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
    ) -> Result<(), String> {
        let value = binding
            .numeric()
            .resolve(
                value as f64,
                NumericOperation::Value {
                    value: value as f64,
                },
            )?
            .value as f32;
        let values = match binding {
            ToolbarNumericBinding::BrushSize => &mut self.size,
            ToolbarNumericBinding::BrushOpacity => &mut self.opacity,
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
) -> Result<f64, String> {
    let numeric = control.slider().ok_or("Not a slider")?.numeric();
    if !position.is_finite() || !travel.is_finite() || travel <= 0. {
        return Err("Invalid slider position".into());
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
            .resolve(0., NumericOperation::Position { position })?
            .value
    })
}

#[derive(Serialize)]
pub struct SliderPreviewLayout {
    pub side: f32,
    pub stamp: Bounds,
    pub viewport: Bounds,
    pub header_fade: f32,
    pub opacity: f32,
    pub text: String,
}
pub fn slider_preview_layout(
    control: ToolbarControl,
    value: f32,
    length: f32,
    extent: f32,
) -> Result<SliderPreviewLayout, String> {
    let binding = control.slider().ok_or("Not a slider")?;
    if !extent.is_finite() || extent <= 0. {
        return Err("Invalid stamp extent".into());
    }
    binding.numeric().validate(value, "Slider value")?;
    let side = if length.is_finite() {
        length.clamp(160., 240.)
    } else {
        180.
    };
    let available = side - 52.;
    let opacity = binding == ToolbarNumericBinding::BrushOpacity;
    // Size remains in document pixels. Oversized tips are clipped by the preview
    // viewport instead of making the largest sizes all look identical.
    let diameter = if opacity {
        available * 0.8
    } else {
        value * extent.clamp(0.02, 50.)
    };
    Ok(SliderPreviewLayout {
        side,
        stamp: Bounds {
            x: (side - diameter) / 2.,
            y: if opacity {
                36. + (available - diameter) / 2.
            } else {
                (side - diameter) / 2.
            },
            width: diameter,
            height: diameter,
        },
        viewport: if opacity {
            Bounds {
                x: 8.,
                y: 36.,
                width: side - 16.,
                height: side - 44.,
            }
        } else {
            Bounds {
                x: 0.,
                y: 0.,
                width: side,
                height: side,
            }
        },
        header_fade: if opacity { 0. } else { 52. },
        opacity: if opacity { value } else { 1. },
        text: format!(
            "{}: {}",
            if opacity { "Opacity" } else { "Size" },
            binding.numeric().compact_text(value as f64)
        ),
    })
}

#[derive(Serialize)]
pub struct BrushStamp {
    pub size: u32,
    /// The square footprint's diameter relative to the nominal brush diameter.
    pub extent: f32,
    pub alpha: Vec<u8>,
}
fn sample(mask: HostImage<'_>, u: f32, v: f32) -> f32 {
    // Match the repeating, bilinear brush sampler, including grain seams.
    let x = u * mask.width as f32 - 0.5;
    let y = v * mask.height as f32 - 0.5;
    let pixel = |x: i32, y: i32| {
        mask.bytes[(y.rem_euclid(mask.height as i32) as u32 * mask.stride
            + x.rem_euclid(mask.width as i32) as u32) as usize] as f32
            / 255.
    };
    let (a, b) = (x - x.floor(), y - y.floor());
    let (x, y) = (x.floor() as i32, y.floor() as i32);
    (pixel(x, y) * (1. - a) + pixel(x + 1, y) * a) * (1. - b)
        + (pixel(x, y + 1) * (1. - a) + pixel(x + 1, y + 1) * a) * b
}
fn coverage(mask: Option<HostImage<'_>>, u: f32, v: f32, hardness: f32) -> f32 {
    let radius = u.hypot(v);
    if radius >= 1. {
        0.
    } else if let Some(mask) = mask {
        sample(mask, u * 0.5 + 0.5, v * 0.5 + 0.5)
    } else {
        ((1. - radius) / (1. - hardness).max(2. / 192.)).clamp(0., 1.)
    }
}
fn grain_coverage(
    grain: Option<(&BrushGrain, HostImage<'_>)>,
    local: [f32; 2],
    world: [f32; 2],
) -> f32 {
    let Some((grain, mask)) = grain else {
        return 1.;
    };
    let [u, v] = if grain.behavior == BrushGrainBehavior::Canvas {
        world.map(|v| v / 256.)
    } else {
        local.map(|v| v * 0.5)
    };
    let (sin, cos) = grain.rotation_radians.sin_cos();
    let alpha = sample(
        mask,
        (u * cos - v * sin) * grain.scale + 0.5,
        (u * sin + v * cos) * grain.scale + 0.5,
    );
    1. - grain.depth + grain.depth * alpha
}
impl<R: CanvasRenderer> UiSession<R> {
    pub fn toolbar_stamp(&self, context: ToolbarContext) -> Result<BrushStamp, String> {
        if context != self.state().toolbar_context() {
            return Err("Obsolete brush preview".into());
        }
        let brush = self.engine().configured_brush();
        let renderer = self.engine().backend();
        let tip = |tip: &BrushTip| -> Result<_, String> {
            Ok(match tip {
                BrushTip::AnalyticEllipse => None,
                BrushTip::Mask(id) => {
                    Some(renderer.tip_mask(id).ok_or("Brush tip is unavailable")?)
                }
            })
        };
        let grain = |grain: &Option<BrushGrain>| -> Result<_, String> {
            grain
                .as_ref()
                .map(|g| {
                    renderer
                        .tip_mask(&g.asset)
                        .ok_or_else(|| "Brush grain is unavailable".to_owned())
                })
                .transpose()
        };
        let mask = tip(&brush.tip)?;
        let grain_mask = grain(&brush.grain)?;
        let dual_mask = brush
            .dual
            .as_ref()
            .map(|d| tip(&d.tip))
            .transpose()?
            .flatten();
        let dual_grain = brush
            .dual
            .as_ref()
            .map(|d| grain(&d.grain))
            .transpose()?
            .flatten();
        let size = 192;
        let extent = (1. / brush.aspect).max(1.);
        let (sin, cos) = brush.angle_radians.sin_cos();
        let mut alpha = Vec::with_capacity((size * size) as usize);
        for y in 0..size {
            for x in 0..size {
                let x = ((x as f32 + 0.5) / size as f32 * 2. - 1.) * extent;
                let y = ((y as f32 + 0.5) / size as f32 * 2. - 1.) * extent;
                let (u, v) = (x * cos + y * sin, (-x * sin + y * cos) * brush.aspect);
                let world = [x * brush.diameter * 0.5, y * brush.diameter * 0.5];
                let primary = coverage(mask, u, v, brush.hardness);
                let mut value =
                    primary * grain_coverage(brush.grain.as_ref().zip(grain_mask), [u, v], world);
                if primary > 0.
                    && let Some(dual) = &brush.dual
                {
                    let (sin, cos) = dual.angle_radians.sin_cos();
                    let (u, v) = (u - dual.offset[0], v - dual.offset[1]);
                    let (u, v) = (
                        (u * cos + v * sin) / (dual.scale * dual.aspect),
                        (-u * sin + v * cos) / dual.scale,
                    );
                    let secondary = coverage(dual_mask, u, v, brush.hardness)
                        * grain_coverage(dual.grain.as_ref().zip(dual_grain), [u, v], world);
                    value = match dual.combine {
                        DualCombineMode::Multiply => value * secondary,
                        DualCombineMode::Add => (value + secondary).min(1.),
                        DualCombineMode::Subtract => (value - secondary).max(0.),
                        DualCombineMode::Difference => (value - secondary).abs(),
                        DualCombineMode::Min => value.min(secondary),
                        DualCombineMode::Max => value.max(secondary),
                    };
                }
                alpha.push((value * 255.).round() as u8);
            }
        }
        Ok(BrushStamp {
            size,
            extent,
            alpha,
        })
    }
}
