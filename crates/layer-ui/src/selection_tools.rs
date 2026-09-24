//! Selection construction and settings. Hosts only supply native contacts/keys;
//! geometry, completion, cancellation, and the single history edit live here.
use super::*;
pub use layer_core::SelectionMode;
use layer_core::{Edit, Point, Selection};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionTool {
    Rectangle,
    Ellipse,
    #[default]
    Lasso,
    Polygon,
    Wand,
    Color,
    Brush,
    Tonal,
}
impl SelectionTool {
    pub const ALL: [Self; 8] = [
        Self::Rectangle,
        Self::Ellipse,
        Self::Lasso,
        Self::Polygon,
        Self::Wand,
        Self::Color,
        Self::Brush,
        Self::Tonal,
    ];
    pub fn command(self) -> CommandId {
        match self {
            Self::Rectangle => CommandId::RectangleSelect,
            Self::Ellipse => CommandId::EllipseSelect,
            Self::Lasso => CommandId::Lasso,
            Self::Polygon => CommandId::PolygonSelect,
            Self::Wand => CommandId::AutoSelect,
            Self::Color => CommandId::ColorSelect,
            Self::Brush => CommandId::SelectionBrush,
            Self::Tonal => CommandId::TonalSelect,
        }
    }
    pub fn canvas_tool(self, source: RegionSource) -> LayerCanvasTool {
        match self {
            Self::Lasso => LayerCanvasTool::Select,
            Self::Wand => LayerCanvasTool::Region {
                fill: false,
                source,
            },
            Self::Color => LayerCanvasTool::SelectColor { source },
            kind => LayerCanvasTool::Selection { kind },
        }
    }
    pub fn geometric(self) -> bool {
        matches!(self, Self::Rectangle | Self::Ellipse)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionConstraint {
    #[default]
    Free,
    Ratio,
    Size,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelectionOptions {
    pub tool: SelectionTool,
    pub tonal: super::tonal_selection::TonalOptions,
    pub brush: super::painted_selections::SelectionBrushOptions,
    pub display: SelectionDisplayOptions,
    pub constraint: SelectionConstraint,
    pub from_center: bool,
    pub mode: SelectionMode,
    pub antialias: bool,
    pub feather: f32,
    pub constrain_angles: bool,
    pub ratio: [f32; 2],
    pub size: [f32; 2],
}
impl Default for SelectionOptions {
    fn default() -> Self {
        Self {
            tool: SelectionTool::Lasso,
            tonal: Default::default(),
            brush: Default::default(),
            display: Default::default(),
            constraint: SelectionConstraint::Free,
            from_center: false,
            mode: SelectionMode::New,
            antialias: true,
            feather: 0.,
            constrain_angles: false,
            ratio: [1., 1.],
            size: [256., 256.],
        }
    }
}
impl SelectionOptions {
    pub fn validate(&self) -> Result<(), String> {
        self.tonal.validate()?;
        self.brush.validate()?;
        self.display.validate()?;
        NumericControl::number(
            0.,
            layer_render::SelectionRefinement::MAX_FEATHER as f64,
            1.,
            1,
        )
        .validate(self.feather, "Feather radius")?;
        for value in self.ratio {
            NumericControl::number(0.01, 10000., 0.1, 2).validate(value, "Aspect ratio")?;
        }
        for value in self.size {
            NumericControl::number(1., 131072., 1., 0).validate(value, "Selection size")?;
            if value.fract() != 0. {
                return Err("Selection size needs whole pixels".into());
            }
        }
        Ok(())
    }
    pub fn edge_controls(&self) -> Vec<ToolSetting> {
        vec![ToolSetting {
            id: "selection_feather",
            label: "Feather radius",
            group: "Edges",
            value: self.feather,
            numeric: NumericControl::number(
                0.,
                layer_render::SelectionRefinement::MAX_FEATHER as f64,
                1.,
                1,
            )
            .unit("px"),
        }]
    }
    pub fn controls(&self) -> Vec<ToolSetting> {
        let (ids, labels, values, numeric) = match self.constraint {
            SelectionConstraint::Free => return Vec::new(),
            SelectionConstraint::Ratio => (
                ["selection_ratio_width", "selection_ratio_height"],
                ["Ratio width", "Ratio height"],
                self.ratio,
                NumericControl::number(0.01, 10000., 0.1, 2),
            ),
            SelectionConstraint::Size => (
                ["selection_width", "selection_height"],
                ["Width", "Height"],
                self.size,
                NumericControl::number(1., 131072., 1., 0).unit("px"),
            ),
        };
        (0..2)
            .map(|i| ToolSetting {
                id: ids[i],
                label: labels[i],
                group: "",
                value: values[i],
                numeric: numeric.clone(),
            })
            .collect()
    }
    pub fn edit(&mut self, id: &str, value: f32) -> Result<(), String> {
        let mut next = self.clone();
        match id {
            "selection_ratio_width" => next.ratio[0] = value,
            "selection_ratio_height" => next.ratio[1] = value,
            "selection_feather" => next.feather = value,
            "selection_width" => next.size[0] = value,
            "selection_height" => next.size[1] = value,
            _ => return Err("Unknown selection setting".into()),
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn corners(&self, start: Point, end: Point, modifiers: Modifiers) -> (Point, Point) {
        let mut dx = end.x - start.x;
        let mut dy = end.y - start.y;
        let centered = self.from_center || modifiers.alt;
        if self.constraint == SelectionConstraint::Size {
            let scale = if centered { 0.5 } else { 1. };
            dx = self.size[0] * scale * if dx < 0. { -1. } else { 1. };
            dy = self.size[1] * scale * if dy < 0. { -1. } else { 1. };
        } else if modifiers.shift || self.constraint == SelectionConstraint::Ratio {
            let ratio = if modifiers.shift {
                1.
            } else {
                self.ratio[0] / self.ratio[1]
            };
            let width = dx.abs().max(dy.abs() * ratio);
            dx = width * if dx < 0. { -1. } else { 1. };
            dy = width / ratio * if dy < 0. { -1. } else { 1. };
        }
        (
            if centered {
                Point {
                    x: start.x - dx,
                    y: start.y - dy,
                }
            } else {
                start
            },
            Point {
                x: start.x + dx,
                y: start.y + dy,
            },
        )
    }
}
#[derive(Default)]
pub(super) struct SelectionTools {
    pub options: SelectionOptions,
    hover: Option<Point>,
    contact: bool,
    pub gesture_mode: Option<SelectionMode>,
    pub start_modifiers: Modifiers,
}
impl SelectionTools {
    pub fn cancel(&mut self) -> bool {
        let active = self.contact || self.hover.is_some();
        self.contact = false;
        self.hover = None;
        self.gesture_mode = None;
        self.start_modifiers = Modifiers::default();
        active
    }
}

pub(crate) fn tool_set(active: SelectionTool) -> ToolSetView {
    let item = |tool: SelectionTool| {
        let command = tool.command();
        ToolSetItem {
            label: command.label(),
            icon: command.icon().unwrap(),
            action: UiAction::Invoke { command },
            selected: tool == active,
            preview: None,
        }
    };
    ToolSetView {
        groups: vec![ToolSetItem {
            label: "Select",
            icon: "select",
            action: UiAction::Invoke {
                command: CommandId::Select,
            },
            selected: true,
            preview: None,
        }],
        subtools: SelectionTool::ALL.into_iter().map(item).collect(),
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn effective_selection_mode(&self) -> SelectionMode {
        if let Some(mode) = self.selection_tools.gesture_mode {
            return mode;
        }
        let keys = self.interaction.modifiers;
        if self.selection_brush_active() {
            return if keys.shift && !keys.alt {
                SelectionMode::Add
            } else if self.selection_tools.options.brush.subtract ^ keys.alt {
                SelectionMode::Subtract
            } else {
                SelectionMode::Add
            };
        }
        match (keys.shift, keys.alt) {
            (true, true) => SelectionMode::Intersect,
            (true, false) => SelectionMode::Add,
            (false, true) => SelectionMode::Subtract,
            _ if keys.command => SelectionMode::New,
            _ => self.selection_tools.options.mode,
        }
    }
    fn selection_geometry_modifiers(&self) -> Modifiers {
        let mut keys = self.interaction.modifiers;
        keys.shift &= !self.selection_tools.start_modifiers.shift;
        keys.alt &= !self.selection_tools.start_modifiers.alt;
        keys
    }

    pub(super) fn selection_outline(&self) -> Vec<Point> {
        let LayerCanvasTool::Selection { kind } = self.layer_interaction.tool else {
            return Vec::new();
        };
        let points = &self.layer_interaction.path;
        if kind == SelectionTool::Polygon {
            let mut path = points.clone();
            if let Some(hover) = self.selection_tools.hover {
                path.push(hover);
            }
            return path;
        }
        let [start, end] = points.as_slice() else {
            return Vec::new();
        };
        if kind==SelectionTool::Tonal {return FigureShape::Rectangle.guide(*start,*end,1.);}
        let (start, end) =
            self.selection_tools
                .options
                .corners(*start, *end, self.selection_geometry_modifiers());
        // Use image-space precision, independent of view zoom, for the committed mask.
        if matches!(kind, SelectionTool::Rectangle | SelectionTool::Tonal) {
            FigureShape::Rectangle.guide(start, end, 1.)
        } else {
            let radii = [(end.x - start.x) * 0.5, (end.y - start.y) * 0.5];
            // Quarter-pixel sagitta even on large images; a multiple of four
            // retains the exact horizontal/vertical extrema.
            let steps =
                ((std::f32::consts::PI * (radii[0].abs().max(radii[1].abs()) / 0.25).sqrt()).ceil()
                    as usize)
                    .clamp(16, 4096)
                    .next_multiple_of(4);
            (0..steps)
                .map(|i| {
                    let angle = i as f32 / steps as f32 * std::f32::consts::TAU;
                    Point {
                        x: (start.x + end.x) * 0.5 + radii[0] * angle.cos(),
                        y: (start.y + end.y) * 0.5 + radii[1] * angle.sin(),
                    }
                })
                .collect()
        }
    }
    pub(super) fn finish_polygon_selection(&mut self) -> Result<(), String> {
        if self.layer_interaction.tool
            != (LayerCanvasTool::Selection {
                kind: SelectionTool::Polygon,
            })
            || self.layer_interaction.path.len() < 3
        {
            return Err("Choose at least three corners first".into());
        }
        let points = std::mem::take(&mut self.layer_interaction.path);
        self.commit_selection_points(points)?;
        self.selection_tools.cancel();
        self.refresh_tools();
        Ok(())
    }
    fn commit_selection_points(&mut self, points: Vec<Point>) -> Result<(), String> {
        // Reject empty/collinear gestures while allowing self-crossing outlines.
        let Some(a) = points.first() else {
            return Ok(());
        };
        let Some(b) = points.iter().find(|b| b != &a) else {
            return Ok(());
        };
        if !points
            .iter()
            .any(|p| ((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)).abs() > 0.0001)
        {
            return Ok(());
        }
        self.commit_tool_selection(Selection::polygon(points).map_err(error)?)?;
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn commit_tool_selection(&mut self, selection: Selection) -> Result<(), String> {
        if let Some(options) = self.selection_refinement(layer_core::Affine::IDENTITY) {
            self.queue_selection(selection, options);
            Ok(())
        } else {
            self.layer_edit(Edit::SetSelection(Some(selection)))
                .map(|_| ())
        }
    }
    pub(super) fn selection_refinement(
        &self,
        basis: layer_core::Affine,
    ) -> Option<layer_render::SelectionRefinement> {
        let options = &self.selection_tools.options;
        let mode = self.effective_selection_mode();
        (CommandId::Select.available_on(self.state.platform)
            && (mode != SelectionMode::New || !options.antialias || options.feather > 0.))
            .then(|| layer_render::SelectionRefinement {
                resize: 0,
                mode,
                antialias: options.antialias,
                feather: options.feather,
                previous: if mode == SelectionMode::New {
                    None
                } else {
                    self.engine
                        .document()
                        .selection
                        .clone()
                        .map(std::sync::Arc::new)
                },
                source_to_document: basis,
            })
    }
    pub(super) fn selection_pen(
        &mut self,
        event: PenEvent,
        p: Point,
        kind: SelectionTool,
    ) -> Result<(), String> {
        if !matches!(
            kind,
            SelectionTool::Rectangle | SelectionTool::Ellipse | SelectionTool::Polygon
        ) {
            return Err("Invalid geometric selection tool".into());
        }
        // Closing at the first vertex takes precedence over angle snapping.
        let closing = kind == SelectionTool::Polygon
            && self.layer_interaction.path.len() >= 3
            && self.layer_interaction.path.first().is_some_and(|first| {
                (p.x - first.x).hypot(p.y - first.y) * self.state.camera.zoom <= 6.
            });
        let p = if kind == SelectionTool::Polygon
            && !closing
            && (self.selection_tools.options.constrain_angles
                || self.selection_geometry_modifiers().shift)
            && let Some(last) = self.layer_interaction.path.last()
        {
            let dx = p.x - last.x;
            let dy = p.y - last.y;
            let step = std::f32::consts::FRAC_PI_4;
            let angle = (dy.atan2(dx) / step).round() * step;
            let length = dx.hypot(dy);
            Point {
                x: last.x + angle.cos() * length,
                y: last.y + angle.sin() * length,
            }
        } else {
            p
        };
        match event.phase {
            PenPhase::Cancel => {
                self.cancel_layer_gesture()?;
            }
            PenPhase::Down => {
                self.selection_tools.contact = true;
                if kind.geometric() {
                    self.layer_interaction.path = vec![p, p];
                } else {
                    self.selection_tools.hover = Some(p);
                }
            }
            PenPhase::Move | PenPhase::Hover => {
                if kind == SelectionTool::Polygon && !self.layer_interaction.path.is_empty() {
                    self.selection_tools.hover = Some(p);
                } else if self.selection_tools.contact && self.layer_interaction.path.len() == 2 {
                    self.layer_interaction.path[1] = p;
                }
            }
            PenPhase::Up if self.selection_tools.contact => {
                self.selection_tools.contact = false;
                if kind == SelectionTool::Polygon {
                    let points = &mut self.layer_interaction.path;
                    let near =
                        |a: Point| (p.x - a.x).hypot(p.y - a.y) * self.state.camera.zoom <= 6.;
                    if points.len() >= 3 && near(points[0]) {
                        self.finish_polygon_selection()?;
                    } else if points.last().is_none_or(|last| !near(*last)) {
                        points.push(p);
                        self.selection_tools.hover = Some(p);
                    }
                } else if self.layer_interaction.path.len() == 2 {
                    self.layer_interaction.path[1] = p;
                    let points = self.selection_outline();
                    self.layer_interaction.path.clear();
                    self.commit_selection_points(points)?;
                    self.selection_tools.cancel();
                }
            }
            _ => (),
        }
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn selection_key(&mut self, key: &str) -> Result<bool, String> {
        if self.tonal_active() && key == "enter" && self.tonal_tools.ready { self.finish_tonal(true)?; return Ok(true); }
        if self.layer_interaction.tool
            != (LayerCanvasTool::Selection {
                kind: SelectionTool::Polygon,
            })
            || self.layer_interaction.path.is_empty()
        {
            return Ok(false);
        }
        match key {
            "enter" => {
                if self.layer_interaction.path.len() >= 3 {
                    self.finish_polygon_selection()?;
                }
            }
            "backspace" | "delete" => {
                self.layer_interaction.path.pop();
                if self.layer_interaction.path.is_empty() {
                    self.selection_tools.cancel();
                }
                self.layer_interaction.changed = true;
            }
            _ => return Ok(false),
        }
        self.refresh_tools();
        Ok(true)
    }
}
