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

pub(super) fn install(renderer: &mut WgpuRasterizer) {
    // Browser capacity-based admission is documented separately from native
    // measured headroom. Retain only this document's completed display pixels.
    renderer.set_complete_display_allowance(raster_project::photo_memory_budget().encode_bytes as u64);
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
    if bytes.len() > 16 * 1024 * 1024 || descriptors.len() > 256 {
        return Err(js("Raster worker chunk exceeds its budget"));
    }
    let mut result = Vec::new();
    let mut offset = 0;
    for descriptor in descriptors {
        let size = descriptor
            .byte_len([TILE_SIZE; 2])
            .ok_or_else(|| js("Unsupported raster pixels"))?;
        let raw = bytes
            .get(offset..offset + size)
            .ok_or_else(|| js("Incomplete raster worker chunk"))?;
        let tile = TileBlob::encode(descriptor, raw).map_err(js)?;
        result.extend_from_slice(&tile.digest);
        result.extend_from_slice(&(tile.compressed().len() as u32).to_le_bytes());
        result.extend_from_slice(tile.compressed());
        offset += size;
    }
    if offset != bytes.len() {
        return Err(js("Trailing raster worker input"));
    }
    Ok(result)
}
