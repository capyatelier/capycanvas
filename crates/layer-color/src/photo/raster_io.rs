//! Shared bounded input and row publication for full-frame raster codecs.
use super::*;
use std::io::{self, SeekFrom};

/// Present the remaining image as an origin-zero file, including when the
/// caller supplied an image embedded in a larger stream. Seeks cannot escape it.
pub(super) struct Input<R> {
    reader: R,
    start: u64,
    pub length: u64,
    position: u64,
}
impl<R: Read + Seek> Input<R> {
    pub fn new(mut reader: R, limits: DecodeLimits) -> Result<Self, String> {
        let start = reader.stream_position().map_err(err)?;
        let end = reader.seek(SeekFrom::End(0)).map_err(err)?;
        let length = end.checked_sub(start).ok_or("Invalid image stream")?;
        if length > limits.codec_bytes as u64 {
            return Err("The encoded image exceeds the codec budget".into());
        }
        reader.seek(SeekFrom::Start(start)).map_err(err)?;
        Ok(Self {
            reader,
            start,
            length,
            position: 0,
        })
    }
}
impl<R: Read> Read for Input<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = buffer.len().min((self.length - self.position) as usize);
        let read = self.reader.read(&mut buffer[..count])?;
        self.position += read as u64;
        Ok(read)
    }
}
impl<R: BufRead> BufRead for Input<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        let buffer = self.reader.fill_buf()?;
        let count = buffer.len().min((self.length - self.position) as usize);
        Ok(&buffer[..count])
    }
    fn consume(&mut self, amount: usize) {
        let amount = amount.min((self.length - self.position) as usize);
        self.reader.consume(amount);
        self.position += amount as u64;
    }
}
impl<R: Seek> Seek for Input<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(p) => i128::from(p),
            SeekFrom::Current(p) => i128::from(self.position) + i128::from(p),
            SeekFrom::End(p) => i128::from(self.length) + i128::from(p),
        };
        if position < 0 || position > i128::from(self.length) {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Image offset is outside the file",
            ));
        }
        self.reader
            .seek(SeekFrom::Start(self.start + position as u64))?;
        self.position = position as u64;
        Ok(self.position)
    }
}

pub(super) fn allocate(size: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| "Image allocation failed")?;
    bytes.resize(size, 0);
    Ok(bytes)
}

pub(super) fn frame_bytes(
    extent: [u32; 2],
    bpp: usize,
    workspace_bpp: usize,
    encoded_bytes: u64,
    overhead: usize,
    limits: DecodeLimits,
) -> Result<usize, String> {
    limits.extent(extent)?;
    let pixels = (extent[0] as usize)
        .checked_mul(extent[1] as usize)
        .ok_or("Image dimensions overflow")?;
    let bytes = pixels.checked_mul(bpp).ok_or("Image size overflow")?;
    // The extra source band is used while packing full-frame output into tiles.
    let band = extent[0] as usize * bpp * layer_core::raster::TILE_SIZE as usize;
    pixels
        .checked_mul(workspace_bpp)
        .and_then(|n| n.checked_add(bytes))
        .and_then(|n| n.checked_add(band))
        .and_then(|n| n.checked_add(overhead))
        .and_then(|n| {
            usize::try_from(encoded_bytes)
                .ok()
                .and_then(|v| n.checked_add(v))
        })
        .filter(|n| *n <= limits.codec_bytes)
        .ok_or("The decoded image exceeds the codec budget")?;
    Ok(bytes)
}

pub(super) fn source(
    pixels: &[u8],
    extent: [u32; 2],
    interpretation: SourceInterpretation,
    metadata: super::metadata::Exif,
    limits: DecodeLimits,
) -> Result<SourceImage, String> {
    let row_bytes = extent[0] as usize * interpretation.pixel_bytes();
    if pixels.len() != row_bytes * extent[1] as usize {
        return Err("Incomplete decoded image".into());
    }
    let mut builder = SourceBuilder::new(extent, interpretation, limits.source_bytes)?;
    for row in pixels.chunks_exact(row_bytes) {
        builder.push_row(row)?;
    }
    let mut source = builder.finish()?;
    source.resolution = metadata.resolution;
    super::orientation::normalize(source, metadata.orientation, limits.source_bytes)
}
