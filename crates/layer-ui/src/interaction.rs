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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerKind {
    Pen,
    Mouse,
    Touch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PenButton {
    Primary,
    Secondary,
}
impl PenButton {
    pub fn trigger(self) -> &'static str {
        match self {
            Self::Primary => "pen.button.primary",
            Self::Secondary => "pen.button.secondary",
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

#[derive(Default)]
pub(crate) struct TouchTaps {
    starts: std::collections::BTreeMap<u64, [f32; 2]>,
    first_ns: u64,
    peak: usize,
    lifted: bool,
    failed: bool,
    pub camera: Option<crate::Camera>,
}
impl TouchTaps {
    pub fn active(&self) -> bool {
        !self.starts.is_empty()
    }
    pub fn fail(&mut self) {
        self.failed |= self.active();
    }
    pub fn contact(
        &mut self,
        id: u64,
        phase: ContactPhase,
        position: [f32; 2],
        time_ns: u64,
        policy: TouchPolicy,
        eligible: bool,
    ) -> Option<u8> {
        match phase {
            ContactPhase::Down => {
                if !self.active() {
                    *self = Self { first_ns: time_ns, camera: self.camera.take(), ..Self::default() };
                }
                self.failed |= !eligible || time_ns == 0 || self.lifted;
                self.starts.insert(id, position);
                self.peak = self.peak.max(self.starts.len());
            }
            ContactPhase::Move => {
                let start = self.starts.get(&id)?;
                self.failed |= !eligible || (position[0] - start[0]).hypot(position[1] - start[1]) > policy.slop;
            }
            ContactPhase::Up => {
                self.starts.remove(&id)?;
                self.lifted = true;
                self.failed |= time_ns < self.first_ns
                    || time_ns - self.first_ns > u64::from(policy.tap_ms) * 1_000_000;
                if !self.active() && !self.failed && (2..=4).contains(&self.peak) {
                    return Some(self.peak as u8);
                }
            }
            ContactPhase::Cancel => {
                self.starts.remove(&id)?;
                self.failed = true;
            }
        }
        None
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
    /// Visible standalone Capy hit target; using it must not reveal over the button.
    #[serde(default)]
    pub zen_button: Option<crate::Bounds>,
    /// Visible canvas action bar; hovering it must not reveal docked chrome.
    #[serde(default)]
    pub canvas_bar: Option<crate::Bounds>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChromeEvent {
    Refresh,
    Motion { position: [f32; 2] },
    Leave { touch: bool },
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
    PenButton {
        button: PenButton,
        pressed: bool,
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
    /// The canvas action bar hides while a canvas contact is in progress.
    pub canvas_bar_hidden: bool,
    /// Whether to show the standalone top-left Capy while chrome is hidden.
    pub keep_zen_button: bool,
    pub pan_cursor: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct PointerContact {
    pub id: u64,
    pub kind: PointerKind,
    pub paint: bool,
    pub position: [f32; 2],
}

#[derive(Default)]
pub(crate) struct Interaction {
    pub modifiers: Modifiers,
    pub hidden: bool,
    pub pan_key: Option<String>,
    pub keyboard_chrome: bool,
    pub keep_chrome_until_contact: bool,
    /// Fixed top-left guard after explicitly entering Zen. Not a preference.
    pub zen_entry_guard: bool,
    pub hover: Option<[f32; 2]>,
    pub facts: ChromeFacts,
    pub viewport: Option<[f32; 2]>,
    pub pointer: Option<PointerContact>,
    pub keys: std::collections::BTreeSet<String>,
    pub holds: Vec<(String, crate::CommandId)>,
    pub held_tool: Option<crate::CommandId>,
    pub hold_base: Option<(crate::LayerCanvasTool, u32)>,
    pub applying_hold: bool,
    pub taps: TouchTaps,
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
