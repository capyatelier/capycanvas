//! Job cancellation: `cancel` sets a job's flag and wakes the futures waiting
//! on it, and worker file streams fail every operation once it is set.
use std::{io::{self, Read, Seek, SeekFrom, Write}, sync::{Arc, Mutex, Weak, atomic::{AtomicBool, Ordering}}};

static WAITING: Mutex<Vec<(usize, Weak<dyn Fn() + Send + Sync>)>> = Mutex::new(Vec::new());
fn address(flag: &AtomicBool) -> usize { std::ptr::from_ref(flag).addr() }
pub fn cancel(flag: &AtomicBool) {
    flag.store(true, Ordering::Release);
    let wakes: Vec<_> = WAITING.lock().unwrap().iter().filter(|(waiting, _)| *waiting == address(flag)).filter_map(|(_, wake)| wake.upgrade()).collect();
    for wake in wakes { wake(); }
}
pub(crate) fn wake_on_cancel(flag: &AtomicBool, wake: &Arc<dyn Fn() + Send + Sync>) {
    let mut waiting = WAITING.lock().unwrap();
    waiting.retain(|(_, wake)| wake.strong_count() != 0);
    waiting.push((address(flag), Arc::downgrade(wake)));
}

pub struct Cancellable<T, F: Fn() -> bool> {
    pub inner: T,
    pub cancelled: F,
}
impl<T, F: Fn() -> bool> Cancellable<T, F> {
    fn check(&self) -> io::Result<()> {
        if (self.cancelled)() {
            Err(io::Error::other("Operation cancelled"))
        } else {
            Ok(())
        }
    }
}
impl<T: Read, F: Fn() -> bool> Read for Cancellable<T, F> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        self.inner.read(bytes)
    }
}
impl<T: Seek, F: Fn() -> bool> Seek for Cancellable<T, F> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check()?;
        self.inner.seek(position)
    }
}
impl<T: Write, F: Fn() -> bool> Write for Cancellable<T, F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.check()?;
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.check()?;
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, io::Cursor};

    #[test]
    fn cancellable_stops_reads_writes_and_seeks() {
        let cancelled = Cell::new(false);
        let mut stream = Cancellable {
            inner: Cursor::new(vec![1, 2, 3]),
            cancelled: || cancelled.get(),
        };
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        stream.write_all(&[9]).unwrap();
        cancelled.set(true);
        assert!(stream.read(&mut byte).is_err());
        assert!(stream.write(&[7]).is_err() && stream.flush().is_err());
        assert!(stream.seek(SeekFrom::Start(0)).is_err());
        assert_eq!(stream.inner.into_inner(), [1, 9, 3]);
    }
}
