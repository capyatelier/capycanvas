use super::{PositionSmoother, curve::Curve};
use layer_core::{BrushStabilization, Point, StrokePoint};

pub(crate) trait PositionField {
    fn sample(&self, time: f64) -> (Point, f32);
    fn acceleration_bound(&self, begin: f64, end: f64) -> f64;
}

#[derive(Clone, Debug)]
pub(crate) struct CurveModel { curve: Curve, real_through: u32 }
impl CurveModel {
    pub(super) fn from_smoother(mut smoother: PositionSmoother, settings: &BrushStabilization, real: &[StrokePoint], ending: bool) -> Option<Self> {
        if settings.strength <= 0. || real.last().is_none_or(|point| point.elapsed_micros == 0) { return None; }
        let begin = smoother.previous().map_or(0, |last| real.partition_point(|point| point.elapsed_micros <= last.elapsed_micros));
        for (index, &point) in real.iter().enumerate().skip(begin) {
            let point = if ending && index + 1 == real.len() { smoother.lift(point) } else { point };
            smoother.append(point, settings);
        }
        Some(Self { curve: smoother.curve?, real_through: real.last()?.elapsed_micros })
    }
    pub(super) fn completion(curve: Curve) -> Self {
        let real_through = curve.latest().elapsed_micros;
        Self { curve, real_through }
    }
    pub(crate) fn endpoint(&self) -> StrokePoint {
        StrokePoint { elapsed_micros: (self.curve.end_time() * 1e6).ceil() as u32, ..self.curve.latest() }
    }
}
impl PositionField for CurveModel {
    fn sample(&self, time: f64) -> (Point, f32) { self.curve.sample(time) }
    fn acceleration_bound(&self, begin: f64, end: f64) -> f64 { self.curve.acceleration_bound(begin, end) }
}

fn progress(u: f64) -> f64 {
    let u = u.clamp(0., 1.);
    u.powi(3) * (10. + u * (-15. + 6. * u))
}
fn reference_delay(strength: f64) -> f64 {
    let radius = 805. / 286. * strength * 0.1;
    let alpha = 0.10026573857943435 * (1. - strength).powi(2);
    let memory = 0.032 * strength;
    let beta = |n: u32| (1..=n).fold(1., |v, i| v * f64::from(i + 2) / f64::from(i + 6));
    let polynomial: [(u32, f64); 6] = [(0, -35. / 512.), (1, 0.5), (2, -105. / 128.), (4, 175. / 256.), (6, -49. / 128.), (8, 45. / 512.)];
    let mut mean_moment = 0.;
    for (power, coefficient) in polynomial {
        let mut choose = 1.;
        for k in 0..=power {
            mean_moment += coefficient * choose * alpha.powi((power - k) as i32) * (memory / radius).powi(k as i32) * beta(k);
            choose *= f64::from(power - k) / f64::from(k + 1);
        }
    }
    3. / 7. * memory - radius * mean_moment
}

#[derive(Clone, Debug)]
struct Display { canonical: CurveModel, continuation: Curve, endpoint: f64 }
#[derive(Clone, Debug)]
struct Landing { begin: u32, endpoint: f64, taper: f32 }
#[derive(Clone, Debug, Default)]
pub(crate) struct CurvePreview { current: Option<Display>, landing: Option<Landing>, now: u32 }

pub(crate) fn pen_up_settlement_micros(settings: &BrushStabilization) -> u32 {
    if settings.strength > 0. { 32_000 } else { 0 }
}
impl CurvePreview {
    pub(crate) fn is_present(&self) -> bool { self.current.is_some() }
    pub(crate) fn taper_amount(&self) -> f32 {
        self.landing.as_ref().map_or(1., |landing| landing.taper * (1. - progress(f64::from(self.now.saturating_sub(landing.begin)) / 32_000.)) as f32)
    }
    pub(crate) fn observe(&mut self, now: u32, model: CurveModel, settings: &BrushStabilization, forecast: &[StrokePoint]) {
        self.now = now.max(self.now);
        let strength = f64::from(settings.strength);
        let delay = reference_delay(strength);
        let lead = (1. - delay / 0.008).clamp(0., 1.);
        let support = model.curve.support();
        let latest = model.curve.latest();
        let mut continuation = model.curve.clone();
        let horizon = forecast.last().map_or(0, |point| point.elapsed_micros.saturating_sub(latest.elapsed_micros));
        let through = latest.elapsed_micros.saturating_add((f64::from(horizon) * lead).round() as u32);
        let mut previous = latest;
        for &point in forecast {
            if point.elapsed_micros <= previous.elapsed_micros { continue; }
            let endpoint = if point.elapsed_micros <= through { point } else {
                let u = f64::from(through.saturating_sub(previous.elapsed_micros)) / f64::from(point.elapsed_micros - previous.elapsed_micros);
                StrokePoint { position: Point { x: previous.position.x + (point.position.x - previous.position.x) * u as f32,
                    y: previous.position.y + (point.position.y - previous.position.y) * u as f32 }, elapsed_micros: through, ..point }
            };
            if endpoint.elapsed_micros > previous.elapsed_micros { continuation.append(endpoint, support); }
            previous = endpoint;
            if point.elapsed_micros >= through { break; }
        }
        let gap = (delay - 0.008).max(0.) / support;
        let mut lo: f64 = 0.; let mut hi: f64 = 1.;
        for _ in 0..28 { let r = (lo + hi) * 0.5; if r.powi(5) * (1. - 2. * r / 3.) < gap { lo = r; } else { hi = r; } }
        let endpoint = continuation.end_time() - support * (lo + hi) * 0.5;
        self.current = Some(Display { canonical: model, continuation, endpoint });
        self.landing = None;
    }
    pub(crate) fn land(&mut self, now: u32, begin: u32, model: CurveModel) {
        let taper = self.taper_amount();
        self.now = now.max(self.now);
        let previous = self.current.as_ref().map_or(model.curve.end_time(), |display| display.endpoint);
        if self.landing.as_ref().is_none_or(|landing| landing.begin != begin) {
            self.landing = Some(Landing { begin, endpoint: previous, taper });
        }
        let landing = self.landing.as_ref().unwrap();
        let end = model.curve.end_time();
        let start = landing.endpoint.clamp(f64::from(model.real_through) * 1e-6, end);
        let endpoint = start + (end - start) * progress(f64::from(self.now.saturating_sub(begin)) / 32_000.);
        self.current = Some(Display { continuation: model.curve.clone(), canonical: model, endpoint });
    }
    pub(crate) fn endpoint(&self, latest: StrokePoint) -> Option<StrokePoint> {
        let display = self.current.as_ref()?;
        let elapsed_micros = (display.endpoint * 1e6).ceil() as u32;
        (elapsed_micros > latest.elapsed_micros).then(|| {
            let (position, pressure) = self.sample(f64::from(elapsed_micros) * 1e-6);
            StrokePoint { position, pressure, elapsed_micros, ..latest }
        })
    }
}
impl PositionField for CurvePreview {
    fn sample(&self, time: f64) -> (Point, f32) {
        let display = self.current.as_ref().unwrap();
        if time * 1e6 <= f64::from(display.canonical.real_through) { display.canonical.sample(time) } else { display.continuation.sample(time) }
    }
    fn acceleration_bound(&self, begin: f64, end: f64) -> f64 {
        let display = self.current.as_ref().unwrap();
        display.continuation.acceleration_bound(begin, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::point;
    #[test]
    fn positive_continuation_has_no_extra_overshoot_and_completes_pen_up() {
        for strength in [0.06,0.5,1.] {
            let settings=BrushStabilization { strength,..Default::default() };
            let real:Vec<_>=(0..=100).map(|i| point(i as f32*2., (i as f32*0.04).sin()*30.,1.,i*4000)).collect();
            let model=CurveModel::from_smoother(PositionSmoother::default(),&settings,&real,false).unwrap();
            let mut preview=CurvePreview::default();
            preview.observe(400000,model.clone(),&settings,&[point(204.,-24.,1.,408000)]);
            let end=preview.endpoint(*real.last().unwrap()).unwrap();
            for i in 0..100 { let t=0.4+(f64::from(end.elapsed_micros)*1e-6-0.4)*i as f64/99.; let p=preview.sample(t).0; assert!(p.x<=204.0001 && p.x>=0. && p.y.abs()<=30.0001); }
            preview.land(400000,400000,model.clone());
            preview.land(416000,400000,model.clone());
            let taper = preview.taper_amount();
            preview.land(416000,416000,model.clone());
            assert_eq!(preview.taper_amount(), taper);
            preview.land(448000,416000,model);
            assert_eq!(preview.taper_amount(), 0.);
            assert_eq!(preview.endpoint(*real.last().unwrap()).unwrap().position,real.last().unwrap().position);
        }
    }
    #[test]
    fn preview_join_preserves_position_tangent_and_curvature() {
        let settings=BrushStabilization { strength:0.06,..Default::default() };
        let real:Vec<_>=(0..=100).map(|i| point(i as f32*2.,(i as f32*0.05).sin()*20.,1.,i*4000)).collect();
        let model=CurveModel::from_smoother(PositionSmoother::default(),&settings,&real,false).unwrap();
        let mut preview=CurvePreview::default();preview.observe(400000,model,&settings,&[point(204.,-15.,1.,408000)]);
        let display=preview.current.as_ref().unwrap();
        let a=super::super::curve::tests::jet_at(&display.canonical.curve,0.4);let b=super::super::curve::tests::jet_at(&display.continuation,0.4);
        for derivative in 0..3 { for axis in 0..2 { assert!((a[derivative][axis]-b[derivative][axis]).abs()<1e-7); } }
    }
}
