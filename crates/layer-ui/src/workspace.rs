//! Portable, durable workspace state. Native widgets and in-flight gestures
//! are deliberately absent: a new session can restore the same value.

use crate::DockLayout;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceState {
    pub version: u32,
    pub layout: DockLayout,
    pub zen_mode: bool,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            version: 1,
            layout: DockLayout::default(),
            zen_mode: false,
        }
    }
}

impl WorkspaceState {
    /// Validate before replacing live state. Storage/transport belongs to the
    /// host; accepted topology and versioning never do.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported workspace version".into());
        }
        self.layout.validate()
    }
}
