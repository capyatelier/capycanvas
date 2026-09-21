//! Physical-pixel, independently clocked input/display regressions.
use super::*;

const ALGORITHMS: [PredictionAlgorithm; 1] = [PredictionAlgorithm::Trajectory];

#[derive(Clone, Copy)]
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

// This measures tip error in the direction of travel against the FRAME clock.
// Measuring against latest input, or calling render only at sample times, hides
// the input/display phase error that grows with speed on a full-screen canvas.
fn run(
    width: f64,
    speed: f64,
    input_hz: f64,
    display_hz: f64,
    circle: bool,
    varying_pressure: bool,
    zoom: f32,
    horizon: u32,
    seed: u32,
) -> [Metrics; 1] {
    run_motion(
        width,
        speed,
        input_hz,
        display_hz,
        if circle { Motion::Circle } else { Motion::Line },
        varying_pressure,
        zoom,
        horizon,
        seed,
    )
}

fn run_motion(
    width: f64,
    speed: f64,
    input_hz: f64,
    display_hz: f64,
    motion: Motion,
    varying_pressure: bool,
    zoom: f32,
    horizon: u32,
    seed: u32,
) -> [Metrics; 1] {
    let mut states = ALGORITHMS.map(|_| PredictionState::default());
    let mut real = Vec::new();
    let mut metrics = [Metrics::default(); 1];
    let mut sample = 0;
    let transform = [zoom, 0., 0., zoom, 0., 0.];
    for frame in 1..=(display_hz * 0.8) as usize {
        let now = frame as f64 / display_hz;
        loop {
            // Fractional phase avoids f32 floor artefacts at integer ticks;
            // real millisecond quantization and irregular report periods remain.
            let t = (sample as f64 + if sample % 2 == 0 { 0. } else { 0.1 }) / input_hz
                + 0.00037
                + f64::from(seed) * 0.000137;
            if t + 0.002 > now {
                break;
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
            let point = StrokePoint {
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
            };
            for state in &mut states {
                state.observe(PenEvent {
                    device_id: 1,
                    sequence: sample,
                    timestamp_ns: u64::from(point.elapsed_micros) * 1000,
                    view_revision: 1,
                    surface_position: point.position,
                    pressure: point.pressure,
                    tilt_radians: [0.; 2],
                    twist_radians: 0.,
                    distance: 0.,
                    phase: crate::PenPhase::Move,
                    tool: ToolKind::Pen,
                    flags: SampleFlags::PRIMARY,
                });
            }
            real.push(point);
            sample += 1;
        }
        if real.is_empty() {
            continue;
        }
        let (truth, tangent) = kinematics(now, width, speed, motion);
        for (j, algorithm) in ALGORITHMS.into_iter().enumerate() {
            let cfg = InstantFeedbackConfig {
                prediction_algorithm: algorithm,
                use_platform_prediction: false,
                prediction_horizon_micros: horizon,
                timestamp_resolution_micros: 1000,
                ..Default::default()
            };
            let tip = states[j]
                .estimate_for(
                    &real,
                    &[],
                    (now * 1e6) as u32 + horizon,
                    (now * 1e6) as u32,
                    transform,
                    cfg,
                )
                .unwrap();
            let latest = real.last().unwrap();
            for p in states[j].engine_intermediates().chain([tip.point]) {
                assert!(p.position.x.is_finite() && p.position.y.is_finite());
                assert!(p.elapsed_micros >= latest.elapsed_micros);
                assert!(p.elapsed_micros - latest.elapsed_micros <= MAX_PREDICTION_HORIZON_MICROS);
                assert!(
                    surface_distance(latest.position, p.position, transform)
                        <= MAX_PREDICTION_DISTANCE_PX + 0.02
                );
                assert_eq!(
                    (p.pressure, p.tilt, p.twist),
                    (latest.pressure, latest.tilt, latest.twist)
                );
            }
            if now > 0.2 {
                let lead = (f64::from(tip.point.position.x * zoom - truth.x) * tangent[0])
                    + (f64::from(tip.point.position.y * zoom - truth.y) * tangent[1]);
                let m = &mut metrics[j];
                if let Some(previous) = m.previous {
                    let jump = lead - previous;
                    m.squared_jump += jump * jump;
                    m.worst_jump = m.worst_jump.max(jump.abs());
                }
                m.count += 1;
                m.sum_lead += lead;
                m.previous = Some(lead);
                m.predicted += usize::from(tip.source == TipSource::Engine);
            }
        }
    }
    metrics
}

#[test]
fn fullscreen_resolution_speed_sweep() {
    println!(
        "width,speed,input_hz,display_hz,circle,algorithm,jitter_rms,worst_jump,mean_lead,predicted_fraction"
    );
    for width in [1280., 1920., 3840., 7680.] {
        for speed in [250., 1000., 4000., 8000., 16000.] {
            for input_hz in [120., 200., 240., 480.] {
                for display_hz in [60., 119.88, 144.] {
                    for circle in [false, true] {
                        let metrics = run(
                            width, speed, input_hz, display_hz, circle, true, 1., 32_000, 0,
                        );
                        for (algorithm, m) in ALGORITHMS.into_iter().zip(metrics) {
                            // RMS budget: half a clock tick of travel plus
                            // 1 px. Worst-frame budget: one tick plus 2 px.
                            // Both must hold without prediction dropouts.
                            assert!(
                                (m.squared_jump / m.count as f64).sqrt() < 1. + speed * 0.0005,
                                "RMS: {width} {speed} {input_hz} {display_hz} {circle} {algorithm:?}"
                            );
                            assert!(
                                m.worst_jump < 2. + speed * 0.001,
                                "jump {}: {width} {speed} {input_hz} {display_hz} {circle} {algorithm:?}",
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
                            println!(
                                "{width},{speed},{input_hz},{display_hz},{circle},{algorithm:?},{},{},{},{}",
                                (m.squared_jump / m.count as f64).sqrt(),
                                m.worst_jump,
                                m.sum_lead / m.count as f64,
                                m.predicted as f64 / m.count as f64
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn full_screen_pressure_modulation_does_not_change_trajectory_or_clock_phase() {
    for input_hz in [120., 200., 240., 480.] {
        for circle in [false, true] {
            let fixed = run(3840., 6800., input_hz, 119.88, circle, false, 1., 32_000, 0);
            let varied = run(3840., 6800., input_hz, 119.88, circle, true, 1., 32_000, 0);
            let zoomed = run(3840., 6800., input_hz, 119.88, circle, true, 4., 32_000, 0);
            for j in 0..1 {
                assert_eq!(fixed[j].squared_jump, varied[j].squared_jump);
                assert_eq!(fixed[j].worst_jump, varied[j].worst_jump);
                assert_eq!(fixed[j].squared_jump, zoomed[j].squared_jump);
            }
        }
    }
}

#[test]
fn fullscreen_prediction_is_stable_across_clock_phases_and_noise_seeds() {
    for seed in 1..=8 {
        for speed in [250., 4000., 16000.] {
            for input_hz in [120., 200., 480.] {
                for circle in [false, true] {
                    let horizon = if seed % 2 == 0 { 16_000 } else { 32_000 };
                    for m in run(
                        3840., speed, input_hz, 119.88, circle, true, 2., horizon, seed,
                    ) {
                        assert!(
                            (m.squared_jump / m.count as f64).sqrt() < 1.5 + speed * 0.0004,
                            "seed={seed} speed={speed} input={input_hz} circle={circle}: {m:?}"
                        );
                        assert!(
                            m.worst_jump < 6. + speed * 0.001,
                            "seed={seed} speed={speed} input={input_hz} circle={circle}: {m:?}"
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
}

#[test]
fn fullscreen_spirals_remain_stable_as_curvature_changes() {
    for width in [1280., 1920., 3840., 7680.] {
        for speed in [1000., 4000., 16000.] {
            for input_hz in [120., 200., 240., 480.] {
                // Radius contracts while angular velocity increases. Actual
                // path speed is constant, so changing lead exposes instability
                // rather than the expected response to slowing input.
                for m in run_motion(
                    width,
                    speed,
                    input_hz,
                    119.88,
                    Motion::Spiral,
                    true,
                    2.,
                    32_000,
                    0,
                ) {
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
}
