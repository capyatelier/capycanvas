use crate::interaction::{Interaction, PointerContact};
use crate::layout::ResizeDrag;
use crate::*;
use layer_core::{Document, LayerId, LayerKind, StrokeTool, default_brush};
use layer_engine::{CanvasEngine, InputProducer, PenEvent, PenPhase, PressureCurve, input_queue};
use layer_render::CanvasRenderer;

const ZEN_CORNER_GUARD: f32 = 300.0;

#[derive(Clone, Copy)]
struct WorkspaceDrag {
    original: DockItem,
    item: DockItem,
    panel: Panel,
    hide_tab: bool,
    source: Bounds,
    floating: Option<u32>,
    offset: [f32; 2],
    press: [f32; 2],
    chrome_revealed: bool,
    moved: bool,
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
    system_theme: Theme,
    logical_viewport: Option<[f32; 2]>,
    initial_fit: bool,
    divider_drag: Option<(u32, ResizeDrag)>,
    floating_resize: Option<FloatingResize>,
    workspace_drag: Option<WorkspaceDrag>,
    workspace_history: workspace::WorkspaceHistory,
    interaction: Interaction,
    cursor: cursor::Cursor,
    next_request: u32,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn blank(renderer: R, viewport: [u32; 2]) -> Result<Self, String> {
        Self::new(renderer, Document::new("untitled", 2048, 1536), viewport)
    }

    pub fn new(renderer: R, document: Document, viewport: [u32; 2]) -> Result<Self, String> {
        let camera = Camera::new([document.width, document.height], viewport);
        let (pen, input) = input_queue(8192);
        let engine = CanvasEngine::new(
            renderer,
            document,
            input,
            camera.view(),
            camera.input_transform(),
        )
        .map_err(|e| e.to_string())?;
        let brush = default_brush(DefaultBrushPreset::GPen);
        let mut session = Self {
            engine,
            pen,
            input_pending: false,
            touch: TouchGesture::default(),
            system_theme: Theme::Light,
            logical_viewport: None,
            initial_fit: true,
            divider_drag: None,
            floating_resize: None,
            workspace_drag: None,
            workspace_history: workspace::WorkspaceHistory::default(),
            interaction: Interaction::default(),
            cursor: cursor::Cursor::default(),
            next_request: 1,
            state: UiState {
                revision: 0,
                workspace: WorkspaceState::default(),
                brush: BrushState {
                    preset: DefaultBrushPreset::GPen as u32,
                    tool: Tool::Brush,
                    diameter: brush.diameter,
                    opacity: brush.opacity,
                    color: [0.075, 0.075, 0.07, 1.0],
                },
                layers: Vec::new(),
                tabs: Vec::new(),
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
        };
        session.apply_brush()?;
        session.refresh_document();
        session.refresh_commands();
        Ok(session)
    }

    pub fn state(&self) -> &UiState {
        &self.state
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
        match target {
            ContextTarget::ZenMode => self.state.settings.zen_menu(self.state.platform),
            _ => self.state.workspace.layout.context_menu(target),
        }
    }
    pub fn workspace_menu(&self) -> ContextMenu {
        let command = |id: CommandId| {
            let state = self.command(id);
            let mut item = ContextMenuItem::command(state.label, UiAction::Invoke { command: id });
            item.enabled = state.enabled;
            item.hint = state.shortcut;
            item
        };
        ContextMenu {
            title: WORKSPACE_MENU_LABEL.into(),
            sections: vec![
                vec![
                    command(CommandId::UndoWorkspace),
                    command(CommandId::RedoWorkspace),
                ],
                self.state
                    .workspace
                    .layout
                    .panel_items(PanelKind::Content, None),
                self.state
                    .workspace
                    .layout
                    .panel_items(PanelKind::Tiles, None),
                vec![command(CommandId::NewToolbar)],
            ],
        }
    }
    pub fn toolbar_prompt(&self) -> Option<crate::customization::ToolbarPromptView> {
        self.state.customization.toolbar_prompt.as_ref().map(|p| {
            p.view(
                &self.state.workspace.layout,
                &self.command(CommandId::UndoWorkspace).shortcut,
            )
        })
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
        let dabs =
            self.engine
                .cursor_contacts(event, &mut self.cursor.hover, self.cursor.origin_ns);
        self.cursor.view(
            self.engine.backend(),
            &self.engine.brush().tip,
            &dabs,
            &self.state.camera,
            scale,
            self.state.settings.cursor,
            view,
            svg,
        );
        true
    }
    /// Small event/reply boundary shared by native and Wasm hosts. Pen samples
    /// are only queued when `paint` is true, without serializing UiState.
    pub fn input(&mut self, input: UiInput) -> Result<InputReply, String> {
        let mut reply = InputReply {
            chrome_hidden: self.button_zen(),
            hide_floating_panels: self.button_zen(),
            keep_zen_button: self.state.settings.zen_show_button,
            ..Default::default()
        };
        let mut contact = None;
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
                    && self.state.customization.expanded.is_some()
                    && !self.button_zen()
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
                    }
                    if key == "escape"
                        && self.state.customization.expanded.is_some()
                        && !self.interaction.facts.popup_open
                    {
                        reply.change = self.dispatch(UiAction::Customize {
                            action: CustomizationAction::CloseExpanded,
                        })?;
                        reply.handled = true;
                    }
                    let blocked = editing
                        || self.state.settings_open
                        || self.state.customization.is_open()
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
                    let paint =
                        button == PointerButton::Primary && self.interaction.pan_key.is_none();
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
                self.cursor_input(None);
                reply.cancel_paint = self.interaction.pointer.take().is_some_and(|p| p.paint)
                    || self.input_pending
                    || self.engine.has_active_stroke();
                self.interaction.keys.clear();
                self.interaction.pan_key = None;
                self.interaction.keyboard_chrome = false;
                self.interaction.facts.held = false;
                // A native DND grab can blur the window without ending the
                // drag. Only the host's drag-end/cancel lifecycle releases it.
                self.interaction.hover = None;
                let divider = self.divider_drag.take();
                let floating = self.floating_resize.take();
                let workspace = self.workspace_drag.take();
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
        reply.hide_floating_panels = self.button_zen();
        reply.keep_zen_button = self.state.settings.zen_show_button;
        reply.pan_cursor = self.interaction.pan_key.is_some();
        Ok(reply)
    }

    fn button_zen(&self) -> bool {
        self.state.workspace.zen_mode
            && self.state.settings.zen_reveal_mode == ZenRevealMode::Button
    }

    fn refresh_chrome(&mut self) {
        // Explicit exit only: proximity, first contact, keyboard chrome hints
        // and drag/popup pins must not reveal the editor in this mode.
        if self.button_zen() {
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
            if self.workspace_drag.is_some_and(|d| d.original == item) {
                self.workspace_drag = None;
                self.workspace_history.cancel(&mut self.state.workspace);
                self.interaction.keep_chrome_until_contact = false;
            }
            return Ok(());
        }
        self.interaction.hover = Some(position);
        self.interaction.viewport = Some(viewport);
        self.interaction.zen_entry_guard = false;
        if phase == ContactPhase::Down {
            let layout = self.layout(viewport);
            let source = match item {
                DockItem::Group { group } => layout.groups.iter().find(|g| g.id == group),
                DockItem::Panel { panel } => {
                    layout.groups.iter().find(|g| g.panels.contains(&panel))
                }
                DockItem::Tile { .. } => return Err("Tools use tile reordering".into()),
            }
            .ok_or("Unknown drag source")?;
            let normalized = if source.panels.len() == 1 {
                DockItem::Group { group: source.id }
            } else {
                item
            };
            let whole = matches!(normalized, DockItem::Group { .. });
            self.workspace_history.begin(&self.state.workspace);
            self.workspace_drag = Some(WorkspaceDrag {
                original: item,
                item: normalized,
                panel: match item {
                    DockItem::Panel { panel } => panel,
                    _ => source.active,
                },
                hide_tab: self
                    .state
                    .workspace
                    .layout
                    .panel(match item {
                        DockItem::Panel { panel } => panel,
                        _ => source.active,
                    })?
                    .hide_tab,
                source: source.bounds,
                floating: (whole && source.floating).then_some(source.id),
                offset: [position[0] - source.bounds.x, position[1] - source.bounds.y],
                press: position,
                chrome_revealed: !source.floating || !self.interaction.hidden,
                moved: false,
            });
            if whole && source.floating {
                let floats = &mut self.state.workspace.layout.floating;
                let index = floats
                    .iter()
                    .position(|f| f.root.id() == source.id)
                    .unwrap();
                let floating = floats.remove(index);
                floats.push(floating);
            }
            self.state.customization = CustomizationState::default();
            return Ok(());
        }
        let mut drag = self
            .workspace_drag
            .filter(|d| d.original == item)
            .ok_or("Workspace drag is not active")?;
        drag.chrome_revealed |= self.layout(viewport).near_chrome(position, viewport, true);
        drag.moved |= position != drag.press;
        if drag.floating.is_none()
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
                    drag.offset[0].min(floated.bounds.width - 10.0).max(0.0),
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
                let merging = matches!(hint.target, DockTarget::Tab { .. });
                self.state
                    .workspace
                    .layout
                    .move_item(viewport, drag.item, hint.target)?;
                if !merging {
                    self.state.workspace.layout.panel_mut(drag.panel)?.hide_tab = drag.hide_tab;
                }
            }
            self.workspace_drag = None;
            self.workspace_history.finish(&self.state.workspace);
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
        if self.button_zen() {
            return None;
        }
        let mut resolved = self.layout(viewport);
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
                group.tiles = Some(tile_layout(
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
                        .tiles()
                        .len(),
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
        let hint = if matches!(item, DockItem::Tile { .. }) {
            resolved.tile_drop_hint(position, &self.state.workspace.layout)?
        } else {
            resolved.drop_hint(position[0], position[1], tabs, !docks_hidden)?
        };
        let mut probe = self.state.workspace.layout.clone();
        probe.move_item(viewport, item, hint.target.clone()).ok()?;
        Some(hint)
    }
    pub fn engine(&self) -> &CanvasEngine<R> {
        &self.engine
    }
    /// Surface lifecycle/presentation only; UI document edits use dispatch.
    pub fn renderer_mut(&mut self) -> &mut R {
        self.engine.backend_mut()
    }
    pub fn command(&self, id: CommandId) -> CommandState {
        let (enabled, selected) = self.command_flags(id);
        CommandState {
            icon: self.command_icon(id),
            id,
            label: id.label(),
            enabled,
            selected,
            bindings: self.state.settings.keys(&id.shortcut_id()),
            shortcut: self
                .state
                .settings
                .shortcut_label(&id.shortcut_id(), self.state.platform),
        }
    }
    fn command_icon(&self, id: CommandId) -> Option<&'static str> {
        if id == CommandId::ZenMode {
            Some(self.state.settings.zen_icon.icon())
        } else {
            id.icon()
        }
    }
    fn command_flags(&self, id: CommandId) -> (bool, bool) {
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
        let idle = !self.input_pending && !self.engine.has_active_stroke();
        let paint_layers = document
            .layers
            .iter()
            .filter(|layer| layer.kind == LayerKind::Paint)
            .count();
        let enabled = match id {
            CommandId::Undo => idle && self.engine.can_undo(),
            CommandId::Redo => idle && self.engine.can_redo(),
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
            CommandId::FitCanvas => idle,
            CommandId::NewWindow => self.state.platform.native_windows(),
            _ => true,
        };
        let selected = matches!(
            (id, self.state.brush.tool),
            (CommandId::Brush, Tool::Brush) | (CommandId::Eraser, Tool::Eraser)
        ) || (id == CommandId::ZenMode && self.state.workspace.zen_mode)
            || (id == CommandId::ToggleTheme
                && self.state.settings.theme.unwrap_or(self.system_theme) == Theme::Dark);
        (enabled, selected)
    }

    pub fn dispatch(&mut self, action: UiAction) -> Result<UiChange, String> {
        use regions::*;
        let revision = self.engine.document().revision;
        let was_expanded = self.state.customization.expanded.is_some();
        let workspace_before = matches!(
            &action,
            UiAction::Customize { .. }
                | UiAction::MovePanel { .. }
                | UiAction::MoveGroup { .. }
                | UiAction::MoveTile { .. }
                | UiAction::SelectPanelTab { .. }
                | UiAction::ResizeDock { .. }
                | UiAction::DoubleClickPanelHandle { .. }
                | UiAction::NudgeDivider { .. }
                | UiAction::PrioritizeBand { .. }
                | UiAction::Invoke {
                    command: CommandId::ResetLayout | CommandId::ZenMode | CommandId::NewToolbar
                }
        )
        .then(|| self.state.workspace.clone());
        let mut save_settings = matches!(
            &action,
            UiAction::SetTheme { .. }
                | UiAction::Invoke {
                    command: CommandId::ToggleTheme
                }
        );
        let (mut changed, wake) = match action {
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
                    self.workspace_history.begin(&self.state.workspace);
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
            UiAction::DoubleClickPanelHandle { group, viewport } => {
                valid_viewport(viewport)?;
                self.state
                    .workspace
                    .layout
                    .double_click_panel_handle(group, viewport)?;
                (LAYOUT, false)
            }
            UiAction::Customize { action } => {
                let viewport = self
                    .logical_viewport
                    .or(self.interaction.viewport)
                    .unwrap_or(self.state.camera.viewport.map(|v| v as f32));
                let changed = self.state.customization.edit(
                    &mut self.state.workspace.layout,
                    action,
                    self.state.platform,
                    viewport,
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
                return self.dispatch(control.action());
            }
            UiAction::RestoreWorkspace { workspace } => {
                workspace.validate()?;
                self.state.workspace = workspace;
                self.workspace_history = workspace::WorkspaceHistory::default();
                self.divider_drag = None;
                self.floating_resize = None;
                self.workspace_drag = None;
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
                let preset = preset(id)?;
                let brush = default_brush(preset);
                self.state.brush.preset = id;
                self.state.brush.tool = if preset == DefaultBrushPreset::Eraser {
                    Tool::Eraser
                } else {
                    Tool::Brush
                };
                self.state.brush.diameter = brush.diameter;
                self.state.brush.opacity = brush.opacity;
                self.apply_brush()?;
                (BRUSH, false)
            }
            UiAction::SetBrushSize { value } => {
                NumericControl::brush_size().validate(value, "Brush size")?;
                self.state.brush.diameter = value;
                self.apply_brush()?;
                (BRUSH, false)
            }
            UiAction::SetBrushOpacity { value } => {
                NumericControl::percent().validate(value, "Opacity")?;
                self.state.brush.opacity = value;
                self.apply_brush()?;
                (BRUSH, false)
            }
            UiAction::SetColor { rgba } => {
                for value in rgba {
                    range(value, 0.0, 1.0, "Color")?;
                }
                self.state.brush.color = rgba;
                self.apply_brush()?;
                (BRUSH, false)
            }
            UiAction::SelectLayer { id } => {
                self.require_idle()?;
                let layer = self
                    .engine
                    .document()
                    .layer(LayerId(id))
                    .ok_or("Unknown layer")?;
                if layer.kind != LayerKind::Paint {
                    return Err("Select a paint layer to draw".into());
                }
                if self.engine.document().active_layer != LayerId(id) {
                    self.engine.set_active_layer(LayerId(id)).map_err(error)?;
                }
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
                NumericControl::percent().validate(opacity, "Opacity")?;
                let id = id
                    .map(LayerId)
                    .unwrap_or(self.engine.document().active_layer);
                self.engine.set_layer_opacity(id, opacity).map_err(error)?;
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
            UiAction::SelectPanelTab { group, panel } => {
                let changed = self.state.workspace.layout.select_tab(group, panel)?;
                if !changed {
                    let action = if self.state.customization.expanded == Some(panel) {
                        CustomizationAction::CloseExpanded
                    } else {
                        CustomizationAction::ShowAllControls { panel }
                    };
                    self.state.customization.edit(
                        &mut self.state.workspace.layout,
                        action,
                        self.state.platform,
                        self.logical_viewport
                            .or(self.interaction.viewport)
                            .unwrap_or(self.state.camera.viewport.map(|v| v as f32)),
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
                        self.workspace_history.begin(&self.state.workspace);
                        self.divider_drag = Some((id, ResizeDrag::new(position, divider.bounds)));
                        (0, false)
                    } else {
                        let (_, drag) = self
                            .divider_drag
                            .filter(|(active, _)| *active == id)
                            .ok_or("Divider drag is not active")?;
                        self.state.workspace.layout.resize_workspace(
                            id,
                            drag.position(position),
                            viewport,
                        )?;
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
            self.workspace_history.record(before, &self.state.workspace);
        }
        if changed & LAYOUT != 0 {
            let layout = &self.state.workspace.layout;
            self.state.customization.expanded = self
                .state
                .customization
                .expanded
                .and_then(|panel| layout.active_panel(panel));
        }
        if was_expanded && self.state.customization.expanded.is_none() {
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
        if self.engine.document().revision != revision {
            self.refresh_document();
            changed |= DOCUMENT;
        }
        if self.refresh_commands() {
            changed |= COMMANDS;
        }
        Ok(self.changed(changed, wake))
    }

    /// Raw records retain platform timestamp/history/prediction metadata. A
    /// full queue returns the untouched record; hosts must retry after a frame.
    pub fn pen(&mut self, event: PenEvent) -> Result<(), PenEvent> {
        self.pen.push(event)?;
        self.initial_fit = false;
        self.input_pending = true;
        Ok(())
    }

    pub fn touch(&mut self, id: u64, phase: PenPhase, position: [f32; 2]) -> UiChange {
        if self.input_pending || self.engine.has_active_stroke() {
            self.touch.clear();
            return self.changed(0, false);
        }
        if self
            .touch
            .update(&mut self.state.camera, id, phase, position)
        {
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

    pub fn frame(&mut self, now_ns: u64, presentation_ns: u64) -> Result<UiChange, String> {
        let revision = self.engine.document().revision;
        self.engine
            .render_frame_for(now_ns, presentation_ns)
            .map_err(error)?;
        self.input_pending = false;
        let mut changed = 0;
        if self.engine.document().revision != revision {
            self.refresh_document();
            changed |= regions::DOCUMENT;
        }
        if self.refresh_commands() {
            changed |= regions::COMMANDS;
        }
        Ok(self.changed(changed, self.engine.has_active_stroke()))
    }

    fn invoke(&mut self, command: CommandId) -> Result<(u32, bool), String> {
        use regions::*;
        match command {
            CommandId::Brush | CommandId::Eraser => {
                self.state.brush.tool = if command == CommandId::Brush {
                    Tool::Brush
                } else {
                    Tool::Eraser
                };
                self.apply_brush()?;
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
            CommandId::AddLayer => {
                let count = self
                    .engine
                    .document()
                    .layers
                    .iter()
                    .filter(|l| l.kind == LayerKind::Paint)
                    .count();
                let index = self
                    .engine
                    .document()
                    .layers
                    .iter()
                    .position(|l| l.id == self.engine.document().active_layer)
                    .unwrap_or(0);
                let id = self
                    .engine
                    .create_paint_layer(format!("Paint {}", count + 1), index)
                    .map_err(error)?;
                self.engine.set_active_layer(id).map_err(error)?;
                Ok((0, true))
            }
            CommandId::DeleteLayer => {
                self.engine
                    .remove_layer(self.engine.document().active_layer)
                    .map_err(error)?;
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
            CommandId::ToggleTheme => {
                self.state.settings.theme = Some(if self.state.theme == Theme::Dark {
                    Theme::Light
                } else {
                    Theme::Dark
                });
                Ok((SETTINGS, true))
            }
            CommandId::ResetLayout => {
                self.state.workspace.layout.reset_docking()?;
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
                self.interaction.keep_chrome_until_contact = self.state.workspace.zen_mode;
                Ok((LAYOUT | CUSTOMIZATION, false))
            }
            CommandId::NewToolbar => {
                let changed = self.state.customization.edit(
                    &mut self.state.workspace.layout,
                    CustomizationAction::NewToolbar { group: None },
                    self.state.platform,
                    self.logical_viewport
                        .or(self.interaction.viewport)
                        .unwrap_or(self.state.camera.viewport.map(|v| v as f32)),
                )?;
                Ok((changed, false))
            }
            CommandId::ZenMode => {
                self.state.workspace.zen_mode = !self.state.workspace.zen_mode;
                self.interaction.zen_entry_guard = self.state.workspace.zen_mode
                    && self.interaction.hover.is_some_and(|[x, y]| {
                        (0.0..ZEN_CORNER_GUARD).contains(&x) && (0.0..ZEN_CORNER_GUARD).contains(&y)
                    });
                self.interaction.hidden = self.state.workspace.zen_mode;
                self.interaction.keep_chrome_until_contact = false;
                self.interaction.keyboard_chrome = false;
                Ok((LAYOUT, false))
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

    fn apply_brush(&mut self) -> Result<(), String> {
        self.cursor.hover.reset();
        let state = &self.state.brush;
        let mut brush = default_brush(preset(state.preset)?);
        brush.diameter = state.diameter;
        brush.opacity = state.opacity;
        brush.color_rgba_linear = [
            srgb_to_linear(state.color[0]),
            srgb_to_linear(state.color[1]),
            srgb_to_linear(state.color[2]),
            state.color[3],
        ];
        self.engine.set_brush(brush).map_err(error)?;
        self.engine.set_tool(if state.tool == Tool::Eraser {
            StrokeTool::Eraser
        } else {
            StrokeTool::Brush
        });
        Ok(())
    }

    fn require_idle(&self) -> Result<(), String> {
        if self.input_pending || self.engine.has_active_stroke() {
            Err("Finish the canvas interaction first".into())
        } else {
            Ok(())
        }
    }
    fn sync_camera(&mut self) {
        self.engine.set_view(
            self.state.camera.view(),
            self.state.camera.input_transform(),
        );
    }
    fn changed(&mut self, regions: u32, canvas_wake: bool) -> UiChange {
        self.state.theme = self.state.settings.theme.unwrap_or(self.system_theme);
        if regions & regions::SETTINGS != 0 {
            self.state.palette = self
                .state
                .settings
                .palette(self.state.theme, self.state.platform);
        }
        if regions != 0 {
            self.state.revision += 1;
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
            if let Some(previous) = self.state.commands.get_mut(index) {
                if previous.enabled != enabled
                    || previous.selected != selected
                    || previous.icon != icon
                {
                    previous.enabled = enabled;
                    previous.selected = selected;
                    previous.icon = icon;
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
            command.bindings = self.state.settings.keys(&command.id.shortcut_id());
            command.shortcut = self
                .state
                .settings
                .shortcut_label(&command.id.shortcut_id(), self.state.platform);
        }
    }
    fn refresh_document(&mut self) {
        let doc = self.engine.document();
        self.state.layers = doc
            .layers
            .iter()
            .map(|l| LayerState {
                id: l.id.0,
                label: l.name.to_string(),
                editable: l.kind == LayerKind::Paint,
                visible: l.visible,
                opacity: l.opacity,
                selected: l.id == doc.active_layer,
            })
            .collect();
        self.state.tabs = vec![DocumentTab {
            id: doc.id.to_string(),
            title: "Untitled".into(),
            active: true,
            width: doc.width,
            height: doc.height,
        }];
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
fn range(value: f32, min: f32, max: f32, label: &str) -> Result<(), String> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        Err(format!("{label} must be between {min} and {max}"))
    } else {
        Ok(())
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
    }
    impl CanvasRenderer for Recorder {
        type Error = BackendError;
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
            assert_eq!(menu.sections[1].len(), 3);
            assert_eq!(menu.sections[2].len(), 1);
            assert!(menu.sections[1].iter().all(|i| i.selected == Some(true)));
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
                &[Panel::Layers, Panel::Brushes]
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
            assert!(
                prompt
                    .message
                    .contains("Undo Workspace Change (Ctrl+Alt+Z)")
            );
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
    fn button_only_zen_never_reveals_on_proximity_or_consumes_drawing() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::Settings);
        edit_preference(
            &mut s,
            PreferenceId::ZenRevealMode,
            PreferenceValue::Choice(1),
        );
        assert!(matches!(s.state.requests.last().unwrap().kind,
            HostRequestKind::SaveSettings { ref settings }
                if settings.zen_reveal_mode == ZenRevealMode::Button));
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
                "all docking targets are hidden with button-only reveal"
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
    fn button_only_zen_has_the_same_policy_on_all_hosts() {
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
                    zen_reveal_mode: ZenRevealMode::Button,
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
        for index in [1, 0] {
            let menu = s.context_menu(target).unwrap();
            assert_eq!(menu.title, "Zen mode");
            assert_eq!(menu.sections.len(), 3);
            assert_eq!(
                menu.sections[0]
                    .iter()
                    .map(|i| i.label.as_str())
                    .collect::<Vec<_>>(),
                ["Reveal at screen edges", "Reveal with Zen button"]
            );
            let change = s
                .dispatch(menu.sections[0][index].action.clone().unwrap())
                .unwrap();
            assert!(!s.state.settings_open && !s.state.workspace.zen_mode);
            assert_ne!(change.regions & regions::SETTINGS, 0);
            assert_eq!(
                s.context_menu(target).unwrap().sections[0][index].selected,
                Some(true)
            );
            assert!(matches!(
                s.state.requests.last().unwrap().kind,
                HostRequestKind::SaveSettings { .. }
            ));
        }
        assert!(
            s.dispatch(UiAction::Preferences {
                action: PreferenceAction::Search {
                    query: "Zen".into(),
                }
            })
            .is_err(),
            "navigation still requires an open dialog"
        );
        for visible in [false, true] {
            let menu = s.context_menu(target).unwrap();
            assert_eq!(menu.sections[1][0].label, "Keep Zen button visible");
            s.dispatch(menu.sections[1][0].action.clone().unwrap())
                .unwrap();
            assert_eq!(s.state.settings.zen_show_button, visible);
            assert_eq!(
                s.context_menu(target).unwrap().sections[1][0].selected,
                Some(visible)
            );
        }
        let saved = s.state.settings.clone();
        let requests = s.state.requests.len();
        let menu = s.context_menu(target).unwrap();
        assert_eq!(menu.sections[2][0].label, "Change icon…");
        let change = s
            .dispatch(menu.sections[2][0].action.clone().unwrap())
            .unwrap();
        assert!(s.state.settings_open);
        let view = s.preferences().unwrap();
        assert_eq!(view.page, SettingsPage::Appearance);
        assert_eq!(view.reveal, Some(PreferenceId::ZenIcon));
        assert_eq!(s.state.settings, saved);
        assert_eq!(s.state.requests.len(), requests);
        assert!(!change.canvas_wake && !s.state.workspace.zen_mode);
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
    fn zen_reveal_and_button_visibility_are_independent() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for mode in [ZenRevealMode::Edges, ZenRevealMode::Button] {
                for show_button in [true, false] {
                    let mut s = session();
                    s.set_platform(platform);
                    s.dispatch(UiAction::RestoreSettings {
                        settings: Settings {
                            zen_reveal_mode: mode,
                            zen_show_button: show_button,
                            ..Settings::default()
                        },
                    })
                    .unwrap();
                    invoke(&mut s, CommandId::ZenMode);
                    let reply = chrome(
                        &mut s,
                        ChromeEvent::Motion {
                            position: [600.0, 450.0],
                        },
                        ChromeFacts::default(),
                    );
                    assert!(reply.chrome_hidden);
                    assert_eq!(reply.keep_zen_button, show_button);
                    assert_eq!(reply.hide_floating_panels, mode == ZenRevealMode::Button);
                    let reply = chrome(
                        &mut s,
                        ChromeEvent::Motion {
                            position: [6.0, 6.0],
                        },
                        ChromeFacts::default(),
                    );
                    assert_eq!(reply.chrome_hidden, mode == ZenRevealMode::Button);
                    assert_eq!(reply.keep_zen_button, show_button);
                    let exit = key(&mut s, "Tab", true, false, false);
                    assert!(exit.handled && !exit.chrome_hidden && !exit.hide_floating_panels);
                    assert!(!s.state.workspace.zen_mode);
                }
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
    fn hidden_tabs_preserve_style_and_restore_docking_choice_through_tear_off() {
        let viewport = [1600.0, 1200.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for style in [TabStyle::Name, TabStyle::Icon] {
                for hidden in [false, true] {
                    for destination in ["float", "edge", "merge", "cancel"] {
                        let mut app = session();
                        app.set_platform(platform);
                        let panel = Panel::Sizes;
                        let group = app.state.workspace.layout.panel_group(panel).unwrap();
                        for action in [
                            CustomizationAction::SetTabStyle {
                                target: ContextTarget::Panel { panel },
                                style,
                            },
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
                                ["Tab with name", "Tab with icon"]
                            );
                            assert_eq!(
                                menu.sections[0]
                                    .iter()
                                    .filter(|i| i.selected == Some(true))
                                    .count(),
                                1
                            );
                            assert_eq!(menu.sections[1][0].label, "Hide tab");
                            assert_eq!(menu.sections[1][0].selected, Some(hidden));
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
                        assert!(
                            floating.floating
                                && !floating.tabs_visible
                                && floating.footer_grip.is_some()
                        );
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
                        assert_eq!(config.tab_style, style);
                        assert_eq!(
                            config.hide_tab,
                            match destination {
                                "float" => true,
                                "edge" => hidden,
                                _ => false,
                            }
                        );
                        let result = app
                            .layout(viewport)
                            .groups
                            .into_iter()
                            .find(|g| g.panels.contains(&panel))
                            .unwrap();
                        assert_eq!(result.floating, destination == "float");
                        assert_eq!(result.tabs_visible, !config.hide_tab);
                        if destination == "merge" {
                            assert!(result.panels.len() > 1);
                            assert!(
                                app.context_menu(ContextTarget::Group { group: result.id })
                                    .unwrap()
                                    .sections
                                    .iter()
                                    .flatten()
                                    .all(|i| i.label != "Hide tab")
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
            drag(
                &mut app,
                ContactPhase::Move,
                [source.x + source.width + 80.0, press[1]],
            );
            assert!(app.state.workspace.layout.floating.is_empty());
            drag(
                &mut app,
                ContactPhase::Move,
                [source.x + source.width + 81.0, press[1]],
            );
            assert_eq!(app.state.workspace.layout.floating[0].root.id(), group);
            drag(&mut app, ContactPhase::Up, [700.0, 450.0]);
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
            drag(
                &mut app,
                ContactPhase::Move,
                [b.x + b.width + 101.0, b.y + 10.0],
            );
            assert_eq!(app.state.workspace.layout.floating.len(), 2);
            assert_eq!(
                app.state.workspace.layout.panel_group(Panel::Brushes),
                Some(group)
            );
            assert_ne!(
                app.state.workspace.layout.panel_group(Panel::Sizes),
                Some(group)
            );
            drag(&mut app, ContactPhase::Cancel, [0.0; 2]);
            assert_eq!(app.state.workspace, multi);
        }
    }

    #[test]
    fn docked_panel_handle_toggles_tabs_without_resizing_on_every_platform() {
        let viewport = [1200.0, 900.0];
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            let mut app = session();
            app.set_platform(platform);
            let panel = Panel::Sizes;
            let group = app.state.workspace.layout.panel_group(panel).unwrap();
            for style in [TabStyle::Name, TabStyle::Icon] {
                app.dispatch(UiAction::Customize {
                    action: CustomizationAction::SetTabStyle {
                        target: ContextTarget::Panel { panel },
                        style,
                    },
                })
                .unwrap();
                let initial = app.state.workspace.clone();
                assert_eq!(
                    initial
                        .layout
                        .panel_handle_target(DockItem::Group { group }),
                    Some(group)
                );
                assert_eq!(
                    initial
                        .layout
                        .panel_handle_target(DockItem::Panel { panel }),
                    None,
                    "Tab labels retain their own activation behavior"
                );
                for hidden in [true, false] {
                    let before = app.state.workspace.clone();
                    let change = app
                        .dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                        .unwrap();
                    assert!(!change.canvas_wake);
                    let after = app.state.workspace.clone();
                    let mut expected = before.clone();
                    expected.layout.panel_mut(panel).unwrap().hide_tab = hidden;
                    assert_eq!(
                        after, expected,
                        "Only tab visibility changes, not dock dimensions or tab style"
                    );
                    let resolved = app
                        .layout(viewport)
                        .groups
                        .into_iter()
                        .find(|g| g.id == group)
                        .unwrap();
                    assert_eq!(resolved.tabs_visible, !hidden);
                    assert!(!resolved.floating);
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
            }
            // A docked toolbar accepts the handle but never toggles its tab.
            let toolbar = app
                .state
                .workspace
                .layout
                .panel_group(Panel::Toolbar)
                .unwrap();
            assert_eq!(
                app.state
                    .workspace
                    .layout
                    .panel_handle_target(DockItem::Group { group: toolbar }),
                Some(toolbar)
            );
            assert_eq!(
                app.state
                    .workspace
                    .layout
                    .panel_handle_target(DockItem::Panel {
                        panel: Panel::Toolbar
                    }),
                Some(toolbar)
            );
            let before = app.state.workspace.clone();
            app.dispatch(UiAction::DoubleClickPanelHandle {
                group: toolbar,
                viewport,
            })
            .unwrap();
            assert_eq!(app.state.workspace, before);
            app.dispatch(UiAction::MovePanel {
                panel: Panel::Brushes,
                viewport,
                target: DockTarget::Tab { group, index: None },
            })
            .unwrap();
            assert_eq!(
                app.state
                    .workspace
                    .layout
                    .panel_handle_target(DockItem::Group { group }),
                None
            );
            let before = app.state.workspace.clone();
            app.dispatch(UiAction::DoubleClickPanelHandle { group, viewport })
                .unwrap();
            assert_eq!(app.state.workspace, before);
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
                            step == 1
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
        value["layout"]["panels"].as_array_mut().unwrap().pop();
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
                ["fit_canvas"],
                ["zen_mode", "toggle_theme"],
                ["reset_layout"]
            ])
        );
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
            tile: tiles[6].id,
        })
        .unwrap();
        assert_eq!(app.state.brush.diameter, 64.0);
        assert!(
            app.panel_view(Panel::Toolbar).unwrap().tiles[6]
                .choice
                .selected
        );
        app.dispatch(UiAction::ActivateTile {
            panel: Panel::Toolbar,
            tile: tiles[7].id,
        })
        .unwrap();
        assert!(
            app.panel_view(Panel::Toolbar).unwrap().tiles[7]
                .choice
                .selected
        );
        app.dispatch(UiAction::Customize {
            action: CustomizationAction::RemoveTool {
                panel: Panel::Toolbar,
                tile: tiles[6].id,
            },
        })
        .unwrap();
        assert!(
            app.dispatch(UiAction::ActivateTile {
                panel: Panel::Toolbar,
                tile: tiles[6].id
            })
            .is_err()
        );
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
            "B / Ctrl+Y",
            "confirmed bindings must take effect immediately"
        );
        assert_eq!(
            s.state.settings.keys(&CommandId::Redo.shortcut_id()).len(),
            1,
            "preserve Redo's other accelerator"
        );
        s.dispatch(UiAction::CloseSettings).unwrap();
        assert_eq!(s.command(CommandId::Brush).shortcut, "B / Ctrl+Y");
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
        let target = CommandId::Brush.shortcut_id();
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
        record_shortcut(&mut s, "custom.size-42", "j", false);
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
        record_shortcut(&mut s, "canvas.pan", "g", false);
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
        s.dispatch(UiAction::CloseSettings).unwrap();
        assert!(
            !key(&mut s, "j", true, false, true).handled,
            "native text editing wins"
        );
        key(&mut s, "j", false, false, true);
        assert!(key(&mut s, "j", true, false, false).handled);
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
            record_shortcut(&mut s, &id, "j", false);
            preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
            preference(
                &mut s,
                PreferenceAction::SearchShortcuts { query: "j".into() },
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
        let id = CommandId::Brush.shortcut_id();
        preference(&mut s, PreferenceAction::EditShortcut { id: id.clone() });
        let modified = |s: &UiSession<Recorder>| {
            let view = s.preferences().unwrap();
            let modified = view.shortcuts.iter().find(|r| r.id == id).unwrap().modified;
            assert_eq!(view.shortcut_editor.unwrap().modified, modified);
            modified
        };
        assert!(!modified(&s));
        record_shortcut(&mut s, &id, "j", false);
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
            let id = CommandId::Brush.shortcut_id();
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
