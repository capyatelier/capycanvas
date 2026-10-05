//! Camera requests through the production GTK owner/worker/presentation path.
//! Synthetic software gestures do not measure physical device input latency.
use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace, source::*};
use std::sync::Arc;
use layer_core::authored::{PortableId, Occurrence, OccurrenceContent, OccurrenceHandle, PaintSource, Definition, EffectApplication, Stack};

pub(super) fn photo(extent: [u32; 2]) -> layer_core::Document {
    let depth = match std::env::var("LAYER_NAVIGATION_HDR").as_deref() { Ok("32") => SampleDepth::F32, Ok("1") => SampleDepth::F16, _ => SampleDepth::U16 };
    let hdr = depth.is_float();
    let mut project = new_drawing(1, 1, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    composition_mut(&mut project).size = extent;
    composition_mut(&mut project).color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth,
    };
    composition_mut(&mut project).blend = match std::env::var("LAYER_PHOTO_BLENDING").as_deref() {
        Ok("perceptual") => layer_core::BlendSpace::Perceptual.for_depth(depth),
        _ => layer_core::BlendSpace::Linear,
    };
    let mut source = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth,
            profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
            profile_assumed: false,
        },
        if depth == SampleDepth::F32 { 1024 * 1024 * 1024 } else { 512 * 1024 * 1024 },
    )
    .unwrap();
    let mut random = 0x1357abcdu32;
    let mut row = Vec::with_capacity(extent[0] as usize * 4 * depth.bytes());
    for y in 0..extent[1] {
        row.clear();
        for x in 0..extent[0] {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let noise = (random % 1024) as u16;
            for (channel, code) in [
                (u64::from(x) * 55000 / u64::from(extent[0])) as u16 + noise,
                (u64::from(y) * 55000 / u64::from(extent[1])) as u16 + noise,
                ((u64::from(x) + u64::from(y)) * 13 % 60000) as u16 + noise,
                65535,
            ].into_iter().enumerate() {
                if depth == SampleDepth::F32 {
                    let value = if channel == 3 { 1. } else { (RgbSpace::ProPhoto.decode(f64::from(code) / 65535.) * 8. - 0.125) as f32 };
                    row.extend_from_slice(&value.to_le_bytes());
                    continue;
                }
                let bits = if hdr {
                    layer_core::color::f16::from_f32(if channel == 3 { 1. }
                        else { (RgbSpace::ProPhoto.decode(f64::from(code) / 65535.) * 8. - 0.125) as f32 }).to_bits()
                } else { code };
                row.extend_from_slice(&bits.to_le_bytes());
            }
        }
        source.push_row(&row).unwrap();
    }
    let active = project.working.occurrence.unwrap();
    let OccurrenceContent::Paint(paint) = project.scene().occurrence(active).unwrap().content else { unreachable!() };
    let source_image = Arc::new(source.finish().unwrap());
    let paint = project.artwork.paint.get_mut(paint).unwrap();
    paint.domain = extent;
    paint.original = Some(source_image);
    for _ in 0..31 {
        insert_paint(&mut project, "empty", None, 1);
    }
    for (name, key, value) in [
        ("exposure", "exposure", 0.25),
        ("white_balance", "temperature", 4.),
        ("levels", "gamma", 1.08),
        ("hue_saturation", "saturation", 5.),
        ("color_balance", "midtones_red", 2.),
    ].into_iter().chain(
        (std::env::var("LAYER_NAVIGATION_PHYSICAL").as_deref() == Ok("1"))
            .then_some(("gaussian_blur", "sigma", 4.)),
    ) {
        let mut effect = layer_core::EffectInstance::new(
            layer_core::bundled_effect_catalog()
                .get(name)
                .unwrap()
                .program(),
        );
        effect
            .set(key, layer_core::EffectValue::Number(value))
            .unwrap();
        insert_effect(&mut project, name, effect, 0);
    }
    if std::env::var("LAYER_NAVIGATION_LONG_CHAIN").as_deref() == Ok("1") {
        for index in 0..24 {
            let mut effect = layer_core::EffectInstance::new(
                layer_core::bundled_effect_catalog().get("exposure").unwrap().program());
            effect.set("exposure", layer_core::EffectValue::Number(if index % 2 == 0 { 0.25 } else { -0.25 })).unwrap();
            insert_effect(&mut project, "HDR long chain", effect, 0);
        }
    }
    project.validate(Default::default()).unwrap();
    project
}

fn refresh(document: &mut layer_core::Document) {
    let working = document.working.clone();
    *document = layer_core::Document::from_artwork(document.artwork.clone()).unwrap();
    document.working = working;
}
fn insert_paint(document: &mut layer_core::Document, name: &str, original: Option<Arc<SourceImage>>, index: usize) -> OccurrenceHandle {
    let source = document.artwork.paint.insert(PortableId::random(), PaintSource { color_mode: Default::default(),domain:document.composition().size, original, raster:Default::default(), operations:Default::default()}).unwrap();
    let occurrence = document.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(source),name)).unwrap();
    document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.insert(index,occurrence);
    refresh(document);
    occurrence
}
fn insert_effect(document: &mut layer_core::Document, name: &str, draft: layer_core::EffectInstance, index: usize) -> OccurrenceHandle {
    let definition = document.artwork.definitions.insert(PortableId::random(),Definition {program:draft.program}).unwrap();
    let effect = document.artwork.effects.insert(PortableId::random(),EffectApplication {definition,values:draft.values}).unwrap();
    let occurrence = document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(effect),name)).unwrap();
    document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.insert(index,occurrence);
    refresh(document);
    occurrence
}
fn retain_photos(document: &mut layer_core::Document) {
    let photos: Vec<_> = document.scene().children(None).iter().copied().filter(|h|document.scene().paint_source(*h).is_some_and(|p|p.original.is_some())).collect();
    document.artwork.stacks.get_mut(document.composition().result).unwrap().entries = photos;
    refresh(document);
}
pub(super) fn layered(document: &mut layer_core::Document) {
    retain_photos(document);
    let photo = document.scene().children(None)[0];
    let extent = document.composition().size;
    let depth = document.composition().color.depth;
    let source = |pixel: &dyn Fn(u32, u32) -> [u16; 4]| {
        let mut builder = SourceBuilder::new(
            extent,
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth: SampleDepth::U16,
                profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
                profile_assumed: false,
            },
            512 * 1024 * 1024,
        )
        .unwrap();
        let mut row = Vec::with_capacity(extent[0] as usize * 8);
        for y in 0..extent[1] {
            row.clear();
            for x in 0..extent[0] {
                for code in pixel(x, y) {
                    row.extend_from_slice(&code.to_le_bytes());
                }
            }
            builder.push_row(&row).unwrap();
        }
        Arc::new(builder.finish().unwrap())
    };
    assert_eq!(depth, SampleDepth::U16, "the layered fixture paints 16-bit sources");
    let strokes = source(&|x, y| {
        let band = (x + 2 * y) % 900;
        let alpha = if band < 60 { 65535 } else if band < 80 { ((80 - band) * 3276) as u16 } else { 0 };
        [52000, 21000, 9000, alpha]
    });
    insert_paint(document, "strokes", Some(strokes), 0);
    let backdrop = source(&|x, y| {
        [(x * 11 % 50000) as u16, (y * 13 % 50000) as u16, ((x ^ y) % 40000) as u16, 65535]
    });
    insert_paint(document, "backdrop", Some(backdrop), 2);
    document.working.occurrence = Some(photo);
    document.working.target = document.scene().source_target(photo);
}
pub(super) fn blended(document: &mut layer_core::Document) {
    layered(document);
    let copy = |document: &mut layer_core::Document, index: usize, name: &str, blend, opacity| {
        let mut occurrence = document.scene().occurrence(document.scene().children(None)[index]).unwrap().clone();
        let OccurrenceContent::Paint(source)=occurrence.content else {unreachable!()};
        let source=document.artwork.paint.get(source).unwrap().clone();
        let source=document.artwork.paint.insert(PortableId::random(),source).unwrap();
        occurrence.content=OccurrenceContent::Paint(source);
        occurrence.name = name.into(); occurrence.blend = blend; occurrence.opacity = opacity;
        document.artwork.occurrences.insert(PortableId::random(),occurrence).unwrap()
    };
    let tone = copy(document, 2, "tone", layer_core::LayerBlend::SoftLight, 0.6);
    let color = copy(document, 0, "color", layer_core::LayerBlend::Color, 0.7);
    let strokes = document.scene().children(None)[0];
    document.artwork.occurrences.get_mut(strokes).unwrap().blend = layer_core::LayerBlend::Screen;
    let root = document.composition().result;
    let entries = &mut document.artwork.stacks.get_mut(root).unwrap().entries;
    entries.insert(1,tone); entries.insert(0,color);
    refresh(document);
}
pub(super) fn pass_through(document: &mut layer_core::Document) {
    blended(document);
    let mut effect = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("hue_saturation").unwrap().program());
    effect.set("saturation", layer_core::EffectValue::Number(25.)).unwrap();
    insert_effect(document,"hue_saturation",effect,0);
    let root = document.composition().result;
    let children: Vec<_> = document.artwork.stacks.get_mut(root).unwrap().entries.drain(..4).collect();
    let stack = document.artwork.stacks.insert(PortableId::random(),Stack {entries:children}).unwrap();
    let mut group = Occurrence::new(OccurrenceContent::Stack(stack),"pass through"); group.blend = layer_core::LayerBlend::PassThrough;
    let group = document.artwork.occurrences.insert(PortableId::random(),group).unwrap();
    document.artwork.stacks.get_mut(root).unwrap().entries.insert(0,group);
    refresh(document);
}

#[test]
#[ignore = "private 120 Hz Wayland display; release hardware navigation qualification"]
fn native_large_photo_navigation() {
    let extent = match std::env::var("LAYER_NAVIGATION_PHOTO")
        .as_deref()
        .unwrap_or("60mp")
    {
        "24mp" => [6000, 4000],
        "45mp" => [8192, 5504],
        "60mp" => [8192, 7324],
        "61mp" => [9504, 6336],
        _ => panic!("LAYER_NAVIGATION_PHOTO must be 24mp, 45mp, 60mp or 61mp"),
    };
    let app = native_test_app("art.capycanvas.PhotoNavigation");
    let mut project = photo(extent);
    match std::env::var("LAYER_NAVIGATION_LAYERS").as_deref() {
        Ok("blended") => blended(&mut project),
        Ok("pass_through") => pass_through(&mut project),
        _ => {}
    }
    let startup = Instant::now();
    let w = Workspace::with_project(&app, Some((project, None)));
    if std::env::var("LAYER_NAVIGATION_MAXIMIZE").as_deref() == Ok("0") {
        w.window.set_default_size(1200, 900);
    } else {
        w.window.maximize();
    }
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut canvas_shaders_ready_ms = None;
    let mut brush_shaders_ready_ms = None;
    let mut all_shaders_ready_ms = None;
    loop {
        pump(5);
        let ready = w.gpu.borrow().as_ref().is_some_and(|g| {
            assert!(!g.session.rendering_suspended());
            let progress = g.session.engine().backend().startup;
            if progress.canvas_ready { canvas_shaders_ready_ms.get_or_insert_with(|| startup.elapsed().as_secs_f64() * 1000.); }
            if progress.brush_ready { brush_shaders_ready_ms.get_or_insert_with(|| startup.elapsed().as_secs_f64() * 1000.); }
            if progress.complete { all_shaders_ready_ms.get_or_insert_with(|| startup.elapsed().as_secs_f64() * 1000.); }
            progress.complete
                && !g.session.engine().has_pending_document_edits()
                && w.frame_timer.borrow().is_none()
        });
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "photo startup must settle");
    }
    let startup_ms = startup.elapsed().as_secs_f64() * 1000.;
    use layer_render::CanvasRenderer;
    ui_session_mut(&w).renderer_mut().set_telemetry_enabled(true);
    let proof = super::proof::benchmark_proof(&w);
    let settle_ms = std::env::var("LAYER_NAVIGATION_SETTLE_MS")
        .ok().map(|v| v.parse::<u64>().unwrap()).unwrap_or(0);
    pump(settle_ms);
    let concurrent = (std::env::var("LAYER_NAVIGATION_CONCURRENT").as_deref() == Ok("1")).then(|| {
        let gpu = w.snapshot_gpu().unwrap();
        let snapshot = {
            let g = w.gpu.borrow();
            let session = &g.as_ref().unwrap().session;
            DocumentExport { capture: session.capture_artwork().unwrap(),
                time: session.engine().animation_time() }
        };
        let prefix = std::path::PathBuf::from(std::env::var("LAYER_PACING_REPORT").unwrap());
        std::thread::spawn(move || {
            let start = Instant::now();
            let project = prefix.with_extension("capy");
            layer_core::atomic_write(&project, |file| write_capture(&snapshot.capture,file)).unwrap();
            let saved_ms = start.elapsed().as_secs_f64() * 1000.;
            let reopened = open_native_document(std::fs::File::open(&project).unwrap());
            assert_eq!(reopened.output().proof, snapshot.output().proof);
            let recipe = ExportRecipe::further_editing(snapshot.composition().color);
            let delivery = prefix.with_extension(recipe.format.extension());
            crate::files::export::write_snapshot(gpu, snapshot, recipe, &delivery, &Default::default()).unwrap();
            serde_json::json!({"save_ms": saved_ms, "save_export_ms": start.elapsed().as_secs_f64()*1000.,
                "master_bytes": std::fs::metadata(project).unwrap().len(),
                "delivery_bytes": std::fs::metadata(delivery).unwrap().len()})
        })
    });
    let original = ui_session(&w)
        .engine()
        .document()
        .clone();
    let stats = ui_session(&w)
        .engine()
        .backend()
        .stats
        .clone();
    *stats.lock().unwrap() = Default::default();
    let viewport = state(&w).camera.viewport;
    let fit = (viewport[0] as f32 / extent[0] as f32).min(viewport[1] as f32 / extent[1] as f32);
    let context = glib::MainContext::default();
    let gtk_frames = Rc::new(RefCell::new(Vec::new()));
    let gtk_phases = Rc::new(RefCell::new(Vec::new()));
    let gtk_frame_start = Rc::new(Cell::new(0_u64));
    let frame_clock = w.window.frame_clock().unwrap();
    let before_paint = frame_clock.connect_before_paint(glib::clone!(
        #[strong] gtk_frame_start,
        move |_| gtk_frame_start.set(glib::monotonic_time() as u64 * 1000)
    ));
    let after_paint = frame_clock.connect_after_paint(glib::clone!(
        #[strong] gtk_frames,
        #[strong] gtk_frame_start,
        move |_| gtk_frames.borrow_mut().push([
            gtk_frame_start.get(), glib::monotonic_time() as u64 * 1000,
        ])
    ));
    let layout = frame_clock.connect_layout(glib::clone!(
        #[strong] gtk_phases,
        move |_| gtk_phases.borrow_mut().push(("layout", glib::monotonic_time() as u64 * 1000))
    ));
    let paint = frame_clock.connect_paint(glib::clone!(
        #[strong] gtk_phases,
        move |_| gtk_phases.borrow_mut().push(("paint", glib::monotonic_time() as u64 * 1000))
    ));
    let mut slow_iterations = Vec::new();
    // Wake the real GLib event loop without polling sleeps that add artificial
    // presentation delay. Requests use an absolute 120 Hz schedule, not a wait
    // for the previous render, so slow frames cannot throttle the workload.
    let tick = glib::timeout_add_local(Duration::from_millis(1), || glib::ControlFlow::Continue);
    let input_phase_ns = std::env::var("LAYER_NAVIGATION_PHASE_NS")
        .ok().map(|v| v.parse::<u64>().unwrap());
    let start = if let Some(phase) = input_phase_ns {
        let gpu = w.gpu.borrow();
        let clock = &gpu.as_ref().unwrap().session.engine().backend().clock;
        let now = glib::monotonic_time() as u64 * 1000;
        assert!(phase < clock.period());
        let due = clock.presentation(now) + 2 * clock.period() + phase;
        Instant::now() + Duration::from_nanos(due.saturating_sub(now))
    } else { Instant::now() };
    let mut requests = Vec::new();
    let motion_begin_ns = glib::monotonic_time() as u64 * 1000;
    for (phase, fixed_scale) in [
        ("fit-pan-rotate", fit),
        ("half-pan-rotate", 0.5),
        ("native-pan-rotate", 1.),
        ("double-pan-rotate", 2.),
        ("zoom-pan-rotate", 0.),
    ] {
        for repeat in 0..2 {
            for step in 0..96 {
                let due = start + Duration::from_secs_f64(requests.len() as f64 / 120.);
                while Instant::now() < due {
                    let before = glib::monotonic_time() as u64 * 1000;
                    context.iteration(true);
                    let after = glib::monotonic_time() as u64 * 1000;
                    if after - before > 2_000_000 {
                        slow_iterations.push([before, after]);
                    }
                }
                let angle = step as f32 / 95. * std::f32::consts::TAU;
                let scale = if fixed_scale == 0. {
                    fit * (2. / fit).powf((angle.sin() + 1.) * 0.5)
                } else {
                    fixed_scale
                };
                let center = [0.5 + 0.4 * angle.cos(), 0.5 + 0.4 * angle.sin()];
                let camera = state(&w).camera;
                let m = camera.document_to_surface();
                let [x, y] = std::array::from_fn(|i| center[i] * extent[i] as f32);
                let from = [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
                let to = viewport.map(|v| v as f32 * 0.5);
                let requested_ns = glib::monotonic_time() as u64 * 1000;
                let change = ui_session_mut(&w).gesture(
                    from,
                    to,
                    scale / camera.zoom,
                    angle - camera.rotation,
                );
                assert!(change.is_ok(), "navigation failed: {change:?}");
                w.changed(change);
                assert!(!ui_session(&w).rendering_suspended(),
                    "GPU worker stopped during {phase}, repeat {repeat}, step {step}");
                requests.push(serde_json::json!({
                    "phase": phase, "repeat": repeat, "step": step,
                    "requested_ns": requested_ns,
                    "lateness_ms": due.elapsed().as_secs_f64() * 1000.,
                    "matrix": state(&w).camera.document_to_surface(),
                }));
            }
        }
    }
    let motion_end_ns = glib::monotonic_time() as u64 * 1000;
    tick.remove();
    frame_clock.disconnect(before_paint);
    frame_clock.disconnect(after_paint);
    frame_clock.disconnect(layout);
    frame_clock.disconnect(paint);
    pump(300);
    let idle_after_300_ms = w.frame_timer.borrow().is_none();
    let deadline = Instant::now() + Duration::from_secs(30);
    while w.frame_timer.borrow().is_some() && Instant::now() < deadline { pump(5); }
    let settled_after_input_ms = (glib::monotonic_time() as u64 * 1000 - motion_end_ns) as f64 / 1e6;
    let idle = w.frame_timer.borrow().is_none();
    let suspended = ui_session(&w).rendering_suspended();
    let document_unchanged = ui_session(&w).engine().document() == &original;
    let stats = stats.lock().unwrap();
    let unchanged_work = stats.camera_work.first().is_some_and(|work|
        stats.camera_work.iter().all(|frame| frame[1] == work[1] && frame[4] == 0));
    let presented = stats.presented.iter().filter(|p| p[3] == 1).count();
    let revision_unchanged = stats
            .camera_views
            .iter()
            .all(|v| v.2 == stats.camera_views[0].2);
    let telemetry = ui_session(&w).engine().backend().telemetry();
    let mut report = serde_json::json!({
        "startup_ready_ms": startup_ms,
        "canvas_shaders_ready_ms": canvas_shaders_ready_ms,
        "brush_shaders_ready_ms": brush_shaders_ready_ms,
        "all_shaders_ready_ms": all_shaders_ready_ms,
        "motion_begin_ns": motion_begin_ns, "motion_end_ns": motion_end_ns,
        "idle_after_300_ms": idle_after_300_ms, "settled_after_input_ms": settled_after_input_ms,
        "idle_navigation": idle, "rendering_suspended": suspended,
        "document_unchanged": document_unchanged, "preview_revision_unchanged": revision_unchanged,
        "hdr": original.composition().color.depth.is_float(),
        "reference_white_nits": if original.composition().color.depth.is_float() { Some(203) } else { None },
        "effect_count": original.scene().order().iter().filter(|h| original.scene().effect(**h).is_some()).count(),
        "process_memory": process_memory(),
        "renderer_resident_bytes": telemetry.resident_bytes,
        "proof": proof,
        "extent": extent, "space": "ProPhoto", "depth": original.composition().color.depth.bits(), "viewport": viewport,
        "gtk_renderer": w.window.renderer().unwrap().type_().name(),
        "requests": requests, "camera_views": stats.camera_views,
        "camera_work": stats.camera_work,
        "monitor_scale": w.area.scale_factor(),
        "physical_filter": std::env::var("LAYER_NAVIGATION_PHYSICAL").as_deref() == Ok("1"),
        "input_phase_ns": input_phase_ns,
        "gtk_frames_ns": *gtk_frames.borrow(),
        "gtk_phases_ns": *gtk_phases.borrow(),
        "settle_ms": settle_ms,
        "slow_event_loop_iterations_ns": slow_iterations,
        "worker_cpu": stats.cpu, "worker_cpu_stages": stats.cpu_stages,
        "worker_thread_cpu": stats.thread_cpu, "worker_gpu": stats.gpu,
        "frame_handler_cpu": stats.frame_handler_cpu,
        "canvas_presentation": stats.presented,
    });
    drop(stats);
    if let Some(worker) = concurrent { report["concurrent"] = worker.join().unwrap(); }
    std::fs::write(
        std::env::var("LAYER_PACING_REPORT").unwrap(),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(idle, "idle navigation must stop requesting frames");
    assert!(!suspended);
    assert!(document_unchanged, "navigation must not change the document");
    assert!(presented > 100, "navigation presented only {presented} frames");
    assert!(revision_unchanged, "navigation must not change the artwork preview revision");
    if std::env::var("LAYER_NAVIGATION_COMPLETE").as_deref() == Ok("1") {
        assert!(unchanged_work,
            "complete display navigation must not recompose or decode source tiles");
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Wayland display; real GTK thumbnail publication and history"]
fn native_photo_thumbnail_finishes_after_idle_and_restores_on_undo() {
    let app = native_test_app("art.capycanvas.PhotoThumbnailIdle");
    let mut project = photo([2049, 1537]);
    retain_photos(&mut project);
    let original = project.clone();
    let target = u64::from(original.scene().children(None)[0].index()) + 1;
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();

    let pixels = || -> Option<Vec<u8>> {
        let button = find_css(w.layer_panel.root.upcast_ref(), "layer-thumbnail")?;
        let texture: gdk::Texture = descendant::<gtk::Picture>(&button)?.paintable()?.downcast().ok()?;
        let mut bytes = vec![0; (texture.width() * texture.height() * 4) as usize];
        texture.download(&mut bytes, texture.width() as usize * 4);
        Some(bytes)
    };
    let wait = |predicate: &dyn Fn(&[u8]) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            pump(5);
            assert!(!w.gpu.borrow().as_ref().is_some_and(|g| g.session.rendering_suspended()));
            if let Some(bytes) = pixels().filter(|bytes| predicate(bytes)) { break bytes; }
            assert!(Instant::now() < deadline, "idle thumbnail must finish and publish");
        }
    };
    let before = wait(&|_| true);
    w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Clear { id: target } });
    let cleared = wait(&|bytes| bytes != before);
    assert_ne!(before, cleared);
    click(&command(&w, CommandId::Undo));
    assert_eq!(wait(&|bytes| bytes == before), before);
    let restored = ui_session(&w).engine().document().clone();
    // Undo publishes a new document revision while restoring exact artwork.
    assert!(restored.revision > original.revision);
    assert_eq!(restored.artwork, original.artwork);
    assert_eq!(restored.working.occurrence, original.working.occurrence);
    assert_eq!(restored.working.target, original.working.target);
    assert_eq!(restored.working.selection, original.working.selection);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Wayland display; reduced spatial filter navigation"]
fn native_spatial_filter_windows() {
    let app = native_test_app("art.capycanvas.SpatialFilterWindows");
    let mut project = photo([6000, 4000]);
    retain_photos(&mut project);
    for sigma in [9., 85., 13.] {
        let mut effect = layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
        effect.set("sigma", layer_core::EffectValue::Number(sigma)).unwrap();
        insert_effect(&mut project, "Gaussian Blur", effect, 0);
    }
    let mut filter = project.scene().children(None)[0];
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    let wait = || {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            pump(10);
            let ready = w.gpu.borrow().as_ref().is_some_and(|g| {
                assert!(!g.session.rendering_suspended());
                g.session.engine().backend().startup.complete && !g.session.engine().has_pending_document_edits()
            });
            if ready && w.frame_timer.borrow().is_none() { break; }
            assert!(Instant::now() < deadline, "spatial filter canvas must finish a frame");
        }
    };
    wait();
    assert!(matches!(w.window.width(),640|1100));
    w.dispatch(UiAction::SelectLayer { id: layer_ui::occurrence_token(filter) });
    super::pointwise::configure_properties(&w);
    let mut input=RemoteInput::new().timeout_secs(30);input.ready();
    let dir = artifact_dir("../../artifacts/ui/spatial-filter-windows-gtk");
    let pixel=|point:[f32;2]| {
        let camera=state(&w).camera;let m=camera.document_to_surface();
        let image=ui_session(&w).engine().backend().capture_in(w.view_color()).unwrap();
        let x=((m[0]*point[0]+m[2]*point[1]+m[4])*image.width as f32/camera.viewport[0] as f32) as usize;
        let y=((m[1]*point[0]+m[3]*point[1]+m[5])*image.height as f32/camera.viewport[1] as f32) as usize;
        let offset=y*image.stride as usize+x*4;let value=&image.bytes[offset..offset+4];
        assert_eq!(value[3],255,"opaque filtered photograph remains visible");
        assert!(value[..3].iter().max().unwrap()-value[..3].iter().min().unwrap()>20,"filtered photograph retains color: {value:?}");
        value.to_vec()
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::SelectLayer {id:layer_ui::occurrence_token(filter)});wait();
        let before=super::pointwise::value(&w,"sigma");
        super::pointwise::edit(&w,&mut input,"sigma","85");
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});wait();
        assert_eq!(super::pointwise::value(&w,"sigma"),before);
        w.dispatch(UiAction::Invoke {command:CommandId::Redo});wait();
        assert_eq!(super::pointwise::value(&w,"sigma"),layer_core::EffectValue::Number(85.));
        let control=super::pointwise::number(&w,"sigma");
        super::histogram::scroll_to(control.upcast_ref());
        let display=find_css(control.upcast_ref(),"number-value").unwrap();
        input.click(screen_point(&display,&w.window,[0.5,0.5]));
        let entry=descendant::<gtk::Entry>(&control).unwrap();entry.set_text("64.");
        let checkpoint=ui_session(&w).engine().checkpoint();input.key(0xff1b);wait();
        assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint);
        assert_eq!(super::pointwise::value(&w,"sigma"),layer_core::EffectValue::Number(85.));
        w.dispatch(UiAction::SetZoom { zoom: 0.5 });
        wait();
        assert!((state(&w).camera.zoom - 0.5).abs() < 1e-6);
        for (step, (center, sigma)) in [([2400., 1600.], 85.), ([3300., 2100.], 85.), ([2400., 1600.], 120.), ([2400., 1600.], 0.), ([2400., 1600.], 7.)].into_iter().enumerate() {
            w.dispatch(UiAction::Effect { action: layer_ui::EffectAction::Set {
                layer: u64::from(filter.index()) + 1, key: "sigma".into(), value: layer_core::EffectValue::Number(sigma),
            } });
            let camera = state(&w).camera;
            let m = camera.document_to_surface();
            let from = [m[0] * center[0] + m[2] * center[1] + m[4], m[1] * center[0] + m[3] * center[1] + m[5]];
            let change = ui_session_mut(&w).gesture(from, camera.viewport.map(|v| v as f32 * 0.5), 1., 0.);
            w.changed(change);
            wait();
            assert_eq!(ui_session(&w).engine().document().scene().effect(filter).unwrap().value("sigma"), Some(&layer_core::EffectValue::Number(sigma)));
            pixel(center);
            crate::capture(&w, &format!("{dir}/{}-{theme:?}-{step}.png",w.window.width()));
        }
        w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Insert {effect:"unsharp_mask".into()}});wait();
        super::pointwise::edit(&w,&mut input,"sigma","85");
        pixel([2400.,1600.]);
        let document=ui_session(&w).engine().document().clone();let unsharp=document.working.occurrence.unwrap();
        let unsharp_id=document.artwork.occurrences.id(unsharp).unwrap();
        let filter_id=document.artwork.occurrences.id(filter).unwrap();
        assert_eq!(document.scene().effect(unsharp).unwrap().program.id.to_string(),"unsharp_mask");
        let (saved,bytes)=super::pointwise::saved_artwork(&w);
        let reopened=open_native_document(std::io::Cursor::new(&bytes));
        super::pointwise::assert_saved_artwork(&saved,&reopened);
        std::fs::write(format!("{dir}/{}-{theme:?}.capy",w.window.width()),bytes).unwrap();
        let activation=state(&w).document_file.epoch;
        w.documents.enqueue(&w,(reopened,None));
        super::new_photo::ready(&w);
        assert!(state(&w).document_file.epoch>activation,"spatial filter archive activation");wait();
        assert_live_artwork_eq(ui_session(&w).engine().document(),&capture_document(&saved));
        let unsharp=ui_session(&w).engine().document().artwork.occurrences.resolve(unsharp_id).unwrap();
        filter=ui_session(&w).engine().document().artwork.occurrences.resolve(filter_id).unwrap();
        w.dispatch(UiAction::SelectLayer {id:layer_ui::occurrence_token(unsharp)});wait();
        assert_eq!(super::pointwise::value(&w,"sigma"),layer_core::EffectValue::Number(85.));
        pixel([3000.,2000.]);
        crate::capture(&w,&format!("{dir}/{}-{theme:?}-unsharp85-reopened.png",w.window.width()));
        w.dispatch(UiAction::SelectLayer {id:layer_ui::occurrence_token(unsharp)});wait();
        assert_eq!(ui_session(&w).engine().document().working.occurrence,Some(unsharp));
        w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});wait();
        assert!(ui_session(&w).engine().document().scene().occurrence(unsharp).is_none());
        assert!(ui_session(&w).engine().document().scene().occurrence(filter).is_some());
    }
    input.finish();
    w.window.destroy();
    pump(100);
}
