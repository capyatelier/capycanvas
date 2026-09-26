//! Media-relative D50 PCS endpoints. Black/white policy belongs to the connection.
use super::super::*;
use super::lut::Pipeline;
use layer_core::color::lab::xyz_to_lab;

pub(super) enum Pcs {
    Matrix {
        decode: [Box<dyn moxcms::ToneCurveEvaluator + Send + Sync>; 3],
        encode: [Box<dyn moxcms::ToneCurveEvaluator + Send + Sync>; 3],
        to_xyz: [[f64; 3]; 3],
        from_xyz: [[f64; 3]; 3],
    },
    Lut {
        forward: Pipeline,
        reverse: Pipeline,
    },
}

impl Pcs {
    pub(super) fn new(
        profile: &Profile,
        rendering_intent: RenderingIntent,
    ) -> Result<Self, String> {
        let kind = channels(profile)?;
        if !matches!(kind, ProfileChannels::Rgb | ProfileChannels::Cmyk) {
            return Err("Soft proofing requires an RGB or CMYK target profile".into());
        }
        if matrix_only(profile) {
            let to_xyz = profile.colorant_matrix();
            let from_xyz = to_xyz.inverse();
            if to_xyz
                .v
                .iter()
                .chain(from_xyz.v.iter())
                .flatten()
                .any(|v| !v.is_finite())
            {
                return Err("Invalid proof profile matrix".into());
            }
            return Ok(Self::Matrix {
                decode: trc_evaluators(profile, false)?,
                encode: trc_evaluators(profile, true)?,
                to_xyz: to_xyz.v,
                from_xyz: from_xyz.v,
            });
        }
        Ok(Self::Lut {
            forward: Pipeline::new(profile, rendering_intent, false)?,
            reverse: Pipeline::new(profile, rendering_intent, true)?,
        })
    }

    pub(super) fn to_xyz(&self, device: [f64; 4]) -> [f64; 3] {
        match self {
            Self::Matrix { decode, to_xyz, .. } => {
                let linear = std::array::from_fn(|i| {
                    f64::from(
                        decode[i]
                            .evaluate_value(device[i].abs() as f32)
                            .copysign(device[i] as f32),
                    )
                });
                layer_core::color::rgb::apply(*to_xyz, linear)
            }
            Self::Lut { forward, .. } => forward.to_xyz(device),
        }
    }

    pub(super) fn device_from_xyz(&self, xyz: [f64; 3]) -> [f64; 4] {
        match self {
            Self::Matrix {
                encode, from_xyz, ..
            } => {
                let linear = layer_core::color::rgb::apply(*from_xyz, xyz);
                let mut device = [0.; 4];
                for i in 0..3 {
                    device[i] = f64::from(
                        encode[i]
                            .evaluate_value(linear[i].abs() as f32)
                            .copysign(linear[i] as f32),
                    )
                    .clamp(0., 1.);
                }
                device
            }
            Self::Lut { reverse, .. } => reverse.device_from_xyz(xyz),
        }
    }

    pub(super) fn roundtrip(&self, reverse: &Self, xyz: [f64; 3]) -> [f64; 3] {
        self.to_xyz(reverse.device_from_xyz(xyz))
    }
}

pub(super) fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let a = xyz_to_lab(a);
    let b = xyz_to_lab(b);
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}
