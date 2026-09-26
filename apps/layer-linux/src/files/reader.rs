//! File-worker cancellation shared by Open, Place and Paste. The caller keeps
//! its document request reserved until the reader acknowledges cancellation.
use layer_core::Cancellable;
use std::{
    fs::File,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) fn cancellable_file(
    path: &Path,
    cancelled: Arc<AtomicBool>,
) -> std::io::Result<Cancellable<File, impl Fn() -> bool>> {
    let inner = File::open(path)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(std::io::Error::other("Operation cancelled"));
    }
    Ok(Cancellable {
        inner,
        cancelled: move || cancelled.load(Ordering::Acquire),
    })
}
