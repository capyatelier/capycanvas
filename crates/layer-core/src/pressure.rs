use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<[f32; 2]>", into = "Vec<[f32; 2]>")]
pub struct PressureResponse {
    points: Vec<[f32; 2]>,
}

impl Default for PressureResponse {
    fn default() -> Self {
        Self { points: vec![[0., 0.125], [0.25, 1.], [1., 1.]] }
    }
}

impl TryFrom<Vec<[f32; 2]>> for PressureResponse {
    type Error = &'static str;
    fn try_from(points: Vec<[f32; 2]>) -> Result<Self, Self::Error> {
        if !(2..=32).contains(&points.len())
            || points.iter().flatten().any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || points[0][0] != 0. || points.last().unwrap() != &[1., 1.]
            || points.windows(2).any(|pair| pair[0][0] > pair[1][0] || pair[0][1] > pair[1][1])
        { return Err("Invalid pressure control points"); }
        Ok(Self { points })
    }
}

impl From<PressureResponse> for Vec<[f32; 2]> {
    fn from(response: PressureResponse) -> Self { response.points }
}

impl PressureResponse {
    pub fn linear() -> Self { Self { points: vec![[0., 0.], [1., 1.]] } }
    pub fn points(&self) -> &[[f32; 2]] { &self.points }
    pub fn set_point(&mut self, index: usize, mut point: [f32; 2]) -> bool {
        if index >= self.points.len() || point.iter().any(|v| !v.is_finite()) { return false; }
        if index + 1 == self.points.len() { return false; }
        let lower = if index == 0 { [0., 0.] } else { self.points[index - 1] };
        let upper = self.points[index + 1];
        for axis in 0..2 { point[axis] = point[axis].clamp(lower[axis], upper[axis]); }
        if index == 0 { point[0] = 0.; }
        if self.points[index] == point { return false; }
        self.points[index] = point; true
    }
    pub fn insert(&mut self, point: [f32; 2]) -> Option<usize> {
        if self.points.len() == 32 || point.iter().any(|v| !v.is_finite()) || !(0. ..1.).contains(&point[0]) { return None; }
        let index = self.points.partition_point(|p| p[0] <= point[0]).clamp(1, self.points.len() - 1);
        let point = [point[0], point[1].clamp(self.points[index - 1][1], self.points[index][1])];
        self.points.insert(index, point); Some(index)
    }
    pub fn remove(&mut self, index: usize) -> bool {
        if index == 0 || index + 1 >= self.points.len() { return false; }
        self.points.remove(index); true
    }
    pub fn evaluate(&self, t: f32) -> [f32; 2] {
        let t = if t.is_finite() { t.clamp(0., 1.) } else { 0. };
        if self.points.len() == 2 { return std::array::from_fn(|axis| self.points[0][axis] + (self.points[1][axis] - self.points[0][axis]) * t); }
        let spans = self.points.len() - 1;
        let span = ((t * spans as f32) as usize).min(spans - 1);
        let u = t * spans as f32 - span as f32;
        let control = self.points[span + 1];
        let start = if span == 0 { self.points[0] }
            else { std::array::from_fn(|axis| (self.points[span][axis] + control[axis]) * 0.5) };
        let end = self.points.get(span + 2).map_or(control, |next| std::array::from_fn(|axis| (control[axis] + next[axis]) * 0.5));
        std::array::from_fn(|axis| {
            let a = start[axis] + (control[axis] - start[axis]) * u;
            let b = control[axis] + (end[axis] - control[axis]) * u;
            a + (b - a) * u
        })
    }
    pub fn map(&self, input: f32) -> f32 {
        if !input.is_finite() { return self.points[0][1]; }
        let input = input.clamp(0., 1.);
        if self.points.len() == 2 { return self.points[0][1] + (1. - self.points[0][1]) * input; }
        if input == 0. { return self.points[0][1]; }
        if input == 1. { return 1.; }
        let (mut low, mut high) = (0., 1.);
        for _ in 0..24 {
            let t = (low + high) * 0.5;
            if self.evaluate(t)[0] < input { low = t; } else { high = t; }
        }
        self.evaluate((low + high) * 0.5)[1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn midpoint_spline_matches_quadratic_and_linear_functions() {
        let curve = PressureResponse::default();
        for i in 0..=100 {
            let t = i as f32 / 100.; let s = 1. - t;
            let expected = [2. * s * t * 0.25 + t * t * 0.625, s * s * 0.125 + 2. * s * t + t * t];
            let value = curve.evaluate(t * 0.5);
            for axis in 0..2 { assert!((value[axis] - expected[axis]).abs() < 1e-6); }
            assert_eq!(curve.evaluate(0.5 + t * 0.5)[1], 1.);
            assert!((PressureResponse::linear().map(t) - t).abs() < 1e-6);
        }
    }
    #[test]
    fn editing_preserves_bounded_monotone_response_and_fixed_endpoints() {
        let mut curve = PressureResponse::default();
        assert!(curve.set_point(1, [1., -1.]));
        assert!(!curve.set_point(curve.points().len()-1, [0., 0.]));
        assert!(!curve.set_point(0, [f32::NAN, 0.]));
        let index = curve.insert([0.3, 0.2]).unwrap();
        assert!(curve.remove(index));
        let mut previous = 0.;
        for i in 0..=1000 { let y = curve.map(i as f32 / 1000.); assert!(y >= previous && y <= 1.); previous = y; }
        assert_eq!(curve.points()[0][0], 0.);
        assert_eq!(curve.map(1.), 1.);
        assert!(serde_json::from_str::<PressureResponse>("[[0,0],[0.5,1],[1,0]]").is_err());
    }
    #[test]
    fn midpoint_joins_are_continuous_with_matching_tangents() {
        let curve = PressureResponse::try_from(vec![[0.,0.1],[0.2,0.5],[0.6,0.8],[1.,1.]]).unwrap();
        for (t,point) in [(1./3.,[0.4,0.65]),(2./3.,[0.8,0.9])] {
            let center=curve.evaluate(t); let before=curve.evaluate(t-0.0001); let after=curve.evaluate(t+0.0001);
            for axis in 0..2 {
                assert!((center[axis]-point[axis]).abs()<1e-6);
                assert!(((center[axis]-before[axis])-(after[axis]-center[axis])).abs()<1e-6);
            }
        }
    }
    #[test]
    fn single_middle_control_matches_observed_ipad_default() {
        let curve=PressureResponse::default();
        assert_eq!(curve.points(),&[[0.,0.125],[0.25,1.],[1.,1.]]);
        for (input,output) in [(0.1,0.4219),(0.2,0.6435),(0.25,0.7315),(0.4,0.9134),(0.5,0.9744),(0.6,1.)] {
            assert!((curve.map(input)-output).abs()<0.006);
        }
        assert_eq!(curve.map(0.625),1.);
    }
    #[test]
    fn soft_default_and_authored_serialization() {
        let curve = PressureResponse::default();
        assert_eq!(curve.map(0.), 0.125);
        assert!((0.95..=1.).contains(&curve.map(0.5)));
        let json = serde_json::to_string(&curve).unwrap();
        assert_eq!(json, "[[0.0,0.125],[0.25,1.0],[1.0,1.0]]");
        assert_eq!(serde_json::from_str::<PressureResponse>(&json).unwrap(), curve);
    }
}
