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
pub(crate) fn list(cancel: &AtomicBool) -> Result<Vec<ProfileEntry>, String> {
    locked(cancel, |directory| {
        inventory(directory)?
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
            .collect()
    })
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
            .map_err(|e| io_error("remove profile", e).into())
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
