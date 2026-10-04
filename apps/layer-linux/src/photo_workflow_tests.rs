//! Actual camera files through native DND, transform handles, painting and save.
//! Frame timings measure the GTK native renderer on an isolated compositor.
use super::*;
use layer_core::{Affine, Document};
use std::sync::{Arc, Mutex};

fn layer(w: &Workspace) -> Document {
    ui_session(w).engine().document().clone()
}

fn window_point(w: &Workspace, document: Point) -> [f32; 2] {
    let p = Affine(state(w).camera.document_to_surface()).map(document);
    let area = w.area.compute_bounds(&w.window).unwrap();
    let dpi = w.area.scale_factor() as f32;
    [area.x() + p.x / dpi, area.y() + p.y / dpi]
}
fn canvas_hit(w: &Workspace, point: [f32; 2]) -> bool {
    w.window
        .pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT)
        .is_some_and(|picked| picked == w.area)
}
fn distribution(mut values: Vec<f64>) -> Value {
    values.sort_by(f64::total_cmp);
    if values.is_empty() {
        return Value::Null;
    }
    json!({"count": values.len(), "p50": values[values.len() / 2],
        "p95": values[((values.len() as f64 * 0.95).ceil() as usize - 1).min(values.len() - 1)],
        "max": values.last().unwrap()})
}
pub(crate) fn frames(stats: &Arc<Mutex<crate::timing::Stats>>) -> Value {
    let stats = stats.lock().unwrap();
    let presented: Vec<_> = stats.presented.iter().filter(|v| v[3] == 1).collect();
    let mut previous_pose = None;
    let mut latency = Vec::new();
    let mut changed_inputs = 0;
    for (at, layer, _, pose) in &stats.photo_inputs {
        if previous_pose.as_ref() == Some(&(layer, pose)) {
            continue;
        }
        previous_pose = Some((layer, pose));
        changed_inputs += 1;
        if let Some(presented_at) = stats
            .photo_frames
            .iter()
            .filter(|(_, id, geometry)| id == layer && geometry == pose)
            .filter_map(|(frame, _, _)| {
                presented
                    .iter()
                    .find(|p| p[0] == *frame && p[1] >= *at)
                    .map(|p| p[1])
            })
            .min()
        {
            latency.push((presented_at - *at) as f64 / 1e6);
        }
    }
    let mut camera_frames = Vec::new();
    let mut previous_camera = None;
    for p in &presented {
        if let Some((_, camera, _)) = stats.camera_views.iter().find(|v| v.0 == p[0])
            && previous_camera != Some(*camera)
        {
            camera_frames.push(p[1]);
            previous_camera = Some(*camera);
        }
    }
    json!({
        "cpu_total_ms": distribution(stats.cpu.iter().map(|v| v[3]).collect()),
        "gpu_ms": distribution(stats.gpu.iter().map(|v| v[1]).collect()),
        "owner_input_ms": distribution(stats.input_handler_cpu.clone()),
        "presentation_gap_ms": distribution(presented.windows(2)
            .map(|v| v[1][1].saturating_sub(v[0][1]) as f64 / 1e6).collect()),
        "cpu": stats.cpu, "cpu_stages": stats.cpu_stages, "thread_cpu": stats.thread_cpu,
        "gpu": stats.gpu, "presented": stats.presented,
        "camera_views": stats.camera_views, "photo_inputs": stats.photo_inputs,
        "material_samples": stats.material_samples,
        "renderer_phases": stats.renderer_phases, "material_phases": stats.material_phases,
        "source_transfers": stats.source_transfers,
        "pen_routes": stats.pen_routes,
        "photo_frames": stats.photo_frames, "changed_pose_inputs": changed_inputs,
        "gtk_pose_to_presentation_ms": distribution(latency),
        "distinct_camera_presentations": camera_frames.len(),
        "distinct_camera_gap_ms": distribution(camera_frames.windows(2)
            .map(|v| v[1].saturating_sub(v[0]) as f64 / 1e6).collect()),
        "camera_work": stats.camera_work, "raster_commits": stats.raster_commits,
    })
}
fn measured_events(
    driver: &mut FileDrag,
    w: &Rc<Workspace>,
    stats: &Arc<Mutex<crate::timing::Stats>>,
    events: Value,
) -> Value {
    ready(w);
    until(
        || {
            ui_session(&w)
                .engine()
                .backend()
                .frames_idle()
                && w.frame_timer.borrow().is_none()
        },
        "preceding workflow frames settle",
    );
    pump(50);
    let previous_presentation_ns = stats
        .lock()
        .unwrap()
        .presented
        .iter()
        .filter(|p| p[3] == 1)
        .map(|p| p[1])
        .max();
    *stats.lock().unwrap() = Default::default();
    let step = driver.input.step;
    let start = Instant::now();
    driver.input.perform(events);
    ready(w);
    let drain = Instant::now();
    until(
        || {
            let gpu = w.gpu.borrow();
            let engine = gpu.as_ref().unwrap().session.engine();
            !w.input.has_pending()
                && !engine.has_pending_input()
                && !engine.has_active_stroke()
                && !engine.has_pending_document_edits()
                && engine.backend().frames_idle()
                && w.frame_timer.borrow().is_none()
        },
        "native stroke and queued renderer frames complete",
    );
    let drain_ms = drain.elapsed().as_secs_f64() * 1000.;
    pump(300); // Drain presentation and timestamp-query feedback.
    let mut report = frames(stats);
    report["previous_presentation_ns"] = json!(previous_presentation_ns);
    report["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.);
    report["stroke_completion_wait_ms"] = json!(drain_ms);
    report["input_trace"] =
        read(&driver.input.dir.join(format!("trace-{step}.json"))).unwrap_or(Value::Null);
    report["process_memory"] = json!(process_memory());
    report
}

#[test]
#[ignore = "isolated compositor, LAYER_PLACEMENT_PHOTO and LAYER_NATIVE_EVENT_MS=8"]
#[allow(deprecated)]
fn native_large_photo_placement_workflow() {
    let path =
        PathBuf::from(std::env::var_os("LAYER_PLACEMENT_PHOTO").expect("actual camera JPEG"));
    let output =
        PathBuf::from(std::env::var_os("LAYER_PHOTO_WORKFLOW_OUTPUT").expect("evidence directory"));
    std::fs::create_dir_all(&output).unwrap();
    let output = output.canonicalize().unwrap();
    let app = native_test_app("art.capycanvas.LargePhotoPlacement");
    let w = Workspace::with_project(&app, Some((new_drawing(2000, 1500, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(), None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    // Optional second real photo exercises a retained multi-layer document.
    // Use the production Import preparation and Apply before the measured Drop.
    let background = std::env::var_os("LAYER_PLACEMENT_BACKGROUND_PHOTO").map(|path| {
        let path = PathBuf::from(path);
        invoke(&w, CommandId::ImportImage);
        let chooser = super::super::new_photo::chooser();
        chooser.set_file(&gtk::gio::File::for_path(&path)).unwrap();
        pump(100);
        chooser.response(gtk::ResponseType::Accept);
        finish(&w);
        ready(&w);
        let photo = layer(&w);
        let source = active_paint(&photo).original.as_ref().expect("retained background photo");
        assert!(u64::from(source.extent[0]) * u64::from(source.extent[1]) >= 24_000_000);
        invoke(&w, CommandId::ApplyTransform);
        ready(&w);
        photo
    });
    let initial_layers = ui_session(&w).engine().document().clone();
    let stats = ui_session(&w)
        .engine()
        .backend()
        .stats
        .clone();
    let mut report = json!({"file": path, "canvas": [2000, 1500],
        "display_scale": w.area.scale_factor(), "event_interval_ms": std::env::var("LAYER_NATIVE_EVENT_MS").ok()});
    if let Some(background) = &background {
        report["background_photo"] = json!({
            "file": std::env::var_os("LAYER_PLACEMENT_BACKGROUND_PHOTO"),
            "extent": active_paint(&background).original.as_ref().unwrap().extent,
            "source_resident_bytes": active_paint(&background).original.as_ref().unwrap().resident_bytes(),
            "placement": active_occurrence(&background).placement.as_affine().unwrap().0,
        });
    }
    let mut driver = FileDrag::start(&w);
    let area = w.area.compute_bounds(&w.window).unwrap();
    let drop_point = ((w.window.width() / 2 + 40)..(w.window.width() - 40))
        .step_by(20)
        .map(|x| [x as f32, area.y() + area.height() * 0.5])
        .find(|&p| canvas_hit(&w, p))
        .expect("visible native drop destination");
    driver.hover(std::slice::from_ref(&path), drop_point, false);
    assert!(w.image_drop_label.is_visible());
    let owner_ticks = Rc::new(RefCell::new(Vec::new()));
    let ticks = owner_ticks.clone();
    let heartbeat = glib::timeout_add_local(Duration::from_millis(5), move || {
        ticks.borrow_mut().push(glib::monotonic_time());
        glib::ControlFlow::Continue
    });
    *stats.lock().unwrap() = Default::default();
    let drop_started_ns = glib::monotonic_time() as u64 * 1000;
    driver.release(false);
    finish(&w);
    ready(&w);
    let photo = layer(&w);
    let source = active_paint(&photo).original.clone().expect("retained camera photo");
    assert!(source.extent[0] as u64 * source.extent[1] as u64 >= 24_000_000);
    assert!(active_paint(&photo).raster.is_empty());
    let fit = (2000. / source.extent[0] as f32).min(1500. / source.extent[1] as f32);
    assert!((active_occurrence(&photo).placement.as_affine().unwrap().0[0] - fit).abs() < 1e-5);
    until(
        || {
            let s = stats.lock().unwrap();
            s.camera_work
                .iter()
                .any(|work| work[3] > 0 && s.presented.iter().any(|p| p[0] == work[0] && p[3] == 1))
        },
        "first presented photo frame",
    );
    let first_ns = {
        let s = stats.lock().unwrap();
        s.camera_work
            .iter()
            .filter(|work| work[3] > 0)
            .filter_map(|work| {
                s.presented
                    .iter()
                    .find(|p| p[0] == work[0] && p[3] == 1)
                    .map(|p| p[1])
            })
            .min()
            .unwrap()
    };
    heartbeat.remove();
    report["drop_to_first_presented_ms"] =
        json!(first_ns.saturating_sub(drop_started_ns) as f64 / 1e6);
    report["loading_owner_tick_gaps_ms"] = distribution(
        owner_ticks
            .borrow()
            .windows(2)
            .map(|v| (v[1] - v[0]) as f64 / 1000.)
            .collect(),
    );
    report["source_extent"] = json!(source.extent);
    report["source_resident_bytes"] = json!(source.resident_bytes());
    report["drop_frames"] = frames(&stats);
    report["loading_memory"] = json!(process_memory());
    println!(
        "photo {:?}: first placement {} ms, extent {:?}",
        path.file_name(),
        report["drop_to_first_presented_ms"],
        source.extent
    );
    driver.stop_source();
    let thumbnail_started = Instant::now();
    super::super::place_source::wait_layer_thumbnail(&w, layer_ui::occurrence_token(photo.working.occurrence.unwrap()));
    report["visible_thumbnail_wait_ms"] = json!(thumbnail_started.elapsed().as_secs_f64() * 1000.);
    report["drop_to_visible_thumbnail_ms"] = json!((glib::monotonic_time() as u64 * 1000)
        .saturating_sub(drop_started_ns) as f64 / 1e6);

    let oversized = std::env::var("LAYER_PLACEMENT_FACTOR")
        .ok().map(|v| v.parse::<f32>().unwrap());
    let events = if let Some(factor) = oversized {
        assert!(factor > 1.);
        // Use the production numeric scale control to establish clipped content,
        // then deliver the measured translation with actual native pointer input.
        w.dispatch(UiAction::SetToolSetting {
            id: "transform_width".into(), value: fit * factor,
        });
        ready(&w);
        let before = active_occurrence(&layer(&w)).placement.clone();
        assert!((before.as_affine().unwrap().0[0] - fit * factor).abs() < 1e-5);
        report["oversized_factor"] = json!(factor);
        report["measured_transform_kind"] = json!("translation");
        report["oversized_placement"] = json!(before.as_affine().unwrap().0);
        let center = window_point(&w, Point { x: 1000., y: 750. });
        assert!(canvas_hit(&w, center));
        let mut events = vec![json!({"point": center}), json!({"down": true})];
        for i in 1..=120 {
            events.push(json!({"point": [center[0] + 32. * (i as f32 * 0.08).sin(),
                center[1] + 0.15 * i as f32]}));
        }
        events.push(json!({"down": false}));
        events
    } else {
        // Native primary-button pickup on a visible corner handle, followed by a
        // continuous one-second scale sweep. No shared-session gesture injection.
        let handle = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .into_iter()
            .map(|f| {
                window_point(
                    &w,
                    active_occurrence(&photo).placement.map(Point {
                        x: f[0] * source.extent[0] as f32,
                        y: f[1] * source.extent[1] as f32,
                    }).unwrap(),
                )
            })
            .find(|&p| canvas_hit(&w, p))
            .expect("uncovered scale handle");
        let center = window_point(
            &w,
            active_occurrence(&photo).placement.map(Point {
                x: source.extent[0] as f32 * 0.5,
                y: source.extent[1] as f32 * 0.5,
            }).unwrap(),
        );
        let mut events = vec![json!({"point": handle}), json!({"down": true})];
        for i in 1..=120 {
            let distance =
                0.12 * (i as f32 / 120. * std::f32::consts::PI).sin() + 0.04 * i as f32 / 120.;
            events.push(
                json!({"point": [handle[0] + (handle[0] - center[0]) * distance,
                handle[1] + (handle[1] - center[1]) * distance]}),
            );
        }
        events.push(json!({"down": false}));
        events
    };
    report["scale_motion"] = measured_events(&mut driver, &w, &stats, json!(events));
    let posed = layer(&w);
    assert_ne!(
        active_occurrence(&posed).placement, active_occurrence(&photo).placement,
        "native handle must change geometry"
    );
    assert_source_samples(active_paint(&posed).original.as_deref().unwrap(), source.as_ref());
    assert!(active_paint(&posed).raster.is_empty());
    if oversized.is_some() {
        assert_eq!(active_occurrence(&posed).placement.as_affine().unwrap().0[..4],
            serde_json::from_value::<[f32; 6]>(report["oversized_placement"].clone()).unwrap()[..4]);
        assert_ne!(active_occurrence(&posed).placement.as_affine().unwrap().0[4..],
            serde_json::from_value::<[f32; 6]>(report["oversized_placement"].clone()).unwrap()[4..],
            "native drag must translate the clipped photo");
    }
    super::super::new_photo::capture_ui(&w, &output, "active-placement.png");
    driver.click_placement(&w, "canvas-bar-ApplyTransform");
    ready(&w);
    assert_eq!(active_occurrence(&layer(&w)).placement, active_occurrence(&posed).placement);
    assert!(active_paint(&layer(&w)).raster.is_empty());
    invoke(&w, CommandId::Undo);
    ready(&w);
    assert_live_artwork_eq(ui_session(&w).engine().document(), &initial_layers);
    invoke(&w, CommandId::Redo);
    ready(&w);
    assert_source_samples(active_paint(&layer(&w)).original.as_deref().unwrap(), source.as_ref());

    // Ordinary painting on the fitted layer must allocate only edited local
    // backing; the immutable source and pose survive the stroke and its undo.
    invoke(&w, CommandId::Pen);
    w.dispatch(UiAction::SetBrushSize { value: 24. });
    w.dispatch(UiAction::SetColor {
        rgba: [1., 0., 1., 1.],
    });
    ready(&w);
    let center = window_point(&w, Point { x: 1000., y: 750. });
    assert!(canvas_hit(&w, center));
    let mut events = vec![
        json!({"point": [center[0] - 90., center[1]]}),
        json!({"down": true}),
    ];
    for i in 0..120 {
        events.push(json!({"point": [center[0] - 90. + 1.5 * i as f32,
            center[1] + 18. * (i as f32 * 0.07).sin()]}));
    }
    events.push(json!({"down": false}));
    report["paint_motion"] = measured_events(&mut driver, &w, &stats, json!(events));
    let painted = layer(&w);
    assert!(
        !active_paint(&painted).raster.is_empty(),
        "native stroke must create paint backing"
    );
    assert_source_samples(active_paint(&painted).original.as_deref().unwrap(), source.as_ref());
    assert_eq!(active_occurrence(&painted).placement, active_occurrence(&posed).placement);
    invoke(&w, CommandId::Undo);
    ready(&w);
    assert!(active_paint(&layer(&w)).raster.is_empty());
    invoke(&w, CommandId::Redo);
    ready(&w);
    assert!(!active_paint(&layer(&w)).raster.is_empty());

    // Optional large-brush stress uses real native pointer strokes. Recording
    // gather counters proves whether it exercised the distant-source path.
    // Undo restores the ordinary paint baseline before navigation/save checks.
    if std::env::var_os("LAYER_PLACEMENT_MATERIALS").is_some() {
        let native_size = std::env::var_os("LAYER_PLACEMENT_MATERIALS_NATIVE_SIZE").is_some();
        if native_size {
            invoke(&w, CommandId::ScaleRotate);
            driver.click_placement(&w, "canvas-bar-PlacementOriginalSize");
            driver.click_placement(&w, "canvas-bar-ApplyTransform");
            ready(&w);
        }
        for (preset, command, label, diameter) in [
            (layer_core::DefaultBrushPreset::Smudge, CommandId::Blend, "smudge", 1024.),
            (layer_core::DefaultBrushPreset::LiquifyTwirl, CommandId::Liquify, "liquify-twirl", 240.),
            (layer_core::DefaultBrushPreset::LiquifyPinch, CommandId::Liquify, "liquify-pinch", 240.),
        ] {
            invoke(&w, command);
            w.dispatch(UiAction::SelectBrush { id: preset as u32 });
            w.dispatch(UiAction::SetBrushSize { value: diameter });
            ready(&w);
            let selected = state(&w);
            super::super::new_photo::capture_ui(&w, &output, &format!("{label}-ready.png"));
            assert_eq!(selected.brush.preset, preset as u32, "{label}: {}", w.status.text());
            assert_eq!(selected.brush.diameter, diameter, "{label}: {}", w.status.text());
            assert!(selected.commands.iter().any(|c| c.id == command && c.selected),
                "{label}: tool must be active: {}", w.status.text());
            assert!(selected.host_error.is_none(), "{label}: {:?}", selected.host_error);
            let center = window_point(&w, Point { x: 1000., y: 750. });
            assert!(canvas_hit(&w, [center[0] - 90., center[1]]));
            assert!(canvas_hit(&w, [center[0] + 90., center[1]]));
            layer_render::CanvasRenderer::set_telemetry_enabled(
                ui_session_mut(&w).renderer_mut(), true);
            let before = layer(&w);
            let counters = stats.lock().unwrap().material_samples.last().copied().unwrap_or_default();
            let mut events = vec![json!({"point": [center[0] - 90., center[1]]}), json!({"down": true})];
            for i in 0..120 {
                events.push(json!({"point": [center[0] - 90. + 1.5 * i as f32,
                    center[1] + 18. * (i as f32 * 0.07).sin()]}));
            }
            events.push(json!({"down": false}));
            let mut result = measured_events(&mut driver, &w, &stats, json!(events));
            let now = stats.lock().unwrap().material_samples.last().copied().unwrap_or_default();
            result["source_sample_jobs"] = json!(now[1].saturating_sub(counters[1]));
            result["source_sample_passes"] = json!(now[2].saturating_sub(counters[2]));
            result["source_sample_field_bytes"] = json!(now[3]);
            result["brush_diameter"] = json!(diameter);
            result["placement"] = json!(active_occurrence(&before).placement.as_affine().unwrap().0);
            result["native_size_comparison"] = json!(native_size);
            result["status"] = json!(w.status.text().as_str());
            result["host_error"] = json!(state(&w).host_error);
            result["input_pending"] = json!(w.input.has_pending());
            result["engine_metrics"] = json!(format!("{:?}", ui_session(&w).engine().metrics()));
            super::super::new_photo::capture_ui(&w, &output, &format!("{label}-after.png"));
            std::fs::write(output.join(format!("{label}-motion.json")), serde_json::to_vec(&result).unwrap()).unwrap();
            if !native_size {
                assert!(now[1] > counters[1], "{label} must exercise distant source gathering: {}", w.status.text());
            }
            let after = layer(&w);
            assert_ne!(active_paint(&after).raster, active_paint(&before).raster, "{label} must create an edit");
            assert_eq!(active_paint(&after).original, active_paint(&before).original);
            assert_eq!(active_occurrence(&after).placement, active_occurrence(&before).placement);
            super::super::new_photo::capture_ui(&w, &output, &format!("{label}.png"));
            invoke(&w, CommandId::Undo); ready(&w);
            assert_eq!(active_paint(&layer(&w)).raster, active_paint(&before).raster);
            invoke(&w, CommandId::Redo); ready(&w);
            assert_eq!(active_paint(&layer(&w)).raster, active_paint(&after).raster);
            invoke(&w, CommandId::Undo); ready(&w);
            assert_eq!(layer(&w), before);
            report[format!("{label}_motion")] = result;
        }
        if native_size {
            invoke(&w, CommandId::Undo);
            ready(&w);
            assert_live_artwork_eq(&layer(&w), &painted);
        }
    }

    let camera_before = state(&w).camera;
    let mut events = vec![
        json!({"point": center}),
        json!({"button": 274, "down": true}),
    ];
    for i in 1..=120 {
        events.push(json!({"point": [center[0] + i as f32 * 0.8, center[1] + 20. * (i as f32 * 0.05).sin()]}));
    }
    events.push(json!({"button": 274, "down": false}));
    report["navigation_motion"] = measured_events(&mut driver, &w, &stats, json!(events));
    assert_ne!(state(&w).camera.translation, camera_before.translation);
    assert_eq!(active_occurrence(&layer(&w)).placement, active_occurrence(&posed).placement);

    let master = output.join("Photo master.capy");
    // Use the production GTK Save dialog and worker; the original JPEG remains
    // an independent input, never the native project's overwrite destination.
    invoke(&w, CommandId::SaveDocument);
    let save = super::super::new_photo::chooser();
    save.set_current_folder(Some(&gtk::gio::File::for_path(&output)))
        .unwrap();
    save.set_current_name("Photo master.capy");
    pump(200);
    let saved_at = Instant::now();
    save.response(gtk::ResponseType::Accept);
    finish(&w);
    report["save_ms"] = json!(saved_at.elapsed().as_secs_f64() * 1000.);
    assert!(!state(&w).document_file.modified);
    let reopened =
        open_native_document(std::fs::File::open(&master).unwrap());
    let mut saved_artwork = painted.clone();
    saved_artwork.artwork.outputs.get_mut(saved_artwork.artwork.default_output).unwrap().context = reopened.output().context.clone();
    assert_eq!(artwork_manifest(&reopened), artwork_manifest(&saved_artwork));
    if let Some(background) = &background {
        let h = background.working.occurrence.unwrap();
        let portable = background.artwork.occurrences.id(h).unwrap();
        let restored = reopened.artwork.occurrences.resolve(portable).unwrap();
        assert_eq!(reopened.scene().occurrence(restored), background.scene().occurrence(h), "the retained background photo remains unchanged");
        assert_eq!(reopened.scene().paint_source(restored), background.scene().paint_source(h));
    }
    let portable = painted.artwork.occurrences.id(painted.working.occurrence.unwrap()).unwrap();
    let h = reopened.artwork.occurrences.resolve(portable).unwrap();
    let restored_layer = reopened.scene().occurrence(h).unwrap();
    let restored_source = reopened.scene().paint_source(h).unwrap();
    assert_source_samples(restored_source.original.as_deref().unwrap(), source.as_ref());
    assert_eq!(restored_layer.placement, active_occurrence(&posed).placement);
    assert!(!restored_source.raster.is_empty());
    let restored_raster = restored_source.raster.clone();
    let before = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 9861))
        .unwrap();
    w.window.destroy();
    pump(200);
    let reopened_at = Instant::now();
    let restored = Workspace::with_project(&app, Some((reopened, None)));
    restored.window.maximize();
    restored.window.present();
    ready(&restored);
    report["reopened_renderer_ready_ms"] = json!(reopened_at.elapsed().as_secs_f64() * 1000.);
    let after = glib::MainContext::default()
        .block_on(read_canvas_pixels(&restored, 9862))
        .unwrap();
    assert!(
        before.bytes == after.bytes,
        "exact artwork capture survives save/reopen"
    );
    invoke(&restored, CommandId::ScaleRotate);
    driver.click_placement(&restored, "canvas-bar-PlacementOriginalSize");
    let native = layer(&restored);
    assert!(
        (active_occurrence(&native).placement.as_affine().unwrap().0[0].hypot(active_occurrence(&native).placement.as_affine().unwrap().0[1]) - 1.).abs()
            < 1e-5
    );
    assert_source_samples(active_paint(&native).original.as_deref().unwrap(), source.as_ref());
    assert_eq!(active_paint(&native).raster, restored_raster);
    driver.click_placement(&restored, "canvas-bar-ApplyTransform");
    ready(&restored);
    let original_thumbnail_at = Instant::now();
    super::super::place_source::wait_layer_thumbnail(&restored, layer_ui::occurrence_token(native.working.occurrence.unwrap()));
    report["original_size_thumbnail_wait_ms"] = json!(original_thumbnail_at.elapsed().as_secs_f64() * 1000.);
    super::super::new_photo::capture_ui(&restored, &output, "original-size.png");

    // Menu Import uses the same preparation contract as Drop on an existing
    // document. Cancel must discard this second large source without changing
    // the accepted original-size layer or the current layer selection.
    let before_import = layer(&restored);
    invoke(&restored, CommandId::ImportImage);
    let import = super::super::new_photo::chooser();
    import.set_file(&gtk::gio::File::for_path(&path)).unwrap();
    pump(100);
    let import_at = Instant::now();
    import.response(gtk::ResponseType::Accept);
    finish(&restored);
    ready(&restored);
    report["menu_import_ready_ms"] = json!(import_at.elapsed().as_secs_f64() * 1000.);
    assert_source_samples(active_paint(&layer(&restored)).original.as_deref().unwrap(), source.as_ref());
    assert!(active_paint(&layer(&restored)).raster.is_empty());
    driver.click_placement(&restored, "canvas-bar-CancelTransform");
    ready(&restored);
    assert_eq!(layer(&restored), before_import);

    let opened = Rc::new(RefCell::new(None));
    let result = opened.clone();
    *restored.open_document.borrow_mut() = Some(Rc::new(move |project, location, _| {
        result.replace(Some((project, location)));
    }));
    invoke(&restored, CommandId::OpenDocument);
    let open = super::super::new_photo::chooser();
    open.set_file(&gtk::gio::File::for_path(&path)).unwrap();
    pump(100);
    let open_at = Instant::now();
    open.response(gtk::ResponseType::Accept);
    finish(&restored);
    report["open_prepared_ms"] = json!(open_at.elapsed().as_secs_f64() * 1000.);
    let (project, location) = opened
        .borrow_mut()
        .take()
        .expect("Open publishes a photo document");
    assert!(location.is_none());
    assert_eq!(
        project.composition().size,
        source.extent
    );
    assert_source_samples(paint_at(&project, 0).original.as_deref().unwrap(), source.as_ref());
    assert_eq!(
        occurrence_at(&project, 0).placement,
        layer_core::LayerPlacement::IDENTITY
    );
    restored.window.destroy();
    pump(200);
    let open_render_at = Instant::now();
    let photo_document = Workspace::with_project(&app, Some((project, None)));
    photo_document.window.maximize();
    photo_document.window.present();
    ready(&photo_document);
    report["open_renderer_ready_ms"] = json!(open_render_at.elapsed().as_secs_f64() * 1000.);
    let thumbnail_started = Instant::now();
    super::super::place_source::wait_layer_thumbnail(&photo_document, layer_ui::occurrence_token(layer(&photo_document).working.occurrence.unwrap()));
    report["open_thumbnail_wait_ms"] = json!(thumbnail_started.elapsed().as_secs_f64() * 1000.);
    super::super::new_photo::capture_ui(&photo_document, &output, "opened-photo.png");
    assert!(active_paint(&layer(&photo_document)).raster.is_empty());
    assert_source_samples(active_paint(&layer(&photo_document)).original.as_deref().unwrap(), source.as_ref());
    report["final_memory"] = json!(process_memory());
    publish(&output.join("workflow.json"), &report);
    println!(
        "native large-photo workflow report: {}",
        output.join("workflow.json").display()
    );
    driver.input.finish();
    photo_document.window.destroy();
}
