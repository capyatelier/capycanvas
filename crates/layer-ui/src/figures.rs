//! Figure tools use the same GPU paint operation and selection path as fills.
use crate::*;
use layer_core::{Figure, LayerOperationKind, Point};
use layer_render::CanvasRenderer;

pub(crate) fn tool_set(shape: FigureShape, paint: FigurePaint) -> ToolSetView {
    let item = |label, icon, shape, paint, selected| ToolSetItem {
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
    let shapes = [
        (FigureShape::Line, "Line", "line"),
        (FigureShape::Rectangle, "Rectangle", "rectangle"),
        (FigureShape::Ellipse, "Ellipse", "ellipse"),
    ];
    let icon = shapes[shape as usize].2;
    ToolSetView {
        groups: shapes
            .into_iter()
            .map(|(s, label, icon)| {
                item(
                    label,
                    icon,
                    s,
                    if s == FigureShape::Line {
                        FigurePaint::Outline
                    } else {
                        paint
                    },
                    s == shape,
                )
            })
            .collect(),
        subtools: [
            (FigurePaint::Outline, "Outline"),
            (FigurePaint::Fill, "Fill"),
            (FigurePaint::Both, "Outline + fill"),
        ]
        .into_iter()
        .filter(|(p, _)| shape != FigureShape::Line || *p == FigurePaint::Outline)
        .map(|(p, label)| {
            let icon = match (shape, p) {
                (FigureShape::Rectangle, FigurePaint::Fill) => "rectangle-fill",
                (FigureShape::Rectangle, FigurePaint::Both) => "rectangle-both",
                (FigureShape::Ellipse, FigurePaint::Fill) => "ellipse-fill",
                (FigureShape::Ellipse, FigurePaint::Both) => "ellipse-both",
                _ => icon,
            };
            item(label, icon, shape, p, p == paint)
        })
        .collect(),
    }
}

impl<B: CanvasRenderer> UiSession<B> {
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
        let id = doc.drawing_content()?;
        let layer = doc.layer(id)?;
        let offset = doc.layer_offset(id);
        let local = |p: Point| Point {
            x: p.x - offset.x,
            y: p.y - offset.y,
        };
        let definitions = [
            if paint == FigurePaint::Both {
                self.state.colors.foreground
            } else {
                self.state.colors.definition()
            },
            self.state.colors.background,
        ];
        let mut colors = [[0.; 4]; 2];
        for (value, definition) in colors.iter_mut().zip(definitions) {
            *value = definition.linear_in(doc.color.space).ok()?;
            value[3] *= self.state.brush.opacity;
        }
        Some(Figure {
            shape,
            paint,
            start: local(*start),
            end: local(end),
            width: self.state.brush.diameter,
            colors,
            alpha_locked: layer.properties.alpha_locked,
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
        self.paint_operation(
            self.engine.document().selection.clone(),
            LayerOperationKind::Figure(figure),
        )
    }
}
