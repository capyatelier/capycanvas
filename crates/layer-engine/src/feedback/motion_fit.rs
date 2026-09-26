//! Constant turn rate and tangential acceleration, fitted to measured positions.
//!
//! Like Android's prediction pipeline, output is a timed trajectory whose length
//! is limited by confidence. This is a portable analytic model, not Android's
//! trained TFLite model. Straight motion, arcs and braking are solutions of the
//! same equations: velocity = max(v + a*t, 0) * direction(theta + omega*t).
//! Position measurements are fitted directly, avoiding noisy finite differences.

use super::transform_vector;
use layer_core::{Point, StrokePoint};

const HISTORY_MICROS: u32 = 128_000;
const MAX_SAMPLES: usize = 64;
// Balance steady-motion variance with tracking changing curvature. Selected
// against independently clocked straight, circular and spiral test sweeps.
const FIT_TIME_CONSTANT_MICROS: f64 = 48_000.;
// Physical pixel precision floor and conservative startup noise estimate.
// Once there is enough history, estimate measurement noise from time-aware
// cubic interpolation residuals, with a separate uncertainty budget for clock quantization.
const MIN_VARIANCE: f64 = 0.03 * 0.03;
const INITIAL_VARIANCE: f64 = 0.3 * 0.3;
const FORECAST_ERROR_PX: f64 = 2.;
pub(super) const STEP_MICROS: u32 = 2_000;

type Vector = [f64; 2];
type Matrix = [[f64; 6]; 6];

fn rotate(v: Vector, angle: f64) -> Vector {
    let (s, c) = angle.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

// Integrals of exp(i*w*s), s*exp(i*w*s), s²*exp(i*w*s) from 0 to t.
// Series around zero avoid cancellation and make straight motion continuous.
fn moments(t: f64, w: f64) -> [Vector; 3] {
    let z = w * t;
    if z.abs() < 0.1 {
        let z2 = z * z;
        [
            [
                t * (1. - z2 / 6. + z2 * z2 / 120.),
                t * z * (0.5 - z2 / 24. + z2 * z2 / 720.),
            ],
            [
                t * t * (0.5 - z2 / 8. + z2 * z2 / 144.),
                t * t * z * (1. / 3. - z2 / 30. + z2 * z2 / 840.),
            ],
            [
                t.powi(3) * (1. / 3. - z2 / 10. + z2 * z2 / 168.),
                t.powi(3) * z * (0.25 - z2 / 36. + z2 * z2 / 960.),
            ],
        ]
    } else {
        let (s, c) = z.sin_cos();
        [
            [s / w, (1. - c) / w],
            [(z * s + c - 1.) / w.powi(2), (s - z * c) / w.powi(2)],
            [
                ((z * z - 2.) * s + 2. * z * c) / w.powi(3),
                ((2. - z * z) * c + 2. * z * s - 2.) / w.powi(3),
            ],
        ]
    }
}

// Parameters [x0, y0, v0*T, heading0, acceleration*T², omega*T].
// Normalized time improves conditioning across different input report rates.
fn evaluate(p: [f64; 6], t: f64) -> (Vector, [Vector; 6]) {
    let stop = if p[4] < 0. {
        -p[2] / p[4]
    } else {
        f64::INFINITY
    };
    let t = t.min(stop.max(0.));
    let [i0, i1, i2] = moments(t, p[5]);
    let local = [p[2] * i0[0] + p[4] * i1[0], p[2] * i0[1] + p[4] * i1[1]];
    let displacement = rotate(local, p[3]);
    let angular = [p[2] * i1[0] + p[4] * i2[0], p[2] * i1[1] + p[4] * i2[1]];
    (
        [p[0] + displacement[0], p[1] + displacement[1]],
        [
            [1., 0.],
            [0., 1.],
            rotate(i0, p[3]),
            [-displacement[1], displacement[0]],
            rotate(i1, p[3]),
            rotate([-angular[1], angular[0]], p[3]),
        ],
    )
}

fn solve(mut a: Matrix, mut b: [f64; 6]) -> Option<[f64; 6]> {
    for col in 0..6 {
        let pivot = (col..6).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        a.swap(col, pivot);
        b.swap(col, pivot);
        if !a[col][col].is_finite() || a[col][col].abs() < 1e-12 {
            return None;
        }
        let divisor = a[col][col];
        for j in col..6 {
            a[col][j] /= divisor;
        }
        b[col] /= divisor;
        for i in 0..6 {
            if i == col {
                continue;
            }
            let scale = a[i][col];
            for j in col..6 {
                a[i][j] -= scale * a[col][j];
            }
            b[i] -= scale * b[col];
        }
    }
    b.iter().all(|v| v.is_finite()).then_some(b)
}

#[derive(Clone, Copy, Default)]
pub(super) struct Sample {
    pub time: u32,
    pub position: Vector,
}

pub(super) fn observations(
    real: &[StrokePoint],
    transform: [f32; 6],
) -> Option<([Sample; MAX_SAMPLES], usize)> {
    let anchor = *real.last()?;
    let cutoff = anchor.elapsed_micros.saturating_sub(HISTORY_MICROS * 2);
    let start = real
        .partition_point(|p| p.elapsed_micros < cutoff)
        .max(real.len().saturating_sub(MAX_SAMPLES));
    let mut samples = [Sample::default(); MAX_SAMPLES];
    let mut n = 0;
    for point in &real[start..] {
        let surface = transform_vector(
            transform,
            Point {
                x: point.position.x - anchor.position.x,
                y: point.position.y - anchor.position.y,
            },
        );
        if !surface.x.is_finite() || !surface.y.is_finite() {
            return None;
        }
        if n > 0 && point.elapsed_micros < samples[n - 1].time {
            return None;
        }
        if n > 0 && point.elapsed_micros == samples[n - 1].time {
            n -= 1;
        }
        samples[n] = Sample {
            time: point.elapsed_micros,
            position: [f64::from(surface.x), f64::from(surface.y)],
        };
        n += 1;
    }
    Some((samples, n))
}

// Estimate sub-tick report times, without modifying recorded stroke samples.
// Regress the report clock on sample index, then shrink quantization residuals
// toward that cadence. The uniform-clock variance q²/12 divided by the observed
// residual variance is a Wiener gain: irregular reports and missing samples
// weaken the cadence prior instead of being forced onto a uniform grid.
// Each adjustment stays inside a centered half-tick cell. Rounding versus
// truncation changes an unknown common phase, not the reconstructed intervals.
pub(super) fn reconstruct_times(samples: &mut [Sample], quantum: u32) {
    if quantum == 0 || samples.len() < 4 {
        return;
    }
    let n = samples.len() as f64;
    let center = (n - 1.) * 0.5;
    let mean = samples.iter().map(|s| f64::from(s.time)).sum::<f64>() / n;
    let numerator = samples
        .iter()
        .enumerate()
        .map(|(i, s)| (i as f64 - center) * (f64::from(s.time) - mean))
        .sum::<f64>();
    let denominator = (0..samples.len())
        .map(|i| (i as f64 - center).powi(2))
        .sum::<f64>();
    let period = numerator / denominator;
    let residual_variance = samples
        .iter()
        .enumerate()
        .map(|(i, s)| (f64::from(s.time) - mean - (i as f64 - center) * period).powi(2))
        .sum::<f64>()
        / (n - 2.);
    let gain = (f64::from(quantum).powi(2) / 12. / residual_variance).min(1.);
    let radius = f64::from(quantum) * 0.5;
    let mut previous = 0;
    for (i, sample) in samples.iter_mut().enumerate() {
        let estimate = mean + (i as f64 - center) * period;
        let correction = (estimate - f64::from(sample.time)).clamp(-radius, radius) * gain;
        let time = (f64::from(sample.time) + correction).round().max(0.) as u32;
        sample.time = if i == 0 { time } else { time.max(previous + 1) };
        previous = sample.time;
    }
}

fn measurement_variance(samples: &[Sample], timestamp_resolution: u32, median: bool) -> f64 {
    let mut errors = [0.; MAX_SAMPLES];
    let mut n = 0;
    for window in samples.windows(5) {
        let center = window[2];
        let mut residual = center.position;
        let mut noise_gain = 1.;
        for i in [0, 1, 3, 4] {
            let mut weight = 1.;
            for j in [0, 1, 3, 4] {
                if i != j {
                    weight *= (f64::from(center.time) - f64::from(window[j].time))
                        / (f64::from(window[i].time) - f64::from(window[j].time));
                }
            }
            for axis in 0..2 {
                residual[axis] -= weight * window[i].position[axis];
            }
            noise_gain += weight * weight;
        }
        errors[n] = (residual[0].powi(2) + residual[1].powi(2)) / noise_gain;
        n += 1;
    }
    // Quantization is correlated timing noise, which a spatial high-pass
    // estimate can miss. Propagate the host clock's uniform rounding variance
    // through measured speed and ADD it to the residual noise estimate. Using
    // max(residual, clock) lets one source consume the other's uncertainty:
    // noisy, millisecond-quantized input then falsely rejects stable long fits.
    // The high-pass estimate may include some clock noise, so this is deliberately
    // conservative, not an assertion that the two estimates are independent.
    // It also covers truncated clocks: a constant half-tick offset cancels.
    let clock_variance = samples.first().zip(samples.last()).map_or(0., |(a, b)| {
        let span_ms = f64::from(b.time - a.time) / 1000.;
        let length: f64 = samples
            .windows(2)
            .map(|pair| {
                (pair[1].position[0] - pair[0].position[0])
                    .hypot(pair[1].position[1] - pair[0].position[1])
            })
            .sum();
        if span_ms > 0. {
            (length / span_ms * f64::from(timestamp_resolution) / 1000.).powi(2) / 12.
        } else {
            0.
        }
    });
    if n < 3 {
        return INITIAL_VARIANCE + clock_variance;
    }
    if median {
        // Preserve the legacy three-sample braking detector's robust estimate:
        // isolated motion changes must not inflate its local derivative bound.
        errors[..n].sort_unstable_by(f64::total_cmp);
        return (errors[n / 2] / (2. * std::f64::consts::LN_2)).max(MIN_VARIANCE) + clock_variance;
    }
    // Use the second moment: clock/sensor noise need not be Gaussian or
    // independent in x/y. A chi-square median can severely underestimate
    // correlated/quantized noise and force an otherwise good long fit to fail.
    (errors[..n].iter().sum::<f64>() / (2. * n as f64)).max(MIN_VARIANCE) + clock_variance
}

// A burst of changing motion also excites the cubic high-pass noise estimator.
// Keeping that inflated estimate after the burst hides a subsequent acceleration.
// A short window alone is too variable: only lower the long-window estimate to
// an UPPER estimate of recent variance, retaining the original clock uncertainty.
// Wilson–Hilferty approximates the chi-square lower tail (z = 3.09, p ≈ .001).
// Overlapping residuals and non-Gaussian sensor noise make this a conservative
// engineering reference, not a calibrated .999 coverage guarantee.
fn tracking_variance(samples: &[Sample], timestamp_resolution: u32) -> f64 {
    let mut variance = measurement_variance(samples, timestamp_resolution, false);
    let mut count = samples.len() / 2;
    while count >= 8 {
        let local = measurement_variance(
            &samples[samples.len() - count..],
            timestamp_resolution,
            false,
        );
        let degrees = 2. * (count - 4) as f64;
        let lower = (1. - 2. / (9. * degrees) - 3.09 * (2. / (9. * degrees)).sqrt()).powi(3);
        if lower > 0. {
            variance = variance.min(local / lower);
        }
        count /= 2;
    }
    variance
}

/// Local stopping envelope for existing predictors. Chord velocities live at
/// interval midpoints; extrapolate speed to the newest observation before
/// integrating deceleration. Only statistically resolved braking sets a bound.
pub(super) fn braking_distance(
    real: &[StrokePoint],
    transform: [f32; 6],
    timestamp_resolution: u32,
    horizon: u32,
) -> Option<f32> {
    let (samples, n) = observations(real, transform)?;
    if n < 3 {
        return None;
    }
    let [a, b, c] = samples[n - 3..n].try_into().ok()?;
    let dt1 = f64::from(b.time - a.time) / 1000.;
    let dt2 = f64::from(c.time - b.time) / 1000.;
    let u = [b.position[0] - a.position[0], b.position[1] - a.position[1]];
    let v = [c.position[0] - b.position[0], c.position[1] - b.position[1]];
    let mid = (dt1 + dt2) * 0.5;
    let turn = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]) / mid;
    let speed = |delta: Vector, dt: f64| {
        let angle = turn * dt * 0.5;
        let correction = if angle.abs() < 1e-6 {
            1.
        } else {
            angle / angle.sin()
        };
        delta[0].hypot(delta[1]) / dt * correction
    };
    let before = speed(u, dt1);
    let current = speed(v, dt2);
    let acceleration = (current - before) / mid;
    let variance = measurement_variance(&samples[..n], timestamp_resolution, true);
    let sigma =
        (2. * variance * (1. / dt1.powi(2) + 1. / dt2.powi(2) + 1. / (dt1 * dt2))).sqrt() / mid;
    if acceleration + 2. * sigma >= 0. {
        return None;
    }
    let velocity = (current + acceleration * dt2 * 0.5).max(0.);
    let duration = (f64::from(horizon) / 1000.).min(velocity / -acceleration);
    Some(
        (velocity * duration + 0.5 * acceleration * duration * duration + 2. * variance.sqrt())
            as f32,
    )
}

fn equations(samples: &[Sample], p: [f64; 6], span: f64) -> (f64, Matrix, [f64; 6]) {
    let mut cost = 0.;
    let mut normal = [[0.; 6]; 6];
    let mut rhs = [0.; 6];
    for s in samples {
        // Older observations constrain steady motion, but carry more model
        // error when curvature/acceleration evolves. Exponential weighting
        // avoids an abrupt change in velocity when a rectangular fit window
        // switches length. This filters model estimation, never recorded input.
        let weight =
            (-f64::from(samples.last().unwrap().time - s.time) / FIT_TIME_CONSTANT_MICROS).exp();
        let t = f64::from(s.time - samples[0].time) / span;
        let (position, jacobian) = evaluate(p, t);
        let residual = [s.position[0] - position[0], s.position[1] - position[1]];
        cost += weight * (residual[0].powi(2) + residual[1].powi(2));
        for i in 0..6 {
            rhs[i] += weight * (jacobian[i][0] * residual[0] + jacobian[i][1] * residual[1]);
            for j in 0..6 {
                normal[i][j] +=
                    weight * (jacobian[i][0] * jacobian[j][0] + jacobian[i][1] * jacobian[j][1]);
            }
        }
    }
    (cost, normal, rhs)
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct MotionFit {
    parameters: [f64; 6],
    covariance: Matrix,
    measurement_variance: f64,
    stopping_distance: f64,
    span_ms: f64,
    anchor: StrokePoint,
    inverse: [f64; 4],
}

impl MotionFit {
    pub(super) fn fit(
        real: &[StrokePoint],
        transform: [f32; 6],
        timestamp_resolution: u32,
    ) -> Option<Self> {
        let anchor = *real.last()?;
        let [a, b, c, d, _, _] = transform.map(f64::from);
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant.abs() < f64::EPSILON {
            return None;
        }
        let (mut samples, n) = observations(real, transform)?;
        // Cadence is a prior, not a new measurement of clock accuracy. Retain
        // the original sensor/clock uncertainty after reconstructing times.
        let variance = tracking_variance(&samples[..n], timestamp_resolution);
        reconstruct_times(&mut samples[..n], timestamp_resolution);
        // Prefer longer, better-conditioned fits. On a change in motion, shorten
        // history until one model explains the measurements within sensor noise.
        let cutoff = anchor.elapsed_micros.saturating_sub(HISTORY_MICROS);
        let mut first = samples[..n].partition_point(|s| s.time < cutoff);
        let mut preferred: Option<Self> = None;
        let mut stopping_distance = f64::INFINITY;
        let mut resolved_braking = true;
        while n - first >= 4 {
            if let Some(mut model) = Self::fit_window(&samples[first..n], variance) {
                model.translate_time(
                    (f64::from(anchor.elapsed_micros) - f64::from(samples[n - 1].time))
                        / (model.span_ms * 1000.),
                );
                model.anchor = anchor;
                model.inverse = [
                    d / determinant,
                    -b / determinant,
                    -c / determinant,
                    a / determinant,
                ];
                // Test recent positions directly. Comparing covariance matrices
                // of nonlinear motion parameters can be indefinite and falsely
                // select a short, poorly conditioned fit.
                if preferred
                    .as_ref()
                    .is_none_or(|old| !old.consistent_with(&model, &samples[first..n], variance))
                {
                    stopping_distance = f64::INFINITY;
                    resolved_braking = true;
                    preferred = Some(model.clone());
                }
                // Intersection across nested windows, not a minimum over
                // independent braking alarms. An isolated noisy short fit must
                // not impose a stop against the evidence at other time scales.
                resolved_braking &= model.braking();
                if resolved_braking {
                    stopping_distance =
                        stopping_distance.min(model.travel(model.duration(f64::INFINITY)));
                }
            }
            first += ((n - first) / 2).min((n - first).saturating_sub(4)).max(1);
        }
        if resolved_braking && let Some(model) = preferred.as_mut() {
            model.stopping_distance = stopping_distance;
        }
        preferred
    }

    /// Transport a briefly missing fit without forgetting covariance or the
    /// distance already spent from its stopping budget. The caller bounds age
    /// and corroborates continuation with fresh observations.
    pub(super) fn advanced_to(&self, anchor: StrokePoint, transform: [f32; 6]) -> Self {
        let mut next = self.clone();
        let elapsed = anchor
            .elapsed_micros
            .saturating_sub(self.anchor.elapsed_micros);
        next.stopping_distance =
            (self.stopping_distance - self.travel(f64::from(elapsed) / 1000.)).max(0.);
        next.translate_time(f64::from(elapsed) / (self.span_ms * 1000.));
        let delta = transform_vector(
            transform,
            Point {
                x: self.anchor.position.x - anchor.position.x,
                y: self.anchor.position.y - anchor.position.y,
            },
        );
        next.parameters[0] += f64::from(delta.x);
        next.parameters[1] += f64::from(delta.y);
        next.anchor = anchor;
        next
    }

    // Rebase both mean and covariance onto the unchanged public sample clock.
    // Discarding the reconstructed last-sample phase would copy clock noise
    // back into the endpoint, especially in the direction of fast travel.
    fn translate_time(&mut self, shift: f64) {
        let (position, derivative) = evaluate(self.parameters, shift);
        let mut jacobian = [[0.; 6]; 6];
        for i in 0..6 {
            jacobian[0][i] = derivative[i][0];
            jacobian[1][i] = derivative[i][1];
        }
        for i in 2..6 {
            jacobian[i][i] = 1.;
        }
        jacobian[2][4] = shift;
        jacobian[3][5] = shift;
        let mut covariance = [[0.; 6]; 6];
        let mut intermediate = [[0.; 6]; 6];
        for i in 0..6 {
            for j in 0..6 {
                for k in 0..6 {
                    intermediate[i][j] += jacobian[i][k] * self.covariance[k][j];
                }
            }
        }
        for i in 0..6 {
            for j in 0..6 {
                for k in 0..6 {
                    covariance[i][j] += intermediate[i][k] * jacobian[j][k];
                }
            }
        }
        self.parameters[0] = position[0];
        self.parameters[1] = position[1];
        self.parameters[2] += self.parameters[4] * shift;
        self.parameters[3] += self.parameters[5] * shift;
        self.covariance = covariance;
    }

    fn cost_on(&self, samples: &[Sample]) -> f64 {
        samples
            .iter()
            .map(|sample| {
                let t = 1.
                    + (f64::from(sample.time) - f64::from(self.anchor.elapsed_micros))
                        / (self.span_ms * 1000.);
                let p = evaluate(self.parameters, t).0;
                (p[0] - sample.position[0]).powi(2) + (p[1] - sample.position[1]).powi(2)
            })
            .sum()
    }

    fn consistent_with(&self, recent: &Self, samples: &[Sample], variance: f64) -> bool {
        // Compare models on measured positions, not weakly identifiable motion
        // parameters. The chi-square(6), 99.9% allowance accounts for testing
        // several shorter fits each frame. It is a local likelihood approximation,
        // not a calibrated guarantee for arbitrary correlated sensor noise.
        self.cost_on(samples) - recent.cost_on(samples) <= 22.46 * variance
    }

    fn fit_window(samples: &[Sample], variance: f64) -> Option<Self> {
        let last = samples.last()?;
        let span = f64::from(last.time - samples[0].time);
        if span < 1. {
            return None;
        }
        // Angular motion beyond Nyquist is unidentifiable from these reports:
        // a nonlinear optimizer can otherwise fit extra revolutions between
        // nearly coincident noisy samples with arbitrarily high speed.
        let max_gap = samples.windows(2).map(|s| s[1].time - s[0].time).max()?;
        let turn_limit = std::f64::consts::PI * span / f64::from(max_gap);
        let k = ((samples.len() - 1) / 3).max(1);
        let before = samples[samples.len() - 1 - k];
        let t1 = f64::from(samples[k].time - samples[0].time) / span;
        let t2 = f64::from(last.time - before.time) / span;
        let v1 = [
            (samples[k].position[0] - samples[0].position[0]) / t1,
            (samples[k].position[1] - samples[0].position[1]) / t1,
        ];
        let v2 = [
            (last.position[0] - before.position[0]) / t2,
            (last.position[1] - before.position[1]) / t2,
        ];
        let mid = 1. - (t1 + t2) * 0.5;
        let stride = ((samples.len() - 1) / 8).max(1);
        let mut previous: Option<Vector> = None;
        let mut first_midpoint = 0.;
        let mut last_midpoint = 0.;
        let mut turn_sum = 0.;
        let mut index = 0;
        while index + 1 < samples.len() {
            let end = (index + stride).min(samples.len() - 1);
            let chord = [
                samples[end].position[0] - samples[index].position[0],
                samples[end].position[1] - samples[index].position[1],
            ];
            let midpoint = (f64::from(samples[index].time) + f64::from(samples[end].time)) * 0.5;
            if let Some(old) = previous {
                turn_sum += (old[0] * chord[1] - old[1] * chord[0])
                    .atan2(old[0] * chord[0] + old[1] * chord[1]);
            } else {
                first_midpoint = midpoint;
            }
            last_midpoint = midpoint;
            previous = Some(chord);
            index = end;
        }
        let turn = turn_sum * span / (last_midpoint - first_midpoint);
        let acceleration = (v2[0].hypot(v2[1]) - v1[0].hypot(v1[1])) / mid;
        let mut p = [
            samples[0].position[0],
            samples[0].position[1],
            (v1[0].hypot(v1[1]) - acceleration * t1 * 0.5).max(0.),
            v1[1].atan2(v1[0]) - turn * t1 * 0.5,
            acceleration,
            turn,
        ];
        // A fit must explain the entire measured window with nonnegative
        // speed. Clipping motion to an earlier stop inside that window creates
        // a degenerate stationary branch with misleading derivative covariance.
        p[4] = p[4].max(-p[2]);
        p[5] = p[5].clamp(-turn_limit, turn_limit);
        let mut lambda = 1e-3;
        let (mut cost, mut normal, mut rhs) = equations(samples, p, span);
        // Levenberg-Marquardt with analytic derivatives; bounded work and no
        // allocation. Damping is numerical, not an artistic motion filter.
        for _ in 0..12 {
            if cost < 1e-12 {
                break;
            }
            let mut system = normal;
            for i in 0..6 {
                system[i][i] += lambda * (normal[i][i] + 1.);
            }
            let delta = solve(system, rhs)?;
            let mut next = p;
            for i in 0..6 {
                next[i] += delta[i];
            }
            next[2] = next[2].max(0.);
            next[4] = next[4].max(-next[2]);
            next[5] = next[5].clamp(-turn_limit, turn_limit);
            let (next_cost, next_normal, next_rhs) = equations(samples, next, span);
            if next_cost < cost {
                let improvement = cost - next_cost;
                p = next;
                cost = next_cost;
                normal = next_normal;
                rhs = next_rhs;
                lambda *= 0.25;
                if improvement < 1e-8 * (cost + 1.) {
                    break;
                }
            } else {
                lambda *= 10.;
            }
        }
        let span_ms = span / 1000.;
        // Covariance of the six fitted parameters. Recent-window likelihood
        // tests in fit() check changes without treating a single noisy endpoint
        // as a reason to discard the whole history.
        let mut inverse_normal = [[0.; 6]; 6];
        for i in 0..6 {
            let mut unit = [0.; 6];
            unit[i] = 1.;
            let column = solve(normal, unit)?;
            for j in 0..6 {
                inverse_normal[j][i] = column[j];
            }
        }
        // Recency weights do not change the sensor's measurement variance.
        // WLS covariance is R * K K^T (the sandwich estimator), not R times
        // the inverse weighted normal matrix. The latter double-counts the
        // weighting and spuriously loses evidence for a freshly measured brake.
        let mut covariance = [[0.; 6]; 6];
        let mut residual_degrees_of_freedom = 0.;
        let mut residual_squared_trace = 0.;
        for sample in samples {
            let weight = (-f64::from(last.time - sample.time) / FIT_TIME_CONSTANT_MICROS).exp();
            let t = f64::from(sample.time - samples[0].time) / span;
            let (_, jacobian) = evaluate(p, t);
            let mut influence = [[0.; 2]; 6];
            for i in 0..6 {
                for j in 0..6 {
                    for axis in 0..2 {
                        influence[i][axis] += weight * inverse_normal[i][j] * jacobian[j][axis];
                    }
                }
            }
            let leverage: f64 = (0..6)
                .map(|i| jacobian[i][0] * influence[i][0] + jacobian[i][1] * influence[i][1])
                .sum();
            residual_degrees_of_freedom += weight * (2. - leverage);
            residual_squared_trace += weight * weight * (2. - 2. * leverage);
            for i in 0..6 {
                for j in 0..6 {
                    covariance[i][j] += variance
                        * (influence[i][0] * influence[j][0] + influence[i][1] * influence[j][1]);
                }
            }
        }
        let residual_variance = cost / residual_degrees_of_freedom;
        // For the linearized fit, residual energy is a quadratic form with
        // B = W - W J (J' W J)^-1 J' W. Its noise mean is R tr(B), and its
        // variance is 2 R² tr(B²). Reuse the six-parameter sandwich covariance
        // to get tr(B²), without constructing a sample-sized residual matrix.
        let mut weighted_projection = [[0.; 6]; 6];
        for i in 0..6 {
            for j in 0..6 {
                for k in 0..6 {
                    weighted_projection[i][j] += covariance[i][k] * normal[k][j] / variance;
                }
            }
        }
        for i in 0..6 {
            for j in 0..6 {
                residual_squared_trace += weighted_projection[i][j] * weighted_projection[j][i];
            }
        }
        // Weighted Gaussian quadratic-form upper bound, with ||B|| <= 1 and
        // log(1000) rounded up. Longer histories provide more evidence of
        // model mismatch; a fixed 4 R allowance was too permissive there.
        // Keep the existing short-fit ceiling. Estimated noise, overlapping
        // samples and nonlinear fitting make this an engineering reference,
        // not a calibrated .999 probability guarantee for pen input.
        // Reference: https://arxiv.org/abs/1110.2842, Proposition 1.
        const LOG_TAIL: f64 = 6.908;
        let upper = (1.
            + (2. * (LOG_TAIL * residual_squared_trace.max(0.)).sqrt() + 2. * LOG_TAIL)
                / residual_degrees_of_freedom)
            .min(4.);
        if residual_degrees_of_freedom <= 0.
            || !residual_variance.is_finite()
            || !residual_squared_trace.is_finite()
            || residual_variance > variance * upper
        {
            return None;
        }
        Some(Self {
            parameters: p,
            covariance,
            measurement_variance: variance,
            stopping_distance: f64::INFINITY,
            span_ms,
            anchor: StrokePoint {
                position: Point { x: 0., y: 0. },
                elapsed_micros: 0,
                pressure: 0.,
                tilt: [0.; 2],
                twist: 0.,
            },
            inverse: [1., 0., 0., 1.],
        })
    }

    fn braking(&self) -> bool {
        // The normalized acceleration parameter and its standard deviation have
        // the same scale; a resolved deceleration overrides output memory.
        self.parameters[4] + 2. * self.covariance[4][4].max(0.).sqrt() < 0.
    }

    fn speed(&self) -> f64 {
        (self.parameters[2] + self.parameters[4]).max(0.) / self.span_ms
    }

    fn duration(&self, horizon_ms: f64) -> f64 {
        let acceleration = self.parameters[4] / self.span_ms.powi(2);
        if acceleration < 0. {
            horizon_ms.min(self.speed() / -acceleration)
        } else {
            horizon_ms
        }
    }

    fn travel(&self, horizon_ms: f64) -> f64 {
        let t = self.duration(horizon_ms);
        (self.speed() * t + 0.5 * self.parameters[4] * (t / self.span_ms).powi(2)).max(0.)
    }

    pub(super) fn point_at(&self, horizon_micros: u32) -> StrokePoint {
        let t = 1. + self.duration(f64::from(horizon_micros) / 1000.) / self.span_ms;
        let (end, _) = evaluate(self.parameters, t);
        let (start, _) = evaluate(self.parameters, 1.);
        let delta = [end[0] - start[0], end[1] - start[1]];
        StrokePoint {
            position: Point {
                x: self.anchor.position.x
                    + (self.inverse[0] * delta[0] + self.inverse[2] * delta[1]) as f32,
                y: self.anchor.position.y
                    + (self.inverse[1] * delta[0] + self.inverse[3] * delta[1]) as f32,
            },
            elapsed_micros: self.anchor.elapsed_micros.saturating_add(horizon_micros),
            ..self.anchor
        }
    }

    /// Forecast the fitted latent position. Re-anchoring every forecast to the
    /// newest raw position copies its timestamp/position error to the tip.
    pub(super) fn fitted_point_at(&self, horizon_micros: u32) -> StrokePoint {
        let elapsed_micros = self.anchor.elapsed_micros.saturating_add(horizon_micros);
        StrokePoint {
            position: self.fitted_position_at_time(elapsed_micros),
            elapsed_micros,
            ..self.anchor
        }
    }

    pub(super) fn fitted_position_at_time(&self, elapsed_micros: u32) -> Point {
        let offset = (f64::from(elapsed_micros) - f64::from(self.anchor.elapsed_micros)) / 1000.;
        let t = 1. + self.duration(offset) / self.span_ms;
        let (position, _) = evaluate(self.parameters, t);
        Point {
            x: self.anchor.position.x
                + (self.inverse[0] * position[0] + self.inverse[2] * position[1]) as f32,
            y: self.anchor.position.y
                + (self.inverse[1] * position[0] + self.inverse[3] * position[1]) as f32,
        }
    }

    fn uncertainty(&self, horizon_micros: u32) -> f64 {
        let t = 1. + self.duration(f64::from(horizon_micros) / 1000.) / self.span_ms;
        let (_, end) = evaluate(self.parameters, t);
        let mut variance = 0.;
        for axis in 0..2 {
            for i in 0..6 {
                for j in 0..6 {
                    variance += end[i][axis] * self.covariance[i][j] * end[j][axis];
                }
            }
        }
        variance.max(0.).sqrt()
    }

    /// Statistical lookahead, separate from deterministic motion limits. It is
    /// an estimate from overlapping noisy windows, not an observed stop.
    pub(super) fn confidence_horizon(&self, requested: u32) -> u32 {
        // Allow a 2 px forecast budget above the input noise envelope (the
        // chi-square(2), 99% reference is 9.21). This is an engineering error
        // budget, not a guarantee of 99% coverage for nonlinear forecasts.
        // A fixed pixel-only bound can be below the measurement precision on
        // high-resolution, fast strokes with millisecond timestamps.
        let budget = (FORECAST_ERROR_PX.powi(2) + 9.21 * self.measurement_variance).sqrt();
        let safe = |t| self.uncertainty(t) <= budget;
        let mut accepted = 0;
        for step in 1..=requested.div_ceil(STEP_MICROS) {
            let t = (step * STEP_MICROS).min(requested);
            if !safe(t) {
                let mut rejected = t;
                while rejected - accepted > 1 {
                    let middle = accepted + (rejected - accepted) / 2;
                    if safe(middle) {
                        accepted = middle;
                    } else {
                        rejected = middle;
                    }
                }
                break;
            }
            accepted = t;
        }
        accepted
    }

    /// Input-age travel plus future distance, bounded by measured braking and
    /// the absolute safety radius. Solve arc length with tangential braking.
    pub(super) fn display_distance_budget(&self, age: u32) -> f32 {
        // Input-age compensation is not extra future lookahead. A fixed cap
        // from the newest report can be shorter than the distance between two
        // reports at high speeds, making a stable display-time target impossible.
        (f64::from(super::PREDICTION_DISTANCE_PX) + self.travel(f64::from(age) / 1000.))
            .min(f64::from(super::MAX_PREDICTION_DISTANCE_PX))
            .min(self.stopping_distance)
            .min(if self.parameters[4] < 0. {
                self.travel(self.duration(f64::INFINITY))
            } else {
                f64::INFINITY
            }) as f32
    }

    pub(super) fn motion_horizon(&self, requested: u32, limit: f32) -> u32 {
        let speed = self.speed();
        if speed * 1000. < f64::from(super::MINIMUM_PREDICTION_SPEED) {
            return 0;
        }
        let mut duration = self.duration(f64::from(requested) / 1000.);
        let limit = f64::from(limit).min(self.stopping_distance);
        if self.travel(duration) > limit {
            let acceleration = self.parameters[4] / self.span_ms.powi(2);
            let end_speed = (speed * speed + 2. * acceleration * limit).max(0.).sqrt();
            duration = duration.min(2. * limit / (speed + end_speed));
        }
        requested.min((duration * 1000.).max(0.) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancing_a_cached_fit_preserves_absolute_geometry_uncertainty_and_stop_budget() {
        use crate::feedback::test_support::point;
        for zoom in [0.25, 1., 4.] {
            let transform = [0., zoom, -zoom, 0., 21., -9.];
            let real: Vec<_> = (0..=50)
                .map(|i| {
                    let t = i as f32 * 0.004;
                    point(
                        100. * (t * 8.).sin() / zoom,
                        100. * (1. - (t * 8.).cos()) / zoom,
                        i * 4000,
                    )
                })
                .collect();
            let mut original = MotionFit::fit(&real, transform, 1).unwrap();
            original.stopping_distance = 30.;
            let mut anchor = original.fitted_point_at(8000);
            anchor.position.x += 0.5 / zoom;
            let rebased = original.advanced_to(anchor, transform);
            for time in [0, 4000, 16000] {
                assert!(
                    super::super::surface_distance(
                        original.fitted_point_at(time + 8000).position,
                        rebased.fitted_point_at(time).position,
                        transform
                    ) < 0.001
                );
                assert!(
                    (original.uncertainty(time + 8000) - rebased.uncertainty(time)).abs() < 1e-7
                );
            }
            assert!((rebased.stopping_distance + original.travel(8.) - 30.).abs() < 1e-8);
            assert!(rebased.stopping_distance < original.stopping_distance);
        }
    }

    #[test]
    fn clock_reconstruction_is_bounded_and_respects_irregular_reports() {
        for rate in [90., 120., 200., 240., 360., 480., 1000.] {
            for quantum in [250, 1000] {
                let truth: Vec<_> = (0..64)
                    .map(|i| 1_000_000. + 371. + i as f64 * 1e6 / rate)
                    .collect();
                let mut samples: Vec<_> = truth
                    .iter()
                    .map(|t| Sample {
                        time: (*t / f64::from(quantum)).floor() as u32 * quantum,
                        position: [0.; 2],
                    })
                    .collect();
                let before: Vec<_> = samples.iter().map(|s| s.time).collect();
                reconstruct_times(&mut samples, quantum);
                let scatter = |times: &[u32]| {
                    let mean = times
                        .iter()
                        .zip(&truth)
                        .map(|(t, ideal)| f64::from(*t) - ideal)
                        .sum::<f64>()
                        / 64.;
                    (times
                        .iter()
                        .zip(&truth)
                        .map(|(t, ideal)| (f64::from(*t) - ideal - mean).powi(2))
                        .sum::<f64>()
                        / 64.)
                        .sqrt()
                };
                let after: Vec<_> = samples.iter().map(|s| s.time).collect();
                assert!(
                    scatter(&after) <= scatter(&before) * 0.2 + 30.,
                    "{rate} Hz q={quantum}"
                );
                for i in 0..64 {
                    assert!(after[i].abs_diff(before[i]) <= quantum / 2 + 1);
                    if i > 0 {
                        assert!(after[i] > after[i - 1]);
                    }
                }
            }
        }
        let mut irregular: Vec<_> = (0..64)
            .map(|i| Sample {
                time: 1_000_000 + i * 4000 + (i % 2) * 2500 + if i > 32 { 50_000 } else { 0 },
                position: [0.; 2],
            })
            .collect();
        let before: Vec<_> = irregular.iter().map(|s| s.time).collect();
        reconstruct_times(&mut irregular, 0);
        assert_eq!(before, irregular.iter().map(|s| s.time).collect::<Vec<_>>());
        reconstruct_times(&mut irregular, 1000);
        assert!(
            irregular
                .iter()
                .zip(before)
                .all(|(s, t)| s.time.abs_diff(t) < 50),
            "large real cadence variation must overpower the uniform-clock prior"
        );
    }

    #[test]
    fn time_translation_preserves_physical_forecast_and_uncertainty() {
        for turn in [0., 0.02, 1.5] {
            let parameters = [3., -4., 80., 0.35, -15., turn];
            let samples: Vec<_> = (0..16)
                .map(|i| Sample {
                    time: i * 8000,
                    position: evaluate(parameters, i as f64 / 15.).0,
                })
                .collect();
            let original = MotionFit::fit_window(&samples, 0.04).unwrap();
            for shift in [-0.004, 0.004] {
                let mut rebased = original.clone();
                rebased.translate_time(shift);
                for time in [0.2, 1., 1.2] {
                    let (a, ja) = evaluate(original.parameters, time + shift);
                    let (b, jb) = evaluate(rebased.parameters, time);
                    assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
                    let variance = |j: [Vector; 6], covariance: Matrix| {
                        let mut result = 0.;
                        for axis in 0..2 {
                            for i in 0..6 {
                                for k in 0..6 {
                                    result += j[i][axis] * covariance[i][k] * j[k][axis];
                                }
                            }
                        }
                        result
                    };
                    let old = variance(ja, original.covariance);
                    let new = variance(jb, rebased.covariance);
                    assert!((old - new).abs() < old * 1e-7 + 1e-10);
                }
            }
        }
    }

    #[test]
    fn weighted_fit_covariance_matches_measurement_perturbations() {
        for turn in [0., 1.5] {
            let parameters = [2., -3., 40., 0.2, -5., turn];
            let mut samples: Vec<_> = (0..17)
                .map(|i| Sample {
                    time: 1_000_000 + i * 4000,
                    position: evaluate(parameters, f64::from(i) / 16.).0,
                })
                .collect();
            let variance = 0.04;
            let fit = MotionFit::fit_window(&samples, variance).unwrap();
            let mut numerical = [[0.; 6]; 6];
            let epsilon = 0.001;
            // Differentiate the actual nonlinear weighted fit with respect to
            // every independent sensor coordinate, not the covariance formula.
            for sample in 0..samples.len() {
                for axis in 0..2 {
                    let original = samples[sample].position[axis];
                    samples[sample].position[axis] = original + epsilon;
                    let plus = MotionFit::fit_window(&samples, variance).unwrap();
                    samples[sample].position[axis] = original - epsilon;
                    let minus = MotionFit::fit_window(&samples, variance).unwrap();
                    samples[sample].position[axis] = original;
                    let sensitivity: [f64; 6] = std::array::from_fn(|i| {
                        (plus.parameters[i] - minus.parameters[i]) / (2. * epsilon)
                    });
                    for i in 0..6 {
                        for j in 0..6 {
                            numerical[i][j] += variance * sensitivity[i] * sensitivity[j];
                        }
                    }
                }
            }
            for i in 0..6 {
                for j in 0..6 {
                    let scale = (numerical[i][i] * numerical[j][j]).sqrt();
                    assert!(
                        (fit.covariance[i][j] - numerical[i][j]).abs() < 0.02 * scale + 1e-7,
                        "turn={turn} covariance[{i}][{j}] {} != {}",
                        fit.covariance[i][j],
                        numerical[i][j]
                    );
                }
            }
        }
    }

    #[test]
    fn analytic_motion_derivatives_match_finite_differences() {
        for turn in [0., 0.099, 0.101, -0.8, 2.4] {
            for acceleration in [-30., 0., 8.] {
                let parameters = [1., 2., 20., 0.7, acceleration, turn];
                for t in [0.4, 1., 1.3] {
                    let (_, derivatives) = evaluate(parameters, t);
                    for j in 0..6 {
                        let mut before = parameters;
                        let mut after = parameters;
                        before[j] -= 1e-5;
                        after[j] += 1e-5;
                        let a = evaluate(before, t).0;
                        let b = evaluate(after, t).0;
                        for axis in 0..2 {
                            assert!(
                                ((b[axis] - a[axis]) / 2e-5 - derivatives[j][axis]).abs() < 1e-4
                            );
                        }
                    }
                }
            }
        }
    }
}
