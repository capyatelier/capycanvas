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

/// UI topology only: never document pixels, engine commands or transient menus.
#[derive(Default)]
pub(crate) struct WorkspaceHistory {
    undo: Vec<WorkspaceState>,
    redo: Vec<WorkspaceState>,
    gesture: Option<WorkspaceState>,
}
impl WorkspaceHistory {
    pub fn can_undo(&self) -> bool {
        self.gesture.is_none() && !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        self.gesture.is_none() && !self.redo.is_empty()
    }
    pub fn record(&mut self, mut before: WorkspaceState, after: &WorkspaceState) {
        before
            .layout
            .measurements
            .clone_from(&after.layout.measurements);
        if before != *after {
            self.undo.push(before);
            self.redo.clear();
        }
    }
    pub fn begin(&mut self, state: &WorkspaceState) {
        self.gesture.get_or_insert_with(|| state.clone());
    }
    pub fn finish(&mut self, state: &WorkspaceState) {
        if let Some(before) = self.gesture.take() {
            self.record(before, state);
        }
    }
    pub fn cancel(&mut self, state: &mut WorkspaceState) {
        if let Some(mut before) = self.gesture.take() {
            before
                .layout
                .measurements
                .clone_from(&state.layout.measurements);
            *state = before;
        }
    }
    pub fn undo(&mut self, state: &mut WorkspaceState) {
        if let Some(mut before) = self.undo.pop() {
            before
                .layout
                .measurements
                .clone_from(&state.layout.measurements);
            self.redo.push(std::mem::replace(state, before));
        }
    }
    pub fn redo(&mut self, state: &mut WorkspaceState) {
        if let Some(mut after) = self.redo.pop() {
            after
                .layout
                .measurements
                .clone_from(&state.layout.measurements);
            self.undo.push(std::mem::replace(state, after));
        }
    }
}
