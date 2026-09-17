//! Evaluate original ICC tables without resampling their PCS stages. ICC.1
//! mft1/mft2 and mAB/mBA stage order and encodings; tetrahedral 3D interpolation.
use super::*;
use moxcms::{LutStore, LutType, LutWarehouse};

type Curve = Box<dyn moxcms::ToneCurveEvaluator + Send + Sync>;
enum Stage {
    Curves(Vec<Curve>),
    Tables {
        values: Vec<f64>,
        entries: usize,
        channels: usize,
    },
    Matrix {
        matrix: [[f64; 3]; 3],
        bias: [f64; 3],
    },
    Clut {
        values: Vec<f32>,
        grid: Vec<usize>,
        outputs: usize,
    },
}

pub(super) struct Pipeline {
    stages: Vec<Stage>,
    lab: bool,
    legacy_lab: bool,
    trilinear: bool,
}

fn data(store: &LutStore) -> Vec<f32> {
    match store {
        LutStore::Store8(v) => v.iter().map(|v| f32::from(*v) / 255.).collect(),
        LutStore::Store16(v) => v.iter().map(|v| f32::from(*v) / 65535.).collect(),
    }
}

impl Pipeline {
    pub(super) fn new(
        profile: &Profile,
        rendering: RenderingIntent,
        reverse: bool,
    ) -> Result<Self, String> {
        let tags = if reverse {
            [
                &profile.lut_b_to_a_perceptual,
                &profile.lut_b_to_a_colorimetric,
                &profile.lut_b_to_a_saturation,
            ]
        } else {
            [
                &profile.lut_a_to_b_perceptual,
                &profile.lut_a_to_b_colorimetric,
                &profile.lut_a_to_b_saturation,
            ]
        };
        let index = match rendering {
            RenderingIntent::Perceptual => 0,
            RenderingIntent::RelativeColorimetric | RenderingIntent::AbsoluteColorimetric => 1,
            RenderingIntent::Saturation => 2,
        };
        // ICC specifies A2B0/B2A0 as the fallback when the requested table is
        // absent. A missing reverse path still fails; never invent an inverse.
        let lut = tags[index].as_ref().or(tags[0].as_ref()).ok_or_else(|| {
            format!(
                "Proof profile lacks a {} transform",
                if reverse {
                    "PCS-to-device"
                } else {
                    "device-to-PCS"
                }
            )
        })?;
        if !matches!(profile.pcs, DataColorSpace::Lab | DataColorSpace::Xyz) {
            return Err("Unsupported proof profile PCS".into());
        }
        let device_channels = if channels(profile)? == ProfileChannels::Cmyk {
            4
        } else {
            3
        };
        let (input, output) = if reverse {
            (3, device_channels)
        } else {
            (device_channels, 3)
        };
        let mut stages = Vec::new();
        let legacy_lab = match lut {
            LutWarehouse::Lut(lut) => {
                if usize::from(lut.num_input_channels) != input
                    || usize::from(lut.num_output_channels) != output
                {
                    return Err("Proof LUT channel count disagrees with its profile".into());
                }
                if reverse && profile.pcs == DataColorSpace::Xyz {
                    stages.push(Stage::matrix(lut.matrix.v, [0.; 3])?);
                }
                stages.push(Stage::table(
                    &lut.input_table,
                    usize::from(lut.num_input_table_entries),
                    input,
                )?);
                stages.push(Stage::clut(
                    &lut.clut_table,
                    vec![usize::from(lut.num_clut_grid_points); input],
                    output,
                )?);
                stages.push(Stage::table(
                    &lut.output_table,
                    usize::from(lut.num_output_table_entries),
                    output,
                )?);
                lut.lut_type == LutType::Lut16
            }
            LutWarehouse::Multidimensional(lut) => {
                if usize::from(lut.num_input_channels) != input
                    || usize::from(lut.num_output_channels) != output
                {
                    return Err("Proof LUT channel count disagrees with its profile".into());
                }
                let curves = |v: &[ToneReprCurve], n: usize| -> Result<Stage, String> {
                    if v.len() != n {
                        return Err("Incomplete proof LUT curves".into());
                    }
                    Ok(Stage::Curves(
                        v.iter()
                            .map(|c| c.make_linear_evaluator().map_err(error))
                            .collect::<Result<_, _>>()?,
                    ))
                };
                if reverse {
                    stages.push(curves(&lut.b_curves, input)?);
                    if !lut.m_curves.is_empty() {
                        stages.push(Stage::matrix(lut.matrix.v, lut.bias.v)?);
                        stages.push(curves(&lut.m_curves, input)?);
                    }
                    if let Some(clut) = &lut.clut {
                        stages.push(Stage::clut(
                            clut,
                            lut.grid_points[..input]
                                .iter()
                                .map(|v| usize::from(*v))
                                .collect(),
                            output,
                        )?);
                        stages.push(curves(&lut.a_curves, output)?);
                    } else if input != output || !lut.a_curves.is_empty() {
                        return Err("Incomplete proof CLUT/A-curve combination".into());
                    }
                } else {
                    if let Some(clut) = &lut.clut {
                        stages.push(curves(&lut.a_curves, input)?);
                        stages.push(Stage::clut(
                            clut,
                            lut.grid_points[..input]
                                .iter()
                                .map(|v| usize::from(*v))
                                .collect(),
                            output,
                        )?);
                    } else if input != output || !lut.a_curves.is_empty() {
                        return Err("Incomplete proof CLUT/A-curve combination".into());
                    }
                    if !lut.m_curves.is_empty() {
                        stages.push(curves(&lut.m_curves, output)?);
                        stages.push(Stage::matrix(lut.matrix.v, lut.bias.v)?);
                    }
                    stages.push(curves(&lut.b_curves, output)?);
                }
                false
            }
        };
        Ok(Self {
            stages,
            lab: profile.pcs == DataColorSpace::Lab,
            legacy_lab,
            trilinear: reverse && profile.pcs == DataColorSpace::Lab,
        })
    }

    fn run(&self, mut values: [f64; 4]) -> [f64; 4] {
        for stage in &self.stages {
            match stage {
                Stage::Curves(curves) => {
                    for (v, curve) in values.iter_mut().zip(curves) {
                        *v = f64::from(curve.evaluate_value(*v as f32));
                    }
                }
                Stage::Tables {
                    values: table,
                    entries,
                    channels,
                } => {
                    for channel in 0..*channels {
                        let x = values[channel].clamp(0., 1.) * (*entries - 1) as f64;
                        let lower = (x as usize).min(*entries - 2);
                        let t = x - lower as f64;
                        let at = channel * entries + lower;
                        values[channel] = table[at] * (1. - t) + table[at + 1] * t;
                    }
                }
                Stage::Matrix { matrix, bias } => {
                    let xyz =
                        layer_core::color::rgb::apply(*matrix, [values[0], values[1], values[2]]);
                    for i in 0..3 {
                        values[i] = xyz[i] + bias[i];
                    }
                }
                Stage::Clut {
                    values: table,
                    grid,
                    outputs,
                } => {
                    let mut low = [0; 4];
                    let mut fraction = [0.; 4];
                    for i in 0..grid.len() {
                        let v = values[i].clamp(0., 1.) * (grid[i] - 1) as f64;
                        low[i] = (v as usize).min(grid[i] - 2);
                        fraction[i] = v - low[i] as f64;
                    }
                    let first = usize::from(grid.len() == 4);
                    let mut order = [first, first + 1, first + 2];
                    order.sort_by(|&a, &b| fraction[b].total_cmp(&fraction[a]));
                    let fetch = |point: [usize; 4]| -> [f64; 4] {
                        let mut at = 0;
                        for i in 0..grid.len() {
                            at = at * grid[i] + point[i];
                        }
                        let mut result = [0.; 4];
                        for c in 0..*outputs {
                            result[c] = f64::from(table[at * outputs + c]);
                        }
                        result
                    };
                    // Lab is a nonlinear index space. Match the reference CMM's
                    // trilinear PCS-to-device rule; tetrahedral is for device RGB.
                    if self.trilinear {
                        values = [0.; 4];
                        for corner in 0..8 {
                            let mut point = low;
                            let mut weight = 1.;
                            for axis in 0..3 {
                                if corner & (1 << axis) == 0 {
                                    weight *= 1. - fraction[axis];
                                } else {
                                    point[axis] += 1;
                                    weight *= fraction[axis];
                                }
                            }
                            let sample = fetch(point);
                            for c in 0..*outputs {
                                values[c] += weight * sample[c];
                            }
                        }
                        continue;
                    }
                    let tetrahedron = |mut point: [usize; 4]| {
                        let mut previous = fetch(point);
                        let mut result = previous;
                        for axis in order {
                            point[axis] += 1;
                            let next = fetch(point);
                            for c in 0..*outputs {
                                result[c] += fraction[axis] * (next[c] - previous[c]);
                            }
                            previous = next;
                        }
                        result
                    };
                    values = tetrahedron(low);
                    if first == 1 {
                        low[0] += 1;
                        let high = tetrahedron(low);
                        for c in 0..*outputs {
                            values[c] += fraction[0] * (high[c] - values[c]);
                        }
                    }
                }
            }
        }
        values
    }

    pub(super) fn to_xyz(&self, device: [f64; 4]) -> [f64; 3] {
        let output = self.run(device);
        if self.lab {
            let scale = if self.legacy_lab { 65535. / 65280. } else { 1. };
            lab_to_xyz([
                output[0] * scale * 100.,
                output[1] * scale * 255. - 128.,
                output[2] * scale * 255. - 128.,
            ])
        } else {
            std::array::from_fn(|i| output[i] * 65535. / 32768.)
        }
    }

    pub(super) fn device_from_xyz(&self, xyz: [f64; 3]) -> [f64; 4] {
        let input = if self.lab {
            let lab = xyz_to_lab(xyz);
            let scale = if self.legacy_lab { 65280. / 65535. } else { 1. };
            [
                lab[0] / 100. * scale,
                (lab[1] + 128.) / 255. * scale,
                (lab[2] + 128.) / 255. * scale,
                0.,
            ]
        } else {
            [
                xyz[0] * 32768. / 65535.,
                xyz[1] * 32768. / 65535.,
                xyz[2] * 32768. / 65535.,
                0.,
            ]
        };
        self.run(input).map(|v| v.clamp(0., 1.))
    }
}

impl Stage {
    fn matrix(matrix: [[f64; 3]; 3], bias: [f64; 3]) -> Result<Self, String> {
        if matrix
            .iter()
            .flatten()
            .chain(bias.iter())
            .any(|v| !v.is_finite())
        {
            return Err("Nonfinite proof matrix".into());
        }
        Ok(Self::Matrix { matrix, bias })
    }
    fn table(store: &LutStore, entries: usize, channels: usize) -> Result<Self, String> {
        let values = data(store).into_iter().map(f64::from).collect::<Vec<_>>();
        if entries < 2 || values.len() != entries * channels {
            return Err("Incomplete proof tone table".into());
        }
        Ok(Self::Tables {
            values,
            entries,
            channels,
        })
    }
    fn clut(store: &LutStore, grid: Vec<usize>, outputs: usize) -> Result<Self, String> {
        if !(3..=4).contains(&grid.len())
            || grid.iter().any(|v| *v < 2)
            || !(3..=4).contains(&outputs)
        {
            return Err("Unsupported proof CLUT dimensions".into());
        }
        let length = grid
            .iter()
            .try_fold(outputs, |n, v| n.checked_mul(*v))
            .ok_or("Proof CLUT size overflow")?;
        let values = data(store);
        if values.len() != length || length > 10_000_000 {
            return Err("Invalid or excessive proof CLUT size".into());
        }
        Ok(Self::Clut {
            values,
            grid,
            outputs,
        })
    }
}
