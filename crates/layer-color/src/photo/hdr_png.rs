//! Full-range RGB PQ PNG: absolute luminance → normalized linear half samples.
//! No gain-map reuse, guessed HLG rendering transform or full-frame staging.
use super::*;
use layer_core::color::{hdr, rgb};

pub(super) fn read<R: BufRead + Seek>(
    mut reader: png::Reader<R>,
    limits: DecodeLimits,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<SourceImage, String> {
    let info = reader.info();
    let cicp = info
        .coding_independent_code_points
        .ok_or("Missing HDR PNG interpretation")?;
    if cicp.transfer_function != 16 {
        return Err("HLG PNG is not supported; convert to full-range 16-bit PQ PNG".into());
    }
    if cicp.matrix_coefficients != 0 || !cicp.is_video_full_range_image {
        return Err("HDR PNG requires full-range RGB".into());
    }
    if info.interlaced {
        return Err("Interlaced HDR PNG is not supported; save a non-interlaced PQ PNG".into());
    }
    let extent = [info.width, info.height];
    limits.extent(extent)?;
    let (channels, depth) = reader.output_color_type();
    if depth != png::BitDepth::Sixteen {
        return Err("HDR PNG requires 16-bit PQ samples".into());
    }
    let channels = match channels {
        png::ColorType::Rgb => SourceChannels::Rgb,
        png::ColorType::Rgba => SourceChannels::Rgba,
        _ => return Err("HDR PNG requires RGB or RGBA samples".into()),
    };
    let matrix = match cicp.color_primaries {
        1 => RgbSpace::Srgb.linear_transform(RgbSpace::Srgb),
        12 => RgbSpace::DisplayP3.linear_transform(RgbSpace::Srgb),
        9 => hdr::bt2020_to_srgb(),
        _ => return Err("HDR PNG primaries must be sRGB, Display P3 or BT.2020".into()),
    };
    let interpretation = SourceInterpretation {
        channels,
        depth: SampleDepth::F16,
        profile: ColorProfile::Builtin(RgbSpace::Srgb),
        profile_assumed: false,
    };
    let mut builder = SourceBuilder::new(extent, interpretation, limits.source_bytes)?;
    let table: Vec<_> = (0..=65535)
        .map(|v| hdr::pq_decode(v as f64 / 65535.) / f64::from(hdr::REFERENCE_WHITE_NITS))
        .collect();
    let mut output = vec![0u8; extent[0] as usize * channels.count() * 2];
    while let Some(row) = reader.next_row().map_err(err)? {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Image read cancelled".into());
        }
        for (p, o) in row
            .data()
            .chunks_exact(channels.count() * 2)
            .zip(output.chunks_exact_mut(channels.count() * 2))
        {
            let code = |c: usize| u16::from_be_bytes([p[c * 2], p[c * 2 + 1]]);
            let rgb = rgb::apply(
                matrix,
                [
                    table[code(0) as usize],
                    table[code(1) as usize],
                    table[code(2) as usize],
                ],
            );
            let alpha = if channels.has_alpha() {
                f32::from(code(3)) / 65535.
            } else {
                1.
            };
            let bits = hdr::encode_pixel([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, alpha])
                .map_err(str::to_string)?;
            for (b, v) in o.chunks_exact_mut(2).zip(bits) {
                b.copy_from_slice(&v.to_le_bytes());
            }
        }
        builder.push_row(&output)?;
    }
    reader.finish().map_err(err)?;
    builder.finish()
}

/// Rows are unmodified linear-premultiplied artwork in `space`. Mapping to PQ's
/// gamut/range is an explicit delivery choice; strict mode fails before publish.
/// Metadata defines BT.2020 PQ in absolute nits, independently of the monitor.
pub fn write_hdr_png_rows(
    mut output: impl Write,
    extent: [u32; 2],
    space: RgbSpace,
    resolution: Option<layer_core::ImageResolution>,
    map_out_of_range: bool,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<crate::OutputStatistics, String> {
    validate_extent(extent, 32768)?;
    let mut info = png::Info::with_size(extent[0], extent[1]);
    info.bit_depth = png::BitDepth::Sixteen;
    info.color_type = png::ColorType::Rgba;
    if let Some(r) = resolution {
        let [xppu, yppu] = r.png_density()?;
        info.pixel_dims = Some(png::PixelDimensions {
            xppu,
            yppu,
            unit: png::Unit::Meter,
        });
    }
    let mut encoder = png::Encoder::with_info(&mut output, info).map_err(err)?;
    encoder.set_deflate_compression(png::DeflateCompression::Level(1));
    let mut writer = encoder.write_header().map_err(err)?;
    // png 0.18.1 reads cICP but does not serialize this Info field.
    writer
        .write_chunk(png::chunk::ChunkType(*b"cICP"), &[9, 16, 0, 1])
        .map_err(err)?;
    let mut statistics = crate::OutputStatistics::default();
    let to_srgb = space.linear_transform(RgbSpace::Srgb);
    let to_2020 = hdr::srgb_to_bt2020();
    {
        let mut stream = writer.stream_writer().map_err(err)?;
        let mut pixels = vec![[0.; 4]; extent[0] as usize];
        let mut row = vec![0u8; extent[0] as usize * 8];
        for y in 0..extent[1] {
            read(y, &mut pixels)?;
            for (p, o) in pixels.iter().zip(row.chunks_exact_mut(8)) {
                if p.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&p[3]) {
                    return Err("Invalid HDR export samples".into());
                }
                let rgb = if p[3] > 0. {
                    [p[0] / p[3], p[1] / p[3], p[2] / p[3]].map(f64::from)
                } else {
                    [0.; 3]
                };
                let rgb = rgb::apply(to_2020, rgb::apply(to_srgb, rgb))
                    .map(|v| v * f64::from(hdr::REFERENCE_WHITE_NITS));
                for c in 0..3 {
                    let nits = rgb[c];
                    if !nits.is_finite() {
                        return Err("Non-finite HDR output".into());
                    }
                    if !(0. ..=10000.).contains(&nits) {
                        if !map_out_of_range {
                            return Err("Artwork exceeds BT.2020 PQ gamut or 10000 cd/m². Enable explicit PQ range mapping or edit the HDR colors.".into());
                        }
                        statistics.clipped_channels += 1;
                    }
                    let code = (hdr::pq_encode(nits.clamp(0., 10000.)) * 65535.).round() as u16;
                    o[c * 2..c * 2 + 2].copy_from_slice(&code.to_be_bytes());
                }
                o[6..].copy_from_slice(&((p[3] * 65535.).round() as u16).to_be_bytes());
            }
            stream.write_all(&row).map_err(err)?;
        }
        stream.finish().map_err(err)?;
    }
    writer.finish().map_err(err)?;
    output.flush().map_err(err)?;
    Ok(statistics)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pq_png_tags_normalization_alpha_and_explicit_range_mapping() {
        let pixels = [
            [0., 0., 0., 1.],
            [1., 1., 1., 1.],
            [4., 2., 1., 0.5],
            [16., 16., 16., 1.],
            [100., 0., 0., 0.],
        ];
        let mut file = Vec::new();
        write_hdr_png_rows(&mut file, [5, 1], RgbSpace::Srgb, None, false, |_, row| {
            row.copy_from_slice(&pixels);
            Ok(())
        })
        .unwrap();
        let decoded = read_photo(std::io::Cursor::new(&file), DecodeLimits::default()).unwrap();
        assert_eq!(decoded.interpretation.depth, SampleDepth::F16);
        let mut raw = vec![0; decoded.row_bytes()];
        decoded.rows().read(0, &mut raw).unwrap();
        let decoder =
            crate::WorkingDecoder::new(&decoded.interpretation, RgbSpace::Srgb, Default::default())
                .unwrap();
        let mut actual = [[0.; 4]; 5];
        decoder.decode_pixels(&raw, &mut actual).unwrap();
        for (p, a) in pixels.iter().zip(actual) {
            for c in 0..3 {
                let expected = if p[3] > 0. { p[c] / p[3] } else { 0. };
                assert!(
                    (a[c] - expected).abs() < expected.abs() * 0.001 + 0.0001,
                    "{a:?} expected {p:?}"
                );
            }
            assert!((a[3] - p[3]).abs() < 0.0005);
        }
        let info = png::Decoder::new(std::io::Cursor::new(&file))
            .read_info()
            .unwrap();
        let cicp = info.info().coding_independent_code_points.unwrap();
        assert_eq!(
            (
                cicp.color_primaries,
                cicp.transfer_function,
                cicp.matrix_coefficients
            ),
            (9, 16, 0)
        );
        assert!(info.info().icc_profile.is_none());
        let invalid = |_: u32, row: &mut [[f32; 4]]| {
            row[0] = [-1., 100., 0., 1.];
            Ok(())
        };
        assert!(
            write_hdr_png_rows(Vec::new(), [1, 1], RgbSpace::Srgb, None, false, invalid).is_err()
        );
        assert!(
            write_hdr_png_rows(Vec::new(), [1, 1], RgbSpace::Srgb, None, true, invalid)
                .unwrap()
                .clipped_channels
                > 0
        );
        let flag = std::sync::atomic::AtomicBool::new(true);
        assert!(
            read_photo_detailed_with_cancel(
                std::io::Cursor::new(file),
                DecodeLimits::default(),
                &flag
            )
            .is_err()
        );
    }
}
