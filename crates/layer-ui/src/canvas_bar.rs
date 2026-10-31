//! The canvas action bar: the next steps for the object being edited, shown beside it.
use super::*;
use layer_core::authored::SourceTarget;
use layer_core::{Point, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasBarKind {
    Picker,
    Placement,
    Transform,
    Polygon,
    Selection,
    QuickMask,
    SelectionLayer,
    LayerMask,
    Guide,
    Crop,
    CloneSource,
}
impl CanvasBarKind {
    /// Kinds that keep their completion at the bottom edge while the bar is turned off.
    fn essential(self) -> bool {
        matches!(self, Self::Placement | Self::Transform | Self::Polygon | Self::Crop | Self::Picker)
    }
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

/// A bar item's dropdown. Hosts open it through `canvas_bar_choice_menu`
/// with this id; a host that ignores it runs the item's primary command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasBarMenu {
    Copy,
    Adjust,
    Mask,
    Refine,
    WarpGrid,
}
impl CanvasBarMenu {
    pub fn id(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Adjust => "adjust",
            Self::Mask => "mask",
            Self::Refine => "refine",
            Self::WarpGrid => "warp_grid",
        }
    }
    pub fn label(self, localizer: &Localizer) -> std::sync::Arc<str> {
        localizer.text(match self {
            Self::Copy => MessageId::TOOLBAR_COPY,
            Self::Adjust => MessageId::TOOLBAR_ADJUST,
            Self::Mask => MessageId::TOOLBAR_MASK,
            Self::Refine => MessageId::TOOLBAR_REFINE,
            Self::WarpGrid => MessageId::TOOL_ACTION_GROUP_TRANSFORM_WARP_GRID,
        })
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Adjust => "add-filter",
            Self::Mask => "mask",
            Self::Refine => "feather",
            Self::WarpGrid => "warp",
        }
    }
    /// The command a host that does not open the menu runs.
    fn primary(self) -> Option<CommandId> {
        match self {
            Self::Copy => Some(CommandId::CopySelectionToLayer),
            Self::Mask => Some(CommandId::MaskSelection),
            Self::Refine => Some(CommandId::FeatherSelection),
            Self::Adjust | Self::WarpGrid => None,
        }
    }
    /// Commands whose state the menu button shows.
    fn shown(self) -> &'static [CommandId] {
        match self {
            Self::WarpGrid => &WARP_GRIDS,
            Self::Copy | Self::Adjust | Self::Mask | Self::Refine => &[],
        }
    }
}

const WARP_GRIDS: [CommandId; 3] = [CommandId::WarpGridThree, CommandId::WarpGridFour, CommandId::WarpGridFive];
const TURNS: [CommandId; 3] = [CommandId::TransformFlipVertical, CommandId::TransformRotateLeft, CommandId::TransformRotateRight];
const CLONE_MORE: [CommandId; 2] = [CommandId::CloneFlipVertical, CommandId::CloneResetOffset];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanvasBarItem {
    /// The primary command, or for a menu without one, an empty choice named
    /// by the menu id.
    pub option: ToolOption,
    /// Short text shown beside the icon where space allows.
    pub label: std::sync::Arc<str>,
    /// The step that finishes the edit, drawn in the accent color.
    pub accent: bool,
    pub menu: Option<CanvasBarMenu>,
    /// The menu's icon, for hosts that draw a menu item as a menu button.
    pub icon: Option<&'static str>,
    /// The host shows the icon alone; `label` then names a menu in its
    /// tooltip and accessibility label.
    pub icon_only: bool,
    /// A divider precedes the item.
    pub separated: bool,
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
            ToolOption::Choice { items, .. } | ToolOption::Checklist { items, .. } => items.iter().map(|i| i.action.clone()).collect(),
            ToolOption::Gradient(_) | ToolOption::Numeric(_) | ToolOption::Range { .. } => Vec::new(),
        })
    }
    pub(crate) fn allows(&self, action: &UiAction) -> bool {
        self.actions().any(|a| a == *action)
    }
}

/// Short labels for commands that appear on the bar; empty shows the icon alone.
pub(crate) fn short_label(command: CommandId, localizer: &Localizer) -> std::sync::Arc<str> {
    localizer.text(match command {
        CommandId::WarpGridThree => MessageId::TOOLBAR_3_3,
        CommandId::WarpGridFour => MessageId::TOOLBAR_4_4,
        CommandId::WarpGridFive => MessageId::TOOLBAR_5_5,
        CommandId::TransformFlipHorizontal
        | CommandId::TransformFlipVertical
        | CommandId::TransformRotateLeft
        | CommandId::TransformRotateRight
        | CommandId::TransformSnapping
        | CommandId::TransformUniform
        | CommandId::TransformPerspective
        | CommandId::WarpSelectPoints
        | CommandId::ApplyTransform
        | CommandId::CancelTransform
        | CommandId::ScaleRotate
        | CommandId::FillSelection
        | CommandId::ClearSelected
        | CommandId::InvertSelection
        | CommandId::Deselect
        | CommandId::FillSelectionMask
        | CommandId::ClearSelectionMask
        | CommandId::SaveSelectionLayer
        | CommandId::InvertSelectionLayer
        | CommandId::InvertLayerMask
        | CommandId::LayerMaskEnabled
        | CommandId::RemoveSelectionPoint
        | CommandId::CancelSelection
        | CommandId::CancelPath
        | CommandId::CompleteSelection
        | CommandId::FinishPath
        | CommandId::DeleteRuler
        | CommandId::SnapRulers
        | CommandId::PlacementOriginalSize => return "".into(),
        CommandId::LoadSelectionLayer => MessageId::TOOLBAR_LOAD,
        CommandId::ApplyLayerMask => MessageId::TOOLBAR_APPLY_MASK,
        CommandId::GrowSelection => MessageId::TOOLBAR_GROW,
        CommandId::ShrinkSelection => MessageId::TOOLBAR_SHRINK,
        CommandId::FeatherSelection => MessageId::TOOLBAR_FEATHER,
        CommandId::BorderSelection => MessageId::TOOLBAR_BORDER,
        CommandId::SmoothSelection => MessageId::TOOLBAR_SMOOTH,
        CommandId::TransformSelectionOutline => MessageId::TOOLBAR_TRANSFORM_OUTLINE,
        CommandId::CropRatioFree => MessageId::TOOLBAR_FREE,
        CommandId::CropRatioOriginal => MessageId::TOOLBAR_ORIGINAL,
        CommandId::CropRatioSquare => MessageId::TOOLBAR_1_1,
        CommandId::CropRatioFourFive => MessageId::TOOLBAR_4_5,
        CommandId::CropRatioTwoThree => MessageId::TOOLBAR_2_3,
        CommandId::CropRatioFiveSeven => MessageId::TOOLBAR_5_7,
        CommandId::CropRatioSixteenNine => MessageId::TOOLBAR_16_9,
        CommandId::CropSwapOrientation => return "".into(),
        CommandId::CropFitContent => MessageId::TOOLBAR_FIT_CONTENT,
        CommandId::StraightenToGuide => MessageId::TOOLBAR_STRAIGHTEN,
        CommandId::CloneAligned => MessageId::TOOLBAR_ALIGNED,
        CommandId::CloneFlipHorizontal | CommandId::CloneFlipVertical => return "".into(),
        _ => return command.localized_label(localizer),
    })
}

#[derive(Clone, PartialEq)]
enum CanvasBarCaption {
    Message(MessageId),
    Editing { message: MessageId, name: std::sync::Arc<str> },
    Layers(usize),
}
impl CanvasBarCaption {
    fn resolve(&self, localizer: &Localizer) -> String {
        let mut args = crate::localization::FluentArgs::new();
        let message = match self {
            Self::Message(message) => return localizer.text(*message).to_string(),
            Self::Editing { message, name } => {
                args.set("name", name.as_ref());
                *message
            }
            Self::Layers(count) => {
                args.set("count", *count as i64);
                MessageId::TOOLBAR_LAYER_COUNT
            }
        };
        localizer.format(message, &args)
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct CanvasBarKey {
    sample_width: Option<u32>,
    language: UiLanguage,
    visible: bool,
    kind: CanvasBarKind,
    toolbar: ToolbarContext,
    transaction: u64,
    guide: Option<u64>,
    label: Option<CanvasBarCaption>,
    narrow: bool,
    anchor: Option<[f32; 4]>,
    flags: Vec<(CommandId, bool, bool, Option<std::sync::Arc<str>>)>,
}

#[derive(Default)]
pub(super) struct CanvasBarState {
    key: Option<CanvasBarKey>,
    generation: u64,
    selection: Option<(layer_core::Selection, Option<[f32; 4]>)>,
    tool: Option<LayerCanvasTool>,
    armed: bool,
    history: bool,
    hides: u32,
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

#[derive(Clone, Copy)]
enum PlanItem {
    Command(CommandId),
    Menu(CanvasBarMenu),
    /// A command shown as a plain button with its own label.
    Button(CommandId, MessageId),
    /// Separates the items before it from the items after it.
    Divider,
}
impl PlanItem {
    /// The commands whose published state the item shows.
    fn commands(self) -> impl Iterator<Item = CommandId> {
        let (primary, shown) = match self {
            Self::Command(id) | Self::Button(id, _) => (Some(id), &[][..]),
            Self::Menu(menu) => (menu.primary(), menu.shown()),
            Self::Divider => (None, &[][..]),
        };
        primary.into_iter().chain(shown.iter().copied())
    }
}

struct Plan {
    kind: CanvasBarKind,
    label: Option<CanvasBarCaption>,
    items: Vec<PlanItem>,
    completion: Vec<PlanItem>,
    placement: Option<CanvasBarPlacement>,
}

impl<R: CanvasRenderer> UiSession<R> {
    fn track_selection(&mut self) {
        let tool = self.layer_interaction.tool;
        if self.canvas_bar.tool.replace(tool).is_some_and(|previous| previous != tool) {
            self.canvas_bar.armed = false;
        }
        let history = std::mem::take(&mut self.canvas_bar.history);
        let selection = self.engine.document().working.selection.as_ref();
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
        self.engine.document().working.selection.as_ref()?;
        let tool = self.layer_interaction.tool;
        let offered = tool.selection_tool().is_some()
            || tool == LayerCanvasTool::Move
            || (self.canvas_bar.armed && !tool.draws());
        if !offered {
            return None;
        }
        let edge = matches!(tool.selection_tool(), Some(SelectionTool::Tonal | SelectionTool::Brush))
            || self.canvas_bar.selection.as_ref().is_none_or(|s| s.1.is_none());
        let command = PlanItem::Command;
        Some(Plan {
            kind: CanvasBarKind::Selection,
            label: None,
            items: vec![
                command(CommandId::ScaleRotate),
                PlanItem::Menu(CanvasBarMenu::Copy),
                command(CommandId::FillSelection),
                command(CommandId::ClearSelected),
                PlanItem::Menu(CanvasBarMenu::Adjust),
                PlanItem::Menu(CanvasBarMenu::Mask),
                PlanItem::Divider,
                command(CommandId::InvertSelection),
                PlanItem::Menu(CanvasBarMenu::Refine),
            ],
            completion: vec![command(CommandId::Deselect)],
            placement: edge.then_some(CanvasBarPlacement::BottomEdge),
        })
    }

    /// Quick Mask, Selection Layer editing and layer-mask editing: a label,
    /// the mode's actions and its exit along the bottom edge.
    fn mode_plan(&self) -> Option<Plan> {
        let doc = self.engine.document();
        let command = PlanItem::Command;
        let (kind, label, items, exit) = match self.selection_masks.target() {
            Some(layer_core::SelectionTarget::Current) => (
                CanvasBarKind::QuickMask,
                CanvasBarCaption::Message(MessageId::TOOLBAR_QUICK_MASK),
                vec![
                    command(CommandId::InvertSelection),
                    command(CommandId::FillSelectionMask),
                    command(CommandId::ClearSelectionMask),
                    PlanItem::Menu(CanvasBarMenu::Refine),
                    command(CommandId::SaveSelectionLayer),
                ],
                PlanItem::Button(CommandId::ReturnToArtwork, MessageId::TOOLBAR_EXIT),
            ),
            Some(layer_core::SelectionTarget::Saved(id)) => (
                CanvasBarKind::SelectionLayer,
                CanvasBarCaption::Editing { message: MessageId::TOOLBAR_EDITING_LAYER, name: doc.scene().occurrence(id)?.name.clone() },
                vec![command(CommandId::LoadSelectionLayer), command(CommandId::InvertSelectionLayer)],
                PlanItem::Button(CommandId::ReturnToArtwork, MessageId::TOOLBAR_RETURN_TO_ARTWORK),
            ),
            None if matches!(doc.working.target, Some(SourceTarget::Coverage(_))) => {
                let layer = doc.scene().occurrence(doc.working.occurrence?)?;
                layer.mask.as_ref()?;
                (
                    CanvasBarKind::LayerMask,
                    CanvasBarCaption::Editing { message: MessageId::TOOLBAR_EDITING_MASK, name: layer.name.clone() },
                    vec![command(CommandId::InvertLayerMask), command(CommandId::LayerMaskEnabled), command(CommandId::ApplyLayerMask)],
                    PlanItem::Button(CommandId::EditLayerContent, MessageId::TOOLBAR_EDIT_CONTENT),
                )
            }
            None => return None,
        };
        Some(Plan {
            kind,
            label: Some(label),
            items,
            completion: vec![exit],
            placement: Some(CanvasBarPlacement::BottomEdge),
        })
    }

    /// The Clone source disc, once tapped.
    fn clone_source_plan(&self) -> Option<Plan> {
        (self.retouch.bar && self.clone_disc().is_some()).then(|| Plan {
            kind: CanvasBarKind::CloneSource,
            label: None,
            items: self.clone_actions().into_iter().map(|a| a.command).filter(|id| !CLONE_MORE.contains(id)).map(PlanItem::Command).collect(),
            completion: Vec::new(),
            placement: None,
        })
    }

    /// A guide selected with the Ruler or Move tool.
    fn guide_plan(&self) -> Option<Plan> {
        let tool = self.layer_interaction.tool;
        if !self.rulers.visible || !matches!(tool, LayerCanvasTool::Ruler { .. } | LayerCanvasTool::Move) {
            return None;
        }
        let ruler = self.selected_ruler()?;
        let straight = matches!(ruler.geometry, layer_core::RulerGeometry::Straight { .. });
        Some(Plan {
            kind: CanvasBarKind::Guide,
            label: None,
            items: [CommandId::DeleteRuler, CommandId::SnapRulers]
                .into_iter()
                .chain(straight.then_some(CommandId::StraightenToGuide))
                .map(PlanItem::Command)
                .collect(),
            completion: Vec::new(),
            placement: None,
        })
    }

    fn object_plan(&self, completion: [CommandId; 2]) -> Option<Plan> {
        let placing = self.objects.placing();
        if self.object_target().is_none() || !matches!(self.layer_interaction.tool, LayerCanvasTool::Move | LayerCanvasTool::Transform)
            || (!placing && self.engine.document().selected_objects().is_empty()) { return None; }
        let transforms = [CommandId::TransformFlipHorizontal, CommandId::PlacementOriginalSize];
        Some(Plan {
            kind: if placing { CanvasBarKind::Placement } else { CanvasBarKind::Transform },
            label: (self.engine.document().selected_objects().len() > 1).then(|| CanvasBarCaption::Layers(self.engine.document().selected_objects().len())),
            items: transforms.into_iter().map(PlanItem::Command).collect(),
            completion: if placing { completion.map(PlanItem::Command).into() } else { Vec::new() },
            placement: None,
        })
    }

    fn canvas_bar_plan(&self) -> Option<Plan> {
        if self.targeted_curve.is_some() {
            return Some(Plan {kind:CanvasBarKind::Picker,label:Some(CanvasBarCaption::Message(MessageId::RESOURCES_CURVE_TARGETED_PROMPT)),
                items:Vec::new(),completion:vec![PlanItem::Button(CommandId::Eyedropper,MessageId::COMMON_DONE)],placement:Some(CanvasBarPlacement::BottomEdge)});
        }
        if let Some(calibration)=&self.eyedropper.calibration {
            let prompt=match calibration.role {layer_core::levels::CalibrationRole::Black=>MessageId::RESOURCES_PICKER_BLACK_PROMPT,layer_core::levels::CalibrationRole::White=>MessageId::RESOURCES_PICKER_WHITE_PROMPT,_=>MessageId::RESOURCES_PICKER_PROMPT};
            return Some(Plan { kind: CanvasBarKind::Picker, label: Some(CanvasBarCaption::Message(prompt)),
                items: Vec::new(), completion: vec![PlanItem::Button(CommandId::Eyedropper, MessageId::TOOLBAR_CANCEL)],
                placement: Some(CanvasBarPlacement::BottomEdge) });
        }
        if self.content_bounds.baking() {
            return Some(Plan { kind: CanvasBarKind::Transform, label: Some(CanvasBarCaption::Message(MessageId::TRANSFORM_APPLYING)),
                items: Vec::new(), completion: vec![PlanItem::Command(CommandId::CancelTransform)],
                placement: Some(CanvasBarPlacement::BottomEdge) });
        }
        let completion = [CommandId::CancelTransform, CommandId::ApplyTransform];
        if self.cropping() {
            return Some(Plan {
                kind: CanvasBarKind::Crop,
                label: None,
                items: ToolActionGroup::CropRatio
                    .commands()
                    .iter()
                    .chain(&[CommandId::CropSwapOrientation, CommandId::CropFitContent, CommandId::CropStraighten])
                    .map(|&id| PlanItem::Command(id))
                    .collect(),
                completion: completion.map(PlanItem::Command).into(),
                placement: Some(CanvasBarPlacement::BottomEdge),
            });
        }
        if let Some(plan) = self.object_plan(completion) {
            return Some(plan);
        }
        if !self.operation.active() && self.vector_paths.drafting() {
            return Some(Plan {
                kind: CanvasBarKind::Polygon,
                label: None,
                items: vec![PlanItem::Command(CommandId::RemoveSelectionPoint)],
                completion: [CommandId::CancelPath, CommandId::FinishPath].map(PlanItem::Command).into(),
                placement: Some(CanvasBarPlacement::BottomEdge),
            });
        }
        if !self.operation.active() {
            let polygon = self.layer_interaction.tool
                == (LayerCanvasTool::Selection { kind: SelectionTool::Polygon })
                && !self.layer_interaction.path.is_empty();
            if !polygon {
                return self
                    .clone_source_plan()
                    .or_else(|| self.guide_plan())
                    .or_else(|| self.mode_plan())
                    .or_else(|| self.selection_plan());
            }
            return Some(Plan {
                kind: CanvasBarKind::Polygon,
                label: None,
                items: vec![PlanItem::Command(CommandId::RemoveSelectionPoint)],
                completion: [CommandId::CancelSelection, CommandId::CompleteSelection].map(PlanItem::Command).into(),
                placement: Some(CanvasBarPlacement::BottomEdge),
            });
        }
        let count = self.operation.placement_count();
        let label = if self.operation.outline() {
            Some(CanvasBarCaption::Message(MessageId::TOOLBAR_TRANSFORM_OUTLINE))
        } else {
            (count > 1).then_some(CanvasBarCaption::Layers(count))
        };
        let offered = |id: &CommandId| self.state.tool_actions.iter().any(|a| a.command == *id);
        let modes = [
            CommandId::TransformFree,
            CommandId::TransformDistort,
            CommandId::TransformWarp,
            CommandId::TransformUniform,
            CommandId::TransformPerspective,
        ];
        let warp = self.warp_cells().map(|_| [PlanItem::Menu(CanvasBarMenu::WarpGrid), PlanItem::Command(CommandId::WarpSelectPoints)]);
        let turns = [CommandId::TransformFlipHorizontal, CommandId::TransformSnapping];
        Some(Plan {
            kind: if self.operation.placing() { CanvasBarKind::Placement } else { CanvasBarKind::Transform },
            label,
            items: modes.iter().filter(|id| offered(id)).map(|&id| PlanItem::Command(id))
                .chain(warp.into_iter().flatten())
                .chain([PlanItem::Divider])
                .chain(turns.iter().filter(|id| offered(id)).map(|&id| PlanItem::Command(id)))
                .collect(),
            completion: completion.map(PlanItem::Command).into(),
            placement: None,
        })
    }

    /// Ordinary commands the More menu lists before the context's own menu.
    fn canvas_bar_more_sections(&self, kind: CanvasBarKind) -> Vec<Vec<ContextMenuItem>> {
        let offered = |id: &CommandId| self.state.tool_actions.iter().any(|a| a.command == *id);
        let item = |&id: &CommandId| self.bar_command_item(id);
        match kind {
            CanvasBarKind::Selection => vec![
                [CommandId::ClearOutside, CommandId::CropCanvasToSelection]
                    .iter()
                    .chain((self.layer_interaction.tool == LayerCanvasTool::Move).then_some(&CommandId::MoveLeaveCopy))
                    .map(item)
                    .collect(),
            ],
            CanvasBarKind::Transform | CanvasBarKind::Placement if self.operation.active() => {
                let interpolation: Vec<_> = ToolActionGroup::TransformInterpolation.commands().iter().filter(|id| offered(id)).map(item).collect();
                vec![TURNS.iter().filter(|id| offered(id)).map(item).collect(), self.bar_submenu(ToolActionGroup::TransformInterpolation, interpolation)]
            }
            CanvasBarKind::Transform | CanvasBarKind::Placement if self.object_target().is_some() => vec![
                TURNS.iter().map(item).collect(),
                self.bar_submenu(ToolActionGroup::TransformInterpolation, [CommandId::TransformNearest, CommandId::TransformBilinear].iter().map(item).collect()),
            ],
            CanvasBarKind::Crop => vec![
                self.bar_submenu(ToolActionGroup::CropOverlay, ToolActionGroup::CropOverlay.commands().iter().map(item).collect()),
                [CommandId::CropDeleteCroppedPixels, CommandId::ResetTransform].iter().map(item).collect(),
            ],
            CanvasBarKind::Guide => vec![vec![item(&CommandId::ShowRulers)]],
            CanvasBarKind::CloneSource => {
                vec![self.clone_actions().iter().map(|a| a.command).filter(|id| CLONE_MORE.contains(id)).map(|id| self.bar_command_item(id)).collect()]
            }
            _ => Vec::new(),
        }
    }

    fn bar_submenu(&self, group: ToolActionGroup, items: Vec<ContextMenuItem>) -> Vec<ContextMenuItem> {
        (!items.is_empty()).then(|| ContextMenuItem::submenu(&group.localized_label(self.localization()), vec![items])).into_iter().collect()
    }

    fn bar_command_item(&self, id: CommandId) -> ContextMenuItem {
        let state = self.command(id);
        ContextMenuItem {
            enabled: state.enabled,
            selected: state.checkable.then_some(state.selected),
            ..ContextMenuItem::command(state.label.as_ref(), UiAction::Invoke { command: id })
        }
    }

    fn mask_menu_sections(&self) -> Vec<Vec<ContextMenuItem>> {
        let doc = self.engine.document();
        let l = self.localization();
        let enabled = self.command(CommandId::MaskSelection).enabled;
        let masks = doc.working.occurrence.and_then(|id| Some((id, doc.scene().occurrence(id)?.mask.is_some()))).map(|(id, masked)| {
            [
                (false, if masked { MessageId::RESOURCES_LAYER_MENU_REPLACE_MASK_REVEAL_SELECTION } else { MessageId::RESOURCES_LAYER_MENU_MASK_REVEAL_SELECTION }),
                (true, if masked { MessageId::RESOURCES_LAYER_MENU_REPLACE_MASK_HIDE_SELECTION } else { MessageId::RESOURCES_LAYER_MENU_MASK_HIDE_SELECTION }),
            ]
            .map(|(hide, label)| ContextMenuItem {
                enabled,
                ..ContextMenuItem::command(l.text(label).as_ref(), UiAction::Layer { action: LayerAction::MaskSelection { id: occurrence_token(id), hide } })
            })
            .to_vec()
        });
        let saved = self.saved_selection_menu_items();
        let replace = self.replace_selection_menu_items();
        vec![
            masks.unwrap_or_default(),
            vec![ContextMenuItem { selected: None, ..self.bar_command_item(CommandId::QuickMask) }],
            std::iter::once(self.bar_command_item(CommandId::SaveSelectionLayer))
                .chain((!saved.is_empty()).then(|| ContextMenuItem::submenu(l.text(MessageId::MENU_LOAD_SELECTION).as_ref(), vec![saved])))
                .chain((!replace.is_empty()).then(|| {
                    ContextMenuItem::submenu(l.text(MessageId::MENU_REPLACE_SELECTION_LAYER_FROM_CURRENT_SELECTION).as_ref(), vec![replace])
                }))
                .collect(),
        ]
        .into_iter()
        .filter(|section| !section.is_empty())
        .collect()
    }

    /// Document-space points of the selected guide's handles.
    fn guide_handles(&self) -> Vec<Point> {
        self.selected_ruler().map_or_else(Vec::new, |ruler| {
            let (a, b) = ruler.geometry.handles();
            [Some(a), b].into_iter().flatten().collect()
        })
    }

    fn canvas_bar_anchor(&self, kind: CanvasBarKind) -> Option<[f32; 4]> {
        match kind {
            CanvasBarKind::Placement | CanvasBarKind::Transform => self.transform_document_bounds().or_else(|| self.object_frame_bounds()),
            CanvasBarKind::Polygon
            | CanvasBarKind::Picker
            | CanvasBarKind::QuickMask
            | CanvasBarKind::SelectionLayer
            | CanvasBarKind::LayerMask
            | CanvasBarKind::Crop => None,
            CanvasBarKind::Selection => self.canvas_bar.selection.as_ref().and_then(|s| s.1),
            CanvasBarKind::Guide => {
                let handles = self.guide_handles();
                let b = Rect::around(handles.iter().copied());
                (!handles.is_empty()).then_some([b.min.x, b.min.y, b.max.x, b.max.y])
            }
            CanvasBarKind::CloneSource => self.clone_disc_bounds(),
        }
    }

    pub(super) fn canvas_bar_contact(&self) -> bool {
        self.interaction.pointer.is_some() || self.touch.is_active()
    }

    fn published(&self, id: CommandId) -> CommandState {
        self.state.commands.iter().find(|c| c.id == id).cloned().unwrap_or_else(|| self.command(id))
    }

    /// Escape dismisses what the bar edits: a selected object, guide or Clone
    /// source, Quick Mask, a Selection Layer or a layer mask.
    pub(super) fn escape_canvas_bar(&mut self, modifiers: Modifiers) -> Result<Option<UiChange>, String> {
        if self.state.settings_open || self.state.customization.is_open() || self.state.customization.header_editing {
            return Ok(None);
        }
        if self.clear_object_selection()? {
            return Ok(Some(self.changed(regions::DOCUMENT | regions::COMMANDS, true)));
        }
        if self.clone_source_plan().is_some() {
            self.retouch.bar = false;
            self.refresh_commands();
            return Ok(Some(self.changed(regions::COMMANDS, true)));
        }
        if self.guide_plan().is_some() {
            self.rulers.selected = None;
            self.refresh_tools();
            self.layer_interaction.changed = true;
            return Ok(Some(self.changed(regions::DOCUMENT | regions::COMMANDS, true)));
        }
        let command = if self.selection_masks.target().is_some() { CommandId::ReturnToArtwork } else { CommandId::EditLayerContent };
        if !self.command_flags(command).0
            || self.held_shortcut_matches("escape", modifiers, true).iter().any(|b| self.binding_enabled(&b.action))
        {
            return Ok(None);
        }
        self.dispatch(UiAction::Invoke { command }).map(Some)
    }

    pub(super) fn update_canvas_bar(&mut self) -> bool {
        if self.canvas_bar_contact() || self.operation.dragging() {
            return false;
        }
        self.track_selection();
        let visible = self.state.workspace.layout.canvas_bar;
        let Some(mut plan) = self
            .canvas_bar_plan()
            .filter(|plan| visible || plan.kind.essential())
            .filter(|plan| !plan.items.is_empty() || !plan.completion.is_empty())
        else {
            self.canvas_bar.key = None;
            return self.state.canvas_bar.take().is_some();
        };
        if !visible {
            plan.items.clear();
        }
        let key = CanvasBarKey {
            sample_width: self.eyedropper.calibration.as_ref().map(|calibration| calibration.width),
            language: self.localization().language(),
            visible,
            kind: plan.kind,
            toolbar: ToolbarContext { generation: 0, ..self.state.toolbar_context() },
            transaction: if self.operation.active() { self.operation.serial() } else { 0 },
            guide: self.selected_ruler_token().filter(|_| plan.kind == CanvasBarKind::Guide),
            label: plan.label.clone(),
            narrow: self.canvas_bar_narrow(plan.kind),
            anchor: self.canvas_bar_anchor(plan.kind),
            flags: plan
                .items
                .iter()
                .chain(&plan.completion)
                .flat_map(|item| item.commands())
                .filter_map(|id| self.state.commands.iter().find(|c| c.id == id))
                .map(|c| (c.id, c.enabled, c.selected, c.disabled_reason.clone()))
                .collect(),
        };
        if self.canvas_bar.key.as_ref() == Some(&key) {
            return false;
        }
        let previous = self.canvas_bar.key.replace(key.clone());
        if previous.is_none_or(|p| {
            p.kind != key.kind || p.toolbar != key.toolbar || p.transaction != key.transaction || p.guide != key.guide
        }) {
            self.canvas_bar.generation += 1;
        }
        let mut items: Vec<CanvasBarItem> = Vec::new();
        if plan.kind == CanvasBarKind::Picker && visible {
            for option in self.state.tool_options().into_iter().filter(|option| matches!(option, ToolOption::Choice { id: "sample-size", .. })) {
                items.push(CanvasBarItem { option, label: self.localization().text(MessageId::TOOLBAR_SAMPLE_SIZE), accent: false, menu: None, icon: None, icon_only: false, separated: false });
            }
        }
        let mut separated = false;
        for &entry in &plan.items {
            let id = match entry {
                PlanItem::Divider => {
                    separated = !items.is_empty();
                    continue;
                }
                PlanItem::Command(id) => id,
                PlanItem::Menu(_) | PlanItem::Button(..) => {
                    items.push(CanvasBarItem { separated: std::mem::take(&mut separated), ..self.canvas_bar_item(entry) });
                    continue;
                }
            };
            let state = self.published(id);
            let group = ToolSettingAction { command: id, checkable: state.checkable }.group();
            match (group, items.last_mut().map(|item| &mut item.option)) {
                (Some(group), Some(ToolOption::Choice { id: current, items, .. })) if *current == group.id() => {
                    items.push(state.choice_item(self.bar_label(id)));
                }
                (Some(group), _) => items.push(CanvasBarItem {
                    option: ToolOption::Choice {
                        id: group.id(),
                        label: group.localized_label(self.localization()),
                        segmented: group.segmented() && !key.narrow,
                        labeled: false,
                        columns: None,
                        beside: None,
                        items: vec![state.choice_item(self.bar_label(id))],
                    },
                    label: group.localized_label(self.localization()),
                    accent: false,
                    menu: None,
                    icon: None,
                    icon_only: false,
                    separated: std::mem::take(&mut separated),
                }),
                (None, _) => items.push(CanvasBarItem { separated: std::mem::take(&mut separated), ..self.canvas_bar_item(entry) }),
            }
        }
        let mut completion: Vec<CanvasBarItem> = plan.completion.iter().map(|&item| self.canvas_bar_item(item)).collect();
        if let Some(first) = completion.first_mut() {
            first.separated = !items.is_empty() && first.icon_only;
        }
        self.state.canvas_bar = Some(CanvasBarView {
            context: CanvasBarContext {
                generation: self.canvas_bar.generation,
                kind: plan.kind,
            },
            label: plan.label.as_ref().map(|caption| caption.resolve(self.localization())),
            items,
            completion,
            placement: plan.placement.unwrap_or(if visible {
                CanvasBarPlacement::NearObject
            } else {
                CanvasBarPlacement::BottomEdge
            }),
            anchor: key.anchor,
        });
        true
    }

    fn bar_label(&self, id: CommandId) -> std::sync::Arc<str> {
        let short = short_label(id, self.localization());
        if short.is_empty() {
            return short;
        }
        self.object_command_label(id).unwrap_or(short)
    }

    /// Mode choices collapse into a dropdown where the work area is narrow.
    fn canvas_bar_narrow(&self, kind: CanvasBarKind) -> bool {
        matches!(kind, CanvasBarKind::Transform | CanvasBarKind::Placement)
            && self.logical_viewport.is_some_and(|viewport| self.layout(viewport).work_area.width < CANVAS_BAR_NARROW_WIDTH)
    }

    fn canvas_bar_item(&self, item: PlanItem) -> CanvasBarItem {
        let (id, label) = match item {
            PlanItem::Menu(menu) => return self.canvas_bar_menu_item(menu),
            PlanItem::Command(id) => (id, self.bar_label(id)),
            PlanItem::Button(id, label) => (id, self.localization().text(label)),
            PlanItem::Divider => unreachable!("dividers mark the next item"),
        };
        let state = self.published(id);
        CanvasBarItem {
            option: ToolOption::Action {
                checkable: state.checkable && matches!(item, PlanItem::Command(_)),
                state,
            },
            accent: matches!(
                id,
                CommandId::ApplyTransform | CommandId::CompleteSelection | CommandId::FinishPath | CommandId::ReturnToArtwork | CommandId::EditLayerContent
            ),
            menu: None,
            icon: matches!(item, PlanItem::Button(_, MessageId::TOOLBAR_CANCEL)).then_some("close"),
            icon_only: label.is_empty(),
            separated: false,
            label,
        }
    }

    fn canvas_bar_menu_item(&self, menu: CanvasBarMenu) -> CanvasBarItem {
        let current = menu.shown().iter().find(|&&id| self.published(id).selected).map(|&id| self.bar_label(id));
        CanvasBarItem {
            option: match menu.primary() {
                Some(id) => self.canvas_bar_item(PlanItem::Command(id)).option,
                None => ToolOption::Choice {
                    id: menu.id(),
                    label: menu.label(self.localization()),
                    segmented: false, labeled: false,
                    columns: None,
                    beside: None,
                    items: Vec::new(),
                },
            },
            icon_only: current.is_none(),
            label: current.unwrap_or_else(|| menu.label(self.localization())),
            accent: false,
            menu: Some(menu),
            icon: Some(menu.icon()),
            separated: false,
        }
    }

    fn near_object_bar(&self) -> bool {
        self.state.canvas_bar.as_ref().is_some_and(|b| b.placement == CanvasBarPlacement::NearObject)
    }

    pub(super) fn canvas_bar_camera_moved(&mut self) {
        if self.near_object_bar() {
            self.canvas_bar.hides = self.canvas_bar.hides.wrapping_add(1);
        }
    }

    /// Changes whenever hosts must hide the canvas bar, and is odd while it
    /// must stay hidden. A hidden bar returns once an even value has held for
    /// `CANVAS_BAR_REAPPEAR_MS`.
    pub fn canvas_bar_hold(&self) -> u32 {
        let contact = self.canvas_bar_contact() && (self.state.canvas_bar.is_none() || self.near_object_bar());
        let floating = self.workspace_drag.as_ref().is_some_and(|drag| drag.floating.is_some());
        self.canvas_bar.hides.wrapping_mul(2) | u32::from(contact || floating)
    }

    pub(super) fn canvas_bar_edit(&mut self, context: CanvasBarContext, action: UiAction) -> Result<UiChange, String> {
        let current = self.state.canvas_bar.as_ref();
        let allowed = current.is_some_and(|bar| {
            bar.context == context
                && (bar.allows(&action)
                    || bar.items.iter().filter_map(|item| item.menu).any(|menu| {
                        menu_actions(&self.canvas_bar_menu_sections(menu)).any(|a| *a == action)
                    }))
        });
        if !allowed {
            return Err(self.localization().text(MessageId::TOOLBAR_STALE_ACTION).to_string());
        }
        self.dispatch(action)
    }

    /// A bar item's dropdown, before its actions are tied to a bar context.
    fn canvas_bar_menu_sections(&self, menu: CanvasBarMenu) -> Vec<Vec<ContextMenuItem>> {
        let command = |id: CommandId| {
            let state = self.command(id);
            ContextMenuItem { enabled: state.enabled, ..ContextMenuItem::command(state.label.as_ref(), UiAction::Invoke { command: id }) }
        };
        let commands = |ids: &[CommandId]| ids.iter().map(|&id| self.bar_command_item(id)).collect::<Vec<_>>();
        let sections = match menu {
            CanvasBarMenu::Copy => vec![
                commands(&[CommandId::CopySelectionToLayer, CommandId::CutSelectionToLayer]),
                commands(&[CommandId::Copy, CommandId::CopyMerged, CommandId::Cut]),
            ],
            CanvasBarMenu::Mask => self.mask_menu_sections(),
            CanvasBarMenu::WarpGrid => vec![
                commands(&WARP_GRIDS),
                commands(ToolActionGroup::TransformWarpSplit.commands()),
                commands(&[CommandId::WarpResetGrid]),
            ],
            CanvasBarMenu::Refine => vec![
                self.refine_items(None),
                vec![ContextMenuItem {
                    label: short_label(CommandId::TransformSelectionOutline, self.localization()).to_string(),
                    ..command(CommandId::TransformSelectionOutline)
                }],
            ],
            CanvasBarMenu::Adjust => vec![self.selection_filter_items()],
        };
        ContextMenu { title: menu.label(self.localization()).to_string(), sections }
            .with_shortcuts(&self.state.settings, self.state.platform)
            .sections
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
            CanvasBarKind::Placement | CanvasBarKind::Transform => match self.transform_hull() {
                Some(hull) => self.transform_handle_points().into_iter().chain(hull).collect(),
                None => self.object_handle_points(),
            },
            CanvasBarKind::Polygon
            | CanvasBarKind::Picker
            | CanvasBarKind::QuickMask
            | CanvasBarKind::SelectionLayer
            | CanvasBarKind::LayerMask
            | CanvasBarKind::Crop => {
                return None;
            }
            CanvasBarKind::Guide => {
                let to_logical = self.document_to_logical();
                self.guide_handles().into_iter().map(to_logical).collect()
            }
            CanvasBarKind::Selection | CanvasBarKind::CloneSource => {
                let [x0, y0, x1, y1] = if kind == CanvasBarKind::Selection {
                    self.canvas_bar.selection.as_ref()?.1?
                } else {
                    self.clone_disc_bounds()?
                };
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
        let area = self.canvas_bar_area(&layout, viewport, bar_width(measure, measure.items.len().min(1)));
        let shown = fitted_items(measure, area.width - 2. * CANVAS_BAR_MARGIN);
        let obstacles: Vec<Bounds> = layout.groups.iter().filter(|g| g.floating).map(|g| g.bounds).collect();
        let window = area.width > layout.work_area.width;
        let (bounds, side) = place_canvas_bar(
            area,
            &obstacles,
            self.canvas_bar_protected(bar.context.kind),
            [bar_width(measure, shown), measure.height],
            if window { CanvasBarPlacement::BottomEdge } else { bar.placement },
        );
        Some(CanvasBarLayout { bounds, items: shown, side })
    }

    /// The menu of a bar choice or bar menu shown as a dropdown.
    pub fn canvas_bar_choice_menu(&self, context: CanvasBarContext, id: &str) -> Option<ContextMenu> {
        let bar = self.state.canvas_bar.as_ref().filter(|bar| bar.context == context)?;
        let wrap = |action: UiAction| UiAction::CanvasBarEdit { context, action: Box::new(action) };
        if let Some(menu) = bar.items.iter().filter_map(|item| item.menu).find(|menu| menu.id() == id) {
            return Some(ContextMenu {
                title: menu.label(self.localization()).to_string(),
                sections: wrap_sections(self.canvas_bar_menu_sections(menu), &wrap),
            });
        }
        bar.items.iter().find_map(|item| match &item.option {
            ToolOption::Choice { id: choice, label, items, .. } if *choice == id => Some(ContextMenu {
                title: label.to_string(),
                sections: vec![choice_items(items, &wrap)],
            }),
            _ => None,
        })
    }

    /// The More menu: items that did not fit, then the context's own menu.
    pub fn canvas_bar_menu(&self, context: CanvasBarContext, shown: usize) -> Option<ContextMenu> {
        let bar = self.state.canvas_bar.as_ref().filter(|bar| bar.context == context)?;
        let wrap = |action: UiAction| UiAction::CanvasBarEdit { context, action: Box::new(action) };
        let overflow = bar.items.iter().skip(shown).flat_map(|item| match (&item.option, item.menu) {
            (_, Some(menu)) => vec![ContextMenuItem::submenu(
                menu.label(self.localization()).as_ref(),
                wrap_sections(self.canvas_bar_menu_sections(menu), &wrap),
            )],
            (ToolOption::Action { state, checkable }, None) => vec![ContextMenuItem {
                selected: checkable.then_some(state.selected),
                enabled: state.enabled,
                ..ContextMenuItem::command(state.label.as_ref(), wrap(UiAction::Invoke { command: state.id }))
            }],
            (ToolOption::Choice { label, items, .. } | ToolOption::Checklist { label, items, .. }, None) => {
                vec![ContextMenuItem::submenu(label.as_ref(), vec![choice_items(items, &wrap)])]
            }
            (ToolOption::Gradient(_) | ToolOption::Numeric(_) | ToolOption::Range { .. }, None) => Vec::new(),
        });
        let toggle = ContextMenuItem {
            selected: Some(self.state.workspace.layout.canvas_bar),
            ..ContextMenuItem::command(
                CommandId::ShowCanvasActionBar.localized_label(self.localization()).as_ref(),
                UiAction::Invoke { command: CommandId::ShowCanvasActionBar },
            )
        };
        let extras = self.canvas_bar_more_sections(bar.context.kind);
        let context_sections = match bar.context.kind {
            CanvasBarKind::Selection => self.selection_menu(SelectionMenu::Selection).sections,
            CanvasBarKind::QuickMask | CanvasBarKind::SelectionLayer | CanvasBarKind::LayerMask => {
                self.application_menu(ApplicationMenu::Layer).sections
            }
            CanvasBarKind::Placement
            | CanvasBarKind::Picker
            | CanvasBarKind::Transform
            | CanvasBarKind::Polygon
            | CanvasBarKind::Guide
            | CanvasBarKind::Crop
            | CanvasBarKind::CloneSource => Vec::new(),
        };
        let menus: Vec<_> = bar.items.iter().filter_map(|item| item.menu).map(|menu| self.canvas_bar_menu_sections(menu)).collect();
        let offered: Vec<&UiAction> = menus.iter().flat_map(|sections| menu_actions(sections)).chain(menu_actions(&extras)).collect();
        let shown: Vec<UiAction> = bar.actions().collect();
        let mut sections = vec![overflow.collect::<Vec<_>>()];
        sections.extend(extras.clone());
        sections.extend(without_offered(context_sections, &|action| shown.contains(action) || offered.contains(&action)));
        sections.push(vec![toggle]);
        Some(
            ContextMenu {
                title: self.localization().text(MessageId::TOOLBAR_MORE).to_string(),
                sections,
            }
            .with_shortcuts(&self.state.settings, self.state.platform),
        )
    }
}

fn wrap_sections(sections: Vec<Vec<ContextMenuItem>>, wrap: &impl Fn(UiAction) -> UiAction) -> Vec<Vec<ContextMenuItem>> {
    sections
        .into_iter()
        .map(|section| {
            section
                .into_iter()
                .map(|item| ContextMenuItem {
                    action: item.action.map(wrap),
                    sections: wrap_sections(item.sections, wrap),
                    ..item
                })
                .collect()
        })
        .collect()
}

/// Drops the items the bar already offers, and the submenus and sections that
/// leaves empty.
fn without_offered(sections: Vec<Vec<ContextMenuItem>>, offered: &impl Fn(&UiAction) -> bool) -> Vec<Vec<ContextMenuItem>> {
    sections
        .into_iter()
        .map(|section| {
            section
                .into_iter()
                .filter_map(|item| {
                    if item.sections.is_empty() {
                        return item.action.as_ref().is_none_or(|action| !offered(action)).then_some(item);
                    }
                    let sections = without_offered(item.sections, offered);
                    (!sections.is_empty()).then_some(ContextMenuItem { sections, ..item })
                })
                .collect::<Vec<_>>()
        })
        .filter(|section| !section.is_empty())
        .collect()
}

fn menu_actions(sections: &[Vec<ContextMenuItem>]) -> Box<dyn Iterator<Item = &UiAction> + '_> {
    Box::new(sections.iter().flatten().flat_map(|item| item.action.iter().chain(menu_actions(&item.sections))))
}

fn choice_items(items: &[ToolSetItem], wrap: &impl Fn(UiAction) -> UiAction) -> Vec<ContextMenuItem> {
    items
        .iter()
        .map(|i| ContextMenuItem {
            selected: Some(i.selected),
            ..ContextMenuItem::command(i.label.as_ref(), wrap(i.action.clone()))
        })
        .collect()
}
