use super::{ByteRange, ByteSource, ImmutableBacking, MAX_RANGE_BYTES, RangeState};
use std::{io::{self, Read, Seek, SeekFrom}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

pub struct BackingReader<'a> {
    backing: &'a ImmutableBacking,
    cancelled: &'a AtomicBool,
    position: u64,
}
impl<'a> BackingReader<'a> {
    pub fn new(backing: &'a ImmutableBacking, cancelled: &'a AtomicBool) -> Self {
        Self { backing, cancelled, position: 0 }
    }
}
impl Read for BackingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) { return Err(io::Error::other("Package operation cancelled")); }
        let length = buffer.len().min(MAX_RANGE_BYTES).min(self.backing.byte_len().saturating_sub(self.position).min(usize::MAX as u64) as usize);
        if length == 0 { return Ok(0); }
        match self.backing.poll(self.position, length).map_err(io::Error::other)? {
            RangeState::Pending => Err(io::Error::new(io::ErrorKind::WouldBlock, "Package bytes are pending")),
            RangeState::Ready(bytes) => {
                buffer[..length].copy_from_slice(&bytes);
                self.position += length as u64;
                Ok(length)
            }
        }
    }
}
impl Seek for BackingReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let value = match position {
            SeekFrom::Start(value) => Some(value),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
            SeekFrom::End(delta) => self.backing.byte_len().checked_add_signed(delta),
        }.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Package seek overflow"))?;
        if value > self.backing.byte_len() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "Package seek exceeds backing")); }
        self.position = value;
        Ok(value)
    }
}

pub struct ChunkedBytes { chunks: Vec<Arc<[u8]>>, length: u64 }
impl ChunkedBytes {
    pub fn new(chunks: Vec<Arc<[u8]>>) -> Result<Self, &'static str> {
        let mut length = 0u64;
        for (index, chunk) in chunks.iter().enumerate() {
            if chunk.is_empty() || chunk.len() > MAX_RANGE_BYTES || (index + 1 < chunks.len() && chunk.len() != MAX_RANGE_BYTES) {
                return Err("Invalid package memory chunk");
            }
            length = length.checked_add(chunk.len() as u64).ok_or("Package memory length overflow")?;
        }
        Ok(Self { chunks, length })
    }
}
impl ByteSource for ChunkedBytes {
    fn byte_len(&self) -> u64 { self.length }
    fn poll(&self, offset: u64, length: usize) -> Result<RangeState, String> {
        let end = offset.checked_add(length as u64).ok_or("Package memory range overflow")?;
        if length > MAX_RANGE_BYTES || end > self.length { return Err("Package memory range exceeds backing".into()); }
        if length == 0 { return Ok(RangeState::Ready(ByteRange::new(Arc::from([]), 0..0)?)); }
        let index = usize::try_from(offset / MAX_RANGE_BYTES as u64).map_err(|_| "Package memory offset overflow")?;
        let start = (offset % MAX_RANGE_BYTES as u64) as usize;
        let chunk = &self.chunks[index];
        if start + length <= chunk.len() { return Ok(RangeState::Ready(ByteRange::new(chunk.clone(), start..start + length)?)); }
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(&chunk[start..]);
        bytes.extend_from_slice(&self.chunks[index + 1][..length - bytes.len()]);
        Ok(RangeState::Ready(ByteRange::new(bytes.into(), 0..length)?))
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::{fs::File, io::Write, path::Path, sync::Mutex};
    struct PrivateFile { file: Mutex<File>, length: u64 }
    impl ByteSource for PrivateFile {
        fn byte_len(&self) -> u64 { self.length }
        fn resident_bytes(&self) -> usize { 0 }
        fn poll(&self, offset: u64, length: usize) -> Result<RangeState, String> {
            if length > MAX_RANGE_BYTES || offset.checked_add(length as u64).is_none_or(|end| end > self.length) {
                return Err("Package file range exceeds backing".into());
            }
            let mut file = self.file.lock().map_err(|_| "Package file lock failed")?;
            file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
            let mut bytes = vec![0; length];
            file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
            Ok(RangeState::Ready(ByteRange::new(bytes.into(), 0..length)?))
        }
    }
    pub fn spool(input: &mut impl Read, directory: &Path, limit: u64, cancelled: &AtomicBool) -> Result<ImmutableBacking, String> {
        let mut file = crate::temp_files::anonymous(directory, "package").map_err(|e| e.to_string())?;
        let mut length = 0u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 { break; }
            length = length.checked_add(count as u64).filter(|length| *length <= limit).ok_or("Package stream exceeds admission limit")?;
            file.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
        }
        ImmutableBacking::new(Arc::new(PrivateFile { file: Mutex::new(file), length })).map_err(String::from)
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub use native::spool;

#[cfg(test)]
mod tests;
