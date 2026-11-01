const DRY_TAP_PRESETS: [DefaultBrushPreset; 7] = [DefaultBrushPreset::Pencil,
    DefaultBrushPreset::PointyPencil, DefaultBrushPreset::ShadingPencil,
    DefaultBrushPreset::Charcoal, DefaultBrushPreset::Chalk,
    DefaultBrushPreset::PastelBlock, DefaultBrushPreset::Crayon];

fn dry_tap_frame(engine: &mut CanvasEngine<RecordingRenderer>, timestamp: u64) {
    engine.render_frame_for(timestamp, timestamp + 8_000_000).unwrap();
}

fn dry_tap_finish(input: &mut InputProducer<PenEvent>, engine: &mut CanvasEngine<RecordingRenderer>, mut pen: PenEvent) {
    pen.sequence += 1;
    pen.phase = PenPhase::Up;
    pen.pressure = 0.;
    pen.timestamp_ns += 16_000_000;
    input.push(pen).unwrap();
    for frame in 0..8 { dry_tap_frame(engine, pen.timestamp_ns + frame * 16_000_000); }
    let mut replay = Vec::new();
    DabGenerator::generate(engine.completed_stroke.as_ref().unwrap(), engine.document().composition().color.space, &mut replay);
    assert_eq!(engine.backend().persistent, replay);
    assert!(engine.backend().preview.is_empty());
}

#[test]
fn dry_tap_waits_for_real_release_and_ignores_prediction_pressure() {
    for preset in DRY_TAP_PRESETS { for feedback in [false, true] {
        let (mut input, mut engine) = engine("dry tap", 256, 256);
        engine.set_brush(default_brush(preset)).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: feedback, ..Default::default() }).unwrap();
        let mut pen = event(1, PenPhase::Down, 64.);
        pen.pressure = 0.2;
        for frame in 0..4 {
            pen.sequence = frame + 1;
            pen.timestamp_ns = 1_000_000_000 + frame * 16_000_000;
            pen.phase = if frame == 0 { PenPhase::Down } else { PenPhase::Move };
            input.push(pen).unwrap();
            input.push(PenEvent { timestamp_ns: pen.timestamp_ns + 8_000_000,
                surface_position: Point { x: 128., y: pen.surface_position.y },
                pressure: 1., flags: SampleFlags::PREDICTED, phase: PenPhase::Move, ..pen }).unwrap();
            dry_tap_frame(&mut engine, pen.timestamp_ns);
            assert!(engine.backend().persistent.is_empty(), "{preset:?}");
            assert!(engine.backend().preview.iter().all(|dab| dab.motion != [0., 0.]), "{preset:?}");
        }
        dry_tap_finish(&mut input, &mut engine, pen);
        assert_eq!(engine.backend().persistent.len(), 1, "{preset:?}");
        let tap = engine.backend().persistent[0];
        assert_eq!(tap.motion, [0., 0.]);
        assert_eq!(tap.contact[0], engine.settings.pressure.map(0.2)*engine.settings.brush.contact.unwrap().pressure_gain*engine.settings.brush.flow);
        let raster = active_paint(engine.document()).raster.identity();
        assert!(engine.undo().unwrap());
        assert!(active_paint(engine.document()).raster.is_empty());
        assert!(engine.redo().unwrap());
        assert_eq!(active_paint(engine.document()).raster.identity(), raster);
    }}
}

#[test]
fn dry_tap_jitter_uses_peak_real_pressure_and_cancel_discards_pending_contact() {
    for preset in DRY_TAP_PRESETS {
        let render = |jitter: bool, peak: f32| {
            let (mut input, mut engine) = engine("dry peak", 256, 256);
            engine.set_brush(default_brush(preset)).unwrap();
            engine.set_instant_feedback(InstantFeedbackConfig { enabled: false, ..Default::default() }).unwrap();
            let mut pen = event(1, PenPhase::Down, 64.);
            pen.pressure = if jitter { 0.2 } else { peak };
            pen.timestamp_ns = 1_000_000_000;
            input.push(pen).unwrap(); dry_tap_frame(&mut engine, pen.timestamp_ns);
            if jitter {
                for index in 0..64 {
                    pen.phase = PenPhase::Move; pen.sequence += 1; pen.timestamp_ns += 16_000_000;
                    pen.surface_position.x = 64. + if index % 2 == 0 { 0.01 } else { 0. };
                    pen.pressure = if index == 0 { peak } else { 0.1 };
                    input.push(pen).unwrap(); dry_tap_frame(&mut engine, pen.timestamp_ns);
                    assert!(engine.backend().persistent.is_empty(), "{preset:?}");
                    assert!(engine.backend().preview.len() <= 1, "{preset:?}");
                }
            }
            dry_tap_finish(&mut input, &mut engine, pen);
            assert_eq!(engine.backend().persistent.len(), 1, "{preset:?}");
            engine.backend().persistent[0]
        };
        let peak = render(false, 0.8);
        let jitter = render(true, 0.8);
        assert_eq!(jitter.radii, peak.radii, "{preset:?}");
        assert_eq!(jitter.flow, peak.flow, "{preset:?}");
        assert_eq!(jitter.contact, peak.contact, "{preset:?}");
        let low = render(false, 0.2);
        assert!(peak.contact[0] > low.contact[0], "{preset:?}");
        assert!(peak.radii != low.radii || peak.flow != low.flow, "{preset:?}");
        let (mut input, mut engine) = engine("dry cancel", 256, 256);
        engine.set_brush(default_brush(preset)).unwrap();
        input.push(event(1, PenPhase::Down, 64.)).unwrap();
        dry_tap_frame(&mut engine, 1_000_000);
        input.push(event(2, PenPhase::Cancel, 64.)).unwrap();
        dry_tap_frame(&mut engine, 2_000_000);
        assert!(engine.backend().persistent.is_empty());
        assert!(engine.backend().preview.is_empty());
        assert!(!engine.can_undo());
    }
}

#[test]
fn dry_drag_flushes_without_a_tap_and_replays_at_either_frame_cadence() {
    for preset in DRY_TAP_PRESETS { for per_sample in [false, true] { for feedback in [false, true] {
        let (mut input, mut engine) = engine("dry drag", 256, 256);
        engine.set_brush(default_brush(preset)).unwrap();
        engine.set_instant_feedback(InstantFeedbackConfig { enabled: feedback, ..Default::default() }).unwrap();
        let mut pen = event(1, PenPhase::Down, 32.);
        for index in 0..5 {
            pen.sequence = index + 1; pen.timestamp_ns = 1_000_000_000 + index * 16_000_000;
            pen.phase = if index == 0 { PenPhase::Down } else { PenPhase::Move };
            pen.surface_position.x = 32. + index as f32 * 32.;
            pen.pressure = if index == 0 { 0.2 } else { 0.8 };
            input.push(pen).unwrap();
            if per_sample { dry_tap_frame(&mut engine, pen.timestamp_ns); }
        }
        dry_tap_finish(&mut input, &mut engine, pen);
        let dabs=&engine.backend().persistent;
        let ordinary=dabs.iter().filter(|dab|!dab.is_completion()).collect::<Vec<_>>();
        assert!(ordinary.len()>1,"{preset:?}");
        assert!(!dabs[0].is_completion(),"{preset:?}: incident body precedes its start completion");
        assert!(ordinary.iter().all(|dab|dab.motion!=[0.,0.]),"{preset:?}");
        assert_eq!(dabs.iter().filter(|dab|dab.is_completion()).count(),2,"{preset:?}: one completion at each end");
        assert_eq!(ordinary[0].previous_contact[0], engine.settings.pressure.map(0.2)*engine.settings.brush.contact.unwrap().pressure_gain*engine.settings.brush.flow);
    }}}
}
