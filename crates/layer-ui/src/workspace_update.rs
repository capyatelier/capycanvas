//! Shared, toolkit-independent workspace publication contract.
//!
//! A consumer retains the models at `model_revision`. When that revision changes
//! it fetches the normal layout/content models before applying this presentation.
//! Otherwise it applies only absolute geometry at placement/draw time. Intermediate
//! presentations may be replaced; input actions and gesture completion may not.
//! This avoids a chain of deltas and keeps cancellation/undo in UiSession.
//!
//! `revision` orders presentations; `model_revision` identifies all retained UI
//! models, including content and settings, not just docking topology. Apply a
//! presentation only against its matching model revision. Host visibility and
//! viewport changes also require a model refresh (NativeHost handles these).
//! Native transports include this object as `workspace_update` in both full
//! snapshots and geometry-only packets; camera data can accompany either.
//!
//! Layout-aware consumers may additionally retain controls at `content_revision`.
//! NativeHost::take_layout_update_bytes opts into packets containing `layout`,
//! `workspace_layout`, `camera`, and `panel_measurements` when only dimensions or
//! reflow measurements changed. Replace those absolute fields against matching
//! content_revision before advancing model_revision. `workspace_layout` replaces
//! bands, floating groups, collapsed columns and fit_tab_groups in the retained
//! workspace; it is live gesture state, not a persistence request. Collapse/expand,
//! content edits, completion and cancellation still publish full models. Existing
//! consumers of take_update_bytes keep their original model_revision behavior.
//!
//! Capture tab slots/clip with BeginTabDrag and publish displayed drawer bounds
//! with MeasureColumnDrawers. Tab presentation moves drawings, not the frozen
//! insertion slots. Group placement moves native hit/clip/overview allocations
//! together. Frontends retain native-resolution rendering; this contract carries
//! neither screenshots nor resampled control textures.
use crate::{Bounds, DropHint, Panel, TabDragPreview};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceUpdate {
    pub revision: u64,
    pub model_revision: u64,
    /// Opt-in layout consumers retain controls/resources at this revision, but
    /// must apply the accompanying resolved layout when model_revision changes.
    /// Older consumers continue refreshing all models at model_revision.
    pub content_revision: u64,
    /// None ends the presentation, including cancellation and undo/redo.
    pub drag: Option<WorkspaceDragPresentation>,
}

/// Absolute reflow publication shared by native transports and Wasm. Apply only
/// against retained models with the matching `content_revision`. This borrows
/// the live workspace, excluding panel definitions and all editor content.
#[derive(Serialize)]
pub struct WorkspaceLayoutUpdate<'a> {
    pub workspace_update: WorkspaceUpdate,
    pub layout: crate::ResolvedLayout,
    pub workspace_layout: WorkspaceLayoutState<'a>,
    // Work area changes without a camera navigation revision.
    pub camera: &'a crate::Camera,
    pub panel_measurements: &'a [crate::PanelMeasurement],
}

#[derive(Serialize)]
pub struct WorkspaceLayoutState<'a> {
    pub bands: &'a [crate::DockBand],
    pub floating: &'a [crate::FloatingGroup],
    pub collapsed: &'a [crate::CollapsedColumn],
    pub fit_tab_groups: &'a [u32],
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceDragPresentation {
    pub group: Option<WorkspaceGroupPosition>,
    pub tab: Option<WorkspaceTabPresentation>,
    pub drop_hint: Option<DropHint>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceGroupPosition {
    pub id: u32,
    pub bounds: Bounds,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceTabPresentation {
    pub group: u32,
    pub panel: Panel,
    pub clip: Bounds,
    pub source: Bounds,
    pub preview: TabDragPreview,
}
