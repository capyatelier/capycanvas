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

/// Strip viewport facts without changing the user's portable logical layout.
pub fn durable_layout(layout: &DockLayout) -> DockLayout {
    let mut layout = layout.clone();
    layout.measurements.clear();
    layout.column_scroll.clear();
    layout.titlebar_insets = [0.0; 3];
    layout
}

/// JSON/JavaScript must never round a generation counter through an f64.
pub(crate) mod counter {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutRevision {
    /// Scoped to this workspace's history; packages remap references on import.
    pub id: String,
    pub layout: DockLayout,
    pub description: String,
    #[serde(with = "counter")]
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutHistory {
    pub version: u32,
    #[serde(with = "counter")]
    pub generation: u64,
    pub current: String,
    pub undo: Vec<String>,
    pub redo: Vec<String>,
    pub revisions: std::collections::BTreeMap<String, LayoutRevision>,
}
impl LayoutHistory {
    pub fn new(layout: &DockLayout) -> Self {
        let revision = LayoutRevision {
            id: "r0".into(),
            layout: durable_layout(layout),
            description: "Starting configuration".into(),
            timestamp_ms: 0,
        };
        Self {
            version: 1,
            generation: 0,
            current: revision.id.clone(),
            undo: Vec::new(),
            redo: Vec::new(),
            revisions: [(revision.id.clone(), revision)].into(),
        }
    }
    pub fn layout(&self) -> &DockLayout {
        &self.revisions[&self.current].layout
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported layout history version".into());
        }
        if self.generation == u64::MAX
            || self.revisions.len() > 100_000
            || self.undo.len() > 100
            || self.redo.len() > 100
        {
            return Err("Layout history exceeds supported limits".into());
        }
        for id in std::iter::once(&self.current)
            .chain(&self.undo)
            .chain(&self.redo)
        {
            if !self.revisions.contains_key(id) {
                return Err("Missing layout history revision".into());
            }
        }
        for (id, revision) in &self.revisions {
            if id.is_empty()
                || id.len() > 128
                || id != &revision.id
                || revision.description.len() > 1024
                || !revision.layout.measurements.is_empty()
                || !revision.layout.column_scroll.is_empty()
                || revision.layout.titlebar_insets != [0.0; 3]
            {
                return Err("Invalid layout history revision".into());
            }
            revision.layout.validate()?;
        }
        Ok(())
    }
    fn advance(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("workspace generation exhausted");
    }
    pub fn append(&mut self, layout: &DockLayout, description: &str) {
        let layout = durable_layout(layout);
        if self.layout() == &layout {
            return;
        }
        self.advance();
        let mut id = format!("r{}", self.generation);
        // Imported IDs are opaque and need not have been allocated by this version.
        while self.revisions.contains_key(&id) {
            id.push('_');
        }
        self.revisions.insert(
            id.clone(),
            LayoutRevision {
                id: id.clone(),
                layout,
                description: description.into(),
                timestamp_ms: 0,
            },
        );
        self.undo.push(std::mem::replace(&mut self.current, id));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear(); // Retain the abandoned content for Layout History.
    }
    pub fn undo(&mut self) -> bool {
        let Some(id) = self.undo.pop() else {
            return false;
        };
        self.advance();
        self.redo.push(std::mem::replace(&mut self.current, id));
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(id) = self.redo.pop() else {
            return false;
        };
        self.advance();
        self.undo.push(std::mem::replace(&mut self.current, id));
        true
    }
}

/// Only the in-flight gesture uses the legacy snapshot, to serve existing drag
/// projections. Durable history stores layouts and never restores working values.
#[derive(Default)]
pub(crate) struct WorkspaceHistory {
    durable: Option<LayoutHistory>,
    gesture: Option<WorkspaceState>,
}
impl WorkspaceHistory {
    fn adopt_layout(state: &mut WorkspaceState, mut layout: DockLayout) {
        layout.measurements.clone_from(&state.layout.measurements);
        layout.column_scroll.clone_from(&state.layout.column_scroll);
        layout.titlebar_insets = state.layout.titlebar_insets;
        state.layout = layout;
    }
    pub fn capture(&mut self, state: &WorkspaceState) -> LayoutHistory {
        let history = self
            .durable
            .get_or_insert_with(|| LayoutHistory::new(&state.layout));
        history.append(&state.layout, "Arrange panels and toolbars");
        history.clone()
    }
    pub fn restore(history: LayoutHistory) -> Self {
        Self {
            durable: Some(history),
            gesture: None,
        }
    }
    pub fn gesture_start(&self) -> Option<&WorkspaceState> {
        self.gesture.as_ref()
    }
    pub fn can_undo(&self) -> bool {
        self.gesture.is_none() && self.durable.as_ref().is_some_and(|h| !h.undo.is_empty())
    }
    pub fn can_redo(&self) -> bool {
        self.gesture.is_none() && self.durable.as_ref().is_some_and(|h| !h.redo.is_empty())
    }
    pub fn record(&mut self, before: WorkspaceState, after: &WorkspaceState) {
        self.record_named(before, after, "Arrange panels and toolbars");
    }
    pub fn record_named(
        &mut self,
        before: WorkspaceState,
        after: &WorkspaceState,
        description: &str,
    ) {
        if self.gesture.is_some() || durable_layout(&before.layout) == durable_layout(&after.layout)
        {
            return;
        }
        let history = self
            .durable
            .get_or_insert_with(|| LayoutHistory::new(&before.layout));
        history.append(&before.layout, "Arrange panels and toolbars");
        history.append(&after.layout, description);
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
            self.cancel(state);
        } else {
            self.finish(state);
        }
    }
    pub fn cancel(&mut self, state: &mut WorkspaceState) {
        if let Some(before) = self.gesture.take() {
            Self::adopt_layout(state, before.layout);
        }
    }
    pub fn undo(&mut self, state: &mut WorkspaceState) {
        if let Some(history) = self.durable.as_mut()
            && history.undo()
        {
            Self::adopt_layout(state, history.layout().clone());
        }
    }
    pub fn redo(&mut self, state: &mut WorkspaceState) {
        if let Some(history) = self.durable.as_mut()
            && history.redo()
        {
            Self::adopt_layout(state, history.layout().clone());
        }
    }
}

/// Latest-only semantic values, independent of layout/history/template payloads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceWorkingState {
    pub version: u32,
    pub preset: u32,
    pub tools: crate::WorkspaceToolMemory,
    pub colors: crate::ColorState,
    pub canvas_tool: crate::LayerCanvasTool,
    pub region_values: std::collections::BTreeMap<String, f32>,
    pub region_sources: [crate::RegionSource; 2],
    pub gradient: [bool; 2],
    pub figure: (crate::FigureShape, crate::FigurePaint),
    pub zen_mode: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCapture {
    pub history: LayoutHistory,
    pub working: WorkspaceWorkingState,
}
