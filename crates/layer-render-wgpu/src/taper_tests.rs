use super::*;
use crate::layer_tests::placement::{occurrence_id, paint, paint_document, paint_mut};
use crate::test_support::{view, wait_until};
use layer_core::color::LayerColorMode;
use layer_core::raster::TileKey;
use layer_core::{BrushSnapshot, BrushTaper, DefaultBrushPreset, Document, TaperMode, default_brush};
use layer_engine::{
    CanvasEngine, InputProducer, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue,
};
use std::collections::BTreeMap;

const EXTENT: [u32; 2] = [768, 768];

fn settle(engine: &mut CanvasEngine<WgpuRasterizer>) {
    wait_until("the engine must settle", || {
        engine.render_frame().unwrap();
        let pending = [
            engine.has_pending_input(),
            engine.has_pending_document_edits(),
            engine.backend().next_frame() == layer_render::FrameDemand::Work,
        ];
        if pending.contains(&true) { Err(pending) } else { Ok(()) }
    });
}

fn tiles(engine: &CanvasEngine<WgpuRasterizer>) -> BTreeMap<TileKey, Vec<u8>> {
    paint(engine.document())
        .raster
        .wait_data()
        .unwrap()
        .tiles
        .iter()
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode_on_worker().unwrap()))
        .collect()
}

fn engine(doc: Document, brush: BrushSnapshot) -> (InputProducer<PenEvent>, CanvasEngine<WgpuRasterizer>) {
    let r = crate::memory::memory_tests::renderer(
        doc.composition().color,
        layer_core::DeviceMemoryFacts { budget_bytes: Some(2 << 30), ..Default::default() },
    )
    .unwrap();
    let (input, consumer) = input_queue(256);
    let mut engine = CanvasEngine::new(r, doc, consumer, view(EXTENT), ViewTransform::IDENTITY).unwrap();
    engine.set_brush(brush).unwrap();
    engine
        .set_instant_feedback(InstantFeedbackConfig { enabled: false, use_platform_prediction: false, ..Default::default() })
        .unwrap();
    settle(&mut engine);
    (input, engine)
}

fn stroke(first: u64, count: usize, at: impl Fn(f32) -> ([f32; 2], f32)) -> Vec<PenEvent> {
    (0..count)
        .map(|i| {
            let phase = if i == 0 { PenPhase::Down } else if i + 1 == count { PenPhase::Up } else { PenPhase::Move };
            let (position, pressure) = at(i as f32 / (count - 1) as f32);
            let sequence = first + i as u64;
            PenEvent {
                timestamp_ns: sequence * 4_000_000,
                pressure,
                ..crate::test_support::pen(sequence, phase, position, SampleFlags::PRIMARY)
            }
        })
        .collect()
}

fn draw(engine: &mut CanvasEngine<WgpuRasterizer>, input: &mut InputProducer<PenEvent>, events: &[PenEvent], cadence: usize) {
    for (index, event) in events.iter().enumerate() {
        input.push(*event).unwrap();
        if index % cadence == cadence - 1 || index + 1 == events.len() {
            loop {
                engine.backend_mut().wait_idle().unwrap();
                engine.render_frame_at(event.timestamp_ns).unwrap();
                if !engine.has_pending_input() {
                    break;
                }
            }
        }
    }
    settle(engine);
}

fn corrected(events: &[PenEvent], index: usize) -> (Vec<PenEvent>, PenEvent) {
    let mut estimated = events.to_vec();
    estimated[index].flags = SampleFlags(SampleFlags::PRIMARY.0 | SampleFlags::ESTIMATED.0);
    estimated[index].pressure = 0.15;
    (estimated, PenEvent { flags: SampleFlags::CORRECTION, ..events[index] })
}

#[test]
fn tapered_strokes_publish_what_their_replay_from_the_accepted_cuts_paints() {
    let figure = |t: f32| {
        let angle = t * std::f32::consts::TAU * 1.1;
        ([384. + 250. * angle.sin(), 384. + 150. * (2. * angle).sin()], 0.5 + 0.4 * (3. * angle).sin())
    };
    let spiral = |t: f32| {
        let angle = t * 14.;
        ([384. + (60. + 260. * t) * angle.cos(), 384. + (60. + 260. * t) * angle.sin()], if (t * 5_000.) as u32 % 2 == 0 { 0.6 } else { 0.85 })
    };
    let fixed = BrushTaper { mode: TaperMode::Fixed, fixed_widths: [2., 3.], ..Default::default() };
    let long = BrushTaper { mode: TaperMode::Proportional, proportional_shares: [0.5, 0.5], ..Default::default() };
    for (name, mode, taper, opacity, base, events, cadence) in [
        ("TwoTone loop", LayerColorMode::TwoTone, fixed, 1., false, stroke(1, 1_200, figure), 5),
        ("forced repair", LayerColorMode::TwoTone, long, 1., false, stroke(1, 5_400, spiral), 32),
        ("grouped opacity", LayerColorMode::default(), fixed, 0.6, true, stroke(100, 1_200, figure), 5),
    ] {
        let mut doc = paint_document(EXTENT, name);
        paint_mut(&mut doc).color_mode = mode;
        let select = doc.select_occurrence_edit(occurrence_id(&doc)).unwrap();
        doc.apply(select).unwrap();
        let mut brush = default_brush(DefaultBrushPreset::GPen);
        brush.diameter = 40.;
        brush.stabilization.strength = 0.;
        brush.opacity = opacity;
        brush.color_rgba_linear = [0.08, 0.015, 0.25, 1.];
        let underlay = stroke(1, 60, |t| ([60. + 640. * t, 384.], 0.9));
        let mut results = Vec::new();
        for replayed in [false, true] {
            let (mut input, mut engine) = engine(doc.clone(), BrushSnapshot { taper: BrushTaper::default(), ..brush.clone() });
            if base {
                draw(&mut engine, &mut input, &underlay, 4);
            }
            let before = tiles(&engine);
            let mut tapered = brush.clone();
            tapered.taper = taper;
            engine.set_brush(tapered).unwrap();
            if replayed {
                let (estimated, correction) = corrected(&events, events.len() / 2);
                draw(&mut engine, &mut input, &estimated, cadence);
                input.push(correction).unwrap();
                settle(&mut engine);
            } else {
                draw(&mut engine, &mut input, &events, cadence);
            }
            let painted = tiles(&engine);
            assert_ne!(painted, before, "{name}: the stroke paints");
            results.push(painted);
            if !replayed {
                engine.undo().unwrap();
                settle(&mut engine);
                assert_eq!(tiles(&engine), before, "{name}: undo restores the layer");
            }
        }
        assert!(results[0] == results[1], "{name}: live slices and the replay from accepted cuts differ");
    }
}
