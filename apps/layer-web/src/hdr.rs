//! HDR presentation remains mapped SDR until a host HDR surface is qualified.
//! Idle analysis and presentation share the same GPU guide as native hosts.
use super::*;
use layer_render_wgpu::{local_tone::GpuToneGuide, snapshot::CaptureControl};
use layer_ui::proof_workflow::ToneKey;
use std::sync::Arc;
use wasm_bindgen_futures::future_to_promise;

/// Browser admission is deliberately independent of installed-RAM hints. Real
/// Chrome measurements exceed the combined renderer/GPU process budget at the
/// larger photo sizes; never open by silently reducing precision or dimensions.
pub(super) fn admit_document(document: &layer_core::Document) -> Result<(), JsValue> {
    if document.color.depth.is_float()
        && u64::from(document.width) * u64::from(document.height) > 12_000_000
    {
        return Err(js(
            "HDR drawings above 12 megapixels are not supported in this browser build. Open the editable master in the native app, or explicitly resize a copy there. Your current drawing is unchanged.",
        ));
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct ToneState {
    key: Option<ToneKey>,
    owner: Option<Arc<std::sync::Mutex<Option<String>>>>,
    generation: u32,
    ready: bool,
    published: Option<ToneKey>,
    publications: u32,
    pub pending: Option<CaptureControl>,
    analysed_time: f32,
    error: Option<String>,
}
#[wasm_bindgen]
pub struct WebTone {
    key: ToneKey,
    owner: Arc<std::sync::Mutex<Option<String>>>,
    guide: Arc<GpuToneGuide>,
    control: CaptureControl,
    time: f32,
}
#[wasm_bindgen]
impl WebApp {
    pub fn proof_control(&mut self, action: JsValue) -> Result<JsValue, JsValue> {
        let action = serde_wasm_bindgen::from_value(action).map_err(js)?;
        serialize(&layer_ui::proof_panel::apply(&mut self.session, action).map_err(js)?)
    }
    pub fn proof_texture(&self, edge: u32) -> Vec<u8> {
        layer_ui::proof_panel::sdr_direction_texture(edge)
    }
    pub fn tone_status(&mut self) -> Result<JsValue, JsValue> {
        let key = ToneKey::current(&self.session);
        let owner = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .map(|g| g.lost.clone());
        let same_owner = match (&owner, &self.tone.owner) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if key != self.tone.key || !same_owner {
            let retain = same_owner
                && self
                    .tone
                    .published
                    .as_ref()
                    .zip(key.as_ref())
                    .is_some_and(|(old, next)| old.can_preview(next));
            if let Some(control) = self.tone.pending.take() {
                control.cancel();
            }
            self.tone.key = key;
            self.tone.owner = owner;
            self.tone.generation = self.tone.generation.wrapping_add(1);
            self.tone.ready = false;
            self.tone.error = None;
            if !retain {
                self.tone.published = None;
                if let Some(gpu) = self.session.renderer_mut().0.as_mut() {
                    gpu.presenter
                        .set_gpu_local_tone_guide(&gpu.renderer, None)
                        .map_err(js)?;
                }
            }
        }
        let animated = self.session.engine().document().has_animated_effects()
            && (self.session.engine().animation_time() - self.tone.analysed_time).abs() >= 0.5;
        serialize(
            &serde_json::json!({"generation":self.tone.generation,"hdr":self.tone.key.is_some(),
            "needed":self.tone.key.is_some() && (!self.tone.ready || animated) && self.tone.error.is_none() && self.session.require_document_snapshot_idle().is_ok(),
            "ready":self.tone.ready,"retained":self.tone.published.is_some(),
            "publications":self.tone.publications,"idle":self.session.require_document_snapshot_idle().is_ok(),"error":self.tone.error}),
        )
    }
    pub fn tone_failed(&mut self, generation: u32, error: String) {
        if generation == self.tone.generation {
            self.tone.error = Some(error);
        }
    }
    pub fn tone_prepare(
        &mut self,
        control: &output::WebCaptureControl,
    ) -> Result<js_sys::Promise, JsValue> {
        self.session.require_document_snapshot_idle().map_err(js)?;
        let key = ToneKey::current(&self.session)
            .ok_or_else(|| js("HDR analysis requires HDR artwork"))?;
        let project = self.session.capture_project_recovery().map_err(js)?;
        let live = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Canvas unavailable"))?;
        let gpu = live.renderer.snapshot_gpu();
        let owner = live.lost.clone();
        let background = self.session.engine().view().background_rgba_linear;
        let time = self.session.engine().animation_time();
        let control = control.inner.clone();
        if let Some(previous) = self.tone.pending.replace(control.clone()) {
            previous.cancel();
        }
        Ok(future_to_promise(async move {
            raster_project::wait_backing(&project).await?;
            let mut capture = gpu
                .capture(
                    project,
                    background,
                    time,
                    Default::default(),
                    control.clone(),
                )
                .map_err(js)?;
            let guide = capture.gpu_local_tone_guide_async().await.map_err(js)?;
            output::cancelled(&control)?;
            Ok(WebTone {
                key,
                owner,
                time,
                guide,
                control,
            }
            .into())
        }))
    }
    pub fn tone_apply(&mut self, tone: WebTone) -> Result<bool, JsValue> {
        if tone.control.is_cancelled()
            || self.session.require_document_snapshot_idle().is_err()
            || ToneKey::current(&self.session).as_ref() != Some(&tone.key)
        {
            return Ok(false);
        }
        let gpu = self
            .session
            .renderer_mut()
            .0
            .as_mut()
            .ok_or_else(|| js("Canvas unavailable"))?;
        if !Arc::ptr_eq(&gpu.lost, &tone.owner) {
            return Ok(false);
        }
        gpu.presenter
            .set_gpu_local_tone_guide(&gpu.renderer, Some(tone.guide))
            .map_err(js)?;
        self.tone.analysed_time = tone.time;
        self.tone.published = Some(tone.key);
        self.tone.publications = self.tone.publications.wrapping_add(1);
        self.tone.ready = true;
        self.tone.error = None;
        Ok(true)
    }
}

/// CPU-only worker entry; no canvas or renderer is needed for the immutable guide.
#[wasm_bindgen]
pub fn proof_texture_build(edge: u32) -> Vec<u8> {
    layer_ui::proof_panel::sdr_direction_texture(edge)
}
/// Comparison previews download only the bounded GPU guide for CPU mapping.
pub(super) async fn preview_document(
    gpu: &layer_render_wgpu::snapshot::SnapshotGpu,
    project: layer_core::Project,
    background: [f32; 4],
    time: f32,
    control: layer_render_wgpu::snapshot::CaptureControl,
) -> Result<layer_render_wgpu::snapshot::SnapshotPreview, JsValue> {
    let extent = [project.document.width, project.document.height];
    let color = project.document.color;
    let rendition = project.document.sdr_rendition;
    let mut preview = layer_color::AreaPreview::new(extent, [512, 384]).map_err(js)?;
    let mut capture = gpu
        .capture(
            project,
            background,
            time,
            Default::default(),
            control.clone(),
        )
        .map_err(js)?;
    let guide = if color.depth.is_float() {
        Some(capture.local_tone_guide_async().await.map_err(js)?)
    } else {
        None
    };
    let mut y = 0;
    while y < extent[1] {
        output::cancelled(&control)?;
        let (rows, pixels) = capture.read_band_async(y).await.map_err(js)?;
        for row in pixels.chunks_exact(extent[0] as usize) {
            preview.push(row).map_err(js)?;
        }
        y += rows;
        documents::yield_browser().await?;
    }
    drop(capture);
    let (size, mut pixels) = preview.finish().map_err(js)?;
    let space = layer_core::color::RgbSpace::Srgb;
    if let Some(guide) = guide {
        let mapper = rendition.mapper(color.space, space);
        for (i, p) in pixels.iter_mut().enumerate() {
            let pos = [i as u32 % size[0], i as u32 / size[0]];
            *p = mapper.map_local_premultiplied(
                *p,
                std::array::from_fn(|c| (pos[c] as f32 + 0.5) * extent[c] as f32 / size[c] as f32),
                &guide,
            );
        }
    } else {
        let matrix = color.space.linear_transform(space);
        for p in &mut pixels {
            let rgb = layer_core::color::rgb::apply(matrix, [p[0], p[1], p[2]].map(f64::from));
            p[..3].copy_from_slice(&rgb.map(|v| v as f32));
        }
    }
    output::cancelled(&control)?;
    Ok(layer_render_wgpu::snapshot::SnapshotPreview {
        extent: size,
        space,
        pixels,
    })
}

impl WebApp {
    pub(super) fn clear_incompatible_tone(&mut self) -> Result<(), JsValue> {
        let compatible = self
            .tone
            .published
            .as_ref()
            .is_none_or(|key| key.can_preview_current(&self.session))
            && self
                .session
                .engine()
                .backend()
                .0
                .as_ref()
                .is_some_and(|gpu| {
                    self.tone
                        .owner
                        .as_ref()
                        .is_some_and(|owner| Arc::ptr_eq(owner, &gpu.lost))
                });
        if !compatible {
            if let Some(control) = self.tone.pending.take() {
                control.cancel();
            }
            self.tone.published = None;
            self.tone.ready = false;
            if let Some(gpu) = self.session.renderer_mut().0.as_mut() {
                gpu.presenter
                    .set_gpu_local_tone_guide(&gpu.renderer, None)
                    .map_err(js)?;
            }
        }
        Ok(())
    }
}
