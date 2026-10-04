//! XDG folders for this installation. Development builds use their own app ID
//! and folders, so they never open an installed release's files.
use gtk::glib;
use layer_host::StorageRoots;

pub const APP_ID: &str = if cfg!(feature = "release-identity") {
    "art.capycanvas.CapyCanvas"
} else {
    "art.capycanvas.CapyCanvas.Devel"
};
const FOLDER: &str = if cfg!(feature = "release-identity") { "capycanvas" } else { "capycanvas-devel" };

/// Test builds store nothing unless `CAPY_STORAGE_DIR` names a private folder;
/// without one they keep only anonymous temporary files in the system folder.
pub fn roots() -> Option<&'static StorageRoots> {
    static ROOTS: std::sync::OnceLock<Option<StorageRoots>> = std::sync::OnceLock::new();
    ROOTS
        .get_or_init(|| {
            if cfg!(test) && std::env::var_os(layer_host::storage::STORAGE_OVERRIDE).is_none() {
                let _ = layer_core::temp_files::set_directory(std::env::temp_dir());
                return None;
            }
            StorageRoots::resolve(|| {
                let cache = glib::user_cache_dir().join(FOLDER);
                Ok(StorageRoots {
                    config: glib::user_config_dir().join(FOLDER),
                    data: glib::user_data_dir().join(FOLDER),
                    state: glib::user_state_dir().join(FOLDER),
                    temp: cache.join("temp"),
                    cache,
                })
            })
            .inspect_err(|error| eprintln!("{error}; nothing will be saved"))
            .ok()
        })
        .as_ref()
}

/// Test builds use workspaces and sessions only when the test created their folders.
fn active(path: std::path::PathBuf) -> Option<std::path::PathBuf> {
    (!cfg!(test) || path.exists()).then_some(path)
}
pub fn workspaces() -> Option<&'static std::path::Path> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| roots().map(StorageRoots::workspaces).and_then(active)).as_deref()
}
pub fn sessions() -> Option<&'static std::path::Path> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| roots().map(StorageRoots::sessions).and_then(active)).as_deref()
}

/// The primary instance owns the temporary folder, so files a stopped process
/// left there are removed when the next one starts.
pub fn clear_temporary_files() {
    if let Err(error) = roots().map_or(Ok(()), StorageRoots::clear_temp) {
        eprintln!("{error}");
    }
}
