//! Bounded binary UI queries. No GPU wait or pixel JSON.
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
/// The packet must be null or a live uniquely owned result of a canvas UI query.
/// The returned buffer stays valid until the packet is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_preview_metadata(packet: *const CapyPreview) -> *const c_char {
    unsafe { packet.as_ref() }.map_or(std::ptr::null(), |p| p.metadata.as_ptr())
}
/// # Safety
/// The packet must be null or a live result of a canvas UI query; length must
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
/// Free a packet from a canvas UI query once, after all buffer readers finish.
/// The packet owns CPU data only and may be transferred to a conversion worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_preview_free(packet: *mut CapyPreview) {
    if !packet.is_null() {
        unsafe { drop(Box::from_raw(packet)) };
    }
}

impl CapyPreview {
    fn packet(metadata: serde_json::Value, bytes: Vec<u8>) -> Result<Self, String> {
        Ok(Self {
            metadata: CString::new(metadata.to_string()).map_err(|e| e.to_string())?,
            bytes,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerMenuQuery {
    epoch: String,
    id: Option<String>,
    mask: Option<bool>,
}

pub fn layer_menu(host: &mut NativeHost, json: &str) -> Result<CapyPreview, String> {
    let query: LayerMenuQuery = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let epoch = host.session.state().document_file.epoch;
    let document = host.session.engine().document();
    let id = query
        .id
        .map(|id| id.parse::<u64>().map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or(document.active_layer.0);
    let mask = query.mask.unwrap_or(document.active_mask);
    let epoch_matches = query.epoch.parse::<u64>().map_err(|e| e.to_string())? == epoch;
    let exists = host
        .session
        .engine()
        .document()
        .layer(layer_core::LayerId(id))
        .is_some_and(|layer| !mask || layer.mask.is_some());
    let menu = if epoch_matches && exists {
        serde_json::to_value(host.session.layer_menu(id, mask)?).map_err(|e| e.to_string())?
    } else {
        serde_json::Value::Null
    };
    CapyPreview::packet(
        serde_json::json!({"epoch":epoch.to_string(),"id":id.to_string(),"mask":mask,"menu":menu}),
        Vec::new(),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThumbnailQuery {
    epoch: String,
    requests: Vec<(String, String)>,
}

impl ThumbnailQuery {
    fn requests(self, epoch: u64) -> Result<Vec<(u64, u64)>, String> {
        if self.requests.len() > 8 {
            return Err("Too many thumbnail requests".into());
        }
        if self.epoch.parse::<u64>().map_err(|e| e.to_string())? != epoch {
            return Ok(Vec::new());
        }
        self.requests
            .into_iter()
            .map(|(request, target)| {
                Ok((
                    request.parse::<u64>().map_err(|e| e.to_string())?,
                    target.parse::<u64>().map_err(|e| e.to_string())?,
                ))
            })
            .collect()
    }
}
pub fn layer_thumbnails(host: &mut NativeHost, json: &str) -> Result<CapyPreview, String> {
    let query: ThumbnailQuery = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let epoch = host.session.state().document_file.epoch;
    let (accepted, images) = host.layer_thumbnails(query.requests(epoch)?)?;
    pack_layer_thumbnails(epoch, accepted, images)
}
fn pack_layer_thumbnails(
    epoch: u64,
    accepted: Vec<u64>,
    images: Vec<layer_render::ReadbackImage>,
) -> Result<CapyPreview, String> {
    if images.len() > 8 {
        return Err("Oversized thumbnail completion batch".into());
    }
    let mut bytes = Vec::with_capacity(images.len() * 32 * 32 * 4);
    let mut rows = Vec::new();
    for image in images {
        if image.width != 32
            || image.height != 32
            || image.stride != 128
            || image.bytes.len() != 4096
        {
            return Err("Invalid layer thumbnail".into());
        }
        rows.push(serde_json::json!({"request":image.request_id.to_string(),
            "width":image.width,"height":image.height,"offset":bytes.len(),"length":image.bytes.len()}));
        bytes.extend(image.bytes);
    }
    CapyPreview::packet(
        serde_json::json!({"epoch":epoch.to_string(),
        "accepted":accepted.into_iter().map(|id|id.to_string()).collect::<Vec<_>>(),"images":rows}),
        bytes,
    )
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

    #[test]
    fn layer_menus_match_shared_policy_and_reject_stale_targets() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let epoch = host.session.state().document_file.epoch;
        let id = host.session.engine().document().active_layer.0;
        let query = |epoch: u64, id: u64, mask| {
            serde_json::json!({
                "epoch":epoch.to_string(),"id":id.to_string(),"mask":mask
            })
            .to_string()
        };
        let before = host.session.engine().document().revision;
        let packet = layer_menu(&mut host, &query(epoch, id, false)).unwrap();
        assert!(packet.bytes.is_empty());
        let metadata: serde_json::Value =
            serde_json::from_str(packet.metadata.to_str().unwrap()).unwrap();
        assert_eq!(
            metadata["menu"],
            serde_json::to_value(host.session.layer_menu(id, false).unwrap()).unwrap()
        );
        assert_eq!(before, host.session.engine().document().revision);
        for json in [
            query(epoch + 1, id, false),
            query(epoch, u64::MAX, false),
            query(epoch, id, true),
        ] {
            let packet = layer_menu(&mut host, &json).unwrap();
            let metadata: serde_json::Value =
                serde_json::from_str(packet.metadata.to_str().unwrap()).unwrap();
            assert!(metadata["menu"].is_null());
        }
        host.dispatch(layer_ui::UiAction::Layer {
            action: layer_ui::LayerAction::AddMask { id, replace: false },
        })
        .unwrap();
        let packet = layer_menu(&mut host, &query(epoch, id, true)).unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(packet.metadata.to_str().unwrap()).unwrap();
        assert_eq!(
            metadata["menu"],
            serde_json::to_value(host.session.layer_menu(id, true).unwrap()).unwrap()
        );
        let current =
            serde_json::json!({"epoch":epoch.to_string(),"id":null,"mask":null}).to_string();
        let packet = layer_menu(&mut host, &current).unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(packet.metadata.to_str().unwrap()).unwrap();
        assert_eq!(metadata["id"], id.to_string());
        assert_eq!(metadata["mask"], true);
        host.dispatch(layer_ui::UiAction::Layer {
            action: layer_ui::LayerAction::New {
                group: true,
                clipped: false,
            },
        })
        .unwrap();
        let target = host.session.engine().document().active_layer.0;
        let packet = layer_menu(&mut host, &current).unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(packet.metadata.to_str().unwrap()).unwrap();
        assert_eq!(metadata["id"], target.to_string());
        assert_eq!(
            metadata["menu"],
            serde_json::to_value(host.session.layer_menu(target, false).unwrap()).unwrap()
        );
    }
    #[test]
    fn thumbnail_identity_bounds_and_epoch_are_lossless() {
        let query = || ThumbnailQuery {
            epoch: "7".into(),
            requests: vec![(u64::MAX.to_string(), (u64::MAX - 1).to_string()); 8],
        };
        assert_eq!(
            query().requests(7).unwrap(),
            vec![(u64::MAX, u64::MAX - 1); 8]
        );
        assert!(query().requests(8).unwrap().is_empty());
        let mut invalid = query();
        invalid.requests.push(("1".into(), "2".into()));
        assert!(invalid.requests(7).is_err());
        let mut invalid = query();
        invalid.requests[0].0 = "-1".into();
        assert!(invalid.requests(7).is_err());
    }
    #[test]
    fn thumbnail_packets_keep_pixels_binary_and_validate_completions() {
        let image = layer_render::ReadbackImage {
            request_id: u64::MAX,
            width: 32,
            height: 32,
            stride: 128,
            bytes: vec![17; 4096],
        };
        let packet =
            pack_layer_thumbnails(u64::MAX, vec![u64::MAX], vec![image.clone(), image.clone()])
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(packet.metadata.to_str().unwrap()).unwrap();
        assert_eq!(metadata["epoch"], u64::MAX.to_string());
        assert_eq!(metadata["accepted"][0], u64::MAX.to_string());
        assert_eq!(metadata["images"][1]["offset"], 4096);
        assert_eq!(packet.bytes, vec![17; 8192]);
        assert!(metadata.to_string().len() < 512);
        assert!(pack_layer_thumbnails(0, vec![], vec![image.clone(); 9]).is_err());
        for field in 0..4 {
            let mut invalid = image.clone();
            match field {
                0 => invalid.width = 31,
                1 => invalid.height = 33,
                2 => invalid.stride = 256,
                _ => {
                    invalid.bytes.pop();
                }
            }
            assert!(pack_layer_thumbnails(0, vec![], vec![invalid]).is_err());
        }
    }
}
