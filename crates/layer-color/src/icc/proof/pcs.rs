//! Media-relative D50 PCS endpoints. Black/white policy belongs to the connection.
use super::super::*;

pub(super) const D50: [f64; 3] = [0.9642, 1., 0.8249];
use super::lut::Pipeline;

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
            let curves = [&profile.red_trc, &profile.green_trc, &profile.blue_trc];
            let decode = curves.map(|c| c.as_ref().unwrap().make_linear_evaluator().map_err(error));
            let encode = curves.map(|c| c.as_ref().unwrap().make_gamma_evaluator().map_err(error));
            let [dr, dg, db] = decode;
            let [er, eg, eb] = encode;
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
                decode: [dr?, dg?, db?],
                encode: [er?, eg?, eb?],
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

pub(super) fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let f = |v: f64| {
        if v > (6f64 / 29.).powi(3) {
            v.cbrt()
        } else {
            v * 841. / 108. + 4. / 29.
        }
    };
    let [x, y, z] = std::array::from_fn(|i| f(xyz[i] / D50[i]));
    [116. * y - 16., 500. * (x - y), 200. * (y - z)]
}

pub(super) fn lab_to_xyz([l, a, b]: [f64; 3]) -> [f64; 3] {
    let y = (l + 16.) / 116.;
    let f = |v: f64| {
        if v > 6. / 29. {
            v.powi(3)
        } else {
            (v - 4. / 29.) * 108. / 841.
        }
    };
    let lab = [y + a / 500., y, y - b / 200.];
    std::array::from_fn(|i| f(lab[i]) * D50[i])
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
