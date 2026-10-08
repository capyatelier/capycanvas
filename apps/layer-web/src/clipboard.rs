use super::*;
use layer_ui::{ClipboardCapture, PixelClip};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[wasm_bindgen]
pub struct WebClipTask {
    capture: ClipboardCapture,
    gpu: layer_render_wgpu::snapshot::SnapshotGpu,
}

#[wasm_bindgen]
pub struct WebClip {
    clip: PixelClip,
}

#[wasm_bindgen]
impl WebClip {
    pub fn png(&self) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(&self.clip.png[..])
    }
}

#[wasm_bindgen]
impl WebClipTask {
    pub fn large(&self) -> bool {
        self.capture.large
    }
    pub fn run(self, control: &output::WebCaptureControl, nonce: String) -> js_sys::Promise {
        future_to_promise(copy(self, control.inner.clone(), nonce))
    }
}

async fn copy(
    task: WebClipTask,
    control: layer_render_wgpu::snapshot::CaptureControl,
    nonce: String,
) -> Result<JsValue, JsValue> {
    let mut capture = task.capture;
    for (id, layer) in capture.layer_captures() {
        let (source, _) = pixels(&layer, &task.gpu, control.clone()).await?;
        capture.set_layer_source(id, source).map_err(js)?;
    }
    capture.finish_layer_sources().map_err(js)?;
    let (source, png) = pixels(&capture, &task.gpu, control).await?;
    Ok(WebClip { clip: capture.finish(nonce, source, png).map_err(js)? }.into())
}

async fn pixels(capture: &ClipboardCapture, gpu: &layer_render_wgpu::snapshot::SnapshotGpu,
    control: layer_render_wgpu::snapshot::CaptureControl,
) -> Result<(std::sync::Arc<layer_core::color::source::SourceImage>, Vec<u8>), JsValue> {
    artwork_transfer::wait_backing(&capture.scene.artwork).await?;
    let document = capture.scene.view();
    let crop = capture.crop;
    let rendition = document.composition().color.depth.is_float().then_some(document.artwork().outputs.get(document.artwork().default_output).unwrap().sdr);
    let clip = output::ClipMetadata {
        origin: [crop[0], crop[1]],
        document: capture.window.map_or(document.composition().size, |(_, extent)| extent),
        source: capture.original.is_none(),
    };
    let mut snapshot = gpu.capture_scene(capture.scene.clone(), capture.scope.clone(), control.clone())
        .map_err(js)?;
    if let Some((origin, extent)) = capture.window { snapshot.capture_window(origin, extent).map_err(js)?; }
    let buffers = js_sys::Array::new();
    let guide = if rendition.is_some() {
        let guide = snapshot.local_tone_guide_async().await.map_err(js)?;
        let samples = js_sys::Float32Array::from(guide.samples.as_flattened());
        buffers.push(&js_sys::Uint8Array::new(&samples.buffer()));
        Some((guide.extent, guide.peak))
    } else {
        None
    };
    let token = raster_worker::call_cancellable("output-begin", "", &js_sys::Array::new(), control.clone())
        .await?
        .as_string()
        .ok_or_else(|| js("Missing output worker token"))?;
    let metadata = output::clip_metadata(&token, [crop[2], crop[3]], document, rendition, guide, clip);
    let result = async {
        let mut y = crop[1];
        while y < crop[1] + crop[3] {
            output::cancelled(&control)?;
            let (rows, mut pixels) = snapshot.read_window_band_async(crop, y).await.map_err(js)?;
            if let Some(selection) = &capture.coverage {
                let coverage = snapshot
                    .selection_coverage_async(selection, [crop[0], y, crop[2], rows])
                    .await
                    .map_err(js)?;
                for (line, row) in pixels.chunks_exact_mut(crop[2] as usize).enumerate() {
                    coverage.apply(crop[0], y + line as u32, row);
                }
            }
            let bytes: Vec<u8> = pixels.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
            let parts = js_sys::Array::new();
            parts.push(&js_sys::Uint8Array::from(bytes.as_slice()));
            drop(bytes);
            JsFuture::from(raster_worker::call("output-band", &token, &parts)?).await?;
            y += rows;
        }
        drop(snapshot);
        raster_worker::call_cancellable("output-encode", &metadata, &buffers, control.clone()).await
    }
    .await;
    if result.is_err() || control.is_cancelled() {
        let _ = JsFuture::from(raster_worker::call("output-close", &token, &js_sys::Array::new())?).await;
    }
    output::cancelled(&control)?;
    let result = result?;
    let parts: js_sys::Array = js_sys::Reflect::get(&result, &js("buffers"))?.dyn_into()?;
    let png_index = js_sys::Reflect::get(&result, &js("png"))?.as_f64().ok_or_else(|| js("Missing clipboard PNG"))?;
    if parts.length().checked_sub(1).map(f64::from) != Some(png_index) {
        return Err(js("Invalid clipboard PNG index"));
    }
    let png = js_sys::Uint8Array::new(&parts.pop()).to_vec();
    let source = match &capture.original {
        Some(original) => original.clone(),
        None => {
            let metadata = js_sys::Reflect::get(&result, &js("metadata"))?
                .as_string()
                .ok_or_else(|| js("Missing clipboard source"))?;
            artwork_transfer::unpack(&metadata, parts)
                .await?
                .paint
                .iter()
                .find_map(|(_,_,paint)| paint.base.as_ref().map(|base| base.image.storage().clone()))
                .ok_or_else(|| js("Missing clipboard source"))?
        }
    };
    Ok((source, png))
}

#[wasm_bindgen]
impl WebApp {
    /// Freeze the pending Copy, Cut or Copy Merged request `id`.
    pub fn capture_clip(&mut self, id: u32) -> Result<WebClipTask, JsValue> {
        let gpu = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Wait for the canvas"))?
            .snapshot_gpu();
        Ok(WebClipTask { capture: self.session.capture_clipboard(id).map_err(js)?, gpu })
    }
    /// Keep a finished copy for every drawing in this window.
    pub fn adopt_clip(&mut self, clip: WebClip) {
        self.documents.clip.set(clip.clip);
    }
    pub fn clip_nonce(&self) -> Option<String> {
        self.documents.clip.get().map(|clip| clip.nonce.clone())
    }
}
