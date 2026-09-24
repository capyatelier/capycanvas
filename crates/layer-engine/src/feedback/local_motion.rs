//! Temporally coherent acceleration, fitted in physical surface coordinates.
//!
//! Transport the previous motion model to the current sample clock before
//! combining it with a new position fit. Averaging endpoints at different
//! target times would add latency. Large model disagreements release memory;
//! the prediction clock bounds the time and physical reach. The stable profile
//! can retain statistical confidence using sustained smooth-motion evidence.
//! Steady motion retains reach at any speed; uncertain turns use less lookahead.

use super::motion_fit::{Sample, observations, reconstruct_times};
use layer_core::{Point, StrokePoint};

const WINDOW_MICROS: u32 = 40_000;
const MEMORY_MICROS: f64 = 24_000.;
const DISAGREEMENT_PX: f64 = 16.;
const FINE_MEMORY_MICROS: f64 = 16_000.;
const FINE_BAND_PX: f64 = 2.;

// Physical screen speed, independent of tablet report rate and document zoom.
// Speed limits the speculative correction, but steady slow motion still needs
// latency compensation. Directional predictability controls that extra reach.
const DETAIL_LOOKAHEAD_MICROS: f64 = 2_000.;
const BROAD_LOOKAHEAD_MICROS: f64 = 24_000.;
const STEADY_LOOKAHEAD_MICROS: f64 = 20_000.;

#[derive(Clone, Copy, Debug, PartialEq)]
struct MotionAllowance {
    detail: f64,
    reach: f64,
    fine_broad: f64,
}

fn motion_allowance(samples: &[Sample], anchor: u32) -> MotionAllowance {
    let minimum = MotionAllowance {
        detail: DETAIL_LOOKAHEAD_MICROS,
        reach: DETAIL_LOOKAHEAD_MICROS,
        fine_broad: 0.,
    };
    let at = |time: u32| -> Option<[f64; 2]> {
        let i = samples.partition_point(|p| p.time < time);
        if let Some(p) = samples.get(i)
            && p.time == time
        {
            return Some(p.position);
        }
        if i == 0 || i == samples.len() {
            return None;
        }
        let (a, b) = (samples[i - 1], samples[i]);
        if b.time - a.time > 32_000 {
            return None;
        }
        let t = f64::from(time - a.time) / f64::from(b.time - a.time);
        Some([0, 1].map(|axis| a.position[axis] + t * (b.position[axis] - a.position[axis])))
    };
    let Some(p0) = anchor.checked_sub(32_000).and_then(at) else {
        return minimum;
    };
    let Some(p1) = at(anchor - 16_000) else {
        return minimum;
    };
    let Some(p2) = at(anchor) else {
        return minimum;
    };
    let v0 = [p1[0] - p0[0], p1[1] - p0[1]];
    let v1 = [p2[0] - p1[0], p2[1] - p1[1]];
    let length = |v: [f64; 2]| v[0].hypot(v[1]);
    let speed = (length(v0) + length(v1)) / 0.032;
    let u = ((speed - 300.) / 1700.).clamp(0., 1.);
    let speed_limit = DETAIL_LOOKAHEAD_MICROS
        + (BROAD_LOOKAHEAD_MICROS - DETAIL_LOOKAHEAD_MICROS) * u * u * (3. - 2. * u);
    let angle = |a: [f64; 2], b: [f64; 2]| {
        if length(a).min(length(b)) < 1. {
            0.
        } else {
            (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1])
        }
    };
    let recent_turn = angle(v0, v1);
    let mut turn = recent_turn.abs();
    if let Some(p) = anchor.checked_sub(48_000).and_then(at) {
        let earlier_turn = angle([p0[0] - p[0], p0[1] - p[1]], v0);
        // Alternating turns matter even when net heading change cancels out.
        turn = turn
            .max(earlier_turn.abs())
            .max((recent_turn - earlier_turn).abs());
    }
    // Forecast no more than about 0.36 radians into a tight/changing turn.
    // A steady broad arc is not treated as erratic just because it curves.
    let turn_limit = (0.36 * 16_000. / turn.max(1e-6)).max(DETAIL_LOOKAHEAD_MICROS);
    let detail = speed_limit.min(turn_limit);
    // An 8 ms chord catches a new corner before the longer position fit does.
    // The overlapping longer chords suppress report noise, while the short
    // adjacent chords detect corners. Below 1 px, heading is undefined.
    // Reach confidence uses the last 24 ms. An older corner must not keep
    // suppressing a now-straight line after the heading has settled.
    let mut direction_risk: f64 = 0.;
    if let Some(p) = at(anchor - 8_000) {
        direction_risk = direction_risk
            .max(angle([p[0] - p1[0], p[1] - p1[1]], [p2[0] - p[0], p2[1] - p[1]]).abs());
        if let Some(earlier) = at(anchor - 24_000) {
            direction_risk =
                direction_risk.max(2. * angle([p[0] - earlier[0], p[1] - earlier[1]], v1).abs());
        }
    }
    let smoothstep = |x: f64| {
        let t = x.clamp(0., 1.);
        t * t * (3. - 2. * t)
    };
    let steady =
        (1. - smoothstep((direction_risk - 0.04) / 0.16)) * smoothstep((speed - 60.) / 120.);
    let steady_limit = STEADY_LOOKAHEAD_MICROS;
    MotionAllowance {
        fine_broad: smoothstep((speed - 1400.) / 600.),
        detail,
        reach: detail
            .max(DETAIL_LOOKAHEAD_MICROS + (steady_limit - DETAIL_LOOKAHEAD_MICROS) * steady),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Quadratic {
    coefficients: [[f64; 2]; 3],
}

impl Quadratic {
    fn fit(samples: &[Sample], anchor: u32) -> Option<Self> {
        let first = samples.partition_point(|s| s.time < anchor.saturating_sub(WINDOW_MICROS));
        let samples = &samples[first..];
        if samples.len() < 4 {
            return None;
        }
        let mut system = [[0.; 5]; 3];
        for sample in samples {
            let t = (f64::from(sample.time) - f64::from(anchor)) / f64::from(WINDOW_MICROS);
            let basis = [1., t, t * t];
            let weight = t.exp();
            for i in 0..3 {
                for j in 0..3 {
                    system[i][j] += weight * basis[i] * basis[j];
                }
                for axis in 0..2 {
                    system[i][3 + axis] += weight * basis[i] * sample.position[axis];
                }
            }
        }
        for col in 0..3 {
            let pivot =
                (col..3).max_by(|&i, &j| system[i][col].abs().total_cmp(&system[j][col].abs()))?;
            system.swap(col, pivot);
            let divisor = system[col][col];
            if !divisor.is_finite() || divisor.abs() < 1e-10 {
                return None;
            }
            for j in col..5 {
                system[col][j] /= divisor;
            }
            for i in 0..3 {
                if i == col {
                    continue;
                }
                let scale = system[i][col];
                for j in col..5 {
                    system[i][j] -= scale * system[col][j];
                }
            }
        }
        let coefficients = system.map(|row| [row[3], row[4]]);
        coefficients
            .iter()
            .flatten()
            .all(|v| v.is_finite())
            .then_some(Self { coefficients })
    }

    fn position(&self, time: u32) -> [f64; 2] {
        let t = f64::from(time) / f64::from(WINDOW_MICROS);
        [0, 1].map(|axis| {
            self.coefficients[0][axis]
                + t * (self.coefficients[1][axis] + t * self.coefficients[2][axis])
        })
    }

    fn transported(&self, elapsed: u32, anchor_delta: [f64; 2]) -> Self {
        let t = f64::from(elapsed) / f64::from(WINDOW_MICROS);
        let mut result = self.clone();
        for axis in 0..2 {
            result.coefficients[0][axis] += anchor_delta[axis]
                + t * (self.coefficients[1][axis] + t * self.coefficients[2][axis]);
            result.coefficients[1][axis] += 2. * t * self.coefficients[2][axis];
        }
        result
    }

    fn blend(&mut self, old: &Self, weight: f64) {
        for i in 0..3 {
            for axis in 0..2 {
                self.coefficients[i][axis] +=
                    weight * (old.coefficients[i][axis] - self.coefficients[i][axis]);
            }
        }
    }

    fn disagreement(&self, other: &Self, horizon: u32) -> f64 {
        // Sample the entire short forecast, including the fitted anchor.
        (0..=8)
            .map(|i| {
                let a = self.position(horizon * i / 8);
                let b = other.position(horizon * i / 8);
                (a[0] - b[0]).hypot(a[1] - b[1])
            })
            .fold(0., f64::max)
    }

    fn preview_disagreement_bound(&self, other: &Self, horizon: u32, anchor_gain: f64) -> f64 {
        // The joined preview is cubic. Its Bezier control polygon bounds the
        // difference over ALL points, including between tessellation samples.
        let delta: [[f64; 2]; 3] = std::array::from_fn(|i| {
            [0, 1].map(|axis| self.coefficients[i][axis] - other.coefficients[i][axis])
        });
        let h = f64::from(horizon) / f64::from(WINDOW_MICROS);
        let b1 = delta[1].map(|v| v * h);
        let b2 = [0, 1].map(|a| delta[2][a] * h * h + 3. * anchor_gain * delta[0][a]);
        let b3 = delta[0].map(|v| -2. * anchor_gain * v);
        [
            b1.map(|v| v / 3.),
            [0, 1].map(|a| (2. * b1[a] + b2[a]) / 3.),
            [0, 1].map(|a| b1[a] + b2[a] + b3[a]),
        ]
        .into_iter()
        .map(|p| p[0].hypot(p[1]))
        .fold(0., f64::max)
    }

    fn joined(&self, time: u32, horizon: u32, anchor_gain: f64) -> [f64; 2] {
        let end = self.position(time);
        let mut delta = [0, 1].map(|a| end[a] - self.coefficients[0][a]);
        let t = (f64::from(time) / f64::from(horizon.max(1))).clamp(0., 1.);
        let mut correction = self.coefficients[0].map(|v| v * anchor_gain * t * t * (3. - 2. * t));
        let radius = delta[0].hypot(delta[1]);
        let length = correction[0].hypot(correction[1]);
        if length > radius {
            correction = correction.map(|v| v * radius / length);
        }
        for axis in 0..2 {
            delta[axis] += correction[axis];
        }
        delta
    }
}

#[derive(Clone, Debug)]
pub(super) struct LocalMotion {
    observed: Quadratic,
    observed_allowance: MotionAllowance,
    responsive: Quadratic,
    fine: Quadratic,
    anchor: StrokePoint,
    transform: [f32; 6],
    inverse: [f64; 4],
    lookahead_micros: f64,
    reach_micros: f64,
    nominal_horizon: Option<u32>,
    sustained: f64,
    reach_stability: f64,
}

impl LocalMotion {
    pub fn fit(
        real: &[StrokePoint],
        transform: [f32; 6],
        quantum: u32,
        horizon: u32,
        memory: Option<(u32, f64)>,
        previous: Option<&Self>,
        continuity: f64,
    ) -> Option<Self> {
        let nominal = memory.map(|(horizon, _)| horizon);
        let stable = memory.is_some();
        let anchor = *real.last()?;
        let [a, b, c, d, _, _] = transform.map(f64::from);
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant.abs() < f64::EPSILON {
            return None;
        }
        let (mut samples, n) = observations(real, transform)?;
        // Use one clock for the chord endpoints. The fit's cadence recovery can
        // move its last sample slightly before the recorded anchor; querying
        // that reconstructed path at the raw anchor would look like missing data.
        let desired = motion_allowance(&samples[..n], anchor.elapsed_micros);
        reconstruct_times(&mut samples[..n], quantum);
        let observed = Quadratic::fit(&samples[..n], anchor.elapsed_micros)?;
        let sustained = memory.map_or(0., |(_, strength)| strength);
        let reach_stability = sustained.powi(4).max(continuity);
        let previous =
            previous.filter(|p| p.transform == transform && p.nominal_horizon == nominal);
        if let Some(old) = previous
            && old.anchor == anchor
            && old.observed == observed
            && old.observed_allowance == desired
            && old.sustained == sustained
            && old.reach_stability == reach_stability
        {
            // Repeated queries (including frames with no new reports) do not
            // compound smoothing. Corrections that change the fit invalidate it.
            return Some(old.clone());
        }
        let mut responsive = observed.clone();
        let previous = previous.filter(|p| {
            anchor.elapsed_micros > p.anchor.elapsed_micros
                && anchor.elapsed_micros - p.anchor.elapsed_micros < 32_000
        });
        let lookahead_micros = previous.map_or(desired.detail, |old| {
            let elapsed = f64::from(anchor.elapsed_micros - old.anchor.elapsed_micros);
            let tau = if desired.detail < old.lookahead_micros {
                12_000.
            } else {
                32_000.
            };
            let next =
                desired.detail + (old.lookahead_micros - desired.detail) * (-elapsed / tau).exp();
            // A change in this allowance alone must not withdraw the target
            // faster than measured time advances. Braking/confidence can still
            // shorten the trajectory immediately through the original clock.
            next.max(old.lookahead_micros - 0.8 * elapsed)
        });
        let reach_micros = previous
            .map_or(desired.reach, |old| {
                let elapsed = f64::from(anchor.elapsed_micros - old.anchor.elapsed_micros);
                let tau = if desired.reach < old.reach_micros {
                    // Retain reach through a brief heading/speed fluctuation
                    // without lengthening the geometric fit's memory. Stops
                    // and changing direction revoke continuity in the caller.
                    3_000. + 24_000. * reach_stability
                } else {
                    8_000.
                };
                desired.reach + (old.reach_micros - desired.reach) * (-elapsed / tau).exp()
            })
            .max(lookahead_micros);
        let broad = if stable { desired.fine_broad } else { 0. };
        let fine_broad = if stable { desired.fine_broad } else { 0. };
        let comparison_horizon = (f64::from(horizon)
            + broad * (f64::from(nominal.unwrap_or(horizon)) - f64::from(horizon)))
        .round() as u32;
        let mut transported_fine = None;
        let mut elapsed = 0;
        if let Some(old) = previous {
            elapsed = anchor.elapsed_micros - old.anchor.elapsed_micros;
            let delta = super::transform_vector(
                transform,
                Point {
                    x: old.anchor.position.x - anchor.position.x,
                    y: old.anchor.position.y - anchor.position.y,
                },
            );
            let delta = [f64::from(delta.x), f64::from(delta.y)];
            let prior = old.responsive.transported(elapsed, delta);
            let disagreement = responsive.disagreement(&prior, comparison_horizon);
            let detail_gain = (lookahead_micros - DETAIL_LOOKAHEAD_MICROS)
                / (BROAD_LOOKAHEAD_MICROS - DETAIL_LOOKAHEAD_MICROS);
            // A few pixels of disagreement are a material heading change in
            // fine motion. Release stale acceleration sooner at that scale.
            let disagreement_budget = 2. + (DISAGREEMENT_PX - 2.) * detail_gain;
            let memory = MEMORY_MICROS;
            let weight = (-f64::from(elapsed) / memory).exp()
                / (1. + (disagreement / disagreement_budget).powi(2));
            responsive.blend(&prior, weight);
            transported_fine = Some(old.fine.transported(elapsed, delta));
        }
        let fine = {
            let mut model = responsive.clone();
            if let Some(prior) = transported_fine {
                let anchor_gain = (lookahead_micros - DETAIL_LOOKAHEAD_MICROS)
                    / (BROAD_LOOKAHEAD_MICROS - DETAIL_LOOKAHEAD_MICROS);
                let difference =
                    responsive.preview_disagreement_bound(&prior, comparison_horizon, anchor_gain);
                let t = (2. - 2. * difference / (FINE_BAND_PX + 2. * fine_broad)).clamp(0., 1.);
                let memory = FINE_MEMORY_MICROS + 16_000. * fine_broad;
                let weight = t * t * (3. - 2. * t) * (-f64::from(elapsed) / memory).exp();
                model.blend(&prior, weight);
            }
            model
        };
        Some(Self {
            observed,
            observed_allowance: desired,
            responsive,
            fine,
            anchor,
            transform,
            inverse: [
                d / determinant,
                -b / determinant,
                -c / determinant,
                a / determinant,
            ],
            lookahead_micros,
            reach_micros,
            nominal_horizon: nominal,
            sustained,
            reach_stability,
        })
    }

    pub fn horizon(&self, accepted: u32, age: u32, configured: u32) -> u32 {
        let fraction = (self.reach_micros - DETAIL_LOOKAHEAD_MICROS)
            / (BROAD_LOOKAHEAD_MICROS - DETAIL_LOOKAHEAD_MICROS);
        // Uncertainty grows from the last measured sample, not from the display
        // clock. Steady motion can pay back input age at any speed; uncertain
        // direction changes retain the shorter spatial-detail allowance.
        let cap = DETAIL_LOOKAHEAD_MICROS
            + (f64::from(configured.saturating_add(age)) - DETAIL_LOOKAHEAD_MICROS).max(0.)
                * fraction;
        accepted.min(cap.round() as u32)
    }

    fn from_delta(&self, delta: [f64; 2], time: u32) -> StrokePoint {
        StrokePoint {
            position: Point {
                x: self.anchor.position.x
                    + (self.inverse[0] * delta[0] + self.inverse[2] * delta[1]) as f32,
                y: self.anchor.position.y
                    + (self.inverse[1] * delta[0] + self.inverse[3] * delta[1]) as f32,
            },
            elapsed_micros: self.anchor.elapsed_micros.saturating_add(time),
            ..self.anchor
        }
    }

    /// Anchor joining describes geometry, not how much of the curve is visible.
    /// Cropping the horizon must not reshape every remaining preview point.
    pub fn join_horizon(&self) -> u32 {
        self.lookahead_micros.max(8_000.).round() as u32
    }

    pub fn output_at(&self, time: u32, horizon: u32) -> StrokePoint {
        // At short detail horizons, do not spend the entire fitted-anchor
        // innovation within a tiny tail. That offset represents a long-window
        // fit and can otherwise dominate actual forward motion near a corner.
        let anchor_gain = (self.lookahead_micros - DETAIL_LOOKAHEAD_MICROS)
            / (BROAD_LOOKAHEAD_MICROS - DETAIL_LOOKAHEAD_MICROS);
        let base = self.responsive.joined(time, horizon, anchor_gain);
        let result = {
            let fine = self.fine.joined(time, horizon, anchor_gain);
            let delta = [fine[0] - base[0], fine[1] - base[1]];
            // Also enforce the bound after the nonlinear anchor-correction cap.
            let gain = (2. / delta[0].hypot(delta[1])).min(1.);
            [0, 1].map(|a| base[a] + gain * delta[a])
        };
        self.from_delta(result, time)
    }

    pub fn point_at(&self, time: u32) -> StrokePoint {
        // Use the responsive component for the age/distance budget.
        let end = self.responsive.position(time);
        let start = self.responsive.position(0);
        self.from_delta([end[0] - start[0], end[1] - start[1]], time)
    }
}
