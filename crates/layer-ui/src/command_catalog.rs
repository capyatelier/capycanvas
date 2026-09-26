//! Semantic commands and command search. Catalog entries are projections of the
//! existing command/menu/tool schemas; execution always returns to dispatch.
use super::*;
use serde::{Deserialize, Serialize};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandToolContext {
    pub category: ToolCategory,
    /// IDs from the current tool's parameter schema; never inferred from widgets.
    pub parameters: Vec<&'static str>,
    pub editing_mask: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Instant,
    Toggle,
    Parameter,
    Held,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandFocus {
    #[default]
    Canvas,
    Palette,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandHistory {
    /// The existing dispatcher/form owns history policy for this operation.
    Inherit,
    None,
    Document,
    Workspace,
    Palette,
    Native,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandTarget {
    Application,
    Document,
    ActiveLayer,
    Workspace,
    Palette,
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
    pub kind: CommandKind,
    pub target: CommandTarget,
    pub history: CommandHistory,
    pub repeat: bool,
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

struct Entry {
    descriptor: CommandDescriptor,
    action: Option<UiAction>,
    search: String,
}

#[derive(Default)]
pub(super) struct CommandSearch {
    pub(super) revision: u64,
    entries: Vec<Entry>,
    recent: Vec<String>,
    epoch: u64,
    focus: CommandFocus,
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
    label: &str,
    category: &str,
    action: UiAction,
    enabled: bool,
    selected: Option<bool>,
    settings: &Settings,
    platform: Platform,
) -> Entry {
    let action = crate::shortcuts::action_command(&action)
        .map(|command| UiAction::Invoke { command })
        .unwrap_or(action);
    let presentation_label = match &action {
        UiAction::Customize {
            action: CustomizationAction::SetPanelVisible { panel, .. },
        } => {
            let noun = if panel.kind() == PanelKind::Tiles { "toolbar" } else { "panel" };
            if label.to_lowercase().ends_with(noun) {
                label.to_owned()
            } else {
                format!("{label} {noun}")
            }
        }
        _ => label.to_owned(),
    };
    let label = presentation_label.as_str();
    let kind = if selected.is_some() {
        CommandKind::Toggle
    } else {
        CommandKind::Instant
    };
    let target = match &action {
        UiAction::Layer { .. } => CommandTarget::ActiveLayer,
        UiAction::Customize { .. } | UiAction::WorkspaceManager { .. } => CommandTarget::Workspace,
        UiAction::Color {
            action: ColorAction::Library { .. },
        } => CommandTarget::Palette,
        UiAction::Invoke {
            command:
                CommandId::Settings
                | CommandId::KeyboardShortcuts
                | CommandId::About
                | CommandId::Website
                | CommandId::SourceCode
                | CommandId::SearchCommands
                | CommandId::NewWindow,
        } => CommandTarget::Application,
        _ => CommandTarget::Document,
    };
    let history = match &action {
        UiAction::Layer {
            action: LayerAction::Tool { .. },
        } => CommandHistory::None,
        UiAction::Layer { .. } | UiAction::Effect { .. } | UiAction::Selection { .. } => {
            CommandHistory::Document
        }
        UiAction::Invoke {
            command:
                CommandId::Undo
                | CommandId::Redo
                | CommandId::AddLayer
                | CommandId::DeleteLayer
                | CommandId::RaiseLayer
                | CommandId::LowerLayer
                | CommandId::ClearLayer
                | CommandId::FillSelection
                | CommandId::SelectAll
                | CommandId::Deselect
                | CommandId::InvertSelection,
        } => CommandHistory::Document,
        UiAction::Invoke {
            command: CommandId::UndoWorkspace | CommandId::RedoWorkspace,
        }
        | UiAction::Customize { .. } => CommandHistory::Workspace,
        UiAction::Color {
            action:
                ColorAction::Library {
                    action:
                        ColorLibraryAction::UndoReorder { .. } | ColorLibraryAction::RedoReorder { .. },
                },
        } => CommandHistory::Palette,
        _ => CommandHistory::Inherit,
    };
    let repeat = matches!(
        &action,
        UiAction::Invoke {
            command: CommandId::Undo | CommandId::Redo
        }
    );
    let shortcut = settings
        .action_keys(&action, platform)
        .into_iter()
        .find(|k| k.available(platform))
        .map(|k| k.label(platform))
        .unwrap_or_default();
    let aliases = match &action {
        UiAction::Invoke {
            command: CommandId::Settings,
        } => "preferences settings options",
        UiAction::Invoke {
            command: CommandId::Eyedropper,
        } => "color picker sampler",
        UiAction::Invoke {
            command: CommandId::Move,
        } => "move object layer",
        UiAction::Invoke {
            command: CommandId::ScaleRotate,
        } => "transform resize scale rotate",
        UiAction::Invoke {
            command: CommandId::FitCanvas,
        } => "fit canvas zoom drawing page",
        UiAction::Invoke {
            command: CommandId::Deselect,
        } => "clear remove selection deselect",
        UiAction::Invoke {
            command: CommandId::ToggleTheme,
        } => "appearance theme light dark",
        _ => "",
    };
    Entry {
        descriptor: CommandDescriptor {
            id: identity(&action),
            label: label.into(),
            category: category.into(),
            description: action_description(&action).into(),
            kind,
            target,
            history,
            repeat,
            enabled,
            disabled_reason: None,
            selected: selected.unwrap_or(false),
            shortcut,
            parameter: None,
        },
        action: Some(action),
        search: format!("{label} {category} {aliases}").to_lowercase(),
    }
}

fn action_description(action: &UiAction) -> &'static str {
    use CommandId::*;
    match action {
        UiAction::Invoke { command } => match command {
            DrawingBrush => "Return to the last drawing brush.",
            Sculpt => "Return to the last sculpting tool.",
            Pen | Pencil | Brush | Eraser | Airbrush | Decoration | Blend | Liquify => {
                "Use the last brush selected in this tool family."
            }
            Select => "Return to the last selection tool.",
            SelectionBrush => "Paint the area that subsequent edits will affect.",
            SelectionIntersect => "Keep only the area shared by the existing and new selections.",
            SelectionVisible => "Find matching colors across the visible artwork.",
            SelectionEditing => "Find matching colors in the editing layer only.",
            SelectionReference => "Find matching colors in layers marked as references.",
            SelectionFixedRatio => "Keep the selection's width-to-height ratio fixed.",
            SelectionFixedSize => "Use the configured selection width and height.",
            QuickMask => "Edit the selection as a painted mask.",
            Reselect => "Restore the previous pixel selection.",
            SaveSelectionLayer => "Keep the current selection as a reusable selection layer.",
            Move => "Move artwork and manage image placement on the canvas.",
            ScaleRotate => "Resize or rotate the current transform target.",
            FitCanvas => "Adjust the zoom to show the entire canvas.",
            FlipHorizontal | FlipVertical => "Mirror the view without changing the artwork.",
            RotateLeft | RotateRight => "Rotate the view without changing the artwork.",
            UndoWorkspace => "Restore the previous toolbar, panel or workspace layout.",
            RedoWorkspace => "Reapply an undone workspace layout change.",
            ZenMode => "Hide or restore workspace controls to give the canvas more room.",
            _ => "",
        },
        UiAction::CycleTool { .. } => "Cycle through tools in this family.",
        _ => "",
    }
}

fn menu_entries(
    items: Vec<Vec<ContextMenuItem>>,
    path: &str,
    entries: &mut Vec<Entry>,
    alias: &dyn Fn(UiAction) -> UiAction,
    settings: &Settings,
    platform: Platform,
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
            );
            if item_entry.descriptor.description.is_empty() {
                item_entry.descriptor.description = format!("Menu: {path} › {}", item.label);
            }
            entries.push(item_entry);
        }
        if !item.sections.is_empty() {
            menu_entries(
                item.sections,
                &format!("{path} › {}", item.label),
                entries,
                alias,
                settings,
                platform,
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
    pub fn command_tool_context(&self) -> CommandToolContext {
        use LayerCanvasTool as T;
        let category = match self.layer_interaction.tool {
            T::Paint => match self.state.brush.tool {
                Tool::Eraser => ToolCategory::Erasing,
                Tool::Blend => ToolCategory::Blending,
                Tool::Liquify => ToolCategory::Warping,
                _ => ToolCategory::Drawing,
            },
            T::Hand => ToolCategory::Navigation,
            T::PickVisible | T::PickLayer => ToolCategory::ColorSampling,
            T::Move | T::Transform => ToolCategory::MoveTransform,
            T::Figure { .. } | T::Ruler { .. } => ToolCategory::ShapesRulers,
            T::Gradient { .. } | T::LassoFill | T::Region { fill: true, .. } => {
                ToolCategory::FillGradient
            }
            _ => ToolCategory::Selection,
        };
        CommandToolContext {
            category,
            parameters: self.state.tool_settings.iter().map(|s| s.id).collect(),
            editing_mask: self.selection_masks.target().is_some()
                || self.engine.document().active_mask,
        }
    }

    pub fn command_catalog(&self) -> Vec<CommandDescriptor> {
        self.catalog_entries()
            .into_iter()
            .map(|e| e.descriptor)
            .collect()
    }

    fn catalog_entries(&self) -> Vec<Entry> {
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
                menu.label(),
                &mut entries,
                &alias,
                settings,
                platform,
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
                state.label,
                "Commands",
                UiAction::Invoke { command },
                state.enabled,
                state.checkable.then_some(state.selected),
                settings,
                platform,
            ));
        }
        for family in ToolFamily::ALL {
            entries.push(entry(
                family.label(),
                "Tools",
                UiAction::CycleTool { family },
                idle,
                None,
                settings,
                platform,
            ));
        }
        for brush in brush_catalog() {
            let mut item = entry(
                brush.label,
                "Brushes",
                UiAction::SelectBrush { id: brush.id },
                idle,
                None,
                settings,
                platform,
            );
            item.descriptor.description = format!("Load this {} brush preset.", brush.category);
            entries.push(item);
        }
        let panels = &self.state.tool_panels;
        for set in panels.brush_sets.groups.iter().chain(&panels.sculpt_sets.groups) {
            let mut item = entry(
                &format!("{} brushes", set.label),
                "Brush sets",
                set.action.clone(),
                idle,
                None,
                settings,
                platform,
            );
            item.descriptor.description = "Use the last brush chosen in this set.".into();
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
                .map_or("", |command| self.command(command).label);
            let view = tools::view(&self.state.brush, tool);
            for item in view.groups.iter().chain(&view.subtools) {
                if !matches!(item.action, UiAction::Invoke { .. }) {
                    entries.push(entry(
                        &format!("{family} › {}", item.label),
                        "Tool options",
                        item.action.clone(),
                        idle,
                        Some(same && item.selected),
                        settings,
                        platform,
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
                        .map_or(label, |command| self.command(command).label);
                    entries.push(entry(
                        &format!("{family} › {}", item.label),
                        "Tool options",
                        item.action,
                        idle,
                        Some(item.selected),
                        settings,
                        platform,
                    ));
                }
            }
        }
        for setting in &self.state.tool_settings {
            let item = parameter_entry(
                format!("tool_setting.{}", setting.id),
                setting.label,
                "Tool settings",
                setting.label,
                &setting.numeric,
                setting.value,
                UiAction::SetToolSetting {
                    id: setting.id.into(),
                    value: setting.value,
                },
                idle,
                settings,
                platform,
            );
            entries.push(item);
        }
        let properties = &self.state.layer_properties;
        if properties.layer == Some(active)
            && !self.selection_masks.quick()
            && document.layer(document.active_layer).is_some_and(|l| l.kind != LayerKind::Selection)
        {
            let set = |key: &str, value| UiAction::Effect {
                action: EffectAction::Set { layer: active, key: key.into(), value },
            };
            for control in &properties.controls {
                let context = format!("{} · {}", properties.title, control.label);
                let label = if matches!(control.key.as_str(), "opacity" | "blend") {
                    format!("Layer {}", control.label.to_lowercase())
                } else {
                    format!("{} {}", properties.title, control.label.to_lowercase())
                };
                match (&control.kind, &control.value) {
                    (PropertyKind::Number { numeric }, layer_core::EffectValue::Number(value)) => {
                        entries.push(parameter_entry(
                            format!("layer_property.{}", control.key),
                            &label,
                            "Layer properties",
                            &context,
                            numeric,
                            *value,
                            set(&control.key, layer_core::EffectValue::Number(*value)),
                            properties.enabled,
                            settings,
                            platform,
                        ));
                    }
                    (PropertyKind::Choice { options }, layer_core::EffectValue::Choice(current)) => {
                        for (i, option) in options.iter().enumerate() {
                            let mut item = entry(
                                &format!("{label}: {option}"),
                                "Layer properties",
                                set(&control.key, layer_core::EffectValue::Choice(i as u32)),
                                properties.enabled,
                                Some(i as u32 == *current),
                                settings,
                                platform,
                            );
                            item.descriptor.description = context.clone();
                            entries.push(item);
                        }
                    }
                    (PropertyKind::Toggle, layer_core::EffectValue::Toggle(on)) => {
                        let mut item = entry(
                            &label,
                            "Layer properties",
                            set(&control.key, layer_core::EffectValue::Toggle(!on)),
                            properties.enabled,
                            Some(*on),
                            settings,
                            platform,
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
                    "Workspaces",
                    UiAction::WorkspaceManager {
                        command: WorkspaceCommand::Switch { id: choice.id.clone() },
                    },
                    switchable || current,
                    Some(current),
                    settings,
                    platform,
                );
                item.descriptor.description = "Switch to this workspace.".into();
                entries.push(item);
            }
        }
        for (slot, label) in crate::color::PAINT_SLOTS {
            let mut item = entry(
                label,
                "Color",
                UiAction::Color { action: ColorAction::Select { slot } },
                idle,
                Some(self.state.colors.slot == slot),
                settings,
                platform,
            );
            item.descriptor.description = "Paint with this color.".into();
            entries.push(item);
        }
        for (label, action) in [
            ("Swap foreground and background", ColorAction::Swap),
            ("Black", ColorAction::QuickColor { white: false }),
            ("White", ColorAction::QuickColor { white: true }),
        ] {
            entries.push(entry(
                label,
                "Color",
                UiAction::Color { action },
                idle,
                None,
                settings,
                platform,
            ));
        }
        let mut pan = entry(
            "Pan while held",
            "Navigation",
            UiAction::Invoke {
                command: CommandId::Hand,
            },
            true,
            None,
            settings,
            platform,
        );
        pan.descriptor.id = "canvas.pan".into();
        pan.descriptor.kind = CommandKind::Held;
        pan.descriptor.description =
            "Temporarily pan the view; release to return to the tool.".into();
        pan.descriptor.shortcut = settings.shortcut_label("canvas.pan", platform);
        pan.action = None;
        entries.push(pan);
        let mut seen = std::collections::BTreeSet::new();
        entries.retain(|e| seen.insert(e.descriptor.id.clone()));
        let mut labels = std::collections::BTreeMap::<String, usize>::new();
        for e in &entries {
            *labels.entry(e.descriptor.label.to_lowercase()).or_default() += 1;
        }
        for e in &mut entries {
            let noun = match &e.action {
                Some(UiAction::SelectBrush { .. }) => "brush",
                Some(UiAction::Effect { action: EffectAction::Insert { .. } }) => "filter",
                _ => continue,
            };
            if labels[&e.descriptor.label.to_lowercase()] > 1 {
                e.descriptor.label = format!("{} {noun}", e.descriptor.label);
                e.search = format!("{} {}", e.descriptor.label.to_lowercase(), e.search);
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
                _ => None,
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
                        "Redo Color Reorder"
                    } else {
                        "Undo Color Reorder"
                    }
                    .into();
                    e.descriptor.category = "Palette".into();
                    e.descriptor.description = if redo {
                        "Reapply the last undone color reorder in this palette."
                    } else {
                        "Restore the previous color order in this palette."
                    }
                    .into();
                    e.descriptor.target = CommandTarget::Palette;
                    e.descriptor.history = CommandHistory::Palette;
                    e.descriptor.enabled =
                        self.state.colors.library.can_undo_reorder(palette, redo);
                    e.descriptor.disabled_reason =
                        (!e.descriptor.enabled).then(|| "No color reorder to restore".into());
                    e.search = e.descriptor.label.to_lowercase();
                }
            }
        }
        if self.command_search.focus == CommandFocus::Text {
            for (id, label) in [
                ("command.undo", "Undo Text Edit"),
                ("command.redo", "Redo Text Edit"),
            ] {
                if let Some(e) = entries.iter_mut().find(|e| e.descriptor.id == id) {
                    e.descriptor.label = label.into();
                    e.descriptor.category = "Text editing".into();
                    e.descriptor.history = CommandHistory::Native;
                    e.descriptor.enabled = false;
                    e.descriptor.disabled_reason =
                        Some("Close command search to undo or redo in the text field".into());
                    e.search = label.to_lowercase();
                }
            }
        }
        entries
    }

    pub(super) fn command_disabled_reason(&self, command: CommandId) -> Option<String> {
        use CommandId as C;
        if self.command_flags(command).0 {
            return None;
        }
        if !command.available_on(self.state.platform) {
            return Some("Not available on this platform".into());
        }
        if self.state.document_file.close_ready {
            return Some("This drawing is closing".into());
        }
        if self.rendering_suspended && !Self::command_without_renderer(command) {
            return Some("Painting is unavailable. Save the drawing and reopen it.".into());
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
            | C::DocumentProperties
            | C::NewDocument
            | C::OpenDocument
            | C::ExportDocument
            | C::SelectAll
            | C::Deselect
            | C::InvertSelection
            | C::ClearLayer
            | C::FillSelection => self.require_document_idle(),
            C::SaveDocument | C::SaveDocumentAs => self.require_raster_snapshot(),
            C::CloseDocument => self.require_document_snapshot_idle(),
            C::ResetLayout if self.managed_workspace.is_some() => self.require_workspace_idle(),
            C::CompleteSelection | C::CancelSelection | C::GamutWarning | C::UndoWorkspace | C::RedoWorkspace => Ok(()),
            _ => self.require_idle(),
        };
        if let Err(reason) = gate {
            return Some(reason);
        }
        let document = self.engine.document();
        let active = document.layer(document.active_layer);
        let paint = active.is_some_and(|l| l.kind == LayerKind::Paint);
        let locked = document.is_locked(document.active_layer);
        let mask_target = self.selection_masks.target();
        let selection = self.current_selection().is_some();
        let reason = match command {
            C::Undo => "Nothing to undo",
            C::Redo => "Nothing to redo",
            C::UndoWorkspace | C::RedoWorkspace if self.state.customization.header_editing => {
                "Finish customizing the title bar first"
            }
            C::UndoWorkspace => "No workspace change to undo",
            C::RedoWorkspace => "No workspace change to redo",
            C::ReturnToArtwork
            | C::ResetMaskColors
            | C::SwapMaskColors
            | C::MaskOverlayProtected
            | C::FillSelectionMask
            | C::ClearSelectionMask
                if mask_target.is_none() =>
            {
                "Edit a selection mask first"
            }
            C::MaskOverlayProtected | C::FillSelectionMask | C::ClearSelectionMask => {
                "This selection layer is locked"
            }
            C::ScaleRotate
            | C::ClearLayer
            | C::Figure
            | C::Move
            | C::FillSelection
            | C::RepairSourceProfile
            | C::RasterizeSource
                if mask_target.is_some() =>
            {
                "Return to the artwork first"
            }
            C::QuickMask | C::NewSelectionLayer | C::PlacementOriginalSize | C::ScaleRotate
                if self.operation.active() && !self.operation.placing() =>
            {
                "Apply or cancel the transform first"
            }
            C::PlacementOriginalSize => "Place an image first",
            C::Reselect if selection => "Deselect before restoring the previous selection",
            C::Reselect => "No previous selection to restore",
            C::Deselect | C::InvertSelection | C::FillSelection | C::SaveSelectionLayer if !selection => {
                "Create a selection first"
            }
            C::DeleteLayer if self.selection_masks.quick() => "Leave Quick Mask first",
            C::DeleteLayer => {
                return Some(
                    document
                        .delete_layers_edit(&[document.active_layer])
                        .err()
                        .map_or_else(|| "This layer can't be deleted".into(), layer_error),
                );
            }
            C::SdrRendition | C::PreviewSdr if !document.color.depth.is_float() => {
                "Requires a high dynamic range drawing"
            }
            C::PreviewSdr if !self.state.hdr_display_available => "Requires a high dynamic range display",
            C::PreviewSdr if self.state.sdr_appearance_preview.is_some() => {
                "Close the SDR appearance preview first"
            }
            C::PreviewSdr => "Turn off soft proofing and the gamut warning first",
            C::GamutWarning => "Set up soft proofing first",
            C::ResetLayout if self.managed_workspace.is_some() => "The layout already matches its starting state",
            C::RepairSourceProfile | C::RasterizeSource if document.active_mask => "Return to the layer's artwork first",
            C::RepairSourceProfile | C::RasterizeSource => "Select an unlocked retained image layer",
            C::ApplyTransform | C::CancelTransform | C::TransformAspect => "Start a transform first",
            C::SnapRulers => "Show rulers first",
            C::DeleteRuler => "Select a ruler first",
            C::CompleteSelection
                if self.layer_interaction.tool
                    != (LayerCanvasTool::Selection { kind: SelectionTool::Polygon }) =>
            {
                "Use the polygon selection tool"
            }
            C::CompleteSelection => "Place at least three points first",
            C::CancelSelection => "No selection path to cancel",
            C::SelectionVisible | C::SelectionEditing | C::SelectionReference => "Choose a selection tool first",
            C::ZoomIn => "Already at the maximum zoom",
            C::ZoomOut => "Already at the minimum zoom",
            _ if self.operation.active() => "Apply or cancel the transform first",
            _ if self.state.document_file.busy => "Wait for the current file operation",
            _ if locked => "The active layer is locked",
            C::ScaleRotate => "Select unlocked paint content or a layer mask",
            C::ClearLayer | C::FillSelection | C::RaiseLayer | C::LowerLayer if !paint => "Select a paint layer",
            C::ClearLayer | C::FillSelection if document.active_mask => "Return to the layer's artwork first",
            C::RaiseLayer => "The layer is already at the top",
            C::LowerLayer => "The layer is already at the bottom",
            _ => "Unavailable in the current tool or edit target",
        };
        Some(reason.into())
    }

    fn action_disabled_reason(&self, action: &UiAction) -> String {
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
        let reason = match action {
            UiAction::Layer { action: LayerAction::GroupSelected } => {
                document.group_layers_edit(&roots, LayerId(0)).err().map(layer_error)
            }
            UiAction::Layer { action: LayerAction::Ungroup { .. } } => {
                document.ungroup_layer_edit(document.active_layer).err().map(layer_error)
            }
            UiAction::Layer { action: LayerAction::DeleteSelected } => {
                document.delete_layers_edit(&roots).err().map(layer_error)
            }
            UiAction::Layer { action: LayerAction::Delete { .. } } => {
                document.delete_layers_edit(&[document.active_layer]).err().map(layer_error)
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
                Some("Create a selection first".into())
            }
            UiAction::Layer { action: LayerAction::PasteMask { .. } }
                if self.layer_interaction.clipboard_mask.is_none() =>
            {
                Some("Copy a layer mask first".into())
            }
            UiAction::Layer { action: LayerAction::CopyMask { .. } | LayerAction::ApplyMask { .. } }
                if document.layer(document.active_layer).is_some_and(|l| l.mask.is_none()) =>
            {
                Some("The layer has no mask".into())
            }
            UiAction::Layer { action: LayerAction::ReferenceSelection } => {
                Some("Mark layers as references first".into())
            }
            UiAction::Effect { .. } if self.selection_masks.target().is_some() || document.active_mask => {
                Some("Return to the artwork before applying a filter".into())
            }
            UiAction::Layer { .. } | UiAction::Effect { .. } | UiAction::Selection { .. }
                if document.is_locked(document.active_layer) =>
            {
                Some("The active layer is locked".into())
            }
            UiAction::Layer { .. } | UiAction::Effect { .. }
                if document.layer(document.active_layer).is_some_and(|l| l.kind == LayerKind::Background) =>
            {
                Some("The background can't be changed this way".into())
            }
            _ => None,
        };
        reason.unwrap_or_else(|| "Unavailable in the current tool or edit target".into())
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
        let terms = query.to_lowercase();
        let mut matches: Vec<_> = self
            .command_search
            .entries
            .iter()
            .filter_map(|entry| {
                let d = &entry.descriptor;
                if d.kind == CommandKind::Held || d.id == "command.search_commands" {
                    return None;
                }
                if d.category == "Brushes" && matches!(terms.trim(), "brush" | "brushes") {
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
                    search_score(&terms, &d.label.to_lowercase(), &entry.search)?
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
        let entry = self
            .catalog_entries()
            .into_iter()
            .find(|e| e.descriptor.id == id)
            .ok_or("This command is no longer available")?;
        if !entry.descriptor.enabled {
            return Err(entry.descriptor.disabled_reason.unwrap_or_default());
        }
        let mut action = entry.action.ok_or("This command requires a held input")?;
        if let Some(parameter) = entry.descriptor.parameter {
            let text = value.ok_or("Enter a value for this command")?;
            let number = parameter
                .numeric
                .resolve(
                    parameter.value as f64,
                    NumericOperation::Expression { text },
                )?
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
        if self.command_search.epoch != self.state.document_file.epoch
            || self.state.command_search.is_none()
        {
            return Err("This command search belongs to a previous drawing".into());
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
                    .ok_or("Choose a current search result")?;
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
    label: &str,
    category: &str,
    context: &str,
    numeric: &NumericControl,
    value: f32,
    action: UiAction,
    enabled: bool,
    settings: &Settings,
    platform: Platform,
) -> Entry {
    let mut item = entry(&format!("{label}…"), category, action, enabled, None, settings, platform);
    item.search.push_str(" set adjust");
    item.descriptor.id = id;
    item.descriptor.kind = CommandKind::Parameter;
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
    let unit = if numeric.unit.is_empty() {
        String::new()
    } else {
        format!(" {}", numeric.unit)
    };
    item.descriptor.description = format!(
        "{context} · Current {}{unit} · Range {}–{}{unit}",
        number(value as f64),
        number(numeric.min),
        number(numeric.max),
    );
    item.descriptor.parameter = Some(CommandParameter {
        numeric: numeric.clone(),
        value,
        text: numeric.compact_value(value as f64),
    });
    item
}

fn layer_error(error: layer_core::DocumentError) -> String {
    match error {
        layer_core::DocumentError::InvalidLayerOperation(message) => message.into(),
        layer_core::DocumentError::ProtectedLayer(_) => "The layer is locked".into(),
        _ => "Unavailable for the selected layers".into(),
    }
}

/// Exact labels, prefixes, word matches, then ordered fuzzy characters. Work is
/// bounded by a small cached catalog and a capped query; no I/O or debounce.
fn search_score(query: &str, label: &str, text: &str) -> Option<i32> {
    if label.trim_end_matches('…') == query.trim() {
        return Some(10000);
    }
    if label.starts_with(query) {
        return Some(8000 - label.len() as i32);
    }
    let mut score = 0;
    for term in query.split_whitespace() {
        if let Some(i) = text.find(term) {
            score += 400 - i.min(200) as i32;
        } else {
            // Fuzzy characters belong to the command name, not distant words
            // in a menu path ("undo" must not match "Brushes › Window").
            let mut rest = label;
            let mut distance = 0;
            for c in term.chars() {
                let i = rest.find(c)?;
                distance += i;
                rest = &rest[i + c.len_utf8()..];
            }
            if distance > term.len() * 3 {
                return None;
            }
            score += 100 - distance.min(99) as i32;
        }
    }
    Some(score)
}
