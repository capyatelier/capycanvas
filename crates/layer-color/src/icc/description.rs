use super::*;
use lcms2::{InfoType, Locale, Tag, TagSignature};

/// A display label only. Never use it as a profile identity or to skip a CMM
/// conversion. Embedded bytes remain authoritative even for familiar names.
pub fn profile_description(profile: &ColorProfile) -> Result<String, String> {
    if let ColorProfile::Builtin(space) = profile {
        return Ok(space.name().into());
    }
    let context = ThreadContext::new();
    let opened = open(&context, profile)?;
    let label = opened
        .info(InfoType::Description, Locale::none())
        .unwrap_or_else(|| "Embedded ICC profile".into())
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect::<String>()
        .trim()
        .to_string();
    Ok(if label.is_empty() {
        "Embedded ICC profile".into()
    } else {
        label
    })
}

/// Choose a supported editing gamut from matrix-profile colorants in the D50
/// PCS. This is a working-space suggestion, NOT equivalence: tone curves, LUTs,
/// source samples and the entire original ICC still pass through the source CMM.
/// In particular a linear P3 source may suggest encoded P3 for new paint without
/// being relabelled as encoded P3. Unknown/LUT gamuts use the documented fallback.
pub fn suggested_working_space(profile: &ColorProfile) -> Result<Option<RgbSpace>, String> {
    if let ColorProfile::Builtin(space) = profile {
        return Ok(Some(*space));
    }
    let context = ThreadContext::new();
    let opened = open(&context, profile)?;
    if channels(&opened)? != ProfileChannels::Rgb || !opened.is_matrix_shaper() {
        return Ok(None);
    }
    let colorants = |p: &Profile<ThreadContext>| -> Option<[[f64; 3]; 3]> {
        let mut matrix = [[0.; 3]; 3];
        for (column, tag) in matrix.iter_mut().zip([
            TagSignature::RedColorantTag,
            TagSignature::GreenColorantTag,
            TagSignature::BlueColorantTag,
        ]) {
            let Tag::CIEXYZ(xyz) = p.read_tag(tag) else {
                return None;
            };
            *column = [xyz.X, xyz.Y, xyz.Z];
        }
        Some(matrix)
    };
    let Some(source) = colorants(&opened) else {
        return Ok(None);
    };
    // The profile's own adaptation can differ from LCMS's current Bradford
    // coefficients. Recover native colorimetry with its declared inverse.
    // lcms2 exposes the nine CHAD doubles through CIExyYTRIPLE.
    let adaptation = match opened.read_tag(TagSignature::ChromaticAdaptationTag) {
        Tag::CIExyYTRIPLE(m) => Some([m.Red, m.Green, m.Blue].map(|r| [r.x, r.y, r.Y])),
        _ => None,
    };
    let native = adaptation
        .and_then(inverse)
        .map(|m| source.map(|c| layer_core::color::rgb::apply(m, c)));
    let xy = |v: [f64; 3]| {
        let sum: f64 = v.iter().sum();
        [v[0] / sum, v[1] / sum]
    };
    for space in RgbSpace::ALL {
        if let Some(native) = native {
            let white = std::array::from_fn(|r| native.iter().map(|c| c[r]).sum());
            // Standard ICC producers use slightly different rounded reference
            // whites (e.g. colord's 6500K). Compare chromaticities rather than
            // scaled columns; the source's full transform remains authoritative.
            if native
                .map(xy)
                .into_iter()
                .chain([xy(white)])
                .flatten()
                .zip(
                    space
                        .primaries()
                        .into_iter()
                        .chain([space.white()])
                        .flatten(),
                )
                .all(|(a, b)| a.is_finite() && (a - b).abs() <= 0.0002)
            {
                return Ok(Some(space));
            }
            continue;
        }
        let reference = builtin(&context, space)?;
        let target = colorants(&reference).ok_or("Working profile has no colorants")?;
        // Accommodate ICC fixed-point serialization and standard rounded
        // colorant variants. This threshold never controls pixel conversion.
        if source
            .into_iter()
            .flatten()
            .zip(target.into_iter().flatten())
            .all(|(a, b)| a.is_finite() && (a - b).abs() <= 0.0002)
        {
            return Ok(Some(space));
        }
    }
    Ok(None)
}

fn inverse(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    if m.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let mut rows: [[f64; 6]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c < 3 {
                m[r][c]
            } else {
                f64::from(c - 3 == r)
            }
        })
    });
    for k in 0..3 {
        let pivot = (k..3).max_by(|&a, &b| rows[a][k].abs().total_cmp(&rows[b][k].abs()))?;
        rows.swap(k, pivot);
        let scale = rows[k][k];
        if scale.abs() < 1e-12 {
            return None;
        }
        rows[k].iter_mut().for_each(|v| *v /= scale);
        for r in 0..3 {
            if r == k {
                continue;
            }
            let scale = rows[r][k];
            for c in 0..6 {
                rows[r][c] -= scale * rows[k][c];
            }
        }
    }
    Some(rows.map(|r| [r[3], r[4], r[5]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "independent colord ICC fixtures"]
    fn installed_working_profile_suggestions() {
        let directory = std::env::var("LAYER_TEST_WORKING_PROFILES")
            .expect("Set the colord ICC fixture directory");
        for (name, space) in [
            ("sRGB.icc", RgbSpace::Srgb),
            ("AdobeRGB1998.icc", RgbSpace::AdobeRgb),
            ("ProPhotoRGB.icc", RgbSpace::ProPhoto),
        ] {
            let profile = ColorProfile::Icc(
                std::fs::read(std::path::Path::new(&directory).join(name))
                    .unwrap()
                    .into(),
            );
            let suggested = suggested_working_space(&profile).unwrap();
            eprintln!(
                "{name}: {} -> {suggested:?}",
                profile_description(&profile).unwrap()
            );
            assert_eq!(suggested, Some(space));
        }
    }
    #[test]
    fn working_suggestions_ignore_names_and_keep_original_profile() {
        for space in RgbSpace::ALL {
            let context = ThreadContext::new();
            let mut profile = builtin(&context, space).unwrap();
            describe(&mut profile, "Pretend sRGB").unwrap();
            let definition = ColorProfile::Icc(profile.icc().unwrap().into());
            let before = definition.clone();
            assert_eq!(suggested_working_space(&definition).unwrap(), Some(space));
            assert_eq!(profile_description(&definition).unwrap(), "Pretend sRGB");
            assert_eq!(definition, before);
        }
        let context = ThreadContext::new();
        let profile = linear_profile(&context, RgbSpace::DisplayP3).unwrap();
        assert_eq!(
            suggested_working_space(&ColorProfile::Icc(profile.icc().unwrap().into())).unwrap(),
            Some(RgbSpace::DisplayP3)
        );
        let gray = gray_profile(RgbSpace::AdobeRgb).unwrap();
        assert_eq!(suggested_working_space(&gray).unwrap(), None);
    }
}
