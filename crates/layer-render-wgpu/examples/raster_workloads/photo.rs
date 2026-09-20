//! Production photo consumers, with exact worker/frame overlap and native
//! history checks. This is offscreen work latency, never GTK presentation time.
use super::*;
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_render_wgpu::snapshot::{CaptureControl, CaptureLimits};
use std::sync::{Arc, Barrier};

#[path = "navigation.rs"]
pub(super) mod navigation;

struct Frame {
    kind: &'static str,
    start: Instant,
    cpu: f64,
    complete: f64,
    cold: bool,
}
#[derive(Default)]
struct Observations {
    frames: Vec<Frame>,
    gpu_allocated_peak: u64,
    gpu_reserved_peak: u64,
}
impl Observations {
    fn memory(&mut self, canvas: &Canvas) {
        if let Some(report) = canvas.engine.backend().device().generate_allocator_report() {
            self.gpu_allocated_peak = self.gpu_allocated_peak.max(report.total_allocated_bytes);
            self.gpu_reserved_peak = self.gpu_reserved_peak.max(report.total_reserved_bytes);
        }
    }
    fn frame(
        &mut self,
        canvas: &Canvas,
        kind: &'static str,
        start: Instant,
        cpu: f64,
        complete: f64,
        cold: bool,
    ) {
        self.frames.push(Frame {
            kind,
            start,
            cpu,
            complete,
            cold,
        });
        // Outside the measured submission/completion interval. This diagnostic
        // run accounts for allocator reporting overhead in whole-job duration.
        self.memory(canvas);
    }
    fn render(
        &mut self,
        canvas: &mut Canvas,
        kind: &'static str,
        edit: impl FnOnce(&mut Engine) -> Result<()>,
    ) -> Result<()> {
        let before = canvas.engine.backend().metrics();
        let start = Instant::now();
        edit(&mut canvas.engine)?;
        canvas.engine.render_frame()?;
        let cpu = ms(start);
        canvas.engine.backend_mut().wait_idle()?;
        let complete = ms(start);
        let cold =
            before.source_tile_misses != canvas.engine.backend().metrics().source_tile_misses;
        self.frame(canvas, kind, start, cpu, complete, cold);
        Ok(())
    }
}
fn change(layer: &mut Layer, key: &str, value: f32) -> Result<()> {
    Arc::make_mut(layer.effect.as_mut().unwrap()).set(key, EffectValue::Number(value))?;
    Ok(())
}
fn adjustments(canvas: &mut Canvas, observations: &mut Observations) -> Result<LayerId> {
    let mut exposure = None;
    for (name, key, value) in [
        ("exposure", "exposure", 0.25),
        ("white_balance", "temperature", 4.),
        ("levels", "gamma", 1.08),
        ("hue_saturation", "saturation", 5.),
        ("color_balance", "midtones_red", 2.),
    ] {
        let id = canvas.engine.allocate_layer_id();
        let mut layer = Layer::paint(id, name);
        layer.kind = LayerKind::Effect;
        layer.effect = Some(Arc::new(EffectInstance::new(
            bundled_effect_catalog().get(name).unwrap().program(),
        )));
        change(&mut layer, key, value)?;
        if name == "exposure" {
            exposure = Some(id);
            // A real native scalar mask crossing page boundaries. Outside this
            // central band the adjustment is fully enabled. No display mip is
            // used as mask data.
            let doc = canvas.engine.document();
            let color = doc.color;
            let columns = doc.width.div_ceil(256);
            let row = doc.height / 512;
            let bytes: Vec<_> = (0..65536)
                .flat_map(|i| {
                    let code = ((i % 256) * 257) as u16;
                    match color.depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only benchmark fixture"),
                        SampleDepth::U8 => vec![(code >> 8) as u8],
                        SampleDepth::U16 => code.to_le_bytes().to_vec(),
                    }
                })
                .collect();
            let tile = RasterTile::backed(TileBlob::encode(color.coverage_descriptor(), &bytes)?);
            let data = RasterData {
                tiles: (0..columns)
                    .map(|x| {
                        (
                            TileKey {
                                plane: RasterPlane::Mask,
                                coordinate: [x, row],
                            },
                            tile.clone(),
                        )
                    })
                    .collect(),
                ..Default::default()
            };
            let mut mask =
                LayerMask::reveal_all(canvas.engine.allocate_layer_id(), Point::default());
            mask.raster = RasterRevision::backed(data);
            layer.mask = Some(mask);
        }
        observations.render(canvas, "adjustment-first", |engine| {
            engine.apply_edit(Edit::InsertLayer { index: 0, layer })?;
            Ok(())
        })?;
    }
    Ok(exposure.unwrap())
}
fn navigate(
    canvas: &mut Canvas,
    observations: &mut Observations,
    frames: usize,
    paced: bool,
) -> Result<()> {
    let extent = [
        canvas.engine.document().width as f32,
        canvas.engine.document().height as f32,
    ];
    for i in 0..frames {
        let start = Instant::now();
        let phase = (i % 64) as f32 / 63.;
        let scale = (1024. / extent[0]).min(768. / extent[1])
            * (1. + 3. * (phase * std::f32::consts::PI).sin());
        let x = -phase * (extent[0] * scale - 1024.).max(0.);
        let y = -phase * (extent[1] * scale - 768.).max(0.);
        observations.render(canvas, "pan-zoom", |engine| {
            engine.set_view(
                ViewState {
                    width_px: 1024,
                    height_px: 768,
                    document_to_surface: [scale, 0., 0., scale, x, y],
                    background_rgba_linear: [0.; 4],
                },
                ViewTransform {
                    revision: 0,
                    surface_to_document: [1. / scale, 0., 0., 1. / scale, -x / scale, -y / scale],
                },
            );
            Ok(())
        })?;
        if paced {
            pace(start);
        }
    }
    Ok(())
}
fn pace(start: Instant) {
    if let Some(delay) = Duration::from_secs_f64(1. / 120.).checked_sub(start.elapsed()) {
        std::thread::sleep(delay);
    }
}
fn sliders(canvas: &mut Canvas, observations: &mut Observations, exposure: LayerId) -> Result<()> {
    let original = canvas.engine.document().layer(exposure).unwrap().clone();
    for i in 0..64 {
        let mut layer = original.clone();
        change(&mut layer, "exposure", i as f32 / 64. - 0.5)?;
        observations.render(canvas, "slider", |engine| {
            engine.preview_edit(Edit::ReplaceLayer(Box::new(layer)))?;
            Ok(())
        })?;
    }
    // Preview cancellation restores retained state, without an adjustment chain
    // accumulating on each slider movement.
    canvas
        .engine
        .preview_edit(Edit::ReplaceLayer(Box::new(original)))?;
    canvas.engine.render_frame()?;
    canvas.engine.backend_mut().wait_idle()?;
    Ok(())
}
fn native_roots(canvas: &Canvas) -> Result<Vec<(LayerId, Vec<([u8; 32], TileKey)>)>> {
    let mut roots = Vec::new();
    for layer in &canvas.engine.document().layers {
        for (id, revision) in std::iter::once((layer.id, &layer.raster))
            .chain(layer.masks().map(|m| (m.id, &m.raster)))
        {
            let data = revision.wait_data()?;
            let mut tiles = Vec::new();
            for (key, tile) in &data.tiles {
                tiles.push((tile.wait_backing()?.digest, *key));
            }
            roots.push((id, tiles));
        }
    }
    Ok(roots)
}
fn history(canvas: &mut Canvas, observations: &mut Observations) -> Result<()> {
    canvas.settle()?;
    let roots = native_roots(canvas)?;
    for redo in [false, true] {
        observations.render(canvas, if redo { "redo" } else { "undo" }, |engine| {
            assert!(if redo { engine.redo()? } else { engine.undo()? });
            Ok(())
        })?;
    }
    assert_eq!(native_roots(canvas)?, roots);
    println!("Native paint and mask roots restored exactly after undo/redo");
    Ok(())
}

struct Job {
    name: &'static str,
    start: Instant,
    end: Instant,
    setup: Option<Instant>,
    cleanup: Option<Instant>,
}
fn concurrent(
    canvas: &mut Canvas,
    observations: &mut Observations,
    path: &Path,
    edit_during_capture: bool,
) -> Result<(Vec<Job>, u64)> {
    let snapshot = canvas.snapshot()?;
    let export_snapshot = snapshot.clone();
    let export_gpu = canvas.engine.backend().snapshot_gpu();
    let saved_snapshot = snapshot.clone();
    let barrier = Arc::new(Barrier::new(3));
    let start_gate = barrier.clone();
    let save_path = path.with_extension("capy");
    let saved_path = save_path.clone();
    let save = std::thread::spawn(move || -> std::result::Result<Job, String> {
        start_gate.wait();
        let start = Instant::now();
        let file = std::fs::File::create(save_path).map_err(|e| e.to_string())?;
        let mut out = BufWriter::new(file);
        snapshot.write(&mut out)?;
        out.flush()
            .and_then(|_| out.get_ref().sync_all())
            .map_err(|e| e.to_string())?;
        drop(out);
        Ok(Job {
            name: "save",
            start,
            end: Instant::now(),
            setup: None,
            cleanup: None,
        })
    });
    let start_gate = barrier.clone();
    let output_path = path.with_extension("png");
    let exported_path = output_path.clone();
    let control = CaptureControl::with_allocation_tracking();
    let export_control = control.clone();
    let export = std::thread::spawn(move || -> std::result::Result<Job, String> {
        start_gate.wait();
        let start = Instant::now();
        let mut renderer = export_gpu
            .capture(
                export_snapshot,
                [0.; 4],
                0.,
                CaptureLimits::default(),
                export_control,
            )
            .map_err(|e| e.to_string())?;
        let setup = Instant::now();
        let target = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
            profile_assumed: false,
        };
        let file = std::fs::File::create(output_path).map_err(|e| e.to_string())?;
        let mut out = BufWriter::new(file);
        let statistics = renderer.write_png(&mut out, &target, Default::default(), None)?;
        out.flush()
            .and_then(|_| out.get_ref().sync_all())
            .map_err(|e| e.to_string())?;
        println!("Profiled U16 ProPhoto PNG statistics {statistics:?}");
        let cleanup = Instant::now();
        drop(renderer);
        drop(out);
        Ok(Job {
            name: "export",
            start,
            end: Instant::now(),
            setup: Some(setup),
            cleanup: Some(cleanup),
        })
    });
    barrier.wait();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut stroke = 0;
    let interaction = (|| -> Result<()> {
        while edit_during_capture
            && (!save.is_finished() || !export.is_finished())
            && Instant::now() < deadline
        {
            canvas.stroke_observed(stroke % 4, |canvas, start, cpu, complete, cold| {
                observations.frame(canvas, "paint-workers", start, cpu, complete, cold);
                pace(start);
            })?;
            navigate(canvas, observations, 64, true)?;
            stroke += 1;
        }
        Ok(())
    })();
    // Both workers are always joined before another workload starts. A worker
    // can outlive the bounded interaction interval; recorded overlap stays exact.
    let saved = save.join();
    let exported = export.join();
    interaction?;
    let jobs = vec![
        saved.map_err(|_| "save panicked")??,
        exported.map_err(|_| "export panicked")??,
    ];
    assert_eq!(control.output_rows(), canvas.engine.document().height);
    let peaks = control.allocation_peaks().unwrap();
    assert!(peaks.observations > 0);
    println!(
        "Export GPU allocator peaks {peaks:?}; output {} bytes",
        std::fs::metadata(&exported_path)?.len()
    );
    compare_saved(&saved_path, &saved_snapshot)?;
    // Check the header and checksum decoded sample rows without retaining a
    // second full photograph.
    let decoder = png::Decoder::new(BufReader::new(std::fs::File::open(&exported_path)?));
    let mut reader = decoder.read_info()?;
    assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
    assert!(reader.info().icc_profile.is_some());
    let mut checksum = crc32fast::Hasher::new();
    while let Some(row) = reader.next_row()? {
        checksum.update(row.data());
    }
    println!(
        "Profiled PNG decoded sample checksum {:08x}",
        checksum.finalize()
    );
    std::fs::remove_file(saved_path)?;
    std::fs::remove_file(exported_path)?;
    Ok((jobs, peaks.reserved_bytes))
}
fn report(
    name: &str,
    origin: Instant,
    observations: &Observations,
    jobs: &[Job],
    output: &Path,
) -> Result<()> {
    let mut file = BufWriter::new(std::fs::File::create(
        output.join(format!("photo-{name}-frames.csv")),
    )?);
    writeln!(
        file,
        "kind,start_ms,cpu_ms,completed_ms,cold_source,save_overlap,export_overlap"
    )?;
    for f in &observations.frames {
        let overlaps = |name| {
            jobs.iter().any(|job| {
                job.name == name
                    && f.start < job.end
                    && f.start + Duration::from_secs_f64(f.complete / 1000.) > job.start
            })
        };
        writeln!(
            file,
            "{},{:.6},{:.6},{:.6},{},{},{}",
            f.kind,
            f.start.duration_since(origin).as_secs_f64() * 1000.,
            f.cpu,
            f.complete,
            f.cold,
            overlaps("save"),
            overlaps("export")
        )?;
    }
    let mut file = BufWriter::new(std::fs::File::create(
        output.join(format!("photo-{name}-jobs.csv")),
    )?);
    writeln!(file, "job,start_ms,end_ms")?;
    for job in jobs {
        writeln!(
            file,
            "{},{:.6},{:.6}",
            job.name,
            job.start.duration_since(origin).as_secs_f64() * 1000.,
            job.end.duration_since(origin).as_secs_f64() * 1000.
        )?;
        println!(
            "{} {:.3} ms",
            job.name,
            job.end.duration_since(job.start).as_secs_f64() * 1000.
        );
        if let Some(setup) = job.setup {
            writeln!(
                file,
                "{}-setup,{:.6},{:.6}",
                job.name,
                job.start.duration_since(origin).as_secs_f64() * 1000.,
                setup.duration_since(origin).as_secs_f64() * 1000.
            )?;
            println!(
                "{} setup {:.3} ms",
                job.name,
                setup.duration_since(job.start).as_secs_f64() * 1000.
            );
        }
        if let Some(cleanup) = job.cleanup {
            writeln!(
                file,
                "{}-cleanup,{:.6},{:.6}",
                job.name,
                cleanup.duration_since(origin).as_secs_f64() * 1000.,
                job.end.duration_since(origin).as_secs_f64() * 1000.
            )?;
            println!(
                "{} cleanup {:.3} ms",
                job.name,
                job.end.duration_since(cleanup).as_secs_f64() * 1000.
            );
        }
    }
    for kind in [
        "adjustment-first",
        "slider",
        "pan-zoom",
        "paint-workers",
        "undo",
        "redo",
        "blur-first",
        "blur-warm",
    ] {
        let frames: Vec<_> = observations
            .frames
            .iter()
            .filter(|f| f.kind == kind)
            .collect();
        if frames.is_empty() {
            continue;
        }
        let mut cpu: Vec<_> = frames.iter().map(|f| f.cpu).collect();
        let mut complete: Vec<_> = frames.iter().map(|f| f.complete).collect();
        println!(
            "{name} {kind}: n={} CPU {:?}; completed {:?} ms; >8.33={} max={:.3} ms",
            frames.len(),
            quantiles(&mut cpu),
            quantiles(&mut complete),
            complete.iter().filter(|v| **v > 8.33).count(),
            complete.last().unwrap()
        );
    }
    println!(
        "{name} live allocator peaks allocated/reserved {}/{} bytes",
        observations.gpu_allocated_peak, observations.gpu_reserved_peak
    );
    Ok(())
}
pub(super) fn run(
    selected: &str,
    color: DocumentColor,
    output: &Path,
    capture_only: bool,
) -> Result<()> {
    if selected == "all" {
        for case in ["24mp", "45mp", "60mp", "multiple"] {
            run(case, color, output, capture_only)?;
        }
        return Ok(());
    }
    let cases = [
        ("24mp", [6000, 4000]),
        ("45mp", [8192, 5504]),
        ("60mp", [8192, 7324]),
    ];
    let multiple = selected == "multiple";
    let mut retained = Vec::new();
    let mut retained_gpu = 0;
    for (name, extent) in cases {
        if !multiple && selected != "all" && selected != name {
            continue;
        }
        let origin = Instant::now();
        let mut canvas = Canvas::new(extent, name, color)?;
        let mut observations = Observations::default();
        canvas.stroke(0)?;
        let exposure = adjustments(&mut canvas, &mut observations)?;
        if !capture_only {
            sliders(&mut canvas, &mut observations, exposure)?;
            navigate(&mut canvas, &mut observations, 128, false)?;
        }
        canvas.settle()?;
        if multiple && name != "60mp" {
            observations.memory(&canvas);
            retained_gpu += observations.gpu_reserved_peak;
            retained.push(canvas);
            continue;
        }
        let (mut jobs, export_gpu) = concurrent(
            &mut canvas,
            &mut observations,
            &output.join(format!("photo-{selected}-{name}")),
            !capture_only,
        )?;
        if !capture_only {
            history(&mut canvas, &mut observations)?;
        }
        let control = CaptureControl::with_allocation_tracking();
        let start = Instant::now();
        let mut capture = canvas.engine.backend().snapshot_gpu().capture(
            canvas.snapshot()?,
            [0.; 4],
            0.,
            CaptureLimits::default(),
            control.clone(),
        )?;
        println!("Histogram worker setup {:.3} ms", ms(start));
        for repetition in 0..3 {
            let start = Instant::now();
            let histogram = capture.histogram()?;
            let end = Instant::now();
            assert_eq!(
                histogram.pixels + histogram.transparent,
                extent[0] as u64 * extent[1] as u64
            );
            assert!(
                histogram
                    .channels
                    .iter()
                    .all(|c| c.bins.iter().sum::<u64>() == histogram.pixels)
            );
            println!(
                "Histogram {name} repetition={repetition}: {:.3} ms pixels={} transparent={}",
                ms(start),
                histogram.pixels,
                histogram.transparent
            );
            let mut checksum = crc32fast::Hasher::new();
            for channel in &histogram.channels {
                for value in channel.bins.iter().copied().chain([
                    channel.below,
                    channel.above,
                    channel.black,
                    channel.white,
                ]) {
                    checksum.update(&value.to_le_bytes());
                }
            }
            println!(
                "Histogram bins and endpoints checksum {:08x}",
                checksum.finalize()
            );
            jobs.push(Job {
                name: "histogram",
                start,
                end,
                setup: None,
                cleanup: None,
            });
        }
        let histogram_gpu = control.allocation_peaks().unwrap();
        println!("Histogram GPU allocator peaks {histogram_gpu:?}");
        drop(capture);
        if !capture_only {
            let id = canvas.engine.allocate_layer_id();
            let mut blur = Layer::paint(id, "Gaussian blur");
            blur.kind = LayerKind::Effect;
            blur.effect = Some(Arc::new(EffectInstance::new(
                bundled_effect_catalog()
                    .get("gaussian_blur")
                    .unwrap()
                    .program(),
            )));
            observations.render(&mut canvas, "blur-first", |engine| {
                engine.apply_edit(Edit::InsertLayer {
                    index: 0,
                    layer: blur.clone(),
                })?;
                Ok(())
            })?;
            for i in 0..16 {
                change(&mut blur, "sigma", 1. + i as f32)?;
                observations.render(&mut canvas, "blur-warm", |engine| {
                    engine.preview_edit(Edit::ReplaceLayer(Box::new(blur.clone())))?;
                    Ok(())
                })?;
            }
        }
        report(
            if multiple { "multiple" } else { name },
            origin,
            &observations,
            &jobs,
            output,
        )?;
        println!(
            "Conservative aggregate GPU reservation bound (capture shares active device; other canvas peaks summed): {} bytes",
            retained_gpu
                + observations
                    .gpu_reserved_peak
                    .max(export_gpu)
                    .max(histogram_gpu.reserved_bytes)
        );
        canvas.memory()?;
    }
    Ok(())
}
