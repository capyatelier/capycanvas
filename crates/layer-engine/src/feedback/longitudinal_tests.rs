use super::*;

const IDENTITY: [f32; 6] = [1., 0., 0., 1., 0., 0.];

fn config(algorithm: PredictionAlgorithm) -> InstantFeedbackConfig {
    InstantFeedbackConfig {
        use_platform_prediction: false,
        prediction_algorithm: algorithm,
        prediction_horizon_micros: 16_000,
        timestamp_resolution_micros: 1_000,
        ..Default::default()
    }
}
fn point(position: Point, time: u32) -> StrokePoint {
    StrokePoint {
        position,
        elapsed_micros: time,
        pressure: 0.6,
        tilt: [0.2, -0.1],
        twist: 0.7,
    }
}
fn path(t: f32, kind: usize) -> Point {
    match kind {
        0 => Point { x: 700. * t, y: 0. },
        1 => Point {
            x: 30. * (40. * t).cos(),
            y: 30. * (40. * t).sin(),
        },
        _ => Point {
            x: (60. - 20. * t) * (20. * t).cos(),
            y: (60. - 20. * t) * (20. * t).sin(),
        },
    }
}

// A steady circle at 480 Hz previously collapsed repeatedly from 32 ms of
// lookahead to 2 ms, with >12 px RMS tip jumps. Vary timestamp phase, rounding,
// spatial noise and the path so the test cannot pass by recognizing one trace.
#[test]
fn trajectory_keeps_steady_motion_lead_stable_with_gtk_timestamps() {
    let algorithms = [PredictionAlgorithm::Trajectory];
    for rate in [240, 480] {
        for kind in 0..3 {
            for truncate in [false, true] {
                for phase in [0., 0.27, 0.61] {
                    let mut states = algorithms.map(|_| PredictionState::default());
                    let mut real = Vec::new();
                    let mut previous = [None; 1];
                    let mut jitter = [0.; 1];
                    let mut worst = [0.0_f32; 1];
                    let mut error = [0.; 1];
                    let mut count = 0;
                    for i in 0..rate {
                        let time =
                            (i as f32 + phase + if i % 2 == 0 { 0. } else { 0.1 }) / rate as f32;
                        let ticks = time * 1000.;
                        let micros = if truncate {
                            ticks.floor()
                        } else {
                            ticks.round()
                        } as u32
                            * 1000;
                        let p = path(time, kind);
                        real.push(point(
                            Point {
                                x: p.x + 0.35 * (i as f32 * 2.399 + phase).sin(),
                                y: p.y + 0.35 * (i as f32 * 1.731 + phase).cos(),
                            },
                            micros,
                        ));
                        let truth = path(time + 0.032, kind);
                        let ahead = path(time + 0.0321, kind);
                        let dx = ahead.x - truth.x;
                        let dy = ahead.y - truth.y;
                        for (j, algorithm) in algorithms.into_iter().enumerate() {
                            let tip = states[j]
                                .estimate(
                                    &real,
                                    &[],
                                    micros + 32_000,
                                    IDENTITY,
                                    InstantFeedbackConfig {
                                        prediction_horizon_micros: 32_000,
                                        ..config(algorithm)
                                    },
                                )
                                .unwrap();
                            if time > 0.2 {
                                let along = ((tip.point.position.x - truth.x) * dx
                                    + (tip.point.position.y - truth.y) * dy)
                                    / dx.hypot(dy);
                                error[j] += along * along;
                                if let Some(before) = previous[j] {
                                    let jump: f32 = along - before;
                                    jitter[j] += jump * jump;
                                    worst[j] = worst[j].max(jump.abs());
                                }
                                previous[j] = Some(along);
                            }
                        }
                        count += usize::from(time > 0.2);
                    }
                    eprintln!(
                        "steady {rate}Hz path={kind} truncate={truncate} phase={phase}: along jitter={:?} worst={worst:?} error={:?}",
                        jitter.map(|v| (v / count as f32).sqrt()),
                        error.map(|v| (v / count as f32).sqrt())
                    );
                    for j in 0..1 {
                        assert!(
                            (jitter[j] / count as f32).sqrt() < 1.5,
                            "{rate}Hz path={kind} {truncate} {phase} {:?}: excessive longitudinal jitter",
                            algorithms[j]
                        );
                        assert!(
                            worst[j] < 4.,
                            "a single spurious retraction still flashes: {worst:?}"
                        );
                        assert!(
                            (error[j] / count as f32).sqrt() < 1.5,
                            "stability must not come from suppressing useful prediction"
                        );
                    }
                }
            }
        }
    }
}
