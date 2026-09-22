//! Causal evidence that smooth motion has persisted beyond the local fit.
use layer_core::StrokePoint;

// Compared with 200 ms on the Wacom recording; 100 ms retains more useful
// evidence through ordinary drawing changes without weakening stop detection.
pub(super) const HISTORY_MICROS: u32 = 100_000;

#[derive(Clone, Copy, Debug)]
pub(super) struct DrawingState {
    pub smooth: f64,
    pub reach: f64,
    pub interrupted: bool,
    pub stop_in_micros: f64,
    pub speed: f64,
}

impl Default for DrawingState {
    fn default() -> Self {
        Self {
            smooth: 0.,
            reach: 0.,
            interrupted: true,
            stop_in_micros: 0.,
            speed: 0.,
        }
    }
}

fn length(v: [f64; 2]) -> f64 {
    v[0].hypot(v[1])
}
fn angle(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1])
}
fn ramp(x: f64) -> f64 {
    let x = x.clamp(0., 1.);
    x * x * (3. - 2. * x)
}

impl DrawingState {
    pub fn measure(real: &[StrokePoint], transform: [f32; 6]) -> Self {
        Self::with_history(real, transform, HISTORY_MICROS)
    }

    fn with_history(real: &[StrokePoint], transform: [f32; 6], history: u32) -> Self {
        let Some(anchor) = real.last() else {
            return Self::default();
        };
        let now = anchor.elapsed_micros;
        if now < history {
            return Self::default();
        }
        let at = |time: u32| -> Option<[f64; 2]> {
            let i = real.partition_point(|p| p.elapsed_micros <= time);
            let a = real.get(i.checked_sub(1)?)?;
            let xy = if a.elapsed_micros == time {
                [f64::from(a.position.x), f64::from(a.position.y)]
            } else {
                let p = real.get(i)?;
                if p.elapsed_micros - a.elapsed_micros > 16_000 {
                    return None;
                }
                let t = f64::from(time - a.elapsed_micros)
                    / f64::from(p.elapsed_micros - a.elapsed_micros);
                [
                    f64::from(a.position.x) + t * f64::from(p.position.x - a.position.x),
                    f64::from(a.position.y) + t * f64::from(p.position.y - a.position.y),
                ]
            };
            Some([
                f64::from(transform[0]) * xy[0] + f64::from(transform[2]) * xy[1],
                f64::from(transform[1]) * xy[0] + f64::from(transform[3]) * xy[1],
            ])
        };
        let mut points = [[0.; 2]; 27];
        let n = (history / 8_000) as usize + 1;
        for (i, p) in points[..n].iter_mut().enumerate() {
            let Some(x) = at(now - history + i as u32 * history / (n - 1) as u32) else {
                return Self::default();
            };
            *p = x;
        }
        let mut chords = [[0.; 2]; 26];
        for i in 0..n - 2 {
            chords[i] = [0, 1].map(|a| points[i + 2][a] - points[i][a]);
        }
        let chords = &chords[..n - 2];
        let mut risk: f64 = 0.;
        let mut turning = 0.;
        let mut previous = 0.;
        for i in 1..chords.len() {
            if length(chords[i]).min(length(chords[i - 1])) < 1. {
                return Self::default();
            }
            let turn = angle(chords[i - 1], chords[i]);
            risk += turn * turn;
            turning += turn * turn;
            if i > 1 {
                risk += 4. * (turn - previous).powi(2);
            }
            previous = turn;
        }
        risk = (risk / (chords.len() - 1) as f64).sqrt();
        let recent = [0, 1].map(|a| points[n - 1][a] - points[n - 2][a]);
        let before = [0, 1].map(|a| points[n - 2][a] - points[n - 3][a]);
        let speed = length(recent) * 1e6 * (n - 1) as f64 / f64::from(history);
        let deceleration = length(recent) / length(before).max(1e-6);
        let older = [0, 1].map(|a| points[n - 3][a] - points[n - 4][a]);
        let older_ratio = length(before) / length(older).max(1e-6);
        let interrupt = (1. - ramp((angle(before, recent).abs() - 0.10) / 0.12))
            * ramp((deceleration - 0.65) / 0.25);
        let mut stop_in_micros = if deceleration < 0.95 && older_ratio < 0.95 {
            length(recent) / (length(older) - length(recent)).max(1e-6) * 2. * f64::from(history)
                / (n - 1) as f64
        } else {
            f64::INFINITY
        };
        let l = chords.len();
        let speeds = [
            length(chords[l - 5]),
            length(chords[l - 3]),
            length(chords[l - 1]),
        ];
        let dt = 2. * f64::from(history) / (n - 1) as f64;
        if speeds[1] < 0.98 * speeds[0] && speeds[2] < 0.98 * speeds[1] {
            let a = (3. * speeds[2] - 4. * speeds[1] + speeds[0]) / (2. * dt);
            let j = (speeds[2] - 2. * speeds[1] + speeds[0]) / (dt * dt);
            if j < 0. {
                let velocity = (speeds[2] + a * dt * 0.5 + j * dt * dt * 0.125).max(0.);
                let acceleration = a + j * dt * 0.5;
                let stop = 2. * velocity
                    / ((acceleration * acceleration - 2. * j * velocity).sqrt() - acceleration)
                        .max(1e-12);
                stop_in_micros = stop_in_micros.min(stop);
            }
        }
        let smooth = (1. - ramp((risk - 0.04) / 0.10)) * ramp((speed - 1100.) / 900.) * interrupt;
        let curvature = (turning / (chords.len() - 1) as f64).sqrt();
        let reach = smooth * (1. - ramp((curvature - 0.04) / 0.04));
        Self {
            speed,
            stop_in_micros,
            reach,
            interrupted: deceleration < 0.65 || angle(before, recent).abs() > 0.22,
            smooth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::test_support::{IDENTITY, point};

    #[test]
    fn both_histories_use_the_full_time_window_at_every_report_rate() {
        for history in [100_000, 200_000] {
            for period in [1_000, 4_000, 8_000] {
                let straight: Vec<_> = (0..=400_000 / period)
                    .map(|i| point(i as f32 * period as f32 * 0.0024, 0., i * period))
                    .collect();
                assert!(DrawingState::with_history(&straight, IDENTITY, history).smooth > 0.99);
                let mut erratic = straight.clone();
                // Identical latest 48 ms, but oscillating earlier in the history.
                for p in &mut erratic {
                    if p.elapsed_micros > 400_000 - history && p.elapsed_micros < 352_000 {
                        p.position.y = 20. * (p.elapsed_micros as f32 * 0.0003).sin();
                    }
                }
                assert!(
                    DrawingState::with_history(&erratic, IDENTITY, history).smooth < 0.2,
                    "{history} / {period}"
                );
            }
        }
    }

    #[test]
    fn long_smooth_history_never_delays_stop_or_corner_detection() {
        for history in [100_000, 200_000] {
            for corner in [false, true] {
                let mut points: Vec<_> = (0..=100)
                    .map(|i| point(i as f32 * 9.6, 0., i * 4000))
                    .collect();
                assert!(DrawingState::with_history(&points, IDENTITY, history).smooth > 0.99);
                for i in 1..=3 {
                    points.push(point(
                        960.,
                        if corner { i as f32 * 9.6 } else { 0. },
                        400_000 + i * 4000,
                    ));
                }
                assert_eq!(
                    DrawingState::with_history(&points, IDENTITY, history).smooth,
                    0.,
                    "history {history}, corner {corner}"
                );
            }
        }
    }

    #[test]
    fn history_tracks_smooth_arcs_and_is_invariant_to_zoom() {
        for history in [100_000, 200_000] {
            let mut reference: Option<f64> = None;
            for zoom in [0.25, 1., 4.] {
                let points: Vec<_> = (0..=100)
                    .map(|i| {
                        let angle = i as f32 * 0.004 * 2.;
                        point(
                            1200. * angle.sin() / zoom,
                            1200. * (1. - angle.cos()) / zoom,
                            i * 4000,
                        )
                    })
                    .collect();
                let score =
                    DrawingState::with_history(&points, [zoom, 0., 0., zoom, 300., -200.], history)
                        .smooth;
                assert!(score > 0.95, "{history}: {score}");
                if let Some(before) = reference {
                    assert!((score - before).abs() < 1e-6)
                }
                reference = Some(score);
                assert_eq!(
                    DrawingState::with_history(&points[..10], IDENTITY, history).smooth,
                    0.
                );
            }
        }
    }
}
