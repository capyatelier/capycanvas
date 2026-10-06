//! Clipboard copies: one pass over captured working rows writes the
//! document-depth source that pastes read and the sRGB PNG that other
//! applications read.
use crate::WorkingEncoder;
use layer_core::color::{
    ColorProfile, DocumentColor, RgbSpace, SampleDepth,
    hdr::{LocalToneGuide, SdrRendition},
    source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation},
};

/// Where a copy was taken and how its rows are delivered.
pub struct ClipRows<'a> {
    /// Width and height of the copied rectangle.
    pub extent: [u32; 2],
    /// Its top-left corner in document pixels.
    pub origin: [u32; 2],
    pub color: DocumentColor,
    pub resolution: Option<layer_core::ImageResolution>,
    /// The SDR rendition of a high dynamic range drawing and its document guide.
    pub rendition: Option<(SdrRendition, &'a LocalToneGuide)>,
    /// Build the document-depth source. A copy of an untouched photo keeps
    /// the photo's own samples and writes only the PNG.
    pub source: bool,
    /// Retained source bytes allowed.
    pub limit: usize,
}

pub fn srgb_png_interpretation() -> SourceInterpretation {
    SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb),
        profile_assumed: false,
    }
}

/// Rows encoded together, split across the worker's threads.
const BAND_ROWS: usize = 32;

/// `read` fills linear premultiplied working rows of the copy, top to bottom.
pub fn write_clip_rows(
    clip: ClipRows<'_>,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<(Option<SourceImage>, Vec<u8>), String> {
    let target = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: clip.color.depth,
        profile: ColorProfile::Builtin(clip.color.space),
        profile_assumed: false,
    };
    let mut builder = clip.source.then(|| SourceBuilder::new(clip.extent, target.clone(), clip.limit)).transpose()?;
    let encoders = BandEncoders::new(&clip, &target)?;
    let width = clip.extent[0] as usize;
    let [source_bytes, png_bytes] = [target.pixel_bytes() * width, 4 * width];
    let mut pixels = vec![[0.; 4]; width * BAND_ROWS];
    let mut source = vec![0; source_bytes * BAND_ROWS];
    let mut png = vec![0; png_bytes * BAND_ROWS];
    let mut band = 0..0;
    let mut bytes = Vec::new();
    crate::photo::write_png_rows(&mut bytes, clip.extent, &srgb_png_interpretation(), &Default::default(), |y, output| {
        if !band.contains(&y) {
            band = y..(y + BAND_ROWS as u32).min(clip.extent[1]);
            let rows = band.len();
            for (row, line) in band.clone().zip(pixels.chunks_exact_mut(width)) {
                read(row, line)?;
            }
            encoders.encode(
                y,
                &mut pixels[..rows * width],
                builder.is_some().then(|| &mut source[..rows * source_bytes]),
                Some(&mut png[..rows * png_bytes]),
            )?;
            if let Some(builder) = &mut builder {
                for row in source[..rows * source_bytes].chunks_exact(source_bytes) {
                    builder.push_row(row)?;
                }
            }
        }
        let row = (y - band.start) as usize;
        output.copy_from_slice(&png[row * png_bytes..(row + 1) * png_bytes]);
        Ok(())
    })?;
    let source = builder
        .map(|builder| {
            let mut source = builder.finish()?;

            source.resolution = clip.resolution;
            Ok::<_, String>(source)
        })
        .transpose()?;
    Ok((source, bytes))
}

/// The document-depth source of a rectangle, written from bands of linear
/// premultiplied working rows, without the PNG `write_clip_rows` encodes.
pub struct SourceRows<'a> {
    clip: ClipRows<'a>,
    target: SourceInterpretation,
    builder: SourceBuilder,
    row: Vec<u8>,
    y: u32,
}
impl<'a> SourceRows<'a> {
    pub fn new(clip: ClipRows<'a>) -> Result<Self, String> {
        let target = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: clip.color.depth,
            profile: ColorProfile::Builtin(clip.color.space),
            profile_assumed: false,
        };
        let builder = SourceBuilder::new(clip.extent, target.clone(), clip.limit)?;
        BandEncoders::new(&clip, &target)?;
        Ok(Self { clip, target, builder, row: Vec::new(), y: 0 })
    }
    /// The next whole rows, top to bottom.
    pub fn push(&mut self, pixels: &mut [[f32; 4]]) -> Result<(), String> {
        let width = self.clip.extent[0] as usize;
        if pixels.len() % width != 0 || self.y as usize + pixels.len() / width > self.clip.extent[1] as usize {
            return Err("Rows outside the converted rectangle".into());
        }
        let row_bytes = self.target.pixel_bytes() * width;
        for band in pixels.chunks_mut(width * BAND_ROWS) {
            let rows = band.len() / width;
            self.row.resize(rows * row_bytes, 0);
            BandEncoders::new(&self.clip, &self.target)?.encode(self.y, band, Some(&mut self.row), None)?;
            for row in self.row.chunks_exact(row_bytes) { self.builder.push_row(row)?; }
            self.y += rows as u32;
        }
        Ok(())
    }
    pub fn finish(self) -> Result<SourceImage, String> {
        let mut source = self.builder.finish()?;
        source.resolution = self.clip.resolution;
        Ok(source)
    }
}

/// The sRGB PNG of an untouched photo copied whole, read straight from its
/// samples: no composition is needed when the copy is the photo itself.
pub fn source_png(
    source: &SourceImage,
    color: DocumentColor,
    limit: usize,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, String> {
    let decoder = crate::WorkingDecoder::new(&source.interpretation, color.space, Default::default())?;
    let mut rows = source.rows();
    let mut samples = vec![0; source.row_bytes()];
    let clip = ClipRows { extent: source.extent, origin: [0, 0], color, resolution: None, rendition: None, source: false, limit };
    write_clip_rows(clip, |y, row| {
        if cancelled() {
            return Err("Copy cancelled".into());
        }
        rows.read(y, &mut samples)?;
        decoder.decode_pixels(&samples, row)?;
        for pixel in row {
            for channel in 0..3 {
                pixel[channel] *= pixel[3];
            }
        }
        Ok(())
    })
    .map(|(_, png)| png)
}

/// Rows of a band that one thread encodes, starting at copy row `y`.
struct BandPart<'a> {
    y: u32,
    pixels: &'a mut [[f32; 4]],
    png: Option<&'a mut [u8]>,
    source: Option<&'a mut [u8]>,
}

/// Encodes bands of working rows into document and sRGB rows on every
/// available thread; each thread owns its encoders.
struct BandEncoders<'a> {
    clip: &'a ClipRows<'a>,
    target: &'a SourceInterpretation,
    threads: usize,
}
impl<'a> BandEncoders<'a> {
    fn new(clip: &'a ClipRows<'a>, target: &'a SourceInterpretation) -> Result<Self, String> {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
        let encoders = Self { clip, target, threads };
        encoders.encoders()?;
        Ok(encoders)
    }

    fn encoders(&self) -> Result<[WorkingEncoder; 2], String> {
        let working = self.clip.color.space;
        Ok([
            WorkingEncoder::new(working, self.target, Default::default())?,
            WorkingEncoder::new(working, &srgb_png_interpretation(), Default::default())?
                .with_sdr_gamut(self.clip.rendition.map(|(r, _)| r)),
        ])
    }

    /// Encode the band of `pixels` whose first row is copy row `y`.
    fn encode(&self, y: u32, pixels: &mut [[f32; 4]], source: Option<&mut [u8]>, png: Option<&mut [u8]>) -> Result<(), String> {
        let width = self.clip.extent[0] as usize;
        let rows = pixels.len() / width;
        let per_thread = rows.div_ceil(self.threads);
        fn split(bytes: Option<&mut [u8]>, rows: usize, per_thread: usize) -> Vec<Option<&mut [u8]>> {
            match bytes {
                Some(bytes) => { let row = bytes.len() / rows; bytes.chunks_mut(per_thread * row).map(Some).collect() }
                None => (0..rows.div_ceil(per_thread)).map(|_| None).collect(),
            }
        }
        let parts: Vec<_> = pixels
            .chunks_mut(per_thread * width)
            .zip(split(png, rows, per_thread))
            .zip(split(source, rows, per_thread))
            .enumerate()
            .map(|(part, ((pixels, png), source))| BandPart { y: y + (part * per_thread) as u32, pixels, png, source })
            .collect();
        if self.threads == 1 {
            return parts.into_iter().try_for_each(|part| self.encode_rows(part));
        }
        std::thread::scope(|scope| {
            let workers: Vec<_> = parts.into_iter().map(|part| scope.spawn(move || self.encode_rows(part))).collect();
            workers.into_iter().try_for_each(|w| w.join().map_err(|_| "Clipboard encoding failed".to_string())?)
        })
    }

    fn encode_rows(&self, BandPart { y, pixels, mut png, mut source }: BandPart<'_>) -> Result<(), String> {
        let [document, srgb] = self.encoders()?;
        let clip = self.clip;
        let width = clip.extent[0] as usize;
        let mapper = clip.rendition.map(|(r, guide)| (r.mapper(clip.color.space, clip.color.space), guide));
        let rows = pixels.len() / width;
        let source_bytes = source.as_ref().map_or(0, |s| s.len() / rows);
        for (row, line) in pixels.chunks_exact_mut(width).enumerate() {
            let origin = [clip.origin[0], clip.origin[1] + y + row as u32];
            for pixel in line.iter_mut().filter(|pixel| pixel[3] > 1.) { pixel[3] = 1.; }
            if let Some(source) = source.as_deref_mut() {
                document.encode_premultiplied(line, &mut source[row * source_bytes..(row + 1) * source_bytes], None, origin)?;
            }
            let Some(png) = png.as_deref_mut() else { continue; };
            let output = &mut png[row * width * 4..(row + 1) * width * 4];
            if let Some((mapper, guide)) = &mapper {
                for (x, pixel) in line.iter_mut().enumerate() {
                    let position = [(origin[0] + x as u32) as f32 + 0.5, origin[1] as f32 + 0.5];
                    *pixel = mapper.tone_local_premultiplied(*pixel, position, guide);
                }
            }
            srgb.encode_premultiplied(line, output, None, origin)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pass_writes_the_document_source_and_an_srgb_png() {
        let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
        let rows = [[[0.5, 0.25, 0., 0.5], [0.; 4]], [[1., 1., 1., 1.], [0., 0., 0.2, 0.2]]];
        let (source, png) = write_clip_rows(
            ClipRows { extent: [2, 2], origin: [3, 4], color, resolution: None, rendition: None, source: true, limit: 1 << 20 },
            |y, row| {
                row.copy_from_slice(&rows[y as usize]);
                Ok(())
            },
        )
        .unwrap();
        let source = source.unwrap();
        assert_eq!(source.extent, [2, 2]);
        assert_eq!(source.interpretation.depth, SampleDepth::U16);
        assert_eq!(source.interpretation.profile, ColorProfile::Builtin(RgbSpace::Srgb));
        let mut row = vec![0; source.row_bytes()];
        source.rows().read(1, &mut row).unwrap();
        let samples: Vec<u16> = row.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        assert_eq!(&samples[..4], &[65535; 4], "straight white");
        assert_eq!(samples[7], 13107, "straight alpha 0.2");
        let photo = crate::photo::read_photo(std::io::Cursor::new(png), Default::default()).unwrap();
        assert_eq!(photo.extent, [2, 2]);
        assert_eq!(photo.interpretation.depth, SampleDepth::U8);
        let mut row = vec![0; photo.row_bytes()];
        photo.rows().read(0, &mut row).unwrap();
        assert_eq!(row[3], 128, "half coverage");
        assert_eq!(&row[4..8], &[0; 4], "transparent");
    }

    #[test]
    fn filter_rounding_past_full_coverage_encodes_as_opaque() {
        let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
        let mut rows = SourceRows::new(ClipRows { extent: [1, 1], origin: [0, 0], color, resolution: None, rendition: None, source: true, limit: 1 << 20 }).unwrap();
        rows.push(&mut [[0.5, 0.25, 0., 1.000_000_1]]).unwrap();
        let source = rows.finish().unwrap();
        let mut row = vec![0; source.row_bytes()];
        source.rows().read(0, &mut row).unwrap();
        assert_eq!(u16::from_le_bytes([row[6], row[7]]), 65535);
        let mut rows = SourceRows::new(ClipRows { extent: [1, 1], origin: [0, 0], color, resolution: None, rendition: None, source: true, limit: 1 << 20 }).unwrap();
        assert!(rows.push(&mut [[f32::NAN, 0., 0., 1.]]).is_err(), "non-finite samples stay errors");
    }

    #[test]
    fn an_untouched_photo_copy_writes_only_the_png() {
        let color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U8 };
        let (source, png) = write_clip_rows(
            ClipRows { extent: [1, 1], origin: [0, 0], color, resolution: None, rendition: None, source: false, limit: 1 << 20 },
            |_, row| {
                row[0] = [0.2, 0.3, 0.4, 1.];
                Ok(())
            },
        )
        .unwrap();
        assert!(source.is_none());
        assert!(png.starts_with(b"\x89PNG"));
    }
}
