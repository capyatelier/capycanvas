//! Validated workspace adoption, without replaying document/tool commands.
use super::*;

impl Default for WorkspaceWorkingState {
    fn default() -> Self {
        let region = region_tools::RegionTools::default();
        let layer = art_layers::LayerInteraction::default();
        Self {
            version: 1,
            preset: layer_core::DefaultBrushPreset::GPen as u32,
            tools: WorkspaceToolMemory::default(),
            colors: ColorState::default(),
            canvas_tool: LayerCanvasTool::Paint,
            selection: SelectionOptions::default(),
            region_values: region
                .controls()
                .into_iter()
                .map(|c| (c.id.into(), c.value))
                .collect(),
            region_sources: region.source,
            gradient: layer.gradient,
            figure: layer.figure,
            zen_mode: false,
        }
    }
}
impl WorkspaceCapture {
    pub fn from_legacy(workspace: WorkspaceState) -> Result<Self, String> {
        workspace.validate()?;
        Ok(Self {
            history: LayoutHistory::new(&workspace.layout),
            working: WorkspaceWorkingState {
                zen_mode: workspace.zen_mode,
                ..Default::default()
            },
        })
    }
    pub fn from_template(layout: &DockLayout) -> Result<Self, String> {
        layout.validate()?;
        Ok(Self {
            history: LayoutHistory::new(layout),
            working: WorkspaceWorkingState::default(),
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        PreparedWorkspace::new(self.clone()).map(|_| ())
    }
}

pub struct PreparedWorkspace {
    capture: WorkspaceCapture,
    region_tools: region_tools::RegionTools,
}
impl PreparedWorkspace {
    pub fn new(capture: WorkspaceCapture) -> Result<Self, String> {
        capture.history.validate()?;
        let state = &capture.working;
        if state.version != 1 {
            return Err("Unsupported workspace working-state version".into());
        }
        state.tools.validate()?;
        state.selection.validate()?;
        if let LayerCanvasTool::Selection { kind } = state.canvas_tool
            && !matches!(kind, SelectionTool::Rectangle | SelectionTool::Ellipse | SelectionTool::Polygon | SelectionTool::Brush) {
            return Err("Invalid geometric selection tool".into());
        }
        state.colors.validate()?;
        let mut brush = state.tools.brush(preset(state.preset)?);
        brush.color_rgba_linear = state.colors.definition().linear_in(layer_core::color::RgbSpace::Srgb)?;
        brush.validate().map_err(error)?;
        let mut region_tools = region_tools::RegionTools::default();
        for (id, &value) in &state.region_values {
            region_tools.edit(id, value)?;
        }
        region_tools.source = state.region_sources;
        Ok(Self {
            capture,
            region_tools,
        })
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub fn install_workspace_toolbar(
        &mut self,
        mut config: PanelConfig,
        replace: Option<Panel>,
        group: Option<u32>,
    ) -> Result<(Panel, UiChange), String> {
        self.require_workspace_idle()?;
        config.id = Panel::Toolbar;
        config.validate()?;
        let PanelContent::Toolbar { name, tiles } = &config.content else {
            return Err("Choose a toolbar".into());
        };
        let controls: Vec<_> = tiles.iter().map(|t| t.control).collect();
        let before = self.state.workspace.clone();
        let mut layout = before.layout.clone();
        let panel = if let Some(panel) = replace {
            layout.panel(panel)?;
            if panel.kind() != PanelKind::Tiles {
                return Err("Choose a toolbar to replace".into());
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
        let description = format!(
            "{} {}",
            if replace.is_some() {
                "Replaced"
            } else {
                "Added"
            },
            workspace::description::panel_name(&self.state.workspace.layout, panel)
        );
        self.workspace_history
            .record_named(before, &self.state.workspace, &description);
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
            return Err("A workspace change is already in progress".into());
        }
        self.workspace_transition = true;
        Ok(())
    }
    pub fn set_workspace_read_only(&mut self, read_only: bool) {
        self.workspace_read_only = read_only;
    }
    pub fn end_workspace_transition(&mut self) {
        self.workspace_transition = false;
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
            return Err("Finish the current interaction before switching workspaces".into());
        }
        Ok(())
    }

    pub fn workspace_working_state(&self) -> WorkspaceWorkingState {
        WorkspaceWorkingState {
            version: 1,
            preset: self.state.brush.preset,
            tools: self.tools.clone(),
            colors: self.state.colors.clone(),
            canvas_tool: self.eyedropper.picking.previous.unwrap_or(self.layer_interaction.tool),
            selection: self.selection_tools.options.clone(),
            region_values: self
                .region_tools
                .controls()
                .into_iter()
                .map(|c| (c.id.into(), c.value))
                .collect(),
            region_sources: self.region_tools.source,
            gradient: self.layer_interaction.gradient,
            figure: self.layer_interaction.figure,
            zen_mode: self
                .workspace_preview
                .as_ref()
                .unwrap_or(&self.state.workspace)
                .zen_mode,
        }
    }

    /// Reset every preset override in this workspace, including inactive tools.
    /// Keep the current color, selected tool, layout and document history.
    pub fn reset_workspace_brushes(&mut self) -> Result<UiChange, String> {
        self.require_workspace_idle()?;
        if self.workspace_preview.is_some() {
            return Err("Finish previewing the workspace first".into());
        }
        if self.tools.overrides.is_empty() {
            return Ok(UiChange::default());
        }
        let mut brush = tools::ToolMemory::default().brush_in(preset(self.state.brush.preset)?, self.engine.document().color.space);
        brush.color_rgba_linear = self.engine.configured_brush().color_rgba_linear;
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
            return Err("Finish arranging the workspace first".into());
        }
        let mut committed = self
            .workspace_preview
            .as_ref()
            .unwrap_or(&self.state.workspace)
            .clone();
        self.state
            .customization
            .committed_header(&mut committed.layout);
        Ok(WorkspaceCapture {
            history: self.workspace_history.capture(&committed),
            working: self.workspace_working_state(),
        })
    }
    pub fn workspace_layout_generation(&self) -> Option<u64> {
        self.workspace_history.generation()
    }

    /// Storage retention only removes unreachable history revisions. It is
    /// not workspace adoption: keep live panels, tools and UI models.
    pub fn refresh_workspace_history(&mut self, history: LayoutHistory) -> Result<(), String> {
        self.require_workspace_idle()?;
        history.validate()?;
        let current = self.capture_workspace()?.history;
        if history.current != current.current
            || history.undo != current.undo
            || history.redo != current.redo
            || std::iter::once(&current.current)
                .chain(&current.undo)
                .chain(&current.redo)
                .any(|id| history.revisions[id].layout != current.revisions[id].layout)
        {
            return Err("Storage maintenance changed the active layout or undo history".into());
        }
        self.workspace_history = workspace::WorkspaceHistory::restore(history);
        Ok(())
    }

    /// Layout browsing changes only the presented layout. Every durable
    /// capture still sees the layout from before the preview was opened.
    pub fn begin_workspace_layout_preview(&mut self) -> Result<(), String> {
        self.require_workspace_idle()?;
        if !self.workspace_transition || self.workspace_preview.is_some() {
            return Err("Layout preview is already open or not ready".into());
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
            return Err("Open a layout preview first".into());
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
            return Err("Finish previewing the layout first".into());
        }
        let PreparedWorkspace {
            capture,
            region_tools,
        } = prepared;
        let mut working = capture.working;
        if self.state.platform == Platform::Gtk {
            working.colors.library.ensure_starters();
        }
        let space = self.engine.document().color.space;
        working.colors.set_rgb_space(space)?;
        working.colors.set_document_depth(self.engine.document().color.depth)?;
        let mut brush = working.tools.brush_in(preset(working.preset)?, space);
        brush.color_rgba_linear = working.colors.definition().linear_in(space)?;
        // This is the only fallible mutation; CanvasEngine validates before setting.
        self.engine.set_brush(brush.clone()).map_err(error)?;
        self.engine.set_paint_color(working.colors.definition());
        let titlebar_insets = self.state.workspace.layout.titlebar_insets;
        let bottom_inset = self.state.workspace.layout.bottom_inset;
        let header_presentation = self.state.workspace.layout.header_presentation.clone();
        self.state.toolbar_context_generation += 1;
        self.state.workspace = WorkspaceState {
            version: 1,
            layout: capture.history.layout().clone(),
            zen_mode: working.zen_mode,
        };
        self.state.workspace.layout.open_default_columns(self.state.platform);
        self.state.workspace.layout.titlebar_insets = titlebar_insets;
        self.state.workspace.layout.bottom_inset = bottom_inset;
        self.state.workspace.layout.header_presentation = header_presentation;
        self.workspace_history = workspace::WorkspaceHistory::restore(capture.history);
        let before = self.state.workspace.clone();
        if self.state.workspace.layout.collapse_empty_toolbar_groups() {
            // Existing history revisions remain immutable. Record this upgrade
            // as one recoverable edit instead of rewriting saved snapshots.
            self.workspace_history.record_named(
                before,
                &self.state.workspace,
                "Remove empty toolbar groups",
            );
        }
        self.tools = working.tools;
        self.state.colors = working.colors;
        self.state.brush = BrushState {
            preset: working.preset,
            tool: tools::group(working.preset).tool(),
            diameter: brush.diameter,
            opacity: brush.opacity,
            color: self.state.colors.preview(self.state.colors.definition()),
        };
        let canvas_tool = if self.state.platform == Platform::Gtk && working.canvas_tool.picks_color() {
            LayerCanvasTool::Paint
        } else { working.canvas_tool };
        self.layer_interaction.tool = canvas_tool;
        self.layer_interaction.gradient = working.gradient;
        self.layer_interaction.figure = working.figure;
        self.state.layer_tools.tool = canvas_tool;
        self.region_tools = region_tools;
        self.selection_tools = selection_tools::SelectionTools::default();
        self.selection_tools.options = working.selection;
        self.engine.set_tool(
            if self.state.brush.tool == Tool::Eraser || self.state.colors.transparent() {
                StrokeTool::Eraser
            } else {
                StrokeTool::Brush
            },
        );
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

    /// Reset and Layout History restoration only replace layout. This is one new
    /// recoverable edit, preserving metadata, latest tool values, and Zen.
    pub fn restore_workspace_layout(
        &mut self,
        layout: DockLayout,
        description: &str,
    ) -> Result<UiChange, String> {
        self.require_workspace_idle()?;
        if self.workspace_preview.is_some() {
            return Err("Finish previewing the layout first".into());
        }
        layout.validate()?;
        let before = self.state.workspace.clone();
        self.state.workspace.layout = durable_layout(&layout);
        self.state.workspace.layout.open_default_columns(self.state.platform);
        self.state.workspace.layout.titlebar_insets = before.layout.titlebar_insets;
        self.state.workspace.layout.bottom_inset = before.layout.bottom_inset;
        self.state.workspace.layout.header_presentation = before.layout.header_presentation.clone();
        self.workspace_history
            .record_named(before, &self.state.workspace, description);
        self.state.customization = CustomizationState::default();
        self.sync_work_area();
        self.refresh_commands();
        Ok(self.changed(
            regions::LAYOUT | regions::CUSTOMIZATION | regions::COMMANDS,
            false,
        ))
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    /// Reveal existing panel placement, including a collapsed column, without
    /// changing the user's arrangement. Native views only forward the request.
    pub fn reveal_panel(&mut self, panel: Panel) -> Result<UiChange, String> {
        use crate::{CustomizationAction as Edit, DrawerAnchor, UiAction};
        if !panel.available_on(self.state.platform) {
            return Err("This panel is not available on this platform".into());
        }
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
            .ok_or("Panel has no workspace group")?;
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
