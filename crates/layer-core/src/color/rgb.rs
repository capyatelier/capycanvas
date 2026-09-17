//! Standard SDR RGB definitions. Profile transfer, primaries, and integer depth
//! are independent. Matrices and white adaptation never clamp extended values.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RgbSpace {
    #[default]
    Srgb,
    DisplayP3,
    AdobeRgb,
    ProPhoto,
}

pub type Matrix3 = [[f64; 3]; 3];
impl RgbSpace {
    pub const ALL: [Self; 4] = [Self::Srgb, Self::DisplayP3, Self::AdobeRgb, Self::ProPhoto];

    pub fn name(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::DisplayP3 => "Display P3",
            Self::AdobeRgb => "Adobe RGB (1998)",
            Self::ProPhoto => "ProPhoto RGB",
        }
    }

    pub fn primaries(self) -> [[f64; 2]; 3] {
        match self {
            Self::Srgb => [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]],
            Self::DisplayP3 => [[0.68, 0.32], [0.265, 0.69], [0.15, 0.06]],
            Self::AdobeRgb => [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06]],
            Self::ProPhoto => [[0.7347, 0.2653], [0.1596, 0.8404], [0.0366, 0.0001]],
        }
    }

    pub fn white(self) -> [f64; 2] {
        if self == Self::ProPhoto {
            [0.3457, 0.3585]
        } else {
            [0.3127, 0.3290]
        }
    }

    pub fn decode(self, value: f64) -> f64 {
        let magnitude = value.abs();
        let linear = match self {
            Self::Srgb | Self::DisplayP3 => {
                if magnitude <= 0.04045 {
                    magnitude / 12.92
                } else {
                    ((magnitude + 0.055) / 1.055).powf(2.4)
                }
            }
            Self::AdobeRgb => magnitude.powf(563. / 256.),
            Self::ProPhoto => {
                if magnitude <= 1. / 32. {
                    magnitude / 16.
                } else {
                    magnitude.powf(1.8)
                }
            }
        };
        linear.copysign(value)
    }

    pub fn encode(self, value: f64) -> f64 {
        let magnitude = value.abs();
        let encoded = match self {
            Self::Srgb | Self::DisplayP3 => {
                if magnitude <= 0.0031308 {
                    magnitude * 12.92
                } else {
                    1.055 * magnitude.powf(1. / 2.4) - 0.055
                }
            }
            Self::AdobeRgb => magnitude.powf(256. / 563.),
            Self::ProPhoto => {
                if magnitude <= 1. / 512. {
                    magnitude * 16.
                } else {
                    magnitude.powf(1. / 1.8)
                }
            }
        };
        encoded.copysign(value)
    }

    pub fn to_xyz(self) -> Matrix3 {
        let p = self.primaries().map(xy_to_xyz);
        let unscaled = std::array::from_fn(|row| std::array::from_fn(|col| p[col][row]));
        let scale = apply(inverse(unscaled), xy_to_xyz(self.white()));
        std::array::from_fn(|row| std::array::from_fn(|col| unscaled[row][col] * scale[col]))
    }

    /// Bradford adaptation between each space's reference whites, then primary
    /// conversion. Suitable for premultiplied linear RGB; alpha is untouched.
    pub fn linear_transform(self, destination: Self) -> Matrix3 {
        if self == destination {
            return [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        }
        let bradford = [
            [0.8951, 0.2664, -0.1614],
            [-0.7502, 1.7135, 0.0367],
            [0.0389, -0.0685, 1.0296],
        ];
        let source_white = apply(bradford, xy_to_xyz(self.white()));
        let target_white = apply(bradford, xy_to_xyz(destination.white()));
        let adapted = std::array::from_fn(|row| {
            std::array::from_fn(|col| bradford[row][col] * target_white[row] / source_white[row])
        });
        multiply(
            inverse(destination.to_xyz()),
            multiply(inverse(bradford), multiply(adapted, self.to_xyz())),
        )
    }

    /// Preserve absolute XYZ, including the source white, without adaptation.
    pub fn absolute_linear_transform(self, destination: Self) -> Matrix3 {
        if self == destination {
            return [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        }
        multiply(inverse(destination.to_xyz()), self.to_xyz())
    }

    pub fn convert(self, destination: Self, encoded: [f64; 3]) -> [f64; 3] {
        if self == destination {
            return encoded;
        }
        apply(
            self.linear_transform(destination),
            encoded.map(|v| self.decode(v)),
        )
        .map(|v| destination.encode(v))
    }
}

pub fn apply(matrix: Matrix3, vector: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row.into_iter().zip(vector).map(|(a, b)| a * b).sum())
}
/// Linear primary conversion with Bradford white adaptation. Used for explicitly
/// tagged interchange spaces as well as the built-in document spaces.
pub fn linear_rgb_transform(primaries: [[f64; 2]; 3], white: [f64; 2], destination: RgbSpace) -> Matrix3 {
    let p = primaries.map(xy_to_xyz);
    let unscaled = std::array::from_fn(|row| std::array::from_fn(|col| p[col][row]));
    let scale = apply(inverse(unscaled), xy_to_xyz(white));
    let source = std::array::from_fn(|row| std::array::from_fn(|col| unscaled[row][col] * scale[col]));
    let bradford = [[0.8951,0.2664,-0.1614],[-0.7502,1.7135,0.0367],[0.0389,-0.0685,1.0296]];
    let sw = apply(bradford, xy_to_xyz(white));
    let dw = apply(bradford, xy_to_xyz(destination.white()));
    let adapted = std::array::from_fn(|row| std::array::from_fn(|col| bradford[row][col]*dw[row]/sw[row]));
    multiply(inverse(destination.to_xyz()), multiply(inverse(bradford), multiply(adapted, source)))
}
fn xy_to_xyz([x, y]: [f64; 2]) -> [f64; 3] {
    [x / y, 1., (1. - x - y) / y]
}
fn multiply(a: Matrix3, b: Matrix3) -> Matrix3 {
    std::array::from_fn(|row| {
        std::array::from_fn(|col| (0..3).map(|k| a[row][k] * b[k][col]).sum())
    })
}
pub(crate) fn inverse(m: Matrix3) -> Matrix3 {
    let cofactor: Matrix3 = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            m[(i + 1) % 3][(j + 1) % 3] * m[(i + 2) % 3][(j + 2) % 3]
                - m[(i + 1) % 3][(j + 2) % 3] * m[(i + 2) % 3][(j + 1) % 3]
        })
    });
    let determinant: f64 = (0..3).map(|j| m[0][j] * cofactor[0][j]).sum();
    std::array::from_fn(|row| std::array::from_fn(|col| cofactor[col][row] / determinant))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integer16_transfer_round_trips_every_code_without_half_float() {
        for space in RgbSpace::ALL {
            for code in 0..=65535u32 {
                let value = f64::from(code) / 65535.;
                // Include the actual Float32 arithmetic/storage boundary used
                // by GPU working values, rather than testing only Float64.
                let decoded = space.decode(value) as f32;
                assert_eq!(
                    (space.encode(f64::from(decoded)) * 65535.).round() as u32,
                    code,
                    "{space:?}"
                );
            }
        }
    }
    #[test]
    fn d50_d65_adaptation_preserves_neutrals_and_extended_p3() {
        for source in RgbSpace::ALL {
            for destination in RgbSpace::ALL {
                let white = source.convert(destination, [1.; 3]);
                assert!(white.into_iter().all(|v| (v - 1.).abs() < 1e-12));
                for value in [[0.; 3], [0.04, 0.2, 0.8], [-0.25, 1.25, 0.5]] {
                    let back = destination.convert(source, source.convert(destination, value));
                    assert!(
                        back.into_iter()
                            .zip(value)
                            .all(|(a, b)| (a - b).abs() < 1e-10)
                    );
                }
            }
        }
        let p3_red = RgbSpace::DisplayP3.convert(RgbSpace::Srgb, [1., 0., 0.]);
        assert!(p3_red[0] > 1. && p3_red[1] < 0. && p3_red[2] < 0.);
        // Independent published sRGB luminance coefficients, rounded to 7 places.
        for (actual, expected) in RgbSpace::Srgb.to_xyz()[1]
            .into_iter()
            .zip([0.2126390, 0.7151687, 0.0721923])
        {
            assert!((actual - expected).abs() < 0.0000001);
        }
    }
}
