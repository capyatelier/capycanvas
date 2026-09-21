use super::*;

pub(super) const IDENTITY: [f32; 6] = [1., 0., 0., 1., 0., 0.];

pub(super) fn point(x: f32, y: f32, time: u32) -> StrokePoint {
    StrokePoint {
        position: Point { x, y },
        elapsed_micros: time,
        pressure: 0.6,
        tilt: [0.2, -0.1],
        twist: 0.7,
    }
}

pub(super) fn config() -> InstantFeedbackConfig {
    InstantFeedbackConfig {
        prediction_algorithm: PredictionAlgorithm::Trajectory,
        prediction_horizon_micros: 16_000,
        use_platform_prediction: false,
        ..Default::default()
    }
}

/// Deliver only samples available at each display frame. Generators retain
/// their own input clocks, noise and ground truth; scoring cannot feed the fit.
pub(super) fn run_frames(
    display_hz: f64,
    duration: f64,
    zoom: f32,
    config: InstantFeedbackConfig,
    observe_pressure: bool,
    mut sample: impl FnMut(usize, f64) -> Option<StrokePoint>,
    mut score: impl FnMut(f64, TipEstimate, StrokePoint),
) {
    let mut state = PredictionState::default();
    let mut real = Vec::new();
    let transform = [zoom, 0., 0., zoom, 0., 0.];
    for frame in 1..=(display_hz * duration) as usize {
        let now = frame as f64 / display_hz;
        while let Some(point) = sample(real.len(), now) {
            if observe_pressure {
                state.observe(PenEvent {
                    device_id: 1,
                    sequence: real.len() as u64,
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
        }
        let Some(&latest) = real.last() else {
            continue;
        };
        let tip = state
            .estimate_for(
                &real,
                &[],
                (now * 1e6) as u32 + config.prediction_horizon_micros,
                (now * 1e6) as u32,
                transform,
                config,
            )
            .unwrap();
        for p in state.engine_intermediates().chain([tip.point]) {
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
        score(now, tip, latest);
    }
}
