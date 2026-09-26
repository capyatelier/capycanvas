//! The canvas action bar: the next steps for the object being edited, shown
//! beside it. Items are ordinary commands, so menus and Tool Options stay
//! complete and every item keeps its shared validation and history.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasBarKind {
    Placement,
    Transform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasBarPlacement {
    NearObject,
    BottomEdge,
}

/// Identifies one bar presentation; edits from an older one are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasBarContext {
    pub generation: u64,
    pub kind: CanvasBarKind,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanvasBarItem {
    pub option: ToolOption,
    /// Short text shown beside the icon where space allows.
    pub label: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanvasBarView {
    pub context: CanvasBarContext,
    pub label: Option<String>,
    /// Priority order; trailing items overflow into More.
    pub items: Vec<CanvasBarItem>,
    /// Apply, Cancel and similar exits, shown after More and never overflowed.
    pub completion: Vec<CanvasBarItem>,
    pub placement: CanvasBarPlacement,
    /// Document-space bounds of the object, `[min_x, min_y, max_x, max_y]`.
    pub anchor: Option<[f32; 4]>,
}

impl CanvasBarView {
    fn actions(&self) -> impl Iterator<Item = UiAction> + '_ {
        self.items.iter().chain(&self.completion).flat_map(|item| match &item.option {
            ToolOption::Action { state, .. } => vec![UiAction::Invoke { command: state.id }],
            ToolOption::Choice { items, .. } => items.iter().map(|i| i.action.clone()).collect(),
            ToolOption::Numeric(_) | ToolOption::Range { .. } => Vec::new(),
        })
    }
    pub(crate) fn allows(&self, action: &UiAction) -> bool {
        self.actions().any(|a| a == *action)
    }
}

/// Short labels for commands that appear on the bar.
pub(crate) fn short_label(command: CommandId) -> &'static str {
    match command {
        CommandId::ApplyTransform => "Apply",
        CommandId::CancelTransform => "Cancel",
        CommandId::TransformAspect => "Uniform",
        CommandId::PlacementOriginalSize => "Original Size",
        CommandId::TransformFlipHorizontal => "Flip H",
        CommandId::TransformFlipVertical => "Flip V",
        CommandId::TransformRotateLeft => "−90°",
        CommandId::TransformRotateRight => "+90°",
        CommandId::ResetTransform => "Reset",
        _ => command.label(),
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct CanvasBarKey {
    preference: CanvasBarPreference,
    kind: CanvasBarKind,
    toolbar: ToolbarContext,
    transaction: u64,
    anchor: Option<[f32; 4]>,
    flags: Vec<(CommandId, bool, bool)>,
}

#[derive(Default)]
pub(super) struct CanvasBarState {
    key: Option<CanvasBarKey>,
    generation: u64,
}

struct Plan {
    kind: CanvasBarKind,
    label: Option<String>,
    items: Vec<(CommandId, bool)>,
    completion: Vec<CommandId>,
}

impl<R: CanvasRenderer> UiSession<R> {
    fn canvas_bar_plan(&self) -> Option<Plan> {
        if !self.state.platform.canvas_bar() || !self.operation.active() {
            return None;
        }
        let transform_items = vec![
            (CommandId::TransformAspect, true),
            (CommandId::TransformFlipHorizontal, false),
            (CommandId::TransformFlipVertical, false),
            (CommandId::TransformRotateLeft, false),
            (CommandId::TransformRotateRight, false),
            (CommandId::ResetTransform, false),
        ];
        let completion = vec![CommandId::CancelTransform, CommandId::ApplyTransform];
        if self.operation.placing() {
            let count = self.operation.placement_count();
            let mut items = transform_items;
            items.insert(1, (CommandId::PlacementOriginalSize, false));
            return Some(Plan {
                kind: CanvasBarKind::Placement,
                label: (count > 1).then(|| format!("{count} images")),
                items,
                completion,
            });
        }
        Some(Plan {
            kind: CanvasBarKind::Transform,
            label: None,
            items: transform_items,
            completion,
        })
    }

    fn canvas_bar_anchor(&self, kind: CanvasBarKind) -> Option<[f32; 4]> {
        match kind {
            CanvasBarKind::Placement | CanvasBarKind::Transform => self.transform_document_bounds(),
        }
    }

    pub(super) fn canvas_bar_contact(&self) -> bool {
        self.interaction.pointer.is_some() || self.touch.is_active()
    }

    pub(super) fn update_canvas_bar(&mut self) -> bool {
        if self.canvas_bar_contact() || self.operation.dragging() {
            return false;
        }
        let preference = self.state.workspace.layout.canvas_bar.clone();
        let Some(mut plan) = self
            .canvas_bar_plan()
            .filter(|plan| preference.visible || !plan.completion.is_empty())
        else {
            self.canvas_bar.key = None;
            return self.state.canvas_bar.take().is_some();
        };
        if !preference.visible {
            plan.items.clear();
        }
        let toolbar = self.state.toolbar_context();
        let commands = || plan.items.iter().map(|i| i.0).chain(plan.completion.iter().copied());
        let idle = self.require_idle().is_ok();
        let previous_flags = self.canvas_bar.key.as_ref().map(|k| k.flags.clone()).unwrap_or_default();
        let key = CanvasBarKey {
            preference: preference.clone(),
            kind: plan.kind,
            toolbar: ToolbarContext { generation: 0, ..toolbar },
            transaction: self.operation.serial(),
            anchor: self.canvas_bar_anchor(plan.kind),
            flags: commands()
                .map(|id| {
                    let (enabled, selected) = self.command_flags(id);
                    let steady = previous_flags.iter().find(|f| f.0 == id).filter(|_| !idle);
                    (id, steady.map_or(enabled, |f| f.1), selected)
                })
                .collect(),
        };
        if self.canvas_bar.key.as_ref() == Some(&key) {
            return false;
        }
        let previous = self.canvas_bar.key.replace(key.clone());
        if previous.is_none_or(|p| p.kind != key.kind || p.toolbar != key.toolbar || p.transaction != key.transaction) {
            self.canvas_bar.generation += 1;
        }
        let item = |id: CommandId, checkable: bool| {
            let mut state = self.command(id);
            state.enabled = key.flags.iter().find(|f| f.0 == id).is_some_and(|f| f.1);
            CanvasBarItem {
                option: ToolOption::Action {
                    state,
                    checkable: checkable && id.is_toggle(),
                },
                label: short_label(id),
            }
        };
        self.state.canvas_bar = Some(CanvasBarView {
            context: CanvasBarContext {
                generation: self.canvas_bar.generation,
                kind: plan.kind,
            },
            label: plan.label,
            items: plan.items.iter().map(|&(id, checkable)| item(id, checkable)).collect(),
            completion: plan.completion.iter().map(|&id| item(id, false)).collect(),
            placement: if preference.visible {
                preference.placement
            } else {
                CanvasBarPlacement::BottomEdge
            },
            anchor: key.anchor,
        });
        true
    }

    pub(super) fn canvas_bar_edit(&mut self, context: CanvasBarContext, action: UiAction) -> Result<UiChange, String> {
        let current = self.state.canvas_bar.as_ref();
        if current.is_none_or(|bar| bar.context != context || !bar.allows(&action)) {
            return Err("This action belongs to a previous canvas selection or transform".into());
        }
        self.dispatch(action)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasBarSide {
    Below,
    Above,
    BottomEdge,
}

/// Natural sizes of the host's bar controls, in logical pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasBarMeasure {
    pub context: CanvasBarContext,
    #[serde(default)]
    pub label: f32,
    pub items: Vec<f32>,
    pub completion: Vec<f32>,
    pub more: f32,
    pub height: f32,
    pub gap: f32,
    pub padding: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CanvasBarLayout {
    pub bounds: Bounds,
    /// Leading items shown on the bar; the rest are in More.
    pub items: usize,
    pub side: CanvasBarSide,
}

pub const CANVAS_BAR_MARGIN: f32 = 12.;
pub const CANVAS_BAR_NARROW_WIDTH: f32 = 600.;
/// Share of the free area an object may cover before the bar moves to the bottom edge.
pub const CANVAS_BAR_COVERAGE_LIMIT: f32 = 0.6;
/// Idle time after a contact or camera change before a hidden bar returns.
pub const CANVAS_BAR_REAPPEAR_MS: u32 = 180;

fn intersect(a: Bounds, b: Bounds) -> Option<Bounds> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(Bounds { x, y, width: right - x, height: bottom - y })
}

/// Below the protected object, else above it, else centred on the bottom edge.
pub fn place_canvas_bar(
    area: Bounds,
    obstacles: &[Bounds],
    protected: Option<Bounds>,
    size: [f32; 2],
    placement: CanvasBarPlacement,
) -> (Bounds, CanvasBarSide) {
    let margin = CANVAS_BAR_MARGIN;
    let [width, height] = [size[0].min(area.width - 2. * margin).max(0.), size[1]];
    let clamp_x = |x: f32| x.clamp(area.x + margin, (area.x + area.width - margin - width).max(area.x + margin));
    let bottom = Bounds {
        x: clamp_x(area.x + (area.width - width) * 0.5),
        y: area.y + area.height - margin - height,
        width,
        height,
    };
    let edge = (bottom, CanvasBarSide::BottomEdge);
    if placement == CanvasBarPlacement::BottomEdge || area.width < CANVAS_BAR_NARROW_WIDTH {
        return edge;
    }
    let Some(object) = protected else {
        return edge;
    };
    let Some(visible) = intersect(object, area) else {
        return edge;
    };
    if visible.width * visible.height > CANVAS_BAR_COVERAGE_LIMIT * area.width * area.height {
        return edge;
    }
    let x = clamp_x(visible.x + (visible.width - width) * 0.5);
    let fits = |y: f32| {
        let bar = Bounds { x, y, width, height };
        y >= area.y + margin
            && y + height <= area.y + area.height - margin
            && obstacles.iter().all(|o| intersect(bar, *o).is_none())
    };
    let below = object.y + object.height + margin;
    let above = object.y - margin - height;
    if fits(below) {
        (Bounds { x, y: below, width, height }, CanvasBarSide::Below)
    } else if fits(above) {
        (Bounds { x, y: above, width, height }, CanvasBarSide::Above)
    } else {
        edge
    }
}

fn fitted_items(measure: &CanvasBarMeasure, width: f32) -> usize {
    let gap = measure.gap;
    let label = if measure.label > 0. { measure.label + gap } else { 0. };
    let fixed = 2. * measure.padding
        + label
        + measure.more
        + measure.completion.iter().map(|w| w + gap).sum::<f32>();
    let mut used = fixed;
    let mut shown = 0;
    for item in &measure.items {
        if used + item + gap > width {
            break;
        }
        used += item + gap;
        shown += 1;
    }
    shown
}

fn bar_width(measure: &CanvasBarMeasure, shown: usize) -> f32 {
    let gap = measure.gap;
    let label = if measure.label > 0. { measure.label + gap } else { 0. };
    2. * measure.padding
        + label
        + measure.items[..shown].iter().map(|w| w + gap).sum::<f32>()
        + measure.more
        + measure.completion.iter().map(|w| w + gap).sum::<f32>()
}

impl<R: CanvasRenderer> UiSession<R> {
    fn canvas_bar_area(&self, layout: &ResolvedLayout) -> Bounds {
        let mut area = layout.work_area;
        let status = layout.status;
        if status.height > 0. && status.y < area.y + area.height {
            area.height = (status.y - area.y).max(0.);
        }
        area
    }

    fn canvas_bar_protected(&self, kind: CanvasBarKind) -> Option<Bounds> {
        let points: Vec<[f32; 2]> = match kind {
            CanvasBarKind::Placement | CanvasBarKind::Transform => {
                let mut points = self.transform_handle_points();
                points.extend(self.transform_hull()?);
                points
            }
        };
        let reach = crate::session::rulers::HIT_DISTANCE;
        let [mut min, mut max] = [[f32::INFINITY; 2], [f32::NEG_INFINITY; 2]];
        for [x, y] in points {
            min = [min[0].min(x), min[1].min(y)];
            max = [max[0].max(x), max[1].max(y)];
        }
        (min[0] <= max[0]).then(|| Bounds {
            x: min[0] - reach,
            y: min[1] - reach,
            width: max[0] - min[0] + 2. * reach,
            height: max[1] - min[1] + 2. * reach,
        })
    }

    /// Place the bar from host-measured control sizes. Stale contexts return None.
    pub fn canvas_bar_layout(&self, measure: &CanvasBarMeasure) -> Option<CanvasBarLayout> {
        let bar = self.state.canvas_bar.as_ref()?;
        if bar.context != measure.context || measure.items.len() != bar.items.len() {
            return None;
        }
        let layout = self.layout(self.logical_viewport?);
        let area = self.canvas_bar_area(&layout);
        let shown = fitted_items(measure, area.width - 2. * CANVAS_BAR_MARGIN);
        let obstacles: Vec<Bounds> = layout.groups.iter().filter(|g| g.floating).map(|g| g.bounds).collect();
        let (bounds, side) = place_canvas_bar(
            area,
            &obstacles,
            self.canvas_bar_protected(bar.context.kind),
            [bar_width(measure, shown), measure.height],
            bar.placement,
        );
        Some(CanvasBarLayout { bounds, items: shown, side })
    }

    /// The More menu: items that did not fit, then the context's own menu.
    pub fn canvas_bar_menu(&self, context: CanvasBarContext, shown: usize) -> Option<ContextMenu> {
        let bar = self.state.canvas_bar.as_ref().filter(|bar| bar.context == context)?;
        let wrap = |action: UiAction| UiAction::CanvasBarEdit { context, action: Box::new(action) };
        let overflow = bar.items.iter().skip(shown).flat_map(|item| match &item.option {
            ToolOption::Action { state, checkable } => vec![ContextMenuItem {
                selected: checkable.then_some(state.selected),
                enabled: state.enabled,
                ..ContextMenuItem::command(state.label, wrap(UiAction::Invoke { command: state.id }))
            }],
            ToolOption::Choice { label, items, .. } => vec![ContextMenuItem::submenu(label, vec![items
                .iter()
                .map(|i| ContextMenuItem {
                    selected: Some(i.selected),
                    ..ContextMenuItem::command(i.label, wrap(i.action.clone()))
                })
                .collect()])],
            ToolOption::Numeric(_) | ToolOption::Range { .. } => Vec::new(),
        });
        let preference = &self.state.workspace.layout.canvas_bar;
        let toggle = ContextMenuItem {
            selected: Some(preference.visible),
            ..ContextMenuItem::command(
                CommandId::ShowCanvasActionBar.label(),
                UiAction::Invoke { command: CommandId::ShowCanvasActionBar },
            )
        };
        Some(
            ContextMenu {
                title: "More".into(),
                sections: vec![overflow.collect(), vec![toggle]],
            }
            .with_shortcuts(&self.state.settings, self.state.platform),
        )
    }
}
