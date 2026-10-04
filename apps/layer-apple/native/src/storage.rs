//! Where each store lives. Swift supplies Apple's folders; shared Rust applies
//! `CAPY_STORAGE_DIR` and names every store within them.
use super::*;
use layer_host::StorageRoots;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn text(path: PathBuf) -> Result<String, String> {
    path.into_os_string().into_string().map_err(|_| "App storage folders must be UTF-8 paths".into())
}
fn locations(roots: StorageRoots) -> Result<Value, String> {
    Ok(json!({
        "settings": text(roots.settings())?,
        "export_presets": text(roots.export_presets())?,
        "color_profiles": text(roots.color_profiles())?,
        "workspaces": text(roots.workspaces())?,
        "sessions": text(roots.sessions())?,
        "shaders": text(roots.shaders())?,
        "state": text(roots.state)?,
    }))
}
fn resolve(request: &Value) -> Result<Value, String> {
    if let Some(directory) = request["directory"].as_str() {
        return locations(StorageRoots::within(Path::new(directory)));
    }
    let folder = |kind: &str| {
        request["platform"][kind].as_str().map(PathBuf::from).ok_or("App storage folders are unavailable")
    };
    locations(StorageRoots::resolve(|| {
        Ok(StorageRoots { config: folder("config")?, data: folder("data")?, state: folder("state")?,
            cache: folder("cache")?, temp: folder("temp")? })
    })?)
}

/// `{"directory": path}` names the stores within one private folder.
/// `{"platform": {config, data, state, cache, temp}}` resolves the installation
/// and sets the process temporary folder; call it once per process.
///
/// # Safety
/// Request is NUL-terminated UTF-8 JSON. Free the result with capy_apple_string_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_storage(request: *const c_char) -> *mut c_char {
    let result = unsafe { project::read_title(request) }
        .and_then(|request| serde_json::from_str(request).map_err(|e| e.to_string()))
        .and_then(|request| resolve(&request))
        .unwrap_or_else(|error| json!({ "error": error }));
    CString::new(result.to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}
