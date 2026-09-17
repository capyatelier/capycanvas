//! Native file transport for shared profile/preset policies. Called on file workers.
use crate::document_io::{atomic_write, check_cancelled, io_error};
use layer_core::color::ColorProfile;
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
    pub fn resolve(self, cancel: &AtomicBool) -> Result<ColorProfile, String> {
        match self {
            Self::Direct(profile) => Ok(profile),
            Self::Library { library } => profile(&library, cancel),
        }
    }
}
fn directory() -> Result<PathBuf, String> {
    Ok(crate::settings::data_directory()?.join("color"))
}
pub(crate) fn locked<T>(
    cancel: &AtomicBool,
    action: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
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
            Err(_) => return Err("Color preferences are busy; try again".into()),
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
                    Err("Unavailable profile".into())
                } else {
                    read(
                        &directory.join(format!("{}.icc", record.id)),
                        policy::PROFILE_READ_LIMIT,
                    )
                };
                Ok(policy::inspect_library_entry(
                    &record,
                    bytes.as_deref().map_err(Clone::clone),
                ))
            })
            .collect()
    })
}
pub(crate) fn profile(id: &str, cancel: &AtomicBool) -> Result<ColorProfile, String> {
    if !policy::valid_profile_id(id) {
        return Err("Select an imported profile".into());
    }
    locked(cancel, |directory| {
        let bytes = read(
            &directory.join(format!("{id}.icc")),
            policy::PROFILE_READ_LIMIT,
        )?;
        policy::read_library_profile(id, &bytes, true)?
            .profile
            .ok_or("Profile is unavailable".into())
    })
}
pub(crate) fn import(path: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let bytes = read(path, policy::PROFILE_READ_LIMIT)?;
    locked(cancel, |directory| {
        let entry = policy::prepare_profile_import(inventory(directory)?, &bytes)?;
        atomic_write(
            &directory.join(format!("{}.icc", entry.id)),
            cancel,
            |stream| {
                stream
                    .write_all(&bytes)
                    .map_err(|e| io_error("save profile", e))
            },
        )
    })
}
pub(crate) fn remove(id: &str, cancel: &AtomicBool) -> Result<(), String> {
    policy::ProfileLibraryAction::Remove { id: id.into() }.execute(&[])?;
    locked(cancel, |directory| {
        fs::remove_file(directory.join(format!("{id}.icc")))
            .map_err(|e| io_error("remove profile", e))
    })
}
pub(crate) fn presets(
    action: layer_ui::ExportPresetAction,
    document: &layer_core::Document,
    cancel: &AtomicBool,
) -> Result<layer_ui::ExportPresetView, String> {
    locked(cancel, |directory| {
        let path = directory.join("export-presets.json");
        let mut library = if path.exists() {
            layer_ui::ExportPresets::decode(&read(&path, layer_ui::ExportPresets::MAX_FILE_BYTES)?)?
        } else {
            Default::default()
        };
        let view = library.operate(action, document.color, |recipe| {
            recipe.validate_for_document(document)
        })?;
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
