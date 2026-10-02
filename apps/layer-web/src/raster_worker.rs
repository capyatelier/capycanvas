//! Browser worker transport. File bytes are validated in the file worker; live
//! capture compression shares the exact native tile codec and pixel descriptors.
use super::*;
use layer_core::{
    color::PixelDescriptor,
    raster::{TILE_SIZE, TileBlob},
};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen_futures::JsFuture;

thread_local! { static WORKER: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) }; }

#[wasm_bindgen]
pub fn configure_raster_worker(worker: js_sys::Function) {
    WORKER.with(|slot| *slot.borrow_mut() = Some(worker));
}

pub(super) fn call(
    operation: &str,
    metadata: &str,
    buffers: &js_sys::Array,
) -> Result<js_sys::Promise, JsValue> {
    let request = js_sys::Object::new();
    js_sys::Reflect::set(&request, &js("operation"), &js(operation))?;
    js_sys::Reflect::set(&request, &js("metadata"), &js(metadata))?;
    js_sys::Reflect::set(&request, &js("buffers"), buffers)?;
    WORKER
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .ok_or_else(|| js("Raster worker unavailable"))?
                .call1(&JsValue::NULL, &request)
        })?
        .dyn_into()
}

/// The callback stays alive until the isolated output worker has settled.
pub(super) async fn call_cancellable(
    operation: &str, metadata: &str, buffers: &js_sys::Array,
    control: layer_render_wgpu::snapshot::CaptureControl,
) -> Result<JsValue, JsValue> {
    let cancelled = wasm_bindgen::closure::Closure::<dyn Fn() -> bool>::new(move || control.is_cancelled());
    let request = js_sys::Object::new();
    js_sys::Reflect::set(&request, &js("operation"), &js(operation))?;
    js_sys::Reflect::set(&request, &js("metadata"), &js(metadata))?;
    js_sys::Reflect::set(&request, &js("buffers"), buffers)?;
    js_sys::Reflect::set(&request, &js("cancelled"), cancelled.as_ref())?;
    let promise = WORKER.with(|slot| slot.borrow().as_ref()
        .ok_or_else(|| js("Raster worker unavailable"))?.call1(&JsValue::NULL, &request))?;
    let result = JsFuture::from(promise.dyn_into::<js_sys::Promise>()?).await;
    drop(cancelled);
    result
}

pub(super) fn install(renderer: &mut WgpuRasterizer) {
    // Browser capacity-based admission is documented separately from native
    // measured headroom. Filters and display share this bounded allowance.
    renderer.set_complete_display_allowance(raster_project::photo_memory_budget().encode_bytes as u64);
    renderer.set_snapshot_worker(Rc::new(|request, control| Box::pin(async move {
        if control.is_cancelled() { return Err("Operation cancelled".into()); }
        let (project, task) = match request {
            layer_render::SnapshotRequest::Bounds(request) => (layer_core::Project { document: (*request.document).clone() },
                SnapshotTask::Bounds(request.scope, request.time, request.effect_times)),
            layer_render::SnapshotRequest::TransformPixels(plan) => (plan.input,
                SnapshotTask::TransformPixels { output: plan.output, target: plan.target, scope: plan.scope, geometry: plan.geometry }),
        };
        let mut packing = std::pin::pin!(raster_project::pack(project));
        let packed = std::future::poll_fn(|cx| {
            if control.is_cancelled() { return std::task::Poll::Ready(Err(js("Operation cancelled"))); }
            std::future::Future::poll(packing.as_mut(), cx)
        }).await.map_err(|e| format!("{e:?}"))?;
        let (metadata, buffers) = packed_parts(&packed).map_err(|e| format!("{e:?}"))?;
        let metadata = serde_json::to_string(&(metadata, &task)).map_err(|e| e.to_string())?;
        let result = call_cancellable("snapshot", &metadata, &buffers, control.clone()).await.map_err(|e| format!("{e:?}"))?;
        if control.is_cancelled() { return Err("Operation cancelled".into()); }
        match task {
            SnapshotTask::Bounds(..) => serde_wasm_bindgen::from_value(result).map(layer_render::SnapshotResult::Bounds).map_err(|e| e.to_string()),
            SnapshotTask::TransformPixels { mut output, scope, .. } => {
                let (metadata, buffers) = packed_parts(&result).map_err(|e| format!("{e:?}"))?;
                let project = raster_project::unpack(&metadata, buffers, true).await.map_err(|e| format!("{e:?}"))?;
                let layer = project.document.layer(output.id).ok_or("Missing transformed layer")?;
                if scope != layer_core::TransformPixelsScope::Mask { output.raster = layer.raster.clone(); }
                if scope != (layer_core::TransformPixelsScope::Paint { linked_mask: false }) {
                    output.mask.as_mut().ok_or("Missing original mask")?.raster = layer.mask.as_ref().ok_or("Missing transformed mask")?.raster.clone();
                }
                Ok(layer_render::SnapshotResult::TransformPixels(Box::new(output)))
            }
        }
    })));
    renderer.set_browser_raster_encoder(Rc::new(|bytes, descriptors| {
        Box::pin(async move {
            let metadata = serde_json::to_string(&descriptors).map_err(|e| e.to_string())?;
            let buffers = js_sys::Array::new();
            buffers.push(&js_sys::Uint8Array::from(bytes.as_slice()));
            drop(bytes);
            let promise = call("encode", &metadata, &buffers).map_err(|e| format!("{e:?}"))?;
            let result = JsFuture::from(promise)
                .await
                .map_err(|e| format!("{e:?}"))?;
            let bytes = js_sys::Uint8Array::new(&result).to_vec();
            let mut offset = 0;
            let mut blobs = Vec::with_capacity(descriptors.len());
            for descriptor in descriptors {
                let header = bytes
                    .get(offset..offset + 36)
                    .ok_or("Incomplete raster worker result")?;
                let digest = header[..32].try_into().unwrap();
                let size = u32::from_le_bytes(header[32..].try_into().unwrap()) as usize;
                offset += 36;
                let encoded = bytes
                    .get(offset..offset + size)
                    .ok_or("Incomplete raster worker tile")?;
                blobs.push(TileBlob::from_verified_worker(
                    descriptor,
                    digest,
                    encoded.into(),
                )?);
                offset += size;
            }
            if offset != bytes.len() {
                return Err("Trailing raster worker data".into());
            }
            Ok(blobs)
        })
    }));
}

#[wasm_bindgen]
pub fn raster_worker_encode(metadata: &str, bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    let descriptors: Vec<PixelDescriptor> = serde_json::from_str(metadata).map_err(js)?;
    let mut result = Vec::new();
    for tile in encode_tiles(bytes, descriptors).map_err(js)? {
        result.extend_from_slice(&tile.digest);
        result.extend_from_slice(&(tile.compressed_len() as u32).to_le_bytes());
        result.extend_from_slice(&tile.compressed().map_err(js)?);
    }
    Ok(result)
}

fn encode_tiles(bytes: &[u8], descriptors: Vec<PixelDescriptor>) -> Result<Vec<TileBlob>, String> {
    if bytes.len() > 16 * 1024 * 1024 || descriptors.len() > 256 {
        return Err("Raster worker chunk exceeds its budget".into());
    }
    let mut result = Vec::with_capacity(descriptors.len());
    let mut offset = 0;
    for descriptor in descriptors {
        let size = descriptor.byte_len([TILE_SIZE; 2]).ok_or("Unsupported raster pixels")?;
        let raw = bytes.get(offset..offset + size).ok_or("Incomplete raster worker chunk")?;
        result.push(TileBlob::encode(descriptor, raw)?);
        offset += size;
    }
    if offset != bytes.len() { return Err("Trailing raster worker input".into()); }
    Ok(result)
}

#[derive(Serialize, Deserialize)]
enum SnapshotTask {
    Bounds(layer_core::ContentScope, f32, Vec<(layer_core::LayerId, f32)>),
    TransformPixels { output: layer_core::Layer, target: layer_core::LayerId,
        scope: layer_core::TransformPixelsScope, geometry: layer_core::ImageTransform },
}

fn packed_parts(packed: &JsValue) -> Result<(String, js_sys::Array), JsValue> {
    let metadata = js_sys::Reflect::get(packed, &js("metadata"))?.as_string().ok_or(js("Missing snapshot metadata"))?;
    let buffers = js_sys::Reflect::get(packed, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
    Ok((metadata, buffers))
}

#[wasm_bindgen]
pub async fn raster_worker_snapshot(metadata: &str, buffers: js_sys::Array) -> Result<JsValue, JsValue> {
    let (metadata, task): (String, SnapshotTask) = serde_json::from_str(metadata).map_err(js)?;
    let mut project = raster_project::unpack(&metadata, buffers, true).await?;
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
    let instance = wgpu::Instance::new(descriptor);
    let (adapter, device, queue) = request_device(&instance, None).await?;
    let mut renderer = WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue, project.document.color).map_err(js)?;
    renderer.set_browser_raster_encoder(Rc::new(|bytes, descriptors| Box::pin(async move { encode_tiles(&bytes, descriptors) })));
    match task {
        SnapshotTask::Bounds(scope, time, effect_times) => {
            let mut request = layer_core::ContentBoundsRequest::new(&project.document, scope);
            request.time = time;
            request.effect_times = effect_times;
            let bounds = renderer.snapshot_gpu().content_bounds(request, Default::default()).await.map_err(js)?;
            serialize(&bounds)
        }
        SnapshotTask::TransformPixels { output, target, scope, geometry } => {
            let plan = layer_core::TransformPixelsPlan { input: project.clone(), output, target, scope, geometry };
            let output = renderer.snapshot_gpu().transform_pixels(plan, Default::default()).await.map_err(js)?;
            let mut transfer = layer_core::Layer::paint(output.id, "");
            if scope != layer_core::TransformPixelsScope::Mask { transfer.raster = output.raster; }
            if scope != (layer_core::TransformPixelsScope::Paint { linked_mask: false }) {
                let output = output.mask.ok_or(js("Missing transformed mask"))?;
                let mut mask = layer_core::LayerMask::reveal_all(output.id, layer_core::Point::default());
                mask.raster = output.raster;
                transfer.mask = Some(mask);
            }
            project.document.layers = vec![transfer];
            project.document.active_mask = false;
            raster_project::pack(project).await
        }
    }
}
