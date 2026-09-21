//! Physical-pixel, independently clocked input/display regressions.
use super::{
    test_support::{config, run_frames},
    *,
};

#[derive(Clone, Copy, Debug)]
enum Motion {
    Line,
    Circle,
    Spiral,
}

fn kinematics(t: f64, width: f64, speed: f64, motion: Motion) -> (Point, [f64; 2]) {
    if matches!(motion, Motion::Line) {
        return (
            Point {
                x: (width * 0.1 + speed * t) as f32,
                y: (width * 0.28125) as f32,
            },
            [1., 0.],
        );
    }
    let initial_radius = width * 0.22;
    let radial_speed = if matches!(motion, Motion::Spiral) {
        -(initial_radius * 0.5).min(speed * 0.1)
    } else {
        0.
    };
    let tangential_speed = (speed * speed - radial_speed * radial_speed).sqrt();
    let radius = initial_radius + radial_speed * t;
    let angle = if radial_speed == 0. {
        speed / radius * t
    } else {
        tangential_speed / radial_speed * (radius / initial_radius).ln()
    };
    let (sin, cos) = angle.sin_cos();
    (
        Point {
            x: (width * 0.5 + radius * cos) as f32,
            y: (width * 0.28125 + radius * sin) as f32,
        },
        [
            (radial_speed * cos - tangential_speed * sin) / speed,
            (radial_speed * sin + tangential_speed * cos) / speed,
        ],
    )
}

#[derive(Clone, Copy, Debug, Default)]
struct Metrics {
    count: usize,
    squared_jump: f64,
    worst_jump: f64,
    sum_lead: f64,
    predicted: usize,
    previous: Option<f64>,
}

// Score longitudinal tip error against the display clock: sample-time queries
// alone hide phase errors that grow with speed on a full-screen canvas.
fn run(
    width: f64,
    speed: f64,
    input_hz: f64,
    display_hz: f64,
    motion: Motion,
    varying_pressure: bool,
    zoom: f32,
    horizon: u32,
    seed: u32,
) -> Metrics {
    let mut metrics = Metrics::default();
    run_frames(
        display_hz,
        0.8,
        zoom,
        InstantFeedbackConfig {
            prediction_horizon_micros: horizon,
            timestamp_resolution_micros: 1000,
            ..config()
        },
        true,
        |sample, now| {
            // Fractional phase avoids f32 floor artefacts at integer ticks;
            // millisecond quantization and irregular report periods remain.
            let t = (sample as f64 + if sample % 2 == 0 { 0. } else { 0.1 }) / input_hz
                + 0.00037
                + f64::from(seed) * 0.000137;
            if t + 0.002 > now {
                return None;
            }
            let p = kinematics(t, width, speed, motion).0;
            let noise = |axis: u32| {
                if seed == 0 {
                    if axis == 0 {
                        (sample as f32 * 2.399).sin()
                    } else {
                        (sample as f32 * 1.731).cos()
                    }
                } else {
                    let mut value = (sample as u32)
                        .wrapping_mul(0x9e3779b9)
                        .wrapping_add(seed.wrapping_mul(0x85ebca6b))
                        .wrapping_add(axis);
                    value ^= value >> 16;
                    value = value.wrapping_mul(0x7feb352d);
                    value ^= value >> 15;
                    (value as f64 / f64::from(u32::MAX) * 2. - 1.) as f32
                }
            };
            Some(StrokePoint {
                position: Point {
                    x: (p.x + 0.35 * noise(0)) / zoom,
                    y: (p.y + 0.35 * noise(1)) / zoom,
                },
                elapsed_micros: (t * 1000.).floor() as u32 * 1000,
                pressure: if varying_pressure {
                    (0.18 + 0.08 * (t * 60.).sin()) as f32
                } else {
                    0.6
                },
                tilt: [0.; 2],
                twist: 0.,
            })
        },
        |now, tip, _| {
            let (truth, tangent) = kinematics(now, width, speed, motion);
            if now > 0.2 {
                let lead = (f64::from(tip.point.position.x * zoom - truth.x) * tangent[0])
                    + (f64::from(tip.point.position.y * zoom - truth.y) * tangent[1]);
                if let Some(previous) = metrics.previous {
                    let jump = lead - previous;
                    metrics.squared_jump += jump * jump;
                    metrics.worst_jump = metrics.worst_jump.max(jump.abs());
                }
                metrics.count += 1;
                metrics.sum_lead += lead;
                metrics.previous = Some(lead);
                metrics.predicted += usize::from(tip.source == TipSource::Engine);
            }
        },
    );
    metrics
}

#[test]
fn fullscreen_resolution_speed_sweep() {
    for width in [1280., 1920., 3840., 7680.] {
        for speed in [250., 1000., 4000., 8000., 16000.] {
            for input_hz in [120., 200., 240., 480.] {
                for display_hz in [60., 119.88, 144.] {
                    for motion in [Motion::Line, Motion::Circle] {
                        let m = run(
                            width, speed, input_hz, display_hz, motion, true, 1., 32_000, 0,
                        );
                        // RMS budget: half a clock tick of travel plus
                        // 1 px. Worst-frame budget: one tick plus 2 px.
                        // Both must hold without prediction dropouts.
                        assert!(
                            (m.squared_jump / m.count as f64).sqrt() < 1. + speed * 0.0005,
                            "RMS: {width} {speed} {input_hz} {display_hz} {motion:?}"
                        );
                        assert!(
                            m.worst_jump < 2. + speed * 0.001,
                            "jump {}: {width} {speed} {input_hz} {display_hz} {motion:?}",
                            m.worst_jump
                        );
                        assert_eq!(
                            m.predicted, m.count,
                            "no prediction dropouts during steady motion"
                        );
                        assert!(
                            m.sum_lead / m.count as f64 > (speed * 0.032).min(96.) * 0.8,
                            "stability must preserve useful lookahead"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn full_screen_pressure_modulation_does_not_change_trajectory_or_clock_phase() {
    for input_hz in [120., 200., 240., 480.] {
        for motion in [Motion::Line, Motion::Circle] {
            let fixed = run(3840., 6800., input_hz, 119.88, motion, false, 1., 32_000, 0);
            let varied = run(3840., 6800., input_hz, 119.88, motion, true, 1., 32_000, 0);
            let zoomed = run(3840., 6800., input_hz, 119.88, motion, true, 4., 32_000, 0);
            assert_eq!(fixed.squared_jump, varied.squared_jump);
            assert_eq!(fixed.worst_jump, varied.worst_jump);
            assert_eq!(fixed.squared_jump, zoomed.squared_jump);
        }
    }
}

#[test]
fn fullscreen_prediction_is_stable_across_clock_phases_and_noise_seeds() {
    for seed in 1..=8 {
        for speed in [250., 4000., 16000.] {
            for input_hz in [120., 200., 480.] {
                for motion in [Motion::Line, Motion::Circle] {
                    let horizon = if seed % 2 == 0 { 16_000 } else { 32_000 };
                    let m = run(
                        3840., speed, input_hz, 119.88, motion, true, 2., horizon, seed,
                    );
                    assert!(
                        (m.squared_jump / m.count as f64).sqrt() < 1.5 + speed * 0.0004,
                        "seed={seed} speed={speed} input={input_hz} motion={motion:?}: {m:?}"
                    );
                    assert!(
                        m.worst_jump < 6. + speed * 0.001,
                        "seed={seed} speed={speed} input={input_hz} motion={motion:?}: {m:?}"
                    );
                    assert_eq!(m.predicted, m.count, "no steady-motion dropouts: {m:?}");
                    assert!(
                        m.sum_lead / m.count as f64
                            > (speed * f64::from(horizon) * 1e-6).min(96.) * 0.8
                    );
                }
            }
        }
    }
}

#[test]
fn fullscreen_spirals_remain_stable_as_curvature_changes() {
    for width in [1280., 1920., 3840., 7680.] {
        for speed in [1000., 4000., 16000.] {
            for input_hz in [120., 200., 240., 480.] {
                // Radius contracts while angular velocity increases. Actual
                // path speed is constant, so changing lead exposes instability
                // rather than the expected response to slowing input.
                let m = run(
                    width,
                    speed,
                    input_hz,
                    119.88,
                    Motion::Spiral,
                    true,
                    2.,
                    32_000,
                    0,
                );
                assert!(
                    (m.squared_jump / m.count as f64).sqrt() < 2. + speed * 0.0004,
                    "{width} {speed} {input_hz}: {m:?}"
                );
                assert!(
                    m.worst_jump < 6. + speed * 0.001,
                    "{width} {speed} {input_hz}: {m:?}"
                );
                assert_eq!(m.predicted, m.count, "no sustained-motion dropouts: {m:?}");
                assert!(m.sum_lead / m.count as f64 > (speed * 0.032).min(96.) * 0.8);
            }
        }
    }
}
