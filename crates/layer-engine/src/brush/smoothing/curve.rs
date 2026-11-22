use layer_core::{Point, StrokePoint};
use std::collections::VecDeque;

type Vector = [f64; 3];
type Moments = [Vector; 5];
const KERNEL: [f64; 5] = [0., 20., -60., 60., -20.];

pub(super) fn mass(u: f64) -> f64 {
    let u = u.clamp(0., 1.);
    u * u * (10. + u * (-20. + u * (15. - 4. * u)))
}
fn moment(u: f64) -> f64 {
    let u = u.clamp(0., 1.);
    u.powi(3) * (20. / 3. + u * (-15. + u * (12. - 10. / 3. * u)))
}
fn polynomial(coefficients: &[f64], at: f64) -> f64 {
    coefficients.iter().rev().fold(0., |value, coefficient| value * at + coefficient)
}
fn noise_gain(support: f64) -> f64 {
    if support > 4. {
        let d = support.fract();
        let c8 = polynomial(&[10./3., -20./7., -5., 20., 10., -32., -10., 160./7., 5., -10., 2.], d);
        let c9 = polynomial(&[-256./63., -4./3., 20., 20./3., -60., -40./3., 208./3., 40./3., -40., -20./3., 44./3., -8./3.], d);
        let c10 = polynomial(&[-8./9., 256./63., 8./3., -80./3., -20./3., 48., 80./9., -832./21., -20./3., 160./9., 8./3., -16./3., 8./9.], d);
        return polynomial(&[0., 100./63., 0., -40./7., 100./9., -28./3., -5., 200./21., c8, c9, c10], 1./support);
    }
    let mut gain = 0.;
    for j in 0..=support.ceil() as usize {
        let j = j as f64;
        let lo = ((j - 1.) / support).clamp(0., 1.);
        let mid = (j / support).clamp(0., 1.);
        let hi = ((j + 1.) / support).clamp(0., 1.);
        let weight = (1. - j) * (mass(mid) - mass(lo)) + support * (moment(mid) - moment(lo))
            + (1. + j) * (mass(hi) - mass(mid)) - support * (moment(hi) - moment(mid));
        gain += weight * weight;
    }
    gain
}
pub(super) fn calibrated_support(tau: f64, interval: f64) -> f64 {
    if interval <= 0. { return 200. / 63. * tau; }
    let gain = (interval / (2. * tau)).tanh();
    if gain >= 5. / 9. {
        return (3. * interval * (1. - gain) / (1. + (2. * gain - 1.).sqrt())).max(1e-9);
    }
    let mut lo = 0.;
    let mut hi = (200. / 63. * tau / interval).max(1.);
    for _ in 0..28 {
        let mid = (lo + hi) * 0.5;
        if noise_gain(mid) > gain { lo = mid; } else { hi = mid; }
    }
    interval * (lo + hi) * 0.5
}

#[derive(Clone, Debug)]
struct Node {
    point: StrokePoint,
    clock: f64,
    rate: f64,
    slope: Vector,
    slope_norm: f64,
    slope_change_norm: f64,
    rate_change_bound: f64,
    origin: Vector,
    prefix: Moments,
}
fn vector(point: StrokePoint) -> Vector { [f64::from(point.position.x), f64::from(point.position.y), f64::from(point.pressure)] }
fn mix(a: Vector, b: Vector, u: f64) -> Vector { std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * u) }
fn integral(begin: f64, end: f64, intercept: Vector, slope: Vector) -> Moments {
    std::array::from_fn(|j| {
        let a = (end.powi(j as i32 + 1) - begin.powi(j as i32 + 1)) / (j + 1) as f64;
        let b = (end.powi(j as i32 + 2) - begin.powi(j as i32 + 2)) / (j + 2) as f64;
        std::array::from_fn(|axis| intercept[axis] * a + slope[axis] * b)
    })
}
fn kernel(u: f64) -> f64 { if (0. ..=1.).contains(&u) { 20. * u * (1. - u).powi(3) } else { 0. } }
fn kernel_bound(lo: f64, hi: f64) -> f64 {
    if hi < 0. || lo > 1. { 0. } else { kernel(0.25f64.clamp(lo.max(0.), hi.min(1.))) }
}

#[derive(Clone, Debug)]
pub(super) struct Curve { nodes: VecDeque<Node> }
impl Curve {
    pub(super) fn new(first: StrokePoint) -> Self {
        Self { nodes: VecDeque::from([Node { point: first, clock: 0., rate: 0., slope: [0.; 3], slope_norm: 0., slope_change_norm: 0., rate_change_bound: 0., origin: vector(first), prefix: [[0.; 3]; 5] }]) }
    }
    pub(super) fn append(&mut self, point: StrokePoint, support: f64) -> (Point, f32) {
        let previous = self.nodes.back().unwrap();
        let dt = f64::from(point.elapsed_micros.saturating_sub(previous.point.elapsed_micros)) * 1e-6;
        if dt == 0. { return self.sample(f64::from(previous.point.elapsed_micros) * 1e-6); }
        let rate = 1. / support.max(1e-9);
        let previous_rate = if previous.rate > 0. { previous.rate } else { rate };
        let clock = previous.clock + dt * (previous_rate + rate) * 0.5;
        let input = vector(previous.point);
        let target = vector(point);
        let slope: Vector = std::array::from_fn(|axis| (target[axis] - input[axis]) / (clock - previous.clock));
        let slope_norm = slope[0].hypot(slope[1]);
        let slope_change_norm = (slope[0] - previous.slope[0]).hypot(slope[1] - previous.slope[1]);
        let rate_change_bound = 1.5 * (rate - previous_rate).abs() / dt;
        let block = clock.floor();
        let same_block = previous.clock.floor() == block;
        let origin = if same_block { previous.origin } else { mix(input, target, (block - previous.clock) / (clock - previous.clock)) };
        let begin = if same_block { previous.clock - block } else { 0. };
        let intercept = std::array::from_fn(|axis| input[axis] + slope[axis] * (block - previous.clock) - origin[axis]);
        let added = integral(begin, clock - block, intercept, slope);
        let prefix = std::array::from_fn(|j| std::array::from_fn(|axis| added[j][axis] + if same_block { previous.prefix[j][axis] } else { 0. }));
        if self.nodes.back().unwrap().rate == 0. { self.nodes.back_mut().unwrap().rate = rate; }
        self.nodes.push_back(Node { point, clock, rate, slope, slope_norm, slope_change_norm, rate_change_bound, origin, prefix });
        self.sample(f64::from(point.elapsed_micros) * 1e-6)
    }
    pub(super) fn latest(&self) -> StrokePoint { self.nodes.back().unwrap().point }
    pub(super) fn support(&self) -> f64 { let rate = self.nodes.back().unwrap().rate; if rate > 0. { 1. / rate } else { 0. } }
    pub(super) fn end_time(&self) -> f64 { f64::from(self.latest().elapsed_micros) * 1e-6 + self.support() }
    fn clock(&self, time: f64) -> (f64, f64, f64) {
        let index = self.nodes.partition_point(|node| f64::from(node.point.elapsed_micros) * 1e-6 <= time);
        if index == 0 { return (self.nodes[0].clock, self.nodes[0].rate, 0.); }
        let a = &self.nodes[index - 1];
        let begin = f64::from(a.point.elapsed_micros) * 1e-6;
        let Some(b) = self.nodes.get(index) else { return (a.clock + (time - begin) * a.rate, a.rate, 0.); };
        let dt = f64::from(b.point.elapsed_micros - a.point.elapsed_micros) * 1e-6;
        let u = ((time - begin) / dt).clamp(0., 1.);
        let change = b.rate - a.rate;
        (a.clock + dt * (a.rate * u + change * u.powi(3) * (1. - 0.5 * u)),
            a.rate + change * u * u * (3. - 2. * u), change * 6. * u * (1. - u) / dt)
    }
    fn input(&self, clock: f64) -> (Vector, Vector) {
        let index = self.nodes.partition_point(|node| node.clock <= clock);
        if index == 0 { return (vector(self.nodes[0].point), [0.; 3]); }
        let a = &self.nodes[index - 1];
        let Some(b) = self.nodes.get(index) else { return (vector(a.point), [0.; 3]); };
        (mix(vector(a.point), vector(b.point), (clock - a.clock) / (b.clock - a.clock)), b.slope)
    }
    fn primitive(&self, clock: f64, block: f64) -> (Moments, Vector) {
        let index = self.nodes.partition_point(|node| node.clock < clock);
        if index == 0 { return ([[0.; 3]; 5], vector(self.nodes[0].point)); }
        let a = &self.nodes[index - 1];
        let slope = self.nodes.get(index).map_or([0.; 3], |node| node.slope);
        let same_block = a.clock.floor() == block;
        let at = vector(a.point);
        let origin = if same_block { a.origin } else { std::array::from_fn(|axis| at[axis] + slope[axis] * (block - a.clock)) };
        let intercept = std::array::from_fn(|axis| at[axis] + slope[axis] * (block - a.clock) - origin[axis]);
        let added = integral(if same_block { a.clock - block } else { 0. }, clock - block, intercept, slope);
        (std::array::from_fn(|j| std::array::from_fn(|axis| added[j][axis] + if same_block { a.prefix[j][axis] } else { 0. })), origin)
    }
    fn evaluate<const N: usize>(&self, clock: f64) -> [Vector; N] {
        let (anchor, slope) = self.input(clock);
        let index = self.nodes.partition_point(|node| node.clock <= clock - 1.);
        if index > 0 && self.nodes.get(index).is_none_or(|node| node.clock > clock) {
            return std::array::from_fn(|derivative| match derivative { 0 => std::array::from_fn(|axis| anchor[axis] - slope[axis] / 3.), 1 => slope, _ => [0.; 3] });
        }
        let mut jet = [[0.; 3]; N];
        jet[0] = anchor;
        let mut lo = clock - 1.;
        for _ in 0..2 {
            let block = lo.floor();
            let hi = clock.min(block + 1.);
            let (a, origin) = self.primitive(lo, block);
            let (b, other_origin) = self.primitive(hi, block);
            let l = lo - block; let h = hi - block; let x = clock - block;
            let moments: Moments = std::array::from_fn(|j| {
                let area = (h.powi(j as i32 + 1) - l.powi(j as i32 + 1)) / (j + 1) as f64;
                std::array::from_fn(|axis| b[j][axis] - a[j][axis] + (origin[axis] - anchor[axis]) * area
                    + (other_origin[axis] - origin[axis]) * h.powi(j as i32 + 1) / (j + 1) as f64)
            });
            for derivative in 0..N {
                for (j, moment) in moments.iter().enumerate().take(5 - derivative) {
                    let mut coefficient = 0.;
                    for n in j..5 - derivative {
                        let mut factor = KERNEL[n + derivative];
                        for k in 0..derivative { factor *= (n + derivative - k) as f64; }
                        let mut choose = 1.;
                        for k in 0..j { choose *= (n - k) as f64 / (k + 1) as f64; }
                        coefficient += factor * choose * x.powi((n - j) as i32) * if j % 2 == 0 { 1. } else { -1. };
                    }
                    for (value, moment) in jet[derivative].iter_mut().zip(moment) { *value += coefficient * moment; }
                }
            }
            lo = hi;
            if lo >= clock { break; }
        }
        jet
    }
    pub(super) fn sample(&self, time: f64) -> (Point, f32) {
        let value = self.evaluate::<1>(self.clock(time).0)[0];
        (Point { x: value[0] as f32, y: value[1] as f32 }, value[2].clamp(0., 1.) as f32)
    }
    pub(super) fn retain_from(&mut self, time: u32) {
        let cutoff = self.clock(f64::from(time) * 1e-6).0 - 1.;
        while self.nodes.len() > 2 && self.nodes[1].clock < cutoff { self.nodes.pop_front(); }
    }
    pub(super) fn acceleration_bound(&self, begin: f64, end: f64) -> f64 {
        let lo = self.clock(begin).0; let hi = self.clock(end).0;
        let mut velocity: f64 = 0.;
        let first = &self.nodes[0];
        let mut acceleration = first.slope_norm * kernel_bound(lo - first.clock, hi - first.clock);
        let start = self.nodes.partition_point(|node| node.clock < lo - 1.).max(1);
        let stop = (self.nodes.partition_point(|node| node.clock <= hi) + 1).min(self.nodes.len());
        for i in start..stop {
            let node = &self.nodes[i];
            velocity = velocity.max(node.slope_norm);
            let source = self.nodes[i - 1].clock;
            acceleration += node.slope_change_norm * kernel_bound(lo - source, hi - source);
        }
        let last = self.nodes.back().unwrap();
        acceleration += last.slope_norm * kernel_bound(lo - last.clock, hi - last.clock);
        let mut rate = self.clock(begin).1.max(self.clock(end).1);
        let mut change: f64 = 0.;
        let start = self.nodes.partition_point(|node| f64::from(node.point.elapsed_micros) * 1e-6 <= begin).max(1);
        let stop = (self.nodes.partition_point(|node| f64::from(node.point.elapsed_micros) * 1e-6 < end) + 1).min(self.nodes.len());
        for i in start..stop {
            let a = &self.nodes[i - 1]; let b = &self.nodes[i];
            rate = rate.max(a.rate).max(b.rate);
            change = change.max(b.rate_change_bound);
        }
        acceleration * rate * rate + velocity * change
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::test_support::point;
    pub(in crate::brush::smoothing) fn jet_at(curve: &Curve, time: f64) -> [Vector; 3] {
        let (clock, rate, acceleration) = curve.clock(time);
        let mut jet = curve.evaluate::<3>(clock);
        jet[2] = std::array::from_fn(|axis| jet[2][axis] * rate * rate + jet[1][axis] * acceleration);
        jet[1] = std::array::from_fn(|axis| jet[1][axis] * rate);
        jet
    }
    #[test]
    fn cached_window_bound_matches_full_scan_before_and_after_retention() {
        fn full_scan(curve: &Curve, begin: f64, end: f64) -> f64 {
            let lo = curve.clock(begin).0; let hi = curve.clock(end).0;
            let mut velocity: f64 = 0.; let mut acceleration = 0.;
            let mut previous = [0.; 3];
            for (at, node) in curve.nodes.iter().enumerate() {
                if at > 0 && node.clock >= lo - 1. && curve.nodes[at - 1].clock <= hi {
                    velocity = velocity.max(node.slope[0].hypot(node.slope[1]));
                }
                let change: Vector = std::array::from_fn(|axis| node.slope[axis] - previous[axis]);
                let source = if at == 0 { node.clock } else { curve.nodes[at - 1].clock };
                acceleration += change[0].hypot(change[1]) * kernel_bound(lo - source, hi - source);
                previous = node.slope;
            }
            let last = curve.nodes.back().unwrap();
            acceleration += last.slope[0].hypot(last.slope[1]) * kernel_bound(lo - last.clock, hi - last.clock);
            let mut rate = curve.clock(begin).1.max(curve.clock(end).1);
            let mut change: f64 = 0.;
            for i in 1..curve.nodes.len() {
                let a = &curve.nodes[i - 1]; let b = &curve.nodes[i];
                if f64::from(b.point.elapsed_micros) * 1e-6 > begin && f64::from(a.point.elapsed_micros) * 1e-6 < end {
                    rate = rate.max(a.rate).max(b.rate);
                    change = change.max(1.5 * (b.rate - a.rate).abs() / (f64::from(b.point.elapsed_micros - a.point.elapsed_micros) * 1e-6));
                }
            }
            acceleration * rate * rate + velocity * change
        }
        let mut curve = Curve::new(point(20., 30., 1., 0));
        let mut elapsed = 0;
        for i in 0..160 {
            elapsed += [1000, 4000, 16000, 7000][i % 4];
            curve.append(point(20. + i as f32 * 2., 30. + (i as f32 * 0.4).sin() * 20., 1., elapsed), [0.006, 0.05, 0.2, 0.07][i % 4]);
        }
        for retained in [false, true] {
            if retained { curve.retain_from(elapsed); }
            let mut times = vec![-0.01, 0., f64::from(elapsed) * 1e-6 + 0.001, curve.end_time(), curve.end_time() + 0.1];
            for node in &curve.nodes {
                let t = f64::from(node.point.elapsed_micros) * 1e-6;
                times.extend([t - 1e-9, t, t + 1e-9]);
            }
            times.sort_by(f64::total_cmp);
            for pair in times.windows(2) {
                for end in [pair[1], pair[1] + 0.03, curve.end_time() + 0.2] {
                    let begin = pair[0];
                    assert_eq!(curve.acceleration_bound(begin, end).to_bits(), full_scan(&curve, begin, end).to_bits(), "retained={retained}, {begin}..{end}");
                }
            }
        }
    }

    #[test]
    fn constant_work_calibration_matches_direct_sample_weights() {
        for support in [0.1f64, 0.9, 1., 1.01, 3.999999, 4., 4.000001, 6.25, 19.0476, 152.3, 635., 2000.75] {
            let expected: f64 = (0..=support.ceil() as usize).map(|j| {
                let j = j as f64;
                let lo = ((j-1.)/support).clamp(0.,1.);
                let mid = (j/support).clamp(0.,1.);
                let hi = ((j+1.)/support).clamp(0.,1.);
                let w = (1.-j)*(mass(mid)-mass(lo))+support*(moment(mid)-moment(lo))
                    +(1.+j)*(mass(hi)-mass(mid))-support*(moment(hi)-moment(mid));
                w*w
            }).sum();
            assert!((noise_gain(support)-expected).abs() < 2e-12, "{support}");
        }
    }
    #[test]
    fn polynomial_kernel_matches_direct_quadrature_and_keeps_all_samples() {
        let mut curve = Curve::new(point(20., 30., 1., 0));
        for i in 1..=120 { curve.append(point(20. + i as f32, 30. + (i as f32 * 0.4).sin() * 20., 1., i * 4000), 0.1); }
        for i in 1..=110 {
            let t = i as f64 * 0.004; let clock = t / 0.1;
            let mut expected = [0.; 2];
            for j in 0..20000 { let u = (j as f64 + 0.5) / 20000.; let p = curve.input(clock - u).0; for axis in 0..2 { expected[axis] += kernel(u) * p[axis] / 20000.; } }
            let got = curve.sample(t).0;
            assert!((f64::from(got.x) - expected[0]).hypot(f64::from(got.y) - expected[1]) < 0.00002, "{t}: {got:?} {expected:?}");
        }
    }
    #[test]
    fn subdividing_a_linear_input_preserves_the_continuous_curve() {
        for support in [1e-9, 0.006, 0.05, 0.3] {
            let mut coarse = Curve::new(point(0., 0., 1., 0)); coarse.append(point(16., 32., 1., 16000), support);
            let mut dense = Curve::new(point(0., 0., 1., 0)); for i in 1..=16 { dense.append(point(i as f32, i as f32 * 2., 1., i * 1000), support); }
            for i in 0..=160 { let a = coarse.sample(i as f64 * 0.0001).0; let b = dense.sample(i as f64 * 0.0001).0; assert!((a.x-b.x).hypot(a.y-b.y) < 0.00001, "{support}, {i}: {a:?} {b:?}"); }
        }
    }
    #[test]
    fn hold_completion_is_c2_and_reaches_the_entire_pen_up_displacement() {
        let mut curve = Curve::new(point(0., 0., 1., 0));
        for i in 1..=100 { curve.append(point(i as f32 * 2., (i as f32 * 0.05).sin() * 30., 1., i * 4000), 0.3); }
        let end = curve.end_time(); let jet = jet_at(&curve, end);
        assert_eq!(curve.sample(end).0, curve.latest().position);
        assert!(jet[1][0].hypot(jet[1][1]) < 1e-7 && jet[2][0].hypot(jet[2][1]) < 1e-5);
        for i in 1..100 { let t=i as f64*0.004; let a=jet_at(&curve,t-1e-9); let b=jet_at(&curve,t+1e-9); for axis in 0..2 { assert!((a[2][axis]-b[2][axis]).abs()<0.02); } }
    }
    #[test]
    fn adaptive_clock_preserves_c2_and_bounds_long_input_segments() {
        let mut curve = Curve::new(point(0., 0., 1., 0));
        for i in 1..=30 { curve.append(point(i as f32 * 100., (i as f32 * 0.7).sin() * 20., 1., i * 100000), if i % 2 == 0 { 0.01 } else { 0.32 }); }
        for i in 1..30 {
            let t = i as f64 * 0.1;
            let a = jet_at(&curve, t - 1e-10); let b = jet_at(&curve, t + 1e-10);
            for derivative in 0..3 { for axis in 0..2 { assert!((a[derivative][axis] - b[derivative][axis]).abs() < 0.1, "{i} {derivative}: {a:?} {b:?}"); } }
            for part in 0..10 {
                let begin = t + part as f64 * 0.01; let end = begin + 0.01;
                let bound = curve.acceleration_bound(begin, end);
                for j in 0..=10 { let jet = jet_at(&curve, begin + (end - begin) * j as f64 / 10.); assert!(jet[2][0].hypot(jet[2][1]) <= bound + 0.001, "{begin} {end}: {jet:?}, bound {bound}"); }
            }
        }
    }
    #[test]
    fn single_point_has_no_completion_horizon() {
        let curve = Curve::new(point(10., 20., 1., 12345));
        assert_eq!(curve.end_time(), 0.012345);
        assert_eq!(curve.sample(curve.end_time()).0, curve.latest().position);
    }
}
