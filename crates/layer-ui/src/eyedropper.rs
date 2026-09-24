//! Latest-point coalescing and color policy, shared by every host. One GPU
//! sample can be in flight; neither pointer events nor frames wait for it.
use crate::{ColorState, LayerCanvasTool};
use layer_core::color::{RgbColor, RgbSpace};
use layer_render::{CanvasRenderer, ColorSampleArea, ColorSampleRequest, ColorSampleSource};
use serde::{Deserialize, Serialize};

pub const COLOR_SAMPLE_WIDTHS: [u32; 5] = [1, 5, 15, 51, 101];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorPickerStyle {
    #[default]
    Glass,
    Eyedropper,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColorPickerAction {
    Toggle,
    Settings { anchor: crate::DrawerAnchor },
    Style { style: ColorPickerStyle },
    Source { layer: bool },
}
#[derive(Clone, Debug, Serialize)]
pub struct ColorPickerState {
    pub style: ColorPickerStyle,
    pub layer: bool,
    pub sample_width: u32,
    pub can_sample_layer: bool,
    pub preview: Option<RgbColor>,
    pub sample_sizes: &'static [u32],
}
impl crate::UiState {
    pub fn preview_colors(&self) -> std::borrow::Cow<'_, ColorState> {
        if let Some(sample) = self.color_picker.preview {
            let mut colors = self.display_colors().clone();
            if colors.set_color(sample).is_ok() {
                return std::borrow::Cow::Owned(colors);
            }
        }
        std::borrow::Cow::Borrowed(self.display_colors())
    }
}

#[derive(Default)]
pub(crate) struct Picking {
    pub previous: Option<LayerCanvasTool>,
    pub original: Option<RgbColor>,
    pub position: Option<[f32; 2]>,
    pub touch: Option<u64>,
    pub touch_offset: f32,
    pub finishing: bool,
    pub consumed: Vec<(crate::PointerKind, u64)>,
}

#[derive(Default)]
pub(crate) struct Eyedropper {
    pub layer: bool,
    pub area: ColorSampleArea,
    pub contact: bool,
    pub picking: Picking,
    pub preview_only: bool,
    pub sample: Option<RgbColor>,
    generation: u64,
    last: Option<(ColorSampleSource, [u32; 2])>,
    queued: Option<ColorSampleRequest>,
    pending: bool,
}
impl Eyedropper {
    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.contact = false;
        self.last = None;
        self.sample = None;
        self.queued = None;
        // Still drain an accepted request, but never apply its stale result.
    }
    pub fn queue(&mut self, source: ColorSampleSource, position: [u32; 2]) {
        if self.last == Some((source, position)) {
            return;
        }
        self.sample = None;
        self.last = Some((source, position));
        self.queued = Some(ColorSampleRequest {
            request_id: self.generation,
            source,
            position,
            area: self.area,
        });
    }
    pub fn renderer_replaced(&mut self) {
        self.cancel();
        self.pending = false;
    }
    pub fn busy(&self) -> bool {
        self.pending || self.queued.is_some()
    }
    pub fn poll<R: CanvasRenderer>(
        &mut self,
        renderer: &mut R,
        space: RgbSpace,
    ) -> Result<Option<RgbColor>, String> {
        if !self.busy() {
            return Ok(None);
        }
        let mut color = None;
        if let Some(result) = renderer.take_color_sample() {
            self.pending = false;
            let sample = result.map_err(|e| e.to_string())?;
            if sample.request_id == self.generation {
                self.sample = None;
            }
            if sample.request_id == self.generation && sample.rgba[3] > 0.0 {
                let [r, g, b, _] = sample.rgba;
                // Pick paint color, not the existing pixel's transparency.
                // Samples are straight document-linear, including extended RGB.
                color = Some(RgbColor::from_linear(space, [r, g, b, 1.])?);
            }
        }
        if !self.pending
            && let Some(request) = self.queued
            && renderer
                .request_color_sample(request)
                .map_err(|e| e.to_string())?
        {
            self.queued = None;
            self.pending = true;
        }
        if let Some(color) = color {
            self.sample = Some(color);
        }
        Ok(color)
    }
}

impl Default for ColorPickerState {
    fn default() -> Self {
        Self { style: ColorPickerStyle::Glass, layer: false, sample_width: 1,
            can_sample_layer: false, preview: None, sample_sizes: &COLOR_SAMPLE_WIDTHS }
    }
}

/// Small, reversible color-panel publication; never a workspace or paint edit.
#[derive(Serialize)]
pub struct PickerPreview<'a> {
    pub picker: &'a ColorPickerState,
    pub colors: std::borrow::Cow<'a, ColorState>,
    pub view: crate::ColorPanelView,
}
impl<R: CanvasRenderer> crate::UiSession<R> {
    pub fn color_preview(&self) -> PickerPreview<'_> {
        let colors = self.state().preview_colors();
        let view = colors.view_mapped(self.effective_sdr_rendition());
        PickerPreview { picker: &self.state().color_picker, colors, view }
    }
}
