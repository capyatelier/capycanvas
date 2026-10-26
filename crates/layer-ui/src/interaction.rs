//! Transient application interaction state. Never saved in a workspace and
//! never mixed into the high-rate pen history queue.
use crate::{ContactPhase, UiChange};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    /// Ctrl on Linux/Windows; Ctrl or Command where supplied by the host.
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Whether a focused native button, switch, check box or list row takes `key`
/// for its own activation instead of the canvas.
pub fn control_takes_key(key: &str, modifiers: Modifiers, keyboard_focus: bool) -> bool {
    !modifiers.command && !modifiers.alt && match key.to_ascii_lowercase().as_str() {
        "enter" => true,
        " " | "space" => keyboard_focus,
        _ => false,
    }
}

pub fn settings_search_key(key: &str, modifiers: Modifiers, editing: bool, capturing: bool) -> bool {
    !editing && !capturing && !modifiers.command && !modifiers.alt
        && key.chars().count() == 1
        && key.chars().all(|c| !c.is_control() && !c.is_whitespace())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerKind {
    Pen,
    Mouse,
    Touch,
}

/// The system's choice for a stylus gesture, such as an Apple Pencil double-tap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylusAction {
    SwitchEraser,
    SwitchPrevious,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PenButton {
    Primary,
    Secondary,
    Tertiary,
}
impl PenButton {
    pub fn trigger(self) -> &'static str {
        match self {
            Self::Primary => "pen.button.primary",
            Self::Secondary => "pen.button.secondary",
            Self::Tertiary => "pen.button.tertiary",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TouchPolicy {
    pub tap_ms: u32,
    pub slop: f32,
}
impl Default for TouchPolicy {
    fn default() -> Self {
        Self { tap_ms: 500, slop: 16.0 }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum TouchGesture {
    Tap(u8),
    Hold,
    QuickPinch { outward: bool },
    Scrub,
}

#[derive(Default)]
pub(crate) struct TouchGestures {
    contacts: std::collections::BTreeMap<u64, ([f32; 2], [f32; 2], u64)>,
    first_ns: u64,
    last_ns: u64,
    peak: usize,
    lifted: bool,
    failed: bool,
    tap_failed: bool,
    scrub: Option<TouchScrub>,
    pinch: Option<([[f32; 2]; 2], u64)>,
    quick_pinch: Option<bool>,
    first_lift_ns: u64,
    repeat_ns: Option<u64>,
    repeated: bool,
    pub scrubbed: bool,
    pub camera: Option<crate::Camera>,
}
impl TouchGestures {
    pub fn active(&self) -> bool {
        !self.contacts.is_empty()
    }
    pub fn fail(&mut self) {
        self.failed |= self.active();
    }
    pub fn suspends_navigation(&self) -> bool {
        self.contacts.len() >= 3 || ((self.scrubbed || self.repeated) && self.active())
    }
    pub fn repeat_pending(&self) -> bool {
        !self.failed && !self.tap_failed && !self.lifted && (2..=3).contains(&self.contacts.len())
    }
    pub fn repeat_fingers(&self) -> usize { self.contacts.len() }
    pub fn advance(&mut self, now_ns: u64, policy: TouchPolicy) -> Option<TouchGesture> {
        if !self.repeat_pending() { self.repeat_ns = None; return None; }
        let deadline = self.repeat_ns.get_or_insert(now_ns.saturating_add(u64::from(policy.tap_ms) * 1_000_000));
        if now_ns < *deadline { return None; }
        *deadline = now_ns.saturating_add(80_000_000);
        self.repeated = true;
        self.scrub = None;
        Some(TouchGesture::Hold)
    }
    pub(crate) fn contact_position(&self,id:u64)->Option<[f32;2]> {self.contacts.get(&id).map(|(_,current,_)|*current)}
    pub fn contact(
        &mut self,
        id: u64,
        phase: ContactPhase,
        position: [f32; 2],
        time_ns: u64,
        policy: TouchPolicy,
        eligible: bool,
    ) -> Option<TouchGesture> {
        if phase == ContactPhase::Down && !self.active() {
            *self = Self { first_ns: time_ns, camera: self.camera.take(), ..Self::default() };
        }
        self.failed |= !eligible || time_ns == 0 || self.contacts.get(&id).is_some_and(|(_, _, last)| time_ns < *last)
            || !position.into_iter().all(f32::is_finite);
        self.last_ns = self.last_ns.max(time_ns);
        match phase {
            ContactPhase::Down => {
                self.repeat_ns = None;
                self.failed |= self.lifted || self.contacts.contains_key(&id);
                self.first_ns = self.first_ns.min(time_ns);
                self.contacts.insert(id, (position, position, time_ns));
                self.peak = self.peak.max(self.contacts.len());
                self.pinch = (self.peak == 2).then(|| ([self.positions()[0], self.positions()[1]], time_ns));
                self.scrub = (self.peak == 3 && !self.failed && !self.tap_failed
                    && time_ns.saturating_sub(self.first_ns) <= u64::from(policy.tap_ms) * 1_000_000)
                    .then(|| TouchScrub::new(self.positions()));
            }
            ContactPhase::Move | ContactPhase::Up => {
                let (start, current, last) = self.contacts.get_mut(&id)?;
                *current = position;
                *last = time_ns;
                self.tap_failed |= (position[0] - start[0]).hypot(position[1] - start[1]) > policy.slop;
                if phase == ContactPhase::Up {
                    if !self.lifted {
                        self.first_lift_ns = time_ns;
                        self.quick_pinch = self.pinch.and_then(|(origins, start)| {
                            if self.peak != 2 || self.repeated || time_ns.saturating_sub(start) > 300_000_000 { return None; }
                            let positions = self.positions();
                            let vector = |p: [[f32; 2]; 2]| [p[1][0] - p[0][0], p[1][1] - p[0][1]];
                            let before = vector(origins);
                            let after = vector([positions[0], positions[1]]);
                            let length = |p: [f32; 2]| p[0].hypot(p[1]);
                            let [a, b] = [length(before), length(after)];
                            let travel = (a - b).abs();
                            if a < policy.slop * 4. || travel < policy.slop * 2. || (0.75..1.3333334).contains(&(b / a)) { return None; }
                            let center_travel = length(std::array::from_fn(|i| (positions[0][i] + positions[1][i] - origins[0][i] - origins[1][i]) * 0.5));
                            if center_travel > travel * 0.5 || (before[0] * after[1] - before[1] * after[0]).abs() > a * b * 0.45 { return None; }
                            if (0..2).any(|i| length(std::array::from_fn(|axis| positions[i][axis] - origins[i][axis])) < policy.slop) { return None; }
                            Some(b > a)
                        });
                    }
                    self.contacts.remove(&id);
                    self.scrub = None;
                    self.lifted = true;
                    self.tap_failed |= self.last_ns.saturating_sub(self.first_ns) > u64::from(policy.tap_ms) * 1_000_000;
                    if !self.active() && !self.failed && time_ns.saturating_sub(self.first_lift_ns) <= 100_000_000
                        && let Some(outward) = self.quick_pinch.take() {
                        return Some(TouchGesture::QuickPinch { outward });
                    }
                    if !self.active() && !self.failed && !self.tap_failed && !self.repeated && (2..=4).contains(&self.peak) {
                        return Some(TouchGesture::Tap(self.peak as u8));
                    }
                } else if !self.failed && self.last_ns.saturating_sub(self.first_ns) <= 2_000_000_000 {
                    let positions = self.positions();
                    if self.scrub.as_mut().is_some_and(|scrub| scrub.update(positions, policy.slop * 2.)) {
                        self.scrub = None;
                        self.tap_failed = true;
                        self.scrubbed = true;
                        return Some(TouchGesture::Scrub);
                    }
                }
            }
            ContactPhase::Cancel => {
                self.contacts.remove(&id)?;
                self.failed = true;
                self.scrub = None;
            }
        }
        None
    }

    fn positions(&self) -> [[f32; 2]; 3] {
        let mut positions = [[0.; 2]; 3];
        for (position, (_, current, _)) in positions.iter_mut().zip(self.contacts.values()) {
            *position = *current;
        }
        positions
    }
}

struct TouchScrub {
    origins: [[f32; 2]; 3],
    axis: Option<[f32; 2]>,
    extremes: [f32; 3],
    legs: u8,
    failed: bool,
}
impl TouchScrub {
    fn new(origins: [[f32; 2]; 3]) -> Self {
        Self { origins, axis: None, extremes: [0.; 3], legs: 0, failed: false }
    }

    fn update(&mut self, positions: [[f32; 2]; 3], distance: f32) -> bool {
        if self.failed { return false; }
        let offsets: [[f32; 2]; 3] = std::array::from_fn(|i| [positions[i][0] - self.origins[i][0], positions[i][1] - self.origins[i][1]]);
        let axis = if let Some(axis) = self.axis { axis } else {
            let center = offsets.iter().fold([0.; 2], |sum, p| [sum[0] + p[0] / 3., sum[1] + p[1] / 3.]);
            let length = center[0].hypot(center[1]);
            if length < distance { return false; }
            [center[0] / length, center[1] / length]
        };
        if offsets.iter().any(|p| (p[0] * axis[1] - p[1] * axis[0]).abs() > distance) {
            self.failed = true;
            return false;
        }
        let projected = offsets.map(|p| p[0] * axis[0] + p[1] * axis[1]);
        if self.legs == 0 {
            if projected.iter().all(|p| *p >= distance) {
                self.axis = Some(axis);
                self.extremes = projected;
                self.legs = 1;
            }
        } else {
            let direction = if self.legs == 1 { 1. } else { -1. };
            for (extreme, current) in self.extremes.iter_mut().zip(projected) {
                if (current - *extreme) * direction > 0. { *extreme = current; }
            }
            if projected.iter().zip(self.extremes).all(|(current, extreme)| (extreme - current) * direction >= distance) {
                self.extremes = projected;
                self.legs += 1;
            }
        }
        self.legs == 3
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Primary,
    Pan,
    Other,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ChromeFacts {
    /// Native title-bar grab (or DOM header contact), including WM-owned grabs.
    pub held: bool,
    /// Native tool-tile DND only. DragWorkspace owns its own Zen lifecycle.
    pub dragging: bool,
    pub popup_open: bool,
    /// Actual animated columns from shared layout, for outside contact.
    #[serde(default)]
    pub expanded_panel: Option<crate::PanelExpansion>,
    /// Actual animated content drawer; origin hit-testing stays in Rust.
    #[serde(default)]
    pub content_drawer: Option<crate::Bounds>,
    #[serde(default)]
    pub drawer_connection: Option<crate::Bounds>,
    /// Native/DOM tab hit for the current contact; labels have host-measured widths.
    #[serde(default)]
    pub contact_tab: Option<crate::Panel>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChromeEvent {
    Refresh,
    Motion,
    Contact { position: [f32; 2], canvas: bool },
}

/// Positions are logical window units for chrome, physical canvas pixels for
/// pointers. Native timestamps, coalesced samples and pointer capture stay in
/// adapters. One pointer event chooses the route for its entire history batch.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiInput {
    Chrome {
        event: ChromeEvent,
        facts: ChromeFacts,
        viewport: [f32; 2],
    },
    Key {
        key: String,
        pressed: bool,
        #[serde(default)]
        repeat: bool,
        #[serde(default)]
        modifiers: Modifiers,
        #[serde(default)]
        editing: bool,
        #[serde(default)]
        divider: Option<u32>,
    },
    Pointer {
        id: u64,
        phase: ContactPhase,
        kind: PointerKind,
        button: PointerButton,
        position: [f32; 2],
        #[serde(default)]
        time_ns: u64,
    },
    TrackpadTouch {
        id: u64,
        phase: ContactPhase,
        position: [f32; 2],
        time_ns: u64,
    },
    LayerTouch {
        surface: u64,
        id: u64,
        phase: ContactPhase,
        position: [f32; 2],
        row: Option<u64>,
        time_ns: u64,
    },
    ColorTouch {
        surface: u64,
        id: u64,
        phase: ContactPhase,
        position: [f32; 2],
        time_ns: u64,
    },
    EdgeContact {
        id: u64,
        phase: ContactPhase,
        kind: PointerKind,
        contact: crate::EdgeContact,
        position: [f32; 2],
        viewport: [f32; 2],
        time_ns: u64,
    },
    EdgeDoublePress {
        kind: PointerKind,
        contact: crate::EdgeContact,
        position: [f32; 2],
        viewport: [f32; 2],
    },
    PenButton {
        button: PenButton,
        pressed: bool,
    },
    StylusAction {
        action: StylusAction,
    },
    Axes {
        pan: [f32; 2],
        zoom: f32,
    },
    CursorLeave,
    ColorPickerHold { id: u64, position: [f32; 2], offset: f32 },
    /// Focus loss/unmap cancels canvas ownership; the host queues its last raw
    /// pen sample as Cancel when `cancel_paint` is returned.
    Blur,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct InputReply {
    pub change: UiChange,
    pub handled: bool,
    pub paint: bool,
    pub cancel_paint: bool,
    pub dismiss_popups: bool,
    pub chrome_hidden: bool,
    /// Whether to show the standalone Capy while chrome is hidden.
    pub keep_zen_button: bool,
    pub pan_cursor: bool,
    pub navigation_cursor: Option<NavigationMode>,
    pub edge_hold: bool,
    pub edge_cursor: Option<&'static str>,
    pub edge_select_on_release: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationMode {
    #[default]
    Pan,
    Zoom,
    ZoomOut,
    Rotate,
}

#[derive(Clone, Copy)]
pub(crate) struct NavigationContact {
    pub mode: NavigationMode,
    pub origin: [f32; 2],
    pub dragged: bool,
    pub zoom: crate::ZoomToolSettings,
}

#[derive(Clone, Copy)]
pub(crate) struct PointerContact {
    pub id: u64,
    pub kind: PointerKind,
    pub paint: bool,
    pub position: [f32; 2],
    pub navigation: Option<NavigationContact>,
}

/// What a momentary binding replaced, restored on release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Restore {
    Toggle(crate::CommandId, bool),
    Slot(crate::ColorSlot),
    Tool(crate::LayerCanvasTool, u32),
}

/// A tool or mode key held down: releasing it after use returns to `restore`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spring {
    pub key: String,
    pub restore: Restore,
    pub used: bool,
}

#[derive(Default)]
pub(crate) struct Interaction {
    pub modifiers: Modifiers,
    pub hidden: bool,
    pub navigation: Option<(String, NavigationMode)>,
    pub navigation_tap: bool,
    pub keep_chrome_until_contact: bool,
    pub facts: ChromeFacts,
    pub viewport: Option<[f32; 2]>,
    pub pointer: Option<PointerContact>,
    pub keys: std::collections::BTreeSet<String>,
    pub holds: Vec<(String, crate::UiAction)>,
    pub held_tool: Option<crate::UiAction>,
    pub momentary: Vec<(String, Restore)>,
    pub restores: Vec<Restore>,
    /// The eraser end's tool stays in use while that end is near the tablet.
    pub eraser_end: bool,
    /// Regions a hold changed outside `input`, reported by the next frame.
    pub hold_regions: u32,
    /// Keys held down, oldest first, by their canonical names.
    pub pressed: Vec<String>,
    /// Modifier keys in effect, by token, with the action each holds.
    pub modifier_holds: Vec<(String, String)>,
    /// Modifier keys an explicit tool choice ended, ignored until released.
    pub suppressed: Vec<String>,
    pub spring: Option<Spring>,
    pub hold_base: Option<(crate::LayerCanvasTool, u32)>,
    /// The current and previous tools the artist chose, ignoring holds.
    pub tools: [Option<(crate::LayerCanvasTool, u32)>; 2],
    pub applying_hold: bool,
    pub gestures: TouchGestures,
    pub touch_policy: TouchPolicy,
    pub axes: NavigationAxes,
}

#[derive(Default)]
pub(crate) struct NavigationAxes {
    pan: [f32; 2],
    zoom: f32,
    last_ns: Option<u64>,
}
impl NavigationAxes {
    const DEAD_ZONE: f32 = 0.15;
    fn shaped(value: f32) -> f32 {
        let magnitude = ((value.abs() - Self::DEAD_ZONE) / (1. - Self::DEAD_ZONE)).clamp(0., 1.);
        magnitude * magnitude * value.signum()
    }
    pub fn set(&mut self, pan: [f32; 2], zoom: f32) -> Result<bool, String> {
        if !pan.into_iter().chain([zoom]).all(|v| v.is_finite() && v.abs() <= 1.) {
            return Err("Axis values must be between -1 and 1".into());
        }
        let length = pan[0].hypot(pan[1]);
        let radial = if length > Self::DEAD_ZONE { Self::shaped(length) / length } else { 0. };
        self.pan = pan.map(|v| v * radial);
        self.zoom = Self::shaped(zoom);
        if !self.active() {
            self.last_ns = None;
        }
        Ok(self.active())
    }
    pub fn active(&self) -> bool {
        self.pan != [0.; 2] || self.zoom != 0.
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn advance(&mut self, now_ns: u64) -> Option<([f32; 2], f32)> {
        if !self.active() {
            return None;
        }
        let dt = self.last_ns.map_or(0., |last| now_ns.saturating_sub(last) as f32 / 1e9).min(0.1);
        self.last_ns = Some(now_ns);
        (dt > 0.).then(|| (self.pan.map(|v| v * dt), self.zoom * dt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_search_preserves_text_and_respects_native_ownership() {
        let plain = Modifiers::default();
        for key in ["p", "P", "é", "日", "/"] {
            assert!(settings_search_key(key, plain, false, false), "{key}");
            assert!(settings_search_key(key, Modifiers { shift: true, ..plain }, false, false));
            assert!(!settings_search_key(key, plain, true, false));
            assert!(!settings_search_key(key, plain, false, true));
            assert!(!settings_search_key(key, Modifiers { command: true, ..plain }, false, false));
            assert!(!settings_search_key(key, Modifiers { alt: true, ..plain }, false, false));
        }
        for key in ["", " ", "\t", "\n", "\u{1b}", "\u{7f}", "arrowup", "home", "enter", "f1", "f24"] {
            assert!(!settings_search_key(key, plain, false, false), "{key:?}");
        }
    }

    #[test]
    fn space_activates_only_a_keyboard_focused_control_and_enter_always_does() {
        let plain = Modifiers::default();
        assert!(control_takes_key(" ", plain, true));
        assert!(!control_takes_key(" ", plain, false), "a clicked button leaves Space to the canvas");
        assert!(control_takes_key("Enter", plain, false) && control_takes_key("enter", plain, true));
        assert!(!control_takes_key(" ", Modifiers { command: true, ..plain }, true));
        assert!(!control_takes_key("Enter", Modifiers { alt: true, ..plain }, false));
        assert!(control_takes_key(" ", Modifiers { shift: true, ..plain }, true));
        assert!(!control_takes_key("a", plain, true));
    }
}
