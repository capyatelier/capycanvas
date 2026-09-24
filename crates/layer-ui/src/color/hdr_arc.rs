//! Shared geometry for the intensity arc; hosts own input capture and timing.
use super::{ColorPanelLayout, ColorWheelGeometry};

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct HdrIntensityArc {
    pub center: [f32; 2],
    pub radius: f32,
    pub width: f32,
    pub marker_radius: f32,
    end_angle: f32,
}
impl HdrIntensityArc {
    pub fn new(size: f32) -> Option<Self> {
        let layout = ColorPanelLayout::new(size)?;
        let wheel = ColorWheelGeometry::new(layout.wheel[2])?;
        let width = wheel.outer - wheel.inner;
        let gap = wheel.inner - wheel.disc_radius();
        let radius = wheel.outer + gap + width * 0.5;
        // Keep the round caps inside the panel at large widths as the gap grows.
        let end_angle = 43f32
            .to_radians()
            .max(((size - width) * 0.5 / radius).clamp(0., 1.).acos());
        Some(Self {
            center: [size * 0.5; 2],
            radius,
            width,
            marker_radius: wheel.marker_radius(),
            end_angle,
        })
    }
    pub fn point(&self, fraction: f32) -> [f32; 2] {
        let a = std::f32::consts::PI
            - self.end_angle
            - fraction.clamp(0., 1.) * (std::f32::consts::PI - self.end_angle * 2.);
        [
            self.center[0] + self.radius * a.cos(),
            self.center[1] + self.radius * a.sin(),
        ]
    }
    /// Clockwise along the lower arc, clamped to the nearest endpoint off-track.
    pub fn fraction(&self, point: [f32; 2]) -> f32 {
        let x = point[0] - self.center[0];
        let y = point[1] - self.center[1];
        let a = y.max(0.).atan2(x);
        ((std::f32::consts::PI - self.end_angle - a) / (std::f32::consts::PI - self.end_angle * 2.))
            .clamp(0., 1.)
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
        for size in [128., 144., 226., 264., 400., 1024.] {
            let arc = HdrIntensityArc::new(size).unwrap();
            let layout = ColorPanelLayout::with_hdr(size).unwrap();
            let sdr = ColorPanelLayout::new(size).unwrap();
            let wheel = ColorWheelGeometry::new(sdr.wheel[2]).unwrap();
            assert_eq!(arc.width, wheel.outer - wheel.inner);
            assert!(
                (arc.radius - arc.width * 0.5 - wheel.outer - (wheel.inner - wheel.disc_radius()))
                    .abs()
                    < 1e-4
            );
            assert_eq!(arc.marker_radius, wheel.marker_radius());
            let clearance = |b: [f32; 4], outer: f32| {
                (b[0] + b[2] * 0.5 - arc.center[0]).hypot(b[1] + b[3] * 0.5 - arc.center[1])
                    - b[2] * 0.5
                    - outer
            };
            for (old, new) in [sdr.foreground, sdr.background]
                .into_iter()
                .zip([layout.foreground, layout.background])
            {
                assert!(
                    (clearance(old, wheel.outer) - clearance(new, arc.radius + arc.width * 0.5))
                        .abs()
                        < 1e-4
                );
                assert!(new[1] + new[3] <= layout.height());
            }
            assert_eq!(layout.transparent[1], layout.foreground[1]);
            assert!(!arc.contains(arc.center));
            for i in 0..=100 {
                let t = i as f32 / 100.;
                let p = arc.point(t);
                assert!((arc.fraction(p) - t).abs() < 1e-5);
                assert!(arc.contains(p));
                assert!(p[0] >= arc.width / 2. - 1e-4 && p[0] + arc.width / 2. <= size + 1e-4);
                for b in [
                    layout.foreground,
                    layout.background,
                    layout.transparent,
                    layout.black,
                    layout.white,
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
