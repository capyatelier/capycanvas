//! Source-only measurement/interchange runner. This does not qualify editing.
use layer_color::photo::{DecodeLimits, read_photo, write_png, write_tiff};
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace, source::*};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::Path,
    sync::Arc,
    time::Instant,
};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 {
        return Err(
            "photo_sources generate WIDTH HEIGHT OUTPUT | roundtrip INPUT OUTPUT | inspect INPUT | cancel INPUT DELAY_MS"
                .into(),
        );
    }
    if args[1] == "cancel" { return cancel(&args); }
    let start = Instant::now();
    let source = match args[1].as_str() {
        "generate" | "generate_noise" if args.len() == 5 => {
            let w: u32 = args[2].parse().map_err(|_| "Invalid width")?;
            let h: u32 = args[3].parse().map_err(|_| "Invalid height")?;
            let profile = ColorProfile::Icc(
                layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::ProPhoto))?.into(),
            );
            let mut builder = SourceBuilder::new(
                [w, h],
                SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth: SampleDepth::U16,
                    profile,
                    profile_assumed: false,
                },
                512 * 1024 * 1024,
            )?;
            for y in 0..h {
                let mut row = Vec::with_capacity(w as usize * 8);
                for x in 0..w {
                    let noise = x
                        .wrapping_mul(1664525)
                        .wrapping_add(y.wrapping_mul(1013904223));
                    let alpha = match x % 17 {
                        0 => 0,
                        1 => 1,
                        2 => 257,
                        _ => 65535,
                    };
                    let mut values = [
                        ((x as u64 * 65535) / u64::from(w.max(2) - 1)) as u16,
                        ((y as u64 * 65535) / u64::from(h.max(2) - 1)) as u16,
                        (noise >> 8) as u16,
                        alpha,
                    ];
                    if args[1] == "generate_noise" {
                        for (channel, value) in values.iter_mut().take(3).enumerate() {
                            let mut n = (x + y * w)
                                .wrapping_mul(0x9e3779b9)
                                .wrapping_add(channel as u32 * 977);
                            n = (n ^ (n >> 16)).wrapping_mul(0x85ebca6b);
                            n = (n ^ (n >> 13)).wrapping_mul(0xc2b2ae35);
                            *value = (n ^ (n >> 16)) as u16;
                        }
                    }
                    for value in values {
                        row.extend_from_slice(&value.to_le_bytes());
                    }
                }
                builder.push_row(&row)?;
            }
            builder.finish()?
        }
        "roundtrip" | "inspect" => read_photo(
            BufReader::new(File::open(&args[2]).map_err(err)?),
            DecodeLimits::default(),
        )?,
        _ => return Err("Invalid source workload arguments".into()),
    };
    println!(
        "decode/generate {:.2} ms; {:?} {:?} {:?}; compressed source {:.2} MiB; decoded row-band bound {:.2} MiB",
        start.elapsed().as_secs_f64() * 1000.,
        source.extent,
        source.interpretation.channels,
        source.interpretation.depth,
        source.resident_bytes() as f64 / 1048576.,
        source.extent[0].div_ceil(256) as f64
            * 256.
            * 256.
            * source.interpretation.pixel_bytes() as f64
            / 1048576.
    );
    memory();
    if args[1] == "inspect" {
        println!("source profile assumed: {}; density: {:?}", source.interpretation.profile_assumed, source.resolution);
        return Ok(());
    }
    let source = Arc::new(source);
    let output = args.last().unwrap();
    let start = Instant::now();
    let mut file = BufWriter::new(File::create(output).map_err(err)?);
    let extension = Path::new(output).extension().and_then(|s| s.to_str());
    match extension {
        Some("png") => write_png(file, &source)?,
        Some("tif" | "tiff") => write_tiff(file, &source)?,
        Some("capy") => {
            let mut document = layer_core::Document::new(
                "source archive measurement",
                source.extent[0],
                source.extent[1],
            );
            document.layers[0].source = Some(source.clone());
            layer_core::Project {
                document,
                assets: Default::default(),
            }
            .write(&mut file)?;
            file.flush().map_err(err)?;
        }
        _ => return Err("Choose a PNG, TIFF or Capy output".into()),
    }
    println!(
        "streamed output {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.
    );
    let start = Instant::now();
    let input = BufReader::new(File::open(output).map_err(err)?);
    let reopened = if extension == Some("capy") {
        let project = layer_core::Project::read(input, Default::default())?;
        project.document.layers[0]
            .source
            .clone()
            .ok_or("Missing archived source")?
    } else {
        Arc::new(read_photo(input, DecodeLimits::default())?)
    };
    if source.extent != reopened.extent
        || source.interpretation.depth != reopened.interpretation.depth
        || source.tiles.len() != reopened.tiles.len()
        || source.tiles.iter().any(|(key, tile)| {
            reopened
                .tiles
                .get(key)
                .is_none_or(|t| t.digest != tile.digest)
        })
    {
        return Err("Identity source samples changed".into());
    }
    if matches!(source.interpretation.profile, ColorProfile::Icc(_))
        && source.interpretation.profile != reopened.interpretation.profile
    {
        return Err("Embedded profile bytes changed".into());
    }
    println!(
        "reopen + exact source comparison {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.
    );
    memory();
    Ok(())
}

// A real file load with cancellation requested while its worker is active.
// This reports acknowledgement latency, not the internal codec phase.
fn cancel(args: &[String]) -> Result<(), String> {
    use std::sync::{atomic::{AtomicBool, Ordering}, mpsc};
    use std::time::Duration;
    if args.len() != 4 { return Err("cancel requires INPUT DELAY_MS".into()); }
    let delay: u64 = args[3].parse().map_err(|_| "Invalid cancellation delay")?;
    if delay > 60_000 { return Err("Cancellation delay exceeds 60 seconds".into()); }
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancelled.clone();
    let (done, wait) = mpsc::channel();
    let start = Instant::now();
    let request = std::thread::spawn(move || {
        if wait.recv_timeout(Duration::from_millis(delay)).is_err() {
            let at = Instant::now();
            worker_cancel.store(true, Ordering::Release);
            Some(at)
        } else { None }
    });
    let result = layer_color::photo::read_photo_detailed_with_cancel(
        BufReader::new(File::open(&args[2]).map_err(err)?), Default::default(), &cancelled);
    let finished = Instant::now();
    let _ = done.send(());
    let requested = request.join().map_err(|_| "Cancellation worker failed")?;
    let message = result.err().ok_or("Image completed before cancellation; increase input size or shorten delay")?;
    if !message.contains("cancelled") { return Err(message); }
    let requested = requested.ok_or("Image failed before cancellation")?;
    println!("cancel requested {:.2} ms; acknowledged {:.2} ms later; total {:.2} ms; {message}",
        requested.duration_since(start).as_secs_f64() * 1000.,
        finished.saturating_duration_since(requested).as_secs_f64() * 1000.,
        finished.duration_since(start).as_secs_f64() * 1000.);
    memory();
    Ok(())
}
fn memory() {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS"))
        {
            println!("{line}");
        }
    }
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
