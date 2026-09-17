//! Native profile selection; parsing and CMM validation stay on a file worker.
use super::*;
use layer_core::color::{ColorProfile, ProfileChannels};
use layer_core::color::{RgbSpace, source::SourceInterpretation};
use std::cell::{Cell, RefCell};
use std::io::Read;
mod library;
pub(crate) use library::manage;

#[derive(Clone)]
pub(super) enum ProfilePurpose {
    Proof,
    Output,
    Source(SourceInterpretation),
}
impl ProfilePurpose {
    pub(super) fn validate(
        &self,
        profile: &ExportProfile,
        working: RgbSpace,
    ) -> Result<(), String> {
        match self {
            Self::Proof => {
                let recipe = layer_core::color::ProofRecipe::new(
                    profile.name.clone(),
                    profile.profile.clone(),
                );
                layer_color::ProofTransform::new(working, &recipe)?;
            }
            Self::Output => {
                // An input profile need not be usable for delivery.
                let recipe = ExportRecipe {
                    format: ExportFormat::Tiff,
                    profile: profile.clone(),
                    background: ExportBackground::White,
                    ..ExportRecipe::web_share()
                };
                let encoder = layer_color::WorkingEncoder::new(
                    working,
                    &recipe.interpretation(),
                    Default::default(),
                )?;
                let mut output = vec![0; recipe.interpretation().pixel_bytes() * 3];
                encoder.encode_straight(
                    &[[0., 0., 0., 1.], [0.5, 0.5, 0.5, 1.], [1.; 4]],
                    &mut output,
                    None,
                    [0, 0],
                )?;
            }
            Self::Source(source) => {
                use layer_core::color::source::SourceChannels;
                let (expected, label) = match source.channels {
                    SourceChannels::Rgb | SourceChannels::Rgba => (ProfileChannels::Rgb, "RGB"),
                    SourceChannels::Gray | SourceChannels::GrayAlpha => {
                        (ProfileChannels::Gray, "grayscale")
                    }
                    SourceChannels::Cmyk => (ProfileChannels::Cmyk, "CMYK"),
                };
                if profile.channels != expected {
                    return Err(format!(
                        "This image is {label}. Choose a matching {label} source profile."
                    ));
                }
                let mut source = source.clone();
                source.profile = profile.profile.clone();
                layer_color::WorkingDecoder::new(&source, working, Default::default())?;
            }
        }
        Ok(())
    }
}

mod picker;
pub(super) use picker::ProfileChooser;
pub(super) const UNNAMED_PROFILE: &str = "Embedded ICC profile";

// Keep a replaced proof locally; the project continues to embed only its active proof.
pub(super) async fn preserve_replaced_proof(
    previous: Option<&layer_core::color::ProofRecipe>,
    next: &layer_core::color::ProofRecipe,
) -> Result<(), String> {
    let Some(previous) = previous.filter(|p| p.profile != next.profile) else {
        return Ok(());
    };
    let ColorProfile::Icc(bytes) = &previous.profile else {
        return Ok(());
    };
    let bytes = bytes.clone();
    let name = previous.name.clone();
    gio::spawn_blocking(move || library::store(&library::directory(), &bytes, &name).map(|_| ()))
        .await
        .map_err(|_| "Profile library worker failed".to_string())?
        .map_err(|error| {
            format!("Could not save the previous proof profile to Saved Profiles: {error}")
        })
}

pub(super) fn describe(profile: ColorProfile) -> Result<ExportProfile, String> {
    let channels = layer_color::profile_channels(&profile)?;
    let name = layer_color::profile_description(&profile)?;
    Ok(ExportProfile {
        profile,
        channels,
        name,
    })
}

pub(super) fn read(
    path: &std::path::Path,
    working: RgbSpace,
    purpose: &ProfilePurpose,
) -> Result<ExportProfile, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(layer_color::MAX_ICC_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > layer_color::MAX_ICC_BYTES {
        return Err("ICC profile exceeds the size limit".into());
    }
    let profile = ColorProfile::Icc(bytes.into());
    let channels = layer_color::profile_channels(&profile)?;
    let fallback: String = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .take(128)
        .collect();
    let name = layer_color::profile_description(&profile)
        .ok()
        .filter(|name| !name.trim().is_empty() && name != UNNAMED_PROFILE)
        .unwrap_or(fallback);
    let result = ExportProfile {
        profile,
        channels,
        name,
    };
    purpose.validate(&result, working)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_loading_keeps_bytes_and_rejects_corrupt_or_oversized_files() {
        let dir = std::env::temp_dir().join(format!("capy-output-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, profile, channels) in [
            (
                "RGB.icc",
                ColorProfile::Builtin(RgbSpace::AdobeRgb),
                ProfileChannels::Rgb,
            ),
            (
                "Gray.icc",
                layer_color::gray_profile(RgbSpace::ProPhoto).unwrap(),
                ProfileChannels::Gray,
            ),
        ] {
            let bytes = layer_color::profile_bytes(&profile).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, &bytes).unwrap();
            let loaded = read(&path, RgbSpace::DisplayP3, &ProfilePurpose::Output).unwrap();
            assert_eq!(
                loaded.name,
                layer_color::profile_description(&profile).unwrap()
            );
            assert_eq!(loaded.channels, channels);
            assert_eq!(loaded.profile, ColorProfile::Icc(bytes.into()));
        }
        let path = dir.join("invalid.icc");
        std::fs::write(&path, b"not a profile").unwrap();
        assert!(read(&path, RgbSpace::Srgb, &ProfilePurpose::Output).is_err());
        std::fs::File::create(&path)
            .unwrap()
            .set_len(layer_color::MAX_ICC_BYTES as u64 + 1)
            .unwrap();
        assert!(
            read(&path, RgbSpace::Srgb, &ProfilePurpose::Output)
                .unwrap_err()
                .contains("size limit")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;
    use layer_core::color::{SampleDepth, source::SourceChannels};
    #[test]
    fn source_roles_validate_actual_channels_without_requiring_delivery() {
        let path =
            std::env::temp_dir().join(format!("capy-source-profile-{}.icc", std::process::id()));
        let rgb = ColorProfile::Builtin(RgbSpace::DisplayP3);
        let gray = layer_color::gray_profile(RgbSpace::Srgb).unwrap();
        for (channels, profile, valid) in [
            (SourceChannels::Rgba, &rgb, true),
            (SourceChannels::Rgba, &gray, false),
            (SourceChannels::GrayAlpha, &gray, true),
            (SourceChannels::GrayAlpha, &rgb, false),
            (SourceChannels::Cmyk, &rgb, false),
        ] {
            let bytes = layer_color::profile_bytes(profile).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            let purpose = ProfilePurpose::Source(SourceInterpretation {
                channels,
                depth: SampleDepth::U16,
                profile: rgb.clone(),
                profile_assumed: false,
            });
            let result = read(&path, RgbSpace::ProPhoto, &purpose);
            assert_eq!(result.is_ok(), valid, "{channels:?}: {result:?}");
            if let Ok(loaded) = result {
                assert_eq!(loaded.profile, ColorProfile::Icc(bytes.into()));
            }
        }
        std::fs::remove_file(path).unwrap();
    }
}
