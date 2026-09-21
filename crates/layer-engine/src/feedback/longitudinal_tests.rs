use super::{
    test_support::{IDENTITY, config, point},
    *,
};

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
    for rate in [240, 480] {
        for kind in 0..3 {
            for truncate in [false, true] {
                for phase in [0., 0.27, 0.61] {
                    let mut state = PredictionState::default();
                    let mut real = Vec::new();
                    let mut previous = None;
                    let mut jitter = 0.;
                    let mut worst = 0.0_f32;
                    let mut error = 0.;
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
                            p.x + 0.35 * (i as f32 * 2.399 + phase).sin(),
                            p.y + 0.35 * (i as f32 * 1.731 + phase).cos(),
                            micros,
                        ));
                        let truth = path(time + 0.032, kind);
                        let ahead = path(time + 0.0321, kind);
                        let dx = ahead.x - truth.x;
                        let dy = ahead.y - truth.y;
                        let tip = state
                            .estimate(
                                &real,
                                &[],
                                micros + 32_000,
                                IDENTITY,
                                InstantFeedbackConfig {
                                    prediction_horizon_micros: 32_000,
                                    timestamp_resolution_micros: 1_000,
                                    ..config()
                                },
                            )
                            .unwrap();
                        if time > 0.2 {
                            let along = ((tip.point.position.x - truth.x) * dx
                                + (tip.point.position.y - truth.y) * dy)
                                / dx.hypot(dy);
                            error += along * along;
                            if let Some(before) = previous {
                                let jump: f32 = along - before;
                                jitter += jump * jump;
                                worst = worst.max(jump.abs());
                            }
                            previous = Some(along);
                        }
                        count += usize::from(time > 0.2);
                    }
                    assert!(
                        (jitter / count as f32).sqrt() < 1.5,
                        "{rate}Hz path={kind} {truncate} {phase}: excessive longitudinal jitter"
                    );
                    assert!(
                        worst < 4.,
                        "a single spurious retraction still flashes: {worst:?}"
                    );
                    assert!(
                        (error / count as f32).sqrt() < 1.5,
                        "stability must not come from suppressing useful prediction"
                    );
                }
            }
        }
    }
}
