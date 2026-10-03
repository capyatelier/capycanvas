//! Canvas geometry commands: Canvas Size, anchored in a shared dialog model,
//! and Crop Canvas to Selection. Both keep every pixel; hosts only present.
use super::*;
use std::sync::Arc;
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
    pub fn localized_label(self, localization: &Localizer) -> Arc<str> {
        localization.text(match self {
            Self::Pixels => MessageId::RESOURCES_SIZE_PIXELS,
            Self::Percent => MessageId::RESOURCES_SIZE_PERCENT,
        })
    }
    pub(super) fn round(self, value: f64) -> f64 {
        match self {
            Self::Pixels => value.round(),
            Self::Percent => (value * 100.).round() / 100.,
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
    pub fn localized_label(self, localization: &Localizer) -> Arc<str> {
        localization.text(match self {
            Self::TopLeft => MessageId::RESOURCES_SIZE_ANCHOR_TOP_LEFT,
            Self::Top => MessageId::RESOURCES_SIZE_ANCHOR_TOP,
            Self::TopRight => MessageId::RESOURCES_SIZE_ANCHOR_TOP_RIGHT,
            Self::Left => MessageId::RESOURCES_SIZE_ANCHOR_LEFT,
            Self::Center => MessageId::RESOURCES_SIZE_ANCHOR_CENTER,
            Self::Right => MessageId::RESOURCES_SIZE_ANCHOR_RIGHT,
            Self::BottomLeft => MessageId::RESOURCES_SIZE_ANCHOR_BOTTOM_LEFT,
            Self::Bottom => MessageId::RESOURCES_SIZE_ANCHOR_BOTTOM,
            Self::BottomRight => MessageId::RESOURCES_SIZE_ANCHOR_BOTTOM_RIGHT,
        })
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
    pub label: Arc<str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanvasUnitChoice {
    pub unit: CanvasSizeUnit,
    pub label: Arc<str>,
}

/// The open Canvas Size dialog. Values are in `unit`, and are changes from
/// the current size when `relative` is set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanvasSizeView {
    pub title: Arc<str>,
    pub apply_label: Arc<str>,
    pub cancel_label: Arc<str>,
    pub labels: [Arc<str>; 2],
    pub values: [f64; 2],
    pub numeric: [NumericControl; 2],
    pub unit: CanvasSizeUnit,
    pub units: Vec<CanvasUnitChoice>,
    pub relative: bool,
    pub relative_label: Arc<str>,
    pub anchor: CanvasAnchor,
    pub anchor_label: Arc<str>,
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

pub(super) fn set_size_unit(current: [u32; 2], values: &mut [f64; 2], unit: &mut CanvasSizeUnit, next: CanvasSizeUnit) {
    if next == *unit {
        return;
    }
    for axis in 0..2 {
        let current = f64::from(current[axis]);
        values[axis] = match next {
            CanvasSizeUnit::Percent => values[axis] / current * 100.,
            CanvasSizeUnit::Pixels => values[axis] * current / 100.,
        };
    }
    *unit = next;
    *values = values.map(|v| next.round(v));
}

impl CanvasSizeDraft {
    fn new(current: [u32; 2], localization: &Localizer) -> Self {
        Self {
            current,
            values: current.map(f64::from),
            unit: CanvasSizeUnit::Pixels,
            relative: false,
            anchor: CanvasAnchor::Center,
            view: CanvasSizeView {
                title: localization.text(MessageId::RESOURCES_SIZE_CANVAS_TITLE),
                apply_label: localization.text(MessageId::COMMON_APPLY),
                cancel_label: localization.text(MessageId::COMMON_CANCEL),
                labels: [localization.text(MessageId::RESOURCES_SIZE_WIDTH), localization.text(MessageId::RESOURCES_SIZE_HEIGHT)],
                values: [0.; 2],
                numeric: [number(1., 1., 0, "px"), number(1., 1., 0, "px")],
                unit: CanvasSizeUnit::Pixels,
                units: CanvasSizeUnit::ALL.map(|unit| CanvasUnitChoice { unit, label: unit.localized_label(localization) }).into(),
                relative: false,
                relative_label: localization.text(MessageId::RESOURCES_SIZE_RELATIVE),
                anchor: CanvasAnchor::Center,
                anchor_label: localization.text(MessageId::RESOURCES_SIZE_ANCHOR),
                anchors: CanvasAnchor::ALL.map(|anchor| CanvasAnchorChoice { anchor, label: anchor.localized_label(localization) }).into(),
                message: String::new(),
                can_apply: false,
            },
        }
    }

    pub(super) fn set_localization(&mut self, document: &Document, limits: layer_core::GeometryLimits, localization: &Localizer) {
        self.view = Self::new(self.current, localization).view;
        self.update(document, limits, localization);
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

    fn update(&mut self, document: &Document, limits: layer_core::GeometryLimits, localization: &Localizer) {
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
            Some(size) if size == self.current => (size_message(localization, MessageId::RESOURCES_SIZE_CURRENT, [width, height]), false),
            Some(size) => match document.check_canvas_geometry(&CanvasGeometry::crop(self.rect(size)), limits) {
                Ok(()) => (size_message(localization, MessageId::RESOURCES_SIZE_NEW, size), true),
                Err(error) => (error.to_string(), false),
            },
        };
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn refresh_size_localization(&mut self) {
        let document = self.engine.document();
        let limits = self.engine.geometry_limits();
        let localization = &self.state.localization;
        if let Some(draft) = &mut self.canvas_size { draft.set_localization(document, limits, localization); }
        if let Some(draft) = &mut self.image_size { draft.set_localization(document, limits, localization); }
    }

    /// Why no geometry command can change the canvas right now.
    pub(super) fn canvas_geometry_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        if self.operation.active() {
            Some(self.operation_refusal())
        } else if self.selection_masks.target().is_some() {
            Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST))
        } else if self.state.document_file.busy {
            Some(l.text(MessageId::COMMANDS_WAIT_FOR_THE_CURRENT_FILE_OPERATION))
        } else if self.painted_selections.busy() {
            Some(l.text(MessageId::COMMANDS_REFUSAL_CANVAS_SIZE_WAIT_FOR_SELECTION_CAPTURE_TO_FINISH))
        } else {
            None
        }
    }

    pub(super) fn crop_to_selection_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        self.canvas_geometry_refusal().or(match &self.engine.document().selection {
            None => Some(l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST)),
            Some(selection) if selection.inverted => Some(l.text(MessageId::COMMANDS_REFUSAL_CANVAS_SIZE_AN_INVERTED_SELECTION_HAS_NO_BOUNDS_TO_CROP_TO)),
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
        let mut draft = CanvasSizeDraft::new([doc.width, doc.height], self.localization());
        draft.update(doc, self.engine.geometry_limits(), self.localization());
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
        let finite = |value: f64| if value.is_finite() { Ok(value) } else { Err(NumericError::FiniteNumber.message(&self.state.localization)) };
        match action {
            CanvasSizeAction::Width { value } => draft.values[0] = draft.unit.round(finite(value)?),
            CanvasSizeAction::Height { value } => draft.values[1] = draft.unit.round(finite(value)?),
            CanvasSizeAction::Unit { unit } => set_size_unit(draft.current, &mut draft.values, &mut draft.unit, unit),
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
        draft.update(self.engine.document(), limits, &self.state.localization);
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

pub(super) fn size_message(localization: &Localizer, message: MessageId, [width, height]: [u32; 2]) -> String {
    let mut args = FluentArgs::new(); args.set("width", width); args.set("height", height);
    localization.format(message, &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::{session, invoke};
    #[test]
    fn canvas_size_language_refresh_preserves_relative_draft_and_checkpoint() {
        let mut session = session(Platform::Gtk);
        invoke(&mut session, CommandId::CanvasSize);
        session.canvas_size_action(CanvasSizeAction::Relative { relative: true }).unwrap();
        session.canvas_size_action(CanvasSizeAction::Width { value: 17. }).unwrap();
        session.canvas_size_action(CanvasSizeAction::Anchor { anchor: CanvasAnchor::BottomLeft }).unwrap();
        let before = session.canvas_size_view().unwrap();
        let checkpoint = session.engine.checkpoint();
        assert!(session.set_localization(Localizer::shared(UiLanguage::Japanese)));
        let after = session.canvas_size_view().unwrap();
        assert_ne!(before.title, after.title);
        assert_ne!(before.message, after.message);
        assert_eq!(after.values, before.values);
        assert_eq!(after.numeric, before.numeric);
        assert_eq!(after.relative, before.relative);
        assert_eq!(after.anchor, before.anchor);
        assert_eq!(after.can_apply, before.can_apply);
        assert_eq!(session.engine.checkpoint(), checkpoint);
    }

    #[test]
    fn retained_canvas_size_status_survives_ordinary_publication() {
        let mut session = session(Platform::Gtk);
        invoke(&mut session, CommandId::CanvasSize);
        let message = session.canvas_size.as_ref().unwrap().view.message.as_ptr();
        for size in [8., 17.] {
            session.dispatch(UiAction::SetBrushSize { value: size }).unwrap();
            session.dispatch(UiAction::SetZoom { zoom: size / 8. }).unwrap();
            session.set_viewport([640. + size, 480.], [640 + size as u32, 480]).unwrap();
            assert_eq!(session.canvas_size.as_ref().unwrap().view.message.as_ptr(), message);
        }
    }
}
