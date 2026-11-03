const DRY_TAP_PRESETS: [DefaultBrushPreset; 7] = [DefaultBrushPreset::Pencil,
    DefaultBrushPreset::PointyPencil, DefaultBrushPreset::ShadingPencil,
    DefaultBrushPreset::Charcoal, DefaultBrushPreset::Chalk,
    DefaultBrushPreset::PastelBlock, DefaultBrushPreset::Crayon];

fn dry_tap_frame(engine: &mut CanvasEngine<RecordingRenderer>, timestamp: u64) {
    engine.render_frame_for(timestamp, timestamp + 8_000_000).unwrap();
}

fn dry_tap_finish(input: &mut InputProducer<PenEvent>, engine: &mut CanvasEngine<RecordingRenderer>, mut pen: PenEvent) {
    pen.pressure = 0.;
    dry_finish(input, engine, pen);
}

fn dry_finish(
    input: &mut InputProducer<PenEvent>,
    engine: &mut CanvasEngine<RecordingRenderer>,
    mut pen: PenEvent,
) {
    pen.sequence += 1;
    pen.phase = PenPhase::Up;
    pen.timestamp_ns += 16_000_000;
    input.push(pen).unwrap();
    for frame in 0..8 { dry_tap_frame(engine, pen.timestamp_ns + frame * 16_000_000); }
    let mut replay = Vec::new();
    DabGenerator::generate(engine.completed_stroke.as_ref().unwrap(), engine.document().composition().color.space, &mut replay);
    assert_eq!(engine.backend().persistent, replay);
    assert!(engine.backend().preview.is_empty());
}

#[test]
fn queued_correction_updates_the_unrendered_pen_up_lift() {
    let (mut input, mut engine) = engine("corrected dry landing", 2048, 512);
    let mut brush = default_brush(DefaultBrushPreset::Pencil);
    brush.diameter = 9.;
    brush.stabilization.strength = 1.;
    engine.set_brush(brush.clone()).unwrap();
    engine
        .set_instant_feedback(InstantFeedbackConfig {
            use_platform_prediction: false,
            ..Default::default()
        })
        .unwrap();
    let mut last = event(1, PenPhase::Down, 100.);
    for index in 0..200 {
        last = PenEvent {
            sequence: index + 1,
            timestamp_ns: 1_000_000_000 + index * 4_000_000,
            phase: if index == 0 {
                PenPhase::Down
            } else {
                PenPhase::Move
            },
            surface_position: Point {
                x: 100. + index as f32 * 4.,
                y: 100.,
            },
            flags: if index == 199 {
                SampleFlags::ESTIMATED
            } else {
                SampleFlags::PRIMARY
            },
            ..last
        };
        input.push(last).unwrap();
        dry_tap_frame(&mut engine, last.timestamp_ns);
    }
    let up = PenEvent {
        sequence: last.sequence + 1,
        timestamp_ns: last.timestamp_ns + 1_000_000,
        phase: PenPhase::Up,
        surface_position: Point {
            y: 130.,
            ..last.surface_position
        },
        flags: SampleFlags::PRIMARY,
        ..last
    };
    let correction = PenEvent {
        surface_position: Point {
            y: 120.,
            ..last.surface_position
        },
        flags: SampleFlags::CORRECTION,
        ..last
    };
    input.push(up).unwrap();
    input.push(correction).unwrap();
    dry_tap_frame(&mut engine, up.timestamp_ns);
    assert!(engine.pending_up.is_some());
    assert!(engine.backend.preview.is_empty());
    dry_tap_frame(&mut engine, up.timestamp_ns + 20_000_000);
    assert_eq!(
        engine.builder.real_points()[199].position,
        correction.surface_position
    );
    let real = engine.builder.real_points();
    let mut lift = engine.dab_generator.clone();
    let mut expected = Vec::new();
    lift.append_points(
        &real[engine.finalized_real_points..],
        &brush,
        true,
        &mut expected,
    );
    lift.finish_lift(&brush, &mut expected);
    let end = |dabs: &[Dab]| {
        *dabs
            .iter()
            .rev()
            .find(|dab| dab.is_completion() && dab.previous_contact[2..] == [0.; 2])
            .unwrap()
    };
    assert_eq!(end(&engine.backend.preview), end(&expected));
    for milliseconds in [52, 84, 116] {
        dry_tap_frame(&mut engine, up.timestamp_ns + milliseconds * 1_000_000);
    }
    let mut replay = Vec::new();
    DabGenerator::generate(
        engine.completed_stroke.as_ref().unwrap(),
        engine.document().composition().color.space,
        &mut replay,
    );
    assert_eq!(engine.backend.persistent, replay);
    assert!(engine.backend.preview.is_empty());
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
            assert_eq!(engine.backend().preview.len(), 1, "{preset:?}");
            assert_eq!(engine.backend().preview[0].motion, [0.; 2], "{preset:?}");
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
                    assert!(
                        engine
                            .backend()
                            .preview
                            .iter()
                            .filter(|dab| !dab.is_completion())
                            .count()
                            <= 1,
                        "{preset:?}"
                    );
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
        let start = dabs
            .iter()
            .find(|dab| dab.is_completion() && dab.previous_contact[..2] == [0.; 2])
            .unwrap();
        assert_eq!(ordinary[0].previous_contact, start.contact);
        assert!(
            start.contact[0]
                > engine.settings.pressure.map(0.2)
                    * engine.settings.brush.contact.unwrap().pressure_gain
                    * engine.settings.brush.flow
        );
    }}}
}

#[test]
fn dry_rest_replays_across_feedback_cadence_corrections_and_rebuilds() {
    for strength in [0., 0.5] {
        for feedback in [false, true] {
            for cadence in [1, 7, 31] {
                let (mut input, mut engine) = engine("rest replay", 512, 256);
                let mut brush = default_brush(DefaultBrushPreset::Pencil);
                brush.diameter = 36.;
                brush.stabilization.strength = strength;
                engine.set_brush(brush).unwrap();
                engine
                    .set_instant_feedback(InstantFeedbackConfig {
                        enabled: feedback,
                        ..Default::default()
                    })
                    .unwrap();
                let mut pen = event(1, PenPhase::Down, 32.);
                let mut correction = pen;
                for i in 0..120 {
                    pen.sequence = i + 1;
                    pen.timestamp_ns = 1_000_000_000 + i * 4_000_000;
                    pen.phase = if i == 0 {
                        PenPhase::Down
                    } else {
                        PenPhase::Move
                    };
                    pen.surface_position.x = 32. + (i.min(40) as f32) * 4.;
                    pen.surface_position.y = 100.;
                    pen.pressure = if i < 70 { 0.4 } else { 0.8 };
                    if i == 75 {
                        correction = pen;
                    }
                    input.push(pen).unwrap();
                    if i % cadence == 0 {
                        dry_tap_frame(&mut engine, pen.timestamp_ns);
                    }
                }
                dry_tap_frame(&mut engine, pen.timestamp_ns);
                assert!(engine.dab_generator.frozen());
                assert!(!engine.backend().preview.is_empty());
                assert!(
                    engine
                        .backend()
                        .preview
                        .iter()
                        .all(|dab| dab.preview_tag() == layer_render::DabPreview::Shared)
                );
                correction.flags = SampleFlags::CORRECTION;
                correction.pressure = 0.9;
                input.push(correction).unwrap();
                dry_tap_frame(&mut engine, pen.timestamp_ns + 4_000_000);
                pen.sequence += 1;
                pen.phase = PenPhase::Up;
                pen.pressure = 0.;
                pen.timestamp_ns += 8_000_000;
                input.push(pen).unwrap();
                dry_tap_frame(&mut engine, pen.timestamp_ns);
                assert!(engine.pending_up.is_none(), "a frozen lift skips landing");
                let stroke = engine.completed_stroke.as_ref().unwrap().clone();
                let mut replay = Vec::new();
                DabGenerator::generate(
                    &stroke,
                    engine.document().composition().color.space,
                    &mut replay,
                );
                assert_eq!(
                    engine.backend().persistent,
                    replay,
                    "strength={strength} feedback={feedback} cadence={cadence}"
                );
                engine.rebuild_all = true;
                dry_tap_frame(&mut engine, pen.timestamp_ns + 16_000_000);
                assert_eq!(engine.backend().persistent, replay);
            }
        }
    }
}

fn dry_gap_stroke(
    strength: f32,
    feedback: bool,
) -> (
    InputProducer<PenEvent>,
    CanvasEngine<RecordingRenderer>,
    PenEvent,
) {
    let (mut input, mut engine) = engine("sample gap", 1024, 256);
    let mut brush = default_brush(DefaultBrushPreset::Pencil);
    brush.diameter = 36.;
    brush.stabilization.strength = strength;
    engine.set_brush(brush).unwrap();
    engine
        .set_instant_feedback(InstantFeedbackConfig {
            enabled: feedback,
            use_platform_prediction: false,
            ..Default::default()
        })
        .unwrap();
    let mut pen = event(1, PenPhase::Down, 32.);
    for i in 0..40 {
        pen = event(
            i + 1,
            if i == 0 {
                PenPhase::Down
            } else {
                PenPhase::Move
            },
            32. + i as f32 * 5.,
        );
        pen.surface_position.y = 100. + (i as f32 * 0.05).sin() * 20.;
        pen.pressure = 0.6;
        pen.timestamp_ns = 1_000_000_000 + i * 4_000_000;
        input.push(pen).unwrap();
        dry_tap_frame(&mut engine, pen.timestamp_ns);
    }
    (input, engine, pen)
}

#[test]
fn dry_sample_gap_freezes_preview_and_ignores_the_late_up_pose() {
    for strength in [0., 0.5, 1.] {
        for feedback in [false, true] {
            let (mut input, mut engine, mut pen) = dry_gap_stroke(strength, feedback);
            let moving_end = engine
                .backend()
                .preview
                .iter()
                .copied()
                .find(|dab| dab.is_completion() && dab.previous_contact[2..] == [0.; 2])
                .unwrap()
                .with_preview_tag(layer_render::DabPreview::Shared);
            let before = engine.backend().persistent.len();
            dry_tap_frame(&mut engine, pen.timestamp_ns + 100_000_000);
            let frozen_preview = engine.backend().preview.clone();
            let frozen: Vec<_> = engine
                .backend()
                .preview
                .iter()
                .copied()
                .map(|dab| dab.with_preview_tag(layer_render::DabPreview::Shared))
                .collect();
            assert!(!frozen.is_empty());
            let frozen_end = *frozen
                .iter()
                .find(|dab| dab.is_completion() && dab.previous_contact[2..] == [0.; 2])
                .unwrap();
            assert_eq!(
                frozen_end, moving_end,
                "freezing without samples keeps the settled end: strength={strength} feedback={feedback}"
            );
            assert!(
                engine
                    .backend()
                    .preview
                    .iter()
                    .all(|dab| dab.preview_tag() != layer_render::DabPreview::MainOnly)
            );
            dry_tap_frame(&mut engine, pen.timestamp_ns + 2_000_000_000);
            assert_eq!(engine.backend().preview, frozen_preview);
            assert_eq!(
                engine
                    .backend()
                    .preview
                    .iter()
                    .copied()
                    .map(|dab| dab.with_preview_tag(layer_render::DabPreview::Shared))
                    .collect::<Vec<_>>(),
                frozen
            );
            pen.phase = PenPhase::Up;
            pen.sequence += 1;
            pen.timestamp_ns += 3_000_000_000;
            pen.surface_position = Point { x: 900., y: 20. };
            pen.pressure = 0.;
            pen.tilt_radians = [1.1, 0.7];
            pen.twist_radians = 1.3;
            input.push(pen).unwrap();
            for i in 0..8 {
                dry_tap_frame(&mut engine, pen.timestamp_ns + i * 16_000_000);
            }
            assert_eq!(
                engine.backend().persistent[before..],
                frozen,
                "strength={strength} feedback={feedback}"
            );
            let mut replay = Vec::new();
            DabGenerator::generate(
                engine.completed_stroke.as_ref().unwrap(),
                engine.document().composition().color.space,
                &mut replay,
            );
            assert_eq!(engine.backend().persistent, replay);
            let (mut landed_input, mut landed, mut up) = dry_gap_stroke(strength, feedback);
            up.phase = PenPhase::Up;
            up.sequence += 1;
            up.timestamp_ns += 16_000_000;
            landed_input.push(up).unwrap();
            for i in 0..10 {
                dry_tap_frame(&mut landed, up.timestamp_ns + i * 16_000_000);
            }
            let landed_end = landed
                .backend()
                .persistent
                .iter()
                .find(|dab| dab.is_completion() && dab.previous_contact[2..] == [0.; 2])
                .unwrap();
            let limit = 1.5f32.min(0.25 * frozen_end.radii[0].min(frozen_end.radii[1]));
            assert!(
                (landed_end.center.x - frozen_end.center.x)
                    .hypot(landed_end.center.y - frozen_end.center.y)
                    < limit,
                "strength={strength} feedback={feedback} frozen={frozen_end:?} landed={landed_end:?} limit={limit}"
            );
            assert_eq!(frozen_end.radii, landed_end.radii);
            assert_eq!(frozen_end.contact, landed_end.contact);
            engine.rebuild_all = true;
            dry_tap_frame(&mut engine, pen.timestamp_ns + 200_000_000);
            assert_eq!(engine.backend().persistent, replay);
        }
    }
}

#[test]
fn dry_gap_commit_depends_on_event_times_and_not_preview_or_settlement_frames() {
    for strength in [0., 0.5, 1.] {
        for feedback in [false, true] {
            for long_gap in [false, true] {
                let render = |dense: bool| {
                    let (mut input, mut engine, mut pen) = dry_gap_stroke(strength, feedback);
                    if dense && long_gap {
                        for i in 1..=180 {
                            dry_tap_frame(&mut engine, pen.timestamp_ns + i * 16_000_000);
                        }
                    }
                    pen.phase = PenPhase::Up;
                    pen.sequence += 1;
                    pen.timestamp_ns += if long_gap { 3_000_000_000 } else { 20_000_000 };
                    pen.surface_position.x += 80.;
                    pen.pressure = 0.;
                    input.push(pen).unwrap();
                    let rendered_at = pen.timestamp_ns + if dense { 0 } else { 3_000_000_000 };
                    for i in 0..8 {
                        dry_tap_frame(&mut engine, rendered_at + i * 16_000_000);
                    }
                    let mut replay = Vec::new();
                    DabGenerator::generate(
                        engine.completed_stroke.as_ref().unwrap(),
                        engine.document().composition().color.space,
                        &mut replay,
                    );
                    assert_eq!(engine.backend().persistent, replay);
                    if !long_gap {
                        assert!(
                            replay.iter().any(|dab| dab.center.x > 240.),
                            "a short event gap keeps the moving release endpoint"
                        );
                    }
                    engine.rebuild_all = true;
                    dry_tap_frame(&mut engine, rendered_at + 200_000_000);
                    assert_eq!(engine.backend().persistent, replay);
                    replay
                };
                assert_eq!(
                    render(true),
                    render(false),
                    "strength={strength} feedback={feedback} long_gap={long_gap}"
                );
            }
        }
    }
}

#[test]
fn dry_single_tap_survives_a_sampleless_three_second_gap() {
    let (mut input, mut engine) = engine("gap tap", 256, 256);
    engine
        .set_brush(default_brush(DefaultBrushPreset::Pencil))
        .unwrap();
    let mut pen = event(1, PenPhase::Down, 64.);
    pen.timestamp_ns = 1_000_000_000;
    pen.pressure = 0.6;
    input.push(pen).unwrap();
    dry_tap_frame(&mut engine, pen.timestamp_ns);
    dry_tap_frame(&mut engine, pen.timestamp_ns + 100_000_000);
    let frozen: Vec<_> = engine
        .backend()
        .preview
        .iter()
        .copied()
        .map(|dab| dab.with_preview_tag(layer_render::DabPreview::Shared))
        .collect();
    assert_eq!(frozen.len(), 1);
    pen.timestamp_ns += 3_000_000_000;
    pen.phase = PenPhase::Up;
    pen.sequence += 1;
    pen.pressure = 0.;
    pen.surface_position.x = 200.;
    input.push(pen).unwrap();
    for i in 0..8 {
        dry_tap_frame(&mut engine, pen.timestamp_ns + i * 16_000_000);
    }
    assert_eq!(engine.backend().persistent, frozen);
    assert!(engine.can_undo());
}

#[test]
fn dry_contact_budget_keeps_the_last_real_move_after_a_sample_gap() {
    for feedback in [false, true] {
        let (mut input, mut engine) = engine("dry budget gap", 256, 256);
        engine
            .set_brush(default_brush(DefaultBrushPreset::Pencil))
            .unwrap();
        engine
            .set_instant_feedback(InstantFeedbackConfig {
                enabled: feedback,
                ..Default::default()
            })
            .unwrap();
        let mut pen = event(1, PenPhase::Down, 8.);
        pen.timestamp_ns = 1_000_000_000;
        input.push(pen).unwrap();
        dry_tap_frame(&mut engine, pen.timestamp_ns);
        for sequence in 2..=MAX_CONTACT_POINTS as u64 {
            pen.sequence = sequence;
            pen.phase = PenPhase::Move;
            pen.timestamp_ns = 1_000_000_000 + sequence * 4_000_000;
            pen.surface_position.x = 80.;
            if sequence == MAX_CONTACT_POINTS as u64 {
                pen.timestamp_ns += 3_000_000_000;
                pen.surface_position.x = 200.;
            }
            engine
                .builder
                .push(pen, ViewTransform::IDENTITY, PressureCurve::default());
            if !feedback {
                let point = *engine.builder.real_points().last().unwrap();
                engine.append_real_dab(point, engine.builder.real_points().len() - 1, false);
            }
        }
        assert_eq!(engine.builder.real_points().len(), MAX_CONTACT_POINTS);
        for i in 0..8 {
            dry_tap_frame(&mut engine, pen.timestamp_ns + 200_000_000 + i * 16_000_000);
        }
        assert_eq!(engine.metrics().input_budget_completions, 1);
        assert!(!engine.has_active_stroke());
        assert!(
            engine
                .backend()
                .persistent
                .iter()
                .any(|dab| !dab.is_completion() && dab.center.x > 150.),
            "feedback={feedback}: the final stored sample is a real Move"
        );
        let mut replay = Vec::new();
        DabGenerator::generate(
            engine.completed_stroke.as_ref().unwrap(),
            engine.document().composition().color.space,
            &mut replay,
        );
        assert_eq!(engine.backend().persistent, replay, "feedback={feedback}");
        engine.rebuild_all = true;
        dry_tap_frame(&mut engine, pen.timestamp_ns + 500_000_000);
        assert_eq!(engine.backend().persistent, replay);
    }
}
