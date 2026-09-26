use super::*;

fn validate_header(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 132 || bytes.len() > MAX_ICC_BYTES {
        return Err("ICC profile size is unsupported".into());
    }
    let integer = |at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let declared = integer(0);
    if declared < 132 || declared > bytes.len() || &bytes[36..40] != b"acsp" {
        return Err("ICC profile header is corrupt".into());
    }
    let count = integer(128);
    let table_end = count
        .checked_mul(12)
        .and_then(|n| n.checked_add(132))
        .filter(|n| *n <= declared)
        .ok_or("ICC tag table is corrupt")?;
    for at in (132..table_end).step_by(12) {
        let offset = integer(at + 4);
        let length = integer(at + 8);
        if offset < table_end
            || length < 8
            || offset.checked_add(length).is_none_or(|end| end > declared)
        {
            return Err("ICC tag data is incomplete".into());
        }
    }
    Ok(())
}

pub(super) fn channels(profile: &Profile) -> Result<ProfileChannels, String> {
    match profile.color_space {
        DataColorSpace::Rgb => Ok(ProfileChannels::Rgb),
        DataColorSpace::Gray => Ok(ProfileChannels::Gray),
        DataColorSpace::Cmyk => Ok(ProfileChannels::Cmyk),
        _ => Err("Only RGB, grayscale and CMYK ICC sources are supported".into()),
    }
}

pub(super) fn open(profile: &ColorProfile) -> Result<Profile, String> {
    match profile {
        ColorProfile::Builtin(space) => builtin(*space),
        ColorProfile::Icc(bytes) => {
            validate_header(bytes)?;
            let profile = Profile::new_from_slice_with_options(
                bytes,
                moxcms::ParsingOptions {
                    max_profile_size: MAX_ICC_BYTES,
                    ..Default::default()
                },
            )
            .map_err(error)?;
            if !matches!(
                profile.profile_class,
                moxcms::ProfileClass::InputDevice
                    | moxcms::ProfileClass::DisplayDevice
                    | moxcms::ProfileClass::OutputDevice
                    | moxcms::ProfileClass::ColorSpace
            ) {
                return Err("This ICC profile is not an image color space".into());
            }
            channels(&profile)?;
            Ok(profile)
        }
    }
}

pub fn profile_channels(profile: &ColorProfile) -> Result<ProfileChannels, String> {
    let opened = open(profile)?;
    let kind = channels(&opened)?;
    let destination = builtin(RgbSpace::Srgb)?;
    // Parsing alone does not establish that a profile can interpret samples.
    let options = ConversionOptions::default();
    match kind {
        ProfileChannels::Gray => {
            CompiledTransform::<1, 4>::new(&opened, &destination, options)?;
        }
        ProfileChannels::Rgb | ProfileChannels::Cmyk => {
            CompiledTransform::<4, 4>::new(&opened, &destination, options)?;
        }
    }
    Ok(kind)
}

/// Preserve imported bytes exactly, including unknown tags. Generated profiles
/// use a fixed creation date; the ICC v4 profile ID is explicitly unspecified (zero).
pub fn profile_bytes(profile: &ColorProfile) -> Result<Vec<u8>, String> {
    let opened = open(profile)?;
    if let ColorProfile::Icc(bytes) = profile {
        return Ok(bytes.to_vec());
    }
    stable_bytes(&opened)
}

pub(super) fn describe(profile: &mut Profile, name: &str) {
    profile.description = Some(moxcms::ProfileText::Localizable(vec![
        moxcms::LocalizableString::new("en".into(), "US".into(), name.into()),
    ]));
    profile.creation_date_time = moxcms::ColorDateTime {
        year: 2026,
        month: 1,
        day_of_the_month: 1,
        hours: 0,
        minutes: 0,
        seconds: 0,
    };
}
pub(super) fn curve(space: RgbSpace) -> ToneReprCurve {
    ToneReprCurve::Parametric(match space {
        RgbSpace::Srgb | RgbSpace::DisplayP3 => {
            vec![2.4, 1. / 1.055, 0.055 / 1.055, 1. / 12.92, 0.04045]
        }
        RgbSpace::AdobeRgb => vec![563. / 256.],
        RgbSpace::ProPhoto => vec![1.8, 1., 0., 1. / 16., 1. / 32.],
    })
}

fn definition(
    white: [f64; 2],
    primaries: [[f64; 2]; 3],
    curve: ToneReprCurve,
    gray: bool,
) -> Result<Profile, String> {
    use moxcms::{Chromaticity, ColorPrimaries, Matrix3d, XyY};
    for [x, y] in std::iter::once(white).chain(primaries) {
        if !x.is_finite() || !y.is_finite() || x < 0. || y <= 0. || x + y > 1.00001 {
            return Err("Invalid ICC chromaticities".into());
        }
    }
    let xy = |[x, y]: [f64; 2]| Chromaticity {
        x: x as f32,
        y: y as f32,
    };
    let [red, green, blue] = primaries.map(xy);
    let wp = XyY {
        x: white[0],
        y: white[1],
        yb: 1.,
    };
    let d50 = moxcms::white_point_d50();
    let mut profile = Profile::new_srgb();
    profile.update_rgb_colorimetry(wp, ColorPrimaries { red, green, blue });
    profile.white_point = d50.to_xyzd();
    profile.media_white_point = Some(d50.to_xyzd());
    // Store the actual source-white -> D50 adaptation, not the Bradford cone
    // matrix itself. Profile suggestions undo this matrix to recover primaries.
    profile.chromatic_adaptation = Some(Matrix3d {
        v: layer_core::color::rgb::bradford(white, [d50.x, d50.y]),
    });
    if gray {
        profile.color_space = DataColorSpace::Gray;
        profile.gray_trc = Some(curve);
        profile.red_trc = None;
        profile.green_trc = None;
        profile.blue_trc = None;
    } else {
        profile.red_trc = Some(curve.clone());
        profile.green_trc = Some(curve.clone());
        profile.blue_trc = Some(curve);
    }
    if [
        profile.red_colorant,
        profile.green_colorant,
        profile.blue_colorant,
    ]
    .iter()
    .any(|v| !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite())
    {
        return Err("Invalid ICC chromaticities".into());
    }
    describe(&mut profile, "Custom RGB/gray profile");
    Ok(profile)
}

pub(super) fn builtin(space: RgbSpace) -> Result<Profile, String> {
    let mut profile = definition(space.white(), space.primaries(), curve(space), false)?;
    describe(&mut profile, space.name());
    Ok(profile)
}
pub(super) fn linear_profile(space: RgbSpace) -> Result<Profile, String> {
    definition(
        space.white(),
        space.primaries(),
        ToneReprCurve::Parametric(vec![1.]),
        false,
    )
}
pub fn gray_profile(space: RgbSpace) -> Result<ColorProfile, String> {
    let mut profile = definition(space.white(), space.primaries(), curve(space), true)?;
    describe(&mut profile, &format!("{} tone curve (gray)", space.name()));
    Ok(ColorProfile::Icc(stable_bytes(&profile)?.into()))
}
pub(crate) fn matrix_profile(
    white: [f64; 2],
    primaries: [[f64; 2]; 3],
    gamma: Option<f64>,
    gray: bool,
) -> Result<ColorProfile, String> {
    let curve = if let Some(gamma) = gamma {
        if !(0.01..=10.).contains(&gamma) {
            return Err("Unsupported PNG gamma".into());
        }
        ToneReprCurve::Parametric(vec![(1. / gamma) as f32])
    } else {
        curve(RgbSpace::Srgb)
    };
    Ok(ColorProfile::Icc(
        stable_bytes(&definition(white, primaries, curve, gray)?)?.into(),
    ))
}

/// BMP V4/V5 endpoints are XYZ colorants and per-channel decoding gamma.
pub(crate) fn calibrated_rgb_profile(
    endpoints: [[f64; 3]; 3],
    gamma: [f64; 3],
) -> Result<ColorProfile, String> {
    let chromaticity = |xyz: [f64; 3]| -> Result<[f64; 2], String> {
        if xyz.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err("Unsupported calibrated BMP colorants".into());
        }
        let total: f64 = xyz.iter().sum();
        if total <= 0. || xyz[1] <= 0. {
            return Err("Invalid calibrated BMP colorants".into());
        }
        Ok([xyz[0] / total, xyz[1] / total])
    };
    let white = chromaticity(std::array::from_fn(|i| endpoints.iter().map(|v| v[i]).sum()))?;
    let [red, green, blue] = endpoints.map(chromaticity);
    if gamma.iter().any(|g| !(0.1..=10.).contains(g)) {
        return Err("Unsupported calibrated BMP gamma".into());
    }
    let mut profile = definition(white, [red?, green?, blue?],
        ToneReprCurve::Parametric(vec![gamma[0] as f32]), false)?;
    profile.green_trc = Some(ToneReprCurve::Parametric(vec![gamma[1] as f32]));
    profile.blue_trc = Some(ToneReprCurve::Parametric(vec![gamma[2] as f32]));
    describe(&mut profile, "Calibrated BMP RGB");
    Ok(ColorProfile::Icc(stable_bytes(&profile)?.into()))
}

/// Materialize supported SDR CICP transfer/primaries as a reusable source ICC.
pub(crate) fn nclx_profile(xy: [f32; 8], transfer: u32) -> Result<ColorProfile, String> {
    let curve = match transfer {
        13 => curve(RgbSpace::Srgb),
        1 | 6 | 14 | 15 => {
            let alpha: f32 = if transfer == 15 { 1.0993 } else { 1.099 };
            let beta: f32 = if transfer == 15 { 0.0181 } else { 0.018 };
            ToneReprCurve::Parametric(vec![1. / 0.45, 1. / alpha, (alpha - 1.) / alpha, 1. / 4.5, 4.5 * beta])
        }
        4 => ToneReprCurve::Parametric(vec![2.2]),
        5 => ToneReprCurve::Parametric(vec![2.8]),
        8 => ToneReprCurve::Parametric(vec![1.]),
        16 | 18 => return Err("HDR HEIF/AVIF needs an explicit SDR conversion before import".into()),
        _ => return Err("Unsupported HEIF/AVIF transfer characteristic".into()),
    };
    let mut profile = definition([xy[6] as f64, xy[7] as f64],
        std::array::from_fn(|i| [xy[i * 2] as f64, xy[i * 2 + 1] as f64]), curve, false)?;
    describe(&mut profile, "HEIF/AVIF source RGB");
    Ok(ColorProfile::Icc(stable_bytes(&profile)?.into()))
}

fn stable_bytes(profile: &Profile) -> Result<Vec<u8>, String> {
    // moxcms 0.9.1's writer currently writes the wall clock instead of the
    // profile's creation_date_time. ICC permits an unspecified (zero) ID.
    let mut bytes = profile.encode().map_err(error)?;
    for (field, value) in bytes[24..36]
        .chunks_exact_mut(2)
        .zip([2026u16, 1, 1, 0, 0, 0])
    {
        field.copy_from_slice(&value.to_be_bytes());
    }
    bytes[84..100].fill(0);
    Ok(bytes)
}
