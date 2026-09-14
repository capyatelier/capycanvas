//! Dense raster, concurrent-save and multiple-document qualification.
//! Run in release on a physical GPU. Synthetic images are reproducible workloads,
//! not claims of photographic color accuracy. Export/undo timings are cold paths.
use layer_core::*;
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
    assets: BTreeMap<AssetId, ProjectAsset>,
    sequence: u64,
}
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}
fn quantiles(values: &mut [f64]) -> [f64; 3] {
    values.sort_by(f64::total_cmp);
    [0.5, 0.95, 0.99].map(|q| values[((values.len() - 1) as f64 * q).ceil() as usize])
}
impl Canvas {
    fn new(extent: [u32; 2], name: &str) -> Result<Self> {
        let start = Instant::now();
        let mut document = Document::new(name, extent[0], extent[1]);
        let id = AssetId::from("qualification:synthetic-source");
        let mut random = 0x1357abcdu32;
        let bytes = (0..u64::from(extent[0]) * u64::from(extent[1]))
            .flat_map(|i| {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                let x = i % u64::from(extent[0]);
                let y = i / u64::from(extent[0]);
                let noise = (random % 9) as u8;
                [
                    ((x * 211 / u64::from(extent[0])) as u8).saturating_add(noise),
                    ((y * 211 / u64::from(extent[1])) as u8).saturating_add(noise),
                    (x.wrapping_add(y) / 32 % 240) as u8,
                    255,
                ]
            })
            .collect::<Vec<_>>();
        let image = ProjectAsset {
            extent,
            format: ProjectAssetFormat::Rgba8Srgb,
            bytes: bytes.into(),
        };
        document.layers[0].asset = Some(id.clone());
        for _ in 0..31 {
            let id = document.allocate_layer_id();
            document.layers.insert(1, Layer::paint(id, "empty"));
        }
        let assets = BTreeMap::from([(id.clone(), image.clone())]);
        let mut gpu = WgpuRasterizer::new_headless()?;
        gpu.set_telemetry_enabled(true);
        gpu.prepare_owned_asset(&id, &image)?;
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
            assets,
            sequence: 0,
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
            )?;
        }
        Ok(())
    }
    fn snapshot(&self) -> Result<Project> {
        Ok(Project::snapshot(self.engine.document(), &self.assets)?)
    }
    fn stroke(&mut self, ordinal: u64) -> Result<(Vec<f64>, Vec<f64>)> {
        let mut brush = default_brush(DefaultBrushPreset::GPen);
        brush.diameter = 96.;
        brush.color_rgba_linear = [0.8, 0.04, 0.2, 0.5];
        self.engine.set_brush(brush)?;
        let mut cpu = Vec::new();
        let mut complete = Vec::new();
        for frame in 0..64 {
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
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let submitted = Instant::now();
                self.engine.render_frame()?;
                frame_cpu += ms(submitted);
                self.engine.backend_mut().wait_idle()?;
                if !self.engine.has_pending_input() {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("Drawing input did not drain".into());
                }
                std::thread::yield_now();
            }
            cpu.push(frame_cpu);
            complete.push(ms(start));
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
            self.assets.values().map(|a| a.bytes.len()).sum::<usize>() as f64 / 1048576.
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
    let selected = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let output = PathBuf::from("artifacts/color-m1/dense");
    std::fs::create_dir_all(&output)?;
    for (name, extent) in [
        ("24mp", [6000, 4000]),
        ("45mp", [8192, 5504]),
        ("60mp", [8192, 7324]),
    ] {
        if selected != "all" && selected != name {
            continue;
        }
        let mut canvas = Canvas::new(extent, name)?;
        canvas.stroke(0)?;
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
        let mut first = Canvas::new([6000, 4000], "multiple-a")?;
        let mut second = Canvas::new([6000, 4000], "multiple-b")?;
        let path = output.join("multiple.capy");
        let worker = save(first.snapshot()?, path.clone());
        let (mut cpu, mut completed) = (Vec::new(), Vec::new());
        for ordinal in 0..4 {
            for canvas in [&mut first, &mut second] {
                let (a, b) = canvas.stroke(ordinal)?;
                cpu.extend(a);
                completed.extend(b);
            }
        }
        println!(
            "two 24mp documents, 512 alternating drawing frames: CPU {:?} ms; completed {:?} ms; concurrent save {:?}",
            quantiles(&mut cpu),
            quantiles(&mut completed),
            worker.join().unwrap()?
        );
        first.settle()?;
        second.settle()?;
        first.memory()?;
        second.memory()?;
        std::fs::remove_file(path)?;
    }
    Ok(())
}
