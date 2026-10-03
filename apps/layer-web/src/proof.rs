use super::*;
use layer_ui::proof_workflow::{ProofPreparation, proof_form};
use std::sync::Arc;
pub(super) fn proof_rejection(reason: layer_ui::ColorFeatureError) -> JsValue {
    let value = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&value, &js("document_host_error"), &serialize(&layer_ui::DocumentHostErrorCopy::Proof(reason)).unwrap_or(JsValue::NULL));
    value.into()
}

#[wasm_bindgen]
pub struct WebProof {
    job: ProofPreparation,
    lut: Option<Arc<layer_color::ProofLut>>,
}
#[wasm_bindgen]
impl WebProof {
    pub fn request(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.job).map_err(js)
    }
    pub fn load(
        &mut self,
        edge: u32,
        dark: bool,
        bytes: js_sys::Uint8Array,
    ) -> Result<(), JsValue> {
        if !matches!(edge, 65 | 129)
            || bytes.length() as usize != (edge as usize).pow(3) * 20
            || bytes.byte_offset() % 4 != 0
        {
            return Err(js("Invalid proof worker sample dimensions"));
        }
        let mut samples = vec![[0.; 5]; (edge as usize).pow(3)].into_boxed_slice();
        js_sys::Float32Array::new_with_byte_offset_and_length(
            &bytes.buffer(),
            bytes.byte_offset(),
            bytes.length() / 4,
        )
        .copy_to(samples.as_flattened_mut());
        self.lut = Some(Arc::new(
            layer_color::ProofLut::from_worker_samples(self.job.space(), edge, dark, samples)
                .map_err(js)?,
        ));
        Ok(())
    }
    pub fn preservation(&self) -> Option<js_sys::Uint8Array> {
        self.job.preservation().map(js_sys::Uint8Array::from)
    }
}
#[wasm_bindgen]
impl WebApp {
    pub fn proof_form(&self) -> Result<JsValue, JsValue> {
        js_sys::JSON::parse(&proof_form(&self.session).to_string())
    }
    pub fn proof_panel_copy(&self) -> Result<JsValue, JsValue> {
        js_sys::JSON::parse(&serde_json::to_string(&layer_ui::proof_workflow::proof_copy(&self.session)).map_err(js)?)
    }
    pub fn proof_status(&mut self) -> Result<JsValue, JsValue> {
        serialize(&self.proof.observe(&self.session))
    }
    pub fn proof_begin(&self, id: u32, recipe: JsValue) -> Result<WebProof, JsValue> {
        let recipe = if recipe.is_null() || recipe.is_undefined() {
            None
        } else {
            Some(serde_wasm_bindgen::from_value(recipe).map_err(js)?)
        };
        Ok(WebProof {
            job: (if id == u32::MAX { ProofPreparation::panel(&self.session,recipe.ok_or_else(||proof_rejection(layer_ui::ColorFeatureError::ProofChooseProfile))?) }
                else { ProofPreparation::begin(&self.session, (id != 0).then_some(id), recipe) })
                .map_err(proof_rejection)?,
            lut: None,
        })
    }
    pub fn proof_check(&self, job: &WebProof) -> Result<(), JsValue> {
        job.job.validate(&self.session).map_err(proof_rejection)
    }
    pub fn proof_apply(&mut self, job: &WebProof, preserved: bool) -> Result<JsValue, JsValue> {
        let lut = job
            .lut
            .clone()
            .ok_or_else(|| proof_rejection(layer_ui::ColorFeatureError::ProofPreviewNotPrepared))?;
        let change = job.job.apply(&mut self.session, preserved).map_err(proof_rejection)?;
        self.proof.retain(&job.job, lut).map_err(proof_rejection)?;
        serialize(&change)
    }
    pub fn proof_failed(&mut self, job: &WebProof, error: JsValue) {
        if let Some(layer_ui::DocumentHostErrorCopy::Proof(reason)) = documents::host_error_reason(&error) {
            self.proof.fail_reason(&self.session, &job.job, reason);
        } else { self.proof.fail(&self.session, &job.job, color_preferences::diagnostic_text(&error)); }
    }
}

#[wasm_bindgen]
pub fn proof_worker_build(request: &str) -> Result<JsValue, JsValue> {
    let job: ProofPreparation = serde_json::from_str(request).map_err(js)?;
    let lut = job.build(|| false).map_err(proof_rejection)?;
    let result = js_sys::Object::new();
    js_sys::Reflect::set(&result, &js("edge"), &JsValue::from(lut.edge()))?;
    js_sys::Reflect::set(&result, &js("dark"), &JsValue::from(lut.dark_grid()))?;
    let values = js_sys::Float32Array::from(lut.samples().as_flattened());
    js_sys::Reflect::set(
        &result,
        &js("bytes"),
        &js_sys::Uint8Array::new(&values.buffer()),
    )?;
    Ok(result.into())
}
