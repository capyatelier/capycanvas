//! Portable JPEG gain-map encoding, reconstruction and encoded previews.
use super::super::jpeg_codec;
use super::*;
use layer_core::color::hdr;
use libjpeg_turbo_rs::{ColorSpace, Encoder, Image, PixelFormat, Subsampling};

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
    check_cancel(cancel)?;
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
    check_cancel(cancel)?;
    Ok(image)
}

pub(super) fn encode(
    extent: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    guide: &hdr::LocalToneGuide,
    quality: u8,
    delivery: &DeliveryMetadata,
    matte: Option<[f32; 3]>,
    clip: bool,
    budget: PhotoMemoryBudget,
    cancel: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<(Vec<u8>, crate::OutputStatistics), String> {
    let count = admit(extent, 0, budget.encode_bytes)?;
    let profile = crate::icc::nclx_profile(
        [0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290],
        13,
    )?;
    let icc = profile_bytes(&profile)?;
    let mut base = Vec::new();
    base.try_reserve_exact(count * 3).map_err(err)?;
    let mut master = Vec::<f32>::new();
    master.try_reserve_exact(count * 3).map_err(err)?;
    let (stats, peak) = render_pair(
        extent, space, rendition, guide, quality, matte, clip, cancel, read,
        |hdr, sdr, a| {
            if a < 1. && matte.is_none() {
                return Err("Enable Flatten transparency for HDR JPEG".into());
            }
            master.extend(hdr);
            base.extend(sdr.map(|v| {
                (RgbSpace::Srgb.encode(f64::from(v)).clamp(0., 1.) * 255.).round() as u8
            }));
            Ok(())
        },
    )?;
    check_cancel(cancel)?;
    let exif = delivery.exif(extent)?;
    let descriptions = delivery.xmp_descriptions()?;
    let encoded_base = jpeg_codec::encode(
        &base,
        extent,
        PixelFormat::Rgb,
        quality,
        &icc,
        jpeg_codec::Markers { exif: exif.as_deref(), xmp: None },
        delivery.resolution,
    )?;
    check_cancel(cancel)?;
    drop(base);
    let mut limits = DecodeLimits::from_memory_budget(budget);
    limits.codec_bytes = budget
        .encode_bytes
        .checked_sub(master.capacity() * 4)
        .ok_or(jpeg_codec::MEMORY_ERROR)?;
    let decoded_base = decode(&encoded_base, encoded_base.capacity(), limits, cancel)?;
    let mut gains = LogGain::default();
    for (logs, codes) in master
        .chunks_mut(extent[0] as usize * 3)
        .zip(decoded_base.data.chunks(extent[0] as usize * 3))
    {
        check_cancel(cancel)?;
        for (value, code) in logs.iter_mut().zip(codes) {
            gains.apply(value, f64::from(*code) / 255.);
        }
    }
    drop(decoded_base);
    let metadata = gains.metadata(peak);
    let mut gains = Vec::new();
    gains.try_reserve_exact(count * 3).map_err(err)?;
    for row in master.chunks(extent[0] as usize * 3) {
        check_cancel(cancel)?;
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
    check_cancel(cancel)?;
    let bytes = super::jpeg_container::assemble(&encoded_base, &gain, metadata, descriptions.as_deref())?;
    check_cancel(cancel)?;
    Ok((bytes, stats))
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
        check_cancel(cancel)?;
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
        let base_metadata = super::super::jpeg_markers::read_source(images.base)?;
        let base_profile = base_metadata
            .profile
            .map(|p| ColorProfile::Icc(p.into()))
            .unwrap_or(ColorProfile::Builtin(RgbSpace::Srgb));
        let application_profile = if images.metadata.use_base_space {
            None
        } else {
            let gain_metadata = super::super::jpeg_markers::read_source(images.gain)?;
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
        let extent = self.extent();
        let gain_extent = [self.gain.width as u32, self.gain.height as u32];
        for x in 0..self.base.width {
            // RGB output from the codec replicates a grayscale gain map's channel.
            let gain = bilinear(extent, gain_extent, x as u32, y, |x, y| {
                let at = (y as usize * self.gain.width + x as usize) * 3;
                std::array::from_fn(|c| self.gain.data[at + c] as f32 / 255.)
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

pub(in crate::photo) fn read(
    bytes: &[u8],
    retained: usize,
    limits: DecodeLimits,
    cancel: &AtomicBool,
) -> Result<SourceImage, String> {
    let pair = Pair::new(bytes, retained, limits, cancel)?;
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
        check_cancel(cancel)?;
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
    guide: &hdr::LocalToneGuide,
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
    let options = options.into();
    let (bytes, stats) = encode(
        extent, space, rendition, guide, options.quality, &Default::default(), matte, true, options.memory, cancel, read,
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
        check_cancel(cancel)?;
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
    use super::super::test_guide;
    use super::*;
    use crate::photo::test_support::hdr_pixels;
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
        write_gainmap_rows(
            &mut bytes,
            EXTENT,
            RgbSpace::Srgb,
            SdrRendition {
                exposure,
                ..Default::default()
            },
            &test_guide(EXTENT, rows),
            GainMapFormat::Jpeg,
            quality,
            &crate::photo::DeliveryMetadata::resolution(Some(layer_core::ImageResolution::ppi(300))),
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
        for (i, actual) in hdr_pixels(&source).into_iter().enumerate() {
            let (x, y) = (i as u32 % EXTENT[0], i as u32 / EXTENT[0]);
            let expected = pixel(x, y);
            for c in 0..3 {
                let error = (actual[c] - expected[c]).abs();
                assert!(
                    error < 0.13 + 0.035 * expected[c],
                    "({x},{y}) {actual:?} != {expected:?}"
                );
            }
            assert_eq!(actual[3], 1.);
        }
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
        let guide = test_guide(EXTENT, rows);
        let (_, h0, s0, _) = preview(
            EXTENT,
            EXTENT,
            RgbSpace::Srgb,
            Default::default(),
            &guide,
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
            &guide,
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
        let guide = test_guide([16, 16], transparent);
        let run = |matte, c: &AtomicBool| {
            write_gainmap_rows(
                std::io::sink(),
                [16, 16],
                RgbSpace::Srgb,
                Default::default(),
                &guide,
                GainMapFormat::Jpeg,
                90,
                &Default::default(),
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
        assert!(read(&bytes, bytes.len(), limits, &c).is_err());
        for len in [0, 2, 90, bytes.len() / 2, bytes.len() - 1] {
            assert!(read(&bytes[..len], len, Default::default(), &c).is_err());
        }
        let mut invalid_offset = bytes;
        invalid_offset[82..86].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(read(&invalid_offset, invalid_offset.len(), Default::default(), &c).is_err());
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
        let bytes = super::super::jpeg_container::assemble(&base, &gain, meta, None).unwrap();
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
}
