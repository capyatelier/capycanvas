//! Validated workspace adoption, without replaying document/tool commands.
use super::*;

impl Default for EditingState {
    fn default() -> Self { Self::new_localized(&Localizer::shared(UiLanguage::English)) }
}
impl EditingState {
    pub fn new_localized(localization: &Localizer) -> Self {
        let region = region_tools::RegionTools::default();
        let layer = art_layers::LayerInteraction::default();
        let mut color_library = ColorLibrary::fresh(&localization.text(MessageId::CREATION_PALETTE_MY_COLORS));
        color_library.ensure_starters_localized(localization);
        Self {
            version: 1,
            preset: layer_core::DefaultBrushPreset::GPen as u32,
            tools: ToolMemory::default(),
            tool_slots: ToolSlotMemory::default(),
            colors: ColorState::default(),
            mask_colors: selection_masks::SelectionMasks::default().colors,
            color_library,
            canvas_tool: LayerCanvasTool::Paint,
            selection: SelectionOptions::default(),
            region_values: region
                .fields()
                .into_iter()
                .map(|(id, _, _, _, value)| (id.into(), value))
                .collect(),
            region_sources: region.source,
            gradient: layer.gradient,
            figure: layer.figure,
        }
    }
    pub fn validate(&self) -> Result<(), WorkspaceValidationError> {
        if self.version != 1 { return Err("Unsupported editing-state version".into()); }
        self.tools.validate()?;
        for (slot, variant) in &self.tool_slots.choices {
            if !slot.variants().contains(variant) { return Err("Invalid remembered tool variant".into()); }
        }
        self.selection.validate()?;
        if let LayerCanvasTool::Selection { kind } = self.canvas_tool
            && !matches!(kind, SelectionTool::Rectangle | SelectionTool::Ellipse | SelectionTool::Polygon | SelectionTool::Brush | SelectionTool::Tonal) {
            return Err("Invalid geometric selection tool".into());
        }
        self.colors.validate()?;
        self.mask_colors.validate()?;
        self.color_library.validate()?;
        let mut brush = self.tools.brush(preset(self.preset)?);
        self.colors.load_paint(&mut brush, layer_core::color::RgbSpace::Srgb)?;
        brush.validate().map_err(error)?;
        let mut region = region_tools::RegionTools::default();
        for (id, &value) in &self.region_values { region.edit_value(id, value)?; }
        Ok(())
    }

}
impl WorkspaceCapture {
    pub fn from_template(layout: &DockLayout) -> Result<Self, String> {
        layout.validate()?;
        Ok(Self {
            history: LayoutHistory::new(layout),
            working: WorkspaceWorkingState::default(),
        })
    }
    pub fn validate_structure(&self) -> Result<(), WorkspaceValidationError> {
        PreparedWorkspace::new(self.clone()).map(|_| ())
    }
}

pub struct PreparedWorkspace {
    capture: WorkspaceCapture,
}
impl PreparedWorkspace {
    pub fn new(capture: WorkspaceCapture) -> Result<Self, WorkspaceValidationError> {
        capture.history.validate()?;
        if capture.working.version != 1 { return Err("Unsupported workspace working-state version".into()); }
        Ok(Self { capture })
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    /// An `exact_name` is a typed name and must not collide; library copies
    /// otherwise receive an unused name.
    pub fn install_workspace_toolbar(
        &mut self,
        mut config: PanelConfig,
        replace: Option<Panel>,
        group: Option<u32>,
        exact_name: bool,
    ) -> Result<(Panel, UiChange), String> {
        self.require_workspace_idle()?;
        config.id = Panel::Toolbar;
        config.validate()?;
        let PanelContent::Toolbar { name, tiles } = &config.content else {
            return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_CHOOSE_A_TOOLBAR).to_string());
        };
        let title = name.clone().unwrap_or_else(|| config.id.localized_label(self.localization()).to_string());
        let name = &title;
        let controls: Vec<_> = tiles.iter().map(|t| t.control).collect();
        let before = self.state.workspace.clone();
        let mut layout = before.layout.clone();
        if exact_name {
            layout.toolbar_name_refusal(name, replace).map_err(|reason| reason.message(self.localization()).to_string())?;
        }
        let panel = if let Some(panel) = replace {
            layout.panel(panel)?;
            if panel.kind() != PanelKind::Tiles {
                return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_CHOOSE_A_TOOLBAR_TO_REPLACE).to_string());
            }
            let name = if layout.check_toolbar_name(name, Some(panel)).is_ok() {
                name.clone()
            } else {
                layout.unused_toolbar_name(name)
            };
            layout
                .panels
                .iter_mut()
                .find(|p| p.id == panel)
                .unwrap()
                .tiles_mut()?
                .clear();
            layout.rename_toolbar(panel, &name)?;
            if !controls.is_empty() {
                layout.insert_tools(panel, None, &controls)?;
            }
            panel
        } else {
            let name = layout.unused_toolbar_name(name);
            layout.add_toolbar(group, &name, &controls)?
        };
        let added = layout.panels.iter_mut().find(|p| p.id == panel).unwrap();
        added.tile_style = config.tile_style;
        added.hide_tab = config.hide_tab;
        layout.validate()?;
        self.state.workspace.layout = layout;
        let description = LayoutChange::panels(
            if replace.is_some() { LayoutPanelAction::Replaced } else { LayoutPanelAction::Added },
            vec![workspace::description::panel_name(&self.state.workspace.layout, panel)],
        );
        self.workspace_history.record_named(before, &self.state.workspace, description);
        self.state.customization = CustomizationState::default();
        self.sync_work_area();
        self.refresh_commands();
        Ok((
            panel,
            self.changed(
                regions::LAYOUT | regions::CUSTOMIZATION | regions::COMMANDS,
                false,
            ),
        ))
    }
    pub fn configure_workspace_manager(
        &mut self,
        workspace: ManagedWorkspace,
    ) -> Result<UiChange, String> {
        workspace.baseline.validate()?;
        self.managed_workspace = Some(workspace);
        self.refresh_commands();
        Ok(self.changed(regions::COMMANDS, false))
    }
    pub fn begin_workspace_transition(&mut self) -> Result<(), String> {
        self.require_workspace_idle()?;
        if self.workspace_transition {
            return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_A_WORKSPACE_CHANGE_IS_ALREADY_IN_PROGRESS).to_string());
        }
        self.workspace_transition = true;
        self.refresh_commands();
        self.changed(regions::COMMANDS, false);
        Ok(())
    }
    pub fn set_workspace_read_only(&mut self, read_only: bool) {
        if self.workspace_read_only != read_only {
            self.workspace_read_only = read_only;
            self.refresh_commands();
            self.changed(regions::COMMANDS, false);
        }
    }
    /// Whether canvas contacts are refused, as while a stored workspace loads.
    pub fn workspace_read_only(&self) -> bool {
        self.workspace_read_only
    }
    pub fn end_workspace_transition(&mut self) {
        self.workspace_transition = false;
        self.refresh_commands();
        self.changed(regions::COMMANDS, false);
    }
    pub fn require_workspace_idle(&self) -> Result<(), String> {
        self.require_document_snapshot_idle()?;
        if self.workspace_history.gesture_start().is_some()
            || self.workspace_drag.is_some()
            || self.divider_drag.is_some()
            || self.floating_resize.is_some()
            || self.interaction.pointer.is_some()
            || self.navigator_drag.is_some()
            || self.state.document_file.busy
        {
            return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_FINISH_THE_CURRENT_INTERACTION_BEFORE_SWITCHING_WORKSPACES).to_string());
        }
        Ok(())
    }

    pub fn editing_state(&self) -> EditingState {
        let restore_tool = |restore: crate::interaction::Restore| match restore {
            crate::interaction::Restore::Tool(tool, preset) => Some((tool, preset)),
            _ => None,
        };
        let base = self.interaction.hold_base
            .or_else(|| self.interaction.spring.as_ref().and_then(|spring| restore_tool(spring.restore)))
            .or_else(|| self.interaction.restores.iter().rev().find_map(|restore| restore_tool(*restore)));
        let (canvas_tool, preset) = base.unwrap_or((self.eyedropper.picking.previous
            .or(self.operation.crop.as_ref().map(|crop| crop.previous()))
            .unwrap_or(self.layer_interaction.tool), self.state.brush.preset));
        let mut tools = self.tools.clone();
        if base.is_some() { tools.remember(preset); }
        EditingState {
            version: 1,
            preset,
            tools,
            tool_slots: self.state.tool_slots.clone(),
            colors: self.state.colors.clone(),
            mask_colors: self.selection_masks.colors.clone(),
            color_library: self.state.color_library.clone(),
            canvas_tool,
            selection: self.selection_tools.options.clone(),
            region_values: self
                .region_tools
                .fields()
                .into_iter()
                .map(|(id, _, _, _, value)| (id.into(), value))
                .collect(),
            region_sources: self.region_tools.source,
            gradient: self.layer_interaction.gradient,
            figure: self.layer_interaction.figure,
        }
    }
    pub fn restore_editing(&mut self, mut state: EditingState) -> Result<UiChange, String> {
        self.require_workspace_idle()?;
        state.validate().map_err(|reason| reason.message(self.localization()))?;
        let space = self.engine.document().composition().color.space;
        state.colors.set_rgb_space(space)?;
        state.colors.set_document_depth(self.engine.document().composition().color.depth)?;
        state.mask_colors.set_document_depth(layer_core::color::SampleDepth::U8)?;
        let mut brush = state.tools.brush_in(preset(state.preset)?, space);
        let tool = stroke_paint(tools::group(state.preset).tool(), &state.colors, space, &mut brush)?;
        let mut region = region_tools::RegionTools::default();
        for (id, &value) in &state.region_values { region.edit_value(id, value).map_err(|reason| reason.message(self.localization()))?; }
        region.source = state.region_sources;
        self.engine.set_brush(brush.clone()).map_err(error)?;
        self.engine.set_paint_color(state.colors.definition());
        self.state.brush = BrushState { preset: state.preset, tool: tools::group(state.preset).tool(),
            diameter: brush.diameter, opacity: brush.opacity, color: state.colors.preview(state.colors.definition()) };
        self.tool_origin = None;
        self.tools = state.tools;
        self.state.tool_slots = state.tool_slots;
        self.state.colors = state.colors;
        self.state.color_library = state.color_library;
        self.selection_masks.colors = state.mask_colors;
        self.layer_interaction.tool = if state.canvas_tool.picks_color() { LayerCanvasTool::Paint } else { state.canvas_tool };
        self.state.layer_tools.tool = self.layer_interaction.tool;
        self.layer_interaction.gradient = state.gradient;
        self.layer_interaction.figure = state.figure;
        self.selection_tools.options = state.selection;
        self.region_tools = region;
        self.engine.set_tool(tool);
        self.refresh_tools();
        self.refresh_commands();
        Ok(self.changed(regions::BRUSH | regions::DOCUMENT | regions::COMMANDS, true))
    }
    pub fn workspace_working_state(&self) -> WorkspaceWorkingState {
        WorkspaceWorkingState { version: 1,
            zen_mode: self.workspace_preview.as_ref().unwrap_or(&self.state.workspace).zen_mode }
    }

    /// Reset every preset override, including inactive tools.
    /// Keep the current color, selected tool, layout and document history.
    pub fn reset_brushes(&mut self) -> Result<UiChange, String> {
        self.require_workspace_idle()?;
        if self.workspace_preview.is_some() {
            return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_FINISH_PREVIEWING_THE_WORKSPACE_FIRST).to_string());
        }
        if self.tools.overrides.is_empty() {
            return Ok(UiChange::default());
        }
        let mut brush = tools::ToolMemory::default().brush_in(preset(self.state.brush.preset)?, self.engine.document().composition().color.space);
        self.state.colors.load_paint(&mut brush, self.engine.document().composition().color.space)?;
        self.engine.set_brush(brush.clone()).map_err(error)?;
        self.tools.overrides.clear();
        self.state.brush.diameter = brush.diameter;
        self.state.brush.opacity = brush.opacity;
        self.cursor.hover.reset();
        self.refresh_tools();
        self.refresh_commands();
        Ok(self.changed(regions::BRUSH | regions::COMMANDS, false))
    }

    /// Includes every accepted edit, even while the storage worker is busy.
    /// Captures use a committed gesture boundary; callers must not use disk alone.
    pub fn capture_workspace(&mut self) -> Result<WorkspaceCapture, String> {
        if self.workspace_history.gesture_start().is_some() {
            return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_FINISH_ARRANGING_THE_WORKSPACE_FIRST).to_string());
        }
        let mut committed = self
            .workspace_preview
            .as_ref()
            .unwrap_or(&self.state.workspace)
            .clone();
        self.state
            .customization
            .committed_header(&mut committed.layout);
        let history=self.workspace_history.capture(&committed);
        Ok(WorkspaceCapture {history,working:self.workspace_working_state()})
    }
    pub fn workspace_layout_generation(&self) -> Option<u64> {
        self.workspace_history.generation()
    }

    /// Layout browsing changes only the presented layout. Every durable
    /// capture still sees the layout from before the preview was opened.
    pub fn begin_workspace_layout_preview(&mut self) -> Result<(), String> {
        self.require_workspace_idle()?;
        if !self.workspace_transition || self.workspace_preview.is_some() {
            return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_LAYOUT_PREVIEW_IS_ALREADY_OPEN_OR_NOT_READY).to_string());
        }
        // A workspace/layout picker can open while the title editor is showing
        // an uncommitted preview. Its own baseline must already be durable:
        // applying a picker preview clears the title editor's transient state.
        let mut original = self.state.workspace.clone();
        self.state.customization.committed_header(&mut original.layout);
        self.workspace_preview = Some(original);
        Ok(())
    }
    pub fn preview_workspace_layout(&mut self, layout: &DockLayout) -> Result<UiChange, String> {
        if self.workspace_preview.is_none() {
            return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_OPEN_A_LAYOUT_PREVIEW_FIRST).to_string());
        }
        layout.validate()?;
        let insets = self.state.workspace.layout.titlebar_insets;
        let bottom_inset = self.state.workspace.layout.bottom_inset;
        let header_presentation = self.state.workspace.layout.header_presentation.clone();
        self.state.workspace.layout = durable_layout(layout);
        self.state.workspace.layout.open_default_columns(self.state.platform);
        self.state.workspace.zen_mode = false;
        self.state.workspace.layout.titlebar_insets = insets;
        self.state.workspace.layout.bottom_inset = bottom_inset;
        self.state.workspace.layout.header_presentation = header_presentation;
        self.state.customization = CustomizationState::default();
        self.sync_work_area();
        self.refresh_commands();
        Ok(self.changed(
            regions::LAYOUT | regions::CUSTOMIZATION | regions::COMMANDS,
            false,
        ))
    }
    pub fn cancel_workspace_layout_preview(&mut self) -> UiChange {
        if let Some(original) = self.workspace_preview.take() {
            let insets = self.state.workspace.layout.titlebar_insets;
            let bottom_inset = self.state.workspace.layout.bottom_inset;
            let header_presentation = self.state.workspace.layout.header_presentation.clone();
            self.state.workspace = original;
            self.state.workspace.layout.titlebar_insets = insets;
            self.state.workspace.layout.bottom_inset = bottom_inset;
            self.state.workspace.layout.header_presentation = header_presentation;
            self.state.customization = CustomizationState::default();
            self.sync_work_area();
            self.refresh_commands();
            return self.changed(
                regions::LAYOUT | regions::CUSTOMIZATION | regions::COMMANDS,
                false,
            );
        }
        UiChange::default()
    }

    pub fn adopt_workspace(&mut self, prepared: PreparedWorkspace) -> Result<UiChange, String> {
        self.require_workspace_idle()?;
        if self.workspace_preview.is_some() {
            return Err(self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_FINISH_PREVIEWING_THE_LAYOUT_FIRST).to_string());
        }
        let PreparedWorkspace { capture } = prepared;
        let working = capture.working;
        let titlebar_insets = self.state.workspace.layout.titlebar_insets;
        let bottom_inset = self.state.workspace.layout.bottom_inset;
        let header_presentation = self.state.workspace.layout.header_presentation.clone();
        self.state.toolbar_context_generation += 1;
        self.state.workspace = WorkspaceState {
            version: 2,
            layout: capture.history.layout().clone(),
            zen_mode: working.zen_mode,
        };
        self.state.workspace.layout.open_default_columns(self.state.platform);
        self.state.workspace.layout.titlebar_insets = titlebar_insets;
        self.state.workspace.layout.bottom_inset = bottom_inset;
        self.state.workspace.layout.header_presentation = header_presentation;
        self.workspace_history = workspace::WorkspaceHistory::restore(capture.history);
        self.tool_origin = None;
        self.state.customization = CustomizationState::default();
        self.eyedropper.cancel();
        self.eyedropper.picking = Default::default();
        self.state.color_picker.preview = None;
        self.cursor.hover.reset();
        self.interaction.keep_chrome_until_contact = working.zen_mode;
        self.refresh_tools();
        self.sync_work_area();
        self.refresh_commands();
        Ok(self.changed(
            regions::LAYOUT | regions::CUSTOMIZATION | regions::BRUSH | regions::COMMANDS,
            true,
        ))
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    /// Reveal existing panel placement, including a collapsed column, without
    /// changing the user's arrangement. Native views only forward the request.
    pub fn reveal_panel(&mut self, panel: Panel) -> Result<UiChange, String> {
        use crate::{CustomizationAction as Edit, DrawerAnchor, UiAction};
        let mut change = self.dispatch(UiAction::Customize {
            action: Edit::SetPanelVisible {
                panel,
                visible: true,
            },
        })?;
        let state = self.state();
        let layout = &state.workspace.layout;
        let group = layout
            .panel_group(panel)
            .ok_or_else(|| self.localization().text(MessageId::WORKSPACE_REFUSAL_SESSION_PANEL_HAS_NO_WORKSPACE_GROUP).to_string())?;
        let action = if let Some(column) = layout.collapsed_column_for_group(group) {
            let settings = layout.column_stack(column);
            let open = if settings.drawers {
                state.customization.column_drawers.iter().any(|d| matches!(
                    d.anchor, DrawerAnchor::Column { group: g, origin, .. } if g == group && origin == panel
                ))
            } else {
                settings.open_column == Some(column) && layout.active_panel(panel) == Some(panel)
            };
            (!open).then_some(UiAction::Customize {
                action: Edit::ToggleColumnDrawer { group, panel },
            })
        } else {
            (layout.active_panel(panel) != Some(panel))
                .then_some(UiAction::SelectPanelTab { group, panel })
        };
        if let Some(action) = action {
            let next = self.dispatch(action)?;
            change.revision = next.revision;
            change.regions |= next.regions;
            change.canvas_wake |= next.canvas_wake;
        }
        Ok(change)
    }
}
