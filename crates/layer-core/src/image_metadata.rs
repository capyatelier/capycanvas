//! Physical density is metadata, independent of pixel dimensions and color.
//! Rational values retain TIFF/Exif numbers exactly in native projects.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionUnit {
    Inch,
    Centimetre,
    Metre,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageResolution {
    pub unit: ResolutionUnit,
    /// X and Y each contain [numerator, denominator] pixels per declared unit.
    pub density: [[u32; 2]; 2],
}
impl ImageResolution {
    pub fn ppi(value: u32) -> Self {
        Self {
            unit: ResolutionUnit::Inch,
            density: [[value, 1]; 2],
        }
    }
    pub fn validate(self) -> Result<(), String> {
        if self.density.into_iter().flatten().any(|v| v == 0) {
            return Err("Image resolution must have positive numerators and denominators".into());
        }
        Ok(())
    }
    pub fn pixels_per_inch(self) -> [f64; 2] {
        let factor = match self.unit {
            ResolutionUnit::Inch => 1.,
            ResolutionUnit::Centimetre => 2.54,
            ResolutionUnit::Metre => 0.0254,
        };
        self.density
            .map(|[n, d]| f64::from(n) / f64::from(d) * factor)
    }
    pub fn swapped(self) -> Self {
        Self {
            density: [self.density[1], self.density[0]],
            ..self
        }
    }
    pub fn png_density(self) -> Result<[u32; 2], String> {
        self.validate()?;
        let (num, den) = match self.unit {
            ResolutionUnit::Inch => (5000u64, 127u64),
            ResolutionUnit::Centimetre => (100, 1),
            ResolutionUnit::Metre => (1, 1),
        };
        let mut result = [0; 2];
        for (out, [n, d]) in result.iter_mut().zip(self.density) {
            let d = u64::from(d) * den;
            let value = (u64::from(n) * num + d / 2) / d;
            *out = u32::try_from(value)
                .ok()
                .filter(|n| *n > 0)
                .ok_or("Resolution cannot be represented by PNG's pixels per metre")?;
        }
        Ok(result)
    }
    /// TIFF and Exif use the same inch/centimetre unit codes and rational tags.
    pub fn tiff_density(self) -> Result<(u16, [[u32; 2]; 2]), String> {
        self.validate()?;
        match self.unit {
            ResolutionUnit::Inch => Ok((2, self.density)),
            ResolutionUnit::Centimetre => Ok((3, self.density)),
            ResolutionUnit::Metre => {
                let mut density = self.density;
                for value in &mut density {
                    let (mut a, mut b) = (u64::from(value[0]), u64::from(value[1]) * 100);
                    let (mut n, mut d) = (a, b);
                    while d != 0 {
                        (n, d) = (d, n % d);
                    }
                    a /= n;
                    b /= n;
                    *value = [
                        a as u32,
                        u32::try_from(b)
                            .map_err(|_| "Resolution fraction exceeds TIFF/Exif limits")?,
                    ];
                }
                Ok((3, density))
            }
        }
    }
    pub fn jfif_density(self) -> Result<(u8, [u16; 2]), String> {
        let (unit, density) = self.tiff_density()?;
        let mut result = [0; 2];
        for (out, [n, d]) in result.iter_mut().zip(density) {
            let value = (u64::from(n) + u64::from(d) / 2) / u64::from(d);
            *out = u16::try_from(value)
                .ok()
                .filter(|v| *v > 0)
                .ok_or("Resolution cannot be represented by JPEG's whole-unit density")?;
        }
        Ok((unit as u8 - 1, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn density_units_preserve_rationals_and_round_only_for_integer_containers() {
        let inch = ImageResolution {
            unit: ResolutionUnit::Inch,
            density: [[601, 2], [150, 1]],
        };
        assert_eq!(inch.pixels_per_inch(), [300.5, 150.]);
        assert_eq!(inch.tiff_density().unwrap(), (2, inch.density));
        assert_eq!(inch.jfif_density().unwrap(), (1, [301, 150]));
        assert_eq!(ImageResolution::ppi(300).png_density().unwrap(), [11811; 2]);
        let metre = ImageResolution {
            unit: ResolutionUnit::Metre,
            density: [[11811, 1], [5906, 1]],
        };
        assert_eq!(metre.png_density().unwrap(), [11811, 5906]);
        assert_eq!(
            metre.tiff_density().unwrap(),
            (3, [[11811, 100], [2953, 50]])
        );
        assert_eq!(inch.swapped().swapped(), inch);
        assert!(ImageResolution::ppi(0).validate().is_err());
        assert!(ImageResolution::ppi(u32::MAX).png_density().is_err());
        assert!(ImageResolution::ppi(65536).jfif_density().is_err());
        let encoded = serde_json::to_vec(&inch).unwrap();
        assert_eq!(
            serde_json::from_slice::<ImageResolution>(&encoded).unwrap(),
            inch
        );
    }
}
