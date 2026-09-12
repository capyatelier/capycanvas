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
    brush: layer_core::BrushSnapshot,
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
        state.colors.validate()?;
        let mut brush = state.tools.brush(preset(state.preset)?);
        let rgba = state.colors.rgba();
        brush.color_rgba_linear = [
            srgb_to_linear(rgba[0]),
            srgb_to_linear(rgba[1]),
            srgb_to_linear(rgba[2]),
            rgba[3],
        ];
        brush.validate().map_err(error)?;
        let mut region_tools = region_tools::RegionTools::default();
        for (id, &value) in &state.region_values {
            region_tools.edit(id, value)?;
        }
        region_tools.source = state.region_sources;
        Ok(Self {
            capture,
            brush,
            region_tools,
        })
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub fn begin_workspace_transition(&mut self) -> Result<(), String> {
        self.require_workspace_idle()?;
        if self.workspace_transition {
            return Err("A workspace change is already in progress".into());
        }
        self.workspace_transition = true;
        Ok(())
    }
    pub fn end_workspace_transition(&mut self) {
        self.workspace_transition = false;
    }
    pub fn require_workspace_idle(&self) -> Result<(), String> {
        self.require_document_idle()?;
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
            canvas_tool: self.layer_interaction.tool,
            region_values: self
                .region_tools
                .controls()
                .into_iter()
                .map(|c| (c.id.into(), c.value))
                .collect(),
            region_sources: self.region_tools.source,
            gradient: self.layer_interaction.gradient,
            figure: self.layer_interaction.figure,
            zen_mode: self.state.workspace.zen_mode,
        }
    }

    /// Includes every accepted edit, even while the storage worker is busy.
    /// Captures use a committed gesture boundary; callers must not use disk alone.
    pub fn capture_workspace(&mut self) -> Result<WorkspaceCapture, String> {
        if self.workspace_history.gesture_start().is_some() {
            return Err("Finish arranging the workspace first".into());
        }
        Ok(WorkspaceCapture {
            history: self.workspace_history.capture(&self.state.workspace),
            working: self.workspace_working_state(),
        })
    }

    pub fn adopt_workspace(&mut self, prepared: PreparedWorkspace) -> Result<UiChange, String> {
        self.require_workspace_idle()?;
        let PreparedWorkspace {
            capture,
            brush,
            region_tools,
        } = prepared;
        // This is the only fallible mutation; CanvasEngine validates before setting.
        self.engine.set_brush(brush.clone()).map_err(error)?;
        let working = capture.working;
        self.state.workspace = WorkspaceState {
            version: 1,
            layout: capture.history.layout().clone(),
            zen_mode: working.zen_mode,
        };
        self.workspace_history = workspace::WorkspaceHistory::restore(capture.history);
        self.tools = working.tools;
        self.state.colors = working.colors;
        self.state.brush = BrushState {
            preset: working.preset,
            tool: tools::group(working.preset).tool(),
            diameter: brush.diameter,
            opacity: brush.opacity,
            color: self.state.colors.rgba(),
        };
        self.layer_interaction.tool = working.canvas_tool;
        self.layer_interaction.gradient = working.gradient;
        self.layer_interaction.figure = working.figure;
        self.state.layer_tools.tool = working.canvas_tool;
        self.region_tools = region_tools;
        self.engine.set_tool(
            if self.state.brush.tool == Tool::Eraser || self.state.colors.transparent() {
                StrokeTool::Eraser
            } else {
                StrokeTool::Brush
            },
        );
        self.state.customization = CustomizationState::default();
        self.eyedropper.cancel();
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
        layout.validate()?;
        let before = self.state.workspace.clone();
        self.state.workspace.layout = durable_layout(&layout);
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
