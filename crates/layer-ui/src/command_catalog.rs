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
fn identity(action: &UiAction) -> String {
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
            action: CustomizationAction::SetPanelVisible { .. },
        } => format!("{label} panel"),
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
            disabled_reason: (!enabled)
                .then(|| "Unavailable in the current tool or edit target".into()),
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
    settings: &Settings,
    platform: Platform,
) {
    for item in items.into_iter().flatten() {
        if let Some(action) = item.action {
            let mut item_entry = entry(
                &item.label,
                path,
                action,
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
        let mut entries = Vec::new();
        for menu in ApplicationMenu::ALL {
            menu_entries(
                self.application_menu(menu).sections,
                menu.label(),
                &mut entries,
                settings,
                platform,
            );
        }
        // Menus own their ordering and validation. Commands outside menus still
        // share CommandState, including selection submodes and temporary locks.
        for command in CommandId::ALL
            .into_iter()
            .filter(|c| c.available_on(platform))
        {
            let state = self.command(command);
            let mut item = entry(
                state.label,
                "Commands",
                UiAction::Invoke { command },
                state.enabled,
                state.checkable.then_some(state.selected),
                settings,
                platform,
            );
            item.descriptor.disabled_reason = self.command_disabled_reason(command);
            entries.push(item);
        }
        for family in ToolFamily::ALL {
            entries.push(entry(
                family.label(),
                "Tools",
                UiAction::CycleTool { family },
                self.require_idle().is_ok(),
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
                self.require_idle().is_ok(),
                None,
                settings,
                platform,
            );
            item.descriptor.description = format!("Load this {} brush preset.", brush.category);
            entries.push(item);
        }
        for tool in self
            .state
            .tool_panels
            .tools
            .groups
            .iter()
            .chain(self.state.tool_panels.tools.subtools.iter())
        {
            entries.push(entry(
                tool.label,
                "Tools",
                tool.action.clone(),
                self.require_idle().is_ok(),
                None,
                settings,
                platform,
            ));
        }
        for setting in &self.state.tool_settings {
            let mut item = entry(
                &format!("{}…", setting.label),
                "Tool settings",
                UiAction::SetToolSetting {
                    id: setting.id.into(),
                    value: setting.value,
                },
                self.require_idle().is_ok(),
                None,
                settings,
                platform,
            );
            item.search.push_str(" set adjust");
            item.descriptor.id = format!("tool_setting.{}", setting.id);
            item.descriptor.kind = CommandKind::Parameter;
            let numeric = &setting.numeric;
            // Compact toolbar readouts round to tenths, which would misstate
            // bounds such as a pressure minimum of 0.25. Keep schema precision.
            let number = |value| {
                let text = numeric
                    .resolve(value, NumericOperation::Format)
                    .expect("valid tool numeric schema")
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
                "{} · Current {}{unit} · Range {}–{}{unit}",
                setting.label,
                number(setting.value as f64),
                number(numeric.min),
                number(numeric.max),
            );
            item.descriptor.parameter = Some(CommandParameter {
                numeric: setting.numeric.clone(),
                value: setting.value,
                text: setting.numeric.compact_value(setting.value as f64),
            });
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
                self.require_idle().is_ok(),
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
        // Use exactly the command's live predicate/reason even when its first
        // appearance was in a menu, and exclude the bar's own opener.
        for e in &mut entries {
            if let Some(UiAction::Invoke { command }) = e.action {
                e.descriptor.enabled = self.command_flags(command).0;
                e.descriptor.disabled_reason = self.command_disabled_reason(command);
            }
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
        if let Err(reason) = self.require_idle() {
            return Some(reason);
        }
        Some(
            match command {
                CommandId::Undo => "Nothing to undo",
                CommandId::Redo => "Nothing to redo",
                CommandId::Deselect
                | CommandId::InvertSelection
                | CommandId::FillSelection
                | CommandId::SaveSelectionLayer
                    if self.current_selection().is_none() =>
                {
                    "Create a selection first"
                }
                CommandId::ApplyTransform
                | CommandId::CancelTransform
                | CommandId::TransformAspect
                    if !self.operation.active() =>
                {
                    "Start a transform first"
                }
                CommandId::DeleteRuler => "Select a ruler first",
                CommandId::Reselect if self.selection_masks.reselect.is_none() => {
                    "No previous selection to restore"
                }
                _ if self.operation.active() => "Apply or cancel the transform first",
                _ if self.state.document_file.busy => "Wait for the current file operation",
                _ if self
                    .engine
                    .document()
                    .is_locked(self.engine.document().active_layer) =>
                {
                    "The active layer is locked"
                }
                _ => "Unavailable in the current tool or edit target",
            }
            .into(),
        )
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
            if let UiAction::SetToolSetting { value, .. } = &mut action {
                *value = number;
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
