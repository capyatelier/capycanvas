//! Portable color/source operation state. Executors supply cancellation and
//! device-lifetime observations, bytes and completed previews; the session owns
//! request identity, admissible choices, comparison readiness and publication.
use crate::{DocumentColorOperation, DocumentRequest, HostRequestKind, UiChange, UiSession, Localizer, MessageId};
use layer_color::DocumentColorChange;
use layer_core::{
    ColorTransition, Document, Edit, EvaluationContext, RecordChange, PreparedColorTransition, authored::{OccurrenceHandle, OccurrenceContent},
    color::{ColorProfile, ConversionOptions, DocumentColor, source::SourceImage},
};
use layer_render::CanvasRenderer;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkflowFailure { InactiveOperation, StaleCandidate, NotColorRequest, ColorChoiceMismatch, MissingColorCandidate, CopyCancelled, PreviewCopy, CopyNotReady, SeparateCopy, PreviewColor, ConsumedHistory, NotSourceRequest, MissingSourceLayer, MissingSource, RasterizationProfile, ChooseProfile, SourceCancelled, SourceNotPrepared, PreviewSource, MissingSourceCandidate }
impl WorkflowFailure {
    fn message(self, localization: &Localizer) -> String {
        localization.text(match self {
            Self::InactiveOperation => MessageId::DOCUMENTS_ERROR_INACTIVE_OPERATION,
            Self::StaleCandidate => MessageId::DOCUMENTS_ERROR_STALE_CANDIDATE,
            Self::NotColorRequest => MessageId::DOCUMENTS_ERROR_NOT_COLOR_REQUEST,
            Self::ColorChoiceMismatch => MessageId::DOCUMENTS_ERROR_COLOR_CHOICE_MISMATCH,
            Self::MissingColorCandidate => MessageId::DOCUMENTS_ERROR_MISSING_COLOR_CANDIDATE,
            Self::CopyCancelled => MessageId::DOCUMENTS_ERROR_COPY_CANCELLED,
            Self::PreviewCopy => MessageId::DOCUMENTS_ERROR_PREVIEW_COPY,
            Self::CopyNotReady => MessageId::DOCUMENTS_ERROR_COPY_NOT_READY,
            Self::SeparateCopy => MessageId::DOCUMENTS_ERROR_SEPARATE_COPY,
            Self::PreviewColor => MessageId::DOCUMENTS_ERROR_PREVIEW_COLOR,
            Self::ConsumedHistory => MessageId::DOCUMENTS_ERROR_CONSUMED_HISTORY,
            Self::NotSourceRequest => MessageId::DOCUMENTS_ERROR_NOT_SOURCE_REQUEST,
            Self::MissingSourceLayer => MessageId::DOCUMENTS_ERROR_MISSING_SOURCE_LAYER,
            Self::MissingSource => MessageId::DOCUMENTS_ERROR_MISSING_SOURCE,
            Self::RasterizationProfile => MessageId::DOCUMENTS_ERROR_RASTERIZATION_PROFILE,
            Self::ChooseProfile => MessageId::DOCUMENTS_ERROR_CHOOSE_PROFILE,
            Self::SourceCancelled => MessageId::DOCUMENTS_ERROR_SOURCE_CANCELLED,
            Self::SourceNotPrepared => MessageId::DOCUMENTS_ERROR_SOURCE_NOT_PREPARED,
            Self::PreviewSource => MessageId::DOCUMENTS_ERROR_PREVIEW_SOURCE,
            Self::MissingSourceCandidate => MessageId::DOCUMENTS_ERROR_MISSING_SOURCE_CANDIDATE,
        }).to_string()
    }
}

#[derive(Clone, Debug)]
pub struct CandidateIdentity {
    epoch: u64,
    revision: u64,
    owner: u64,
    request: u32,
}
impl CandidateIdentity {
    pub fn capture<R: CanvasRenderer>(
        session: &UiSession<R>,
        request: u32,
    ) -> Result<Self, String> {
        session.require_document_idle()?;
        if !session.state().requests.iter().any(|r| r.id == request) {
            return Err(WorkflowFailure::InactiveOperation.message(session.localization()));
        }
        Ok(Self {
            epoch: session.state().document_file.epoch,
            revision: session.engine().document().revision,
            owner: session.engine().document().owner,
            request,
        })
    }
    pub fn request(&self) -> u32 {
        self.request
    }
    /// Device equality is an observation: native generations, browser device
    /// owners and worker lifetimes need not share a representation.
    pub fn validate<R: CanvasRenderer>(
        &self,
        session: &UiSession<R>,
        cancelled: bool,
        device_current: bool,
    ) -> Result<(), String> {
        if cancelled
            || !device_current
            || session.state().document_file.epoch != self.epoch
            || session.engine().document().revision != self.revision
            || session.engine().document().owner != self.owner
            || !session
                .state()
                .requests
                .iter()
                .any(|r| r.id == self.request)
        {
            return Err(
                WorkflowFailure::StaleCandidate.message(session.localization()),
            );
        }
        session.require_document_idle()
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ColorPreparation {
    History,
    Edit(DocumentColorChange),
    Flatten {
        color: DocumentColor,
        options: ConversionOptions,
    },
}
/// Immutable source capture plus one selected candidate. No GPU resources or
/// executor are retained here, so workers can prepare on any supported host.
pub struct ColorWorkflow {
    localization: Arc<Localizer>,
    pub identity: CandidateIdentity,
    pub context: EvaluationContext,
    pub original: Document,
    pub candidate: Option<Document>,
    transition: Option<PreparedColorTransition>,
    operation: Option<DocumentColorOperation>,
    plan: Option<ColorPreparation>,
    compared: bool,
}
impl ColorWorkflow {
    pub fn begin<R: CanvasRenderer>(s: &UiSession<R>, id: u32) -> Result<Self, String> {
        let identity = CandidateIdentity::capture(s, id)?;
        let request = s.state().requests.iter().find(|r| r.id == id).unwrap();
        let (operation, transition, candidate) = match &request.kind {
            HostRequestKind::Document {
                request: DocumentRequest::ChangeColor { operation },
            } => (Some(*operation), None, None),
            HostRequestKind::Document {
                request: DocumentRequest::ColorHistory { redo },
            } => {
                let (transition, project) = s.prepare_document_color_transition(if *redo {
                    ColorTransition::Redo
                } else {
                    ColorTransition::Undo
                })?;
                (None, Some(transition), Some(project))
            }
            _ => return Err(WorkflowFailure::NotColorRequest.message(s.localization())),
        };
        Ok(Self {
            localization: s.localization().clone(),
            identity,
            context: s.engine().scene_snapshot().context.clone(),
            original: s.document_snapshot()?,
            operation,
            transition,
            candidate,
            plan: None,
            compared: false,
        })
    }
    pub fn select(
        &mut self,
        choice: Option<DocumentColorChange>,
        copy: bool,
    ) -> Result<ColorPreparation, String> {
        use DocumentColorChange as C;
        use DocumentColorOperation as O;
        if self.operation.is_some() {
            self.candidate = None;
        }
        self.plan = None;
        self.compared = false;
        let plan = match (self.operation, choice, copy) {
            (None, None, false) => ColorPreparation::History,
            (Some(O::Convert), Some(C::Convert { space, options }), true) => {
                ColorPreparation::Flatten {
                    color: DocumentColor {
                        space,
                        ..self.original.composition().color
                    },
                    options,
                }
            }
            (Some(O::Assign), Some(c @ C::Assign(_)), false)
            | (Some(O::Convert), Some(c @ C::Convert { .. }), false)
            | (Some(O::Depth), Some(c @ C::Depth { .. }), false) => ColorPreparation::Edit(c),
            _ => return Err(WorkflowFailure::ColorChoiceMismatch.message(&self.localization)),
        };
        self.plan = Some(plan);
        self.compared = false;
        Ok(plan)
    }
    pub fn is_history(&self) -> bool {
        self.operation.is_none()
    }
    pub fn is_copy(&self) -> bool {
        matches!(self.plan, Some(ColorPreparation::Flatten { .. }))
    }
    pub fn comparison_completed(&mut self) -> Result<(), String> {
        if self.candidate.is_none() || self.plan.is_none() {
            return Err(WorkflowFailure::MissingColorCandidate.message(&self.localization));
        }
        self.compared = true;
        Ok(())
    }
    pub fn copy_project(&self, cancelled: bool) -> Result<&Document, String> {
        if cancelled {
            return Err(WorkflowFailure::CopyCancelled.message(&self.localization));
        }
        if !self.is_copy() || !self.compared {
            return Err(WorkflowFailure::PreviewCopy.message(&self.localization));
        }
        self.candidate
            .as_ref()
            .ok_or_else(|| WorkflowFailure::CopyNotReady.message(&self.localization))
    }
    pub fn prepare_commit<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
        cancelled: bool,
        device_current: bool,
    ) -> Result<PreparedColorTransition, String> {
        self.identity.validate(s, cancelled, device_current)?;
        if self.is_copy() {
            return Err(WorkflowFailure::SeparateCopy.message(&self.localization));
        }
        if self.plan.is_none() || (!self.is_history() && !self.compared) {
            return Err(WorkflowFailure::PreviewColor.message(&self.localization));
        }
        if self.is_history() {
            return self.transition.take().ok_or_else(|| WorkflowFailure::ConsumedHistory.message(&self.localization));
        }
        let p = self
            .candidate
            .as_ref()
            .ok_or_else(|| WorkflowFailure::MissingColorCandidate.message(&self.localization))?;
        let document = s.engine().document();
        let paint = p.artwork.paint.iter().map(|(handle, _, source)| RecordChange::replace(&document.artwork.paint, handle, Some(source.clone())).map_err(str::to_string)).collect::<Result<Vec<_>, _>>()?;
        let coverage = p.artwork.coverage.iter().map(|(handle, _, source)| RecordChange::replace(&document.artwork.coverage, handle, Some(source.clone())).map_err(str::to_string)).collect::<Result<Vec<_>, _>>()?;
        let edit = document.color_edit(p.composition().color, paint, coverage).map_err(crate::session::error)?;
        Ok(s.prepare_document_color_transition(ColorTransition::Apply { edit: Box::new(edit) })?.0)
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    /// Swap with a prepared renderer, commit exact history, and swap back on
    /// failure. The adapter keeps the displaced resource for off-owner disposal.
    /// Remote renderers may use a no-op swap and adopt through CanvasRenderer.
    pub fn commit_document_color_candidate(
        &mut self,
        prepared: PreparedColorTransition,
        mut swap: impl FnMut(&mut R),
    ) -> Result<UiChange, String> {
        swap(self.renderer_mut());
        match self.commit_document_color_transition(prepared) {
            Ok(change) => Ok(change),
            Err(error) => {
                swap(self.renderer_mut());
                Err(error)
            }
        }
    }
}

#[derive(Clone)]
pub struct SourceWorkflow {
    localization: Arc<Localizer>,
    pub identity: CandidateIdentity,
    pub context: EvaluationContext,
    pub project: Document,
    pub original: Arc<SourceImage>,
    layer: OccurrenceHandle,
    rasterize: bool,
    adds_layer: bool,
    converted: Option<Arc<SourceImage>>,
    prepared: Option<Edit>,
    compared: bool,
}
impl SourceWorkflow {
    pub fn begin<R: CanvasRenderer>(s: &UiSession<R>, id: u32) -> Result<Self, String> {
        let identity = CandidateIdentity::capture(s, id)?;
        let (layer, rasterize) = match &s.state().requests.iter().find(|r| r.id == id).unwrap().kind
        {
            HostRequestKind::Document {
                request: DocumentRequest::RepairSourceProfile { layer },
            } => (crate::session::occurrence_handle(*layer)?, false),
            HostRequestKind::Document {
                request: DocumentRequest::RasterizeSource { layer },
            } => (crate::session::occurrence_handle(*layer)?, true),
            _ => return Err(WorkflowFailure::NotSourceRequest.message(s.localization())),
        };
        let project = s.document_snapshot()?;
        let occurrence = project.scene().occurrence(layer).ok_or_else(|| WorkflowFailure::MissingSourceLayer.message(s.localization()))?;
        let OccurrenceContent::Paint(paint) = occurrence.content else { return Err(WorkflowFailure::MissingSource.message(s.localization())); };
        let source = project.artwork.paint.get(paint).ok_or_else(|| WorkflowFailure::MissingSource.message(s.localization()))?;
        let original = source.base.as_ref().map(|base| base.image.storage().clone()).ok_or_else(|| WorkflowFailure::MissingSource.message(s.localization()))?;
        let adds_layer = !rasterize && crate::session::source_edit::baked(source);
        Ok(Self {
            localization: s.localization().clone(),
            identity,
            context: s.engine().scene_snapshot().context.clone(),
            project,
            original,
            layer,
            rasterize,
            adds_layer,
            converted: None,
            prepared: None,
            compared: false,
        })
    }
    pub fn rasterize(&self) -> bool {
        self.rasterize
    }
    pub fn adds_layer(&self) -> bool {
        self.adds_layer && self.converted.as_ref().is_none_or(|source| **source != *self.original)
    }
    pub fn validate_choice(&self, profile: &Option<ColorProfile>) -> Result<(), String> {
        match (self.rasterize, profile.is_some()) {
            (true, true) => Err(WorkflowFailure::RasterizationProfile.message(&self.localization)),
            (false, false) => Err(WorkflowFailure::ChooseProfile.message(&self.localization)),
            _ => Ok(()),
        }
    }
    /// Executors supply their allocation budget; conversion and source policy
    /// remain shared, including failure before publishing over-budget results.
    pub fn prepare(
        &self,
        profile: Option<ColorProfile>,
        budget: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<(Arc<SourceImage>, u64), String> {
        self.validate_choice(&profile)?;
        if cancelled() {
            return Err(WorkflowFailure::SourceCancelled.message(&self.localization));
        }
        let (source, clipped) = if self.rasterize {
            let (source, statistics) = layer_color::rasterize_source(
                &self.original,
                self.project.composition().color,
                budget,
                &mut cancelled,
            )?;
            (source, statistics.clipped_channels)
        } else {
            let mut source = (*self.original).clone();
            source.interpretation = layer_color::repair_source_interpretation(
                source.interpretation,
                self.project.composition().color.space,
                profile.unwrap(),
            )?;
            source.validate()?;
            (source, 0)
        };
        if cancelled() {
            return Err(WorkflowFailure::SourceCancelled.message(&self.localization));
        }
        Ok((Arc::new(source), clipped))
    }
    pub fn preview<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
        source: Arc<SourceImage>,
        cancelled: bool,
        device_current: bool,
    ) -> Result<Document, String> {
        self.identity.validate(s, cancelled, device_current)?;
        self.compared = false;
        self.converted = None;
        self.prepared = None;
        let (project, edit) = s.prepare_source_edit(self.layer, &self.original, source.clone(), self.rasterize)?;
        self.prepared = Some(edit);
        self.converted = Some(source);
        Ok(project)
    }
    pub fn comparison_completed(&mut self) -> Result<(), String> {
        if self.converted.is_none() {
            return Err(WorkflowFailure::SourceNotPrepared.message(&self.localization));
        }
        self.compared = true;
        Ok(())
    }
    pub fn commit<R: CanvasRenderer>(
        &mut self,
        s: &mut UiSession<R>,
        cancelled: bool,
        device_current: bool,
    ) -> Result<(), String> {
        self.identity.validate(s, cancelled, device_current)?;
        if !self.compared {
            return Err(WorkflowFailure::PreviewSource.message(&self.localization));
        }
        if self.converted.is_none() { return Err(WorkflowFailure::MissingSourceCandidate.message(&self.localization)); }
        let edit = self.prepared.as_ref().ok_or_else(|| WorkflowFailure::MissingSourceCandidate.message(&self.localization))?.clone();
        s.commit_prepared_source_edit(edit)?;
        self.prepared = None;
        self.converted = None;
        self.compared = false;
        Ok(())
    }
}
