//! Color policies apply to future documents; retained originals stay unchanged.
use super::*;
use layer_core::color::{SampleDepth, RgbSpace};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingProfilePolicy {
    #[default]
    AssumeSrgb,
    Ask,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhotoOpenPolicy {
    pub promote_to_16: bool,
    pub missing_profile: MissingProfilePolicy,
}
impl PhotoOpenPolicy {
    pub fn editing_depth(self, source: SampleDepth) -> SampleDepth {
        if self.promote_to_16 && !source.is_float() {
            SampleDepth::U16
        } else {
            source
        }
    }
}

impl Settings {
    pub(super) fn color_groups(&self, platform: Platform) -> Vec<PreferenceGroup> {
        use PreferenceId::*;
        let choice = |id, title: &str, description: &str, options: &[&str], selected| {
            row(
                id,
                title,
                description,
                PreferenceKind::Choice {
                    presentation: ChoicePresentation::Dropdown,
                    options: options.iter().map(|s| (*s).into()).collect(),
                    icons: vec![],
                    selected,
                },
            )
        };
        let defaults = self.new_document.defaults;
        vec![
            PreferenceGroup {
                title: "New drawings".into(),
                rows: vec![
                    choice(
                        NewColorSpace,
                        "Color space",
                        "Defaults apply to future drawings.",
                        &RgbSpace::ALL.map(|s| s.name()),
                        RgbSpace::ALL
                            .iter()
                            .position(|s| *s == defaults.color.space)
                            .unwrap() as u32,
                    ),
                    choice(
                        NewBitDepth,
                        "Bit depth",
                        "16-bit improves precision for subsequent edits.",
                        if platform == Platform::Gtk { &["8-bit SDR", "16-bit SDR", "16-bit float HDR", "32-bit float HDR"] }
                        else { &["8-bit SDR", "16-bit SDR"] },
                        match defaults.color.depth { SampleDepth::U8 => 0, SampleDepth::U16 => 1, SampleDepth::F16 => 2, SampleDepth::F32 => 3 },
                    ),
                    choice(
                        NewBackground,
                        "Background",
                        "",
                        &["White", "Transparent"],
                        u32::from(defaults.background == DocumentBackground::Transparent),
                    ),
                ],
            },
            PreferenceGroup {
                title: "Opening photos".into(),
                rows: vec![
                    choice(
                        PhotoDepth,
                        "Editing precision",
                        "Retain original samples and embedded profiles.",
                        &["Source depth", "16-bit"],
                        u32::from(self.photo_open.promote_to_16),
                    ),
                    choice(
                        MissingProfile,
                        "Untagged RGB and grayscale",
                        "Tagged photos keep their profiles without a prompt.",
                        &["Assume sRGB", "Ask"],
                        u32::from(self.photo_open.missing_profile == MissingProfilePolicy::Ask),
                    ),
                ],
            },
        ]
    }
    pub(super) fn edit_color(&mut self, id: PreferenceId, value: u32) {
        use PreferenceId::*;
        match id {
            NewColorSpace => self.new_document.defaults.color.space = RgbSpace::ALL[value as usize],
            NewBitDepth => {
                self.new_document.defaults.color.depth = [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32][value as usize]
            }
            NewBackground => {
                self.new_document.defaults.background = if value == 0 {
                    DocumentBackground::White
                } else {
                    DocumentBackground::Transparent
                }
            }
            PhotoDepth => self.photo_open.promote_to_16 = value == 1,
            MissingProfile => {
                self.photo_open.missing_profile = if value == 0 {
                    MissingProfilePolicy::AssumeSrgb
                } else {
                    MissingProfilePolicy::Ask
                }
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn future_document_policies_validate_and_round_trip_independently() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
            let mut settings = Settings::default();
            let existing = settings.new_document.defaults.project().unwrap();
            for (id, value) in [
                (PreferenceId::NewColorSpace, 3),
                (PreferenceId::NewBitDepth, 1),
                (PreferenceId::NewBackground, 1),
                (PreferenceId::PhotoDepth, 1),
                (PreferenceId::MissingProfile, 1),
            ] {
                settings
                    .edit(id, PreferenceValue::Choice(value), platform)
                    .unwrap();
            }
            let saved = serde_json::to_string(&settings).unwrap();
            assert_eq!(serde_json::from_str::<Settings>(&saved).unwrap(), settings);
            settings.validate().unwrap();
            assert_eq!(existing.document.color, Default::default());
            let new = settings.new_document.defaults.project().unwrap();
            assert_eq!(new.document.color.space, RgbSpace::ProPhoto);
            assert_eq!(new.document.color.depth, SampleDepth::U16);
            assert!(!new.document.layers[1].visible);
            assert_eq!(
                settings.photo_open.editing_depth(SampleDepth::U8),
                SampleDepth::U16
            );
            assert_eq!(
                PhotoOpenPolicy::default().editing_depth(SampleDepth::U8),
                SampleDepth::U8
            );
            assert_eq!(
                settings.photo_open.missing_profile,
                MissingProfilePolicy::Ask
            );
            let before = settings.clone();
            assert!(
                settings
                    .edit(
                        PreferenceId::NewColorSpace,
                        PreferenceValue::Choice(4),
                        platform
                    )
                    .is_err()
            );
            assert!(
                settings
                    .edit(
                        PreferenceId::PhotoDepth,
                        PreferenceValue::Choice(0),
                        Platform::Generic
                    )
                    .is_err()
            );
            assert_eq!(settings, before);
        }
    }
}
