//! Figure tools use the same GPU paint operation and selection path as fills.
use crate::*;
use layer_core::{Figure, RasterOperationKind, Point};
use layer_render::CanvasRenderer;
use std::sync::Arc;

pub(crate) fn modes(shape: FigureShape, paint: FigurePaint, localizer: &crate::localization::Localizer) -> Vec<ToolSetItem> {
    let item = |label, icon, shape, paint, selected| ToolSetItem { enabled: true, disabled_reason: None,
        label,
        icon,
        selected,
        preview: None,
        action: UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::Figure { shape, paint },
            },
        },
    };
    let icon = ToolVariant::Figure { shape }.icon();
    [
        (FigurePaint::Outline, localizer.text(crate::localization::MessageId::TOOL_FIGURES_OUTLINE)),
        (FigurePaint::Fill, localizer.text(crate::localization::MessageId::TOOL_FIGURES_FILL)),
        (FigurePaint::Both, localizer.text(crate::localization::MessageId::TOOL_FIGURES_OUTLINE_AND_FILL)),
    ]
        .into_iter()
        .filter(|(p, _)| shape != FigureShape::Line || *p == FigurePaint::Outline)
        .map(|(p, label)| {
            let icon = match (shape, p) {
                (FigureShape::Rectangle, FigurePaint::Fill) => "rectangle-fill",
                (FigureShape::Rectangle, FigurePaint::Both) => "rectangle-both",
                (FigureShape::Ellipse, FigurePaint::Fill) => "ellipse-fill",
                (FigureShape::Ellipse, FigurePaint::Both) => "ellipse-both",
                (FigureShape::Path, FigurePaint::Fill) => "path-fill",
                (FigureShape::Path, FigurePaint::Both) => "path-both",
                (FigureShape::Polygon, FigurePaint::Fill) => "polygon-fill",
                (FigureShape::Polygon, FigurePaint::Both) => "polygon-both",
                (FigureShape::Star, FigurePaint::Fill) => "star-fill",
                (FigureShape::Star, FigurePaint::Both) => "star-both",
                _ => icon,
            };
            item(label, icon, shape, p, p == paint)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FigureOptions { corner_radius: f32, corners: f32, points: f32, inner: f32 }
impl Default for FigureOptions {
    fn default() -> Self { Self { corner_radius: 0., corners: 5., points: 5., inner: 0.5 } }
}
const FIGURE_SETTINGS: [(&str, crate::localization::MessageId); 4] = [
    ("figure_corner_radius", crate::localization::MessageId::TOOL_CONTROL_CORNER_RADIUS),
    ("figure_corners", crate::localization::MessageId::TOOL_CONTROL_CORNERS),
    ("figure_points", crate::localization::MessageId::TOOL_CONTROL_POINTS),
    ("figure_inner_radius", crate::localization::MessageId::TOOL_CONTROL_INNER_RADIUS),
];
impl FigureOptions {
    fn numeric(id: &str) -> NumericControl {
        match id {
            "figure_corner_radius" => NumericControl::number(0., 4096., 1., 0).unit("px"),
            "figure_inner_radius" => NumericControl { min: 0.05, soft_min: 0.05, ..NumericControl::percent() },
            _ => NumericControl::number(3., 100., 1., 0),
        }
    }
    fn value(&self, id: &str) -> f32 {
        match id { "figure_corner_radius" => self.corner_radius, "figure_corners" => self.corners, "figure_points" => self.points, _ => self.inner }
    }
    pub(crate) fn controls(&self, shape: FigureShape, localizer: &crate::localization::Localizer) -> Vec<ToolSetting> {
        let ids: &[&str] = match shape {
            FigureShape::Rectangle => &["figure_corner_radius"],
            FigureShape::Polygon => &["figure_corners"],
            FigureShape::Star => &["figure_points", "figure_inner_radius"],
            _ => &[],
        };
        FIGURE_SETTINGS.iter().filter(|(id, _)| ids.contains(id)).map(|&(id, label)| ToolSetting { preview: false,
            id, label: localizer.text(label), label_id: label, group_id: "", group: Arc::from(""),
            numeric: Self::numeric(id), value: self.value(id),
        }).collect()
    }
    pub(crate) fn edit(&mut self, id: &str, value: f32, localizer: &crate::localization::Localizer) -> Result<(), String> {
        let (_, label) = FIGURE_SETTINGS.iter().find(|(key, _)| *key == id).ok_or("Unknown figure setting")?;
        Self::numeric(id).validate(value, *label).map_err(|reason| crate::WorkspaceValidationError::from(reason).message(localizer))?;
        let whole = value.round();
        match id { "figure_corner_radius" => self.corner_radius = value, "figure_corners" => self.corners = whole, "figure_points" => self.points = whole, _ => self.inner = value }
        Ok(())
    }
    pub(crate) fn default_value(id: &str) -> Option<f32> {
        FIGURE_SETTINGS.iter().any(|(key, _)| *key == id).then(|| Self::default().value(id))
    }
    pub(crate) fn shape(&self, shape: FigureShape, start: [f64; 2], end: [f64; 2]) -> Option<layer_core::authored::vector::LiveShape> {
        use layer_core::authored::vector::{LiveShape, ShapeKind};
        let kind = match shape {
            FigureShape::Rectangle => ShapeKind::Rectangle { radii: [f64::from(self.corner_radius); 4] },
            FigureShape::Ellipse => ShapeKind::Ellipse,
            FigureShape::Polygon => ShapeKind::Polygon { corners: self.corners as u32 },
            FigureShape::Star => ShapeKind::Star { points: self.points as u32, inner: f64::from(self.inner) },
            FigureShape::Line | FigureShape::Path => return None,
        };
        Some(LiveShape { kind, center: [(start[0] + end[0]) * 0.5, (start[1] + end[1]) * 0.5], size: [(end[0] - start[0]).abs(), (end[1] - start[1]).abs()], rotation: 0. })
    }
    pub(crate) fn needs_path(&self, shape: FigureShape) -> bool {
        matches!(shape, FigureShape::Polygon | FigureShape::Star | FigureShape::Path) || (shape == FigureShape::Rectangle && self.corner_radius > 0.)
    }
}

impl<B: CanvasRenderer> UiSession<B> {
    pub(super) fn vector_figure_layer(&self) -> Option<layer_core::authored::OccurrenceHandle> {
        self.editing_vector_layer().filter(|_| !self.state.colors.transparent())
    }
    pub(super) fn vector_tool_accepts(&self) -> bool {
        let layer = match self.layer_interaction.tool {
            LayerCanvasTool::Figure { .. } | LayerCanvasTool::LassoFill => self.vector_figure_layer(),
            LayerCanvasTool::Region { fill: true, .. } | LayerCanvasTool::EncloseFill {..} | LayerCanvasTool::PaintUnfilled {..} => self.editing_vector_layer(),
            _ => None,
        };
        layer.is_some_and(|layer| !self.engine.document().is_locked(layer))
    }
    fn figure_colors(&self, paint: FigurePaint) -> Option<[[f32; 4]; 2]> {
        let definitions = [if paint == FigurePaint::Both { self.state.colors.foreground } else { self.state.colors.definition() }, self.state.colors.background];
        let mut colors = [[0.; 4]; 2];
        for (value, definition) in colors.iter_mut().zip(definitions) {
            *value = definition.linear_in(self.engine.document().composition().color.space).ok()?;
            value[3] *= self.state.brush.opacity;
        }
        Some(colors)
    }
    pub(super) fn vector_figure_style(&self, paint: FigurePaint, cap: layer_core::authored::vector::Cap, join: layer_core::authored::vector::Join) -> Result<layer_core::authored::vector::Style, String> {
        use layer_core::authored::vector::*;
        let colors = self.figure_colors(paint).ok_or("Choose a color")?;
        let space = self.engine.document().composition().color.space;
        let color = |rgba: [f32; 4]| layer_core::color::RgbColor::from_linear(space, rgba).map(Paint::color).map_err(|e| e.to_string());
        let outline = |rgba: [f32; 4], cap: Cap| -> Result<PaintEntry, String> {
            let mut stroke = Stroke::new(color(rgba)?, f64::from(self.state.brush.diameter));
            (stroke.cap, stroke.join) = (cap, join);
            Ok(PaintEntry::Stroke(stroke))
        };
        let fill = |rgba: [f32; 4]| -> Result<PaintEntry, String> { Ok(PaintEntry::Fill(Fill { paint: color(rgba)?, unknown: None })) };
        Ok(Style::new(match paint {
            FigurePaint::Outline => vec![outline(colors[0], cap)?],
            FigurePaint::Fill => vec![fill(colors[0])?],
            FigurePaint::Both => vec![fill(colors[1])?, outline(colors[0], Cap::Butt)?],
        }))
    }
    fn figure_path(&self, figure: &Figure, id: layer_core::authored::vector::ItemId) -> Result<layer_core::authored::vector::PathItem, String> {
        use layer_core::authored::vector::*;
        let (start, end) = ([f64::from(figure.start.x), f64::from(figure.start.y)], [f64::from(figure.end.x), f64::from(figure.end.y)]);
        let join = if figure.shape == FigureShape::Ellipse { Join::Round } else { Join::Miter };
        let style = self.vector_figure_style(figure.paint, if figure.shape == FigureShape::Line { Cap::Round } else { Cap::Butt }, join)?;
        let line = PathGeometry::new(vec![Subpath::new(&[Anchor::corner(start), Anchor::corner(end)], false)]).map_err(str::to_string)?;
        let path = PathItem::new(id, Arc::new(line), Arc::new(style));
        match self.figure_options.shape(figure.shape, start, end) {
            Some(shape) => path.with_shape(shape).map_err(str::to_string),
            None => Ok(path),
        }
    }
    pub(super) fn figure_guide(&self, figure: &Figure) -> Vec<Point> {
        let (start, end) = ([f64::from(figure.start.x), f64::from(figure.start.y)], [f64::from(figure.end.x), f64::from(figure.end.y)]);
        let shaped = self.figure_options.shape(figure.shape, start, end).filter(|_| self.figure_options.needs_path(figure.shape)).and_then(|shape| shape.geometry().ok());
        let Some(geometry) = shaped else { return figure.shape.guide(figure.start, figure.end, self.state.camera.zoom) };
        let mut points = Vec::new();
        let tolerance = 0.25 / f64::from(self.state.camera.zoom.max(1e-3));
        for segment in geometry.segments() { segment.flatten(tolerance, |p, _| points.push(Point { x: p[0] as f32, y: p[1] as f32 })); }
        points
    }
    pub(super) fn paint_path(&mut self, path: layer_core::authored::vector::PathItem) -> Result<(), String> {
        let document = self.engine.document();
        let target = document.drawing_content().ok_or("Select a drawing layer")?;
        let (edits, operation) = document.paint_path_plan(target, path).map_err(|refusal| super::layer_conversions::conversion_refusal_text(refusal, self.localization()).to_string())?;
        self.engine.insert_with_operations(edits, vec![(target, operation)], None).map_err(|e| e.to_string())
    }
    fn commit_vector_figure(&mut self, layer: layer_core::authored::OccurrenceHandle, figure: &Figure) -> Result<(), String> {
        if self.engine.document().is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
        let doc = self.engine.document();
        let object = doc.scene().vector_layer(layer).ok_or("The vector layer was removed")?;
        let id = object.allocate_id();
        let path = self.figure_path(figure, id)?;
        let edit = doc.vector_object_edit(layer, object.insert_front(vec![layer_core::authored::vector::Record::Path(path)])).map_err(|e| e.to_string())?;
        self.layer_edit(edit)?;
        self.select_vector_items(layer, [id].into()).map(|_| ())
    }
    /// Local-space operation snapshot; pointer moves only update the guide.
    pub(super) fn current_figure(&self) -> Option<Figure> {
        let LayerCanvasTool::Figure { shape, paint } = self.layer_interaction.tool else {
            return None;
        };
        let [start, end] = self.layer_interaction.path.as_slice() else {
            return None;
        };
        let end = if self.interaction.modifiers.shift {
            shape.constrained_end(*start, *end)
        } else {
            *end
        };
        let doc = self.engine.document();
        let (alpha_locked, offset) = match self.vector_figure_layer() {
            Some(layer) => { let [x, y] = doc.scene().occurrence_offset64(layer); (false, Point { x: x as f32, y: y as f32 }) }
            None => {
                let id = doc.drawing_content()?;
                (doc.scene().occurrence(doc.target_owner(id)?)?.alpha_locked, layer_core::offsets::point(doc.target_offset(id)))
            }
        };
        let local = |p: Point| Point {
            x: p.x - offset.x,
            y: p.y - offset.y,
        };
        let colors = self.figure_colors(paint)?;
        Some(Figure {
            shape,
            paint,
            start: local(*start),
            end: local(end),
            width: self.state.brush.diameter,
            colors,
            alpha_locked,
            erase: self.state.colors.transparent(),
        })
    }

    pub(super) fn commit_figure(&mut self) -> Result<(), String> {
        let Some(figure) = self.current_figure() else {
            return Ok(());
        };
        let dx = (figure.end.x - figure.start.x).abs();
        let dy = (figure.end.y - figure.start.y).abs();
        let threshold = (0.5 / self.state.camera.zoom).max(0.001);
        if (figure.shape == FigureShape::Line && dx.hypot(dy) < threshold)
            || (figure.shape != FigureShape::Line && dx.min(dy) < threshold)
        {
            return Ok(());
        }
        if let Some(layer) = self.vector_figure_layer() { return self.commit_vector_figure(layer, &figure); }
        if self.figure_options.needs_path(figure.shape) {
            if self.state.colors.transparent() { return Err(self.localization().text(MessageId::COMMANDS_PATHS_NEED_A_COLOR).to_string()); }
            let path = self.figure_path(&figure, layer_core::authored::vector::ItemId(1))?;
            return self.paint_path(path);
        }
        let both = figure.paint == FigurePaint::Both;
        let colors = if both {
            [self.state.colors.foreground, self.state.colors.background]
        } else {
            [self.state.colors.definition(); 2]
        };
        self.paint_operation(
            self.engine.document().working.selection.clone(),
            RasterOperationKind::Figure(figure),
            &colors[..if both { 2 } else { 1 }],
        )
    }
}
