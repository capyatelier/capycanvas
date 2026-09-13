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

pub(super) async fn png(image: layer_render::ReadbackImage) -> Result<JsValue, JsValue> {
    let metadata = serde_json::to_string(&(image.width, image.height, image.stride)).map_err(js)?;
    let buffers = js_sys::Array::new();
    for chunk in image.bytes.chunks(4 * 1024 * 1024) {
        buffers.push(&js_sys::Uint8Array::from(chunk));
        documents::yield_browser().await?;
    }
    drop(image);
    JsFuture::from(call("png", &metadata, &buffers)?).await
}
#[wasm_bindgen]
pub fn raster_worker_png(metadata: &str, buffers: js_sys::Array) -> Result<Vec<u8>, JsValue> {
    let (width, height, stride): (u32, u32, u32) = serde_json::from_str(metadata).map_err(js)?;
    let size = (stride as u64)
        .checked_mul(height as u64)
        .filter(|v| *v <= 512 * 1024 * 1024)
        .ok_or_else(|| js("PNG exceeds the browser export budget"))?;
    let mut bytes = Vec::new();
    for value in buffers.iter() {
        let part = value.dyn_into::<js_sys::Uint8Array>()?;
        if part.length() > 4 * 1024 * 1024 || bytes.len() as u64 + part.length() as u64 > size {
            return Err(js("Invalid PNG worker block"));
        }
        bytes.extend(part.to_vec());
    }
    let image = layer_render::ReadbackImage {
        request_id: 0,
        width,
        height,
        stride,
        bytes,
    };
    let mut result = Vec::new();
    image.write_png(&mut result).map_err(js)?;
    Ok(result)
}
