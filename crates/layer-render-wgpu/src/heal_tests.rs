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

/// Layer 1, empty, over a photo marked as a reference.
fn photo(pixel: impl Fn(u32, u32) -> [u8; 4]) -> Document {
    let mut doc = Document::new("heal", EXTENT[0], EXTENT[1]);
    assert_eq!(doc.allocate_layer_id(), PHOTO);
    let mut layer = Layer::paint(PHOTO, "Photo");
    layer.source = Some(rgba8_source(EXTENT, pixel));
    doc.layers.insert(1, layer);
    doc.reference_layers = [PHOTO].into();
    doc
}

fn healer(doc: Document, preset: DefaultBrushPreset, diameter: f32, feedback: bool) -> (InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>) {
    let r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    let (input, consumer) = input_queue(256);
    let mut engine = CanvasEngine::new(r, doc, consumer, view(EXTENT), ViewTransform::IDENTITY).unwrap();
    engine
        .set_brush(BrushSnapshot { diameter, hardness: 1., opacity: 1., mappings: Arc::from([]), ..default_brush(preset) })
        .unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig { enabled: feedback, ..Default::default() }).unwrap();
    engine.set_retouch(Some(RetouchSource::References));
    flush(&mut engine);
    (input, engine)
}

fn stroke(engine: &mut CanvasEngine<WgpuRasterizer>, input: &mut InputProducer<PenEvent>, sequence: u64, from: [f32; 2], to: [f32; 2]) {
    draw(engine, input, pen(sequence, PenPhase::Down, from, SampleFlags::PRIMARY));
    for i in 1..8 {
        let t = i as f32 / 8.;
        let at = [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t];
        draw(engine, input, pen(sequence + i, PenPhase::Move, at, SampleFlags::PRIMARY));
    }
    draw(engine, input, pen(sequence + 8, PenPhase::Up, to, SampleFlags::PRIMARY));
    flush(engine);
}

fn source_at(engine: &mut CanvasEngine<WgpuRasterizer>, x: f32, y: f32) {
    engine.set_clone_source(CloneSource { point: Some(Point { x, y }), ..CloneSource::default() });
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
    doc.blend_space = blend_space;
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
fn with_matching_surroundings_healing_is_the_clone() {
    let periodic = |x: u32, y: u32| grey(0.4 + 0.2 * noise(x % 32, y % 32));
    for blend_space in layer_core::BlendSpace::ALL {
        let mut pages = Vec::new();
        for preset in [DefaultBrushPreset::CloneStamp, DefaultBrushPreset::HealingBrush] {
            let mut doc = photo(periodic);
            doc.blend_space = blend_space;
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
        doc.blend_space = blend_space;
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
        doc.blend_space = blend_space;
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
    let estimated = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
    let path = [[580., 190.], [630., 210.], [690., 200.], [750., 240.]];
    let corrected = |mut event: PenEvent| {
        event.pressure = 0.4;
        event.surface_position.y += 6.;
        event
    };
    for preset in [DefaultBrushPreset::HealingBrush, DefaultBrushPreset::SpotHealingBrush] {
        let mut results = Vec::new();
        for late in [false, true] {
            let doc = photo(|x, y| grey(texture_and_gradient(x, y)));
            let (mut input, mut engine) = healer(doc, preset, 40., true);
            source_at(&mut engine, 200., 300.);
            let events: Vec<_> = path
                .iter()
                .enumerate()
                .map(|(i, &at)| {
                    let phase = if i == 0 { PenPhase::Down } else { PenPhase::Move };
                    pen(1 + i as u64, phase, at, if late { estimated } else { SampleFlags::PRIMARY })
                })
                .collect();
            for (i, event) in events.iter().enumerate() {
                draw(&mut engine, &mut input, if late { *event } else { corrected(*event) });
                if late && i == 1 {
                    let mut fix = corrected(events[0]);
                    fix.flags = SampleFlags(SampleFlags::CORRECTION.0 | SampleFlags::ESTIMATED.0);
                    draw(&mut engine, &mut input, fix);
                }
            }
            draw(&mut engine, &mut input, pen(9, PenPhase::Up, [760., 246.], SampleFlags::PRIMARY));
            flush(&mut engine);
            if late {
                for event in &events {
                    let mut fix = corrected(*event);
                    fix.flags = SampleFlags::CORRECTION;
                    draw(&mut engine, &mut input, fix);
                }
                flush(&mut engine);
                assert!(counts(&engine).heals >= 2, "{preset:?}: the correction heals again");
            }
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

#[test]
#[ignore = "hardware pen-up healing benchmark; release, serial"]
fn heal_pen_up_cost() {
    let extent = [6000, 4000];
    let doc = |preset| {
        let mut doc = Document::new("heal cost", extent[0], extent[1]);
        assert_eq!(doc.allocate_layer_id(), PHOTO);
        let mut layer = Layer::paint(PHOTO, "Photo");
        layer.source = Some(rgba8_source(extent, pattern));
        doc.layers.insert(1, layer);
        doc.reference_layers = [PHOTO].into();
        (doc, preset)
    };
    let zoom = 1600. / 6000.;
    for (label, diameter, from, to) in [
        ("A 60 px spot", 60., [2500., 2500.], [2530., 2500.]),
        ("0.25 MP", 180., [2560., 2560.], [2570., 2560.]),
        ("1 MP", 500., [2400., 2400.], [2700., 2700.]),
        ("4 MP", 900., [2000., 2560.], [5000., 2560.]),
    ] {
        for (doc, preset) in [doc(DefaultBrushPreset::CloneStamp), doc(DefaultBrushPreset::HealingBrush), doc(DefaultBrushPreset::SpotHealingBrush)] {
            let fit = ViewState { width_px: 1600, height_px: 1067, document_to_surface: [zoom, 0., 0., zoom, 0., 0.], background_rgba_linear: [0.; 4] };
            let r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
            let (mut input, consumer) = input_queue(1024);
            let transform = ViewTransform { revision: 0, surface_to_document: [1. / zoom, 0., 0., 1. / zoom, 0., 0.] };
            let mut engine = CanvasEngine::new(r, doc, consumer, fit, transform).unwrap();
            engine
                .set_brush(BrushSnapshot { diameter, hardness: 0.6, mappings: Arc::from([]), ..default_brush(preset) })
                .unwrap();
            engine.set_instant_feedback(InstantFeedbackConfig { enabled: false, ..Default::default() }).unwrap();
            engine.set_retouch(Some(RetouchSource::References));
            engine.set_clone_source(CloneSource { point: Some(Point { x: from[0] - 1200., y: from[1] + 900. }), ..CloneSource::default() });
            flush(&mut engine);
            while engine.backend().has_pending_work() {
                timed_frame(&mut engine, &mut Vec::new());
            }
            let mut pen_up = Vec::new();
            for repeat in 0..4u64 {
                for i in 0..24u64 {
                    let t = i as f32 / 23.;
                    let at = [(from[0] + (to[0] - from[0]) * t) * zoom, (from[1] + (to[1] - from[1]) * t) * zoom];
                    let phase = match i { 0 => PenPhase::Down, 23 => PenPhase::Up, _ => PenPhase::Move };
                    input.push(pen(1 + repeat * 100 + i, phase, at, SampleFlags::PRIMARY)).unwrap();
                    let start = std::time::Instant::now();
                    engine.render_frame().unwrap();
                    let cpu = start.elapsed().as_secs_f64() * 1000.;
                    engine.backend_mut().wait_idle().unwrap();
                    if phase == PenPhase::Up {
                        pen_up.push([cpu, start.elapsed().as_secs_f64() * 1000.]);
                    }
                }
                flush(&mut engine);
            }
            let damage = engine.backend().paint_layers.iter().find(|l| l.id == TARGET).map_or(0, |l| l.coverage_pages.len());
            let captures = engine.backend().retouch.as_ref().map_or(0, |r| r.counts.captures);
            eprintln!(
                "{label} {preset:?} pen-up ({damage} pages, {captures} reference captures): CPU ms {:.2?}; CPU and GPU ms {:.2?}",
                pen_up.iter().map(|s| s[0]).collect::<Vec<_>>(),
                pen_up.iter().map(|s| s[1]).collect::<Vec<_>>()
            );
        }
    }
}
