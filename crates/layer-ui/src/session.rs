use crate::interaction::{Interaction, PointerContact};
use crate::layout::{ResizeDrag, ResizeDragPhase};
use crate::*;
use layer_core::{DefaultBrushPreset, Document, LayerId, LayerKind, StrokeTool, default_brush};
use layer_engine::{CanvasEngine, InputProducer, PenEvent, PenPhase, PressureCurve, input_queue};
use layer_render::CanvasRenderer;
#[path = "art_layers.rs"]
mod art_layers;
#[path = "figures.rs"]
pub(crate) mod figures;
#[path = "operation.rs"]
pub(crate) mod operation;
#[path = "region_tools.rs"]
mod region_tools;
#[path = "rulers.rs"]
pub(crate) mod rulers;
pub use art_layers::{LayerAction, LayerCanvasTool, LayerControls, LayersView, RegionSource};
#[path = "application_menu.rs"]
mod application_menu;
#[path = "document_files.rs"]
mod document_files;
#[path = "workspace_session.rs"]
mod workspace_session;
pub use application_menu::{ApplicationLink, ApplicationMenu};
pub use workspace_session::PreparedWorkspace;
#[path = "effects.rs"]
mod effects;
#[path = "filter_loading.rs"]
mod filter_loading;
#[path = "project_files.rs"]
mod project_files;
pub use document_files::*;
pub use effects::{
    AdjustmentChoice, EffectAction, FilterCategoryChoice, FilterPickerAction, FilterPickerState,
    LayerPropertiesView, PropertyControl, PropertyKind,
};
pub use filter_loading::FilterLoadState;

const ZEN_CORNER_GUARD: f32 = 300.0;

#[derive(Clone, Copy)]
struct WorkspaceDrag {
    original: DockItem,
    item: DockItem,
    panel: Panel,
    source: Bounds,
    floating: Option<u32>,
    offset: [f32; 2],
    press: [f32; 2],
    chrome_revealed: bool,
    moved: bool,
    drawer: Option<DrawerAnchor>,
    position: [f32; 2],
    viewport: [f32; 2],
}
#[derive(Clone, Copy)]
struct FloatingResize {
    group: u32,
    edge: ResizeEdge,
    start: Bounds,
    press: [f32; 2],
}

/// A host-owned session: call inline or put the entire owner behind a host
/// worker's message boundary. It never creates threads or calls UI callbacks.
pub struct UiSession<R: CanvasRenderer> {
    engine: CanvasEngine<R>,
    state: UiState,
    pen: InputProducer<PenEvent>,
    input_pending: bool,
    touch: TouchGesture,
    navigator_drag: Option<[f32; 2]>,
    navigator_preview: crate::navigator::Preview,
    eyedropper: crate::eyedropper::Eyedropper,
    region_tools: region_tools::RegionTools,
    rulers: rulers::RulerInteraction,
    operation: operation::Operation,
    system_theme: Theme,
    logical_viewport: Option<[f32; 2]>,
    initial_fit: bool,
    divider_drag: Option<(u32, ResizeDrag)>,
    floating_resize: Option<FloatingResize>,
    workspace_drag: Option<WorkspaceDrag>,
    workspace_tab_drag: Option<crate::tab_drag::TabDrag>,
    workspace_drag_tabs: Vec<TabHit>,
    workspace_model_revision: u64,
    workspace_content_revision: u64,
    workspace_history: workspace::WorkspaceHistory,
    workspace_transition: bool,
    workspace_read_only: bool,
    workspace_preview: Option<WorkspaceState>,
    managed_workspace: Option<ManagedWorkspace>,
    interaction: Interaction,
    cursor: cursor::Cursor,
    next_request: u32,
    layer_interaction: art_layers::LayerInteraction,
    effect_catalog: layer_core::EffectCatalog,
    pending_filters: Option<filter_loading::Pending>,
    tools: tools::ToolMemory,
    files: document_files::DocumentFiles,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn blank(renderer: R, viewport: [u32; 2]) -> Result<Self, String> {
        let [width, height] = DEFAULT_DOCUMENT_EXTENT;
        Self::new(renderer, Document::new("untitled", width, height), viewport)
    }

    pub fn new(renderer: R, document: Document, viewport: [u32; 2]) -> Result<Self, String> {
        let camera = Camera::new([document.width, document.height], viewport);
        let (pen, input) = input_queue(8192);
        let mut engine = CanvasEngine::new(
            renderer,
            document,
            input,
            camera.view(),
            camera.input_transform(),
        )
        .map_err(|e| e.to_string())?;
        let brush = default_brush(DefaultBrushPreset::GPen);
        engine.set_brush(brush.clone()).map_err(error)?;
        let effect_catalog = layer_core::bundled_effect_catalog().clone();
        let mut session = Self {
            engine,
            pen,
            input_pending: false,
            touch: TouchGesture::default(),
            navigator_drag: None,
            navigator_preview: Default::default(),
            eyedropper: Default::default(),
            region_tools: Default::default(),
            rulers: Default::default(),
            operation: Default::default(),
            system_theme: Theme::Light,
            logical_viewport: None,
            initial_fit: true,
            divider_drag: None,
            floating_resize: None,
            workspace_drag: None,
            workspace_tab_drag: None,
            workspace_drag_tabs: Vec::new(),
            workspace_model_revision: 0,
            workspace_content_revision: 0,
            workspace_history: workspace::WorkspaceHistory::default(),
            workspace_transition: false,
            workspace_read_only: false,
            workspace_preview: None,
            managed_workspace: None,
            interaction: Interaction::default(),
            cursor: cursor::Cursor::default(),
            next_request: 1,
            layer_interaction: Default::default(),
            tools: tools::ToolMemory::default(),
            files: document_files::DocumentFiles::default(),
            state: UiState {
                revision: 0,
                fullscreen: false,
                workspace: WorkspaceState::default(),
                brush: BrushState {
                    preset: DefaultBrushPreset::GPen as u32,
                    tool: Tool::Pen,
                    diameter: brush.diameter,
                    opacity: brush.opacity,
                    color: [0.075, 0.075, 0.07, 1.0],
                },
                colors: ColorState::default(),
                tool_settings: Vec::new(),
                tool_actions: Vec::new(),
                tool_set: ToolSetView::default(),
                layers: Vec::new(),
                layer_tools: LayersView::default(),
                adjustments: effects::catalog(&effect_catalog, &Default::default()),
                filter_picker: Default::default(),
                filter_categories: effects::categories(&effect_catalog),
                filter_catalog_revision: 0,
                filter_load: FilterLoadState::default(),
                layer_properties: LayerPropertiesView::default(),
                tabs: Vec::new(),
                document_file: DocumentFileState::default(),
                commands: Vec::new(),
                settings: Settings::default(),
                theme: Theme::Light,
                palette: Settings::default().palette(Theme::Light, Platform::Generic),
                settings_open: false,
                preferences: PreferencesState::default(),
                customization: CustomizationState::default(),
                platform: Platform::Generic,
                requests: Vec::new(),
                host_error: None,
                camera,
            },
            effect_catalog,
            pending_filters: None,
        };
        session.apply_brush()?;
        session.refresh_document();
        session.refresh_commands();
        Ok(session)
    }

    pub fn state(&self) -> &UiState {
        &self.state
    }
    /// Only committed workspace topology is durable. A process interruption
    /// during a drag must restore its starting layout, not an intermediate tear-off.
    pub fn durable_workspace(&self) -> WorkspaceState {
        let mut workspace = self
            .workspace_history
            .gesture_start()
            .or(self.workspace_preview.as_ref())
            .unwrap_or(&self.state.workspace)
            .clone();
        workspace.layout.measurements.clear();
        workspace.layout.column_scroll.clear();
        workspace.layout.titlebar_insets = [0.0; 3];
        workspace.zen_mode = self
            .workspace_preview
            .as_ref()
            .unwrap_or(&self.state.workspace)
            .zen_mode;
        workspace
    }
    pub fn poll_navigator_preview(
        &mut self,
        now_ns: u64,
        visible: bool,
    ) -> Result<Option<layer_render::ReadbackImage>, String> {
        self.navigator_preview
            .poll(self.engine.backend_mut(), now_ns, visible)
    }
    /// UI hosts may retain their animation clock while live thumbnails change.
    /// Camera-only motion does not make the document overview animated.
    pub fn navigator_updates_continuously(&self) -> bool {
        self.input_pending || self.engine.has_active_stroke() || self.wants_continuous_frames()
    }
    /// Lets event-driven hosts sleep after the last image, including a final
    /// document update that arrived inside the preview throttle interval.
    pub fn navigator_preview_current(&self, revision: u64) -> bool {
        self.navigator_preview.is_current(revision)
    }
    pub fn set_platform(&mut self, platform: Platform) {
        self.state.platform = platform;
        self.state.palette = self.state.settings.palette(self.state.theme, platform);
        self.refresh_commands();
        self.refresh_shortcuts();
    }
    pub fn preferences(&self) -> Option<PreferencesView> {
        self.state.settings_open.then(|| {
            self.state
                .preferences
                .view(&self.state.settings, self.state.platform)
        })
    }
    pub fn context_menu(&self, target: ContextTarget) -> Result<ContextMenu, String> {
        let mut menu = match target {
            ContextTarget::ZenMode => self.state.settings.zen_menu(self.state.platform),
            _ => self
                .state
                .workspace
                .layout
                .context_menu_on(target, self.state.platform),
        }?;
        if self.managed_workspace.is_some() {
            fn route(items: &mut [Vec<ContextMenuItem>]) {
                for item in items.iter_mut().flatten() {
                    let command = match &item.action {
                        Some(UiAction::Customize {
                            action: CustomizationAction::NewToolbar { group },
                        }) => Some(WorkspaceCommand::NewToolbar { group: *group }),
                        Some(UiAction::Customize {
                            action: CustomizationAction::ManageToolbars,
                        }) => Some(WorkspaceCommand::ManageToolbars),
                        _ => None,
                    };
                    if let Some(command) = command {
                        item.action = Some(UiAction::WorkspaceManager { command });
                    }
                    route(&mut item.sections);
                }
            }
            route(&mut menu.sections);
            let panel = match target {
                ContextTarget::Panel { panel }
                | ContextTarget::Ribbon { panel }
                | ContextTarget::Tile { panel, .. } => Some(panel),
                ContextTarget::Group { group } => self
                    .state
                    .workspace
                    .layout
                    .group_panels(group)
                    .ok()
                    .filter(|p| p.len() == 1)
                    .map(|p| p[0]),
                _ => None,
            };
            // Web and Android retain their existing simple toolbar manager;
            // they do not expose a separate saved-toolbar library yet.
            if let Some(panel) = panel.filter(|p| {
                p.kind() == PanelKind::Tiles
                    && !matches!(self.state.platform, Platform::Web | Platform::Android)
            }) {
                menu.sections.push(vec![ContextMenuItem::command(
                    "Save to Toolbar Library…",
                    UiAction::WorkspaceManager {
                        command: WorkspaceCommand::SaveToolbar { panel },
                    },
                )]);
            }
        }
        Ok(menu.with_shortcuts(&self.state.settings, self.state.platform))
    }
    pub fn workspace_menu(&self) -> ContextMenu {
        let command = |id: CommandId| {
            let state = self.command(id);
            let mut item = ContextMenuItem::command(state.label, UiAction::Invoke { command: id });
            item.enabled = state.enabled;
            item.hint = state.shortcut;
            item
        };
        let mut menu = ContextMenu {
            title: WORKSPACE_MENU_LABEL.into(),
            sections: vec![
                vec![
                    command(CommandId::UndoWorkspace),
                    command(CommandId::RedoWorkspace),
                ],
                self.state.workspace.layout.panel_items(
                    PanelKind::Content,
                    None,
                    self.state.platform,
                ),
                self.state.workspace.layout.panel_items(
                    PanelKind::Tiles,
                    None,
                    self.state.platform,
                ),
                vec![
                    command(CommandId::NewToolbar),
                    command(CommandId::ManageToolbars),
                ],
            ],
        };
        if let Some(workspace) = &self.managed_workspace {
            let undo = menu.sections.remove(0);
            let mut panels = menu.sections.remove(0);
            let mut toolbars = menu.sections.remove(0);
            let toolbar_actions = menu.sections.remove(0);
            for item in panels.iter_mut().chain(toolbars.iter_mut()) {
                match item.action {
                    Some(UiAction::Customize {
                        action: CustomizationAction::SetPanelVisible { panel, .. },
                    }) => {
                        if let Ok(config) = self.state.workspace.layout.panel(panel) {
                            item.label = config.title().into();
                        }
                    }
                    Some(UiAction::Customize {
                        action: CustomizationAction::RestoreBuiltinToolbar { panel, .. },
                    }) => {
                        item.label = format!("Restore {}", panel.label());
                    }
                    _ => (),
                }
            }
            let workspaces = workspace.menu(
                self.require_workspace_idle().is_ok(),
                durable_layout(&self.state.workspace.layout) != workspace.baseline,
            );
            let toolbars =
                ContextMenuItem::submenu("Quick Access Toolbars", vec![toolbars, toolbar_actions]);
            menu.sections = vec![undo, vec![workspaces, toolbars], panels];
        }
        menu.with_shortcuts(&self.state.settings, self.state.platform)
    }
    pub fn toolbar_prompt(&self) -> Option<crate::customization::ToolbarPromptView> {
        self.state.customization.toolbar_prompt.as_ref().map(|p| {
            let mut view = p.view(
                &self.state.workspace.layout,
                &self.command(CommandId::UndoWorkspace).shortcut,
            );
            if view.destructive
                && let Some(workspace) = &self.managed_workspace
            {
                view.message.push_str(&format!(
                    " This removes the toolbar from {}.",
                    workspace.name
                ));
            }
            view
        })
    }
    pub fn toolbar_manager(&self) -> Option<crate::customization::ToolbarManagerView> {
        self.state
            .customization
            .toolbar_manager
            .as_ref()
            .map(|m| m.view(&self.state.workspace.layout))
    }
    pub fn panel_view(&self, panel: Panel) -> Result<PanelView, String> {
        customization::panel_view(&self.state, panel)
    }
    pub fn tool_picker(&self) -> Option<ToolPickerView> {
        self.state
            .customization
            .picker
            .as_ref()
            .map(|p| p.view(&self.state.workspace.layout, self.state.platform))
    }

    /// Hover is presentation input, separate from the paint queue and UI state.
    pub fn cursor_input(&mut self, event: Option<PenEvent>) {
        if event.is_some_and(|e| {
            ![
                e.surface_position.x,
                e.surface_position.y,
                e.pressure,
                e.tilt_radians[0],
                e.tilt_radians[1],
                e.twist_radians,
            ]
            .into_iter()
            .all(f32::is_finite)
        }) {
            return;
        }
        if self.cursor.event.is_none() {
            self.cursor.origin_ns = event.map_or(0, |e| e.timestamp_ns);
        }
        if event.is_none() {
            self.cursor.hover.reset();
        }
        self.cursor.event = event;
    }

    pub fn canvas_cursor(&mut self) -> Option<CanvasCursor> {
        let mut view = CanvasCursor::default();
        self.update_canvas_cursor(&mut view, true).then_some(view)
    }

    /// Refill a host-owned cursor buffer without discarding its capacity.
    /// Native presenters need segments only;
    /// SVG formatting is optional, without changing the shared outline geometry.
    pub fn update_canvas_cursor(&mut self, view: &mut CanvasCursor, svg: bool) -> bool {
        view.outline.clear();
        view.marker.clear();
        view.segments.clear();
        let Some(event) = self.cursor.event else {
            return false;
        };
        if self.interaction.pan_key.is_some()
            || self.layer_interaction.tool == LayerCanvasTool::Hand
            || self.interaction.pointer.is_some_and(|p| !p.paint)
            || self.interaction.facts.popup_open
            || self.state.settings_open
            || self.touch.is_active()
        {
            return false;
        }
        let scale = self
            .logical_viewport
            .map_or(1.0, |v| self.state.camera.viewport[0] as f32 / v[0]);
        let dabs = if self.layer_interaction.tool == LayerCanvasTool::Paint {
            self.engine
                .cursor_contacts(event, &mut self.cursor.hover, self.cursor.origin_ns)
        } else {
            Vec::new()
        };
        self.cursor.view(
            self.engine.backend(),
            &self.engine.brush().tip,
            &dabs,
            &self.state.camera,
            scale,
            if self.layer_interaction.tool == LayerCanvasTool::Paint {
                self.state.settings.cursor
            } else {
                CursorMode::Cross
            },
            view,
            svg,
        );
        true
    }
    fn switches_toolbar_drawer(&self, anchor: TileAnchor) -> bool {
        matches!(
            self.state.platform,
            Platform::Gtk | Platform::Android | Platform::Web
        ) && self
            .state
            .customization
            .drawer
            .as_ref()
            .and_then(|d| d.anchor.tile())
            .is_some_and(|old| old.panel == anchor.panel && old != anchor)
            && self
                .state
                .workspace
                .layout
                .panel(anchor.panel)
                .is_ok_and(|panel| {
                    panel.tiles().iter().any(|tile| {
                        tile.id == anchor.tile && tile.control.drawer_columns().is_some()
                    })
                })
    }

    /// Small event/reply boundary shared by native and Wasm hosts. Pen samples
    /// are only queued when `paint` is true, without serializing UiState.
    pub fn input(&mut self, input: UiInput) -> Result<InputReply, String> {
        let mut reply = InputReply {
            chrome_hidden: self.state.partial_zen(),
            hide_floating_panels: self.state.partial_zen(),
            keep_zen_button: !self.state.settings.total_zen,
            partial_zen: self.state.partial_zen(),
            ..Default::default()
        };
        let mut contact = None;
        if self.workspace_transition {
            reply.handled = true;
            return Ok(reply);
        }
        if self.workspace_read_only
            && matches!(
                &input,
                UiInput::Key { pressed: true, .. }
                    | UiInput::Pointer {
                        phase: ContactPhase::Down,
                        ..
                    }
                    | UiInput::Chrome {
                        event: ChromeEvent::Contact { .. },
                        ..
                    }
            )
        {
            reply.handled = true;
            return Ok(reply);
        }
        let mut released_chrome_pin = false;
        match input {
            UiInput::Chrome {
                event,
                facts,
                viewport,
            } => {
                valid_viewport(viewport)?;
                let position = match event {
                    ChromeEvent::Motion { position } | ChromeEvent::Contact { position, .. } => {
                        Some(position)
                    }
                    _ => None,
                };
                if position.is_some_and(|p| !p.into_iter().all(f32::is_finite)) {
                    return Err("Invalid chrome position".into());
                }
                let was_hidden = self.interaction.hidden;
                if matches!(event, ChromeEvent::Contact { .. })
                    || matches!(event, ChromeEvent::Leave { touch: false })
                    || position.is_some_and(|[x, y]| {
                        !(0.0..ZEN_CORNER_GUARD).contains(&x)
                            || !(0.0..ZEN_CORNER_GUARD).contains(&y)
                    })
                {
                    self.interaction.zen_entry_guard = false;
                }
                self.interaction.facts = facts;
                self.interaction.viewport = Some(viewport);
                if matches!(event, ChromeEvent::Contact { .. }) {
                    released_chrome_pin =
                        std::mem::take(&mut self.interaction.keep_chrome_until_contact);
                }
                if let ChromeEvent::Contact { position, .. } = event
                    && let Some(drawer) = &self.state.customization.drawer
                    && drawer.dismissal == DrawerDismissal::OutsideContact
                    && !facts.popup_open
                    && !facts
                        .content_drawer
                        .is_some_and(|b| b.contains(position[0], position[1]))
                    && !facts
                        .drawer_connection
                        .is_some_and(|b| b.contains(position[0], position[1]))
                    && !self
                        .state
                        .customization
                        .drawer_placement(
                            drawer,
                            &self.state.workspace.layout,
                            viewport,
                            &vec![0.0; drawer.columns.len()],
                            self.state.partial_zen(),
                        )
                        .is_some_and(|p| p.anchor.contains(position[0], position[1]))
                {
                    let tile = if self.state.partial_zen() {
                        self.state
                            .workspace
                            .layout
                            .zen_toolbars(viewport)
                            .tile_at(position)
                    } else {
                        self.layout(viewport)
                            .tile_at(&self.state.workspace.layout, position)
                            .or_else(|| self.state.customization.drawer_tile_at(position))
                    };
                    // Preserve the open drawer until an eligible button in its
                    // toolbar activates on release (or the contact is cancelled).
                    if !tile.is_some_and(|anchor| self.switches_toolbar_drawer(anchor)) {
                        reply.change = self.dispatch(UiAction::Customize {
                            action: CustomizationAction::CloseExpanded,
                        })?;
                        // Other tiles can still activate; bare canvas only dismisses.
                        reply.handled = tile.is_none();
                    }
                }
                if let ChromeEvent::Contact { position, .. } = event
                    && self.state.customization.expanded.is_some()
                    && !self.state.partial_zen()
                    && !facts.popup_open
                    && !facts.expanded_panel.is_some_and(|e| {
                        // Tabs activate on release. Leave the press available
                        // for native drag/hold recognition, even on the active tab.
                        let tab = facts.contact_tab.is_some_and(|tab| {
                            self.state.workspace.layout.panel_group(tab) == Some(e.group)
                        });
                        e.contains(position) && (!e.header_contains(position) || tab)
                    })
                {
                    reply.change = self.dispatch(UiAction::Customize {
                        action: CustomizationAction::CloseExpanded,
                    })?;
                    reply.handled = true;
                }
                match event {
                    ChromeEvent::Motion { position } | ChromeEvent::Contact { position, .. } => {
                        self.interaction.hover = Some(position);
                        self.interaction.keyboard_chrome = false;
                    }
                    ChromeEvent::Leave { touch: false } => self.interaction.hover = None,
                    _ => {}
                }
                if let ChromeEvent::Contact { canvas: true, .. } = event {
                    contact = Some((was_hidden, facts.popup_open));
                }
            }
            UiInput::Key {
                key,
                pressed,
                repeat,
                modifiers,
                editing,
                divider,
            } => {
                self.interaction.modifiers = modifiers;
                if key.eq_ignore_ascii_case("shift")
                    || key.eq_ignore_ascii_case("shift_l")
                    || key.eq_ignore_ascii_case("shift_r")
                {
                    self.interaction.modifiers.shift = pressed;
                }
                if key.eq_ignore_ascii_case("alt")
                    || key.eq_ignore_ascii_case("alt_l")
                    || key.eq_ignore_ascii_case("alt_r")
                {
                    self.interaction.modifiers.alt = pressed;
                }
                if (matches!(self.layer_interaction.tool, LayerCanvasTool::Figure { .. })
                    && !self.layer_interaction.path.is_empty())
                    || self.update_ruler_preview()
                    || self.update_transform_drag()?
                {
                    reply.change = self.changed(regions::DOCUMENT | regions::BRUSH, true);
                }
                // Native editors/IMEs own their text. Elsewhere in settings,
                // printable keys start search with the original case intact.
                if pressed
                    && self.state.settings_open
                    && !editing
                    && !modifiers.command
                    && !modifiers.alt
                    && self.state.preferences.capture.is_none()
                    && self.state.preferences.editing_shortcut.is_none()
                    && key.chars().count() == 1
                    && key.chars().all(|c| !c.is_control() && !c.is_whitespace())
                {
                    let query = format!("{}{key}", self.state.preferences.query);
                    reply.change = self.dispatch(UiAction::Preferences {
                        action: PreferenceAction::Search { query },
                    })?;
                    self.state.preferences.search_focus += 1;
                    reply.handled = true;
                    return Ok(reply);
                }
                let key = key.to_ascii_lowercase();
                if !pressed {
                    self.interaction.keys.remove(&key);
                    if self.interaction.pan_key.as_deref() == Some(&key) {
                        self.interaction.pan_key = None;
                    }
                } else {
                    let repeat = !self.interaction.keys.insert(key.clone()) || repeat;
                    if self.state.preferences.capture.is_some() {
                        if !repeat {
                            self.state.preferences.record(
                                &self.state.settings,
                                KeyChord::new(&key, modifiers),
                                self.state.platform,
                            );
                        }
                        reply.change = self.changed(regions::SETTINGS, false);
                        reply.handled = true;
                        reply.chrome_hidden = reply.hide_floating_panels;
                        return Ok(reply);
                    }
                    if key == "escape" {
                        self.interaction.keyboard_chrome = true;
                        reply.dismiss_popups = true;
                        if !editing && self.cancel_layer_gesture()? {
                            self.interaction.pointer = None;
                            reply.cancel_paint = true;
                            reply.change = self.changed(regions::DOCUMENT, true);
                            reply.handled = true;
                        }
                    }
                    if key == "escape"
                        && self.state.customization.has_drawer()
                        && !self.interaction.facts.popup_open
                    {
                        reply.change = self.dispatch(UiAction::Customize {
                            action: CustomizationAction::CloseExpanded,
                        })?;
                        reply.handled = true;
                    }
                    let blocked = editing
                        || self.state.settings_open
                        || self.state.customization.blocks_shortcuts()
                        || self.interaction.facts.popup_open;
                    if !blocked {
                        if let Some(id) = divider.filter(|_| {
                            matches!(
                                key.as_str(),
                                "arrowleft" | "arrowup" | "arrowright" | "arrowdown"
                            )
                        }) {
                            let viewport = self
                                .logical_viewport
                                .or(self.interaction.viewport)
                                .ok_or("Workspace has no viewport")?;
                            reply.change = self.dispatch(UiAction::NudgeDivider {
                                id,
                                forward: matches!(key.as_str(), "arrowright" | "arrowdown"),
                                viewport,
                            })?;
                            reply.handled = true;
                        } else if let Some(binding) = self
                            .state
                            .settings
                            .shortcut_match(&KeyChord::new(&key, modifiers), self.state.platform)
                        {
                            reply.handled = true;
                            if !repeat || binding.repeat {
                                match binding.action {
                                    ShortcutAction::Pan => self.interaction.pan_key = Some(key),
                                    ShortcutAction::Action { action } => {
                                        if !matches!(*action, UiAction::Invoke { command } if !self.command(command).enabled)
                                        {
                                            reply.change = self.dispatch(*action)?;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            UiInput::Pointer {
                id,
                phase,
                kind,
                button,
                position,
            } => {
                if !position.into_iter().all(f32::is_finite) {
                    return Err("Invalid pointer position".into());
                }
                if kind == PointerKind::Touch {
                    if self.interaction.pointer.is_none() && !self.state.settings_open {
                        reply.change = self.touch(id, pen_phase(phase), position);
                        reply.handled = true;
                    }
                } else {
                    let paint = button == PointerButton::Primary
                        && self.interaction.pan_key.is_none()
                        && self.layer_interaction.tool != LayerCanvasTool::Hand;
                    if phase == ContactPhase::Down
                        && self.interaction.pointer.is_none()
                        && !self.state.settings_open
                        && (paint || self.require_idle().is_ok())
                        && button != PointerButton::Other
                    {
                        self.touch.clear();
                        self.interaction.pointer = Some(PointerContact {
                            id,
                            paint,
                            position,
                        });
                    }
                    if let Some(contact) = self.interaction.pointer.filter(|p| p.id == id) {
                        reply.handled = true;
                        reply.paint = contact.paint;
                        if !contact.paint
                            && matches!(phase, ContactPhase::Move | ContactPhase::Up)
                            && position != contact.position
                        {
                            reply.change = self.gesture(contact.position, position, 1.0, 0.0)?;
                        }
                        self.interaction.pointer =
                            if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
                                None
                            } else {
                                Some(PointerContact {
                                    position,
                                    ..contact
                                })
                            };
                    }
                }
            }
            UiInput::Blur => {
                self.eyedropper.cancel();
                if self.cancel_layer_gesture()? {
                    reply.cancel_paint = true;
                    reply.change = self.changed(regions::DOCUMENT, true);
                }
                self.cursor_input(None);
                reply.cancel_paint |= self.interaction.pointer.take().is_some_and(|p| p.paint)
                    || self.input_pending
                    || self.engine.has_active_stroke();
                self.interaction.keys.clear();
                self.interaction.modifiers = Modifiers::default();
                self.interaction.pan_key = None;
                self.interaction.keyboard_chrome = false;
                self.interaction.facts.held = false;
                // A native DND grab can blur the window without ending the
                // drag. Only the host's drag-end/cancel lifecycle releases it.
                self.interaction.hover = None;
                let divider = self.divider_drag.take();
                let floating = self.floating_resize.take();
                let workspace = self.workspace_drag.take();
                self.workspace_tab_drag = None;
                if divider.is_some() || floating.is_some() || workspace.is_some() {
                    self.workspace_history.cancel(&mut self.state.workspace);
                    self.sync_work_area();
                    self.refresh_commands();
                    reply.change = self.changed(regions::LAYOUT | regions::COMMANDS, false);
                }
                self.touch.clear();
            }
        }
        self.refresh_chrome();
        if let Some((was_hidden, popup_open)) = contact {
            reply.dismiss_popups = popup_open;
            reply.handled |= popup_open
                || (was_hidden && !self.interaction.hidden)
                || (released_chrome_pin && self.interaction.hidden);
        }
        reply.chrome_hidden = self.interaction.hidden;
        reply.hide_floating_panels = self.state.partial_zen();
        reply.keep_zen_button = !self.state.settings.total_zen;
        reply.partial_zen = self.state.partial_zen();
        reply.pan_cursor = self.interaction.pan_key.is_some()
            || self.layer_interaction.tool == LayerCanvasTool::Hand;
        Ok(reply)
    }

    fn refresh_chrome(&mut self) {
        // Explicit exit only: proximity, first contact, keyboard chrome hints
        // and drag/popup pins must not reveal the editor in this mode.
        if self.state.partial_zen() {
            self.interaction.hidden = true;
            self.interaction.zen_entry_guard = false;
            self.interaction.keep_chrome_until_contact = false;
            self.interaction.keyboard_chrome = false;
            return;
        }
        if !self.state.workspace.zen_mode {
            self.interaction.keep_chrome_until_contact = false;
            self.interaction.zen_entry_guard = false;
        }
        // Hover/refresh and button release inside the activation corner must
        // not undo the user's explicit request to hide. A fresh contact or
        // pointer movement outside the fixed guard restores normal revealing.
        if self.interaction.zen_entry_guard {
            self.interaction.hidden = true;
            return;
        }
        let pinned = self.interaction.facts.held
            || self.interaction.facts.dragging
            || self.interaction.facts.popup_open
            || self.interaction.keyboard_chrome
            || self.interaction.keep_chrome_until_contact
            || self.state.settings_open
            || self.state.customization.is_open()
            || self.divider_drag.is_some()
            || self.floating_resize.is_some()
            || self.workspace_drag.is_some_and(|drag| drag.chrome_revealed);
        if !self.state.workspace.zen_mode || pinned {
            self.interaction.hidden = false;
        } else if self.workspace_drag.is_some() {
            // Moving a float is not a reveal gesture. Once an occupied screen
            // edge reveals docks, that visibility is latched for this drag.
            self.interaction.hidden = true;
        } else if self.interaction.pointer.is_none()
            && !self.input_pending
            && !self.engine.has_active_stroke()
        {
            let near = self
                .interaction
                .viewport
                .zip(self.interaction.hover)
                .is_some_and(|(viewport, position)| {
                    self.layout(viewport)
                        .near_chrome(position, viewport, self.interaction.hidden)
                });
            self.interaction.hidden = !near;
        }
    }
    /// Measure once in logical workspace units; all hosts use the same chrome
    /// insets, docking topology, and tile allocation rules.
    pub fn layout(&self, viewport: [f32; 2]) -> ResolvedLayout {
        self.state.workspace.layout.workspace(
            viewport[0],
            viewport[1],
            HEADER_HEIGHT,
            STATUS_HEIGHT,
        )
    }
    /// Layout-aware hosts retain controls at content_revision and apply these
    /// live dimensions on their display clock. Gesture completion remains full.
    pub fn workspace_layout_update(&self, viewport: [f32; 2]) -> crate::WorkspaceLayoutUpdate<'_> {
        let layout = &self.state.workspace.layout;
        crate::WorkspaceLayoutUpdate {
            workspace_update: self.workspace_update(),
            layout: self.layout(viewport),
            workspace_layout: crate::WorkspaceLayoutState {
                bands: &layout.bands,
                floating: &layout.floating,
                collapsed: &layout.collapsed,
                fit_tab_groups: &layout.fit_tab_groups,
            },
            camera: &self.state.camera,
            panel_measurements: &layout.measurements,
        }
    }
    /// Hosts can slide the pressed tab while the shared gesture still owns its
    /// source group. Once detached, the floating panel supplies the feedback.
    pub fn dragging_attached_tab(&self) -> bool {
        self.workspace_drag.is_some_and(|drag| {
            matches!(drag.original, DockItem::Panel { .. }) && drag.floating.is_none()
        })
    }

    /// Hosts supply the complete tab geometry captured on pointer down, once
    /// their drag recognizer starts the workspace gesture.
    pub fn begin_tab_drag(&mut self, tabs: &[TabHit], clip: Bounds) {
        self.workspace_tab_drag = (|| {
            let drag = self
                .workspace_drag
                .filter(|_| self.dragging_attached_tab())?;
            let group = self.state.workspace.layout.panel_group(drag.panel)?;
            let panels = self.state.workspace.layout.group_panels(group).ok()?;
            if tabs.iter().filter(|tab| tab.group == group).count() != panels.len() {
                return None;
            }
            let source = panels.iter().position(|p| *p == drag.panel)?;
            crate::tab_drag::TabDrag::new(group, source, drag.press, tabs, clip)
        })();
    }

    pub fn tab_drag_preview(&self, position: [f32; 2]) -> Option<crate::TabDragPreview> {
        if !self.dragging_attached_tab() {
            return None;
        }
        self.workspace_tab_drag.as_ref()?.preview(position)
    }

    pub fn workspace_model_revision(&self) -> u64 {
        self.workspace_model_revision
    }

    /// Revision of retained controls/resources, excluding eligible resize layouts.
    pub fn workspace_content_revision(&self) -> u64 {
        self.workspace_content_revision
    }

    /// One coherent presentation, usable directly by GTK, through Wasm, or via
    /// a native bridge. Geometry is in logical workspace units, never pixels.
    pub fn workspace_update(&self) -> WorkspaceUpdate {
        WorkspaceUpdate {
            revision: self.state.revision,
            model_revision: self.workspace_model_revision,
            content_revision: self.workspace_content_revision,
            drag: self.workspace_drag.map(|drag| WorkspaceDragPresentation {
                group: drag.floating.and_then(|id| {
                    self.layout(drag.viewport)
                        .groups
                        .into_iter()
                        .find(|g| g.id == id)
                        .map(|g| WorkspaceGroupPosition {
                            id,
                            bounds: g.bounds,
                        })
                }),
                tab: self.workspace_tab_drag.as_ref().and_then(|tab| {
                    self.tab_drag_preview(drag.position)
                        .map(|preview| WorkspaceTabPresentation {
                            group: tab.group,
                            panel: drag.panel,
                            clip: tab.clip,
                            source: tab.source_bounds(),
                            preview,
                        })
                }),
                drop_hint: self.drop_hint(
                    drag.viewport,
                    drag.position,
                    &self.workspace_drag_tabs,
                    drag.original,
                    None,
                ),
            }),
        }
    }

    fn drag_workspace(
        &mut self,
        item: DockItem,
        phase: ContactPhase,
        position: [f32; 2],
        viewport: [f32; 2],
        tabs: &[TabHit],
    ) -> Result<(), String> {
        valid_viewport(viewport)?;
        if !position.into_iter().all(f32::is_finite) {
            return Err("Invalid drag position".into());
        }
        if phase == ContactPhase::Cancel {
            if let Some(drag) = self.workspace_drag.filter(|d| d.original == item) {
                self.workspace_drag = None;
                self.workspace_tab_drag = None;
                self.workspace_history.cancel(&mut self.state.workspace);
                if let Some(DrawerAnchor::Column {
                    column,
                    group,
                    origin,
                }) = drag.drawer
                {
                    self.state.customization.column_drawers.retain(|d| {
                        !matches!(d.anchor, DrawerAnchor::Column { column: c, .. } if c == column)
                    });
                    self.state
                        .customization
                        .column_drawers
                        .push(ContentDrawer::for_column(
                            &self.state.workspace.layout,
                            group,
                            origin,
                        )?);
                }
                self.interaction.keep_chrome_until_contact = false;
            }
            return Ok(());
        }
        self.workspace_drag_tabs.clear();
        self.workspace_drag_tabs.extend_from_slice(tabs);
        self.interaction.hover = Some(position);
        self.interaction.viewport = Some(viewport);
        self.interaction.zen_entry_guard = false;
        if phase == ContactPhase::Down {
            self.workspace_tab_drag = None;
            let mut layout = self.layout(viewport);
            layout.groups.extend(
                self.state
                    .customization
                    .column_drawer_groups(&self.state.workspace.layout),
            );
            if let DockItem::Column { column } = item {
                let source = layout
                    .collapsed
                    .iter()
                    .find(|c| c.id == column)
                    .ok_or("Unknown collapsed column")?;
                self.workspace_history.begin_named(
                    &self.state.workspace,
                    format!(
                        "Moved {}",
                        workspace::description::item_name(&self.state.workspace.layout, item)
                    ),
                );
                self.workspace_drag = Some(WorkspaceDrag {
                    original: item,
                    item,
                    panel: source.groups[0].active,
                    source: source.bounds,
                    floating: None,
                    offset: [0.; 2],
                    press: position,
                    chrome_revealed: true,
                    moved: false,
                    drawer: None,
                    position,
                    viewport,
                });
                self.state.customization = CustomizationState::default();
                return Ok(());
            }
            // Collapsed icon tiles are panel sources too. Keep their small
            // pickup bounds so tear-off starts from the icon, without opening
            // a drawer or expanding the column as a separate history action.
            let icon_source = if let DockItem::Panel { panel } = item {
                layout
                    .collapsed
                    .iter()
                    .flat_map(|c| &c.groups)
                    .find_map(|g| {
                        g.icons
                            .iter()
                            .find(|i| {
                                i.panel == panel && i.bounds.contains(position[0], position[1])
                            })
                            .map(|i| (g.group, g.icons.len(), g.active, i.bounds, false))
                    })
            } else {
                None
            };
            let (source_id, source_count, source_active, source_bounds, source_floating) =
                icon_source
                    .or_else(|| {
                        let source = match item {
                            DockItem::Group { group } => {
                                layout.groups.iter().find(|g| g.id == group)
                            }
                            DockItem::Panel { panel } => {
                                layout.groups.iter().find(|g| g.panels.contains(&panel))
                            }
                            _ => None,
                        }?;
                        Some((
                            source.id,
                            source.panels.len(),
                            source.active,
                            source.bounds,
                            source.floating,
                        ))
                    })
                    .ok_or("Unknown drag source")?;
            let normalized = if source_count == 1 {
                DockItem::Group { group: source_id }
            } else {
                item
            };
            let whole = matches!(normalized, DockItem::Group { .. });
            self.workspace_history.begin_named(
                &self.state.workspace,
                format!(
                    "Moved {}",
                    workspace::description::item_name(&self.state.workspace.layout, item)
                ),
            );
            self.workspace_drag = Some(WorkspaceDrag {
                position,
                viewport,
                original: item,
                item: normalized,
                panel: match item {
                    DockItem::Panel { panel } => panel,
                    _ => source_active,
                },
                source: source_bounds,
                floating: (whole && source_floating).then_some(source_id),
                offset: [position[0] - source_bounds.x, position[1] - source_bounds.y],
                press: position,
                chrome_revealed: !source_floating || !self.interaction.hidden,
                moved: false,
                drawer: self
                    .state
                    .customization
                    .column_drawers
                    .iter()
                    .find_map(|d| {
                        matches!(d.anchor, DrawerAnchor::Column { group, .. } if group == source_id)
                            .then_some(d.anchor)
                    }),
            });
            if whole && source_floating {
                let floats = &mut self.state.workspace.layout.floating;
                let index = floats
                    .iter()
                    .position(|f| f.root.id() == source_id)
                    .unwrap();
                let floating = floats.remove(index);
                floats.push(floating);
            }
            // Measured column drawers are dock projections: keep them available
            // for drops until a move removes their source from the column. Hosts
            // without drawer drag support retain their existing dismissal behavior.
            let mut column_drawers = std::mem::take(&mut self.state.customization.column_drawers);
            column_drawers.retain(|d| {
                d.tabs.as_ref().is_some_and(|tabs| {
                    self.state
                        .customization
                        .column_drawer_bounds
                        .iter()
                        .any(|m| m.group == tabs.group)
                })
            });
            self.state.customization = CustomizationState {
                column_drawers,
                column_drawer_bounds: std::mem::take(
                    &mut self.state.customization.column_drawer_bounds,
                ),
                drawer_tiles: std::mem::take(&mut self.state.customization.drawer_tiles),
                ..CustomizationState::default()
            };
            return Ok(());
        }
        let mut drag = self
            .workspace_drag
            .filter(|d| d.original == item)
            .ok_or("Workspace drag is not active")?;
        drag.position = position;
        drag.viewport = viewport;
        drag.chrome_revealed |= self.layout(viewport).near_chrome(position, viewport, true);
        drag.moved |= position != drag.press;
        if drag.floating.is_none()
            && !matches!(drag.item, DockItem::Column { .. })
            && drag.source.distance_to(position) > crate::layout::WORKSPACE_PROXIMITY
        {
            self.state.workspace.layout.move_item(
                viewport,
                drag.item,
                DockTarget::Float { position },
            )?;
            let group = self.state.workspace.layout.panel_group(drag.panel).unwrap();
            let floated = self
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap();
            drag.floating = Some(group);
            drag.item = DockItem::Group { group };
            // A ribbon becomes a compact vertical grid, with its grip at the
            // bottom. Tabbed groups keep the original header grab offset.
            drag.offset = if floated.tabs_visible {
                [
                    self.workspace_tab_drag
                        .as_ref()
                        .map_or(drag.offset[0], |tab| tab.grab_offset_x())
                        .min(floated.bounds.width - 10.0)
                        .max(0.0),
                    drag.offset[1].min(TAB_BAR_HEIGHT),
                ]
            } else {
                [floated.bounds.width * 0.5, floated.bounds.height - 10.0]
            };
        }
        if let Some(group) = drag.floating {
            self.state.workspace.layout.move_floating(
                group,
                [position[0] - drag.offset[0], position[1] - drag.offset[1]],
                viewport,
            )?;
        }
        self.workspace_drag = Some(drag);
        if phase == ContactPhase::Up {
            if drag.moved
                && let Some(hint) = self.drop_hint(viewport, position, tabs, item, None)
            {
                self.state
                    .workspace
                    .layout
                    .move_item(viewport, drag.item, hint.target)?;
            }
            self.workspace_drag = None;
            self.workspace_tab_drag = None;
            self.workspace_history
                .finish_move(&mut self.state.workspace);
            self.interaction.keep_chrome_until_contact = false;
        }
        Ok(())
    }

    /// A preview is offered only when the same transactional move will succeed.
    /// Measured native tab rectangles are input, not host-side docking policy.
    pub fn drop_hint(
        &self,
        viewport: [f32; 2],
        position: [f32; 2],
        tabs: &[TabHit],
        item: DockItem,
        expansion: Option<PanelExpansion>,
    ) -> Option<DropHint> {
        let item = self
            .workspace_drag
            .filter(|d| d.original == item)
            .map_or(item, |d| d.item);
        if !viewport.into_iter().all(|v| v.is_finite() && v > 0.0)
            || !position.into_iter().all(f32::is_finite)
            || !(Bounds {
                x: 0.0,
                y: 0.0,
                width: viewport[0],
                height: viewport[1],
            })
            .contains(position[0], position[1])
        {
            return None;
        }
        if self.state.partial_zen() {
            return None;
        }
        let mut resolved = self.layout(viewport);
        resolved.groups.extend(
            self.state
                .customization
                .column_drawer_groups(&self.state.workspace.layout),
        );
        let docks_hidden = self.state.workspace.zen_mode
            && self
                .workspace_drag
                .map_or(self.interaction.hidden, |drag| !drag.chrome_revealed);
        if docks_hidden {
            resolved.groups.retain(|g| g.floating);
            resolved.dividers.clear();
        }
        if let Some(expansion) = expansion
            && expansion.bounds.contains(position[0], position[1])
        {
            let mut preview = expansion.preview;
            preview.x += expansion.bounds.x;
            preview.y += expansion.bounds.y;
            // The configuration column is not a docking destination.
            if !preview.contains(position[0], position[1]) {
                return None;
            }
            let index = resolved
                .groups
                .iter()
                .position(|g| g.id == expansion.group)?;
            let mut group = resolved.groups.remove(index);
            group.bounds = preview;
            if group.tiles.is_some() {
                group.tiles = Some(toolbar_tile_layout(
                    preview.width,
                    preview.height
                        - if group.tabs_visible {
                            TAB_BAR_HEIGHT
                        } else {
                            0.0
                        },
                    group.axis,
                    self.state
                        .workspace
                        .layout
                        .panel(group.active)
                        .ok()?
                        .tiles(),
                    !group.tabs_visible,
                    self.state
                        .workspace
                        .layout
                        .panel(group.active)
                        .ok()?
                        .tile_style,
                ));
            }
            resolved.groups.push(group);
        }
        // Moving a complete floating group must not target its own tab area.
        let source_group = match item {
            DockItem::Group { group } => Some(group),
            DockItem::Panel { panel } => {
                self.state.workspace.layout.panel_group(panel).filter(|g| {
                    self.state
                        .workspace
                        .layout
                        .group_panels(*g)
                        .is_ok_and(|p| p.len() == 1)
                })
            }
            _ => None,
        };
        if source_group.is_some() {
            resolved.groups.retain(|g| Some(g.id) != source_group);
        }
        let mut hint = if let DockItem::Column { column } = item {
            if docks_hidden {
                return None;
            }
            self.state
                .workspace
                .layout
                .column_drop_hint(&resolved, column, position)?
        } else if let DockItem::Tile { panel, tile } = item {
            let layout = &self.state.workspace.layout;
            let source = layout
                .panel(panel)
                .ok()?
                .tiles()
                .iter()
                .find(|t| t.id == tile)?;
            // Separators themselves still reorder as ordinary toolbar tiles.
            let group_hint = (source.control != crate::ToolbarControl::Divider)
                .then(|| resolved.tile_group_drop_hint(position, layout))
                .flatten();
            group_hint.or_else(|| resolved.tile_drop_hint(position, layout))?
        } else {
            resolved.drop_hint(position[0], position[1], tabs, !docks_hidden)?
        };
        // The attached preview and the committed drop use the same frozen
        // switch points, including a release between pointer-motion events.
        if let DockTarget::Tab {
            group,
            index: Some(_),
        } = hint.target
            && let Some(drag) = self.workspace_tab_drag.as_ref()
            && drag.group == group
            && let Some(preview) = self.tab_drag_preview(position)
        {
            hint.target = DockTarget::Tab {
                group,
                index: Some(preview.insertion),
            };
            hint.bounds = crate::layout::tab_insertion_line(
                resolved.groups.iter().find(|g| g.id == group)?,
                tabs,
                preview.insertion,
            );
        }
        let mut probe = self.state.workspace.layout.clone();
        probe.move_item(viewport, item, hint.target.clone()).ok()?;
        (probe != self.state.workspace.layout).then_some(hint)
    }
    pub fn engine(&self) -> &CanvasEngine<R> {
        &self.engine
    }
    /// Surface lifecycle/presentation only; UI document edits use dispatch.
    pub fn renderer_mut(&mut self) -> &mut R {
        self.engine.backend_mut()
    }
    pub fn renderer_stats(&self) -> crate::StatsView {
        crate::stats::view(self.engine.backend().telemetry())
    }
    pub fn command(&self, id: CommandId) -> CommandState {
        let (enabled, selected) = self.command_flags(id);
        let label = if id == CommandId::ResetLayout && self.managed_workspace.is_some() {
            "Restore Starting Layout…"
        } else {
            id.label()
        };
        CommandState {
            checkable: id.is_toggle(),
            icon: self.command_icon(id),
            id,
            label,
            tooltip: self.state.settings.action_tooltip(
                label,
                &UiAction::Invoke { command: id },
                self.state.platform,
            ),
            enabled,
            selected,
            bindings: self.state.settings.command_keys(id),
            shortcut: self
                .state
                .settings
                .action_shortcut(&UiAction::Invoke { command: id }, self.state.platform),
        }
    }
    fn command_icon(&self, id: CommandId) -> Option<&'static str> {
        if id == CommandId::ZenMode {
            Some(self.state.settings.zen_icon.icon())
        } else if id == CommandId::Fullscreen && self.state.fullscreen {
            Some("fullscreen-exit")
        } else {
            id.icon()
        }
    }
    fn command_flags(&self, id: CommandId) -> (bool, bool) {
        if !id.available_on(self.state.platform) || self.state.document_file.close_ready {
            return (false, false);
        }
        let document = self.engine.document();
        let index = document
            .layers
            .iter()
            .position(|layer| layer.id == document.active_layer)
            .unwrap_or(0);
        let editable = document
            .layers
            .get(index)
            .is_some_and(|layer| layer.kind == LayerKind::Paint);
        let idle = self.require_idle().is_ok();
        let paint_layers = document
            .layers
            .iter()
            .filter(|layer| layer.kind == LayerKind::Paint)
            .count();
        let enabled = match id {
            CommandId::ResetLayout if self.managed_workspace.is_some() => {
                self.require_workspace_idle().is_ok()
                    && self
                        .managed_workspace
                        .as_ref()
                        .is_some_and(|w| durable_layout(&self.state.workspace.layout) != w.baseline)
            }
            CommandId::NewDocument
            | CommandId::OpenDocument
            | CommandId::SaveDocument
            | CommandId::SaveDocumentAs
            | CommandId::ExportDocument => {
                self.require_document_idle().is_ok() && !self.state.document_file.busy
            }
            CommandId::CloseDocument => self.require_document_idle().is_ok(),
            CommandId::ScaleRotate => idle && self.can_transform(),
            CommandId::ApplyTransform | CommandId::CancelTransform | CommandId::TransformAspect => {
                idle && self.operation.active()
            }
            CommandId::ShowRulers => idle,
            CommandId::SnapRulers => idle && self.rulers.visible,
            CommandId::DeleteRuler => {
                idle && self
                    .rulers
                    .selected
                    .is_some_and(|id| document.rulers.iter().any(|r| r.id == id))
            }
            CommandId::Undo => idle && self.engine.can_undo(),
            CommandId::Redo => idle && self.engine.can_redo(),
            CommandId::SelectAll => self.require_document_idle().is_ok(),
            CommandId::Deselect | CommandId::InvertSelection => {
                self.require_document_idle().is_ok() && document.selection.is_some()
            }
            CommandId::ClearLayer | CommandId::FillSelection => {
                self.require_document_idle().is_ok()
                    && editable
                    && !document.active_mask
                    && !document.is_locked(document.active_layer)
                    && (id == CommandId::ClearLayer || document.selection.is_some())
            }
            CommandId::UndoWorkspace => self.workspace_history.can_undo(),
            CommandId::RedoWorkspace => self.workspace_history.can_redo(),
            CommandId::AddLayer => idle,
            CommandId::DeleteLayer => idle && editable && paint_layers > 1,
            CommandId::RaiseLayer => idle && editable && index > 0,
            CommandId::LowerLayer => {
                idle && editable
                    && document
                        .layers
                        .get(index + 1)
                        .is_some_and(|layer| layer.kind == LayerKind::Paint)
            }
            CommandId::FitCanvas
            | CommandId::Hand
            | CommandId::Eyedropper
            | CommandId::Gradient
            | CommandId::Figure
            | CommandId::Ruler
            | CommandId::AutoSelect
            | CommandId::Fill
            | CommandId::RotateLeft
            | CommandId::RotateRight
            | CommandId::FlipHorizontal
            | CommandId::FlipVertical => idle,
            CommandId::ZoomIn => idle && self.state.camera.zoom < 16.0,
            CommandId::ZoomOut => idle && self.state.camera.zoom > 0.02,
            _ => true,
        };
        let selected = (self.layer_interaction.tool == LayerCanvasTool::Paint
            && id.paint_tool() == Some(self.state.brush.tool))
            || matches!(
                (id, self.layer_interaction.tool),
                (CommandId::Lasso, LayerCanvasTool::Select)
                    | (
                        CommandId::Move,
                        LayerCanvasTool::Move | LayerCanvasTool::Transform
                    )
                    | (CommandId::Hand, LayerCanvasTool::Hand)
                    | (CommandId::Gradient, LayerCanvasTool::Gradient { .. })
                    | (CommandId::Figure, LayerCanvasTool::Figure { .. })
                    | (CommandId::Ruler, LayerCanvasTool::Ruler { .. })
                    | (
                        CommandId::AutoSelect,
                        LayerCanvasTool::Region { fill: false, .. }
                    )
                    | (CommandId::Fill, LayerCanvasTool::Region { fill: true, .. })
                    | (
                        CommandId::Eyedropper,
                        LayerCanvasTool::PickVisible | LayerCanvasTool::PickLayer
                    )
            )
            || (id == CommandId::ZenMode && self.state.workspace.zen_mode)
            || (id == CommandId::Fullscreen && self.state.fullscreen)
            || (id == CommandId::ShowRulers && self.rulers.visible)
            || (id == CommandId::SnapRulers && self.rulers.snapping)
            || (id == CommandId::TransformAspect && self.operation.aspect)
            || (id == CommandId::FlipHorizontal && self.state.camera.flipped[0])
            || (id == CommandId::FlipVertical && self.state.camera.flipped[1])
            || (id == CommandId::ToggleTheme
                && self.state.settings.theme.unwrap_or(self.system_theme) == Theme::Dark);
        (enabled, selected)
    }

    pub fn dispatch(&mut self, action: UiAction) -> Result<UiChange, String> {
        if self.workspace_read_only
            && !matches!(
                &action,
                UiAction::WorkspaceManager { .. }
                    | UiAction::CompleteRequest { .. }
                    | UiAction::CloseSettings
                    | UiAction::RestoreSettings { .. }
                    | UiAction::MeasureColumnDrawers { .. }
                    | UiAction::MeasureDrawerTiles { .. }
                    | UiAction::MeasureColumnScroll { .. }
                    | UiAction::MeasurePanels { .. }
                    | UiAction::MeasureTitlebar { .. }
                    | UiAction::SystemThemeChanged { .. }
                    | UiAction::WindowFullscreen { .. }
                    | UiAction::Invoke {
                        command: CommandId::ApplyTransform | CommandId::CancelTransform
                    }
                    | UiAction::DragWorkspace {
                        phase: ContactPhase::Up | ContactPhase::Cancel,
                        ..
                    }
                    | UiAction::DragDivider {
                        phase: ContactPhase::Up | ContactPhase::Cancel,
                        ..
                    }
                    | UiAction::ResizeFloating {
                        phase: ContactPhase::Up | ContactPhase::Cancel,
                        ..
                    }
            )
        {
            return Err(
                "Workspace ownership needs recovery. Use Save as New Workspace or export a backup."
                    .into(),
            );
        }
        if self.workspace_transition
            && !matches!(
                &action,
                UiAction::CompleteRequest { .. }
                    | UiAction::CloseSettings
                    | UiAction::RestoreSettings { .. }
                    | UiAction::MeasureColumnDrawers { .. }
                    | UiAction::MeasureDrawerTiles { .. }
                    | UiAction::MeasureColumnScroll { .. }
                    | UiAction::SystemThemeChanged { .. }
                    | UiAction::MeasurePanels { .. }
                    | UiAction::MeasureTitlebar { .. }
                    | UiAction::WindowFullscreen { .. }
            )
        {
            return Err("A workspace change is in progress".into());
        }
        use regions::*;
        let model_revision = self.workspace_model_revision;
        let content_revision = self.workspace_content_revision;
        // Only a dimension/measurement change with no open customization UI can
        // retain content. Collapse/expand transitions still require full models.
        let layout_only = !self.state.customization.is_open()
            && !self.state.partial_zen()
            && matches!(
                &action,
                UiAction::DragDivider {
                    phase: ContactPhase::Move,
                    ..
                } | UiAction::ResizeFloating {
                    phase: ContactPhase::Move,
                    ..
                } | UiAction::MeasurePanels { .. }
            );
        let collapsed_before = layout_only.then(|| self.state.workspace.layout.collapsed.clone());
        let drag_before = self.workspace_drag;
        let moving_workspace = matches!(
            &action,
            UiAction::DragWorkspace {
                phase: ContactPhase::Move,
                ..
            }
        );
        let revision = self.engine.document().revision;
        let transforming = self.operation.active();
        let was_expanded = self.state.customization.has_drawer();
        let was_zen = self.state.workspace.zen_mode;
        let tool_before = (self.state.brush.tool, self.layer_interaction.tool);
        let explicit_color = matches!(action, UiAction::Color { .. } | UiAction::SetColor { .. });
        let workspace_before = matches!(
            &action,
            UiAction::Customize { .. }
                | UiAction::MovePanel { .. }
                | UiAction::MoveGroup { .. }
                | UiAction::MoveColumn { .. }
                | UiAction::MoveTile { .. }
                | UiAction::SelectPanelTab { .. }
                | UiAction::ResizeDock { .. }
                | UiAction::DoubleClickPanelHandle { .. }
                | UiAction::ResetColumnWidth { .. }
                | UiAction::NudgeDivider { .. }
                | UiAction::PrioritizeBand { .. }
                | UiAction::Invoke {
                    command: CommandId::ResetLayout | CommandId::ZenMode | CommandId::NewToolbar
                }
        )
        .then(|| self.state.workspace.clone());
        let move_item = match &action {
            UiAction::MovePanel { panel, .. } => Some(DockItem::Panel { panel: *panel }),
            UiAction::MoveGroup { group, .. } => Some(DockItem::Group { group: *group }),
            UiAction::MoveColumn { column, .. } => Some(DockItem::Column { column: *column }),
            _ => None,
        };
        let workspace_description = move_item.map(|item| {
            format!(
                "Moved {}",
                workspace::description::item_name(&self.state.workspace.layout, item)
            )
        });
        let mut save_settings = matches!(
            &action,
            UiAction::SetTheme { .. }
                | UiAction::Invoke {
                    command: CommandId::ToggleTheme
                }
        );
        let (mut changed, wake) = match action {
            UiAction::WorkspaceManager { command } => {
                if self.managed_workspace.is_none() {
                    return Err("Workspace management is not connected".into());
                }
                if let WorkspaceCommand::Switch { id } = &command
                    && self.managed_workspace.as_ref().is_some_and(|w| &w.id == id)
                {
                    return Ok(UiChange::default());
                }
                self.request(HostRequestKind::Workspace { command })?;
                (HOST, false)
            }
            UiAction::BeginTabDrag { tabs, clip } => {
                self.begin_tab_drag(&tabs, clip);
                return Ok(self.changed(0, false));
            }
            UiAction::MeasureColumnDrawers { measurements } => {
                self.state
                    .customization
                    .measure_column_drawers(measurements)?;
                return Ok(UiChange::default());
            }
            UiAction::MeasureDrawerTiles { measurements } => {
                self.state
                    .customization
                    .measure_drawer_tiles(&self.state.workspace.layout, measurements)?;
                // Allocation facts do not change workspace history or rebuild
                // widgets. The host can position the child in this same frame.
                return Ok(UiChange::default());
            }
            UiAction::MeasureColumnScroll { column, offset } => {
                if !offset.is_finite()
                    || offset < 0.
                    || !self.state.workspace.layout.is_collapsed(column)
                {
                    return Err("Invalid column scroll".into());
                }
                let scroll = &mut self.state.workspace.layout.column_scroll;
                if scroll
                    .iter()
                    .find(|(id, _)| *id == column)
                    .map_or(0., |(_, v)| *v)
                    == offset
                {
                    return Ok(self.changed(0, false));
                }
                if let Some((_, v)) = scroll.iter_mut().find(|(id, _)| *id == column) {
                    *v = offset;
                } else {
                    scroll.push((column, offset));
                }
                (LAYOUT, false)
            }
            UiAction::Navigator {
                phase,
                position,
                viewport,
            } => {
                if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
                    self.navigator_drag = None;
                    (0, false)
                } else {
                    self.require_idle()?;
                    if !position.into_iter().all(f32::is_finite) {
                        return Err("Invalid Navigator position".into());
                    }
                    let doc = self.engine.document();
                    let camera = &mut self.state.camera;
                    let geometry =
                        NavigatorGeometry::new(camera, [doc.width, doc.height], viewport)
                            .ok_or("Invalid Navigator size")?;
                    let point = geometry.document_point(position);
                    if phase == ContactPhase::Down {
                        self.navigator_drag = None;
                        if geometry.image.contains(position[0], position[1]) {
                            let [x, y] = camera.work_area_center();
                            let center = camera.input_transform().map(layer_core::Point { x, y });
                            self.navigator_drag =
                                Some(if geometry.in_work_area(camera, position) {
                                    [center.x - point[0], center.y - point[1]]
                                } else {
                                    [0.0; 2]
                                });
                        }
                    }
                    if let Some(offset) = self.navigator_drag {
                        camera.center_on([point[0] + offset[0], point[1] + offset[1]]);
                        self.initial_fit = false;
                        self.sync_camera();
                        (CAMERA, true)
                    } else {
                        (0, false)
                    }
                }
            }
            UiAction::FilterPicker { action } => {
                self.state.filter_picker.apply(action);
                self.state.adjustments =
                    effects::catalog(&self.effect_catalog, &self.state.filter_picker);
                (DOCUMENT, false)
            }
            UiAction::Effect { action } => {
                self.require_idle()?;
                self.effect_action(action)?;
                self.refresh_document();
                (DOCUMENT | LAYOUT, true)
            }
            UiAction::Layer { action } => {
                self.require_idle()?;
                self.layer_action(action)?;
                self.refresh_document();
                (DOCUMENT | BRUSH, true)
            }
            UiAction::MeasureTitlebar { insets } => {
                if !insets
                    .into_iter()
                    .all(|v| v.is_finite() && (0.0..1_000_000.0).contains(&v))
                {
                    return Err("Invalid titlebar measurement".into());
                }
                if self.state.workspace.layout.titlebar_insets == insets {
                    (0, false)
                } else {
                    self.state.workspace.layout.titlebar_insets = insets;
                    (LAYOUT, false)
                }
            }
            UiAction::MeasurePanels { measurements } => {
                let mut accepted = Vec::new();
                for measurement in measurements {
                    if ![measurement.tab_width, measurement.content_height]
                        .into_iter()
                        .all(|v| v.is_finite() && (0.0..1_000_000.0).contains(&v))
                    {
                        return Err("Invalid panel measurement".into());
                    }
                    if accepted
                        .iter()
                        .any(|m: &PanelMeasurement| m.panel == measurement.panel)
                    {
                        return Err("Duplicate panel measurement".into());
                    }
                    if self.state.workspace.layout.panel(measurement.panel).is_ok() {
                        accepted.push(measurement);
                    }
                }
                if self.state.workspace.layout.measurements == accepted {
                    (0, false)
                } else {
                    self.state.workspace.layout.measurements = accepted;
                    (LAYOUT, false)
                }
            }
            UiAction::DragWorkspace {
                item,
                phase,
                position,
                viewport,
                tabs,
            } => {
                self.drag_workspace(item, phase, position, viewport, &tabs)?;
                (LAYOUT | CUSTOMIZATION, false)
            }
            UiAction::ResizeFloating {
                group,
                edge,
                phase,
                position,
                viewport,
            } => {
                valid_viewport(viewport)?;
                if !position.into_iter().all(f32::is_finite) {
                    return Err("Invalid resize position".into());
                }
                if phase == ContactPhase::Down {
                    let b = self
                        .layout(viewport)
                        .groups
                        .into_iter()
                        .find(|g| g.id == group && g.floating)
                        .ok_or("Unknown floating group")?
                        .bounds;
                    self.workspace_history.begin_named(
                        &self.state.workspace,
                        format!(
                            "Resized {}",
                            workspace::description::item_name(
                                &self.state.workspace.layout,
                                DockItem::Group { group }
                            )
                        ),
                    );
                    self.floating_resize = Some(FloatingResize {
                        group,
                        edge,
                        start: b,
                        press: position,
                    });
                } else if phase == ContactPhase::Cancel {
                    if self
                        .floating_resize
                        .is_some_and(|resize| resize.group == group)
                    {
                        self.floating_resize = None;
                        self.workspace_history.cancel(&mut self.state.workspace);
                    }
                } else {
                    let resize = self
                        .floating_resize
                        .filter(|resize| resize.group == group && resize.edge == edge)
                        .ok_or("Floating resize is not active")?;
                    self.state.workspace.layout.resize_floating(
                        group,
                        edge,
                        resize.start,
                        [position[0] - resize.press[0], position[1] - resize.press[1]],
                        viewport,
                    )?;
                    if phase == ContactPhase::Up {
                        self.floating_resize = None;
                        self.workspace_history.finish(&self.state.workspace);
                    }
                }
                (LAYOUT, false)
            }
            UiAction::ResetColumnWidth { id, viewport } => {
                self.state
                    .workspace
                    .layout
                    .reset_column_width(id, viewport)?;
                (LAYOUT, false)
            }
            UiAction::DoubleClickPanelHandle { group, viewport } => {
                valid_viewport(viewport)?;
                let layout = &mut self.state.workspace.layout;
                if matches!(
                    self.state.platform,
                    Platform::Gtk
                        | Platform::Generic
                        | Platform::Android
                        | Platform::Web
                        | Platform::Ios
                        | Platform::Mac
                        | Platform::Windows
                ) && layout.column_for_group(group).is_some()
                {
                    layout.set_column_collapsed(group, true, viewport)?;
                } else {
                    layout.double_click_panel_handle(group, viewport)?;
                }
                (LAYOUT, false)
            }
            UiAction::Customize { action } => {
                let viewport = self
                    .logical_viewport
                    .or(self.interaction.viewport)
                    .unwrap_or(self.state.camera.viewport.map(|v| v as f32));
                let partial_zen = self.state.partial_zen();
                let changed = self.state.customization.edit(
                    &mut self.state.workspace.layout,
                    action,
                    self.state.platform,
                    viewport,
                    partial_zen,
                )?;
                if changed & LAYOUT != 0
                    && let Some(before) = workspace_before.as_ref()
                {
                    let geometry = before.layout.workspace(
                        viewport[0],
                        viewport[1],
                        HEADER_HEIGHT,
                        STATUS_HEIGHT,
                    );
                    self.state
                        .workspace
                        .layout
                        .reclaim_removed_columns(&before.layout, &geometry);
                }
                (changed, false)
            }
            UiAction::ActivateTile { panel, tile } => {
                let control = self
                    .state
                    .workspace
                    .layout
                    .panel(panel)?
                    .tiles()
                    .iter()
                    .find(|t| t.id == tile)
                    .ok_or("The tool no longer exists")?
                    .control;
                if control == ToolbarControl::Divider {
                    return Ok(UiChange::default());
                }
                let selected = self
                    .panel_view(panel)?
                    .tiles
                    .iter()
                    .find(|t| t.id == tile)
                    .is_some_and(|t| t.choice.selected && t.enabled);
                if !selected
                    && control.selectable()
                    && self.switches_toolbar_drawer(TileAnchor { panel, tile })
                {
                    let selected =
                        self.dispatch(control.action().ok_or("This tool is unavailable")?)?;
                    let mut opened = self.dispatch(UiAction::Customize {
                        action: CustomizationAction::ToggleToolDrawer {
                            anchor: TileAnchor { panel, tile },
                        },
                    })?;
                    opened.regions |= selected.regions;
                    opened.canvas_wake |= selected.canvas_wake;
                    return Ok(opened);
                }
                if matches!(
                    self.state.platform,
                    Platform::Gtk
                        | Platform::Generic
                        | Platform::Android
                        | Platform::Web
                        | Platform::Ios
                        | Platform::Mac
                        | Platform::Windows
                ) && control.drawer_columns().is_some()
                    && (!control.selectable()
                        || selected
                        || self
                            .state
                            .customization
                            .drawer
                            .as_ref()
                            .is_some_and(|d| d.anchor.tile() == Some(TileAnchor { panel, tile })))
                {
                    return self.dispatch(UiAction::Customize {
                        action: CustomizationAction::ToggleToolDrawer {
                            anchor: TileAnchor { panel, tile },
                        },
                    });
                }
                let action = control.action().ok_or("This panel drawer is unavailable")?;
                if let UiAction::Customize {
                    action: CustomizationAction::OpenControl { control },
                } = &action
                    && self.state.customization.control == Some(*control)
                {
                    return self.dispatch(UiAction::Customize {
                        action: CustomizationAction::CloseControl,
                    });
                }
                return self.dispatch(action);
            }
            UiAction::RestoreWorkspace { mut workspace } => {
                workspace.validate()?;
                workspace.layout.titlebar_insets = self.state.workspace.layout.titlebar_insets;
                self.state.workspace = workspace;
                self.workspace_history = workspace::WorkspaceHistory::default();
                self.divider_drag = None;
                self.floating_resize = None;
                self.workspace_drag = None;
                self.workspace_tab_drag = None;
                self.state.customization = CustomizationState::default();
                (LAYOUT | CUSTOMIZATION, false)
            }
            UiAction::Invoke { command } => {
                if !self.command(command).enabled {
                    return Err(format!(
                        "{} is unavailable during this interaction",
                        command.label()
                    ));
                }
                self.invoke(command)?
            }
            UiAction::SelectBrush { id } => {
                self.select_brush(id)?;
                (BRUSH, false)
            }
            UiAction::CycleTool { family } => {
                // Honor customized toolbar order, appending absent family tools
                // in their catalog order. Duplicate tiles never repeat a tool.
                let mut commands = Vec::new();
                for control in self
                    .state
                    .workspace
                    .layout
                    .panels
                    .iter()
                    .flat_map(|p| p.tiles())
                    .map(|t| t.control)
                    .chain(
                        family
                            .commands()
                            .iter()
                            .map(|&command| ToolbarControl::Command { command }),
                    )
                {
                    if let ToolbarControl::Command { command } = control
                        && family.commands().contains(&command)
                        && !commands.contains(&command)
                    {
                        commands.push(command);
                    }
                }
                let next = commands
                    .iter()
                    .position(|&c| self.command_flags(c).1)
                    .map_or(0, |i| (i + 1) % commands.len());
                self.invoke(commands[next])?
            }
            UiAction::SelectToolGroup { group } => {
                if self.layer_interaction.tool != LayerCanvasTool::Paint
                    || group.tool() != self.state.brush.tool
                {
                    return Err("This group belongs to another tool".into());
                }
                self.select_brush(self.tools.group(group))?;
                (BRUSH, false)
            }
            UiAction::SetBrushSize { value } => {
                NumericControl::brush_size().validate(value, "Brush size")?;
                self.state.brush.diameter = value;
                self.apply_brush()?;
                self.tools
                    .set_override(self.state.brush.preset, "size", value)?;
                (BRUSH, false)
            }
            UiAction::SetBrushOpacity { value } => {
                NumericControl::percent().validate(value, "Opacity")?;
                self.state.brush.opacity = value;
                self.apply_brush()?;
                self.tools
                    .set_override(self.state.brush.preset, "opacity", value)?;
                (BRUSH, false)
            }
            UiAction::SetColor { rgba } => {
                self.state.colors.set_rgba(rgba)?;
                self.state.brush.color = self.state.colors.rgba();
                self.apply_brush()?;
                (BRUSH, false)
            }
            UiAction::Color { action } => {
                self.state.colors.apply(action)?;
                self.state.brush.color = self.state.colors.rgba();
                self.apply_brush()?;
                (BRUSH, false)
            }
            UiAction::SetToolSetting { id, value } => {
                if !self.state.tool_settings.iter().any(|c| c.id == id) {
                    return Err("This setting is not used by the selected tool".into());
                }
                if matches!(self.layer_interaction.tool, LayerCanvasTool::Region { .. })
                    && id != "opacity"
                {
                    self.region_tools.edit(&id, value)?;
                    self.refresh_tools();
                    return Ok(self.changed(BRUSH, false));
                }
                if id.starts_with("transform_") {
                    self.set_transform_control(&id, value)?;
                    return Ok(self.changed(BRUSH, true));
                }
                let brush = tool_settings::edit(self.engine.configured_brush(), &id, value)?;
                self.state.brush.diameter = brush.diameter;
                self.state.brush.opacity = brush.opacity;
                self.engine.set_brush(brush).map_err(error)?;
                self.apply_brush()?;
                self.tools
                    .set_override(self.state.brush.preset, &id, value)?;
                (BRUSH, false)
            }
            UiAction::SelectLayer { id } => {
                self.require_idle()?;
                self.layer_action(LayerAction::Select { id, mask: false })?;
                (0, true)
            }
            UiAction::SetLayerVisibility { id, visible } => {
                self.require_idle()?;
                self.engine
                    .set_layer_visibility(LayerId(id), visible)
                    .map_err(error)?;
                (0, true)
            }
            UiAction::SetLayerOpacity { id, opacity } => {
                self.require_idle()?;
                self.set_layer_opacity(id, opacity)?;
                (0, true)
            }
            UiAction::MoveLayer { id, index } => {
                self.require_idle()?;
                let layers = &self.engine.document().layers;
                if !layers
                    .iter()
                    .any(|layer| layer.id == LayerId(id) && layer.kind == LayerKind::Paint)
                    || !layers
                        .get(index as usize)
                        .is_some_and(|layer| layer.kind == LayerKind::Paint)
                {
                    return Err("Move paint layers within the paint stack".into());
                }
                self.engine
                    .move_layer(LayerId(id), index as usize)
                    .map_err(error)?;
                (0, true)
            }
            UiAction::MovePanel {
                panel,
                target,
                viewport,
            } => {
                self.state
                    .workspace
                    .layout
                    .move_panel(viewport, panel, target)?;
                (LAYOUT, false)
            }
            UiAction::MoveGroup {
                group,
                target,
                viewport,
            } => {
                self.state.workspace.layout.move_item(
                    viewport,
                    DockItem::Group { group },
                    target,
                )?;
                (LAYOUT, false)
            }
            UiAction::MoveColumn {
                column,
                target,
                viewport,
            } => {
                self.state.workspace.layout.move_item(
                    viewport,
                    DockItem::Column { column },
                    target,
                )?;
                (LAYOUT, false)
            }
            UiAction::SelectPanelTab { group, panel } => {
                let changed = self.state.workspace.layout.select_tab(group, panel)?;
                if !changed
                    && self
                        .state
                        .workspace
                        .layout
                        .collapsed_column_for_group(group)
                        .is_none()
                {
                    let action = if self.state.customization.expanded == Some(panel) {
                        CustomizationAction::CloseExpanded
                    } else {
                        CustomizationAction::ShowAllControls { panel }
                    };
                    let partial_zen = self.state.partial_zen();
                    self.state.customization.edit(
                        &mut self.state.workspace.layout,
                        action,
                        self.state.platform,
                        self.logical_viewport
                            .or(self.interaction.viewport)
                            .unwrap_or(self.state.camera.viewport.map(|v| v as f32)),
                        partial_zen,
                    )?;
                } else if let Some(previous) = self.state.customization.expanded {
                    self.state.customization.expanded =
                        (self.state.workspace.layout.panel_group(previous) == Some(group))
                            .then_some(panel);
                }
                (LAYOUT | CUSTOMIZATION, false)
            }
            UiAction::MoveTile {
                panel,
                tile,
                target,
                viewport,
            } => {
                self.state.workspace.layout.move_item(
                    viewport,
                    DockItem::Tile { panel, tile },
                    target,
                )?;
                (LAYOUT, false)
            }
            UiAction::ResizeDock {
                id,
                position,
                viewport,
            } => {
                self.state
                    .workspace
                    .layout
                    .resize_workspace(id, position, viewport)?;
                (LAYOUT, false)
            }
            UiAction::DragDivider {
                id,
                phase,
                position,
                viewport,
            } => {
                if phase == ContactPhase::Cancel {
                    if self.divider_drag.is_some_and(|(active, _)| active == id) {
                        self.divider_drag = None;
                        self.workspace_history.cancel(&mut self.state.workspace);
                    }
                    (LAYOUT, false)
                } else {
                    valid_viewport(viewport)?;
                    if !position.into_iter().all(f32::is_finite) {
                        return Err("Invalid divider position".into());
                    }
                    if phase == ContactPhase::Down {
                        let divider = self.divider(id, viewport)?;
                        let mut drag = ResizeDrag::new(position, divider.bounds);
                        if self.column_resize_enabled() {
                            let columns = self
                                .state
                                .workspace
                                .layout
                                .collapsed_divider_columns(&divider);
                            if columns.iter().any(Option::is_some) {
                                drag.phase = ResizeDragPhase::Expand {
                                    columns,
                                    edge: divider.bounds.x + divider.bounds.width * 0.5,
                                };
                            }
                        }
                        self.workspace_history.begin(&self.state.workspace);
                        self.divider_drag = Some((id, drag));
                        (0, false)
                    } else {
                        let (_, mut drag) = self
                            .divider_drag
                            .filter(|(active, _)| *active == id)
                            .ok_or("Divider drag is not active")?;
                        self.resize_divider(id, position, viewport, &mut drag)?;
                        self.divider_drag = Some((id, drag));
                        if phase == ContactPhase::Up {
                            self.divider_drag = None;
                            self.workspace_history.finish(&self.state.workspace);
                        }
                        (LAYOUT, false)
                    }
                }
            }
            UiAction::NudgeDivider {
                id,
                forward,
                viewport,
            } => {
                valid_viewport(viewport)?;
                let divider = self.divider(id, viewport)?;
                let mut position = [
                    divider.bounds.x + divider.bounds.width * 0.5,
                    divider.bounds.y + divider.bounds.height * 0.5,
                ];
                position[usize::from(divider.axis == Axis::Vertical)] +=
                    if forward { 12.0 } else { -12.0 };
                self.state
                    .workspace
                    .layout
                    .resize_workspace(id, position, viewport)?;
                (LAYOUT, false)
            }
            UiAction::PrioritizeBand { id } => {
                self.state.workspace.layout.prioritize(id)?;
                (LAYOUT, false)
            }
            UiAction::SetTheme { theme } => {
                self.state.settings.theme = theme;
                (SETTINGS, true)
            }
            UiAction::WindowFullscreen { fullscreen } => {
                let changed = self.state.fullscreen != fullscreen;
                self.state.fullscreen = fullscreen;
                (if changed { DOCUMENT } else { 0 }, false)
            }
            UiAction::SystemThemeChanged { theme } => {
                self.system_theme = theme;
                if self.state.settings.theme.is_none() && self.state.theme != theme {
                    (SETTINGS, true)
                } else {
                    (0, false)
                }
            }
            UiAction::EditSettings { settings } => {
                settings.validate()?;
                if !self.state.settings_open {
                    return Err("Settings are not open".into());
                }
                save_settings = settings != self.state.settings;
                if save_settings {
                    self.apply_settings(settings)?;
                }
                (SETTINGS | COMMANDS, save_settings)
            }
            UiAction::OpenSettings { page } => {
                self.open_settings(page);
                (SETTINGS, false)
            }
            UiAction::Preferences { action } => {
                if !self.state.settings_open
                    && !matches!(
                        action,
                        PreferenceAction::Edit { .. }
                            | PreferenceAction::Reset { .. }
                            | PreferenceAction::Reveal { .. }
                    )
                {
                    return Err("Settings are not open".into());
                }
                // Validate edits atomically. Search, navigation and recording
                // change only view state and never trigger storage or rendering.
                let reveal = matches!(action, PreferenceAction::Reveal { .. });
                let mut settings = self.state.settings.clone();
                self.state
                    .preferences
                    .edit(&mut settings, action, self.state.platform);
                if reveal && self.state.preferences.error.is_none() {
                    self.state.settings_open = true;
                }
                save_settings =
                    self.state.preferences.error.is_none() && settings != self.state.settings;
                if save_settings {
                    settings.validate()?;
                    self.apply_settings(settings)?;
                }
                (SETTINGS, save_settings)
            }
            UiAction::RestoreSettings { settings } => {
                settings.validate()?;
                self.apply_settings(settings)?;
                (SETTINGS | COMMANDS, true)
            }
            UiAction::CompleteRequest { id, error } => {
                let index = self
                    .state
                    .requests
                    .iter()
                    .position(|r| r.id == id)
                    .ok_or("Unknown host request")?;
                if matches!(
                    self.state.requests[index].kind,
                    HostRequestKind::Document { .. }
                ) {
                    return Err("Complete document requests through the document service".into());
                }
                self.state.requests.remove(index);
                self.state.host_error = error;
                (HOST, false)
            }
            UiAction::CloseSettings => {
                self.state.settings_open = false;
                self.state.preferences = PreferencesState::default();
                (SETTINGS, false)
            }
        };
        if let Some(before) = workspace_before {
            if let Some(description) = workspace_description {
                self.workspace_history
                    .record_named(before, &self.state.workspace, &description);
            } else {
                self.workspace_history.record(before, &self.state.workspace);
            }
        }
        if explicit_color
            || revision != self.engine.document().revision
            || tool_before != (self.state.brush.tool, self.layer_interaction.tool)
        {
            self.eyedropper.cancel();
            self.region_tools.cancel();
        }
        if changed & LAYOUT != 0 {
            let layout = &self.state.workspace.layout;
            self.state.customization.expanded = self
                .state
                .customization
                .expanded
                .and_then(|panel| layout.active_panel(panel));
        }
        if self.state.customization.drawer.is_some()
            && (tool_before != (self.state.brush.tool, self.layer_interaction.tool)
                || self.state.customization.expanded.is_some()
                || (changed & LAYOUT != 0
                    && self.state.customization.drawer.as_ref().is_some_and(|d| {
                        let layout = &self.state.workspace.layout;
                        d.anchor.tile().is_none_or(|a| {
                            layout.active_panel(a.panel) != Some(a.panel)
                                || layout
                                    .panel(a.panel)
                                    .map_or(true, |p| !p.tiles().iter().any(|t| t.id == a.tile))
                        })
                    })))
        {
            self.state.customization.drawer = None;
            changed |= CUSTOMIZATION;
        }
        if changed & LAYOUT != 0 {
            self.state.customization.column_drawers = self
                .state
                .customization
                .column_drawers
                .iter()
                .filter_map(|d| match d.anchor {
                    DrawerAnchor::Column { group, origin, .. } => {
                        // These hosts draw the sidebar selection and connector at the visible tab.
                        let origin = if matches!(
                            self.state.platform,
                            Platform::Gtk | Platform::Android | Platform::Web
                        ) {
                            self.state.workspace.layout.active_panel(origin)?
                        } else {
                            origin
                        };
                        ContentDrawer::for_column(&self.state.workspace.layout, group, origin).ok()
                    }
                    _ => None,
                })
                .collect();
            changed |= CUSTOMIZATION;
        }
        if let Some(anchor) = self
            .state
            .customization
            .drawer
            .as_ref()
            .and_then(|d| d.anchor.tile())
            && let Some(group) = self.state.workspace.layout.panel_group(anchor.panel)
            && self
                .state
                .workspace
                .layout
                .collapsed_column_for_group(group)
                .is_some()
            && !self.state.customization.column_drawers.iter().any(|d| {
                d.tabs
                    .as_ref()
                    .is_some_and(|t| t.group == group && t.active == anchor.panel)
            })
        {
            self.state.customization.drawer = None;
            changed |= CUSTOMIZATION;
        }
        if was_expanded && !self.state.customization.has_drawer() && was_zen {
            self.interaction.keep_chrome_until_contact = self.state.workspace.zen_mode;
        }
        if save_settings {
            self.request(HostRequestKind::SaveSettings {
                settings: Box::new(self.state.settings.clone()),
            })?;
            changed |= HOST;
        }
        if changed & LAYOUT != 0 {
            self.sync_work_area();
        }
        if self.engine.document().revision != revision || self.layer_interaction.changed {
            self.layer_interaction.changed = false;
            self.refresh_document();
            changed |= DOCUMENT;
        }
        if self.refresh_commands() {
            changed |= COMMANDS;
        }
        let change = self.changed(changed, wake || transforming != self.operation.active());
        // Preserve legacy region notifications, but let incremental consumers
        // distinguish ordinary motion from tear-off and every other UI change.
        // An unrelated action/measurement advances model_revision independently.
        if moving_workspace
            && changed == (LAYOUT | CUSTOMIZATION)
            && drag_before
                .zip(self.workspace_drag)
                .is_some_and(|(before, after)| {
                    before.original == after.original
                        && before.floating == after.floating
                        && before.viewport == after.viewport
                        && before.chrome_revealed == after.chrome_revealed
                })
        {
            self.workspace_model_revision = model_revision;
            self.workspace_content_revision = content_revision;
        }
        if layout_only
            && changed == (LAYOUT | CUSTOMIZATION)
            && collapsed_before.as_ref() == Some(&self.state.workspace.layout.collapsed)
            && !self.state.customization.is_open()
        {
            self.workspace_content_revision = content_revision;
        }
        Ok(change)
    }

    /// Raw records retain platform timestamp/history/prediction metadata. A
    /// full queue returns the untouched record; hosts must retry after a frame.
    pub fn pen(&mut self, event: PenEvent) -> Result<(), PenEvent> {
        if self.workspace_transition || (self.workspace_read_only && event.phase == PenPhase::Down)
        {
            return Ok(());
        }
        if matches!(self.layer_interaction.tool, LayerCanvasTool::Region { .. }) {
            self.region_pen(event);
            return Ok(());
        }
        if self.layer_interaction.tool == LayerCanvasTool::Hand {
            // Hand input is routed through UiInput::Pointer's pan gesture.
            return Ok(());
        }
        if self.layer_interaction.tool.picks_color() {
            match event.phase {
                PenPhase::Down => {
                    self.eyedropper.cancel();
                    self.eyedropper.contact = true;
                }
                PenPhase::Cancel => {
                    self.eyedropper.cancel();
                    return Ok(());
                }
                PenPhase::Hover => return Ok(()),
                _ => (),
            }
            if self.eyedropper.contact {
                let mut point = self
                    .state
                    .camera
                    .input_transform()
                    .map(event.surface_position);
                let doc = self.engine.document();
                let source = if self.eyedropper.layer {
                    let offset = doc.layer_offset(doc.active_layer);
                    point.x -= offset.x;
                    point.y -= offset.y;
                    layer_render::ColorSampleSource::Layer(doc.active_layer)
                } else {
                    layer_render::ColorSampleSource::Composite
                };
                if point.x.is_finite()
                    && point.y.is_finite()
                    && point.x >= 0.0
                    && point.y >= 0.0
                    && point.x < doc.width as f32
                    && point.y < doc.height as f32
                {
                    self.eyedropper
                        .queue(source, [point.x as u32, point.y as u32]);
                }
                if event.phase == PenPhase::Up {
                    self.eyedropper.contact = false;
                }
            }
            return Ok(());
        }
        if self.layer_interaction.tool != LayerCanvasTool::Paint {
            if let Err(error) = self.layer_pen(event) {
                let _ = self.cancel_layer_gesture();
                self.state.host_error = Some(error);
            }
            self.input_pending = true;
            return Ok(());
        }
        self.pen.push(event)?;
        self.initial_fit = false;
        self.input_pending = true;
        Ok(())
    }

    pub fn touch(&mut self, id: u64, phase: PenPhase, position: [f32; 2]) -> UiChange {
        if self.input_pending
            || self.engine.has_active_stroke()
            || !self.layer_interaction.path.is_empty()
        {
            self.touch.clear();
            return self.changed(0, false);
        }
        if self.touch.update(
            &mut self.state.camera,
            id,
            phase,
            position,
            self.layer_interaction.tool == LayerCanvasTool::Hand,
        ) {
            self.initial_fit = false;
            self.sync_camera();
            self.changed(regions::CAMERA, true)
        } else {
            self.changed(0, false)
        }
    }

    /// Host-normalized logical wheel deltas; camera positions remain physical.
    pub fn scroll(
        &mut self,
        anchor: [f32; 2],
        delta: [f32; 2],
        dpi: f32,
        zoom: bool,
        horizontal: bool,
    ) -> Result<UiChange, String> {
        if self.interaction.pointer.is_some() || self.require_idle().is_err() {
            return Ok(self.changed(0, false));
        }
        if zoom {
            return self.gesture(
                anchor,
                anchor,
                (-delta[1] * 0.0015 * self.state.settings.zoom_speed).exp(),
                0.0,
            );
        }
        let delta = if horizontal {
            [delta[0] + delta[1], 0.0]
        } else {
            delta
        };
        let delta = delta.map(|v| v * self.state.settings.pan_speed);
        self.gesture(
            anchor,
            [anchor[0] - delta[0] * dpi, anchor[1] - delta[1] * dpi],
            1.0,
            0.0,
        )
    }

    pub fn gesture(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        scale: f32,
        rotation: f32,
    ) -> Result<UiChange, String> {
        self.require_idle()?;
        self.state.camera.gesture(from, to, scale, rotation)?;
        self.initial_fit = false;
        self.sync_camera();
        Ok(self.changed(regions::CAMERA, true))
    }

    /// Hosts report measured logical and physical window sizes. Dock fitting
    /// bounds and the first viewport fit are core policy, not host arithmetic.
    pub fn set_viewport(
        &mut self,
        logical: [f32; 2],
        physical: [u32; 2],
    ) -> Result<UiChange, String> {
        valid_viewport(logical)?;
        if physical.contains(&0) {
            return Err("Canvas viewport must be nonzero".into());
        }
        if self.logical_viewport == Some(logical)
            && self.state.camera.viewport == physical
            && !self.initial_fit
        {
            return Ok(self.changed(0, false));
        }
        let old_scale = self
            .logical_viewport
            .map_or(1.0, |v| self.state.camera.viewport[0] as f32 / v[0]);
        let new_scale = physical[0] as f32 / logical[0];
        let resized = self.state.camera.viewport != physical;
        if resized {
            self.engine
                .resize_surface(physical[0], physical[1])
                .map_err(error)?;
            self.state.camera.resize(physical);
            self.touch.clear();
        }
        self.logical_viewport = Some(logical);
        if let Some(event) = self.cursor.event.as_mut() {
            event.surface_position.x *= new_scale / old_scale;
            event.surface_position.y *= new_scale / old_scale;
        }
        let bounds_changed = self.sync_work_area();
        let fit = self.initial_fit;
        if fit {
            self.initial_fit = false;
            let doc = self.engine.document();
            self.state.camera.fit([doc.width, doc.height]);
        }
        if resized || fit {
            self.sync_camera();
        }
        Ok(self.changed(
            if resized || fit || bounds_changed {
                regions::CAMERA
            } else {
                0
            },
            resized || fit,
        ))
    }

    fn sync_work_area(&mut self) -> bool {
        let Some(logical) = self.logical_viewport else {
            return false;
        };
        let b = self.layout(logical).work_area;
        let scale = [
            self.state.camera.viewport[0] as f32 / logical[0],
            self.state.camera.viewport[1] as f32 / logical[1],
        ];
        let bounds = [
            b.x * scale[0],
            b.y * scale[1],
            b.width * scale[0],
            b.height * scale[1],
        ];
        let changed = self.state.camera.work_area != bounds;
        self.state.camera.work_area = bounds;
        changed
    }

    fn divider(&self, id: u32, viewport: [f32; 2]) -> Result<Divider, String> {
        self.layout(viewport)
            .dividers
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| "Unknown divider".into())
    }

    fn column_resize_enabled(&self) -> bool {
        matches!(
            self.state.platform,
            Platform::Gtk
                | Platform::Generic
                | Platform::Android
                | Platform::Web
                | Platform::Ios
                | Platform::Mac
                | Platform::Windows
        )
    }

    fn resize_divider(
        &mut self,
        id: u32,
        position: [f32; 2],
        viewport: [f32; 2],
        drag: &mut ResizeDrag,
    ) -> Result<(), String> {
        let point = drag.position(position);
        loop {
            match drag.phase {
                ResizeDragPhase::Resizing => break,
                ResizeDragPhase::Collapsed(collapse) => {
                    if collapse.contains(point[0]) {
                        return Ok(());
                    }
                    // Recreate the geometry just before collapse, then apply the current
                    // pointer normally. This clamps reopening to the minimum and keeps
                    // nested dividers' threshold anchored to the same parent edge.
                    let layout = &mut self.state.workspace.layout;
                    if let Some(column) = layout
                        .collapsed
                        .iter_mut()
                        .find(|c| c.root == collapse.root)
                    {
                        column.expanded_width = collapse.expanded_width;
                    }
                    layout.set_column_collapsed(collapse.root, false, viewport)?;
                    drag.phase = ResizeDragPhase::Resizing;
                }
                ResizeDragPhase::Expand { columns, edge } => {
                    let delta = point[0] - edge;
                    // Opening uses a fixed 36 logical pixels of outward movement,
                    // independent of the column's saved or minimum width.
                    if delta.abs() < TILE_SIZE {
                        return Ok(());
                    }
                    let reversed = delta < 0.;
                    let Some(root) = columns[usize::from(reversed)] else {
                        return Ok(());
                    };
                    self.state
                        .workspace
                        .layout
                        .expand_column_for_resize(root, viewport)?;
                    let divider = self.divider(id, viewport)?;
                    drag.phase = ResizeDragPhase::CatchUp {
                        columns,
                        collapsed_edge: edge,
                        edge: divider.bounds.x + divider.bounds.width * 0.5,
                        reversed,
                    };
                }
                ResizeDragPhase::CatchUp {
                    columns,
                    collapsed_edge,
                    edge,
                    reversed,
                } => {
                    if if reversed {
                        point[0] <= edge
                    } else {
                        point[0] >= edge
                    } {
                        drag.phase = ResizeDragPhase::Resizing;
                    } else {
                        let outward = (point[0] - collapsed_edge) * if reversed { -1. } else { 1. };
                        if outward >= TILE_SIZE {
                            return Ok(());
                        }
                        // Before reaching the edge, undo opening exactly. Preserve
                        // saved widths and split fractions across repeated attempts,
                        // and leave no history entry if the user releases collapsed.
                        let mut restored = self
                            .workspace_history
                            .gesture_start()
                            .ok_or("Divider drag is not active")?
                            .layout
                            .clone();
                        // Keep host measurements and strip scrolling, as workspace undo does.
                        restored
                            .measurements
                            .clone_from(&self.state.workspace.layout.measurements);
                        restored
                            .column_scroll
                            .clone_from(&self.state.workspace.layout.column_scroll);
                        restored.titlebar_insets = self.state.workspace.layout.titlebar_insets;
                        self.state.workspace.layout = restored;
                        drag.phase = ResizeDragPhase::Expand {
                            columns,
                            edge: collapsed_edge,
                        };
                        // The same event can already be past the opposite opening
                        // threshold when both neighbors started collapsed.
                    }
                }
            }
        }
        let collapse = self
            .column_resize_enabled()
            .then(|| {
                self.state
                    .workspace
                    .layout
                    .collapse_at_divider(id, point, viewport)
            })
            .flatten();
        if let Some(collapse) = collapse {
            let root = collapse.root;
            let original_width = self.workspace_history.gesture_start().and_then(|s| {
                // A gesture that began collapsed must remember the expanded
                // width, not overwrite it with the strip width when closing again.
                s.layout
                    .collapsed
                    .iter()
                    .find(|c| c.root == root)
                    .map(|c| c.expanded_width)
                    .or_else(|| s.layout.column_width_before_resize(root, viewport))
            });
            self.state
                .workspace
                .layout
                .set_column_collapsed(root, true, viewport)?;
            if let Some(width) = original_width
                && let Some(c) = self
                    .state
                    .workspace
                    .layout
                    .collapsed
                    .iter_mut()
                    .find(|c| c.root == root)
            {
                c.expanded_width = width;
            }
            drag.phase = ResizeDragPhase::Collapsed(collapse);
        } else {
            self.state
                .workspace
                .layout
                .resize_workspace(id, point, viewport)?;
        }
        Ok(())
    }

    /// Includes shared background work as well as the drawing engine's needs.
    pub fn wants_continuous_frames(&self) -> bool {
        self.engine.wants_continuous_frames()
            || self.pending_filters.is_some()
            || self.eyedropper.busy()
            || self.region_tools.busy()
    }
    pub fn frame(&mut self, now_ns: u64, presentation_ns: u64) -> Result<UiChange, String> {
        let mut changed = self.poll_filter_installation();
        let revision = self.engine.document().revision;
        self.engine
            .render_frame_for(now_ns, presentation_ns)
            .map_err(error)?;
        self.input_pending = false;
        changed |= self.poll_document_close();
        if std::mem::take(&mut self.operation.changed) {
            changed |= regions::BRUSH;
        }
        self.poll_region_tool()?;
        if let Some(color) = self.eyedropper.poll(self.engine.backend_mut())? {
            self.state.colors.set_rgba(color)?;
            self.state.brush.color = self.state.colors.rgba();
            self.apply_brush()?;
            changed |= regions::BRUSH;
        }
        if self.engine.document().revision != revision || self.layer_interaction.changed {
            self.layer_interaction.changed = false;
            self.refresh_document();
            changed |= regions::DOCUMENT;
        }
        if self.refresh_commands() {
            changed |= regions::COMMANDS;
        }
        Ok(self.changed(
            changed,
            self.wants_continuous_frames() || self.engine.has_pending_document_edits(),
        ))
    }

    fn invoke(&mut self, command: CommandId) -> Result<(u32, bool), String> {
        use regions::*;
        match command {
            CommandId::Fullscreen => {
                self.request(HostRequestKind::SetFullscreen {
                    fullscreen: !self.state.fullscreen,
                })?;
                Ok((HOST, false))
            }
            CommandId::NewDocument | CommandId::OpenDocument => {
                self.request_document_open(command == CommandId::OpenDocument)?;
                Ok((DOCUMENT | HOST, false))
            }
            CommandId::SaveDocument | CommandId::SaveDocumentAs => {
                self.request_save(command == CommandId::SaveDocumentAs)?;
                Ok((DOCUMENT | HOST, false))
            }
            CommandId::ExportDocument => {
                self.request_document(DocumentRequest::Export {
                    name: self.document_filename("png"),
                })?;
                Ok((DOCUMENT | HOST, false))
            }
            CommandId::CloseDocument => {
                self.request_document_close()?;
                Ok((DOCUMENT | HOST, false))
            }
            CommandId::ScaleRotate => {
                self.begin_transform()?;
                Ok((BRUSH | DOCUMENT, true))
            }
            CommandId::ApplyTransform | CommandId::CancelTransform => {
                self.finish_transform(command == CommandId::ApplyTransform)?;
                Ok((BRUSH | DOCUMENT, true))
            }
            CommandId::TransformAspect => {
                self.operation.aspect = !self.operation.aspect;
                self.refresh_tools();
                Ok((BRUSH, false))
            }
            CommandId::Ruler => {
                self.layer_action(LayerAction::Tool {
                    tool: LayerCanvasTool::Ruler {
                        kind: self.rulers.kind,
                    },
                })?;
                Ok((BRUSH | DOCUMENT, true))
            }
            CommandId::ShowRulers | CommandId::SnapRulers | CommandId::DeleteRuler => {
                self.ruler_command(command)?;
                Ok((BRUSH | DOCUMENT, true))
            }
            CommandId::Figure => {
                let (shape, paint) = self.layer_interaction.figure;
                self.layer_action(LayerAction::Tool {
                    tool: LayerCanvasTool::Figure { shape, paint },
                })?;
                Ok((BRUSH | DOCUMENT, true))
            }
            CommandId::AutoSelect | CommandId::Fill => {
                let fill = command == CommandId::Fill;
                self.layer_action(LayerAction::Tool {
                    tool: LayerCanvasTool::Region {
                        fill,
                        source: self.region_tools.source[usize::from(fill)],
                    },
                })?;
                Ok((DOCUMENT | BRUSH | COMMANDS, true))
            }
            CommandId::Gradient => {
                let [radial, transparent] = self.layer_interaction.gradient;
                self.layer_action(LayerAction::Tool {
                    tool: LayerCanvasTool::Gradient {
                        radial,
                        transparent,
                    },
                })?;
                Ok((BRUSH | DOCUMENT, false))
            }
            CommandId::Hand | CommandId::Eyedropper => {
                self.layer_action(LayerAction::Tool {
                    tool: if command == CommandId::Hand {
                        LayerCanvasTool::Hand
                    } else if self.eyedropper.layer {
                        LayerCanvasTool::PickLayer
                    } else {
                        LayerCanvasTool::PickVisible
                    },
                })?;
                Ok((BRUSH | DOCUMENT, false))
            }
            CommandId::Lasso | CommandId::Move => {
                self.layer_action(LayerAction::Tool {
                    tool: if command == CommandId::Lasso {
                        LayerCanvasTool::Select
                    } else {
                        LayerCanvasTool::Move
                    },
                })?;
                Ok((BRUSH | DOCUMENT, true))
            }
            CommandId::Pen
            | CommandId::Pencil
            | CommandId::Brush
            | CommandId::Eraser
            | CommandId::Airbrush
            | CommandId::Decoration
            | CommandId::Blend
            | CommandId::Liquify => {
                self.select_brush(self.tools.tool(command.paint_tool().unwrap()))?;
                Ok((BRUSH, false))
            }
            CommandId::Undo => {
                self.engine.undo().map_err(error)?;
                Ok((0, true))
            }
            CommandId::Redo => {
                self.engine.redo().map_err(error)?;
                Ok((0, true))
            }
            CommandId::SelectAll => {
                let doc = self.engine.document();
                let [w, h] = [doc.width as f32, doc.height as f32];
                let selection = layer_core::Selection::polygon(
                    [[0., 0.], [w, 0.], [w, h], [0., h]]
                        .map(|[x, y]| layer_core::Point { x, y })
                        .to_vec(),
                )
                .map_err(error)?;
                self.layer_edit(layer_core::Edit::SetSelection(Some(selection)))?;
                Ok((DOCUMENT, true))
            }
            CommandId::ClearLayer
            | CommandId::FillSelection
            | CommandId::Deselect
            | CommandId::InvertSelection => {
                self.layer_action(match command {
                    CommandId::ClearLayer => LayerAction::Clear {
                        id: self.engine.document().active_layer.0,
                    },
                    CommandId::FillSelection => LayerAction::FillSelection,
                    CommandId::Deselect => LayerAction::Deselect,
                    _ => LayerAction::InvertSelection,
                })?;
                Ok((DOCUMENT, true))
            }
            CommandId::AddLayer => {
                self.layer_action(LayerAction::New {
                    group: false,
                    clipped: false,
                })?;
                Ok((0, true))
            }
            CommandId::DeleteLayer => {
                self.layer_action(LayerAction::Delete {
                    id: self.engine.document().active_layer.0,
                })?;
                Ok((0, true))
            }
            CommandId::RaiseLayer | CommandId::LowerLayer => {
                let id = self.engine.document().active_layer;
                let index = self
                    .engine
                    .document()
                    .layers
                    .iter()
                    .position(|l| l.id == id)
                    .ok_or("Unknown layer")?;
                self.engine
                    .move_layer(
                        id,
                        if command == CommandId::RaiseLayer {
                            index - 1
                        } else {
                            index + 1
                        },
                    )
                    .map_err(error)?;
                Ok((0, true))
            }
            CommandId::FitCanvas => {
                self.initial_fit = false;
                let doc = self.engine.document();
                self.state.camera.fit([doc.width, doc.height]);
                self.sync_camera();
                Ok((CAMERA, true))
            }
            CommandId::ZoomIn
            | CommandId::ZoomOut
            | CommandId::RotateLeft
            | CommandId::RotateRight
            | CommandId::FlipHorizontal
            | CommandId::FlipVertical => {
                self.initial_fit = false;
                let camera = &mut self.state.camera;
                match command {
                    CommandId::FlipHorizontal | CommandId::FlipVertical => {
                        camera.flip(command == CommandId::FlipHorizontal)
                    }
                    _ => {
                        let center = camera.work_area_center();
                        let scale = match command {
                            CommandId::ZoomIn => 2.0_f32.sqrt(),
                            CommandId::ZoomOut => 0.5_f32.sqrt(),
                            _ => 1.0,
                        };
                        let rotation = match command {
                            CommandId::RotateLeft => -std::f32::consts::FRAC_PI_2,
                            CommandId::RotateRight => std::f32::consts::FRAC_PI_2,
                            _ => 0.0,
                        };
                        camera.gesture(center, center, scale, rotation)?;
                    }
                }
                self.sync_camera();
                Ok((CAMERA, true))
            }
            CommandId::Settings | CommandId::KeyboardShortcuts | CommandId::About => {
                self.state.customization = CustomizationState::default();
                self.open_settings(match command {
                    CommandId::KeyboardShortcuts => SettingsPage::Shortcuts,
                    CommandId::About => SettingsPage::About,
                    _ => SettingsPage::Appearance,
                });
                Ok((SETTINGS | CUSTOMIZATION, false))
            }
            CommandId::NewWindow => {
                self.request(HostRequestKind::NewWindow)?;
                Ok((HOST, false))
            }
            CommandId::Website | CommandId::SourceCode => {
                self.request(HostRequestKind::OpenLink {
                    link: if command == CommandId::Website {
                        ApplicationLink::Website
                    } else {
                        ApplicationLink::SourceCode
                    },
                })?;
                Ok((HOST, false))
            }
            CommandId::ToggleTheme => {
                self.state.settings.theme = Some(if self.state.theme == Theme::Dark {
                    Theme::Light
                } else {
                    Theme::Dark
                });
                Ok((SETTINGS, true))
            }
            CommandId::ResetLayout => {
                if self.managed_workspace.is_some() {
                    self.require_workspace_idle()?;
                    self.request(HostRequestKind::Workspace {
                        command: WorkspaceCommand::ResetLayout,
                    })?;
                    return Ok((HOST, false));
                }
                self.state
                    .workspace
                    .layout
                    .reset_docking(self.state.platform)?;
                Ok((LAYOUT, false))
            }
            CommandId::UndoWorkspace | CommandId::RedoWorkspace => {
                if command == CommandId::UndoWorkspace {
                    self.workspace_history.undo(&mut self.state.workspace);
                } else {
                    self.workspace_history.redo(&mut self.state.workspace);
                }
                self.divider_drag = None;
                self.state.customization = CustomizationState::default();
                self.floating_resize = None;
                self.workspace_drag = None;
                self.workspace_tab_drag = None;
                self.interaction.keep_chrome_until_contact = self.state.workspace.zen_mode;
                Ok((LAYOUT | CUSTOMIZATION, false))
            }
            CommandId::NewToolbar | CommandId::ManageToolbars => {
                if self.managed_workspace.is_some() {
                    self.request(HostRequestKind::Workspace {
                        command: if command == CommandId::NewToolbar {
                            WorkspaceCommand::NewToolbar { group: None }
                        } else {
                            WorkspaceCommand::ManageToolbars
                        },
                    })?;
                    return Ok((HOST, false));
                }
                let partial_zen = self.state.partial_zen();
                let changed = self.state.customization.edit(
                    &mut self.state.workspace.layout,
                    if command == CommandId::ManageToolbars {
                        CustomizationAction::ManageToolbars
                    } else {
                        CustomizationAction::NewToolbar { group: None }
                    },
                    self.state.platform,
                    self.logical_viewport
                        .or(self.interaction.viewport)
                        .unwrap_or(self.state.camera.viewport.map(|v| v as f32)),
                    partial_zen,
                )?;
                Ok((changed, false))
            }
            CommandId::ZenMode => {
                self.state.workspace.zen_mode = !self.state.workspace.zen_mode;
                if self.state.workspace.zen_mode {
                    self.state.customization.drawer = None;
                    self.state.customization.column_drawers.clear();
                }
                self.interaction.zen_entry_guard = self.state.workspace.zen_mode
                    && self.interaction.hover.is_some_and(|[x, y]| {
                        (0.0..ZEN_CORNER_GUARD).contains(&x) && (0.0..ZEN_CORNER_GUARD).contains(&y)
                    });
                self.interaction.hidden = self.state.workspace.zen_mode;
                self.interaction.keep_chrome_until_contact = false;
                self.interaction.keyboard_chrome = false;
                Ok((LAYOUT | CUSTOMIZATION, false))
            }
        }
    }

    fn request(&mut self, kind: HostRequestKind) -> Result<(), String> {
        let id = self.next_request;
        self.next_request = id.checked_add(1).ok_or("Host request IDs exhausted")?;
        self.state.requests.push(HostRequest { id, kind });
        Ok(())
    }
    fn open_settings(&mut self, page: SettingsPage) {
        self.state.settings_open = true;
        self.state.preferences.edit(
            &mut self.state.settings,
            PreferenceAction::Page { page },
            self.state.platform,
        );
    }
    fn apply_settings(&mut self, settings: Settings) -> Result<(), String> {
        self.engine
            .set_instant_feedback(settings.feedback_config())
            .map_err(error)?;
        self.engine.set_pressure_curve(PressureCurve {
            gamma: settings.pressure_gamma,
            ..Default::default()
        });
        self.state.settings = settings;
        self.refresh_shortcuts();
        Ok(())
    }

    fn select_brush(&mut self, id: u32) -> Result<(), String> {
        let preset = preset(id)?;
        self.cancel_layer_gesture()?;
        self.tools
            .remember(self.state.brush.preset, self.engine.configured_brush());
        let brush = self.tools.brush(preset);
        self.engine.set_brush(brush.clone()).map_err(error)?;
        self.layer_interaction.tool = LayerCanvasTool::Paint;
        self.state.layer_tools.tool = LayerCanvasTool::Paint;
        self.state.brush.preset = id;
        self.state.brush.tool = tools::group(id).tool();
        self.state.brush.diameter = brush.diameter;
        self.state.brush.opacity = brush.opacity;
        self.apply_brush()
    }

    fn apply_brush(&mut self) -> Result<(), String> {
        self.cursor.hover.reset();
        let state = &self.state.brush;
        let mut brush = self.engine.configured_brush().clone();
        brush.diameter = state.diameter;
        brush.opacity = state.opacity;
        brush.color_rgba_linear = [
            srgb_to_linear(state.color[0]),
            srgb_to_linear(state.color[1]),
            srgb_to_linear(state.color[2]),
            state.color[3],
        ];
        self.engine.set_brush(brush).map_err(error)?;
        self.engine.set_tool(
            if state.tool == Tool::Eraser || self.state.colors.transparent() {
                StrokeTool::Eraser
            } else {
                StrokeTool::Brush
            },
        );
        self.tools
            .remember(self.state.brush.preset, self.engine.configured_brush());
        self.refresh_tools();
        Ok(())
    }

    fn refresh_tools(&mut self) {
        self.state.tool_actions = if self.operation.active() {
            [
                CommandId::TransformAspect,
                CommandId::ApplyTransform,
                CommandId::CancelTransform,
            ]
            .into_iter()
            .map(|command| ToolSettingAction {
                command,
                checkable: command.is_toggle(),
            })
            .collect()
        } else if matches!(self.layer_interaction.tool, LayerCanvasTool::Ruler { .. })
            || !self.engine.document().rulers.is_empty()
        {
            [
                CommandId::ShowRulers,
                CommandId::SnapRulers,
                CommandId::DeleteRuler,
            ]
            .into_iter()
            .filter(|c| {
                *c != CommandId::DeleteRuler
                    || matches!(self.layer_interaction.tool, LayerCanvasTool::Ruler { .. })
                    || self.layer_interaction.tool == LayerCanvasTool::Move
            })
            .map(|command| ToolSettingAction {
                command,
                checkable: command.is_toggle(),
            })
            .collect()
        } else {
            Vec::new()
        };
        self.state.tool_set = tools::view(&self.state.brush, self.layer_interaction.tool);
        self.state.tool_settings = if self.operation.active() {
            self.transform_controls()
        } else if self.layer_interaction.tool == LayerCanvasTool::Paint {
            tool_settings::controls(self.engine.configured_brush())
        } else if let LayerCanvasTool::Figure { paint, .. } = self.layer_interaction.tool {
            tool_settings::controls(self.engine.configured_brush())
                .into_iter()
                .filter(|c| c.id == "opacity" || (c.id == "size" && paint != FigurePaint::Fill))
                .map(|mut c| {
                    if c.id == "size" {
                        c.label = "Line width";
                    }
                    c
                })
                .collect()
        } else if let LayerCanvasTool::Region { fill, .. } = self.layer_interaction.tool {
            let mut controls = self.region_tools.controls();
            if fill {
                controls.extend(
                    tool_settings::controls(self.engine.configured_brush())
                        .into_iter()
                        .filter(|c| c.id == "opacity"),
                );
            }
            controls
        } else if matches!(
            self.layer_interaction.tool,
            LayerCanvasTool::Gradient { .. }
        ) {
            tool_settings::controls(self.engine.configured_brush())
                .into_iter()
                .filter(|c| c.id == "opacity")
                .collect()
        } else {
            Vec::new()
        };
    }

    fn require_idle(&self) -> Result<(), String> {
        if self.input_pending
            || self.engine.has_active_stroke()
            || !self.layer_interaction.path.is_empty()
        {
            Err("Finish the canvas interaction first".into())
        } else {
            Ok(())
        }
    }
    fn sync_camera(&mut self) {
        self.sync_ruler_snapping();
        self.engine.set_view(
            self.state.camera.view(),
            self.state.camera.input_transform(),
        );
    }
    /// Reapply visibility-dependent sampling after a host attaches its GPU.
    pub fn sync_renderer_telemetry(&mut self) {
        self.engine.backend_mut().set_telemetry_enabled(
            (self.state.workspace.layout.active_panel(Panel::Stats) == Some(Panel::Stats)
                && self
                    .state
                    .workspace
                    .layout
                    .panel_group(Panel::Stats)
                    .is_none_or(|g| {
                        self.state
                            .workspace
                            .layout
                            .collapsed_column_for_group(g)
                            .is_none()
                    }))
                || self
                    .state
                    .customization
                    .drawer
                    .iter()
                    .chain(self.state.customization.column_drawers.iter())
                    .any(|d| d.columns.iter().any(|c| c.contains(&Panel::Stats))),
        );
    }
    fn changed(&mut self, regions: u32, canvas_wake: bool) -> UiChange {
        if regions & (regions::LAYOUT | regions::CUSTOMIZATION) != 0 {
            self.sync_renderer_telemetry();
        }
        self.state.theme = self.state.settings.theme.unwrap_or(self.system_theme);
        if regions & regions::SETTINGS != 0 {
            self.state.palette = self
                .state
                .settings
                .palette(self.state.theme, self.state.platform);
        }
        if regions != 0 {
            self.state.revision += 1;
            if regions != regions::CAMERA {
                self.workspace_model_revision = self.state.revision;
                self.workspace_content_revision = self.state.revision;
            }
        }
        UiChange {
            revision: self.state.revision,
            regions,
            canvas_wake,
        }
    }
    fn refresh_commands(&mut self) -> bool {
        let mut changed = false;
        for (index, id) in CommandId::ALL.into_iter().enumerate() {
            let (enabled, selected) = self.command_flags(id);
            let icon = self.command_icon(id);
            let label = if id == CommandId::ResetLayout && self.managed_workspace.is_some() {
                "Restore Starting Layout…"
            } else {
                id.label()
            };
            if let Some(previous) = self.state.commands.get_mut(index) {
                if previous.enabled != enabled
                    || previous.selected != selected
                    || previous.icon != icon
                    || previous.label != label
                {
                    if previous.label != label {
                        previous.tooltip = self.state.settings.action_tooltip(
                            label,
                            &UiAction::Invoke { command: id },
                            self.state.platform,
                        );
                    }
                    previous.enabled = enabled;
                    previous.selected = selected;
                    previous.icon = icon;
                    previous.label = label;
                    changed = true;
                }
            } else {
                self.state.commands.push(self.command(id));
                changed = true;
            }
        }
        changed
    }
    fn refresh_shortcuts(&mut self) {
        // Static presentation data changes with the applied keymap/platform,
        // not every pen event or frame's command-availability update.
        for command in &mut self.state.commands {
            command.tooltip = self.state.settings.action_tooltip(
                command.label,
                &UiAction::Invoke {
                    command: command.id,
                },
                self.state.platform,
            );
            command.bindings = self.state.settings.command_keys(command.id);
            command.shortcut = self.state.settings.action_shortcut(
                &UiAction::Invoke {
                    command: command.id,
                },
                self.state.platform,
            );
        }
    }
    fn refresh_document(&mut self) {
        self.refresh_file_state();
        self.reconcile_transform();
        if self
            .rulers
            .selected
            .is_some_and(|id| !self.engine.document().rulers.iter().any(|r| r.id == id))
            && self.layer_interaction.path.is_empty()
        {
            self.rulers.selected = None;
        }
        let doc = self.engine.document();
        let interaction = &mut self.layer_interaction;
        if interaction.editing != Some(doc.active_layer) {
            interaction.editing = Some(doc.active_layer);
            interaction.selected = std::collections::BTreeSet::from([doc.active_layer]);
        }
        interaction.selected.retain(|id| doc.layer(*id).is_some());
        let layer_state = |l: &layer_core::Layer| LayerState {
            id: l.id.0,
            content_icon: l.effect.as_ref().map(|fx| {
                format!(
                    "layer-{}-symbolic",
                    self.effect_catalog
                        .get(&fx.program.id)
                        .map_or("adjustments", |e| e.icon.as_ref())
                )
            }),
            label: l.name.to_string(),
            editable: l.kind == LayerKind::Paint,
            visible: l.visible,
            opacity: l.opacity,
            selected: self.layer_interaction.selected.contains(&l.id),
            selection_icon: if self.layer_interaction.selected.contains(&l.id)
                && (self.layer_interaction.selected.len() > 1 || l.id != doc.active_layer)
            {
                "layer-selection-checked-symbolic"
            } else if doc.reference_layers.contains(&l.id) {
                "layer-reference-symbolic"
            } else if l.id == doc.active_layer && (l.kind == LayerKind::Paint || doc.active_mask) {
                "layer-brush-symbolic"
            } else {
                "layer-selection-empty-symbolic"
            },
            editing: l.id == doc.active_layer,
            mask_selected: l.id == doc.active_layer && doc.active_mask,
            has_mask: l.mask.is_some(),
            mask_enabled: l.mask.as_ref().is_some_and(|m| m.enabled),
            mask_linked: l.mask.as_ref().is_some_and(|m| m.linked),
            show_mask_area: l.mask.as_ref().is_some_and(|m| m.show_area),
            alpha_locked: l.properties.alpha_locked,
            locked: doc.is_locked(l.id),
            clipped: l.properties.clipped,
            reference: doc.reference_layers.contains(&l.id),
            group: l.kind == LayerKind::Group,
            can_drop_below: l.kind != LayerKind::Background,
            depth: self.layer_interaction.depth(doc, l),
            collapsed: self.layer_interaction.collapsed.contains(&l.id),
            blend: l.properties.blend as u32,
            blend_label: l.properties.blend.label().into(),
            paint_revision: l
                .strokes
                .last()
                .map_or(0, |id| id.0)
                .wrapping_mul(4099)
                .wrapping_add(l.operations.len() as u64 * 2)
                .wrapping_add(u64::from(l.asset.is_some()))
                .wrapping_add(if l.kind == LayerKind::Background {
                    u64::from(l.opacity.to_bits())
                } else {
                    0
                }),
            mask_revision: l.mask.as_ref().map_or(0, |m| {
                m.id.0
                    .wrapping_mul(65537)
                    .wrapping_add(m.strokes.last().map_or(0, |id| id.0) * 2)
                    .wrapping_add(u64::from(m.inverted))
            }),
            mask_id: l.mask.as_ref().map(|m| m.id.0),
        };
        self.state.layer_tools.editing_layer = doc.layer(doc.active_layer).map(&layer_state);
        self.state.layer_properties = effects::properties(doc);
        self.state.layer_tools.controls = doc
            .layer(doc.active_layer)
            .map(|l| art_layers::LayerControls::for_layer(doc, l))
            .unwrap_or_default();
        self.state.layers = doc
            .ordered_layers()
            .into_iter()
            .filter(|l| !self.layer_interaction.hidden_by_group(doc, l))
            .map(layer_state)
            .collect();
        self.state.layer_tools.has_selection = doc.selection.is_some();
        self.state.layer_tools.tool = self.layer_interaction.tool;
        let references = self.reference_selection();
        self.state.layer_tools.can_reference = !references.is_empty();
        self.state.layer_tools.can_delete =
            doc.can_delete_layers(&doc.layer_roots(&self.layer_interaction.selected));
        self.state.layer_tools.references_selected = !references.is_empty()
            && references
                .iter()
                .any(|id| doc.reference_layers.contains(id));
        self.state.layer_tools.reference_action_label = if self.reference_action_removes() {
            "Stop using this layer as a reference"
        } else {
            "Use selected layers as references"
        };
        self.state.tabs = vec![DocumentTab {
            id: doc.id.to_string(),
            title: self.state.document_file.title().into(),
            active: true,
            width: doc.width,
            height: doc.height,
        }];
        self.refresh_tools();
    }
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}
fn valid_viewport(viewport: [f32; 2]) -> Result<(), String> {
    if viewport.into_iter().all(|v| v.is_finite() && v > 0.0) {
        Ok(())
    } else {
        Err("Invalid logical viewport".into())
    }
}
fn pen_phase(phase: ContactPhase) -> PenPhase {
    match phase {
        ContactPhase::Down => PenPhase::Down,
        ContactPhase::Move => PenPhase::Move,
        ContactPhase::Up => PenPhase::Up,
        ContactPhase::Cancel => PenPhase::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{AssetId, Point};
    use layer_engine::{SampleFlags, ToolKind};
    use layer_render::{BackendError, FramePacket, HostImage, ReadbackImage};

    /// Protocol recorder only: no canvas storage or software rasterization.
    #[derive(Default)]
    struct Recorder {
        dabs: usize,
        composites: usize,
        validation: Option<layer_render::EffectValidationRequest>,
        validation_result: Option<layer_render::EffectValidationResult>,
        preview_requests: Vec<Option<u64>>,
        preview_reply: Option<layer_render::CanvasPreview>,
        sample_requests: Vec<layer_render::ColorSampleRequest>,
        sample_reply: Option<layer_render::ColorSample>,
        region_requests: Vec<layer_render::RegionRequest>,
        region_reply: Option<layer_render::RegionResult>,
        transform: Option<layer_render::TransformPreview>,
    }
    impl CanvasRenderer for Recorder {
        type Error = BackendError;
        fn set_transform_preview(
            &mut self,
            preview: Option<&layer_render::TransformPreview>,
        ) -> Result<(), Self::Error> {
            self.transform = preview.cloned();
            Ok(())
        }
        fn request_region(
            &mut self,
            request: layer_render::RegionRequest,
        ) -> Result<bool, Self::Error> {
            self.region_requests.push(request);
            Ok(true)
        }
        fn take_region(&mut self) -> Option<Result<layer_render::RegionResult, Self::Error>> {
            self.region_reply.take().map(Ok)
        }
        fn request_color_sample(
            &mut self,
            request: layer_render::ColorSampleRequest,
        ) -> Result<bool, Self::Error> {
            self.sample_requests.push(request);
            Ok(true)
        }
        fn take_color_sample(&mut self) -> Option<Result<layer_render::ColorSample, Self::Error>> {
            self.sample_reply.take().map(Ok)
        }
        fn request_canvas_preview(&mut self, known: Option<u64>) -> Result<bool, Self::Error> {
            self.preview_requests.push(known);
            Ok(true)
        }
        fn take_canvas_preview(
            &mut self,
        ) -> Option<Result<layer_render::CanvasPreview, Self::Error>> {
            self.preview_reply.take().map(Ok)
        }
        fn request_effect_validation(
            &mut self,
            request: layer_render::EffectValidationRequest,
        ) -> Result<bool, Self::Error> {
            self.validation = Some(request);
            Ok(true)
        }
        fn take_effect_validation(&mut self) -> Option<layer_render::EffectValidationResult> {
            self.validation_result.take()
        }
        fn tip_outline(&self, asset: &AssetId) -> Option<&layer_render::TipOutline> {
            static OUTLINE: std::sync::OnceLock<layer_render::TipOutline> =
                std::sync::OnceLock::new();
            (asset == &AssetId::from("cursor-test")).then(|| {
                OUTLINE.get_or_init(|| {
                    vec![vec![[-1.0, -0.5], [0.5, -0.5], [-1.0, 0.5], [-1.0, -0.5]]]
                })
            })
        }
        fn resize_surface(&mut self, _: u32, _: u32) -> Result<(), Self::Error> {
            Ok(())
        }
        fn prepare_asset(&mut self, _: &AssetId, _: HostImage<'_>) -> Result<(), Self::Error> {
            Ok(())
        }
        fn release_asset(&mut self, _: &AssetId) {}
        fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
            self.dabs += packet.dabs.len();
            self.composites += usize::from(packet.composite_all);
            Ok(())
        }
        fn request_readback(&mut self, _: u64) -> Result<(), Self::Error> {
            Err(BackendError("Recorder has no pixels"))
        }
        fn take_readback(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
            None
        }
    }
    fn session() -> UiSession<Recorder> {
        UiSession::new(
            Recorder::default(),
            Document::new("test", 1000, 1000),
            [1000, 1000],
        )
        .unwrap()
    }

    #[test]
    fn history_preview_never_changes_capture_and_restoration_is_one_undoable_edit() {
        let mut s = session();
        let starting = s.capture_workspace().unwrap();
        for position in [[410., 170.], [520., 220.]] {
            s.dispatch(UiAction::MovePanel {
                panel: Panel::Layers,
                target: DockTarget::Float { position },
                viewport: [1200., 900.],
            })
            .unwrap();
        }
        invoke(&mut s, CommandId::UndoWorkspace);
        s.dispatch(UiAction::SetBrushSize { value: 73. }).unwrap();
        invoke(&mut s, CommandId::ZenMode);
        let original = s.capture_workspace().unwrap();
        assert!(!original.history.redo.is_empty());
        let document = s.engine.document().clone();
        let durable = s.durable_workspace();
        assert!(s.begin_workspace_layout_preview().is_err());
        s.begin_workspace_transition().unwrap();
        s.begin_workspace_layout_preview().unwrap();
        assert!(s.begin_workspace_layout_preview().is_err());
        for layout in [
            starting.history.layout(),
            original.history.layout(),
            starting.history.layout(),
        ] {
            s.preview_workspace_layout(layout).unwrap();
            assert_eq!(durable_layout(&s.state.workspace.layout), *layout);
            assert!(!s.state.workspace.zen_mode);
            assert_eq!(s.capture_workspace().unwrap(), original);
            assert_eq!(s.durable_workspace(), durable);
            assert_eq!(
                s.workspace_layout_generation(),
                Some(original.history.generation)
            );
        }
        assert!(s.dispatch(UiAction::SetBrushSize { value: 99. }).is_err());
        assert!(
            s.restore_workspace_layout(starting.history.layout().clone(), "Restored layout")
                .is_err()
        );
        assert!(
            s.adopt_workspace(PreparedWorkspace::new(starting.clone()).unwrap())
                .is_err()
        );
        s.cancel_workspace_layout_preview();
        s.end_workspace_transition();
        assert_eq!(s.capture_workspace().unwrap(), original);
        assert!(s.state.workspace.zen_mode);
        s.restore_workspace_layout(starting.history.layout().clone(), "Restored layout")
            .unwrap();
        let restored = s.capture_workspace().unwrap();
        assert_eq!(
            restored.history.revisions.len(),
            original.history.revisions.len() + 1
        );
        assert_eq!(restored.history.generation, original.history.generation + 1);
        assert_eq!(restored.working, original.working);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(
            durable_layout(&s.state.workspace.layout),
            *original.history.layout()
        );
        assert_eq!(s.engine.document(), &document);
    }

    #[test]
    fn history_names_affected_panels_and_toolbars() {
        let mut s = session();
        s.dispatch(UiAction::MovePanel {
            panel: Panel::Layers,
            target: DockTarget::Float {
                position: [410., 170.],
            },
            viewport: [1200., 900.],
        })
        .unwrap();
        let history = s.capture_workspace().unwrap().history;
        assert_eq!(
            history.revisions[&history.current].description,
            "Moved Layers panel"
        );
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::SetPanelVisible {
                panel: Panel::Layers,
                visible: false,
            },
        })
        .unwrap();
        let history = s.capture_workspace().unwrap().history;
        assert_eq!(
            history.revisions[&history.current].description,
            "Hid Layers panel"
        );
        let mut toolbar = s
            .state
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .clone();
        if let PanelContent::Toolbar { name, .. } = &mut toolbar.content {
            *name = "Inking".into();
        }
        let (panel, _) = s.install_workspace_toolbar(toolbar, None, None).unwrap();
        let history = s.capture_workspace().unwrap().history;
        assert_eq!(
            history.revisions[&history.current].description,
            "Added Inking toolbar"
        );
        let mut after = history.layout().clone();
        after.panels.retain(|p| p.id != panel);
        assert_eq!(
            layout_change_description(history.layout(), &after),
            "Deleted Inking toolbar"
        );
        let before_settings = s.capture_workspace().unwrap().history;
        s.dispatch(UiAction::SetBrushSize { value: 82. }).unwrap();
        assert_eq!(s.capture_workspace().unwrap().history, before_settings);
    }

    #[test]
    fn managed_window_menu_saves_workspaces_without_separate_layout_controls() {
        let mut s = session();
        s.configure_workspace_manager(ManagedWorkspace {
            id: "test".into(),
            name: "Painting".into(),
            baseline: durable_layout(&s.state.workspace.layout),
            choices: Vec::new(),
        })
        .unwrap();
        let menu = s.workspace_menu();
        assert!(matches!(
            menu.sections[0][0].action,
            Some(UiAction::Invoke {
                command: CommandId::UndoWorkspace
            })
        ));
        assert!(matches!(
            menu.sections[0][1].action,
            Some(UiAction::Invoke {
                command: CommandId::RedoWorkspace
            })
        ));
        assert_eq!(menu.sections[1][0].label, "Workspaces");
        assert!(
            menu.sections[1][0]
                .sections
                .iter()
                .flatten()
                .any(|i| i.label == "Reset All Brushes…")
        );
        assert!(menu.sections[2].iter().any(|i| i.label == "Layers"));
        assert!(
            menu.sections[2]
                .iter()
                .all(|i| !i.label.ends_with(" panel"))
        );
        assert_eq!(menu.sections.len(), 3);
        let workspaces = &menu.sections[1][0];
        assert_eq!(
            workspaces
                .sections
                .iter()
                .skip(1)
                .map(|section| {
                    section
                        .iter()
                        .map(|item| item.label.as_str())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
            vec![
                vec!["New Workspace…", "Manage Workspaces…"],
                vec!["Layout History…", "Restore Starting Layout…"],
                vec!["Reset All Brushes…"],
            ]
        );
        assert!(
            s.command(CommandId::ResetLayout)
                .tooltip
                .contains("Restore Starting Layout")
        );
        assert!(workspaces.sections.iter().flatten().all(|item| !matches!(
            item.action,
            Some(UiAction::WorkspaceManager {
                command: WorkspaceCommand::SaveAsTemplate | WorkspaceCommand::ManageTemplates
            })
        )));
        assert_eq!(menu.sections[1].len(), 2);
        let toolbars = &menu.sections[1][1];
        assert_eq!(toolbars.label, "Quick Access Toolbars");
        assert!(
            toolbars.sections[0]
                .iter()
                .all(|i| !i.label.ends_with(" toolbar"))
        );
        assert_eq!(toolbars.sections[1][0].label, "New Toolbar…");
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            s.set_platform(platform);
            let context = s
                .context_menu(ContextTarget::Panel {
                    panel: Panel::Toolbar,
                })
                .unwrap();
            assert_eq!(
                context.sections.iter().flatten().any(|item| matches!(
                    item.action,
                    Some(UiAction::WorkspaceManager {
                        command: WorkspaceCommand::SaveToolbar { .. }
                    })
                )),
                platform == Platform::Gtk
            );
        }
    }

    #[test]
    fn read_only_workspace_rejects_new_contacts_and_keys_but_finishes_existing_ink() {
        let mut s = session();
        let before = s.engine.document().clone();
        s.set_workspace_read_only(true);
        s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
        s.pen(event(&s, 2, PenPhase::Move, 1.)).unwrap();
        s.pen(event(&s, 3, PenPhase::Up, 1.)).unwrap();
        s.frame(4, 4).unwrap();
        assert_eq!(s.engine.document(), &before);
        let reply = s
            .input(UiInput::Key {
                key: "b".into(),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
                editing: false,
                divider: None,
            })
            .unwrap();
        assert!(reply.handled && s.interaction.keys.is_empty());
        s.set_workspace_read_only(false);
        s.pen(event(&s, 5, PenPhase::Down, 1.)).unwrap();
        s.frame(6, 6).unwrap();
        s.set_workspace_read_only(true);
        s.pen(event(&s, 7, PenPhase::Up, 1.)).unwrap();
        s.frame(8, 8).unwrap();
        assert_ne!(s.engine.document(), &before);
        s.require_workspace_idle().unwrap();
        s.set_workspace_read_only(false);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layers, before.layers);
    }

    #[test]
    fn pending_workspace_adoption_blocks_new_input_without_touching_artwork() {
        let mut s = session();
        let capture = s.capture_workspace().unwrap();
        let before = s.engine.document().clone();
        s.begin_workspace_transition().unwrap();
        assert!(s.begin_workspace_transition().is_err());
        assert!(s.dispatch(UiAction::SetBrushSize { value: 50. }).is_err());
        s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
        s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
        s.frame(3, 3).unwrap();
        assert_eq!(s.engine.document(), &before);
        s.adopt_workspace(PreparedWorkspace::new(capture).unwrap())
            .unwrap();
        s.end_workspace_transition();
        s.dispatch(UiAction::SetBrushSize { value: 50. }).unwrap();
        assert_eq!(s.state.brush.diameter, 50.);
        assert_eq!(s.engine.document(), &before);
    }

    #[test]
    fn reset_all_workspace_brushes_preserves_layout_color_tool_and_document() {
        let mut s = session();
        s.dispatch(UiAction::SetBrushSize { value: 73. }).unwrap();
        invoke(&mut s, CommandId::Eraser);
        s.dispatch(UiAction::SetBrushOpacity { value: 0.35 })
            .unwrap();
        s.dispatch(UiAction::SetColor {
            rgba: [0.2, 0.4, 0.6, 1.],
        })
        .unwrap();
        invoke(&mut s, CommandId::Move);
        let before = s.capture_workspace().unwrap();
        let document = s.engine.document().clone();
        assert_eq!(before.working.tools.overrides.len(), 2);
        s.reset_workspace_brushes().unwrap();
        let after = s.capture_workspace().unwrap();
        assert!(after.working.tools.overrides.is_empty());
        assert_eq!(after.history, before.history);
        assert_eq!(after.working.colors, before.working.colors);
        assert_eq!(after.working.preset, before.working.preset);
        assert_eq!(after.working.canvas_tool, before.working.canvas_tool);
        assert_eq!(s.engine.document(), &document);
        assert_eq!(
            s.state.brush.opacity,
            default_brush(DefaultBrushPreset::Eraser).opacity
        );
        invoke(&mut s, CommandId::Pen);
        assert_eq!(
            s.state.brush.diameter,
            default_brush(DefaultBrushPreset::GPen).diameter
        );
        let unchanged = s.capture_workspace().unwrap();
        s.reset_workspace_brushes().unwrap();
        assert_eq!(s.capture_workspace().unwrap(), unchanged);
        s.begin_workspace_transition().unwrap();
        s.begin_workspace_layout_preview().unwrap();
        assert!(s.reset_workspace_brushes().is_err());
        s.cancel_workspace_layout_preview();
        s.end_workspace_transition();
    }

    #[test]
    fn library_copies_remap_tiles_and_full_reset_recovers_toolbar_customization() {
        let mut s = session();
        let initial = s.capture_workspace().unwrap();
        let baseline = initial.history.layout().clone();
        let source = baseline
            .panels
            .iter()
            .find(|p| p.id.kind() == PanelKind::Tiles && !p.tiles().is_empty())
            .unwrap()
            .clone();
        let mut library = source.clone();
        if let PanelContent::Toolbar { name, tiles } = &mut library.content {
            *name = "Library Tools".into();
            tiles[0].control = ToolbarControl::Size { pixels: 20 };
        }
        s.dispatch(UiAction::SetBrushSize { value: 73. }).unwrap();
        invoke(&mut s, CommandId::ZenMode);
        let working = s.workspace_working_state();
        let (copy, _) = s
            .install_workspace_toolbar(library.clone(), None, None)
            .unwrap();
        assert_ne!(copy, source.id);
        assert_eq!(s.state.workspace.layout.panel(source.id).unwrap(), &source);
        let installed = s.state.workspace.layout.panel(copy).unwrap();
        assert_eq!(
            installed.tiles()[0].control,
            ToolbarControl::Size { pixels: 20 }
        );
        assert!(
            installed
                .tiles()
                .iter()
                .all(|tile| source.tiles().iter().all(|old| old.id != tile.id))
        );
        if let PanelContent::Toolbar { tiles, .. } = &mut library.content {
            tiles[0].control = ToolbarControl::Size { pixels: 30 };
        }
        let placement = s.state.workspace.layout.panel_group(copy);
        s.install_workspace_toolbar(library, Some(copy), None)
            .unwrap();
        assert_eq!(s.state.workspace.layout.panel_group(copy), placement);
        let customized = durable_layout(&s.state.workspace.layout);
        assert_eq!(s.workspace_working_state(), working);
        s.restore_workspace_layout(baseline.clone(), "Reset to original layout")
            .unwrap();
        assert_eq!(durable_layout(&s.state.workspace.layout), baseline);
        assert_eq!(s.workspace_working_state(), working);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(durable_layout(&s.state.workspace.layout), customized);
        assert_eq!(s.workspace_working_state(), working);
        assert_eq!(s.capture_workspace().unwrap().history.revisions.len(), 4);
    }

    #[test]
    fn host_acknowledgements_finish_during_workspace_transition_and_ownership_recovery() {
        let mut s = session();
        for read_only in [false, true] {
            s.set_workspace_read_only(false);
            s.dispatch(UiAction::SetTheme {
                theme: Some(Theme::Dark),
            })
            .unwrap();
            let request = s
                .state
                .requests
                .iter()
                .find(|r| matches!(r.kind, HostRequestKind::SaveSettings { .. }))
                .unwrap()
                .id;
            let artwork = s.engine.document().clone();
            if read_only {
                s.set_workspace_read_only(true);
            } else {
                s.begin_workspace_transition().unwrap();
            }
            s.dispatch(UiAction::CompleteRequest {
                id: request,
                error: None,
            })
            .unwrap();
            assert!(!s.state.requests.iter().any(|r| r.id == request));
            let workspace = s.capture_workspace().unwrap();
            let mut settings = s.state.settings.clone();
            settings.theme = Some(Theme::Light);
            s.dispatch(UiAction::RestoreSettings { settings }).unwrap();
            assert_eq!(s.state.settings.theme, Some(Theme::Light));
            assert_eq!(s.capture_workspace().unwrap(), workspace);
            assert!(s.dispatch(UiAction::SetBrushSize { value: 79. }).is_err());
            assert_eq!(s.engine.document(), &artwork);
            s.end_workspace_transition();
        }
    }

    #[test]
    fn replacing_a_toolbar_with_an_empty_library_entry_preserves_placement_and_undo() {
        let mut s = session();
        let before = s.capture_workspace().unwrap();
        let source = before
            .history
            .layout()
            .panels
            .iter()
            .find(|p| p.id.kind() == PanelKind::Tiles && !p.tiles().is_empty())
            .unwrap()
            .clone();
        let placement = s.state.workspace.layout.panel_group(source.id);
        let mut empty = source.clone();
        if let PanelContent::Toolbar { name, tiles } = &mut empty.content {
            *name = "Empty Library Toolbar".into();
            tiles.clear();
        }
        let artwork = s.engine.document().clone();
        s.install_workspace_toolbar(empty, Some(source.id), None)
            .unwrap();
        assert!(
            s.state
                .workspace
                .layout
                .panel(source.id)
                .unwrap()
                .tiles()
                .is_empty()
        );
        assert_eq!(s.state.workspace.layout.panel_group(source.id), placement);
        assert_eq!(s.workspace_working_state(), before.working);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(s.state.workspace.layout.panel(source.id).unwrap(), &source);
        invoke(&mut s, CommandId::RedoWorkspace);
        assert!(
            s.state
                .workspace
                .layout
                .panel(source.id)
                .unwrap()
                .tiles()
                .is_empty()
        );
        assert_eq!(s.workspace_working_state(), before.working);
        assert_eq!(s.engine.document(), &artwork);
    }

    #[test]
    fn durable_workspace_history_resumes_redo_and_keeps_abandoned_revisions() {
        let mut s = session();
        let original = s.state.workspace.layout.clone();
        s.dispatch(UiAction::MovePanel {
            panel: Panel::Sizes,
            target: DockTarget::Float {
                position: [400., 200.],
            },
            viewport: [1200., 900.],
        })
        .unwrap();
        let moved = s.state.workspace.layout.clone();
        s.dispatch(UiAction::SetBrushSize { value: 83. }).unwrap();
        invoke(&mut s, CommandId::ZenMode);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(s.state.workspace.layout, original);
        assert!(s.state.workspace.zen_mode);
        assert_eq!(s.state.brush.diameter, 83.);
        let captured = s.capture_workspace().unwrap();
        assert_eq!(captured.history.revisions.len(), 2);
        let encoded = serde_json::to_string(&captured).unwrap();
        let mut reopened = session();
        reopened
            .adopt_workspace(
                PreparedWorkspace::new(serde_json::from_str(&encoded).unwrap()).unwrap(),
            )
            .unwrap();
        let document = reopened.engine.document().clone();
        invoke(&mut reopened, CommandId::RedoWorkspace);
        assert_eq!(reopened.state.workspace.layout, moved);
        assert!(reopened.state.workspace.zen_mode);
        assert_eq!(reopened.state.brush.diameter, 83.);
        invoke(&mut reopened, CommandId::UndoWorkspace);
        reopened
            .dispatch(UiAction::MovePanel {
                panel: Panel::Sizes,
                target: DockTarget::Float {
                    position: [600., 300.],
                },
                viewport: [1200., 900.],
            })
            .unwrap();
        let recovered = reopened.capture_workspace().unwrap();
        assert!(recovered.history.redo.is_empty());
        assert!(
            recovered
                .history
                .revisions
                .values()
                .any(|r| r.layout == moved)
        );
        assert_eq!(reopened.engine.document(), &document);
        assert!(!reopened.engine.can_undo());
    }

    #[test]
    fn layout_reset_and_switch_keep_independent_working_values_and_document_history() {
        let mut s = session();
        let baseline = s.state.workspace.layout.clone();
        invoke(&mut s, CommandId::AddLayer);
        let document = s.engine.document().clone();
        let blank = s.capture_workspace().unwrap();
        s.dispatch(UiAction::SetBrushSize { value: 73. }).unwrap();
        s.dispatch(UiAction::SetToolSetting {
            id: "flow".into(),
            value: 0.32,
        })
        .unwrap();
        s.dispatch(UiAction::SetColor {
            rgba: [0.1, 0.2, 0.3, 1.],
        })
        .unwrap();
        invoke(&mut s, CommandId::ZenMode);
        assert_eq!(s.capture_workspace().unwrap().history.revisions.len(), 1);
        s.dispatch(UiAction::MovePanel {
            panel: Panel::Sizes,
            target: DockTarget::Float {
                position: [400., 200.],
            },
            viewport: [1200., 900.],
        })
        .unwrap();
        let working = s.workspace_working_state();
        let painting = s.capture_workspace().unwrap();
        s.restore_workspace_layout(baseline.clone(), "Reset layout")
            .unwrap();
        assert_eq!(s.state.workspace.layout, baseline);
        assert_eq!(s.workspace_working_state(), working);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(&s.state.workspace.layout, painting.history.layout());
        s.adopt_workspace(PreparedWorkspace::new(blank.clone()).unwrap())
            .unwrap();
        assert_eq!(s.workspace_working_state(), blank.working);
        assert!(!s.command(CommandId::UndoWorkspace).enabled);
        s.adopt_workspace(PreparedWorkspace::new(painting.clone()).unwrap())
            .unwrap();
        assert_eq!(s.workspace_working_state(), working);
        assert_eq!(s.engine.document(), &document);
        assert!(s.engine.can_undo());
        assert_eq!(s.engine.configured_brush().flow, 0.32);
    }

    #[test]
    fn sparse_overrides_only_reset_the_setting_explicitly_edited() {
        let mut s = session();
        let id = s.state.brush.preset;
        let defaults = layer_core::default_brush(preset(id).unwrap());
        let mut saved = s.capture_workspace().unwrap();
        // A previously explicit value can become equal to an updated default.
        saved
            .working
            .tools
            .overrides
            .insert(id, [("flow".into(), defaults.flow)].into());
        s.adopt_workspace(PreparedWorkspace::new(saved).unwrap())
            .unwrap();
        s.dispatch(UiAction::SetBrushSize { value: 91. }).unwrap();
        assert_eq!(
            s.workspace_working_state().tools.overrides[&id]["flow"],
            defaults.flow
        );
        s.dispatch(UiAction::SelectBrush {
            id: Tool::Eraser.default_preset(),
        })
        .unwrap();
        s.dispatch(UiAction::SelectBrush { id }).unwrap();
        assert_eq!(s.state.brush.diameter, 91.);
        s.dispatch(UiAction::SetToolSetting {
            id: "flow".into(),
            value: defaults.flow,
        })
        .unwrap();
        assert!(!s.workspace_working_state().tools.overrides[&id].contains_key("flow"));
        s.dispatch(UiAction::SetBrushSize {
            value: defaults.diameter,
        })
        .unwrap();
        assert!(s.workspace_working_state().tools.overrides.is_empty());
        assert_eq!(s.capture_workspace().unwrap().history.revisions.len(), 1);
    }

    #[test]
    fn workspace_adoption_rejects_invalid_state_and_active_artwork_without_side_effects() {
        let mut s = session();
        let captured = s.capture_workspace().unwrap();
        let mut invalid = captured.clone();
        invalid
            .working
            .tools
            .overrides
            .insert(0, [("future_setting".into(), 0.2)].into());
        assert!(PreparedWorkspace::new(invalid).is_err());
        let prepared = PreparedWorkspace::new(captured.clone()).unwrap();
        s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
        let document = s.engine.document().clone();
        let working = s.workspace_working_state();
        assert!(s.adopt_workspace(prepared).is_err());
        assert_eq!(s.engine.document(), &document);
        assert_eq!(s.workspace_working_state(), working);
        s.pen(event(&s, 2, PenPhase::Cancel, 0.)).unwrap();
        s.frame(3, 3).unwrap();
        s.workspace_history.begin(&s.state.workspace);
        assert!(s.capture_workspace().is_err());
        assert!(
            s.adopt_workspace(PreparedWorkspace::new(captured).unwrap())
                .is_err()
        );
        s.workspace_history.cancel(&mut s.state.workspace);
    }

    #[test]
    fn document_save_keeps_source_assets_and_tracks_the_saved_undo_state() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let pixels = std::sync::Arc::<[u8]>::from([200, 20, 90, 128]);
        s.import_layer_asset(
            "Image",
            layer_core::ProjectAsset {
                extent: [1, 1],
                format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                bytes: pixels.clone(),
            },
        )
        .unwrap();
        assert!(s.state.document_file.modified);
        invoke(&mut s, CommandId::SaveDocument);
        let id = s.files.pending.as_ref().unwrap().0;
        assert!(s.complete_document_request(id, Ok(true)).is_err());
        let location = DocumentLocation {
            uri: "file:///drawing.capy".into(),
            name: "drawing.capy".into(),
        };
        let project = s
            .capture_project_save(id, location.clone())
            .unwrap()
            .pruned()
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &project.assets.values().next().unwrap().bytes,
            &pixels
        ));
        assert!(s.capture_project_save(id, location.clone()).is_err());
        // The writer holds its snapshot while drawing/editing continues.
        invoke(&mut s, CommandId::AddLayer);
        s.complete_document_request(id, Ok(true)).unwrap();
        assert_eq!(s.state.document_file.location, Some(location));
        assert_eq!(s.state.tabs[0].title, "drawing.capy");
        assert!(s.state.document_file.modified);
        invoke(&mut s, CommandId::Undo);
        assert!(!s.state.document_file.modified);
        invoke(&mut s, CommandId::Redo);
        assert!(s.state.document_file.modified);
        let mut stream = Vec::new();
        project.write(&mut stream).unwrap();
        let decoded = layer_core::Project::read(stream.as_slice(), Default::default()).unwrap();
        let reopened = UiSession::from_project(
            Recorder::default(),
            decoded,
            s.state.document_file.location.clone(),
            [800, 600],
        )
        .unwrap();
        assert!(!reopened.state.document_file.modified);
        assert_eq!(reopened.files.assets, project.assets);
        assert_eq!(reopened.engine.document(), &project.document);
    }

    #[test]
    fn in_place_new_and_open_share_unsaved_and_save_completion_policy() {
        for platform in [Platform::Ios, Platform::Mac] {
            let mut s = session();
            s.set_platform(platform);
            s.set_document_replacement(true);
            invoke(&mut s, CommandId::AddLayer);
            invoke(&mut s, CommandId::NewDocument);
            let id = s.files.pending.as_ref().unwrap().0;
            s.respond_document_close(id, CloseDecision::Cancel).unwrap();
            assert!(s.files.pending.is_none());
            assert!(s.state.document_file.modified);
            invoke(&mut s, CommandId::OpenDocument);
            let id = s.files.pending.as_ref().unwrap().0;
            s.respond_document_close(id, CloseDecision::Save).unwrap();
            let id = s.files.pending.as_ref().unwrap().0;
            s.capture_project_save(
                id,
                DocumentLocation {
                    uri: "file:///fixture.capy".into(),
                    name: "fixture.capy".into(),
                },
            )
            .unwrap();
            s.complete_document_request(id, Ok(true)).unwrap();
            assert!(!s.state.document_file.modified);
            assert!(!s.state.document_file.close_ready);
            assert!(matches!(
                s.state.requests.last().unwrap().kind,
                HostRequestKind::Document {
                    request: DocumentRequest::Open
                }
            ));
            let id = s.files.pending.as_ref().unwrap().0;
            s.complete_document_request(id, Ok(false)).unwrap();
            invoke(&mut s, CommandId::AddLayer);
            invoke(&mut s, CommandId::NewDocument);
            let id = s.files.pending.as_ref().unwrap().0;
            s.respond_document_close(id, CloseDecision::Discard)
                .unwrap();
            assert!(
                s.state.document_file.modified,
                "The host has not replaced the document yet"
            );
            assert!(matches!(
                s.state.requests.last().unwrap().kind,
                HostRequestKind::Document {
                    request: DocumentRequest::New
                }
            ));
        }
    }

    #[test]
    fn close_save_cancel_failure_and_concurrent_edits_never_discard_work() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::AddLayer);
        let location = DocumentLocation {
            uri: "file:///drawing.capy".into(),
            name: "drawing.capy".into(),
        };
        for outcome in [Ok(false), Err("Disk full".into())] {
            s.request_document_close().unwrap();
            let id = s.files.pending.as_ref().unwrap().0;
            s.respond_document_close(id, CloseDecision::Save).unwrap();
            let id = s.files.pending.as_ref().unwrap().0;
            s.capture_project_save(id, location.clone()).unwrap();
            s.complete_document_request(id, outcome).unwrap();
            assert!(s.state.document_file.modified);
            assert!(!s.state.document_file.close_ready);
            assert!(!s.state.document_file.busy);
        }
        s.request_document_close().unwrap();
        let id = s.files.pending.as_ref().unwrap().0;
        s.respond_document_close(id, CloseDecision::Cancel).unwrap();
        assert!(!s.state.document_file.close_ready);
        invoke(&mut s, CommandId::SaveDocument);
        let id = s.files.pending.as_ref().unwrap().0;
        s.capture_project_save(id, location.clone()).unwrap();
        s.request_document_close().unwrap(); // do not interrupt the write
        invoke(&mut s, CommandId::AddLayer);
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(s.state.document_file.modified);
        assert!(!s.state.document_file.close_ready);
        let id = s.files.pending.as_ref().unwrap().0;
        assert!(matches!(
            s.state.requests.last().unwrap().kind,
            HostRequestKind::Document {
                request: DocumentRequest::ConfirmClose { .. }
            }
        ));
        s.respond_document_close(id, CloseDecision::Save).unwrap();
        let id = s.files.pending.as_ref().unwrap().0;
        s.capture_project_save(id, location).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(s.state.document_file.close_ready);
        assert!(!s.state.document_file.modified);
    }

    #[test]
    fn file_requests_are_single_flight_and_new_open_do_not_replace_artwork() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::AddLayer);
        let document = s.engine.document().clone();
        for command in [
            CommandId::NewDocument,
            CommandId::OpenDocument,
            CommandId::ExportDocument,
        ] {
            invoke(&mut s, command);
            let id = s.files.pending.as_ref().unwrap().0;
            assert!(s.request_save(false).is_err());
            assert!(
                s.dispatch(UiAction::CompleteRequest { id, error: None })
                    .is_err()
            );
            s.complete_document_request(id, Ok(true)).unwrap();
            assert_eq!(s.engine.document(), &document);
            assert!(s.state.document_file.modified);
            assert!(s.complete_document_request(id, Ok(true)).is_err());
        }
        s.request_document_close().unwrap();
        let id = s.files.pending.as_ref().unwrap().0;
        s.respond_document_close(id, CloseDecision::Discard)
            .unwrap();
        assert!(s.state.document_file.close_ready);
        assert_eq!(s.engine.document(), &document);
        assert!(new_drawing(0, 10).is_err());
        assert!(new_drawing(10, 8193).is_err());
        assert_eq!(new_drawing(512, 128).unwrap().document.width, 512);
    }

    #[test]
    fn save_completion_during_a_live_stroke_defers_close_until_pen_up() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::AddLayer);
        invoke(&mut s, CommandId::SaveDocument);
        let id = s.state.requests.last().unwrap().id;
        s.capture_project_save(
            id,
            DocumentLocation {
                uri: "file:///live.capy".into(),
                name: "live.capy".into(),
            },
        )
        .unwrap();
        s.request_document_close().unwrap();
        s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
        s.frame(10_000_000, 18_000_000).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(!s.state.document_file.close_ready);
        assert!(s.state.requests.is_empty());
        s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
        s.frame(20_000_000, 28_000_000).unwrap();
        assert!(s.state.document_file.modified);
        assert!(matches!(
            s.state.requests.last().unwrap().kind,
            HostRequestKind::Document {
                request: DocumentRequest::ConfirmClose { .. }
            }
        ));
    }

    #[test]
    fn navigator_drags_without_jump_and_outside_click_recenters() {
        let mut s = UiSession::new(
            Recorder::default(),
            Document::new("test", 2048, 1536),
            [1000, 1000],
        )
        .unwrap();
        s.state.camera.work_area = [200.0, 50.0, 400.0, 300.0];
        s.state.camera.zoom = 2.0;
        s.state.camera.rotation = 0.3;
        s.state.camera.center_on([1000.0, 700.0]);
        let doc_revision = s.engine.document().revision;
        for flipped in [[false, false], [true, false], [false, true], [true, true]] {
            s.state.camera.flipped = flipped;
            s.state.camera.center_on([1000.0, 700.0]);
            let doc = s.engine.document();
            let g =
                NavigatorGeometry::new(&s.state.camera, [doc.width, doc.height], [264.0, 200.0])
                    .unwrap();
            let middle = [
                g.work_area.iter().map(|p| p[0]).sum::<f32>() * 0.25 + 2.0,
                g.work_area.iter().map(|p| p[1]).sum::<f32>() * 0.25,
            ];
            let action = |phase, position| UiAction::Navigator {
                phase,
                position,
                viewport: [264.0, 200.0],
            };
            let before = s.state.camera.translation;
            s.dispatch(action(ContactPhase::Down, middle)).unwrap();
            assert!(
                before
                    .into_iter()
                    .zip(s.state.camera.translation)
                    .all(|(a, b)| (a - b).abs() < 0.001)
            );
            let moved = [middle[0] + 16.0, middle[1] + 8.0];
            s.dispatch(action(ContactPhase::Move, moved)).unwrap();
            let [x, y] = s.state.camera.work_area_center();
            let center = s
                .state
                .camera
                .input_transform()
                .map(layer_core::Point { x, y });
            assert!((center.x - 1128.0).abs() < 0.001 && (center.y - 764.0).abs() < 0.001);
            s.dispatch(action(ContactPhase::Up, moved)).unwrap();
            let before = s.state.camera.clone();
            s.dispatch(action(ContactPhase::Move, middle)).unwrap();
            assert_eq!(s.state.camera, before);
            let point = [g.image.x + 5.0, g.image.y + 5.0];
            s.dispatch(action(ContactPhase::Down, point)).unwrap();
            let center = s
                .state
                .camera
                .input_transform()
                .map(layer_core::Point { x, y });
            let expected = g.document_point(point);
            assert!(
                (center.x - expected[0]).abs() < 0.001 && (center.y - expected[1]).abs() < 0.001
            );
            s.dispatch(action(ContactPhase::Cancel, point)).unwrap();
        }
        for id in [
            CommandId::ZoomIn,
            CommandId::ZoomOut,
            CommandId::RotateLeft,
            CommandId::RotateRight,
            CommandId::FlipHorizontal,
            CommandId::FlipVertical,
        ] {
            let change = invoke(&mut s, id);
            assert!(change.canvas_wake && change.regions & regions::CAMERA != 0);
        }
        assert_eq!(s.engine.document().revision, doc_revision);
    }

    #[test]
    fn connected_tools_share_sources_history_and_stale_reply_policy() {
        use layer_core::{Edit, Selection, SelectionPixels};
        use layer_render::{RegionResult, RegionSource};
        use std::sync::Arc;
        let coverage =
            Arc::new(SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![0x44444444]).unwrap());
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            let send = |s: &mut UiSession<Recorder>, phase| {
                let mut e = event(s, 10, phase, 1.);
                let m = s.state.camera.document_to_surface();
                e.surface_position = Point {
                    x: m[0] * 48. + m[2] * 72. + m[4],
                    y: m[1] * 48. + m[3] * 72. + m[5],
                };
                s.pen(e).unwrap();
            };
            invoke(&mut s, CommandId::AutoSelect);
            assert_eq!(s.state.tool_set.subtools.len(), 3);
            assert_eq!(s.state.tool_settings[0].id, "tolerance");
            assert_eq!(s.state.tool_settings.len(), 4);
            assert_eq!(s.region_tools.refinement.smoothing, 1.);
            for (id, value) in [("gap_closing", 3.), ("expansion", -2.), ("smoothing", 0.75)] {
                s.dispatch(UiAction::SetToolSetting {
                    id: id.into(),
                    value,
                })
                .unwrap();
                assert_eq!(
                    s.state
                        .tool_settings
                        .iter()
                        .find(|c| c.id == id)
                        .unwrap()
                        .value,
                    value
                );
            }
            let refinement = s.region_tools.refinement;
            for (id, value) in [
                ("gap_closing", 33.),
                ("expansion", -33.),
                ("gap_closing", 1.5),
                ("smoothing", f32::NAN),
            ] {
                assert!(
                    s.dispatch(UiAction::SetToolSetting {
                        id: id.into(),
                        value
                    })
                    .is_err()
                );
                assert_eq!(s.region_tools.refinement, refinement);
            }
            send(&mut s, PenPhase::Down);
            send(&mut s, PenPhase::Up);
            assert!(s.wants_continuous_frames());
            s.frame(1, 1).unwrap();
            let request = s.renderer_mut().region_requests[0].clone();
            assert_eq!(request.position, [48, 72]);
            assert_eq!(request.refinement, refinement);
            assert_eq!(request.source, RegionSource::Composite);
            assert!(request.limit.is_none());
            s.renderer_mut().region_reply = Some(RegionResult {
                request_id: request.request_id,
                pixels: coverage.clone(),
            });
            assert!(
                s.frame(2, 2).unwrap().canvas_wake,
                "reply schedules the frame that displays the selection"
            );
            assert_eq!(
                s.engine.document().selection,
                Some(Selection::pixels(coverage.clone()))
            );
            s.frame(3, 3).unwrap();
            invoke(&mut s, CommandId::Undo);
            assert!(s.engine.document().selection.is_none());
            invoke(&mut s, CommandId::Redo);
            assert!(s.engine.document().selection.is_some());

            let id = s.engine.document().active_layer;
            let mut layer = s.engine.document().layer(id).unwrap().clone();
            layer.properties.offset = Point { x: 8., y: 12. };
            s.layer_edit(Edit::ReplaceLayer(Box::new(layer))).unwrap();
            s.frame(4, 4).unwrap();
            invoke(&mut s, CommandId::Fill);
            let source = s.state.tool_set.subtools[1].action.clone();
            s.dispatch(source).unwrap();
            s.dispatch(UiAction::SetToolSetting {
                id: "tolerance".into(),
                value: 0.2,
            })
            .unwrap();
            send(&mut s, PenPhase::Down);
            send(&mut s, PenPhase::Up);
            s.frame(5, 5).unwrap();
            let request = s.renderer_mut().region_requests.last().unwrap().clone();
            assert_eq!(request.position, [40, 60]);
            assert_eq!(request.source, RegionSource::Layer(id));
            assert_eq!(request.tolerance, 0.2);
            assert_eq!(
                request.refinement, refinement,
                "Fill uses the same Rust settings"
            );
            assert_eq!(
                request.limit.as_ref().unwrap().affine,
                layer_core::Affine::translation(Point { x: -8., y: -12. })
            );
            s.renderer_mut().region_reply = Some(RegionResult {
                request_id: request.request_id,
                pixels: coverage.clone(),
            });
            assert!(s.frame(6, 6).unwrap().canvas_wake);
            let operation = &s.engine.document().layer(id).unwrap().operations[0];
            assert_eq!(
                operation.coverage.initial,
                Some(Selection::pixels(coverage.clone())),
                "raw-layer result is converted to document then layer coordinates once"
            );
            s.frame(7, 7).unwrap();
            invoke(&mut s, CommandId::Undo);
            assert!(s.engine.document().layer(id).unwrap().operations.is_empty());

            // Esc cancels the gesture/request; changing tools also rejects an
            // already submitted reply. Neither creates an empty history entry.
            invoke(&mut s, CommandId::AutoSelect);
            send(&mut s, PenPhase::Down);
            assert!(s.cancel_layer_gesture().unwrap());
            send(&mut s, PenPhase::Up);
            s.frame(8, 8).unwrap();
            assert_eq!(s.renderer_mut().region_requests.len(), 2);
            send(&mut s, PenPhase::Down);
            send(&mut s, PenPhase::Up);
            s.frame(9, 9).unwrap();
            let request = s.renderer_mut().region_requests.last().unwrap().clone();
            let before = s.engine.document().selection.clone();
            invoke(&mut s, CommandId::Hand);
            s.renderer_mut().region_reply = Some(RegionResult {
                request_id: request.request_id,
                pixels: coverage.clone(),
            });
            s.frame(10, 10).unwrap();
            assert_eq!(s.engine.document().selection, before);
            assert!(!s.region_tools.busy());

            invoke(&mut s, CommandId::AutoSelect);
            s.dispatch(s.state.tool_set.subtools[2].action.clone())
                .unwrap();
            send(&mut s, PenPhase::Down);
            send(&mut s, PenPhase::Up);
            assert!(s.frame(11, 11).unwrap_err().contains("reference layer"));
            s.layer_edit(Edit::SetReferences([id].into())).unwrap();
            send(&mut s, PenPhase::Down);
            send(&mut s, PenPhase::Up);
            s.frame(12, 12).unwrap();
            let request = s.renderer_mut().region_requests.last().unwrap().clone();
            assert_eq!(
                request.position,
                [48, 72],
                "reference composition uses document coordinates"
            );
            let RegionSource::Layers(layers) = request.source else {
                panic!("reference snapshot")
            };
            assert_eq!(
                layers
                    .iter()
                    .filter(|l| l.visible)
                    .map(|l| l.id)
                    .collect::<Vec<_>>(),
                [id]
            );
            s.renderer_mut().region_reply = Some(RegionResult {
                request_id: request.request_id,
                pixels: coverage.clone(),
            });
            s.frame(13, 13).unwrap();
            assert_eq!(
                s.engine.document().selection.as_ref().unwrap().affine,
                layer_core::Affine::IDENTITY
            );

            // Parameter edits during async detection apply only to the next fill.
            invoke(&mut s, CommandId::Fill);
            s.dispatch(UiAction::SetToolSetting {
                id: "opacity".into(),
                value: 0.25,
            })
            .unwrap();
            send(&mut s, PenPhase::Down);
            send(&mut s, PenPhase::Up);
            s.frame(14, 14).unwrap();
            let request = s.renderer_mut().region_requests.last().unwrap().clone();
            s.dispatch(UiAction::SetToolSetting {
                id: "opacity".into(),
                value: 0.75,
            })
            .unwrap();
            s.renderer_mut().region_reply = Some(RegionResult {
                request_id: request.request_id,
                pixels: coverage.clone(),
            });
            s.frame(15, 15).unwrap();
            let layer_core::LayerOperationKind::Fill { color, .. } = s
                .engine
                .document()
                .layer(id)
                .unwrap()
                .operations
                .last()
                .unwrap()
                .kind
            else {
                panic!("fill")
            };
            assert_eq!(color[3], 0.25);
        }
    }

    #[test]
    fn ruler_tools_preview_edit_cancel_and_undo_without_repainting() {
        use layer_core::RulerGeometry;
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            s.set_viewport([600., 500.], [1200, 1000]).unwrap();
            s.state.camera.rotation = 0.3;
            s.state.camera.flipped = [true, true];
            s.state.camera.zoom = 2.;
            s.sync_camera();
            s.frame(1, 1).unwrap();
            invoke(&mut s, CommandId::Ruler);
            assert_eq!(s.state.tool_set.groups.len(), 3);
            assert_eq!(s.state.tool_actions.len(), 3);
            let composites = s.renderer_mut().composites;
            let send = |s: &mut UiSession<Recorder>, phase, p: [f32; 2]| {
                let m = s.state.camera.document_to_surface();
                let mut e = event(s, 1, phase, 1.);
                e.surface_position = Point {
                    x: m[0] * p[0] + m[2] * p[1] + m[4],
                    y: m[1] * p[0] + m[3] * p[1] + m[5],
                };
                s.pen(e).unwrap();
                s.frame(1, 1).unwrap();
                assert!(s.state.host_error.is_none(), "{:?}", s.state.host_error);
            };
            for (index, y) in [(0, 100.), (1, 400.), (2, 700.)] {
                s.dispatch(s.state.tool_set.groups[index].action.clone())
                    .unwrap();
                send(&mut s, PenPhase::Down, [100., y]);
                send(&mut s, PenPhase::Move, [300., y + 50.]);
                assert_eq!(
                    s.engine.document().rulers.len(),
                    index,
                    "preview is not history"
                );
                let mut plain = Vec::new();
                s.append_layer_overlay(&mut plain);
                assert!(!plain.is_empty());
                if index < 2 {
                    let reply = key(&mut s, "Shift_L", true, false, false);
                    assert!(reply.change.canvas_wake);
                    let mut snapped = Vec::new();
                    s.append_layer_overlay(&mut snapped);
                    assert_ne!(format!("{plain:?}"), format!("{snapped:?}"));
                    key(&mut s, "Shift_L", false, false, false);
                }
                send(&mut s, PenPhase::Up, [300., y + 50.]);
                assert_eq!(s.engine.document().rulers.len(), index + 1);
                let geometry = s.engine.document().rulers[index].geometry;
                invoke(&mut s, CommandId::Undo);
                s.frame(1, 1).unwrap();
                assert_eq!(s.engine.document().rulers.len(), index);
                invoke(&mut s, CommandId::Redo);
                s.frame(1, 1).unwrap();
                assert_eq!(s.engine.document().rulers[index].geometry, geometry);
            }
            let saved = s.engine.document().rulers.clone();
            // Drag the straight guide's body, then edit its start handle.
            send(&mut s, PenPhase::Down, [200., 125.]);
            send(&mut s, PenPhase::Up, [200., 160.]);
            let (a, b) = s.engine.document().rulers[0].geometry.handles();
            assert!((a.y - 135.).abs() < 0.001);
            send(&mut s, PenPhase::Down, [a.x, a.y]);
            send(&mut s, PenPhase::Up, [a.x - 20., a.y + 20.]);
            let (moved, end) = s.engine.document().rulers[0].geometry.handles();
            assert!((moved.x - 80.).abs() < 0.001);
            assert_eq!(end, b);
            let before = s.engine.document().rulers.clone();
            send(&mut s, PenPhase::Down, [moved.x, moved.y]);
            send(&mut s, PenPhase::Move, [50., 10.]);
            key(&mut s, "Escape", true, false, false);
            s.frame(1, 1).unwrap();
            assert_eq!(
                s.engine.document().rulers,
                before,
                "cancel never commits a guide"
            );
            assert_eq!(
                s.renderer_mut().composites,
                composites,
                "guide edits must not repaint"
            );
            assert_eq!(s.renderer_mut().dabs, 0);
            invoke(&mut s, CommandId::DeleteRuler);
            s.frame(1, 1).unwrap();
            assert_eq!(s.engine.document().rulers.len(), 2);
            invoke(&mut s, CommandId::Undo);
            s.frame(1, 1).unwrap();
            assert_eq!(s.engine.document().rulers, before);
            invoke(&mut s, CommandId::ShowRulers);
            let mut hidden = Vec::new();
            s.append_layer_overlay(&mut hidden);
            assert!(hidden.is_empty());
            assert!(
                !s.state
                    .commands
                    .iter()
                    .find(|c| c.id == CommandId::SnapRulers)
                    .unwrap()
                    .enabled
            );
            invoke(&mut s, CommandId::ShowRulers);
            assert!(
                s.state
                    .commands
                    .iter()
                    .find(|c| c.id == CommandId::SnapRulers)
                    .unwrap()
                    .selected
            );
            invoke(&mut s, CommandId::SnapRulers);
            assert!(!s.rulers.snapping);
            assert!(matches!(saved[2].geometry, RulerGeometry::Radial { .. }));
            // Operation edits existing guides, but never creates a ruler and
            // never moves the paint layer while a ruler handle owns contact.
            invoke(&mut s, CommandId::Move);
            let layers = s.engine.document().layers.clone();
            let end = s.engine.document().rulers[0].geometry.handles().1.unwrap();
            send(&mut s, PenPhase::Down, [end.x, end.y]);
            send(&mut s, PenPhase::Up, [end.x + 25., end.y + 30.]);
            assert_eq!(s.state.layer_tools.tool, LayerCanvasTool::Move);
            assert_eq!(s.engine.document().layers, layers);
            assert_ne!(s.engine.document().rulers, before);
            invoke(&mut s, CommandId::Undo);
            s.frame(1, 1).unwrap();
            assert_eq!(s.engine.document().rulers, before);
            send(&mut s, PenPhase::Down, [1900., 1300.]);
            send(&mut s, PenPhase::Up, [1900., 1300.]);
            assert_eq!(s.engine.document().rulers, before);
        }
    }

    #[test]
    fn figure_tools_use_shared_controls_constrain_cancel_and_commit_once() {
        use layer_core::{Edit, LayerOperationKind};
        let mut s = session();
        assert!(key(&mut s, "u", true, false, false).handled);
        key(&mut s, "u", false, false, false);
        assert_eq!(s.state.tool_set.groups.len(), 3);
        assert_eq!(s.state.tool_set.subtools.len(), 1);
        assert_eq!(
            s.state
                .tool_settings
                .iter()
                .map(|c| c.label)
                .collect::<Vec<_>>(),
            ["Line width", "Opacity"]
        );
        let id = s.engine.document().active_layer;
        let mut layer = s.engine.document().layer(id).unwrap().clone();
        layer.properties.offset = Point { x: 10., y: 20. };
        layer.properties.alpha_locked = true;
        s.layer_edit(Edit::ReplaceLayer(Box::new(layer))).unwrap();
        s.dispatch(UiAction::SetColor {
            rgba: [1., 0., 0., 1.],
        })
        .unwrap();
        s.dispatch(UiAction::SetToolSetting {
            id: "opacity".into(),
            value: 0.4,
        })
        .unwrap();
        s.dispatch(UiAction::SetToolSetting {
            id: "size".into(),
            value: 8.,
        })
        .unwrap();
        s.state.camera.rotation = 0.3;
        s.state.camera.flipped = [true, false];
        s.frame(1, 1).unwrap();
        let send = |s: &mut UiSession<Recorder>, phase, p: [f32; 2]| {
            let m = s.state.camera.document_to_surface();
            let mut e = event(s, 1, phase, 1.);
            e.surface_position = Point {
                x: m[0] * p[0] + m[2] * p[1] + m[4],
                y: m[1] * p[0] + m[3] * p[1] + m[5],
            };
            s.pen(e).unwrap();
        };
        for index in 0..3 {
            let action = s.state.tool_set.groups[index].action.clone();
            s.dispatch(action).unwrap();
            let count = if index == 0 { 1 } else { 3 };
            assert_eq!(s.state.tool_set.subtools.len(), count);
            for mode in 0..count {
                let action = s.state.tool_set.subtools[mode].action.clone();
                s.dispatch(action).unwrap();
                let remembered = s.layer_interaction.tool;
                invoke(&mut s, CommandId::Hand);
                invoke(&mut s, CommandId::Figure);
                assert_eq!(s.layer_interaction.tool, remembered);
                assert_eq!(s.state.tool_settings.len(), if mode == 1 { 1 } else { 2 });
                send(&mut s, PenPhase::Down, [30., 40.]);
                for i in 0..100 {
                    send(&mut s, PenPhase::Move, [50. + i as f32, 90.]);
                }
                assert_eq!(s.layer_interaction.path.len(), 2);
                let before = s.engine.document().layer(id).unwrap().operations.len();
                let plain = s.current_figure().unwrap();
                let change = s
                    .input(UiInput::Key {
                        key: "Shift_L".into(),
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers {
                            shift: true,
                            ..Default::default()
                        },
                        editing: false,
                        divider: None,
                    })
                    .unwrap();
                assert!(change.change.canvas_wake);
                let constrained = s.current_figure().unwrap();
                assert_ne!(plain.end, constrained.end);
                let mut guide = Vec::new();
                s.append_layer_overlay(&mut guide);
                assert!(!guide.is_empty());
                s.input(UiInput::Key {
                    key: "Shift_L".into(),
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers {
                        shift: true,
                        ..Default::default()
                    },
                    editing: false,
                    divider: None,
                })
                .unwrap();
                assert_eq!(
                    s.current_figure().unwrap().end,
                    plain.end,
                    "release ignores stale GDK modifier bit"
                );
                key(&mut s, "shift", true, false, false);
                send(&mut s, PenPhase::Up, [130., 90.]);
                s.frame(2, 2).unwrap();
                key(&mut s, "shift", false, false, false);
                let ops = &s.engine.document().layer(id).unwrap().operations;
                assert_eq!(ops.len(), before + 1);
                let LayerOperationKind::Figure(f) = &ops.last().unwrap().kind else {
                    panic!("figure");
                };
                assert!((f.start.x - 20.).abs() < 0.001 && (f.start.y - 20.).abs() < 0.001);
                assert_eq!(f.colors[0], [1., 0., 0., 0.4]);
                assert!(f.alpha_locked);
                assert!(!f.erase);
                assert_eq!(f.width, 8.);
                if index != 0 {
                    assert!(
                        ((f.end.x - f.start.x).abs() - (f.end.y - f.start.y).abs()).abs() < 0.001
                    );
                }
                invoke(&mut s, CommandId::Undo);
                s.frame(3, 3).unwrap();
                assert_eq!(
                    s.engine.document().layer(id).unwrap().operations.len(),
                    before
                );
                invoke(&mut s, CommandId::Redo);
                s.frame(4, 4).unwrap();
                assert_eq!(
                    s.engine.document().layer(id).unwrap().operations.len(),
                    before + 1
                );
                assert_eq!(s.renderer_mut().dabs, 0, "no brush stamping for figures");
            }
        }
        let before = s.engine.document().layer(id).unwrap().operations.len();
        for cancel in 0..3 {
            send(&mut s, PenPhase::Down, [30., 40.]);
            send(&mut s, PenPhase::Move, [90., 100.]);
            match cancel {
                0 => send(&mut s, PenPhase::Cancel, [90., 100.]),
                1 => {
                    key(&mut s, "escape", true, false, false);
                    key(&mut s, "escape", false, false, false);
                }
                _ => {
                    s.input(UiInput::Blur).unwrap();
                }
            }
            send(&mut s, PenPhase::Up, [90., 100.]);
            s.frame(5, 5).unwrap();
            assert_eq!(
                s.engine.document().layer(id).unwrap().operations.len(),
                before
            );
        }
        send(&mut s, PenPhase::Down, [30., 40.]);
        send(&mut s, PenPhase::Up, [30., 40.]);
        s.frame(6, 6).unwrap();
        assert_eq!(
            s.engine.document().layer(id).unwrap().operations.len(),
            before
        );
        s.dispatch(UiAction::Color {
            action: ColorAction::Select {
                slot: ColorSlot::Transparent,
            },
        })
        .unwrap();
        send(&mut s, PenPhase::Down, [30., 40.]);
        send(&mut s, PenPhase::Move, [90., 100.]);
        assert!(s.current_figure().unwrap().erase);
        send(&mut s, PenPhase::Cancel, [90., 100.]);
        // Protected content and mask targets cannot acquire figure operations.
        for mask in [false, true] {
            let mut layer = s.engine.document().layer(id).unwrap().clone();
            if mask {
                layer.mask = Some(layer_core::LayerMask::reveal_all(
                    layer_core::LayerId(99),
                    Point::default(),
                ));
            }
            layer.properties.locked = !mask;
            s.layer_edit(Edit::ReplaceLayer(Box::new(layer))).unwrap();
            s.layer_edit(Edit::SetMaskTarget(mask)).unwrap();
            send(&mut s, PenPhase::Down, [30., 40.]);
            send(&mut s, PenPhase::Move, [90., 100.]);
            send(&mut s, PenPhase::Up, [90., 100.]);
            s.frame(7, 7).unwrap();
            assert_eq!(
                s.engine.document().layer(id).unwrap().operations.len(),
                before
            );
        }
    }

    #[test]
    fn gradient_tools_commit_once_with_local_selection_and_cancel_cleanly() {
        use layer_core::{Edit, LayerOperationKind, Selection};
        let mut s = session();
        assert!(key(&mut s, "g", true, false, false).handled);
        key(&mut s, "g", false, false, false);
        assert_eq!(s.state.tool_set.subtools.len(), 4);
        assert_eq!(s.state.tool_settings.len(), 1);
        assert_eq!(s.state.tool_settings[0].id, "opacity");
        let action = s.state.tool_set.subtools[3].action.clone();
        s.dispatch(action).unwrap();
        let tool = s.layer_interaction.tool;
        invoke(&mut s, CommandId::Hand);
        invoke(&mut s, CommandId::Gradient);
        assert_eq!(s.layer_interaction.tool, tool, "remember gradient variant");
        s.dispatch(UiAction::SetColor {
            rgba: [1.0, 0.0, 0.0, 1.0],
        })
        .unwrap();
        s.dispatch(UiAction::SetToolSetting {
            id: "opacity".into(),
            value: 0.4,
        })
        .unwrap();
        assert!(
            s.dispatch(UiAction::SetToolSetting {
                id: "size".into(),
                value: 20.0
            })
            .is_err()
        );
        let id = s.engine.document().active_layer;
        let mut layer = s.engine.document().layer(id).unwrap().clone();
        layer.properties.offset = Point { x: 10.0, y: 20.0 };
        layer.properties.alpha_locked = true;
        s.layer_edit(Edit::ReplaceLayer(Box::new(layer))).unwrap();
        let selection = Selection::polygon(vec![
            Point { x: 10.0, y: 20.0 },
            Point { x: 100.0, y: 20.0 },
            Point { x: 100.0, y: 100.0 },
        ])
        .unwrap();
        s.layer_edit(Edit::SetSelection(Some(selection))).unwrap();
        s.state.camera.rotation = 0.4;
        s.state.camera.flipped = [true, false];
        s.frame(1, 1).unwrap();
        let send = |s: &mut UiSession<Recorder>, phase, p: [f32; 2]| {
            let m = s.state.camera.document_to_surface();
            let mut e = event(s, 1, phase, 1.0);
            e.surface_position = Point {
                x: m[0] * p[0] + m[2] * p[1] + m[4],
                y: m[1] * p[0] + m[3] * p[1] + m[5],
            };
            s.pen(e).unwrap();
        };
        send(&mut s, PenPhase::Down, [30.0, 40.0]);
        for i in 0..100 {
            send(&mut s, PenPhase::Move, [50.0 + i as f32, 70.0]);
        }
        s.frame(2, 2).unwrap();
        assert_eq!(
            s.layer_interaction.path.len(),
            2,
            "guide stores endpoints, not a stroke path"
        );
        assert!(!s.command(CommandId::Undo).enabled);
        assert!(s.engine.document().layer(id).unwrap().operations.is_empty());
        send(&mut s, PenPhase::Up, [130.0, 140.0]);
        s.frame(3, 3).unwrap();
        let operations = &s.engine.document().layer(id).unwrap().operations;
        assert_eq!(operations.len(), 1);
        let LayerOperationKind::Gradient {
            start,
            end,
            colors,
            radial,
            alpha_locked,
        } = operations[0].kind
        else {
            panic!("gradient")
        };
        assert!((start.x - 20.0).abs() < 0.001 && (start.y - 20.0).abs() < 0.001);
        assert!((end.x - 120.0).abs() < 0.001 && (end.y - 120.0).abs() < 0.001);
        assert!(radial && alpha_locked);
        assert_eq!(colors, [[1.0, 0.0, 0.0, 0.4], [1.0, 0.0, 0.0, 0.0]]);
        let selection = operations[0].coverage.initial.as_ref().unwrap();
        let first = selection.contours()[0][0];
        assert_eq!(selection.affine.map(first), Point::default());
        assert!(s.layer_interaction.path.is_empty());
        assert_eq!(s.renderer_mut().dabs, 0);
        invoke(&mut s, CommandId::Undo);
        s.frame(4, 4).unwrap();
        assert!(s.engine.document().layer(id).unwrap().operations.is_empty());
        invoke(&mut s, CommandId::Redo);
        s.frame(5, 5).unwrap();
        assert_eq!(s.engine.document().layer(id).unwrap().operations.len(), 1);
        for cancel in [0, 1, 2] {
            send(&mut s, PenPhase::Down, [30.0, 40.0]);
            send(&mut s, PenPhase::Move, [90.0, 100.0]);
            match cancel {
                0 => send(&mut s, PenPhase::Cancel, [90.0, 100.0]),
                1 => {
                    assert!(key(&mut s, "escape", true, false, false).handled);
                    key(&mut s, "escape", false, false, false);
                }
                _ => {
                    s.input(UiInput::Blur).unwrap();
                }
            }
            send(&mut s, PenPhase::Up, [90.0, 100.0]);
            s.frame(6, 6).unwrap();
            assert!(s.layer_interaction.path.is_empty());
            assert_eq!(s.engine.document().layer(id).unwrap().operations.len(), 1);
        }
        send(&mut s, PenPhase::Down, [30.0, 40.0]);
        send(&mut s, PenPhase::Up, [30.0, 40.0]);
        s.frame(7, 7).unwrap();
        assert_eq!(
            s.engine.document().layer(id).unwrap().operations.len(),
            1,
            "tap is not a whole-layer fill"
        );
        assert!(s.state.host_error.is_none());
    }

    #[test]
    fn hand_uses_shared_pointer_and_touch_navigation_without_painting() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for kind in [PointerKind::Mouse, PointerKind::Pen, PointerKind::Touch] {
                let mut s = session();
                s.set_platform(platform);
                assert!(key(&mut s, "h", true, false, false).handled);
                key(&mut s, "h", false, false, false);
                assert!(s.command(CommandId::Hand).selected);
                let before = s.state.camera.clone();
                let revision = s.engine.document().revision;
                let p = before.input_transform().map(Point { x: 200.0, y: 300.0 });
                for (phase, position) in [
                    (ContactPhase::Down, [200.0, 300.0]),
                    (ContactPhase::Move, [260.0, 340.0]),
                    (ContactPhase::Up, [260.0, 340.0]),
                ] {
                    let reply = s
                        .input(UiInput::Pointer {
                            id: 1,
                            phase,
                            kind,
                            button: PointerButton::Primary,
                            position,
                        })
                        .unwrap();
                    assert!(!reply.paint && reply.pan_cursor);
                }
                let after = s
                    .state
                    .camera
                    .input_transform()
                    .map(Point { x: 260.0, y: 340.0 });
                assert!((after.x - p.x).abs() < 0.001 && (after.y - p.y).abs() < 0.001);
                assert_eq!(s.state.camera.zoom, before.zoom);
                s.frame(1_000_000, 1_000_000).unwrap();
                assert_eq!(s.renderer_mut().dabs, 0);
                assert_eq!(s.engine.document().revision, revision);
                assert!(!s.command(CommandId::Undo).enabled);
            }
        }
    }

    #[test]
    fn eyedropper_coalesces_points_resolves_color_slots_and_rejects_stale_results() {
        use layer_render::{ColorSample, ColorSampleSource};
        let mut s = session();
        assert!(key(&mut s, "i", true, false, false).handled);
        key(&mut s, "i", false, false, false);
        assert_eq!(s.state.tool_set.subtools.len(), 2);
        s.dispatch(UiAction::Color {
            action: ColorAction::Select {
                slot: ColorSlot::Background,
            },
        })
        .unwrap();
        s.dispatch(UiAction::Color {
            action: ColorAction::Select {
                slot: ColorSlot::Transparent,
            },
        })
        .unwrap();
        let foreground = s.state.colors.foreground;
        let revision = s.engine.document().revision;
        let send = |s: &mut UiSession<Recorder>, phase, p: [f32; 2]| {
            let m = s.state.camera.document_to_surface();
            let mut e = event(s, 1, phase, 1.0);
            e.surface_position = Point {
                x: m[0] * p[0] + m[2] * p[1] + m[4],
                y: m[1] * p[0] + m[3] * p[1] + m[5],
            };
            s.pen(e).unwrap();
        };
        s.state.camera.flipped = [true, false];
        s.state.camera.rotation = 0.7;
        send(&mut s, PenPhase::Down, [64.25, 80.25]);
        assert!(s.wants_continuous_frames());
        s.frame(1, 1).unwrap();
        let request = s.renderer_mut().sample_requests[0];
        assert_eq!(request.position, [64, 80]);
        assert_eq!(request.source, ColorSampleSource::Composite);
        for x in 100..200 {
            send(&mut s, PenPhase::Move, [x as f32 + 0.25, 80.25]);
        }
        send(&mut s, PenPhase::Up, [199.25, 80.25]);
        s.frame(2, 2).unwrap();
        assert_eq!(s.renderer_mut().sample_requests.len(), 1);
        s.renderer_mut().sample_reply = Some(ColorSample {
            request_id: request.request_id,
            rgba: [0.25, 0.5, 1.0, 0.3],
        });
        let change = s.frame(3, 3).unwrap();
        assert_ne!(change.regions & regions::BRUSH, 0);
        assert_eq!(s.state.colors.foreground, foreground);
        assert_eq!(s.state.colors.slot, ColorSlot::Background);
        for (a, b) in s
            .state
            .colors
            .background
            .into_iter()
            .zip([0.537099, 0.735357, 1.0, 1.0])
        {
            assert!((a - b).abs() < 0.0001);
        }
        assert_eq!(s.renderer_mut().sample_requests[1].position, [199, 80]);
        let color = [0.1, 0.2, 0.3, 1.0];
        s.dispatch(UiAction::SetColor { rgba: color }).unwrap();
        s.renderer_mut().sample_reply = Some(ColorSample {
            request_id: request.request_id,
            rgba: [1.0; 4],
        });
        s.frame(4, 4).unwrap();
        assert_eq!(
            s.state.colors.background, color,
            "late sample cannot overwrite a manual choice"
        );
        assert!(!s.wants_continuous_frames());
        assert_eq!(s.engine.document().revision, revision);
        assert_eq!(s.renderer_mut().dabs, 0);

        let mut layer = s
            .engine
            .document()
            .layer(s.engine.document().active_layer)
            .unwrap()
            .clone();
        layer.properties.offset = Point { x: 16.0, y: 8.0 };
        let id = layer.id;
        s.layer_edit(layer_core::Edit::ReplaceLayer(Box::new(layer)))
            .unwrap();
        s.dispatch(UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::PickLayer,
            },
        })
        .unwrap();
        send(&mut s, PenPhase::Down, [64.25, 80.25]);
        s.frame(5, 5).unwrap();
        let request = *s.renderer_mut().sample_requests.last().unwrap();
        assert_eq!(
            (request.source, request.position),
            (ColorSampleSource::Layer(id), [48, 72])
        );
        invoke(&mut s, CommandId::Hand);
        s.renderer_mut().sample_reply = Some(ColorSample {
            request_id: request.request_id,
            rgba: [1.0; 4],
        });
        s.frame(6, 6).unwrap();
        assert_eq!(
            s.state.colors.background, color,
            "tool changes cancel sampling"
        );
        invoke(&mut s, CommandId::Eyedropper);
        assert_eq!(
            s.state.layer_tools.tool,
            LayerCanvasTool::PickLayer,
            "remember source subtool"
        );
    }

    #[test]
    fn operation_controls_support_active_masks_and_linked_paint_without_host_logic() {
        use layer_core::{LayerOperationKind, Point, Selection};
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for linked in [false, true] {
                let mut s = session();
                s.set_platform(platform);
                let area = Selection::polygon(vec![
                    Point { x: 100., y: 100. },
                    Point { x: 300., y: 100. },
                    Point { x: 300., y: 300. },
                    Point { x: 100., y: 300. },
                ])
                .unwrap();
                s.fill_selection(area.clone()).unwrap();
                s.layer_edit(layer_core::Edit::SetSelection(Some(area)))
                    .unwrap();
                let id = s.engine.document().active_layer;
                s.dispatch(UiAction::Layer {
                    action: LayerAction::AddMask {
                        id: id.0,
                        replace: false,
                    },
                })
                .unwrap();
                s.dispatch(UiAction::Layer {
                    action: LayerAction::LinkMask {
                        id: id.0,
                        value: linked,
                    },
                })
                .unwrap();
                s.frame(1, 1).unwrap();
                let before = s.engine.document().layers.clone();
                assert!(s.engine.document().active_mask);
                assert!(s.command(CommandId::ScaleRotate).enabled);
                invoke(&mut s, CommandId::ScaleRotate);
                s.frame(2, 2).unwrap();
                assert_eq!(
                    s.renderer_mut().transform.as_ref().unwrap().layer,
                    before[0].mask.as_ref().unwrap().id
                );
                s.dispatch(UiAction::SetToolSetting {
                    id: "transform_x".into(),
                    value: 24.,
                })
                .unwrap();
                invoke(&mut s, CommandId::ApplyTransform);
                s.frame(2, 2).unwrap();
                let after = &s.engine.document().layers[0];
                assert_eq!(
                    after.operations.len(),
                    before[0].operations.len() + usize::from(linked)
                );
                let mask = after.mask.as_ref().unwrap();
                assert_eq!(mask.operations.len(), 1);
                assert!(matches!(
                    mask.operations[0].kind,
                    LayerOperationKind::Transform(_)
                ));
                invoke(&mut s, CommandId::Undo);
                assert_eq!(s.engine.document().layers, before);
                s.layer_edit(layer_core::Edit::SetMaskTarget(false))
                    .unwrap();
                assert!(s.command(CommandId::ScaleRotate).enabled);
                invoke(&mut s, CommandId::ScaleRotate);
                s.dispatch(UiAction::SetToolSetting {
                    id: "transform_width".into(),
                    value: 1.2,
                })
                .unwrap();
                invoke(&mut s, CommandId::CancelTransform);
                assert_eq!(s.engine.document().layers, before);
            }
        }
    }

    #[test]
    fn operation_controls_preview_cancel_apply_and_undo_share_one_transaction() {
        use layer_core::{Affine, Point, Selection};
        let mut s = session();
        let selection = Selection::polygon(vec![
            Point { x: 100., y: 100. },
            Point { x: 300., y: 100. },
            Point { x: 300., y: 300. },
            Point { x: 100., y: 300. },
        ])
        .unwrap();
        s.fill_selection(selection.clone()).unwrap();
        s.layer_edit(layer_core::Edit::SetSelection(Some(selection.clone())))
            .unwrap();
        s.frame(1, 1).unwrap();
        let original = s.engine.document().clone();
        invoke(&mut s, CommandId::ScaleRotate);
        assert_eq!(s.state.layer_tools.tool, LayerCanvasTool::Transform);
        assert_eq!(s.state.tool_settings.len(), 5);
        assert_eq!(s.state.tool_set.groups.len(), 2);
        for (id, value) in [
            ("transform_x", 30.),
            ("transform_width", 1.5),
            ("transform_angle", 0.3),
        ] {
            assert!(
                s.dispatch(UiAction::SetToolSetting {
                    id: id.into(),
                    value
                })
                .unwrap()
                .canvas_wake
            );
        }
        s.frame(2, 2).unwrap();
        let preview = s.renderer_mut().transform.clone().unwrap();
        assert_eq!(s.engine.document().revision, original.revision);
        assert_ne!(preview.transform.affine, Affine::IDENTITY);
        let mut overlay = Vec::new();
        s.append_layer_overlay(&mut overlay);
        assert_eq!(overlay.iter().filter(|s| s.marker == 2.).count(), 9);
        invoke(&mut s, CommandId::ApplyTransform);
        s.frame(3, 3).unwrap();
        assert!(!s.operation.active());
        let op = s
            .engine
            .document()
            .layer(original.active_layer)
            .unwrap()
            .operations
            .last()
            .unwrap();
        assert_eq!(
            op.kind,
            layer_core::LayerOperationKind::Transform(preview.transform)
        );
        assert_ne!(s.engine.document().selection.as_ref(), Some(&selection));
        invoke(&mut s, CommandId::Undo);
        s.frame(4, 4).unwrap();
        assert_eq!(s.engine.document().layers, original.layers);
        assert_eq!(s.engine.document().selection, original.selection);
        for action in [
            UiAction::Invoke {
                command: CommandId::CancelTransform,
            },
            UiAction::Invoke {
                command: CommandId::Pen,
            },
            UiAction::Layer {
                action: LayerAction::AlphaLock {
                    id: original.active_layer.0,
                    value: true,
                },
            },
        ] {
            invoke(&mut s, CommandId::ScaleRotate);
            s.dispatch(UiAction::SetToolSetting {
                id: "transform_x".into(),
                value: 80.,
            })
            .unwrap();
            assert!(s.dispatch(action).unwrap().canvas_wake);
            s.frame(5, 5).unwrap();
            assert!(!s.operation.active());
            assert!(s.renderer_mut().transform.is_none());
            assert!(
                s.state
                    .tool_settings
                    .iter()
                    .all(|c| !c.id.starts_with("transform_"))
            );
        }
        invoke(&mut s, CommandId::ScaleRotate);
        s.frame(6, 6).unwrap();
        let before = s.renderer_mut().transform.clone();
        assert!(
            s.dispatch(UiAction::SetToolSetting {
                id: "transform_width".into(),
                value: 0.
            })
            .is_err()
        );
        s.frame(7, 7).unwrap();
        assert_eq!(s.renderer_mut().transform, before);
        for (phase, point) in [
            (PenPhase::Down, Point { x: 300., y: 300. }),
            (PenPhase::Move, Point { x: 340., y: 320. }),
        ] {
            let mut e = event(&s, 1, phase, 1.);
            e.surface_position = Affine(s.state.camera.document_to_surface()).map(point);
            s.pen(e).unwrap();
            s.frame(7, 7).unwrap();
        }
        assert!(
            key(&mut s, "Shift_L", true, false, false)
                .change
                .canvas_wake
        );
        let size = |s: &UiSession<Recorder>, id: &str| {
            s.state
                .tool_settings
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .value
        };
        assert!((size(&s, "transform_width") - size(&s, "transform_height")).abs() < 0.001);
        assert!(key(&mut s, "Alt_L", true, false, true).change.canvas_wake);
        assert!(size(&s, "transform_x").abs() < 0.001);
        assert!(size(&s, "transform_y").abs() < 0.001);
        key(&mut s, "escape", true, false, false);
        s.frame(8, 8).unwrap();
        assert!(!s.operation.active());
    }

    #[test]
    fn navigator_previews_are_visible_only_throttled_and_single_flight() {
        let mut s = session();
        s.poll_navigator_preview(0, false).unwrap();
        assert!(s.renderer_mut().preview_requests.is_empty());
        s.poll_navigator_preview(0, true).unwrap();
        for now in [1, 66_666_667, 1_000_000_000] {
            s.poll_navigator_preview(now, true).unwrap();
        }
        assert_eq!(s.renderer_mut().preview_requests, [None]);
        s.renderer_mut().preview_reply = Some(layer_render::CanvasPreview {
            revision: 42,
            image: None,
        });
        s.poll_navigator_preview(1_000_000_000, false).unwrap();
        assert_eq!(s.renderer_mut().preview_requests, [None]);
        s.poll_navigator_preview(1_000_000_000, true).unwrap();
        assert_eq!(s.renderer_mut().preview_requests, [None, Some(42)]);
        s.renderer_mut().preview_reply = Some(layer_render::CanvasPreview {
            revision: 42,
            image: None,
        });
        invoke(&mut s, CommandId::ZoomIn);
        s.poll_navigator_preview(1_010_000_000, true).unwrap();
        assert_eq!(s.renderer_mut().preview_requests.len(), 2);
        s.poll_navigator_preview(1_067_000_000, true).unwrap();
        assert_eq!(
            s.renderer_mut().preview_requests,
            [None, Some(42), Some(42)]
        );
    }

    #[test]
    fn tool_groups_remember_subtools_edits_and_never_change_paint_color() {
        let mut s = session();
        let color = [0.2, 0.5, 0.8, 1.0];
        s.dispatch(UiAction::SetColor { rgba: color }).unwrap();
        let mut remembered = Vec::new();
        for tool in Tool::ALL {
            invoke(&mut s, tool.command());
            assert_eq!(s.state.brush.tool, tool);
            let groups = s.state.tool_set.groups.clone();
            assert_eq!(groups.iter().filter(|g| g.selected).count(), 1);
            for group in groups {
                s.dispatch(group.action).unwrap();
                let last = s.state.tool_set.subtools.last().unwrap().clone();
                s.dispatch(last.action).unwrap();
                let id = last.preview.unwrap();
                assert_eq!(s.state.brush.preset, id);
                let size = id as f32 + 10.0;
                s.dispatch(UiAction::SetBrushSize { value: size }).unwrap();
                s.dispatch(UiAction::SetToolSetting {
                    id: "spacing".into(),
                    value: 0.23,
                })
                .unwrap();
                remembered.push((tool, tools::group(id), id, size));
                assert_eq!(
                    s.state
                        .tool_set
                        .subtools
                        .iter()
                        .filter(|b| b.selected)
                        .count(),
                    1
                );
                assert_eq!(s.state.brush.color, color);
            }
        }
        for tool in Tool::ALL {
            invoke(&mut s, tool.command());
            assert_eq!(
                s.state.brush.preset,
                remembered.iter().rfind(|r| r.0 == tool).unwrap().2
            );
            assert!(s.command(tool.command()).selected);
            assert_eq!(
                Tool::ALL
                    .iter()
                    .filter(|t| s.command(t.command()).selected)
                    .count(),
                1
            );
            for &(_, group, id, size) in remembered.iter().filter(|r| r.0 == tool) {
                s.dispatch(UiAction::SelectToolGroup { group }).unwrap();
                assert_eq!(s.state.brush.preset, id);
                assert_eq!(s.state.brush.diameter, size);
                assert_eq!(s.engine.configured_brush().spacing, 0.23);
            }
        }
        invoke(&mut s, CommandId::Lasso);
        assert!(s.state.tool_settings.is_empty());
        assert!(
            s.state
                .tool_set
                .subtools
                .iter()
                .all(|b| b.preview.is_none())
        );
        let before = serde_json::to_value(s.state()).unwrap();
        assert!(s.dispatch(UiAction::SelectBrush { id: u32::MAX }).is_err());
        assert!(
            s.dispatch(UiAction::SelectToolGroup {
                group: ToolGroup::Pen
            })
            .is_err()
        );
        assert_eq!(serde_json::to_value(s.state()).unwrap(), before);
        invoke(&mut s, CommandId::Pen);
        assert!(!s.state.tool_settings.is_empty());
        assert_eq!(s.state.brush.color, color);
    }

    #[test]
    fn tool_switch_keeps_active_stroke_snapshot_and_restores_next_stroke_settings() {
        let mut s = session();
        let original = s.engine.configured_brush().clone();
        s.pen(event(&s, 1, PenPhase::Down, 1.0)).unwrap();
        s.frame(10_000_000, 18_000_000).unwrap();
        invoke(&mut s, CommandId::Liquify);
        assert_eq!(s.engine.brush(), &original);
        assert_eq!(
            s.engine.configured_brush().execution_class(),
            layer_core::BrushExecution::Liquify
        );
        s.pen(event(&s, 2, PenPhase::Up, 1.0)).unwrap();
        s.frame(20_000_000, 28_000_000).unwrap();
        assert_eq!(
            s.engine.document().strokes().last().unwrap().brush,
            original
        );
        invoke(&mut s, CommandId::Pen);
        assert_eq!(s.engine.configured_brush(), &original);
    }

    #[test]
    fn tool_family_keys_cycle_in_toolbar_order_and_direct_bindings_stay_direct() {
        let mut s = session();
        for (letter, family) in [
            ("p", ToolFamily::Ink),
            ("b", ToolFamily::Paint),
            ("j", ToolFamily::Blend),
        ] {
            invoke(&mut s, CommandId::Eraser);
            for &command in family
                .commands()
                .iter()
                .cycle()
                .take(family.commands().len() * 2)
            {
                assert!(key(&mut s, letter, true, false, false).handled);
                assert!(s.command(command).selected);
                assert_eq!(s.command(command).shortcut, letter.to_uppercase());
                key(&mut s, letter, false, false, false);
            }
        }
        let panel = s.state.workspace.layout.panel_mut(Panel::Toolbar).unwrap();
        let tiles = panel.tiles_mut().unwrap();
        tiles.clear();
        for (i, command) in [
            CommandId::Decoration,
            CommandId::Airbrush,
            CommandId::Decoration,
            CommandId::Brush,
        ]
        .into_iter()
        .enumerate()
        {
            tiles.push(ToolbarTile {
                id: i as u32 + 1,
                control: ToolbarControl::Command { command },
            });
        }
        invoke(&mut s, CommandId::Eraser);
        for command in [
            CommandId::Decoration,
            CommandId::Airbrush,
            CommandId::Brush,
            CommandId::Decoration,
        ] {
            s.dispatch(UiAction::CycleTool {
                family: ToolFamily::Paint,
            })
            .unwrap();
            assert!(s.command(command).selected);
        }
        let mut settings = s.state.settings.clone();
        settings.shortcuts.insert(
            CommandId::Airbrush.shortcut_id(),
            vec![KeyChord::new("k", Modifiers::default())],
        );
        s.dispatch(UiAction::RestoreSettings { settings }).unwrap();
        for _ in 0..3 {
            key(&mut s, "k", true, false, false);
            assert!(s.command(CommandId::Airbrush).selected);
            key(&mut s, "k", false, false, false);
        }
        key(&mut s, "b", true, false, true);
        assert!(
            s.command(CommandId::Airbrush).selected,
            "typing in a field does not select tools"
        );
    }

    #[test]
    fn tool_edits_preserve_other_parameters_and_the_live_stroke() {
        let mut s = session();
        let original = s.engine.configured_brush().clone();
        s.pen(event(&s, 1, PenPhase::Down, 1.0)).unwrap();
        s.frame(10_000_000, 18_000_000).unwrap();
        for (id, value) in [("flow", 0.35), ("spacing", 0.2), ("size_jitter", 0.3)] {
            s.dispatch(UiAction::SetToolSetting {
                id: id.into(),
                value,
            })
            .unwrap();
        }
        s.dispatch(UiAction::SetBrushSize { value: 40.0 }).unwrap();
        s.dispatch(UiAction::SetColor {
            rgba: [0.8, 0.1, 0.2, 1.],
        })
        .unwrap();
        assert_eq!(s.engine.configured_brush().flow, 0.35);
        assert_eq!(s.engine.configured_brush().spacing, 0.2);
        assert_eq!(s.engine.configured_brush().shape.size_jitter, 0.3);
        assert_eq!(s.engine.configured_brush().diameter, 40.0);
        assert_eq!(s.engine.brush(), &original);
        s.pen(event(&s, 2, PenPhase::Up, 1.0)).unwrap();
        s.frame(20_000_000, 28_000_000).unwrap();
        assert_eq!(
            s.engine.document().strokes().next().unwrap().brush,
            original
        );
        s.pen(event(&s, 3, PenPhase::Down, 1.0)).unwrap();
        s.pen(event(&s, 4, PenPhase::Up, 1.0)).unwrap();
        s.frame(40_000_000, 48_000_000).unwrap();
        let next = s.engine.document().strokes().nth(1).unwrap();
        assert_eq!(next.brush.flow, 0.35);
        assert_eq!(next.brush.diameter, 40.0);
    }

    #[test]
    fn shared_color_slots_drive_transparent_paint_without_changing_the_tip() {
        let mut s = session();
        let preset = s.state.brush.preset;
        s.dispatch(UiAction::Color {
            action: ColorAction::Select {
                slot: ColorSlot::Background,
            },
        })
        .unwrap();
        s.dispatch(UiAction::SetColor {
            rgba: [0.1, 0.8, 0.3, 1.],
        })
        .unwrap();
        assert_eq!(s.state.colors.background, s.state.brush.color);
        s.dispatch(UiAction::Color {
            action: ColorAction::Select {
                slot: ColorSlot::Transparent,
            },
        })
        .unwrap();
        s.pen(event(&s, 1, PenPhase::Down, 1.0)).unwrap();
        s.pen(event(&s, 2, PenPhase::Up, 1.0)).unwrap();
        s.frame(20_000_000, 28_000_000).unwrap();
        assert_eq!(
            s.engine.document().strokes().next().unwrap().tool,
            StrokeTool::Eraser
        );
        assert_eq!(s.state.brush.preset, preset);
        s.dispatch(UiAction::Color {
            action: ColorAction::Component {
                index: 0,
                value: 240.0,
            },
        })
        .unwrap();
        assert_eq!(s.state.colors.slot, ColorSlot::Background);
        s.pen(event(&s, 3, PenPhase::Down, 1.0)).unwrap();
        s.pen(event(&s, 4, PenPhase::Up, 1.0)).unwrap();
        s.frame(40_000_000, 48_000_000).unwrap();
        assert_eq!(
            s.engine.document().strokes().nth(1).unwrap().tool,
            StrokeTool::Brush
        );
    }

    #[test]
    fn runtime_filter_publication_is_atomic_and_uses_current_values() {
        use layer_core::{EffectInstallMode, EffectPackage, EffectValue};
        use std::sync::Arc;
        let mut s = session();
        s.dispatch(UiAction::Effect {
            action: EffectAction::Insert {
                effect: "unsharp_mask".into(),
            },
        })
        .unwrap();
        s.frame(0, 0).unwrap();
        let id = s.engine.document().active_layer;
        let original = s
            .engine
            .document()
            .layer(id)
            .unwrap()
            .effect
            .clone()
            .unwrap();
        let mut definition = s.effect_catalog.get("unsharp_mask").unwrap().clone();
        Arc::make_mut(&mut definition.program).label = "Runtime sharpness".into();
        let parameters = Arc::make_mut(&mut Arc::make_mut(&mut definition.program).parameters);
        parameters[0].label = "Runtime radius".into();
        let package = EffectPackage {
            format: 1,
            categories: s.effect_catalog.categories().to_vec(),
            filters: vec![definition],
        };
        let json = serde_json::to_string(&package).unwrap();
        let change = s
            .load_effect_package(
                &json,
                |_| panic!("inline sources"),
                EffectInstallMode::Replace,
            )
            .unwrap();
        assert!(change.canvas_wake);
        assert_eq!(s.state.filter_catalog_revision, 0);
        assert_eq!(
            s.engine
                .document()
                .layer(id)
                .unwrap()
                .effect
                .as_ref()
                .unwrap(),
            &original
        );
        assert!(
            s.load_effect_package(&json, |_| panic!(), EffectInstallMode::Replace)
                .is_err()
        );
        s.dispatch(UiAction::Effect {
            action: EffectAction::Set {
                layer: id.0,
                key: "amount".into(),
                value: EffectValue::Number(175.),
            },
        })
        .unwrap();
        s.frame(1, 1).unwrap();
        let request = s.renderer_mut().validation.take().unwrap();
        assert_eq!(request.programs.len(), 1);
        s.renderer_mut().validation_result = Some(layer_render::EffectValidationResult {
            request_id: request.request_id,
            result: Ok(()),
        });
        s.input_pending = true;
        assert!(s.frame(2, 2).unwrap().canvas_wake);
        assert!(
            s.state.filter_load.pending,
            "publication waits for pending pen input"
        );
        s.frame(3, 3).unwrap();
        assert!(!s.state.filter_load.pending);
        assert!(s.state.filter_load.error.is_none());
        assert_eq!(s.state.filter_catalog_revision, 1);
        let current = s
            .engine
            .document()
            .layer(id)
            .unwrap()
            .effect
            .clone()
            .unwrap();
        assert_eq!(current.program.label.as_ref(), "Runtime sharpness");
        assert_eq!(current.value("amount"), Some(&EffectValue::Number(175.)));
        assert_eq!(s.state.layer_properties.controls[0].label, "Runtime radius");
        assert_eq!(s.filter_preview_revision().2, 1);
        s.frame(4, 4).unwrap();
        // Device-side compilation failure must not publish any metadata or layers.
        s.load_effect_package(&json, |_| panic!(), EffectInstallMode::Replace)
            .unwrap();
        let request_id = s.state.filter_load.request_id;
        s.renderer_mut().validation_result = Some(layer_render::EffectValidationResult {
            request_id,
            result: Err("Invalid WGSL".into()),
        });
        s.frame(5, 5).unwrap();
        assert_eq!(s.state.filter_load.error.as_deref(), Some("Invalid WGSL"));
        assert_eq!(s.state.filter_catalog_revision, 1);
        assert_eq!(
            s.engine
                .document()
                .layer(id)
                .unwrap()
                .effect
                .as_ref()
                .unwrap(),
            &current
        );
    }

    #[test]
    fn runtime_filter_add_refreshes_catalog_and_shared_controls() {
        use layer_core::{EffectCategory, EffectInstallMode, EffectPackage};
        use std::sync::Arc;
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            let mut definition = s.effect_catalog.get("brightness_contrast").unwrap().clone();
            s.frame(0, 0).unwrap();
            let program = Arc::make_mut(&mut definition.program);
            program.id = "test:runtime".into();
            program.label = "Runtime test".into();
            definition.category = "examples".into();
            let package = EffectPackage {
                format: 1,
                categories: vec![EffectCategory {
                    id: "examples".into(),
                    label: "Examples".into(),
                }],
                filters: vec![definition],
            };
            s.load_effect_package(
                &serde_json::to_string(&package).unwrap(),
                |_| panic!(),
                EffectInstallMode::Add,
            )
            .unwrap();
            let request_id = s.state.filter_load.request_id;
            s.renderer_mut().validation_result = Some(layer_render::EffectValidationResult {
                request_id,
                result: Ok(()),
            });
            s.frame(0, 0).unwrap();
            assert_eq!(s.state.adjustments.len(), 41);
            assert_eq!(
                s.state.filter_categories.last().unwrap().label.as_ref(),
                "Examples"
            );
            let menu = s.application_menu(ApplicationMenu::Filter);
            let category = menu
                .sections
                .iter()
                .flatten()
                .find(|i| i.label == "Examples")
                .unwrap();
            let filter = &category.sections[0][0];
            assert_eq!(filter.label, "Runtime test");
            s.dispatch(filter.action.clone().unwrap()).unwrap();
            assert_eq!(s.state.layer_properties.controls.len(), 2);
        }
    }
    #[test]
    fn runtime_filter_add_cannot_replace_a_document_only_program() {
        use layer_core::{Edit, EffectInstallMode, EffectInstance, EffectPackage, Layer};
        use std::sync::Arc;
        let mut s = session();
        let mut definition = s.effect_catalog.get("brightness_contrast").unwrap().clone();
        Arc::make_mut(&mut definition.program).id = "document:custom".into();
        let id = s.engine.allocate_layer_id();
        let mut layer = Layer::paint(id, "Document-only filter");
        layer.kind = LayerKind::Effect;
        layer.effect = Some(Arc::new(EffectInstance::new(definition.program())));
        s.layer_edit(Edit::InsertLayer { index: 0, layer }).unwrap();
        s.frame(0, 0).unwrap();
        let original = s.engine.document().layer(id).unwrap().effect.clone();
        Arc::make_mut(&mut definition.program).label = "Different definition".into();
        let package = EffectPackage {
            format: 1,
            categories: s.effect_catalog.categories().to_vec(),
            filters: vec![definition],
        };
        let error = s
            .load_effect_package(
                &serde_json::to_string(&package).unwrap(),
                |_| panic!(),
                EffectInstallMode::Add,
            )
            .unwrap_err();
        assert!(error.contains("document program"));
        assert!(s.renderer_mut().validation.is_none());
        assert_eq!(s.engine.document().layer(id).unwrap().effect, original);
        assert!(s.effect_catalog.get("document:custom").is_none());
    }

    #[test]
    fn clean_close_during_library_warmup_preserves_document_and_input_guards() {
        use layer_core::{EffectInstallMode, EffectPackage};
        for library in [false, true] {
            for dirty in [false, true] {
                for interaction in [false, true] {
                    let mut s = session();
                    s.set_document_replacement(true);
                    if dirty {
                        s.dispatch(UiAction::Invoke {
                            command: CommandId::AddLayer,
                        })
                        .unwrap();
                        s.frame(0, 0).unwrap();
                    }
                    s.frame(0, 0).unwrap();
                    let package = EffectPackage {
                        format: 1,
                        categories: s.effect_catalog.categories().to_vec(),
                        filters: vec![s.effect_catalog.get("unsharp_mask").unwrap().clone()],
                    };
                    let manifest = serde_json::to_string(&package).unwrap();
                    if library {
                        s.load_effect_library(&manifest, |_| panic!(), EffectInstallMode::Merge)
                            .unwrap();
                    } else {
                        s.load_effect_package(&manifest, |_| panic!(), EffectInstallMode::Merge)
                            .unwrap();
                    }
                    assert!(s.state.filter_load.pending);
                    // A pending read-only library does not make document
                    // replacement safe: its validation still belongs to this session.
                    assert!(s.request_document_open(false).is_err());
                    assert!(s.request_document_open(true).is_err());
                    let checkpoint = s.engine.checkpoint();
                    s.input_pending = interaction;
                    assert_eq!(s.require_workspace_idle().is_ok(), library && !interaction);
                    let accepted = library && !interaction;
                    let can_close = accepted && !dirty;
                    assert_eq!(s.request_document_close().is_ok(), accepted);
                    if accepted && dirty {
                        let id = s.files.pending.as_ref().unwrap().0;
                        s.respond_document_close(id, CloseDecision::Cancel).unwrap();
                        assert!(s.state.document_file.modified);
                    }
                    assert_eq!(s.state.document_file.close_ready, can_close);
                    assert_eq!(s.engine.checkpoint(), checkpoint);
                    assert!(s.state.filter_load.pending);
                    s.input_pending = false;
                    // If application termination is cancelled, completion remains
                    // valid and a later unsaved prompt still preserves edits.
                    if can_close {
                        s.reset_document_close();
                    }
                    s.renderer_mut().validation_result =
                        Some(layer_render::EffectValidationResult {
                            request_id: s.state.filter_load.request_id,
                            result: Ok(()),
                        });
                    s.frame(1, 1).unwrap();
                    assert!(!s.state.filter_load.pending);
                    assert_eq!(s.engine.checkpoint(), checkpoint);
                    if !can_close {
                        s.request_document_close().unwrap();
                        if dirty {
                            let id = s.files.pending.as_ref().unwrap().0;
                            s.respond_document_close(id, CloseDecision::Cancel).unwrap();
                            assert!(!s.state.document_file.close_ready);
                            assert!(s.state.document_file.modified);
                        } else {
                            assert!(s.state.document_file.close_ready);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn save_and_close_during_library_warmup_waits_for_the_saved_checkpoint() {
        use layer_core::{EffectInstallMode, EffectPackage};
        let mut s = session();
        s.dispatch(UiAction::Invoke {
            command: CommandId::AddLayer,
        })
        .unwrap();
        s.frame(0, 0).unwrap();
        let document = s.engine.document().clone();
        let package = EffectPackage {
            format: 1,
            categories: s.effect_catalog.categories().to_vec(),
            filters: vec![s.effect_catalog.get("unsharp_mask").unwrap().clone()],
        };
        s.load_effect_library(
            &serde_json::to_string(&package).unwrap(),
            |_| panic!(),
            EffectInstallMode::Merge,
        )
        .unwrap();
        s.request_document_close().unwrap();
        let id = s.files.pending.as_ref().unwrap().0;
        s.respond_document_close(id, CloseDecision::Save).unwrap();
        let id = s.files.pending.as_ref().unwrap().0;
        let project = s
            .capture_project_save(
                id,
                DocumentLocation {
                    uri: "fixture.capy".into(),
                    name: "fixture.capy".into(),
                },
            )
            .unwrap();
        assert_eq!(project.document, document);
        assert!(s.state.filter_load.pending);
        assert!(s.state.document_file.modified);
        assert!(!s.state.document_file.close_ready);
        s.complete_document_request(id, Ok(true)).unwrap();
        assert!(s.state.filter_load.pending);
        assert!(!s.state.document_file.modified);
        assert!(s.state.document_file.close_ready);
        assert_eq!(s.engine.document(), &document);
    }

    #[test]
    fn startup_filter_library_does_not_rewrite_saved_programs() {
        use layer_core::{EffectInstallMode, EffectPackage};
        use std::sync::Arc;
        let mut s = session();
        s.dispatch(UiAction::Effect {
            action: EffectAction::Insert {
                effect: "unsharp_mask".into(),
            },
        })
        .unwrap();
        s.frame(0, 0).unwrap();
        let document = s.engine.document().clone();
        let original = document
            .layer(document.active_layer)
            .unwrap()
            .effect
            .as_ref()
            .unwrap()
            .program
            .clone();
        let checkpoint = s.engine.checkpoint();
        let mut definition = s.effect_catalog.get("unsharp_mask").unwrap().clone();
        Arc::make_mut(&mut definition.program).label = "New library version".into();
        let package = EffectPackage {
            format: 1,
            categories: s.effect_catalog.categories().to_vec(),
            filters: vec![definition],
        };
        s.load_effect_library(
            &serde_json::to_string(&package).unwrap(),
            |_| panic!(),
            EffectInstallMode::Replace,
        )
        .unwrap();
        assert!(
            s.renderer_mut()
                .validation
                .as_ref()
                .unwrap()
                .namespace
                .contains(&original)
        );
        s.renderer_mut().validation_result = Some(layer_render::EffectValidationResult {
            request_id: s.state.filter_load.request_id,
            result: Ok(()),
        });
        s.frame(1, 1).unwrap();
        assert_eq!(s.engine.document(), &document);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(
            s.effect_catalog.get("unsharp_mask").unwrap().label(),
            "New library version"
        );
    }

    #[test]
    fn filter_picker_search_and_categories_are_ui_only() {
        let mut s = session();
        let revision = s.engine.document().revision;
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            s.set_platform(platform);
            let update = s
                .dispatch(UiAction::FilterPicker {
                    action: FilterPickerAction::Search {
                        query: "  COLOR   balance ".into(),
                    },
                })
                .unwrap();
            assert!(!update.canvas_wake);
            assert_eq!(s.state.adjustments.len(), 1);
            assert_eq!(s.state.adjustments[0].id, "color_balance".into());
            s.dispatch(UiAction::FilterPicker {
                action: FilterPickerAction::Category {
                    category: Some("tone".into()),
                },
            })
            .unwrap();
            assert!(s.state.adjustments.is_empty());
            s.dispatch(UiAction::FilterPicker {
                action: FilterPickerAction::ToggleSearch,
            })
            .unwrap();
            assert!(s.state.filter_picker.search.is_none());
            assert!(
                s.state
                    .adjustments
                    .iter()
                    .all(|f| f.category == "tone".into())
            );
            s.dispatch(UiAction::FilterPicker {
                action: FilterPickerAction::Category { category: None },
            })
            .unwrap();
            assert_eq!(
                s.state.adjustments.len(),
                layer_core::bundled_effect_catalog().filters().len()
            );
            assert_eq!(s.engine.document().revision, revision);
            for choice in &s.state.adjustments {
                assert_eq!(
                    choice.animated,
                    s.effect_catalog.get(&choice.id).unwrap().program.time
                );
                assert_eq!(choice.tooltip.contains("Animated"), choice.animated);
            }
        }
    }

    #[test]
    fn filter_insertion_preserves_clipping_stack_and_delete_capabilities() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            for _ in 0..2 {
                s.dispatch(UiAction::Layer {
                    action: LayerAction::New {
                        group: false,
                        clipped: true,
                    },
                })
                .unwrap();
            }
            let clips: Vec<_> = s
                .engine
                .document()
                .layers
                .iter()
                .filter(|l| l.properties.clipped)
                .map(|l| l.id)
                .collect();
            let top = clips[0];
            s.dispatch(UiAction::SetLayerVisibility {
                id: top.0,
                visible: false,
            })
            .unwrap();
            for selected in [LayerId(1), clips[1], top] {
                s.dispatch(UiAction::SelectLayer { id: selected.0 })
                    .unwrap();
                assert_eq!(
                    s.state.layer_tools.can_delete,
                    selected != LayerId(1),
                    "base cannot be deleted without its clips"
                );
                s.dispatch(UiAction::Effect {
                    action: EffectAction::Insert {
                        effect: "heat_haze".into(),
                    },
                })
                .unwrap();
                let doc = s.engine.document();
                assert_eq!(doc.layers[0].id, doc.active_layer);
                assert_eq!(doc.layers[1].id, top);
                for clip in &clips {
                    assert_eq!(doc.clipping_base(*clip), Some(LayerId(1)));
                }
                assert!(s.state.layer_tools.can_delete);
                s.dispatch(UiAction::Layer {
                    action: LayerAction::DeleteSelected,
                })
                .unwrap();
                assert_eq!(s.engine.document().layers[0].id, top);
            }
            s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
            assert!(!s.state.layer_tools.can_delete, "paper is protected");
        }
    }

    #[test]
    fn layer_row_selection_references_and_editing_are_independent() {
        let mut s = session();
        let send = |s: &mut UiSession<Recorder>, action| {
            s.dispatch(UiAction::Layer { action }).unwrap();
        };
        send(
            &mut s,
            LayerAction::New {
                group: false,
                clipped: false,
            },
        );
        let second = s.engine.document().active_layer;
        send(
            &mut s,
            LayerAction::AddMask {
                id: second.0,
                replace: false,
            },
        );
        send(&mut s, LayerAction::ToggleSelection { id: 1 });
        assert_eq!(s.state.layers.iter().filter(|l| l.selected).count(), 2);
        assert_eq!(s.engine.document().active_layer, second);
        assert!(s.engine.document().active_mask);
        send(&mut s, LayerAction::ReferenceSelection);
        assert_eq!(s.engine.document().reference_layers.len(), 2);
        assert!(s.state.layer_tools.references_selected);
        assert_eq!(s.state.layers.iter().filter(|l| l.selected).count(), 1);
        assert_eq!(
            s.state
                .layer_tools
                .editing_layer
                .as_ref()
                .unwrap()
                .selection_icon,
            "layer-reference-symbolic"
        );
        send(&mut s, LayerAction::ToggleSelection { id: 1 });
        assert!(
            s.state
                .layers
                .iter()
                .filter(|l| l.selected)
                .all(|l| l.selection_icon == "layer-selection-checked-symbolic")
        );
        // The editing target can be unselected and remains editable.
        send(&mut s, LayerAction::ToggleSelection { id: second.0 });
        assert!(
            s.state
                .layers
                .iter()
                .any(|l| l.editing && !l.selected && l.mask_selected)
        );
        assert_eq!(
            s.state
                .layers
                .iter()
                .find(|l| l.id == 1)
                .unwrap()
                .selection_icon,
            "layer-selection-checked-symbolic"
        );
        assert_eq!(
            s.state
                .layers
                .iter()
                .find(|l| l.editing)
                .unwrap()
                .selection_icon,
            "layer-reference-symbolic"
        );
        // A checked reference is an add operation, never a surprising removal.
        send(&mut s, LayerAction::ReferenceSelection);
        assert_eq!(s.engine.document().reference_layers.len(), 2);
        assert_eq!(s.state.layers.iter().filter(|l| l.selected).count(), 1);
        assert_eq!(s.engine.document().active_layer, second);
        assert!(s.engine.document().active_mask);
        // Only the sole editing target toggles reference use off.
        assert_eq!(
            s.state.layer_tools.reference_action_label,
            "Stop using this layer as a reference"
        );
        send(&mut s, LayerAction::ReferenceSelection);
        assert_eq!(
            s.engine.document().reference_layers,
            std::collections::BTreeSet::from([LayerId(1)])
        );
        s.engine.undo().unwrap();
        assert_eq!(s.engine.document().reference_layers.len(), 2);
        send(&mut s, LayerAction::Delete { id: 1 });
        assert!(!s.engine.document().reference_layers.contains(&LayerId(1)));
        s.engine.undo().unwrap();
        assert_eq!(s.engine.document().reference_layers.len(), 2);
        send(&mut s, LayerAction::Select { id: 1, mask: false });
        assert_eq!(s.state.layers.iter().filter(|l| l.selected).count(), 1);
        assert!(s.state.layers.iter().any(|l| l.id == 1 && l.editing));
        assert_eq!(
            s.state
                .layers
                .iter()
                .find(|l| l.id == 1)
                .unwrap()
                .selection_icon,
            "layer-reference-symbolic"
        );
    }

    #[test]
    fn property_opacity_uses_layer_lock_and_validation_policy() {
        let property = |layer, opacity| UiAction::Effect {
            action: EffectAction::Set {
                layer,
                key: "opacity".into(),
                value: layer_core::EffectValue::Number(opacity),
            },
        };
        for kind in ["paint", "group", "inherited"] {
            let mut s = session();
            let paint = s.engine.document().active_layer.0;
            let mut target = paint;
            let mut lock = paint;
            if kind != "paint" {
                s.dispatch(UiAction::Layer {
                    action: LayerAction::New {
                        group: true,
                        clipped: false,
                    },
                })
                .unwrap();
                lock = s.engine.document().active_layer.0;
                target = lock;
                if kind == "inherited" {
                    s.dispatch(UiAction::Layer {
                        action: LayerAction::Drop {
                            id: paint,
                            target: lock,
                            fraction: 0.5,
                        },
                    })
                    .unwrap();
                    target = paint;
                }
            }
            s.dispatch(property(target, 0.35)).unwrap();
            s.dispatch(UiAction::Layer {
                action: LayerAction::Lock {
                    id: lock,
                    value: true,
                },
            })
            .unwrap();
            let revision = s.engine.document().revision;
            for action in [
                property(target, 0.55),
                UiAction::Effect {
                    action: EffectAction::Reset {
                        layer: target,
                        key: "opacity".into(),
                    },
                },
                UiAction::Effect {
                    action: EffectAction::Number {
                        layer: target,
                        key: "opacity".into(),
                        operation: NumericOperation::Step { steps: 1. },
                    },
                },
                UiAction::SetLayerOpacity {
                    id: Some(target),
                    opacity: 0.55,
                },
            ] {
                assert!(
                    s.dispatch(action).is_err(),
                    "{kind} must reject a locked opacity edit"
                );
                assert_eq!(
                    s.engine.document().revision,
                    revision,
                    "Rejected edits must not add history"
                );
                assert_eq!(
                    s.engine.document().layer(LayerId(target)).unwrap().opacity,
                    0.35
                );
            }
            s.dispatch(UiAction::Layer {
                action: LayerAction::Lock {
                    id: lock,
                    value: false,
                },
            })
            .unwrap();
            s.dispatch(property(target, 0.55)).unwrap();
            assert_eq!(
                s.engine.document().layer(LayerId(target)).unwrap().opacity,
                0.55
            );
        }
        let mut s = session();
        let target = s.engine.document().active_layer.0;
        let revision = s.engine.document().revision;
        for opacity in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert!(s.dispatch(property(target, opacity)).is_err());
            assert_eq!(s.engine.document().revision, revision);
        }
        // Paper opacity remains supported despite its protected stack position.
        let mut s = UiSession::from_project(
            Recorder::default(),
            new_drawing(64, 64).unwrap(),
            None,
            [128, 128],
        )
        .unwrap();
        let paper = s
            .engine
            .document()
            .layers
            .iter()
            .find(|l| l.kind == LayerKind::Background)
            .unwrap()
            .id
            .0;
        s.dispatch(property(paper, 0.4)).unwrap();
        assert_eq!(
            s.engine.document().layer(LayerId(paper)).unwrap().opacity,
            0.4
        );
        invoke(&mut s, CommandId::Undo);
        assert_eq!(
            s.engine.document().layer(LayerId(paper)).unwrap().opacity,
            1.
        );
    }

    #[test]
    fn canvas_tools_are_shared_toolbar_commands() {
        let mut s = session();
        for (command, tool) in [
            (CommandId::Lasso, LayerCanvasTool::Select),
            (CommandId::Move, LayerCanvasTool::Move),
            (CommandId::Brush, LayerCanvasTool::Paint),
        ] {
            s.dispatch(UiAction::Invoke { command }).unwrap();
            assert_eq!(s.state.layer_tools.tool, tool);
            for candidate in [
                CommandId::Lasso,
                CommandId::Move,
                CommandId::Brush,
                CommandId::Eraser,
            ] {
                assert_eq!(s.command(candidate).selected, candidate == command);
            }
        }
    }

    #[test]
    fn layer_context_preserves_checks_and_bulk_duplicate_keeps_clipping_stacks() {
        let mut s = session();
        let send =
            |s: &mut UiSession<Recorder>, action| s.dispatch(UiAction::Layer { action }).unwrap();
        send(
            &mut s,
            LayerAction::New {
                group: false,
                clipped: true,
            },
        );
        let shade = s.engine.document().active_layer;
        send(&mut s, LayerAction::ToggleSelection { id: 1 });
        send(
            &mut s,
            LayerAction::Context {
                id: shade.0,
                mask: false,
            },
        );
        assert_eq!(s.layer_interaction.selected.len(), 2);
        send(&mut s, LayerAction::DuplicateSelected);
        let doc = s.engine.document();
        let copies: Vec<_> = doc.ordered_layers().into_iter().take(2).collect();
        assert!(copies[0].properties.clipped);
        assert_eq!(doc.clipping_base(copies[0].id), Some(copies[1].id));
        assert_eq!(doc.clipping_base(shade), Some(LayerId(1)));
        assert_eq!(s.layer_interaction.selected.len(), 2);
        send(&mut s, LayerAction::DeleteSelected);
        assert_eq!(s.engine.document().layers.len(), 3);
        s.engine.undo().unwrap();
        assert_eq!(s.engine.document().layers.len(), 5);
    }

    #[test]
    fn paper_can_be_selected_but_never_painted_or_moved_above_artwork() {
        let mut s = session();
        s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
        assert!(
            s.state
                .layers
                .iter()
                .any(|l| l.id == 2 && l.selected && l.editing)
        );
        assert!(s.state.layer_tools.controls.opacity);
        assert!(!s.state.layer_tools.controls.mask);
        assert!(!s.state.layer_tools.controls.blend);
        assert!(!s.state.layer_tools.controls.move_layer);
        for (seq, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
            s.pen(event(&s, seq, phase, 1.)).unwrap();
        }
        s.frame(30_000_000, 38_000_000).unwrap();
        assert_eq!(s.engine.document().strokes().count(), 0);
        assert!(s.state.host_error.is_none());
        s.dispatch(UiAction::SetLayerOpacity {
            id: None,
            opacity: 0.5,
        })
        .unwrap();
        assert_eq!(s.engine.document().layer(LayerId(2)).unwrap().opacity, 0.5);
        assert!(
            s.engine
                .apply_edit(layer_core::Edit::MoveLayer {
                    id: LayerId(2),
                    to: 0
                })
                .is_err()
        );
        s.dispatch(UiAction::Layer {
            action: LayerAction::New {
                group: false,
                clipped: false,
            },
        })
        .unwrap();
        let id = s.engine.document().active_layer.0;
        s.dispatch(UiAction::Layer {
            action: LayerAction::Drop {
                id,
                target: 2,
                fraction: 1.,
            },
        })
        .unwrap();
        assert_eq!(s.state.layers.last().unwrap().id, 2);
        assert_eq!(s.state.layers[s.state.layers.len() - 2].id, id);
        s.engine
            .apply_edit(layer_core::Edit::InsertLayer {
                index: usize::MAX,
                layer: layer_core::Layer::paint(LayerId(99), "Bottom"),
            })
            .unwrap();
        assert_eq!(
            s.engine.document().ordered_layers().last().unwrap().id,
            LayerId(2)
        );
    }

    #[test]
    fn copied_masks_are_independent_and_keep_their_canvas_position() {
        let mut s = session();
        let send =
            |s: &mut UiSession<Recorder>, action| s.dispatch(UiAction::Layer { action }).unwrap();
        send(
            &mut s,
            LayerAction::AddMask {
                id: 1,
                replace: false,
            },
        );
        for (seq, phase) in [(1, PenPhase::Down), (2, PenPhase::Move), (3, PenPhase::Up)] {
            s.pen(event(&s, seq, phase, 1.)).unwrap();
        }
        s.frame(30_000_000, 38_000_000).unwrap();
        let mut source = s.engine.document().layer(LayerId(1)).unwrap().clone();
        source.mask.as_mut().unwrap().offset = Point { x: 11., y: 17. };
        s.engine
            .apply_edit(layer_core::Edit::ReplaceLayer(Box::new(source.clone())))
            .unwrap();
        send(&mut s, LayerAction::Lock { id: 1, value: true });
        send(&mut s, LayerAction::CopyMask { id: 1 });
        send(
            &mut s,
            LayerAction::New {
                group: true,
                clipped: false,
            },
        );
        let parent = s.engine.document().active_layer;
        let mut group = s.engine.document().layer(parent).unwrap().clone();
        group.properties.offset = Point { x: 30., y: 50. };
        s.engine
            .apply_edit(layer_core::Edit::ReplaceLayer(Box::new(group)))
            .unwrap();
        send(
            &mut s,
            LayerAction::New {
                group: false,
                clipped: false,
            },
        );
        let target = s.engine.document().active_layer;
        send(&mut s, LayerAction::PasteMask { id: target.0 });
        let doc = s.engine.document();
        let original = doc.layer(LayerId(1)).unwrap().mask.as_ref().unwrap();
        let copy = doc.layer(target).unwrap().mask.as_ref().unwrap();
        assert_ne!(original.id, copy.id);
        assert_eq!(doc.layer_offset(original.id), doc.layer_offset(copy.id));
        assert_eq!(original.strokes.len(), 1);
        assert_ne!(original.strokes[0], copy.strokes[0]);
        assert!(std::sync::Arc::ptr_eq(
            &doc.stroke(original.strokes[0]).unwrap().points,
            &doc.stroke(copy.strokes[0]).unwrap().points
        ));
        s.engine.undo().unwrap();
        assert!(s.engine.document().layer(target).unwrap().mask.is_none());
        assert_eq!(
            s.engine.document().layer(LayerId(1)).unwrap().mask,
            source.mask
        );
    }

    #[test]
    fn dropping_into_a_closed_group_expands_it_without_changing_edit_target() {
        let mut s = session();
        s.dispatch(UiAction::Layer {
            action: LayerAction::New {
                group: true,
                clipped: false,
            },
        })
        .unwrap();
        let group = s.engine.document().active_layer;
        for action in [
            LayerAction::Collapse { id: group.0 },
            LayerAction::Select { id: 1, mask: false },
            LayerAction::Drop {
                id: 1,
                target: group.0,
                fraction: 0.5,
            },
        ] {
            s.dispatch(UiAction::Layer { action }).unwrap();
        }
        assert!(!s.layer_interaction.collapsed.contains(&group));
        assert_eq!(s.engine.document().active_layer, LayerId(1));
        assert!(
            s.state
                .layers
                .iter()
                .any(|l| l.id == 1 && l.depth == 1 && l.editing)
        );
    }

    #[test]
    fn layer_mask_creation_deletion_apply_and_targets_round_trip_history() {
        use layer_core::{LayerOperationKind, Selection};
        let mut s = session();
        let id = s.engine.document().active_layer.0;
        let selection = Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 100., y: 0. },
            Point { x: 100., y: 100. },
        ])
        .unwrap();
        s.engine
            .apply_edit(layer_core::Edit::SetSelection(Some(selection.clone())))
            .unwrap();
        s.layer_action(LayerAction::AddMask { id, replace: false })
            .unwrap();
        assert!(s.engine.document().active_mask);
        assert!(s.engine.document().selection.is_none());
        s.engine.undo().unwrap();
        assert!(!s.engine.document().active_mask);
        assert_eq!(s.engine.document().selection, Some(selection));
        s.engine.redo().unwrap();
        assert!(s.engine.document().active_mask);
        let mask = s.engine.document().layer(LayerId(id)).unwrap().mask.clone();
        for apply in [false, true] {
            s.layer_action(if apply {
                LayerAction::ApplyMask { id }
            } else {
                LayerAction::DeleteMask { id }
            })
            .unwrap();
            assert!(!s.engine.document().active_mask);
            let l = s.engine.document().layer(LayerId(id)).unwrap();
            assert!(l.mask.is_none());
            if apply {
                assert_eq!(
                    l.operations.last().unwrap().kind,
                    LayerOperationKind::ApplyMask
                );
            }
            s.engine.undo().unwrap();
            assert!(s.engine.document().active_mask);
            assert_eq!(s.engine.document().layer(LayerId(id)).unwrap().mask, mask);
        }
    }

    #[test]
    fn undo_mask_removal_after_navigating_restores_the_owner_target() {
        let mut s = session();
        let first = s.engine.document().active_layer;
        s.layer_action(LayerAction::New {
            group: false,
            clipped: false,
        })
        .unwrap();
        let second = s.engine.document().active_layer;
        s.layer_action(LayerAction::AddMask {
            id: second.0,
            replace: false,
        })
        .unwrap();
        s.layer_action(LayerAction::DeleteMask { id: second.0 })
            .unwrap();
        s.layer_action(LayerAction::Select {
            id: first.0,
            mask: false,
        })
        .unwrap();
        s.engine.undo().unwrap();
        assert_eq!(s.engine.document().active_layer, second);
        assert!(s.engine.document().active_mask);
    }

    #[test]
    fn reparent_keeps_world_position_and_group_rows_travel_together() {
        let mut s = session();
        let id = s.engine.document().active_layer.0;
        s.layer_action(LayerAction::AddMask { id, replace: false })
            .unwrap();
        s.layer_action(LayerAction::New {
            group: true,
            clipped: false,
        })
        .unwrap();
        let group = s.engine.document().active_layer;
        let edit = s
            .engine
            .document()
            .move_target_edit(Point { x: 50., y: 90. })
            .unwrap();
        s.engine.apply_edit(edit).unwrap();
        s.layer_action(LayerAction::Reparent {
            id,
            parent: Some(group.0),
            index: 0,
        })
        .unwrap();
        let doc = s.engine.document();
        assert_eq!(doc.layer_offset(LayerId(id)), Point::default());
        assert_eq!(
            doc.layer_offset(doc.layer(LayerId(id)).unwrap().mask.as_ref().unwrap().id),
            Point::default()
        );
        let rows = doc.ordered_layers();
        let g = rows.iter().position(|l| l.id == group).unwrap();
        assert_eq!(rows[g + 1].id, LayerId(id));
        assert!(
            s.layer_action(LayerAction::Reparent {
                id: group.0,
                parent: Some(id),
                index: 0
            })
            .is_err()
        );
        s.layer_action(LayerAction::Solo { id: group.0 }).unwrap();
        assert!(s.engine.document().layer(LayerId(id)).unwrap().visible);
    }
    #[test]
    fn workspace_management_is_shared_transactional_and_independent_of_artwork() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            let original = s.state.workspace.clone();
            let revision = s.engine.document().revision;
            let edit = |s: &mut UiSession<Recorder>, action| {
                s.dispatch(UiAction::Customize { action }).unwrap()
            };
            let menu = s.workspace_menu();
            assert_eq!(
                menu.sections[1].len(),
                Panel::ALL
                    .iter()
                    .filter(|p| p.available_on(platform) && p.kind() == PanelKind::Content)
                    .count()
            );
            assert_eq!(
                menu.sections[2].len(),
                1 + usize::from(Panel::Commands.available_on(platform))
            );
            assert_eq!(
                menu.sections[1]
                    .iter()
                    .filter(|i| i.selected == Some(true))
                    .count(),
                5
            );
            assert!(!menu.sections[0][0].enabled);
            assert_eq!(menu.sections[0][0].hint, "Ctrl+Alt+Z");
            edit(
                &mut s,
                CustomizationAction::SetPanelVisible {
                    panel: Panel::Brushes,
                    visible: false,
                },
            );
            assert!(
                s.state
                    .workspace
                    .layout
                    .panel_group(Panel::Brushes)
                    .is_none()
            );
            assert_eq!(
                s.state.workspace.layout.panel(Panel::Brushes).unwrap(),
                original.layout.panel(Panel::Brushes).unwrap()
            );
            assert_eq!(s.workspace_menu().sections[1][0].selected, Some(false));
            let saved = s.state.workspace.clone();
            saved.validate().unwrap();
            invoke(&mut s, CommandId::UndoWorkspace);
            assert_eq!(s.state.workspace, original);
            invoke(&mut s, CommandId::RedoWorkspace);
            assert_eq!(s.state.workspace, saved);
            edit(
                &mut s,
                CustomizationAction::AddPanel {
                    panel: Panel::Brushes,
                    group: 8,
                },
            );
            assert_eq!(
                s.state.workspace.layout.group_panels(8).unwrap(),
                &[
                    Panel::Layers,
                    Panel::Adjustments,
                    Panel::Properties,
                    Panel::Brushes
                ]
            );
            assert!(!s.command(CommandId::RedoWorkspace).enabled);
            edit(
                &mut s,
                CustomizationAction::DuplicateToolbar {
                    panel: Panel::Toolbar,
                },
            );
            let proposed = s.toolbar_prompt().unwrap().name.unwrap();
            assert_eq!(proposed, "Tools Copy");
            edit(
                &mut s,
                CustomizationAction::ToolbarName {
                    name: "Layers".into(),
                },
            );
            assert!(!s.toolbar_prompt().unwrap().can_confirm);
            let before = s.state.workspace.clone();
            edit(&mut s, CustomizationAction::ConfirmToolbar);
            assert_eq!(s.state.workspace, before);
            edit(&mut s, CustomizationAction::ToolbarName { name: proposed });
            edit(&mut s, CustomizationAction::ConfirmToolbar);
            let copied = s.state.workspace.layout.panels.last().unwrap().clone();
            assert_eq!(
                copied.tiles().len(),
                original.layout.panel(Panel::Toolbar).unwrap().tiles().len()
            );
            assert_ne!(
                copied.tiles()[0].id,
                original.layout.panel(Panel::Toolbar).unwrap().tiles()[0].id
            );
            edit(
                &mut s,
                CustomizationAction::SetTileStyle {
                    panel: copied.id,
                    style: TileStyle::Labeled,
                },
            );
            let labeled = s.state.workspace.clone();
            invoke(&mut s, CommandId::UndoWorkspace);
            assert_eq!(
                s.state
                    .workspace
                    .layout
                    .panel(copied.id)
                    .unwrap()
                    .tile_style,
                TileStyle::Small
            );
            invoke(&mut s, CommandId::RedoWorkspace);
            assert_eq!(s.state.workspace, labeled);
            edit(
                &mut s,
                CustomizationAction::RenameToolbar { panel: copied.id },
            );
            edit(
                &mut s,
                CustomizationAction::ToolbarName {
                    name: "Painting".into(),
                },
            );
            edit(&mut s, CustomizationAction::ConfirmToolbar);
            assert_eq!(
                s.state.workspace.layout.panel(copied.id).unwrap().title(),
                "Painting"
            );
            edit(
                &mut s,
                CustomizationAction::DeleteToolbar { panel: copied.id },
            );
            let prompt = s.toolbar_prompt().unwrap();
            assert!(prompt.destructive && prompt.name.is_none());
            assert!(prompt.message.contains("Undo Layout Change (Ctrl+Alt+Z)"));
            let before = s.state.workspace.clone();
            edit(&mut s, CustomizationAction::ConfirmToolbar);
            assert!(s.state.workspace.layout.panel(copied.id).is_err());
            invoke(&mut s, CommandId::UndoWorkspace);
            assert_eq!(s.state.workspace, before);
            assert_eq!(s.engine.document().revision, revision);
            assert!(!s.command(CommandId::Undo).enabled);
            assert!(
                s.dispatch(UiAction::Customize {
                    action: CustomizationAction::DeleteToolbar {
                        panel: Panel::Layers
                    }
                })
                .is_err()
            );
            s.state.workspace.validate().unwrap();
            s.dispatch(UiAction::RestoreWorkspace { workspace: saved })
                .unwrap();
            assert!(!s.command(CommandId::UndoWorkspace).enabled);
        }
    }

    #[test]
    fn toolbar_manager_selects_hidden_toolbars_and_deletes_with_confirmation_and_undo() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            let edit = |s: &mut UiSession<Recorder>, action| {
                s.dispatch(UiAction::Customize { action }).unwrap();
            };
            edit(
                &mut s,
                CustomizationAction::DuplicateToolbar {
                    panel: Panel::Toolbar,
                },
            );
            edit(&mut s, CustomizationAction::ConfirmToolbar);
            let copy = s.state.workspace.layout.panels.last().unwrap().id;
            edit(
                &mut s,
                CustomizationAction::SetPanelVisible {
                    panel: copy,
                    visible: false,
                },
            );
            let saved = s.state.workspace.clone();
            let revision = s.engine.document().revision;
            invoke(&mut s, CommandId::ManageToolbars);
            assert!(s.state.customization.is_open());
            assert_eq!(s.state.workspace, saved);
            let view = s.toolbar_manager().unwrap();
            assert_eq!(view.toolbars.len(), 2);
            assert!(view.delete_action.is_none());
            assert!(
                view.toolbars
                    .iter()
                    .find(|p| p.panel == copy)
                    .unwrap()
                    .subtitle
                    .ends_with("Hidden")
            );
            assert_eq!(
                s.workspace_menu()
                    .sections
                    .last()
                    .unwrap()
                    .iter()
                    .map(|i| i.label.as_str())
                    .collect::<Vec<_>>(),
                ["New Toolbar…", "Manage Toolbars…"]
            );
            let options = s.panel_view(Panel::Toolbar).unwrap().toolbar_options;
            assert!(
                !options
                    .iter()
                    .flatten()
                    .any(|i| i.label.starts_with("Delete "))
            );
            let menu = s
                .context_menu(ContextTarget::Ribbon {
                    panel: Panel::Toolbar,
                })
                .unwrap();
            assert!(
                !menu
                    .sections
                    .iter()
                    .flatten()
                    .any(|i| i.label.starts_with("Delete "))
            );
            assert!(
                s.dispatch(UiAction::Customize {
                    action: CustomizationAction::SelectManagedToolbar {
                        panel: Some(Panel::Layers)
                    }
                })
                .is_err()
            );
            assert!(s.toolbar_manager().unwrap().selected.is_none());
            edit(
                &mut s,
                CustomizationAction::SelectManagedToolbar { panel: Some(copy) },
            );
            let delete = s.toolbar_manager().unwrap().delete_action.unwrap();
            edit(&mut s, delete.clone());
            assert!(s.toolbar_prompt().unwrap().destructive);
            edit(&mut s, CustomizationAction::CancelToolbar);
            assert_eq!(s.state.workspace, saved);
            assert_eq!(s.toolbar_manager().unwrap().selected, Some(copy));
            edit(&mut s, delete);
            edit(&mut s, CustomizationAction::ConfirmToolbar);
            assert!(s.state.workspace.layout.panel(copy).is_err());
            assert_eq!(s.toolbar_manager().unwrap().toolbars.len(), 1);
            assert!(s.toolbar_manager().unwrap().delete_action.is_none());
            invoke(&mut s, CommandId::UndoWorkspace);
            assert_eq!(s.state.workspace, saved);
            assert!(s.toolbar_manager().is_none());
            invoke(&mut s, CommandId::ManageToolbars);
            for panel in [Panel::Toolbar, copy] {
                edit(
                    &mut s,
                    CustomizationAction::SelectManagedToolbar { panel: Some(panel) },
                );
                edit(&mut s, CustomizationAction::DeleteToolbar { panel });
                edit(&mut s, CustomizationAction::ConfirmToolbar);
            }
            let empty = s.toolbar_manager().unwrap();
            assert!(empty.toolbars.is_empty() && empty.delete_action.is_none());
            edit(&mut s, CustomizationAction::CloseToolbarManager);
            assert!(!s.state.customization.is_open());
            assert_eq!(s.engine.document().revision, revision);
            assert!(!s.command(CommandId::Undo).enabled);
        }
    }

    #[test]
    fn workspace_resize_history_coalesces_and_cancel_restores_geometry() {
        let mut s = session();
        let viewport = [1200.0, 900.0];
        let before = s.state.workspace.clone();
        let d = s.layout(viewport).dividers[0].clone();
        let point = [
            d.bounds.x + d.bounds.width / 2.0,
            d.bounds.y + d.bounds.height / 2.0,
        ];
        for end in [ContactPhase::Cancel, ContactPhase::Up] {
            for (phase, offset) in [
                (ContactPhase::Down, 0.0),
                (ContactPhase::Move, 20.0),
                (ContactPhase::Move, 40.0),
                (end, 40.0),
            ] {
                s.dispatch(UiAction::DragDivider {
                    id: d.id,
                    phase,
                    position: [point[0] + offset, point[1]],
                    viewport,
                })
                .unwrap();
            }
            if end == ContactPhase::Cancel {
                assert_eq!(s.state.workspace, before);
                assert!(!s.command(CommandId::UndoWorkspace).enabled);
            } else {
                assert_ne!(s.state.workspace, before);
                invoke(&mut s, CommandId::UndoWorkspace);
                assert_eq!(s.state.workspace, before);
                assert!(!s.command(CommandId::UndoWorkspace).enabled);
            }
        }
    }
    fn key(
        s: &mut UiSession<Recorder>,
        name: &str,
        pressed: bool,
        command: bool,
        editing: bool,
    ) -> InputReply {
        s.input(UiInput::Key {
            key: name.into(),
            pressed,
            repeat: false,
            modifiers: Modifiers {
                command,
                ..Modifiers::default()
            },
            editing,
            divider: None,
        })
        .unwrap()
    }
    fn pointer(
        s: &mut UiSession<Recorder>,
        id: u64,
        phase: ContactPhase,
        position: [f32; 2],
        button: PointerButton,
    ) -> InputReply {
        s.input(UiInput::Pointer {
            id,
            phase,
            position,
            button,
            kind: PointerKind::Pen,
        })
        .unwrap()
    }
    fn chrome(s: &mut UiSession<Recorder>, event: ChromeEvent, facts: ChromeFacts) -> InputReply {
        s.input(UiInput::Chrome {
            event,
            facts,
            viewport: [1200.0, 900.0],
        })
        .unwrap()
    }
    #[test]
    fn partial_zen_never_reveals_on_proximity_or_consumes_drawing() {
        let mut s = session();
        s.state.settings.total_zen = true;
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::Settings);
        edit_preference(&mut s, PreferenceId::TotalZen, PreferenceValue::Bool(false));
        assert!(matches!(s.state.requests.last().unwrap().kind,
            HostRequestKind::SaveSettings { ref settings }
                if !settings.total_zen));
        s.dispatch(UiAction::CloseSettings).unwrap();
        let viewport = [1200.0, 900.0];
        s.dispatch(UiAction::MovePanel {
            panel: Panel::Sizes,
            viewport,
            target: DockTarget::Float {
                position: [600.0, 450.0],
            },
        })
        .unwrap();
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::ShowAllControls {
                panel: Panel::Sizes,
            },
        })
        .unwrap();
        assert!(s.state.customization.expanded.is_some());
        let camera = s.state.camera.clone();
        let layout = s.state.workspace.layout.clone();
        invoke(&mut s, CommandId::ZenMode);
        let assert_hidden = |reply: InputReply| {
            assert!(reply.chrome_hidden && reply.hide_floating_panels);
            reply
        };
        let facts = ChromeFacts::default();
        for position in [
            [6.0, 6.0],
            [600.0, 450.0],
            [1199.0, 450.0],
            [600.0, 899.0],
            [1.0, 450.0],
            [600.0, 1.0],
        ] {
            assert_hidden(chrome(&mut s, ChromeEvent::Motion { position }, facts));
            let reply = assert_hidden(chrome(
                &mut s,
                ChromeEvent::Contact {
                    position,
                    canvas: true,
                },
                facts,
            ));
            assert!(
                !reply.handled,
                "hidden panels/drawers must not eat the first contact"
            );
            assert!(
                s.drop_hint(
                    viewport,
                    position,
                    &[],
                    DockItem::Panel {
                        panel: Panel::Brushes,
                    },
                    None
                )
                .is_none(),
                "normal docking targets are hidden in partial Zen"
            );
        }
        for facts in [
            ChromeFacts {
                held: true,
                ..facts
            },
            ChromeFacts {
                dragging: true,
                ..facts
            },
            ChromeFacts {
                popup_open: true,
                ..facts
            },
        ] {
            assert_hidden(chrome(&mut s, ChromeEvent::Refresh, facts));
        }
        assert_hidden(chrome(&mut s, ChromeEvent::Leave { touch: false }, facts));
        assert_hidden(s.input(UiInput::Blur).unwrap());
        assert_hidden(key(&mut s, "Escape", true, false, false));
        assert_hidden(key(&mut s, "Escape", false, false, false));
        assert!(
            assert_hidden(pointer(
                &mut s,
                1,
                ContactPhase::Down,
                [1.0, 450.0],
                PointerButton::Primary
            ))
            .paint
        );
        assert!(
            assert_hidden(pointer(
                &mut s,
                1,
                ContactPhase::Move,
                [600.0, 450.0],
                PointerButton::Primary
            ))
            .paint
        );
        assert!(
            assert_hidden(pointer(
                &mut s,
                1,
                ContactPhase::Up,
                [600.0, 450.0],
                PointerButton::Primary
            ))
            .paint
        );
        // Explicit modal shortcuts remain usable, without revealing the editor
        // behind them, including the early search/shortcut-capture replies.
        invoke(&mut s, CommandId::Settings);
        assert_hidden(key(&mut s, "p", true, false, false));
        assert_eq!(s.preferences().unwrap().query, "p");
        preference(
            &mut s,
            PreferenceAction::BeginShortcut {
                id: CommandId::Brush.shortcut_id(),
            },
        );
        assert_hidden(key(&mut s, "w", true, false, false));
        assert!(s.preferences().unwrap().capture.unwrap().chord.is_some());
        s.dispatch(UiAction::CloseSettings).unwrap();
        invoke(&mut s, CommandId::ZenMode);
        let reply = chrome(&mut s, ChromeEvent::Refresh, facts);
        assert!(!reply.chrome_hidden && !reply.hide_floating_panels);
        assert!(!s.state.workspace.zen_mode);
        assert_eq!(s.state.workspace.layout, layout);
        assert_eq!(s.state.camera, camera);
    }

    #[test]
    fn partial_zen_has_the_same_policy_on_all_hosts() {
        for platform in [
            Platform::Generic,
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
            let mut s = session();
            s.set_platform(platform);
            s.dispatch(UiAction::RestoreSettings {
                settings: Settings {
                    total_zen: false,
                    ..Settings::default()
                },
            })
            .unwrap();
            invoke(&mut s, CommandId::ZenMode);
            let facts = ChromeFacts::default();
            assert!(
                chrome(
                    &mut s,
                    ChromeEvent::Motion {
                        position: [600.0, 450.0]
                    },
                    facts
                )
                .chrome_hidden
            );
            let reply = chrome(
                &mut s,
                ChromeEvent::Motion {
                    position: [6.0, 6.0],
                },
                facts,
            );
            assert!(reply.chrome_hidden);
            assert!(reply.hide_floating_panels);
        }
    }

    #[test]
    fn zen_context_menu_uses_preference_options_and_edits_without_a_dialog() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let target = ContextTarget::ZenMode;
        for total in [true, false] {
            let menu = s.context_menu(target).unwrap();
            assert_eq!(menu.title, "Zen mode");
            assert_eq!(menu.sections.len(), 2);
            assert_eq!(menu.sections[0].len(), 1);
            assert_eq!(menu.sections[0][0].label, "Total zen");
            let change = s
                .dispatch(menu.sections[0][0].action.clone().unwrap())
                .unwrap();
            assert_eq!(s.state.settings.total_zen, total);
            assert!(!s.state.settings_open && !s.state.workspace.zen_mode);
            assert_ne!(change.regions & regions::SETTINGS, 0);
            assert_eq!(
                s.context_menu(target).unwrap().sections[0][0].selected,
                Some(total)
            );
            assert!(matches!(
                s.state.requests.last().unwrap().kind,
                HostRequestKind::SaveSettings { .. }
            ));
        }
        assert!(
            s.dispatch(UiAction::Preferences {
                action: PreferenceAction::Search {
                    query: "Zen".into()
                },
            })
            .is_err()
        );
        let saved = s.state.settings.clone();
        let requests = s.state.requests.len();
        let menu = s.context_menu(target).unwrap();
        assert_eq!(menu.sections[1][0].label, "Change icon…");
        s.dispatch(menu.sections[1][0].action.clone().unwrap())
            .unwrap();
        let view = s.preferences().unwrap();
        assert_eq!(view.page, SettingsPage::Appearance);
        assert_eq!(view.reveal, Some(PreferenceId::ZenIcon));
        assert_eq!(s.state.settings, saved);
        assert_eq!(s.state.requests.len(), requests);
        for platform in [Platform::Web, Platform::Android] {
            s.set_platform(platform);
            assert_eq!(
                serde_json::to_value(s.context_menu(target).unwrap()).unwrap(),
                serde_json::to_value(&menu).unwrap()
            );
        }
    }

    #[test]
    fn zen_icons_are_core_choices_with_live_icons_and_reset() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let id = PreferenceId::ZenIcon;
        let field = |s: &Settings, platform| {
            s.pages(platform)
                .into_iter()
                .flat_map(|p| p.groups)
                .flat_map(|g| g.rows)
                .find(|r| r.id == id)
        };
        let row = field(&s.state.settings, Platform::Gtk).unwrap();
        assert!(matches!(row.kind, PreferenceKind::Choice {
            presentation: ChoicePresentation::ImageTiles { columns: 4 }, selected: 0, ref options, ref icons,
        } if options.len() == 4 && icons.len() == 4));
        assert_eq!(row.reset.unwrap().value, "Looking up");
        let camera = s.state.camera.clone();
        for (index, (symbol, _)) in ZenIcon::CHOICES.into_iter().enumerate() {
            let change = s
                .dispatch(UiAction::Preferences {
                    action: PreferenceAction::Edit {
                        id,
                        value: PreferenceValue::Choice(index as u32),
                    },
                })
                .unwrap();
            assert_eq!(s.command(CommandId::ZenMode).icon, Some(symbol.icon()));
            assert_eq!(
                s.state
                    .commands
                    .iter()
                    .find(|c| c.id == CommandId::ZenMode)
                    .unwrap()
                    .icon,
                Some(symbol.icon())
            );
            assert_eq!(s.state.settings.zen_icon, symbol);
            assert!(!s.state.workspace.zen_mode);
            assert_eq!(s.state.camera, camera);
            assert_eq!(change.regions & regions::CAMERA, 0);
            let saved = serde_json::to_string(&s.state.settings).unwrap();
            assert_eq!(
                serde_json::from_str::<Settings>(&saved).unwrap(),
                s.state.settings
            );
            assert!(ui_catalog().icons.contains(&symbol.icon()));
        }
        let before = s.state.settings.clone();
        preference(
            &mut s,
            PreferenceAction::Edit {
                id,
                value: PreferenceValue::Choice(4),
            },
        );
        assert_eq!(s.state.settings, before);
        assert!(s.state.preferences.error.is_some());
        preference(&mut s, PreferenceAction::Reset { id });
        assert_eq!(s.state.settings.zen_icon, ZenIcon::LookingUp);
        assert!(
            !field(&s.state.settings, Platform::Gtk)
                .unwrap()
                .reset
                .unwrap()
                .enabled
        );
        for platform in [Platform::Web, Platform::Android] {
            s.set_platform(platform);
            assert!(field(&s.state.settings, platform).is_some());
            preference(
                &mut s,
                PreferenceAction::Edit {
                    id,
                    value: PreferenceValue::Choice(3),
                },
            );
            assert_eq!(
                s.command(CommandId::ZenMode).icon,
                Some(ZenIcon::Sleeping.icon())
            );
        }
    }

    #[test]
    fn tab_toggles_zen_but_preserves_editor_navigation_and_custom_bindings() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            assert_eq!(s.command(CommandId::ZenMode).shortcut, "Tab");
            assert!(!key(&mut s, "Tab", true, false, true).handled);
            key(&mut s, "Tab", false, false, true);
            assert!(!s.state.workspace.zen_mode);
            invoke(&mut s, CommandId::Settings);
            assert!(!key(&mut s, "Tab", true, false, false).handled);
            key(&mut s, "Tab", false, false, false);
            s.dispatch(UiAction::CloseSettings).unwrap();
            assert!(key(&mut s, "Tab", true, false, false).handled);
            assert!(s.state.workspace.zen_mode);
            key(&mut s, "Tab", false, false, false);
            assert!(key(&mut s, "Tab", true, false, false).handled);
            assert!(!s.state.workspace.zen_mode);
            key(&mut s, "Tab", false, false, false);
            s.state.settings.shortcuts.insert(
                CommandId::ZenMode.shortcut_id(),
                vec![KeyChord::new("z", Modifiers::default())],
            );
            assert!(!key(&mut s, "Tab", true, false, false).handled);
            assert!(key(&mut s, "z", true, false, false).handled);
            assert!(s.state.workspace.zen_mode);
        }
    }

    #[test]
    fn total_and_partial_zen_have_one_shared_policy() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for total in [false, true] {
                let mut s = session();
                s.set_platform(platform);
                s.dispatch(UiAction::RestoreSettings {
                    settings: Settings {
                        total_zen: total,
                        ..Settings::default()
                    },
                })
                .unwrap();
                let layout = s.state.workspace.layout.clone();
                invoke(&mut s, CommandId::ZenMode);
                let reply = chrome(
                    &mut s,
                    ChromeEvent::Motion {
                        position: [600.0, 450.0],
                    },
                    ChromeFacts::default(),
                );
                assert!(reply.chrome_hidden);
                assert_eq!(reply.hide_floating_panels, !total);
                assert_eq!(reply.keep_zen_button, !total);
                assert_eq!(reply.partial_zen, !total);
                let reply = chrome(
                    &mut s,
                    ChromeEvent::Motion {
                        position: [6.0, 6.0],
                    },
                    ChromeFacts::default(),
                );
                assert_eq!(reply.chrome_hidden, !total);
                assert_eq!(reply.keep_zen_button, !total);
                let exit = key(&mut s, "Tab", true, false, false);
                assert!(
                    exit.handled
                        && !exit.chrome_hidden
                        && !exit.hide_floating_panels
                        && !exit.partial_zen
                );
                assert!(!s.state.workspace.zen_mode);
                assert_eq!(s.state.workspace.layout, layout);
            }
        }
    }

    #[test]
    fn retired_global_panel_toggle_does_not_break_saved_workspaces_or_settings() {
        let mut s = session();
        let mut saved = serde_json::to_value(&s.state.workspace).unwrap();
        saved["layout"]["panels_visible"] = false.into();
        saved["layout"]["panels"][0]["content"]["tiles"][0]["control"]["command"] =
            "toggle_panels".into();
        s.dispatch(UiAction::RestoreWorkspace {
            workspace: serde_json::from_value(saved).unwrap(),
        })
        .unwrap();
        assert!(!s.layout([1200.0, 900.0]).groups.is_empty());
        assert_eq!(
            s.state
                .workspace
                .layout
                .panel(Panel::Toolbar)
                .unwrap()
                .tiles()[0]
                .control,
            ToolbarControl::Command {
                command: CommandId::ZenMode
            }
        );
        let serialized = serde_json::to_string(&s.state.workspace).unwrap();
        assert!(!serialized.contains("panels_visible") && !serialized.contains("toggle_panels"));
        let saved = serde_json::json!({ "type": "restore_settings", "settings": {
            "pressure_gamma": 1.5, "shortcuts": {
                "command.TogglePanels": [{"key":"h","command":false,"alt":false,"shift":false}],
                "command.Brush": []
            }
        }});
        s.dispatch(serde_json::from_value(saved).unwrap()).unwrap();
        assert_eq!(s.state.settings.pressure_gamma, 1.5);
        assert_eq!(s.state.settings.shortcuts.len(), 1);
        assert!(s.state.settings.shortcuts.contains_key("command.Brush"));
        invoke(&mut s, CommandId::Settings);
        assert!(
            !s.preferences()
                .unwrap()
                .shortcuts
                .iter()
                .any(|r| r.id == "command.TogglePanels")
        );
        assert!(
            !serde_json::to_string(&s.state.commands)
                .unwrap()
                .contains("toggle_panels")
        );
        assert!(
            !serde_json::to_string(MENUS)
                .unwrap()
                .contains("toggle_panels")
        );
    }

    #[test]
    fn zen_visibility_pinning_and_first_contact_are_core_state() {
        let mut s = session();
        s.state.settings.total_zen = true;
        invoke(&mut s, CommandId::ZenMode);
        let motion = |p| ChromeEvent::Motion { position: p };
        let touch = |p| ChromeEvent::Contact {
            position: p,
            canvas: true,
        };
        let facts = ChromeFacts::default();
        assert!(chrome(&mut s, motion([600.0, 450.0]), facts).chrome_hidden);
        assert!(
            !chrome(&mut s, touch([600.0, 898.0]), facts).handled,
            "HUD is not a bottom panel"
        );
        let first = chrome(&mut s, touch([20.0, 450.0]), facts);
        assert!(first.handled && !first.chrome_hidden);
        assert!(!chrome(&mut s, touch([20.0, 450.0]), facts).handled);
        assert!(!chrome(&mut s, ChromeEvent::Leave { touch: true }, facts).chrome_hidden);
        assert!(chrome(&mut s, motion([600.0, 450.0]), facts).chrome_hidden);
        for facts in [
            ChromeFacts {
                held: true,
                ..facts
            },
            ChromeFacts {
                dragging: true,
                ..facts
            },
            ChromeFacts {
                popup_open: true,
                ..facts
            },
        ] {
            assert!(!chrome(&mut s, ChromeEvent::Refresh, facts).chrome_hidden);
        }
        let dismiss = chrome(
            &mut s,
            touch([600.0, 450.0]),
            ChromeFacts {
                popup_open: true,
                ..facts
            },
        );
        assert!(dismiss.handled && dismiss.dismiss_popups);
        invoke(&mut s, CommandId::Settings);
        assert!(!chrome(&mut s, ChromeEvent::Refresh, facts).chrome_hidden);
        s.dispatch(UiAction::CloseSettings).unwrap();
        assert!(chrome(&mut s, ChromeEvent::Refresh, facts).chrome_hidden);
        assert!(!key(&mut s, "Tab", true, false, false).chrome_hidden);
        assert!(!s.state.workspace.zen_mode);
        assert!(!chrome(&mut s, motion([600.0, 450.0]), facts).chrome_hidden);
    }
    #[test]
    fn enabling_zen_hides_immediately_and_guards_the_activation_corner() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.state.settings.total_zen = true;
            s.set_platform(platform);
            let facts = ChromeFacts::default();
            chrome(
                &mut s,
                ChromeEvent::Motion {
                    position: [24.0, 24.0],
                },
                facts,
            );
            invoke(&mut s, CommandId::ZenMode);
            assert!(s.interaction.hidden);
            for event in [
                ChromeEvent::Refresh,
                ChromeEvent::Motion {
                    position: [24.0, 24.0],
                },
                ChromeEvent::Motion {
                    position: [299.0, 79.0],
                },
                ChromeEvent::Motion {
                    position: [79.0, 299.0],
                },
            ] {
                assert!(chrome(&mut s, event, facts).chrome_hidden);
            }
            assert!(
                chrome(
                    &mut s,
                    ChromeEvent::Motion {
                        position: [500.0, 400.0]
                    },
                    facts
                )
                .chrome_hidden
            );
            assert!(
                !chrome(
                    &mut s,
                    ChromeEvent::Motion {
                        position: [24.0, 24.0]
                    },
                    facts
                )
                .chrome_hidden
            );
            invoke(&mut s, CommandId::ZenMode);
            invoke(&mut s, CommandId::ZenMode);
            assert!(
                !chrome(
                    &mut s,
                    ChromeEvent::Contact {
                        position: [24.0, 24.0],
                        canvas: true
                    },
                    facts
                )
                .chrome_hidden
            );
            invoke(&mut s, CommandId::ZenMode);
            assert!(!chrome(&mut s, ChromeEvent::Refresh, facts).chrome_hidden);
        }
    }
    #[test]
    fn zen_stays_visible_through_drag_focus_loss_until_drag_end() {
        let mut s = session();
        s.state.settings.total_zen = true;
        invoke(&mut s, CommandId::ZenMode);
        let dragging = ChromeFacts {
            dragging: true,
            ..ChromeFacts::default()
        };
        assert!(
            !chrome(
                &mut s,
                ChromeEvent::Motion {
                    position: [600.0, 450.0]
                },
                dragging
            )
            .chrome_hidden
        );
        assert!(!chrome(&mut s, ChromeEvent::Leave { touch: false }, dragging).chrome_hidden);
        assert!(!s.input(UiInput::Blur).unwrap().chrome_hidden);
        assert!(!chrome(&mut s, ChromeEvent::Refresh, dragging).chrome_hidden);
        // End/cancel returns immediately to normal pointer proximity.
        assert!(chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default()).chrome_hidden);
        assert!(
            chrome(
                &mut s,
                ChromeEvent::Motion {
                    position: [600.0, 450.0]
                },
                ChromeFacts::default()
            )
            .chrome_hidden
        );
        let next = chrome(
            &mut s,
            ChromeEvent::Contact {
                position: [600.0, 450.0],
                canvas: true,
            },
            ChromeFacts::default(),
        );
        assert!(!next.handled && next.chrome_hidden && !next.paint);
    }

    #[test]
    fn shortcuts_share_modifiers_editing_guards_and_repeat_policy() {
        let mut s = session();
        assert!(!key(&mut s, "e", true, true, false).handled);
        key(&mut s, "e", false, true, false);
        assert!(!key(&mut s, "e", true, false, true).handled);
        key(&mut s, "e", false, false, true);
        assert!(key(&mut s, "E", true, false, false).handled);
        assert_eq!(s.state.brush.tool, Tool::Eraser);
        assert!(key(&mut s, "Tab", true, false, false).handled);
        assert!(s.state.workspace.zen_mode);
        key(&mut s, "Tab", true, false, false);
        assert!(s.state.workspace.zen_mode, "repeat cannot retoggle Zen");
        key(&mut s, "Tab", false, false, false);
        key(&mut s, "Tab", true, false, false);
        assert!(!s.state.workspace.zen_mode);
        chrome(
            &mut s,
            ChromeEvent::Refresh,
            ChromeFacts {
                popup_open: true,
                ..ChromeFacts::default()
            },
        );
        assert!(!key(&mut s, " ", true, false, false).pan_cursor);
        key(&mut s, " ", false, false, false);
        chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default());
        assert!(key(&mut s, " ", true, false, false).pan_cursor);
        assert!(
            !key(&mut s, " ", false, false, true).pan_cursor,
            "release always clears Space even after focus changed"
        );
        invoke(&mut s, CommandId::Settings);
        assert!(key(&mut s, "b", true, false, false).handled);
        assert_eq!(
            s.preferences().unwrap().query,
            "b",
            "typing searches instead of selecting a tool"
        );
        s.dispatch(UiAction::CloseSettings).unwrap();
        s.input(UiInput::Blur).unwrap();
        assert!(key(&mut s, "b", true, false, false).handled);
    }
    #[test]
    fn pointer_ownership_keeps_pan_and_ink_stable_through_modifier_changes() {
        use ContactPhase::*;
        use PointerButton::*;
        let mut s = session();
        let before = s.state.camera.clone();
        key(&mut s, " ", true, false, false);
        assert!(!pointer(&mut s, 1, Down, [300.0; 2], Primary).paint);
        pointer(&mut s, 1, Move, [320.0, 340.0], Primary);
        key(&mut s, " ", false, false, false);
        assert!(!pointer(&mut s, 2, Down, [300.0; 2], Primary).handled);
        assert!(!pointer(&mut s, 1, Up, [350.0, 360.0], Primary).paint);
        assert_eq!(
            s.state.camera.translation,
            [before.translation[0] + 50.0, before.translation[1] + 60.0]
        );
        assert!(pointer(&mut s, 3, Down, [400.0; 2], Primary).paint);
        key(&mut s, " ", true, false, false);
        assert!(pointer(&mut s, 3, Move, [420.0; 2], Primary).paint);
        assert_eq!(
            s.scroll([100.0; 2], [0.0, 40.0], 1.0, false, false)
                .unwrap()
                .regions,
            0
        );
        let cancel = s.input(UiInput::Blur).unwrap();
        assert!(cancel.cancel_paint && !cancel.pan_cursor);
        assert!(!pointer(&mut s, 3, Move, [430.0; 2], Primary).paint);
        assert!(pointer(&mut s, 4, Down, [400.0; 2], Pan).handled);
        assert!(!pointer(&mut s, 4, Cancel, [400.0; 2], Pan).paint);
    }
    #[test]
    fn routed_strokes_can_queue_back_to_back_before_a_frame() {
        let mut s = session();
        for (index, phase) in [
            ContactPhase::Down,
            ContactPhase::Up,
            ContactPhase::Down,
            ContactPhase::Up,
        ]
        .into_iter()
        .enumerate()
        {
            assert!(pointer(&mut s, 1, phase, [500.0; 2], PointerButton::Primary).paint);
            s.pen(event(&s, index as u64 + 1, pen_phase(phase), 1.0))
                .unwrap();
        }
        s.frame(50_000_000, 58_000_000).unwrap();
        assert_eq!(
            s.engine
                .document()
                .layer(s.engine.document().active_layer)
                .unwrap()
                .strokes
                .len(),
            2
        );
    }
    #[test]
    fn viewport_owns_initial_fit_and_layout_bounds_without_refitting_edits() {
        let mut s = session();
        let logical = [1200.0, 900.0];
        let physical = [2400, 1800];
        assert!(s.set_viewport(logical, physical).unwrap().canvas_wake);
        let b = s.layout(logical).work_area;
        assert_eq!(
            s.state.camera.work_area,
            [b.x, b.y, b.width, b.height].map(|v| v * 2.0)
        );
        let mut expected = Camera::new([1000; 2], physical);
        expected.work_area = s.state.camera.work_area;
        expected.fit([1000; 2]);
        assert_eq!(s.state.camera.translation, expected.translation);
        assert_eq!(s.state.camera.zoom, expected.zoom);
        assert_eq!(s.set_viewport(logical, physical).unwrap().regions, 0);
        s.gesture([100.0; 2], [200.0; 2], 1.2, 0.4).unwrap();
        let camera = s.state.camera.clone();
        s.dispatch(UiAction::ResizeDock {
            id: 3,
            position: [310.0, 100.0],
            viewport: logical,
        })
        .unwrap();
        assert_ne!(s.state.camera.work_area, camera.work_area);
        assert_eq!(s.state.camera.translation, camera.translation);
        assert_eq!(s.state.camera.rotation, camera.rotation);
        assert_eq!(s.state.camera.revision, camera.revision);
        let before = s.state.camera.clone();
        for (logical, physical) in [
            ([f32::NAN, 900.0], physical),
            ([0.0; 2], physical),
            (logical, [0, 100]),
        ] {
            assert!(s.set_viewport(logical, physical).is_err());
            assert_eq!(s.state.camera, before);
        }
    }

    #[test]
    fn divider_lifecycle_preserves_grab_offset_and_uses_shared_nudges() {
        let mut s = session();
        let viewport = [1200.0, 900.0];
        for d in s.layout(viewport).dividers {
            let pointer = [d.bounds.x + 1.0, d.bounds.y + 1.0];
            let before = s.state.workspace.layout.clone();
            s.dispatch(UiAction::DragDivider {
                id: d.id,
                phase: ContactPhase::Down,
                position: pointer,
                viewport,
            })
            .unwrap();
            for _ in 0..3 {
                s.dispatch(UiAction::DragDivider {
                    id: d.id,
                    phase: ContactPhase::Move,
                    position: pointer,
                    viewport,
                })
                .unwrap();
                assert_eq!(s.state.workspace.layout, before);
            }
            let axis = usize::from(d.axis == Axis::Vertical);
            let mut to = pointer;
            to[axis] += 12.0;
            s.dispatch(UiAction::DragDivider {
                id: d.id,
                phase: ContactPhase::Up,
                position: to,
                viewport,
            })
            .unwrap();
            let dragged = s.state.workspace.layout.clone();
            assert!(
                s.dispatch(UiAction::DragDivider {
                    id: d.id,
                    phase: ContactPhase::Move,
                    position: to,
                    viewport
                })
                .is_err()
            );
            assert_eq!(s.state.workspace.layout, dragged);
            s.state.workspace.layout = before;
            s.dispatch(UiAction::NudgeDivider {
                id: d.id,
                forward: true,
                viewport,
            })
            .unwrap();
            assert_eq!(s.state.workspace.layout, dragged);
        }
    }
    #[test]
    fn workspace_roundtrips_into_a_fresh_session_without_document_changes() {
        let mut source = session();
        let viewport = [1200.0, 900.0];
        for action in [
            UiAction::MovePanel {
                panel: Panel::Toolbar,
                target: DockTarget::Edge {
                    edge: Edge::Left,
                    outer: true,
                },
                viewport,
            },
            UiAction::MovePanel {
                panel: Panel::Sizes,
                target: DockTarget::Tab {
                    group: 5,
                    index: Some(0),
                },
                viewport,
            },
            UiAction::SelectPanelTab {
                group: 5,
                panel: Panel::Brushes,
            },
            UiAction::PrioritizeBand { id: 7 },
            UiAction::Invoke {
                command: CommandId::ZenMode,
            },
        ] {
            source.dispatch(action).unwrap();
        }
        let divider = source
            .layout(viewport)
            .dividers
            .into_iter()
            .find(|d| d.id == 7)
            .unwrap();
        source
            .dispatch(UiAction::ResizeDock {
                id: 7,
                position: [divider.bounds.x - 30.0 + divider.bounds.width * 0.5, 450.0],
                viewport,
            })
            .unwrap();
        for zen_mode in [true, false] {
            if !zen_mode {
                invoke(&mut source, CommandId::ZenMode);
            }
            let saved = serde_json::to_string(&source.state.workspace).unwrap();
            let mut restored = session();
            let camera = restored.state.camera.clone();
            let revision = restored.engine.document().revision;
            let change = restored
                .dispatch(UiAction::RestoreWorkspace {
                    workspace: serde_json::from_str(&saved).unwrap(),
                })
                .unwrap();
            assert_ne!(change.regions & regions::LAYOUT, 0);
            assert!(!change.canvas_wake);
            assert_eq!(restored.engine.document().revision, revision);
            assert_eq!(restored.state.camera, camera);
            assert_eq!(restored.state.workspace, source.state.workspace);
            assert_eq!(restored.command(CommandId::ZenMode).selected, zen_mode);
            for viewport in [[1200.0, 900.0], [680.0, 480.0], [900.0, 1200.0]] {
                assert_eq!(
                    serde_json::to_value(source.layout(viewport)).unwrap(),
                    serde_json::to_value(restored.layout(viewport)).unwrap()
                );
            }
            // The saved ID allocator remains safe for subsequent editing.
            restored
                .dispatch(UiAction::MovePanel {
                    panel: Panel::Sizes,
                    target: DockTarget::Edge {
                        edge: Edge::Left,
                        outer: false,
                    },
                    viewport,
                })
                .unwrap();
            restored.state.workspace.validate().unwrap();
        }
    }
    #[test]
    fn same_slot_drag_and_tearoff_restore_geometry_without_history_on_every_host() {
        let viewport = [1200.0, 900.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for tearoff in [false, true] {
                let mut app = session();
                app.set_platform(platform);
                let baseline = app.state.workspace.clone();
                let item = DockItem::Group { group: 5 };
                let bounds = app
                    .layout(viewport)
                    .groups
                    .into_iter()
                    .find(|g| g.id == 5)
                    .unwrap()
                    .bounds;
                let drag = |app: &mut UiSession<_>, phase, position| {
                    app.dispatch(UiAction::DragWorkspace {
                        item,
                        phase,
                        position,
                        viewport,
                        tabs: vec![],
                    })
                    .unwrap();
                };
                drag(
                    &mut app,
                    ContactPhase::Down,
                    [bounds.x + 70.0, bounds.y + 12.0],
                );
                if tearoff {
                    drag(&mut app, ContactPhase::Move, [600.0, 400.0]);
                    assert_eq!(app.state.workspace.layout.floating.len(), 1);
                }
                let neighbor = app
                    .layout(viewport)
                    .groups
                    .into_iter()
                    .find(|g| g.id == 6)
                    .unwrap()
                    .bounds;
                let destination = [
                    neighbor.x + neighbor.width * 0.5,
                    neighbor.y + TAB_BAR_HEIGHT + 3.0,
                ];
                drag(&mut app, ContactPhase::Move, destination);
                drag(&mut app, ContactPhase::Up, destination);
                assert_eq!(
                    app.state.workspace, baseline,
                    "{platform:?}, tearoff={tearoff}"
                );
                assert!(!app.command(CommandId::UndoWorkspace).enabled);
                assert!(app.workspace_drag.is_none());
            }
        }
    }

    #[test]
    fn floating_gestures_update_live_preserve_grab_offset_and_coalesce_history() {
        let viewport = [1200.0, 900.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut app = session();
            app.set_platform(platform);
            app.dispatch(UiAction::MovePanel {
                panel: Panel::Brushes,
                viewport,
                target: DockTarget::Float {
                    position: [600.0, 210.0],
                },
            })
            .unwrap();
            let baseline = app.state.workspace.clone();
            app.dispatch(UiAction::RestoreWorkspace {
                workspace: baseline.clone(),
            })
            .unwrap();
            let group = baseline.layout.panel_group(Panel::Brushes).unwrap();
            let bounds = |app: &UiSession<_>| {
                app.layout(viewport)
                    .groups
                    .into_iter()
                    .find(|g| g.id == group)
                    .unwrap()
                    .bounds
            };
            let start = bounds(&app);
            let point = [start.x + 23.0, start.y + 12.0];
            let drag = |app: &mut UiSession<_>, edge: Option<ResizeEdge>, phase, position| {
                let changed = app
                    .dispatch(if let Some(edge) = edge {
                        UiAction::ResizeFloating {
                            group,
                            edge,
                            phase,
                            position,
                            viewport,
                        }
                    } else {
                        UiAction::DragWorkspace {
                            item: DockItem::Panel {
                                panel: Panel::Brushes,
                            },
                            phase,
                            position,
                            viewport,
                            tabs: vec![],
                        }
                    })
                    .unwrap();
                assert!(!changed.canvas_wake);
            };
            drag(&mut app, None, ContactPhase::Down, point);
            for delta in [1.0, 10.0, 24.0, 50.0] {
                drag(
                    &mut app,
                    None,
                    ContactPhase::Move,
                    [point[0] + delta, point[1] + delta],
                );
                assert_eq!(
                    bounds(&app),
                    Bounds {
                        x: start.x + delta,
                        y: start.y + delta,
                        ..start
                    }
                );
                assert!(!app.command(CommandId::UndoWorkspace).enabled);
            }
            drag(
                &mut app,
                None,
                ContactPhase::Up,
                [point[0] + 50.0, point[1] + 50.0],
            );
            let moved = app.state.workspace.clone();
            assert_ne!(moved, baseline);
            invoke(&mut app, CommandId::UndoWorkspace);
            assert_eq!(app.state.workspace, baseline);
            assert!(!app.command(CommandId::UndoWorkspace).enabled);
            invoke(&mut app, CommandId::RedoWorkspace);
            assert_eq!(app.state.workspace, moved);
            let b = bounds(&app);
            let corner = [b.x + b.width - 4.0, b.y + b.height - 7.0];
            drag(
                &mut app,
                Some(ResizeEdge::BottomRight),
                ContactPhase::Down,
                corner,
            );
            drag(
                &mut app,
                Some(ResizeEdge::BottomRight),
                ContactPhase::Move,
                [corner[0] + 40.0, corner[1] + 30.0],
            );
            assert_eq!(bounds(&app).width, b.width + 40.0);
            assert_eq!(bounds(&app).height, b.height + 30.0);
            drag(
                &mut app,
                Some(ResizeEdge::BottomRight),
                ContactPhase::Cancel,
                corner,
            );
            assert_eq!(app.state.workspace, moved);
            drag(&mut app, None, ContactPhase::Down, point);
            drag(
                &mut app,
                None,
                ContactPhase::Move,
                [point[0] - 20.0, point[1] + 15.0],
            );
            assert_ne!(app.state.workspace, moved);
            let changed = app.input(UiInput::Blur).unwrap().change;
            assert_ne!(changed.regions & regions::LAYOUT, 0);
            assert_eq!(app.state.workspace, moved);
            assert!(app.workspace_drag.is_none());
            // Releasing over a dock merges in the same undo transaction.
            let layers = app
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.active == Panel::Layers)
                .unwrap();
            let target = [
                layers.bounds.x + layers.bounds.width * 0.5,
                layers.bounds.y + layers.bounds.height * 0.5,
            ];
            drag(&mut app, None, ContactPhase::Down, point);
            drag(&mut app, None, ContactPhase::Move, target);
            assert_eq!(app.state.workspace.layout.floating.len(), 1);
            drag(&mut app, None, ContactPhase::Up, target);
            assert!(app.state.workspace.layout.floating.is_empty());
            assert_eq!(
                app.state.workspace.layout.panel_group(Panel::Brushes),
                Some(layers.id)
            );
            invoke(&mut app, CommandId::UndoWorkspace);
            assert_eq!(app.state.workspace, moved);
        }
    }

    #[test]
    fn removing_edge_most_panel_shifts_its_neighbor_without_expanding_it() {
        let viewport = [1600.0, 1200.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for edge in [Edge::Left, Edge::Right] {
                for separate_bands in [false, true] {
                    for float in [false, true] {
                        let mut app = session();
                        app.set_platform(platform);
                        let layout = &mut app.state.workspace.layout;
                        layout.set_panel_visible(Panel::Toolbar, false).unwrap();
                        layout.set_panel_visible(Panel::Sizes, false).unwrap();
                        for panel in [Panel::Adjustments, Panel::Properties] {
                            layout.set_panel_visible(panel, false).unwrap();
                        }
                        let group = layout.panel_group(Panel::Brushes).unwrap();
                        // Place Brushes on this edge, then Layers closest to
                        // the edge, either within its split or in another band.
                        layout
                            .move_panel(
                                viewport,
                                Panel::Brushes,
                                DockTarget::Edge { edge, outer: false },
                            )
                            .unwrap();
                        layout
                            .move_panel(
                                viewport,
                                Panel::Layers,
                                if separate_bands {
                                    DockTarget::Edge { edge, outer: true }
                                } else {
                                    DockTarget::Split { group, edge }
                                },
                            )
                            .unwrap();
                        let before = app.state.workspace.clone();
                        let bounds = |app: &UiSession<_>| {
                            app.layout(viewport)
                                .groups
                                .into_iter()
                                .find(|g| g.active == Panel::Brushes)
                                .unwrap()
                                .bounds
                        };
                        let original = bounds(&app);
                        if float {
                            app.dispatch(UiAction::MovePanel {
                                panel: Panel::Layers,
                                viewport,
                                target: DockTarget::Float {
                                    position: [800.0, 600.0],
                                },
                            })
                            .unwrap();
                        } else {
                            app.interaction.viewport = Some(viewport);
                            app.dispatch(UiAction::Customize {
                                action: CustomizationAction::SetPanelVisible {
                                    panel: Panel::Layers,
                                    visible: false,
                                },
                            })
                            .unwrap();
                        }
                        let after = bounds(&app);
                        assert!(
                            (after.width - original.width).abs() < 0.01,
                            "{platform:?} {edge:?} separate={separate_bands} float={float}: {original:?} -> {after:?}"
                        );
                        if edge == Edge::Left {
                            assert_eq!(after.x, WORKSPACE_SPACING);
                            assert!(after.x < original.x);
                        } else {
                            assert_eq!(after.x + after.width, viewport[0] - WORKSPACE_SPACING);
                            assert!(after.x > original.x);
                        }
                        invoke(&mut app, CommandId::UndoWorkspace);
                        assert_eq!(app.state.workspace, before);
                    }
                }
            }
        }
    }

    #[test]
    fn application_menus_reuse_live_models_and_selection_commands_are_undoable() {
        let mut app = session();
        app.set_platform(Platform::Gtk);
        assert_eq!(
            ApplicationMenu::ALL.map(|m| m.label()),
            [
                "File", "Edit", "Layer", "Select", "Filter", "View", "Window", "Help"
            ]
        );
        for menu in ApplicationMenu::ALL {
            assert!(!app.application_menu(menu).sections.is_empty());
        }
        for (command, link) in [
            (CommandId::Website, ApplicationLink::Website),
            (CommandId::SourceCode, ApplicationLink::SourceCode),
        ] {
            invoke(&mut app, command);
            let request = app.state.requests.last().unwrap();
            assert!(
                matches!(request.kind, HostRequestKind::OpenLink { link: actual } if actual == link)
            );
            app.dispatch(UiAction::CompleteRequest {
                id: request.id,
                error: None,
            })
            .unwrap();
        }
        let serialize = |menu: ContextMenu| serde_json::to_value(menu.sections).unwrap();
        let doc = app.engine.document();
        assert_eq!(
            serialize(app.application_menu(ApplicationMenu::Layer)),
            serialize(app.layer_menu(doc.active_layer.0, doc.active_mask).unwrap())
        );
        assert_eq!(
            serialize(app.application_menu(ApplicationMenu::Window)),
            serialize(app.workspace_menu())
        );
        let all_filters = serialize(app.application_menu(ApplicationMenu::Filter));
        app.dispatch(UiAction::FilterPicker {
            action: FilterPickerAction::Search {
                query: "no such filter".into(),
            },
        })
        .unwrap();
        assert!(app.state.adjustments.is_empty());
        assert_eq!(
            serialize(app.application_menu(ApplicationMenu::Filter)),
            all_filters
        );
        assert!(!app.command(CommandId::FillSelection).enabled);
        assert!(!app.command(CommandId::Deselect).enabled);
        let saved = app.engine.checkpoint();
        invoke(&mut app, CommandId::SelectAll);
        assert_eq!(
            app.engine.checkpoint(),
            saved,
            "selection is not saved artwork"
        );
        assert!(app.command(CommandId::FillSelection).enabled);
        let all = app.engine.document().selection.clone().unwrap();
        assert_eq!(all.contours()[0].len(), 4);
        invoke(&mut app, CommandId::InvertSelection);
        assert!(app.engine.document().selection.as_ref().unwrap().inverted);
        invoke(&mut app, CommandId::Undo);
        assert_eq!(app.engine.document().selection.as_ref(), Some(&all));
        invoke(&mut app, CommandId::FillSelection);
        let painted = app.engine.document().layers.clone();
        assert!(app.engine.checkpoint() != saved);
        invoke(&mut app, CommandId::ClearLayer);
        assert!(
            app.engine
                .document()
                .layer(app.engine.document().active_layer)
                .unwrap()
                .operations
                .is_empty()
        );
        invoke(&mut app, CommandId::Undo);
        assert_eq!(app.engine.document().layers, painted);
        invoke(&mut app, CommandId::Deselect);
        assert!(app.engine.document().selection.is_none());
        let mask_layer = app.engine.document().active_layer.0;
        invoke(&mut app, CommandId::SelectAll);
        app.dispatch(UiAction::Layer {
            action: LayerAction::Lock {
                id: mask_layer,
                value: true,
            },
        })
        .unwrap();
        assert!(!app.command(CommandId::ClearLayer).enabled);
        assert!(!app.command(CommandId::FillSelection).enabled);
        app.dispatch(UiAction::Layer {
            action: LayerAction::Lock {
                id: mask_layer,
                value: false,
            },
        })
        .unwrap();
        app.dispatch(UiAction::Layer {
            action: LayerAction::AddMask {
                id: mask_layer,
                replace: false,
            },
        })
        .unwrap();
        app.dispatch(UiAction::Layer {
            action: LayerAction::Select {
                id: mask_layer,
                mask: true,
            },
        })
        .unwrap();
        assert!(!app.command(CommandId::ClearLayer).enabled);
        let doc = app.engine.document();
        assert_eq!(
            serialize(app.application_menu(ApplicationMenu::Layer)),
            serialize(app.layer_menu(doc.active_layer.0, true).unwrap())
        );
        app.input_pending = true;
        assert!(!app.command(CommandId::SelectAll).enabled);
        assert!(
            app.application_menu(ApplicationMenu::Filter)
                .sections
                .iter()
                .flatten()
                .flat_map(|c| c.sections.iter().flatten())
                .all(|item| !item.enabled)
        );
    }

    #[test]
    fn panel_availability_is_consistent_across_controls_and_menus() {
        for platform in [
            Platform::Windows,
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Ios,
            Platform::Mac,
        ] {
            let mut app = session();
            app.set_platform(platform);
            for panel in [Panel::ToolSettings, Panel::Color, Panel::Navigator] {
                let available = match panel {
                    Panel::ToolSettings | Panel::Color => matches!(
                        platform,
                        Platform::Gtk
                            | Platform::Windows
                            | Platform::Android
                            | Platform::Web
                            | Platform::Ios
                            | Platform::Mac
                    ),
                    Panel::Navigator => matches!(
                        platform,
                        Platform::Gtk
                            | Platform::Windows
                            | Platform::Android
                            | Platform::Web
                            | Platform::Ios
                            | Platform::Mac
                    ),
                    _ => unreachable!(),
                };
                assert_eq!(
                    !app.panel_view(panel).unwrap().controls.is_empty(),
                    available
                );
                assert_eq!(app.workspace_menu().sections[1].iter().any(|i| matches!(
                        i.action, Some(UiAction::Customize { action: CustomizationAction::SetPanelVisible { panel: p, .. } }) if p == panel
                    )), available);
                let result = app.dispatch(UiAction::Customize {
                    action: CustomizationAction::SetPanelVisible {
                        panel,
                        visible: true,
                    },
                });
                assert_eq!(result.is_ok(), available);
            }
        }
    }

    #[test]
    fn group_tab_presentation_selection_moves_and_history_are_shared() {
        let viewport = [1600.0, 1000.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut app = session();
            app.set_platform(platform);
            let group = app
                .state
                .workspace
                .layout
                .panel_group(Panel::Brushes)
                .unwrap();
            app.dispatch(UiAction::MovePanel {
                panel: Panel::Sizes,
                target: DockTarget::Tab { group, index: None },
                viewport,
            })
            .unwrap();
            for style in TabStyle::ALL {
                let before = app.state.workspace.clone();
                app.dispatch(UiAction::Customize {
                    action: CustomizationAction::SetTabStyle { group, style },
                })
                .unwrap();
                let after = app.state.workspace.clone();
                if before != after {
                    invoke(&mut app, CommandId::UndoWorkspace);
                    assert_eq!(app.state.workspace, before);
                    invoke(&mut app, CommandId::RedoWorkspace);
                    assert_eq!(app.state.workspace, after);
                }
                for active in [Panel::Brushes, Panel::Sizes] {
                    app.dispatch(UiAction::SelectPanelTab {
                        group,
                        panel: active,
                    })
                    .unwrap();
                    for panel in [Panel::Brushes, Panel::Sizes] {
                        let tab = app.panel_view(panel).unwrap().tab;
                        assert_eq!(tab.show_icon, style != TabStyle::Name);
                        assert_eq!(
                            tab.show_name,
                            matches!(
                                style,
                                TabStyle::Name | TabStyle::IconName | TabStyle::Automatic
                            ) || (style == TabStyle::ActiveName && active == panel)
                        );
                    }
                }
                let saved = serde_json::to_string(&app.state.workspace).unwrap();
                assert_eq!(
                    serde_json::from_str::<WorkspaceState>(&saved).unwrap(),
                    app.state.workspace
                );
                app.dispatch(UiAction::MoveGroup {
                    group,
                    viewport,
                    target: DockTarget::Float {
                        position: [800.0, 300.0],
                    },
                })
                .unwrap();
                assert_eq!(
                    app.state.workspace.layout.group_tab_style(group).unwrap(),
                    style
                );
            }
            let destination = app
                .state
                .workspace
                .layout
                .panel_group(Panel::Layers)
                .unwrap();
            app.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTabStyle {
                    group: destination,
                    style: TabStyle::Name,
                },
            })
            .unwrap();
            app.dispatch(UiAction::MoveGroup {
                group,
                viewport,
                target: DockTarget::Tab {
                    group: destination,
                    index: None,
                },
            })
            .unwrap();
            for panel in [Panel::Brushes, Panel::Sizes, Panel::Layers] {
                assert_eq!(
                    app.panel_view(panel).unwrap().tab,
                    TabPresentation {
                        show_icon: false,
                        show_name: true
                    }
                );
            }
            // Splitting a tab creates a fresh group's default, not a per-panel preference.
            app.dispatch(UiAction::MovePanel {
                panel: Panel::Sizes,
                viewport,
                target: DockTarget::Float {
                    position: [600.0, 400.0],
                },
            })
            .unwrap();
            let detached = app
                .state
                .workspace
                .layout
                .panel_group(Panel::Sizes)
                .unwrap();
            assert_eq!(
                app.state
                    .workspace
                    .layout
                    .group_tab_style(detached)
                    .unwrap(),
                TabStyle::Automatic
            );
            assert_eq!(
                app.state
                    .workspace
                    .layout
                    .group_tab_style(destination)
                    .unwrap(),
                TabStyle::Name
            );
            let before = app.state.workspace.clone();
            assert!(
                app.dispatch(UiAction::Customize {
                    action: CustomizationAction::SetTabStyle {
                        group: u32::MAX,
                        style: TabStyle::Icon
                    }
                })
                .is_err()
            );
            assert_eq!(app.state.workspace, before);
            assert!(serde_json::from_value::<CustomizationAction>(serde_json::json!({"type":"set_tab_style","target":{"kind":"panel","panel":"sizes"},"style":"icon"})).is_err());
            assert!(
                serde_json::to_value(app.state.workspace.layout.panel(Panel::Sizes).unwrap())
                    .unwrap()
                    .get("tab_style")
                    .is_none()
            );
        }
    }

    #[test]
    fn dragging_panels_preserves_tab_visibility_and_history() {
        let viewport = [1600.0, 1200.0];
        for platform in [
            Platform::Generic,
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
            for style in TabStyle::ALL {
                for hidden in [false, true] {
                    for destination in ["float", "edge", "merge", "cancel"] {
                        let mut app = session();
                        app.set_platform(platform);
                        let panel = Panel::Sizes;
                        let group = app.state.workspace.layout.panel_group(panel).unwrap();
                        for action in [
                            CustomizationAction::SetTabStyle { group, style },
                            CustomizationAction::SetTabHidden { panel, hidden },
                        ] {
                            app.dispatch(UiAction::Customize { action }).unwrap();
                        }
                        let before = app.state.workspace.clone();
                        for target in [
                            ContextTarget::Panel { panel },
                            ContextTarget::Group { group },
                        ] {
                            let menu = app.context_menu(target).unwrap();
                            assert_eq!(
                                menu.sections[0]
                                    .iter()
                                    .map(|i| i.label.as_str())
                                    .collect::<Vec<_>>(),
                                if matches!(target, ContextTarget::Group { .. }) {
                                    TabStyle::ALL.map(TabStyle::label).to_vec()
                                } else {
                                    vec!["Show tab bar"]
                                }
                            );
                            assert_eq!(
                                menu.sections[0]
                                    .iter()
                                    .filter(|i| i.selected == Some(true))
                                    .count(),
                                if matches!(target, ContextTarget::Group { .. }) {
                                    1
                                } else {
                                    usize::from(!hidden)
                                }
                            );
                            let hide = menu
                                .sections
                                .iter()
                                .flatten()
                                .find(|i| i.label == "Show tab bar")
                                .unwrap();
                            assert_eq!(hide.selected, Some(!hidden));
                            if hidden {
                                assert!(menu.sections.iter().flatten().any(|i| i.label
                                    == "Configure Brush size panel…"
                                    && matches!(
                                        i.action,
                                        Some(UiAction::Customize {
                                            action: CustomizationAction::ShowAllControls {
                                                panel: Panel::Sizes
                                            }
                                        })
                                    )));
                            }
                        }
                        let source = app
                            .layout(viewport)
                            .groups
                            .into_iter()
                            .find(|g| g.id == group)
                            .unwrap();
                        assert_eq!(source.tabs_visible, !hidden);
                        assert_eq!(source.footer_grip.is_some(), hidden);
                        let press = [source.bounds.x + 50.0, source.bounds.y + 12.0];
                        let drag = |app: &mut UiSession<_>, phase, position| {
                            app.dispatch(UiAction::DragWorkspace {
                                item: DockItem::Panel { panel },
                                phase,
                                position,
                                viewport,
                                tabs: vec![],
                            })
                            .unwrap();
                        };
                        drag(&mut app, ContactPhase::Down, press);
                        drag(&mut app, ContactPhase::Move, [800.0, 550.0]);
                        let floating = app
                            .layout(viewport)
                            .groups
                            .into_iter()
                            .find(|g| g.panels.contains(&panel))
                            .unwrap();
                        assert!(floating.floating);
                        assert_eq!(floating.tabs_visible, !hidden);
                        assert_eq!(floating.footer_grip.is_some(), hidden);
                        assert_eq!(app.state.workspace.layout.panels, before.layout.panels);
                        let position = match destination {
                            "edge" => [1599.0, 600.0],
                            "merge" => {
                                let b = app
                                    .layout(viewport)
                                    .groups
                                    .into_iter()
                                    .find(|g| g.active == Panel::Brushes)
                                    .unwrap()
                                    .bounds;
                                [b.x + b.width * 0.5, b.y + 10.0]
                            }
                            _ => [800.0, 550.0],
                        };
                        drag(
                            &mut app,
                            if destination == "cancel" {
                                ContactPhase::Cancel
                            } else {
                                ContactPhase::Up
                            },
                            position,
                        );
                        if destination == "cancel" {
                            assert_eq!(app.state.workspace, before);
                            continue;
                        }
                        let config = app.state.workspace.layout.panel(panel).unwrap();
                        let current_group = app.state.workspace.layout.panel_group(panel).unwrap();
                        assert_eq!(
                            app.state
                                .workspace
                                .layout
                                .group_tab_style(current_group)
                                .unwrap(),
                            if destination == "merge" {
                                TabStyle::default()
                            } else {
                                style
                            }
                        );
                        assert_eq!(config.hide_tab, hidden);
                        assert_eq!(app.state.workspace.layout.panels, before.layout.panels);
                        let result = app
                            .layout(viewport)
                            .groups
                            .into_iter()
                            .find(|g| g.panels.contains(&panel))
                            .unwrap();
                        assert_eq!(result.floating, destination == "float");
                        assert_eq!(result.tabs_visible, destination == "merge" || !hidden);
                        if destination == "merge" {
                            assert!(result.panels.len() > 1);
                            assert!(
                                app.context_menu(ContextTarget::Group { group: result.id })
                                    .unwrap()
                                    .sections
                                    .iter()
                                    .flatten()
                                    .all(|i| i.label != "Show tab bar")
                            );
                            assert!(
                                app.dispatch(UiAction::Customize {
                                    action: CustomizationAction::SetTabHidden {
                                        panel,
                                        hidden: true
                                    }
                                })
                                .is_err()
                            );
                        }
                        let after = app.state.workspace.clone();
                        invoke(&mut app, CommandId::UndoWorkspace);
                        assert_eq!(app.state.workspace, before);
                        invoke(&mut app, CommandId::RedoWorkspace);
                        assert_eq!(app.state.workspace, after);
                    }
                }
            }
        }
    }

    #[test]
    fn collapsed_icon_drag_uses_shared_history_and_cancellation() {
        let viewport = [1600., 1200.];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for cancel in [false, true] {
                let mut app = session();
                app.set_platform(platform);
                let group = app
                    .state
                    .workspace
                    .layout
                    .panel_group(Panel::Layers)
                    .unwrap();
                app.dispatch(UiAction::Customize {
                    action: CustomizationAction::SetColumnCollapsed {
                        group,
                        collapsed: true,
                    },
                })
                .unwrap();
                let source = app
                    .layout(viewport)
                    .collapsed
                    .into_iter()
                    .flat_map(|c| c.groups)
                    .flat_map(|g| g.icons)
                    .find(|i| i.panel == Panel::Layers)
                    .unwrap()
                    .bounds;
                let before = serde_json::to_value(&app.state.workspace).unwrap();
                let point = [750., 500.];
                for (phase, position) in [
                    (
                        ContactPhase::Down,
                        [source.x + source.width / 2., source.y + source.height / 2.],
                    ),
                    (ContactPhase::Move, point),
                    (
                        if cancel {
                            ContactPhase::Cancel
                        } else {
                            ContactPhase::Up
                        },
                        point,
                    ),
                ] {
                    app.dispatch(UiAction::DragWorkspace {
                        item: DockItem::Panel {
                            panel: Panel::Layers,
                        },
                        phase,
                        position,
                        viewport,
                        tabs: vec![],
                    })
                    .unwrap();
                }
                if !cancel {
                    let after = serde_json::to_value(&app.state.workspace).unwrap();
                    assert_ne!(after, before);
                    assert_eq!(app.state.workspace.layout.floating.len(), 1);
                    app.dispatch(UiAction::Invoke {
                        command: CommandId::UndoWorkspace,
                    })
                    .unwrap();
                    assert_eq!(serde_json::to_value(&app.state.workspace).unwrap(), before);
                    app.dispatch(UiAction::Invoke {
                        command: CommandId::RedoWorkspace,
                    })
                    .unwrap();
                    assert_eq!(serde_json::to_value(&app.state.workspace).unwrap(), after);
                    app.dispatch(UiAction::Invoke {
                        command: CommandId::UndoWorkspace,
                    })
                    .unwrap();
                }
                assert_eq!(serde_json::to_value(&app.state.workspace).unwrap(), before);
                assert!(app.workspace_drag.is_none());
            }
        }
    }

    #[test]
    fn tear_off_threshold_singleton_identity_and_multi_tab_cancel_are_shared() {
        let viewport = [1600.0, 1200.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut app = session();
            app.set_platform(platform);
            let group = app
                .state
                .workspace
                .layout
                .panel_group(Panel::Sizes)
                .unwrap();
            let source = app
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds;
            let baseline = app.state.workspace.clone();
            let drag = |app: &mut UiSession<_>, phase, position| {
                let changed = app
                    .dispatch(UiAction::DragWorkspace {
                        item: DockItem::Panel {
                            panel: Panel::Sizes,
                        },
                        phase,
                        position,
                        viewport,
                        tabs: vec![],
                    })
                    .unwrap();
                assert!(!changed.canvas_wake);
            };
            let press = [source.x + 30.0, source.y + 12.0];
            drag(&mut app, ContactPhase::Down, press);
            assert!(app.dragging_attached_tab());
            drag(
                &mut app,
                ContactPhase::Move,
                [source.x + source.width + 80.0, press[1]],
            );
            assert!(app.state.workspace.layout.floating.is_empty());
            assert!(app.dragging_attached_tab());
            drag(
                &mut app,
                ContactPhase::Move,
                [source.x + source.width + 81.0, press[1]],
            );
            assert_eq!(app.state.workspace.layout.floating[0].root.id(), group);
            assert!(!app.dragging_attached_tab());
            drag(&mut app, ContactPhase::Up, [700.0, 450.0]);
            assert!(!app.dragging_attached_tab());
            let floated = app.state.workspace.clone();
            assert_eq!(floated.layout.floating.len(), 1);
            invoke(&mut app, CommandId::UndoWorkspace);
            assert_eq!(app.state.workspace, baseline);
            invoke(&mut app, CommandId::RedoWorkspace);
            assert_eq!(app.state.workspace, floated);
            // A singleton tab immediately moves its entire float, without
            // waiting for tear-off or replacing its identity/default size.
            let b = app
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds;
            drag(&mut app, ContactPhase::Down, [b.x + 20.0, b.y + 10.0]);
            assert!(!app.dragging_attached_tab());
            drag(&mut app, ContactPhase::Move, [b.x + 30.0, b.y + 25.0]);
            assert_eq!(app.state.workspace.layout.floating[0].root.id(), group);
            assert_eq!(
                app.state.workspace.layout.floating[0].position,
                [b.x + 10.0, b.y + 15.0]
            );
            drag(&mut app, ContactPhase::Cancel, [0.0; 2]);
            assert_eq!(app.state.workspace, floated);
            app.dispatch(UiAction::MovePanel {
                panel: Panel::Brushes,
                viewport,
                target: DockTarget::Tab { group, index: None },
            })
            .unwrap();
            let multi = app.state.workspace.clone();
            let b = app
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds;
            drag(&mut app, ContactPhase::Down, [b.x + 20.0, b.y + 10.0]);
            assert!(app.dragging_attached_tab());
            drag(
                &mut app,
                ContactPhase::Move,
                [b.x + b.width + 101.0, b.y + 10.0],
            );
            assert_eq!(app.state.workspace.layout.floating.len(), 2);
            assert!(!app.dragging_attached_tab());
            assert_eq!(
                app.state.workspace.layout.panel_group(Panel::Brushes),
                Some(group)
            );
            assert_ne!(
                app.state.workspace.layout.panel_group(Panel::Sizes),
                Some(group)
            );
            drag(&mut app, ContactPhase::Cancel, [0.0; 2]);
            assert!(!app.dragging_attached_tab());
            assert_eq!(app.state.workspace, multi);
        }
    }

    #[test]
    fn column_width_reset_is_one_undoable_action() {
        let viewport = [1600., 1000.];
        for (platform, collapsed) in [Platform::Gtk, Platform::Windows]
            .into_iter()
            .flat_map(|platform| [false, true].map(|collapsed| (platform, collapsed)))
        {
            let mut app = session();
            app.set_platform(platform);
            app.state.workspace.layout.bands[0].extent = 400.;
            if collapsed {
                app.state
                    .workspace
                    .layout
                    .set_column_collapsed(5, true, viewport)
                    .unwrap();
            }
            let before = app.state.workspace.clone();
            let action = UiAction::ResetColumnWidth { id: 3, viewport };
            app.dispatch(action.clone()).unwrap();
            let after = app.state.workspace.clone();
            assert_eq!(
                after.layout.bands[0].extent,
                Panel::Brushes.default_width() + WORKSPACE_SPACING
            );
            assert!(after.layout.collapsed.is_empty());
            // A repeated reset is a no-op, not a second undo entry.
            app.dispatch(action).unwrap();
            invoke(&mut app, CommandId::UndoWorkspace);
            assert_eq!(app.state.workspace, before);
            invoke(&mut app, CommandId::RedoWorkspace);
            assert_eq!(app.state.workspace, after);
        }
    }

    #[test]
    fn windows_column_handles_collapse_with_workspace_history() {
        let viewport = [1200.0, 900.0];
        let mut app = session();
        app.set_platform(Platform::Windows);
        let panel = Panel::Sizes;
        let group = app.state.workspace.layout.panel_group(panel).unwrap();
        for style in TabStyle::ALL {
            app.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTabStyle { group, style },
            })
            .unwrap();
            let before = app.state.workspace.clone();
            assert_eq!(
                before.layout.panel_handle_target(DockItem::Panel { panel }),
                None,
                "A tab label keeps its activation behavior"
            );
            let change = app
                .dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                .unwrap();
            assert!(!change.canvas_wake);
            assert!(
                app.state
                    .workspace
                    .layout
                    .collapsed_column_for_group(group)
                    .is_some()
            );
            assert_eq!(
                app.state.workspace.layout.panel(panel).unwrap().hide_tab,
                before.layout.panel(panel).unwrap().hide_tab
            );
            let collapsed = app.state.workspace.clone();
            let json = serde_json::to_string(&collapsed).unwrap();
            assert_eq!(
                serde_json::from_str::<WorkspaceState>(&json).unwrap(),
                collapsed
            );
            invoke(&mut app, CommandId::UndoWorkspace);
            assert_eq!(app.state.workspace, before);
            invoke(&mut app, CommandId::RedoWorkspace);
            assert_eq!(app.state.workspace, collapsed);
            invoke(&mut app, CommandId::UndoWorkspace);
        }
    }

    #[test]
    fn configure_from_collapsed_column_reveals_the_ordinary_panel() {
        for platform in [Platform::Gtk, Platform::Android, Platform::Web] {
            let mut s = session();
            s.set_platform(platform);
            let viewport = [1200., 900.];
            s.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport })
                .unwrap();
            s.dispatch(UiAction::Customize {
                action: CustomizationAction::ToggleColumnDrawer {
                    group: 5,
                    panel: Panel::Brushes,
                },
            })
            .unwrap();
            s.dispatch(UiAction::Customize {
                action: CustomizationAction::ShowAllControls {
                    panel: Panel::Brushes,
                },
            })
            .unwrap();
            assert_eq!(s.state.customization.expanded, Some(Panel::Brushes));
            assert!(s.state.customization.column_drawers.is_empty());
            assert!(s.state.workspace.layout.collapsed.is_empty());
            assert!(
                s.layout(viewport)
                    .groups
                    .iter()
                    .any(|g| g.active == Panel::Brushes)
            );
        }
    }

    #[test]
    fn collapsed_drawer_pins_revealed_total_zen_but_explicit_zen_closes_it() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let viewport = [1200., 900.];
        s.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport })
            .unwrap();
        let mut settings = s.state.settings.clone();
        settings.total_zen = true;
        s.dispatch(UiAction::RestoreSettings { settings }).unwrap();
        s.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        })
        .unwrap();
        let hover = |s: &mut UiSession<Recorder>, position| {
            s.input(UiInput::Chrome {
                event: ChromeEvent::Motion { position },
                facts: ChromeFacts::default(),
                viewport,
            })
            .unwrap()
        };
        assert!(!hover(&mut s, [10., 400.]).chrome_hidden);
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::ToggleColumnDrawer {
                group: 5,
                panel: Panel::Brushes,
            },
        })
        .unwrap();
        assert!(!hover(&mut s, [600., 500.]).chrome_hidden);
        assert!(!s.state.customization.blocks_shortcuts());
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::ToggleColumnDrawer {
                group: 5,
                panel: Panel::Brushes,
            },
        })
        .unwrap();
        // Closing chrome retains the existing one-contact guard: the next
        // canvas press may hide it, but cannot also begin a stroke.
        let reply = s
            .input(UiInput::Chrome {
                event: ChromeEvent::Contact {
                    position: [600., 500.],
                    canvas: true,
                },
                facts: ChromeFacts::default(),
                viewport,
            })
            .unwrap();
        assert!(reply.chrome_hidden && reply.handled);
        s.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        })
        .unwrap();
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::ToggleColumnDrawer {
                group: 5,
                panel: Panel::Brushes,
            },
        })
        .unwrap();
        s.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        })
        .unwrap();
        assert!(s.state.customization.column_drawers.is_empty());
        assert!(hover(&mut s, [600., 500.]).chrome_hidden);
    }

    #[test]
    fn incoming_panels_groups_and_toolbars_share_collapsed_drop_targets() {
        let viewport = [1200., 900.];
        for item in [
            DockItem::Panel {
                panel: Panel::Properties,
            },
            DockItem::Group { group: 8 },
            DockItem::Group { group: 2 },
        ] {
            for slot in 0..3 {
                let mut s = session();
                s.set_platform(Platform::Gtk);
                s.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport })
                    .unwrap();
                let resolved = s.layout(viewport);
                let c = &resolved.collapsed[0];
                let point = [
                    c.bounds.x + TILE_SIZE * 0.5,
                    match slot {
                        0 => c.groups[0].bounds.y + 14.,
                        1 => c.groups[1].bounds.y - 3.,
                        _ => c.empty.y + 5.,
                    },
                ];
                let hint = s.drop_hint(viewport, point, &[], item, None).unwrap();
                match (&hint.target, slot) {
                    (DockTarget::Tab { group: 5, .. }, 0) => {}
                    (
                        DockTarget::Split {
                            group: 6,
                            edge: Edge::Top,
                        },
                        1,
                    ) => {}
                    (
                        DockTarget::Split {
                            group: 6,
                            edge: Edge::Bottom,
                        },
                        2,
                    ) => {}
                    _ => panic!("Wrong collapsed insertion: {:?}", hint.target),
                }
                s.dispatch(item.move_action(hint.target, viewport)).unwrap();
                s.state.workspace.validate().unwrap();
                assert!(s.state.workspace.layout.floating.is_empty());
                let c = &s.layout(viewport).collapsed[0];
                assert_eq!(c.id, 4);
                assert_eq!(c.groups.len(), if slot == 0 { 2 } else { 3 });
                let moved = match item {
                    DockItem::Panel { panel } => panel,
                    DockItem::Group { group: 8 } => Panel::Layers,
                    _ => Panel::Toolbar,
                };
                assert!(
                    c.groups
                        .iter()
                        .flat_map(|g| &g.icons)
                        .any(|i| i.panel == moved)
                );
                assert!(
                    !s.layout(viewport)
                        .groups
                        .iter()
                        .any(|g| g.panels.contains(&moved))
                );
            }
        }
    }
    #[test]
    fn panels_and_groups_drop_in_new_columns_beside_collapsed_sidebars() {
        let viewport = [1600., 1000.];
        for right in [false, true] {
            for whole in [false, true] {
                let mut s = session();
                s.set_platform(Platform::Gtk);
                s.state
                    .workspace
                    .layout
                    .add_panel_to_group(Panel::ToolSettings, 5)
                    .unwrap();
                let (target, source, panel) = if right {
                    (8, 5, Panel::Brushes)
                } else {
                    (5, 8, Panel::Properties)
                };
                let item = if whole {
                    DockItem::Group { group: source }
                } else {
                    DockItem::Panel { panel }
                };
                s.state
                    .workspace
                    .layout
                    .set_column_collapsed(target, true, viewport)
                    .unwrap();
                let before = s.state.workspace.clone();
                let root = before.layout.column_for_group(target).unwrap();
                let band = before
                    .layout
                    .bands
                    .iter()
                    .find(|b| b.root.id() == root)
                    .unwrap();
                let moved = if whole {
                    before.layout.group_panels(source).unwrap().to_vec()
                } else {
                    vec![panel]
                };
                let g = s
                    .layout(viewport)
                    .groups
                    .into_iter()
                    .find(|g| g.id == source)
                    .unwrap();
                let start = [g.bounds.x + 20., g.bounds.y + 20.];
                let drag = |s: &mut UiSession<Recorder>, phase, position| {
                    s.dispatch(UiAction::DragWorkspace {
                        item,
                        phase,
                        position,
                        viewport,
                        tabs: vec![],
                    })
                    .unwrap();
                };
                for cancel in [true, false] {
                    drag(&mut s, ContactPhase::Down, start);
                    drag(&mut s, ContactPhase::Move, [800., 500.]);
                    let c = s
                        .layout(viewport)
                        .collapsed
                        .into_iter()
                        .find(|c| c.id == root)
                        .unwrap();
                    let point = [
                        if right {
                            c.bounds.x - 20.
                        } else {
                            c.bounds.x + c.bounds.width + 20.
                        },
                        c.bounds.y + c.bounds.height * 0.5,
                    ];
                    drag(&mut s, ContactPhase::Move, point);
                    assert_eq!(
                        s.drop_hint(viewport, point, &[], item, None)
                            .unwrap()
                            .target,
                        DockTarget::BesideBand { band: band.id }
                    );
                    drag(
                        &mut s,
                        if cancel {
                            ContactPhase::Cancel
                        } else {
                            ContactPhase::Up
                        },
                        point,
                    );
                    if cancel {
                        assert_eq!(s.state.workspace, before);
                        assert!(!s.command(CommandId::UndoWorkspace).enabled);
                        continue;
                    }
                    let after = s.state.workspace.clone();
                    after.validate().unwrap();
                    assert!(after.layout.floating.is_empty());
                    assert_eq!(after.layout.collapsed, before.layout.collapsed);
                    let index = after
                        .layout
                        .bands
                        .iter()
                        .position(|b| b.id == band.id)
                        .unwrap();
                    assert_eq!(after.layout.bands[index], *band);
                    let new_column = &after.layout.bands[index + 1];
                    assert_eq!(new_column.edge, band.edge);
                    assert_eq!(
                        after.layout.group_panels(new_column.root.id()).unwrap(),
                        moved
                    );
                    assert!(
                        after
                            .layout
                            .collapsed_column_for_group(new_column.root.id())
                            .is_none()
                    );
                    let r = s.layout(viewport);
                    let c = r.collapsed.iter().find(|c| c.id == root).unwrap();
                    let g = r
                        .groups
                        .iter()
                        .find(|g| g.id == new_column.root.id())
                        .unwrap();
                    assert!(if right {
                        g.bounds.x + g.bounds.width <= c.bounds.x
                    } else {
                        g.bounds.x >= c.bounds.x + c.bounds.width
                    });
                    invoke(&mut s, CommandId::UndoWorkspace);
                    assert_eq!(s.state.workspace, before);
                    assert!(!s.command(CommandId::UndoWorkspace).enabled);
                    invoke(&mut s, CommandId::RedoWorkspace);
                    assert_eq!(s.state.workspace, after);
                }
            }
        }
    }

    #[test]
    fn collapsed_column_drop_reaches_the_full_canvas_side_snap_zone() {
        let viewport = [1200., 900.];
        for right in [false, true] {
            for structure in ["stacked", "nested", "single"] {
                for collapsed in [false, true] {
                    let mut s = session();
                    s.set_platform(Platform::Gtk);
                    let stacked = structure != "single";
                    let (source, band) = if stacked { (8, 3) } else { (5, 7) };
                    let layout = &mut s.state.workspace.layout;
                    for b in &mut layout.bands {
                        if matches!(b.edge, Edge::Left | Edge::Right) {
                            b.edge = if (b.id == band) == right {
                                Edge::Right
                            } else {
                                Edge::Left
                            };
                        }
                    }
                    if structure == "nested" {
                        layout
                            .move_panel(
                                viewport,
                                Panel::Properties,
                                DockTarget::Split {
                                    group: 5,
                                    edge: Edge::Right,
                                },
                            )
                            .unwrap();
                    }
                    layout.set_column_collapsed(source, true, viewport).unwrap();
                    if collapsed {
                        let root = layout
                            .bands
                            .iter()
                            .find(|b| b.id == band)
                            .unwrap()
                            .root
                            .id();
                        layout.set_column_collapsed(root, true, viewport).unwrap();
                    }
                    let column = layout.column_for_group(source).unwrap();
                    let original = s.state.workspace.clone();
                    let r = s.layout(viewport);
                    let divider = r.dividers.iter().find(|d| d.id == band).unwrap();
                    let x = if right {
                        divider.bounds.x
                    } else {
                        divider.bounds.x + divider.bounds.width
                    };
                    let sign = if right { -1. } else { 1. };
                    let item = DockItem::Column { column };
                    for distance in 0..=80 {
                        for fraction in [0.2, 0.5, 0.8] {
                            let point = [
                                x + sign * distance as f32,
                                divider.bounds.y + divider.bounds.height * fraction,
                            ];
                            let hint = s.drop_hint(viewport, point, &[], item, None).unwrap_or_else(|| {
                                panic!("missing column target: right={right}, structure={structure}, collapsed={collapsed}, distance={distance}, fraction={fraction}")
                            });
                            assert_eq!(hint.bounds.height, divider.bounds.height);
                        }
                    }
                    let y = divider.bounds.y + divider.bounds.height * 0.5;
                    assert!(
                        s.drop_hint(viewport, [x + sign * 90., y], &[], item, None)
                            .is_none()
                    );
                    let grip = r.collapsed.iter().find(|c| c.id == column).unwrap().grip;
                    for (phase, position) in [
                        (
                            ContactPhase::Down,
                            [grip.x + grip.width * 0.5, grip.y + grip.height * 0.5],
                        ),
                        (ContactPhase::Move, [x + sign * 20., y]),
                        (ContactPhase::Up, [x + sign * 20., y]),
                    ] {
                        s.dispatch(UiAction::DragWorkspace {
                            item,
                            phase,
                            position,
                            viewport,
                            tabs: vec![],
                        })
                        .unwrap();
                    }
                    let moved = s.state.workspace.clone();
                    assert_ne!(moved, original);
                    assert_eq!(
                        moved.layout.collapsed.len(),
                        original.layout.collapsed.len()
                    );
                    for column in &original.layout.collapsed {
                        assert!(moved.layout.collapsed.contains(column));
                    }
                    assert!(moved.layout.floating.is_empty());
                    assert_eq!(
                        moved.layout.group_edge(source),
                        Some(if right { Edge::Right } else { Edge::Left })
                    );
                    moved.validate().unwrap();
                    invoke(&mut s, CommandId::UndoWorkspace);
                    assert_eq!(s.state.workspace, original);
                    invoke(&mut s, CommandId::RedoWorkspace);
                    assert_eq!(s.state.workspace, moved);
                }
            }
        }
    }

    #[test]
    fn collapsed_column_drag_never_tears_off_and_is_one_undo_transaction() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let viewport = [1200., 900.];
        s.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport })
            .unwrap();
        let original = s.state.workspace.clone();
        let grip = s.layout(viewport).collapsed[0].grip;
        let start = [grip.x + grip.width * 0.5, grip.y + grip.height * 0.5];
        let item = DockItem::Column { column: 4 };
        let drag = |s: &mut UiSession<Recorder>, phase, position| {
            s.dispatch(UiAction::DragWorkspace {
                item,
                phase,
                position,
                viewport,
                tabs: vec![],
            })
            .unwrap();
        };
        drag(&mut s, ContactPhase::Down, start);
        drag(&mut s, ContactPhase::Move, [600., 400.]);
        assert_eq!(s.state.workspace, original);
        assert!(
            s.drop_hint(viewport, [600., 400.], &[], item, None)
                .is_none()
        );
        drag(&mut s, ContactPhase::Up, [600., 400.]);
        assert_eq!(s.state.workspace, original);
        let right = s
            .layout(viewport)
            .groups
            .into_iter()
            .find(|g| g.id == 8)
            .unwrap()
            .bounds;
        let point = [right.x - 2., right.y + 200.];
        drag(&mut s, ContactPhase::Down, start);
        drag(&mut s, ContactPhase::Move, point);
        assert!(s.drop_hint(viewport, point, &[], item, None).is_some());
        drag(&mut s, ContactPhase::Up, point);
        let moved = s.state.workspace.clone();
        assert!(moved.layout.floating.is_empty());
        assert_eq!(moved.layout.group_edge(5), Some(Edge::Right));
        s.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        })
        .unwrap();
        assert_eq!(s.state.workspace, original);
        s.dispatch(UiAction::Invoke {
            command: CommandId::RedoWorkspace,
        })
        .unwrap();
        assert_eq!(s.state.workspace, moved);
    }
    #[test]
    fn resize_collapse_preserves_history_and_pre_gesture_width() {
        let viewport = [1200., 900.];
        for cancel in [false, true] {
            for group in [5, 8] {
                let mut s = session();
                s.set_platform(Platform::Gtk);
                let original = s.state.workspace.clone();
                let root = original.layout.column_for_group(group).unwrap();
                let width = original
                    .layout
                    .column_width_before_resize(root, viewport)
                    .unwrap();
                let band = original
                    .layout
                    .bands
                    .iter()
                    .find(|b| b.root.id() == root)
                    .unwrap();
                let divider = s
                    .layout(viewport)
                    .dividers
                    .into_iter()
                    .find(|d| d.id == band.id)
                    .unwrap();
                let start = [
                    divider.bounds.x + divider.bounds.width * 0.5,
                    divider.bounds.y + 20.,
                ];
                let minimum = if group == 5 {
                    crate::TOOL_PANEL_MIN_WIDTH
                } else {
                    crate::LAYERS_MIN_WIDTH
                };
                let x_for_width = |width| {
                    if divider.reversed {
                        divider.parent.x + divider.parent.width - width - WORKSPACE_SPACING * 0.5
                    } else {
                        divider.parent.x + width + WORKSPACE_SPACING * 0.5
                    }
                };
                let drag = |s: &mut UiSession<Recorder>, phase, x| {
                    s.dispatch(UiAction::DragDivider {
                        id: band.id,
                        phase,
                        position: [x, start[1]],
                        viewport,
                    })
                    .unwrap();
                };
                drag(&mut s, ContactPhase::Down, start[0]);
                for width in [minimum, minimum - TILE_SIZE + 0.5] {
                    drag(&mut s, ContactPhase::Move, x_for_width(width));
                    assert!(!s.state.workspace.layout.is_collapsed(root));
                }
                drag(
                    &mut s,
                    ContactPhase::Move,
                    x_for_width(minimum - TILE_SIZE - 0.5),
                );
                assert!(s.state.workspace.layout.is_collapsed(root));
                let collapsed = s.state.workspace.clone();
                drag(
                    &mut s,
                    ContactPhase::Move,
                    x_for_width(minimum - TILE_SIZE - 1.),
                );
                assert_eq!(s.state.workspace, collapsed, "Hold below the threshold");
                drag(
                    &mut s,
                    if cancel {
                        ContactPhase::Cancel
                    } else {
                        ContactPhase::Up
                    },
                    x_for_width(minimum - TILE_SIZE - 1.),
                );
                if cancel {
                    assert_eq!(s.state.workspace, original);
                    assert!(!s.command(CommandId::UndoWorkspace).enabled);
                } else {
                    assert_eq!(s.state.workspace.layout.collapsed[0].expanded_width, width);
                    s.dispatch(UiAction::Invoke {
                        command: CommandId::UndoWorkspace,
                    })
                    .unwrap();
                    assert_eq!(s.state.workspace, original);
                    s.dispatch(UiAction::Invoke {
                        command: CommandId::RedoWorkspace,
                    })
                    .unwrap();
                    assert_eq!(s.state.workspace, collapsed);
                    s.dispatch(UiAction::Customize {
                        action: CustomizationAction::SetColumnCollapsed {
                            group: root,
                            collapsed: false,
                        },
                    })
                    .unwrap();
                    assert!(
                        (s.state
                            .workspace
                            .layout
                            .column_width_before_resize(root, viewport)
                            .unwrap()
                            - width)
                            .abs()
                            < 0.5
                    );
                }
            }
        }
    }

    struct CollapsedResizeTest {
        session: UiSession<Recorder>,
        viewport: [f32; 2],
        id: u32,
        root: u32,
        start: [f32; 2],
        center: f32,
        outward: f32,
        saved_width: f32,
        minimum: f32,
    }
    impl CollapsedResizeTest {
        fn new(platform: Platform, right: bool, nested: Option<bool>) -> Self {
            let mut session = session();
            session.set_platform(platform);
            let viewport = [1600., 1000.];
            let (id, root) = if let Some(second) = nested {
                let band = &mut session.state.workspace.layout.bands[0];
                band.extent = 560.;
                band.edge = if right { Edge::Right } else { Edge::Left };
                let DockNode::Split { axis, .. } = &mut band.root else {
                    unreachable!();
                };
                *axis = Axis::Horizontal;
                (4, if second { 6 } else { 5 })
            } else if right {
                session.state.workspace.layout.bands[1].extent = 400.;
                (7, 8)
            } else {
                session.state.workspace.layout.bands[0].extent = 400.;
                (3, 4)
            };
            let saved_width = session
                .state
                .workspace
                .layout
                .column_width_before_resize(root, viewport)
                .unwrap();
            session
                .state
                .workspace
                .layout
                .set_column_collapsed(root, true, viewport)
                .unwrap();
            let d = session.divider(id, viewport).unwrap();
            Self {
                session,
                viewport,
                id,
                root,
                // Grab off center to exercise the native divider's hit-area offset.
                start: [d.bounds.x + 1., d.bounds.y + 20.],
                center: d.bounds.x + d.bounds.width * 0.5,
                outward: if nested.unwrap_or(right) { -1. } else { 1. },
                saved_width,
                minimum: if nested == Some(true) {
                    TILE_SIZE
                } else if nested.is_none() && right {
                    crate::LAYERS_MIN_WIDTH
                } else {
                    crate::TOOL_PANEL_MIN_WIDTH
                },
            }
        }
        fn drag(&mut self, phase: ContactPhase, distance: f32) {
            self.session
                .dispatch(UiAction::DragDivider {
                    id: self.id,
                    phase,
                    position: [self.start[0] + self.outward * distance, self.start[1]],
                    viewport: self.viewport,
                })
                .unwrap();
        }
        fn edge_distance(&self) -> f32 {
            let d = self.session.divider(self.id, self.viewport).unwrap();
            (d.bounds.x + d.bounds.width * 0.5 - self.center) * self.outward
        }
        fn width(&self) -> f32 {
            self.session
                .state
                .workspace
                .layout
                .column_width_before_resize(self.root, self.viewport)
                .unwrap()
        }
    }

    #[test]
    fn collapsed_divider_does_not_move_or_record_history_below_opening_threshold() {
        for right in [false, true] {
            for end in [ContactPhase::Up, ContactPhase::Cancel] {
                let mut t = CollapsedResizeTest::new(Platform::Gtk, right, None);
                let before = t.session.state.workspace.clone();
                t.drag(ContactPhase::Down, 0.);
                for distance in [0., 18., 35.5, -100., 35.5] {
                    t.drag(ContactPhase::Move, distance);
                    assert_eq!(
                        t.session.state.workspace, before,
                        "collapsed dock must stay fixed"
                    );
                    assert_eq!(t.edge_distance(), 0.);
                }
                t.drag(end, 35.5);
                assert_eq!(t.session.state.workspace, before);
                assert!(!t.session.command(CommandId::UndoWorkspace).enabled);
            }
        }
    }

    #[test]
    fn expanded_divider_waits_for_pointer_then_resizes_and_can_collapse_again() {
        for platform in [
            Platform::Generic,
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
            for right in [false, true] {
                let mut t = CollapsedResizeTest::new(platform, right, None);
                let before = t.session.state.workspace.clone();
                t.drag(ContactPhase::Down, 0.);
                t.drag(ContactPhase::Move, 36.);
                assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
                assert!(t.saved_width > t.minimum);
                assert!((t.width() - t.minimum).abs() < 0.01);
                let expanded = t.session.state.workspace.clone();
                let edge = t.edge_distance();
                assert!(edge > 36.);
                for distance in [36., 40., edge - 0.5, 36.] {
                    t.drag(ContactPhase::Move, distance);
                    assert_eq!(
                        t.session.state.workspace, expanded,
                        "use the opening threshold until the pointer reaches the edge"
                    );
                }
                for distance in [35.5, 0., -40., 35.5] {
                    t.drag(ContactPhase::Move, distance);
                    assert_eq!(t.session.state.workspace, before);
                    assert_eq!(t.edge_distance(), 0.);
                }
                t.drag(ContactPhase::Move, 36.);
                assert_eq!(t.session.state.workspace, expanded);
                t.drag(ContactPhase::Move, edge);
                assert_eq!(t.session.state.workspace, expanded);
                t.drag(ContactPhase::Move, edge + 45.);
                assert!((t.width() - t.minimum - 45.).abs() < 0.01);
                let minimum = if right {
                    crate::LAYERS_MIN_WIDTH
                } else {
                    crate::TOOL_PANEL_MIN_WIDTH
                };
                t.drag(ContactPhase::Move, minimum - TILE_SIZE - TILE_SIZE + 0.5);
                assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
                t.drag(ContactPhase::Move, minimum - TILE_SIZE - TILE_SIZE - 0.5);
                let collapsed = t.session.state.workspace.clone();
                assert!(collapsed.layout.is_collapsed(t.root));
                assert_eq!(collapsed.layout.collapsed[0].expanded_width, t.saved_width);
                // Reversing this collapse resumes immediately, even before the
                // pointer has reached the minimum-width edge.
                t.drag(ContactPhase::Move, minimum - TILE_SIZE - TILE_SIZE + 0.5);
                assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
                assert!((t.width() - t.minimum).abs() < 0.01);
                t.drag(ContactPhase::Move, minimum - TILE_SIZE - TILE_SIZE - 0.5);
                assert_eq!(t.session.state.workspace, collapsed);
                t.drag(ContactPhase::Up, edge + 100.);
                assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
                assert!((t.width() - t.minimum - 100.).abs() < 0.01);
            }
        }
    }

    #[test]
    fn reversing_an_opening_before_the_edge_restores_the_collapsed_layout_without_history() {
        for right in [false, true] {
            for end in [ContactPhase::Up, ContactPhase::Cancel] {
                for release_crosses_threshold in [false, true] {
                    let mut t = CollapsedResizeTest::new(Platform::Gtk, right, None);
                    let mut before = t.session.state.workspace.clone();
                    t.drag(ContactPhase::Down, 0.);
                    for cycle in 0..3 {
                        t.drag(ContactPhase::Move, 36.);
                        if cycle == 0 {
                            // Hosts may deliver fresh measurements when panels appear.
                            before.layout.measurements.push(crate::PanelMeasurement {
                                panel: Panel::Brushes,
                                tab_width: 120.,
                                content_height: 200.,
                            });
                            before.layout.column_scroll.push((t.root, 12.));
                            t.session
                                .state
                                .workspace
                                .layout
                                .measurements
                                .clone_from(&before.layout.measurements);
                            t.session
                                .state
                                .workspace
                                .layout
                                .column_scroll
                                .clone_from(&before.layout.column_scroll);
                        }
                        assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
                        assert!((t.width() - t.minimum).abs() < 0.01);
                        t.drag(ContactPhase::Move, 35.5);
                        assert_eq!(t.session.state.workspace, before);
                        assert_eq!(t.edge_distance(), 0.);
                    }
                    if release_crosses_threshold {
                        t.drag(ContactPhase::Move, 36.);
                    }
                    t.drag(end, 35.5);
                    assert_eq!(t.session.state.workspace, before);
                    assert!(!t.session.command(CommandId::UndoWorkspace).enabled);
                }
            }
        }
    }

    #[test]
    fn reversing_an_opening_can_open_the_opposite_collapsed_neighbor_in_one_move() {
        for right in [false, true] {
            // Open the anchored subcolumn first so its edge moves outward.
            let mut t = CollapsedResizeTest::new(Platform::Gtk, right, Some(right));
            let other = if right { 5 } else { 6 };
            t.session
                .state
                .workspace
                .layout
                .add_panel_to_group(Panel::ToolSettings, 6)
                .unwrap();
            t.session
                .state
                .workspace
                .layout
                .set_column_collapsed(other, true, t.viewport)
                .unwrap();
            let d = t.session.divider(t.id, t.viewport).unwrap();
            t.start[0] = d.bounds.x + 1.;
            t.center = d.bounds.x + d.bounds.width * 0.5;
            let before = t.session.state.workspace.clone();
            t.drag(ContactPhase::Down, 0.);
            t.drag(ContactPhase::Move, 36.);
            assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
            assert!(t.edge_distance() > 36.);
            t.drag(ContactPhase::Move, -36.);
            assert!(t.session.state.workspace.layout.is_collapsed(t.root));
            assert!(!t.session.state.workspace.layout.is_collapsed(other));
            t.drag(ContactPhase::Cancel, -36.);
            assert_eq!(t.session.state.workspace, before);
        }
    }

    #[test]
    fn drag_collapse_reverses_at_its_original_threshold_without_an_edge_wait() {
        for right in [false, true] {
            for nested in [None, Some(false), Some(true)] {
                for finish in 0..3 {
                    let mut t = CollapsedResizeTest::new(Platform::Gtk, right, nested);
                    t.session
                        .state
                        .workspace
                        .layout
                        .set_column_collapsed(t.root, false, t.viewport)
                        .unwrap();
                    let before = t.session.state.workspace.clone();
                    let d = t.session.divider(t.id, t.viewport).unwrap();
                    t.start = [d.bounds.x + 1., d.bounds.y + 20.];
                    t.center = d.bounds.x + d.bounds.width * 0.5;
                    let origin = if t.outward < 0. {
                        d.parent.x + d.parent.width - WORKSPACE_SPACING * 0.5
                    } else {
                        d.parent.x + WORKSPACE_SPACING * 0.5
                    };
                    let base = (origin - t.center) * t.outward;
                    let distance = |width| base + width;
                    let threshold = (t.minimum - TILE_SIZE).max(TILE_SIZE);
                    t.drag(ContactPhase::Down, 0.);
                    // Jump directly from a wide column across the threshold.
                    // Nested columns must retain the pre-collapse parent edge.
                    for _ in 0..3 {
                        t.drag(ContactPhase::Move, distance(threshold - 0.5));
                        assert!(t.session.state.workspace.layout.is_collapsed(t.root));
                        let collapsed = t.session.state.workspace.clone();
                        t.drag(ContactPhase::Move, distance(threshold - 10.));
                        assert_eq!(t.session.state.workspace, collapsed);
                        t.drag(ContactPhase::Move, distance(threshold));
                        assert!(
                            t.session.state.workspace.layout.is_collapsed(t.root),
                            "the 36px collapse threshold is inclusive"
                        );
                        t.drag(ContactPhase::Move, distance(threshold + 0.5));
                        assert!(
                            !t.session.state.workspace.layout.is_collapsed(t.root),
                            "right={right}, nested={nested:?}"
                        );
                        if nested.is_none() || nested == Some(false) {
                            assert!((t.width() - t.minimum).abs() < 0.01);
                        }
                        // The next iteration returns inside the threshold
                        // without ever catching up to the expanded edge.
                    }
                    let after = t.session.state.workspace.clone();
                    match finish {
                        0 => {
                            t.drag(ContactPhase::Up, distance(threshold + 0.5));
                            invoke(&mut t.session, CommandId::UndoWorkspace);
                            assert_eq!(t.session.state.workspace, before);
                            assert!(!t.session.command(CommandId::UndoWorkspace).enabled);
                            invoke(&mut t.session, CommandId::RedoWorkspace);
                            assert_eq!(t.session.state.workspace, after);
                        }
                        1 => t.drag(ContactPhase::Cancel, distance(threshold + 0.5)),
                        _ => {
                            t.session.input(UiInput::Blur).unwrap();
                        }
                    }
                    if finish != 0 {
                        assert_eq!(t.session.state.workspace, before);
                        assert!(!t.session.command(CommandId::UndoWorkspace).enabled);
                    }
                }
            }
        }
    }

    #[test]
    fn collapsed_divider_expansion_preserves_history_cancel_and_blur() {
        for right in [false, true] {
            for skip_to_edge in [false, true] {
                for finish in 0..3 {
                    let mut t = CollapsedResizeTest::new(Platform::Gtk, right, None);
                    let before = t.session.state.workspace.clone();
                    t.drag(ContactPhase::Down, 0.);
                    let distance = if skip_to_edge {
                        t.minimum - TILE_SIZE + 70.
                    } else {
                        36.
                    };
                    // Repeated tentative openings still form a single history action.
                    t.drag(ContactPhase::Move, 36.);
                    t.drag(ContactPhase::Move, 35.5);
                    assert_eq!(t.session.state.workspace, before);
                    // Include a release that is the first event past the threshold.
                    if finish == 0 {
                        t.drag(ContactPhase::Up, distance);
                    } else {
                        t.drag(ContactPhase::Move, distance);
                    }
                    assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
                    assert!(
                        (t.width() - t.minimum - if skip_to_edge { 70. } else { 0. }).abs() < 0.01
                    );
                    let after = t.session.state.workspace.clone();
                    match finish {
                        0 => {
                            invoke(&mut t.session, CommandId::UndoWorkspace);
                            assert_eq!(t.session.state.workspace, before);
                            assert!(!t.session.command(CommandId::UndoWorkspace).enabled);
                            invoke(&mut t.session, CommandId::RedoWorkspace);
                            assert_eq!(t.session.state.workspace, after);
                        }
                        1 => t.drag(ContactPhase::Cancel, distance),
                        _ => {
                            t.session.input(UiInput::Blur).unwrap();
                        }
                    }
                    if finish != 0 {
                        assert_eq!(t.session.state.workspace, before);
                        assert!(!t.session.command(CommandId::UndoWorkspace).enabled);
                    }
                }
            }
        }
    }

    #[test]
    fn nested_collapsed_dividers_wait_then_open_the_requested_side() {
        for right in [false, true] {
            for second in [false, true] {
                let mut t = CollapsedResizeTest::new(Platform::Gtk, right, Some(second));
                let before = t.session.state.workspace.clone();
                t.drag(ContactPhase::Down, 0.);
                for distance in [35.5, -80., 0.] {
                    t.drag(ContactPhase::Move, distance);
                    assert_eq!(t.session.state.workspace, before);
                }
                t.drag(ContactPhase::Move, 36.);
                assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
                let expanded = t.session.state.workspace.clone();
                let edge = t.edge_distance();
                if edge > 36. {
                    t.drag(ContactPhase::Move, edge - 0.5);
                    assert_eq!(t.session.state.workspace, expanded);
                    t.drag(ContactPhase::Move, 35.5);
                    assert_eq!(t.session.state.workspace, before);
                    t.drag(ContactPhase::Move, 36.);
                    assert_eq!(t.session.state.workspace, expanded);
                }
                t.drag(ContactPhase::Move, edge + 20.);
                assert!((t.edge_distance() - edge - 20.).abs() < 0.01);
                t.drag(ContactPhase::Cancel, edge + 20.);
                assert_eq!(t.session.state.workspace, before);
            }
        }
    }

    #[test]
    fn adjacent_collapsed_columns_do_not_latch_the_expanding_neighbor() {
        for right in [false, true] {
            for second in [false, true] {
                let mut t = CollapsedResizeTest::new(Platform::Gtk, right, Some(second));
                let other = if second { 5 } else { 6 };
                t.session
                    .state
                    .workspace
                    .layout
                    .set_column_collapsed(other, true, t.viewport)
                    .unwrap();
                let d = t.session.divider(t.id, t.viewport).unwrap();
                t.start[0] = d.bounds.x + 1.;
                t.center = d.bounds.x + d.bounds.width * 0.5;
                let before = t.session.state.workspace.clone();
                t.drag(ContactPhase::Down, 0.);
                t.drag(ContactPhase::Move, 35.5);
                assert_eq!(t.session.state.workspace, before);
                t.drag(ContactPhase::Move, 36.);
                assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
                assert!(t.session.state.workspace.layout.is_collapsed(other));
                t.drag(ContactPhase::Move, t.edge_distance().max(36.));
                let d = t.session.divider(t.id, t.viewport).unwrap();
                let x = if second {
                    d.parent.x + d.parent.width - TILE_SIZE * 0.5
                } else {
                    d.parent.x + TILE_SIZE * 0.5
                };
                t.drag(ContactPhase::Move, (x - t.center) * t.outward);
                assert!(t.session.state.workspace.layout.is_collapsed(t.root));
                assert!(t.session.state.workspace.layout.is_collapsed(other));
                t.drag(ContactPhase::Cancel, 0.);
                assert_eq!(t.session.state.workspace, before);
            }
        }
    }

    #[test]
    fn outside_band_handle_opens_its_collapsed_subcolumn() {
        for right in [false, true] {
            let mut t = CollapsedResizeTest::new(Platform::Gtk, right, Some(!right));
            t.id = 3;
            t.outward = if right { -1. } else { 1. };
            let d = t.session.divider(t.id, t.viewport).unwrap();
            t.start[0] = d.bounds.x + 1.;
            t.center = d.bounds.x + d.bounds.width * 0.5;
            let before = t.session.state.workspace.clone();
            t.drag(ContactPhase::Down, 0.);
            t.drag(ContactPhase::Move, 35.5);
            assert_eq!(t.session.state.workspace, before);
            t.drag(ContactPhase::Move, 36.);
            assert!(!t.session.state.workspace.layout.is_collapsed(t.root));
            // A content-free subcolumn's minimum edge is already behind the
            // pointer at opening, so normal band resizing can grow it at once.
            assert!(t.width() >= t.minimum && t.width() < t.saved_width);
            let expanded = t.session.state.workspace.clone();
            let edge = t.edge_distance();
            if edge > 36. {
                assert!((t.width() - t.minimum).abs() < 0.01);
                t.drag(ContactPhase::Move, edge - 0.5);
                assert_eq!(t.session.state.workspace, expanded);
                t.drag(ContactPhase::Move, 35.5);
                assert_eq!(t.session.state.workspace, before);
                t.drag(ContactPhase::Move, 36.);
                assert_eq!(t.session.state.workspace, expanded);
            }
            t.drag(ContactPhase::Move, edge + 20.);
            assert!((t.edge_distance() - edge - 20.).abs() < 0.01);
            t.drag(ContactPhase::Cancel, 0.);
            assert_eq!(t.session.state.workspace, before);
        }
    }

    #[test]
    fn column_drawer_anchor_follows_tab_and_sidebar_selection_on_gtk_android_and_web() {
        for platform in [Platform::Gtk, Platform::Android, Platform::Web] {
            let mut s = session();
            s.set_platform(platform);
            let viewport = [1200., 900.];
            s.dispatch(UiAction::DoubleClickPanelHandle { group: 8, viewport })
                .unwrap();
            let toggle = |s: &mut UiSession<Recorder>, panel| {
                s.dispatch(UiAction::Customize {
                    action: CustomizationAction::ToggleColumnDrawer { group: 8, panel },
                })
                .unwrap();
            };
            toggle(&mut s, Panel::Layers);
            s.dispatch(UiAction::SelectPanelTab {
                group: 8,
                panel: Panel::Properties,
            })
            .unwrap();
            let drawer = &s.state.customization.column_drawers[0];
            assert!(matches!(
                drawer.anchor,
                DrawerAnchor::Column {
                    origin: Panel::Properties,
                    ..
                }
            ));
            assert_eq!(drawer.columns, vec![vec![Panel::Properties]]);
            let layout = &s.state.workspace.layout;
            let resolved = layout.workspace(viewport[0], viewport[1], HEADER_HEIGHT, STATUS_HEIGHT);
            let column = &resolved.collapsed[0];
            let expected = column
                .groups
                .iter()
                .flat_map(|g| &g.icons)
                .find(|i| i.panel == Panel::Properties)
                .unwrap()
                .bounds
                .intersection(column.content)
                .unwrap();
            assert_eq!(
                drawer
                    .placement(layout, viewport, &[500.], false)
                    .unwrap()
                    .anchor,
                expected
            );
            // The former opening button switches back; the current one closes the drawer.
            toggle(&mut s, Panel::Layers);
            assert_eq!(s.state.customization.column_drawers.len(), 1);
            assert!(matches!(
                s.state.customization.column_drawers[0].anchor,
                DrawerAnchor::Column {
                    origin: Panel::Layers,
                    ..
                }
            ));
            toggle(&mut s, Panel::Layers);
            assert!(s.state.customization.column_drawers.is_empty());
        }
    }

    #[test]
    fn collapsed_drawers_are_persistent_per_column_and_follow_tab_selection() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let viewport = [1200., 900.];
        let collapse = |s: &mut UiSession<Recorder>, group| {
            s.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                .unwrap()
        };
        collapse(&mut s, 5);
        collapse(&mut s, 8);
        assert_eq!(s.state.workspace.layout.collapsed.len(), 2);
        let open = |s: &mut UiSession<Recorder>, group, panel| {
            s.dispatch(UiAction::Customize {
                action: CustomizationAction::ToggleColumnDrawer { group, panel },
            })
            .unwrap()
        };
        open(&mut s, 8, Panel::Layers);
        open(&mut s, 5, Panel::Brushes);
        assert_eq!(s.state.customization.column_drawers.len(), 2);
        let width = s.state.customization.column_drawers[0].column_widths();
        assert_eq!(
            width,
            vec![
                Panel::Layers
                    .drawer_width()
                    .max(Panel::Properties.drawer_width())
                    .max(Panel::Adjustments.drawer_width())
            ]
        );
        s.dispatch(UiAction::SelectPanelTab {
            group: 8,
            panel: Panel::Properties,
        })
        .unwrap();
        let d = &s.state.customization.column_drawers[0];
        assert_eq!(d.tabs.as_ref().unwrap().active, Panel::Properties);
        assert_eq!(d.column_widths(), width);
        assert_eq!(d.dismissal, DrawerDismissal::Explicit);
        s.input(UiInput::Chrome {
            event: ChromeEvent::Contact {
                position: [600., 500.],
                canvas: true,
            },
            facts: ChromeFacts::default(),
            viewport,
        })
        .unwrap();
        assert_eq!(s.state.customization.column_drawers.len(), 2);
        // Re-selecting the active drawer tab never opens configuration.
        s.dispatch(UiAction::SelectPanelTab {
            group: 8,
            panel: Panel::Properties,
        })
        .unwrap();
        assert!(s.state.customization.expanded.is_none());
        open(&mut s, 8, Panel::Properties);
        assert_eq!(s.state.customization.column_drawers.len(), 1);
        open(&mut s, 6, Panel::Sizes);
        assert_eq!(s.state.customization.column_drawers.len(), 1);
        assert_eq!(
            s.state.customization.column_drawers[0].columns,
            vec![vec![Panel::Sizes]]
        );
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::SetColumnCollapsed {
                group: 4,
                collapsed: false,
            },
        })
        .unwrap();
        assert!(s.state.customization.column_drawers.is_empty());
        assert!(!s.state.workspace.layout.is_collapsed(4));
    }

    #[test]
    fn column_drawer_drag_tear_off_cancel_dock_and_history() {
        let viewport = [1200., 900.];
        for (platform, (group, panel, whole)) in [Platform::Gtk, Platform::Windows]
            .into_iter()
            .flat_map(|platform| {
                [
                    (5, Panel::Brushes, false),
                    (8, Panel::Properties, false),
                    (8, Panel::Layers, true),
                ]
                .map(|case| (platform, case))
            })
        {
            let mut s = session();
            s.set_platform(platform);
            s.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                .unwrap();
            s.dispatch(UiAction::Customize {
                action: CustomizationAction::ToggleColumnDrawer { group, panel },
            })
            .unwrap();
            let drawer = s.state.customization.column_drawers[0].clone();
            let bounds = drawer
                .placement(&s.state.workspace.layout, viewport, &[450.], false)
                .unwrap()
                .bounds;
            let before = s.state.workspace.clone();
            let item = if whole {
                DockItem::Group { group }
            } else {
                DockItem::Panel { panel }
            };
            let measure = |s: &mut UiSession<Recorder>| {
                s.dispatch(UiAction::MeasureColumnDrawers {
                    measurements: vec![ColumnDrawerMeasurement { group, bounds }],
                })
                .unwrap();
            };
            let drag = |s: &mut UiSession<Recorder>, phase, position| {
                s.dispatch(UiAction::DragWorkspace {
                    item,
                    phase,
                    position,
                    viewport,
                    tabs: vec![],
                })
                .unwrap();
            };
            let press = [bounds.x + bounds.width * 0.5, bounds.y + 18.];
            let away = [600., 500.];
            measure(&mut s);
            drag(&mut s, ContactPhase::Down, press);
            drag(&mut s, ContactPhase::Move, [press[0] + 12., press[1]]);
            assert_eq!(s.state.workspace, before);
            assert_eq!(
                s.state.customization.column_drawers,
                std::slice::from_ref(&drawer)
            );
            drag(&mut s, ContactPhase::Move, away);
            assert_eq!(s.state.workspace.layout.floating.len(), 1);
            assert!(s.state.customization.column_drawers.is_empty());
            drag(&mut s, ContactPhase::Cancel, away);
            assert_eq!(s.state.workspace, before);
            assert_eq!(
                s.state.customization.column_drawers,
                std::slice::from_ref(&drawer)
            );
            for dock in [false, true] {
                measure(&mut s);
                drag(&mut s, ContactPhase::Down, press);
                drag(&mut s, ContactPhase::Move, away);
                let point = if dock {
                    let target = s
                        .layout(viewport)
                        .groups
                        .into_iter()
                        .find(|g| g.id == if group == 5 { 8 } else { 5 })
                        .unwrap();
                    [
                        target.bounds.x + target.bounds.width * 0.5,
                        target.bounds.y + 18.,
                    ]
                } else {
                    away
                };
                drag(&mut s, ContactPhase::Move, point);
                drag(&mut s, ContactPhase::Up, point);
                assert_eq!(s.state.workspace.layout.floating.len(), usize::from(!dock));
                if dock {
                    let target = if group == 5 { 8 } else { 5 };
                    assert_eq!(s.state.workspace.layout.panel_group(panel), Some(target));
                    if whole {
                        for panel in before.layout.group_panels(group).unwrap() {
                            assert_eq!(s.state.workspace.layout.panel_group(*panel), Some(target));
                        }
                    }
                }
                s.state.workspace.layout.validate().unwrap();
                let after = s.state.workspace.clone();
                invoke(&mut s, CommandId::UndoWorkspace);
                assert_eq!(s.state.workspace, before);
                invoke(&mut s, CommandId::RedoWorkspace);
                assert_eq!(s.state.workspace, after);
                invoke(&mut s, CommandId::UndoWorkspace);
                s.state.customization.column_drawers = vec![drawer.clone()];
            }
        }
    }

    #[test]
    fn attached_tab_release_uses_frozen_halfway_points() {
        for (delta, insertion, order) in [
            (
                31.,
                3,
                [Panel::Layers, Panel::Properties, Panel::Adjustments],
            ),
            (
                -41.,
                0,
                [Panel::Adjustments, Panel::Layers, Panel::Properties],
            ),
        ] {
            let mut s = session();
            let viewport = [1200., 900.];
            let group = 8;
            let bounds = s
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds;
            let mut tabs: Vec<_> = [80., 36., 60.]
                .into_iter()
                .enumerate()
                .scan(bounds.x, |x, (index, width)| {
                    let tab = TabHit {
                        group,
                        index,
                        bounds: Bounds {
                            x: *x,
                            width,
                            height: TAB_BAR_HEIGHT,
                            ..bounds
                        },
                    };
                    *x += width;
                    Some(tab)
                })
                .collect();
            let press = [bounds.x + 98., bounds.y + 18.];
            let item = DockItem::Panel {
                panel: Panel::Adjustments,
            };
            let before = s.state.workspace.clone();
            s.drag_workspace(item, ContactPhase::Down, press, viewport, &tabs)
                .unwrap();
            s.begin_tab_drag(&tabs, bounds);
            s.drag_workspace(
                item,
                ContactPhase::Move,
                [press[0] + 9., press[1]],
                viewport,
                &tabs,
            )
            .unwrap();
            assert_eq!(s.state.workspace, before);
            // Even a changed host measurement cannot change the chosen slot.
            for tab in &mut tabs {
                tab.bounds.x += 100.;
            }
            let release = [press[0] + delta, press[1]];
            assert_eq!(s.tab_drag_preview(release).unwrap().insertion, insertion);
            assert_eq!(
                s.drop_hint(viewport, release, &tabs, item, None)
                    .unwrap()
                    .target,
                DockTarget::Tab {
                    group,
                    index: Some(insertion)
                }
            );
            // Release can cross a switch without an intervening move event.
            s.drag_workspace(item, ContactPhase::Up, release, viewport, &tabs)
                .unwrap();
            assert_eq!(
                s.state.workspace.layout.group_panels(group).unwrap(),
                &order
            );
            assert!(s.tab_drag_preview(release).is_none());
            invoke(&mut s, CommandId::UndoWorkspace);
            assert_eq!(s.state.workspace, before);
        }
    }

    #[test]
    fn detached_tabs_preserve_the_grab_point_within_each_tab() {
        for (index, panel) in [Panel::Layers, Panel::Adjustments, Panel::Properties]
            .into_iter()
            .enumerate()
        {
            let mut s = session();
            let viewport = [1200., 900.];
            let bounds = s
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.id == 8)
                .unwrap()
                .bounds;
            let tabs: Vec<_> = [80., 36., 60.]
                .into_iter()
                .enumerate()
                .scan(bounds.x, |x, (index, width)| {
                    let tab = TabHit {
                        group: 8,
                        index,
                        bounds: Bounds {
                            x: *x,
                            width,
                            height: TAB_BAR_HEIGHT,
                            ..bounds
                        },
                    };
                    *x += width;
                    Some(tab)
                })
                .collect();
            let before = s.state.workspace.clone();
            let item = DockItem::Panel { panel };
            for offset in [7., tabs[index].bounds.width - 2.] {
                let press = [tabs[index].bounds.x + offset, bounds.y + 18.];
                s.drag_workspace(item, ContactPhase::Down, press, viewport, &tabs)
                    .unwrap();
                s.begin_tab_drag(&tabs, bounds);
                let away = [600., 450.];
                s.drag_workspace(item, ContactPhase::Move, away, viewport, &tabs)
                    .unwrap();
                let floated = s
                    .layout(viewport)
                    .groups
                    .into_iter()
                    .find(|g| g.floating && g.active == panel)
                    .unwrap();
                assert_eq!(floated.bounds.x, away[0] - offset);
                assert_eq!(floated.bounds.y, away[1] - 18.);
                assert!(s.tab_drag_preview(away).is_none());
                s.drag_workspace(item, ContactPhase::Cancel, away, viewport, &tabs)
                    .unwrap();
                assert_eq!(s.state.workspace, before);
            }
        }
    }

    #[test]
    fn column_drawer_tabs_reorder_and_reject_stale_geometry() {
        for platform in [Platform::Generic, Platform::Gtk, Platform::Windows] {
            let mut s = session();
            s.set_platform(platform);
            let viewport = [1200., 900.];
            let group = 8;
            s.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                .unwrap();
            s.dispatch(UiAction::Customize {
                action: CustomizationAction::ToggleColumnDrawer {
                    group,
                    panel: Panel::Layers,
                },
            })
            .unwrap();
            let bounds = s.state.customization.column_drawers[0]
                .placement(&s.state.workspace.layout, viewport, &[450.], false)
                .unwrap()
                .bounds;
            s.dispatch(UiAction::MeasureColumnDrawers {
                measurements: vec![ColumnDrawerMeasurement { group, bounds }],
            })
            .unwrap();
            let before = s.state.workspace.clone();
            let tabs: Vec<_> = (0..3)
                .map(|index| TabHit {
                    group,
                    index,
                    bounds: Bounds {
                        x: bounds.x + index as f32 * 90.,
                        y: bounds.y,
                        width: 90.,
                        height: TAB_BAR_HEIGHT,
                    },
                })
                .collect();
            let item = DockItem::Panel {
                panel: Panel::Properties,
            };
            for (phase, position) in [
                (ContactPhase::Down, [bounds.x + 220., bounds.y + 18.]),
                (ContactPhase::Move, [bounds.x + 10., bounds.y + 18.]),
                (ContactPhase::Up, [bounds.x + 10., bounds.y + 18.]),
            ] {
                s.dispatch(UiAction::DragWorkspace {
                    item,
                    phase,
                    position,
                    viewport,
                    tabs: tabs.clone(),
                })
                .unwrap();
            }
            assert_eq!(
                s.state.workspace.layout.group_panels(group).unwrap(),
                &[Panel::Properties, Panel::Layers, Panel::Adjustments]
            );
            assert!(s.state.workspace.layout.is_collapsed(group));
            assert!(s.state.workspace.layout.floating.is_empty());
            assert_eq!(s.state.customization.column_drawers.len(), 1);
            invoke(&mut s, CommandId::UndoWorkspace);
            assert_eq!(s.state.workspace, before);
            s.state.customization.column_drawers.clear();
            assert!(
                s.dispatch(UiAction::DragWorkspace {
                    item,
                    phase: ContactPhase::Down,
                    position: [bounds.x + 10., bounds.y + 18.],
                    viewport,
                    tabs,
                })
                .is_err()
            );
            assert!(
                serde_json::to_value(&s.state.customization)
                    .unwrap()
                    .get("column_drawer_bounds")
                    .is_none()
            );
        }
    }

    #[test]
    fn collapsed_toolbar_tiles_use_live_origins_and_keep_the_parent_drawer() {
        let viewport = [1200., 900.];
        for group in [5, 8] {
            let mut s = session();
            s.set_platform(Platform::Gtk);
            s.dispatch(UiAction::MovePanel {
                panel: Panel::Toolbar,
                target: DockTarget::Tab { group, index: None },
                viewport,
            })
            .unwrap();
            s.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                .unwrap();
            s.dispatch(UiAction::Customize {
                action: CustomizationAction::ToggleColumnDrawer {
                    group,
                    panel: Panel::Toolbar,
                },
            })
            .unwrap();
            let DrawerAnchor::Column { column, .. } =
                s.state.customization.column_drawers[0].anchor
            else {
                panic!()
            };
            let tile = s
                .state
                .workspace
                .layout
                .panel(Panel::Toolbar)
                .unwrap()
                .tiles()[0]
                .id;
            let anchor = TileAnchor {
                panel: Panel::Toolbar,
                tile,
            };
            let activate = UiAction::ActivateTile {
                panel: anchor.panel,
                tile,
            };
            s.dispatch(activate.clone()).unwrap(); // First press selects Brush.
            assert!(s.state.customization.drawer.is_none());
            assert!(s.dispatch(activate.clone()).is_err()); // No visible native origin yet.
            let before = s.state.workspace.clone();
            let measure = |s: &mut UiSession<Recorder>, y| {
                s.dispatch(UiAction::MeasureDrawerTiles {
                    measurements: vec![DrawerTileMeasurement {
                        column,
                        anchor,
                        bounds: Bounds {
                            x: if group == 5 { 80. } else { 1000. },
                            y,
                            width: 36.,
                            height: 36.,
                        },
                    }],
                })
                .unwrap()
            };
            measure(&mut s, 200.);
            assert_eq!(s.state.workspace, before);
            s.dispatch(activate.clone()).unwrap();
            let placement = |s: &UiSession<Recorder>| {
                s.state
                    .customization
                    .drawer_placement(
                        s.state.customization.drawer.as_ref().unwrap(),
                        &s.state.workspace.layout,
                        viewport,
                        &[400., 450.],
                        false,
                    )
                    .unwrap()
            };
            let first = placement(&s);
            assert_eq!(
                first.direction,
                if group == 5 { Edge::Right } else { Edge::Left }
            );
            assert!(first.connection().is_some());
            measure(&mut s, 160.); // Scrolling/animation updates the same open drawer.
            let current = placement(&s);
            assert_eq!(current.anchor.y, 160.);
            assert_eq!(s.state.customization.column_drawers.len(), 1);
            let reply = s
                .input(UiInput::Chrome {
                    event: ChromeEvent::Contact {
                        position: [current.anchor.x + 4., 164.],
                        canvas: false,
                    },
                    facts: ChromeFacts {
                        content_drawer: Some(current.bounds),
                        ..Default::default()
                    },
                    viewport,
                })
                .unwrap();
            assert!(!reply.handled);
            assert!(s.state.customization.drawer.is_some());
            s.dispatch(activate.clone()).unwrap(); // Original tile closes only its child.
            assert!(s.state.customization.drawer.is_none());
            assert_eq!(s.state.customization.column_drawers.len(), 1);
            s.dispatch(activate.clone()).unwrap();
            let reply = s
                .input(UiInput::Chrome {
                    event: ChromeEvent::Contact {
                        position: [600., 850.],
                        canvas: true,
                    },
                    facts: ChromeFacts {
                        content_drawer: Some(placement(&s).bounds),
                        ..Default::default()
                    },
                    viewport,
                })
                .unwrap();
            assert!(reply.handled);
            assert!(s.state.customization.drawer.is_none());
            assert_eq!(s.state.customization.column_drawers.len(), 1);
            s.dispatch(activate.clone()).unwrap();
            let other = if group == 5 {
                Panel::Brushes
            } else {
                Panel::Layers
            };
            s.dispatch(UiAction::SelectPanelTab {
                group,
                panel: other,
            })
            .unwrap();
            assert!(s.state.customization.drawer.is_none());
            // Old bounds from the hidden tab must not create a phantom origin.
            assert!(
                s.state
                    .customization
                    .drawer_tile_at([current.anchor.x + 4., 164.])
                    .is_none()
            );
            let projected = serde_json::to_value(&s.state.customization).unwrap();
            assert!(projected.get("drawer_tiles").is_none());
        }
    }

    #[test]
    fn docked_toolbar_handles_restore_minimum_lanes_on_every_platform() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for viewport in [[1600.0, 1200.0], [640.0, 480.0]] {
                for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
                    for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
                        let mut app = session();
                        app.set_platform(platform);
                        app.dispatch(UiAction::Customize {
                            action: CustomizationAction::SetTileStyle {
                                panel: Panel::Toolbar,
                                style,
                            },
                        })
                        .unwrap();
                        app.dispatch(UiAction::MovePanel {
                            panel: Panel::Toolbar,
                            viewport,
                            target: DockTarget::Edge { edge, outer: true },
                        })
                        .unwrap();
                        let group = app
                            .state
                            .workspace
                            .layout
                            .panel_group(Panel::Toolbar)
                            .unwrap();
                        let natural = app
                            .layout(viewport)
                            .groups
                            .into_iter()
                            .find(|g| g.id == group)
                            .unwrap();
                        app.state
                            .workspace
                            .layout
                            .bands
                            .iter_mut()
                            .find(|b| b.root.id() == group)
                            .unwrap()
                            .extent += 120.0;
                        let before = app.state.workspace.clone();
                        app.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                            .unwrap();
                        let fitted = app
                            .layout(viewport)
                            .groups
                            .into_iter()
                            .find(|g| g.id == group)
                            .unwrap();
                        assert_eq!(
                            fitted.bounds, natural.bounds,
                            "{platform:?} {edge:?} {style:?} {viewport:?}"
                        );
                        assert_eq!(
                            fitted.tiles.as_ref().unwrap().tiles,
                            natural.tiles.as_ref().unwrap().tiles
                        );
                        assert!(!fitted.tabs_visible && !fitted.floating);
                        let after = app.state.workspace.clone();
                        invoke(&mut app, CommandId::UndoWorkspace);
                        assert_eq!(app.state.workspace, before);
                        invoke(&mut app, CommandId::RedoWorkspace);
                        assert_eq!(app.state.workspace, after);
                        app.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                            .unwrap();
                        assert_eq!(
                            app.state.workspace, after,
                            "Repeated reset is idempotent, never a floating layout cycle"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn floating_drag_area_cycles_defaults_and_panel_headers_on_every_platform() {
        let viewport = [1600.0, 1200.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for panel in [Panel::Toolbar, Panel::Sizes] {
                let mut app = session();
                app.set_platform(platform);
                app.dispatch(UiAction::MovePanel {
                    panel,
                    viewport,
                    target: DockTarget::Float {
                        position: [500.0, 200.0],
                    },
                })
                .unwrap();
                let group = app.state.workspace.layout.panel_group(panel).unwrap();
                app.state
                    .workspace
                    .layout
                    .reset_floating_size(group)
                    .unwrap();
                let initial = app.state.workspace.clone();
                let count = if panel.kind() == PanelKind::Tiles {
                    3
                } else {
                    2
                };
                for step in 0..count {
                    let before = app.state.workspace.clone();
                    let change = app
                        .dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                        .unwrap();
                    assert!(!change.canvas_wake);
                    if panel.kind() == PanelKind::Tiles {
                        assert_eq!(
                            app.state.workspace.layout.floating[0].toolbar_layout,
                            [
                                FloatingToolbarLayout::Vertical,
                                FloatingToolbarLayout::Horizontal,
                                FloatingToolbarLayout::Compact
                            ][step]
                        );
                    } else {
                        assert_eq!(
                            app.state.workspace.layout.panel(panel).unwrap().hide_tab,
                            step == 0
                        );
                    }
                    let after = app.state.workspace.clone();
                    assert_ne!(before, after);
                    invoke(&mut app, CommandId::UndoWorkspace);
                    assert_eq!(app.state.workspace, before);
                    invoke(&mut app, CommandId::RedoWorkspace);
                    assert_eq!(app.state.workspace, after);
                    let json = serde_json::to_string(&after).unwrap();
                    assert_eq!(
                        serde_json::from_str::<WorkspaceState>(&json).unwrap(),
                        after
                    );
                }
                assert_eq!(app.state.workspace, initial);
                // Custom size gets one reset before cycling/toggling. Geometry,
                // not the presence of an explicit size, determines "default".
                let floating = &mut app.state.workspace.layout.floating[0];
                floating.width = 480.0;
                floating.height = Some(500.0);
                app.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                    .unwrap();
                assert_eq!(app.state.workspace, initial);
                let bounds = app
                    .layout(viewport)
                    .groups
                    .into_iter()
                    .find(|g| g.id == group)
                    .unwrap()
                    .bounds;
                let floating = &mut app.state.workspace.layout.floating[0];
                floating.width = bounds.width;
                floating.height = Some(bounds.height);
                app.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                    .unwrap();
                assert_ne!(app.state.workspace, initial);
                if panel.kind() == PanelKind::Content {
                    app.dispatch(UiAction::MovePanel {
                        panel: Panel::Brushes,
                        viewport,
                        target: DockTarget::Tab { group, index: None },
                    })
                    .unwrap();
                    let before = app.state.workspace.clone();
                    app.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                        .unwrap();
                    assert_eq!(
                        app.state.workspace, before,
                        "Multi-tab groups do not hide their header"
                    );
                }
            }
        }
    }

    #[test]
    fn zen_hidden_docks_only_allow_floating_tab_targets() {
        let viewport = [1200.0, 900.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut app = session();
            app.state.settings.total_zen = true;
            app.set_platform(platform);
            let panel = Panel::Toolbar;
            let item = DockItem::Panel { panel };
            app.dispatch(UiAction::MovePanel {
                panel,
                viewport,
                target: DockTarget::Float {
                    position: [600.0, 400.0],
                },
            })
            .unwrap();
            invoke(&mut app, CommandId::ZenMode);
            assert!(
                chrome(
                    &mut app,
                    ChromeEvent::Motion {
                        position: [600.0, 450.0]
                    },
                    ChromeFacts::default()
                )
                .chrome_hidden
            );
            let drag = |app: &mut UiSession<_>, phase, position| {
                app.dispatch(UiAction::DragWorkspace {
                    item,
                    phase,
                    position,
                    viewport,
                    tabs: vec![],
                })
                .unwrap();
                chrome(app, ChromeEvent::Refresh, ChromeFacts::default())
            };
            // Bottom has no dock/reveal zone; top snapping reaches below the
            // header, beyond the 80px reveal zone. Neither may dock invisibly.
            // The other points lie on the hidden sidebars, outside reveal zones.
            for point in [
                [600.0, 899.0],
                [600.0, crate::HEADER_HEIGHT + 50.0],
                [100.0, 450.0],
                [1100.0, 450.0],
            ] {
                assert!(drag(&mut app, ContactPhase::Down, [600.0, 450.0]).chrome_hidden);
                assert!(drag(&mut app, ContactPhase::Move, point).chrome_hidden);
                assert!(
                    app.drop_hint(viewport, point, &[], item, None).is_none(),
                    "{point:?}"
                );
                assert!(drag(&mut app, ContactPhase::Up, point).chrome_hidden);
                assert_eq!(app.state.workspace.layout.floating.len(), 1);
            }
            app.dispatch(UiAction::MovePanel {
                panel: Panel::Sizes,
                viewport,
                target: DockTarget::Float {
                    position: [950.0, 750.0],
                },
            })
            .unwrap();
            let target = app
                .state
                .workspace
                .layout
                .panel_group(Panel::Sizes)
                .unwrap();
            let floating = app
                .state
                .workspace
                .layout
                .floating
                .iter_mut()
                .find(|f| f.root.id() == target)
                .unwrap();
            floating.position = [850.0, 730.0];
            floating.width = 200.0;
            floating.height = Some(100.0);
            assert!(drag(&mut app, ContactPhase::Down, [600.0, 450.0]).chrome_hidden);
            // Including near the bottom edge: a closer hidden screen edge must
            // not steal the float's merge target. Floats never split side by side.
            for point in [
                [851.0, 780.0],
                [1049.0, 780.0],
                [950.0, 829.0],
                [950.0, 880.0],
            ] {
                assert!(drag(&mut app, ContactPhase::Move, point).chrome_hidden);
                assert_eq!(
                    app.drop_hint(viewport, point, &[], item, None)
                        .unwrap()
                        .target,
                    DockTarget::Tab {
                        group: target,
                        index: None
                    }
                );
            }
            assert!(drag(&mut app, ContactPhase::Up, [950.0, 880.0]).chrome_hidden);
            assert_eq!(app.state.workspace.layout.panel_group(panel), Some(target));
            assert_eq!(app.state.workspace.layout.floating.len(), 1);
            invoke(&mut app, CommandId::UndoWorkspace);
            assert_eq!(app.state.workspace.layout.floating.len(), 2);
            invoke(&mut app, CommandId::RedoWorkspace);
            assert_eq!(app.state.workspace.layout.panel_group(panel), Some(target));
        }
    }

    #[test]
    fn zen_floating_drag_reveals_at_edges_and_release_uses_normal_proximity() {
        let viewport = [1200.0, 900.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut app = session();
            app.state.settings.total_zen = true;
            app.set_platform(platform);
            app.dispatch(UiAction::MovePanel {
                panel: Panel::Sizes,
                viewport,
                target: DockTarget::Float {
                    position: [640.0, 250.0],
                },
            })
            .unwrap();
            invoke(&mut app, CommandId::ZenMode);
            let facts = ChromeFacts::default();
            let center = [650.0, 440.0];
            assert!(
                chrome(&mut app, ChromeEvent::Motion { position: center }, facts).chrome_hidden
            );
            let drag = |app: &mut UiSession<_>, phase, position| {
                app.dispatch(UiAction::DragWorkspace {
                    item: DockItem::Panel {
                        panel: Panel::Sizes,
                    },
                    phase,
                    position,
                    viewport,
                    tabs: vec![],
                })
                .unwrap();
                chrome(app, ChromeEvent::Refresh, facts)
            };
            let start = app
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.floating)
                .unwrap()
                .bounds;
            let grip = [start.x + 110.0, start.y + 12.0];
            assert!(drag(&mut app, ContactPhase::Down, grip).chrome_hidden);
            assert!(drag(&mut app, ContactPhase::Move, center).chrome_hidden);
            assert!(drag(&mut app, ContactPhase::Up, center).chrome_hidden);
            assert!(!app.interaction.keep_chrome_until_contact);
            // A hidden dock cannot intercept a floating panel near its old
            // boundary, before the artist has reached a window reveal edge.
            let hidden_panel = app
                .layout(viewport)
                .groups
                .into_iter()
                .find(|g| g.active == Panel::Brushes)
                .unwrap();
            assert!(
                app.drop_hint(
                    viewport,
                    [
                        hidden_panel.bounds.x + hidden_panel.bounds.width + 20.0,
                        450.0
                    ],
                    &[],
                    DockItem::Panel {
                        panel: Panel::Sizes
                    },
                    None
                )
                .is_none()
            );
            assert!(drag(&mut app, ContactPhase::Down, center).chrome_hidden);
            assert!(!drag(&mut app, ContactPhase::Move, [40.0, 450.0]).chrome_hidden);
            assert!(!drag(&mut app, ContactPhase::Move, center).chrome_hidden);
            assert!(drag(&mut app, ContactPhase::Up, center).chrome_hidden);
            assert!(!app.interaction.keep_chrome_until_contact);
            // Docked drops also return to normal cursor proximity.
            assert!(drag(&mut app, ContactPhase::Down, center).chrome_hidden);
            assert!(!drag(&mut app, ContactPhase::Move, [40.0, 450.0]).chrome_hidden);
            let target = [
                hidden_panel.bounds.x + hidden_panel.bounds.width * 0.5,
                450.0,
            ];
            assert!(!drag(&mut app, ContactPhase::Up, target).chrome_hidden);
            assert!(app.state.workspace.layout.floating.is_empty());
            assert!(!app.interaction.keep_chrome_until_contact);
            assert!(
                chrome(&mut app, ChromeEvent::Motion { position: center }, facts).chrome_hidden
            );
            assert!(
                chrome(
                    &mut app,
                    ChromeEvent::Contact {
                        position: center,
                        canvas: true
                    },
                    facts
                )
                .chrome_hidden
            );
        }
    }

    #[test]
    fn malformed_workspace_restore_is_atomic() {
        let mut app = session();
        let valid = serde_json::to_value(&app.state.workspace).unwrap();
        let mut invalid = Vec::new();
        let mut value = valid.clone();
        value["version"] = 2.into();
        invalid.push(value);
        let mut value = valid.clone();
        value["layout"]["next_id"] = 1.into();
        invalid.push(value);
        let mut value = valid.clone();
        value["layout"]["bands"][0]["extent"] = (-1).into();
        invalid.push(value);
        let mut value = valid.clone();
        value["layout"]["bands"][0]["root"]["fraction"] = 1.into();
        invalid.push(value);
        let mut value = valid.clone();
        value["layout"]["bands"][0]["root"]["first"]["active"] = "layers".into();
        invalid.push(value);
        let mut value = valid.clone();
        value["layout"]["bands"][0]["root"]["second"]["panels"] = serde_json::json!(["brushes"]);
        invalid.push(value);
        let mut value = valid.clone();
        value["layout"]["bands"][1]["id"] = 3.into();
        invalid.push(value);
        let mut value = valid.clone();
        // Missing placement is now valid (hidden); missing registry data is not.
        value["layout"]["panels"].as_array_mut().unwrap().remove(1);
        invalid.push(value);
        for value in invalid {
            let before = serde_json::to_value(&app.state).unwrap();
            assert!(
                app.dispatch(UiAction::RestoreWorkspace {
                    workspace: serde_json::from_value(value).unwrap()
                })
                .is_err()
            );
            assert_eq!(serde_json::to_value(&app.state).unwrap(), before);
        }
        let mut nan = app.state.workspace.clone();
        nan.layout.bands[0].extent = f32::NAN;
        assert!(
            app.dispatch(UiAction::RestoreWorkspace { workspace: nan })
                .is_err()
        );
        assert_eq!(serde_json::to_value(&app.state.workspace).unwrap(), valid);
    }

    #[test]
    fn effect_creation_properties_and_navigation_are_shared() {
        use layer_core::EffectValue;
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut app = session();
            app.set_platform(platform);
            let base = app.engine.document().active_layer;
            let send = |app: &mut UiSession<Recorder>, action| {
                app.dispatch(UiAction::Effect { action }).unwrap()
            };
            send(
                &mut app,
                EffectAction::Insert {
                    effect: "curves".into(),
                },
            );
            let id = app.engine.document().active_layer;
            assert_eq!(app.engine.document().layers[0].id, id);
            assert_eq!(app.engine.document().layers[1].id, base);
            assert_eq!(
                app.state.workspace.layout.active_panel(Panel::Properties),
                Some(Panel::Properties)
            );
            send(
                &mut app,
                EffectAction::CurvePoint {
                    layer: id.0,
                    key: "curve_0".into(),
                    index: None,
                    point: [0.4, 0.7],
                    remove: false,
                },
            );
            assert_eq!(
                app.state.layer_properties.controls[0].value,
                EffectValue::Curve(vec![[0., 0.], [0.4, 0.7], [1., 1.]])
            );
            send(
                &mut app,
                EffectAction::Reset {
                    layer: id.0,
                    key: "curve_0".into(),
                },
            );
            assert_eq!(
                app.state.layer_properties.controls[0].value,
                app.state.layer_properties.controls[0].default
            );
            app.layer_action(LayerAction::Clip {
                id: id.0,
                value: true,
            })
            .unwrap();
            assert_eq!(app.engine.document().clipping_base(id), Some(base));
            app.state
                .workspace
                .layout
                .move_panel(
                    [1200., 900.],
                    Panel::Properties,
                    DockTarget::Float {
                        position: [450., 150.],
                    },
                )
                .unwrap();
            let previous = app.state.workspace.layout.panel_group(Panel::Properties);
            send(
                &mut app,
                EffectAction::Insert {
                    effect: "levels".into(),
                },
            );
            assert_eq!(
                app.state.workspace.layout.panel_group(Panel::Properties),
                previous,
                "visible Properties stays put"
            );
            app.state
                .workspace
                .layout
                .set_panel_visible(Panel::Properties, false)
                .unwrap();
            send(
                &mut app,
                EffectAction::Insert {
                    effect: "brightness_contrast".into(),
                },
            );
            let panels = app.state.workspace.layout.group_panels(8).unwrap();
            let a = panels
                .iter()
                .position(|p| *p == Panel::Adjustments)
                .unwrap();
            assert_eq!(panels[a + 1], Panel::Properties);
            send(
                &mut app,
                EffectAction::Insert {
                    effect: "color_balance".into(),
                },
            );
            let controls = &app.state.layer_properties.controls;
            assert_eq!(controls[0].section.as_deref(), Some("Shadows"));
            assert_eq!(controls[3].section.as_deref(), Some("Midtones"));
            assert_eq!(controls[6].section.as_deref(), Some("Highlights"));
            assert!(controls[9].section.is_none());
            assert_eq!(controls[0].label, "Cyan — Red");
            send(
                &mut app,
                EffectAction::Insert {
                    effect: "gradient_map".into(),
                },
            );
            let layer = app.state.layer_properties.layer.unwrap();
            send(
                &mut app,
                EffectAction::GradientStop {
                    layer,
                    key: "gradient".into(),
                    index: None,
                    position: 0.5,
                    color: Some([0.7, 0.2, 0.1, 0.5]),
                    remove: false,
                },
            );
            let edited = app.state.layer_properties.controls[0].value.clone();
            assert!(
                matches!(&edited, EffectValue::Gradient(stops) if stops.len()==3 && stops[1].color[3]==0.5)
            );
            app.dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            })
            .unwrap();
            assert_eq!(
                app.state.layer_properties.controls[0].value,
                app.state.layer_properties.controls[0].default
            );
            app.dispatch(UiAction::Invoke {
                command: CommandId::Redo,
            })
            .unwrap();
            assert_eq!(app.state.layer_properties.controls[0].value, edited);
        }
    }
    #[test]
    fn shared_drop_preview_validates_the_exact_move() {
        let mut app = session();
        let viewport = [1200.0, 900.0];
        let item = DockItem::Panel {
            panel: Panel::Toolbar,
        };
        let target = app
            .layout(viewport)
            .groups
            .into_iter()
            .find(|g| g.active == Panel::Layers)
            .unwrap()
            .bounds;
        let hint = app
            .drop_hint(
                viewport,
                [
                    target.x + target.width * 0.5,
                    target.y + target.height * 0.5,
                ],
                &[],
                item,
                None,
            )
            .unwrap();
        assert_eq!(
            hint.target,
            DockTarget::Tab {
                group: 8,
                index: None
            }
        );
        app.dispatch(item.move_action(hint.target, viewport))
            .unwrap();
        assert!(app.state.workspace.layout.validate().is_ok());
        assert!(
            app.drop_hint(viewport, [f32::NAN, 10.0], &[], item, None)
                .is_none()
        );
        assert!(
            app.drop_hint(viewport, [-1.0, 10.0], &[], item, None)
                .is_none()
        );
        assert!(
            app.drop_hint(
                viewport,
                [1000.0, 400.0],
                &[],
                DockItem::Group { group: u32::MAX },
                None
            )
            .is_none()
        );
    }
    #[test]
    fn wheel_pans_shift_pans_horizontally_and_control_zooms_without_rotation() {
        let mut s = session();
        let before = s.state.camera.clone();
        s.scroll([500.0, 500.0], [0.0, 40.0], 2.0, false, false)
            .unwrap();
        assert_eq!(
            s.state.camera.translation,
            [before.translation[0], before.translation[1] - 80.0]
        );
        s.scroll([500.0, 500.0], [0.0, 40.0], 2.0, false, true)
            .unwrap();
        assert_eq!(
            s.state.camera.translation,
            [before.translation[0] - 80.0, before.translation[1] - 80.0]
        );
        assert_eq!(s.state.camera.zoom, before.zoom);
        s.scroll([500.0, 500.0], [0.0, -40.0], 2.0, true, false)
            .unwrap();
        assert!(s.state.camera.zoom > before.zoom);
        assert_eq!(s.state.camera.rotation, before.rotation);
    }
    #[test]
    fn zen_does_not_move_camera_layout_or_request_canvas_work() {
        let mut s = session();
        s.set_viewport([1000.0; 2], [1000; 2]).unwrap();
        let camera = s.state.camera.clone();
        let layout = s.state.workspace.layout.clone();
        for zen_mode in [true, false] {
            let change = invoke(&mut s, CommandId::ZenMode);
            assert!(!change.canvas_wake);
            assert_eq!(s.state.camera, camera);
            assert_eq!(s.state.workspace.layout, layout);
            assert_eq!(s.state.workspace.zen_mode, zen_mode);
            assert_eq!(s.command(CommandId::ZenMode).selected, zen_mode);
        }
        invoke(&mut s, CommandId::FitCanvas);
        assert_eq!(s.state.camera.viewport, [1000, 1000]);
        assert_eq!(s.state.camera.translation, camera.translation);
        assert_eq!(s.state.camera.zoom, camera.zoom);
        let mut full = session();
        full.set_viewport([2000.0; 2], [2000; 2]).unwrap();
        invoke(&mut full, CommandId::FitCanvas);
        assert!(full.state.camera.zoom > camera.zoom);
    }
    fn invoke(session: &mut UiSession<Recorder>, command: CommandId) -> UiChange {
        session.dispatch(UiAction::Invoke { command }).unwrap()
    }
    fn event(
        session: &UiSession<Recorder>,
        sequence: u64,
        phase: PenPhase,
        pressure: f32,
    ) -> PenEvent {
        PenEvent {
            device_id: 1,
            sequence,
            timestamp_ns: sequence * 10_000_000,
            view_revision: session.state.camera.revision,
            surface_position: Point {
                x: 200.0 + sequence as f32 * 25.0,
                y: 300.0,
            },
            pressure,
            tilt_radians: [0.0; 2],
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        }
    }
    #[test]
    fn initial_state_and_every_preset_are_valid() {
        let mut app = session();
        assert_eq!(app.state.tabs.len(), 1);
        assert!(!app.command(CommandId::Undo).enabled);
        assert!(!app.command(CommandId::DeleteLayer).enabled);
        for choice in brush_catalog() {
            let change = app
                .dispatch(UiAction::SelectBrush { id: choice.id })
                .unwrap();
            assert_ne!(change.regions & regions::BRUSH, 0);
            assert_eq!(app.state.brush.preset, choice.id);
        }
        let previous = app.state.brush.clone();
        for action in [
            UiAction::SelectBrush { id: 999 },
            UiAction::SetBrushSize { value: f32::NAN },
            UiAction::SetColor { rgba: [1.1; 4] },
            UiAction::SetBrushOpacity { value: -1.0 },
        ] {
            assert!(app.dispatch(action).is_err());
            assert_eq!(app.state.brush, previous);
        }
    }

    #[test]
    fn cursor_is_display_only_and_obeys_zoom_dpi_settings_and_pan() {
        let mut s = session();
        s.set_viewport([500.0, 500.0], [1000, 1000]).unwrap();
        let before = s.state.revision;
        let document = s.engine.document().clone();
        s.cursor_input(Some(event(&s, 1, PenPhase::Hover, 0.0)));
        let initial = s.canvas_cursor().unwrap();
        assert_eq!(initial.center, [112.5, 150.0]);
        assert!(!initial.outline.is_empty());
        assert_eq!(initial.outline, s.canvas_cursor().unwrap().outline);
        let mut native = CanvasCursor::default();
        assert!(s.update_canvas_cursor(&mut native, false));
        assert_eq!(native.segments, initial.segments);
        assert!(native.outline.is_empty() && native.marker.is_empty());
        let capacity = native.segments.capacity();
        let pointer = native.segments.as_ptr();
        for _ in 0..100 {
            assert!(s.update_canvas_cursor(&mut native, false));
            assert_eq!(native.segments, initial.segments);
            assert_eq!(native.segments.capacity(), capacity);
            assert_eq!(native.segments.as_ptr(), pointer);
        }
        assert_eq!(s.state.revision, before);
        assert_eq!(s.engine.document(), &document);
        assert_eq!(s.engine.backend().dabs, 0);
        s.set_viewport([500.0, 500.0], [500, 500]).unwrap();
        assert_eq!(
            s.canvas_cursor().unwrap().center,
            initial.center,
            "DPI changes preserve the logical pointer location"
        );
        key(&mut s, " ", true, false, false);
        assert!(s.canvas_cursor().is_none());
        assert!(!s.update_canvas_cursor(&mut native, false));
        assert!(native.segments.is_empty());
        key(&mut s, " ", false, false, false);
        assert!(s.canvas_cursor().is_some());
        invoke(&mut s, CommandId::Settings);
        assert!(s.canvas_cursor().is_none());
        let settings = Settings {
            cursor: CursorMode::Cross,
            ..Settings::default()
        };
        s.dispatch(UiAction::EditSettings { settings }).unwrap();
        s.dispatch(UiAction::CloseSettings).unwrap();
        let cross = s.canvas_cursor().unwrap();
        assert!(cross.outline.is_empty());
        assert!(!cross.marker.is_empty());
        s.cursor_input(None);
        assert!(s.canvas_cursor().is_none());
        let old: Settings = serde_json::from_str(r#"{"theme":null,"pressure_gamma":1.0}"#).unwrap();
        assert_eq!(old.cursor, CursorMode::BrushSize);
    }

    #[test]
    fn mask_cursor_composes_tip_flip_rotation_aspect_camera_and_dpi() {
        let mut s = session();
        s.set_viewport([500.0, 500.0], [1000, 1000]).unwrap();
        s.state.camera.zoom = 2.0;
        s.state.camera.rotation = std::f32::consts::FRAC_PI_2;
        s.state.camera.revision += 1;
        s.sync_camera();
        let mut brush = layer_core::BrushSnapshot {
            diameter: 40.0,
            aspect: 2.0,
            angle_radians: std::f32::consts::FRAC_PI_2,
            tip: layer_core::BrushTip::Mask(AssetId::from("cursor-test")),
            mappings: std::sync::Arc::from([]),
            ..Default::default()
        };
        brush.shape.flip_x_probability = 1.0;
        s.engine.set_brush(brush).unwrap();
        s.cursor_input(Some(event(&s, 1, PenPhase::Hover, 0.0)));
        let svg = s.canvas_cursor().unwrap();
        assert_eq!(
            svg.outline,
            "M92.50 155.00L122.50 155.00L92.50 145.00L92.50 155.00Z"
        );
        let mut native = CanvasCursor::default();
        assert!(s.update_canvas_cursor(&mut native, false));
        assert_eq!(native.segments, svg.segments);
        assert!(native.outline.is_empty());
    }

    #[test]
    fn contact_cursor_uses_live_pressure_without_depositing_another_dab() {
        let mut s = session();
        s.dispatch(UiAction::SetBrushSize { value: 200.0 }).unwrap();
        let low = event(&s, 1, PenPhase::Down, 0.2);
        s.pen(low).unwrap();
        s.frame(low.timestamp_ns, low.timestamp_ns).unwrap();
        let dabs = s.engine.backend().dabs;
        s.cursor_input(Some(low));
        let radius = |c: CanvasCursor| {
            c.outline
                .split('A')
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse::<f32>()
                .unwrap()
        };
        let low_radius = radius(s.canvas_cursor().unwrap());
        s.cursor_input(Some(PenEvent {
            pressure: 0.8,
            ..low
        }));
        let high_radius = radius(s.canvas_cursor().unwrap());
        assert!(high_radius > low_radius * 1.9);
        assert_eq!(s.engine.backend().dabs, dabs);
    }
    #[test]
    fn menu_sections_are_nonempty_unique_and_keep_toggles_together() {
        assert_eq!(
            MENUS
                .iter()
                .filter(|m| m.sections.is_empty())
                .map(|m| m.label)
                .collect::<Vec<_>>(),
            [WORKSPACE_MENU_LABEL]
        );
        for sections in MENUS
            .iter()
            .filter(|m| !m.sections.is_empty())
            .map(|m| m.sections)
            .chain([PRIMARY_MENU])
        {
            assert!(!sections.is_empty());
            let mut seen = Vec::new();
            for &section in sections {
                assert!(!section.is_empty());
                for &command in section {
                    assert_eq!(command.is_toggle(), section[0].is_toggle());
                    assert!(!seen.contains(&command));
                    seen.push(command);
                }
            }
        }
        assert_eq!(PRIMARY_MENU[0], &[CommandId::NewWindow]);
        assert_eq!(
            PRIMARY_MENU[1],
            &[
                CommandId::Settings,
                CommandId::KeyboardShortcuts,
                CommandId::About
            ]
        );
        assert_eq!(
            serde_json::to_value(MENUS).unwrap()[1]["sections"],
            serde_json::json!([
                ["zoom_in", "zoom_out", "fit_canvas"],
                ["rotate_left", "rotate_right"],
                ["flip_horizontal", "flip_vertical"],
                ["show_rulers", "snap_rulers"],
                ["zen_mode", "fullscreen"],
                ["reset_layout"]
            ])
        );
    }
    #[test]
    fn fullscreen_follows_host_notifications_and_leaves_browser_f11_unbound() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let workspace = s.state.workspace.clone();
        assert_eq!(s.command(CommandId::Fullscreen).shortcut, "F11");
        s.dispatch(UiAction::Invoke {
            command: CommandId::Fullscreen,
        })
        .unwrap();
        assert!(matches!(
            s.state.requests.last().unwrap().kind,
            HostRequestKind::SetFullscreen { fullscreen: true }
        ));
        assert!(
            !s.command(CommandId::Fullscreen).selected,
            "Wait for the compositor acknowledgement"
        );
        s.dispatch(UiAction::WindowFullscreen { fullscreen: true })
            .unwrap();
        assert!(s.command(CommandId::Fullscreen).selected);
        assert_eq!(
            s.command(CommandId::Fullscreen).icon,
            Some("fullscreen-exit")
        );
        assert_eq!(
            s.state.workspace, workspace,
            "Fullscreen is transient host state"
        );
        s.set_platform(Platform::Mac);
        assert!(s.command(CommandId::Fullscreen).enabled);
        s.dispatch(UiAction::Invoke {
            command: CommandId::Fullscreen,
        })
        .unwrap();
        assert!(matches!(
            s.state.requests.last().unwrap().kind,
            HostRequestKind::SetFullscreen { fullscreen: false }
        ));
        assert!(s.command(CommandId::Fullscreen).selected);
        s.set_platform(Platform::Ios);
        assert!(!s.command(CommandId::Fullscreen).enabled);
        s.set_platform(Platform::Web);
        assert_eq!(s.command(CommandId::Fullscreen).shortcut, "");
        assert!(
            s.state
                .settings
                .shortcut_match(&KeyChord::new("F11", Default::default()), Platform::Web)
                .is_none()
        );
        s.dispatch(UiAction::WindowFullscreen { fullscreen: false })
            .unwrap();
        assert!(!s.command(CommandId::Fullscreen).selected);
    }

    #[test]
    fn catalog_covers_each_brush_panel_and_declared_command() {
        let catalog = ui_catalog();
        let mut ids = catalog
            .brush_categories
            .iter()
            .flat_map(|c| c.brushes.iter().map(|b| b.id))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        let mut expected = brush_catalog().map(|b| b.id).collect::<Vec<_>>();
        expected.sort_unstable();
        assert_eq!(ids, expected);
        assert_eq!(
            catalog.panels.iter().map(|p| p.id).collect::<Vec<_>>(),
            Panel::ALL
        );
        let session = session();
        for id in catalog
            .menus
            .iter()
            .flat_map(|m| m.sections)
            .chain(PRIMARY_MENU)
            .flat_map(|s| s.iter())
            .chain(catalog.layer_commands)
            .chain(catalog.tool_commands)
        {
            assert!(session.state.commands.iter().any(|c| c.id == *id));
        }
        for control in catalog.toolbar {
            if let ToolbarControl::Command { command } = control {
                assert!(session.state.commands.iter().any(|c| c.id == *command));
            }
        }
        assert_eq!(
            serde_json::to_value(&catalog).unwrap()["pressure"]["step"],
            0.05
        );
        for control in [catalog.brush_size, catalog.opacity, catalog.pressure] {
            assert!(control.validate(control.min as f32, "test").is_ok());
            assert!(control.validate(control.max as f32, "test").is_ok());
            assert!(control.validate(f32::NAN, "test").is_err());
            assert!(control.validate(control.max as f32 + 1.0, "test").is_err());
        }
    }
    #[test]
    fn opacity_without_id_uses_the_live_selected_layer() {
        let mut app = session();
        invoke(&mut app, CommandId::AddLayer);
        let second = app.engine.document().active_layer.0;
        let action: UiAction =
            serde_json::from_str(r#"{"type":"set_layer_opacity","opacity":0.25}"#).unwrap();
        app.dispatch(action).unwrap();
        assert_eq!(
            app.engine
                .document()
                .layer(LayerId(second))
                .unwrap()
                .opacity,
            0.25
        );
        app.dispatch(UiAction::SelectLayer { id: 1 }).unwrap();
        app.dispatch(UiAction::SetLayerOpacity {
            id: None,
            opacity: 0.75,
        })
        .unwrap();
        assert_eq!(
            app.engine.document().layer(LayerId(1)).unwrap().opacity,
            0.75
        );
        assert_eq!(
            app.engine
                .document()
                .layer(LayerId(second))
                .unwrap()
                .opacity,
            0.25
        );
    }
    #[test]
    fn panel_configuration_keeps_compact_preview_live_without_changing_docking() {
        let mut app = session();
        let before = app.state.workspace.clone();
        assert_eq!(
            app.panel_view(Panel::Brushes)
                .unwrap()
                .controls
                .iter()
                .filter(|c| c.visible_in_panel)
                .count(),
            1
        );
        app.dispatch(UiAction::Customize {
            action: CustomizationAction::ShowAllControls {
                panel: Panel::Brushes,
            },
        })
        .unwrap();
        assert_eq!(app.state.workspace, before);
        assert_eq!(
            app.panel_view(Panel::Brushes)
                .unwrap()
                .controls
                .iter()
                .filter(|c| c.visible_in_panel)
                .count(),
            1
        );
        app.dispatch(UiAction::SetBrushOpacity { value: 0.4 })
            .unwrap();
        assert_eq!(app.state.brush.opacity, 0.4);
        app.dispatch(UiAction::Customize {
            action: CustomizationAction::SetControlVisible {
                panel: Panel::Brushes,
                control: PanelControl::BrushOpacity,
                visible: true,
            },
        })
        .unwrap();
        app.dispatch(UiAction::Customize {
            action: CustomizationAction::CloseExpanded,
        })
        .unwrap();
        assert_eq!(
            app.panel_view(Panel::Brushes)
                .unwrap()
                .controls
                .iter()
                .filter(|c| c.visible_in_panel)
                .map(|c| c.control)
                .collect::<Vec<_>>(),
            [PanelControl::Brushes, PanelControl::BrushOpacity]
        );
        assert_eq!(app.state.workspace.layout.bands, before.layout.bands);
        // Restoring a workspace closes transient inspectors/pickers atomically.
        app.dispatch(UiAction::Customize {
            action: CustomizationAction::NewToolbar { group: Some(8) },
        })
        .unwrap();
        app.dispatch(UiAction::RestoreWorkspace { workspace: before })
            .unwrap();
        assert!(!app.state.customization.is_open());
    }

    #[test]
    fn expanded_panel_dismissal_is_core_policy_and_never_paints() {
        let mut app = session();
        app.state.settings.total_zen = true;
        invoke(&mut app, CommandId::ZenMode);
        let open = |app: &mut UiSession<Recorder>| {
            app.dispatch(UiAction::Customize {
                action: CustomizationAction::ShowAllControls {
                    panel: Panel::Sizes,
                },
            })
            .unwrap();
        };
        open(&mut app);
        let facts = ChromeFacts {
            expanded_panel: Some(PanelExpansion {
                group: 6,
                concave_join: false,
                bounds: Bounds {
                    x: 10.0,
                    y: 100.0,
                    width: 600.0,
                    height: 400.0,
                },
                preview: Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 220.0,
                    height: 400.0,
                },
                configuration: Bounds {
                    x: 220.0,
                    y: TAB_BAR_HEIGHT,
                    width: 380.0,
                    height: 400.0 - TAB_BAR_HEIGHT,
                },
            }),
            ..ChromeFacts::default()
        };
        let inside = chrome(
            &mut app,
            ChromeEvent::Contact {
                position: [400.0, 200.0],
                canvas: false,
            },
            facts,
        );
        assert!(!inside.handled);
        assert_eq!(app.state.customization.expanded, Some(Panel::Sizes));
        let outside = chrome(
            &mut app,
            ChromeEvent::Contact {
                position: [800.0, 700.0],
                canvas: true,
            },
            facts,
        );
        assert!(outside.handled && !outside.paint);
        assert!(
            !outside.chrome_hidden,
            "closing a drawer must not also hide the workspace"
        );
        assert_ne!(outside.change.regions & regions::CUSTOMIZATION, 0);
        assert!(app.state.customization.expanded.is_none());
        assert!(
            !chrome(
                &mut app,
                ChromeEvent::Motion {
                    position: [600.0, 600.0]
                },
                ChromeFacts::default()
            )
            .chrome_hidden
        );
        assert!(
            !chrome(
                &mut app,
                ChromeEvent::Leave { touch: false },
                ChromeFacts::default()
            )
            .chrome_hidden
        );
        let second = chrome(
            &mut app,
            ChromeEvent::Contact {
                position: [600.0, 600.0],
                canvas: true,
            },
            ChromeFacts::default(),
        );
        assert!(second.handled && second.chrome_hidden && !second.paint);
        open(&mut app);
        let header = chrome(
            &mut app,
            ChromeEvent::Contact {
                position: [80.0, 115.0],
                canvas: false,
            },
            facts,
        );
        assert!(header.handled);
        assert!(app.state.customization.expanded.is_none());
        open(&mut app);
        let reply = app
            .input(UiInput::Key {
                key: "escape".into(),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
                editing: true,
                divider: None,
            })
            .unwrap();
        assert!(reply.handled);
        assert!(app.state.customization.expanded.is_none());
    }
    #[test]
    fn selected_tab_toggles_drawer_and_other_tabs_preserve_its_open_state() {
        let mut app = session();
        app.dispatch(UiAction::MovePanel {
            panel: Panel::Sizes,
            target: DockTarget::Tab {
                group: 8,
                index: None,
            },
            viewport: [1200.0, 900.0],
        })
        .unwrap();
        // Moving Sizes into the group selected it. One activation opens it.
        let activate = |app: &mut UiSession<Recorder>, panel| {
            app.dispatch(UiAction::SelectPanelTab { group: 8, panel })
                .unwrap();
        };
        app.dispatch(UiAction::SelectPanelTab {
            group: 8,
            panel: Panel::Sizes,
        })
        .unwrap();
        assert_eq!(app.state.customization.expanded, Some(Panel::Sizes));
        let expanded = app
            .state
            .workspace
            .layout
            .expanded_panel([1200.0, 900.0], Panel::Sizes, [400.0; 2], 1.0)
            .unwrap();
        let event = ChromeEvent::Contact {
            position: [
                expanded.bounds.x + expanded.preview.x + 10.0,
                expanded.bounds.y + 10.0,
            ],
            canvas: false,
        };
        let facts = ChromeFacts {
            expanded_panel: Some(expanded),
            contact_tab: Some(Panel::Layers),
            ..ChromeFacts::default()
        };
        assert!(!chrome(&mut app, event, facts).handled);
        assert_eq!(app.state.customization.expanded, Some(Panel::Sizes));
        activate(&mut app, Panel::Layers);
        assert_eq!(app.state.customization.expanded, Some(Panel::Layers));
        // Press alone never toggles: it must remain available for a drag/hold.
        assert!(!chrome(&mut app, event, facts).handled);
        assert_eq!(app.state.customization.expanded, Some(Panel::Layers));
        activate(&mut app, Panel::Layers);
        assert!(app.state.customization.expanded.is_none());
        activate(&mut app, Panel::Sizes);
        assert!(app.state.customization.expanded.is_none());
        activate(&mut app, Panel::Sizes);
        assert_eq!(app.state.customization.expanded, Some(Panel::Sizes));
        assert!(
            chrome(
                &mut app,
                event,
                ChromeFacts {
                    contact_tab: None,
                    ..facts
                }
            )
            .handled
        );
        assert!(app.state.customization.expanded.is_none());
        let saved = app.state.workspace.clone();
        assert!(
            app.dispatch(UiAction::SelectPanelTab {
                group: 8,
                panel: Panel::Brushes,
            })
            .is_err()
        );
        assert_eq!(app.state.workspace, saved);
        assert!(app.state.customization.expanded.is_none());
    }

    #[test]
    fn expanded_configuration_tracks_the_selected_tab_after_docking() {
        let mut app = session();
        app.dispatch(UiAction::SelectPanelTab {
            group: 6,
            panel: Panel::Sizes,
        })
        .unwrap();
        assert_eq!(app.state.customization.expanded, Some(Panel::Sizes));
        app.dispatch(UiAction::MovePanel {
            panel: Panel::Layers,
            target: DockTarget::Tab {
                group: 6,
                index: None,
            },
            viewport: [1200.0, 900.0],
        })
        .unwrap();
        assert_eq!(app.state.customization.expanded, Some(Panel::Layers));
        app.dispatch(UiAction::Customize {
            action: CustomizationAction::SetPanelVisible {
                panel: Panel::Layers,
                visible: false,
            },
        })
        .unwrap();
        assert!(app.state.customization.expanded.is_none());
    }
    #[test]
    fn gtk_android_and_web_switch_eligible_toolbar_drawers_on_the_first_click() {
        for platform in [Platform::Gtk, Platform::Android, Platform::Web] {
            for zen in [false, true] {
                let mut s = session();
                s.set_platform(platform);
                s.state.workspace.zen_mode = zen;
                s.state.settings.total_zen = false;
                let viewport = [1200., 900.];
                let tiles = s
                    .state
                    .workspace
                    .layout
                    .panel(Panel::Toolbar)
                    .unwrap()
                    .tiles()
                    .to_vec();
                let brush = tiles
                    .iter()
                    .find(|t| {
                        t.control
                            == ToolbarControl::Command {
                                command: CommandId::Brush,
                            }
                    })
                    .unwrap()
                    .id;
                let eraser = tiles
                    .iter()
                    .find(|t| {
                        t.control
                            == ToolbarControl::Command {
                                command: CommandId::Eraser,
                            }
                    })
                    .unwrap()
                    .id;
                let color = tiles
                    .iter()
                    .find(|t| t.control == ToolbarControl::Color)
                    .unwrap()
                    .id;
                let activate = |s: &mut UiSession<Recorder>, tile| {
                    s.dispatch(UiAction::ActivateTile {
                        panel: Panel::Toolbar,
                        tile,
                    })
                    .unwrap()
                };
                activate(&mut s, brush);
                assert!(
                    s.state.customization.drawer.is_none(),
                    "A closed drawer still requires selecting first"
                );
                activate(&mut s, brush);
                for tile in [eraser, color, brush] {
                    let previous = s.state.customization.drawer.as_ref().unwrap().clone();
                    let old = previous
                        .placement(
                            &s.state.workspace.layout,
                            viewport,
                            &vec![400.; previous.columns.len()],
                            zen,
                        )
                        .unwrap();
                    let next = ContentDrawer::for_tile(
                        &s.state.workspace.layout,
                        TileAnchor {
                            panel: Panel::Toolbar,
                            tile,
                        },
                    )
                    .unwrap();
                    let target = next
                        .placement(
                            &s.state.workspace.layout,
                            viewport,
                            &vec![400.; next.columns.len()],
                            zen,
                        )
                        .unwrap();
                    let reply = chrome(
                        &mut s,
                        ChromeEvent::Contact {
                            position: [target.anchor.x + 4., target.anchor.y + 4.],
                            canvas: false,
                        },
                        ChromeFacts {
                            content_drawer: Some(old.bounds),
                            drawer_connection: old.connection().map(|c| c.bounds),
                            ..Default::default()
                        },
                    );
                    assert!(!reply.handled);
                    assert_eq!(
                        s.state.customization.drawer.as_ref().unwrap(),
                        &previous,
                        "Press retains the drawer until activation"
                    );
                    let change = activate(&mut s, tile);
                    assert_eq!(s.state.customization.drawer.as_ref().unwrap(), &next);
                    assert_ne!(change.regions & regions::CUSTOMIZATION, 0);
                    if tile != color {
                        assert_eq!(
                            s.state.brush.tool,
                            if tile == brush {
                                Tool::Brush
                            } else {
                                Tool::Eraser
                            }
                        );
                        assert_ne!(
                            change.regions & regions::BRUSH,
                            0,
                            "The same reply publishes the selected tool"
                        );
                        assert!(
                            s.panel_view(Panel::Toolbar)
                                .unwrap()
                                .tiles
                                .iter()
                                .find(|t| t.id == tile)
                                .unwrap()
                                .choice
                                .selected
                        );
                    }
                    assert!(
                        !s.switches_toolbar_drawer(TileAnchor {
                            panel: Panel::Commands,
                            tile
                        }),
                        "A different toolbar cannot switch this drawer"
                    );
                }
                activate(&mut s, brush);
                assert!(
                    s.state.customization.drawer.is_none(),
                    "Clicking the current opener closes it"
                );
                activate(&mut s, color);
                invoke(&mut s, CommandId::Eraser);
                assert!(
                    s.state.customization.drawer.is_none(),
                    "Tool shortcuts still dismiss the drawer"
                );
            }
        }
    }

    #[test]
    fn tool_drawer_selection_dismissal_and_configuration_are_core_policy() {
        let mut s = session();
        s.state.settings.total_zen = true;
        s.set_platform(Platform::Gtk);
        let viewport = [1200.0, 900.0];
        chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default());
        let tiles = s
            .state
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .to_vec();
        let activate = |s: &mut UiSession<Recorder>, index: usize| {
            s.dispatch(UiAction::ActivateTile {
                panel: Panel::Toolbar,
                tile: tiles[index].id,
            })
            .unwrap();
        };
        // Brush is not the initial Pen tool. First selects, second opens.
        activate(&mut s, 0);
        assert!(s.state.customization.drawer.is_none());
        assert_eq!(s.state.brush.tool, Tool::Brush);
        activate(&mut s, 0);
        assert_eq!(
            s.state.customization.drawer.as_ref().unwrap().columns,
            [vec![Panel::Brushes], vec![Panel::ToolSettings]]
        );
        s.dispatch(UiAction::SetBrushSize { value: 123.0 }).unwrap();
        assert!(s.state.customization.drawer.is_some());
        let placement = s
            .state
            .customization
            .drawer
            .as_ref()
            .unwrap()
            .placement(&s.state.workspace.layout, viewport, &[400.0, 600.0], false)
            .unwrap();
        let facts = ChromeFacts {
            content_drawer: Some(placement.bounds),
            ..Default::default()
        };
        let point = |b: Bounds| [b.x + 10.0, b.y + 10.0];
        assert!(
            !chrome(
                &mut s,
                ChromeEvent::Contact {
                    position: point(placement.anchor),
                    canvas: false
                },
                facts
            )
            .handled
        );
        activate(&mut s, 0);
        assert!(s.state.customization.drawer.is_none());
        activate(&mut s, 0);
        // Another eligible tool in this toolbar selects and switches its drawer.
        let other = ContentDrawer::for_tile(
            &s.state.workspace.layout,
            TileAnchor {
                panel: Panel::Toolbar,
                tile: tiles[1].id,
            },
        )
        .unwrap()
        .placement(&s.state.workspace.layout, viewport, &[0.0, 0.0], false)
        .unwrap();
        assert!(
            !chrome(
                &mut s,
                ChromeEvent::Contact {
                    position: point(other.anchor),
                    canvas: false
                },
                facts
            )
            .handled
        );
        activate(&mut s, 1);
        assert_eq!(
            s.state.customization.drawer.as_ref().unwrap().anchor.tile(),
            Some(TileAnchor {
                panel: Panel::Toolbar,
                tile: tiles[1].id
            })
        );
        assert_eq!(s.state.brush.tool, Tool::Eraser);
        let tool = s.state.brush.tool;
        activate(&mut s, 6); // Color is a direct-open, non-selectable tile.
        assert_eq!(s.state.brush.tool, tool);
        assert_eq!(
            s.state.customization.drawer.as_ref().unwrap().columns,
            [vec![Panel::Color]]
        );
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::ShowAllControls {
                panel: Panel::Sizes,
            },
        })
        .unwrap();
        assert!(s.state.customization.drawer.is_none());
        activate(&mut s, 6);
        assert!(s.state.customization.expanded.is_none());
        let reply = chrome(
            &mut s,
            ChromeEvent::Contact {
                position: [900.0, 800.0],
                canvas: true,
            },
            ChromeFacts::default(),
        );
        assert!(reply.handled && !reply.paint);
        assert!(s.state.customization.drawer.is_none());
        // Explicit policy is for persistent column drawers: outside does not
        // dismiss, without a per-platform special case.
        activate(&mut s, 6);
        s.state.customization.drawer.as_mut().unwrap().dismissal = DrawerDismissal::Explicit;
        chrome(
            &mut s,
            ChromeEvent::Contact {
                position: [900.0, 800.0],
                canvas: true,
            },
            ChromeFacts::default(),
        );
        assert!(s.state.customization.drawer.is_some());
        activate(&mut s, 6);
        invoke(&mut s, CommandId::ZenMode);
        activate(&mut s, 6);
        chrome(
            &mut s,
            ChromeEvent::Contact {
                position: [900.0, 800.0],
                canvas: true,
            },
            ChromeFacts::default(),
        );
        assert!(!chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default()).chrome_hidden);
        // Removing the originating tile also closes the transient drawer.
        activate(&mut s, 6);
        s.dispatch(UiAction::Customize {
            action: CustomizationAction::RemoveTool {
                panel: Panel::Toolbar,
                tile: tiles[6].id,
            },
        })
        .unwrap();
        assert!(s.state.customization.drawer.is_none());
    }

    #[test]
    fn tool_drawer_shortcuts_select_tools_and_zen_entry_stays_hidden() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default());
        let tile = s
            .state
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()[6]
            .id;
        let open = UiAction::ActivateTile {
            panel: Panel::Toolbar,
            tile,
        };
        let press = |s: &mut UiSession<Recorder>, key: &str| {
            s.input(UiInput::Key {
                key: key.into(),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
                editing: false,
                divider: None,
            })
            .unwrap()
        };
        s.dispatch(open.clone()).unwrap();
        assert!(press(&mut s, "b").handled);
        assert_eq!(s.state.brush.tool, Tool::Brush);
        assert!(s.state.customization.drawer.is_none());
        s.dispatch(open).unwrap();
        assert!(press(&mut s, "tab").chrome_hidden);
        assert!(s.state.customization.drawer.is_none());
        assert!(chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default()).chrome_hidden);
    }

    #[test]
    fn tile_activation_uses_live_core_commands_and_stale_drag_ids_are_rejected() {
        let mut app = session();
        app.state
            .workspace
            .layout
            .insert_tools(
                Panel::Toolbar,
                None,
                &[
                    ToolbarControl::Size { pixels: 64 },
                    ToolbarControl::Command {
                        command: CommandId::ToggleTheme,
                    },
                ],
            )
            .unwrap();
        let tiles = app
            .state
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .to_vec();
        app.dispatch(UiAction::ActivateTile {
            panel: Panel::Toolbar,
            tile: tiles[TOOLBAR_CONTROLS.len()].id,
        })
        .unwrap();
        assert_eq!(app.state.brush.diameter, 64.0);
        assert!(
            app.panel_view(Panel::Toolbar).unwrap().tiles[TOOLBAR_CONTROLS.len()]
                .choice
                .selected
        );
        app.dispatch(UiAction::ActivateTile {
            panel: Panel::Toolbar,
            tile: tiles[TOOLBAR_CONTROLS.len() + 1].id,
        })
        .unwrap();
        assert!(
            app.panel_view(Panel::Toolbar).unwrap().tiles[TOOLBAR_CONTROLS.len() + 1]
                .choice
                .selected
        );
        app.dispatch(UiAction::Customize {
            action: CustomizationAction::RemoveTool {
                panel: Panel::Toolbar,
                tile: tiles[TOOLBAR_CONTROLS.len()].id,
            },
        })
        .unwrap();
        assert!(
            app.dispatch(UiAction::ActivateTile {
                panel: Panel::Toolbar,
                tile: tiles[TOOLBAR_CONTROLS.len()].id
            })
            .is_err()
        );
    }
    #[test]
    fn android_drawers_and_columns_replace_legacy_popup_behavior() {
        let mut app = session();
        app.set_platform(Platform::Android);
        let viewport = [1200.0, 900.0];
        app.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport })
            .unwrap();
        assert_eq!(app.state.workspace.layout.collapsed.len(), 1);
        invoke(&mut app, CommandId::UndoWorkspace);
        assert!(app.state.workspace.layout.collapsed.is_empty());

        let tile = app
            .state
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .iter()
            .find(|tile| tile.control == ToolbarControl::Color)
            .unwrap()
            .id;
        for open in [true, false] {
            app.dispatch(UiAction::ActivateTile {
                panel: Panel::Toolbar,
                tile,
            })
            .unwrap();
            assert_eq!(app.state.customization.drawer.is_some(), open);
            assert!(app.state.customization.control.is_none());
        }
        assert!(Panel::Navigator.available_on(Platform::Android));
        assert!(Panel::Navigator.available_on(Platform::Windows));
    }

    #[test]
    fn windows_tiles_toggle_drawers_and_explicit_control_open_is_idempotent() {
        let mut app = session();
        app.set_platform(Platform::Windows);
        let tile = |app: &UiSession<Recorder>, control| {
            app.state
                .workspace
                .layout
                .panel(Panel::Toolbar)
                .unwrap()
                .tiles()
                .iter()
                .find(|t| t.control == control)
                .unwrap()
                .id
        };
        let color = tile(&app, ToolbarControl::Color);
        let opacity = tile(&app, ToolbarControl::Opacity);
        for id in [color, opacity] {
            for open in [true, false] {
                app.dispatch(UiAction::ActivateTile {
                    panel: Panel::Toolbar,
                    tile: id,
                })
                .unwrap();
                assert_eq!(app.state.customization.drawer.is_some(), open);
                assert!(app.state.customization.control.is_none());
            }
        }
        for _ in 0..2 {
            app.dispatch(UiAction::Customize {
                action: CustomizationAction::OpenControl {
                    control: PanelControl::BrushColor,
                },
            })
            .unwrap();
            assert_eq!(
                app.state.customization.control,
                Some(PanelControl::BrushColor)
            );
        }
        app.dispatch(UiAction::ActivateTile {
            panel: Panel::Toolbar,
            tile: opacity,
        })
        .unwrap();
        assert!(app.state.customization.control.is_none());
        assert!(app.state.customization.drawer.is_some());
    }

    #[test]
    fn appearance_follows_system_unless_overridden() {
        let mut app = session();
        let check_menu = |state: &UiState| {
            let command = state
                .commands
                .iter()
                .find(|c| c.id == CommandId::ToggleTheme)
                .unwrap();
            assert_eq!(command.label, "Dark Mode");
            assert_eq!(command.selected, state.theme == Theme::Dark);
        };
        assert_eq!(app.state.settings.theme, None);
        check_menu(&app.state);
        for theme in [Theme::Dark, Theme::Light] {
            let change = app
                .dispatch(UiAction::SystemThemeChanged { theme })
                .unwrap();
            assert_eq!(app.state.theme, theme);
            assert!(change.canvas_wake);
            assert_eq!(app.state.settings.theme, None);
            check_menu(&app.state);
            assert_ne!(change.regions & regions::COMMANDS, 0);
        }
        app.dispatch(UiAction::SetTheme {
            theme: Some(Theme::Light),
        })
        .unwrap();
        let change = app
            .dispatch(UiAction::SystemThemeChanged { theme: Theme::Dark })
            .unwrap();
        assert_eq!(app.state.theme, Theme::Light);
        assert!(!change.canvas_wake);
        assert_eq!(change.regions, 0);
        check_menu(&app.state);
        // Return to auto using the latest system value, without a new OS event.
        app.dispatch(UiAction::SetTheme { theme: None }).unwrap();
        assert_eq!(app.state.theme, Theme::Dark);
        check_menu(&app.state);
        invoke(&mut app, CommandId::ToggleTheme);
        assert_eq!(app.state.settings.theme, Some(Theme::Light));
        check_menu(&app.state);
        invoke(&mut app, CommandId::Settings);
        app.dispatch(UiAction::EditSettings {
            settings: Settings::default(),
        })
        .unwrap();
        assert_eq!(app.state.theme, Theme::Dark);
        check_menu(&app.state);
        app.dispatch(UiAction::CloseSettings).unwrap();
        assert_eq!(app.state.theme, Theme::Dark);
        assert_eq!(app.state.settings.theme, None);
        check_menu(&app.state);
    }
    #[test]
    fn settings_apply_individually_and_dismissal_never_reverts_them() {
        let mut app = session();
        invoke(&mut app, CommandId::Settings);
        let settings = Settings {
            theme: Some(Theme::Light),
            pressure_gamma: 1.6,
            ..Settings::default()
        };
        app.dispatch(UiAction::EditSettings {
            settings: settings.clone(),
        })
        .unwrap();
        assert_eq!(app.state.settings, settings);
        app.dispatch(UiAction::CloseSettings).unwrap();
        assert!(!app.state.settings_open);
        assert_eq!(app.state.settings, settings);
        invoke(&mut app, CommandId::Settings);
        app.dispatch(UiAction::EditSettings {
            settings: settings.clone(),
        })
        .unwrap();
        app.dispatch(UiAction::CloseSettings).unwrap();
        assert_eq!(app.state.settings, settings);
        assert!(
            app.dispatch(UiAction::EditSettings {
                settings: Settings::default()
            })
            .is_err()
        );
    }

    fn preference(s: &mut UiSession<Recorder>, action: PreferenceAction) {
        s.dispatch(UiAction::Preferences { action }).unwrap();
    }
    fn edit_preference(s: &mut UiSession<Recorder>, id: PreferenceId, value: PreferenceValue) {
        preference(s, PreferenceAction::Edit { id, value });
    }
    #[test]
    fn base_colors_validate_save_and_follow_the_resolved_theme() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            invoke(&mut s, CommandId::Settings);
            let before = s.state.palette;
            for invalid in ["red", "#12345g", "#123", "#12345678"] {
                edit_preference(
                    &mut s,
                    PreferenceId::DarkBase,
                    PreferenceValue::Text(invalid.into()),
                );
                assert!(s.preferences().unwrap().error.is_some());
                assert_eq!(s.state.palette, before);
                assert!(s.state.requests.is_empty());
            }
            edit_preference(
                &mut s,
                PreferenceId::DarkBase,
                PreferenceValue::Text("#1C2c3C".into()),
            );
            assert_eq!(s.state.settings.dark_base.to_string(), "#1c2c3c");
            assert_eq!(
                s.state.palette, before,
                "editing the inactive mode does not recolor the current one"
            );
            assert!(
                s.state
                    .requests
                    .iter()
                    .any(|r| matches!(r.kind, HostRequestKind::SaveSettings { .. }))
            );
            s.dispatch(UiAction::SetTheme {
                theme: Some(Theme::Dark),
            })
            .unwrap();
            assert_eq!(s.state.palette.bg, s.state.settings.dark_base);
            assert_eq!(s.state.palette.text, HexColor([250, 250, 251]));
            edit_preference(
                &mut s,
                PreferenceId::LightBase,
                PreferenceValue::Text("#c0b49c".into()),
            );
            s.dispatch(UiAction::SetTheme { theme: None }).unwrap();
            s.dispatch(UiAction::SystemThemeChanged {
                theme: Theme::Light,
            })
            .unwrap();
            assert_eq!(s.state.palette.bg, s.state.settings.light_base);
            let json = serde_json::to_string(&s.state.settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, s.state.settings);
            assert_eq!(restored.palette(s.state.theme, platform), s.state.palette);
            assert!(serde_json::from_str::<Settings>(&json.replace("#1c2c3c", "bad")).is_err());
            let mut legacy = serde_json::to_value(&restored).unwrap();
            legacy.as_object_mut().unwrap().remove("dark_base");
            legacy.as_object_mut().unwrap().remove("light_base");
            let legacy: Settings = serde_json::from_value(legacy).unwrap();
            assert_eq!(legacy.dark_base, Theme::Dark.default_base());
            assert_eq!(legacy.light_base, Theme::Light.default_base());
        }
    }
    #[test]
    fn settings_navigation_validation_and_autosave_are_shared() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            invoke(&mut s, CommandId::Settings);
            preference(
                &mut s,
                PreferenceAction::Page {
                    page: SettingsPage::Input,
                },
            );
            assert!(s.state.requests.is_empty());
            for value in ["abc", "NaN", "65", "-1"] {
                edit_preference(
                    &mut s,
                    PreferenceId::PredictionHorizon,
                    PreferenceValue::Text(value.into()),
                );
                assert!(s.preferences().unwrap().error.is_some());
                assert_eq!(s.state.settings.prediction_ms, 8.0);
                assert!(
                    s.state.requests.is_empty(),
                    "invalid edits never reach storage"
                );
            }
            edit_preference(
                &mut s,
                PreferenceId::PredictionHorizon,
                PreferenceValue::Text("64".into()),
            );
            assert_eq!(
                s.state.settings.feedback_config().prediction_horizon_micros,
                64_000
            );
            assert!(s.preferences().unwrap().error.is_none());
            assert_eq!(s.state.requests.len(), 1);
            edit_preference(
                &mut s,
                PreferenceId::PredictionHorizon,
                PreferenceValue::Text("64".into()),
            );
            assert_eq!(
                s.state.requests.len(),
                1,
                "unchanged values do not write again"
            );
            assert_eq!(s.preferences().unwrap().page, SettingsPage::Input);
            preference(
                &mut s,
                PreferenceAction::EditShortcut {
                    id: CommandId::Brush.shortcut_id(),
                },
            );
            assert!(s.preferences().unwrap().shortcut_editor.is_some());
            preference(
                &mut s,
                PreferenceAction::Page {
                    page: SettingsPage::Canvas,
                },
            );
            assert!(s.preferences().unwrap().shortcut_editor.is_none());
            assert_eq!(
                s.state.requests.len(),
                1,
                "navigation does not write settings"
            );
            s.dispatch(UiAction::CloseSettings).unwrap();
            assert_eq!(s.state.settings.prediction_ms, 64.0);
            assert_eq!(s.state.requests.len(), 1, "Done only dismisses");
            invoke(&mut s, CommandId::Settings);
            assert_eq!(s.state.settings.prediction_ms, 64.0);
        }
    }
    #[test]
    fn settings_sliders_use_core_ranges_steps_and_dependencies() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            invoke(&mut s, CommandId::Settings);
            for row in s
                .preferences()
                .unwrap()
                .pages
                .into_iter()
                .flat_map(|p| p.groups)
                .flat_map(|g| g.rows)
            {
                let PreferenceKind::Number { control, .. } = row.kind else {
                    continue;
                };
                let resolved = control
                    .resolve(control.min, NumericOperation::Position { position: 0.37 })
                    .unwrap()
                    .value as f32;
                edit_preference(&mut s, row.id, PreferenceValue::Number(resolved));
                assert!(s.preferences().unwrap().error.is_none());
                let actual = s
                    .preferences()
                    .unwrap()
                    .pages
                    .into_iter()
                    .flat_map(|p| p.groups)
                    .flat_map(|g| g.rows)
                    .find(|r| r.id == row.id)
                    .unwrap();
                let PreferenceKind::Number { value, .. } = actual.kind else {
                    unreachable!()
                };
                assert_eq!(value, resolved);
                for value in [control.min as f32, control.max as f32] {
                    edit_preference(&mut s, row.id, PreferenceValue::Number(value));
                    assert!(s.preferences().unwrap().error.is_none());
                }
                let saved = s.state.settings.clone();
                let requests = s.state.requests.len();
                for value in [
                    (control.max + 1.0) as f32,
                    (control.min - 1.0) as f32,
                    f32::NAN,
                ] {
                    edit_preference(&mut s, row.id, PreferenceValue::Number(value));
                    assert!(s.preferences().unwrap().error.is_some());
                    assert_eq!(s.state.settings, saved);
                    assert_eq!(s.state.requests.len(), requests);
                }
            }
            edit_preference(&mut s, PreferenceId::Feedback, PreferenceValue::Bool(false));
            let saved = s.state.settings.clone();
            preference(
                &mut s,
                PreferenceAction::Edit {
                    id: PreferenceId::PredictionHorizon,
                    value: PreferenceValue::Number(20.0),
                },
            );
            assert!(s.preferences().unwrap().error.is_some());
            assert_eq!(s.state.settings, saved);
            preference(
                &mut s,
                PreferenceAction::Edit {
                    id: PreferenceId::Theme,
                    value: PreferenceValue::Number(1.5),
                },
            );
            assert!(s.preferences().unwrap().error.is_some());
        }
    }

    #[test]
    fn settings_type_to_search_is_shared_and_preserves_native_editing() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            invoke(&mut s, CommandId::Settings);
            let settings = s.state.settings.clone();
            for character in ["P", "r", "é", "s", "s"] {
                assert!(key(&mut s, character, true, false, false).handled);
                key(&mut s, character, false, false, false);
            }
            let view = s.preferences().unwrap();
            assert_eq!(view.query, "Préss");
            assert_eq!(view.search_focus, 5);
            assert!(view.searching);
            for (name, command, editing) in [
                ("x", false, true),
                ("c", true, false),
                ("Dead", false, false),
                ("ArrowLeft", false, false),
                (" ", false, false),
            ] {
                assert!(!key(&mut s, name, true, command, editing).handled);
                key(&mut s, name, false, command, editing);
            }
            s.input(UiInput::Key {
                key: "x".into(),
                pressed: true,
                repeat: false,
                modifiers: Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
                editing: false,
                divider: None,
            })
            .unwrap();
            assert_eq!(s.preferences().unwrap().query, "Préss");
            assert_eq!(s.preferences().unwrap().search_focus, 5);
            preference(
                &mut s,
                PreferenceAction::EditShortcut {
                    id: CommandId::Brush.shortcut_id(),
                },
            );
            key(&mut s, "b", true, false, false);
            assert!(s.preferences().unwrap().query.is_empty());
            preference(
                &mut s,
                PreferenceAction::BeginShortcut {
                    id: CommandId::Brush.shortcut_id(),
                },
            );
            key(&mut s, "w", true, false, false);
            assert!(s.preferences().unwrap().capture.unwrap().chord.is_some());
            assert!(s.preferences().unwrap().query.is_empty());
            assert_eq!(
                s.state.settings, settings,
                "search must not change saved settings"
            );
        }
    }

    #[test]
    fn android_persistent_search_returns_to_categories_when_empty() {
        let mut s = session();
        s.set_platform(Platform::Android);
        invoke(&mut s, CommandId::Settings);
        for query in ["", "   ", "prediction", ""] {
            preference(
                &mut s,
                PreferenceAction::Search {
                    query: query.into(),
                },
            );
            assert_eq!(s.preferences().unwrap().searching, !query.trim().is_empty());
        }
        s.set_platform(Platform::Gtk);
        preference(&mut s, PreferenceAction::ToggleSearch { open: true });
        assert!(
            s.preferences().unwrap().searching,
            "Desktop keeps its explicit search view"
        );
    }

    fn record_shortcut(s: &mut UiSession<Recorder>, id: &str, name: &str, command: bool) {
        preference(s, PreferenceAction::BeginShortcut { id: id.into() });
        assert!(key(s, name, true, command, true).handled);
        key(s, name, false, command, true);
    }
    #[test]
    fn about_links_are_shared_searchable_and_read_only() {
        for platform in [Platform::Gtk, Platform::Web] {
            let mut s = session();
            s.set_platform(platform);
            invoke(&mut s, CommandId::About);
            let view = s.preferences().unwrap();
            let about = view
                .pages
                .iter()
                .find(|p| p.id == SettingsPage::About)
                .unwrap();
            let links: Vec<_> = about
                .groups
                .iter()
                .flat_map(|g| &g.rows)
                .filter_map(|r| {
                    if let PreferenceKind::Link { label, url } = &r.kind {
                        Some((r.title.as_str(), label.as_str(), url.as_str()))
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(
                links,
                [
                    ("Website", "capycanvas.art", "https://capycanvas.art/"),
                    (
                        "Source code",
                        "github.com/capyatelier/capycanvas",
                        "https://github.com/capyatelier/capycanvas"
                    ),
                ]
            );
            for id in [PreferenceId::Website, PreferenceId::SourceCode] {
                edit_preference(&mut s, id, PreferenceValue::Choice(0));
                assert!(s.preferences().unwrap().error.is_some());
                assert_eq!(s.state.settings, Settings::default());
            }
            preference(
                &mut s,
                PreferenceAction::Search {
                    query: "github".into(),
                },
            );
            let view = s.preferences().unwrap();
            assert_eq!(view.page, SettingsPage::About);
            assert_eq!(
                view.search_results
                    .iter()
                    .map(|r| r.title.as_str())
                    .collect::<Vec<_>>(),
                ["Source code"]
            );
        }
    }
    #[test]
    fn preferences_metadata_search_dependencies_and_deep_links() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::Settings);
        assert_eq!(s.preferences().unwrap().pages.len(), 5);
        let rows = |s: &UiSession<Recorder>| {
            s.preferences()
                .unwrap()
                .pages
                .into_iter()
                .flat_map(|p| p.groups)
                .flat_map(|g| g.rows)
                .collect::<Vec<_>>()
        };
        assert!(
            !rows(&s)
                .iter()
                .any(|r| r.id == PreferenceId::PlatformPrediction)
        );
        edit_preference(&mut s, PreferenceId::Pressure, PreferenceValue::Number(1.7));
        invoke(&mut s, CommandId::KeyboardShortcuts);
        assert_eq!(s.preferences().unwrap().page, SettingsPage::Shortcuts);
        assert_eq!(s.state.settings.pressure_gamma, 1.7);
        preference(
            &mut s,
            PreferenceAction::Search {
                query: "pressure response".into(),
            },
        );
        let view = s.preferences().unwrap();
        assert_eq!(
            view.page,
            SettingsPage::Shortcuts,
            "search does not replace the content page"
        );
        assert_eq!(view.search_results.len(), 1);
        assert_eq!(view.search_results[0].title, "Pressure response");
        preference(&mut s, view.search_results[0].action.clone());
        assert_eq!(s.preferences().unwrap().page, SettingsPage::Input);
        preference(
            &mut s,
            PreferenceAction::Search {
                query: "no-such-preference".into(),
            },
        );
        assert!(s.preferences().unwrap().empty);
        preference(
            &mut s,
            PreferenceAction::Page {
                page: SettingsPage::Input,
            },
        );
        assert!(s.preferences().unwrap().query.is_empty());
        assert!(!s.preferences().unwrap().empty);
        edit_preference(&mut s, PreferenceId::Feedback, PreferenceValue::Bool(false));
        assert!(
            !rows(&s)
                .iter()
                .find(|r| r.id == PreferenceId::PredictionHorizon)
                .unwrap()
                .enabled
        );
        let before = s.state.settings.clone();
        edit_preference(
            &mut s,
            PreferenceId::PredictionHorizon,
            PreferenceValue::Number(20.0),
        );
        assert!(s.preferences().unwrap().error.is_some());
        assert_eq!(s.state.settings, before);
        edit_preference(
            &mut s,
            PreferenceId::Pressure,
            PreferenceValue::Number(f32::NAN),
        );
        assert_eq!(s.state.settings, before);
        edit_preference(&mut s, PreferenceId::Theme, PreferenceValue::Choice(99));
        assert_eq!(s.state.settings, before);
        s.set_platform(Platform::Web);
        assert!(
            rows(&s)
                .iter()
                .any(|r| r.id == PreferenceId::PlatformPrediction)
        );
    }
    #[test]
    fn settings_json_is_versioned_validated_and_backwards_compatible() {
        let old: Settings =
            serde_json::from_str(r#"{"theme":null,"pressure_gamma":1.5,"cursor":"brush_size"}"#)
                .unwrap();
        assert_eq!(old.pressure_gamma, 1.5);
        let mut s = session();
        s.dispatch(UiAction::RestoreSettings {
            settings: old.clone(),
        })
        .unwrap();
        assert!(s.state.requests.is_empty(), "restore must not echo a save");
        for invalid in [
            Settings {
                version: 2,
                ..old.clone()
            },
            Settings {
                pan_speed: -1.0,
                ..old.clone()
            },
            Settings {
                tip_lock: 5.0,
                ..old.clone()
            },
        ] {
            assert!(
                s.dispatch(UiAction::RestoreSettings { settings: invalid })
                    .is_err()
            );
            assert_eq!(s.state.settings, old);
        }
        assert!(serde_json::from_str::<Settings>(r#"{"unknown_setting":true}"#).is_err());
        let action: UiAction = serde_json::from_str(
            r#"{"type":"preferences","action":{"type":"edit","id":"theme","value":2}}"#,
        )
        .unwrap();
        invoke(&mut s, CommandId::Settings);
        s.dispatch(action).unwrap();
        assert_eq!(s.state.settings.theme, Some(Theme::Dark));
        assert_eq!(s.state.settings.theme, Some(Theme::Dark));
        s.dispatch(UiAction::CloseSettings).unwrap();
        assert_eq!(s.state.requests.len(), 1);
    }
    #[test]
    fn shortcut_conflicts_require_explicit_replacement_and_update_menu_hints_immediately() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::KeyboardShortcuts);
        let target = CommandId::Brush.shortcut_id();
        record_shortcut(&mut s, &target, "y", true);
        assert_eq!(
            s.preferences()
                .unwrap()
                .capture
                .unwrap()
                .conflict
                .as_deref(),
            Some("Redo")
        );
        let before = s.state.settings.clone();
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
        assert_eq!(s.state.settings, before);
        assert!(s.preferences().unwrap().error.is_some());
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: true });
        assert_eq!(
            s.command(CommandId::Brush).shortcut,
            "Ctrl+Y / B",
            "confirmed bindings must take effect immediately"
        );
        assert_eq!(
            s.state.settings.keys(&CommandId::Redo.shortcut_id()).len(),
            1,
            "preserve Redo's other accelerator"
        );
        s.dispatch(UiAction::CloseSettings).unwrap();
        assert_eq!(s.command(CommandId::Brush).shortcut, "Ctrl+Y / B");
        for command in &s.state.commands {
            let current = s.command(command.id);
            assert_eq!(command.bindings, current.bindings);
            assert_eq!(command.shortcut, current.shortcut);
        }
        invoke(&mut s, CommandId::Eraser);
        assert!(key(&mut s, "y", true, true, false).handled);
        assert_eq!(s.state.brush.tool, Tool::Brush);
        let saved = serde_json::to_string(&s.state.settings).unwrap();
        let restored: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored, s.state.settings);
        restored.validate().unwrap();
        invoke(&mut s, CommandId::Settings);
        preference(
            &mut s,
            PreferenceAction::ResetShortcut {
                id: CommandId::Redo.shortcut_id(),
            },
        );
        assert!(
            s.preferences().unwrap().error.is_some(),
            "reset cannot steal another shortcut"
        );
        preference(&mut s, PreferenceAction::ResetAllShortcuts);
        assert!(s.state.settings.shortcuts.is_empty());
    }
    #[test]
    fn shortcut_recording_cancel_clear_and_platform_reservations() {
        let mut s = session();
        s.set_platform(Platform::Web);
        assert!(!s.command(CommandId::NewWindow).enabled);
        invoke(&mut s, CommandId::KeyboardShortcuts);
        assert!(
            !s.preferences()
                .unwrap()
                .shortcuts
                .iter()
                .any(|r| r.id == CommandId::NewWindow.shortcut_id())
        );
        let target = ToolFamily::Paint.shortcut_id().to_string();
        record_shortcut(&mut s, &target, "control", false);
        assert!(s.preferences().unwrap().capture.unwrap().chord.is_none());
        record_shortcut(&mut s, &target, "w", true);
        assert!(s.preferences().unwrap().capture.unwrap().error.is_some());
        key(&mut s, "escape", true, false, true);
        assert!(s.preferences().unwrap().capture.is_none());
        assert!(
            s.state.settings_open,
            "Escape only closes the recording sheet"
        );
        record_shortcut(&mut s, &target, "tab", false);
        let capture = s.preferences().unwrap().capture.unwrap();
        assert!(capture.error.is_none());
        assert_eq!(capture.conflict.as_deref(), Some("Zen mode"));
        preference(
            &mut s,
            PreferenceAction::EditShortcut { id: target.clone() },
        );
        preference(
            &mut s,
            PreferenceAction::RemoveShortcut {
                id: target.clone(),
                index: 0,
            },
        );
        assert!(s.state.settings.keys(&target).is_empty());
        preference(
            &mut s,
            PreferenceAction::ResetShortcut { id: target.clone() },
        );
        assert_eq!(s.state.settings.keys(&target).len(), 1);
    }
    #[test]
    fn arbitrary_typed_actions_and_momentary_pan_use_the_same_keymap() {
        let mut s = session();
        invoke(&mut s, CommandId::KeyboardShortcuts);
        preference(
            &mut s,
            PreferenceAction::RegisterAction {
                definition: ShortcutDefinition {
                    id: "custom.size-42".into(),
                    label: "My drawing size".into(),
                    repeat: false,
                    action: ShortcutAction::Action {
                        action: Box::new(UiAction::SetBrushSize { value: 42.0 }),
                    },
                },
            },
        );
        record_shortcut(&mut s, "custom.size-42", "k", false);
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
        record_shortcut(&mut s, "canvas.pan", "g", false);
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: true });
        s.dispatch(UiAction::CloseSettings).unwrap();
        assert!(
            !key(&mut s, "k", true, false, true).handled,
            "native text editing wins"
        );
        key(&mut s, "k", false, false, true);
        assert!(key(&mut s, "k", true, false, false).handled);
        assert_eq!(s.state.brush.diameter, 42.0);
        assert!(key(&mut s, " ", true, false, false).pan_cursor);
        key(&mut s, " ", false, false, false);
        assert!(key(&mut s, "g", true, false, false).pan_cursor);
        assert!(
            !key(&mut s, "g", false, true, true).pan_cursor,
            "release clears pan even if modifiers/focus changed"
        );
    }
    #[test]
    fn typography_is_fixed_and_retired_preferences_preserve_other_settings() {
        assert_eq!(ui_catalog().text_size_pt, UI_TEXT_PT);
        assert_eq!(UI_TEXT_PT, 11);
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut s = session();
            s.set_platform(platform);
            for points in [9, 11, 13] {
                let json = format!(
                    r#"{{"panel_text_pt":{points},"zen_hide":200,"zen_reveal":120,"pressure_gamma":1.5}}"#
                );
                let native =
                    Settings::deserialize_saved(&mut serde_json::Deserializer::from_str(&json))
                        .unwrap();
                let action: UiAction = serde_json::from_str(&format!(
                    r#"{{"type":"restore_settings","settings":{json}}}"#
                ))
                .unwrap();
                s.dispatch(action).unwrap();
                assert_eq!(s.state.settings, native);
                assert_eq!(native.pressure_gamma, 1.5);
                let saved = serde_json::to_value(native).unwrap();
                assert!(saved.get("panel_text_pt").is_none());
                assert!(saved.get("zen_hide").is_none());
                assert!(saved.get("zen_reveal").is_none());
            }
            invoke(&mut s, CommandId::Settings);
            assert!(
                !s.preferences()
                    .unwrap()
                    .pages
                    .iter()
                    .flat_map(|p| &p.groups)
                    .flat_map(|g| &g.rows)
                    .any(|r| matches!(
                        r.title.as_str(),
                        "Panel text size" | "Keep-visible distance (px)" | "Edge reveal distance"
                    ))
            );
        }
        assert!(
            Settings::deserialize_saved(&mut serde_json::Deserializer::from_str(
                r#"{"unknown_setting":true}"#
            ))
            .is_err()
        );
        assert!(
            serde_json::from_str::<UiAction>(
                r#"{"type":"restore_settings","settings":{"unknown_setting":true}}"#
            )
            .is_err()
        );
    }
    #[test]
    fn shortcut_search_includes_current_bindings_and_modifiers_on_every_platform() {
        for platform in [
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
            let mut s = session();
            s.set_platform(platform);
            invoke(&mut s, CommandId::KeyboardShortcuts);
            for query in ["z", " Z "] {
                preference(
                    &mut s,
                    PreferenceAction::SearchShortcuts {
                        query: query.into(),
                    },
                );
                preference(
                    &mut s,
                    PreferenceAction::Search {
                        query: query.into(),
                    },
                );
                let view = s.preferences().unwrap();
                for command in [
                    CommandId::Undo,
                    CommandId::Redo,
                    CommandId::UndoWorkspace,
                    CommandId::RedoWorkspace,
                ] {
                    let id = command.shortcut_id();
                    assert!(
                        view.shortcuts.iter().any(|r| r.id == id && r.visible),
                        "{platform:?}: {query} must find {id}"
                    );
                    assert!(view.search_results.iter().any(|r| matches!(&r.action, PreferenceAction::EditShortcut { id: found } if *found == id)));
                }
            }
            let command_key = if platform.apple() { "⌘" } else { "ctrl" };
            preference(
                &mut s,
                PreferenceAction::SearchShortcuts {
                    query: format!("{command_key}+z"),
                },
            );
            let view = s.preferences().unwrap();
            assert!(
                view.shortcuts
                    .iter()
                    .any(|r| r.id == CommandId::Undo.shortcut_id() && r.visible)
            );
            assert!(
                !view
                    .shortcuts
                    .iter()
                    .find(|r| r.id == CommandId::ZenMode.shortcut_id())
                    .unwrap()
                    .visible
            );
            let id = CommandId::Brush.shortcut_id();
            preference(&mut s, PreferenceAction::EditShortcut { id: id.clone() });
            record_shortcut(&mut s, &id, "q", false);
            preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
            preference(
                &mut s,
                PreferenceAction::SearchShortcuts { query: "q".into() },
            );
            assert!(
                s.preferences()
                    .unwrap()
                    .shortcuts
                    .iter()
                    .find(|r| r.id == id)
                    .unwrap()
                    .visible
            );
            preference(&mut s, PreferenceAction::ResetShortcut { id: id.clone() });
            assert!(
                !s.preferences()
                    .unwrap()
                    .shortcuts
                    .iter()
                    .find(|r| r.id == id)
                    .unwrap()
                    .visible,
                "Search must not retain removed bindings"
            );
        }
    }

    #[test]
    fn shortcut_modified_tracks_binding_sets_not_saved_override_presence() {
        let mut s = session();
        invoke(&mut s, CommandId::KeyboardShortcuts);
        let id = ToolFamily::Paint.shortcut_id().to_string();
        preference(&mut s, PreferenceAction::EditShortcut { id: id.clone() });
        let modified = |s: &UiSession<Recorder>| {
            let view = s.preferences().unwrap();
            let modified = view.shortcuts.iter().find(|r| r.id == id).unwrap().modified;
            assert_eq!(view.shortcut_editor.unwrap().modified, modified);
            modified
        };
        assert!(!modified(&s));
        record_shortcut(&mut s, &id, "k", false);
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
        assert!(modified(&s));
        preference(
            &mut s,
            PreferenceAction::RemoveShortcut {
                id: id.clone(),
                index: 1,
            },
        );
        assert!(s.state.settings.shortcuts.contains_key(&id));
        assert!(
            !modified(&s),
            "Removing the extra binding restores the default"
        );
        preference(
            &mut s,
            PreferenceAction::RemoveShortcut {
                id: id.clone(),
                index: 0,
            },
        );
        assert!(modified(&s));
        assert_eq!(
            s.preferences()
                .unwrap()
                .shortcuts
                .iter()
                .find(|r| r.id == id)
                .unwrap()
                .shortcut,
            "Disabled"
        );
        record_shortcut(&mut s, &id, "b", false);
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
        assert!(
            !modified(&s),
            "Re-recording the default clears the indicator"
        );
        let redo = CommandId::Redo.shortcut_id();
        let mut reversed = s.state.settings.keys(&redo);
        reversed.reverse();
        s.state.settings.shortcuts.insert(redo.clone(), reversed);
        assert!(
            !s.preferences()
                .unwrap()
                .shortcuts
                .iter()
                .find(|r| r.id == redo)
                .unwrap()
                .modified,
            "Reordering alternatives doesn't customize the binding set"
        );
    }

    #[test]
    fn shortcut_editor_preserves_alternatives_and_owns_search_limits_and_defaults() {
        for platform in [Platform::Gtk, Platform::Web] {
            let mut s = session();
            s.set_platform(platform);
            invoke(&mut s, CommandId::KeyboardShortcuts);
            let id = ToolFamily::Paint.shortcut_id().to_string();
            preference(&mut s, PreferenceAction::EditShortcut { id: id.clone() });
            let editor = s.preferences().unwrap().shortcut_editor.unwrap();
            assert_eq!(editor.bindings, ["B"]);
            assert_eq!(editor.defaults, ["B"]);
            for name in ["j", "k", "l"] {
                record_shortcut(&mut s, &id, name, false);
                preference(&mut s, PreferenceAction::ConfirmShortcut { replace: true });
                assert!(s.preferences().unwrap().error.is_none());
            }
            let view = s.preferences().unwrap();
            assert_eq!(
                view.shortcut_editor.as_ref().unwrap().bindings,
                ["B", "J", "K", "L"]
            );
            assert!(!view.shortcut_editor.unwrap().can_add);
            preference(&mut s, PreferenceAction::BeginShortcut { id: id.clone() });
            assert!(s.preferences().unwrap().capture.is_none());
            preference(
                &mut s,
                PreferenceAction::RemoveShortcut {
                    id: id.clone(),
                    index: 2,
                },
            );
            assert_eq!(
                s.preferences().unwrap().shortcut_editor.unwrap().bindings,
                ["B", "J", "L"]
            );
            record_shortcut(&mut s, &id, "j", false);
            let before = s.state.settings.clone();
            preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
            assert_eq!(s.state.settings, before, "duplicate addition is atomic");
            preference(&mut s, PreferenceAction::CancelShortcut);
            preference(&mut s, PreferenceAction::ResetShortcut { id });
            assert_eq!(
                s.preferences().unwrap().shortcut_editor.unwrap().bindings,
                ["B"]
            );
            preference(&mut s, PreferenceAction::CloseShortcutEditor);
            preference(
                &mut s,
                PreferenceAction::SearchShortcuts {
                    query: "eraser".into(),
                },
            );
            assert!(
                s.preferences()
                    .unwrap()
                    .shortcuts
                    .iter()
                    .filter(|r| r.visible)
                    .all(|r| r.label.to_lowercase().contains("eraser"))
            );
            preference(
                &mut s,
                PreferenceAction::Page {
                    page: SettingsPage::Input,
                },
            );
            edit_preference(
                &mut s,
                PreferenceId::PredictionHorizon,
                PreferenceValue::Number(64.0),
            );
            assert!(s.preferences().unwrap().error.is_none());
            s.state.settings.feedback_config().validate().unwrap();
            let before = s.state.settings.clone();
            edit_preference(
                &mut s,
                PreferenceId::PredictionHorizon,
                PreferenceValue::Number(65.0),
            );
            assert!(s.preferences().unwrap().error.is_some());
            assert_eq!(s.state.settings, before);
        }
    }
    #[test]
    fn applied_settings_emit_durable_host_requests_and_configure_input() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::NewWindow);
        let first = s.state.requests[0].id;
        invoke(&mut s, CommandId::Settings);
        edit_preference(&mut s, PreferenceId::PanSpeed, PreferenceValue::Number(2.0));
        edit_preference(
            &mut s,
            PreferenceId::PredictionHorizon,
            PreferenceValue::Number(4.0),
        );
        s.dispatch(UiAction::CloseSettings).unwrap();
        assert_eq!(
            s.state.settings.feedback_config().prediction_horizon_micros,
            4000
        );
        assert_eq!(s.state.requests.len(), 3);
        assert!(matches!(
            s.state.requests[0].kind,
            HostRequestKind::NewWindow
        ));
        assert!(matches!(
            s.state.requests[1].kind,
            HostRequestKind::SaveSettings { .. }
        ));
        let start = s.state.camera.translation;
        s.scroll([100.0; 2], [10.0, 20.0], 1.0, false, false)
            .unwrap();
        assert_eq!(
            s.state.camera.translation,
            [start[0] - 20.0, start[1] - 40.0]
        );
        s.dispatch(UiAction::CompleteRequest {
            id: first,
            error: Some("Window unavailable".into()),
        })
        .unwrap();
        assert_eq!(s.state.requests.len(), 2);
        assert_eq!(s.state.host_error.as_deref(), Some("Window unavailable"));
        assert!(
            s.dispatch(UiAction::CompleteRequest {
                id: first,
                error: None
            })
            .is_err()
        );
    }

    #[test]
    fn camera_and_viewport_changes_reuse_the_document_composite() {
        let mut app = session();
        app.frame(0, 8_000_000).unwrap();
        assert_eq!(app.engine.backend().composites, 1);
        app.gesture([200.0, 200.0], [250.0, 300.0], 1.25, 0.4)
            .unwrap();
        app.frame(8_000_000, 16_000_000).unwrap();
        app.set_viewport([800.0, 700.0], [800, 700]).unwrap();
        app.frame(16_000_000, 24_000_000).unwrap();
        assert_eq!(app.engine.backend().composites, 1);
        assert_eq!(app.engine.backend().dabs, 0);
    }
    #[test]
    fn layers_commands_and_undo_have_one_source_of_truth() {
        let mut app = session();
        invoke(&mut app, CommandId::AddLayer);
        let id = app.engine.document().active_layer.0;
        assert_eq!(app.state.layers.len(), 3);
        assert!(app.state.layers[0].selected);
        assert!(app.command(CommandId::DeleteLayer).enabled);
        invoke(&mut app, CommandId::Undo);
        assert_eq!(
            app.state.layers.len(),
            2,
            "New layer is one undo step, selection is navigation"
        );
        assert!(app.command(CommandId::Redo).enabled);
        invoke(&mut app, CommandId::Redo);
        app.dispatch(UiAction::SelectLayer { id }).unwrap();
        app.dispatch(UiAction::SetLayerVisibility { id, visible: false })
            .unwrap();
        assert!(!app.state.layers[0].visible);
        invoke(&mut app, CommandId::Undo);
        assert!(app.state.layers[0].visible);
        app.dispatch(UiAction::SelectLayer { id: 1 }).unwrap();
        assert!(
            app.command(CommandId::Redo).enabled,
            "Selecting a layer preserves redo"
        );
        app.dispatch(UiAction::SelectLayer { id }).unwrap();
        invoke(&mut app, CommandId::LowerLayer);
        assert_eq!(app.state.layers[1].id, id);
        assert!(!app.command(CommandId::LowerLayer).enabled);
        app.dispatch(UiAction::SetLayerOpacity {
            id: Some(id),
            opacity: 0.35,
        })
        .unwrap();
        assert_eq!(app.state.layers[1].opacity, 0.35);
        invoke(&mut app, CommandId::DeleteLayer);
        assert_eq!(app.state.layers.len(), 2);
        assert!(app.state.layers.iter().any(|l| l.selected && l.id == 1));
    }
    #[test]
    fn pen_uses_camera_and_pressure_without_ui_updates_per_move() {
        let mut app = session();
        app.pen(event(&app, 1, PenPhase::Down, 0.2)).unwrap();
        assert!(!app.command(CommandId::AddLayer).enabled);
        assert!(app.dispatch(UiAction::SelectLayer { id: 1 }).is_err());
        assert!(app.gesture([0.0; 2], [1.0; 2], 1.0, 0.0).is_err());
        app.frame(10_000_000, 18_000_000).unwrap();
        app.pen(event(&app, 2, PenPhase::Move, 0.8)).unwrap();
        let change = app.frame(20_000_000, 28_000_000).unwrap();
        assert_eq!(change.regions, 0);
        assert!(change.canvas_wake);
        app.pen(event(&app, 3, PenPhase::Up, 0.5)).unwrap();
        let change = app.frame(30_000_000, 38_000_000).unwrap();
        assert_ne!(change.regions & regions::DOCUMENT, 0);
        assert!(app.command(CommandId::Undo).enabled);
        let stroke = app.engine.document().strokes().next().unwrap();
        assert_eq!(stroke.points[0].pressure, 0.2);
        assert_eq!(stroke.points[1].pressure, 0.8);
        let expected = app
            .state
            .camera
            .input_transform()
            .map(Point { x: 225.0, y: 300.0 });
        assert!((stroke.points[0].position.x - expected.x).abs() < 0.001);
        assert!(app.engine.backend().dabs > 0);
        invoke(&mut app, CommandId::Undo);
        assert_eq!(app.engine.document().strokes().count(), 0);
    }
    #[test]
    fn cancel_removes_provisional_stroke_and_binding_actions_roundtrip() {
        let mut app = session();
        app.pen(event(&app, 1, PenPhase::Down, 0.5)).unwrap();
        app.frame(10_000_000, 18_000_000).unwrap();
        app.pen(event(&app, 2, PenPhase::Cancel, 0.0)).unwrap();
        app.frame(20_000_000, 28_000_000).unwrap();
        assert_eq!(app.engine.document().strokes().count(), 0);
        assert!(!app.command(CommandId::Undo).enabled);
        let action: UiAction = serde_json::from_str(r#"{"type":"move_panel","panel":"sizes","target":{"kind":"edge","edge":"right","outer":false},"viewport":[1200,900]}"#).unwrap();
        let value = serde_json::to_string(&action).unwrap();
        app.dispatch(serde_json::from_str(&value).unwrap()).unwrap();
        assert_eq!(app.state.workspace.layout.bands.len(), 4);
        assert!(serde_json::to_value(&app.state).unwrap()["commands"].is_array());
    }
}
