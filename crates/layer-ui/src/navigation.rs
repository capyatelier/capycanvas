use super::*;
use crate::interaction::NavigationContact;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct ViewPosition {
    zoom: f32,
    rotation: f32,
    flipped: [bool; 2],
    center: [f32; 2],
}
impl ViewPosition {
    fn capture(camera: &Camera) -> Self {
        let center = camera.work_area_center().map(f64::from);
        Self { zoom: camera.zoom, rotation: camera.rotation, flipped: camera.flipped,
            center: camera.surface_to_document64(center).map(|v| v as f32) }
    }
    fn restore(self, camera: &mut Camera) {
        camera.zoom = self.zoom;
        camera.rotation = self.rotation;
        camera.flipped = self.flipped;
        camera.center_on(self.center);
    }
}
#[derive(Default)]
pub(super) struct Navigation {
    pub previous: Option<ViewPosition>,
    pub saved: Option<ViewPosition>,
}

pub(super) fn command_mode(command: CommandId) -> Option<NavigationMode> {
    match command {
        CommandId::Hand => Some(NavigationMode::Pan),
        CommandId::Zoom => Some(NavigationMode::Zoom),
        CommandId::RotateView => Some(NavigationMode::Rotate),
        _ => None,
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn append_navigation_overlay(&self, segments: &mut Vec<layer_render::CursorSegment>) {
        let Some(pointer) = self.interaction.pointer else { return };
        let Some(contact) = pointer.navigation.filter(|c| c.rectangle && c.dragged) else { return };
        let dpi = self.logical_viewport.map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        let [x, y] = contact.origin.map(|v| v / dpi);
        let [u, v] = pointer.position.map(|v| v / dpi);
        let points = [[x, y], [u, y], [u, v], [x, v]];
        for index in 0..4 {
            segments.push(layer_render::CursorSegment { from: points[index], to: points[(index + 1) % 4], distance: 0., marker: 0., scale: 1. });
        }
    }
    pub fn begin_view_gesture(&mut self) {
        if self.navigation_idle() { self.remember_view(); }
    }
    pub(super) fn remember_view(&mut self) {
        self.navigation.previous = Some(ViewPosition::capture(&self.state.camera));
    }
    pub fn navigation_mode(&self) -> Option<NavigationMode> {
        if let Some(mode) = self.interaction.pointer.and_then(|p| p.navigation.map(|n| n.mode)) { return Some(mode); }
        self.interaction.navigation.as_ref().map(|(_, mode)| *mode)
            .or_else(|| self.layer_interaction.tool.navigation())
            .map(|mode| if mode == NavigationMode::Zoom && self.interaction.modifiers.alt { NavigationMode::ZoomOut } else { mode })
    }
    pub(super) fn begin_navigation(&mut self, button: PointerButton, origin: [f32; 2]) -> NavigationContact {
        self.remember_view();
        self.interaction.navigation_tap = false;
        let mut mode = if button == PointerButton::Pan { NavigationMode::Pan }
            else { self.navigation_mode().unwrap_or_default() };
        if mode == NavigationMode::Zoom && self.interaction.modifiers.alt { mode = NavigationMode::ZoomOut; }
        NavigationContact { mode, origin, dragged: false,
            rectangle: matches!(mode, NavigationMode::Zoom | NavigationMode::ZoomOut) && self.interaction.modifiers.shift }
    }
    pub(super) fn release_navigation(&mut self, key: &str) -> Result<UiChange, String> {
        if self.interaction.navigation.as_ref().is_some_and(|(token, _)| token == key) {
            let (_, mode) = self.interaction.navigation.take().unwrap();
            if std::mem::take(&mut self.interaction.navigation_tap) {
                let command = match mode { NavigationMode::Pan => CommandId::Hand,
                    NavigationMode::Rotate => CommandId::RotateView, _ => CommandId::Zoom };
                return self.dispatch(UiAction::Invoke { command });
            }
        }
        Ok(UiChange::default())
    }
    pub(super) fn navigate_pointer(&mut self, contact: &mut NavigationContact, previous: [f32; 2],
        phase: ContactPhase, position: [f32; 2]) -> Result<UiChange, String> {
        if phase == ContactPhase::Cancel { return Ok(self.changed(regions::CAMERA, true)); }
        if phase == ContactPhase::Down { return Ok(UiChange::default()); }
        let dpi = self.logical_viewport.map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        let moved = (position[0] - contact.origin[0]).hypot(position[1] - contact.origin[1]) > 3. * dpi;
        let from = if contact.dragged { previous } else { contact.origin };
        contact.dragged |= moved;
        if contact.dragged {
            if contact.rectangle {
                if phase == ContactPhase::Up && !self.state.camera.zoom_locked {
                    let camera = &mut self.state.camera;
                    let center = [(contact.origin[0] + position[0]) * 0.5, (contact.origin[1] + position[1]) * 0.5];
                    let point = camera.surface_to_document64(center.map(f64::from)).map(|v| v as f32);
                    let scale = (camera.work_area[2] / (position[0] - contact.origin[0]).abs().max(1.))
                        .min(camera.work_area[3] / (position[1] - contact.origin[1]).abs().max(1.));
                    camera.zoom = (camera.zoom * scale).clamp(MIN_ZOOM, MAX_ZOOM);
                    camera.center_on(point);
                    self.initial_fit = false;
                    self.sync_camera();
                }
                return Ok(self.changed(regions::CAMERA, true));
            }
            return match contact.mode {
                NavigationMode::Pan => self.gesture(from, position, 1., 0.),
                NavigationMode::Zoom | NavigationMode::ZoomOut => {
                    let scale = ((position[0] - from[0]) / dpi * 0.01 * self.state.settings.zoom_speed).clamp(-10., 10.).exp();
                    self.gesture(contact.origin, contact.origin, scale, 0.)
                }
                NavigationMode::Rotate => {
                    let center = self.state.camera.work_area_center();
                    let angle = |p: [f32; 2]| (p[1] - center[1]).atan2(p[0] - center[0]);
                    let delta = (angle(position) - angle(from) + std::f32::consts::PI)
                        .rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                    self.gesture(center, center, 1., delta)
                }
            };
        }
        if phase != ContactPhase::Up { return Ok(UiChange::default()); }
        if matches!(contact.mode, NavigationMode::Zoom | NavigationMode::ZoomOut) {
            let scale = if contact.mode == NavigationMode::ZoomOut { 0.5_f32.sqrt() } else { 2_f32.sqrt() };
            return self.gesture(contact.origin, contact.origin, scale, 0.);
        }
        Ok(UiChange::default())
    }
    pub(super) fn navigation_key(&mut self, key: &str, pressed: bool, reply: &mut InputReply) -> Result<bool, String> {
        if self.navigation_mode() != Some(NavigationMode::Pan) || !self.navigation_idle()
            || self.interaction.modifiers.command || self.interaction.modifiers.alt { return Ok(false); }
        let direction = match key {
            "arrowleft" => [1., 0.], "arrowright" => [-1., 0.],
            "arrowup" | "pageup" => [0., 1.], "arrowdown" | "pagedown" => [0., -1.],
            _ => return Ok(false),
        };
        if pressed {
            self.interaction.navigation_tap = false;
            self.remember_view();
            let camera = &self.state.camera;
            let center = camera.work_area_center();
            let amount = if key.starts_with("page") { camera.work_area[3] * 0.9 } else { 40. *
                self.logical_viewport.map_or(1., |v| camera.viewport[0] as f32 / v[0]) };
            reply.change = self.gesture(center, [center[0] + direction[0] * amount, center[1] + direction[1] * amount], 1., 0.)?;
        }
        reply.handled = true;
        reply.pan_cursor = true;
        reply.navigation_cursor = Some(NavigationMode::Pan);
        Ok(true)
    }
    pub(super) fn navigate_command(&mut self, command: CommandId) -> Result<(), String> {
        use CommandId as C;
        let position = ViewPosition::capture(&self.state.camera);
        if command == C::SaveView { self.navigation.saved = Some(position); return Ok(()); }
        let restored = match command { C::PreviousView => self.navigation.previous, C::RestoreView => self.navigation.saved, _ => None };
        self.remember_view();
        let size = self.engine.document().composition().size;
        let selection = self.engine.document().working.selection.as_ref().map(|selection| {
            if selection.inverted { layer_core::Rect::from_extent(size) } else { selection.bounds() }
        }).filter(|bounds| !bounds.is_empty());
        let camera = &mut self.state.camera;
        if let Some(position) = restored { position.restore(camera); }
        else {
            match command {
                C::ResetView => { camera.rotation = 0.; camera.flipped = [false; 2]; camera.fit(size); }
                C::FitCanvas | C::FitWidth | C::FillView => {
                    camera.fit_bounds([0., 0., size[0] as f32, size[1] as f32], command == C::FitWidth, command == C::FillView);
                }
                C::ZoomSelection => if let Some(bounds) = selection {
                    camera.fit_bounds([bounds.min.x, bounds.min.y, bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y], false, false);
                },
                C::ActualPixels => camera.zoom_to(1.)?,
                C::ResetRotation => camera.rotate_to(0.)?,
                C::FlipHorizontal | C::FlipVertical => camera.flip(command == C::FlipHorizontal),
                C::ZoomIn | C::ZoomOut | C::RotateLeft | C::RotateRight => {
                    let step: f32 = if self.state.settings.keymap.as_ref().is_some_and(|p| p.id == "krita") { 15. } else { 5. };
                    let scale = match command { C::ZoomIn => 2_f32.sqrt(), C::ZoomOut => 0.5_f32.sqrt(), _ => 1. };
                    let rotation = match command { C::RotateLeft => -step.to_radians(), C::RotateRight => step.to_radians(), _ => 0. };
                    let center = camera.work_area_center();
                    camera.transform(center, center, scale, rotation)?;
                }
                _ => {}
            }
        }
        self.initial_fit = false;
        self.sync_camera();
        Ok(())
    }
}
