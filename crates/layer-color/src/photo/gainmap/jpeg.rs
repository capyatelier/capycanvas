//! Portable JPEG gain-map encoding, reconstruction and encoded previews.
use super::super::jpeg_codec;
use super::*;
use layer_core::color::{hdr, rgb};
use libjpeg_turbo_rs::{ColorSpace, Encoder, Image, PixelFormat, Subsampling};
use std::sync::atomic::Ordering;

fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("HDR operation cancelled".into())
    } else {
        Ok(())
    }
}
fn admit(extent: [u32; 2], compressed: usize, budget: usize) -> Result<usize, String> {
    validate_extent(extent, 32768)?;
    // Master/gain floats, JPEG planes and metadata insertion copies coexist.
    // Use u64 so the same admission check also runs on 32-bit WebAssembly.
    let needed = u64::from(extent[0]) * u64::from(extent[1]) * 96
        + 8 * 1024 * 1024
        + (compressed as u64).saturating_mul(4);
    if needed > (budget as u64).min(16 * 1024 * 1024 * 1024) {
        return Err("HDR gain-map output exceeds the available memory budget. Choose a smaller export size.".into());
    }
    Ok(extent[0] as usize * extent[1] as usize)
}
fn decode(
    bytes: &[u8],
    retained: usize,
    limits: DecodeLimits,
    cancel: &AtomicBool,
) -> Result<Image, String> {
    check(cancel)?;
    let mut decoder = jpeg_codec::decoder(bytes, retained, limits)?;
    if decoder.header().precision != 8
        || !matches!(
            decoder.jpeg_color_space(),
            ColorSpace::Grayscale | ColorSpace::YCbCr | ColorSpace::Rgb
        )
    {
        return Err("Gain-map JPEG requires 8-bit RGB or grayscale samples".into());
    }
    decoder.set_output_format(PixelFormat::Rgb);
    decoder.output_buffer_size().map_err(err)?;
    let image = decoder.decode_image().map_err(err)?;
    check(cancel)?;
    Ok(image)
}

fn encode(
    extent: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    guide: Option<&hdr::LocalToneGuide>,
    quality: u8,
    resolution: Option<layer_core::ImageResolution>,
    matte: Option<[f32; 3]>,
    clip: bool,
    budget: PhotoMemoryBudget,
    cancel: &AtomicBool,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<(Vec<u8>, crate::OutputStatistics), String> {
    check(cancel)?;
    rendition.validate().map_err(str::to_string)?;
    if !(1..=100).contains(&quality) {
        return Err("Invalid HDR quality".into());
    }
    if matte.is_some_and(|p| p.iter().any(|v| !v.is_finite() || !(0. ..=1.).contains(v))) {
        return Err("Invalid HDR background".into());
    }
    let count = admit(extent, 0, budget.encode_bytes)?;
    let profile = crate::icc::nclx_profile(
        [0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290],
        13,
    )?;
    let icc = profile_bytes(&profile)?;
    let matrix = hdr::to_bt2020(space);
    let mapper = rendition.mapper(space, RgbSpace::Srgb);
    let generated = if guide.is_none() {
        Some(crate::build_local_tone_guide(
            extent,
            space,
            || cancel.load(Ordering::Relaxed),
            &mut read,
        )?)
    } else {
        None
    };
    let guide = guide
        .or(generated.as_ref())
        .ok_or("Missing HDR tone guide")?;
    let mut base = Vec::new();
    base.try_reserve_exact(count * 3).map_err(err)?;
    let mut master = Vec::<f32>::new();
    master.try_reserve_exact(count * 3).map_err(err)?;
    let mut row = vec![[0.; 4]; extent[0] as usize];
    let mut stats = crate::OutputStatistics::default();
    let mut peak = 1f32;
    for y in 0..extent[1] {
        check(cancel)?;
        read(y, &mut row)?;
        for (x, p) in row.iter().enumerate() {
            if p.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&p[3]) {
                return Err("Invalid HDR output pixel".into());
            }
            let a = p[3];
            if a < 1. && matte.is_none() {
                return Err("Enable Flatten transparency for HDR JPEG".into());
            }
            let raw = std::array::from_fn(|c| if a > 0. { f64::from(p[c] / a) } else { 0. });
            let mut hdr = rgb::apply(matrix, raw).map(|v| v as f32);
            let position = [
                (x as f32 + 0.5) * guide.document_extent[0] as f32 / extent[0] as f32,
                (y as f32 + 0.5) * guide.document_extent[1] as f32 / extent[1] as f32,
            ];
            let mapped = mapper.map_local_premultiplied(*p, position, guide);
            let mut sdr = rgb::apply(
                hdr::srgb_to_bt2020(),
                std::array::from_fn(|c| if a > 0. { f64::from(mapped[c] / a) } else { 0. }),
            )
            .map(|v| v as f32);
            for c in 0..3 {
                sdr[c] = sdr[c].clamp(0., 1.);
                if let Some(background) = matte {
                    hdr[c] = hdr[c] * a + background[c] * (1. - a);
                    sdr[c] = sdr[c] * a + background[c] * (1. - a);
                }
                if !hdr[c].is_finite() {
                    return Err("Invalid HDR output pixel".into());
                }
                if hdr[c] < -1e-6 || hdr[c] > hdr::MAX_LINEAR {
                    if !clip {
                        return Err("HDR gain-map output exceeds BT.2020 or the half-float range. Enable Clip out-of-range colors to export a mapped copy.".into());
                    }
                    stats.clipped_channels += 1;
                }
                hdr[c] = hdr[c].clamp(0., hdr::MAX_LINEAR);
                peak = peak.max(hdr[c]);
                master.push(hdr[c]);
                base.push(
                    (RgbSpace::Srgb.encode(f64::from(sdr[c])).clamp(0., 1.) * 255.).round() as u8,
                );
            }
        }
    }
    check(cancel)?;
    let exif = resolution
        .map(super::super::metadata::exif_output)
        .transpose()?
        .unwrap_or_default();
    let mut encoder = Encoder::new(
        &base,
        extent[0] as usize,
        extent[1] as usize,
        PixelFormat::Rgb,
    )
    .quality(quality)
    .subsampling(Subsampling::S444)
    .force_baseline(true)
    .icc_profile(&icc);
    if !exif.is_empty() {
        encoder = encoder.exif_data(&exif[6..]);
    }
    if let Some(resolution) = resolution {
        let (unit, [x, y]) = resolution.jfif_density()?;
        encoder = encoder.density(unit, x, y);
    }
    let encoded_base = encoder.encode().map_err(err)?;
    check(cancel)?;
    drop(base);
    let mut limits = DecodeLimits::from_memory_budget(budget);
    limits.codec_bytes = budget
        .encode_bytes
        .checked_sub(master.capacity() * 4)
        .ok_or(jpeg_codec::MEMORY_ERROR)?;
    let decoded_base = decode(&encoded_base, encoded_base.capacity(), limits, cancel)?;
    let mut low = 0f32;
    let mut high = 0f32;
    const OFFSET: f32 = 1. / 64.;
    for (logs, codes) in master
        .chunks_mut(extent[0] as usize * 3)
        .zip(decoded_base.data.chunks(extent[0] as usize * 3))
    {
        check(cancel)?;
        for (value, code) in logs.iter_mut().zip(codes) {
            *value = ((*value + OFFSET)
                / (RgbSpace::Srgb.decode(f64::from(*code) / 255.) as f32 + OFFSET))
                .log2();
            low = low.min(*value);
            high = high.max(*value);
        }
    }
    drop(decoded_base);
    let metadata = GainMapMetadata {
        min_log2: low,
        max_log2: high.max(low + 0.001),
        offset: OFFSET,
        headroom: peak.log2().max(0.001),
    };
    let mut gains = Vec::new();
    gains.try_reserve_exact(count * 3).map_err(err)?;
    for row in master.chunks(extent[0] as usize * 3) {
        check(cancel)?;
        gains.extend(
            row.iter()
                .map(|v| (metadata.encode(*v) * 255.).round() as u8),
        );
    }
    drop(master);
    // Gains are RGB data, not photographic luma/chroma. Avoid color conversion
    // and chroma subsampling, including at low SDR base quality.
    let gain = Encoder::new(
        &gains,
        extent[0] as usize,
        extent[1] as usize,
        PixelFormat::Rgb,
    )
    .quality(100)
    .colorspace(ColorSpace::Rgb)
    .subsampling(Subsampling::S444)
    .force_baseline(true)
    .encode()
    .map_err(err)?;
    check(cancel)?;
    let bytes = super::jpeg_container::assemble(&encoded_base, &gain, metadata)?;
    check(cancel)?;
    Ok((bytes, stats))
}

pub(super) fn write(
    mut output: impl Write,
    extent: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    guide: Option<&hdr::LocalToneGuide>,
    options: impl Into<GainMapEncodeOptions>,
    resolution: Option<layer_core::ImageResolution>,
    matte: Option<[f32; 3]>,
    clip: bool,
    cancel: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<crate::OutputStatistics, String> {
    let options = options.into();
    let (bytes, stats) = encode(
        extent, space, rendition, guide, options.quality, resolution, matte, clip, options.memory, cancel, read,
    )?;
    for chunk in bytes.chunks(65536) {
        check(cancel)?;
        output.write_all(chunk).map_err(err)?;
    }
    output.flush().map_err(err)?;
    Ok(stats)
}

struct Pair {
    base: Image,
    gain: Image,
    metadata: Metadata,
    color: crate::icc::GainMapColor,
}
impl Pair {
    fn new(
        bytes: &[u8],
        retained: usize,
        limits: DecodeLimits,
        cancel: &AtomicBool,
    ) -> Result<Self, String> {
        check(cancel)?;
        let images = super::jpeg_container::parse(bytes)?;
        let header = jpeg_codec::decoder(images.base, retained, limits)?;
        let extent = [header.header().width as u32, header.header().height as u32];
        limits.extent(extent)?;
        admit(extent, retained, limits.codec_bytes)?;
        drop(header);
        let header = jpeg_codec::decoder(images.gain, retained, limits)?;
        let gain_extent = [header.header().width as u32, header.header().height as u32];
        limits.extent(gain_extent)?;
        if gain_extent[0] > extent[0] || gain_extent[1] > extent[1] {
            return Err("JPEG gain map is larger than its base image".into());
        }
        drop(header);
        let base_metadata =
            super::super::jpeg_markers::read_source(std::io::Cursor::new(images.base))?;
        let base_profile = base_metadata
            .profile
            .map(|p| ColorProfile::Icc(p.into()))
            .unwrap_or(ColorProfile::Builtin(RgbSpace::Srgb));
        let application_profile = if images.metadata.use_base_space {
            None
        } else {
            let gain_metadata =
                super::super::jpeg_markers::read_source(std::io::Cursor::new(images.gain))?;
            Some(ColorProfile::Icc(
                gain_metadata
                    .profile
                    .ok_or("Gain-map application color profile is missing")?
                    .into(),
            ))
        };
        let color = crate::icc::GainMapColor::new(&base_profile, application_profile.as_ref())?;
        let base = decode(images.base, retained, limits, cancel)?;
        let mut gain_limits = limits;
        gain_limits.codec_bytes = limits
            .codec_bytes
            .checked_sub(base.data.capacity())
            .ok_or(jpeg_codec::MEMORY_ERROR)?;
        let gain = decode(images.gain, retained, gain_limits, cancel)?;
        Ok(Self {
            base,
            gain,
            metadata: images.metadata,
            color,
        })
    }
    fn extent(&self) -> [u32; 2] {
        [self.base.width as u32, self.base.height as u32]
    }
    fn row(&self, y: u32, hdr: &mut [[f32; 4]], sdr: &mut [[f32; 4]]) -> Result<(), String> {
        let sample_gain =
            |x: usize, y: usize, c| self.gain.data[(y * self.gain.width + x) * 3 + c] as f32 / 255.;
        for x in 0..self.base.width {
            // Sample-center bilinear interpolation also handles reduced grayscale
            // gain maps. RGB output from the codec replicates their one channel.
            let gx =
                ((x as f32 + 0.5) * self.gain.width as f32 / self.base.width as f32 - 0.5).max(0.);
            let gy = ((y as f32 + 0.5) * self.gain.height as f32 / self.base.height as f32 - 0.5)
                .max(0.);
            let (x0, y0) = (gx as usize, gy as usize);
            let (x1, y1) = (
                (x0 + 1).min(self.gain.width - 1),
                (y0 + 1).min(self.gain.height - 1),
            );
            let gain = std::array::from_fn(|c| {
                let top = sample_gain(x0, y0, c) * (1. - gx.fract())
                    + sample_gain(x1, y0, c) * gx.fract();
                let bottom = sample_gain(x0, y1, c) * (1. - gx.fract())
                    + sample_gain(x1, y1, c) * gx.fract();
                top * (1. - gy.fract()) + bottom * gy.fract()
            });
            let at = (y as usize * self.base.width + x) * 3;
            let base = self.color.linear_base(std::array::from_fn(|c| {
                self.base.data[at + c] as f32 / 255.
            }));
            let h = self.color.to_srgb(self.metadata.reconstruct(base, gain));
            let s = self.color.to_srgb(base);
            if h.iter()
                .chain(&s)
                .any(|v| !v.is_finite() || v.abs() > hdr::MAX_LINEAR)
            {
                return Err("Reconstructed JPEG exceeds the half-float range".into());
            }
            hdr[x] = [h[0], h[1], h[2], 1.];
            sdr[x] = [s[0], s[1], s[2], 1.];
        }
        Ok(())
    }
}

pub(super) fn read(
    input: impl Read,
    limits: DecodeLimits,
    cancel: &AtomicBool,
) -> Result<SourceImage, String> {
    check(cancel)?;
    let bytes = jpeg_codec::read_bounded(input, limits.codec_bytes / 4)?;
    let pair = Pair::new(&bytes, bytes.capacity(), limits, cancel)?;
    let extent = pair.extent();
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F16,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        limits.source_bytes,
    )?;
    let mut hdr = vec![[0.; 4]; extent[0] as usize];
    let mut sdr = hdr.clone();
    let mut row = vec![0; extent[0] as usize * 8];
    for y in 0..extent[1] {
        check(cancel)?;
        pair.row(y, &mut hdr, &mut sdr)?;
        for (pixel, bytes) in hdr.iter().zip(row.chunks_exact_mut(8)) {
            let bits = layer_core::color::hdr::encode_pixel(*pixel).map_err(str::to_string)?;
            for (v, dst) in bits.into_iter().zip(bytes.chunks_exact_mut(2)) {
                dst.copy_from_slice(&v.to_le_bytes());
            }
        }
        builder.push_row(&row)?;
    }
    builder.finish()
}

pub(super) fn preview(
    extent: [u32; 2],
    bounds: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    guide: Option<&hdr::LocalToneGuide>,
    options: impl Into<GainMapEncodeOptions>,
    matte: Option<[f32; 3]>,
    cancel: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<
    (
        [u32; 2],
        Vec<[f32; 4]>,
        Vec<[f32; 4]>,
        crate::OutputStatistics,
    ),
    String,
> {
    if bounds.into_iter().any(|n| !(1..=1024).contains(&n)) {
        return Err("Invalid preview dimensions".into());
    }
    let options = options.into();
    let (bytes, stats) = encode(
        extent, space, rendition, guide, options.quality, None, matte, true, options.memory, cancel, read,
    )?;
    let mut limits = DecodeLimits::from_memory_budget(options.memory);
    // Both rendition accumulators and their finished outputs coexist with the
    // decoded pair. Reserve their bounded storage before admitting that pair.
    let scratch = u64::from(bounds[0]) * u64::from(bounds[1]) * 128 + 4 * 1024 * 1024;
    limits.codec_bytes = limits.codec_bytes.checked_sub(usize::try_from(scratch).map_err(err)?)
        .ok_or(jpeg_codec::MEMORY_ERROR)?;
    let pair = Pair::new(&bytes, bytes.capacity(), limits, cancel)?;
    let mut hdr_preview = crate::AreaPreview::new(extent, bounds)?;
    let mut sdr_preview = crate::AreaPreview::new(extent, bounds)?;
    let mut hdr = vec![[0.; 4]; extent[0] as usize];
    let mut sdr = hdr.clone();
    for y in 0..extent[1] {
        check(cancel)?;
        pair.row(y, &mut hdr, &mut sdr)?;
        hdr_preview.push(&hdr)?;
        sdr_preview.push(&sdr)?;
    }
    let (extent, hdr) = hdr_preview.finish()?;
    let (_, sdr) = sdr_preview.finish()?;
    Ok((extent, hdr, sdr, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXTENT: [u32; 2] = [48, 32];
    fn pixel(x: u32, y: u32) -> [f32; 4] {
        match x / 16 {
            0 => [0.001 + y as f32 / 64., 0.04, 0.12, 1.],
            1 => [8., 0.02 + y as f32 / 32., 0., 1.],
            _ => [0.15, 2., 4. + y as f32 / 16., 1.],
        }
    }
    fn rows(y: u32, row: &mut [[f32; 4]]) -> Result<(), String> {
        for (x, p) in row.iter_mut().enumerate() {
            *p = pixel(x as u32, y);
        }
        Ok(())
    }
    fn encoded(quality: u8, exposure: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        write(
            &mut bytes,
            EXTENT,
            RgbSpace::Srgb,
            SdrRendition {
                exposure,
                ..Default::default()
            },
            None,
            quality,
            Some(layer_core::ImageResolution::ppi(300)),
            None,
            false,
            &AtomicBool::new(false),
            rows,
        )
        .unwrap();
        bytes
    }
    fn assert_hdr(source: SourceImage) {
        assert_eq!(source.extent, EXTENT);
        assert_eq!(source.interpretation.depth, SampleDepth::F16);
        let mut rows = source.rows();
        let mut bytes = vec![0; source.row_bytes()];
        let mut largest = 0f32;
        for y in 0..EXTENT[1] {
            rows.read(y, &mut bytes).unwrap();
            for (x, p) in bytes.chunks_exact(8).enumerate() {
                let actual = hdr::decode_pixel(std::array::from_fn(|c| {
                    u16::from_le_bytes([p[c * 2], p[c * 2 + 1]])
                }))
                .unwrap();
                let expected = pixel(x as u32, y);
                for c in 0..3 {
                    let error = (actual[c] - expected[c]).abs();
                    largest = largest.max(error);
                    assert!(
                        error < 0.13 + 0.035 * expected[c],
                        "({x},{y}) {actual:?} != {expected:?}"
                    );
                }
                assert_eq!(actual[3], 1.);
            }
        }
        eprintln!("portable JPEG largest HDR error: {largest}");
    }
    fn hide_namespace(bytes: &mut [u8], namespace: &[u8]) {
        for at in 0..bytes.len().saturating_sub(namespace.len()) {
            if bytes[at..].starts_with(namespace) {
                bytes[at] = b'x';
            }
        }
    }
    #[test]
    fn jpeg_hdr_and_authored_sdr_roundtrip_without_native_features() {
        for quality in [35, 90, 100] {
            for exposure in [0., -2.] {
                let bytes = encoded(quality, exposure);
                let source = read_photo(std::io::Cursor::new(&bytes), Default::default()).unwrap();
                assert!(source.resolution.is_some());
                assert_hdr(source);
                for namespace in [
                    b"urn:iso:std:iso:ts:21496:-1\0".as_slice(),
                    b"http://ns.adobe.com/xap/1.0/\0",
                ] {
                    let mut legacy = bytes.clone();
                    hide_namespace(&mut legacy, namespace);
                    assert_hdr(
                        read_photo(std::io::Cursor::new(legacy), Default::default()).unwrap(),
                    );
                }
            }
        }
        let c = AtomicBool::new(false);
        let (_, h0, s0, _) = preview(
            EXTENT,
            EXTENT,
            RgbSpace::Srgb,
            Default::default(),
            None,
            90,
            None,
            &c,
            rows,
        )
        .unwrap();
        let (_, h1, s1, _) = preview(
            EXTENT,
            EXTENT,
            RgbSpace::Srgb,
            SdrRendition {
                exposure: -2.,
                ..Default::default()
            },
            None,
            90,
            None,
            &c,
            rows,
        )
        .unwrap();
        assert!(s0.iter().zip(&s1).any(|(a, b)| (a[0] - b[0]).abs() > 0.05));
        assert!(h0.iter().zip(&h1).all(|(a, b)| (a[0] - b[0]).abs() < 0.4));
    }
    #[test]
    fn jpeg_transparency_cancel_limits_and_malformed_containers() {
        let c = AtomicBool::new(false);
        let transparent = |_: u32, row: &mut [[f32; 4]]| {
            row.fill([2., 0.5, 0., 0.5]);
            Ok(())
        };
        let run = |matte, c: &AtomicBool| {
            write(
                std::io::sink(),
                [16, 16],
                RgbSpace::Srgb,
                Default::default(),
                None,
                90,
                None,
                matte,
                false,
                c,
                transparent,
            )
        };
        assert!(run(None, &c).unwrap_err().contains("Flatten"));
        run(Some([1.; 3]), &c).unwrap();
        assert!(
            run(Some([1.; 3]), &AtomicBool::new(true))
                .unwrap_err()
                .contains("cancelled")
        );
        assert!(admit([32768, 32768], 0, 512 * 1024 * 1024).is_err());
        let bytes = encoded(90, 0.);
        let limits = DecodeLimits {
            codec_bytes: 1024,
            ..Default::default()
        };
        assert!(read(std::io::Cursor::new(&bytes), limits, &c).is_err());
        for len in [0, 2, 90, bytes.len() / 2, bytes.len() - 1] {
            assert!(read(std::io::Cursor::new(&bytes[..len]), Default::default(), &c).is_err());
        }
        let mut invalid_offset = bytes;
        invalid_offset[82..86].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(read(std::io::Cursor::new(invalid_offset), Default::default(), &c).is_err());
    }
    #[test]
    fn jpeg_grayscale_reduced_gain_maps_and_orientation() {
        let exif = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let base = Encoder::new(&vec![128; 12 * 8 * 3], 12, 8, PixelFormat::Rgb)
            .quality(100)
            .exif_data(exif)
            .encode()
            .unwrap();
        let gain = Encoder::new(&[255; 6], 3, 2, PixelFormat::Grayscale)
            .quality(100)
            .encode()
            .unwrap();
        let meta = GainMapMetadata {
            min_log2: 0.,
            max_log2: 2.,
            offset: 0.,
            headroom: 2.,
        };
        let bytes = super::super::jpeg_container::assemble(&base, &gain, meta).unwrap();
        assert_eq!(
            read_photo(std::io::Cursor::new(&bytes), Default::default())
                .unwrap()
                .extent,
            [8, 12]
        );
        let pair = Pair::new(
            &bytes,
            bytes.capacity(),
            Default::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let mut hdr = vec![[0.; 4]; 12];
        let mut sdr = hdr.clone();
        for y in 0..8 {
            pair.row(y, &mut hdr, &mut sdr).unwrap();
            for (h, s) in hdr.iter().zip(&sdr) {
                for c in 0..3 {
                    assert!((h[c] - s[c] * 4.).abs() < 1e-5);
                }
            }
        }
    }
    #[cfg(all(feature = "native-codec-reference", target_os = "linux"))]
    #[test]
    #[ignore = "independent libultrahdr interoperability oracle; CAPY_PHOTO_CODEC_DIR"]
    fn jpeg_interoperates_both_directions_with_libultrahdr() {
        let c = AtomicBool::new(false);
        let bytes = encoded(90, -2.);
        for hide in [
            None,
            Some(b"urn:iso:std:iso:ts:21496:-1\0".as_slice()),
            Some(b"http://ns.adobe.com/xap/1.0/\0".as_slice()),
        ] {
            let mut bytes = bytes.clone();
            if let Some(ns) = hide {
                hide_namespace(&mut bytes, ns);
            }
            assert_hdr(
                super::super::native::read_gainmap(
                    std::io::Cursor::new(bytes),
                    GainMapFormat::Jpeg,
                    Default::default(),
                    &c,
                )
                .unwrap(),
            );
        }
        let mut native = Vec::new();
        super::super::native::write(
            &mut native,
            EXTENT,
            RgbSpace::Srgb,
            SdrRendition {
                exposure: -2.,
                ..Default::default()
            },
            None,
            GainMapFormat::Jpeg,
            90,
            None,
            None,
            false,
            &c,
            rows,
        )
        .unwrap();
        assert_hdr(read_photo(std::io::Cursor::new(native), Default::default()).unwrap());
    }
}
