//! Color policies apply to future documents; retained originals stay unchanged.
use super::*;
use layer_core::color::SampleDepth;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingProfilePolicy {
    #[default]
    AssumeSrgb,
    Ask,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
    pub(super) fn color_groups(&self, localizer: &Localizer) -> Vec<PreferenceGroup> {
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
        vec![
            PreferenceGroup {
                title: localizer.text(MessageId::SETTINGS_OPENING_PHOTOS).to_string(),
                rows: vec![
                    choice(
                        PhotoDepth,
                        &localizer.text(MessageId::SETTINGS_EDITING_PRECISION),
                        &localizer.text(MessageId::SETTINGS_RETAIN_ORIGINAL_SAMPLES_AND_EMBEDDED_PROFILES),
                        &[&localizer.text(MessageId::SETTINGS_SOURCE_DEPTH), &localizer.text(MessageId::SETTINGS_16_BIT)],
                        u32::from(self.photo_open.promote_to_16),
                    ),
                    choice(
                        MissingProfile,
                        &localizer.text(MessageId::SETTINGS_UNTAGGED_RGB_AND_GRAYSCALE),
                        &localizer.text(MessageId::SETTINGS_TAGGED_PHOTOS_KEEP_THEIR_PROFILES_WITHOUT_A_PROMPT),
                        &[&localizer.text(MessageId::SETTINGS_ASSUME_SRGB), &localizer.text(MessageId::SETTINGS_ASK)],
                        u32::from(self.photo_open.missing_profile == MissingProfilePolicy::Ask),
                    ),
                ],
            },
        ]
    }
    pub(super) fn edit_color(&mut self, id: PreferenceId, value: u32) {
        use PreferenceId::*;
        match id {
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
    fn photo_opening_policies_validate_and_round_trip_independently() {
        for platform in Platform::ALL {
            let mut settings = Settings::default();
            for (id, value) in [(PreferenceId::PhotoDepth, 1), (PreferenceId::MissingProfile, 1)] {
                settings.edit(id, PreferenceValue::Choice(value), platform).unwrap();
            }
            let saved = serde_json::to_string(&settings).unwrap();
            assert_eq!(serde_json::from_str::<Settings>(&saved).unwrap(), settings);
            settings.validate().unwrap();
            assert_eq!(settings.photo_open.editing_depth(SampleDepth::U8), SampleDepth::U16);
            assert_eq!(PhotoOpenPolicy::default().editing_depth(SampleDepth::U8), SampleDepth::U8);
            assert_eq!(settings.photo_open.missing_profile, MissingProfilePolicy::Ask);
            let before = settings.clone();
            assert!(settings.edit(PreferenceId::MissingProfile, PreferenceValue::Choice(2), platform).is_err());
            assert_eq!(settings, before);
        }
    }
}
