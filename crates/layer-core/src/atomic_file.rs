//! Native Unix publication shared by desktop and Android file workers.
use std::{
    io::{BufWriter, Write},
    path::Path,
};

/// Local atomic streaming write: the original survives validation, encoding or
/// disk errors. Only a fully flushed sibling temporary file replaces it.
pub fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> Result<(), String>,
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
    std::fs::rename(&cleanup.0, path).map_err(|e| format!("Cannot replace drawing: {e}"))?;
    std::fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("Cannot finish saving: {e}"))?;
    Ok(())
}
