//! HDR presentation remains mapped SDR until a host HDR surface is qualified.
//! Full-resolution capture is asynchronous; bounded analysis runs in a worker.
use super::*;
use layer_core::color::hdr::{LocalToneBuilder, LocalToneGuide};
use layer_ui::proof_workflow::ToneKey;
use std::sync::Arc;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

/// Browser admission is deliberately independent of installed-RAM hints. Real
/// Chrome measurements exceed the combined renderer/GPU process budget at the
/// larger photo sizes; never open by silently reducing precision or dimensions.
pub(super) fn admit_document(document: &layer_core::Document) -> Result<(), JsValue> {
    if document.color.depth.is_float()
        && u64::from(document.width) * u64::from(document.height) > 12_000_000
    {
        return Err(js("HDR drawings above 12 megapixels are not supported in this browser build. Open the editable master in the native app, or explicitly resize a copy there. Your current drawing is unchanged."));
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct ToneState {
    key: Option<ToneKey>,
    owner: Option<Arc<std::sync::Mutex<Option<String>>>>,
    generation: u32,
    ready: bool,
    analysed_time: f32,
    error: Option<String>,
}
#[wasm_bindgen]
pub struct WebTone {
    key: ToneKey,
    owner: Arc<std::sync::Mutex<Option<String>>>,
    guide: Arc<LocalToneGuide>,
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
            self.tone.key = key;
            self.tone.owner = owner;
            self.tone.generation = self.tone.generation.wrapping_add(1);
            self.tone.ready = false;
            self.tone.error = None;
            if let Some(gpu) = self.session.renderer_mut().0.as_mut() {
                gpu.presenter
                    .set_local_tone_guide(&gpu.renderer, None)
                    .map_err(js)?;
            }
        }
        let animated = self.session.engine().document().has_animated_effects()
            && (self.session.engine().animation_time() - self.tone.analysed_time).abs() >= 0.5;
        serialize(
            &serde_json::json!({"generation":self.tone.generation,"hdr":self.tone.key.is_some(),
            "needed":self.tone.key.is_some() && (!self.tone.ready || animated) && self.tone.error.is_none() && self.session.require_document_snapshot_idle().is_ok(),
            "ready":self.tone.ready,"error":self.tone.error}),
        )
    }
    pub fn tone_failed(&mut self, generation: u32, error: String) {
        if generation == self.tone.generation {
            self.tone.error = Some(error);
        }
    }
    pub fn tone_prepare(
        &self,
        control: &output::WebCaptureControl,
        worker: js_sys::Function,
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
        Ok(future_to_promise(async move {
            raster_project::wait_backing(&project).await?;
            let extent = [project.document.width, project.document.height];
            let space = project.document.color.space;
            let mut builder = LocalToneBuilder::new(extent, space).map_err(js)?;
            let mut capture = gpu
                .capture(
                    project,
                    background,
                    time,
                    Default::default(),
                    control.clone(),
                )
                .map_err(js)?;
            let mut y = 0;
            while y < extent[1] {
                output::cancelled(&control)?;
                let (rows, pixels) = capture.read_band_async(y).await.map_err(js)?;
                for row in pixels.chunks_exact(extent[0] as usize) {
                    builder.push(row).map_err(js)?;
                }
                y += rows;
                documents::yield_browser().await?;
            }
            drop(capture);
            output::cancelled(&control)?;
            let (samples, peak) = builder.into_worker_samples().map_err(js)?;
            let request = js_sys::JSON::parse(
                &serde_json::json!({"type":"tone","extent":extent,"space":space,"peak":peak})
                    .to_string(),
            )?;
            let floats = js_sys::Float32Array::from(samples.as_flattened());
            js_sys::Reflect::set(
                &request,
                &js("bytes"),
                &js_sys::Uint8Array::new(&floats.buffer()),
            )?;
            drop(samples);
            let result = JsFuture::from(js_sys::Promise::resolve(
                &worker.call1(&JsValue::NULL, &request)?,
            ))
            .await?;
            output::cancelled(&control)?;
            let size: [u32; 2] =
                serde_wasm_bindgen::from_value(js_sys::Reflect::get(&result, &js("extent"))?)
                    .map_err(js)?;
            let bytes = js_sys::Uint8Array::new(&js_sys::Reflect::get(&result, &js("bytes"))?);
            if size.contains(&0)
                || size
                    .iter()
                    .any(|v| *v > layer_core::color::hdr::LOCAL_GUIDE_EDGE)
                || bytes.length() as usize != size[0] as usize * size[1] as usize * 16
            {
                return Err(js("Invalid local tone response"));
            }
            let mut samples = vec![[0.; 4]; size[0] as usize * size[1] as usize];
            js_sys::Float32Array::new_with_byte_offset_and_length(
                &bytes.buffer(),
                bytes.byte_offset(),
                bytes.length() / 4,
            )
            .copy_to(samples.as_flattened_mut());
            if samples.iter().flatten().any(|v| !v.is_finite()) {
                return Err(js("Invalid local tone response"));
            }
            Ok(WebTone {
                key,
                owner,
                time,
                guide: Arc::new(LocalToneGuide {
                    extent: size,
                    document_extent: extent,
                    samples,
                    peak,
                }),
            }
            .into())
        }))
    }
    pub fn tone_apply(&mut self, tone: WebTone) -> Result<(), JsValue> {
        if ToneKey::current(&self.session).as_ref() != Some(&tone.key) {
            return Err(js("HDR artwork changed during analysis"));
        }
        let gpu = self
            .session
            .renderer_mut()
            .0
            .as_mut()
            .ok_or_else(|| js("Canvas unavailable"))?;
        if !Arc::ptr_eq(&gpu.lost, &tone.owner) {
            return Err(js("HDR canvas changed during analysis"));
        }
        gpu.presenter
            .set_local_tone_guide(&gpu.renderer, Some(tone.guide))
            .map_err(js)?;
        self.tone.analysed_time = tone.time;
        self.tone.ready = true;
        self.tone.error = None;
        Ok(())
    }
}
#[wasm_bindgen]
pub fn tone_worker_build(request: JsValue) -> Result<JsValue, JsValue> {
    #[derive(Deserialize)]
    struct Input {
        extent: [u32; 2],
        space: layer_core::color::RgbSpace,
        peak: f32,
    }
    let input: Input = serde_wasm_bindgen::from_value(request.clone()).map_err(js)?;
    let bytes = js_sys::Uint8Array::new(&js_sys::Reflect::get(&request, &js("bytes"))?);
    if bytes.length() % 12 != 0 || bytes.length() > 768 * 768 * 12 {
        return Err(js("Invalid local tone input"));
    }
    let mut sums = vec![[0.; 3]; bytes.length() as usize / 12];
    js_sys::Float32Array::new_with_byte_offset_and_length(
        &bytes.buffer(),
        bytes.byte_offset(),
        bytes.length() / 4,
    )
    .copy_to(sums.as_flattened_mut());
    let guide = LocalToneBuilder::from_worker_samples(input.extent, input.space, sums, input.peak)
        .map_err(js)?
        .finish(|| false)
        .map_err(js)?;
    let result = js_sys::JSON::parse(&serde_json::json!({"extent":guide.extent}).to_string())?;
    let values = js_sys::Float32Array::from(guide.samples.as_flattened());
    js_sys::Reflect::set(
        &result,
        &js("bytes"),
        &js_sys::Uint8Array::new(&values.buffer()),
    )?;
    Ok(result)
}

/// Comparison previews share GTK's reduce-then-map semantics. GPU readback
/// yields; only the bounded local analysis is sent to an isolated CPU worker.
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
    let mut analysis = color
        .depth
        .is_float()
        .then(|| LocalToneBuilder::new(extent, color.space))
        .transpose()
        .map_err(js)?;
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
    let mut y = 0;
    while y < extent[1] {
        output::cancelled(&control)?;
        let (rows, pixels) = capture.read_band_async(y).await.map_err(js)?;
        for row in pixels.chunks_exact(extent[0] as usize) {
            preview.push(row).map_err(js)?;
            if let Some(a) = &mut analysis {
                a.push(row).map_err(js)?;
            }
        }
        y += rows;
        documents::yield_browser().await?;
    }
    drop(capture);
    let (size, mut pixels) = preview.finish().map_err(js)?;
    let space = layer_core::color::RgbSpace::Srgb;
    if let Some(analysis) = analysis {
        let (samples, peak) = analysis.into_worker_samples().map_err(js)?;
        let floats = js_sys::Float32Array::from(samples.as_flattened());
        drop(samples);
        let buffers = js_sys::Array::new();
        buffers.push(&js_sys::Uint8Array::new(&floats.buffer()));
        let metadata =
            serde_json::json!({"extent":extent,"space":color.space,"peak":peak}).to_string();
        let response =
            raster_worker::call_cancellable("tone", &metadata, &buffers, control.clone()).await?;
        let guide_size =
            serde_wasm_bindgen::from_value(js_sys::Reflect::get(&response, &js("extent"))?)
                .map_err(js)?;
        let bytes = js_sys::Uint8Array::new(&js_sys::Reflect::get(&response, &js("bytes"))?);
        let mut samples = vec![[0.; 4]; bytes.length() as usize / 16];
        js_sys::Float32Array::new_with_byte_offset_and_length(
            &bytes.buffer(),
            bytes.byte_offset(),
            bytes.length() / 4,
        )
        .copy_to(samples.as_flattened_mut());
        let guide = LocalToneGuide {
            extent: guide_size,
            document_extent: extent,
            samples,
            peak,
        };
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
