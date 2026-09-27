//! The canvas action bar: the next steps for the object being edited, shown beside it.
use super::*;
use layer_core::{Point, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasBarKind {
    Placement,
    Transform,
    Polygon,
    Selection,
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

/// Short labels for commands that appear on the bar; empty shows the icon alone.
fn short_label(command: CommandId) -> &'static str {
    match command {
        CommandId::ApplyTransform => "Apply",
        CommandId::CancelTransform | CommandId::CancelSelection => "Cancel",
        CommandId::PlacementOriginalSize => "Original Size",
        CommandId::TransformFree => "Free",
        CommandId::TransformUniform => "Uniform",
        CommandId::TransformNearest => "Nearest",
        CommandId::WarpGridThree => "3 × 3",
        CommandId::WarpGridFour => "4 × 4",
        CommandId::WarpGridFive => "5 × 5",
        CommandId::TransformFlipHorizontal
        | CommandId::TransformFlipVertical
        | CommandId::TransformRotateLeft
        | CommandId::TransformRotateRight => "",
        CommandId::ResetTransform => "Reset",
        CommandId::RemoveSelectionPoint => "Remove Point",
        CommandId::Deselect => "Deselect",
        CommandId::InvertSelection => "Invert",
        CommandId::MaskSelection => "Mask",
        CommandId::FillSelection => "Fill",
        CommandId::SaveSelectionLayer => "Save",
        CommandId::CompleteSelection => "Finish",
        _ => command.label(),
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct CanvasBarKey {
    visible: bool,
    kind: CanvasBarKind,
    toolbar: ToolbarContext,
    transaction: u64,
    anchor: Option<[f32; 4]>,
    flags: Vec<(CommandId, bool, bool, Option<std::borrow::Cow<'static, str>>)>,
}

#[derive(Default)]
pub(super) struct CanvasBarState {
    key: Option<CanvasBarKey>,
    generation: u64,
    selection: Option<(layer_core::Selection, Option<[f32; 4]>)>,
    tool: Option<LayerCanvasTool>,
    armed: bool,
    history: bool,
}
impl CanvasBarState {
    pub(super) fn history_step(&mut self) {
        self.history = true;
    }
}

fn same_selection(a: &layer_core::Selection, b: &layer_core::Selection) -> bool {
    let shape = match (&a.shape, &b.shape) {
        (layer_core::SelectionShape::Contours(a), layer_core::SelectionShape::Contours(b)) => std::sync::Arc::ptr_eq(a, b),
        (layer_core::SelectionShape::Pixels(a), layer_core::SelectionShape::Pixels(b)) => std::sync::Arc::ptr_eq(a, b),
        _ => false,
    };
    shape && a.affine.0.map(f32::to_bits) == b.affine.0.map(f32::to_bits) && a.inverted == b.inverted
}

struct Plan {
    kind: CanvasBarKind,
    label: Option<String>,
    items: Vec<CommandId>,
    completion: Vec<CommandId>,
    placement: Option<CanvasBarPlacement>,
}

impl<R: CanvasRenderer> UiSession<R> {
    fn track_selection(&mut self) {
        let tool = self.layer_interaction.tool;
        if self.canvas_bar.tool.replace(tool).is_some_and(|previous| previous != tool) {
            self.canvas_bar.armed = false;
        }
        let history = std::mem::take(&mut self.canvas_bar.history);
        let selection = self.engine.document().selection.as_ref();
        let previous = self.canvas_bar.selection.as_ref().map(|s| &s.0);
        if (selection.is_none() && previous.is_none()) || selection.zip(previous).is_some_and(|(a, b)| same_selection(a, b)) {
            return;
        }
        self.canvas_bar.armed = selection.is_some() && !history;
        self.canvas_bar.selection = selection.map(|selection| {
            let bounds = (!selection.inverted)
                .then(|| selection.bounds())
                .filter(|b| !b.is_empty())
                .map(|b| [b.min.x, b.min.y, b.max.x, b.max.y]);
            (selection.clone(), bounds)
        });
    }

    fn selection_plan(&self) -> Option<Plan> {
        let doc = self.engine.document();
        doc.selection.as_ref()?;
        if doc.active_mask || self.selection_masks.target().is_some() {
            return None;
        }
        let tool = self.layer_interaction.tool;
        let offered = tool.selection_tool().is_some()
            || tool == LayerCanvasTool::Move
            || (self.canvas_bar.armed && !tool.draws());
        if !offered {
            return None;
        }
        let edge = matches!(tool.selection_tool(), Some(SelectionTool::Tonal | SelectionTool::Brush))
            || self.canvas_bar.selection.as_ref().is_none_or(|s| s.1.is_none());
        Some(Plan {
            kind: CanvasBarKind::Selection,
            label: None,
            items: vec![
                CommandId::Deselect,
                CommandId::InvertSelection,
                CommandId::ScaleRotate,
                CommandId::MaskSelection,
                CommandId::FillSelection,
                CommandId::QuickMask,
                CommandId::SaveSelectionLayer,
            ],
            completion: Vec::new(),
            placement: edge.then_some(CanvasBarPlacement::BottomEdge),
        })
    }

    fn canvas_bar_plan(&self) -> Option<Plan> {
        if !self.operation.active() {
            let polygon = self.layer_interaction.tool
                == (LayerCanvasTool::Selection { kind: SelectionTool::Polygon })
                && !self.layer_interaction.path.is_empty();
            if !polygon {
                return self.selection_plan();
            }
            return Some(Plan {
                kind: CanvasBarKind::Polygon,
                label: None,
                items: vec![CommandId::RemoveSelectionPoint],
                completion: vec![CommandId::CancelSelection, CommandId::CompleteSelection],
                placement: Some(CanvasBarPlacement::BottomEdge),
            });
        }
        let completion = vec![CommandId::CancelTransform, CommandId::ApplyTransform];
        let count = self.operation.placement_count();
        Some(Plan {
            kind: if self.operation.placing() { CanvasBarKind::Placement } else { CanvasBarKind::Transform },
            label: (count > 1).then(|| format!("{count} images")),
            items: self.state.tool_actions.iter().map(|a| a.command).filter(|id| !completion.contains(id)).collect(),
            completion,
            placement: None,
        })
    }

    fn canvas_bar_anchor(&self, kind: CanvasBarKind) -> Option<[f32; 4]> {
        match kind {
            CanvasBarKind::Placement | CanvasBarKind::Transform => self.transform_document_bounds(),
            CanvasBarKind::Polygon => None,
            CanvasBarKind::Selection => self.canvas_bar.selection.as_ref().and_then(|s| s.1),
        }
    }

    pub(super) fn canvas_bar_contact(&self) -> bool {
        self.interaction.pointer.is_some() || self.touch.is_active()
    }

    fn published(&self, id: CommandId) -> CommandState {
        self.state.commands.iter().find(|c| c.id == id).cloned().unwrap_or_else(|| self.command(id))
    }

    pub(super) fn update_canvas_bar(&mut self) -> bool {
        if self.canvas_bar_contact() || self.operation.dragging() {
            return false;
        }
        self.track_selection();
        let visible = self.state.workspace.layout.canvas_bar;
        let Some(mut plan) = self
            .canvas_bar_plan()
            .filter(|plan| visible || !plan.completion.is_empty())
            .filter(|plan| !plan.items.is_empty() || !plan.completion.is_empty())
        else {
            self.canvas_bar.key = None;
            return self.state.canvas_bar.take().is_some();
        };
        if !visible {
            plan.items.clear();
        }
        let key = CanvasBarKey {
            visible,
            kind: plan.kind,
            toolbar: ToolbarContext { generation: 0, ..self.state.toolbar_context() },
            transaction: self.operation.serial(),
            anchor: self.canvas_bar_anchor(plan.kind),
            flags: plan
                .items
                .iter()
                .chain(&plan.completion)
                .filter_map(|id| self.state.commands.iter().find(|c| c.id == *id))
                .map(|c| (c.id, c.enabled, c.selected, c.disabled_reason.clone()))
                .collect(),
        };
        if self.canvas_bar.key.as_ref() == Some(&key) {
            return false;
        }
        let previous = self.canvas_bar.key.replace(key.clone());
        if previous.is_none_or(|p| p.kind != key.kind || p.toolbar != key.toolbar || p.transaction != key.transaction) {
            self.canvas_bar.generation += 1;
        }
        let mut items: Vec<CanvasBarItem> = Vec::new();
        for &id in &plan.items {
            let state = self.published(id);
            let group = ToolSettingAction { command: id, checkable: state.checkable }.group();
            match (group, items.last_mut().map(|item| &mut item.option)) {
                (Some(group), Some(ToolOption::Choice { id: choice, items, .. })) if *choice == group.id() => {
                    items.push(state.choice_item(short_label(id)));
                }
                (Some(group), _) => items.push(CanvasBarItem {
                    option: ToolOption::Choice {
                        id: group.id(),
                        label: group.label(),
                        segmented: group.segmented(),
                        items: vec![state.choice_item(short_label(id))],
                    },
                    label: group.label(),
                }),
                (None, _) => items.push(self.canvas_bar_action(id)),
            }
        }
        self.state.canvas_bar = Some(CanvasBarView {
            context: CanvasBarContext {
                generation: self.canvas_bar.generation,
                kind: plan.kind,
            },
            label: plan.label,
            items,
            completion: plan.completion.iter().map(|&id| self.canvas_bar_action(id)).collect(),
            placement: plan.placement.unwrap_or(if visible {
                CanvasBarPlacement::NearObject
            } else {
                CanvasBarPlacement::BottomEdge
            }),
            anchor: key.anchor,
        });
        true
    }

    fn canvas_bar_action(&self, id: CommandId) -> CanvasBarItem {
        let state = self.published(id);
        CanvasBarItem {
            option: ToolOption::Action {
                checkable: state.checkable,
                state,
            },
            label: short_label(id),
        }
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
    (1..=measure.items.len()).take_while(|&shown| bar_width(measure, shown) <= width).count()
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
    fn canvas_bar_area(&self, layout: &ResolvedLayout, viewport: [f32; 2], width: f32) -> Bounds {
        let mut area = layout.work_area;
        let status = layout.status;
        if status.height > 0. && status.y < area.y + area.height {
            area.height = (status.y - area.y).max(0.);
        }
        if area.width < width + 2. * CANVAS_BAR_MARGIN {
            area.x = 0.;
            area.width = viewport[0];
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
            CanvasBarKind::Polygon => return None,
            CanvasBarKind::Selection => {
                let [x0, y0, x1, y1] = self.canvas_bar.selection.as_ref()?.1?;
                let to_logical = self.document_to_logical();
                [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|[x, y]| to_logical(Point { x, y })).to_vec()
            }
        };
        let b = Rect::around(points.into_iter().map(|[x, y]| Point { x, y }))
            .outset(crate::session::rulers::HIT_DISTANCE);
        (!b.is_empty()).then_some(Bounds {
            x: b.min.x,
            y: b.min.y,
            width: b.max.x - b.min.x,
            height: b.max.y - b.min.y,
        })
    }

    /// Place the bar from host-measured control sizes. Stale contexts return None.
    pub fn canvas_bar_layout(&self, measure: &CanvasBarMeasure) -> Option<CanvasBarLayout> {
        let bar = self.state.canvas_bar.as_ref()?;
        if bar.context != measure.context || measure.items.len() != bar.items.len() {
            return None;
        }
        let viewport = self.logical_viewport?;
        let layout = self.layout(viewport);
        let area = self.canvas_bar_area(&layout, viewport, bar_width(measure, 0));
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

    /// The menu of a bar choice shown as a dropdown.
    pub fn canvas_bar_choice_menu(&self, context: CanvasBarContext, id: &str) -> Option<ContextMenu> {
        let bar = self.state.canvas_bar.as_ref().filter(|bar| bar.context == context)?;
        let wrap = |action: UiAction| UiAction::CanvasBarEdit { context, action: Box::new(action) };
        bar.items.iter().find_map(|item| match &item.option {
            ToolOption::Choice { id: choice, label, items, .. } if *choice == id => Some(ContextMenu {
                title: (*label).into(),
                sections: vec![choice_items(items, &wrap)],
            }),
            _ => None,
        })
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
            ToolOption::Choice { label, items, .. } => {
                vec![ContextMenuItem::submenu(label, vec![choice_items(items, &wrap)])]
            }
            ToolOption::Numeric(_) | ToolOption::Range { .. } => Vec::new(),
        });
        let toggle = ContextMenuItem {
            selected: Some(self.state.workspace.layout.canvas_bar),
            ..ContextMenuItem::command(
                CommandId::ShowCanvasActionBar.label(),
                UiAction::Invoke { command: CommandId::ShowCanvasActionBar },
            )
        };
        let mut sections = vec![overflow.collect::<Vec<_>>()];
        if bar.context.kind == CanvasBarKind::Selection {
            sections.extend(self.selection_menu(SelectionMenu::Selection).sections);
        }
        sections.push(vec![toggle]);
        Some(
            ContextMenu {
                title: "More".into(),
                sections,
            }
            .with_shortcuts(&self.state.settings, self.state.platform),
        )
    }
}

fn choice_items(items: &[ToolSetItem], wrap: &impl Fn(UiAction) -> UiAction) -> Vec<ContextMenuItem> {
    items
        .iter()
        .map(|i| ContextMenuItem {
            selected: Some(i.selected),
            ..ContextMenuItem::command(i.label, wrap(i.action.clone()))
        })
        .collect()
}
