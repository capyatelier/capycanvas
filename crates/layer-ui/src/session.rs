use crate::interaction::{Interaction, PointerContact};
use crate::layout::ResizeDrag;
use crate::*;
use layer_core::{Document, LayerId, LayerKind, StrokeTool, default_brush};
use layer_engine::{CanvasEngine, InputProducer, PenEvent, PenPhase, PressureCurve, input_queue};
use layer_render::CanvasRenderer;

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
                settings_draft: None,
                preferences: PreferencesState::default(),
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
        self.refresh_commands();
        self.refresh_shortcuts();
    }
    pub fn preferences(&self) -> Option<PreferencesView> {
        self.state.settings_draft.as_ref().map(|draft| {
            self.state
                .preferences
                .view(draft, &self.state.settings, self.state.platform)
        })
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
        let event = self.cursor.event?;
        if self.interaction.pan_key.is_some()
            || self.interaction.pointer.is_some_and(|p| !p.paint)
            || self.interaction.facts.popup_open
            || self.state.settings_draft.is_some()
            || self.touch.is_active()
        {
            return None;
        }
        let scale = self
            .logical_viewport
            .map_or(1.0, |v| self.state.camera.viewport[0] as f32 / v[0]);
        let dabs =
            self.engine
                .cursor_contacts(event, &mut self.cursor.hover, self.cursor.origin_ns);
        Some(self.cursor.view(
            self.engine.backend(),
            &self.engine.brush().tip,
            &dabs,
            &self.state.camera,
            scale,
            self.state.settings.cursor,
        ))
    }
    /// Small event/reply boundary shared by native and Wasm hosts. Pen samples
    /// are only queued when `paint` is true, without serializing UiState.
    pub fn input(&mut self, input: UiInput) -> Result<InputReply, String> {
        let mut reply = InputReply::default();
        let mut contact = None;
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
                self.interaction.facts = facts;
                self.interaction.viewport = Some(viewport);
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
                                self.state.settings_draft.as_ref().unwrap(),
                                KeyChord::new(&key, modifiers),
                                self.state.platform,
                            );
                        }
                        reply.change = self.changed(regions::SETTINGS, false);
                        reply.handled = true;
                        reply.chrome_hidden = false;
                        return Ok(reply);
                    }
                    if matches!(key.as_str(), "tab" | "escape") {
                        self.interaction.keyboard_chrome = true;
                        reply.dismiss_popups = key == "escape";
                    }
                    let blocked = editing
                        || self.state.settings_draft.is_some()
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
                    if self.interaction.pointer.is_none() && self.state.settings_draft.is_none() {
                        reply.change = self.touch(id, pen_phase(phase), position);
                        reply.handled = true;
                    }
                } else {
                    let paint =
                        button == PointerButton::Primary && self.interaction.pan_key.is_none();
                    if phase == ContactPhase::Down
                        && self.interaction.pointer.is_none()
                        && self.state.settings_draft.is_none()
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
                self.interaction.facts.dragging = false;
                self.interaction.hover = None;
                self.divider_drag = None;
                self.touch.clear();
            }
        }
        self.refresh_chrome();
        if let Some((was_hidden, popup_open)) = contact {
            reply.dismiss_popups = popup_open;
            reply.handled = popup_open || (was_hidden && !self.interaction.hidden);
        }
        reply.chrome_hidden = self.interaction.hidden;
        reply.pan_cursor = self.interaction.pan_key.is_some();
        Ok(reply)
    }

    fn refresh_chrome(&mut self) {
        let pinned = self.interaction.facts.held
            || self.interaction.facts.dragging
            || self.interaction.facts.popup_open
            || self.interaction.keyboard_chrome
            || self.state.settings_draft.is_some()
            || self.divider_drag.is_some();
        if !self.state.workspace.zen_mode || pinned {
            self.interaction.hidden = false;
        } else if self.interaction.pointer.is_none()
            && !self.input_pending
            && !self.engine.has_active_stroke()
        {
            let near = self
                .interaction
                .viewport
                .zip(self.interaction.hover)
                .is_some_and(|(viewport, position)| {
                    self.layout(viewport).near_chrome_with_distances(
                        position,
                        viewport,
                        self.interaction.hidden,
                        self.state.settings.zen_reveal,
                        self.state.settings.zen_hide,
                    )
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
    /// A preview is offered only when the same transactional move will succeed.
    /// Measured native tab rectangles are input, not host-side docking policy.
    pub fn drop_hint(
        &self,
        viewport: [f32; 2],
        position: [f32; 2],
        tabs: &[TabHit],
        item: DockItem,
    ) -> Option<DropHint> {
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
        let hint = self
            .layout(viewport)
            .drop_hint(position[0], position[1], tabs);
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
            icon: id.icon(),
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
            || (id == CommandId::TogglePanels && self.state.workspace.layout.panels_visible)
            || (id == CommandId::ToggleTheme
                && self.state.settings.theme.unwrap_or(self.system_theme) == Theme::Dark);
        (enabled, selected)
    }

    pub fn dispatch(&mut self, action: UiAction) -> Result<UiChange, String> {
        use regions::*;
        let revision = self.engine.document().revision;
        let save_settings = matches!(
            &action,
            UiAction::ApplySettings
                | UiAction::SetTheme { .. }
                | UiAction::Invoke {
                    command: CommandId::ToggleTheme
                }
        );
        let (mut changed, wake) = match action {
            UiAction::RestoreWorkspace { workspace } => {
                workspace.validate()?;
                self.state.workspace = workspace;
                self.divider_drag = None;
                (LAYOUT, false)
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
                BRUSH_SIZE_CONTROL.validate(value, "Brush size")?;
                self.state.brush.diameter = value;
                self.apply_brush()?;
                (BRUSH, false)
            }
            UiAction::SetBrushOpacity { value } => {
                OPACITY_CONTROL.validate(value, "Opacity")?;
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
                OPACITY_CONTROL.validate(opacity, "Opacity")?;
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
                self.state.workspace.layout.select_tab(group, panel)?;
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
                    }
                    (0, false)
                } else {
                    valid_viewport(viewport)?;
                    if !position.into_iter().all(f32::is_finite) {
                        return Err("Invalid divider position".into());
                    }
                    if phase == ContactPhase::Down {
                        let divider = self.divider(id, viewport)?;
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
                if self.state.settings_draft.is_none() {
                    return Err("Settings are not open".into());
                }
                self.state.settings_draft = Some(settings);
                (SETTINGS, false)
            }
            UiAction::OpenSettings { page } => {
                self.open_settings(page);
                (SETTINGS, false)
            }
            UiAction::Preferences { action } => {
                let draft = self
                    .state
                    .settings_draft
                    .as_mut()
                    .ok_or("Preferences are not open")?;
                self.state
                    .preferences
                    .edit(draft, action, self.state.platform);
                (SETTINGS, false)
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
            UiAction::ApplySettings => {
                let settings = self
                    .state
                    .settings_draft
                    .clone()
                    .ok_or("Settings are not open")?;
                settings.validate()?;
                self.apply_settings(settings)?;
                self.state.settings_draft = None;
                self.state.preferences = PreferencesState::default();
                (SETTINGS | COMMANDS, true)
            }
            UiAction::CancelSettings => {
                self.state.settings_draft = None;
                self.state.preferences = PreferencesState::default();
                (SETTINGS, false)
            }
        };
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
                self.open_settings(match command {
                    CommandId::KeyboardShortcuts => SettingsPage::Shortcuts,
                    CommandId::About => SettingsPage::About,
                    _ => SettingsPage::Appearance,
                });
                Ok((SETTINGS, false))
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
                self.state.workspace.layout = DockLayout::default();
                Ok((LAYOUT, false))
            }
            CommandId::TogglePanels => {
                self.state.workspace.layout.panels_visible =
                    !self.state.workspace.layout.panels_visible;
                Ok((LAYOUT, false))
            }
            CommandId::ZenMode => {
                self.state.workspace.zen_mode = !self.state.workspace.zen_mode;
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
        let draft = self
            .state
            .settings_draft
            .get_or_insert_with(|| self.state.settings.clone());
        self.state
            .preferences
            .edit(draft, PreferenceAction::Page { page }, self.state.platform);
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
            if let Some(previous) = self.state.commands.get_mut(index) {
                if previous.enabled != enabled || previous.selected != selected {
                    previous.enabled = enabled;
                    previous.selected = selected;
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
        s.dispatch(UiAction::CancelSettings).unwrap();
        assert!(chrome(&mut s, ChromeEvent::Refresh, facts).chrome_hidden);
        assert!(!key(&mut s, "Tab", true, false, false).chrome_hidden);
        assert!(chrome(&mut s, motion([600.0, 450.0]), facts).chrome_hidden);
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
        assert!(key(&mut s, "z", true, false, false).handled);
        assert!(s.state.workspace.zen_mode);
        key(&mut s, "z", true, false, false);
        assert!(s.state.workspace.zen_mode, "repeat cannot retoggle Zen");
        key(&mut s, "z", false, false, false);
        key(&mut s, "z", true, false, false);
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
        assert!(!key(&mut s, "b", true, false, false).handled);
        s.dispatch(UiAction::CancelSettings).unwrap();
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
        for visible in [true, false] {
            if !visible {
                invoke(&mut source, CommandId::TogglePanels);
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
            assert!(restored.command(CommandId::ZenMode).selected);
            assert_eq!(restored.command(CommandId::TogglePanels).selected, visible);
            for viewport in [[1200.0, 900.0], [680.0, 480.0], [900.0, 1200.0]] {
                assert_eq!(
                    serde_json::to_value(source.layout(viewport)).unwrap(),
                    serde_json::to_value(restored.layout(viewport)).unwrap()
                );
            }
            // The saved ID allocator remains safe for subsequent editing.
            if !visible {
                invoke(&mut restored, CommandId::TogglePanels);
            }
            restored
                .dispatch(UiAction::MovePanel {
                    panel: Panel::Sizes,
                    target: DockTarget::Edge {
                        edge: Edge::Bottom,
                        outer: false,
                    },
                    viewport,
                })
                .unwrap();
            restored.state.workspace.validate().unwrap();
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
        value["layout"]["bands"].as_array_mut().unwrap().pop();
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
            app.drop_hint(viewport, [f32::NAN, 10.0], &[], item)
                .is_none()
        );
        assert!(app.drop_hint(viewport, [-1.0, 10.0], &[], item).is_none());
        assert!(
            app.drop_hint(
                viewport,
                [1000.0, 400.0],
                &[],
                DockItem::Group { group: u32::MAX }
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
    fn zen_and_panels_do_not_move_camera_or_request_canvas_work() {
        let mut s = session();
        s.set_viewport([1000.0; 2], [1000; 2]).unwrap();
        let camera = s.state.camera.clone();
        let layout = s.state.workspace.layout.clone();
        for command in [CommandId::ZenMode, CommandId::TogglePanels] {
            let change = invoke(&mut s, command);
            assert!(!change.canvas_wake);
            assert_eq!(s.state.camera.translation, camera.translation);
            assert_eq!(s.state.camera.revision, camera.revision);
            assert_eq!(s.state.camera.zoom, camera.zoom);
        }
        assert!(s.state.workspace.zen_mode);
        assert!(s.command(CommandId::ZenMode).selected);
        assert!(!s.state.workspace.layout.panels_visible);
        assert!(
            s.state
                .workspace
                .layout
                .resolve(1000.0, 1000.0)
                .groups
                .is_empty()
        );
        assert_eq!(s.state.workspace.layout.bands, layout.bands);
        invoke(&mut s, CommandId::TogglePanels);
        assert_eq!(s.state.workspace.layout, layout);
        assert_eq!(s.state.camera.translation, camera.translation);
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
        key(&mut s, " ", false, false, false);
        assert!(s.canvas_cursor().is_some());
        invoke(&mut s, CommandId::Settings);
        assert!(s.canvas_cursor().is_none());
        let settings = Settings {
            cursor: CursorMode::Cross,
            ..Settings::default()
        };
        s.dispatch(UiAction::EditSettings { settings }).unwrap();
        s.dispatch(UiAction::ApplySettings).unwrap();
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
        assert_eq!(
            s.canvas_cursor().unwrap().outline,
            "M92.50 155.00L122.50 155.00L92.50 145.00L92.50 155.00Z"
        );
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
        for sections in MENUS.iter().map(|m| m.sections).chain([PRIMARY_MENU]) {
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
                ["zen_mode", "toggle_theme", "toggle_panels"],
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
        for control in [
            catalog.brush_size,
            catalog.brush_size_slider,
            catalog.opacity,
            catalog.pressure,
        ] {
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
        assert_eq!(app.state.theme, Theme::Light);
        check_menu(&app.state);
        app.dispatch(UiAction::ApplySettings).unwrap();
        assert_eq!(app.state.theme, Theme::Dark);
        assert_eq!(app.state.settings.theme, None);
        check_menu(&app.state);
    }
    #[test]
    fn settings_are_transactional_and_dismissible() {
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
        assert_eq!(app.state.settings, Settings::default());
        app.dispatch(UiAction::CancelSettings).unwrap();
        assert!(app.state.settings_draft.is_none());
        assert_eq!(app.state.settings, Settings::default());
        invoke(&mut app, CommandId::Settings);
        app.dispatch(UiAction::EditSettings {
            settings: settings.clone(),
        })
        .unwrap();
        app.dispatch(UiAction::ApplySettings).unwrap();
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
                        "Source Code",
                        "github.com/capyatelier/capycanvas",
                        "https://github.com/capyatelier/capycanvas"
                    ),
                ]
            );
            for id in [PreferenceId::Website, PreferenceId::SourceCode] {
                edit_preference(&mut s, id, PreferenceValue::Choice(0));
                assert!(s.preferences().unwrap().error.is_some());
                assert!(!s.preferences().unwrap().dirty);
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
                ["Source Code"]
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
        assert_eq!(s.state.settings_draft.as_ref().unwrap().pressure_gamma, 1.7);
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
        let before = s.state.settings_draft.clone();
        edit_preference(
            &mut s,
            PreferenceId::PredictionHorizon,
            PreferenceValue::Number(20.0),
        );
        assert!(s.preferences().unwrap().error.is_some());
        assert_eq!(s.state.settings_draft, before);
        edit_preference(
            &mut s,
            PreferenceId::Pressure,
            PreferenceValue::Number(f32::NAN),
        );
        assert_eq!(s.state.settings_draft, before);
        edit_preference(&mut s, PreferenceId::Theme, PreferenceValue::Choice(99));
        assert_eq!(s.state.settings_draft, before);
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
        assert_eq!(old.zen_reveal, 80.0);
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
        assert_eq!(
            s.state.settings_draft.as_ref().unwrap().theme,
            Some(Theme::Dark)
        );
        assert_eq!(s.state.settings.theme, None);
        s.dispatch(UiAction::CancelSettings).unwrap();
        assert!(s.state.requests.is_empty());
    }
    #[test]
    fn shortcut_conflicts_require_explicit_replacement_and_update_menu_hints_on_apply() {
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
        let before = s.state.settings_draft.clone();
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
        assert_eq!(s.state.settings_draft, before);
        assert!(s.preferences().unwrap().error.is_some());
        preference(&mut s, PreferenceAction::ConfirmShortcut { replace: true });
        assert_eq!(
            s.command(CommandId::Brush).shortcut,
            "B",
            "draft must not change live bindings"
        );
        assert_eq!(
            s.state
                .settings_draft
                .as_ref()
                .unwrap()
                .keys(&CommandId::Redo.shortcut_id())
                .len(),
            1,
            "preserve Redo's other accelerator"
        );
        s.dispatch(UiAction::ApplySettings).unwrap();
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
        assert!(
            s.state
                .settings_draft
                .as_ref()
                .unwrap()
                .shortcuts
                .is_empty()
        );
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
            s.state.settings_draft.is_some(),
            "Escape only closes the recording sheet"
        );
        record_shortcut(&mut s, &target, "tab", false);
        assert!(s.preferences().unwrap().capture.unwrap().error.is_some());
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
        assert!(
            s.state
                .settings_draft
                .as_ref()
                .unwrap()
                .keys(&target)
                .is_empty()
        );
        preference(
            &mut s,
            PreferenceAction::ResetShortcut { id: target.clone() },
        );
        assert_eq!(
            s.state.settings_draft.as_ref().unwrap().keys(&target).len(),
            1
        );
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
        s.dispatch(UiAction::ApplySettings).unwrap();
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
        for platform in [Platform::Gtk, Platform::Web] {
            let mut s = session();
            s.set_platform(platform);
            for points in [9, 11, 13] {
                let json = format!(r#"{{"panel_text_pt":{points},"pressure_gamma":1.5}}"#);
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
                assert!(
                    !serde_json::to_value(native)
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .contains_key("panel_text_pt")
                );
            }
            invoke(&mut s, CommandId::Settings);
            assert!(
                !s.preferences()
                    .unwrap()
                    .pages
                    .iter()
                    .flat_map(|p| &p.groups)
                    .flat_map(|g| &g.rows)
                    .any(|r| r.title == "Panel text size")
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
            let before = s.state.settings_draft.clone();
            preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
            assert_eq!(
                s.state.settings_draft, before,
                "duplicate addition is atomic"
            );
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
            s.state
                .settings_draft
                .as_ref()
                .unwrap()
                .feedback_config()
                .validate()
                .unwrap();
            let before = s.state.settings_draft.clone();
            edit_preference(
                &mut s,
                PreferenceId::PredictionHorizon,
                PreferenceValue::Number(65.0),
            );
            assert!(s.preferences().unwrap().error.is_some());
            assert_eq!(s.state.settings_draft, before);
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
        s.dispatch(UiAction::ApplySettings).unwrap();
        assert_eq!(
            s.state.settings.feedback_config().prediction_horizon_micros,
            4000
        );
        assert_eq!(s.state.requests.len(), 2);
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
        assert_eq!(s.state.requests.len(), 1);
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
