//! Portable two-axis parameter pad. Hosts own drawing and pointer capture;
//! normalized coordinates, labels, ranges and defaults are shared policy.
use crate::NumericControl;
use serde::Serialize;
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
        assert!((pad.defaults()[0] - f64::from(recipe.tone)).abs() < 1e-6);
        assert_eq!(pad.defaults()[1], f64::from(recipe.detail));
    }
}
