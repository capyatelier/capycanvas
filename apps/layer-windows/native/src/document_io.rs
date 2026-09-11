//! Local project transport. Only fully flushed sibling files replace a drawing.
use layer_ui::DocumentLocation;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) fn io_error(operation: &str, error: std::io::Error) -> String {
    // Paths are private host state, not part of diagnostics or project metadata.
    format!("Could not {operation} drawing ({:?}).", error.kind())
}
pub(crate) fn location(path: &str) -> Result<DocumentLocation, String> {
    let value = Path::new(path);
    let name = value
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("Choose a drawing filename")?;
    if !value.is_absolute()
        || path.len() > 16_384
        || path.contains('\0')
        || name.is_empty()
        || name.len() > 1024
        || name.chars().any(char::is_control)
        || name.contains(':')
    {
        return Err("Choose an absolute drawing path on this device".into());
    }
    Ok(DocumentLocation {
        uri: path.into(),
        name: name.into(),
    })
}
pub(crate) fn check_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Document operation cancelled".into())
    } else {
        Ok(())
    }
}
pub(crate) struct Stream<'a, T> {
    pub inner: T,
    pub cancel: &'a AtomicBool,
}
impl<T: Read> Read for Stream<'_, T> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        check_cancelled(self.cancel).map_err(std::io::Error::other)?;
        self.inner.read(bytes)
    }
}
impl<T: Write> Write for Stream<'_, T> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        check_cancelled(self.cancel).map_err(std::io::Error::other)?;
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        check_cancelled(self.cancel).map_err(std::io::Error::other)?;
        self.inner.flush()
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
pub(crate) fn atomic_write(
    path: &Path,
    cancel: &AtomicBool,
    write: impl FnOnce(&mut dyn Write) -> Result<(), String>,
) -> Result<(), String> {
    check_cancelled(cancel)?;
    let parent = fs::canonicalize(path.parent().ok_or("Choose a destination folder")?)
        .map_err(|e| io_error("locate the destination for", e))?;
    let destination = parent.join(path.file_name().ok_or("Choose a drawing filename")?);
    let mut reserved = None;
    for _ in 0..32 {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".capy-save-{}-{serial}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                reserved = Some((Temporary(temporary), file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error("create temporary", e)),
        }
    }
    let (temporary, file) = reserved.ok_or("Could not reserve a temporary drawing file")?;
    let mut stream = Stream {
        inner: BufWriter::new(file),
        cancel,
    };
    write(&mut stream)?;
    stream.flush().map_err(|e| io_error("write", e))?;
    stream
        .inner
        .get_ref()
        .sync_all()
        .map_err(|e| io_error("flush", e))?;
    // Close before replacement on Windows, including all error and unwind paths.
    drop(stream);
    check_cancelled(cancel)?;
    crate::settings::replace(&temporary.0, &destination).map_err(|e| io_error("replace saved", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn directory() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "capy-document-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        directory
    }
    #[test]
    fn atomic_failure_and_cancellation_preserve_the_previous_project() {
        let directory = directory();
        let path = directory.join("drawing.capy");
        let cancel = AtomicBool::new(false);
        let project = layer_ui::new_drawing(32, 24).unwrap();
        atomic_write(&path, &cancel, |f| project.write(f)).unwrap();
        let before = fs::read(&path).unwrap();
        let error = atomic_write(&path, &cancel, |f| {
            f.write_all(b"incomplete").unwrap();
            Err("Simulated disk full".into())
        })
        .unwrap_err();
        assert_eq!(error, "Simulated disk full");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        assert!(
            atomic_write(&path, &cancel, |f| {
                f.write_all(b"incomplete").unwrap();
                cancel.store(true, Ordering::Release);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        cancel.store(false, Ordering::Release);
        let next = layer_ui::new_drawing(40, 30).unwrap();
        atomic_write(&path, &cancel, |f| next.write(f)).unwrap();
        assert_eq!(
            layer_core::Project::read(File::open(&path).unwrap(), Default::default()).unwrap(),
            next
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn failed_replacement_and_unwind_remove_only_the_reserved_temporary() {
        let directory = directory();
        let destination = directory.join("existing-directory");
        fs::create_dir(&destination).unwrap();
        let cancel = AtomicBool::new(false);
        assert!(
            atomic_write(&destination, &cancel, |f| {
                f.write_all(b"complete").map_err(|e| e.to_string())
            })
            .is_err()
        );
        assert!(destination.is_dir());
        let result = std::panic::catch_unwind(|| {
            atomic_write(&directory.join("drawing.capy"), &cancel, |_| {
                panic!("Simulated encoder panic");
            })
        });
        assert!(result.is_err());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir(destination).unwrap();
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn location_rejects_relative_paths_and_invalid_names() {
        assert!(location("drawing.capy").is_err());
        assert!(location("").is_err());
        let directory = directory();
        assert!(location(directory.join("drawing.capy:stream").to_str().unwrap()).is_err());
        assert!(location(directory.join("bad\n.capy").to_str().unwrap()).is_err());
        let path = directory.join("試し café.capy");
        assert_eq!(
            location(path.to_str().unwrap()).unwrap().name,
            "試し café.capy"
        );
        fs::remove_dir(directory).unwrap();
    }
}
