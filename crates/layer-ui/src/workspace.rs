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
    pub fn for_platform(platform: crate::Platform) -> Self {
        Self {
            layout: DockLayout::for_platform(platform),
            ..Self::default()
        }
    }

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
    fn retain_measurements(target: &mut WorkspaceState, source: &WorkspaceState) {
        target
            .layout
            .measurements
            .clone_from(&source.layout.measurements);
        target
            .layout
            .column_scroll
            .clone_from(&source.layout.column_scroll);
    }
    pub fn gesture_start(&self) -> Option<&WorkspaceState> {
        self.gesture.as_ref()
    }
    pub fn can_undo(&self) -> bool {
        self.gesture.is_none() && !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        self.gesture.is_none() && !self.redo.is_empty()
    }
    pub fn record(&mut self, mut before: WorkspaceState, after: &WorkspaceState) {
        Self::retain_measurements(&mut before, after);
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
    pub fn finish_move(&mut self, state: &mut WorkspaceState) {
        if self
            .gesture
            .as_ref()
            .is_some_and(|before| before.layout.same_placement(&state.layout))
        {
            // A temporary tear-off may have changed sizes/IDs before a drop
            // back into the original slot. Restore the pre-drag layout exactly.
            self.cancel(state);
        } else {
            self.finish(state);
        }
    }
    pub fn cancel(&mut self, state: &mut WorkspaceState) {
        if let Some(mut before) = self.gesture.take() {
            Self::retain_measurements(&mut before, state);
            *state = before;
        }
    }
    pub fn undo(&mut self, state: &mut WorkspaceState) {
        if let Some(mut before) = self.undo.pop() {
            Self::retain_measurements(&mut before, state);
            self.redo.push(std::mem::replace(state, before));
        }
    }
    pub fn redo(&mut self, state: &mut WorkspaceState) {
        if let Some(mut after) = self.redo.pop() {
            Self::retain_measurements(&mut after, state);
            self.undo.push(std::mem::replace(state, after));
        }
    }
}
