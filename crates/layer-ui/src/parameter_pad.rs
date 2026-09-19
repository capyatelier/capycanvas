//! Portable two-axis parameter pad. Hosts own drawing and pointer capture;
//! normalized coordinates, labels, ranges and defaults are shared policy.
use crate::NumericControl;
use serde::Serialize;

/// Circular host layout, shared with the color picker's proportions and its
/// square-to-disc mapping so every combination remains reachable on the rim.
#[derive(Clone, Copy, Debug)]
pub struct ParameterDialGeometry {
    pub field: crate::ColorWheelGeometry,
    pub arcs: [ParameterArc; 2],
    pub reset: [f32; 4],
}
/// Percent baseline and a passive symbolic icon. Arc percentages follow their
/// track; side percentages are horizontal, with the icon above.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParameterDialReadout {
    pub icon: [f32; 4],
    pub text: [f32; 2],
    pub curve: Option<(f32, f32, bool)>, // radius, degrees, reverse
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParameterArc {
    pub center: [f32; 2],
    pub radius: f32,
    pub width: f32,
    pub marker_radius: f32,
    pub start: f32,
    pub sweep: f32,
}
impl ParameterDialGeometry {
    pub fn text_size(size: f32) -> f32 {
        (size * 0.044).clamp(9., 12.)
    }
    pub fn readouts(&self, size: f32) -> [ParameterDialReadout; 4] {
        let [cx, cy] = self.field.center;
        let font = Self::text_size(size);
        let icon = font + 1.;
        let side_x = (cx - self.field.disc_radius() - self.arcs[0].marker_radius - 3.) * 0.5;
        let side = |x| ParameterDialReadout {
            icon: [x - icon * 0.5, cy - icon - 3., icon, icon],
            text: [x, cy + font],
            curve: None,
        };
        let arc = |index: usize| {
            let a = self.arcs[index];
            let outside = a.radius + (a.width * 0.5).max(a.marker_radius);
            let radius = outside + if index == 0 { 5. } else { 4. + font };
            let angle = (8. / radius).asin().to_degrees();
            let degrees = if index == 0 {
                -90. + angle
            } else {
                90. - angle
            };
            let text = [
                cx + radius * degrees.to_radians().cos(),
                cy + radius * degrees.to_radians().sin(),
            ];
            ParameterDialReadout {
                icon: [cx - 17. - icon * 0.5, text[1] - icon + 1., icon, icon],
                text,
                curve: Some((radius, degrees, index == 1)),
            }
        };
        [side(side_x), side(size - side_x), arc(0), arc(1)]
    }
    pub fn new(size: f32) -> Option<Self> {
        let layout = crate::ColorPanelLayout::new(size)?;
        let mut field = crate::ColorWheelGeometry::new(layout.wheel[2])?;
        field.center = [size * 0.5; 2];
        let arc = |start: f32, sweep: f32| ParameterArc {
            center: field.center,
            radius: (field.outer + field.inner) * 0.5,
            width: field.outer - field.inner,
            marker_radius: field.marker_radius(),
            start: start.to_radians(),
            sweep: sweep.to_radians(),
        };
        Some(Self {
            field,
            arcs: [arc(-150., 120.), arc(150., -120.)],
            reset: layout.edit,
        })
    }
}
impl ParameterArc {
    pub fn point(&self, fraction: f32) -> [f32; 2] {
        let angle = self.start + fraction.clamp(0., 1.) * self.sweep;
        [
            self.center[0] + self.radius * angle.cos(),
            self.center[1] + self.radius * angle.sin(),
        ]
    }
    pub fn fraction(&self, point: [f32; 2]) -> f32 {
        let angle = (point[1] - self.center[1]).atan2(point[0] - self.center[0]);
        let mid = self.start + self.sweep * 0.5;
        let delta = (angle - mid + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        (0.5 + delta / self.sweep).clamp(0., 1.)
    }
    pub fn contains(&self, point: [f32; 2]) -> bool {
        let p = self.point(self.fraction(point));
        (point[0] - p[0]).hypot(point[1] - p[1]) <= self.width * 0.5 + 4.
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ParameterPadAxis {
    pub key: &'static str,
    pub label: &'static str,
    pub numeric: NumericControl,
    pub default: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ParameterPadSpec {
    pub axes: [ParameterPadAxis; 2],
}
impl ParameterPadSpec {
    pub fn values(&self, fraction: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|i| {
            let s = &self.axes[i].numeric;
            if fraction[i].is_nan() {
                return self.axes[i].default;
            }
            let v = fraction[i].clamp(0., 1.) * (s.max - s.min);
            (v / s.step)
                .round()
                .mul_add(s.step, s.min)
                .clamp(s.min, s.max)
        })
    }
    pub fn fractions(&self, values: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|i| {
            let s = &self.axes[i].numeric;
            let value = if values[i].is_nan() {
                self.axes[i].default
            } else {
                values[i]
            };
            ((value - s.min) / (s.max - s.min)).clamp(0., 1.)
        })
    }
    pub fn defaults(&self) -> [f64; 2] {
        self.axes.each_ref().map(|a| a.default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dial_arcs_and_circle_have_disjoint_hits_and_reachable_extremes() {
        for size in [128., 160., 196., 226., 400.] {
            let g = ParameterDialGeometry::new(size).unwrap();
            for (i, arc) in g.arcs.iter().enumerate() {
                assert!((arc.sweep.abs().to_degrees() - 120.).abs() < 1e-4);
                assert!(!arc.contains(g.field.center));
                assert!(arc.radius - arc.width * 0.5 > g.field.disc_radius());
                for j in 0..=100 {
                    let t = j as f32 / 100.;
                    let p = arc.point(t);
                    assert!((arc.fraction(p) - t).abs() < 1e-5);
                    assert!(arc.contains(p) && !g.arcs[1 - i].contains(p));
                    assert!(p[0] - arc.width * 0.5 >= 0. && p[0] + arc.width * 0.5 <= size);
                }
            }
            for x in [0., 0.5, 1.] {
                for y in [0., 0.5, 1.] {
                    let p = g.field.disc_marker([x, y]);
                    let v = g.field.disc_components(p);
                    assert!((v[0] - x).abs() < 0.0005 && (v[1] - y).abs() < 0.0005);
                }
            }
        }
    }
    #[test]
    fn axes_snap_from_their_own_minimum_and_bound_pointer_input() {
        let axis = ParameterPadAxis {
            key: "x",
            label: "X",
            numeric: NumericControl::number(1., 9., 2., 0),
            default: 5.,
        };
        let pad = ParameterPadSpec {
            axes: [axis.clone(), axis],
        };
        assert_eq!(pad.values([0., 1.]), [1., 9.]);
        assert_eq!(pad.values([0.3, 0.6]), [3., 5.]);
        assert_eq!(pad.values([f64::NAN, f64::INFINITY]), [5., 9.]);
        assert_eq!(pad.fractions([f64::NAN, -10.]), [0.5, 0.]);
        let pad = crate::proof_panel::sdr_tone_pad();
        let recipe = layer_core::color::hdr::SdrRendition::default();
        assert_eq!(
            crate::proof_panel::sdr_from_pad(recipe, pad.defaults()),
            recipe
        );
    }
}
