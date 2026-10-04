//! The installation's private folder for transient files. Hosts set it once at
//! startup to disk-backed storage that the platform may clear between launches.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

pub fn set_directory(directory: PathBuf) -> Result<(), String> {
    if !directory.is_absolute() {
        return Err("The temporary folder must be an absolute path".into());
    }
    if *DIRECTORY.get_or_init(|| directory.clone()) == directory {
        Ok(())
    } else {
        Err("The temporary folder is already set".into())
    }
}

pub fn directory() -> Result<&'static Path, String> {
    DIRECTORY.get().map(PathBuf::as_path).ok_or_else(|| "Temporary storage is unavailable".into())
}

/// A new private file in `directory` that has no name once open, so it
/// disappears with its last handle, including when the process stops.
#[cfg(any(unix, windows))]
pub fn anonymous(directory: &Path, prefix: &str) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut folder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    folder.mode(0o700);
    folder.recursive(true).create(directory)?;
    let path = directory.join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    #[cfg(windows)]
    options.custom_flags(0x0400_0100).share_mode(0x7); // DELETE_ON_CLOSE | TEMPORARY; share read/write/delete
    let file = options.open(&path)?;
    #[cfg(unix)]
    std::fs::remove_file(&path)?;
    Ok(file)
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};

    #[test]
    fn anonymous_files_leave_nothing_behind() {
        let directory = std::env::temp_dir().join(format!("capy-anonymous-{}", crate::PortableId::random()));
        let mut file = anonymous(&directory, "test").unwrap();
        file.write_all(b"kept while open").unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        assert_eq!(text, "kept while open");
        drop(file);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn the_folder_is_absolute_and_set_once() {
        assert!(set_directory(PathBuf::from("relative")).is_err());
        let first = std::env::temp_dir().join("capy-temp-files-test");
        set_directory(first.clone()).unwrap();
        set_directory(first.clone()).unwrap();
        assert!(set_directory(std::env::temp_dir().join("capy-other")).is_err());
        assert_eq!(directory().unwrap(), first);
    }
}
