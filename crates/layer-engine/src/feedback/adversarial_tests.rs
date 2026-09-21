//! Independent motion families motivated by real error classes, without captured
//! coordinates or device-specific constants. Truth is integrated offline; only
//! causally delivered, noisy, quantized samples reach PredictionState.
use super::*;

#[derive(Clone, Copy, Debug)]
enum Motion {
    SpeedWave,
    BreathingCurve,
    ChangingTurn,
    Startup,
    Staircase,
    Reversal,
    Burst,
    CurvedBurst,
    Chirp,
    Maneuver,
}
const MOTIONS: [Motion; 6] = [
    Motion::SpeedWave,
    Motion::BreathingCurve,
    Motion::ChangingTurn,
    Motion::Startup,
    Motion::Staircase,
    Motion::Reversal,
];
const STEP: f64 = 0.0001;
struct Truth(Vec<(Point, f64)>);
impl Truth {
    fn new(
        motion: Motion,
        speed: f64,
        radius: f64,
        frequency: f64,
        phase: f64,
        brake: f64,
    ) -> Self {
        let mut values = vec![(Point { x: 0., y: 0. }, 0.)];
        let mut p = [0.; 2];
        let mut heading = 0.;
        for i in 1..=14_000 {
            let t = (i as f64 - 0.5) * STEP;
            let wave = (t * frequency * std::f64::consts::TAU + phase).sin();
            let (v, omega) = match motion {
                Motion::SpeedWave => (speed * (1. + 0.55 * wave), 0.),
                Motion::BreathingCurve => (
                    speed * (1. + 0.45 * wave),
                    speed / radius * (1. + 0.6 * wave),
                ),
                Motion::ChangingTurn => (speed, speed / radius * 2. * wave),
                Motion::Chirp => {
                    let angle = std::f64::consts::TAU * frequency * (t + 0.35 * t * t) + phase;
                    (
                        speed * (1. + 0.45 * angle.sin()),
                        speed / radius * (0.7 + 0.7 * (0.71 * angle).cos()),
                    )
                }
                Motion::Maneuver => {
                    // Nonperiodic changes in speed and turn. Independent knot
                    // values, with continuous acceleration at the joins, avoid
                    // reusing either captured paths or the periodic families.
                    let u = t / brake + phase;
                    let segment = u.floor() as u32;
                    let fraction = u.fract();
                    let blend = fraction * fraction * (3. - 2. * fraction);
                    let knot = |index: u32, salt: u32| {
                        let mut h = index.wrapping_add(salt).wrapping_mul(0x9e37_79b9);
                        h ^= h >> 16;
                        h = h.wrapping_mul(0x85eb_ca6b);
                        h ^= h >> 13;
                        f64::from(h) / f64::from(u32::MAX)
                    };
                    let interpolate =
                        |salt| knot(segment, salt) * (1. - blend) + knot(segment + 1, salt) * blend;
                    (
                        speed * (0.15 + 1.7 * interpolate(17)),
                        speed / radius * (4. * interpolate(73) - 2.),
                    )
                }
                Motion::Startup => (
                    speed * (1. - (-t / (0.012 + 0.06 / frequency)).exp()),
                    speed / radius,
                ),
                Motion::Burst | Motion::CurvedBurst => {
                    // Smooth velocity pulses expose slow-to-fast tracking lag:
                    // the rising side has increasing acceleration, unlike a
                    // constant-acceleration launch. No captured coordinates.
                    let local = (t + phase * 0.01) % 0.4;
                    let v = speed * (0.02 + 0.98 * (-0.5 * ((local - 0.22) / brake).powi(2)).exp());
                    (
                        v,
                        if matches!(motion, Motion::CurvedBurst) {
                            v / radius
                        } else {
                            0.
                        },
                    )
                }
                Motion::Staircase | Motion::Reversal => {
                    let phase = t % 0.4;
                    let v = if phase < 0.2 {
                        speed
                    } else if phase < 0.2 + brake {
                        speed * (1. + ((phase - 0.2) / brake * std::f64::consts::PI).cos()) / 2.
                    } else if phase < 0.32 {
                        0.
                    } else if phase < 0.32 + brake {
                        speed * (1. - ((phase - 0.32) / brake * std::f64::consts::PI).cos()) / 2.
                    } else {
                        speed
                    };
                    let turn = if matches!(motion, Motion::Reversal) {
                        std::f64::consts::PI
                    } else {
                        std::f64::consts::FRAC_PI_2
                    };
                    let omega = if (0.25..0.31).contains(&phase) {
                        turn / 0.06
                    } else {
                        0.
                    };
                    (v, omega)
                }
            };
            let mid = heading + omega * STEP * 0.5;
            p[0] += v * mid.cos() * STEP;
            p[1] += v * mid.sin() * STEP;
            heading += omega * STEP;
            values.push((
                Point {
                    x: p[0] as f32,
                    y: p[1] as f32,
                },
                v,
            ));
        }
        Self(values)
    }
    fn at(&self, t: f64) -> (Point, f64) {
        let f = (t / STEP).clamp(0., (self.0.len() - 2) as f64);
        let i = f.floor() as usize;
        let w = (f - i as f64) as f32;
        let (a, va) = self.0[i];
        let (b, vb) = self.0[i + 1];
        (
            Point {
                x: a.x + (b.x - a.x) * w,
                y: a.y + (b.y - a.y) * w,
            },
            va + (vb - va) * f64::from(w),
        )
    }
}

#[test]
#[ignore = "independent nonperiodic validation; run after candidate selection"]
fn nonperiodic_maneuver_holdout() {
    for case in 0..48 {
        for algorithm in [PredictionAlgorithm::Trajectory] {
            run_with(
                Motion::Maneuver,
                [1440., 3200., 5760.][case % 3],
                [650., 1800., 4500., 10_000.][(case / 3) % 4],
                [100., 144., 300., 720.][case % 4],
                [50., 100., 200.][(case / 4) % 3],
                1.,
                701 + case as u32,
                algorithm,
                Perturbations {
                    phase: 0.23 + case as f64 * 1.17,
                    brake: [0.055, 0.095, 0.17][(case / 8) % 3],
                    quantum: [0, 250, 1000, 2000][(case / 2) % 4],
                    jitter: [0., 0.08, 0.27][case % 3],
                    noise: [0., 0.15, 0.65, 1.5][(case / 4) % 4],
                    missing: case % 5 == 0,
                },
            );
        }
    }
}
#[derive(Default, Debug)]
struct Metrics {
    queries: usize,
    wrong_jumps: usize,
    sum_error2: f64,
    sum_lead: f64,
    moving: usize,
    predicted: usize,
    worst_error: f64,
    large_jumps: usize,
    severe_jumps: usize,
    extreme_jumps: usize,
    worst_jump: f64,
    transitions: usize,
    tiny_jumps: usize,
    sum_jump2: f64,
}
#[derive(Clone, Copy)]
struct Perturbations {
    phase: f64,
    brake: f64,
    quantum: u32,
    jitter: f64,
    noise: f64,
    missing: bool,
}
impl Default for Perturbations {
    fn default() -> Self {
        Self {
            phase: 0.37,
            brake: 0.04,
            quantum: 1000,
            jitter: 0.11,
            noise: 0.4,
            missing: false,
        }
    }
}
fn run(
    motion: Motion,
    width: f64,
    speed: f64,
    input_hz: f64,
    display_hz: f64,
    frequency: f64,
    seed: u32,
    algorithm: PredictionAlgorithm,
) -> Metrics {
    run_with(
        motion,
        width,
        speed,
        input_hz,
        display_hz,
        frequency,
        seed,
        algorithm,
        Perturbations::default(),
    )
}
fn run_with(
    motion: Motion,
    width: f64,
    speed: f64,
    input_hz: f64,
    display_hz: f64,
    frequency: f64,
    seed: u32,
    algorithm: PredictionAlgorithm,
    input: Perturbations,
) -> Metrics {
    let truth = Truth::new(
        motion,
        speed,
        width * 0.18,
        frequency,
        input.phase,
        input.brake,
    );
    let zoom = [0.25, 1., 4.][seed as usize % 3];
    let transform = [zoom, 0., 0., zoom, 0., 0.];
    let mut state = PredictionState::default();
    let mut real = Vec::new();
    let mut sample = 0usize;
    let mut noise = [0.; 2];
    let mut random = seed.wrapping_add(17);
    let mut previous: Option<([f64; 2], f64)> = None;
    let cfg = InstantFeedbackConfig {
        prediction_algorithm: algorithm,
        prediction_horizon_micros: 32_000,
        use_platform_prediction: false,
        timestamp_resolution_micros: input.quantum,
        ..Default::default()
    };
    let mut m = Metrics::default();
    for frame in 1..=(display_hz * 1.1) as usize {
        let now = frame as f64 / display_hz;
        loop {
            let missing = if input.missing { 2 * (sample / 37) } else { 0 };
            let t = ((sample + missing) as f64 + input.jitter * (sample % 2) as f64) / input_hz
                + 0.00027
                + f64::from(seed) * 0.000073;
            let delay = if seed % 3 == 2 && sample % 41 < 3 {
                0.010
            } else {
                0.0006 + f64::from(seed % 4) * 0.0005
            };
            if t + delay > now {
                break;
            }
            let p = truth.at(t).0;
            for axis in 0..2 {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                noise[axis] = 0.45 * noise[axis]
                    + input.noise * (f64::from(random) / f64::from(u32::MAX) * 2. - 1.);
            }
            real.push(StrokePoint {
                position: Point {
                    x: (p.x + noise[0] as f32) / zoom,
                    y: (p.y + noise[1] as f32) / zoom,
                },
                elapsed_micros: (t * 1e6 / f64::from(input.quantum.max(1))).floor() as u32
                    * input.quantum.max(1),
                pressure: 0.6,
                tilt: [0.; 2],
                twist: 0.,
            });
            sample += 1;
        }
        if real.is_empty() {
            continue;
        }
        let tip = state
            .estimate_for(
                &real,
                &[],
                (now * 1e6) as u32 + 32_000,
                (now * 1e6) as u32,
                transform,
                cfg,
            )
            .unwrap();
        let p = Point {
            x: tip.point.position.x * zoom,
            y: tip.point.position.y * zoom,
        };
        assert!(p.x.is_finite() && p.y.is_finite());
        assert!(
            surface_distance(tip.point.position, real.last().unwrap().position, transform)
                <= MAX_PREDICTION_DISTANCE_PX + 0.01,
            "absolute physical distance bound"
        );
        assert!(tip.point.elapsed_micros >= real.last().unwrap().elapsed_micros);
        assert!(tip.point.elapsed_micros - real.last().unwrap().elapsed_micros <= 64_000);
        let exact = truth.at(f64::from(tip.point.elapsed_micros) * 1e-6).0;
        let error = [f64::from(p.x - exact.x), f64::from(p.y - exact.y)];
        let amount = error[0].hypot(error[1]);
        if let Some((old, old_amount)) = previous {
            let jump = (error[0] - old[0]).hypot(error[1] - old[1]);
            m.transitions += 1;
            m.sum_jump2 += jump * jump;
            m.tiny_jumps += usize::from((4. ..8.).contains(&jump) && amount.max(old_amount) >= 4.);
            if jump >= 8. && amount.max(old_amount) >= 8. {
                m.wrong_jumps += 1;
                m.large_jumps += usize::from(jump >= 16.);
                m.severe_jumps += usize::from(jump >= 32.);
                m.extreme_jumps += usize::from(jump >= 40.);
                m.worst_jump = m.worst_jump.max(jump);
            }
        }
        previous = Some((error, amount));
        m.queries += 1;
        m.sum_error2 += amount * amount;
        m.worst_error = m.worst_error.max(amount);
        if truth.at(now).1 > speed * 0.3 && now > 0.06 {
            m.moving += 1;
            m.predicted += usize::from(tip.source == TipSource::Engine);
            m.sum_lead += (f64::from(tip.point.elapsed_micros) * 1e-6 - now) * 1000.;
        }
    }
    // Opt-in machine-readable scores include the smaller errors as well as
    // lookahead. Keep every generated case, including startup and hard pulses.
    if std::env::var_os("CAPY_PREDICTION_SCORE").is_some() {
        eprintln!(
            "prediction-score,{motion:?},{width},{speed},{input_hz},{display_hz},{frequency},{seed},{algorithm:?},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            m.queries,
            m.transitions,
            m.tiny_jumps,
            m.wrong_jumps,
            m.large_jumps,
            m.severe_jumps,
            m.extreme_jumps,
            m.sum_error2,
            m.sum_jump2,
            m.sum_lead,
            m.moving,
            m.predicted,
            m.worst_jump,
        );
    }
    m
}

#[test]
fn increasing_frequency_motion_holdout() {
    // New phases, report/display rates and scales. Frequency changes during
    // each stroke, so this cannot reuse a tuned periodic sampling pattern.
    let mut counts = [0; 4];
    let mut worst_jump = 0_f64;
    let mut error2 = 0.;
    let mut lead = 0.;
    let mut coverage = 0.;
    for case in 0..36 {
        for algorithm in [PredictionAlgorithm::Trajectory] {
            let m = run_with(
                Motion::Chirp,
                [1536., 4096., 8192.][case % 3],
                [750., 2500., 7500.][(case / 3) % 3],
                [100., 200., 500.][(case / 9) % 3],
                [90., 120., 180.][(case / 4) % 3],
                [1.3, 3.7, 7.1][(case / 2) % 3],
                case as u32 + 101,
                algorithm,
                Perturbations {
                    phase: 0.91 + case as f64 * 0.43,
                    quantum: [0, 500, 1000][case % 3],
                    noise: [0., 0.25, 0.75][(case / 4) % 3],
                    jitter: [0.05, 0.2][case % 2],
                    missing: case % 7 == 0,
                    ..Default::default()
                },
            );
            println!(
                "chirp,{case},{algorithm:?},{},{},{},{},{},{},{},{}",
                m.wrong_jumps,
                m.large_jumps,
                m.severe_jumps,
                m.extreme_jumps,
                m.worst_jump,
                (m.sum_error2 / m.queries as f64).sqrt(),
                m.sum_lead / m.moving.max(1) as f64,
                m.predicted as f64 / m.moving.max(1) as f64
            );
            for (a, b) in counts.iter_mut().zip([
                m.wrong_jumps,
                m.large_jumps,
                m.severe_jumps,
                m.extreme_jumps,
            ]) {
                *a += b;
            }
            worst_jump = worst_jump.max(m.worst_jump);
            error2 += m.sum_error2 / m.queries as f64;
            lead += m.sum_lead / m.moving.max(1) as f64;
            coverage += m.predicted as f64 / m.moving.max(1) as f64;
        }
    }
    println!(
        "chirp-total,{counts:?},{worst_jump},{},{},{}",
        (error2 / 36.).sqrt(),
        lead / 36.,
        coverage / 36.
    );
    // Bound accuracy and flicker while retaining useful forecasts in hard cases.
    assert!(counts[0] <= 1500, "chirp flicker: {counts:?}");
    assert!(counts[1] <= 545, "chirp medium/large: {counts:?}");
    assert!(counts[2] <= 190, "chirp severe: {counts:?}");
    assert!(counts[3] <= 138, "chirp extreme: {counts:?}");
    assert!(worst_jump <= 176., "chirp worst jump: {worst_jump}");
    assert!((error2 / 36.).sqrt() <= 14., "chirp accuracy");
    assert!(lead / 36. >= 14., "retain chirp lookahead");
    assert!(coverage / 36. >= 0.90, "retain chirp coverage");
}

#[test]
fn accelerating_bursts_and_turns_regression() {
    println!("burst,motion,algorithm,jumps,large,severe,extreme,worst_jump,rms,lead,coverage");
    let mut large = 0;
    let mut severe = 0;
    let mut extreme = 0;
    let mut worst_jump = 0_f64;
    let mut lead = 0.;
    let mut coverage = 0.;
    let mut error2 = 0.;
    for motion in [Motion::Burst, Motion::CurvedBurst] {
        for case in 0..36 {
            for algorithm in [PredictionAlgorithm::Trajectory] {
                let m = run_with(
                    motion,
                    [1280., 3840., 7680.][case % 3],
                    [1000., 4000., 12_000.][(case / 3) % 3],
                    [120., 240., 480., 1000.][case % 4],
                    [60., 119.88, 165.][(case / 4) % 3],
                    1.,
                    case as u32 + 31,
                    algorithm,
                    Perturbations {
                        phase: 0.13 + case as f64 * 0.71,
                        brake: [0.012, 0.025, 0.05][(case / 9) % 3],
                        quantum: [0, 500, 1000][case % 3],
                        jitter: [0., 0.15, 0.35][(case / 3) % 3],
                        noise: [0.05, 0.4, 1.2][(case / 4) % 3],
                        missing: case % 5 == 0,
                    },
                );
                println!(
                    "{case},{motion:?},{algorithm:?},{},{},{},{},{},{},{},{}",
                    m.wrong_jumps,
                    m.large_jumps,
                    m.severe_jumps,
                    m.extreme_jumps,
                    m.worst_jump,
                    (m.sum_error2 / m.queries as f64).sqrt(),
                    m.sum_lead / m.moving.max(1) as f64,
                    m.predicted as f64 / m.moving.max(1) as f64
                );
                large += m.large_jumps;
                severe += m.severe_jumps;
                extreme += m.extreme_jumps;
                worst_jump = worst_jump.max(m.worst_jump);
                lead += m.sum_lead / m.moving.max(1) as f64;
                coverage += m.predicted as f64 / m.moving.max(1) as f64;
                error2 += m.sum_error2 / m.queries as f64;
            }
        }
    }
    // Score all cases, including the hard 12 ms / 12,000 px/s pulses at
    // 120 Hz. Their largest errors remain unresolved; do not silently drop
    // them or claim the aggregate improvement fixes that worst case.
    assert!(large <= 810, "large burst errors: {large}");
    // Preserve the preceding revision's severe tail while reducing lesser
    // flicker. The unresolved worst case remains included and bounded.
    assert!(severe <= 325, "severe burst errors: {severe}");
    assert!(extreme <= 258, "extreme burst errors: {extreme}");
    assert!(worst_jump <= 317., "worst burst error: {worst_jump}");
    assert!((error2 / 72.).sqrt() <= 15.9, "burst accuracy");
    assert!(lead / 72. >= 4.75, "retain burst lookahead");
    assert!(coverage / 72. >= 0.68, "retain burst coverage");
}

#[test]
#[ignore = "offline parameter sweep; reports baseline and candidate metrics"]
fn real_error_family_sweep() {
    println!(
        "motion,width,speed,input_hz,display_hz,frequency,seed,algorithm,queries,wrong_jumps,error_rms,worst_error,mean_lead_ms,predicted_fraction,large_jumps,severe_jumps,extreme_jumps,worst_jump"
    );
    for motion in MOTIONS {
        for width in [1280., 3840., 7680.] {
            for speed in [500., 2000., 6000.] {
                for case in 0..6 {
                    let input = [120., 240., 480.][case % 3];
                    let display = [60., 119.88, 144.][(case + 1) % 3];
                    let frequency = [1.7, 4.3, 8.1][(case + 2) % 3];
                    for algorithm in [PredictionAlgorithm::Trajectory] {
                        let m = run(
                            motion,
                            width,
                            speed,
                            input,
                            display,
                            frequency,
                            case as u32,
                            algorithm,
                        );
                        println!(
                            "{motion:?},{width},{speed},{input},{display},{frequency},{case},{algorithm:?},{},{},{},{},{},{},{},{},{},{}",
                            m.queries,
                            m.wrong_jumps,
                            (m.sum_error2 / m.queries as f64).sqrt(),
                            m.worst_error,
                            m.sum_lead / m.moving.max(1) as f64,
                            m.predicted as f64 / m.moving.max(1) as f64,
                            m.large_jumps,
                            m.severe_jumps,
                            m.extreme_jumps,
                            m.worst_jump
                        );
                    }
                }
            }
        }
    }
}

// Grade entire synthetic families, not selected trace coordinates. Ceilings
// preserve the retained trajectory predictor; accuracy and useful lookahead
// are constrained too, so suppressing prediction cannot pass.
fn family_regression(
    motion: Motion,
    jumps: usize,
    error_rms: f64,
    mean_lead: f64,
    severe_limits: [usize; 2],
) {
    let mut total_jumps = 0;
    let mut severe = [0; 2];
    let mut squared_error = 0.;
    let mut lead = 0.;
    let mut coverage = 0.;
    let mut cases = 0.;
    for width in [1280., 3840., 7680.] {
        for speed in [500., 2000., 6000.] {
            for case in 0..6 {
                let m = run(
                    motion,
                    width,
                    speed,
                    [120., 240., 480.][case % 3],
                    [60., 119.88, 144.][(case + 1) % 3],
                    [1.7, 4.3, 8.1][(case + 2) % 3],
                    case as u32,
                    PredictionAlgorithm::Trajectory,
                );
                total_jumps += m.wrong_jumps;
                severe[0] += m.severe_jumps;
                severe[1] += m.extreme_jumps;
                squared_error += m.sum_error2 / m.queries as f64;
                lead += m.sum_lead / m.moving as f64;
                coverage += m.predicted as f64 / m.moving as f64;
                cases += 1.;
            }
        }
    }
    eprintln!(
        "{motion:?}: jumps={total_jumps}, RMS={}, lead={}, coverage={}",
        (squared_error / cases).sqrt(),
        lead / cases,
        coverage / cases
    );
    assert!(total_jumps <= jumps, "{motion:?}: {total_jumps} > {jumps}");
    // Preserve the retained predictor's severe and extreme counts.
    assert!(
        severe[0] <= severe_limits[0] && severe[1] <= severe_limits[1],
        "{motion:?}: severe {severe:?} > {severe_limits:?}"
    );
    assert!(
        (squared_error / cases).sqrt() <= error_rms,
        "{motion:?}: forecast accuracy"
    );
    assert!(
        lead / cases >= mean_lead,
        "{motion:?}: retain useful lookahead"
    );
    assert!(
        coverage / cases >= 0.94,
        "{motion:?}: retain prediction coverage"
    );
}
#[test]
fn variable_speed_over_and_undershoot_regression() {
    family_regression(Motion::SpeedWave, 1320, 17.8, 18., [96, 69]);
}
#[test]
fn varying_acceleration_and_curvature_regression() {
    family_regression(Motion::BreathingCurve, 1700, 21., 20., [205, 138]);
}
#[test]
fn changing_turn_regression() {
    family_regression(Motion::ChangingTurn, 1180, 17.5, 22.5, [105, 67]);
}
#[test]
fn accelerating_startup_regression() {
    family_regression(Motion::Startup, 85, 3.8, 25., [0, 0]);
}
#[test]
fn braking_and_corner_restart_regression() {
    family_regression(Motion::Staircase, 900, 17.4, 20., [219, 186]);
}
#[test]
fn stop_and_reverse_regression() {
    family_regression(Motion::Reversal, 920, 17.3, 20., [210, 183]);
}

// Additional combinations were set aside from tuning: different rates, speeds,
// spatial scales, turn frequencies, sensor noise, timestamp precision, real
// report irregularity, dropped reports, batching, and 12–80 ms braking ramps.
#[test]
fn expanded_motion_and_timing_holdout() {
    println!(
        "holdout,motion,algorithm,queries,wrong_jumps,error_rms,worst_error,mean_lead_ms,predicted_fraction"
    );
    for (shape, motion) in MOTIONS.into_iter().enumerate() {
        let mut total_jumps = 0;
        let mut error2 = 0.;
        let mut lead = 0.;
        let mut coverage = 0.;
        for case in 0..24 {
            let speed = [800., 3500., 12_000., 16_000.][case % 4];
            let input = Perturbations {
                phase: 0.19 + case as f64 * 0.73,
                brake: [0.012, 0.04, 0.08][case % 3],
                quantum: [0, 500, 1000, 2000][(case / 3) % 4],
                jitter: [0., 0.15, 0.35][(case / 4) % 3],
                noise: [0., 0.25, 1.2][case % 3],
                missing: case % 5 == 0,
            };
            for algorithm in [PredictionAlgorithm::Trajectory] {
                let m = run_with(
                    motion,
                    [1920., 2560., 5120., 15_360.][(case / 3) % 4],
                    speed,
                    [90., 180., 360., 1000.][(case / 2) % 4],
                    [75., 165., 240.][case % 3],
                    [2.9, 6.7, 11.3][(case / 4) % 3],
                    case as u32 + 17,
                    algorithm,
                    input,
                );
                println!(
                    "{case},{motion:?},{algorithm:?},{},{},{},{},{},{}",
                    m.queries,
                    m.wrong_jumps,
                    (m.sum_error2 / m.queries as f64).sqrt(),
                    m.worst_error,
                    m.sum_lead / m.moving as f64,
                    m.predicted as f64 / m.moving as f64
                );
                if algorithm == PredictionAlgorithm::Trajectory {
                    total_jumps += m.wrong_jumps;
                    error2 += m.sum_error2 / m.queries as f64;
                    lead += m.sum_lead / m.moving as f64;
                    coverage += m.predicted as f64 / m.moving as f64;
                }
                assert!(
                    m.worst_error < speed * 0.064 * 3. + 8.,
                    "finite, bounded error: {motion:?} {case} {m:?}"
                );
            }
        }
        // Fast motion with rapidly changing acceleration at 90 Hz may have no
        // trustworthy future point. Compare family coverage/lead rather than
        // demanding a forecast on such intrinsically ambiguous individual frames.
        assert!(
            total_jumps <= [1233, 1582, 1447, 314, 550, 572][shape],
            "{motion:?}: holdout flicker {total_jumps}"
        );
        assert!(
            (error2 / 24.).sqrt() <= [38., 44.2, 26.1, 7.39, 22.4, 23.5][shape],
            "{motion:?}: holdout accuracy"
        );
        assert!(
            lead / 24. >= [7.9, 9.1, 12.4, 14.6, 12.1, 12.1][shape],
            "{motion:?}: holdout useful lookahead"
        );
        assert!(
            coverage / 24. >= [0.88, 0.91, 0.93, 0.96, 0.93, 0.93][shape],
            "{motion:?}: holdout coverage"
        );
    }
}
