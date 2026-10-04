//! Select › Modify: Grow, Shrink, Feather, Border and Smooth, previewed live.
//! While the value changes, the canvas shows a preview the renderer may
//! compute on coarse cells, and the document stays as it is. Once the value
//! rests, or on Apply, its exact result becomes one history step and later
//! results amend it; Cancel withdraws it and Apply keeps it.
use super::*;
use layer_core::{Selection, SelectionTarget};
use layer_render::{ModifyStep, RegionRequest, SelectionModify, SelectionRefinement};
use std::sync::Arc;

/// How long a value rests before its exact result replaces the preview.
pub(super) const SETTLE_NS: u64 = 200_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefineKind {
    Grow,
    Shrink,
    Feather,
    Border,
    Smooth,
}
impl RefineKind {
    pub const ALL: [Self; 5] = [Self::Grow, Self::Shrink, Self::Feather, Self::Border, Self::Smooth];
    /// Smooth closes the selection, then opens it, with a disk of its radius:
    /// grow r, shrink 2r (the two shrinks in one pass), grow r. The merged
    /// shrink must stay within the renderer's resize limit.
    pub const MAX_SMOOTH: u32 = SelectionRefinement::MAX_RESIZE / 2;
    pub fn command(self) -> CommandId {
        match self {
            Self::Grow => CommandId::GrowSelection,
            Self::Shrink => CommandId::ShrinkSelection,
            Self::Feather => CommandId::FeatherSelection,
            Self::Border => CommandId::BorderSelection,
            Self::Smooth => CommandId::SmoothSelection,
        }
    }
    pub(super) fn of_command(command: CommandId) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.command() == command)
    }
    fn title(self) -> &'static str {
        match self {
            Self::Grow => "Grow Selection",
            Self::Shrink => "Shrink Selection",
            Self::Feather => "Feather Selection",
            Self::Border => "Border Selection",
            Self::Smooth => "Smooth Selection",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Grow => "Grow by",
            Self::Shrink => "Shrink by",
            Self::Feather => "Feather radius",
            Self::Border => "Border width",
            Self::Smooth => "Smooth radius",
        }
    }
    fn numeric(self) -> NumericControl {
        match self {
            Self::Feather => NumericControl::number(0.1, SelectionRefinement::MAX_FEATHER.into(), 1., 1),
            Self::Smooth => NumericControl::number(1., Self::MAX_SMOOTH.into(), 1., 0),
            Self::Grow | Self::Shrink | Self::Border => {
                NumericControl::number(1., SelectionRefinement::MAX_RESIZE.into(), 1., 0)
            }
        }
        .unit("px")
    }
    fn steps(self, radius: f32) -> Vec<ModifyStep> {
        let r = radius as i32;
        let resize = |resize, chained| ModifyStep { chained, resize, ..ModifyStep::default() };
        match self {
            Self::Grow => vec![resize(r, false)],
            Self::Shrink => vec![resize(-r, false)],
            Self::Feather => vec![ModifyStep { feather: radius, ..ModifyStep::default() }],
            Self::Border => vec![resize(r, false), ModifyStep { subtract: true, ..resize(-r, false) }],
            Self::Smooth => vec![
                resize(r, false),
                ModifyStep { keep_canvas_edges: true, ..resize(-2 * r, true) },
                resize(r, true),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionRefineView {
    pub kind: RefineKind,
    pub title: &'static str,
    /// The value's name, such as "Feather radius".
    pub label: &'static str,
    pub radius: f32,
    pub numeric: NumericControl,
}

/// A result the canvas shows but the document does not hold.
struct Shown {
    radius: f32,
    selection: Selection,
    exact: bool,
}
pub(super) struct RefineDraft {
    view: SelectionRefineView,
    target: SelectionTarget,
    original: Arc<Selection>,
    active: Option<layer_core::authored::OccurrenceHandle>,
    revision: u64,
    owner: u64,
    committed: bool,
    applying: bool,
    /// The radius of the job in progress, and whether it is exact.
    job: Option<(f32, bool)>,
    shown: Option<Shown>,
    /// The radius whose exact result the document holds.
    settled: Option<f32>,
    /// When the value last changed; None until the next frame.
    changed_ns: Option<u64>,
}
impl RefineDraft {
    pub(super) fn view(&self) -> Option<SelectionRefineView> {
        (!self.applying).then(|| self.view.clone())
    }
    /// The preview drawn for the mask the canvas displays.
    pub(super) fn preview(&self, displayed: Option<SelectionTarget>) -> Option<&Selection> {
        let shown = self.shown.as_ref()?;
        (displayed.unwrap_or(SelectionTarget::Current) == self.target).then_some(&shown.selection)
    }
    /// Only a preview runs, which another edit may simply end.
    fn previewing(&self) -> bool {
        self.job.is_some_and(|(_, exact)| !exact) && !self.applying
    }
    /// Frames continue until the document holds the current value.
    pub(super) fn unsettled(&self) -> bool {
        self.settled != Some(self.view.radius) || self.applying || self.job.is_some()
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    /// A running Refine preview leaves commands available, so their states
    /// hold still while a value is dragged.
    pub(super) fn refine_previewing(&self) -> bool {
        self.selection_masks.refine.as_ref().is_some_and(RefineDraft::previewing)
    }
    /// The mask a Refine command changes: Quick Mask, the Selection Layer
    /// being edited, or else the current selection.
    pub(super) fn refine_layer(&self) -> Option<u64> {
        match self.selection_masks.target()? {
            SelectionTarget::Current => Some(0),
            SelectionTarget::Saved(id) => Some(super::session::occurrence_token(id)),
        }
    }
    pub(super) fn refine_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        let doc = self.engine.document();
        match self.selection_masks.target() {
            Some(SelectionTarget::Saved(id)) if doc.is_locked(id) => Some(l.text(MessageId::COMMANDS_THIS_SELECTION_LAYER_IS_LOCKED)),
            Some(_) => None,
            None => doc.working.selection.is_none().then_some(l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST)),
        }
    }
    pub(super) fn begin_refine(&mut self, kind: RefineKind, layer: Option<u64>) -> Result<(), String> {
        self.cancel_refine()?;
        if layer.is_none() && matches!(self.selection_masks.target(), Some(SelectionTarget::Saved(_))) {
            self.return_to_artwork()?;
        }
        let target = match layer {
            Some(0) if self.selection_masks.quick() => SelectionTarget::Current,
            Some(id) => SelectionTarget::Saved(super::session::occurrence_handle(id)?),
            None => SelectionTarget::Current,
        };
        if target == SelectionTarget::Current && self.current_selection().is_none() {
            return Err("Make a selection first".into());
        }
        if let SelectionTarget::Saved(id) = target
            && self.engine.document().is_locked(id)
        {
            return Err("This selection layer is locked".into());
        }
        let coverage = self.mask_coverage(target)?;
        let doc = self.engine.document();
        doc.selection_edit(target, coverage.clone()).map_err(error)?;
        self.selection_masks.refine = Some(RefineDraft {
            view: SelectionRefineView {
                kind,
                title: kind.title(),
                label: kind.label(),
                radius: 5.,
                numeric: kind.numeric(),
            },
            target,
            original: Arc::new(coverage),
            active: doc.working.occurrence,
            revision: doc.revision,
            owner:doc.owner,
            committed: false,
            applying: false,
            job: None,
            shown: None,
            settled: None,
            changed_ns: None,
        });
        self.queue_refine(false);
        Ok(())
    }
    pub(super) fn set_refine_radius(&mut self, radius: f32) -> Result<(), String> {
        let draft = self.selection_masks.refine.as_mut().filter(|d| !d.applying).ok_or("No selection adjustment is open")?;
        draft.view.numeric.validate(radius, draft.view.label).map_err(|reason| reason.message(&self.state.localization))?;
        if draft.view.kind != RefineKind::Feather && radius.fract() != 0. {
            return Err(NumericError::WholePixels { label: draft.view.label.into() }.message(&self.state.localization));
        }
        if draft.view.radius != radius {
            draft.view.radius = radius;
            draft.changed_ns = None;
            self.selection_masks.refine_values += 1;
            let job = draft.job;
            self.state.layer_tools.selection_resize = self.selection_masks.refine_view();
            match job {
                Some((_, true)) => {
                    self.abandon_region();
                    self.queue_refine(false);
                }
                Some(_) => {}
                None => self.queue_refine(false),
            }
        }
        Ok(())
    }
    /// Start a job for the current value. Values that change while a preview
    /// runs wait for it to show, so a dragged value keeps updating the canvas
    /// and only the latest value runs next.
    fn queue_refine(&mut self, exact: bool) {
        let Some(draft) = self.selection_masks.refine.as_mut() else {
            return;
        };
        let modify = SelectionModify {
            selection: draft.original.clone(),
            steps: draft.view.kind.steps(draft.view.radius),
            preview: (!exact).then(|| 1. / self.state.camera.zoom),
        };
        draft.job = Some((draft.view.radius, exact));
        self.queue_refine_region(RegionRequest {
            request_id: 0,
            contiguous: false,
            source: layer_render::RegionSource::Modify(Arc::new(modify)),
            position: [0, 0],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
            selection: None,
        });
    }
    pub(super) fn refine_result(&mut self, result: layer_render::RegionResult) -> Result<(), String> {
        let Some(draft) = self.selection_masks.refine.as_mut() else {
            return Ok(());
        };
        let Some((radius, requested)) = draft.job.take() else {
            return Ok(());
        };
        let exact = result.placement == layer_core::Affine::IDENTITY;
        let selection = Selection {
            affine: result.placement,
            ..Selection::pixels(result.pixels)
        };
        let current = radius == draft.view.radius;
        if current && exact && (requested || draft.applying) {
            return self.commit_refine(selection);
        }
        draft.shown = Some(Shown { radius, selection, exact });
        self.selection_masks.refine_previews += 1;
        if !current {
            self.queue_refine(false);
        }
        self.show_refine_preview()
    }
    fn commit_refine(&mut self, selection: Selection) -> Result<(), String> {
        let draft = self.selection_masks.refine.as_mut().unwrap();
        let (target, revision, radius) = (draft.target, draft.revision, draft.view.radius);
        if draft.committed {
            self.engine.refine_selection(target, selection, revision).map_err(error)?;
        } else {
            self.set_mask_coverage(target, selection)?;
        }
        let draft = self.selection_masks.refine.as_mut().unwrap();
        draft.committed = true;
        draft.revision = self.engine.document().revision;
        draft.settled = Some(radius);
        draft.shown = None;
        if draft.applying {
            self.selection_masks.refine = None;
        }
        self.show_refined_selection()
    }
    pub(super) fn apply_refine(&mut self) -> Result<(), String> {
        let draft = self.selection_masks.refine.as_mut().filter(|d| !d.applying).ok_or("No selection adjustment is open")?;
        draft.applying = true;
        if draft.settled == Some(draft.view.radius) && draft.job.is_none() {
            self.close_settled_refine()?;
        }
        Ok(())
    }
    fn close_settled_refine(&mut self) -> Result<(), String> {
        let shown = self.selection_masks.refine.take().is_some_and(|d| d.shown.is_some());
        self.selection_masks.refine_changed = true;
        if shown {
            self.show_refine_preview()?;
        }
        Ok(())
    }
    /// Close the dialog and restore the selection it started from.
    pub(super) fn cancel_refine(&mut self) -> Result<(), String> {
        let Some(draft) = self.selection_masks.refine.take() else {
            return Ok(());
        };
        if draft.job.is_some() {
            self.abandon_region();
        }
        if draft.committed {
            self.engine.withdraw_selection(draft.target, draft.revision).map_err(error)?;
            self.show_refined_selection()?;
        } else if draft.shown.is_some() {
            self.show_refine_preview()?;
            self.selection_masks.refine_changed = true;
        }
        Ok(())
    }
    /// Draw a result on the canvas without publishing anything else.
    fn show_refine_preview(&mut self) -> Result<(), String> {
        self.sync_selection_overlay();
        let display = self.engine.display_selection().map(|s| s.into_owned());
        self.engine.backend_mut().set_selection_outline(display.as_ref()).map_err(error)
    }
    fn show_refined_selection(&mut self) -> Result<(), String> {
        self.show_refine_preview()?;
        self.selection_masks.refine_changed = true;
        self.refresh_document();
        Ok(())
    }
    /// A draft outlives neither another edit of the document nor its target.
    /// Jobs that another tool cancelled start again. A value that has rested,
    /// or one to apply, replaces its preview with the exact result.
    pub(super) fn advance_refine(&mut self, now_ns: u64) -> Result<(), String> {
        let doc = self.engine.document();
        let Some(draft) = self.selection_masks.refine.as_mut() else {
            return Ok(());
        };
        if draft.owner != doc.owner || draft.revision != doc.revision || draft.active != doc.working.occurrence {
            let (job, shown) = (draft.job.is_some(), draft.shown.is_some());
            self.selection_masks.refine = None;
            self.selection_masks.refine_changed = true;
            if job {
                self.abandon_region();
            }
            if shown {
                self.show_refine_preview()?;
            }
            self.notify("The selection changed, so its refinement was closed");
            return Ok(());
        }
        let changed = *draft.changed_ns.get_or_insert(now_ns);
        if let Some((_, exact)) = draft.job {
            if !self.region_tools.refining() {
                self.queue_refine(exact);
            }
            return Ok(());
        }
        if draft.settled == Some(draft.view.radius) {
            if draft.applying {
                return self.close_settled_refine();
            }
            if draft.shown.take().is_some() {
                self.show_refine_preview()?;
            }
            return Ok(());
        }
        if !draft.applying && now_ns.saturating_sub(changed) < SETTLE_NS {
            return Ok(());
        }
        match draft.shown.take_if(|s| s.exact && s.radius == draft.view.radius) {
            Some(shown) => self.commit_refine(shown.selection),
            None => {
                self.queue_refine(true);
                Ok(())
            }
        }
    }
    /// Stop the region job in progress; no result follows.
    fn abandon_region(&mut self) {
        if self.region_tools.abandon() {
            self.engine.backend_mut().cancel_region();
        }
    }
}
