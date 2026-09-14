//! Bounded output/CMM measurement, independent of GPU capture and GTK export.
use layer_color::{
    WorkingEncoder,
    photo::{write_png_rows, write_tiff_rows},
};
use layer_core::color::{
    ColorProfile, IntegerDepth, RgbSpace,
    source::{SourceChannels, SourceInterpretation},
};
use std::{fs::File, io::BufWriter, time::Instant};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err(
            "sdr_output WIDTH HEIGHT srgb|p3|adobe|prophoto|gray|cmyk OUTPUT.png|tif ramp|noise".into(),
        );
    }
    let extent = [
        args[1].parse::<u32>().map_err(err)?,
        args[2].parse::<u32>().map_err(err)?,
    ];
    if extent.contains(&0) || extent.iter().any(|v| *v > 32768) {
        return Err("Invalid extent".into());
    }
    let (channels, profile) = match args[3].as_str() {
        "srgb" => (SourceChannels::Rgba, ColorProfile::Builtin(RgbSpace::Srgb)),
        "p3" => (
            SourceChannels::Rgba,
            ColorProfile::Builtin(RgbSpace::DisplayP3),
        ),
        "adobe" => (
            SourceChannels::Rgba,
            ColorProfile::Builtin(RgbSpace::AdobeRgb),
        ),
        "prophoto" => (
            SourceChannels::Rgba,
            ColorProfile::Builtin(RgbSpace::ProPhoto),
        ),
        "gray" => (
            SourceChannels::GrayAlpha,
            ColorProfile::Builtin(RgbSpace::ProPhoto),
        ),
        "cmyk" => (
            SourceChannels::Cmyk,
            ColorProfile::Icc(
                std::fs::read(std::env::var("LAYER_TEST_CMYK_PROFILE").map_err(err)?)
                    .map_err(err)?
                    .into(),
            ),
        ),
        _ => return Err("Invalid output space".into()),
    };
    // The final argument selects deterministic ramp/noise data for codec costs.
    let noise = match args[5].as_str() {
        "ramp" => false,
        "noise" => true,
        _ => return Err("Choose ramp or noise".into()),
    };
    let target = SourceInterpretation {
        channels,
        depth: IntegerDepth::U16,
        profile,
        profile_assumed: false,
    };
    let start = Instant::now();
    let encoder = WorkingEncoder::new(RgbSpace::ProPhoto, &target, Default::default())?;
    let lut: Vec<f32> = (0..=65535)
        .map(|v| RgbSpace::ProPhoto.decode(v as f64 / 65535.) as f32)
        .collect();
    let mut working = vec![[0.; 4]; extent[0] as usize];
    println!(
        "prepare_ms={:.3} working_bytes={} decode_lut_bytes={}",
        start.elapsed().as_secs_f64() * 1000.,
        working.len() * 16,
        lut.len() * 4
    );
    let start = Instant::now();
    let mut clipped = 0;
    let mut provider = |y: u32, output: &mut [u8]| {
        for (x, p) in working.iter_mut().enumerate() {
            let x = x as u32;
            let mut codes = [
                x as u64 * 65535 / u64::from(extent[0].max(2) - 1),
                y as u64 * 65535 / u64::from(extent[1].max(2) - 1),
                u64::from((x ^ (y * 17)) & 65535),
            ];
            if noise {
                for (c, code) in codes.iter_mut().enumerate() {
                    let mut n = (x + y * extent[0])
                        .wrapping_mul(0x9e3779b9)
                        .wrapping_add(c as u32 * 977);
                    n = (n ^ (n >> 16)).wrapping_mul(0x85ebca6b);
                    n = (n ^ (n >> 13)).wrapping_mul(0xc2b2ae35);
                    *code = u64::from((n ^ (n >> 16)) & 65535);
                }
            }
            let alpha = match x % 17 {
                0 => 0,
                1 => 1,
                2 => 257,
                _ => 65535,
            };
            *p = [
                lut[codes[0] as usize],
                lut[codes[1] as usize],
                lut[codes[2] as usize],
                alpha as f32 / 65535.,
            ];
        }
        clipped += encoder
            .encode_straight(
                &working,
                output,
                (channels == SourceChannels::Cmyk).then_some([1.; 3]),
            )?
            .clipped_channels;
        Ok(())
    };
    let file = BufWriter::new(File::create(&args[4]).map_err(err)?);
    if args[4].ends_with(".png") {
        write_png_rows(file, extent, encoder.interpretation(), &mut provider)?;
    } else {
        write_tiff_rows(file, extent, encoder.interpretation(), &mut provider)?;
    }
    println!(
        "extent={extent:?} target={} noise={noise} output_ms={:.3} file_bytes={} clipped_channels={clipped}",
        args[3],
        start.elapsed().as_secs_f64() * 1000.,
        std::fs::metadata(&args[4]).map_err(err)?.len()
    );
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|line| line.starts_with("VmHWM:") || line.starts_with("VmRSS:"))
        {
            println!("{line}");
        }
    }
    Ok(())
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
