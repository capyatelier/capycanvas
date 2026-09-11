//! Shared ruler creation, editing and display. Hosts only render the projection.
use crate::*;
use layer_core::{Edit, Point, Rect, Ruler, RulerConstraint, RulerGeometry};
use layer_engine::{PenEvent, PenPhase};
use layer_render::{CanvasRenderer, CursorSegment};

const HIT_DISTANCE: f32 = 12.;
#[derive(Clone, Copy)]
enum Grab {
    Body,
    Start,
    End,
}
struct Drag {
    original: Ruler,
    preview: Ruler,
    existing: bool,
    part: Grab,
    press: Point,
    current: Point,
}
pub(super) struct RulerInteraction {
    pub kind: RulerKind,
    pub selected: Option<u64>,
    pub visible: bool,
    pub snapping: bool,
    drag: Option<Drag>,
}
impl Default for RulerInteraction {
    fn default() -> Self {
        Self {
            kind: RulerKind::Straight,
            selected: None,
            visible: true,
            snapping: true,
            drag: None,
        }
    }
}
pub(crate) fn tool_set(kind: RulerKind) -> ToolSetView {
    let groups = [
        (RulerKind::Straight, "Straight", "ruler"),
        (RulerKind::Parallel, "Parallel", "ruler-parallel"),
        (RulerKind::Radial, "Radial", "ruler-radial"),
    ]
    .into_iter()
    .map(|(k, label, icon)| ToolSetItem {
        label,
        icon,
        selected: k == kind,
        preview: None,
        action: UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::Ruler { kind: k },
            },
        },
    })
    .collect();
    ToolSetView {
        groups,
        subtools: Vec::new(),
    }
}

impl<B: CanvasRenderer> UiSession<B> {
    pub(super) fn ruler_reach(&self) -> f32 {
        let scale = self
            .logical_viewport
            .map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        HIT_DISTANCE * scale / self.state.camera.zoom
    }
    pub(super) fn sync_ruler_snapping(&mut self) {
        self.engine.set_ruler_snapping(
            (self.rulers.visible && self.rulers.snapping).then(|| self.ruler_reach()),
        );
    }
    pub(super) fn ruler_command(&mut self, command: CommandId) -> Result<(), String> {
        self.require_idle()?;
        match command {
            CommandId::ShowRulers => self.rulers.visible = !self.rulers.visible,
            CommandId::SnapRulers => self.rulers.snapping = !self.rulers.snapping,
            CommandId::DeleteRuler => {
                let selected = self.rulers.selected.ok_or("Select a ruler first")?;
                let mut rulers = self.engine.document().rulers.clone();
                rulers.retain(|r| r.id != selected);
                self.layer_edit(Edit::SetRulers(rulers))?;
                self.rulers.selected = None;
            }
            _ => return Err("Unknown ruler action".into()),
        }
        self.sync_ruler_snapping();
        self.refresh_tools();
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn ruler_pen(&mut self, event: PenEvent, p: Point) -> Result<(), String> {
        match event.phase {
            PenPhase::Down => {
                self.rulers.drag = None;
                self.layer_interaction.path = vec![p];
                let reach = self.ruler_reach();
                let doc = self.engine.document();
                let hit = self
                    .rulers
                    .visible
                    .then(|| {
                        doc.rulers
                            .iter()
                            .rev()
                            .filter_map(|r| {
                                let (a, b) = r.geometry.handles();
                                let distance = |v: Point| (p.x - v.x).hypot(p.y - v.y);
                                let (d, part) = if distance(a) <= reach {
                                    (distance(a), Grab::Start)
                                } else if b.is_some_and(|b| distance(b) <= reach) {
                                    (distance(b.unwrap()), Grab::End)
                                } else {
                                    (r.geometry.distance(p) + reach, Grab::Body)
                                };
                                (d <= reach * 2.).then_some((d, *r, part))
                            })
                            .min_by(|a, b| a.0.total_cmp(&b.0))
                    })
                    .flatten();
                let (original, part, existing) = if let Some((_, r, part)) = hit {
                    (r, part, true)
                } else {
                    let id = doc
                        .rulers
                        .iter()
                        .map(|r| r.id)
                        .max()
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("No ruler IDs available")?;
                    (
                        Ruler {
                            id,
                            geometry: RulerGeometry::from_drag(self.rulers.kind, p, p),
                        },
                        Grab::End,
                        false,
                    )
                };
                self.rulers.selected = Some(original.id);
                self.rulers.kind = original.geometry.kind();
                self.layer_interaction.tool = LayerCanvasTool::Ruler {
                    kind: self.rulers.kind,
                };
                self.rulers.visible = true;
                self.sync_ruler_snapping();
                self.rulers.drag = Some(Drag {
                    original,
                    preview: original,
                    existing,
                    part,
                    press: p,
                    current: p,
                });
            }
            PenPhase::Move | PenPhase::Up => {
                if let Some(drag) = &mut self.rulers.drag {
                    drag.current = p;
                }
                self.update_ruler_preview();
                if event.phase == PenPhase::Up {
                    if let Some(drag) = self.rulers.drag.take() {
                        if drag.preview.geometry.validate().is_ok()
                            && (!drag.existing || drag.preview != drag.original)
                        {
                            let mut rulers = self.engine.document().rulers.clone();
                            if let Some(r) = rulers.iter_mut().find(|r| r.id == drag.original.id) {
                                *r = drag.preview;
                            } else {
                                rulers.push(drag.preview);
                            }
                            self.layer_edit(Edit::SetRulers(rulers))?;
                        } else if !drag.existing {
                            self.rulers.selected = None;
                        }
                    }
                    self.layer_interaction.path.clear();
                }
            }
            PenPhase::Cancel => {
                self.cancel_ruler_gesture();
                self.layer_interaction.path.clear();
            }
            PenPhase::Hover => return Ok(()),
        }
        if event.phase != PenPhase::Move {
            self.refresh_tools();
            self.layer_interaction.changed = true;
        }
        Ok(())
    }
    pub(super) fn update_ruler_preview(&mut self) -> bool {
        let Some(drag) = &mut self.rulers.drag else {
            return false;
        };
        let (a, b) = drag.original.geometry.handles();
        drag.preview.geometry = if let Some(b) = b.filter(|_| !matches!(drag.part, Grab::Body)) {
            let fixed = if matches!(drag.part, Grab::Start) {
                b
            } else {
                a
            };
            let p = if self.interaction.modifiers.shift {
                FigureShape::Line.constrained_end(fixed, drag.current)
            } else {
                drag.current
            };
            let (start, end) = if matches!(drag.part, Grab::Start) {
                (p, b)
            } else {
                (a, p)
            };
            RulerGeometry::from_drag(drag.original.geometry.kind(), start, end)
        } else {
            drag.original.geometry.translated(Point {
                x: drag.current.x - drag.press.x,
                y: drag.current.y - drag.press.y,
            })
        };
        true
    }
    pub(super) fn cancel_ruler_gesture(&mut self) {
        if self.rulers.drag.take().is_some_and(|d| !d.existing) {
            self.rulers.selected = None;
        }
    }
    pub(super) fn append_ruler_overlay(&self, segments: &mut Vec<CursorSegment>) {
        if !self.rulers.visible {
            return;
        }
        let matrix = self.state.camera.view().document_to_surface;
        let scale = self
            .logical_viewport
            .map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        let map = |p: Point| {
            [
                (matrix[0] * p.x + matrix[2] * p.y + matrix[4]) / scale,
                (matrix[1] * p.x + matrix[3] * p.y + matrix[5]) / scale,
            ]
        };
        let bounds = Rect {
            min: Point::default(),
            max: Point {
                x: self.engine.document().width as f32,
                y: self.engine.document().height as f32,
            },
        };
        let line = |out: &mut Vec<CursorSegment>, from, to, solid| {
            out.push(CursorSegment {
                from,
                to,
                distance: 0.,
                marker: if solid { 1. } else { 0. },
                scale: 1.,
            })
        };
        for ruler in self
            .engine
            .document()
            .rulers
            .iter()
            .copied()
            .filter(|r| {
                self.rulers
                    .drag
                    .as_ref()
                    .is_none_or(|d| d.original.id != r.id)
            })
            .chain(self.rulers.drag.as_ref().map(|d| d.preview))
        {
            let (a, b) = ruler.geometry.handles();
            let selected = self.rulers.selected == Some(ruler.id)
                && matches!(self.layer_interaction.tool, LayerCanvasTool::Ruler { .. });
            if let Some(b) = b {
                let mut axis = RulerConstraint {
                    origin: a,
                    direction: None,
                };
                axis.resolve(b);
                if let Some([a, b]) = axis.clipped(bounds) {
                    line(segments, map(a), map(b), false);
                }
            }
            for p in [Some(a), b].into_iter().flatten() {
                let p = map(p);
                let s = if selected { 4. } else { 2. };
                for (from, to) in [
                    ([-s, -s], [s, -s]),
                    ([s, -s], [s, s]),
                    ([s, s], [-s, s]),
                    ([-s, s], [-s, -s]),
                ] {
                    line(
                        segments,
                        [p[0] + from[0], p[1] + from[1]],
                        [p[0] + to[0], p[1] + to[1]],
                        selected,
                    );
                }
                if b.is_none() {
                    line(segments, [p[0] - 12., p[1]], [p[0] + 12., p[1]], false);
                    line(segments, [p[0], p[1] - 12.], [p[0], p[1] + 12.], false);
                }
            }
        }
        if let Some(snap) = self.engine.active_ruler_constraint()
            && let Some([a, b]) = snap.clipped(bounds)
        {
            line(segments, map(a), map(b), false);
        }
    }
}
