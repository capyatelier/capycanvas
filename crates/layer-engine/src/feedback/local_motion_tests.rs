use super::{
    test_support::{IDENTITY, config, point},
    *,
};

#[test]
fn continuity_memory_resets_on_policy_view_native_handoff_and_stale_input() {
    let cfg = config();
    let mut real = Vec::new();
    let mut warm = PredictionState::default();
    for i in 0..80 {
        let now = i * 4000;
        real.push(point(i as f32 * 3.2, 0.2 * (i as f32 * 0.4).sin(), now));
        warm.estimate_for(&real, &[], now + 16000, now, IDENTITY, cfg);
    }
    let now = real.last().unwrap().elapsed_micros;
    assert!(warm.last_motion.is_some());
    for (transform, policy) in [
        ([2., 0., 0., 2., 0., 0.], cfg),
        (
            IDENTITY,
            InstantFeedbackConfig {
                prediction_horizon_micros: 8000,
                ..cfg
            },
        ),
        (
            IDENTITY,
            InstantFeedbackConfig {
                prediction_horizon_micros: 0,
                ..cfg
            },
        ),
    ] {
        let mut state = warm.clone();
        let mut fresh = PredictionState::default();
        let a = state.estimate_for(&real, &[], now + 16000, now, transform, policy);
        let b = fresh.estimate_for(&real, &[], now + 16000, now, transform, policy);
        assert_eq!(a, b);
        assert_eq!(
            state.engine_intermediates().collect::<Vec<_>>(),
            fresh.engine_intermediates().collect::<Vec<_>>()
        );
        if policy.prediction_horizon_micros == 0 {
            assert_eq!(a.unwrap().source, TipSource::Real);
            assert!(state.output.is_none());
        }
    }
    let mut native = warm.clone();
    let tip = native
        .estimate_for(
            &real,
            &[point(270., 0., now + 16000)],
            now + 16000,
            now,
            IDENTITY,
            InstantFeedbackConfig {
                use_platform_prediction: true,
                ..cfg
            },
        )
        .unwrap();
    assert_eq!(tip.source, TipSource::Platform);
    assert!(native.last_motion.is_none() && native.output.is_none());
    let tip = warm
        .estimate_for(&real, &[], now + 116000, now + 100000, IDENTITY, cfg)
        .unwrap();
    assert_eq!(tip.source, TipSource::Real);
    assert!(warm.last_motion.is_none() && warm.output.is_none());
}

#[test]
fn a_missing_delivery_cannot_advance_the_retained_forecast() {
    for horizon in [8000, 16000, 32000] {
        for quantum in [1, 1000] {
            for period in [4167, 8333] {
                let cfg = InstantFeedbackConfig {
                    timestamp_resolution_micros: quantum,
                    prediction_horizon_micros: horizon,
                    ..config()
                };
                let mut state = PredictionState::default();
                let mut real = Vec::new();
                for i in 0..60 {
                    let time = i * period;
                    real.push(point(time as f32 * 0.0012, 0., time / quantum * quantum));
                    state.estimate_for(&real, &[], time + horizon, time, IDENTITY, cfg);
                }
                let latest = real.last().unwrap().elapsed_micros;
                let mut target = state
                    .output
                    .as_ref()
                    .unwrap()
                    .point_at(state.output.as_ref().unwrap().horizon)
                    .elapsed_micros;
                for age in [2000, 4000, 8000, 12000] {
                    let now = latest + age;
                    let tip = state
                        .estimate_for(&real, &[], now + horizon, now, IDENTITY, cfg)
                        .unwrap();
                    assert!(
                        tip.point.elapsed_micros <= target,
                        "missing reports must not earn reach: {period}/{quantum}/{horizon}"
                    );
                    target = tip.point.elapsed_micros;
                    assert_eq!(
                        state.estimate_for(&real, &[], now + horizon, now, IDENTITY, cfg),
                        Some(tip)
                    );
                }
            }
        }
    }
}

#[test]
fn local_motion_keeps_honest_target_times_sensors_bounds_and_repeatability() {
    for zoom in [0.25, 1., 4.] {
        let transform = [0., zoom, -zoom, 0., 19., -7.];
        let cfg = InstantFeedbackConfig {
            timestamp_resolution_micros: 1000,
            ..config()
        };
        let mut state = PredictionState::default();
        let mut real = Vec::new();
        for i in 0..160 {
            let t = i as f32 * 0.004;
            real.push(point(
                (900. * t + 20. * (t * 30.).sin()) / zoom,
                30. * (t * 18.).cos() / zoom,
                i * 4000,
            ));
            let before = real.clone();
            let now = i * 4000 + 2000;
            let tip = state
                .estimate_for(&real, &[], now + 16000, now, transform, cfg)
                .unwrap();
            assert!(
                tip.point.elapsed_micros
                    <= (now + 16000).min(i * 4000 + MAX_PREDICTION_HORIZON_MICROS)
            );
            assert!(tip.point.elapsed_micros >= real.last().unwrap().elapsed_micros);
            assert_eq!(
                state.estimate_for(&real, &[], now + 16000, now, transform, cfg),
                Some(tip)
            );
            let latest = *real.last().unwrap();
            for p in state.engine_intermediates().chain([tip.point]) {
                assert!(p.elapsed_micros <= tip.point.elapsed_micros);
                assert!(p.position.x.is_finite() && p.position.y.is_finite());
                assert!(
                    surface_distance(latest.position, p.position, transform)
                        <= MAX_PREDICTION_DISTANCE_PX + 0.01
                );
                assert_eq!(
                    (p.pressure, p.tilt, p.twist),
                    (latest.pressure, latest.tilt, latest.twist)
                );
            }
            assert_eq!(real, before, "forecast never edits permanent ink");
        }
        let now = real.last().unwrap().elapsed_micros + 100000;
        let stale = state
            .estimate_for(&real, &[], now + 16000, now, transform, cfg)
            .unwrap();
        assert_eq!(stale.source, TipSource::Real);
    }
}

#[test]
fn lookahead_distinguishes_straight_motion_from_turns_at_every_speed_and_zoom() {
    for zoom in [0.25, 1., 4.] {
        for (speed, radius, maximum, minimum) in [
            (200., 0., 20_000, 19_000),
            (800., 0., 20_000, 19_000),
            (2200., 0., 24_000, 20_000),
            (2200., 300., 24_000, 20_000),
            (800., 12., 9_000, 1_000),
        ] {
            let path = |i: u32| {
                let t = i as f32 * 0.004;
                if radius == 0. {
                    point(speed * t / zoom, 0., i * 4000)
                } else {
                    let angle = speed * t / radius;
                    point(
                        radius * angle.sin() / zoom,
                        radius * (1. - angle.cos()) / zoom,
                        i * 4000,
                    )
                }
            };
            let real: Vec<_> = (0..100).map(path).collect();
            let model = local_motion::LocalMotion::fit(
                &real,
                [zoom, 0., 0., zoom, 0., 0.],
                0,
                24000,
                None,
                None,
                0.,
            )
            .unwrap();
            let horizon = model.horizon(24_000, 0, 24_000);
            assert!(
                (minimum..=maximum).contains(&horizon),
                "speed {speed}, radius {radius}: {horizon}"
            );
            assert!(model.horizon(24_000, 3_000, 24_000) <= (horizon + 3_000).min(24_000));
            if radius == 0. {
                assert!(
                    model.horizon(39_000, 15_000, 24_000) >= 32_000,
                    "steady slow motion must compensate for delivery age too"
                );
            }
            if speed > 2000. {
                assert_eq!(model.horizon(48_000, 0, 48_000), 48_000);
            }
        }
    }
}

#[test]
fn detail_lookahead_does_not_pulse_with_queries_or_report_rate() {
    let mut previous: Option<local_motion::LocalMotion> = None;
    let mut real = Vec::new();
    let mut last_horizon = 0;
    let mut last_time = 0;
    for i in 0..160 {
        let time = i * 4000;
        let t = time as f32 / 1e6;
        // Smoothly decelerate from a broad line into detailed motion.
        let x = if t < 0.3 {
            2200. * t
        } else {
            660. + 200. * (t - 0.3)
        };
        real.push(point(x, 0., time));
        if let Some(model) =
            local_motion::LocalMotion::fit(&real, IDENTITY, 0, 24000, None, previous.as_ref(), 0.)
        {
            let horizon = model.horizon(24000, 0, 24000);
            if previous.is_some() {
                assert!(
                    time + horizon >= last_time + last_horizon,
                    "allowance alone withdrew the target"
                );
            }
            let repeat =
                local_motion::LocalMotion::fit(&real, IDENTITY, 0, 24000, None, Some(&model), 0.)
                    .unwrap();
            assert_eq!(horizon, repeat.horizon(24000, 0, 24000));
            previous = Some(model);
            last_horizon = horizon;
            last_time = time;
        }
    }
    assert!(
        last_horizon > 19000,
        "a slower straight line still needs to follow the pen"
    );
}

#[test]
fn quantized_tablet_clock_does_not_misclassify_fast_strokes_as_missing_history() {
    for period in [3333., 4166.666, 8333.333] {
        let mut real = Vec::new();
        let mut previous: Option<local_motion::LocalMotion> = None;
        for i in 0..100 {
            let time = (i as f64 * period / 1000.).floor() as u32 * 1000;
            real.push(point(i as f32 * period as f32 * 0.0022, 0., time));
            if let Some(model) = local_motion::LocalMotion::fit(
                &real,
                IDENTITY,
                1000,
                24000,
                None,
                previous.as_ref(),
                0.,
            ) {
                if i > 40 {
                    assert!(
                        model.horizon(24000, 0, 24000) > 23_000,
                        "quantized {period} us cadence lost broad-stroke lookahead at report {i}"
                    );
                }
                previous = Some(model);
            }
        }
    }
}

#[test]
fn correcting_turn_history_invalidates_the_detail_allowance_without_a_new_tip() {
    let mut real: Vec<_> = (0..100)
        .map(|i| point(i as f32 * 8.8, 0., i * 4000))
        .collect();
    let before = local_motion::LocalMotion::fit(&real, IDENTITY, 0, 24000, None, None, 0.).unwrap();
    assert_eq!(before.horizon(24000, 0, 24000), 24000);
    // This sample informs the turn detector but lies outside the 40 ms fit.
    // The anchor and fitted polynomial are unchanged by the late correction.
    real[87].position.y += 60.;
    let after =
        local_motion::LocalMotion::fit(&real, IDENTITY, 0, 24000, None, Some(&before), 0.).unwrap();
    // The recent heading is still steady, but the corrected older turn must
    // invalidate the cached fast/detail allowance even without a new tip.
    assert!(after.horizon(24000, 0, 24000) < before.horizon(24000, 0, 24000));
    let repeat =
        local_motion::LocalMotion::fit(&real, IDENTITY, 0, 24000, None, Some(&after), 0.).unwrap();
    assert_eq!(
        after.horizon(24000, 0, 24000),
        repeat.horizon(24000, 0, 24000)
    );
}

#[test]
fn slow_and_medium_corners_drop_reach_then_recover_on_the_settled_line() {
    for speed in [300., 900.] {
        for stable in [false, true] {
            let mut previous = None;
            let mut real = Vec::new();
            for i in 0..130 {
                let time = i * 4000;
                let t = time as f32 / 1e6;
                real.push(point(speed * t.min(0.4), speed * (t - 0.4).max(0.), time));
                if let Some(model) = local_motion::LocalMotion::fit(
                    &real,
                    IDENTITY,
                    0,
                    24000,
                    stable.then_some((24_000, 0.)),
                    previous.as_ref(),
                    0.,
                ) {
                    let horizon = model.horizon(24000, 0, 24000);
                    if time == 396000 || time == 464000 {
                        assert!(
                            horizon > 18000,
                            "steady {speed} px/s must retain useful reach: {horizon}"
                        );
                    }
                    if time == 412000 || time == 416000 {
                        assert!(
                            horizon < 8000,
                            "corner {speed} px/s retained a ghost: {horizon}"
                        );
                    }
                    previous = Some(model);
                }
            }
        }
    }
}

#[test]
fn local_motion_tracks_cartesian_acceleration_and_zoom() {
    for zoom in [0.25, 1., 4.] {
        let cfg = InstantFeedbackConfig { ..config() };
        let path = |time: u32| {
            let t = time as f32 / 1e6;
            Point {
                x: (900. * t + 500. * t * t) / zoom,
                y: (200. * t + 1500. * t * t) / zoom,
            }
        };
        let real: Vec<_> = (0..50)
            .map(|i| {
                let p = path(i * 4000);
                point(p.x, p.y, i * 4000)
            })
            .collect();
        let original = real.clone();
        let transform = [zoom, 0., 0., zoom, 0., 0.];
        let latest = real.last().unwrap().elapsed_micros;
        let tip = PredictionState::default()
            .estimate(&real, &[], latest + 16000, transform, cfg)
            .unwrap();
        assert!(
            surface_distance(
                path(tip.point.elapsed_micros),
                tip.point.position,
                transform
            ) < 0.01
        );
        assert_eq!(real, original);
        let clamped = PredictionState::default()
            .estimate(
                &real,
                &[],
                latest + 16000,
                transform,
                InstantFeedbackConfig {
                    max_prediction_distance_px: 2.,
                    ..cfg
                },
            )
            .unwrap();
        assert!(
            surface_distance(
                real.last().unwrap().position,
                clamped.point.position,
                transform
            ) <= 2.001
        );
    }
}

#[test]
fn local_motion_braking_does_not_invent_a_reversal() {
    let mut state = PredictionState::default();
    let mut real = Vec::new();
    let cfg = InstantFeedbackConfig { ..config() };
    for i in 0..75 {
        let t = (i as f32 * 0.004).min(0.2);
        let x = 1000. * t - 2500. * t * t;
        real.push(point(x, 0., i * 4000));
        let tip = state
            .estimate(&real, &[], i * 4000 + 16000, IDENTITY, cfg)
            .unwrap();
        if i >= 8 {
            assert!(
                tip.point.position.x >= x - 0.05,
                "backward tail: {i} {:?}",
                tip.point
            );
            assert!(
                tip.point.position.x <= 100.1,
                "stop overshoot: {i} {:?}",
                tip.point
            );
        }
    }
}

#[test]
fn coherent_model_is_exact_for_acceleration_despite_irregular_queries() {
    let mut state = PredictionState::default();
    let mut real = Vec::new();
    for i in 0..150 {
        let t = i as f32 * 0.004;
        real.push(point(500. * t + 200. * t * t, 800. * t * t, i * 4000));
        if i % 3 == 0 || i % 7 == 0 {
            let tip = state
                .estimate(
                    &real,
                    &[],
                    i * 4000 + 16000,
                    IDENTITY,
                    InstantFeedbackConfig { ..config() },
                )
                .unwrap();
            let t = tip.point.elapsed_micros as f32 / 1e6;
            if i > 10 {
                assert!(
                    surface_distance(
                        tip.point.position,
                        Point {
                            x: 500. * t + 200. * t * t,
                            y: 800. * t * t
                        },
                        IDENTITY
                    ) < 0.02
                );
            }
        }
    }
}

#[test]
fn correction_smoothing_does_not_delay_constant_pen_motion() {
    for speed in [300., 900., 2400., 6000.] {
        let mut state = PredictionState::default();
        let mut real = Vec::new();
        for i in 0..150 {
            let now = i * 4_000;
            real.push(point(speed * now as f32 / 1e6, 0., now));
            let tip = state
                .estimate_for(
                    &real,
                    &[],
                    now + 16_000,
                    now,
                    IDENTITY,
                    InstantFeedbackConfig {
                        prediction_horizon_micros: 24_000,
                        ..config()
                    },
                )
                .unwrap();
            if now > 220_000 {
                assert!(
                    (tip.point.position.x - speed * tip.point.elapsed_micros as f32 / 1e6).abs()
                        < 0.005,
                    "speed {speed} now {now} tip {:?}",
                    tip.point
                );
                assert_eq!(tip.point.elapsed_micros, now + 16_000);
                assert_eq!(
                    state.estimate_for(
                        &real,
                        &[],
                        now + 16_000,
                        now,
                        IDENTITY,
                        InstantFeedbackConfig {
                            prediction_horizon_micros: 24_000,
                            ..config()
                        }
                    ),
                    Some(tip)
                );
            }
        }
    }
}

#[test]
fn sustained_motion_prior_does_not_delay_a_real_stop() {
    for speed in [300., 900., 2400., 6000.] {
        for horizon in [8_000, 24_000, 48_000] {
            for period in [4_000, 8_000] {
                let mut state = PredictionState::default();
                let mut real = Vec::new();
                for time in (0..=280_000).step_by(period as usize) {
                    real.push(point(speed * time.min(240_000) as f32 / 1e6, 0., time));
                    let tip = state
                        .estimate_for(
                            &real,
                            &[],
                            time + 2000 + horizon,
                            time + 2000,
                            IDENTITY,
                            InstantFeedbackConfig {
                                prediction_horizon_micros: horizon,
                                ..config()
                            },
                        )
                        .unwrap();
                    assert!(tip.point.elapsed_micros <= time + 2000 + horizon);
                    if time >= 256_000 {
                        let error = surface_distance(
                            tip.point.position,
                            real.last().unwrap().position,
                            IDENTITY,
                        );
                        assert!(
                            error < 1.,
                            "stop ghost {speed}/{period}/{horizon} at {time}: {error}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn sustained_motion_releases_corrections_on_a_corner_and_native_takeover() {
    let cfg = InstantFeedbackConfig {
        prediction_horizon_micros: 24_000,
        use_platform_prediction: true,
        ..config()
    };
    for native in [false, true] {
        let mut state = PredictionState::default();
        let mut real = Vec::new();
        for time in (0u32..=240_000).step_by(4_000) {
            real.push(point(time as f32 * 0.0024, 0., time));
            state.estimate_for(&real, &[], time + 16_000, time, IDENTITY, cfg);
        }
        assert!(state.correction_field.is_some());
        if native {
            let platform = [point(600., 2., 256_000)];
            let tip = state
                .estimate_for(&real, &platform, 256_000, 240_000, IDENTITY, cfg)
                .unwrap();
            assert_eq!(tip.source, TipSource::Platform);
            assert!(state.output.is_none());
        } else {
            for time in (244_000..=252_000).step_by(4_000) {
                real.push(point(576., 0.0024 * (time - 240_000) as f32, time));
                state.estimate_for(&real, &[], time + 16_000, time, IDENTITY, cfg);
            }
        }
        assert!(state.correction_field.is_none());
    }
}

#[test]
fn prediction_length_is_not_quantized_to_curve_sampling_intervals() {
    let real = (0..40)
        .map(|i| point(i as f32 * 4., 0., i * 4000))
        .collect::<Vec<_>>();
    let latest = *real.last().unwrap();
    let mut state = PredictionState::default();
    for limit in [10.25, 10.5, 10.75, 11., 11.25] {
        let cfg = InstantFeedbackConfig {
            timestamp_resolution_micros: 1000,
            prediction_horizon_micros: 32_000,
            max_prediction_distance_px: limit,
            ..config()
        };
        let tip = state
            .estimate(&real, &[], latest.elapsed_micros + 32_000, IDENTITY, cfg)
            .unwrap();
        let lead = surface_distance(latest.position, tip.point.position, IDENTITY);
        assert!(lead <= limit + 0.001);
        assert!(
            lead >= limit - 0.002,
            "lead {lead} must follow the {limit} px bound continuously"
        );
        for p in state.engine_intermediates() {
            assert!(p.elapsed_micros < tip.point.elapsed_micros);
            assert!(p.position.x < tip.point.position.x);
        }
    }
}

#[test]
fn physical_motion_stays_bounded_across_input_and_display_clocks() {
    for speed in [300., 900., 2400., 8000.] {
        for input_hz in [120., 200., 240., 480.] {
            for display_hz in [60., 120., 144.] {
                for curved in [false, true] {
                    let at = |t: f64| {
                        if curved {
                            Point {
                                x: (850. * (speed * t / 850.).sin()) as f32,
                                y: (850. * (1. - (speed * t / 850.).cos())) as f32,
                            }
                        } else {
                            Point {
                                x: (speed * t) as f32,
                                y: 0.,
                            }
                        }
                    };
                    let mut scored = 0;
                    let mut predicted = 0;
                    test_support::run_frames(
                        display_hz,
                        0.6,
                        1.,
                        InstantFeedbackConfig {
                            timestamp_resolution_micros: 1000,
                            prediction_horizon_micros: 24_000,
                            ..config()
                        },
                        false,
                        |i, now| {
                            let t =
                                (i as f64 + if i % 2 == 0 { 0. } else { 0.1 }) / input_hz + 0.00037;
                            if t + 0.002 > now {
                                return None;
                            }
                            let p = at(t);
                            Some(point(
                                p.x + 0.15 * (i as f32 * 2.399).sin(),
                                p.y + 0.15 * (i as f32 * 1.731).sin(),
                                (t * 1000.).floor() as u32 * 1000,
                            ))
                        },
                        |now, tip, _| {
                            if now < 0.2 {
                                return;
                            }
                            scored += 1;
                            predicted += usize::from(tip.source == TipSource::Engine);
                            // Accepted-time geometry is distinct from the recorded
                            // whole-preview continuity/coverage regression metrics.
                            let truth = at(f64::from(tip.point.elapsed_micros) / 1e6);
                            assert!(
                                f64::from(surface_distance(tip.point.position, truth, IDENTITY))
                                    < 3. + speed * 0.004,
                                "geometry: {speed}/{input_hz}/{display_hz}/{curved}"
                            );
                        },
                    );
                    assert!(
                        predicted * 10 >= scored * 9,
                        "coverage: {speed}/{input_hz}/{display_hz}/{curved}"
                    );
                }
            }
        }
    }
}
