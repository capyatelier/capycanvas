//! This installation's folders. A packaged app keeps its files in its package's
//! app data; other builds use per-user folders named for their identity, so a
//! development build never opens a release's files or windows.
use layer_host::{StorageRoots, storage::STORAGE_OVERRIDE};
use std::{path::Path, sync::OnceLock};

#[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Windows folder names"))]
const FOLDER: &str = if cfg!(feature = "release-identity") { "CapyCanvas" } else { "CapyCanvas-Dev" };

struct Storage {
    roots: StorageRoots,
    isolated: bool,
    instance: Vec<u16>,
}

fn storage() -> Result<&'static Storage, String> {
    static STORAGE: OnceLock<Result<Storage, String>> = OnceLock::new();
    STORAGE
        .get_or_init(|| {
            let isolated = std::env::var_os(STORAGE_OVERRIDE).is_some();
            if cfg!(test) && !isolated {
                return Err(format!("Tests store files only in {STORAGE_OVERRIDE}"));
            }
            let roots = StorageRoots::resolve(platform)?;
            Ok(Storage { instance: instance(&roots.config), roots, isolated })
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn roots() -> Result<&'static StorageRoots, String> {
    storage().map(|storage| &storage.roots)
}

#[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Used by the Windows host"))]
pub(crate) fn isolated() -> bool {
    storage().is_ok_and(|storage| storage.isolated)
}

/// NUL-terminated name shared by every launch that uses these folders.
#[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Used by the Windows entry point"))]
pub(crate) fn instance_name() -> Result<&'static [u16], String> {
    storage().map(|storage| storage.instance.as_slice())
}

fn instance(config: &Path) -> Vec<u16> {
    let hash = config.to_string_lossy().to_lowercase().encode_utf16().fold(0xcbf2_9ce4_8422_2325_u64, |hash, unit| {
        (hash ^ u64::from(unit)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("CapyCanvas-{hash:016x}").encode_utf16().chain([0]).collect()
}

#[cfg(target_os = "windows")]
fn platform() -> Result<StorageRoots, String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
    use windows::{
        Storage::{ApplicationData, StorageFolder, UserDataPaths},
        Win32::{Foundation::APPMODEL_ERROR_NO_PACKAGE, Storage::FileSystem::GetTempPath2W},
        core::HSTRING,
    };
    let unavailable = |error: windows::core::Error| format!("App storage is unavailable ({error})");
    let path = |path: HSTRING| PathBuf::from(path.to_os_string());
    match ApplicationData::Current() {
        Ok(package) => {
            let folder = |folder: windows::core::Result<StorageFolder>| folder.and_then(|f| f.Path()).map(path).map_err(unavailable);
            let local = folder(package.LocalFolder())?;
            Ok(StorageRoots {
                config: local.clone(),
                data: local.clone(),
                state: local,
                cache: folder(package.LocalCacheFolder())?,
                temp: folder(package.TemporaryFolder())?,
            })
        }
        Err(error) if error.code() == APPMODEL_ERROR_NO_PACKAGE.to_hresult() => {
            let user = UserDataPaths::GetDefault().map_err(unavailable)?;
            let named = |folder: PathBuf| folder.join("CapyAtelier").join(FOLDER);
            let local = named(path(user.LocalAppData().map_err(unavailable)?));
            let mut temp = [0; 261];
            let length = unsafe { GetTempPath2W(Some(&mut temp)) } as usize;
            if length == 0 || length >= temp.len() {
                return Err("App storage is unavailable (no temporary folder)".into());
            }
            Ok(StorageRoots {
                config: named(path(user.RoamingAppData().map_err(unavailable)?)),
                data: local.clone(),
                state: local.clone(),
                cache: local.join("Cache"),
                temp: PathBuf::from(OsString::from_wide(&temp[..length])).join(FOLDER),
            })
        }
        Err(error) => Err(unavailable(error)),
    }
}

#[cfg(not(target_os = "windows"))]
fn platform() -> Result<StorageRoots, String> {
    Err("App storage is only available on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_installation_has_its_own_instance_name() {
        let release = instance(Path::new(r"C:\Users\Painter\AppData\Roaming\CapyAtelier\CapyCanvas"));
        let development = instance(Path::new(r"C:\Users\Painter\AppData\Roaming\CapyAtelier\CapyCanvas-Dev"));
        let isolated = instance(&StorageRoots::within(Path::new(r"C:\fixtures\run\profile")).config);
        assert_ne!(release, development);
        assert_ne!(release, isolated);
        assert_eq!(release, instance(Path::new(r"c:\users\painter\appdata\roaming\capyatelier\capycanvas")));
        assert_eq!(release.last(), Some(&0));
        assert_eq!(release.iter().filter(|unit| **unit == 0).count(), 1);
        assert_eq!(String::from_utf16(&release[..release.len() - 1]).unwrap().len(), "CapyCanvas-".len() + 16);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn an_unpackaged_build_uses_known_folders_named_for_its_identity() {
        let roots = platform().unwrap();
        let named = Path::new("CapyAtelier").join(FOLDER);
        assert!(roots.config.ends_with(&named) && roots.data.ends_with(&named));
        assert_ne!(roots.config, roots.data);
        assert_eq!(roots.state, roots.data);
        assert_eq!(roots.cache, roots.data.join("Cache"));
        assert!(roots.temp.ends_with(FOLDER) && !roots.temp.starts_with(&roots.data));
        assert!([&roots.config, &roots.data, &roots.temp].iter().all(|path| path.is_absolute()));
    }
}
