//! Native Unix publication shared by desktop and Android file workers.
use std::{
    io::{BufWriter, Write},
    path::Path,
};

/// Local atomic streaming write: the original survives validation, encoding or
/// disk errors. Only a fully flushed sibling temporary file replaces it.
pub fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<std::fs::File>) -> Result<(), String>,
) -> Result<(), String> {
    atomic_write_checked(path, write, || Ok(()))
}

/// Seekable codecs share the save writer. `ready` runs after the completed
/// temporary file is durable, immediately before publication; a cancelled job
/// rejects publication here without changing the existing destination.
pub fn atomic_write_checked(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<std::fs::File>) -> Result<(), String>,
    ready: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let parent = path.parent().ok_or("Choose a destination folder")?;
    let temporary = loop {
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let candidate = parent.join(format!(".capy-save-{}-{serial}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot save drawing: {e}")),
        }
    };
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let cleanup = Cleanup(temporary.0);
    let mut file = BufWriter::new(temporary.1);
    write(&mut file)?;
    file.flush()
        .and_then(|()| file.get_ref().sync_all())
        .map_err(|e| format!("Cannot finish saving: {e}"))?;
    ready()?;
    std::fs::rename(&cleanup.0, path).map_err(|e| format!("Cannot replace drawing: {e}"))?;
    std::fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("Cannot finish saving: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom};

    #[test]
    fn seekable_output_checks_cancellation_before_publication() {
        let dir = std::env::temp_dir().join(format!("capy-atomic-export-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("image.tif");
        std::fs::write(&path, b"original").unwrap();
        let write = |file: &mut BufWriter<std::fs::File>| {
            file.write_all(b"head payload").unwrap();
            file.seek(SeekFrom::Start(0)).unwrap();
            file.write_all(b"TIFF").unwrap();
            Ok(())
        };
        let error = atomic_write_checked(&path, write, || Err("Cancelled".into())).unwrap_err();
        assert_eq!(error, "Cancelled");
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        atomic_write_checked(&path, write, || Ok(())).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"TIFF payload");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
