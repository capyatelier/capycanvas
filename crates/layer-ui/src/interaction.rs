//! Transient application interaction state. Never saved in a workspace and
//! never mixed into the high-rate pen history queue.
use crate::{ContactPhase, UiChange};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
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
pub enum PointerButton {
    Primary,
    Pan,
    Other,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ChromeFacts {
    /// Native title-bar grab (or DOM header contact), including WM-owned grabs.
    pub held: bool,
    pub dragging: bool,
    pub popup_open: bool,
    /// Actual animated columns from shared layout, for outside contact.
    #[serde(default)]
    pub expanded_panel: Option<crate::PanelExpansion>,
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
    },
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
    pub pan_cursor: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct PointerContact {
    pub id: u64,
    pub paint: bool,
    pub position: [f32; 2],
}

#[derive(Default)]
pub(crate) struct Interaction {
    pub hidden: bool,
    pub pan_key: Option<String>,
    pub keyboard_chrome: bool,
    pub hover: Option<[f32; 2]>,
    pub facts: ChromeFacts,
    pub viewport: Option<[f32; 2]>,
    pub pointer: Option<PointerContact>,
    pub keys: std::collections::BTreeSet<String>,
}
