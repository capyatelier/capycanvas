//! Worker file streams that fail every operation once their job is cancelled.
use std::io::{self, Read, Seek, SeekFrom, Write};

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
