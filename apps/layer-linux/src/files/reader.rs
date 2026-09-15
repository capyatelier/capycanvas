//! File-worker cancellation shared by Open, Place and Paste. The caller keeps
//! its document request reserved until the reader acknowledges cancellation.
use std::{
    io::{Read, Seek},
    path::Path,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

pub(super) struct CancelRead {
    input: std::fs::File,
    cancelled: Arc<AtomicBool>,
}
impl CancelRead {
    pub fn new(path: &Path, cancelled: Arc<AtomicBool>) -> std::io::Result<Self> {
        let reader = Self { input: std::fs::File::open(path)?, cancelled };
        reader.check()?;
        Ok(reader)
    }
    fn check(&self) -> std::io::Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            // Interrupted would make read_exact retry indefinitely.
            Err(std::io::Error::other("Image read cancelled"))
        } else {
            Ok(())
        }
    }
}
impl Read for CancelRead {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.check()?;
        self.input.read(output)
    }
}
impl Seek for CancelRead {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        self.check()?;
        self.input.seek(position)
    }
}
