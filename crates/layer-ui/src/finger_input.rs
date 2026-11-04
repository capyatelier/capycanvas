use super::*;
use std::collections::BTreeSet;
use super::held_actions::merge_change;

struct Draft {
    working: layer_core::WorkingState,
    path: Vec<layer_core::Point>,
    paths: vector_paths::VectorPaths,
    points: vector_points::VectorPoints,
    selection: selection_tools::SelectionTools,
}
struct Contact {
    id: u64,
    time_ns: u64,
    camera: Camera,
    draft: Option<Draft>,
}
#[derive(Default)]
pub(super) struct FingerInput {
    pub(super) device: DeviceClass,
    touches: BTreeSet<u64>,
    rejected: BTreeSet<u64>,
    pen: Option<u64>,
    contact: Option<Contact>,
    pub raw: Option<u64>,
    pub withdrawn: bool,
    pub admitted_pen: bool,
}
impl FingerInput {
    pub fn new(platform: Platform) -> Self {
        Self { device: if matches!(platform, Platform::Android | Platform::Ios) { DeviceClass::Tablet } else { DeviceClass::Desktop }, ..Self::default() }
    }
    pub fn owns(&self, id: u64) -> bool { self.contact.as_ref().is_some_and(|contact| contact.id == id) }
    pub fn recognizing(&self) -> bool { self.contact.is_some() || self.withdrawn }
    pub fn finish_input(&mut self) {
        if self.touches.is_empty() { self.withdrawn = false; }
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub fn set_device_class(&mut self, device: DeviceClass) -> UiChange {
        self.finger.device = device;
        self.changed(regions::CUSTOMIZATION | regions::SETTINGS, false)
    }
    pub fn finger_drawing(&self) -> bool {
        self.state.settings.finger_drawing.enabled(self.finger.device, self.state.settings.pen_seen)
    }
    pub(super) fn capture_finger_draft(&mut self, event: &PenEvent) {
        if event.tool != layer_engine::ToolKind::Finger || event.phase != PenPhase::Down
            || self.finger.contact.as_ref().is_none_or(|contact| contact.draft.is_some() || contact.time_ns != event.timestamp_ns) { return; }
        let draft = Draft { working: self.engine.document().working.clone(), path: self.layer_interaction.path.clone(),
            paths: self.vector_paths.clone(), points: self.vector_points.clone(), selection: self.selection_tools.clone() };
        self.finger.contact.as_mut().unwrap().draft = Some(draft);
    }
    fn withdraw_finger(&mut self, reply: &mut InputReply) -> Result<(), String> {
        let contact = self.finger.contact.take();
        let pointer = self.interaction.pointer.filter(|pointer| pointer.kind == PointerKind::Touch);
        let raw = self.finger.raw.take();
        if contact.is_none() && pointer.is_none() && raw.is_none() { return Ok(()); }
        if let Some(raw) = raw && let Some(event) = self.take_pen_cancel(raw, layer_engine::ToolKind::Finger) {
            self.admit_pen(event);
        }
        if let Some(pointer) = pointer {
            self.clone_source_input(pointer.id, ContactPhase::Cancel, pointer.position);
            self.interaction.pointer = None;
        }
        if let Some(contact) = contact {
            if let Some(draft) = contact.draft {
                self.engine.preview_edit(layer_core::Edit::Working(draft.working)).map_err(error)?;
                self.layer_interaction.path = draft.path;
                self.vector_paths.restore_contact(draft.paths);
                self.vector_points = draft.points;
                self.selection_tools.restore_contact(draft.selection);
                self.show_vector_path_preview()?;
            }
            if self.state.camera.viewport == contact.camera.viewport && self.state.camera != contact.camera {
                let revision = self.state.camera.revision + 1;
                self.state.camera = Camera { revision, ..contact.camera };
                self.sync_camera();
            }
        }
        self.finger.withdrawn = true;
        reply.cancel_paint = true;
        self.refresh_document();
        self.refresh_tools();
        self.refresh_commands();
        reply.change = merge_change(reply.change, self.changed(regions::DOCUMENT | regions::BRUSH | regions::COMMANDS | regions::CAMERA, true));
        Ok(())
    }
    pub(super) fn finger_input(&mut self, input: &UiInput, reply: &mut InputReply) -> Result<bool, String> {
        match *input {
            UiInput::Pointer { id, phase, kind: PointerKind::Touch, position, time_ns, .. } => {
                let rejected = self.finger.rejected.contains(&id);
                if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
                    self.finger.touches.remove(&id);
                    self.finger.rejected.remove(&id);
                }
                if rejected {
                    reply.handled = true;
                    return Ok(true);
                }
                if phase == ContactPhase::Down {
                    self.finger.touches.insert(id);
                    if self.finger.pen.is_some() {
                        self.finger.rejected.insert(id);
                        reply.handled = true;
                        return Ok(true);
                    }
                    self.edge_swipe(id, phase, position);
                    if let Some(contact) = self.finger.contact.as_ref().filter(|contact| contact.id != id) {
                        if time_ns.saturating_sub(contact.time_ns) <= u64::from(self.interaction.touch_policy.tap_ms) * 1_000_000 {
                            self.withdraw_finger(reply)?;
                        } else {
                            self.finger.rejected.insert(id);
                            reply.handled = true;
                            return Ok(true);
                        }
                    } else if self.finger_drawing() && self.interaction.pointer.is_none()
                        && !self.uses_fill_gesture() && self.navigation_mode().is_none()
                        && !self.zen_peek.swiping()
                        && !self.workspace_transition && !self.workspace_read_only && !self.state.settings_open
                        && !self.interaction.facts.popup_open && self.state.command_search.is_none()
                        && self.eyedropper.picking.previous.is_none()
                        && self.finger.touches.iter().all(|other| *other == id || self.finger.rejected.contains(other)) {
                        self.finger.contact = Some(Contact { id, time_ns, camera: self.state.camera.clone(), draft: None });
                    }
                }
                if self.finger.owns(id) {
                    if phase == ContactPhase::Cancel { self.withdraw_finger(reply)?; }
                    else if phase == ContactPhase::Up { self.finger.contact = None; }
                    else { self.touch.update(&mut self.state.camera, id, pen_phase(phase), position, false, &self.state.settings); }
                }
            }
            UiInput::Pointer { id, phase, kind, .. } if kind != PointerKind::Touch => {
                if phase == ContactPhase::Down {
                    self.withdraw_finger(reply)?;
                    self.finger.withdrawn = false;
                    self.finger.rejected.extend(self.finger.touches.iter().copied());
                    self.interaction.gestures = Default::default();
                    if kind == PointerKind::Pen {
                        self.finger.pen = Some(id);
                    }
                    self.touch.clear();
                } else if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) && self.finger.pen == Some(id) {
                    self.finger.pen = None;
                }
            }
            UiInput::ColorPickerHold { id, .. } if self.finger.owns(id) => self.withdraw_finger(reply)?,
            UiInput::Blur => {
                self.withdraw_finger(reply)?;
                self.finger.touches.clear();
                self.finger.rejected.clear();
                self.finger.pen = None;
            }
            _ => (),
        }
        Ok(false)
    }
    pub(super) fn save_admitted_pen(&mut self) -> Result<u32, String> {
        let admitted = std::mem::take(&mut self.finger.admitted_pen) | self.engine.take_admitted_pen();
        if !admitted || self.state.settings.pen_seen { return Ok(0); }
        self.state.settings.pen_seen = true;
        self.request(HostRequestKind::SaveSettings { settings: Box::new(self.state.settings.clone()) })?;
        Ok(regions::SETTINGS | regions::CUSTOMIZATION)
    }
}
