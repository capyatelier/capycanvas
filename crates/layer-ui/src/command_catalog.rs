//! Semantic commands and command search. Catalog entries are projections of the
//! existing command/menu/tool schemas; execution always returns to dispatch.
use super::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;



/// Shared rhythm for native search surfaces; toolkit themes supply colors,
/// typography and motion. Touch hosts may increase row_height.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct CommandSearchStyle {
    pub width: i32,
    pub inset: i32,
    pub gap: i32,
    pub row_height: i32,
    pub radius: i32,
    pub top_min: i32,
    pub top_max: i32,
}
pub const COMMAND_SEARCH_STYLE: CommandSearchStyle = CommandSearchStyle {
    width: 560,
    inset: 12,
    gap: 8,
    row_height: 44,
    radius: 12,
    top_min: 48,
    top_max: 192,
};

impl CommandSearchStyle {
    pub fn top(&self, height: f32) -> f32 {
        (height / 5.).clamp(self.top_min as f32, self.top_max as f32)
    }
}

/// Behavior scopes, independent of brush media, cycling families and edit target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Drawing,
    Erasing,
    Blending,
    Warping,
    Selection,
    FillGradient,
    ShapesRulers,
    MoveTransform,
    ColorSampling,
    Navigation,
    Retouching,
}
impl ToolCategory {
    pub fn label(self) -> std::sync::Arc<str> { self.localized_label(&Localizer::shared(UiLanguage::English)) }
    pub fn localized_label(self, l: &Localizer) -> std::sync::Arc<str> {
        l.text(match self {
            Self::Drawing => MessageId::COMMANDS_DRAWING,
            Self::Erasing => MessageId::COMMANDS_ERASING,
            Self::Blending => MessageId::COMMANDS_BLENDING,
            Self::Warping => MessageId::COMMANDS_WARPING,
            Self::Selection => MessageId::COMMANDS_SELECTION,
            Self::FillGradient => MessageId::COMMANDS_FILL_AND_GRADIENT,
            Self::ShapesRulers => MessageId::COMMANDS_SHAPE_AND_RULER,
            Self::MoveTransform => MessageId::COMMANDS_MOVE_AND_TRANSFORM,
            Self::ColorSampling => MessageId::COMMANDS_COLOR_SAMPLING,
            Self::Navigation => MessageId::COMMANDS_NAVIGATION,
            Self::Retouching => MessageId::COMMANDS_RETOUCHING,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandFocus {
    #[default]
    Canvas,
    Palette,
    Text,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandParameter {
    pub numeric: NumericControl,
    pub value: f32,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandDescriptor {
    /// Stable wire identity, independent of labels and Rust Debug formatting.
    pub id: String,
    pub label: String,
    pub category: String,
    /// Concise behavior/scope help, or the menu location when available.
    /// Empty when there is nothing useful beyond the label; never filler.
    pub description: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub selected: bool,
    /// Only the first effective binding is shown in the compact command bar.
    pub shortcut: String,
    pub parameter: Option<CommandParameter>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CommandSearchView {
    pub query: String,
    pub results: Vec<CommandDescriptor>,
    pub selected: usize,
    pub parameter: Option<CommandDescriptor>,
    pub error: Option<String>,
    pub detail: String,
}

impl CommandSearchView {
    fn refresh_detail(&mut self) {
        let selected = self.parameter.as_ref().or_else(|| self.results.get(self.selected));
        self.detail = self
            .error
            .clone()
            .or_else(|| selected.and_then(|d| d.disabled_reason.clone()))
            .or_else(|| selected.map(|d| d.description.clone()))
            .unwrap_or_default();
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandSearchAction {
    /// Hosts report the meaningful editor surface before opening a popup.
    Focus {
        focus: CommandFocus,
    },
    /// Submit current native text, including when query publication is pending.
    Commit {
        text: String,
    },
    Query {
        text: String,
    },
    Move {
        delta: i32,
    },
    Select {
        id: String,
    },
    Execute {
        id: String,
        value: Option<String>,
    },
    Back,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryCategory { Other, Brushes }

struct Entry {
    descriptor: CommandDescriptor,
    action: Option<UiAction>,
    search: String,
    label_search: String,
    canonical_search: String,
    category: EntryCategory,
}

enum CommandSearchError {
    Numeric(NumericError),
    Message(MessageId),
    Disabled(String),
}

#[derive(Default)]
pub(super) struct CommandSearch {
    pub(super) revision: u64,
    entries: Vec<Entry>,
    recent: Vec<String>,
    epoch: u64,
    focus: CommandFocus,
    error_copy: Option<CommandSearchError>,
}

/// Existing snake_case action tags and parameter names are the public wire
/// contract. Active-layer commands omit the ephemeral layer ID; toggles omit
/// their next boolean value. Saved-selection and resource IDs remain explicit.
pub(super) fn identity(action: &UiAction) -> String {
    let mut value = serde_json::to_value(action).expect("serializable action");
    if let UiAction::Customize {
        action: CustomizationAction::SetPanelVisible { panel, .. },
    } = action
    {
        return format!("panel.visibility:{}", serde_json::to_string(panel).unwrap());
    }
    if let UiAction::Invoke { command } = action {
        return format!(
            "command.{}",
            serde_json::to_value(command).unwrap().as_str().unwrap()
        );
    }
    if let UiAction::Effect { .. } = action
        && let Some(fields) = value["action"].as_object_mut()
    {
        fields.remove("layer");
    }
    if let UiAction::Selection {
        action: SelectionAction::LoadCoverage { .. } | SelectionAction::NewLayer { .. },
    } = action
        && let Some(fields) = value["action"].as_object_mut()
    {
        fields.remove("id");
        fields.remove("parent");
    }
    if let UiAction::Layer { .. } = action {
        if let Some(fields) = value["action"].as_object_mut() {
            fields.remove("id");
            if fields
                .get("value")
                .is_some_and(serde_json::Value::is_boolean)
            {
                fields.remove("value");
            }
        }
    }
    format!("action:{}", value)
}

fn entry(
    label: impl AsRef<str>,
    category: impl AsRef<str>,
    action: UiAction,
    enabled: bool,
    selected: Option<bool>,
    settings: &Settings,
    platform: Platform,
    l: &Localizer,
) -> Entry {
    let label = label.as_ref();
    let category = category.as_ref();
    let action = crate::shortcuts::action_command(&action)
        .map(|command| UiAction::Invoke { command })
        .unwrap_or(action);
    let presentation_label = match &action {
        UiAction::Invoke { .. } => label.to_owned(),
        _ => label.to_owned(),
    };
    let label = presentation_label.as_str();
    let shortcut = settings
        .action_keys(&action, platform)
        .into_iter()
        .find(|k| k.available(platform))
        .map(|k| k.localized_label(platform, l))
        .unwrap_or_default();
    let aliases = match &action {
        UiAction::Invoke {
            command: CommandId::Settings,
        } => Some(MessageId::COMMANDS_ALIAS_SETTINGS),
        UiAction::Invoke {
            command: CommandId::Eyedropper,
        } => Some(MessageId::COMMANDS_ALIAS_EYEDROPPER),
        UiAction::Invoke {
            command: CommandId::Move,
        } => Some(MessageId::COMMANDS_ALIAS_MOVE),
        UiAction::Invoke {
            command: CommandId::ScaleRotate,
        } => Some(MessageId::COMMANDS_ALIAS_SCALE_ROTATE),
        UiAction::Invoke {
            command: CommandId::FitCanvas,
        } => Some(MessageId::COMMANDS_ALIAS_FIT_CANVAS),
        UiAction::Invoke {
            command: CommandId::Deselect,
        } => Some(MessageId::COMMANDS_ALIAS_DESELECT),
        UiAction::Invoke {
            command: CommandId::ToggleTheme,
        } => Some(MessageId::COMMANDS_ALIAS_TOGGLE_THEME),
        UiAction::Invoke {
            command: CommandId::GrowSelection,
        } => Some(MessageId::COMMANDS_ALIAS_GROW_SELECTION),
        UiAction::Invoke {
            command: CommandId::ShrinkSelection,
        } => Some(MessageId::COMMANDS_ALIAS_SHRINK_SELECTION),
        UiAction::Invoke {
            command: CommandId::FeatherSelection,
        } => Some(MessageId::COMMANDS_ALIAS_FEATHER_SELECTION),
        UiAction::Invoke {
            command: CommandId::BorderSelection,
        } => Some(MessageId::COMMANDS_ALIAS_BORDER_SELECTION),
        UiAction::Invoke {
            command: CommandId::SmoothSelection,
        } => Some(MessageId::COMMANDS_ALIAS_SMOOTH_SELECTION),
        UiAction::Invoke {
            command: CommandId::TransformSelectionOutline,
        } => Some(MessageId::COMMANDS_ALIAS_TRANSFORM_SELECTION_OUTLINE),
        UiAction::Invoke {
            command: CommandId::MoveLeaveCopy,
        } => Some(MessageId::COMMANDS_ALIAS_MOVE_LEAVE_COPY),
        UiAction::Invoke {
            command: CommandId::CopyMerged,
        } => Some(MessageId::COMMANDS_ALIAS_COPY_MERGED),
        UiAction::Invoke {
            command: CommandId::PasteImage,
        } => Some(MessageId::COMMANDS_ALIAS_PASTE_IMAGE),
        UiAction::Invoke {
            command: CommandId::NewDodgeBurnLayer,
        } => Some(MessageId::COMMANDS_ALIAS_NEW_DODGE_BURN_LAYER),
        UiAction::Invoke {
            command: CommandId::FrequencySeparation,
        } => Some(MessageId::COMMANDS_ALIAS_FREQUENCY_SEPARATION),
        _ => None,
    };
    let english = Localizer::shared(UiLanguage::English);
    let aliases = aliases.map_or_else(String::new, |id| format!("{} {}", l.text(id), english.text(id)));
    let canonical = match &action {
        UiAction::Invoke { command } => command.localized_label(&english).to_string(),
        UiAction::CycleTool { family } => family.localized_label(&english).to_string(),
        UiAction::SelectBrush { id } => tools::brush_label_localized(*id, &english).map_or_else(String::new, |label| label.to_string()),
        _ => String::new(),
    };
    Entry {
        descriptor: CommandDescriptor {
            id: identity(&action),
            label: label.into(),
            category: category.into(),
            description: action_description(&action, l),
            enabled,
            disabled_reason: None,
            selected: selected.unwrap_or(false),
            shortcut,
            parameter: None,
        },
        action: Some(action.clone()),
        search: crate::search::normalize(&format!("{label} {category} {aliases} {canonical}")),
        label_search: crate::search::normalize(label),
        canonical_search: crate::search::normalize(&canonical),
        category: EntryCategory::Other,
    }
}

fn action_description(action: &UiAction, l: &Localizer) -> String {
    use CommandId::*;
    match action {
        UiAction::Invoke { command } => match command {
            DrawingBrush => l.text(MessageId::COMMANDS_HELP_DRAWING_BRUSH).to_string(),
            Sculpt => l.text(MessageId::COMMANDS_HELP_SCULPT).to_string(),
            Pen | Pencil | Brush | Eraser | Airbrush | Decoration | Blend | Liquify => {
                l.text(MessageId::COMMANDS_HELP_PEN).to_string()
            }
            Clone => l.text(MessageId::COMMANDS_HELP_CLONE).to_string(),
            Heal => l.text(MessageId::COMMANDS_HELP_HEAL).to_string(),
            SpotHeal => l.text(MessageId::COMMANDS_HELP_SPOT_HEAL).to_string(),
            Select => l.text(MessageId::COMMANDS_HELP_SELECT).to_string(),
            SelectionBrush => l.text(MessageId::COMMANDS_HELP_SELECTION_BRUSH).to_string(),
            SelectionIntersect => l.text(MessageId::COMMANDS_HELP_SELECTION_INTERSECT).to_string(),
            SelectionVisible => l.text(MessageId::COMMANDS_HELP_SELECTION_VISIBLE).to_string(),
            SelectionEditing => l.text(MessageId::COMMANDS_HELP_SELECTION_EDITING).to_string(),
            SelectionReference => l.text(MessageId::COMMANDS_HELP_SELECTION_REFERENCE).to_string(),
            SelectionFixedRatio => l.text(MessageId::COMMANDS_HELP_SELECTION_FIXED_RATIO).to_string(),
            SelectionFixedSize => l.text(MessageId::COMMANDS_HELP_SELECTION_FIXED_SIZE).to_string(),
            QuickMask => l.text(MessageId::COMMANDS_HELP_QUICK_MASK).to_string(),
            Reselect => l.text(MessageId::COMMANDS_HELP_RESELECT).to_string(),
            SaveSelectionLayer => l.text(MessageId::COMMANDS_HELP_SAVE_SELECTION_LAYER).to_string(),
            Move => l.text(MessageId::COMMANDS_HELP_MOVE).to_string(),
            TransformAgain => l.text(MessageId::COMMANDS_TRANSFORM_AGAIN_HELP).to_string(),
            TransformSnapping => l.text(MessageId::COMMANDS_TRANSFORM_SNAPPING_HELP).to_string(),
            ScaleRotate => l.text(MessageId::COMMANDS_HELP_SCALE_ROTATE).to_string(),
            FitCanvas => l.text(MessageId::COMMANDS_HELP_FIT_CANVAS).to_string(),
            ActualPixels => l.text(MessageId::COMMANDS_HELP_ACTUAL_PIXELS).to_string(),
            FlipHorizontal | FlipVertical => l.text(MessageId::COMMANDS_HELP_FLIP_HORIZONTAL).to_string(),
            RotateLeft | RotateRight => l.text(MessageId::COMMANDS_HELP_ROTATE_LEFT).to_string(),
            UndoWorkspace => l.text(MessageId::COMMANDS_HELP_UNDO_WORKSPACE).to_string(),
            RedoWorkspace => l.text(MessageId::COMMANDS_HELP_REDO_WORKSPACE).to_string(),
            ZenMode => l.text(MessageId::COMMANDS_HELP_ZEN_MODE).to_string(),
            ShowCanvasActionBar => l.text(MessageId::COMMANDS_HELP_SHOW_CANVAS_ACTION_BAR).to_string(),
            TransformFlipHorizontal | TransformFlipVertical => l.text(MessageId::COMMANDS_HELP_TRANSFORM_FLIP_HORIZONTAL).to_string(),
            TransformRotateLeft | TransformRotateRight => l.text(MessageId::COMMANDS_HELP_TRANSFORM_ROTATE_LEFT).to_string(),
            ResetTransform => l.text(MessageId::COMMANDS_HELP_RESET_TRANSFORM).to_string(),
            RemoveSelectionPoint => l.text(MessageId::COMMANDS_HELP_REMOVE_SELECTION_POINT).to_string(),
            MaskSelection => l.text(MessageId::COMMANDS_HELP_MASK_SELECTION).to_string(),
            TransformFree | TransformUniform => l.text(MessageId::COMMANDS_HELP_TRANSFORM_FREE).to_string(),
            TransformDistort => l.text(MessageId::COMMANDS_HELP_TRANSFORM_DISTORT).to_string(),
            TransformPerspective => l.text(MessageId::COMMANDS_HELP_TRANSFORM_PERSPECTIVE).to_string(),
            TransformNearest | TransformBilinear | TransformBicubic | TransformLanczos => l.text(MessageId::COMMANDS_HELP_TRANSFORM_NEAREST).to_string(),
            TransformWarp => l.text(MessageId::COMMANDS_HELP_TRANSFORM_WARP).to_string(),
            WarpGridThree | WarpGridFour | WarpGridFive => l.text(MessageId::COMMANDS_HELP_WARP_GRID_THREE).to_string(),
            UseReferenceBelow => l.text(MessageId::COMMANDS_HELP_USE_REFERENCE_BELOW).to_string(),
            CloneSourceArm => l.text(MessageId::COMMANDS_HELP_CLONE_SOURCE_ARM).to_string(),
            CloneAligned => l.text(MessageId::COMMANDS_HELP_CLONE_ALIGNED).to_string(),
            CloneFlipHorizontal | CloneFlipVertical => l.text(MessageId::COMMANDS_HELP_CLONE_FLIP_HORIZONTAL).to_string(),
            CloneResetOffset => l.text(MessageId::COMMANDS_HELP_CLONE_RESET_OFFSET).to_string(),
            ClearLayer => l.text(MessageId::COMMANDS_HELP_CLEAR_LAYER).to_string(),
            ClearSelected => l.text(MessageId::COMMANDS_HELP_CLEAR_SELECTED).to_string(),
            ClearOutside => l.text(MessageId::COMMANDS_HELP_CLEAR_OUTSIDE).to_string(),
            CopySelectionToLayer => l.text(MessageId::COMMANDS_HELP_COPY_SELECTION_TO_LAYER).to_string(),
            CutSelectionToLayer => l.text(MessageId::COMMANDS_HELP_CUT_SELECTION_TO_LAYER).to_string(),
            RevertToOriginal => l.text(MessageId::COMMANDS_HELP_REVERT_TO_ORIGINAL).to_string(),
            ApplyTransformPixels => l.text(MessageId::COMMANDS_HELP_APPLY_TRANSFORM_PIXELS).to_string(),
            MergeDown => l.text(MessageId::COMMANDS_HELP_MERGE_DOWN).to_string(),
            MergeGroup => l.text(MessageId::COMMANDS_HELP_MERGE_GROUP).to_string(),
            MergeVisible => l.text(MessageId::COMMANDS_HELP_MERGE_VISIBLE).to_string(),
            FlattenImage => l.text(MessageId::COMMANDS_HELP_FLATTEN_IMAGE).to_string(),
            StampVisible => l.text(MessageId::COMMANDS_HELP_STAMP_VISIBLE).to_string(),
            BlendPerceptual => l.text(MessageId::COMMANDS_HELP_BLEND_PERCEPTUAL).to_string(),
            BlendLinear => l.text(MessageId::COMMANDS_HELP_BLEND_LINEAR).to_string(),
            LoadSelectionLayer => l.text(MessageId::COMMANDS_HELP_LOAD_SELECTION_LAYER).to_string(),
            InvertSelectionLayer => l.text(MessageId::COMMANDS_HELP_INVERT_SELECTION_LAYER).to_string(),
            InvertLayerMask => l.text(MessageId::COMMANDS_HELP_INVERT_LAYER_MASK).to_string(),
            LayerMaskEnabled => l.text(MessageId::COMMANDS_HELP_LAYER_MASK_ENABLED).to_string(),
            ApplyLayerMask => l.text(MessageId::COMMANDS_HELP_APPLY_LAYER_MASK).to_string(),
            EditLayerMask => l.text(MessageId::COMMANDS_HELP_EDIT_LAYER_MASK).to_string(),
            EditLayerContent => l.text(MessageId::COMMANDS_HELP_EDIT_LAYER_CONTENT).to_string(),
            LassoFill => l.text(MessageId::COMMANDS_HELP_LASSO_FILL).to_string(),
            CanvasSize => l.text(MessageId::COMMANDS_HELP_CANVAS_SIZE).to_string(),
            CropCanvasToSelection => l.text(MessageId::COMMANDS_HELP_CROP_CANVAS_TO_SELECTION).to_string(),
            GrowSelection => l.text(MessageId::COMMANDS_HELP_GROW_SELECTION).to_string(),
            ShrinkSelection => l.text(MessageId::COMMANDS_HELP_SHRINK_SELECTION).to_string(),
            FeatherSelection => l.text(MessageId::COMMANDS_HELP_FEATHER_SELECTION).to_string(),
            BorderSelection => l.text(MessageId::COMMANDS_HELP_BORDER_SELECTION).to_string(),
            SmoothSelection => l.text(MessageId::COMMANDS_HELP_SMOOTH_SELECTION).to_string(),
            TransformSelectionOutline => l.text(MessageId::COMMANDS_HELP_TRANSFORM_SELECTION_OUTLINE).to_string(),
            Crop => l.text(MessageId::COMMANDS_HELP_CROP).to_string(),
            CropSwapOrientation => l.text(MessageId::COMMANDS_HELP_CROP_SWAP_ORIENTATION).to_string(),
            CropCycleOverlay => l.text(MessageId::COMMANDS_HELP_CROP_CYCLE_OVERLAY).to_string(),
            CropStraighten => l.text(MessageId::COMMANDS_HELP_CROP_STRAIGHTEN).to_string(),
            CropDeleteCroppedPixels => l.text(MessageId::COMMANDS_HELP_CROP_DELETE_CROPPED_PIXELS).to_string(),
            StraightenToGuide => l.text(MessageId::COMMANDS_HELP_STRAIGHTEN_TO_GUIDE).to_string(),
            CropFitContent => l.text(MessageId::COMMANDS_HELP_CROP_FIT_CONTENT).to_string(),
            ImageSize => l.text(MessageId::COMMANDS_HELP_IMAGE_SIZE).to_string(),
            RotateImageLeft | RotateImageRight | RotateImage180 => l.text(MessageId::COMMANDS_HELP_ROTATE_IMAGE_LEFT).to_string(),
            FlipImageHorizontal | FlipImageVertical => l.text(MessageId::COMMANDS_HELP_FLIP_IMAGE_HORIZONTAL).to_string(),
            Trim => l.text(MessageId::COMMANDS_HELP_TRIM).to_string(),
            RevealAll => l.text(MessageId::COMMANDS_HELP_REVEAL_ALL).to_string(),
            MoveLeaveCopy => l.text(MessageId::COMMANDS_HELP_MOVE_LEAVE_COPY).to_string(),
            Copy => l.text(MessageId::COMMANDS_HELP_COPY).to_string(),
            Cut => l.text(MessageId::COMMANDS_HELP_CUT).to_string(),
            CopyMerged => l.text(MessageId::COMMANDS_HELP_COPY_MERGED).to_string(),
            PasteImage => l.text(MessageId::COMMANDS_HELP_PASTE_IMAGE).to_string(),
            PasteInPlace => l.text(MessageId::COMMANDS_HELP_PASTE_IN_PLACE).to_string(),
            PasteInto => l.text(MessageId::COMMANDS_HELP_PASTE_INTO).to_string(),
            NewDodgeBurnLayer => l.text(MessageId::COMMANDS_HELP_NEW_DODGE_BURN_LAYER).to_string(),
            FrequencySeparation => l.text(MessageId::COMMANDS_HELP_FREQUENCY_SEPARATION).to_string(),
            ColorMixOklab | ColorMixLinear | ColorMixClassic => l.text(MessageId::COMMANDS_HELP_COLOR_MIX_OKLAB).to_string(),
            _ => String::new(),
        },
        UiAction::CycleTool { .. } => l.text(MessageId::COMMANDS_CYCLE_THROUGH_TOOLS_IN_THIS_FAMILY).to_string(),
        _ => String::new(),
    }
}

fn menu_entries(
    items: Vec<Vec<ContextMenuItem>>,
    path: &str,
    entries: &mut Vec<Entry>,
    alias: &dyn Fn(UiAction) -> UiAction,
    settings: &Settings,
    platform: Platform,
    l: &Localizer,
) {
    for item in items.into_iter().flatten() {
        if let Some(action) = item.action {
            let mut item_entry = entry(
                &item.label,
                path,
                alias(action),
                item.enabled,
                item.selected,
                settings,
                platform,
                l,
            );
            if item_entry.descriptor.description.is_empty() {
                item_entry.descriptor.description = command_text(l, MessageId::COMMANDS_MENU_LOCATION, &[("path", path.to_owned()), ("label", item.label.to_string())]);
            }
            entries.push(item_entry);
        }
        if !item.sections.is_empty() {
            menu_entries(
                item.sections,
                &command_text(l, MessageId::COMMANDS_PATH, &[("path", path.to_owned()), ("label", item.label.to_string())]),
                entries,
                alias,
                settings,
                platform,
                l,
            );
        }
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn command_search_revision(&self) -> u64 {
        self.command_search.revision
    }
    pub fn set_command_focus(&mut self, focus: CommandFocus) {
        if self.state.command_search.is_none() {
            self.command_search.focus = focus;
        }
    }
    pub(crate) fn tool_category(tool: LayerCanvasTool, brush: Tool) -> ToolCategory {
        use LayerCanvasTool as T;
        match tool {
            T::Paint => match brush {
                Tool::Eraser => ToolCategory::Erasing,
                Tool::Blend => ToolCategory::Blending,
                Tool::Liquify => ToolCategory::Warping,
                Tool::Clone | Tool::Heal | Tool::SpotHeal => ToolCategory::Retouching,
                _ => ToolCategory::Drawing,
            },
            T::Hand => ToolCategory::Navigation,
            T::PickVisible | T::PickLayer => ToolCategory::ColorSampling,
            T::Move | T::Transform | T::Crop => ToolCategory::MoveTransform,
            T::Figure { .. } | T::Ruler { .. } => ToolCategory::ShapesRulers,
            T::Gradient { .. } | T::LassoFill | T::Region { fill: true, .. } => {
                ToolCategory::FillGradient
            }
            _ => ToolCategory::Selection,
        }
    }
    #[cfg(test)]
    pub fn command_catalog(&self) -> Vec<CommandDescriptor> {
        self.catalog_entries()
            .into_iter()
            .map(|e| e.descriptor)
            .collect()
    }

    fn catalog_entries(&self) -> Vec<Entry> {
        let l = self.localization();
        let english = Localizer::shared(UiLanguage::English);
        let settings = &self.state.settings;
        let platform = self.state.platform;
        let document = self.engine.document();
        let active = document.active_layer.0;
        let idle = self.require_idle().is_ok();
        let managed = self.managed_workspace.is_some();
        let artwork = !document.active_mask;
        let alias = |action: UiAction| {
            let command = match &action {
                UiAction::Layer { action: LayerAction::Clear { id } } if *id == active && artwork => CommandId::ClearLayer,
                UiAction::Layer { action: LayerAction::Delete { id } } if *id == active => CommandId::DeleteLayer,
                UiAction::Layer { action: LayerAction::RasterizeSource { id } } if *id == active && artwork => {
                    CommandId::RasterizeSource
                }
                UiAction::Layer { action: LayerAction::RepairSourceProfile { id } } if *id == active && artwork => {
                    CommandId::RepairSourceProfile
                }
                UiAction::WorkspaceManager { command: WorkspaceCommand::ResetLayout } if managed => CommandId::ResetLayout,
                _ => return action,
            };
            UiAction::Invoke { command }
        };
        let mut entries = Vec::new();
        for menu in ApplicationMenu::ALL {
            menu_entries(
                self.application_menu(menu).sections,
                &menu.localized_label(l),
                &mut entries,
                &alias,
                settings,
                platform,
                l,
            );
        }
        // Menus own their ordering and validation. Commands outside menus still
        // share CommandState, including selection submodes and temporary locks.
        for command in CommandId::ALL
            .into_iter()
            .filter(|c| c.available_on(platform) && !self.proof_panel_command(*c))
        {
            let state = self.command(command);
            entries.push(entry(
                &state.label,
                l.text(MessageId::COMMANDS_COMMANDS).to_string(),
                UiAction::Invoke { command },
                state.enabled,
                state.checkable.then_some(state.selected),
                settings,
                platform,
                l,
            ));
        }
        for family in ToolFamily::ALL {
            entries.push(entry(
                family.localized_label(l),
                l.text(MessageId::COMMANDS_TOOLS).to_string(),
                UiAction::CycleTool { family },
                idle,
                None,
                settings,
                platform,
                l,
            ));
        }
        for brush in tools::brush_catalog_localized(l) {
            let mut item = entry(
                &brush.label,
                l.text(MessageId::COMMANDS_BRUSHES).to_string(),
                UiAction::SelectBrush { id: brush.id },
                idle,
                None,
                settings,
                platform,
                l,
            );
            item.category = EntryCategory::Brushes;
            item.descriptor.description = command_text(l, MessageId::COMMANDS_BRUSH_PRESET_HELP, &[("category", brush.category.to_string())]);
            entries.push(item);
        }
        let panels = &self.state.tool_panels;
        for set in panels.brush_sets.groups.iter().chain(&panels.sculpt_sets.groups) {
            let mut item = entry(
                &command_text(l, MessageId::COMMANDS_BRUSH_SET_LABEL, &[("label", set.label.to_string())]),
                l.text(MessageId::COMMANDS_BRUSH_SETS).to_string(),
                set.action.clone(),
                idle,
                None,
                settings,
                platform,
                l,
            );
            item.descriptor.description = l.text(MessageId::COMMANDS_USE_THE_LAST_BRUSH_CHOSEN_IN_THIS_SET).to_string().into();
            entries.push(item);
        }
        let current = self.layer_interaction.tool;
        let (shape, paint) = self.layer_interaction.figure;
        let [radial, transparent] = self.layer_interaction.gradient;
        for representative in [
            LayerCanvasTool::Ruler { kind: RulerKind::Straight },
            LayerCanvasTool::Figure { shape, paint },
            LayerCanvasTool::Region { fill: false, source: RegionSource::Visible },
            LayerCanvasTool::Region { fill: true, source: RegionSource::Visible },
            LayerCanvasTool::Gradient { radial, transparent },
        ] {
            use LayerCanvasTool as T;
            let same = match (current, representative) {
                (T::Region { fill: a, .. }, T::Region { fill: b, .. }) => a == b,
                (a, b) => std::mem::discriminant(&a) == std::mem::discriminant(&b),
            };
            let tool = if same { current } else { representative };
            let family = crate::shortcuts::tool_command(&UiAction::Layer { action: LayerAction::Tool { tool } })
                .map_or_else(String::new, |command| self.command(command).label.to_string());
            let view = tools::view(&self.state.brush, tool, l);
            for item in view.groups.iter().chain(&view.subtools) {
                if !matches!(item.action, UiAction::Invoke { .. }) {
                    entries.push(entry(
                        &command_text(l, MessageId::COMMANDS_PATH, &[("path", family.to_string()), ("label", item.label.to_string())]),
                        l.text(MessageId::COMMANDS_TOOL_OPTIONS).to_string(),
                        item.action.clone(),
                        idle,
                        Some(same && item.selected),
                        settings,
                        platform,
                        l,
                    ));
                }
            }
        }
        for option in self.state.tool_options() {
            if let ToolOption::Choice { label, items, .. } = option {
                for item in items.into_iter().filter(|i| {
                    !matches!(
                        i.action,
                        UiAction::Invoke { .. } | UiAction::SelectBrush { .. } | UiAction::SelectToolGroup { .. }
                    )
                }) {
                    let family = crate::shortcuts::tool_command(&item.action)
                        .map_or_else(|| label.to_string(), |command| self.command(command).label.to_string());
                    entries.push(entry(
                        &command_text(l, MessageId::COMMANDS_PATH, &[("path", family.to_string()), ("label", item.label.to_string())]),
                        l.text(MessageId::COMMANDS_TOOL_OPTIONS).to_string(),
                        item.action,
                        idle,
                        Some(item.selected),
                        settings,
                        platform,
                        l,
                    ));
                }
            }
        }
        for setting in &self.state.tool_settings {
            let item = parameter_entry(
                format!("tool_setting.{}", setting.id),
                &setting.label,
                english.text(setting.label_id).as_ref(),
                l.text(MessageId::COMMANDS_TOOL_SETTINGS).to_string(),
                &setting.label,
                &setting.numeric,
                setting.value,
                UiAction::SetToolSetting {
                    id: setting.id.into(),
                    value: setting.value,
                },
                idle,
                settings,
                platform,
                l,
            );
            entries.push(item);
        }
        let properties = &self.state.layer_properties;
        if properties.layer == Some(active)
            && !self.selection_masks.quick()
            && document.layer(document.active_layer).is_some_and(|l| l.kind != LayerKind::Selection)
        {
            let canonical_properties = effects::properties(document, self.state.settings.selection_painting, &english);
            let set = |key: &str, value| UiAction::Effect {
                action: EffectAction::Set { layer: active, key: key.into(), value },
            };
            for control in properties.controls.iter().filter(|c| c.key != "blend") {
                let context = command_text(l, MessageId::COMMANDS_PROPERTY_CONTEXT, &[("context", properties.title.to_string()), ("label", control.label.to_string())]);
                let label = if control.key == "opacity" {
                    l.text(MessageId::COMMANDS_LAYER_OPACITY_LABEL).to_string()
                } else {
                    command_text(l, MessageId::COMMANDS_PROPERTY_LABEL, &[("context", properties.title.to_string()), ("label", control.label.to_string())])
                };
                match (&control.kind, &control.value) {
                    (PropertyKind::Number { numeric }, layer_core::EffectValue::Number(value)) => {
                        entries.push(parameter_entry(
                            format!("layer_property.{}", control.key),
                            &label,
                            &if control.key == "opacity" {
                                english.text(MessageId::COMMANDS_LAYER_OPACITY_LABEL).to_string()
                            } else {
                                let canonical = canonical_properties.controls.iter().find(|item| item.key == control.key).expect("same property schema");
                                command_text(&english, MessageId::COMMANDS_PROPERTY_LABEL, &[("context", canonical_properties.title.to_string()), ("label", canonical.label.to_string())])
                            },
                            l.text(MessageId::COMMANDS_LAYER_PROPERTIES).to_string(),
                            &context,
                            numeric,
                            *value,
                            set(&control.key, layer_core::EffectValue::Number(*value)),
                            properties.enabled,
                            settings,
                            platform,
                            l,
                        ));
                    }
                    (PropertyKind::Choice { options }, layer_core::EffectValue::Choice(current)) => {
                        for (i, option) in options.iter().enumerate() {
                            let mut item = entry(
                                &command_text(l, MessageId::COMMANDS_PROPERTY_CHOICE, &[("label", label.to_string()), ("option", option.to_string())]),
                                l.text(MessageId::COMMANDS_LAYER_PROPERTIES).to_string(),
                                set(&control.key, layer_core::EffectValue::Choice(i as u32)),
                                properties.enabled,
                                Some(i as u32 == *current),
                                settings,
                                platform,
                                l,
                            );
                            item.descriptor.description = context.clone();
                            entries.push(item);
                        }
                    }
                    (PropertyKind::Toggle, layer_core::EffectValue::Toggle(on)) => {
                        let mut item = entry(
                            &label,
                            l.text(MessageId::COMMANDS_LAYER_PROPERTIES).to_string(),
                            set(&control.key, layer_core::EffectValue::Toggle(!on)),
                            properties.enabled,
                            Some(*on),
                            settings,
                            platform,
                            l,
                        );
                        item.descriptor.id = format!("layer_property.{}", control.key);
                        item.descriptor.description = context;
                        entries.push(item);
                    }
                    _ => {}
                }
            }
        }
        if let Some(workspace) = &self.managed_workspace {
            let switchable = self.require_workspace_idle().is_ok();
            for choice in &workspace.choices {
                let current = choice.id == workspace.id;
                let mut item = entry(
                    &choice.name,
                    l.text(MessageId::COMMANDS_WORKSPACES).to_string(),
                    UiAction::WorkspaceManager {
                        command: WorkspaceCommand::Switch { id: choice.id.clone() },
                    },
                    switchable || current,
                    Some(current),
                    settings,
                    platform,
                    l,
                );
                item.descriptor.description = l.text(MessageId::COMMANDS_SWITCH_TO_THIS_WORKSPACE).to_string().into();
                entries.push(item);
            }
        }
        for (slot, _) in crate::color::PAINT_SLOTS {
            let label = l.text(match slot {
                ColorSlot::Foreground => MessageId::COMMANDS_FOREGROUND_COLOR,
                ColorSlot::Background => MessageId::COMMANDS_BACKGROUND_COLOR,
                ColorSlot::Transparent => MessageId::COMMANDS_TRANSPARENT_PAINT,
                ColorSlot::Temporary => MessageId::COMMANDS_TEMPORARY_COLOR,
            });
            let mut item = entry(
                label,
                l.text(MessageId::COMMANDS_COLOR).to_string(),
                UiAction::Color { action: ColorAction::Select { slot } },
                idle,
                Some(self.state.colors.slot == slot),
                settings,
                platform,
                l,
            );
            item.descriptor.description = l.text(MessageId::COMMANDS_PAINT_WITH_THIS_COLOR).to_string().into();
            entries.push(item);
        }
        for (label, action) in [
            (l.text(MessageId::COMMANDS_SWAP_FOREGROUND_AND_BACKGROUND).to_string(), ColorAction::Swap),
            (l.text(MessageId::COMMANDS_BLACK).to_string(), ColorAction::QuickColor { white: false }),
            (l.text(MessageId::COMMANDS_WHITE).to_string(), ColorAction::QuickColor { white: true }),
        ] {
            entries.push(entry(
                label,
                l.text(MessageId::COMMANDS_COLOR).to_string(),
                UiAction::Color { action },
                idle,
                None,
                settings,
                platform,
                l,
            ));
        }
        let mut pan = entry(
            l.text(MessageId::COMMANDS_PAN_WHILE_HELD).to_string(),
            l.text(MessageId::COMMANDS_NAVIGATION).to_string(),
            UiAction::Invoke {
                command: CommandId::Hand,
            },
            true,
            None,
            settings,
            platform,
            l,
        );
        pan.descriptor.id = "canvas.pan".into();
        pan.descriptor.description =
            l.text(MessageId::COMMANDS_TEMPORARILY_PAN_THE_VIEW_RELEASE_TO_RETURN_TO_THE_TOOL).to_string().into();
        pan.descriptor.shortcut = settings.shortcut_label_localized("canvas.pan", platform, l);
        pan.action = None;
        entries.push(pan);
        let definitions = crate::shortcuts::definitions(platform);
        let bound = |id: &str| {
            !settings.keys(id).is_empty()
                || crate::GESTURE_TRIGGERS.iter().any(|t| settings.gesture_binding(t.id) == id)
        };
        for (definition, _) in definitions.clone() {
            let momentary = matches!(definition.action, crate::shortcuts::ShortcutAction::Momentary { .. });
            match definition.action {
                crate::shortcuts::ShortcutAction::Hold { action } | crate::shortcuts::ShortcutAction::Momentary { action }
                    if bound(&definition.id) =>
                {
                    let target = definitions
                        .iter()
                        .find(|(d, _)| Some(&d.id) == definition.target.as_ref())
                        .map_or_else(String::new, |(d, _)| d.label.resolve(l));
                    let enabled = match *action {
                        UiAction::Invoke { command } => self.command(command).enabled,
                        _ => true,
                    };
                    let reason = (!enabled).then(|| self.action_disabled_reason(&action));
                    let mut held = entry(definition.label.resolve(l), l.text(MessageId::COMMANDS_CANVAS).to_string(), *action, enabled, None, settings, platform, l);
                    held.descriptor.disabled_reason = reason;
                    held.descriptor.id = definition.id.clone();
                    held.descriptor.description = if momentary {
                        command_text(l, MessageId::COMMANDS_MOMENTARY_HELP, &[("target", target.to_string())])
                    } else {
                        command_text(l, MessageId::COMMANDS_HOLD_HELP, &[("target", target.to_string())])
                    };
                    held.descriptor.shortcut = settings.shortcut_label_localized(&definition.id, platform, l);
                    held.action = None;
                    entries.push(held);
                }
                crate::shortcuts::ShortcutAction::Action { action }
                    if matches!(&*action, UiAction::StepToolSetting { .. }) =>
                {
                    let enabled = matches!(&*action, UiAction::StepToolSetting { id, .. }
                        if self.state.tool_settings.iter().any(|c| c.id == *id));
                    let mut step = entry(definition.label.resolve(l), l.text(MessageId::COMMANDS_TOOL_SETTINGS).to_string(), *action, enabled, None, settings, platform, l);
                    step.descriptor.id = definition.id;
                    entries.push(step);
                }
                _ => (),
            }
        }
        for entry in &mut entries {
            if let Some(UiAction::Effect { action: EffectAction::Insert { effect } }) = &entry.action
                && let Some(filter) = self.effect_catalog.get(effect)
            {
                entry.canonical_search = crate::search::normalize(&effects::resource_label(filter.label(), &english));
                entry.search.push(' ');
                entry.search.push_str(&entry.canonical_search);
            }
            if let Some(UiAction::Customize { action: CustomizationAction::SetPanelVisible { panel, .. } }) = &entry.action {
                entry.descriptor.label = match self.state.workspace.layout.panel(*panel) {
                    Ok(config) if matches!(&config.content, PanelContent::Toolbar { .. }) =>
                        command_text(l, MessageId::COMMANDS_TOOLBAR_LABEL, &[("label", config.title_localized(l))]),
                    _ => panel_visibility_label(l, *panel),
                };
                entry.search = crate::search::normalize(&format!("{} {}", entry.descriptor.label, entry.search));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        entries.retain(|e| seen.insert(e.descriptor.id.clone()));
        let mut labels = std::collections::BTreeMap::<String, usize>::new();
        for e in &entries {
            *labels.entry(crate::search::normalize(&e.descriptor.label)).or_default() += 1;
        }
        for e in &mut entries {
            let message = match &e.action {
                Some(UiAction::SelectBrush { .. }) => MessageId::COMMANDS_BRUSH_LABEL,
                Some(UiAction::Effect { action: EffectAction::Insert { .. } }) => MessageId::COMMANDS_FILTER_LABEL,
                _ => continue,
            };
            if labels[&crate::search::normalize(&e.descriptor.label)] > 1 {
                e.descriptor.label = command_text(l, message, &[("label", e.descriptor.label.clone())]);
                e.search = format!("{} {}", crate::search::normalize(&e.descriptor.label), e.search);
            }
        }
        // Use exactly the command's live predicate/reason even when its first
        // appearance was in a menu, and exclude the bar's own opener.
        for e in &mut entries {
            if let Some(UiAction::Invoke { command }) = e.action {
                e.descriptor.enabled = self.command_flags(command).0;
            }
            e.descriptor.disabled_reason = match &e.action {
                Some(action) if !e.descriptor.enabled => Some(self.action_disabled_reason(action)),
                Some(_) => None,
                None => e.descriptor.disabled_reason.take(),
            };
        }
        if self.command_search.focus == CommandFocus::Palette {
            let palette = self.state.colors.library.active_palette().id;
            for (id, redo) in [("command.undo", false), ("command.redo", true)] {
                if let Some(e) = entries.iter_mut().find(|e| e.descriptor.id == id) {
                    e.action = Some(UiAction::Color {
                        action: ColorAction::Library {
                            action: if redo {
                                ColorLibraryAction::RedoReorder { palette }
                            } else {
                                ColorLibraryAction::UndoReorder { palette }
                            },
                        },
                    });
                    e.descriptor.label = if redo {
                        l.text(MessageId::COMMANDS_REDO_COLOR_REORDER).to_string()
                    } else {
                        l.text(MessageId::COMMANDS_UNDO_COLOR_REORDER).to_string()
                    }
                    .into();
                    e.descriptor.category = l.text(MessageId::COMMANDS_PALETTE).to_string().into();
                    e.descriptor.description = if redo {
                        l.text(MessageId::COMMANDS_REAPPLY_THE_LAST_UNDONE_COLOR_REORDER_IN_THIS_PALETTE).to_string()
                    } else {
                        l.text(MessageId::COMMANDS_RESTORE_THE_PREVIOUS_COLOR_ORDER_IN_THIS_PALETTE).to_string()
                    }
                    .into();
                    e.descriptor.enabled =
                        self.state.colors.library.can_undo_reorder(palette, redo);
                    e.descriptor.disabled_reason =
                        (!e.descriptor.enabled).then(|| l.text(MessageId::COMMANDS_NO_COLOR_REORDER_TO_RESTORE).to_string().into());
                    e.search = crate::search::normalize(&format!("{} {}", e.descriptor.label, e.search));
                }
            }
        }
        if self.command_search.focus == CommandFocus::Text {
            for (id, label) in [
                ("command.undo", l.text(MessageId::COMMANDS_UNDO_TEXT_EDIT).to_string()),
                ("command.redo", l.text(MessageId::COMMANDS_REDO_TEXT_EDIT).to_string()),
            ] {
                if let Some(e) = entries.iter_mut().find(|e| e.descriptor.id == id) {
                    e.descriptor.label = label.clone();
                    e.descriptor.category = l.text(MessageId::COMMANDS_TEXT_EDITING).to_string().into();
                    e.descriptor.enabled = false;
                    e.descriptor.disabled_reason =
                        Some(l.text(MessageId::COMMANDS_CLOSE_COMMAND_SEARCH_TO_UNDO_OR_REDO_IN_THE_TEXT_FIELD).to_string().into());
                    e.search = crate::search::normalize(&format!("{label} {}", e.search));
                }
            }
        }
        for entry in &mut entries { entry.label_search = crate::search::normalize(&entry.descriptor.label); }
        entries
    }

    pub fn command_disabled_reason(&self, command: CommandId) -> Option<String> {
        (!self.command_flags(command).0).then(|| self.disabled_reason_unchecked(command).to_string())
    }

    /// The reason for a command that `command_flags` reports disabled.
    pub(super) fn disabled_reason_unchecked(&self, command: CommandId) -> Arc<str> {
        let l = self.localization();
        use CommandId as C;
        if !command.available_on(self.state.platform) {
            return l.text(MessageId::COMMANDS_NOT_AVAILABLE_ON_THIS_PLATFORM).into();
        }
        if self.state.document_file.close_ready {
            return l.text(MessageId::COMMANDS_THIS_DRAWING_IS_CLOSING).into();
        }
        if self.workspace_read_only && !matches!(command, C::ApplyTransform | C::CancelTransform) {
            return if self.managed_workspace.is_none() {
                l.text(MessageId::COMMANDS_THE_WORKSPACE_IS_STILL_LOADING)
            } else {
                l.text(MessageId::COMMANDS_WORKSPACE_OWNERSHIP_NEEDS_RECOVERY)
            }.into();
        }
        if self.workspace_transition {
            return l.text(MessageId::COMMANDS_A_WORKSPACE_CHANGE_IS_IN_PROGRESS).into();
        }
        if self.rendering_suspended && !Self::command_without_renderer(command) {
            return l.text(MessageId::COMMANDS_PAINTING_IS_UNAVAILABLE_SAVE_THE_DRAWING_AND_REOPEN_IT).into();
        }
        let gate = match command {
            C::SdrRendition
            | C::PreviewSdr
            | C::SoftProofSetup
            | C::SoftProof
            | C::RepairSourceProfile
            | C::RasterizeSource
            | C::AssignProfile
            | C::ConvertColorSpace
            | C::ChangeBitDepth
            | C::ImportImage
            | C::PasteImage
            | C::PasteInPlace
            | C::PasteInto
            | C::Copy
            | C::Cut
            | C::CopyMerged
            | C::DocumentProperties
            | C::NewDocument
            | C::OpenDocument
            | C::ExportDocument
            | C::UseReferenceBelow
            | C::SelectAll
            | C::Deselect
            | C::InvertSelection
            | C::ClearLayer
            | C::FillSelection
            | C::ClearSelected
            | C::ClearOutside
            | C::CopySelectionToLayer
            | C::CutSelectionToLayer
            | C::RevertToOriginal
            | C::ApplyTransformPixels
            | C::MergeDown
            | C::MergeGroup
            | C::MergeVisible
            | C::FlattenImage
            | C::StampVisible
            | C::NewDodgeBurnLayer
            | C::FrequencySeparation
            | C::BlendPerceptual
            | C::BlendLinear
            | C::LoadSelectionLayer
            | C::InvertSelectionLayer
            | C::InvertLayerMask
            | C::LayerMaskEnabled
            | C::ApplyLayerMask
            | C::CanvasSize
            | C::CropCanvasToSelection
            | C::GrowSelection
            | C::ShrinkSelection
            | C::FeatherSelection
            | C::BorderSelection
            | C::SmoothSelection
            | C::TransformSelectionOutline
            | C::ImageSize
            | C::RotateImageLeft
            | C::RotateImageRight
            | C::RotateImage180
            | C::FlipImageHorizontal
            | C::FlipImageVertical
            | C::Trim
            | C::RevealAll => self.require_document_idle(),
            C::SaveDocument | C::SaveDocumentAs => self.require_raster_snapshot(),
            C::CloseDocument => self.require_document_snapshot_idle(),
            C::ResetLayout if self.managed_workspace.is_some() => self.require_workspace_idle(),
            C::CompleteSelection | C::CancelSelection | C::RemoveSelectionPoint | C::GamutWarning | C::UndoWorkspace | C::RedoWorkspace => Ok(()),
            _ => self.require_idle(),
        };
        if let Err(reason) = gate {
            return reason.into();
        }
        let document = self.engine.document();
        let active = document.layer(document.active_layer);
        let paint = active.is_some_and(|l| l.kind == LayerKind::Paint);
        let locked = document.is_locked(document.active_layer);
        let mask_target = self.selection_masks.target();
        let selection = self.has_selection();
        let apply_refusal = active.and_then(|l| art_layers::apply_mask_refusal(l.kind, self.localization()));
        let reason: Arc<str> = match command {
            C::Undo => l.text(MessageId::COMMANDS_NOTHING_TO_UNDO),
            C::Redo if self.cropping() => l.text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_CROP_FIRST),
            C::Redo => l.text(MessageId::COMMANDS_NOTHING_TO_REDO),
            C::UndoWorkspace | C::RedoWorkspace if self.state.customization.header_editing => {
                l.text(MessageId::COMMANDS_FINISH_CUSTOMIZING_THE_TITLE_BAR_FIRST)
            }
            C::UndoWorkspace => l.text(MessageId::COMMANDS_NO_WORKSPACE_CHANGE_TO_UNDO),
            C::RedoWorkspace => l.text(MessageId::COMMANDS_NO_WORKSPACE_CHANGE_TO_REDO),
            C::ReturnToArtwork
            | C::ResetMaskColors
            | C::SwapMaskColors
            | C::MaskOverlayProtected
            | C::FillSelectionMask
            | C::ClearSelectionMask
                if mask_target.is_none() =>
            {
                l.text(MessageId::COMMANDS_EDIT_A_SELECTION_MASK_FIRST)
            }
            C::MaskOverlayProtected | C::FillSelectionMask | C::ClearSelectionMask => {
                l.text(MessageId::COMMANDS_THIS_SELECTION_LAYER_IS_LOCKED)
            }
            C::LoadSelectionLayer | C::InvertSelectionLayer
                if !matches!(mask_target, Some(layer_core::SelectionTarget::Saved(_))) =>
            {
                l.text(MessageId::COMMANDS_EDIT_A_SELECTION_LAYER_FIRST)
            }
            C::InvertSelectionLayer => l.text(MessageId::COMMANDS_THIS_SELECTION_LAYER_IS_LOCKED),
            C::ScaleRotate
            | C::ClearLayer
            | C::Figure
            | C::Move
            | C::LassoFill
            | C::FillSelection
            | C::RepairSourceProfile
            | C::RasterizeSource
            | C::InvertLayerMask
            | C::LayerMaskEnabled
            | C::ApplyLayerMask
                if mask_target.is_some() =>
            {
                l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST)
            }
            C::InvertLayerMask | C::LayerMaskEnabled | C::ApplyLayerMask | C::EditLayerMask
                if active.is_none_or(|l| l.mask.is_none()) =>
            {
                l.text(MessageId::COMMANDS_THE_LAYER_HAS_NO_MASK)
            }
            C::EditLayerMask => l.text(MessageId::COMMANDS_ALREADY_EDITING_THE_LAYER_MASK),
            C::EditLayerContent => l.text(MessageId::COMMANDS_ALREADY_EDITING_THE_LAYER_CONTENT),
            C::QuickMask | C::NewSelectionLayer | C::PlacementOriginalSize | C::ScaleRotate
                if self.operation.active() && !self.operation.placing() =>
            {
                self.operation_refusal()
            }
            C::PlacementOriginalSize => l.text(MessageId::COMMANDS_TRANSFORM_ORIGINAL_AFFINE),
            C::Reselect if selection => l.text(MessageId::COMMANDS_DESELECT_BEFORE_RESTORING_THE_PREVIOUS_SELECTION),
            C::Reselect => l.text(MessageId::COMMANDS_NO_PREVIOUS_SELECTION_TO_RESTORE),
            C::Deselect | C::InvertSelection | C::FillSelection | C::SaveSelectionLayer if !selection => {
                l.text(MessageId::COMMANDS_CREATE_A_SELECTION_FIRST)
            }
            C::DeleteLayer if self.selection_masks.quick() => l.text(MessageId::COMMANDS_LEAVE_QUICK_MASK_FIRST),
            C::DeleteLayer => {
                return document
                    .delete_layers_edit(&[document.active_layer])
                    .err()
                    .map_or(l.text(MessageId::COMMANDS_THIS_LAYER_CAN_T_BE_DELETED).into(), |e| layer_error(e, l).into());
            }
            C::SdrRendition | C::PreviewSdr if !document.color.depth.is_float() => {
                l.text(MessageId::COMMANDS_REQUIRES_A_HIGH_DYNAMIC_RANGE_DRAWING)
            }
            C::PreviewSdr if !self.state.hdr_display_available => l.text(MessageId::COMMANDS_REQUIRES_A_HIGH_DYNAMIC_RANGE_DISPLAY),
            C::PreviewSdr => l.text(MessageId::COMMANDS_TURN_OFF_SOFT_PROOFING_AND_THE_GAMUT_WARNING_FIRST),
            C::GamutWarning => l.text(MessageId::COMMANDS_SET_UP_SOFT_PROOFING_FIRST),
            C::ResetLayout if self.managed_workspace.is_some() => l.text(MessageId::COMMANDS_THE_LAYOUT_ALREADY_MATCHES_ITS_STARTING_STATE),
            C::RepairSourceProfile | C::RasterizeSource if document.active_mask => l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST),
            C::RepairSourceProfile | C::RasterizeSource => l.text(MessageId::COMMANDS_SELECT_AN_UNLOCKED_RETAINED_IMAGE_LAYER),
            C::ApplyTransform if self.region_tools.applying_transform() => l.text(MessageId::COMMANDS_APPLYING_THE_TRANSFORM),
            C::TransformPerspective if self.operation.transforming() => l.text(MessageId::COMMANDS_CHOOSE_DISTORT_FIRST),
            C::ApplyTransform
            | C::CancelTransform
            | C::TransformFlipHorizontal
            | C::TransformFlipVertical
            | C::TransformRotateLeft
            | C::TransformRotateRight
            | C::ResetTransform
            | C::TransformFree
            | C::TransformUniform
            | C::TransformPerspective => l.text(MessageId::COMMANDS_START_A_TRANSFORM_FIRST),
            C::TransformDistort | C::TransformWarp if self.operation.outline() => l.text(crate::session::operation::OUTLINE_AFFINE),
            C::TransformNearest | C::TransformBilinear | C::TransformBicubic if self.operation.outline() => {
                l.text(crate::session::operation::OUTLINE_PIXELS)
            }
            C::TransformWarp if self.operation.placing() => l.text(MessageId::COMMANDS_TRANSFORM_SINGLE_WARP),
            C::TransformWarp => l.text(MessageId::COMMANDS_START_A_TRANSFORM_FIRST),
            C::WarpSplitVertical | C::WarpSplitHorizontal | C::WarpSplitCross if self.warp_cells().is_some() => l.text(MessageId::COMMANDS_WARP_SPLIT_LIMIT),
            C::WarpGridThree | C::WarpGridFour | C::WarpGridFive if self.warp_cells().is_some() => l.text(MessageId::COMMANDS_WARP_RESET_GRID),
            C::WarpGridThree | C::WarpGridFour | C::WarpGridFive | C::WarpSplitVertical | C::WarpSplitHorizontal | C::WarpSplitCross
            | C::WarpSelectPoints | C::WarpResetGrid => l.text(MessageId::COMMANDS_CHOOSE_WARP_FIRST),
            C::TransformNearest | C::TransformBilinear | C::TransformBicubic | C::TransformLanczos => l.text(MessageId::COMMANDS_START_A_TRANSFORM_FIRST),
            C::TransformDistort => l.text(MessageId::COMMANDS_START_A_TRANSFORM_FIRST),
            C::ColorMixOklab | C::ColorMixLinear | C::ColorMixClassic => l.text(MessageId::COMMANDS_PAINT_MIXING_UNAVAILABLE),
            C::SnapRulers => l.text(MessageId::COMMANDS_SHOW_RULERS_FIRST),
            C::DeleteRuler => l.text(MessageId::COMMANDS_SELECT_A_RULER_FIRST),
            C::CompleteSelection
                if self.layer_interaction.tool
                    != (LayerCanvasTool::Selection { kind: SelectionTool::Polygon }) =>
            {
                l.text(MessageId::COMMANDS_USE_THE_POLYGON_SELECTION_TOOL)
            }
            C::CompleteSelection => l.text(MessageId::COMMANDS_PLACE_AT_LEAST_THREE_POINTS_FIRST),
            C::CancelSelection => l.text(MessageId::COMMANDS_NO_SELECTION_PATH_TO_CANCEL),
            C::RemoveSelectionPoint => l.text(MessageId::COMMANDS_PLACE_A_POLYGON_POINT_FIRST),
            C::UseReferenceBelow => self.use_reference_below_reason().unwrap_or_else(|| l.text(super::notices::NO_REFERENCE_BELOW)),
            C::ClearSelected | C::ClearOutside => self.clear_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::CopySelectionToLayer | C::CutSelectionToLayer => {
                self.selection_to_layer_refusal(command == C::CutSelectionToLayer).unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET))
            }
            C::RevertToOriginal => self.revert_to_original_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::ApplyTransformPixels => self.transform_pixels_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::MergeDown | C::MergeGroup | C::MergeVisible | C::FlattenImage | C::StampVisible => {
                super::merges::merge_kind(command).and_then(|kind| self.merge_refusal(kind)).unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET))
            }
            C::BlendPerceptual | C::BlendLinear => self.blending_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::NewDodgeBurnLayer => self.dodge_burn_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::FrequencySeparation => self.separation_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::CanvasSize
            | C::ImageSize
            | C::RotateImageLeft
            | C::RotateImageRight
            | C::RotateImage180
            | C::FlipImageHorizontal
            | C::FlipImageVertical
            | C::Trim
            | C::RevealAll => self.canvas_geometry_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::Crop if self.operation.transforming() => l.text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST),
            C::Crop => self.canvas_geometry_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::CropRatioFree
            | C::CropRatioOriginal
            | C::CropRatioSquare
            | C::CropRatioFourFive
            | C::CropRatioTwoThree
            | C::CropRatioFiveSeven
            | C::CropRatioSixteenNine
            | C::CropSwapOrientation
            | C::CropOverlayThirds
            | C::CropOverlayGrid
            | C::CropOverlayDiagonal
            | C::CropOverlayGolden
            | C::CropCycleOverlay
            | C::CropStraighten
            | C::CropDeleteCroppedPixels
            | C::CropFitContent => l.text(MessageId::COMMANDS_CHOOSE_THE_CROP_TOOL_FIRST),
            C::StraightenToGuide => self.straighten_to_guide_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::CropCanvasToSelection => self.crop_to_selection_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::GrowSelection | C::ShrinkSelection | C::FeatherSelection | C::BorderSelection | C::SmoothSelection => {
                self.refine_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET))
            }
            C::TransformSelectionOutline => self.outline_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::Copy | C::Cut | C::CopyMerged | C::PasteInto if self.state.document_file.busy => {
                l.text(MessageId::COMMANDS_WAIT_FOR_THE_CURRENT_FILE_OPERATION)
            }
            C::Copy | C::Cut | C::CopyMerged => self.copy_refusal(command).unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::PasteInto => self.paste_into_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::MaskSelection if self.engine.document().selection.is_none() => l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST),
            C::ApplyLayerMask if apply_refusal.is_some() => apply_refusal.unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET)),
            C::ApplyLayerMask if active.and_then(|l| l.mask.as_ref()).is_some_and(|m| !m.enabled) => {
                l.text(MessageId::COMMANDS_ENABLE_THE_MASK_BEFORE_APPLYING_IT)
            }
            C::MaskSelection => l.text(MessageId::COMMANDS_SELECT_AN_UNLOCKED_ARTWORK_LAYER),
            C::SelectionVisible => l.text(MessageId::COMMANDS_CHOOSE_A_SELECTION_TOOL_FIRST),
            C::SelectionEditing | C::SelectionReference => l.text(MessageId::COMMANDS_CHOOSE_A_SELECTION_OR_RETOUCHING_TOOL_FIRST),
            C::CloneSourceArm | C::CloneAligned | C::CloneFlipHorizontal | C::CloneFlipVertical | C::CloneResetOffset
                if self.state.brush.tool == Tool::SpotHeal && self.retouching() =>
            {
                l.text(MessageId::COMMANDS_SPOT_HEALING_FINDS_ITS_OWN_SOURCE)
            }
            C::CloneSourceArm | C::CloneAligned | C::CloneFlipHorizontal | C::CloneFlipVertical => {
                l.text(MessageId::COMMANDS_CHOOSE_A_RETOUCHING_TOOL_FIRST)
            }
            C::CloneResetOffset if self.retouching() => l.text(MessageId::COMMANDS_CLONE_AN_ALIGNED_STROKE_FIRST),
            C::CloneResetOffset => l.text(MessageId::COMMANDS_CHOOSE_A_RETOUCHING_TOOL_FIRST),
            C::ZoomIn => l.text(MessageId::COMMANDS_ALREADY_AT_THE_MAXIMUM_ZOOM),
            C::ZoomOut => l.text(MessageId::COMMANDS_ALREADY_AT_THE_MINIMUM_ZOOM),
            _ if self.operation.active() => self.operation_refusal(),
            _ if self.state.document_file.busy => l.text(MessageId::COMMANDS_WAIT_FOR_THE_CURRENT_FILE_OPERATION),
            _ if locked => l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED),
            C::TransformAgain => self.transform_again_refusal().unwrap_or_else(|| l.text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST)),
            C::TransformSnapping => l.text(MessageId::COMMANDS_START_A_TRANSFORM_FIRST),
            C::ScaleRotate => l.text(MessageId::COMMANDS_SELECT_UNLOCKED_PAINT_CONTENT_OR_A_LAYER_MASK),
            C::ClearLayer | C::FillSelection | C::RaiseLayer | C::LowerLayer if !paint => l.text(MessageId::COMMANDS_SELECT_A_PAINT_LAYER),
            C::ClearLayer | C::FillSelection if document.active_mask => l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST),
            C::RaiseLayer => l.text(MessageId::COMMANDS_THE_LAYER_IS_ALREADY_AT_THE_TOP),
            C::LowerLayer => l.text(MessageId::COMMANDS_THE_LAYER_IS_ALREADY_AT_THE_BOTTOM),
            _ => l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET),
        };
        reason.into()
    }

    fn action_disabled_reason(&self, action: &UiAction) -> String {
        let l = self.localization();
        if let UiAction::Invoke { command } = action
            && let Some(reason) = self.command_disabled_reason(*command)
        {
            return reason;
        }
        let gate = match action {
            UiAction::WorkspaceManager { .. } | UiAction::Customize { .. } => self.require_workspace_idle(),
            UiAction::Layer { .. } | UiAction::Selection { .. } | UiAction::Effect { .. } => self.require_document_idle(),
            _ => self.require_idle(),
        };
        if let Err(reason) = gate {
            return reason;
        }
        let document = self.engine.document();
        let roots = document.layer_roots(&self.layer_interaction.selected);
        let apply_mask_refusal = document
            .layer(document.active_layer)
            .and_then(|l| art_layers::apply_mask_refusal(l.kind, self.localization()));
        let reason = match action {
            UiAction::Layer { action: LayerAction::GroupSelected } => {
                document.group_layers_edit(&roots, LayerId(0), layer_core::LayerBlend::Normal, "").err().map(|e| layer_error(e, l).to_string())
            }
            UiAction::Layer { action: LayerAction::Ungroup { .. } } => {
                document.ungroup_layer_edit(document.active_layer).err().map(|e| layer_error(e, l).to_string())
            }
            UiAction::Layer { action: LayerAction::DeleteSelected } => {
                document.delete_layers_edit(&roots).err().map(|e| layer_error(e, l).to_string())
            }
            UiAction::Layer { action: LayerAction::Delete { .. } } => {
                document.delete_layers_edit(&[document.active_layer]).err().map(|e| layer_error(e, l).to_string())
            }
            UiAction::Layer {
                action:
                    LayerAction::MaskSelection { .. }
                    | LayerAction::FillSelection
                    | LayerAction::InvertSelection
                    | LayerAction::Deselect,
            }
            | UiAction::Selection { .. }
                if document.selection.is_none() && self.current_selection().is_none() =>
            {
                Some(l.text(MessageId::COMMANDS_CREATE_A_SELECTION_FIRST).to_string().into())
            }
            UiAction::Layer { action: LayerAction::PasteMask { .. } }
                if self.layer_interaction.clipboard_mask.is_none() =>
            {
                Some(l.text(MessageId::COMMANDS_COPY_A_LAYER_MASK_FIRST).to_string().into())
            }
            UiAction::Layer { action: LayerAction::CopyMask { .. } | LayerAction::ApplyMask { .. } }
                if document.layer(document.active_layer).is_some_and(|l| l.mask.is_none()) =>
            {
                Some(l.text(MessageId::COMMANDS_THE_LAYER_HAS_NO_MASK).to_string().into())
            }
            UiAction::Layer { action: LayerAction::ApplyMask { .. } } if apply_mask_refusal.is_some() => {
                apply_mask_refusal.map(|reason| reason.to_string())
            }
            UiAction::Layer { action: LayerAction::ReferenceSelection } => {
                Some(l.text(MessageId::COMMANDS_MARK_LAYERS_AS_REFERENCES_FIRST).to_string().into())
            }
            UiAction::StepToolSetting { id, .. } if !self.state.tool_settings.iter().any(|c| c.id == *id) => {
                Some(command_text(l, MessageId::COMMANDS_SETTING_UNAVAILABLE, &[("setting", id.to_string())]))
            }
            UiAction::Effect { .. } if self.selection_masks.target().is_some() || document.active_mask => {
                Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_BEFORE_APPLYING_A_FILTER).to_string().into())
            }
            UiAction::Layer { .. } | UiAction::Effect { .. } | UiAction::Selection { .. }
                if document.is_locked(document.active_layer) =>
            {
                Some(l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string().into())
            }
            UiAction::Layer { .. } | UiAction::Effect { .. }
                if document.layer(document.active_layer).is_some_and(|l| l.kind == LayerKind::Background) =>
            {
                Some(l.text(MessageId::COMMANDS_THE_BACKGROUND_CAN_T_BE_CHANGED_THIS_WAY).to_string().into())
            }
            _ => None,
        };
        reason.unwrap_or_else(|| l.text(MessageId::COMMANDS_UNAVAILABLE_IN_THE_CURRENT_TOOL_OR_EDIT_TARGET).to_string().into())
    }
    pub(super) fn refresh_command_search_localization(&mut self) {
        if self.state.command_search.is_none() { return; }
        self.command_search.entries = self.catalog_entries();
        let view = self.state.command_search.as_mut().unwrap();
        let descriptor = |id: &str| self.command_search.entries.iter().find(|entry| entry.descriptor.id == id).map(|entry| entry.descriptor.clone());
        let selected = view.results.get(view.selected).map(|entry| entry.id.clone());
        view.results = view.results.iter().filter_map(|entry| descriptor(&entry.id)).collect();
        view.selected = selected.and_then(|id| view.results.iter().position(|entry| entry.id == id)).unwrap_or(0);
        if let Some(parameter) = &mut view.parameter {
            if let Some(mut current) = descriptor(&parameter.id) {
                current.parameter = parameter.parameter.clone();
                *parameter = current;
            }
        }
        if let Some(error) = &self.command_search.error_copy {
            view.error = match error {
                CommandSearchError::Numeric(error) => Some(error.message(&self.state.localization)),
                CommandSearchError::Message(message) => Some(self.state.localization.text(*message).to_string()),
                CommandSearchError::Disabled(id) => descriptor(id).and_then(|entry| entry.disabled_reason),
            };
        }
        view.refresh_detail();
    }

    pub(super) fn open_command_search(&mut self) -> Result<(), String> {
        self.require_idle()?;
        self.command_search.entries = self.catalog_entries();
        self.command_search.epoch = self.state.document_file.epoch;
        // Popup keyboard grabs may own the opener's release. A new native
        // focus owner must not inherit the canvas's pressed-key repeat set.
        self.interaction.keys.clear();
        self.interaction.pan_key = None;
        self.state.command_search = Some(CommandSearchView::default());
        self.search_commands(String::new());
        Ok(())
    }

    fn search_commands(&mut self, query: String) {
        self.command_search.error_copy = None;
        let terms = crate::search::normalize(&query);
        let brush_query = crate::search::normalize(&CommandId::DrawingBrush.localized_label(self.localization()));
        let brush_category_query = crate::search::normalize(&self.localization().text(MessageId::COMMANDS_BRUSHES));
        let mut matches: Vec<_> = self
            .command_search
            .entries
            .iter()
            .filter_map(|entry| {
                let d = &entry.descriptor;
                if entry.action.is_none() || d.id == "command.search_commands" {
                    return None;
                }
                if entry.category == EntryCategory::Brushes && (matches!(terms.trim(), "brush" | "brushes") || terms.trim() == brush_query || terms.trim() == brush_category_query) {
                    return None;
                }
                let score = if terms.trim().is_empty() {
                    if !d.enabled {
                        return None;
                    }
                    if let Some(i) = self.command_search.recent.iter().position(|id| id == &d.id) {
                        1000 - i as i32
                    } else {
                        match d.id.as_str() {
                            "command.undo" => 100,
                            "command.fit_canvas" => 99,
                            "command.save_document" => 98,
                            "command.settings" => 97,
                            "command.keyboard_shortcuts" => 96,
                            _ => return None,
                        }
                    }
                } else {
                    search_score(&terms, &entry.label_search, &entry.search)
                        .into_iter().chain(search_score(&terms, &entry.canonical_search, &entry.search)).max()?
                };
                Some((score, d))
            })
            .collect();
        matches.sort_by(|(a, ad), (b, bd)| {
            b.cmp(a)
                .then_with(|| {
                    bd.id
                        .starts_with("command.")
                        .cmp(&ad.id.starts_with("command."))
                })
                .then_with(|| ad.label.cmp(&bd.label))
                .then_with(|| ad.id.cmp(&bd.id))
        });
        let limit = if terms.trim().is_empty() { 5 } else { 8 };
        if let Some(view) = &mut self.state.command_search {
            view.results = matches
                .into_iter()
                .take(limit)
                .map(|(_, d)| d.clone())
                .collect();
            view.query = query;
            view.selected = 0;
            view.error = None;
            view.parameter = None;
            view.refresh_detail();
        }
    }

    pub(super) fn execute_catalog_command(
        &mut self,
        id: &str,
        value: Option<String>,
    ) -> Result<UiChange, String> {
        let l = self.localization().clone();
        self.command_search.error_copy = None;
        let entry = self
            .catalog_entries()
            .into_iter()
            .find(|e| e.descriptor.id == id)
            .ok_or_else(|| {
                self.command_search.error_copy = Some(CommandSearchError::Message(MessageId::COMMANDS_THIS_COMMAND_IS_NO_LONGER_AVAILABLE));
                l.text(MessageId::COMMANDS_THIS_COMMAND_IS_NO_LONGER_AVAILABLE).to_string()
            })?;
        if !entry.descriptor.enabled {
            self.command_search.error_copy = Some(CommandSearchError::Disabled(entry.descriptor.id));
            return Err(entry.descriptor.disabled_reason.unwrap_or_default());
        }
        let mut action = entry.action.ok_or_else(|| {
            self.command_search.error_copy = Some(CommandSearchError::Message(MessageId::COMMANDS_THIS_COMMAND_REQUIRES_A_HELD_INPUT));
            l.text(MessageId::COMMANDS_THIS_COMMAND_REQUIRES_A_HELD_INPUT).to_string()
        })?;
        if let Some(parameter) = entry.descriptor.parameter {
            let text = value.ok_or_else(|| {
                self.command_search.error_copy = Some(CommandSearchError::Message(MessageId::COMMANDS_ENTER_A_VALUE_FOR_THIS_COMMAND));
                l.text(MessageId::COMMANDS_ENTER_A_VALUE_FOR_THIS_COMMAND).to_string()
            })?;
            let number = parameter
                .numeric
                .resolve(
                    parameter.value as f64,
                    NumericOperation::Expression { text },
                ).map_err(|reason| {
                    let message = reason.message(&l);
                    self.command_search.error_copy = Some(CommandSearchError::Numeric(reason));
                    message
                })?
                .value as f32;
            match &mut action {
                UiAction::SetToolSetting { value, .. } => *value = number,
                UiAction::Effect { action: EffectAction::Set { value, .. } } => {
                    *value = layer_core::EffectValue::Number(number)
                }
                _ => {}
            }
        }
        self.dispatch(action)
    }

    pub(super) fn command_search_action(
        &mut self,
        action: CommandSearchAction,
    ) -> Result<UiChange, String> {
        let l = self.localization().clone();
        use CommandSearchAction as A;
        if let A::Focus { focus } = action {
            self.set_command_focus(focus);
            return Ok(self.changed(0, false));
        }
        if matches!(action, A::Close) {
            self.state.command_search = None;
            self.command_search.entries.clear();
            return Ok(self.changed(regions::COMMAND_SEARCH, false));
        }
        if self.state.command_search.is_none() {
            return Ok(self.changed(0, false));
        }
        if self.command_search.epoch != self.state.document_file.epoch {
            return Err(l.text(MessageId::COMMANDS_THIS_COMMAND_SEARCH_BELONGS_TO_A_PREVIOUS_DRAWING).to_string().into());
        }
        match action {
            A::Commit { text } => {
                let view = self.state.command_search.as_ref().unwrap();
                if let Some(parameter) = &view.parameter {
                    return self.command_search_action(A::Execute {
                        id: parameter.id.clone(),
                        value: Some(text),
                    });
                }
                if view.query != text {
                    self.search_commands(text.chars().take(256).collect());
                }
                let view = self.state.command_search.as_ref().unwrap();
                if let Some(selected) = view.results.get(view.selected) {
                    return self.command_search_action(A::Execute {
                        id: selected.id.clone(),
                        value: None,
                    });
                }
            }
            A::Query { text } => {
                // Native text changes can arrive before a serial host paints
                // the parameter step. Only Back can return to result search.
                if self
                    .state
                    .command_search
                    .as_ref()
                    .unwrap()
                    .parameter
                    .is_none()
                {
                    self.search_commands(text.chars().take(256).collect());
                }
            }
            A::Move { delta } => {
                let view = self.state.command_search.as_mut().unwrap();
                if !view.results.is_empty() {
                    view.selected = (view.selected as i64 + delta as i64)
                        .rem_euclid(view.results.len() as i64)
                        as usize;
                }
            }
            A::Select { id } => {
                let view = self.state.command_search.as_mut().unwrap();
                if let Some(i) = view.results.iter().position(|d| d.id == id) {
                    view.selected = i;
                }
            }
            A::Back => {
                self.command_search.error_copy = None;
                let view = self.state.command_search.as_mut().unwrap();
                if view.parameter.is_none() {
                    return self.command_search_action(A::Close);
                }
                view.parameter = None;
                view.error = None;
            }
            A::Execute { id, value } => {
                let view = self.state.command_search.as_ref().unwrap();
                let descriptor = view
                    .parameter
                    .as_ref()
                    .filter(|d| d.id == id)
                    .or_else(|| view.results.iter().find(|d| d.id == id))
                    .cloned()
                    .ok_or(l.text(MessageId::COMMANDS_CHOOSE_A_CURRENT_SEARCH_RESULT).to_string())?;
                if descriptor.parameter.is_some() && value.is_none() {
                    self.state.command_search.as_mut().unwrap().parameter = Some(descriptor);
                } else {
                    // Dismiss before handing off a native dialog, restore on error.
                    let previous = self.state.command_search.take();
                    match self.execute_catalog_command(&id, value) {
                        Ok(mut change) => {
                            self.command_search.recent.retain(|v| v != &id);
                            self.command_search.recent.insert(0, id);
                            self.command_search.recent.truncate(5);
                            self.command_search.entries.clear();
                            change.revision = self.changed(regions::COMMAND_SEARCH, false).revision;
                            change.regions |= regions::COMMAND_SEARCH;
                            return Ok(change);
                        }
                        Err(error) => {
                            self.state.command_search = previous;
                            self.state.command_search.as_mut().unwrap().error = Some(error);
                        }
                    }
                }
            }
            A::Close | A::Focus { .. } => unreachable!(),
        }
        if let Some(view) = &mut self.state.command_search {
            view.refresh_detail();
        }
        Ok(self.changed(regions::COMMAND_SEARCH, false))
    }
}

#[allow(clippy::too_many_arguments)]
fn parameter_entry(
    id: String,
    label: impl AsRef<str>,
    canonical_label: &str,
    category: impl AsRef<str>,
    context: impl AsRef<str>,
    numeric: &NumericControl,
    value: f32,
    action: UiAction,
    enabled: bool,
    settings: &Settings,
    platform: Platform,
    l: &Localizer,
) -> Entry {
    let label = label.as_ref();
    let category = category.as_ref();
    let context = context.as_ref();
    let mut item = entry(&command_text(l, MessageId::COMMANDS_PARAMETER_LABEL, &[("label", label.to_owned())]), category, action, enabled, None, settings, platform, l);
    item.canonical_search = crate::search::normalize(canonical_label);
    item.search.push(' ');
    item.search.push_str(&item.canonical_search);
    item.search.push_str(" set adjust");
    item.descriptor.id = id;
    // Compact toolbar readouts round to tenths, which would misstate
    // bounds such as a pressure minimum of 0.25. Keep schema precision.
    let number = |value| {
        let text = numeric
            .resolve(value, NumericOperation::Format)
            .expect("valid numeric schema")
            .edit;
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text
        }
    };
    item.descriptor.description = command_text(l, MessageId::COMMANDS_PARAMETER_HELP, &[
        ("context", context.to_owned()), ("current", number(value as f64)),
        ("minimum", number(numeric.min)), ("maximum", number(numeric.max)),
        ("unit", numeric.unit.to_string()),
        ("hasUnit", if numeric.unit.is_empty() { "no" } else { "yes" }.to_owned()),
    ]);
    item.descriptor.parameter = Some(CommandParameter {
        numeric: numeric.clone(),
        value,
        text: numeric.compact_value(value as f64),
    });
    item
}

fn layer_error(error: layer_core::DocumentError, l: &Localizer) -> Arc<str> {
    match error {
        layer_core::DocumentError::InvalidLayerOperation(message) => Arc::from(message),
        layer_core::DocumentError::ProtectedLayer(_) => l.text(MessageId::COMMANDS_THE_LAYER_IS_LOCKED),
        _ => l.text(MessageId::COMMANDS_UNAVAILABLE_FOR_THE_SELECTED_LAYERS),
    }
}

/// Exact labels, prefixes, word matches, then ordered fuzzy characters. Work is
/// bounded by a small cached catalog and a capped query; no I/O or debounce.
fn search_score(query: &str, label: &str, text: &str) -> Option<i32> {
    if label.trim_end_matches('…') == query.trim() { return Some(10000); }
    if label.starts_with(query) { return Some(8000 - label.chars().count() as i32); }
    let mut score = 0;
    for term in query.split_whitespace() {
        if let Some(byte) = text.find(term) {
            let position = text[..byte].chars().count();
            score += 400 - position.min(200) as i32;
        } else {
            let mut rest = label.chars();
            let mut distance = 0;
            for character in term.chars() {
                distance += rest.by_ref().position(|candidate| candidate == character)?;
            }
            if distance > term.chars().count() * 3 { return None; }
            score += 100 - distance.min(99) as i32;
        }
    }
    Some(score)
}

fn command_text(l: &Localizer, id: MessageId, values: &[(&str, String)]) -> String {
    let mut args = localization::FluentArgs::new();
    for (name, value) in values { args.set(*name, value.as_str()); }
    l.format(id, &args)
}

#[cfg(test)]
mod scoring_tests {
    use super::search_score;
    #[test]
    fn fuzzy_distance_counts_characters_across_scripts() {
        assert_eq!(search_score("ad", "abcd", ""), search_score("一四", "一二三四", ""));
        assert_eq!(search_score("a", "abcd", ""), search_score("一", "一二三四", ""));
        assert_eq!(search_score("ab", "a1234567b", ""), None);
        assert_eq!(search_score("一二", "一三四五六七八九二", ""), None);
    }
}

fn panel_visibility_label(l: &Localizer, panel: Panel) -> String {
    let message = match panel {
        Panel::Toolbar => MessageId::COMMANDS_SHOW_TOOLBAR,
        Panel::Commands => MessageId::COMMANDS_SHOW_COMMANDS,
        Panel::Brushes => MessageId::COMMANDS_SHOW_BRUSHES,
        Panel::BrushSets => MessageId::COMMANDS_SHOW_BRUSH_SETS,
        Panel::FilterTypes => MessageId::COMMANDS_SHOW_FILTER_TYPES,
        Panel::SculptSets => MessageId::COMMANDS_SHOW_SCULPT_SETS,
        Panel::Tools => MessageId::COMMANDS_SHOW_TOOLS,
        Panel::ToolSettings => MessageId::COMMANDS_SHOW_TOOL_SETTINGS,
        Panel::Color => MessageId::COMMANDS_SHOW_COLOR,
        Panel::Palettes => MessageId::COMMANDS_SHOW_PALETTES,
        Panel::Sizes => MessageId::COMMANDS_SHOW_SIZES,
        Panel::Layers => MessageId::COMMANDS_SHOW_LAYERS,
        Panel::Adjustments => MessageId::COMMANDS_SHOW_ADJUSTMENTS,
        Panel::Properties => MessageId::COMMANDS_SHOW_PROPERTIES,
        Panel::Stats => MessageId::COMMANDS_SHOW_STATS,
        Panel::Navigator => MessageId::COMMANDS_SHOW_NAVIGATOR,
        Panel::Proof => MessageId::COMMANDS_SHOW_PROOF,
        Panel::CustomToolbar(_) => MessageId::COMMANDS_CUSTOM_TOOLBAR,
    };
    l.text(message).to_string()
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    use crate::session::test_support::{session, invoke};

    #[test]
    fn command_search_language_refresh_reprojects_disabled_error() {
        let mut session = session(Platform::Gtk);
        invoke(&mut session, CommandId::SearchCommands);
        session.command_search_action(CommandSearchAction::Query { text: "Undo".into() }).unwrap();
        session.command_search_action(CommandSearchAction::Execute { id: "command.undo".into(), value: None }).unwrap();
        let before = session.state.command_search.as_ref().unwrap().error.clone().unwrap();
        assert!(session.set_localization(Localizer::shared(UiLanguage::Japanese)));
        let view = session.state.command_search.as_ref().unwrap();
        let descriptor = view.results.iter().find(|entry| entry.id == "command.undo").unwrap();
        assert_ne!(view.error.as_ref().unwrap(), &before);
        assert_eq!(view.error, descriptor.disabled_reason);
    }

    #[test]
    fn command_search_language_refresh_retains_query_selection_recents_and_numeric_error() {
        let mut session = session(Platform::Gtk);
        invoke(&mut session, CommandId::SearchCommands);
        session.command_search_action(CommandSearchAction::Query { text: "size".into() }).unwrap();
        let entry = session.command_search.entries.iter().find(|entry| entry.descriptor.id == "tool_setting.size").unwrap().descriptor.clone();
        session.command_search_action(CommandSearchAction::Select { id: entry.id.clone() }).unwrap();
        session.command_search_action(CommandSearchAction::Execute { id: entry.id.clone(), value: None }).unwrap();
        session.command_search_action(CommandSearchAction::Execute { id: entry.id.clone(), value: Some("１２".into()) }).unwrap();
        session.command_search.recent = vec!["command.fit_canvas".into()];
        let before = session.state.command_search.clone().unwrap();
        let selected = before.results[before.selected].id.clone();
        let epoch = session.command_search.epoch;
        assert!(session.set_localization(Localizer::shared(UiLanguage::Japanese)));
        let after = session.state.command_search.as_ref().unwrap();
        assert_eq!(after.query, before.query);
        assert_eq!(after.results[after.selected].id, selected);
        assert_ne!(after.parameter.as_ref().unwrap().label, before.parameter.as_ref().unwrap().label);
        assert_eq!(after.parameter.as_ref().unwrap().parameter.as_ref().unwrap().text, before.parameter.as_ref().unwrap().parameter.as_ref().unwrap().text);
        assert_ne!(after.error, before.error);
        assert_eq!(session.command_search.epoch, epoch);
        assert_eq!(session.command_search.recent, ["command.fit_canvas"]);
    }
}
