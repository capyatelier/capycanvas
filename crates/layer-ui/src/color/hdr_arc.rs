//! Shared geometry for the intensity arc; hosts own input capture and timing.
use super::ColorPanelLayout;

#[derive(Clone, Copy, Debug)]
pub struct HdrIntensityArc {
    pub center: [f32; 2],
    pub radius: f32,
    pub width: f32,
}
impl HdrIntensityArc {
    pub fn new(size: f32) -> Option<Self> {
        let layout = ColorPanelLayout::new(size)?;
        let width = layout.wheel[2] * 0.11;
        Some(Self {
            center: [size * 0.5; 2],
            radius: layout.wheel[2] * 0.49 + 5. + width * 0.5,
            width,
        })
    }
    pub fn point(&self, fraction: f32) -> [f32; 2] {
        let a = (145. - fraction.clamp(0., 1.) * 110.).to_radians();
        [
            self.center[0] + self.radius * a.cos(),
            self.center[1] + self.radius * a.sin(),
        ]
    }
    /// Clockwise along the lower arc, clamped to the nearest endpoint off-track.
    pub fn fraction(&self, point: [f32; 2]) -> f32 {
        let x = point[0] - self.center[0];
        let y = point[1] - self.center[1];
        let a = y.max(0.).atan2(x).to_degrees();
        ((145. - a) / 110.).clamp(0., 1.)
    }
    pub fn contains(&self, point: [f32; 2]) -> bool {
        let p = self.point(self.fraction(point));
        (point[0] - p[0]).hypot(point[1] - p[1]) <= self.width * 0.5 + 3.
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arc_hits_follow_angles_and_leave_wheel_and_footer_controls_clear() {
        for size in [128., 144., 226., 264., 400.] {
            let arc = HdrIntensityArc::new(size).unwrap();
            let layout = ColorPanelLayout::with_hdr(size).unwrap();
            assert!((arc.width - layout.wheel[2] * 0.11).abs() < 1e-5);
            assert!(!arc.contains(arc.center));
            for i in 0..=100 {
                let t = i as f32 / 100.;
                let p = arc.point(t);
                assert!((arc.fraction(p) - t).abs() < 1e-5);
                assert!(arc.contains(p));
                assert!(p[0] >= arc.width / 2. && p[0] + arc.width / 2. <= size);
                for b in [
                    layout.foreground,
                    layout.background,
                    layout.transparent,
                    layout.swap,
                ] {
                    let distance = (p[0] - b[0] - b[2] / 2.).hypot(p[1] - b[1] - b[3] / 2.);
                    assert!(
                        distance > (arc.width + b[2]) / 2.,
                        "size={size} {p:?} {b:?}"
                    );
                }
            }
        }
    }
}
