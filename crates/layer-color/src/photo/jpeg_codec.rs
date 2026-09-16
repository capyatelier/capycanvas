//! Pure Rust JPEG codec. This backend buffers full images; enforce admission
//! limits before allocating pixels, and account for retained compressed input.
use super::*;
use libjpeg_turbo_rs::{ColorSpace, Decoder, Encoder as JpegEncoder, PixelFormat, Subsampling};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(super) const MEMORY_ERROR: &str = "JPEG exceeds the codec memory budget; use a smaller image";
const SCRATCH_BYTES: usize = 2 * 1024 * 1024;

fn io<T>(operation: impl FnOnce() -> std::io::Result<T>) -> Result<T, String> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result.map_err(err),
        Err(payload) => {
            std::mem::forget(payload);
            Err("JPEG I/O callback panicked".into())
        }
    }
}

pub(super) fn read_bounded(mut input: impl Read, budget: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = io(|| {
            loop {
                match input.read(&mut buffer) {
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    result => break result,
                }
            }
        })?;
        if count == 0 {
            return Ok(bytes);
        }
        let needed = bytes
            .len()
            .checked_add(count)
            .filter(|n| *n <= budget)
            .ok_or(MEMORY_ERROR)?;
        if needed > bytes.capacity() {
            let capacity = needed.max(bytes.capacity().saturating_mul(2)).min(budget);
            bytes
                .try_reserve_exact(capacity - bytes.len())
                .map_err(|_| MEMORY_ERROR)?;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

pub(super) fn decoder(
    bytes: &[u8],
    input_capacity: usize,
    limits: DecodeLimits,
) -> Result<Decoder<'_>, String> {
    // Marker parsing retains selected metadata and coefficient/table snapshots.
    let budget = limits
        .codec_bytes
        .checked_sub(
            input_capacity
                .saturating_mul(4)
                .saturating_add(SCRATCH_BYTES),
        )
        .ok_or(MEMORY_ERROR)?;
    let mut decoder = Decoder::new_with_limits(
        bytes,
        libjpeg_turbo_rs::DecodeLimits {
            max_width: limits.dimension.min(32768) as usize,
            max_height: limits.dimension.min(32768) as usize,
            max_scans: 256,
            max_memory: Some(budget as u64),
            ..Default::default()
        },
    )
    .map_err(err)?;
    decoder.set_stop_on_warning(true);
    decoder.set_dct_method(libjpeg_turbo_rs::DctMethod::IsLow);
    Ok(decoder)
}

pub(super) fn channels(
    decoder: &Decoder<'_>,
    adobe: Option<u8>,
) -> Result<(SourceChannels, PixelFormat), String> {
    match decoder.jpeg_color_space() {
        ColorSpace::Grayscale => Ok((SourceChannels::Gray, PixelFormat::Grayscale)),
        ColorSpace::YCbCr | ColorSpace::Rgb => Ok((SourceChannels::Rgb, PixelFormat::Rgb)),
        ColorSpace::Cmyk | ColorSpace::Ycck if matches!(adobe, Some(0 | 2)) => {
            Ok((SourceChannels::Cmyk, PixelFormat::Cmyk))
        }
        ColorSpace::Cmyk | ColorSpace::Ycck => {
            Err("This CMYK JPEG needs an explicit sample-polarity interpretation".into())
        }
        _ => Err("Unsupported JPEG color encoding".into()),
    }
}

pub(super) struct Encoder<W> {
    output: W,
    pixels: Vec<u8>,
    extent: [usize; 2],
    format: PixelFormat,
    quality: u8,
    resolution: Option<layer_core::ImageResolution>,
    icc: Vec<u8>,
    exif: Vec<u8>,
    row: usize,
    finished: bool,
    budget: usize,
    pixel_working_bytes: usize,
}
impl<W: Write> Encoder<W> {
    pub fn new(
        output: W,
        extent: [u32; 2],
        channels: usize,
        quality: u8,
        resolution: Option<layer_core::ImageResolution>,
        budget: usize,
    ) -> Result<Self, String> {
        super::validate_extent(extent, 32768)?;
        if !(1..=100).contains(&quality) {
            return Err("JPEG quality must be between 1 and 100".into());
        }
        let format = match channels {
            1 => PixelFormat::Grayscale,
            3 => PixelFormat::Rgb,
            4 => PixelFormat::Cmyk,
            _ => return Err("Unsupported JPEG color encoding".into()),
        };
        let extent = extent.map(|v| v as usize);
        let len = extent[0]
            .checked_mul(extent[1])
            .and_then(|n| n.checked_mul(channels))
            .ok_or(MEMORY_ERROR)?;
        // Reserve estimated room for input, coding planes, entropy output and
        // metadata injection copies, within the caller's available-memory budget.
        let pixel_working_bytes = len
            .checked_mul(8)
            .and_then(|v| v.checked_add(SCRATCH_BYTES))
            .ok_or(MEMORY_ERROR)?;
        if pixel_working_bytes > budget {
            return Err(MEMORY_ERROR.into());
        }
        if let Some(resolution) = resolution {
            resolution.jfif_density()?;
        }
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).map_err(|_| MEMORY_ERROR)?;
        pixels.resize(len, 0);
        Ok(Self {
            output,
            pixels,
            extent,
            format,
            quality,
            resolution,
            icc: Vec::new(),
            exif: Vec::new(),
            row: 0,
            finished: false,
            budget,
            pixel_working_bytes,
        })
    }
    pub fn profile(&mut self, profile: &[u8]) -> Result<(), String> {
        if profile.len() > crate::MAX_ICC_BYTES.min(255 * 65519) {
            return Err("JPEG ICC profile is too large".into());
        }
        self.check_metadata_budget(profile.len(), self.exif.len())?;
        self.icc = profile.to_vec();
        Ok(())
    }
    pub fn marker(&mut self, marker: u8, data: &[u8]) -> Result<(), String> {
        if marker != 1 || !data.starts_with(b"Exif\0\0") || data.len() > 65533 {
            return Err("Unsupported JPEG output marker".into());
        }
        self.check_metadata_budget(self.icc.len(), data.len())?;
        self.exif = data[6..].to_vec();
        Ok(())
    }
    fn check_metadata_budget(&self, icc: usize, exif: usize) -> Result<(), String> {
        // Retained metadata, marker construction and final output insertion can
        // coexist. Profile bytes are not necessarily small relative to pixels.
        let total = self
            .pixel_working_bytes
            .saturating_add(icc.saturating_mul(4))
            .saturating_add(exif.saturating_mul(4));
        if total > self.budget {
            return Err(MEMORY_ERROR.into());
        }
        Ok(())
    }
    pub fn row(&mut self, row: &[u8]) -> Result<(), String> {
        let len = self.extent[0] * self.format.bytes_per_pixel();
        if self.finished || self.row >= self.extent[1] || row.len() != len {
            return Err("Invalid JPEG output row".into());
        }
        self.pixels[self.row * len..(self.row + 1) * len].copy_from_slice(row);
        self.row += 1;
        Ok(())
    }
    pub fn finish(&mut self) -> Result<(), String> {
        if self.finished {
            return Err("JPEG codec is unavailable after a completed or failed operation".into());
        }
        self.finished = true;
        if self.row != self.extent[1] {
            return Err("Incomplete JPEG output".into());
        }
        let mut encoder =
            JpegEncoder::new(&self.pixels, self.extent[0], self.extent[1], self.format)
                .quality(self.quality)
                .subsampling(Subsampling::S444)
                .force_baseline(true)
                .icc_profile(&self.icc);
        if !self.exif.is_empty() {
            encoder = encoder.exif_data(&self.exif);
        }
        if let Some(resolution) = self.resolution {
            let (unit, [x, y]) = resolution.jfif_density()?;
            encoder = encoder.density(unit, x, y);
        }
        let bytes = encoder.encode().map_err(err)?;
        io(|| self.output.write_all(&bytes))
    }
}
