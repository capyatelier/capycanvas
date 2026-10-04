//! Healing oracles through the real engine: the healed copy keeps its
//! source's texture and takes the tone around the stroke, Spot Healing finds a
//! patch like the surroundings, and pen-up healing replays exactly.
use super::*;
use layer_core::{CloneSource, DefaultBrushPreset, default_brush};

const EXTENT: [u32; 2] = [1024, 512];

fn encode(linear: f32) -> u8 {
    let c = linear.clamp(0., 1.);
    let v = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1. / 2.4) - 0.055 };
    (v * 255.).round() as u8
}

fn noise(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9e37_79b1) ^ y.wrapping_mul(0x85eb_ca77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65535. * 2. - 1.
}

fn grey(v: f32) -> [u8; 4] {
    let c = encode(v);
    [c, c, c, 255]
}

fn photo(pixel: impl Fn(u32, u32) -> [u8; 4]) -> Document { photo_document(EXTENT, pixel) }

fn healer(doc: Document, preset: DefaultBrushPreset, diameter: f32, feedback: bool) -> (InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>) {
    let extent = doc.composition().size;
    let r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let (input, consumer) = input_queue(256);
    let mut engine = CanvasEngine::new(r, doc, consumer, view(extent), ViewTransform::IDENTITY).unwrap();
    engine
        .set_brush(BrushSnapshot { diameter, hardness: 1., opacity: 1., mappings: Arc::from([]), ..default_brush(preset) })
        .unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig { enabled: feedback, ..Default::default() }).unwrap();
    engine.set_retouch(Some(RetouchSource::References));
    flush(&mut engine);
    (input, engine)
}

#[test]
#[ignore = "24 MP physical GPU memory measurement"]
fn broad_spot_healing_has_bounded_memory() {
    let extent = [6000, 4000];
    let mut doc = document(extent);
    set_blend(&mut doc, layer_core::BlendSpace::Perceptual);
    let (mut input, mut engine) = healer(doc, DefaultBrushPreset::SpotHealingBrush, 512., false);
    let mut fit = view(extent);
    fit.width_px = 1500;
    fit.height_px = 1000;
    fit.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
    engine.set_view(fit, ViewTransform { revision: 1, surface_to_document: [4., 0., 0., 4., 0., 0.] });
    flush(&mut engine);
    engine.backend_mut().wait_idle().unwrap();
    let device = engine.backend().device.clone();
    let allocated = |device: &wgpu::Device| device.generate_allocator_report().unwrap().allocations.iter().map(|a| a.size).sum::<u64>();
    let before = allocated(&device);
    let started = std::time::Instant::now();
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = finished.clone();
    let monitor = std::thread::spawn(move || {
        let mut peak = before;
        while !done.load(std::sync::atomic::Ordering::Acquire) {
            peak = peak.max(allocated(&device));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        peak
    });
    for step in 0..=120 {
        let angle = step as f32 * std::f32::consts::TAU / 120.;
        let phase = if step == 0 { PenPhase::Down } else if step == 120 { PenPhase::Up } else { PenPhase::Move };
        let mut event = pen(step + 1, phase, [750. + 450. * angle.cos(), 500. + 280. * angle.sin()], SampleFlags::PRIMARY);
        event.view_revision = 1;
        draw(&mut engine, &mut input, event);
        engine.backend_mut().wait_idle().unwrap();
    }
    flush(&mut engine);
    engine.backend_mut().wait_idle().unwrap();
    finished.store(true, std::sync::atomic::Ordering::Release);
    let peak = monitor.join().unwrap();
    eprintln!("broad spot heal: {:.1} ms, before {:.1} MiB, peak {:.1} MiB, after {:.1} MiB, {:?}",
        started.elapsed().as_secs_f64() * 1000., before as f64 / 1048576., peak as f64 / 1048576.,
        allocated(&engine.backend().device) as f64 / 1048576., counts(&engine));
    assert_eq!(counts(&engine).heals, 1);
    assert!(peak <= before + (1024 << 20), "healing scratch exceeds one GiB");
}

fn source_at(engine: &mut CanvasEngine<WgpuRasterizer>, x: f32, y: f32) {
    engine.set_clone_source(CloneSource { point: Some(Point { x, y }), ..CloneSource::default() });
}

#[test]
fn spot_healing_batches_resident_candidates_across_pages() {
    let (mut input, mut engine) = healer(document([2048, 1024]), DefaultBrushPreset::SpotHealingBrush, 512., false);
    for step in 0..20 {
        draw(&mut engine, &mut input, pen(step + 1, if step == 0 { PenPhase::Down } else { PenPhase::Move },
            [600. + step as f32 * 40., 450. + step as f32 * 8.], SampleFlags::PRIMARY));
    }
    for page in &mut engine.backend_mut().paint_layers.iter_mut().find(|l| l.id == TARGET).unwrap().pages {
        page.discard_inactive();
    }
    let before = engine.backend().metrics.command_passes;
    draw(&mut engine, &mut input, pen(21, PenPhase::Up, [1360., 602.], SampleFlags::PRIMARY));
    let pages = &engine.backend().paint_layers.iter().find(|l| l.id == TARGET).unwrap().pages;
    assert!(pages.iter().filter(|p| p.secondary.is_some()).count() < pages.len(), "pen-up allocates every Healing destination before its batch");
    flush(&mut engine);
    assert_eq!(counts(&engine).heals, 1);
    let pages = engine.backend().paint_layers.iter().find(|l| l.id == TARGET).unwrap().pages.len() as u64;
    let passes = engine.backend().metrics.command_passes - before;
    assert!(pages >= 12);
    assert!(passes <= 6 * pages + 80, "{passes} passes for {pages} pages");
}

/// The target's pixels, read back page by page.
fn target(r: &WgpuRasterizer) -> impl Fn(u32, u32) -> [f32; 4] + use<> {
    let pages: std::collections::BTreeMap<_, _> = target_pages(r).into_iter().collect();
    move |x, y| pages.get(&[x / PAGE_SIZE, y / PAGE_SIZE]).map_or([0.; 4], |p| p[((y % PAGE_SIZE) * PAGE_SIZE + x % PAGE_SIZE) as usize])
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

fn variance(values: &[f32]) -> f32 {
    let m = mean(values);
    values.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / values.len() as f32
}

/// Each value less the mean of its 5 x 5 neighborhood.
fn high_pass(value: impl Fn(u32, u32) -> f32, points: &[(u32, u32)]) -> Vec<f32> {
    points
        .iter()
        .map(|&(x, y)| {
            let mut sum = 0.;
            for dy in 0..5 {
                for dx in 0..5 {
                    sum += value(x + dx - 2, y + dy - 2);
                }
            }
            value(x, y) - sum / 25.
        })
        .collect()
}

fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let (ma, mb) = (mean(a), mean(b));
    let covariance: f32 = a.iter().zip(b).map(|(a, b)| (a - ma) * (b - mb)).sum();
    covariance / (variance(a) * variance(b)).sqrt() / a.len() as f32
}

/// Noise on 0.3 grey left of x = 512, a 0.6 to 0.8 vertical gradient right.
fn texture_and_gradient(x: u32, y: u32) -> f32 {
    if x < 512 { 0.3 + 0.08 * noise(x, y) } else { 0.6 + 0.2 * y as f32 / EXTENT[1] as f32 }
}

#[test]
fn healing_keeps_the_source_texture_and_takes_the_destination_tone() {
    for blend_space in layer_core::BlendSpace::ALL {
        heals_texture_and_tone(blend_space);
    }
}

fn heals_texture_and_tone(blend_space: layer_core::BlendSpace) {
    let mut doc = photo(|x, y| grey(texture_and_gradient(x, y)));
    set_blend(&mut doc, blend_space);
    let (mut input, mut engine) = healer(doc, DefaultBrushPreset::HealingBrush, 64., false);
    source_at(&mut engine, 150., 256.);
    stroke(&mut engine, &mut input, 1, [600., 256.], [880., 256.]);
    let healed = target(engine.backend());
    let decoded = |x: u32, y: u32| linear(encode(texture_and_gradient(x, y)));
    let offset = -450;
    let interior: Vec<(u32, u32)> = (640..840).flat_map(|x| (244..=268).map(move |y| (x, y))).collect();
    assert!(interior.iter().all(|&(x, y)| healed(x, y)[3] > 0.999), "the interior is fully healed");
    for x0 in (640..832).step_by(16) {
        let block: Vec<_> = (x0..x0 + 16).flat_map(|x| (248..264).map(move |y| (x, y))).collect();
        let got = mean(&block.iter().map(|&(x, y)| healed(x, y)[1]).collect::<Vec<_>>());
        let wanted = mean(&block.iter().map(|&(x, y)| decoded(x, y)).collect::<Vec<_>>());
        assert!((got - wanted).abs() < 0.01, "{blend_space:?} block at {x0}: mean {got} against the destination's {wanted}");
    }
    let result = high_pass(|x, y| healed(x, y)[1], &interior);
    let source = high_pass(|x, y| decoded((x as i32 + offset) as u32, y), &interior);
    let r = correlation(&result, &source);
    assert!(r > 0.9, "{blend_space:?}: the healed texture follows its source: correlation {r}");
    assert_eq!(counts(&engine).heals, 1);
}

#[test]
fn healing_pen_up_refreshes_every_painted_display_page() {
    for preset in [DefaultBrushPreset::HealingBrush, DefaultBrushPreset::SpotHealingBrush] {
        let doc = photo(|x, y| grey(texture_and_gradient(x, y)));
        let (mut input, mut engine) = healer(doc, preset, 64., false);
        let mut fit = view(EXTENT);
        fit.document_to_surface = [0.25, 0., 0., 0.25, 0., 0.];
        engine.set_view(fit, ViewTransform { revision: 1, surface_to_document: [4., 0., 0., 4., 0., 0.] });
        source_at(&mut engine, 150., 256.);
        for i in 0..9 {
            let phase = if i == 0 { PenPhase::Down } else if i == 8 { PenPhase::Up } else { PenPhase::Move };
            let mut event = pen(i + 1, phase, [(580. + i as f32 * 40.) * 0.25, 64.], SampleFlags::PRIMARY);
            event.view_revision = 1;
            draw(&mut engine, &mut input, event);
        }
        flush(&mut engine);
        assert_eq!(counts(&engine).heals, 1);
        let r = engine.backend();
        let cache = r.scale_display.as_ref().unwrap();
        assert_eq!(cache.plan.level, 2);
        let output = floats(&crate::layer_tests::page_bytes(r, cache.texture()));
        let healed = target(r);
        for y in 62..66 {
            for x in 152..180 {
                let mut expected = [0.; 4];
                for dy in 0..4 { for dx in 0..4 {
                    let p = healed(x * 4 + dx, y * 4 + dy);
                    assert!(p[3] > 0.999);
                    for c in 0..4 { expected[c] += p[c] / 16.; }
                }}
                let actual = output[(y * cache.plan.size[0] + x) as usize];
                assert!(close(actual, expected), "{preset:?} ({x},{y}): displayed {actual:?}, healed {expected:?}");
            }
        }
    }
}

#[test]
fn abandoning_a_heal_resolves_its_pending_revision() {
    let (mut input, mut engine) = healer(photo(|x, y| grey(texture_and_gradient(x, y))), DefaultBrushPreset::HealingBrush, 64., false);
    source_at(&mut engine, 150., 256.);
    draw(&mut engine, &mut input, pen(1, PenPhase::Down, [600., 256.], SampleFlags::PRIMARY));
    draw(&mut engine, &mut input, pen(2, PenPhase::Up, [620., 256.], SampleFlags::PRIMARY));
    let root = engine.document().target_raster(TARGET).unwrap().clone();
    assert!(root.try_data().is_none());
    drop(engine);
    assert!(root.try_data().is_some_and(|data| data.is_err()));
}

#[test]
fn settling_preserves_sources_while_navigation_and_queued_paint_wait_for_publication() {
    for preset in [DefaultBrushPreset::HealingBrush, DefaultBrushPreset::SpotHealingBrush] {
        let doc = photo(|x, y| grey(texture_and_gradient(x, y)));
        let (mut input, mut engine) = healer(doc, preset, 64., false);
        source_at(&mut engine, 150., 256.);
        for i in 0..9 {
            let phase = if i == 0 { PenPhase::Down } else if i == 8 { PenPhase::Up } else { PenPhase::Move };
            draw(&mut engine, &mut input, pen(i + 1, phase, [580. + i as f32 * 40., 256.], SampleFlags::PRIMARY));
        }
        assert!(engine.backend().has_pending_submission());
        assert!(!engine.backend().can_submit());
        assert!(!engine.can_undo());
        let root = engine.document().target_raster(TARGET).unwrap().clone();
        assert!(root.try_data().is_none());
        engine.set_brush(BrushSnapshot { diameter: 8., ..default_brush(DefaultBrushPreset::GPen) }).unwrap();
        engine.set_retouch(None);
        for event in [pen(20, PenPhase::Down, [700., 300.], SampleFlags::PRIMARY), pen(21, PenPhase::Up, [710., 300.], SampleFlags::PRIMARY)] {
            input.push(event).unwrap();
            engine.capture_queued_contact(event);
        }
        let consumed = engine.metrics().input_events;
        for revision in 1..80 {
            let mut moved = view(EXTENT);
            moved.document_to_surface[4] = revision as f32;
            engine.set_view(moved, ViewTransform { revision, surface_to_document: [1., 0., 0., 1., -(revision as f32), 0.] });
            engine.render_frame().unwrap();
            assert_eq!(engine.metrics().input_events, consumed);
            assert!(root.try_data().is_none());
        }
        flush(&mut engine);
        assert!(root.try_data().is_some_and(|data| data.is_ok()));
        assert_eq!(engine.metrics().committed_strokes, 2);
        assert!(engine.undo().unwrap());
        flush(&mut engine);
        assert!(engine.undo().unwrap());
        flush(&mut engine);
        assert!(engine.redo().unwrap());
        flush(&mut engine);
        assert!(engine.redo().unwrap());
        flush(&mut engine);
    }
}

#[test]
fn with_matching_surroundings_healing_is_the_clone() {
    let periodic = |x: u32, y: u32| grey(0.4 + 0.2 * noise(x % 32, y % 32));
    for blend_space in layer_core::BlendSpace::ALL {
        let mut pages = Vec::new();
        for preset in [DefaultBrushPreset::CloneStamp, DefaultBrushPreset::HealingBrush] {
            let mut doc = photo(periodic);
            set_blend(&mut doc, blend_space);
            let (mut input, mut engine) = healer(doc, preset, 48., false);
            source_at(&mut engine, 216., 144.);
            stroke(&mut engine, &mut input, 1, [600., 144.], [860., 300.]);
            pages.push(target_pages(engine.backend()));
        }
        assert!(pages[0].iter().any(|(_, page)| page.iter().any(|p| p[3] > 0.)), "{blend_space:?}: the clone painted");
        assert_eq!(pages[0], pages[1], "{blend_space:?}: a membrane of zero leaves the clone exactly");
    }
}

#[test]
fn spot_healing_replaces_a_dot_with_texture_like_its_surroundings() {
    let dot = |x: u32, y: u32| (x as f32 - 520.).hypot(y as f32 - 250.) < 12.;
    let texture = |x: u32, y: u32| if dot(x, y) { 0.05 } else { 0.5 + 0.1 * noise(x, y) };
    for blend_space in layer_core::BlendSpace::ALL {
        let mut doc = photo(|x, y| grey(texture(x, y)));
        set_blend(&mut doc, blend_space);
        let (mut input, mut engine) = healer(doc, DefaultBrushPreset::SpotHealingBrush, 40., false);
        stroke(&mut engine, &mut input, 1, [516., 250.], [524., 250.]);
        let healed = target(engine.backend());
        let spot: Vec<_> = (500..540).flat_map(|x| (230..270).map(move |y| (x, y))).filter(|&(x, y)| dot(x, y)).collect();
        let around: Vec<_> = (440..600).flat_map(|x| (170..330).map(move |y| (x, y))).filter(|&(x, y)| (x as f32 - 520.).hypot(y as f32 - 250.) > 40.).collect();
        let inside: Vec<f32> = spot.iter().map(|&(x, y)| healed(x, y)[1]).collect();
        let outside: Vec<f32> = around.iter().map(|&(x, y)| linear(encode(texture(x, y)))).collect();
        assert!(spot.iter().all(|&(x, y)| healed(x, y)[3] > 0.999), "{blend_space:?}");
        assert!((mean(&inside) - mean(&outside)).abs() < 0.01, "{blend_space:?}: mean {} against {}", mean(&inside), mean(&outside));
        let ratio = variance(&inside) / variance(&outside);
        assert!((0.75..1.33).contains(&ratio), "{blend_space:?}: variance ratio {ratio}");
        assert_eq!(counts(&engine).heals, 1);
    }
}

#[test]
fn healing_matches_tone_on_the_documents_values() {
    let code = |encoded: f32| (encoded * 255.).round() as u8;
    let texture = |x: u32, y: u32| {
        let c = code(if x < 512 { 0.5 + 0.35 * noise(x, y) } else { 0.4 });
        [c, c, c, 255]
    };
    let destination = f32::from(code(0.4)) / 255.;
    let healed_tone = |blend_space| {
        let mut doc = photo(texture);
        set_blend(&mut doc, blend_space);
        let (mut input, mut engine) = healer(doc, DefaultBrushPreset::HealingBrush, 64., false);
        source_at(&mut engine, 150., 256.);
        stroke(&mut engine, &mut input, 1, [600., 256.], [880., 256.]);
        let healed = target(engine.backend());
        let interior: Vec<f32> = (640..840)
            .flat_map(|x| (248..264).map(move |y| (x, y)))
            .map(|(x, y)| layer_core::color::RgbSpace::Srgb.encode(f64::from(healed(x, y)[1])) as f32)
            .collect();
        mean(&interior)
    };
    let perceptual = healed_tone(layer_core::BlendSpace::Perceptual);
    assert!((perceptual - destination).abs() < 0.01, "Perceptual heals to {perceptual}, not {destination}");
    let linear = healed_tone(layer_core::BlendSpace::Linear);
    assert!((linear - destination).abs() > 0.03, "Linear light matches mean light, so its encoded tone {linear} differs");
}

#[test]
fn healing_replays_from_estimates_and_corrections_match_a_direct_stroke() {
    let path = [[580., 190.], [630., 210.], [690., 200.], [750., 240.]];
    for preset in [DefaultBrushPreset::HealingBrush, DefaultBrushPreset::SpotHealingBrush] {
        let mut results = Vec::new();
        for late in [false, true] {
            let doc = photo(|x, y| grey(texture_and_gradient(x, y)));
            let (mut input, mut engine) = healer(doc, preset, 40., true);
            source_at(&mut engine, 200., 300.);
            corrected_replay(&mut engine, &mut input, &path, [760., 246.], late);
            if late { assert!(counts(&engine).heals >= 2, "{preset:?}: the correction heals again"); }
            results.push(target_pages(engine.backend()));
        }
        assert!(results[0].iter().any(|(_, page)| page.iter().any(|p| p[3] > 0.)), "{preset:?} painted");
        assert_eq!(results[0], results[1], "{preset:?}: corrections replay to the direct stroke");
    }
}

#[test]
fn each_healing_stroke_is_one_undo_step() {
    for preset in [DefaultBrushPreset::HealingBrush, DefaultBrushPreset::SpotHealingBrush] {
        let doc = photo(|x, y| grey(texture_and_gradient(x, y)));
        let (mut input, mut engine) = healer(doc, preset, 40., false);
        source_at(&mut engine, 200., 300.);
        stroke(&mut engine, &mut input, 1, [600., 200.], [700., 200.]);
        let first = target_pages(engine.backend());
        stroke(&mut engine, &mut input, 20, [600., 300.], [700., 300.]);
        assert_ne!(target_pages(engine.backend()), first);
        assert!(engine.undo().unwrap());
        flush(&mut engine);
        assert_eq!(target_pages(engine.backend()), first, "{preset:?}: one undo removes the second heal");
        assert!(engine.undo().unwrap());
        flush(&mut engine);
        assert!(target_pages(engine.backend()).iter().all(|(_, page)| page.iter().all(|p| p[3] == 0.)), "{preset:?}");
        assert!(!engine.can_undo());
    }
}
