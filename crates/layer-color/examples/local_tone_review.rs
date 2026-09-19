//! Reproducible HDR review fixture importer and CPU local-tone comparison.
//! This example reads Poly Haven Radiance RGBE originals; it does not add a
//! production format reader. Original values, applied exposure and underflow
//! counts are reported. RGB primaries are assumed linear Rec.709 / D65.
use layer_core::{
    Document, Project,
    color::{
        ColorProfile, RgbSpace, SampleDepth,
        hdr::{self, SdrRendition},
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
    },
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read},
    path::Path,
    sync::Arc,
    time::Instant,
};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("local_tone_review INPUT.hdr OUTPUT_DIRECTORY".into());
    }
    let input = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    std::fs::create_dir_all(out).map_err(err)?;
    let name = input.file_stem().unwrap().to_str().unwrap();
    let (extent, mut pixels) = read_rgbe(input)?;
    let original_peak = pixels
        .iter()
        .flat_map(|p| &p[..3])
        .copied()
        .fold(0f32, f32::max);
    let exposure = -(original_peak / hdr::MAX_LINEAR).log2().ceil().max(0.);
    let gain = exposure.exp2();
    let mut underflow = 0u64;
    let mut rounded = 0u64;
    let mut error = 0f32;
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::F16,
        profile: ColorProfile::Builtin(RgbSpace::Srgb),
        profile_assumed: true,
    };
    let mut source = SourceBuilder::new(extent, interpretation, 512 * 1024 * 1024)?;
    for row in pixels.chunks_exact_mut(extent[0] as usize) {
        let mut bytes = Vec::with_capacity(row.len() * 8);
        for p in row {
            for c in 0..3 {
                p[c] *= gain;
            }
            let bits = hdr::encode_pixel(*p).map_err(str::to_string)?;
            let decoded = hdr::decode_pixel(bits).map_err(str::to_string)?;
            for c in 0..3 {
                if p[c] != decoded[c] {
                    rounded += 1;
                    error = error.max((p[c] - decoded[c]).abs());
                }
                if p[c] != 0. && decoded[c] == 0. {
                    underflow += 1;
                }
            }
            *p = decoded;
            for b in bits {
                bytes.extend_from_slice(&b.to_le_bytes());
            }
        }
        source.push_row(&bytes)?;
    }
    let mut document = Document::new(name, extent[0], extent[1]);
    document.color.depth = SampleDepth::F16;
    document.layers[1].visible = false;
    document.layers[0].name = format!("Poly Haven · {name}").into();
    document.layers[0].source = Some(Arc::new(source.finish()?));
    let start = Instant::now();
    let guide = layer_color::build_local_tone_guide(
        extent,
        RgbSpace::Srgb,
        || false,
        |y, row| {
            let start = y as usize * extent[0] as usize;
            row.copy_from_slice(&pixels[start..start + extent[0] as usize]);
            Ok(())
        },
    )?;
    let analysis_ms = start.elapsed().as_secs_f64() * 1000.;
    document.sdr_rendition.headroom = guide.peak.log2().max(0.);
    let project = Project::snapshot(&document, &BTreeMap::new())?;
    project.write(BufWriter::new(
        File::create(out.join(format!("{name}.capy"))).map_err(err)?,
    ))?;
    let target = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U16,
        profile: ColorProfile::Builtin(RgbSpace::Srgb),
        profile_assumed: false,
    };
    for (label, recipe) in [
        (
            "low-contrast",
            SdrRendition {
                headroom: document.sdr_rendition.headroom,
                contrast:0.5,
                ..Default::default()
            },
        ),
        ("local", document.sdr_rendition),
        (
            "soft",
            SdrRendition {
                balance: -1.,
                ..document.sdr_rendition
            },
        ),
        (
            "detail",
            SdrRendition {
                balance: 1.,
                ..document.sdr_rendition
            },
        ),
        (
            "open",
            SdrRendition {
                contrast: 2.,
                ..document.sdr_rendition
            },
        ),
    ] {
        let output =
            BufWriter::new(File::create(out.join(format!("{name}-{label}.png"))).map_err(err)?);
        let size = [1024, 512];
        layer_color::encode_working_rows_with_guide(
            RgbSpace::Srgb,
            extent,
            size,
            &target,
            Default::default(),
            None,
            Some(recipe),
            Some(&guide),
            |y, row| {
                let start = y as usize * extent[0] as usize;
                row.copy_from_slice(&pixels[start..start + extent[0] as usize]);
                Ok(())
            },
            |e, t, read| layer_color::photo::write_png_rows(output, e, t, None, read),
        )?;
    }
    println!(
        "{name}: {}x{} source_peak={original_peak} exposure_ev={exposure} stored_peak={} headroom_ev={} f16_rounded_channels={rounded} f16_underflow_channels={underflow} f16_max_error={error} guide={}x{} guide_bytes={} analysis_ms={analysis_ms:.2}",
        extent[0],
        extent[1],
        original_peak * gain,
        document.sdr_rendition.headroom,
        guide.extent[0],
        guide.extent[1],
        guide.byte_len()
    );
    Ok(())
}
fn read_rgbe(path: &Path) -> Result<([u32; 2], Vec<[f32; 4]>), String> {
    let mut input = BufReader::new(File::open(path).map_err(err)?);
    let mut line = String::new();
    input.read_line(&mut line).map_err(err)?;
    if !line.starts_with("#?RADIANCE") && !line.starts_with("#?RGBE") {
        return Err("Expected Radiance RGBE".into());
    }
    loop {
        line.clear();
        input.read_line(&mut line).map_err(err)?;
        if line.trim().is_empty() {
            break;
        }
        if line.starts_with("EXPOSURE=") || line.starts_with("COLORCORR=") {
            return Err(
                "Fixture requires explicit Radiance exposure/color correction handling".into(),
            );
        }
    }
    line.clear();
    input.read_line(&mut line).map_err(err)?;
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() != 4 || parts[0] != "-Y" || parts[2] != "+X" {
        return Err("Unsupported fixture orientation".into());
    }
    let h: u32 = parts[1].parse().map_err(err)?;
    let w: u32 = parts[3].parse().map_err(err)?;
    if w > 16384 || h > 16384 || w < 8 || h == 0 {
        return Err("Invalid fixture dimensions".into());
    }
    let mut pixels = Vec::with_capacity((w * h) as usize);
    let mut row = vec![0u8; w as usize * 4];
    for _ in 0..h {
        let mut header = [0u8; 4];
        input.read_exact(&mut header).map_err(err)?;
        if header != [2, 2, (w >> 8) as u8, w as u8] {
            return Err("Unsupported fixture RLE".into());
        }
        for c in 0..4 {
            let mut x = 0usize;
            while x < w as usize {
                let mut count = [0];
                input.read_exact(&mut count).map_err(err)?;
                let n = if count[0] > 128 {
                    (count[0] - 128) as usize
                } else {
                    count[0] as usize
                };
                if n == 0 || x + n > w as usize {
                    return Err("Invalid RGBE run".into());
                }
                let output = &mut row[c * w as usize + x..c * w as usize + x + n];
                if count[0] > 128 {
                    let mut value = [0];
                    input.read_exact(&mut value).map_err(err)?;
                    output.fill(value[0]);
                } else {
                    input.read_exact(output).map_err(err)?;
                }
                x += n;
            }
        }
        for x in 0..w as usize {
            let e = row[3 * w as usize + x];
            let scale = if e == 0 {
                0.
            } else {
                2f32.powi(e as i32 - 136)
            };
            pixels.push([
                row[x] as f32 * scale,
                row[w as usize + x] as f32 * scale,
                row[2 * w as usize + x] as f32 * scale,
                1.,
            ]);
        }
    }
    Ok(([w, h], pixels))
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
