use super::{
    test_support::{IDENTITY, config, point},
    *,
};

#[test]
fn measured_braking_bounds_prediction_without_tail_filter_lag() {
    let mut real = Vec::new();
    let mut state = PredictionState::default();
    for i in 0..70 {
        let t = i as f32 * 0.004;
        let u = (t - 0.2).clamp(0., 0.02);
        let x = 1000. * t.min(0.2) + 1000. * u - 25_000. * u * u;
        real.push(point(x, 0., i * 4000));
        let tip = state
            .estimate(&real, &[], i * 4000 + 16_000, IDENTITY, config())
            .unwrap();
        if (52..=60).contains(&i) {
            assert!(tip.point.position.x <= 210.1, "t={t}: {:?}", tip.point);
            assert!(tip.point.position.x >= x - 0.01, "do not invent a reversal");
        }
        if (40..=48).contains(&i) {
            assert!(
                tip.point.position.x > x + 14.,
                "preserve useful prediction before braking"
            );
        }
    }
}

#[test]
fn trajectory_resolves_braking_with_gtk_clock_uncertainty() {
    for horizon in [16_000, 32_000] {
        let cfg = InstantFeedbackConfig {
            timestamp_resolution_micros: 1_000,
            prediction_horizon_micros: horizon,
            ..config()
        };
        let mut state = PredictionState::default();
        let mut real = Vec::new();
        for i in 0..65 {
            let t = i as f32 * 0.004;
            let u = (t - 0.2).clamp(0., 0.02);
            let x = 1000. * t.min(0.2) + 1000. * u - 25_000. * u * u;
            real.push(point(x, 0., i * 4000));
            let tip = state
                .estimate(&real, &[], i * 4000 + horizon, IDENTITY, cfg)
                .unwrap();
            if i == 52 {
                assert!(
                    tip.point.position.x <= 214.,
                    "h={horizon} 8 ms braking: {:?}",
                    tip.point
                );
            }
            if (53..=60).contains(&i) {
                assert!(
                    tip.point.position.x <= 210.1,
                    "h={horizon} t={t}: {:?}",
                    tip.point
                );
                assert!(tip.point.position.x >= x - 0.01);
            }
        }
    }
}

#[test]
fn trajectory_follows_fast_curves_with_noise_and_irregular_timestamps() {
    for rate in [60, 120, 240, 480] {
        for (radius, omega, radial_speed) in [(60., 20., 0.), (30., 40., 0.), (60., 20., -20.)] {
            for noise in [0., 0.3] {
                let path = |t: f32| Point {
                    x: (radius + radial_speed * t) * (omega * t).cos(),
                    y: (radius + radial_speed * t) * (omega * t).sin(),
                };
                let mut real = Vec::new();
                let mut state = PredictionState::default();
                let mut error = 0.;
                let mut count = 0;
                for i in 0..rate {
                    let time =
                        ((i as f64 + if i % 2 == 0 { 0. } else { 0.1 }) * 1e6 / rate as f64) as u32;
                    let t = time as f32 * 1e-6;
                    let p = path(t);
                    real.push(point(
                        p.x + noise * (i as f32 * 2.399).sin(),
                        p.y + noise * (i as f32 * 1.731).cos(),
                        time,
                    ));
                    let truth = path(t + 0.016);
                    let tip = state
                        .estimate(&real, &[], time + 16_000, IDENTITY, config())
                        .unwrap();
                    if time > 200_000 {
                        error += surface_distance(tip.point.position, truth, IDENTITY);
                    }
                    count += usize::from(time > 200_000);
                }
                let mean = error / count as f32;
                assert!(mean < 2., "must retain useful lookahead: {mean:?}");
            }
        }
    }
}

#[test]
fn trajectory_emits_a_curve_preserves_sensors_and_is_frame_independent() {
    let real = (0..80)
        .map(|i| {
            let angle = i as f32 * 0.004 * 40.;
            point(30. * angle.cos(), 30. * angle.sin(), i * 4000)
        })
        .collect::<Vec<_>>();
    let latest = *real.last().unwrap();
    let cfg = config();
    let mut state = PredictionState::default();
    let estimate = state
        .estimate(&real, &[], latest.elapsed_micros + 16_000, IDENTITY, cfg)
        .unwrap();
    assert_eq!(estimate.source, TipSource::Engine);
    let points = state
        .engine_intermediates()
        .chain([estimate.point])
        .collect::<Vec<_>>();
    assert_eq!(points.len(), 8);
    let mut previous = latest.elapsed_micros;
    for p in &points {
        assert!(p.elapsed_micros > previous);
        previous = p.elapsed_micros;
        assert!((p.position.x.hypot(p.position.y) - 30.).abs() < 0.01);
        assert_eq!(
            (p.pressure, p.tilt, p.twist),
            (latest.pressure, latest.tilt, latest.twist)
        );
    }
    assert_eq!(
        state.estimate(&real, &[], latest.elapsed_micros + 16_000, IDENTITY, cfg),
        Some(estimate)
    );
    let mut replay = PredictionState::default();
    for end in (1..real.len()).step_by(7) {
        replay.estimate(
            &real[..end],
            &[],
            real[end - 1].elapsed_micros + 16_000,
            IDENTITY,
            cfg,
        );
    }
    assert_eq!(
        replay.estimate(&real, &[], latest.elapsed_micros + 16_000, IDENTITY, cfg),
        Some(estimate)
    );
    for zoom in [0.1, 1., 8.] {
        let mapped = real
            .iter()
            .map(|p| StrokePoint {
                position: Point {
                    x: p.position.y / zoom,
                    y: -p.position.x / zoom,
                },
                ..*p
            })
            .collect::<Vec<_>>();
        let mut state = PredictionState::default();
        let transform = [0., zoom, -zoom, 0., 0., 0.];
        let actual = state
            .estimate(&mapped, &[], latest.elapsed_micros + 16_000, transform, cfg)
            .unwrap();
        assert!(
            surface_distance(
                transform_vector(transform, actual.point.position),
                estimate.point.position,
                IDENTITY
            ) < 0.01
        );
    }
    let native = [point(20., 20., latest.elapsed_micros + 16_000)];
    let native_tip = state
        .estimate(
            &real,
            &native,
            native[0].elapsed_micros,
            IDENTITY,
            InstantFeedbackConfig {
                use_platform_prediction: true,
                ..cfg
            },
        )
        .unwrap();
    assert_eq!(native_tip.source, TipSource::Platform);
    assert_eq!(state.engine_intermediates().count(), 0);
}

#[test]
fn staircase_pauses_do_not_keep_old_direction_on_restart() {
    let mut state = PredictionState::default();
    let mut real = Vec::new();
    for i in 0..120 {
        let leg = i / 30;
        let phase = (i % 30) as f32 * 0.004;
        let u = (phase - 0.06).clamp(0., 0.02);
        let distance = 1000. * phase.min(0.06) + 1000. * u - 25000. * u * u;
        let (x, y) = match leg {
            0 => (distance, 0.),
            1 => (70., distance),
            2 => (70. + distance, 70.),
            _ => (140., 70. + distance),
        };
        real.push(point(x, y, i * 4000));
        let tip = state
            .estimate(&real, &[], i * 4000 + 16000, IDENTITY, config())
            .unwrap()
            .point;
        if phase >= 0.072 {
            assert!(
                surface_distance(tip.position, real.last().unwrap().position, IDENTITY) <= 1.7,
                "leg={leg} phase={phase} tip={tip:?}"
            );
        }
        if leg % 2 == 0 {
            assert!((tip.position.y - y).abs() < 0.2, "old direction: {tip:?}");
        } else {
            assert!((tip.position.x - x).abs() < 0.2, "old direction: {tip:?}");
        }
    }
}

#[test]
fn prediction_expires_using_sample_age_and_restarts_from_fresh_measurements() {
    let mut real = (0..40)
        .map(|i| point(i as f32 * 4., 0., i * 4000))
        .collect::<Vec<_>>();
    let mut state = PredictionState::default();
    let cfg = config();
    let latest = *real.last().unwrap();
    let fresh = state
        .estimate_for(
            &real,
            &[],
            latest.elapsed_micros + 16_000,
            latest.elapsed_micros,
            IDENTITY,
            cfg,
        )
        .unwrap();
    assert!(fresh.point.position.x > latest.position.x + 14.);
    let stale = state
        .estimate_for(
            &real,
            &[],
            latest.elapsed_micros + 516_000,
            latest.elapsed_micros + 500_000,
            IDENTITY,
            cfg,
        )
        .unwrap();
    assert_eq!(stale.point, latest);
    assert_eq!(state.engine_intermediates().count(), 0);
    // New time-stamped observations, not presentation frames, restore motion.
    for i in 0..12 {
        real.push(point(
            156.,
            i as f32 * 4.,
            latest.elapsed_micros + 600_000 + i * 4000,
        ));
    }
    let newest = *real.last().unwrap();
    let tip = state
        .estimate(&real, &[], newest.elapsed_micros + 16_000, IDENTITY, cfg)
        .unwrap();
    assert!((tip.point.position.x - 156.).abs() < 0.01);
    assert!(tip.point.position.y > newest.position.y + 8.);
}

#[test]
fn trajectory_handles_quantized_times_duplicates_and_distance_caps() {
    let cfg = InstantFeedbackConfig {
        timestamp_resolution_micros: 1_000,
        ..config()
    };
    for rate in [60, 120, 240, 480] {
        for truncate in [false, true] {
            let mut state = PredictionState::default();
            let mut real = Vec::new();
            let mut error = 0.;
            let mut worst = 0.0_f32;
            let mut count = 0;
            for i in 0..rate {
                let t = i as f32 / rate as f32;
                let ticks = t * 1000.;
                let micros = if truncate {
                    ticks.floor()
                } else {
                    ticks.round()
                } as u32
                    * 1000;
                let p = point(60. * (20. * t).cos(), 60. * (20. * t).sin(), micros);
                real.extend([p, p]);
                let tip = state
                    .estimate(&real, &[], micros + 16_000, IDENTITY, cfg)
                    .unwrap();
                if t > 0.2 {
                    count += 1;
                    let t = t + 0.016;
                    let distance = surface_distance(
                        tip.point.position,
                        Point {
                            x: 60. * (20. * t).cos(),
                            y: 60. * (20. * t).sin(),
                        },
                        IDENTITY,
                    );
                    error += distance;
                    worst = worst.max(distance);
                }
            }
            // The 2 px forecast budget is RMS uncertainty, not a hard error bound.
            assert!(worst < 3., "quantized time worst={worst}");
            assert!(
                error / (count as f32) < 1.,
                "quantized time error {}",
                error / count as f32
            );
            let latest = *real.last().unwrap();
            let tip = state
                .estimate(
                    &real,
                    &[],
                    latest.elapsed_micros + 64000,
                    IDENTITY,
                    InstantFeedbackConfig {
                        prediction_horizon_micros: 64000,
                        max_prediction_distance_px: 8.,
                        ..cfg
                    },
                )
                .unwrap();
            for p in state.engine_intermediates().chain([tip.point]) {
                assert!(surface_distance(latest.position, p.position, IDENTITY) <= 8.01);
            }
        }
    }
}

#[test]
fn trajectory_length_is_not_quantized_to_curve_sampling_intervals() {
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
