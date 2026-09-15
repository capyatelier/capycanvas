//! Dense raster, concurrent-save and multiple-document qualification.
//! Run in release on a physical GPU. Synthetic images are reproducible workloads,
//! not claims of photographic color accuracy. Export/undo timings are cold paths.
use layer_core::*;
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace, source::*};
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::{CanvasRenderer, ViewState};
use layer_render_wgpu::WgpuRasterizer;
use std::{
    collections::BTreeMap,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

type Engine = CanvasEngine<WgpuRasterizer>;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Canvas {
    engine: Engine,
    input: InputProducer<PenEvent>,
    sequence: u64,
    source_frames: [Vec<(f64, f64, f64)>; 2],
}
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}
fn thread_cpu_ms() -> f64 {
    #[cfg(target_os = "linux")]
    {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // A valid writable timespec and the current thread's CPU clock.
        assert_eq!(
            unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) },
            0
        );
        time.tv_sec as f64 * 1000. + time.tv_nsec as f64 / 1_000_000.
    }
    #[cfg(not(target_os = "linux"))]
    {
        f64::NAN
    }
}
fn quantiles(values: &mut [f64]) -> [f64; 3] {
    values.sort_by(f64::total_cmp);
    [0.5, 0.95, 0.99].map(|q| values[((values.len() - 1) as f64 * q).ceil() as usize])
}
impl Canvas {
    fn new(extent: [u32; 2], name: &str, color: DocumentColor) -> Result<Self> {
        let start = Instant::now();
        let mut document = Document::new(name, extent[0], extent[1]);
        document.color = color;
        let mut builder = SourceBuilder::new(
            extent,
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth: color.depth,
                profile: ColorProfile::Builtin(color.space),
                profile_assumed: false,
            },
            512 * 1024 * 1024,
        )?;
        // Generate one row at a time, with real low-order U16 values. A full
        // temporary photograph would inflate the workload's source ownership.
        let mut random = 0x1357abcdu32;
        let mut row = Vec::with_capacity(extent[0] as usize * color.depth.bytes() * 4);
        for y in 0..extent[1] {
            row.clear();
            for x in 0..extent[0] {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                let noise = (random % 1024) as u16;
                for code in [
                    (u64::from(x) * 55000 / u64::from(extent[0])) as u16 + noise,
                    (u64::from(y) * 55000 / u64::from(extent[1])) as u16 + noise,
                    ((u64::from(x) + u64::from(y)) * 13 % 60000) as u16 + noise,
                    65535,
                ] {
                    match color.depth {
                        IntegerDepth::U8 => row.push((code >> 8) as u8),
                        IntegerDepth::U16 => row.extend_from_slice(&code.to_le_bytes()),
                    }
                }
            }
            builder.push_row(&row)?;
        }
        document.layers[0].source = Some(std::sync::Arc::new(builder.finish()?));
        for _ in 0..31 {
            let id = document.allocate_layer_id();
            document.layers.insert(1, Layer::paint(id, "empty"));
        }
        let mut gpu = WgpuRasterizer::new_native_headless(color)?;
        gpu.set_telemetry_enabled(true);
        let (input, consumer) = input_queue(64);
        let scale = (1024. / extent[0] as f32).min(768. / extent[1] as f32);
        let mut engine = CanvasEngine::new(
            gpu,
            document,
            consumer,
            ViewState {
                width_px: 1024,
                height_px: 768,
                document_to_surface: [scale, 0., 0., scale, 0., 0.],
                background_rgba_linear: [0.; 4],
            },
            ViewTransform {
                revision: 0,
                surface_to_document: [1. / scale, 0., 0., 1. / scale, 0., 0.],
            },
        )?;
        engine.render_frame()?;
        engine.backend_mut().wait_idle()?;
        let submitted = ms(start);
        let mut canvas = Self {
            engine,
            input,
            sequence: 0,
            source_frames: Default::default(),
        };
        canvas.settle()?;
        println!(
            "{name} {extent:?}: source generation + device + initial submission {submitted:.2} ms; initial host-backed {0:.2} ms",
            ms(start)
        );
        Ok(canvas)
    }
    fn settle(&mut self) -> Result<()> {
        let until = Instant::now() + Duration::from_secs(30);
        while !self.engine.backend().raster_ready() {
            if Instant::now() >= until {
                return Err("Raster backing deadline exceeded".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        for layer in &self.engine.document().layers {
            layer.raster.wait_data()?.validate(
                [self.engine.document().width, self.engine.document().height],
                false,
                self.engine.document().color,
            )?;
        }
        Ok(())
    }
    fn snapshot(&self) -> Result<Project> {
        Ok(Project::snapshot(self.engine.document(), &BTreeMap::new())?)
    }
    fn stroke(&mut self, ordinal: u64) -> Result<(Vec<f64>, Vec<f64>)> {
        let mut brush = default_brush(DefaultBrushPreset::GPen);
        brush.diameter = 96.;
        brush.color_rgba_linear = [0.8, 0.04, 0.2, 0.5];
        self.engine.set_brush(brush)?;
        let mut cpu = Vec::new();
        let mut complete = Vec::new();
        for frame in 0..64 {
            let before = self.engine.backend().metrics();
            let start = Instant::now();
            for j in 0..8 {
                let sample = frame * 8 + j;
                self.sequence += 1;
                self.input
                    .push(PenEvent {
                        device_id: 1,
                        sequence: self.sequence,
                        timestamp_ns: self.sequence * 1_000_000,
                        view_revision: 0,
                        surface_position: Point {
                            x: 80. + sample as f32 / 511. * 850.,
                            y: 160. + ordinal as f32 * 65. + (sample as f32 * 0.025).sin() * 35.,
                        },
                        pressure: 0.6,
                        tilt_radians: [0.; 2],
                        twist_radians: 0.,
                        distance: 0.,
                        tool: ToolKind::Pen,
                        flags: SampleFlags::PRIMARY,
                        phase: if sample == 0 {
                            PenPhase::Down
                        } else if sample == 511 {
                            PenPhase::Up
                        } else {
                            PenPhase::Move
                        },
                    })
                    .map_err(|_| "Input queue overflow")?;
            }
            let mut frame_cpu = 0.;
            let mut frame_thread_cpu = 0.;
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let thread = thread_cpu_ms();
                let submitted = Instant::now();
                self.engine.render_frame()?;
                frame_cpu += ms(submitted);
                frame_thread_cpu += thread_cpu_ms() - thread;
                self.engine.backend_mut().wait_idle()?;
                if !self.engine.has_pending_input() {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("Drawing input did not drain".into());
                }
                std::thread::yield_now();
            }
            let completed = ms(start);
            let after = self.engine.backend().metrics();
            let misses = after.source_tile_misses - before.source_tile_misses;
            self.source_frames[usize::from(misses != 0)].push((
                frame_cpu,
                completed,
                frame_thread_cpu,
            ));
            if frame_cpu > 8.33 || completed > 8.33 {
                println!(
                    "outlier {:?} stroke={ordinal} frame={frame} CPU-wall={frame_cpu:.3} thread-CPU={frame_thread_cpu:.3} completed={completed:.3} ms misses={misses} capacity-waits={} capture-reserved={} bytes",
                    self.engine.document().id,
                    after.source_upload_submissions - before.source_upload_submissions,
                    after.raster_backing_reserved_bytes
                );
            }
            cpu.push(frame_cpu);
            complete.push(completed);
        }
        Ok((cpu, complete))
    }
    fn export(&mut self) -> Result<Vec<u8>> {
        let doc = self.engine.document();
        let stride = doc.width as usize * 4;
        let mut bytes = vec![0; stride * doc.height as usize];
        self.engine
            .backend_mut()
            .copy_rgba8_srgb(&mut bytes, stride)?;
        Ok(bytes)
    }
    fn undo_redo(&mut self) -> Result<()> {
        self.settle()?;
        let expected = crc32fast::hash(&self.export()?);
        for redo in [false, true] {
            let start = Instant::now();
            assert!(if redo {
                self.engine.redo()?
            } else {
                self.engine.undo()?
            });
            let previous = self.engine.metrics().frames;
            while self.engine.metrics().frames == previous {
                self.engine.render_frame()?;
                self.engine.backend_mut().wait_idle()?;
            }
            println!(
                "{} completed {:.2} ms",
                if redo { "redo" } else { "undo" },
                ms(start)
            );
        }
        let start = Instant::now();
        assert_eq!(crc32fast::hash(&self.export()?), expected);
        println!(
            "full export + checksum {:.2} ms; redo checksum {expected:08x}",
            ms(start)
        );
        Ok(())
    }
    fn memory(&self) -> Result<()> {
        for (kind, samples) in ["warm source", "cold source"]
            .into_iter()
            .zip(&self.source_frames)
        {
            if samples.is_empty() {
                continue;
            }
            let mut cpu: Vec<_> = samples.iter().map(|s| s.0).collect();
            let mut completed: Vec<_> = samples.iter().map(|s| s.1).collect();
            let mut thread: Vec<_> = samples.iter().map(|s| s.2).collect();
            println!(
                "{kind}: {} frames; CPU p50/p95/p99 {:?} ms; completed {:?} ms; CPU/completed over 8.33 ms {}/{}",
                samples.len(),
                quantiles(&mut cpu),
                quantiles(&mut completed),
                cpu.iter().filter(|v| **v > 8.33).count(),
                completed.iter().filter(|v| **v > 8.33).count()
            );
            println!(
                "{kind} thread CPU p50/p95/p99 {:?} ms",
                quantiles(&mut thread)
            );
        }
        let metrics = self.engine.backend().metrics();
        println!(
            "source upload staging/scratch peak {:.2} MiB; bounded upload submissions {}",
            metrics.source_upload_peak_bytes as f64 / 1048576.,
            metrics.source_upload_submissions,
        );
        match self.engine.backend().device().generate_allocator_report() {
            Some(report) => println!(
                "GPU allocator live {:.2} MiB; reserved {:.2} MiB (includes staging; excludes driver-private allocations)",
                report.total_allocated_bytes as f64 / 1048576.,
                report.total_reserved_bytes as f64 / 1048576.,
            ),
            None => println!("GPU allocator report unavailable on this backend"),
        }
        let backed: usize = self
            .engine
            .document()
            .layers
            .iter()
            .map(|l| l.raster.wait_data().unwrap().resident_bytes())
            .sum();
        println!(
            "renderer allocated/reserved {:.2} MiB; current compressed tiles {:.2} MiB; source {:.2} MiB",
            self.engine.backend().telemetry().resident_bytes as f64 / 1048576.,
            backed as f64 / 1048576.,
            self.engine.document().layers.iter()
                .filter_map(|l| l.source.as_ref())
                .map(|s| s.resident_bytes()).sum::<usize>() as f64
                / 1048576.
        );
        for line in std::fs::read_to_string("/proc/self/status")?
            .lines()
            .filter(|l| l.starts_with("VmRSS:") || l.starts_with("VmHWM:"))
        {
            println!("{line}");
        }
        Ok(())
    }
}
fn save(
    project: Project,
    path: PathBuf,
) -> std::thread::JoinHandle<std::result::Result<(f64, u64), String>> {
    std::thread::spawn(move || {
        let start = Instant::now();
        let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        let mut out = BufWriter::new(file);
        project.write(&mut out)?;
        out.flush().map_err(|e| e.to_string())?;
        out.get_ref().sync_all().map_err(|e| e.to_string())?;
        Ok((
            ms(start),
            std::fs::metadata(path).map_err(|e| e.to_string())?.len(),
        ))
    })
}
fn compare_saved(path: &Path, snapshot: &Project) -> Result<()> {
    let start = Instant::now();
    let reopened = Project::read(
        BufReader::new(std::fs::File::open(path)?),
        ProjectLimits::default(),
    )?;
    assert_eq!(reopened.document.color, snapshot.document.color);
    for (before, after) in snapshot
        .document
        .layers
        .iter()
        .zip(&reopened.document.layers)
    {
        assert!(
            before.source == after.source,
            "Original source samples/profile changed"
        );
        let before = before.raster.wait_data()?;
        let after = after.raster.wait_data()?;
        assert_eq!(before.tiles.len(), after.tiles.len());
        for (key, tile) in &before.tiles {
            assert_eq!(
                tile.wait_backing()?.digest,
                after.tiles[key].wait_backing()?.digest
            );
        }
    }
    println!(
        "archive reopen + exact tile digest comparison {:.2} ms",
        ms(start)
    );
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut selected = "all";
    let mut color = DocumentColor { space: RgbSpace::ProPhoto, depth: IntegerDepth::U16 };
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "all" | "24mp" | "45mp" | "60mp" | "multiple" => selected = argument,
            "--space" => color.space = match arguments.next().map(String::as_str) {
                Some("srgb") => RgbSpace::Srgb,
                Some("p3") => RgbSpace::DisplayP3,
                Some("adobe-rgb") => RgbSpace::AdobeRgb,
                Some("prophoto") => RgbSpace::ProPhoto,
                _ => return Err("--space needs srgb, p3, adobe-rgb or prophoto".into()),
            },
            "--depth" => color.depth = match arguments.next().map(String::as_str) {
                Some("8") => IntegerDepth::U8,
                Some("16") => IntegerDepth::U16,
                _ => return Err("--depth needs 8 or 16".into()),
            },
            _ => return Err("raster_workloads [all|24mp|45mp|60mp|multiple] [--space srgb|p3|adobe-rgb|prophoto] [--depth 8|16]".into()),
        }
    }
    println!("Source ownership: tiled copy-on-write; native {color:?}, Float32 working tiles");
    let output = PathBuf::from("artifacts/color-m2/final-performance/dense");
    std::fs::create_dir_all(&output)?;
    for (name, extent) in [
        ("24mp", [6000, 4000]),
        ("45mp", [8192, 5504]),
        ("60mp", [8192, 7324]),
    ] {
        if selected != "all" && selected != name {
            continue;
        }
        let mut canvas = Canvas::new(extent, name, color)?;
        canvas.stroke(0)?;
        canvas.source_frames = Default::default();
        canvas.settle()?;
        let snapshot = canvas.snapshot()?;
        let path = output.join(format!("{name}.capy"));
        let worker = save(snapshot.clone(), path.clone());
        let (mut cpu, mut complete) = (Vec::new(), Vec::new());
        for ordinal in 1..5 {
            let (a, b) = canvas.stroke(ordinal)?;
            cpu.extend(a);
            complete.extend(b);
        }
        let (saved, size) = worker.join().unwrap()?;
        println!(
            "{name} 256 drawing frames concurrent with save: CPU p50/p95/p99 {:?} ms; completed {:?} ms; save {saved:.2} ms, {} MiB",
            quantiles(&mut cpu),
            quantiles(&mut complete),
            size as f64 / 1048576.
        );
        compare_saved(&path, &snapshot)?;
        println!(
            "unchanged second save {:?}",
            save(snapshot, path.clone()).join().unwrap()?
        );
        canvas.undo_redo()?;
        canvas.memory()?;
        std::fs::remove_file(path)?;
    }
    if selected == "all" || selected == "multiple" {
        let mut first = Canvas::new([6000, 4000], "multiple-a", color)?;
        let mut second = Canvas::new([8192, 5504], "multiple-b", color)?;
        let mut third = Canvas::new([8192, 7324], "multiple-c", color)?;
        let path = output.join("multiple.capy");
        let worker = save(first.snapshot()?, path.clone());
        let (mut cpu, mut completed) = (Vec::new(), Vec::new());
        for ordinal in 0..4 {
            for canvas in [&mut first, &mut second, &mut third] {
                let (a, b) = canvas.stroke(ordinal)?;
                cpu.extend(a);
                completed.extend(b);
            }
        }
        println!(
            "24 + 45 + 60 MP documents, 768 alternating drawing frames: CPU {:?} ms; completed {:?} ms; concurrent save {:?}",
            quantiles(&mut cpu),
            quantiles(&mut completed),
            worker.join().unwrap()?
        );
        first.settle()?;
        second.settle()?;
        third.settle()?;
        first.memory()?;
        second.memory()?;
        third.memory()?;
        std::fs::remove_file(path)?;
    }
    Ok(())
}
