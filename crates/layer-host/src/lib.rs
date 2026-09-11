//! Shared transport facade for native hosts. No UI toolkit or surface ownership.
//! Call from one engine/render owner; platform callbacks enqueue owned batches.
mod renderer;
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_render::CanvasRenderer;
use layer_ui::{ContactPhase, PointerButton, PointerKind, UiAction, UiInput, UiSession};
pub use renderer::Renderer;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(PartialEq)]
struct SnapshotKey {
    revision: u64,
    logical: [f32; 2],
    chrome_hidden: bool,
    hide_floating_panels: bool,
    keep_zen_button: bool,
    gpu_ready: bool,
    startup: layer_render_wgpu::StartupProgress,
    error: Option<String>,
}

/// Owned by the caller for this call. The camera revision is captured with the
/// samples, not looked up after they have waited in a platform work queue.
pub struct PointerBatch<'a> {
    pub id: u64,
    pub tool: u8,
    pub button: u8,
    pub records: &'a [f64],
    pub predicted: bool,
    pub view_revision: u64,
}

pub struct NativeHost {
    pub session: UiSession<Renderer>,
    pub logical: [f32; 2],
    pub dirty: bool,
    pub chrome_hidden: bool,
    hide_floating_panels: bool,
    keep_zen_button: bool,
    pub error: Option<String>,
    pub sequence: u64,
    pub startup: layer_render_wgpu::StartupProgress,
    deferred_contacts: std::collections::BTreeSet<u64>,
    last_pen: Option<PenEvent>,
    last_snapshot: Option<SnapshotKey>,
    last_camera_revision: Option<u64>,
    document_view_revision: u64,
    last_durable_workspace: Option<layer_ui::WorkspaceState>,
}

impl NativeHost {
    pub fn new(platform: layer_ui::Platform) -> Result<Self, String> {
        let mut session = UiSession::blank(Renderer::default(), [1, 1])?;
        session.set_platform(platform);
        Ok(Self {
            session,
            logical: [1.0, 1.0],
            dirty: true,
            chrome_hidden: false,
            hide_floating_panels: false,
            keep_zen_button: true,
            error: None,
            sequence: 0,
            // Eager hosts are ready on GPU attachment; staged hosts reset this.
            startup: layer_render_wgpu::StartupProgress {
                canvas_ready: true,
                brush_ready: true,
                complete: true,
            },
            deferred_contacts: Default::default(),
            last_pen: None,
            last_snapshot: None,
            last_camera_revision: None,
            document_view_revision: 0,
            last_durable_workspace: None,
        })
    }
    /// Invalidate host input and snapshot caches after shared document adoption.
    pub fn document_adopted(&mut self) {
        self.deferred_contacts.clear();
        self.last_pen = None;
        self.last_snapshot = None;
        self.last_camera_revision = None;
        self.document_view_revision = self.session.state().camera.revision;
        self.dirty = true;
    }
    pub fn resize(&mut self, width: u32, height: u32, density: f32) -> Result<(), String> {
        if width == 0 || height == 0 || !density.is_finite() || density <= 0.0 {
            return Err("Invalid native surface dimensions".into());
        }
        self.logical = [width as f32 / density, height as f32 / density];
        self.session.set_viewport(self.logical, [width, height])?;
        self.dirty = true;
        Ok(())
    }
    /// Shared staged GPU lifecycle. Presenters submit the first paper frame
    /// before passing `has_presented = true`; it must not consume document replay.
    pub fn prepare_canvas_frame(
        &mut self,
        now: u64,
        presentation: u64,
        has_presented: bool,
    ) -> Result<(), String> {
        if has_presented || self.startup.complete {
            let engine = self.session.engine();
            let gpu = engine
                .backend()
                .0
                .as_ref()
                .ok_or("Missing native renderer")?;
            let transform = engine.transform_preview().is_some();
            if gpu.startup_needs_update(engine.document(), engine.brush(), transform) {
                let (document, brush) = (engine.document().clone(), engine.brush().clone());
                self.session
                    .renderer_mut()
                    .0
                    .as_mut()
                    .unwrap()
                    .prepare_startup(&document, &brush, transform)
                    .map_err(|e| e.to_string())?;
            }
            self.startup = self
                .session
                .renderer_mut()
                .0
                .as_mut()
                .unwrap()
                .poll_startup()
                .map_err(|e| e.to_string())?;
            if self.startup.canvas_ready {
                let previous = self.session.state().revision;
                let change = self.session.frame(now, presentation)?;
                self.dirty = change.canvas_wake;
                self.apply_change(previous, change);
            }
        } else {
            let view = self.session.state().camera.view();
            let document = self.session.engine().document();
            let extent = [document.width, document.height];
            let layers: Vec<_> = document
                .layers
                .iter()
                .filter(|l| l.kind == layer_core::LayerKind::Background)
                .cloned()
                .collect();
            self.session
                .renderer_mut()
                .0
                .as_mut()
                .ok_or("Missing native renderer")?
                .submit(layer_render::FramePacket {
                    time_seconds: 0.,
                    view,
                    document_extent: extent,
                    layers: &layers,
                    dabs: &[],
                    dab_batches: &[],
                    reset_layers: true,
                    composite_all: true,
                })
                .map_err(|e| e.to_string())?;
        }
        self.dirty |= !self.startup.complete;
        Ok(())
    }
    pub fn dispatch(&mut self, action: UiAction) -> Result<(), String> {
        let previous = self.session.state().revision;
        let change = self.session.dispatch(action)?;
        self.apply_change(previous, change);
        Ok(())
    }
    pub fn import_layer_image(
        &mut self,
        name: &str,
        image: layer_render::HostImage<'_>,
    ) -> Result<(), String> {
        self.session.import_layer_image(name, image)?;
        self.dirty = true;
        Ok(())
    }
    /// Camera motion changes the core revision without changing the workspace
    /// models. Only acknowledge it if no unpublished structural change precedes
    /// it; otherwise the next snapshot must still include that pending change.
    pub fn apply_change(&mut self, previous: u64, change: layer_ui::UiChange) {
        self.dirty |= change.canvas_wake;
        if change.regions == layer_ui::regions::CAMERA
            && let Some(key) = &mut self.last_snapshot
            && key.revision == previous
        {
            key.revision = change.revision;
        }
    }
    pub fn input(&mut self, input: UiInput) -> Result<layer_ui::InputReply, String> {
        let previous = self.session.state().revision;
        let reply = self.session.input(input)?;
        self.chrome_hidden = reply.chrome_hidden;
        self.hide_floating_panels = reply.hide_floating_panels;
        self.keep_zen_button = reply.keep_zen_button;
        self.apply_change(previous, reply.change);
        if reply.cancel_paint {
            self.cancel_pen()?;
        }
        Ok(reply)
    }
    pub fn scroll(
        &mut self,
        anchor: [f32; 2],
        delta: [f32; 2],
        density: f32,
        zoom: bool,
        horizontal: bool,
    ) -> Result<(), String> {
        if !anchor
            .into_iter()
            .chain(delta)
            .chain([density])
            .all(f32::is_finite)
            || density <= 0.0
        {
            return Err("Invalid native scroll".into());
        }
        let previous = self.session.state().revision;
        let change = self
            .session
            .scroll(anchor, delta, density, zoom, horizontal)?;
        self.apply_change(previous, change);
        Ok(())
    }
    pub fn gesture(&mut self, anchor: [f32; 2], scale: f32, rotation: f32) -> Result<(), String> {
        if !anchor
            .into_iter()
            .chain([scale, rotation])
            .all(f32::is_finite)
            || scale <= 0.0
        {
            return Err("Invalid native gesture".into());
        }
        let previous = self.session.state().revision;
        let change = self.session.gesture(anchor, anchor, scale, rotation)?;
        self.apply_change(previous, change);
        Ok(())
    }
    fn cancel_pen(&mut self) -> Result<(), String> {
        if let Some(mut event) = self.last_pen.take() {
            event.phase = PenPhase::Cancel;
            self.sequence += 1;
            event.sequence = self.sequence;
            self.enqueue(event)?;
        }
        Ok(())
    }
    fn enqueue(&mut self, event: PenEvent) -> Result<(), String> {
        if let Err(event) = self.session.pen(event) {
            // The sole render owner may drain a full input queue. The platform
            // UI thread never waits here, and a stroke boundary is never dropped.
            self.session.frame(event.timestamp_ns, event.timestamp_ns)?;
            self.session
                .pen(event)
                .map_err(|_| "Pen queue remained full")?;
        }
        self.dirty = true;
        Ok(())
    }
    /// Records: x/y, pressure, tilt x/y, twist, distance, monotonic ns, phase.
    /// Phase 0 hover, 1 down, 2 move, 3 up, 4 cancel. Tool 0 pen, 1 mouse,
    /// 2 eraser, 3 touch. Pointer routing is decided by the shared core.
    pub fn pointer(
        &mut self,
        id: u64,
        tool: u8,
        button: u8,
        records: &[f64],
        predicted: bool,
    ) -> Result<(), String> {
        self.pointer_batch(PointerBatch {
            id,
            tool,
            button,
            records,
            predicted,
            view_revision: self.session.state().camera.revision,
        })
    }

    pub fn accepts_pointer_input(&self, view_revision: u64) -> bool {
        view_revision >= self.document_view_revision
            && !self.session.state().document_file.close_ready
    }
    pub fn pointer_batch(&mut self, batch: PointerBatch<'_>) -> Result<(), String> {
        self.pointer_batch_updates(batch, &[], false)
    }
    /// Optional pairs per sample: contact-local estimate token and whether
    /// further corrections are expected (0/1). Corrections bypass UI routing
    /// and pointer ownership: they refer to previously admitted paint only.
    pub fn pointer_batch_updates(
        &mut self,
        batch: PointerBatch<'_>,
        updates: &[u64],
        correction: bool,
    ) -> Result<(), String> {
        let PointerBatch {
            id,
            tool,
            button,
            records,
            predicted,
            view_revision,
        } = batch;
        if records.is_empty()
            || !records.len().is_multiple_of(9)
            || !records.iter().all(|n| n.is_finite())
            || records
                .chunks_exact(9)
                .any(|r| r[7] < 0.0 || r[8] < 0.0 || r[8] > 4.0 || r[8].fract() != 0.0)
        {
            return Err("Invalid native pointer batch".into());
        }
        if (!updates.is_empty() && updates.len() != records.len() / 9 * 2)
            || updates
                .chunks_exact(2)
                .any(|u| u[1] > 1 || (u[1] != 0 && u[0] == 0))
            || (correction
                && (predicted || updates.is_empty() || updates.chunks_exact(2).any(|u| u[0] == 0)))
            || (predicted && !updates.is_empty())
        {
            return Err("Invalid native input estimates".into());
        }
        if !self.accepts_pointer_input(view_revision) {
            return Ok(());
        }
        for (index, sample) in records.chunks_exact(9).enumerate() {
            let update = updates.get(index * 2..index * 2 + 2).unwrap_or(&[0, 0]);
            let phase = match sample[8] as u8 {
                0 => PenPhase::Hover,
                1 => PenPhase::Down,
                2 => PenPhase::Move,
                3 => PenPhase::Up,
                _ => PenPhase::Cancel,
            };
            let position = [sample[0] as f32, sample[1] as f32];
            let event = PenEvent {
                device_id: id,
                sequence: if update[0] != 0 {
                    update[0]
                } else {
                    self.sequence + 1
                },
                timestamp_ns: sample[7] as u64,
                view_revision,
                surface_position: Point {
                    x: position[0],
                    y: position[1],
                },
                pressure: sample[2] as f32,
                tilt_radians: [sample[3] as f32, sample[4] as f32],
                twist_radians: sample[5] as f32,
                distance: sample[6] as f32,
                phase,
                tool: match tool {
                    1 => ToolKind::Mouse,
                    2 => ToolKind::Eraser,
                    3 => ToolKind::Finger,
                    _ => ToolKind::Pen,
                },
                flags: SampleFlags(
                    SampleFlags::PRIMARY.0
                        | if correction {
                            SampleFlags::CORRECTION.0
                        } else {
                            0
                        }
                        | if update[1] != 0 {
                            SampleFlags::ESTIMATED.0
                        } else {
                            0
                        }
                        | if predicted {
                            SampleFlags::PREDICTED.0
                        } else {
                            0
                        },
                ),
            };
            if correction {
                // Corrections refer to previously admitted sample tokens and never
                // enter UI pointer ownership or replace the current cursor.
                self.sequence += 1;
                self.enqueue(event)?;
                continue;
            }
            self.pointer_event_inner(
                event,
                match button {
                    0 => PointerButton::Primary,
                    1 => PointerButton::Pan,
                    _ => PointerButton::Other,
                },
                update[0] != 0,
            )?;
        }
        Ok(())
    }
    /// Route a validated typed platform sample through shared input policy.
    /// Capture-time coordinates, axes, timestamps and flags remain unchanged.
    /// The host assigns engine sequence numbers, including inserted cancellations.
    pub fn pointer_event(&mut self, event: PenEvent, button: PointerButton) -> Result<(), String> {
        if !self.accepts_pointer_input(event.view_revision) {
            return Ok(());
        }
        self.pointer_event_inner(event, button, false)
    }
    fn pointer_event_inner(
        &mut self,
        mut event: PenEvent,
        button: PointerButton,
        preserve_token: bool,
    ) -> Result<(), String> {
        let id = event.device_id;
        let phase = event.phase;
        let predicted = event.flags.0 & SampleFlags::PREDICTED.0 != 0;
        let touch = event.tool == ToolKind::Finger;
        let contact = match phase {
            PenPhase::Hover => None,
            PenPhase::Down => Some(ContactPhase::Down),
            PenPhase::Move => Some(ContactPhase::Move),
            PenPhase::Up => Some(ContactPhase::Up),
            PenPhase::Cancel => Some(ContactPhase::Cancel),
        };
        let paint = if predicted {
            self.last_pen.is_some_and(|p| p.device_id == id) && !touch
        } else if let Some(phase) = contact {
            self.input(UiInput::Pointer {
                id,
                phase,
                kind: if touch {
                    PointerKind::Touch
                } else if event.tool == ToolKind::Mouse {
                    PointerKind::Mouse
                } else {
                    PointerKind::Pen
                },
                button,
                position: [event.surface_position.x, event.surface_position.y],
            })?
            .paint
        } else {
            false
        };
        if !preserve_token {
            event.sequence = self.sequence + 1;
        }
        if !touch && !predicted {
            self.session.cursor_input(if phase == PenPhase::Cancel {
                None
            } else {
                Some(event)
            });
            self.dirty = true;
        }
        if !predicted && phase == PenPhase::Down {
            if self.paint_ready() {
                self.deferred_contacts.remove(&id);
            } else {
                self.deferred_contacts.insert(id);
            }
        }
        let preparing = self.deferred_contacts.contains(&id);
        if !predicted && matches!(phase, PenPhase::Up | PenPhase::Cancel) {
            self.deferred_contacts.remove(&id);
        }
        if paint && !preparing && self.session.engine().backend().0.is_some() {
            self.sequence += 1;
            self.enqueue(event)?;
            if !predicted {
                self.last_pen = if matches!(phase, PenPhase::Up | PenPhase::Cancel) {
                    None
                } else {
                    Some(event)
                };
            }
        }
        Ok(())
    }
    fn paint_ready(&self) -> bool {
        let engine = self.session.engine();
        engine.backend().0.as_ref().is_some_and(|gpu| {
            self.startup.brush_ready
                && !gpu.startup_needs_update(
                    engine.document(),
                    engine.brush(),
                    engine.transform_preview().is_some(),
                )
        })
    }
    /// Check the core revision before building any layout, panel models or JSON.
    /// Camera-only changes return a small patch, without constructing UI models.
    /// Presentation-only state is included because it is not part of UiState.
    pub fn take_snapshot(&mut self) -> Option<Value> {
        let key = SnapshotKey {
            revision: self.session.state().revision,
            logical: self.logical,
            chrome_hidden: self.chrome_hidden,
            hide_floating_panels: self.hide_floating_panels,
            keep_zen_button: self.keep_zen_button,
            gpu_ready: self.session.engine().backend().0.is_some(),
            startup: self.startup,
            error: self.error.clone(),
        };
        if self.last_snapshot.as_ref() == Some(&key) {
            let camera = &self.session.state().camera;
            if self.last_camera_revision != Some(camera.revision) {
                self.last_camera_revision = Some(camera.revision);
                return Some(json!({"camera": camera, "revision": key.revision}));
            }
            return None;
        }
        self.last_snapshot = Some(key);
        self.last_camera_revision = Some(self.session.state().camera.revision);
        let mut snapshot = self.snapshot();
        let workspace = self.session.durable_workspace();
        if self.last_durable_workspace.as_ref() != Some(&workspace) {
            snapshot["workspace_persistence"] = json!(workspace);
            self.last_durable_workspace = Some(workspace);
        }
        Some(snapshot)
    }
    fn snapshot(&self) -> Value {
        let layout = self.session.layout(self.logical);
        let state = self.session.state();
        let zen = if state.partial_zen() {
            state.workspace.layout.zen_toolbars(self.logical)
        } else {
            Default::default()
        };
        // Drawers and partial Zen can project panels absent from the ordinary
        // dock groups. Publish their shared views as well, without moving docks.
        let mut panel_ids: Vec<_> = layout
            .groups
            .iter()
            .flat_map(|g| &g.panels)
            .copied()
            .collect();
        for panel in zen
            .sections
            .iter()
            .map(|s| s.panel)
            .chain(
                layout
                    .collapsed
                    .iter()
                    .flat_map(|c| &c.groups)
                    .flat_map(|g| &g.icons)
                    .map(|i| i.panel),
            )
            .chain(
                state
                    .customization
                    .drawer
                    .iter()
                    .chain(&state.customization.column_drawers)
                    .flat_map(|d| d.columns.iter().flatten())
                    .copied(),
            )
        {
            if !panel_ids.contains(&panel) {
                panel_ids.push(panel);
            }
        }
        let panels: Vec<_> = panel_ids
            .into_iter()
            .filter_map(|p| self.session.panel_view(p).ok())
            .collect();
        json!({"state": self.session.state(), "layout": layout, "panels": panels,
            "filter_preview_revision": self.session.filter_preview_revision(),
            "partial_zen": state.partial_zen(), "zen_toolbars": zen,
            "application_menus": layer_ui::ApplicationMenu::ALL.map(|menu| json!({"id": menu, "label": menu.label(), "model": self.session.application_menu(menu)})),
            "color_panel": self.session.state().colors.view(),
            "document_options": {"extent": layer_ui::DEFAULT_DOCUMENT_EXTENT,
                "max_dimension": layer_ui::MAX_NEW_DOCUMENT_DIMENSION,
                "width_label": layer_ui::DOCUMENT_WIDTH_LABEL, "height_label": layer_ui::DOCUMENT_HEIGHT_LABEL,
                "new_title": layer_ui::DocumentRequest::New.title(),
                "unsaved_description": layer_ui::UNSAVED_DESCRIPTION, "discard_label": layer_ui::DISCARD_DOCUMENT_LABEL,
                "cancel_label": layer_ui::CANCEL_DOCUMENT_LABEL,
                "save_label": layer_ui::DocumentRequest::ConfirmClose { title: String::new() }.accept_label(),
                "open_label": layer_ui::DocumentRequest::Open.accept_label(),
                "filter_label": layer_ui::DocumentRequest::Open.filter().0,
                "extension": layer_ui::DocumentRequest::Open.filter().1},
            "preferences": self.session.preferences(), "picker": self.session.tool_picker(),
            "workspace_menu": self.session.workspace_menu(), "toolbar_prompt": self.session.toolbar_prompt(),
            "toolbar_manager": self.session.toolbar_manager(),
            "panel_measurements": self.session.state().workspace.layout.measurements,
            "chrome_hidden": self.chrome_hidden, "gpu_ready": self.session.engine().backend().0.is_some(),
            "hide_floating_panels": self.hide_floating_panels, "keep_zen_button": self.keep_zen_button,
            "canvas_ready": self.session.engine().backend().0.is_some() && self.startup.canvas_ready,
            "brush_ready": self.session.engine().backend().0.is_some() && self.startup.brush_ready,
            "shaders_ready": self.session.engine().backend().0.is_some() && self.startup.complete,
            "error": self.error})
    }
    pub fn query(&mut self, query: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Query {
            FilterPackageModules {
                manifest: String,
            },
            LoadFilterPackage {
                manifest: String,
                modules: std::collections::BTreeMap<String, std::sync::Arc<str>>,
                mode: layer_core::EffectInstallMode,
                #[serde(default)]
                library: bool,
            },
            Catalog,
            ApplicationMenu {
                menu: layer_ui::ApplicationMenu,
            },
            ApplicationLink {
                link: layer_ui::ApplicationLink,
            },
            RendererStats,
            FilterPreviews {
                request: u64,
                revision: Option<(u64, u64, u64)>,
                filters: Vec<std::sync::Arc<str>>,
                size: [u32; 2],
            },
            ActionTooltip {
                label: String,
                action: UiAction,
            },
            LayerMenu {
                id: u64,
                mask: bool,
            },
            LayerThumbnails {
                requests: Vec<(u64, u64)>,
            },
            Context {
                target: layer_ui::ContextTarget,
            },
            PanelHandleTarget {
                item: layer_ui::DockItem,
            },
            Drop {
                position: [f32; 2],
                tabs: Vec<layer_ui::TabHit>,
                item: layer_ui::DockItem,
                #[serde(default)]
                expansion: Option<layer_ui::PanelExpansion>,
            },
            Drawer {
                column: Option<u32>,
                heights: Vec<f32>,
                progress: f32,
                from: Option<layer_ui::DrawerPlacement>,
                #[serde(default)]
                closing: bool,
            },
            DrawerToolbar {
                panel: layer_ui::Panel,
                width: f32,
                height: f32,
            },
            Navigator {
                viewport: [f32; 2],
            },
            Expansion {
                panel: layer_ui::Panel,
                heights: [f32; 2],
                progress: f32,
                #[serde(default)]
                from: Option<layer_ui::PanelExpansion>,
                #[serde(default)]
                closing: bool,
            },
        }
        let result = match serde_json::from_value(query).map_err(|e| e.to_string())? {
            Query::FilterPackageModules { manifest } => {
                json!(layer_core::EffectPackage::parse(&manifest)?.module_names()?)
            }
            Query::LoadFilterPackage {
                manifest,
                modules,
                mode,
                library,
            } => {
                let read = |name: &str| {
                    modules
                        .get(name)
                        .cloned()
                        .ok_or_else(|| format!("Missing filter module: {name}"))
                };
                let change = if library {
                    self.session.load_effect_library(&manifest, read, mode)
                } else {
                    self.session.load_effect_package(&manifest, read, mode)
                }?;
                self.dirty |= change.canvas_wake;
                json!(self.session.state().filter_load)
            }
            Query::Catalog => json!(layer_ui::ui_catalog()),
            Query::ApplicationMenu { menu } => json!(self.session.application_menu(menu)),
            Query::ApplicationLink { link } => json!(link.url()),
            Query::RendererStats => json!(self.session.renderer_stats()),
            Query::FilterPreviews {
                request,
                revision,
                filters,
                size,
            } => {
                let current = self.session.filter_preview_revision();
                let accepted = revision == Some(current)
                    && !filters.is_empty()
                    && self
                        .session
                        .request_filter_previews(request, filters, size)?;
                json!({ "revision": current, "accepted": accepted })
            }
            Query::ActionTooltip { label, action } => {
                let state = self.session.state();
                json!(
                    state
                        .settings
                        .action_tooltip(&label, &action, state.platform)
                )
            }
            Query::LayerMenu { id, mask } => json!(self.session.layer_menu(id, mask)?),
            Query::LayerThumbnails { requests } => {
                let mut accepted = Vec::new();
                if !self.dirty && !self.session.engine().has_pending_document_edits() {
                    for (request, target) in requests.into_iter().take(8) {
                        if self
                            .session
                            .renderer_mut()
                            .request_thumbnail(request, layer_core::LayerId(target))
                            .is_ok()
                        {
                            accepted.push(request);
                        }
                    }
                }
                let mut images = Vec::new();
                if let Some(gpu) = &self.session.renderer_mut().0 {
                    gpu.device()
                        .poll(wgpu::PollType::Poll)
                        .map_err(|e| e.to_string())?;
                }
                while let Some(image) = self.session.renderer_mut().take_thumbnail() {
                    let image = image.map_err(|e| e.to_string())?;
                    images.push((image.request_id, image.width, image.height, image.bytes));
                }
                json!({ "accepted": accepted, "images": images })
            }
            Query::Context { target } => json!(self.session.context_menu(target)?),
            Query::PanelHandleTarget { item } => {
                json!(
                    self.session
                        .state()
                        .workspace
                        .layout
                        .panel_handle_target(item)
                )
            }
            Query::Drop {
                position,
                tabs,
                item,
                expansion,
            } => self
                .session
                .drop_hint(self.logical, position, &tabs, item, expansion)
                .map_or(Value::Null, |hint| {
                    let action = item.move_action(hint.target.clone(), self.logical);
                    let mut value = json!(hint);
                    // Panel/group gestures use DragWorkspace for live movement,
                    // grab offsets and one history transaction. Tile drops apply
                    // a single action after their final preview has resolved.
                    if matches!(item, layer_ui::DockItem::Tile { .. }) {
                        value["action"] = json!(action);
                    }
                    value
                }),
            Query::Drawer {
                column,
                heights,
                progress,
                from,
                closing,
            } => {
                let state = self.session.state();
                let drawer = match column {
                    None => state.customization.drawer.as_ref(),
                    Some(id) => state.customization.column_drawers.iter().find(|d|
                        matches!(d.anchor, layer_ui::DrawerAnchor::Column { column, .. } if column == id)),
                };
                let end = if closing {
                    from.as_ref().map(|p| p.closed())
                } else {
                    drawer.and_then(|d| {
                        state.customization.drawer_placement(
                            d,
                            &state.workspace.layout,
                            self.logical,
                            &heights,
                            state.partial_zen(),
                        )
                    })
                };
                json!(end.map(|end| {
                    let from = from.unwrap_or_else(|| end.closed());
                    let placement = end.interpolate_from(&from, progress);
                    json!({"connection": placement.connection(), "placement": placement})
                }))
            }
            Query::DrawerToolbar {
                panel,
                width,
                height,
            } => {
                if ![width, height].into_iter().all(|v| v.is_finite() && v > 0.) {
                    return Err("Invalid drawer toolbar size".into());
                }
                let config = self.session.state().workspace.layout.panel(panel)?;
                let geometry = layer_ui::toolbar_tile_layout(
                    width,
                    layer_ui::toolbar_content_height(width, config.tiles(), config.tile_style),
                    layer_ui::Axis::Vertical,
                    config.tiles(),
                    false,
                    config.tile_style,
                );
                let content_height = geometry
                    .tiles
                    .iter()
                    .map(|b| b.y + b.height + 4.)
                    .fold(0., f32::max);
                let mut value = json!(geometry);
                value["content_height"] = json!(content_height);
                value
            }
            Query::Navigator { viewport } => {
                let state = self.session.state();
                let doc = self.session.engine().document();
                json!(layer_ui::NavigatorGeometry::new(
                    &state.camera,
                    [doc.width, doc.height],
                    viewport
                ))
            }
            Query::Expansion {
                panel,
                heights,
                progress,
                from,
                closing,
            } => {
                let layout = &self.session.state().workspace.layout;
                let end = layout.expanded_panel(
                    self.logical,
                    panel,
                    heights,
                    if closing { 0.0 } else { 1.0 },
                );
                let from =
                    from.or_else(|| layout.expanded_panel(self.logical, panel, heights, 0.0));
                let resolved = self.session.layout(self.logical);
                end.zip(from).map_or(Value::Null, |(end, from)| {
                    let placement = end.interpolate_from(from, progress);
                    let mut value = json!(placement);
                    if panel.kind() == layer_ui::PanelKind::Tiles {
                        let config = layout.panel(panel).expect("expanded panel exists");
                        let group = resolved
                            .groups
                            .iter()
                            .find(|g| g.panels.contains(&panel))
                            .expect("expanded group exists");
                        value["tiles"] = json!(layer_ui::toolbar_tile_layout(
                            placement.preview.width,
                            (placement.preview.height - placement.configuration.y).max(0.0),
                            group.axis,
                            config.tiles(),
                            !group.tabs_visible,
                            config.tile_style,
                        ));
                    }
                    value
                })
            }
        };
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn application_menus_follow_actions_without_entering_camera_patches() {
        use layer_ui::{ApplicationMenu, CommandId, Platform};
        for platform in [Platform::Ios, Platform::Mac] {
            let mut host = NativeHost::new(platform).unwrap();
            host.resize(1200, 900, 1.).unwrap();
            let menus = host.take_snapshot().unwrap()["application_menus"].clone();
            assert_eq!(menus.as_array().unwrap().len(), ApplicationMenu::ALL.len());
            for (id, menu) in ApplicationMenu::ALL
                .into_iter()
                .zip(menus.as_array().unwrap())
            {
                let expected =
                    json!({"id":id,"label":id.label(),"model":host.session.application_menu(id)});
                assert_eq!(
                    host.query(json!({"type":"application_menu","menu":id}))
                        .unwrap(),
                    expected["model"]
                );
                assert_eq!(*menu, expected);
            }
            let help = menus
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == "help")
                .unwrap();
            assert_eq!(
                help["model"]["sections"][1][0]["action"]["command"],
                "website"
            );
            assert_eq!(help["model"]["sections"][1][0]["enabled"], true);
            assert!(host.take_snapshot().is_none());
            host.dispatch(UiAction::Invoke {
                command: CommandId::SelectAll,
            })
            .unwrap();
            let next = host.take_snapshot().unwrap();
            let select = next["application_menus"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == "select")
                .unwrap();
            assert!(
                select["model"]["sections"][0][1]["enabled"]
                    .as_bool()
                    .unwrap()
            );
            host.dispatch(UiAction::Invoke {
                command: CommandId::ZoomIn,
            })
            .unwrap();
            let camera = host.take_snapshot().unwrap();
            assert!(camera.get("camera").is_some());
            assert!(
                camera.get("application_menus").is_none(),
                "No menu rebuild at camera input rate"
            );
            for link in [
                layer_ui::ApplicationLink::Website,
                layer_ui::ApplicationLink::SourceCode,
            ] {
                assert_eq!(
                    host.query(json!({"type":"application_link", "link":link}))
                        .unwrap(),
                    link.url()
                );
            }
        }
    }
    #[test]
    fn partial_zen_publishes_shared_edge_sections_without_changing_docks() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2880, 1800, 1.75).unwrap();
        let mut settings = app.session.state().settings.clone();
        settings.total_zen = false;
        app.dispatch(UiAction::RestoreSettings { settings })
            .unwrap();
        let layout = app.session.state().workspace.layout.clone();
        app.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::ZenMode,
        })
        .unwrap();
        let snapshot = app.take_snapshot().unwrap();
        assert_eq!(snapshot["partial_zen"], true);
        let sections = snapshot["zen_toolbars"]["sections"].as_array().unwrap();
        assert!(!sections.is_empty());
        for section in sections {
            let panel = snapshot["panels"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == section["panel"])
                .unwrap();
            for tile in section["tiles"].as_array().unwrap() {
                assert!(
                    panel["tiles"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|t| t["id"] == tile[0])
                );
            }
        }
        assert_eq!(app.session.state().workspace.layout, layout);
        app.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::ZenMode,
        })
        .unwrap();
        assert_eq!(app.take_snapshot().unwrap()["partial_zen"], false);
    }
    #[test]
    fn android_drawer_queries_follow_collapsed_toolbar_measurements() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2880, 1800, 1.75).unwrap();
        let group = app
            .session
            .state()
            .workspace
            .layout
            .panel_group(layer_ui::Panel::Brushes)
            .unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"move_panel", "panel":"toolbar", "target":{"kind":"tab","group":group}, "viewport":app.logical})).unwrap()).unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"customize", "action":{"type":"set_column_collapsed","group":group,"collapsed":true}})).unwrap()).unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"customize", "action":{"type":"toggle_column_drawer","group":group,"panel":"toolbar"}})).unwrap()).unwrap();
        let column = app
            .session
            .state()
            .workspace
            .layout
            .collapsed_column_for_group(group)
            .unwrap();
        let tile = app
            .session
            .state()
            .workspace
            .layout
            .panel(layer_ui::Panel::Toolbar)
            .unwrap()
            .tiles()[0]
            .id;
        app.dispatch(serde_json::from_value(json!({"type":"measure_drawer_tiles", "measurements":[{"column":column,"anchor":{"panel":"toolbar","tile":tile},"bounds":{"x":80.,"y":100.,"width":36.,"height":36.}}]})).unwrap()).unwrap();
        app.dispatch(serde_json::from_value(json!({"type":"customize", "action":{"type":"toggle_tool_drawer","anchor":{"panel":"toolbar","tile":tile}}})).unwrap()).unwrap();
        let query = json!({"type":"drawer","heights":[900.,200.],"progress":1.});
        let first = app.query(query.clone()).unwrap();
        assert_eq!(first["placement"]["anchor"]["y"], 100.);
        assert!(first["connection"].is_object());
        app.dispatch(serde_json::from_value(json!({"type":"measure_drawer_tiles", "measurements":[{"column":column,"anchor":{"panel":"toolbar","tile":tile},"bounds":{"x":80.,"y":60.,"width":36.,"height":20.}}]})).unwrap()).unwrap();
        assert_eq!(
            app.query(query.clone()).unwrap()["placement"]["anchor"]["height"],
            20.
        );
        app.dispatch(
            serde_json::from_value(json!({"type":"measure_drawer_tiles", "measurements":[]}))
                .unwrap(),
        )
        .unwrap();
        assert!(
            app.query(query).unwrap().is_null(),
            "A clipped-out tile has no child drawer"
        );
    }
    #[test]
    fn workspace_persistence_only_emits_committed_topology_changes() {
        for platform in [layer_ui::Platform::Mac, layer_ui::Platform::Ios] {
            let mut app = NativeHost::new(platform).unwrap();
            let viewport = [1200., 900.];
            app.resize(2400, 1800, 2.).unwrap();
            let initial = app.take_snapshot().unwrap()["workspace_persistence"].clone();
            assert_eq!(initial["version"], 1);
            app.dispatch(UiAction::SetBrushSize { value: 40. }).unwrap();
            assert!(
                app.take_snapshot()
                    .unwrap()
                    .get("workspace_persistence")
                    .is_none()
            );
            app.dispatch(UiAction::MeasurePanels {
                measurements: vec![layer_ui::PanelMeasurement {
                    panel: layer_ui::Panel::Brushes,
                    tab_width: 100.,
                    content_height: 900.,
                }],
            })
            .unwrap();
            if let Some(snapshot) = app.take_snapshot() {
                assert!(
                    snapshot.get("workspace_persistence").is_none(),
                    "Host measurements are transient"
                );
            }
            let divider = app.session.layout(viewport).dividers[0].clone();
            let point = [
                divider.bounds.x + divider.bounds.width / 2.,
                divider.bounds.y + divider.bounds.height / 2.,
            ];
            for end in [ContactPhase::Cancel, ContactPhase::Up] {
                for (phase, delta) in [(ContactPhase::Down, 0.), (ContactPhase::Move, 40.)] {
                    app.dispatch(UiAction::DragDivider {
                        id: divider.id,
                        phase,
                        position: [point[0] + delta, point[1]],
                        viewport,
                    })
                    .unwrap();
                    if let Some(snapshot) = app.take_snapshot() {
                        assert!(
                            snapshot.get("workspace_persistence").is_none(),
                            "Never persist a provisional resize"
                        );
                    }
                }
                app.dispatch(UiAction::DragDivider {
                    id: divider.id,
                    phase: end,
                    position: [point[0] + 40., point[1]],
                    viewport,
                })
                .unwrap();
                let snapshot = app.take_snapshot().unwrap();
                if end == ContactPhase::Cancel {
                    assert!(snapshot.get("workspace_persistence").is_none());
                } else {
                    let saved = snapshot["workspace_persistence"].clone();
                    assert!(!saved.is_null() && saved != initial);
                    let mut restored = NativeHost::new(platform).unwrap();
                    restored
                        .dispatch(
                            serde_json::from_value(
                                json!({"type":"restore_workspace", "workspace":saved}),
                            )
                            .unwrap(),
                        )
                        .unwrap();
                    assert_eq!(
                        restored.take_snapshot().unwrap()["workspace_persistence"],
                        saved
                    );
                    app.dispatch(UiAction::Invoke {
                        command: layer_ui::CommandId::UndoWorkspace,
                    })
                    .unwrap();
                    assert_eq!(
                        app.take_snapshot().unwrap()["workspace_persistence"],
                        initial
                    );
                }
            }
        }
    }
    #[test]
    fn native_navigation_preserves_camera_patches_and_rejects_nonfinite_input() {
        let mut app = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        app.resize(2400, 1800, 2.0).unwrap();
        app.take_snapshot().unwrap();
        let anchor = [1200., 900.];
        let before = app.session.state().camera.clone();
        app.scroll(anchor, [30., -20.], 2., false, false).unwrap();
        let patch = app.take_snapshot().unwrap();
        assert!(
            patch.get("state").is_none(),
            "Wheel pan must not rebuild all editor models"
        );
        assert_ne!(patch["camera"], json!(before));
        let zoom = app.session.state().camera.zoom;
        app.gesture(anchor, 1.5, 0.2).unwrap();
        assert!((app.session.state().camera.zoom - zoom * 1.5).abs() < 0.0001);
        assert!(app.take_snapshot().unwrap().get("state").is_none());
        let camera = json!(app.session.state().camera);
        assert!(
            app.scroll(anchor, [f32::NAN, 0.], 2., false, false)
                .is_err()
        );
        assert!(app.gesture(anchor, 0., 0.).is_err());
        assert_eq!(json!(app.session.state().camera), camera);
        app.dispatch(UiAction::SetBrushSize { value: 42. }).unwrap();
        app.scroll(anchor, [0., 1.], 2., false, false).unwrap();
        assert_eq!(
            app.take_snapshot().unwrap()["state"]["brush"]["diameter"],
            42.
        );
    }
    #[test]
    fn stale_filter_preview_requests_do_not_touch_the_renderer() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        let response = app
            .query(json!({"type":"filter_previews", "request":1,
            "revision":null, "filters":["curves"], "size":[240,40]}))
            .unwrap();
        assert_eq!(response["accepted"], false);
        assert_eq!(
            response["revision"],
            json!(app.session.filter_preview_revision())
        );
        assert!(app.session.renderer_mut().0.is_none());
    }
    #[test]
    fn workspace_views_expose_shared_actions_and_transient_measurements() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        app.dispatch(UiAction::MeasurePanels {
            measurements: vec![layer_ui::PanelMeasurement {
                panel: layer_ui::Panel::Brushes,
                tab_width: 76.0,
                content_height: 480.0,
            }],
        })
        .unwrap();
        let snapshot = app.snapshot();
        assert_eq!(snapshot["panel_measurements"][0]["tab_width"], 76.0);
        assert!(
            snapshot["state"]["workspace"]["layout"]
                .get("measurements")
                .is_none()
        );
        assert_eq!(
            snapshot["workspace_menu"]["sections"][0][0]["action"]["type"],
            "invoke"
        );
        app.dispatch(
            serde_json::from_value(json!({
                "type": "move_panel", "panel": "toolbar", "viewport": [1280, 800],
                "target": {"kind": "float", "position": [500, 300]}
            }))
            .unwrap(),
        )
        .unwrap();
        let target = app.query(json!({"type": "panel_handle_target", "item": {"kind": "panel", "panel": "toolbar"}})).unwrap();
        assert!(target.is_u64());
        app.dispatch(serde_json::from_value(json!({"type": "customize", "action": {"type": "duplicate_toolbar", "panel": "toolbar"}})).unwrap()).unwrap();
        assert_eq!(
            app.snapshot()["toolbar_prompt"]["title"],
            "Duplicate Toolbar"
        );
    }

    #[test]
    fn ui_is_available_without_a_gpu() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        app.dispatch(UiAction::OpenSettings {
            page: layer_ui::SettingsPage::Input,
        })
        .unwrap();
        let snapshot = app.snapshot();
        assert_eq!(snapshot["state"]["platform"], "android");
        assert_eq!(snapshot["gpu_ready"], false);
        assert_eq!(snapshot["preferences"]["page"], "input");
        assert!(!snapshot["layout"]["groups"].as_array().unwrap().is_empty());
        assert_eq!(
            app.query(json!({"type":"catalog"})).unwrap()["app_name"],
            "Capy Canvas"
        );
    }
    #[test]
    fn snapshots_skip_unchanged_input_but_publish_state_and_chrome() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        assert!(app.take_snapshot().is_some());
        for i in 0..1000 {
            app.pointer(
                1,
                0,
                0,
                &[100. + i as f64, 200., 1., 0., 0., 0., 0., i as f64, 0.],
                false,
            )
            .unwrap();
            assert!(app.take_snapshot().is_none());
        }
        app.dispatch(UiAction::SetBrushSize { value: 42.0 })
            .unwrap();
        assert_eq!(
            app.take_snapshot().unwrap()["state"]["brush"]["diameter"],
            42.0
        );
        assert!(app.take_snapshot().is_none());
        app.chrome_hidden = true;
        assert_eq!(app.take_snapshot().unwrap()["chrome_hidden"], true);
        app.hide_floating_panels = true;
        assert_eq!(app.take_snapshot().unwrap()["hide_floating_panels"], true);
        app.keep_zen_button = false;
        assert_eq!(app.take_snapshot().unwrap()["keep_zen_button"], false);
        assert!(app.take_snapshot().is_none());
        app.error = Some("test surface error".into());
        assert_eq!(app.take_snapshot().unwrap()["error"], "test surface error");
        assert!(app.take_snapshot().is_none());
        app.resize(1600, 2560, 2.0).unwrap();
        assert!(app.take_snapshot().is_some());
    }
    #[test]
    fn camera_patches_preserve_pending_structural_updates() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        app.take_snapshot().unwrap();
        let finger = |app: &mut NativeHost, id, x, phase| {
            app.pointer(
                id,
                3,
                0,
                &[x, 300., 1., 0., 0., 0., 0., 1_000_000., phase],
                false,
            )
            .unwrap();
        };
        finger(&mut app, 1, 400., 1.);
        finger(&mut app, 2, 600., 1.);
        for i in 1..100 {
            finger(&mut app, 2, 600. + i as f64, 2.);
            let patch = app.take_snapshot().unwrap();
            assert_eq!(patch["camera"], json!(app.session.state().camera));
            assert_eq!(patch["revision"], app.session.state().revision);
            assert!(
                patch.get("state").is_none(),
                "Camera motion must not build full UI models"
            );
            assert!(app.take_snapshot().is_none());
        }
        // A camera update must not acknowledge an unpublished brush change.
        app.dispatch(UiAction::SetBrushSize { value: 42.0 })
            .unwrap();
        finger(&mut app, 2, 710., 2.);
        let full = app.take_snapshot().unwrap();
        assert_eq!(full["state"]["brush"]["diameter"], 42.0);
        assert_eq!(full["state"]["camera"], json!(app.session.state().camera));
        // Also cover changes that bypass the host's dispatch wrapper.
        app.session
            .dispatch(UiAction::SetBrushSize { value: 52.0 })
            .unwrap();
        finger(&mut app, 2, 720., 2.);
        assert_eq!(
            app.take_snapshot().unwrap()["state"]["brush"]["diameter"],
            52.0
        );
        app.error = Some("surface lost".into());
        finger(&mut app, 2, 730., 2.);
        assert_eq!(app.take_snapshot().unwrap()["error"], "surface lost");
        app.resize(1600, 2560, 2.0).unwrap();
        assert!(app.take_snapshot().unwrap().get("layout").is_some());
    }

    #[test]
    fn predicted_boundaries_cannot_end_a_deferred_real_contact() {
        let mut app = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        app.resize(1600, 1000, 2.0).unwrap();
        let mut event = PenEvent {
            device_id: 1,
            sequence: 99,
            timestamp_ns: (1u64 << 54) + 1,
            view_revision: app.session.state().camera.revision,
            surface_position: Point { x: 400., y: 300. },
            pressure: 0.5,
            tilt_radians: [0.1, 0.2],
            twist_radians: 0.3,
            distance: 0.,
            phase: PenPhase::Down,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        };
        app.pointer_event(event, PointerButton::Primary).unwrap();
        assert!(app.deferred_contacts.contains(&1));
        event.phase = PenPhase::Up;
        event.flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::PREDICTED.0);
        app.pointer_event(event, PointerButton::Primary).unwrap();
        assert!(app.deferred_contacts.contains(&1));
        assert_eq!(
            app.sequence, 0,
            "Unavailable brushes must not create partial strokes"
        );
        event.flags = SampleFlags::PRIMARY;
        app.pointer_event(event, PointerButton::Primary).unwrap();
        assert!(!app.deferred_contacts.contains(&1));
    }

    #[test]
    fn typed_touch_uses_the_same_shared_navigation_as_packed_input() {
        let mut typed = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let mut packed = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        for app in [&mut typed, &mut packed] {
            app.resize(1600, 1000, 2.0).unwrap();
        }
        for (id, x, phase) in [(1, 400., 1), (2, 600., 1), (2, 750., 2), (2, 750., 3)] {
            let event = PenEvent {
                device_id: id,
                sequence: 1,
                timestamp_ns: 1_000_000,
                view_revision: typed.session.state().camera.revision,
                surface_position: Point { x, y: 300. },
                pressure: 1.,
                tilt_radians: [0., 0.],
                twist_radians: 0.,
                distance: 0.,
                phase: match phase {
                    1 => PenPhase::Down,
                    2 => PenPhase::Move,
                    _ => PenPhase::Up,
                },
                tool: ToolKind::Finger,
                flags: SampleFlags::PRIMARY,
            };
            typed.pointer_event(event, PointerButton::Primary).unwrap();
            packed
                .pointer(
                    id,
                    3,
                    0,
                    &[x as f64, 300., 1., 0., 0., 0., 0., 1_000_000., phase as f64],
                    false,
                )
                .unwrap();
        }
        assert_eq!(
            json!(typed.session.state().camera),
            json!(packed.session.state().camera)
        );
        assert_eq!(typed.sequence, 0);
    }

    #[test]
    fn typed_samples_from_retired_or_closed_documents_do_not_acquire_contacts() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.resize(800, 600, 1.).unwrap();
        let old_revision = host.session.state().camera.revision;
        host.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::ZoomIn,
        })
        .unwrap();
        host.document_adopted();
        assert!(host.session.state().camera.revision > old_revision);
        let mut event = PenEvent {
            device_id: 1,
            sequence: 1,
            timestamp_ns: 1_000_000,
            view_revision: old_revision,
            surface_position: Point { x: 400., y: 300. },
            pressure: 0.5,
            tilt_radians: [0.; 2],
            twist_radians: 0.,
            distance: 0.,
            phase: PenPhase::Down,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        };
        host.pointer_event(event, PointerButton::Primary).unwrap();
        assert!(host.deferred_contacts.is_empty());
        host.session.require_document_idle().unwrap();
        host.session.request_document_close().unwrap();
        assert!(host.session.state().document_file.close_ready);
        event.view_revision = host.session.state().camera.revision;
        host.pointer_event(event, PointerButton::Primary).unwrap();
        assert!(host.deferred_contacts.is_empty());
        host.session.require_document_idle().unwrap();
    }

    #[test]
    #[ignore = "Requires an explicitly selected hardware GPU"]
    fn estimated_input_tokens_reach_committed_stroke_corrections() {
        let gpu = layer_render_wgpu::WgpuRasterizer::new_headless().unwrap();
        assert_ne!(gpu.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
        let mut host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        host.session = UiSession::from_project(
            Renderer(Some(gpu)),
            layer_ui::new_drawing(64, 48).unwrap(),
            None,
            [64, 48],
        )
        .unwrap();
        host.session.frame(0, 0).unwrap();
        assert!(host.paint_ready());
        let revision = host.session.state().camera.revision;
        for (token, pending, x, phase, time) in [
            (9001, 1, 20., 1., 10_000_000.),
            (9002, 0, 40., 2., 20_000_000.),
            (9003, 0, 40., 3., 21_000_000.),
        ] {
            host.pointer_batch_updates(
                PointerBatch {
                    id: 7,
                    tool: 0,
                    button: 0,
                    predicted: false,
                    view_revision: revision,
                    records: &[x, 24., 0.25, 0., 0., 0., 0., time, phase],
                },
                &[token, pending],
                false,
            )
            .unwrap();
            host.session.frame(time as u64, time as u64).unwrap();
        }
        assert!(host.last_pen.is_none());
        let stroke = host.session.engine().document().strokes().next().unwrap();
        let before = stroke.points[0];
        assert_eq!(before.pressure, 0.25);
        let count = stroke.points.len();
        let camera = json!(host.session.state().camera);
        host.pointer_batch_updates(
            PointerBatch {
                id: 7,
                tool: 0,
                button: 0,
                predicted: false,
                view_revision: revision,
                records: &[24., 24., 0.9, 0.2, -0.3, 1.7, 0., 10_000_000., 1.],
            },
            &[9001, 0],
            true,
        )
        .unwrap();
        host.session.frame(30_000_000, 30_000_000).unwrap();
        let stroke = host.session.engine().document().strokes().next().unwrap();
        assert_eq!(stroke.points.len(), count);
        assert_eq!(stroke.points[0].pressure, 0.9);
        assert_eq!(stroke.points[0].tilt, [0.2, -0.3]);
        assert_eq!(stroke.points[0].twist, 1.7);
        assert!(stroke.points[0].position.x > before.position.x);
        assert_eq!(host.session.engine().document().strokes().count(), 1);
        assert!(host.last_pen.is_none());
        assert!(host.deferred_contacts.is_empty());
        assert_eq!(json!(host.session.state().camera), camera);
        host.session.require_document_idle().unwrap();
    }

    #[test]
    fn malformed_input_is_rejected_before_changing_state() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        for sample in [
            vec![],
            vec![0.0; 8],
            vec![f64::NAN; 9],
            vec![0., 0., 1., 0., 0., 0., 0., -1., 1.],
        ] {
            assert!(app.pointer(1, 0, 0, &sample, false).is_err());
        }
        assert_eq!(app.sequence, 0);
        assert!(app.resize(0, 100, 1.0).is_err());
        assert!(app.resize(100, 100, 0.0).is_err());
    }
    #[test]
    fn touch_uses_shared_navigation_not_paint() {
        let mut app = NativeHost::new(layer_ui::Platform::Android).unwrap();
        app.resize(2560, 1600, 2.0).unwrap();
        let before = app.session.state().camera.revision;
        app.pointer(
            1,
            3,
            0,
            &[100., 100., 1., 0., 0., 0., 0., 1_000_000., 1.],
            false,
        )
        .unwrap();
        app.pointer(
            2,
            3,
            0,
            &[200., 100., 1., 0., 0., 0., 0., 1_000_000., 1.],
            false,
        )
        .unwrap();
        app.pointer(
            1,
            3,
            0,
            &[120., 120., 1., 0., 0., 0., 0., 2_000_000., 2.],
            false,
        )
        .unwrap();
        app.pointer(
            1,
            3,
            0,
            &[120., 120., 1., 0., 0., 0., 0., 3_000_000., 3.],
            false,
        )
        .unwrap();
        assert_eq!(app.sequence, 0);
        assert!(app.session.state().camera.revision > before);
    }
}
