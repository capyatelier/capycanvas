//! Canvas geometry commands: Canvas Size, anchored in a shared dialog model,
//! and Crop Canvas to Selection. Both keep every pixel; hosts only present.
use super::*;
use layer_core::{CanvasGeometry, CanvasRect};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasSizeUnit {
    #[default]
    Pixels,
    Percent,
}
impl CanvasSizeUnit {
    pub const ALL: [Self; 2] = [Self::Pixels, Self::Percent];
    pub fn label(self) -> &'static str {
        match self {
            Self::Pixels => "Pixels",
            Self::Percent => "Percent",
        }
    }
}

crate::variants! {
    /// Where the current image stays when the canvas changes size, in reading order.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum CanvasAnchor {
        TopLeft,
        Top,
        TopRight,
        Left,
        #[default]
        Center,
        Right,
        BottomLeft,
        Bottom,
        BottomRight,
    }
}
impl CanvasAnchor {
    /// Column and row in the 3×3 picker.
    pub fn cell(self) -> [u32; 2] {
        let index = self as u32;
        [index % 3, index / 3]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "Top left",
            Self::Top => "Top",
            Self::TopRight => "Top right",
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
            Self::BottomLeft => "Bottom left",
            Self::Bottom => "Bottom",
            Self::BottomRight => "Bottom right",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CanvasSizeAction {
    Width { value: f64 },
    Height { value: f64 },
    Unit { unit: CanvasSizeUnit },
    Relative { relative: bool },
    Anchor { anchor: CanvasAnchor },
    Apply,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanvasAnchorChoice {
    pub anchor: CanvasAnchor,
    pub label: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanvasUnitChoice {
    pub unit: CanvasSizeUnit,
    pub label: &'static str,
}

/// The open Canvas Size dialog. Values are in `unit`, and are changes from
/// the current size when `relative` is set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanvasSizeView {
    pub title: &'static str,
    pub labels: [&'static str; 2],
    pub values: [f64; 2],
    pub numeric: [NumericControl; 2],
    pub unit: CanvasSizeUnit,
    pub units: Vec<CanvasUnitChoice>,
    pub relative: bool,
    pub relative_label: &'static str,
    pub anchor: CanvasAnchor,
    pub anchor_label: &'static str,
    pub anchors: Vec<CanvasAnchorChoice>,
    /// The resulting size, or why it can't be applied.
    pub message: String,
    pub can_apply: bool,
}

pub(super) struct CanvasSizeDraft {
    current: [u32; 2],
    values: [f64; 2],
    unit: CanvasSizeUnit,
    relative: bool,
    anchor: CanvasAnchor,
    view: CanvasSizeView,
}

pub(super) fn number(min: f64, max: f64, digits: u32, unit: &str) -> NumericControl {
    NumericControl {
        kind: NumericKind::Number,
        ..NumericControl::number(min, max.max(min), 1., digits).unit(unit)
    }
}

impl CanvasSizeDraft {
    fn new(current: [u32; 2]) -> Self {
        Self {
            current,
            values: current.map(f64::from),
            unit: CanvasSizeUnit::Pixels,
            relative: false,
            anchor: CanvasAnchor::Center,
            view: CanvasSizeView {
                title: "Canvas Size",
                labels: ["Width", "Height"],
                values: [0.; 2],
                numeric: [number(1., 1., 0, "px"), number(1., 1., 0, "px")],
                unit: CanvasSizeUnit::Pixels,
                units: CanvasSizeUnit::ALL.map(|unit| CanvasUnitChoice { unit, label: unit.label() }).into(),
                relative: false,
                relative_label: "Relative",
                anchor: CanvasAnchor::Center,
                anchor_label: "Anchor",
                anchors: CanvasAnchor::ALL.map(|anchor| CanvasAnchorChoice { anchor, label: anchor.label() }).into(),
                message: String::new(),
                can_apply: false,
            },
        }
    }

    fn pixels(&self, axis: usize) -> Option<u32> {
        let current = f64::from(self.current[axis]);
        let value = self.values[axis];
        let pixels = match (self.unit, self.relative) {
            (CanvasSizeUnit::Pixels, false) => value,
            (CanvasSizeUnit::Pixels, true) => current + value,
            (CanvasSizeUnit::Percent, false) => current * value / 100.,
            (CanvasSizeUnit::Percent, true) => current * (100. + value) / 100.,
        }
        .round();
        (pixels >= 1. && pixels <= f64::from(u32::MAX)).then_some(pixels as u32)
    }

    fn size(&self) -> Option<[u32; 2]> {
        Some([self.pixels(0)?, self.pixels(1)?])
    }

    fn rect(&self, size: [u32; 2]) -> CanvasRect {
        let cell = self.anchor.cell();
        CanvasRect {
            origin: std::array::from_fn(|axis| {
                let spare = i64::from(self.current[axis]) - i64::from(size[axis]);
                (spare * i64::from(cell[axis])).div_euclid(2) as i32
            }),
            size,
        }
    }

    fn round(&self, value: f64) -> f64 {
        match self.unit {
            CanvasSizeUnit::Pixels => value.round(),
            CanvasSizeUnit::Percent => (value * 100.).round() / 100.,
        }
    }

    fn set_unit(&mut self, unit: CanvasSizeUnit) {
        if unit == self.unit {
            return;
        }
        for axis in 0..2 {
            let current = f64::from(self.current[axis]);
            self.values[axis] = match unit {
                CanvasSizeUnit::Percent => self.values[axis] / current * 100.,
                CanvasSizeUnit::Pixels => self.values[axis] * current / 100.,
            };
        }
        self.unit = unit;
        self.values = self.values.map(|v| self.round(v));
    }

    fn set_relative(&mut self, relative: bool) {
        if relative == self.relative {
            return;
        }
        for axis in 0..2 {
            let whole = match self.unit {
                CanvasSizeUnit::Pixels => f64::from(self.current[axis]),
                CanvasSizeUnit::Percent => 100.,
            };
            self.values[axis] += if relative { -whole } else { whole };
        }
        self.relative = relative;
    }

    fn update(&mut self, document: &Document, limits: layer_core::GeometryLimits) {
        let limit = f64::from(limits.canvas_dimension());
        self.view.numeric = std::array::from_fn(|axis| {
            let current = f64::from(self.current[axis]);
            match (self.unit, self.relative) {
                (CanvasSizeUnit::Pixels, false) => number(1., limit, 0, "px"),
                (CanvasSizeUnit::Pixels, true) => number(1. - current, limit - current, 0, "px"),
                (CanvasSizeUnit::Percent, false) => number(0.01, limit * 100. / current, 2, "%"),
                (CanvasSizeUnit::Percent, true) => number(0.01 - 100., limit * 100. / current - 100., 2, "%"),
            }
        });
        self.view.values = self.values;
        self.view.unit = self.unit;
        self.view.relative = self.relative;
        self.view.anchor = self.anchor;
        let [width, height] = self.current;
        (self.view.message, self.view.can_apply) = match self.size() {
            None => (layer_core::CanvasGeometryError::Empty.to_string(), false),
            Some(size) if size == self.current => (format!("Current size: {width} × {height} px"), false),
            Some(size) => match document.check_canvas_geometry(&CanvasGeometry::crop(self.rect(size)), limits) {
                Ok(()) => (format!("New size: {} × {} px", size[0], size[1]), true),
                Err(error) => (error.to_string(), false),
            },
        };
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    /// Why no geometry command can change the canvas right now.
    pub(super) fn canvas_geometry_refusal(&self) -> Option<&'static str> {
        if self.operation.active() {
            Some(self.operation_refusal())
        } else if self.selection_masks.target().is_some() {
            Some("Return to the artwork first")
        } else if self.state.document_file.busy {
            Some("Wait for the current file operation")
        } else if self.painted_selections.busy() {
            Some("Wait for selection capture to finish")
        } else {
            None
        }
    }

    pub(super) fn crop_to_selection_refusal(&self) -> Option<&'static str> {
        self.canvas_geometry_refusal().or(match &self.engine.document().selection {
            None => Some("Make a selection first"),
            Some(selection) if selection.inverted => Some("An inverted selection has no bounds to crop to"),
            Some(_) => None,
        })
    }

    pub fn canvas_size_view(&self) -> Option<CanvasSizeView> {
        self.canvas_size.as_ref().map(|draft| draft.view.clone())
    }

    pub(super) fn open_canvas_size(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.canvas_geometry_refusal())?;
        let doc = self.engine.document();
        let mut draft = CanvasSizeDraft::new([doc.width, doc.height]);
        draft.update(doc, self.engine.geometry_limits());
        self.canvas_size = Some(draft);
        self.refresh_tools();
        Ok(())
    }

    pub(super) fn canvas_size_action(&mut self, action: CanvasSizeAction) -> Result<(), String> {
        if action == CanvasSizeAction::Cancel {
            self.canvas_size = None;
            self.refresh_tools();
            return Ok(());
        }
        let draft = self.canvas_size.as_mut().ok_or("Canvas Size is not open")?;
        let finite = |value: f64| if value.is_finite() { Ok(value) } else { Err("Enter a number") };
        match action {
            CanvasSizeAction::Width { value } => draft.values[0] = draft.round(finite(value)?),
            CanvasSizeAction::Height { value } => draft.values[1] = draft.round(finite(value)?),
            CanvasSizeAction::Unit { unit } => draft.set_unit(unit),
            CanvasSizeAction::Relative { relative } => draft.set_relative(relative),
            CanvasSizeAction::Anchor { anchor } => draft.anchor = anchor,
            CanvasSizeAction::Apply => {
                refused(self.canvas_geometry_refusal())?;
                let draft = self.canvas_size.as_ref().ok_or("Canvas Size is not open")?;
                let doc = self.engine.document();
                if draft.current != [doc.width, doc.height] {
                    self.canvas_size = None;
                    self.refresh_tools();
                    return Err("The canvas changed; open Canvas Size again".into());
                }
                let size = draft.size().ok_or_else(|| layer_core::CanvasGeometryError::Empty.to_string())?;
                let rect = draft.rect(size);
                self.apply_canvas_geometry(&CanvasGeometry::crop(rect), Vec::new()).map_err(|e| e.to_string())?;
                self.canvas_size = None;
                self.refresh_tools();
                return Ok(());
            }
            CanvasSizeAction::Cancel => unreachable!(),
        }
        let limits = self.engine.geometry_limits();
        let draft = self.canvas_size.as_mut().unwrap();
        draft.update(self.engine.document(), limits);
        self.refresh_tools();
        Ok(())
    }

    /// The canvas crops to the selection's nonzero coverage within it.
    pub(super) fn crop_canvas_to_selection(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.crop_to_selection_refusal())?;
        let doc = self.engine.document();
        let bounds = doc.selection.as_ref().unwrap().coverage_bounds();
        const TOLERANCE: f32 = 1e-3;
        let min = [bounds.min.x, bounds.min.y].map(|v| (v + TOLERANCE).floor().max(0.));
        let max = [(bounds.max.x, doc.width), (bounds.max.y, doc.height)]
            .map(|(v, limit)| (v - TOLERANCE).ceil().min(limit as f32));
        if bounds.is_empty() || (0..2).any(|axis| max[axis] <= min[axis]) {
            return Err("The selection doesn't cover any of the canvas".into());
        }
        let origin = min.map(|v| v as i32);
        let size = std::array::from_fn(|axis| (max[axis] - min[axis]) as u32);
        if origin == [0; 2] && size == [doc.width, doc.height] {
            return Err("The selection already covers the whole canvas".into());
        }
        self.apply_canvas_geometry(&CanvasGeometry::crop(CanvasRect { origin, size }), Vec::new()).map_err(|e| e.to_string())
    }

    /// Keep the image where it was on screen after the canvas origin moves.
    pub(super) fn follow_canvas_origin(&mut self, origin: [i32; 2]) {
        self.state.camera.follow_document_origin(origin.map(|v| v as f32));
        self.sync_camera();
    }
}
