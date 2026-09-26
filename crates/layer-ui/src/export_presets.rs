//! Application delivery preferences; hosts own storage and validate ICC support.
//! Profile bytes are retained once, independent of external profile-library files.
use crate::{ExportProfile, ExportRecipe};
use layer_core::{binary_payload, color::{ColorProfile, DocumentColor, ProfileChannels, ProfileReference}};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Named {
    name: String,
    recipe: ExportRecipe<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPresets<P = ColorProfile> {
    profiles: Vec<ExportProfile<P>>,
    destinations: [Option<ExportRecipe<usize>>; 4],
    named: Vec<Named>,
}
impl ExportPresets {
    pub const MAX_NAMES: usize = 64;
    pub const MAX_PROFILE_BYTES: usize = 16 * 1024 * 1024;
    pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
    pub const DESTINATIONS: [&'static str; 4] = [
        "Web / Share",
        "Wide-color image",
        "Further editing",
        "Custom",
    ];

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.named.iter().map(|v| v.name.as_str())
    }
    fn resolve(&self, recipe: &ExportRecipe<usize>) -> Result<ExportRecipe, String> {
        let profile = self
            .profiles
            .get(recipe.profile)
            .ok_or("Preset profile is missing")?;
        let recipe = recipe.clone().with_profile(profile.clone());
        recipe.validate()?;
        Ok(recipe)
    }
    pub fn recipe(&self, index: usize, document: DocumentColor) -> Result<ExportRecipe, String> {
        Ok(self.stored_recipe(index, document)?.for_color(document))
    }
    fn stored_recipe(&self, index: usize, document: DocumentColor) -> Result<ExportRecipe, String> {
        if index < 4 {
            if let Some(recipe) = &self.destinations[index] {
                return self.resolve(recipe);
            }
            return Ok(match index {
                1 => ExportRecipe::wide_color(),
                2 => ExportRecipe::further_editing(document),
                _ => ExportRecipe::web_share(),
            });
        }
        self.resolve(
            &self
                .named
                .get(index - 4)
                .ok_or("Select a saved preset")?
                .recipe,
        )
    }
    fn intern(&mut self, recipe: ExportRecipe) -> ExportRecipe<usize> {
        let index = self
            .profiles
            .iter()
            .position(|p| {
                p.profile == recipe.profile.profile && p.channels == recipe.profile.channels
            })
            .unwrap_or_else(|| {
                self.profiles.push(recipe.profile.clone());
                self.profiles.len() - 1
            });
        recipe.with_profile(index)
    }
    fn compact(&mut self) {
        let profiles = std::mem::take(&mut self.profiles);
        for recipe in self
            .destinations
            .iter_mut()
            .flatten()
            .chain(self.named.iter_mut().map(|v| &mut v.recipe))
        {
            let profile = &profiles[recipe.profile];
            recipe.profile = self
                .profiles
                .iter()
                .position(|p| p == profile)
                .unwrap_or_else(|| {
                    self.profiles.push(profile.clone());
                    self.profiles.len() - 1
                });
        }
    }
    fn change(&mut self, f: impl FnOnce(&mut Self) -> Result<(), String>) -> Result<(), String> {
        self.validate()?;
        let mut next = self.clone();
        f(&mut next)?;
        next.compact();
        next.validate()?;
        *self = next;
        Ok(())
    }
    /// Remember only after a successful delivery. Named presets require an
    /// explicit update; temporary edits to one belong to the Custom destination.
    pub fn remember(&mut self, destination: usize, recipe: ExportRecipe) -> Result<(), String> {
        if destination >= 4 {
            return Err("Choose a delivery destination".into());
        }
        recipe.validate()?;
        self.change(|next| {
            let recipe = next.intern(recipe);
            next.destinations[destination] = Some(recipe);
            Ok(())
        })
    }
    pub fn save(&mut self, name: &str, recipe: ExportRecipe) -> Result<usize, String> {
        let name = name.trim();
        Self::validate_name(name)?;
        if self.named.len() >= Self::MAX_NAMES {
            return Err("The export preset limit is 64".into());
        }
        if Self::DESTINATIONS
            .iter()
            .copied()
            .chain(self.names())
            .any(|v| v.to_lowercase() == name.to_lowercase())
        {
            return Err("An export preset already uses this name".into());
        }
        recipe.validate()?;
        self.change(|next| {
            let recipe = next.intern(recipe);
            next.named.push(Named {
                name: name.into(),
                recipe,
            });
            Ok(())
        })?;
        Ok(self.named.len() + 3)
    }
    pub fn update(&mut self, index: usize, recipe: ExportRecipe) -> Result<(), String> {
        let index = index
            .checked_sub(4)
            .filter(|i| *i < self.named.len())
            .ok_or("Select a saved preset")?;
        recipe.validate()?;
        self.change(|next| {
            let recipe = next.intern(recipe);
            next.named[index].recipe = recipe;
            Ok(())
        })
    }
    pub fn remove(&mut self, index: usize) -> Result<(), String> {
        let index = index
            .checked_sub(4)
            .filter(|i| *i < self.named.len())
            .ok_or("Select a saved preset")?;
        self.change(|next| {
            next.named.remove(index);
            Ok(())
        })
    }
    pub fn reset_destination(&mut self, index: usize) -> Result<(), String> {
        if index >= 4 {
            return Err("Choose a delivery destination".into());
        }
        self.change(|next| {
            next.destinations[index] = None;
            Ok(())
        })
    }
    fn validate_name(name: &str) -> Result<(), String> {
        if name.trim() != name
            || name.is_empty()
            || name.chars().count() > 80
            || name.chars().any(char::is_control)
        {
            return Err(
                "Use a preset name of 1 to 80 characters without control characters".into(),
            );
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.named.len() > Self::MAX_NAMES || self.profiles.len() > Self::MAX_NAMES + 4 {
            return Err("Too many export presets or profiles".into());
        }
        let mut bytes = 0usize;
        for profile in &self.profiles {
            if profile.name.len() > 1024 {
                return Err("Export profile name is too long".into());
            }
            match &profile.profile {
                ColorProfile::Builtin(_) if profile.channels != ProfileChannels::Rgb => {
                    return Err("Builtin export profiles are RGB".into());
                }
                ColorProfile::Icc(data) => {
                    if data.is_empty() {
                        return Err("Preset ICC profile is empty".into());
                    }
                    bytes = bytes.saturating_add(data.len());
                }
                _ => (),
            }
        }
        if bytes > Self::MAX_PROFILE_BYTES {
            return Err("Export preset profiles exceed 16 MiB".into());
        }
        let mut names: Vec<String> = Self::DESTINATIONS
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        for named in &self.named {
            Self::validate_name(&named.name)?;
            let name = named.name.to_lowercase();
            if names.contains(&name) {
                return Err("Duplicate export preset name".into());
            }
            names.push(name);
        }
        for recipe in self
            .destinations
            .iter()
            .flatten()
            .chain(self.named.iter().map(|v| &v.recipe))
        {
            self.resolve(recipe)?;
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut payloads = Vec::new();
        let metadata = ExportPresets {
            profiles: self.profiles.iter().map(|p| p.clone().with_profile(
                ProfileReference::detach(&p.profile, &mut payloads))).collect(),
            destinations: self.destinations.clone(), named: self.named.clone(),
        };
        binary_payload::encode(b"CAPYPRESETS\x01", &metadata, &payloads, Self::MAX_FILE_BYTES)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > Self::MAX_FILE_BYTES {
            return Err("Export preset file exceeds 64 MiB".into());
        }
        let (metadata, blocks): (ExportPresets<ProfileReference>, _) =
            binary_payload::decode(b"CAPYPRESETS\x01", bytes, Self::MAX_FILE_BYTES)?;
        if blocks.len() > Self::MAX_NAMES + 4
            || blocks.iter().map(|b| b.len()).sum::<usize>() > Self::MAX_PROFILE_BYTES {
            return Err("Export preset profiles exceed 16 MiB".into());
        }
        let payloads: Vec<std::sync::Arc<[u8]>> = blocks.into_iter().map(Into::into).collect();
        let mut used = std::collections::BTreeSet::new();
        let profiles = metadata.profiles.into_iter().map(|p| {
            if let ProfileReference::Embedded(id) = &p.profile { used.insert(*id); }
            let profile = p.profile.resolve(&payloads)?;
            Ok(p.with_profile(profile))
        }).collect::<Result<_, String>>()?;
        if used.len() != payloads.len() { return Err("Unused preset ICC payload".into()); }
        let value = Self { profiles, destinations: metadata.destinations, named: metadata.named };
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{SampleDepth, RgbSpace};
    #[test]
    fn binary_profiles_are_compact_and_lossless() {
        let mut library = ExportPresets::default();
        let mut recipe = ExportRecipe::web_share();
        recipe.profile.profile = ColorProfile::Icc((0..2 * 1024 * 1024).map(|n| n as u8).collect::<Vec<_>>().into());
        library.save("Embedded", recipe.clone()).unwrap();
        library.remember(3, recipe).unwrap();
        let bytes = library.encode().unwrap();
        assert_eq!(ExportPresets::decode(&bytes).unwrap(), library);
        assert!(bytes.len() < 2 * 1024 * 1024 + 4096);
        for end in [0, 12, 40, bytes.len() - 1] {
            assert!(ExportPresets::decode(&bytes[..end]).is_err());
        }
        let mut corrupt = bytes; corrupt[100] ^= 1;
        assert!(ExportPresets::decode(&corrupt).is_err());
    }
    #[test]
    fn named_and_remembered_recipes_retain_profiles_independently_and_retire_unused_bytes() {
        let mut library = ExportPresets::default();
        let document = DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        };
        let mut recipe = ExportRecipe::further_editing(document);
        recipe.profile.profile = ColorProfile::Icc(vec![1, 2, 3, 4].into()); // host validates CMM support
        let first = library.save("Lab A", recipe.clone()).unwrap();
        let second = library.save("Lab B", recipe.clone()).unwrap();
        library.remember(2, recipe.clone()).unwrap();
        assert_eq!(library.profiles.len(), 1);
        let bytes = library.encode().unwrap();
        library = ExportPresets::decode(&bytes).unwrap();
        for index in [first, second, 2] {
            assert_eq!(library.recipe(index, document).unwrap(), recipe);
        }
        assert_eq!(
            library.recipe(0, document).unwrap(),
            ExportRecipe::web_share()
        );
        library.update(first, ExportRecipe::wide_color()).unwrap();
        library.remove(second).unwrap();
        assert_eq!(library.profiles.len(), 2);
        library.reset_destination(2).unwrap();
        assert_eq!(library.profiles.len(), 1);
        library.remove(first).unwrap();
        assert!(library.profiles.is_empty());
        assert_eq!(
            library.recipe(2, document).unwrap(),
            ExportRecipe::further_editing(document)
        );
    }
    #[test]
    fn hdr_recipes_resolve_to_sdr_delivery_for_sdr_documents() {
        use crate::{ExportBackground, ExportDraftAction, ExportFormat, ExportSize};
        let hdr = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F16 };
        let sdr = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U8 };
        let mut jpeg = ExportRecipe::web_share().draft(ExportDraftAction::Format(ExportFormat::JpegHdr)).recipe;
        jpeg.jpeg_quality = 75;
        jpeg.size = ExportSize::Fit { bounds: [640, 480], enlarge: false };
        let exr = ExportRecipe::further_editing(hdr);
        let avif = ExportRecipe::web_share().draft(ExportDraftAction::Format(ExportFormat::AvifHdrMapped)).recipe;
        let mut library = ExportPresets::default();
        library.remember(0, jpeg.clone()).unwrap();
        library.remember(2, exr.clone()).unwrap();
        let named = library.save("HDR share", avif.clone()).unwrap();
        assert_eq!(library.recipe(0, hdr).unwrap(), jpeg);
        assert_eq!(library.recipe(2, hdr).unwrap(), exr);
        assert_eq!(library.recipe(named, hdr).unwrap(), avif);
        let share = library.recipe(0, sdr).unwrap();
        assert_eq!((share.format, share.depth, share.background), (ExportFormat::Jpeg, SampleDepth::U8, ExportBackground::White));
        assert_eq!((share.jpeg_quality, &share.size), (75, &jpeg.size));
        let editing = library.recipe(2, sdr).unwrap();
        assert_eq!(editing, ExportRecipe::further_editing(sdr));
        let named = library.recipe(named, sdr).unwrap();
        assert_eq!((named.format, named.background), (ExportFormat::Png, ExportBackground::Preserve));
        for recipe in [share, editing, named] {
            assert!(!recipe.format.is_hdr());
            recipe.validate().unwrap();
        }
    }
    #[test]
    fn invalid_or_oversized_changes_are_atomic_and_corrupt_profiles_never_fallback() {
        let mut library = ExportPresets::default();
        library.save("Lab", ExportRecipe::web_share()).unwrap();
        let before = library.clone();
        for name in ["", "  ", "LAB", "Web / Share", "bad\nname"] {
            assert!(library.save(name, ExportRecipe::web_share()).is_err());
            assert_eq!(library, before);
        }
        let mut recipe = ExportRecipe::web_share();
        recipe.profile.profile =
            ColorProfile::Icc(vec![0; ExportPresets::MAX_PROFILE_BYTES + 1].into());
        assert!(library.update(4, recipe).is_err());
        assert_eq!(library, before);
        let mut malformed = before.clone();
        malformed.named[0].recipe.profile = 100;
        let bytes = binary_payload::encode(b"CAPYPRESETS\x01", &malformed, &[] as &[&[u8]], ExportPresets::MAX_FILE_BYTES).unwrap();
        assert!(ExportPresets::decode(&bytes).unwrap_err().contains("missing"));
        let mut bad = ExportRecipe::web_share();
        bad.jpeg_quality = 0;
        assert!(library.remember(0, bad).is_err());
        assert!(library.remove(3).is_err());
        assert_eq!(library, before);
    }
}

/// Portable worker protocol. Hosts persist the resulting library atomically;
/// listing names never expands every preset's embedded ICC bytes into UI state.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExportPresetAction {
    List,
    Get { index: usize },
    Save { name: String, recipe: ExportRecipe },
    Update { index: usize, recipe: ExportRecipe },
    Remove { index: usize },
    Reset { index: usize },
    Remember { index: usize, recipe: ExportRecipe },
}
#[derive(Serialize)]
pub struct ExportPresetView {
    pub names: Vec<String>,
    pub index: Option<usize>,
    pub recipe: Option<ExportRecipe>,
    pub changed: bool,
}
impl ExportPresets {
    pub fn operate(
        &mut self,
        action: ExportPresetAction,
        color: DocumentColor,
    ) -> Result<ExportPresetView, String> {
        match &action {
            ExportPresetAction::Save { recipe, .. }
            | ExportPresetAction::Update { recipe, .. }
            | ExportPresetAction::Remember { recipe, .. } => recipe.validate_for_color(color)?,
            _ => (),
        }
        let mut changed = true;
        let index = match action {
            ExportPresetAction::List => {
                changed = false;
                None
            }
            ExportPresetAction::Get { index } => {
                changed = false;
                Some(index)
            }
            ExportPresetAction::Save { name, recipe } => Some(self.save(&name, recipe)?),
            ExportPresetAction::Update { index, recipe } => {
                self.update(index, recipe)?;
                Some(index)
            }
            ExportPresetAction::Remove { index } => {
                self.remove(index)?;
                Some(0)
            }
            ExportPresetAction::Reset { index } => {
                self.reset_destination(index)?;
                Some(index)
            }
            ExportPresetAction::Remember { index, recipe } => {
                self.remember(index, recipe)?;
                None
            }
        };
        let recipe = index.map(|index| self.recipe(index, color)).transpose()?;
        if let Some(recipe) = &recipe {
            recipe.validate_for_color(color)?;
        }
        Ok(ExportPresetView {
            names: Self::DESTINATIONS
                .iter()
                .map(|s| (*s).to_string())
                .chain(self.names().map(str::to_string))
                .collect(),
            index,
            recipe,
            changed,
        })
    }
}

#[test]
fn worker_protocol_validates_before_mutation_and_lists_only_names() {
    let mut library = ExportPresets::default();
    let recipe = ExportRecipe::wide_color();
    let hdr = ExportRecipe::web_share()
        .draft(crate::ExportDraftAction::Format(crate::ExportFormat::Exr))
        .recipe;
    assert!(
        library
            .operate(
                ExportPresetAction::Save {
                    name: "Rejected".into(),
                    recipe: hdr
                },
                Default::default(),
            )
            .is_err()
    );
    assert_eq!(library, ExportPresets::default());
    let view = library
        .operate(
            ExportPresetAction::Save {
                name: "Exact output".into(),
                recipe: recipe.clone(),
            },
            Default::default(),
        )
        .unwrap();
    assert_eq!(view.index, Some(4));
    assert!(view.changed);
    let mut restored = ExportPresets::decode(&library.encode().unwrap()).unwrap();
    let view = restored
        .operate(ExportPresetAction::List, Default::default())
        .unwrap();
    assert_eq!(view.names.len(), 5);
    assert!(view.recipe.is_none());
    assert!(!view.changed);
    let view = restored
        .operate(
            ExportPresetAction::Get { index: 4 },
            Default::default(),
        )
        .unwrap();
    assert_eq!(view.recipe, Some(recipe));
    assert!(!view.changed);
}
