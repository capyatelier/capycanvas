use super::*;

pub fn read_png(
    mut input: impl BufRead + Seek,
    limits: DecodeLimits,
) -> Result<SourceImage, String> {
    let has_icc = profile_chunk_present(&mut input, limits)?;
    let mut decoder = png::Decoder::new_with_limits(
        input,
        png::Limits {
            bytes: limits.codec_bytes,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(err)?;
    let info = reader.info();
    // png 0.18.1 intentionally discards errors in iCCP decompression. An invalid
    // declared profile cannot become our ordinary untagged-sRGB assumption.
    if has_icc && info.icc_profile.is_none() && info.coding_independent_code_points.is_none() {
        return Err("The PNG declares an unreadable ICC profile".into());
    }
    let extent = [info.width, info.height];
    let orientation = info
        .exif_metadata
        .as_deref()
        .map(super::orientation::exif)
        .transpose()?
        .unwrap_or(1);
    limits.extent(extent)?;
    if info.animation_control.is_some() {
        return Err("Animated PNG is not a still-photo source".into());
    }
    let (color, depth) = reader.output_color_type();
    let channels = match color {
        png::ColorType::Grayscale => SourceChannels::Gray,
        png::ColorType::GrayscaleAlpha => SourceChannels::GrayAlpha,
        png::ColorType::Rgb => SourceChannels::Rgb,
        png::ColorType::Rgba => SourceChannels::Rgba,
        _ => return Err("PNG palette expansion failed".into()),
    };
    let depth = match depth {
        png::BitDepth::Eight => IntegerDepth::U8,
        png::BitDepth::Sixteen => IntegerDepth::U16,
        _ => return Err("Unsupported PNG sample depth".into()),
    };
    let (profile, assumed) = interpretation_from_tags(info)?;
    check_channels(channels, &profile)?;
    let interpretation = SourceInterpretation {
        channels,
        depth,
        profile,
        profile_assumed: assumed,
    };
    let row_bytes = extent[0] as usize * interpretation.pixel_bytes();
    let mut builder = SourceBuilder::new(extent, interpretation, limits.source_bytes)?;
    if info.interlaced {
        // The pinned decoder's Adam7 convenience path requires a full output.
        // Keep this explicit/bounded until a tiled Adam7 spool is implemented.
        let size = reader
            .output_buffer_size()
            .filter(|n| *n <= limits.codec_bytes)
            .ok_or("Interlaced PNG exceeds the decoded-image budget")?;
        let mut frame = vec![0; size];
        reader.next_frame(&mut frame).map_err(err)?;
        for row in frame.chunks_exact_mut(row_bytes) {
            if depth == IntegerDepth::U16 {
                swap_u16(row);
            }
            builder.push_row(row)?;
        }
    } else {
        let mut converted = Vec::new();
        while let Some(row) = reader.next_row().map_err(err)? {
            if depth == IntegerDepth::U16 {
                converted.clear();
                converted.extend_from_slice(row.data());
                swap_u16(&mut converted);
                builder.push_row(&converted)?;
            } else {
                builder.push_row(row.data())?;
            }
        }
    }
    reader.finish().map_err(err)?;
    super::orientation::normalize(builder.finish()?, orientation, limits.source_bytes)
}

fn profile_chunk_present(
    input: &mut (impl Read + Seek),
    limits: DecodeLimits,
) -> Result<bool, String> {
    let origin = input.stream_position().map_err(err)?;
    let mut signature = [0; 8];
    input.read_exact(&mut signature).map_err(err)?;
    if signature != *b"\x89PNG\r\n\x1a\n" {
        return Err("Invalid PNG signature".into());
    }
    let mut found = false;
    let mut scanned = 0u64;
    loop {
        let mut header = [0; 8];
        input.read_exact(&mut header).map_err(err)?;
        if matches!(&header[4..], b"IDAT" | b"IEND") {
            break;
        }
        let size = u64::from(u32::from_be_bytes(header[..4].try_into().unwrap()));
        scanned = scanned
            .checked_add(size + 12)
            .filter(|n| *n <= limits.codec_bytes as u64)
            .ok_or("PNG metadata exceeds the codec budget")?;
        found |= &header[4..] == b"iCCP";
        input
            .seek(std::io::SeekFrom::Current((size + 4) as i64))
            .map_err(err)?;
    }
    input.seek(std::io::SeekFrom::Start(origin)).map_err(err)?;
    Ok(found)
}

fn interpretation_from_tags(info: &png::Info<'_>) -> Result<(ColorProfile, bool), String> {
    // PNG3 precedence: cICP > iCCP > sRGB > cHRM/gAMA. An unsupported explicit
    // interpretation is an error, never an untagged-sRGB fallback.
    if let Some(cicp) = info.coding_independent_code_points {
        if cicp.matrix_coefficients != 0 || !cicp.is_video_full_range_image {
            return Err("PNG matrix/range encoding is not supported for SDR editing".into());
        }
        if matches!(cicp.transfer_function, 16 | 18) {
            return Err("This PNG uses HDR transfer; SDR import would discard its range".into());
        }
        let space = match (cicp.color_primaries, cicp.transfer_function) {
            (1, 13) => RgbSpace::Srgb,
            (12, 13) => RgbSpace::DisplayP3,
            _ => return Err("This PNG cICP color encoding is not supported".into()),
        };
        return Ok((ColorProfile::Builtin(space), false));
    }
    if let Some(icc) = &info.icc_profile {
        let profile = ColorProfile::Icc(icc.to_vec().into());
        profile_channels(&profile)?;
        return Ok((profile, false));
    }
    if info.srgb.is_some() {
        return Ok((ColorProfile::default(), false));
    }
    if info.gama_chunk.is_some() || info.chrm_chunk.is_some() {
        let convert_xy = |(x, y): (png::ScaledFloat, png::ScaledFloat)| {
            [
                f64::from(x.into_scaled()) / 100000.,
                f64::from(y.into_scaled()) / 100000.,
            ]
        };
        let (white, primaries) = info
            .chrm_chunk
            .map(|c| {
                (
                    convert_xy(c.white),
                    [convert_xy(c.red), convert_xy(c.green), convert_xy(c.blue)],
                )
            })
            .unwrap_or((RgbSpace::Srgb.white(), RgbSpace::Srgb.primaries()));
        let gamma = info
            .gama_chunk
            .map(|g| f64::from(g.into_scaled()) / 100000.);
        let gray = matches!(
            info.color_type,
            png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha
        );
        return Ok((
            crate::icc::matrix_profile(white, primaries, gamma, gray)?,
            false,
        ));
    }
    Ok((ColorProfile::default(), true))
}

/// Identity source delivery. Profile transforms/depth conversion create a
/// separate row provider; this function cannot mutate the retained source.
pub fn write_png(mut output: impl Write, source: &SourceImage) -> Result<(), String> {
    source.validate()?;
    let interpretation = &source.interpretation;
    check_channels(interpretation.channels, &interpretation.profile)?;
    let mut info = png::Info::with_size(source.extent[0], source.extent[1]);
    info.bit_depth = match interpretation.depth {
        IntegerDepth::U8 => png::BitDepth::Eight,
        IntegerDepth::U16 => png::BitDepth::Sixteen,
    };
    info.color_type = match interpretation.channels {
        SourceChannels::Gray => png::ColorType::Grayscale,
        SourceChannels::GrayAlpha => png::ColorType::GrayscaleAlpha,
        SourceChannels::Rgb => png::ColorType::Rgb,
        SourceChannels::Rgba => png::ColorType::Rgba,
        SourceChannels::Cmyk => {
            return Err("PNG delivery requires RGB or grayscale conversion".into());
        }
    };
    if interpretation.profile == ColorProfile::default() {
        info.srgb = Some(png::SrgbRenderingIntent::RelativeColorimetric);
    } else {
        info.icc_profile = Some(profile_bytes(&interpretation.profile)?.into());
    }
    let encoder = png::Encoder::with_info(&mut output, info).map_err(err)?;
    let mut writer = encoder.write_header().map_err(err)?;
    {
        let mut stream = writer.stream_writer().map_err(err)?;
        let mut source_rows = source.rows();
        let mut row = vec![0; source.row_bytes()];
        for y in 0..source.extent[1] {
            source_rows.read(y, &mut row)?;
            if interpretation.depth == IntegerDepth::U16 {
                swap_u16(&mut row);
            }
            stream.write_all(&row).map_err(err)?;
        }
        stream.finish().map_err(err)?;
    }
    writer.finish().map_err(err)?;
    output.flush().map_err(err)
}
