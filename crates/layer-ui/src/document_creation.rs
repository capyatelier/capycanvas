//! Portable creation choices and reusable presets. Display state is never a
//! document color default; precision and working RGB remain independent.
use crate::{DEFAULT_DOCUMENT_EXTENT, MAX_NEW_DOCUMENT_DIMENSION};
use layer_core::{
    Document, Project,
    color::{DocumentColor, IntegerDepth, RgbSpace},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentBackground {
    #[default]
    White,
    Transparent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDocumentOptions {
    pub extent: [u32; 2],
    pub color: DocumentColor,
    pub background: DocumentBackground,
}
impl Default for NewDocumentOptions {
    fn default() -> Self {
        Self {
            extent: DEFAULT_DOCUMENT_EXTENT,
            color: DocumentColor::default(),
            background: DocumentBackground::White,
        }
    }
}
impl NewDocumentOptions {
    pub fn validate(self) -> Result<(), String> {
        if self
            .extent
            .iter()
            .any(|v| *v == 0 || *v > MAX_NEW_DOCUMENT_DIMENSION)
        {
            return Err(format!(
                "Choose a canvas size from 1 to {MAX_NEW_DOCUMENT_DIMENSION} pixels"
            ));
        }
        Ok(())
    }
    pub fn project(self) -> Result<Project, String> {
        self.validate()?;
        let mut document = Document::new("untitled", self.extent[0], self.extent[1]);
        document.color = self.color;
        document.layers[1].visible = self.background == DocumentBackground::White;
        Ok(Project {
            document,
            assets: Default::default(),
        })
    }
    pub fn description(self) -> String {
        format!(
            "{} × {} px · {} · {}-bit SDR · {}",
            self.extent[0],
            self.extent[1],
            self.color.space.name(),
            self.color.depth.bits(),
            if self.background == DocumentBackground::White {
                "White"
            } else {
                "Transparent"
            }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDocumentPreset {
    pub name: String,
    pub options: NewDocumentOptions,
}
impl NewDocumentPreset {
    pub fn builtins() -> [Self; 3] {
        [
            ("Standard drawing", RgbSpace::Srgb, IntegerDepth::U8),
            ("Wide color", RgbSpace::DisplayP3, IntegerDepth::U8),
            ("Photo editing", RgbSpace::ProPhoto, IntegerDepth::U16),
        ]
        .map(|(name, space, depth)| Self {
            name: name.into(),
            options: NewDocumentOptions {
                color: DocumentColor { space, depth },
                ..Default::default()
            },
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDocumentSettings {
    pub defaults: NewDocumentOptions,
    pub presets: Vec<NewDocumentPreset>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NewDocumentAction {
    Remember { options: NewDocumentOptions, name: String, defaults: bool },
    Remove { index: usize },
}
impl NewDocumentSettings {
    pub fn apply(&mut self, action: NewDocumentAction) -> Result<(), String> {
        let mut next = self.clone();
        match action {
            NewDocumentAction::Remember { options, name, defaults } => {
                options.validate()?;
                if !name.trim().is_empty() {
                    next.presets.push(NewDocumentPreset { name: name.trim().into(), options });
                }
                if defaults { next.defaults = options; }
            }
            NewDocumentAction::Remove { index } => {
                if index >= next.presets.len() { return Err("Drawing preset is unavailable".into()); }
                next.presets.remove(index);
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        self.defaults.validate()?;
        if self.presets.len() > 64 {
            return Err("Keep at most 64 saved drawing presets".into());
        }
        let mut names: Vec<String> = NewDocumentPreset::builtins()
            .iter()
            .map(|p| p.name.to_lowercase())
            .collect();
        for preset in &self.presets {
            preset.options.validate()?;
            let name = preset.name.trim();
            if name != preset.name
                || name.is_empty()
                || name.chars().count() > 64
                || name.chars().any(char::is_control)
            {
                return Err("Use a preset name with 1 to 64 characters".into());
            }
            if names.contains(&name.to_lowercase()) {
                return Err("A drawing preset already has that name".into());
            }
            names.push(name.to_lowercase());
        }
        Ok(())
    }
    pub fn save(&mut self, name: &str, options: NewDocumentOptions) -> Result<(), String> {
        let mut next = self.clone();
        next.presets.push(NewDocumentPreset {
            name: name.trim().into(),
            options,
        });
        next.validate()?;
        *self = next;
        Ok(())
    }
}

/// Native and browser creation sheets use the same independent options.
#[derive(Serialize)]
pub struct NewDocumentForm {
    pub options: NewDocumentOptions,
    pub presets: Vec<NewDocumentPreset>,
    pub spaces: Vec<(RgbSpace, &'static str)>,
}
impl NewDocumentSettings {
    pub fn form(&self) -> NewDocumentForm {
        NewDocumentForm {
            options: self.defaults,
            presets: NewDocumentPreset::builtins().into_iter().chain(self.presets.iter().cloned()).collect(),
            spaces: RgbSpace::ALL.into_iter().map(|space| (space, space.name())).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preference_actions_validate_before_changing_defaults_or_presets() {
        let mut settings = NewDocumentSettings::default();
        let options = NewDocumentOptions { extent: [513, 257], ..Default::default() };
        settings.apply(NewDocumentAction::Remember { options, name: " Photo ".into(), defaults: true }).unwrap();
        let saved = settings.clone();
        assert!(settings.apply(NewDocumentAction::Remember { options: Default::default(), name: "photo".into(), defaults: true }).is_err());
        assert_eq!(settings, saved);
        assert!(settings.apply(NewDocumentAction::Remove { index: 1 }).is_err());
        assert_eq!(settings, saved);
        settings.apply(NewDocumentAction::Remove { index: 0 }).unwrap();
        assert!(settings.presets.is_empty());
        assert_eq!(settings.defaults, options);
    }
    #[test]
    fn creation_preserves_independent_depth_space_and_background() {
        for space in RgbSpace::ALL {
            for depth in [IntegerDepth::U8, IntegerDepth::U16] {
                for background in [DocumentBackground::White, DocumentBackground::Transparent] {
                    let options = NewDocumentOptions {
                        extent: [513, 257],
                        color: DocumentColor { space, depth },
                        background,
                    };
                    let project = options.project().unwrap();
                    assert_eq!(project.document.color, options.color);
                    assert_eq!(
                        project.document.layers[1].visible,
                        background == DocumentBackground::White
                    );
                    let mut bytes = Vec::new();
                    project.write(&mut bytes).unwrap();
                    assert_eq!(
                        Project::read(std::io::Cursor::new(bytes), Default::default()).unwrap(),
                        project
                    );
                }
            }
        }
        assert_eq!(
            NewDocumentPreset::builtins()[0].options,
            NewDocumentOptions::default()
        );
    }
    #[test]
    fn saved_presets_validate_atomically_and_roundtrip() {
        let mut settings = NewDocumentSettings::default();
        let options = NewDocumentPreset::builtins()[2].options;
        settings.save(" My photo ", options).unwrap();
        settings.defaults = options;
        let before = settings.clone();
        assert!(settings.save("my PHOTO", options).is_err());
        assert!(settings.save("Wide color", options).is_err());
        assert!(
            settings
                .save(
                    "Bad size",
                    NewDocumentOptions {
                        extent: [0, 8193],
                        ..options
                    }
                )
                .is_err()
        );
        assert_eq!(settings, before);
        assert_eq!(
            serde_json::from_str::<NewDocumentSettings>(&serde_json::to_string(&settings).unwrap())
                .unwrap(),
            settings
        );
    }
}
