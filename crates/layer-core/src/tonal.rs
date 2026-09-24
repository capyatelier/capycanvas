//! Tonal selection recipes. Stops measure linear luminance relative to document
//! reference white, independently of the monitor, proof, and source encoding.
use serde::{Deserialize, Serialize};

pub const MAX_BANDS: usize = 16;
pub const MIN_STOP: f32 = -149.;
pub const MAX_STOP: f32 = 128.;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TonalBand {
    pub name: String,
    pub lower: Option<f32>,
    pub upper: Option<f32>,
    pub falloff: [f32; 2],
}
impl TonalBand {
    pub fn defaults() -> Vec<Self> {
        [
            ("Shadows", None, Some(-5.)),
            ("Mid-shadows", Some(-5.), Some(-3.5)),
            ("Midtones", Some(-3.5), Some(-1.5)),
            ("Mid-highlights", Some(-1.5), Some(-0.5)),
            ("Highlights", Some(-0.5), None),
            ("Deep shadows", None, Some(-7.)),
            ("Bright HDR", Some(1.), None),
        ]
        .into_iter()
        .map(|(name, lower, upper)| Self {
            name: name.into(),
            lower,
            upper,
            falloff: [0.5; 2],
        })
        .collect()
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err("Name the tonal band (up to 128 bytes)");
        }
        if self
            .lower
            .into_iter()
            .chain(self.upper)
            .any(|v| !v.is_finite() || !(MIN_STOP..=MAX_STOP).contains(&v))
            || self.lower.zip(self.upper).is_some_and(|(a, b)| a > b)
            || self
                .falloff
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=16.).contains(v))
        {
            return Err("Invalid tonal range or falloff");
        }
        Ok(())
    }
    /// Independent scalar reference; GPU masks evaluate the same continuous curve.
    pub fn coverage(&self, luminance: f64) -> f64 {
        let stop = if luminance > 0. {
            luminance.log2()
        } else {
            -1000.
        };
        let ramp = |distance: f64, width: f32| {
            if distance >= 0. {
                1.
            } else if width == 0. {
                0.
            } else {
                let t = (1. + distance / f64::from(width)).clamp(0., 1.);
                t * t * (3. - 2. * t)
            }
        };
        self.lower
            .map_or(1., |v| ramp(stop - f64::from(v), self.falloff[0]))
            * self
                .upper
                .map_or(1., |v| ramp(f64::from(v) - stop, self.falloff[1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tonal_bands_cover_adjacent_ranges_and_unbounded_hdr() {
        let bands = TonalBand::defaults();
        for b in &bands {
            b.validate().unwrap();
        }
        for stop in [-140., -7., -5., -4., -3.5, -2.47, -1.5, -0.5, 0., 2.3, 100.] {
            assert_eq!(
                bands[..5]
                    .iter()
                    .map(|b| b.coverage(2f64.powf(stop)))
                    .fold(0., f64::max),
                1.
            );
        }
        assert_eq!(bands[0].coverage(0.), 1.);
        assert_eq!(bands[4].coverage(0.), 0.);
        assert!((bands[4].coverage(2f64.powf(-0.75)) - 0.5).abs() < 1e-12);
        assert_eq!(bands[4].coverage(1024.), 1.);
        let mut invalid = bands[0].clone();
        invalid.falloff[0] = f32::NAN;
        assert!(invalid.validate().is_err());
    }
}
