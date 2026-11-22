//! Portable, durable workspace state. Native widgets and in-flight gestures
//! are deliberately absent: a new session can restore the same value.

use crate::DockLayout;
use serde::{Deserialize, Serialize};
#[path = "workspace_description.rs"]
pub(crate) mod description;
pub use description::{layout_change_description, LayoutChange, LayoutPanelName, LayoutPanelAction};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub version: u32,
    pub layout: DockLayout,
    pub zen_mode: bool,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            version: 2,
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
        self.layout.validate()
    }
}

/// Strip viewport facts without changing the user's portable logical layout.
pub fn durable_layout(layout: &DockLayout) -> DockLayout {
    let mut layout = layout.clone();
    layout.measurements.clear();
    layout.layout_grid = 0.;
    layout.column_scroll.clear();
    for s in &mut layout.column_stacks {
        s.open_column = None;
    }
    layout.titlebar_insets = [0.0; 3];
    layout.bottom_inset = 0.0;
    layout.header_presentation = Default::default();
    layout
}

/// JSON/JavaScript must never round a generation counter through an f64.
pub mod counter {
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
pub struct LayoutRevision {
    /// Scoped to this workspace's history; packages remap references on import.
    pub id: String,
    pub layout: DockLayout,
    #[serde(default, deserialize_with = "crate::lenient::or_default")]
    pub description: LayoutChange,
    #[serde(with = "counter")]
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutHistory {
    #[serde(default, deserialize_with = "crate::lenient::or_default")]
    pub version: u32,
    #[serde(with = "counter")]
    pub generation: u64,
    pub current: String,
    #[serde(default, deserialize_with = "crate::lenient::vec")]
    pub undo: Vec<String>,
    #[serde(default, deserialize_with = "crate::lenient::vec")]
    pub redo: Vec<String>,
    #[serde(deserialize_with = "crate::lenient::map")]
    pub revisions: std::collections::BTreeMap<String, LayoutRevision>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutHead {
    #[serde(default, deserialize_with = "crate::lenient::or_default")]
    pub version: u32,
    #[serde(with = "counter")]
    pub generation: u64,
    pub current: LayoutRevision,
    #[serde(default, deserialize_with = "crate::lenient::vec")]
    pub undo: Vec<String>,
    #[serde(default, deserialize_with = "crate::lenient::vec")]
    pub redo: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutEdit {
    pub head: LayoutHead,
    #[serde(default, deserialize_with = "crate::lenient::map", skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub unsaved: std::collections::BTreeMap<String, LayoutRevision>,
}
impl LayoutRevision {
    pub fn validate(&self, id: &str) -> Result<(), String> {
        if id.is_empty()
            || id.len() > 128
            || id != self.id
            || !self.layout.measurements.is_empty()
            || !self.layout.column_scroll.is_empty()
            || self.layout.layout_grid != 0.
            || self.layout.titlebar_insets != [0.0; 3]
            || self.layout.bottom_inset != 0.0
            || self.layout.header_presentation != Default::default()
        {
            return Err("Invalid layout history revision".into());
        }
        self.description.validate()?;
        self.layout.validate()
    }
    pub fn retain_admitted(&mut self) {
        self.layout.retain_admitted();
        if self.description.validate().is_err() {
            self.description = LayoutChange::default();
        }
    }
}
fn within_limits(generation: u64, revisions: usize, undo: &[String], redo: &[String]) -> Result<(), String> {
    if generation == u64::MAX || revisions > 100_000 || undo.len() > 100 || redo.len() > 100 {
        return Err("Layout history exceeds supported limits".into());
    }
    Ok(())
}
impl LayoutHistory {
    pub fn new(layout: &DockLayout) -> Self {
        LayoutEdit::starting(layout).into_history()
    }
    pub fn layout(&self) -> &DockLayout {
        &self.revisions[&self.current].layout
    }
    pub fn head(&self) -> LayoutHead {
        LayoutHead {
            version: self.version,
            generation: self.generation,
            current: self.revisions[&self.current].clone(),
            undo: self.undo.clone(),
            redo: self.redo.clone(),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        within_limits(self.generation, self.revisions.len(), &self.undo, &self.redo)?;
        for id in std::iter::once(&self.current)
            .chain(&self.undo)
            .chain(&self.redo)
        {
            if !self.revisions.contains_key(id) {
                return Err("Missing layout history revision".into());
            }
        }
        for (id, revision) in &self.revisions {
            revision.validate(id)?;
        }
        Ok(())
    }
    pub fn retain_admitted(&mut self) {
        let current = self.current.clone();
        self.revisions.retain(|id, revision| {
            revision.retain_admitted();
            id == &current || revision.validate(id).is_ok()
        });
        let revisions = &self.revisions;
        self.undo.retain(|id| revisions.contains_key(id));
        self.redo.retain(|id| revisions.contains_key(id));
    }
}
impl LayoutHead {
    pub fn new(layout: &DockLayout) -> Self {
        Self {
            version: 2,
            generation: 0,
            current: LayoutRevision {
                id: "r0".into(),
                layout: durable_layout(layout),
                description: LayoutChange::Starting,
                timestamp_ms: 0,
            },
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
    pub fn layout(&self) -> &DockLayout {
        &self.current.layout
    }
    pub fn validate(&self) -> Result<(), String> {
        within_limits(self.generation, 0, &self.undo, &self.redo)?;
        if self.undo.iter().chain(&self.redo).any(|id| id.is_empty() || id.len() > 128) {
            return Err("Missing layout history revision".into());
        }
        self.current.validate(&self.current.id)
    }
}
impl LayoutEdit {
    pub fn new(head: LayoutHead) -> Self {
        Self { head, unsaved: Default::default() }
    }
    pub fn starting(layout: &DockLayout) -> Self {
        let head = LayoutHead::new(layout);
        Self { unsaved: [(head.current.id.clone(), head.current.clone())].into(), head }
    }
    pub fn layout(&self) -> &DockLayout {
        self.head.layout()
    }
    pub fn validate(&self) -> Result<(), String> {
        self.head.validate()?;
        within_limits(0, self.unsaved.len(), &[], &[])?;
        for (id, revision) in &self.unsaved {
            revision.validate(id)?;
        }
        Ok(())
    }
    fn advance(&mut self) {
        self.head.generation = self
            .head
            .generation
            .checked_add(1)
            .expect("workspace generation exhausted");
    }
    fn known(&self, id: &str) -> bool {
        self.head.current.id == id
            || self.head.undo.iter().chain(&self.head.redo).any(|known| known == id)
            || self.unsaved.contains_key(id)
    }
    pub fn append(&mut self, layout: &DockLayout, description: LayoutChange) {
        let layout = durable_layout(layout);
        if self.layout() == &layout {
            return;
        }
        let description = if description == LayoutChange::Automatic {
            layout_change_description(self.layout(), &layout)
        } else {
            description
        };
        self.advance();
        let mut id = format!("r{}", self.head.generation);
        while self.known(&id) {
            id.push('_');
        }
        let revision = LayoutRevision { id: id.clone(), layout, description, timestamp_ms: 0 };
        self.unsaved.insert(id, revision.clone());
        let previous = std::mem::replace(&mut self.head.current, revision);
        self.head.undo.push(previous.id);
        if self.head.undo.len() > 100 {
            self.head.undo.remove(0);
        }
        self.head.redo.clear();
    }
    pub fn target(&self, redo: bool) -> Option<&String> {
        if redo { self.head.redo.last() } else { self.head.undo.last() }
    }
    pub fn navigate(&mut self, redo: bool, revision: LayoutRevision) -> bool {
        if self.target(redo) != Some(&revision.id) {
            return false;
        }
        self.forget(redo);
        self.advance();
        let previous = std::mem::replace(&mut self.head.current, revision).id;
        if redo { self.head.undo.push(previous) } else { self.head.redo.push(previous) }
        true
    }
    pub fn step(&mut self, redo: bool) -> bool {
        match self.target(redo).and_then(|id| self.unsaved.get(id)).cloned() {
            Some(revision) => self.navigate(redo, revision),
            None => false,
        }
    }
    pub fn forget(&mut self, redo: bool) {
        if redo { self.head.redo.pop(); } else { self.head.undo.pop(); }
    }
    pub fn acknowledge(&mut self, ids: &[String]) {
        self.unsaved.retain(|id, _| !ids.contains(id));
    }
    pub fn into_history(self) -> LayoutHistory {
        let mut revisions = self.unsaved;
        revisions.insert(self.head.current.id.clone(), self.head.current.clone());
        let retained = |id: &String| revisions.contains_key(id);
        LayoutHistory {
            version: self.head.version,
            generation: self.head.generation,
            undo: self.head.undo.into_iter().filter(retained).collect(),
            redo: self.head.redo.into_iter().filter(retained).collect(),
            current: self.head.current.id,
            revisions,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutRead {
    pub revision: String,
    #[serde(with = "counter")]
    pub generation: u64,
    pub redo: bool,
}

/// Only the in-flight gesture uses the legacy snapshot, to serve existing drag
/// projections. Durable history stores layouts and never restores working values.
#[derive(Default, Clone)]
pub(crate) struct WorkspaceHistory {
    durable: Option<LayoutEdit>,
    read: Option<LayoutRead>,
    supplied: Option<(LayoutRead, Option<LayoutRevision>)>,
    gesture: Option<WorkspaceState>,
    gesture_description: Option<LayoutChange>,
}
impl WorkspaceHistory {
    pub fn generation(&self) -> Option<u64> {
        self.durable.as_ref().map(|h| h.head.generation)
    }
    fn adopt_layout(state: &mut WorkspaceState, mut layout: DockLayout) {
        layout.adopt_runtime(&state.layout);
        layout.column_scroll.clone_from(&state.layout.column_scroll);
        for s in &mut layout.column_stacks {
            s.open_column = s.open_column.or_else(|| state.layout.column_stack(s.column).open_column);
        }
        state.layout = layout;
    }
    pub fn capture(&mut self, state: &WorkspaceState) -> LayoutEdit {
        let history = self
            .durable
            .get_or_insert_with(|| LayoutEdit::starting(&state.layout));
        history.append(&state.layout, LayoutChange::Automatic);
        history.clone()
    }
    pub fn restore(history: LayoutEdit) -> Self {
        Self { durable: Some(history), ..Default::default() }
    }
    pub fn gesture_start(&self) -> Option<&WorkspaceState> {
        self.gesture.as_ref()
    }
    pub fn can_undo(&self) -> bool {
        self.gesture.is_none() && self.durable.as_ref().is_some_and(|h| h.target(false).is_some())
    }
    pub fn can_redo(&self) -> bool {
        self.gesture.is_none() && self.durable.as_ref().is_some_and(|h| h.target(true).is_some())
    }
    pub fn record(&mut self, before: WorkspaceState, after: &WorkspaceState) {
        self.record_named(before, after, LayoutChange::Automatic);
    }
    pub fn record_named(
        &mut self,
        before: WorkspaceState,
        after: &WorkspaceState,
        description: LayoutChange,
    ) {
        if self.gesture.is_some() || durable_layout(&before.layout) == durable_layout(&after.layout)
        {
            return;
        }
        let history = self
            .durable
            .get_or_insert_with(|| LayoutEdit::starting(&before.layout));
        history.append(&before.layout, LayoutChange::Automatic);
        history.append(&after.layout, description);
    }
    pub fn begin(&mut self, state: &WorkspaceState) {
        self.gesture.get_or_insert_with(|| state.clone());
    }
    pub fn begin_named(&mut self, state: &WorkspaceState, description: LayoutChange) {
        if self.gesture.is_none() {
            self.gesture_description = Some(description);
        }
        self.begin(state);
    }
    pub fn finish(&mut self, state: &WorkspaceState) {
        if let Some(before) = self.gesture.take() {
            let description = self
                .gesture_description
                .take()
                .unwrap_or_else(|| layout_change_description(&before.layout, &state.layout));
            self.record_named(before, state, description);
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
        self.gesture_description = None;
        if let Some(before) = self.gesture.take() {
            Self::adopt_layout(state, before.layout);
        }
    }
    pub fn undo(&mut self, state: &mut WorkspaceState) -> bool {
        self.navigate(false, state)
    }
    pub fn redo(&mut self, state: &mut WorkspaceState) -> bool {
        self.navigate(true, state)
    }
    fn navigate(&mut self, redo: bool, state: &mut WorkspaceState) -> bool {
        let mut supplied = self.supplied.take();
        let waiting = self.read.as_ref().is_some_and(|read| Some(read.generation) == self.generation());
        let Some(history) = self.durable.as_mut().filter(|_| !waiting && self.gesture.is_none()) else {
            return false;
        };
        loop {
            if history.step(redo) {
                Self::adopt_layout(state, history.layout().clone());
                return true;
            }
            let Some(target) = history.target(redo).cloned() else {
                return false;
            };
            let read = LayoutRead { revision: target, generation: history.head.generation, redo };
            match supplied.take().filter(|(supplied, _)| supplied == &read) {
                Some((_, Some(revision))) => {
                    history.navigate(redo, revision);
                    Self::adopt_layout(state, history.layout().clone());
                    return true;
                }
                Some((_, None)) => history.forget(redo),
                None => {
                    self.read = Some(read);
                    return false;
                }
            }
        }
    }
    pub fn read(&self) -> Option<&LayoutRead> {
        self.read.as_ref()
    }
    pub fn supply(&mut self, read: &LayoutRead, revision: Option<LayoutRevision>) -> bool {
        if self.read.as_ref() != Some(read) {
            return false;
        }
        self.read = None;
        let current = self.durable.as_ref().is_some_and(|history| {
            history.head.generation == read.generation && history.target(read.redo) == Some(&read.revision)
        });
        self.supplied = current.then(|| (read.clone(), revision.filter(|revision| revision.validate(&read.revision).is_ok())));
        current
    }
    pub fn cancel_read(&mut self) {
        self.read = None;
        self.supplied = None;
    }
    pub fn acknowledge(&mut self, revisions: &[String]) {
        if let Some(history) = &mut self.durable {
            history.acknowledge(revisions);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditingState {
    pub version: u32,
    pub preset: u32,
    pub tools: crate::ToolMemory,
    pub tool_slots: crate::ToolSlotMemory,
    pub colors: crate::ColorState,
    pub mask_colors: crate::ColorState,
    pub color_library: crate::ColorLibrary,
    pub canvas_tool: crate::LayerCanvasTool,
    pub selection: crate::SelectionOptions,
    #[serde(deserialize_with = "crate::lenient::map")]
    pub region_values: std::collections::BTreeMap<String, f32>,
    pub region_sources: [crate::RegionSource; 3],
    #[serde(default = "default_enabled")]
    pub region_stop_at_colors: bool,
    #[serde(default)]
    pub fill_areas:layer_core::FillAreas,
    #[serde(default = "default_enabled")]
    pub region_close_gaps: bool,
    #[serde(default)]
    pub region_gap_reach:Option<u32>,
    #[serde(default)]
    pub region_edges: layer_core::RegionEdges,
    #[serde(default)]
    pub fill_scope: crate::FillScope,
    #[serde(default = "default_enabled")]
    pub region_smooth_edges: bool,
    pub gradient: crate::GradientToolSettings,
    pub figure: (crate::FigureShape, crate::FigurePaint),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceWorkingState {
    pub version: u32,
    pub zen_mode: bool,
}
impl Default for WorkspaceWorkingState {
    fn default() -> Self { Self { version: 1, zen_mode: false } }
}
impl WorkspaceWorkingState {
    pub fn restore(value: serde_json::Value) -> Self {
        let mut restored = crate::saved_state::restore_fields(value, Self::default, |_| true);
        restored.version = 1;
        restored
    }
    pub fn deserialize_option<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Self>, D::Error> {
        Ok(Option::<serde_json::Value>::deserialize(deserializer)?.map(Self::restore))
    }
}
fn default_enabled() -> bool { true }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCapture {
    pub history: LayoutEdit,
    pub working: WorkspaceWorkingState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Panel, Localizer, UiLanguage};

    #[test]
    fn native_layout_grid_is_not_durable_history() {
        for grid in [1., 0.8, 2.] {
            let mut state = WorkspaceState::default();
            let mut history = WorkspaceHistory::default();
            let captured = history.capture(&state);
            state.layout.layout_grid = grid;
            assert_eq!(durable_layout(&state.layout), *captured.layout());
            assert_eq!(LayoutEdit::starting(&state.layout), captured);
            history.record(WorkspaceState::default(), &state);
            assert_eq!(history.capture(&state), captured);
            assert!(!history.can_undo());
            let before = state.clone();
            state.layout.canvas_info.visible = !state.layout.canvas_info.visible;
            history.record(before, &state);
            assert!(history.can_undo());
            history.undo(&mut state);
            assert_eq!(durable_layout(&state.layout), *captured.layout());
            assert_eq!(state.layout.layout_grid, grid);
            history.redo(&mut state);
            assert_eq!(state.layout.layout_grid, grid);
            assert_ne!(durable_layout(&state.layout), *captured.layout());
            let stored = history.capture(&state);
            assert!(stored.unsaved.values().all(|r| r.layout.layout_grid == 0.));
            let mut invalid = captured.clone();
            invalid.head.current.layout.layout_grid = grid;
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn semantic_history_keeps_custom_names_after_rename_and_deletion() {
        let mut layout = DockLayout::default();
        let panel = layout.add_toolbar(None, "Tools", &[]).unwrap();
        let mut edit = LayoutEdit::starting(&layout);
        layout.rename_toolbar(panel, "日本語 한글 {quoted} 🎨").unwrap();
        edit.append(&layout, LayoutChange::Automatic);
        let renamed = edit.head.current.description.clone();
        assert!(matches!(&renamed, LayoutChange::Renamed { before, after } if before.custom_name.as_deref() == Some("Tools") && after.custom_name.as_deref() == Some("日本語 한글 {quoted} 🎨")));
        layout.delete_toolbar(panel).unwrap();
        edit.append(&layout, LayoutChange::Automatic);
        let deleted = edit.head.current.description.clone();
        assert!(matches!(&deleted, LayoutChange::Panels { action: LayoutPanelAction::Deleted, panels } if panels[0].custom_name.as_deref() == Some("日本語 한글 {quoted} 🎨")));
        let history = edit.into_history();
        let json = serde_json::to_string(&history).unwrap();
        for language in UiLanguage::ALL {
            let localization = Localizer::shared(language);
            assert!(renamed.display(&localization).contains("日本語 한글 {quoted} 🎨"));
            assert!(deleted.display(&localization).contains("日本語 한글 {quoted} 🎨"));
        }
        assert_eq!(serde_json::to_string(&history).unwrap(), json);
        assert_eq!(serde_json::from_str::<LayoutHistory>(&json).unwrap(), history);
        history.validate().unwrap();
    }

    #[test]
    fn edits_append_navigate_and_cap_like_the_stored_history() {
        let mut layout = DockLayout::default();
        let mut edit = LayoutEdit::starting(&layout);
        for extent in 1..=105 {
            layout.bands[0].extent += 1.;
            edit.append(&layout, LayoutChange::Automatic);
            assert_eq!(edit.head.current.id, format!("r{extent}"));
        }
        assert_eq!(edit.head.undo.len(), 100);
        assert_eq!(edit.head.undo[0], "r5");
        assert!(edit.step(false) && edit.step(false));
        assert_eq!((edit.head.current.id.as_str(), edit.head.generation), ("r103", 107));
        assert_eq!(edit.head.redo, ["r105", "r104"]);
        assert!(edit.step(true));
        layout.bands[0].extent += 10.;
        edit.append(&layout, LayoutChange::Automatic);
        assert_eq!(edit.head.current.id, "r109");
        assert!(edit.head.redo.is_empty());
        assert!(edit.unsaved.contains_key("r105"));
        edit.acknowledge(&["r0".into(), "r102".into()]);
        let history = edit.clone().into_history();
        assert!(!history.undo.contains(&"r102".to_string()));
        assert!(!history.revisions.contains_key("r0"));
        history.validate().unwrap();
        let mut stored = LayoutEdit::new(edit.head.clone());
        assert!(!stored.step(false));
        assert_eq!(stored.target(false).map(String::as_str), Some("r104"));
        let mut collision = LayoutEdit::new(LayoutHead { undo: vec!["r110".into()], ..edit.head.clone() });
        layout.bands[0].extent += 1.;
        collision.append(&layout, LayoutChange::Automatic);
        assert_eq!(collision.head.current.id, "r110_");
    }

    #[test]
    fn a_stored_target_is_read_once_and_an_unreadable_one_is_skipped() {
        let mut state = WorkspaceState::default();
        let mut edit = LayoutEdit::starting(&state.layout);
        let mut layouts = vec![state.layout.clone()];
        for _ in 0..3 {
            let mut layout = layouts.last().unwrap().clone();
            layout.bands[0].extent += 10.;
            edit.append(&layout, LayoutChange::Automatic);
            layouts.push(layout);
        }
        let bodies = edit.unsaved.clone();
        state.layout = layouts[3].clone();
        let mut history = WorkspaceHistory::restore(LayoutEdit::new(edit.head.clone()));
        assert!(!history.undo(&mut state));
        let read = history.read().cloned().unwrap();
        assert_eq!((read.revision.as_str(), read.generation, read.redo), ("r2", 3, false));
        assert!(!history.undo(&mut state));
        assert!(history.supply(&read, Some(bodies["r2"].clone())));
        assert!(history.undo(&mut state));
        assert_eq!(durable_layout(&state.layout), layouts[2]);
        assert_eq!(history.generation(), Some(4));
        assert!(!history.undo(&mut state));
        let read = history.read().cloned().unwrap();
        assert!(history.supply(&read, None));
        assert!(!history.undo(&mut state));
        let next = history.read().cloned().unwrap();
        assert_eq!((next.revision.as_str(), next.generation), ("r0", 4));
        assert!(history.supply(&next, Some(bodies["r0"].clone())));
        assert!(history.undo(&mut state));
        assert_eq!(durable_layout(&state.layout), layouts[0]);
        assert!(!history.can_undo());
        assert!(!history.redo(&mut state));
        let read = history.read().cloned().unwrap();
        assert_eq!(read.revision, "r2");
        let mut moved = state.clone();
        moved.layout.bands[0].extent += 1.;
        history.record(state.clone(), &moved);
        assert!(!history.can_redo());
        assert!(!history.undo(&mut state));
        let undo = history.read().cloned().unwrap();
        assert_eq!((undo.revision.as_str(), undo.generation), ("r0", 6));
        assert!(!history.supply(&read, Some(bodies["r2"].clone())));
        assert_eq!(history.read(), Some(&undo));
    }

    #[test]
    fn default_looking_custom_title_remains_explicit_history_data() {
        let before = DockLayout::default();
        assert!(before.panel(Panel::Toolbar).unwrap().custom_name().is_none());
        let mut after = before.clone();
        after.rename_toolbar(Panel::Toolbar, "Tools").unwrap();
        assert!(matches!(layout_change_description(&before, &after), LayoutChange::Renamed { before, after } if before.custom_name.is_none() && after.custom_name.as_deref() == Some("Tools")));
    }

    #[test]
    fn one_gesture_records_once_and_cancel_discards_semantic_arguments() {
        let mut state = WorkspaceState::default();
        let mut history = WorkspaceHistory::default();
        let initial = history.capture(&state);
        let action = LayoutChange::panels(LayoutPanelAction::Resized, vec![description::panel_name(&state.layout, Panel::Toolbar)]);
        history.begin_named(&state, action.clone());
        state.layout.canvas_bar = !state.layout.canvas_bar;
        history.cancel(&mut state);
        assert_eq!(history.capture(&state), initial);
        history.begin_named(&state, action.clone());
        state.layout.canvas_bar = !state.layout.canvas_bar;
        history.finish(&state);
        history.finish(&state);
        let captured = history.capture(&state);
        assert_eq!(captured.head.generation, 1);
        assert_eq!(captured.head.undo.len(), 1);
        assert_eq!(captured.head.current.description, action);
        history.undo(&mut state);
        assert_eq!(state.layout, initial.layout().clone());
    }

    #[test]
    fn loaded_history_rejects_unresolved_or_unbounded_semantics() {
        let mut history = LayoutHistory::new(&DockLayout::default());
        let revision = history.revisions.get_mut(&history.current).unwrap();
        revision.description = LayoutChange::Automatic;
        assert!(history.validate().is_err());
        history.revisions.get_mut(&history.current).unwrap().description = LayoutChange::panels(LayoutPanelAction::Moved, vec![LayoutPanelName { panel: Panel::Toolbar, custom_name: Some("x".repeat(65)) }]);
        assert!(history.validate().is_err());
        history.revisions.get_mut(&history.current).unwrap().description = LayoutChange::panels(LayoutPanelAction::Moved, vec![LayoutPanelName { panel: Panel::Toolbar, custom_name: None }; 4097]);
        assert!(history.validate().is_err());
    }
}
