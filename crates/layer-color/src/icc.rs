use layer_core::color::{ColorProfile, ConversionOptions, RenderingIntent, RgbSpace};
use moxcms::{ColorProfile as Profile, DataColorSpace, Layout, ToneReprCurve};
use std::sync::Arc;

pub use layer_core::color::ProfileChannels;
pub use layer_core::color::source::MAX_PROFILE_BYTES as MAX_ICC_BYTES;

mod profiles;
pub(crate) use profiles::{matrix_profile, calibrated_rgb_profile};
pub(crate) use profiles::nclx_profile;
use profiles::*;
mod description;
mod output;
mod working;
pub use description::{profile_description, suggested_working_space};
pub use output::{OutputStatistics, WorkingEncoder};
pub use profiles::{gray_profile, profile_bytes, profile_channels};
pub use working::WorkingDecoder;
mod proof;
pub use proof::{ProofLut, ProofTransform};

type FloatTransform<const N: usize> = CompiledTransform<N, 4>;

/// Transform handles are pure Rust and can be shared between file workers.
/// The f64 executor preserves 16-bit source precision; buffers stay strip-sized.
struct CompiledTransform<const N: usize, const M: usize> {
    transform: Option<Arc<dyn moxcms::TransformExecutor<f64> + Send + Sync>>,
    pre: Option<MatrixTransform>,
    matrix: Option<MatrixTransform>,
    input_cmyk: bool,
    output_cmyk: bool,
    copy_alpha: bool,
}
impl<const N: usize, const M: usize> CompiledTransform<N, M> {
    fn new(input: &Profile, output: &Profile, options: ConversionOptions) -> Result<Self, String> {
        validate_options(options)?;
        let input_cmyk = channels(input)? == ProfileChannels::Cmyk;
        let output_cmyk = channels(output)? == ProfileChannels::Cmyk;
        let matrix = MatrixTransform::new(input, output, options)?;
        let (mut source, mut destination) = (input.clone(), output.clone());
        // A uniform LUT sampled in linear RGB loses dark colors. Shape matrix
        // input with the sRGB curve before the backend samples a device LUT.
        let pre = if matrix.is_none() && matrix_only(input) && has_lut(output) {
            source.red_trc = Some(profiles::curve(RgbSpace::Srgb));
            source.green_trc = source.red_trc.clone();
            source.blue_trc = source.red_trc.clone();
            MatrixTransform::new(input, &source, ConversionOptions::default())?
        } else {
            None
        };
        if matrix.is_none() && options.intent == RenderingIntent::AbsoluteColorimetric {
            let scale = media_white_scale(input, output)?;
            // moxcms selects the colorimetric LUT for absolute intent, but does
            // not apply the media-white scale. Fold it into the matrix endpoint.
            if matrix_only(&source) {
                scale_colorants(&mut source, scale);
            } else if matrix_only(&destination) {
                scale_colorants(&mut destination, scale.map(|v| 1. / v));
            } else if scale.iter().any(|v| (*v - 1.).abs() > 1e-8) {
                return Err(
                    "Absolute color conversion between two non-matrix profiles is unavailable"
                        .into(),
                );
            }
        }
        let layout = |n| match n {
            1 => Layout::Gray,
            3 => Layout::Rgb,
            4 => Layout::Rgba,
            _ => unreachable!(),
        };
        let transform = if matrix.is_none() {
            Some(
                source
                    .create_transform_f64(
                        layout(N),
                        &destination,
                        layout(M),
                        moxcms::TransformOptions {
                            rendering_intent: intent(options.intent),
                            allow_use_cicp_transfer: false,
                            prefer_fixed_point: false,
                            allow_extended_range_rgb_xyz: true,
                            interpolation_method: moxcms::InterpolationMethod::Tetrahedral,
                            ..Default::default()
                        },
                    )
                    .map_err(error)?,
            )
        } else {
            None
        };
        Ok(Self {
            transform,
            pre,
            matrix,
            input_cmyk,
            output_cmyk,
            copy_alpha: N == 4 && M == 4 && !input_cmyk && !output_cmyk,
        })
    }
    fn transform_pixels(&self, input: &[[f32; N]], output: &mut [[f32; M]]) {
        assert_eq!(input.len(), output.len());
        if let Some(matrix) = &self.matrix {
            for (input, output) in input.iter().zip(output) {
                let rgb = matrix.apply([input[0], input[1], input[2]]);
                output[..3].copy_from_slice(&rgb);
                if M == 4 {
                    output[3] = if self.copy_alpha { input[3] } else { 1. };
                }
            }
            return;
        }
        for (input, output) in input.chunks(256).zip(output.chunks_mut(256)) {
            let mut source = [[0f64; N]; 256];
            let mut destination = [[0f64; M]; 256];
            for (a, b) in input.iter().zip(&mut source) {
                *b = a.map(|v| f64::from(v) / if self.input_cmyk { 100. } else { 1. });
                if let Some(pre) = &self.pre {
                    b[..3].copy_from_slice(&pre.apply([a[0], a[1], a[2]]).map(f64::from));
                }
            }
            self.transform
                .as_ref()
                .expect("non-matrix transform")
                .transform(
                    source[..input.len()].as_flattened(),
                    destination[..input.len()].as_flattened_mut(),
                )
                .expect("validated ICC sample layout");
            for ((original, converted), result) in input.iter().zip(destination).zip(output) {
                *result = converted.map(|v| (v * if self.output_cmyk { 100. } else { 1. }) as f32);
                if self.copy_alpha {
                    result[3] = original[3];
                }
            }
        }
    }
}

/// Explicit requests for unsupported conversion policy must not become a no-op.
fn validate_options(options: ConversionOptions) -> Result<(), String> {
    if options.black_point_compensation {
        return Err(
            "Black point compensation is unavailable. Disable it for this color conversion.".into(),
        );
    }
    Ok(())
}

fn intent(value: RenderingIntent) -> moxcms::RenderingIntent {
    match value {
        RenderingIntent::Perceptual => moxcms::RenderingIntent::Perceptual,
        RenderingIntent::RelativeColorimetric => moxcms::RenderingIntent::RelativeColorimetric,
        RenderingIntent::Saturation => moxcms::RenderingIntent::Saturation,
        RenderingIntent::AbsoluteColorimetric => moxcms::RenderingIntent::AbsoluteColorimetric,
    }
}
fn error(error: impl std::fmt::Display) -> String {
    format!("ICC color transform failed: {error}")
}
#[cfg(test)]
mod tests;

struct MatrixTransform {
    input: [Box<dyn moxcms::ToneCurveEvaluator + Send + Sync>; 3],
    output: [Box<dyn moxcms::ToneCurveEvaluator + Send + Sync>; 3],
    matrix: [[f64; 3]; 3],
}

/// Gain is applied in linear application RGB, before conversion to the editing
/// primaries. Applying RGB gains after an ICC transform changes saturated HDR.
pub(crate) struct GainMapColor {
    base_to_application: MatrixTransform,
    application_to_srgb: MatrixTransform,
}
impl GainMapColor {
    pub fn new(base: &ColorProfile, application: Option<&ColorProfile>) -> Result<Self, String> {
        let base = open(base)?;
        let mut linear = match application { Some(p) => open(p)?, None => base.clone() };
        if !matrix_only(&base) || !matrix_only(&linear) {
            return Err("Gain maps require a matrix RGB color profile".into());
        }
        linear.red_trc = Some(ToneReprCurve::Parametric(vec![1.]));
        linear.green_trc = linear.red_trc.clone();
        linear.blue_trc = linear.red_trc.clone();
        let options = ConversionOptions::default();
        Ok(Self {
            base_to_application: MatrixTransform::new(&base, &linear, options)?.ok_or("Invalid gain-map color profile")?,
            application_to_srgb: MatrixTransform::new(&linear, &linear_profile(RgbSpace::Srgb)?, options)?.ok_or("Invalid gain-map color profile")?,
        })
    }
    pub fn linear_base(&self, encoded: [f32; 3]) -> [f32; 3] { self.base_to_application.apply(encoded) }
    pub fn to_srgb(&self, linear: [f32; 3]) -> [f32; 3] { self.application_to_srgb.apply(linear) }
}
impl MatrixTransform {
    fn new(
        input: &Profile,
        output: &Profile,
        options: ConversionOptions,
    ) -> Result<Option<Self>, String> {
        if !matrix_only(input) || !matrix_only(output) {
            return Ok(None);
        }
        let curves = |p: &Profile, invert: bool| -> Result<_, String> {
            let values = [&p.red_trc, &p.green_trc, &p.blue_trc].map(|curve| {
                let curve = curve.as_ref().ok_or("ICC profile has no RGB curve")?;
                if invert {
                    curve.make_gamma_evaluator()
                } else {
                    curve.make_linear_evaluator()
                }
                .map_err(error)
            });
            let [r, g, b] = values;
            Ok([r?, g?, b?])
        };
        let mut source_matrix = input.colorant_matrix();
        if options.intent == RenderingIntent::AbsoluteColorimetric {
            let scale = media_white_scale(input, output)?;
            for (row, scale) in source_matrix.v.iter_mut().zip(scale) {
                row.iter_mut().for_each(|v| *v *= scale);
            }
        }
        let matrix = output.colorant_matrix().inverse().mat_mul(source_matrix).v;
        if matrix.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Invalid ICC colorant matrix".into());
        }
        Ok(Some(Self {
            input: curves(input, false)?,
            output: curves(output, true)?,
            matrix,
        }))
    }
    fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        // Extended RGB follows the same sign-preserving curve convention as
        // built-in working spaces. Alpha never participates.
        let linear = std::array::from_fn(|c| {
            f64::from(self.input[c].evaluate_value(rgb[c].abs()).copysign(rgb[c]))
        });
        let linear = layer_core::color::rgb::apply(self.matrix, linear);
        std::array::from_fn(|c| {
            self.output[c]
                .evaluate_value(linear[c].abs() as f32)
                .copysign(linear[c] as f32)
        })
    }
}
fn matrix_only(profile: &Profile) -> bool {
    profile.color_space == DataColorSpace::Rgb
        && profile.pcs == DataColorSpace::Xyz
        && profile.red_trc.is_some()
        && profile.green_trc.is_some()
        && profile.blue_trc.is_some()
        && !has_lut(profile)
}
fn has_lut(profile: &Profile) -> bool {
    [
        &profile.lut_a_to_b_perceptual,
        &profile.lut_a_to_b_colorimetric,
        &profile.lut_a_to_b_saturation,
        &profile.lut_b_to_a_perceptual,
        &profile.lut_b_to_a_colorimetric,
        &profile.lut_b_to_a_saturation,
    ]
    .iter()
    .any(|v| v.is_some())
}

fn media_white_scale(input: &Profile, output: &Profile) -> Result<[f64; 3], String> {
    let a = input.media_white_point.unwrap_or(input.white_point);
    let b = output.media_white_point.unwrap_or(output.white_point);
    let scale = [a.x / b.x, a.y / b.y, a.z / b.z];
    if scale.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("Invalid ICC media white point".into());
    }
    Ok(scale)
}
fn scale_colorants(profile: &mut Profile, scale: [f64; 3]) {
    for c in [
        &mut profile.red_colorant,
        &mut profile.green_colorant,
        &mut profile.blue_colorant,
    ] {
        c.x *= scale[0];
        c.y *= scale[1];
        c.z *= scale[2];
    }
}
