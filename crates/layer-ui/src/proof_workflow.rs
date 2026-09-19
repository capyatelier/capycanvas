//! Print-view policy shared by hosts. Scheduling, cancellation and durable
//! profile writes belong to the host; no viewing state enters document history.
use crate::{HostRequestKind, UiAction, UiChange, UiSession};
use layer_core::color::{ColorProfile, ProofRecipe, RgbSpace};
use layer_render::CanvasRenderer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Image-analysis identity deliberately excludes camera, proof and rendition
/// settings. Hosts additionally check their GPU generation before publishing.
#[derive(Clone, PartialEq)]
pub struct ToneKey {
    epoch: u64,
    color: layer_core::color::DocumentColor,
    extent: [u32; 2],
    background: [f32; 4],
    layers: Vec<layer_core::Layer>,
}
impl ToneKey {
    pub fn current<R: CanvasRenderer>(s: &UiSession<R>) -> Option<Self> {
        let d = s.engine().document();
        (d.color.depth.is_float() && !s.rendering_suspended()).then(|| Self {
            epoch: s.state().document_file.epoch, color: d.color,
            extent: [d.width, d.height], background: s.engine().view().background_rgba_linear,
            layers: d.layers.iter().map(layer_core::Layer::composite_snapshot).collect(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofKey {
    epoch: u64,
    space: RgbSpace,
    recipe: Option<ProofRecipe>,
}
impl ProofKey {
    fn current<R: CanvasRenderer>(s: &UiSession<R>) -> Self {
        Self {
            epoch: s.state().document_file.epoch,
            space: s.engine().document().color.space,
            recipe: s.engine().document().proof.clone(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProofPreparation {
    original: ProofKey,
    request: Option<u32>,
    #[serde(default)]
    edit: bool,
    pub recipe: ProofRecipe,
}
impl ProofPreparation {
    pub fn begin<R: CanvasRenderer>(
        s: &UiSession<R>,
        request: Option<u32>,
        recipe: Option<ProofRecipe>,
    ) -> Result<Self, String> {
        let original = ProofKey::current(s);
        let recipe = recipe
            .or_else(|| original.recipe.clone())
            .ok_or("Choose a proof profile")?;
        recipe.validate()?;
        let job = Self {
            original,
            request,
            edit: request.is_some(),
            recipe,
        };
        job.validate(s)?;
        Ok(job)
    }
    pub fn panel<R: CanvasRenderer>(s: &UiSession<R>, recipe: ProofRecipe) -> Result<Self,String> {
        let mut job=Self::begin(s,None,Some(recipe))?;
        s.require_document_idle()?;
        job.edit=true;
        Ok(job)
    }
    pub fn space(&self) -> RgbSpace {
        self.original.space
    }
    pub fn build(
        &self,
        cancelled: impl Fn() -> bool,
    ) -> Result<Arc<layer_color::ProofLut>, String> {
        if cancelled() {
            return Err("Proof preparation cancelled".into());
        }
        layer_color::ProofLut::build(self.space(), &self.recipe, cancelled).map(Arc::new)
    }
    pub fn validate<R: CanvasRenderer>(&self, s: &UiSession<R>) -> Result<(), String> {
        if ProofKey::current(s) != self.original || s.rendering_suspended() {
            return Err("The drawing changed; reopen Proof Setup".into());
        }
        if self.edit {
            s.require_document_idle()?;
            if s.state().document_file.busy { return Err("A document operation is in progress".into()); }
        }
        if let Some(id) = self.request {
            if s.state().document_file.busy
                || !s
                    .state()
                    .requests
                    .iter()
                    .any(|r| r.id == id && matches!(r.kind, HostRequestKind::SoftProofSetup))
            {
                return Err("Proof Setup is no longer active".into());
            }
        }
        Ok(())
    }
    /// Only the replaced embedded ICC needs a durable local copy. Builtins are
    /// always selectable. The document still embeds only its active recipe.
    pub fn preservation(&self) -> Option<&[u8]> {
        if !self.edit {
            return None;
        }
        let old = self.original.recipe.as_ref()?;
        if old.profile == self.recipe.profile {
            return None;
        }
        match &old.profile {
            ColorProfile::Icc(bytes) => Some(bytes),
            _ => None,
        }
    }
    pub fn apply<R: CanvasRenderer>(
        &self,
        s: &mut UiSession<R>,
        preserved: bool,
    ) -> Result<UiChange, String> {
        self.validate(s)?;
        if self.preservation().is_some() && !preserved {
            return Err("Save the original in Saved Profiles before replacing it".into());
        }
        if self.edit {
            let mut change = s.set_proof_recipe(Some(self.recipe.clone()))?;
            if let Some(id) = self.request {
                let done = s.dispatch(UiAction::CompleteRequest { id, error: None })?;
                change.regions |= done.regions;
                change.revision = done.revision;
            }
            Ok(change)
        } else {
            Ok(UiChange {
                canvas_wake: true,
                ..Default::default()
            })
        }
    }
}

#[derive(Default)]
pub struct ProofView {
    key: Option<ProofKey>,
    generation: u32,
    cache: Option<(RgbSpace, ProofRecipe, Arc<layer_color::ProofLut>)>,
    error: Option<String>,
}
#[derive(Serialize)]
pub struct ProofStatus {
    pub generation: u32,
    pub needed: bool,
    pub text: String,
    pub error: Option<String>,
    pub bytes: usize,
}
impl ProofView {
    pub fn observe<R: CanvasRenderer>(&mut self, s: &UiSession<R>) -> ProofStatus {
        let key = ProofKey::current(s);
        if self.key.as_ref() != Some(&key) {
            self.generation = self.generation.wrapping_add(1);
            self.error = None;
            if self.cache.as_ref().is_some_and(|(space, recipe, _)| {
                *space != key.space || Some(recipe) != key.recipe.as_ref()
            }) {
                self.cache = None;
            }
            self.key = Some(key);
        }
        let visible = (s.state().soft_proof || s.state().gamut_warning)
            && s.engine().document().proof.is_some();
        let needed =
            visible && self.cache.is_none() && self.error.is_none() && !s.rendering_suspended();
        let text = if !visible {
            String::new()
        } else if self.error.is_some() {
            "Proof unavailable".into()
        } else if self.cache.is_none() {
            "Preparing proof…".into()
        } else {
            let name = &s.engine().document().proof.as_ref().unwrap().name;
            if s.state().soft_proof {
                format!(
                    "Proof: {name}{}",
                    if s.state().gamut_warning {
                        " · Gamut warning"
                    } else {
                        ""
                    }
                )
            } else {
                format!("Gamut: {name}")
            }
        };
        ProofStatus {
            generation: self.generation,
            needed,
            text,
            error: self.error.clone(),
            bytes: self.cache.as_ref().map_or(0, |c| c.2.byte_len()),
        }
    }
    pub fn lut<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
    ) -> Option<Arc<layer_color::ProofLut>> {
        self.observe(s);
        self.cache.as_ref().map(|c| c.2.clone())
    }
    pub fn retain(
        &mut self,
        job: &ProofPreparation,
        lut: Arc<layer_color::ProofLut>,
    ) -> Result<(), String> {
        if lut.space() != job.space() {
            return Err("Proof working space changed".into());
        }
        self.cache = Some((job.space(), job.recipe.clone(), lut));
        self.error = None;
        Ok(())
    }
    pub fn fail<R: CanvasRenderer>(
        &mut self,
        s: &UiSession<R>,
        job: &ProofPreparation,
        error: String,
    ) {
        self.observe(s);
        if job.request.is_none() && job.validate(s).is_ok() {
            self.cache = None;
            self.error = Some(error);
        }
    }
}

pub fn proof_form<R: CanvasRenderer>(s: &UiSession<R>) -> serde_json::Value {
    let document = s.engine().document();
    serde_json::json!({
        "mode": s.proof_panel_mode(),
        "hdr": document.color.depth.is_float(),
        "rendition": s.effective_sdr_rendition(),
        "numbers": crate::proof_panel::sdr_number_controls(),
        "pad": crate::proof_panel::sdr_tone_pad(),
        "pad_values": crate::proof_panel::sdr_pad_values(s.effective_sdr_rendition()),
        "recipe": document.proof.clone().unwrap_or_else(|| ProofRecipe::new(document.color.space.name().into(), ColorProfile::Builtin(document.color.space))),
        "document_profile": document.proof,
        "profiles": RgbSpace::ALL.map(crate::ExportProfile::builtin),
    })
}
