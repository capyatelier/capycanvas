//! Application-owned ICC files. Imported bytes are exact and content-addressed;
//! removing a library entry cannot remove source files or embedded project data.
use super::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

use layer_ui::profile_library as policy;
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) channels: Option<ProfileChannels>,
    pub(super) issue: Option<String>,
    pub(super) visible: bool,
}
impl Entry {
    fn description(&self) -> String {
        if let Some(issue) = &self.issue {
            return issue.clone();
        }
        let channels = match self.channels.unwrap() {
            ProfileChannels::Rgb => "RGB",
            ProfileChannels::Gray => "Grayscale",
            ProfileChannels::Cmyk => "CMYK",
        };
        if self.visible {
            channels.into()
        } else {
            format!("{channels} · Hidden")
        }
    }
}
pub(super) fn directory() -> PathBuf {
    if let Some(path) = std::env::var_os("LAYER_SETTINGS_FILE")
        .map(PathBuf::from)
        .and_then(|p| p.parent().map(|p| p.join("color-profiles")))
    {
        return path;
    }
    if cfg!(test) {
        return std::env::temp_dir().join(format!("capy-color-profiles-{}", std::process::id()));
    }
    glib::user_data_dir().join("capycanvas/color-profiles")
}
// Display metadata only: the ICC bytes and digest remain authoritative.
fn saved_name(path: &Path, description: String) -> String {
    if description != UNNAMED_PROFILE {
        return description;
    }
    let mut name = String::new();
    if std::fs::File::open(path.with_extension("name"))
        .and_then(|file| file.take(1024).read_to_string(&mut name))
        .is_ok()
    {
        let name: String = name.chars().filter(|c| !c.is_control()).take(128).collect();
        if !name.trim().is_empty() {
            return name.trim().to_owned();
        }
    }
    description
}
fn read_profile(path: &Path) -> Result<(Vec<u8>, String, ProfileChannels), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Choose a profile file".into());
    }
    let mut bytes = Vec::new();
    file.take(layer_color::MAX_ICC_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > layer_color::MAX_ICC_BYTES {
        return Err("ICC profile exceeds the size limit".into());
    }
    let profile = ColorProfile::Icc(bytes.clone().into());
    let channels = layer_color::profile_channels(&profile)?;
    let name = layer_color::profile_description(&profile)?;
    Ok((bytes, name, channels))
}
fn inventory(directory: &Path) -> Result<Vec<policy::ProfileRecord>, String> {
    let reader = match std::fs::read_dir(directory) {
        Ok(reader) => reader,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut records = Vec::new();
    for entry in reader {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() || path.extension().is_none_or(|v| v != "icc") { continue; }
        records.push(policy::ProfileRecord {
            id: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
            bytes: entry.metadata().map_err(|e| e.to_string())?.len(), issue: None,
        });
    }
    Ok(policy::profile_inventory(records))
}
pub(super) fn list(directory: &Path) -> Result<Vec<Entry>, String> {
    let mut result = Vec::new();
    for record in inventory(directory)? {
        let path = directory.join(format!("{}.icc", record.id));
        let bytes = if record.issue.is_some() { Ok(vec![]) } else { read_profile(&path).map(|(bytes, _, _)| bytes) };
        let entry = policy::inspect_library_entry(&record, bytes.as_deref().map_err(Clone::clone));
        result.push(Entry { visible: !path.with_extension("hidden").exists(), name: saved_name(&path, entry.name), path, channels: entry.channels, issue: entry.issue });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    Ok(result)
}
fn import(directory: &Path, source: &Path) -> Result<Vec<Entry>, String> {
    let (bytes, mut name, _) = read_profile(source)?;
    if name == UNNAMED_PROFILE {
        name = source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    store(directory, &bytes, &name)
}

// Store exactly the bytes the picker validated, without reopening the source.
pub(super) fn store(directory: &Path, bytes: &[u8], name: &str) -> Result<Vec<Entry>, String> {
    let _lock = STORE_LOCK.lock().map_err(|e| e.to_string())?;
    let entry = policy::prepare_profile_import(inventory(directory)?, bytes)?;
    let target = directory.join(format!("{}.icc", entry.id));
    let entries = list(directory)?;
    if entries.iter().any(|e| e.path == target && e.issue.is_none()) { return Ok(entries); }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let name: String = name.chars().filter(|c| !c.is_control()).take(128).collect();
    layer_core::atomic_write(&target.with_extension("name"), |file| {
        file.write_all(name.as_bytes()).map_err(|e| e.to_string())
    })?;
    layer_core::atomic_write(&target, |file| file.write_all(bytes).map_err(|e| e.to_string()))?;
    list(directory)
}
fn remove(directory: &Path, path: &Path) -> Result<Vec<Entry>, String> {
    let _lock = STORE_LOCK.lock().map_err(|e| e.to_string())?;
    let id = path.file_stem().and_then(|s| s.to_str()).ok_or("Select an imported profile")?;
    policy::ProfileLibraryAction::Remove { id: id.into() }.execute(&[])?;
    if path != directory.join(format!("{id}.icc")) { return Err("Select an imported profile".into()); }
    for extension in ["name", "hidden"] {
        match std::fs::remove_file(path.with_extension(extension)) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    std::fs::remove_file(path).map_err(|e| e.to_string())?;
    list(directory)
}

fn set_visible(directory: &Path, path: &Path, visible: bool) -> Result<Vec<Entry>, String> {
    let _lock = STORE_LOCK.lock().map_err(|e| e.to_string())?;
    if !list(directory)?.iter().any(|e| e.path == path) {
        return Err("This profile is no longer saved".into());
    }
    let marker = path.with_extension("hidden");
    if visible {
        match std::fs::remove_file(marker) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    } else {
        layer_core::atomic_write(&marker, |_| Ok(()))?;
    }
    list(directory)
}
pub(super) fn read_entry(
    path: &Path,
    working: RgbSpace,
    purpose: &ProfilePurpose,
) -> Result<ExportProfile, String> {
    let mut profile = super::read(path, working, purpose)?;
    let ColorProfile::Icc(bytes) = &profile.profile else {
        unreachable!()
    };
    let id = path.file_stem().and_then(|s| s.to_str()).ok_or("Select an imported profile")?;
    policy::read_library_profile(id, bytes, false)?;
    profile.name = saved_name(path, layer_color::profile_description(&profile.profile)?);
    Ok(profile)
}
mod manager;
pub(crate) use manager::{manage, manage_for_window};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unnamed_profiles_keep_the_filename_when_reused() {
        let root =
            std::env::temp_dir().join(format!("capy-unnamed-profile-{}", std::process::id()));
        let directory = root.join("library");
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("My printer and paper.icm");
        let mut bytes =
            layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::AdobeRgb)).unwrap();
        let count = u32::from_be_bytes(bytes[128..132].try_into().unwrap()) as usize;
        for tag in bytes[132..132 + count * 12].chunks_exact_mut(12) {
            if &tag[..4] == b"desc" {
                tag[..4].copy_from_slice(b"zzzz");
            }
        }
        assert_eq!(
            layer_color::profile_description(&ColorProfile::Icc(bytes.clone().into())).unwrap(),
            UNNAMED_PROFILE
        );
        std::fs::write(&original, &bytes).unwrap();
        let chosen = read(&original, RgbSpace::Srgb, &ProfilePurpose::Output).unwrap();
        assert_eq!(chosen.name, "My printer and paper.icm");
        let entries = import(&directory, &original).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, chosen.name);
        let saved = read_entry(&entries[0].path, RgbSpace::Srgb, &ProfilePurpose::Output).unwrap();
        assert_eq!(saved.name, chosen.name);
        assert_eq!(saved.profile, ColorProfile::Icc(bytes.clone().into()));
        assert_eq!(std::fs::read(&entries[0].path).unwrap(), bytes);
        let duplicate = root.join("Renamed.icc");
        std::fs::write(&duplicate, &bytes).unwrap();
        assert_eq!(import(&directory, &duplicate).unwrap()[0].name, chosen.name);
        assert!(remove(&directory, &entries[0].path).unwrap().is_empty());
        assert!(!entries[0].path.with_extension("name").exists());
        assert_eq!(std::fs::read(&original).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn library_preserves_bytes_deduplicates_and_never_removes_the_original() {
        let root =
            std::env::temp_dir().join(format!("capy-profile-library-{}", std::process::id()));
        let store = root.join("library");
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("original.icc");
        let bytes =
            layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
        std::fs::write(&original, &bytes).unwrap();
        let entries = import(&store, &original).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].channels, Some(ProfileChannels::Rgb));
        assert!(entries[0].visible);
        assert!(!set_visible(&store, &entries[0].path, false).unwrap()[0].visible);
        assert!(!list(&store).unwrap()[0].visible);
        assert_eq!(
            read_entry(&entries[0].path, RgbSpace::Srgb, &ProfilePurpose::Output)
                .unwrap()
                .profile,
            ColorProfile::Icc(bytes.clone().into())
        );
        assert!(set_visible(&store, &entries[0].path, true).unwrap()[0].visible);
        assert!(set_visible(&store, &original, false).is_err());
        assert_eq!(std::fs::read(&entries[0].path).unwrap(), bytes);
        assert_eq!(import(&store, &original).unwrap().len(), 1);
        std::fs::write(&entries[0].path, b"damaged").unwrap();
        assert!(list(&store).unwrap()[0].issue.is_some());
        assert!(read_entry(&entries[0].path, RgbSpace::Srgb, &ProfilePurpose::Output).is_err());
        let repaired = import(&store, &original).unwrap();
        assert!(repaired[0].issue.is_none());
        assert_eq!(std::fs::read(&repaired[0].path).unwrap(), bytes);
        assert!(remove(&store, &original).is_err());
        let embedded = read(
            &entries[0].path,
            RgbSpace::ProPhoto,
            &ProfilePurpose::Output,
        )
        .unwrap();
        assert!(remove(&store, &entries[0].path).unwrap().is_empty());
        assert_eq!(std::fs::read(&original).unwrap(), bytes);
        assert_eq!(embedded.profile, ColorProfile::Icc(bytes.clone().into()));
        std::fs::write(&original, b"invalid profile").unwrap();
        assert!(import(&store, &original).is_err());
        assert!(list(&store).unwrap().is_empty());
        std::fs::File::create(&original)
            .unwrap()
            .set_len(layer_color::MAX_ICC_BYTES as u64 + 1)
            .unwrap();
        assert!(
            import(&store, &original)
                .unwrap_err()
                .contains("size limit")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
