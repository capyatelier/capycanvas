//! Figure tools use the same GPU paint operation and selection path as fills.
use crate::*;
use layer_core::{Figure, RasterOperationKind, Point};
use layer_render::CanvasRenderer;

pub(crate) fn modes(shape: FigureShape, paint: FigurePaint, localizer: &crate::localization::Localizer) -> Vec<ToolSetItem> {
    let item = |label, icon, shape, paint, selected| ToolSetItem { enabled: true,
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
                _ => icon,
            };
            item(label, icon, shape, p, p == paint)
        })
        .collect()
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
        let layer = doc.scene().occurrence(doc.target_owner(id)?)?;
        let offset = layer_core::offsets::point(doc.target_offset(id));
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
            *value = definition.linear_in(doc.composition().color.space).ok()?;
            value[3] *= self.state.brush.opacity;
        }
        Some(Figure {
            shape,
            paint,
            start: local(*start),
            end: local(end),
            width: self.state.brush.diameter,
            colors,
            alpha_locked: layer.alpha_locked,
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
