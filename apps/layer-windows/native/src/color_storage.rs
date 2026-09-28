//! Native file transport for shared profile/preset policies. Called on file workers.
use crate::document_io::{atomic_write, check_cancelled, io_error};
use layer_core::color::ColorProfile;
use layer_ui::ColorFeatureError;
use layer_ui::profile_library::{self as policy, ProfileEntry, ProfileRecord};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ProfileChoice {
    Library { library: String },
    Direct(ColorProfile),
}
impl ProfileChoice {
    pub fn resolve(self, cancel: &AtomicBool, localization: &layer_ui::Localizer) -> Result<ColorProfile, ColorFeatureError> {
        match self {
            Self::Direct(profile) => Ok(profile),
            Self::Library { library } => profile(&library, cancel, localization),
        }
    }
}
fn directory() -> Result<PathBuf, String> {
    Ok(crate::settings::data_directory()?.join("color"))
}
pub(crate) fn locked<T, E: From<String>>(
    cancel: &AtomicBool,
    action: impl FnOnce(&Path) -> Result<T, E>,
) -> Result<T, E> {
    let directory = directory()?;
    fs::create_dir_all(&directory).map_err(|e| io_error("prepare color preferences", e))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("storage.lock"))
        .map_err(|e| io_error("open color storage lock", e))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        check_cancelled(cancel)?;
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(_) => return Err("Color preferences are busy; try again".to_owned().into()),
        }
    }
    action(&directory)
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| io_error("read color preferences", e))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("read color preferences", e))?;
    Ok(bytes)
}
fn inventory(directory: &Path) -> Result<Vec<ProfileRecord>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| io_error("list profiles", e))? {
        let entry = entry.map_err(|e| io_error("list profiles", e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(id) = name.strip_suffix(".icc") else {
            continue;
        };
        if !policy::valid_profile_id(id) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|e| io_error("inspect profile", e))?;
        if metadata.is_file() {
            entries.push(ProfileRecord {
                id: id.into(),
                bytes: metadata.len(),
                issue: None,
            });
        }
    }
    Ok(policy::profile_inventory(entries))
}
fn hidden(directory: &Path) -> Result<Vec<String>, String> {
    let saved = match fs::read(directory.join("menus.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::Value::Null,
        Err(error) => return Err(io_error("read profile menus", error)),
    };
    let action: policy::ProfileLibraryAction = serde_json::from_value(
        serde_json::json!({"type": "visibility", "hidden": saved, "id": null, "visible": null}),
    )
    .map_err(|e| e.to_string())?;
    serde_json::from_value(action.execute(&[]).map_err(|reason| reason.diagnostic())?).map_err(|e| e.to_string())
}
fn store_hidden(directory: &Path, hidden: Vec<String>, id: &str, visible: bool, cancel: &AtomicBool) -> Result<(), ColorFeatureError> {
    let next: Vec<String> = serde_json::from_value(
        policy::ProfileLibraryAction::Visibility { hidden, id: Some(id.into()), visible: Some(visible) }.execute(&[])?,
    )
    .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&next).map_err(|e| e.to_string())?;
    atomic_write(&directory.join("menus.json"), cancel, |stream| {
        stream
            .write_all(&bytes)
            .map_err(|e| io_error("save profile menus", e))
    })
    .map_err(Into::into)
}
pub(crate) fn show(id: &str, visible: bool, cancel: &AtomicBool) -> Result<(), ColorFeatureError> {
    locked(cancel, |directory| store_hidden(directory, hidden(directory)?, id, visible, cancel))
}
pub(crate) fn library(cancel: &AtomicBool) -> Result<(Vec<ProfileEntry>, Vec<String>), String> {
    locked(cancel, |directory| {
        let hidden = hidden(directory)?;
        let entries = inventory(directory)?
            .into_iter()
            .map(|record| {
                check_cancelled(cancel)?;
                let bytes = if record.issue.is_some() {
                    Err(layer_ui::ColorFeatureError::ProfileMissing)
                } else {
                    read(
                        &directory.join(format!("{}.icc", record.id)),
                        policy::PROFILE_READ_LIMIT,
                    ).map_err(layer_ui::ColorFeatureError::Diagnostic)
                };
                Ok(policy::inspect_library_entry(
                    &record,
                    bytes.as_deref().map_err(Clone::clone),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok((entries, hidden))
    })
}
pub(crate) fn list(cancel: &AtomicBool) -> Result<Vec<ProfileEntry>, String> {
    let (entries, hidden) = library(cancel)?;
    Ok(entries.into_iter().filter(|entry| !hidden.contains(&entry.id)).collect())
}
pub(crate) fn profile(id: &str, cancel: &AtomicBool, _localization: &layer_ui::Localizer) -> Result<ColorProfile, ColorFeatureError> {
    if !policy::valid_profile_id(id) {
        return Err(ColorFeatureError::SelectImportedProfile);
    }
    locked(cancel, |directory| {
        let bytes = read(
            &directory.join(format!("{id}.icc")),
            policy::PROFILE_READ_LIMIT,
        )?;
        policy::read_library_profile(id, &bytes, true)?
            .profile
            .ok_or(ColorFeatureError::ProfileMissing)
    })
}
pub(crate) fn export_profile(id: &str, cancel: &AtomicBool, localization: &layer_ui::Localizer) -> Result<layer_ui::ExportProfile, ColorFeatureError> {
    let profile = profile(id, cancel, localization)?;
    Ok(layer_ui::ExportProfile {
        channels: layer_color::profile_channels(&profile)?,
        name: layer_ui::profile_library::profile_display_name(&profile, localization)?,
        profile,
    })
}
pub(crate) fn import(path: &Path, cancel: &AtomicBool, localization: &layer_ui::Localizer) -> Result<(), ColorFeatureError> {
    let bytes = read(path, policy::PROFILE_READ_LIMIT)?;
    preserve(&bytes, cancel, localization)
}
pub(crate) fn preserve(bytes: &[u8], cancel: &AtomicBool, _localization: &layer_ui::Localizer) -> Result<(), ColorFeatureError> {
    locked(cancel, |directory| {
        let entry = policy::prepare_profile_import(inventory(directory)?, bytes)?;
        atomic_write(
            &directory.join(format!("{}.icc", entry.id)),
            cancel,
            |stream| {
                stream
                    .write_all(bytes)
                    .map_err(|e| io_error("save profile", e))
            },
        ).map_err(Into::into)
    })
}
pub(crate) fn remove(id: &str, cancel: &AtomicBool, _localization: &layer_ui::Localizer) -> Result<(), ColorFeatureError> {
    policy::ProfileLibraryAction::Remove { id: id.into() }.execute(&[])?;
    locked(cancel, |directory| {
        fs::remove_file(directory.join(format!("{id}.icc")))
            .map_err(|e| io_error("remove profile", e))?;
        store_hidden(directory, hidden(directory)?, id, true, cancel)
    })
}
pub(crate) fn presets(
    action: layer_ui::ExportPresetAction,
    document: &layer_core::Document,
    cancel: &AtomicBool,
    localization: &layer_ui::Localizer,
) -> Result<layer_ui::ExportPresetView, ColorFeatureError> {
    locked(cancel, |directory| {
        let path = directory.join("export-presets.json");
        let mut library = if path.exists() {
            layer_ui::ExportPresets::restore(&read(&path, layer_ui::ExportPresets::MAX_FILE_BYTES)?)
        } else {
            Default::default()
        };
        let mut view = library.operate(action, document.color)?;
        view.localize_names(document.color, localization);
        if view.changed {
            let bytes = library.encode()?;
            atomic_write(&path, cancel, |file| {
                file.write_all(&bytes)
                    .map_err(|e| io_error("save export presets", e))
            })?;
        }
        Ok(view)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires an isolated CAPY_SETTINGS_DIRECTORY"]
    fn hidden_profiles_leave_menus_until_shown_or_removed() {
        assert!(std::env::var_os("CAPY_SETTINGS_DIRECTORY").is_some(), "use isolated storage");
        let cancel = AtomicBool::new(false);
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        let bytes = layer_color::profile_bytes(&ColorProfile::Builtin(layer_core::color::RgbSpace::DisplayP3)).unwrap();
        let id = policy::profile_identity(&bytes);
        let listed = |cancel: &AtomicBool| list(cancel).unwrap().iter().any(|p| p.id == id);
        preserve(&bytes, &cancel, &localization).unwrap();
        let menus = directory().unwrap().join("menus.json");
        std::fs::write(&menus, serde_json::json!([id, 7, {"id": id}]).to_string()).unwrap();
        assert!(!listed(&cancel));
        std::fs::write(&menus, "not json").unwrap();
        assert!(listed(&cancel));
        show(&id, false, &cancel).unwrap();
        assert!(!listed(&cancel));
        let (entries, hidden) = library(&cancel).unwrap();
        assert!(entries.iter().any(|p| p.id == id) && hidden.contains(&id));
        show(&id, true, &cancel).unwrap();
        assert!(listed(&cancel));
        show(&id, false, &cancel).unwrap();
        remove(&id, &cancel, &localization).unwrap();
        preserve(&bytes, &cancel, &localization).unwrap();
        assert!(listed(&cancel));
        remove(&id, &cancel, &localization).unwrap();
    }
}