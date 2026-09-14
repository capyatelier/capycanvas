use layer_core::color::{ColorProfile, ConversionOptions, RenderingIntent, RgbSpace};
use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, DisallowCache, Flags, Intent, PixelFormat, Profile,
    ThreadContext, ToneCurve, Transform,
};

pub const MAX_ICC_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileChannels {
    Rgb,
    Gray,
    Cmyk,
}

type FloatTransform<const N: usize> = Transform<[f32; N], [f32; 4], ThreadContext, DisallowCache>;

/// Straight encoded RGB in/out; linear coverage is copied verbatim. Construct
/// once per operation and reuse for rows/tiles. Float32 formats avoid LCMS's
/// integer formatter/CLUT optimization as an implicit 8/16-bit work boundary.
pub struct RgbTransform {
    transform: Option<FloatTransform<4>>,
    // LCMS objects carry raw context handles. Keep the owner until after them.
    _context: ThreadContext,
}

impl RgbTransform {
    pub fn new(
        source: &ColorProfile,
        destination: &ColorProfile,
        options: ConversionOptions,
    ) -> Result<Self, String> {
        let context = ThreadContext::new();
        let input = open(&context, source)?;
        let output = open(&context, destination)?;
        if channels(&input)? != ProfileChannels::Rgb || channels(&output)? != ProfileChannels::Rgb {
            return Err("RGB conversion requires two RGB profiles".into());
        }
        // Identity is a real copy, including hidden RGB. Never run a lossy ICC
        // round trip just to return to the identical interpretation.
        let compiled = Transform::new_flags_context(
            &context,
            &input,
            PixelFormat::RGBA_FLT,
            &output,
            PixelFormat::RGBA_FLT,
            intent(options.intent),
            flags(options) | Flags::COPY_ALPHA,
        )
        .map_err(error)?;
        let transform = if source == destination {
            None
        } else {
            Some(compiled)
        };
        Ok(Self {
            transform,
            _context: context,
        })
    }

    pub fn apply(&self, pixels: &mut [[f32; 4]]) {
        if let Some(transform) = &self.transform {
            transform.transform_in_place(pixels);
        }
    }
}

/// Grayscale/CMYK import converts actual source channels through their embedded
/// profile. CMYK float values use LCMS's 0..100 percent convention, not RGB's
/// 0..1 convention. Alpha is a separate linear channel and never enters the CMM.
pub struct InputTransform {
    transform: InputKind,
    _context: ThreadContext,
}
enum InputKind {
    Gray(FloatTransform<1>),
    Cmyk(FloatTransform<4>),
}
impl InputTransform {
    pub fn new(
        source: &ColorProfile,
        destination: &ColorProfile,
        options: ConversionOptions,
    ) -> Result<Self, String> {
        let context = ThreadContext::new();
        let input = open(&context, source)?;
        let output = open(&context, destination)?;
        if channels(&output)? != ProfileChannels::Rgb {
            return Err("The editing destination must be RGB".into());
        }
        let transform = match channels(&input)? {
            ProfileChannels::Gray => InputKind::Gray(
                Transform::new_flags_context(
                    &context,
                    &input,
                    PixelFormat::GRAY_FLT,
                    &output,
                    PixelFormat::RGBA_FLT,
                    intent(options.intent),
                    flags(options),
                )
                .map_err(error)?,
            ),
            ProfileChannels::Cmyk => InputKind::Cmyk(
                Transform::new_flags_context(
                    &context,
                    &input,
                    PixelFormat::CMYK_FLT,
                    &output,
                    PixelFormat::RGBA_FLT,
                    intent(options.intent),
                    flags(options),
                )
                .map_err(error)?,
            ),
            ProfileChannels::Rgb => return Err("Use RGB conversion for an RGB source".into()),
        };
        Ok(Self {
            transform,
            _context: context,
        })
    }

    pub fn gray(&self, source: &[[f32; 1]], output: &mut [[f32; 4]]) -> Result<(), String> {
        let InputKind::Gray(transform) = &self.transform else {
            return Err("Source is not grayscale".into());
        };
        if source.len() != output.len() {
            return Err("Incomplete grayscale strip".into());
        }
        transform.transform_pixels(source, output);
        for pixel in output {
            pixel[3] = 1.;
        }
        Ok(())
    }

    pub fn cmyk_percent(&self, source: &[[f32; 4]], output: &mut [[f32; 4]]) -> Result<(), String> {
        let InputKind::Cmyk(transform) = &self.transform else {
            return Err("Source is not CMYK".into());
        };
        if source.len() != output.len() {
            return Err("Incomplete CMYK strip".into());
        }
        transform.transform_pixels(source, output);
        for pixel in output {
            pixel[3] = 1.;
        }
        Ok(())
    }
}

pub fn profile_channels(profile: &ColorProfile) -> Result<ProfileChannels, String> {
    let context = ThreadContext::new();
    let opened = open(&context, profile)?;
    let channels = channels(&opened)?;
    // ICC tag parsing can be lazy. Verify that source color data is usable,
    // including on an identity import, before adopting the image interpretation.
    let destination = Profile::new_srgb_context(&context);
    let input = match channels {
        ProfileChannels::Rgb => PixelFormat::RGB_FLT,
        ProfileChannels::Gray => PixelFormat::GRAY_FLT,
        ProfileChannels::Cmyk => PixelFormat::CMYK_FLT,
    };
    let _probe: Transform<u8, u8, ThreadContext, DisallowCache> = Transform::new_flags_context(
        &context,
        &opened,
        input,
        &destination,
        PixelFormat::RGB_FLT,
        Intent::RelativeColorimetric,
        Flags::NO_CACHE | Flags::NO_OPTIMIZE,
    )
    .map_err(error)?;
    Ok(channels)
}

/// Original ICC data is returned exactly. Built-in definitions use generated
/// ICC v4 matrix/TRC profiles with a stable creation timestamp and profile ID.
pub fn profile_bytes(profile: &ColorProfile) -> Result<Vec<u8>, String> {
    let context = ThreadContext::new();
    let opened = open(&context, profile)?;
    if let ColorProfile::Icc(bytes) = profile {
        return Ok(bytes.to_vec());
    }
    let mut bytes = opened.icc().map_err(error)?;
    for (field, value) in bytes[24..36]
        .chunks_exact_mut(2)
        .zip([2026u16, 1, 1, 0, 0, 0])
    {
        field.copy_from_slice(&value.to_be_bytes());
    }
    let mut stable = Profile::new_icc_context(&context, &bytes).map_err(error)?;
    stable.set_default_profile_id();
    stable.icc().map_err(error)
}

fn channels(profile: &Profile<ThreadContext>) -> Result<ProfileChannels, String> {
    match profile.color_space() {
        ColorSpaceSignature::RgbData => Ok(ProfileChannels::Rgb),
        ColorSpaceSignature::GrayData => Ok(ProfileChannels::Gray),
        ColorSpaceSignature::CmykData => Ok(ProfileChannels::Cmyk),
        _ => Err("Only RGB, grayscale and CMYK ICC sources are supported".into()),
    }
}

fn open(context: &ThreadContext, profile: &ColorProfile) -> Result<Profile<ThreadContext>, String> {
    match profile {
        ColorProfile::Builtin(space) => builtin(context, *space),
        ColorProfile::Icc(bytes) => {
            validate_header(bytes)?;
            let profile = Profile::new_icc_context(context, bytes).map_err(error)?;
            if !matches!(
                profile.device_class(),
                lcms2::ProfileClassSignature::InputClass
                    | lcms2::ProfileClassSignature::DisplayClass
                    | lcms2::ProfileClassSignature::OutputClass
                    | lcms2::ProfileClassSignature::ColorSpaceClass
            ) {
                return Err("This ICC profile is not an image color space".into());
            }
            channels(&profile)?;
            Ok(profile)
        }
    }
}

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

fn builtin(context: &ThreadContext, space: RgbSpace) -> Result<Profile<ThreadContext>, String> {
    let xy = |[x, y]: [f64; 2]| CIExyY { x, y, Y: 1. };
    let [red, green, blue] = space.primaries().map(xy);
    let curve = match space {
        RgbSpace::Srgb | RgbSpace::DisplayP3 => {
            ToneCurve::new_parametric(4, &[2.4, 1. / 1.055, 0.055 / 1.055, 1. / 12.92, 0.04045])
        }
        RgbSpace::AdobeRgb => ToneCurve::new_parametric(1, &[563. / 256.]),
        RgbSpace::ProPhoto => ToneCurve::new_parametric(4, &[1.8, 1., 0., 1. / 16., 1. / 32.]),
    }
    .map_err(error)?;
    let mut profile = Profile::new_rgb_context(
        context,
        &xy(space.white()),
        &CIExyYTRIPLE {
            Red: red,
            Green: green,
            Blue: blue,
        },
        &[&curve; 3],
    )
    .map_err(error)?;
    profile.set_version(4.3);
    let mut description = lcms2::MLU::new(1);
    description.set_text(space.name(), lcms2::Locale::new("en_US"));
    if !profile.write_tag(
        lcms2::TagSignature::ProfileDescriptionTag,
        lcms2::Tag::MLU(&description),
    ) {
        return Err("Cannot describe working profile".into());
    }
    Ok(profile)
}

/// PNG cHRM/gAMA describes a matrix/curve space even without an ICC payload.
/// Missing primaries/curve components have already been resolved by the caller.
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
    let context = ThreadContext::new();
    let curve = if let Some(gamma) = gamma {
        if !(0.01..=10.).contains(&gamma) {
            return Err("Unsupported PNG gamma".into());
        }
        ToneCurve::new_parametric(1, &[1. / gamma])
    } else {
        ToneCurve::new_parametric(4, &[2.4, 1. / 1.055, 0.055 / 1.055, 1. / 12.92, 0.04045])
    }
    .map_err(error)?;
    let xy = |[x, y]: [f64; 2]| CIExyY { x, y, Y: 1. };
    let profile = if gray {
        Profile::new_gray_context(&context, &xy(white), &curve)
    } else {
        let [red, green, blue] = primaries.map(xy);
        Profile::new_rgb_context(
            &context,
            &xy(white),
            &CIExyYTRIPLE {
                Red: red,
                Green: green,
                Blue: blue,
            },
            &[&curve; 3],
        )
    }
    .map_err(error)?;
    Ok(ColorProfile::Icc(profile.icc().map_err(error)?.into()))
}

fn intent(value: RenderingIntent) -> Intent {
    match value {
        RenderingIntent::Perceptual => Intent::Perceptual,
        RenderingIntent::RelativeColorimetric => Intent::RelativeColorimetric,
        RenderingIntent::Saturation => Intent::Saturation,
        RenderingIntent::AbsoluteColorimetric => Intent::AbsoluteColorimetric,
    }
}
fn flags(options: ConversionOptions) -> Flags<DisallowCache> {
    let flags = Flags::NO_CACHE | Flags::NO_OPTIMIZE;
    if options.black_point_compensation {
        flags | Flags::BLACKPOINT_COMPENSATION
    } else {
        flags
    }
}
fn error(error: lcms2::Error) -> String {
    format!("ICC color transform failed: {error}")
}

#[cfg(test)]
mod tests;
