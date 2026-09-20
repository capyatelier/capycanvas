//! Sustained real-owner/worker/presentation qualification; software contacts.
use super::*;

#[test]
#[ignore = "private 120 Hz Wayland/GPU; retained 60 MP fixture and pinned codecs"]
fn native_local_tone_sustained_qualification() {
    use layer_render::CanvasRenderer;
    let prefix = std::path::PathBuf::from(std::env::var_os("LAYER_PACING_REPORT").unwrap());
    let path = std::env::var_os("LAYER_HDR_LARGE_INPUT").unwrap();
    let mut p =
        layer_core::Project::read(std::fs::File::open(path).unwrap(), Default::default()).unwrap();
    let extent = [p.document.width, p.document.height];
    assert!(u64::from(extent[0]) * u64::from(extent[1]) >= 59_000_000);
    assert_eq!(p.document.color.depth, SampleDepth::F16);
    p.document.sdr_rendition = Default::default();
    let original_layers = p.document.layers.clone();
    let app = native_test_app("art.capycanvas.LocalToneQualification");
    let started = Instant::now();
    let w = Workspace::with_project(&app, Some((p, None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let canvas_ready_ms = started.elapsed().as_secs_f64() * 1000.;
    invoke(&w, CommandId::SdrRendition);
    appearance(&w);
    let deadline = Instant::now() + Duration::from_secs(60);
    while w.local_tone.ready_count().is_none() {
        pump(5);
        assert!(Instant::now() < deadline);
    }
    let guide_ready_ms = started.elapsed().as_secs_f64() * 1000.;
    let guide_count = w.local_tone.ready_count();
    pump(300);
    let concurrent = std::env::var_os("LAYER_LOCAL_CONCURRENT").is_some();
    // A second live HDR document exercises per-window ownership and residency.
    let other = concurrent.then(|| {
        let photo = std::path::Path::new(
            "../../artifacts/color-m4/proof-polish/images/abandoned_hall_01_2k.capy",
        );
        let p = layer_core::Project::read(std::fs::File::open(photo).unwrap(), Default::default())
            .unwrap();
        let other = Workspace::with_project(&app, Some((p, None)));
        other.window.present();
        ready(&other);
        other
    });
    w.window.present();
    pump(300);
    let worker = concurrent.then(|| {
        let gpu = w.snapshot_gpu().unwrap();
        let snapshot = {
            let g = w.gpu.borrow(); let session = &g.as_ref().unwrap().session;
            DocumentExport { project: session.capture_project_recovery().unwrap(),
                background: session.engine().view().background_rgba_linear,
                time: session.engine().animation_time() }
        };
        let prefix = prefix.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            let master = prefix.with_extension("capy");
            layer_core::atomic_write(&master, |f| snapshot.project.write(f)).unwrap();
            let save_ms = start.elapsed().as_secs_f64() * 1000.;
            let restored = layer_core::Project::read(std::fs::File::open(&master).unwrap(), Default::default()).unwrap();
            assert_eq!(restored.document, snapshot.project.document, "exact saved master and rendition");
            drop(restored);
            let mut exports = Vec::new();
            for (format, name) in [(layer_ui::ExportFormat::JpegHdrMapped, "jpg"), (layer_ui::ExportFormat::AvifHdrMapped, "avif")] {
                let destination = prefix.with_extension(name);
                let started = Instant::now();
                let mut recipe = layer_ui::ExportRecipe::web_share();
                recipe.format = format;
                recipe.depth = SampleDepth::U16;
                recipe.background = if name == "jpg" { layer_ui::ExportBackground::White } else { layer_ui::ExportBackground::Preserve };
                let copy = DocumentExport { project: snapshot.project.clone(), background: snapshot.background, time: snapshot.time };
                let clipped = crate::files::export::write_snapshot(gpu.clone(), copy, recipe, &destination, &Default::default()).unwrap();
                let export_ms = started.elapsed().as_secs_f64() * 1000.;
                let decoded = layer_color::photo::read_photo(std::io::BufReader::new(std::fs::File::open(&destination).unwrap()), Default::default()).unwrap();
                assert_eq!(decoded.extent, extent);
                assert_eq!(decoded.interpretation.depth, SampleDepth::F16);
                let mut row = vec![0; decoded.row_bytes()];
                let mut rows = decoded.rows();
                let mut peak = 0f32; let mut pixels = 0u64;
                for y in 0..extent[1] {
                    rows.read(y, &mut row).unwrap();
                    for pixel in row.chunks_exact(8) {
                        let value = layer_core::color::hdr::decode_pixel(std::array::from_fn(|c| u16::from_le_bytes([pixel[c*2], pixel[c*2+1]]))).unwrap();
                        assert!(value.iter().all(|v| v.is_finite()));
                        peak = peak.max(value[0]).max(value[1]).max(value[2]); pixels += 1;
                    }
                }
                assert_eq!(pixels, u64::from(extent[0]) * u64::from(extent[1]));
                assert!(peak > 1., "HDR range must survive actual codec publication");
                exports.push(serde_json::json!({"format": name, "extent": extent, "export_ms": export_ms,
                    "export_and_decode_ms": started.elapsed().as_secs_f64()*1000., "decoded_pixels": pixels,
                    "peak": peak, "clipped_channels": clipped, "bytes": std::fs::metadata(&destination).unwrap().len()}));
                eprintln!("LOCAL_FULL_EXPORT {}", exports.last().unwrap());
            }
            serde_json::json!({"save_ms": save_ms, "elapsed_ms": start.elapsed().as_secs_f64()*1000., "exports": exports})
        })
    });
    let field = find_named(w.proof_panel.root.upcast_ref(), "sdr-tone-pad-surface").unwrap();
    let geometry = layer_ui::parameter_pad::ParameterDialGeometry::new(
        field.width().min(field.height()) as f32,
    )
    .unwrap();
    let controllers = field.observe_controllers();
    let drag = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureDrag>())
        .unwrap();
    let rendition = || {
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .sdr_rendition
    };
    let before = rendition();
    w.gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .renderer_mut()
        .set_telemetry_enabled(true);
    let stats = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .stats
        .clone();
    *stats.lock().unwrap() = Default::default();
    let seconds = std::env::var("LAYER_LOCAL_SECONDS")
        .ok()
        .map(|v| v.parse::<u64>().unwrap())
        .unwrap_or(120);
    assert!(seconds >= 8);
    let context = glib::MainContext::default();
    let tick = glib::timeout_add_local(Duration::from_millis(1), || glib::ControlFlow::Continue);
    let center = geometry.field.disc_marker([0.5, 0.5]);
    drag.emit_by_name::<()>("drag-begin", &[&(center[0] as f64), &(center[1] as f64)]);
    let start = Instant::now();
    let mut requests = Vec::new();
    let mut last = center;
    let mut memory = Vec::new();
    for i in 0..seconds * 120 {
        let due = start + Duration::from_secs_f64(i as f64 / 120.);
        while Instant::now() < due {
            context.iteration(true);
        }
        // Cross a quantized control value on every tick. Otherwise legitimate
        // unchanged-value suppression appears as a missed presentation slot.
        let angle = i as f32 * 0.04;
        last = geometry
            .field
            .disc_marker([0.5 + 0.4 * angle.cos(), 0.5 + 0.4 * angle.sin()]);
        let requested_ns = glib::monotonic_time() as u64 * 1000;
        let cpu = Instant::now();
        drag.emit_by_name::<()>(
            "drag-update",
            &[
                &((last[0] - center[0]) as f64),
                &((last[1] - center[1]) as f64),
            ],
        );
        requests.push(serde_json::json!({"requested_ns": requested_ns, "recipe": rendition(),
            "cpu_ms": cpu.elapsed().as_secs_f64()*1000., "lateness_ms": due.elapsed().as_secs_f64()*1000.}));
        if i % 120 == 0 {
            memory.push(serde_json::json!({"second": i/120, "memory": process_memory()}));
        }
        assert!(
            !w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .rendering_suspended()
        );
    }
    drag.emit_by_name::<()>(
        "drag-end",
        &[
            &((last[0] - center[0]) as f64),
            &((last[1] - center[1]) as f64),
        ],
    );
    tick.remove();
    pump(300);
    let s = stats.lock().unwrap();
    let telemetry = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .telemetry();
    let mut report = serde_json::json!({"canvas_ready_ms": canvas_ready_ms, "guide_ready_ms": guide_ready_ms,
        "extent": extent, "seconds": seconds, "concurrent": concurrent, "requests": requests,
        "hdr_views": s.hdr_views, "worker_cpu": s.cpu, "worker_gpu": s.gpu,
        "canvas_presentation": s.presented, "camera_work": s.camera_work,
        "renderer_resident_bytes": telemetry.resident_bytes, "memory": memory});
    drop(s);
    assert_eq!(
        w.local_tone.ready_count(),
        guide_count,
        "sustained recipe changes must reuse the guide"
    );
    invoke(&w, CommandId::Undo);
    ready(&w);
    assert_eq!(rendition(), before, "one drag remains one undo step");
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers,
        original_layers
    );
    if let Some(worker) = worker {
        let deadline = Instant::now() + Duration::from_secs(900);
        while !worker.is_finished() {
            pump(10);
            assert!(Instant::now() < deadline, "full exports timed out");
        }
        report["operations"] = worker.join().unwrap();
    }
    report["final_memory"] = serde_json::json!(process_memory());
    std::fs::write(&prefix, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    if let Some(other) = other {
        other.window.destroy();
    }
    w.window.destroy();
    pump(100);
}

fn process_memory() -> Vec<String> {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
        .map(str::to_owned)
        .collect()
}
