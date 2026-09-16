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
    let d50 = moxcms::white_point_d50().to_xyzd();
    let mut profile = Profile::new_srgb();
    profile.update_rgb_colorimetry(wp, ColorPrimaries { red, green, blue });
    profile.white_point = d50;
    profile.media_white_point = Some(d50);
    // Store the actual source-white -> D50 adaptation, not the Bradford cone
    // matrix itself. Profile suggestions undo this matrix to recover primaries.
    let bradford = Matrix3d {
        v: [
            [0.8951, 0.2664, -0.1614],
            [-0.7502, 1.7135, 0.0367],
            [0.0389, -0.0685, 1.0296],
        ],
    };
    let source_white = wp.to_xyzd();
    let a =
        layer_core::color::rgb::apply(bradford.v, [source_white.x, source_white.y, source_white.z]);
    let b = layer_core::color::rgb::apply(bradford.v, [d50.x, d50.y, d50.z]);
    let scaled = Matrix3d {
        v: std::array::from_fn(|r| bradford.v[r].map(|v| v * b[r] / a[r])),
    };
    profile.chromatic_adaptation = Some(bradford.inverse().mat_mul(scaled));
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
    for [x, y] in std::iter::once(white).chain(primaries) {
        if !x.is_finite() || !y.is_finite() || x < 0. || y <= 0. || x + y > 1.00001 {
            return Err("Invalid PNG chromaticities".into());
        }
    }
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
