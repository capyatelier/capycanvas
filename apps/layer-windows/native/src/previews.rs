//! Bounded binary filter-preview transport. No GPU wait or pixel JSON.
use layer_host::NativeHost;
use layer_render::CanvasRenderer;
use serde::Deserialize;
use std::{
    ffi::{CString, c_char},
    sync::Arc,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    request: String,
    revision: Option<String>,
    filters: Vec<Arc<str>>,
    size: [u32; 2],
}
pub struct CapyPreview {
    metadata: CString,
    bytes: Vec<u8>,
}
fn token(epoch: u64, revision: (u64, u64, u64)) -> String {
    format!("{epoch}:{}:{}:{}", revision.0, revision.1, revision.2)
}
fn validate(query: &Query) -> Result<u64, String> {
    if query.filters.len() > 8
        || query.filters.iter().any(|id| id.len() > 256)
        || query.size.contains(&0)
        || query.size[0] > 512
        || query.size[1] > 128
    {
        return Err("Invalid filter preview bounds".into());
    }
    query
        .request
        .parse()
        .map_err(|_| "Invalid preview request".into())
}
pub fn query(host: &mut NativeHost, json: &str) -> Result<CapyPreview, String> {
    let query: Query = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let request = validate(&query)?;
    let revision = host.session.filter_preview_revision();
    let current = token(host.session.state().document_file.epoch, revision);
    let matched = query.revision.as_deref() == Some(current.as_str());
    let status = host.query(serde_json::json!({
        "type":"filter_previews", "request":request,
        "revision":if matched {Some(revision)} else {None},
        "filters":query.filters, "size":query.size
    }))?;
    let renderer = host.session.renderer_mut();
    if let Some(gpu) = &renderer.0 {
        gpu.device()
            .poll(wgpu::PollType::Poll)
            .map_err(|e| e.to_string())?;
    }
    let mut metadata = serde_json::json!({"revision":current,"accepted":status["accepted"]});
    let bytes = if let Some(atlas) = renderer.take_filter_previews() {
        let atlas = atlas.map_err(|e| e.to_string())?;
        let image = atlas.image;
        let count = atlas.filters.len();
        if count == 0
            || count > 8
            || image.width == 0
            || image.width > 512
            || image.height == 0
            || !(image.height as usize).is_multiple_of(count)
            || image.height as usize / count > 128
            || image.stride != image.width * 4
            || image.bytes.len() != image.stride as usize * image.height as usize
        {
            return Err("Invalid filter preview atlas".into());
        }
        metadata["atlas"] = serde_json::json!({
            "request":image.request_id.to_string(), "width":image.width,
            "height":image.height, "stride":image.stride, "filters":atlas.filters
        });
        image.bytes
    } else {
        Vec::new()
    };
    Ok(CapyPreview {
        metadata: CString::new(metadata.to_string()).map_err(|e| e.to_string())?,
        bytes,
    })
}
/// # Safety
/// The packet must be null or a live uniquely owned result of capy_filter_previews.
/// The returned buffer stays valid until the packet is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_preview_metadata(packet: *const CapyPreview) -> *const c_char {
    unsafe { packet.as_ref() }.map_or(std::ptr::null(), |p| p.metadata.as_ptr())
}
/// # Safety
/// The packet must be null or a live result of capy_filter_previews; length must
/// point to writable storage. The bytes are immutable until capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_preview_bytes(
    packet: *const CapyPreview,
    length: *mut usize,
) -> *const u8 {
    let Some(length) = (unsafe { length.as_mut() }) else {
        return std::ptr::null();
    };
    *length = 0;
    unsafe { packet.as_ref() }.map_or(std::ptr::null(), |p| {
        *length = p.bytes.len();
        p.bytes.as_ptr()
    })
}
/// # Safety
/// Free a packet from capy_filter_previews once, after all buffer readers finish.
/// The packet owns CPU data only and may be transferred to a conversion worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_preview_free(packet: *mut CapyPreview) {
    if !packet.is_null() {
        unsafe { drop(Box::from_raw(packet)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_preserves_large_revisions_and_document_replacement() {
        let a = token(0, (u64::MAX, u64::MAX - 1, 1));
        assert_ne!(a, token(1, (u64::MAX, u64::MAX - 1, 1)));
        assert_ne!(a, token(0, (u64::MAX - 1, u64::MAX - 1, 1)));
        assert!(a.contains(&u64::MAX.to_string()));
    }
    #[test]
    fn transport_bounds_and_owned_binary_lifetime() {
        let mut q = Query {
            request: u64::MAX.to_string(),
            revision: None,
            filters: vec!["curves".into(); 8],
            size: [512, 128],
        };
        assert_eq!(validate(&q).unwrap(), u64::MAX);
        q.filters.push("curves".into());
        assert!(validate(&q).is_err());
        q.filters.pop();
        for size in [[0, 1], [513, 128], [512, 129]] {
            q.size = size;
            assert!(validate(&q).is_err());
        }
        let p = Box::into_raw(Box::new(CapyPreview {
            metadata: CString::new("{}").unwrap(),
            bytes: vec![1, 2, 3, 4],
        }));
        unsafe {
            let mut n = 0;
            let bytes = capy_preview_bytes(p, &mut n);
            assert_eq!(std::slice::from_raw_parts(bytes, n), &[1, 2, 3, 4]);
            assert_eq!(
                std::ffi::CStr::from_ptr(capy_preview_metadata(p))
                    .to_str()
                    .unwrap(),
                "{}"
            );
            capy_preview_free(p);
            assert!(capy_preview_bytes(std::ptr::null(), &mut n).is_null());
            assert_eq!(n, 0);
        }
    }
}
