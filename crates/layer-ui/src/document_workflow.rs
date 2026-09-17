//! Portable color/source operation state. Executors supply cancellation and
//! device-lifetime observations, bytes and completed previews; the session owns
//! request identity, admissible choices, comparison readiness and publication.
use crate::{DocumentColorOperation, DocumentRequest, HostRequestKind, UiChange, UiSession};
use layer_color::DocumentColorChange;
use layer_core::{
    ColorTransition, LayerId, PreparedColorTransition, Project,
    color::{ColorProfile, ConversionOptions, DocumentColor, source::SourceImage},
};
use layer_render::CanvasRenderer;
use std::sync::Arc;

pub const PREPARED_DOCUMENT_LIMIT: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct CandidateIdentity {
    epoch: u64,
    revision: u64,
    request: u32,
}
impl CandidateIdentity {
    pub fn capture<R: CanvasRenderer>(
        session: &UiSession<R>,
        request: u32,
    ) -> Result<Self, String> {
        session.require_document_idle()?;
        if !session.state().requests.iter().any(|r| r.id == request) {
            return Err("The operation is no longer active".into());
        }
        Ok(Self {
            epoch: session.state().document_file.epoch,
            revision: session.engine().document().revision,
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
            || !session
                .state()
                .requests
                .iter()
                .any(|r| r.id == self.request)
        {
            return Err(
                "The document, operation or canvas changed; prepare the change again".into(),
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
    pub identity: CandidateIdentity,
    pub original: Project,
    pub candidate: Option<Project>,
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
            _ => return Err("Not a document color request".into()),
        };
        Ok(Self {
            identity,
            original: s.capture_project_recovery()?,
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
                        ..self.original.document.color
                    },
                    options,
                }
            }
            (Some(O::Assign), Some(c @ C::Assign(_)), false)
            | (Some(O::Convert), Some(c @ C::Convert { .. }), false)
            | (Some(O::Depth), Some(c @ C::Depth { .. }), false) => ColorPreparation::Edit(c),
            _ => return Err("Color choice does not match the request".into()),
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
            return Err("Color candidate is missing".into());
        }
        self.compared = true;
        Ok(())
    }
    pub fn copy_project(&self, cancelled: bool) -> Result<&Project, String> {
        if cancelled {
            return Err("Converted copy cancelled".into());
        }
        if !self.is_copy() || !self.compared {
            return Err("Preview a flattened copy before saving".into());
        }
        self.candidate
            .as_ref()
            .ok_or_else(|| "Converted copy is not ready".into())
    }
    pub fn prepare_commit<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
        cancelled: bool,
        device_current: bool,
    ) -> Result<PreparedColorTransition, String> {
        self.identity.validate(s, cancelled, device_current)?;
        if self.is_copy() {
            return Err("Save the converted copy as a separate document".into());
        }
        if self.plan.is_none() || (!self.is_history() && !self.compared) {
            return Err("Preview the complete color result first".into());
        }
        if self.is_history() {
            return self.transition.take().ok_or_else(|| "The history candidate was already consumed; prepare it again".into());
        }
        let p = self
            .candidate
            .as_ref()
            .ok_or("Color candidate is missing")?;
        Ok(s.prepare_document_color_transition(ColorTransition::Apply {
            color: p.document.color,
            layers: p.document.layers.clone(),
        })?
        .0)
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
    pub identity: CandidateIdentity,
    pub project: Project,
    pub original: Arc<SourceImage>,
    layer: LayerId,
    rasterize: bool,
    adds_layer: bool,
    converted: Option<Arc<SourceImage>>,
    compared: bool,
}
impl SourceWorkflow {
    pub fn begin<R: CanvasRenderer>(s: &UiSession<R>, id: u32) -> Result<Self, String> {
        let identity = CandidateIdentity::capture(s, id)?;
        let (layer, rasterize) = match &s.state().requests.iter().find(|r| r.id == id).unwrap().kind
        {
            HostRequestKind::Document {
                request: DocumentRequest::RepairSourceProfile { layer },
            } => (LayerId(*layer), false),
            HostRequestKind::Document {
                request: DocumentRequest::RasterizeSource { layer },
            } => (LayerId(*layer), true),
            _ => return Err("Not a retained source request".into()),
        };
        let project = s.capture_project_recovery()?;
        let l = project
            .document
            .layer(layer)
            .ok_or("Source layer no longer exists")?;
        let original = l.source.clone().ok_or("No retained source")?;
        let adds_layer = !rasterize && crate::session::source_edit::baked(l);
        Ok(Self {
            identity,
            project,
            original,
            layer,
            rasterize,
            adds_layer,
            converted: None,
            compared: false,
        })
    }
    pub fn rasterize(&self) -> bool {
        self.rasterize
    }
    pub fn adds_layer(&self) -> bool {
        self.adds_layer
    }
    pub fn validate_choice(&self, profile: &Option<ColorProfile>) -> Result<(), String> {
        match (self.rasterize, profile.is_some()) {
            (true, true) => Err("Rasterization uses the document profile".into()),
            (false, false) => Err("Choose a source profile".into()),
            _ => Ok(()),
        }
    }
    pub fn prepare(
        &self,
        profile: Option<ColorProfile>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<(Arc<SourceImage>, u64), String> {
        self.validate_choice(&profile)?;
        if cancelled() {
            return Err("Source change cancelled".into());
        }
        let (source, clipped) = if self.rasterize {
            let (source, statistics) = layer_color::rasterize_source(
                &self.original,
                self.project.document.color,
                PREPARED_DOCUMENT_LIMIT,
                &mut cancelled,
            )?;
            (source, statistics.clipped_channels)
        } else {
            let mut source = (*self.original).clone();
            source.interpretation = layer_color::repair_source_interpretation(
                source.interpretation,
                self.project.document.color.space,
                profile.unwrap(),
            )?;
            source.validate()?;
            (source, 0)
        };
        if cancelled() {
            return Err("Source change cancelled".into());
        }
        Ok((Arc::new(source), clipped))
    }
    pub fn preview<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
        source: Arc<SourceImage>,
        cancelled: bool,
        device_current: bool,
    ) -> Result<Project, String> {
        self.identity.validate(s, cancelled, device_current)?;
        self.compared = false;
        self.converted = None;
        let project = if self.rasterize {
            s.preview_rasterized_source(self.layer, &self.original, source.clone())?
        } else {
            s.preview_layer_source(self.layer, &self.original, (*source).clone())?
        };
        self.converted = Some(source);
        Ok(project)
    }
    pub fn comparison_completed(&mut self) -> Result<(), String> {
        if self.converted.is_none() {
            return Err("Source comparison is not prepared".into());
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
            return Err("Preview the complete source result first".into());
        }
        let source = self
            .converted
            .as_ref()
            .ok_or("Source candidate is missing")?
            .clone();
        if self.rasterize {
            s.apply_rasterized_source(self.layer, &self.original, source)?;
        } else {
            s.repair_layer_source(self.layer, &self.original, (*source).clone())?;
        }
        self.converted = None;
        self.compared = false;
        Ok(())
    }
}
