//! Workspace-owned palettes. IDs survive rename/delete; every saved swatch
//! carries its own color definition and never stores a clipped display preview.
use super::*;
#[path = "palette_reorder.rs"]
mod reorder;
pub use reorder::ColorReorderPreview;
#[path = "starter_palettes.rs"]
mod starters;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedColor {
    pub id: u64,
    pub name: String,
    pub color: RgbColor,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorPalette {
    pub id: u64,
    pub name: String,
    pub swatches: Vec<SavedColor>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorLibrary {
    pub palettes: Vec<ColorPalette>,
    pub active: u64,
    pub history: Vec<RgbColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_name: Option<(RgbColor, String)>,
    starters_installed: bool,
    #[serde(skip)]
    reorders: reorder::ReorderHistory,
    next_id: u64,
}
#[derive(Default)]
struct UniqueNames {
    used: std::collections::BTreeSet<String>,
    suffixes: std::collections::BTreeMap<String, u32>,
}
impl UniqueNames {
    fn claim(&mut self, base: String) -> String {
        if self.used.insert(base.to_lowercase()) {
            return base;
        }
        let stem = base.chars().take(55).collect::<String>();
        let suffix = self.suffixes.entry(stem.to_lowercase()).or_insert(2);
        loop {
            let candidate = format!("{stem} {suffix}");
            *suffix += 1;
            if self.used.insert(candidate.to_lowercase()) {
                return candidate;
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ColorLibraryAction {
    NameCurrent {
        color: RgbColor,
        name: String,
    },
    SelectPalette {
        id: u64,
    },
    Import {
        name: String,
        swatches: Vec<(String, RgbColor)>,
    },
    CreatePalette {
        name: String,
    },
    RenamePalette {
        id: u64,
        name: String,
    },
    RemovePalette {
        id: u64,
    },
    Store {
        palette: u64,
        name: String,
        color: RgbColor,
    },
    Rename {
        id: u64,
        name: String,
    },
    Remove {
        id: u64,
    },
    Use {
        id: u64,
    },
    Reorder {
        palette: u64,
        id: u64,
        before: Option<u64>,
    },
    UndoReorder {
        palette: u64,
    },
    RedoReorder {
        palette: u64,
    },
}
impl Default for ColorLibrary {
    fn default() -> Self {
        Self {
            palettes: vec![ColorPalette {
                id: 1,
                name: "My colors".into(),
                swatches: Vec::new(),
            }],
            next_id: 2,
            starters_installed: false,
            reorders: Default::default(),
            active: 1,
            history: Vec::new(),
            pending_name: None,
        }
    }
}
fn name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 64 || value.chars().any(char::is_control) {
        return Err("Choose a name of 1–64 characters without control characters".into());
    }
    Ok(value.split_whitespace().collect::<Vec<_>>().join(" "))
}
impl ColorLibrary {
    pub const MAX_PALETTES: usize = 64;
    pub const MAX_SWATCHES: usize = 4096;
    pub const MAX_HISTORY: usize = 64;
    pub fn active_palette(&self) -> &ColorPalette {
        self.palettes
            .iter()
            .find(|p| p.id == self.active)
            .unwrap_or(&self.palettes[0])
    }
    /// Only successful artwork operations call this, never picker actions.
    pub(crate) fn record_use(&mut self, color: RgbColor) {
        if color.rgba[3] == 0. || self.history.first() == Some(&color) {
            return;
        }
        self.history.retain(|c| *c != color);
        self.history.insert(0, color);
        self.history.truncate(Self::MAX_HISTORY);
    }
    pub fn hex_preview(color: RgbColor) -> String {
        let rgba = color.encoded_in(RgbSpace::Srgb).expect("validated color");
        format!(
            "#{:02X}{:02X}{:02X}",
            (rgba[0].clamp(0., 1.) * 255.).round() as u8,
            (rgba[1].clamp(0., 1.) * 255.).round() as u8,
            (rgba[2].clamp(0., 1.) * 255.).round() as u8
        )
    }
    pub fn suggested_name(color: RgbColor) -> String {
        let p = color.encoded_in(RgbSpace::Srgb).expect("validated color");
        let [r, g, b] = [p[0], p[1], p[2]].map(|v| v.clamp(0., 1.));
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let label = if max < 0.12 {
            "Ink"
        } else if delta < 0.08 {
            if min > 0.9 {
                "White"
            } else if max < 0.4 {
                "Charcoal"
            } else {
                "Gray"
            }
        } else {
            match components([r, g, b, 1.], ColorSpace::Hsv, 0.)[0] as u32 {
                0..=19 | 345..=359 => {
                    if max < 0.65 {
                        "Brick"
                    } else {
                        "Coral"
                    }
                }
                20..=44 => {
                    if max < 0.65 {
                        "Umber"
                    } else if delta < 0.45 {
                        "Sand"
                    } else {
                        "Amber"
                    }
                }
                45..=69 => {
                    if delta < 0.4 {
                        "Linen"
                    } else {
                        "Gold"
                    }
                }
                70..=159 => "Green",
                160..=194 => "Teal",
                195..=254 => "Blue",
                255..=284 => "Violet",
                _ => "Rose",
            }
        };
        label.into()
    }
    pub fn current_name(&self, color: RgbColor) -> Option<&str> {
        self.pending_name
            .as_ref()
            .filter(|(c, _)| *c == color)
            .map(|(_, name)| name.as_str())
    }
    fn unique_name(base: String, names: impl Iterator<Item = String>) -> String {
        UniqueNames { used: names.map(|n| n.to_lowercase()).collect(), ..Default::default() }.claim(base)
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.history.len() > Self::MAX_HISTORY {
            return Err("Color history exceeds its supported size".into());
        }
        for color in &self.history {
            ColorState::validate_definition(*color)?;
        }
        if let Some((color, value)) = &self.pending_name {
            ColorState::validate_definition(*color)?;
            name(value)?;
        }
        if self.palettes.is_empty()
            || self.palettes.len() > Self::MAX_PALETTES
            || self
                .palettes
                .iter()
                .map(|p| p.swatches.len())
                .sum::<usize>()
                > Self::MAX_SWATCHES
        {
            return Err("Palette library exceeds its supported size".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        for palette in &self.palettes {
            if name(&palette.name).is_err() || !names.insert(palette.name.to_lowercase()) {
                return Err("Palette names must be unique".into());
            }
            if palette.id == 0 || palette.id >= self.next_id || !ids.insert(palette.id) {
                return Err("Invalid palette ID".into());
            }
            let mut swatch_names = std::collections::BTreeSet::new();
            for swatch in &palette.swatches {
                if name(&swatch.name)? != swatch.name
                    || !swatch_names.insert(swatch.name.to_lowercase())
                {
                    return Err("Color names must be unique within a palette".into());
                }
                if swatch.id == 0 || swatch.id >= self.next_id || !ids.insert(swatch.id) {
                    return Err("Invalid swatch ID".into());
                }
                ColorState::validate_definition(swatch.color)?;
            }
        }
        Ok(())
    }
    pub fn swatch(&self, id: u64) -> Option<&SavedColor> {
        self.palettes
            .iter()
            .flat_map(|p| &p.swatches)
            .find(|s| s.id == id)
    }
    fn palette_name(&self, id: Option<u64>, value: &str) -> Result<String, String> {
        let value = name(value)?;
        if self
            .palettes
            .iter()
            .any(|p| Some(p.id) != id && p.name.to_lowercase() == value.to_lowercase())
        {
            return Err("A palette already uses that name".into());
        }
        Ok(value)
    }
    pub fn apply(&mut self, action: ColorLibraryAction) -> Result<Option<RgbColor>, String> {
        match action {
            ColorLibraryAction::NameCurrent { color, name: value } => {
                ColorState::validate_definition(color)?;
                let name = self.swatch_name(self.active_palette().id, None, &value, color)?;
                self.pending_name = Some((color, name));
            }
            ColorLibraryAction::SelectPalette { id } => {
                if !self.palettes.iter().any(|p| p.id == id) {
                    return Err("Palette no longer exists".into());
                }
                self.active = id;
            }
            ColorLibraryAction::Import {
                name: value,
                swatches,
            } => {
                // Validate on a copy so malformed imports never leave partial palettes.
                if swatches.is_empty() {
                    return Err("This palette contains no colors".into());
                }
                if self
                    .palettes
                    .iter()
                    .map(|p| p.swatches.len())
                    .sum::<usize>()
                    + swatches.len()
                    > Self::MAX_SWATCHES
                {
                    return Err("The library can hold at most 4096 colors".into());
                }
                let mut next = self.clone();
                let base = if value.trim().is_empty() {
                    "Imported palette".into()
                } else {
                    name(&value)?
                };
                let value = Self::unique_name(base, next.palettes.iter().map(|p| p.name.clone()));
                next.apply(ColorLibraryAction::CreatePalette { name: value })?;
                let end = next
                    .next_id
                    .checked_add(swatches.len() as u64)
                    .ok_or("Swatch IDs exhausted")?;
                let mut names = UniqueNames::default();
                for (value, color) in swatches {
                    ColorState::validate_definition(color)?;
                    let base = if value.trim().is_empty() {
                        Self::suggested_name(color)
                    } else {
                        name(&value)?
                    };
                    let value = names.claim(base);
                    next.palettes.last_mut().unwrap().swatches.push(SavedColor {
                        id: next.next_id,
                        name: value,
                        color,
                    });
                    next.next_id += 1;
                }
                debug_assert_eq!(next.next_id, end);
                *self = next;
            }
            ColorLibraryAction::CreatePalette { name } => {
                let name = if name.trim().is_empty() {
                    Self::unique_name(
                        "New palette".into(),
                        self.palettes.iter().map(|p| p.name.clone()),
                    )
                } else {
                    self.palette_name(None, &name)?
                };
                if self.palettes.len() >= Self::MAX_PALETTES {
                    return Err("The library already has 64 palettes".into());
                }
                let next = self.next_id.checked_add(1).ok_or("Palette IDs exhausted")?;
                self.palettes.push(ColorPalette {
                    id: self.next_id,
                    name,
                    swatches: Vec::new(),
                });
                self.active = self.next_id;
                self.next_id = next;
            }
            ColorLibraryAction::RenamePalette { id, name } => {
                let name = self.palette_name(Some(id), &name)?;
                self.palettes
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("Palette no longer exists")?
                    .name = name;
            }
            ColorLibraryAction::RemovePalette { id } => {
                let index = self
                    .palettes
                    .iter()
                    .position(|p| p.id == id)
                    .ok_or("Palette no longer exists")?;
                if self.palettes.len() == 1 {
                    return Err("Keep at least one palette".into());
                }
                self.palettes.remove(index);
                self.forget_reorders(id);
                if self.active == id {
                    self.active = self.palettes[0].id;
                }
            }
            ColorLibraryAction::Store {
                palette,
                name: value,
                color,
            } => {
                ColorState::validate_definition(color)?;
                let name = self.swatch_name(palette, None, &value, color)?;
                if self
                    .palettes
                    .iter()
                    .map(|p| p.swatches.len())
                    .sum::<usize>()
                    >= Self::MAX_SWATCHES
                {
                    return Err("The library already has 4096 swatches".into());
                }
                let next = self.next_id.checked_add(1).ok_or("Swatch IDs exhausted")?;
                let palette = self
                    .palettes
                    .iter_mut()
                    .find(|p| p.id == palette)
                    .ok_or("Palette no longer exists")?;
                palette.swatches.push(SavedColor {
                    id: self.next_id,
                    name,
                    color,
                });
                let palette_id = palette.id;
                self.forget_reorders(palette_id);
                self.next_id = next;
            }
            ColorLibraryAction::Rename { id, name: value } => {
                let palette = self
                    .palettes
                    .iter()
                    .find(|p| p.swatches.iter().any(|s| s.id == id))
                    .ok_or("Swatch no longer exists")?;
                let color = self.swatch(id).unwrap().color;
                let name = self.swatch_name(palette.id, Some(id), &value, color)?;
                self.palettes
                    .iter_mut()
                    .flat_map(|p| &mut p.swatches)
                    .find(|s| s.id == id)
                    .ok_or("Swatch no longer exists")?
                    .name = name;
            }
            ColorLibraryAction::Remove { id } => {
                let palette = self
                    .palettes
                    .iter_mut()
                    .find(|p| p.swatches.iter().any(|s| s.id == id))
                    .ok_or("Swatch no longer exists")?;
                palette.swatches.retain(|s| s.id != id);
                let palette_id = palette.id;
                self.forget_reorders(palette_id);
            }
            ColorLibraryAction::Use { id } => {
                return Ok(Some(
                    self.swatch(id).ok_or("Swatch no longer exists")?.color,
                ));
            }
            ColorLibraryAction::Reorder {
                palette,
                id,
                before,
            } => self.reorder(palette, id, before)?,
            ColorLibraryAction::UndoReorder { palette } => self.restore_reorder(palette, false)?,
            ColorLibraryAction::RedoReorder { palette } => self.restore_reorder(palette, true)?,
        }
        Ok(None)
    }
    fn swatch_name(
        &self,
        palette: u64,
        id: Option<u64>,
        value: &str,
        color: RgbColor,
    ) -> Result<String, String> {
        let palette = self
            .palettes
            .iter()
            .find(|p| p.id == palette)
            .ok_or("Palette no longer exists")?;
        let others = || palette.swatches.iter().filter(|s| Some(s.id) != id);
        if value.trim().is_empty() {
            let base = self
                .current_name(color)
                .map(str::to_owned)
                .unwrap_or_else(|| Self::suggested_name(color));
            return Ok(Self::unique_name(base, others().map(|s| s.name.clone())));
        }
        let value = name(value)?;
        if others().any(|s| s.name.to_lowercase() == value.to_lowercase()) {
            return Err("Another color in this palette already has that name".into());
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_imports_keep_ids_and_definitions() {
        let mut library = ColorLibrary::default();
        let color = RgbColor::from_linear(RgbSpace::DisplayP3, [4., 0.1, 0.3, 0.25]).unwrap();
        library
            .apply(ColorLibraryAction::Store {
                palette: 1,
                name: "  Warm   red ".into(),
                color,
            })
            .unwrap();
        let id = library.active_palette().swatches[0].id;
        let before = library.clone();
        assert!(
            library
                .apply(ColorLibraryAction::Store {
                    palette: 1,
                    name: "warm red".into(),
                    color
                })
                .is_err()
        );
        assert_eq!(library, before);
        library
            .apply(ColorLibraryAction::Store {
                palette: 1,
                name: String::new(),
                color,
            })
            .unwrap();
        library
            .apply(ColorLibraryAction::Store {
                palette: 1,
                name: String::new(),
                color,
            })
            .unwrap();
        assert_ne!(
            library.active_palette().swatches[1].name,
            library.active_palette().swatches[2].name
        );
        let file = library.export_palette(1, PaletteFormat::Capycolor).unwrap();
        assert!(file.notice.is_none());
        library
            .apply(ColorLibrary::import_file(&file.bytes, "Unused").unwrap())
            .unwrap();
        assert_eq!(library.active_palette().name, "My colors 2");
        assert!(
            library
                .active_palette()
                .swatches
                .iter()
                .all(|s| s.color == color)
        );
        assert_eq!(library.swatch(id).unwrap().color, color);
        let before = library.clone();
        assert!(
            library
                .apply(ColorLibraryAction::Import {
                    name: "Bad".into(),
                    swatches: vec![("OK".into(), color), ("bad\nname".into(), color)]
                })
                .is_err()
        );
        assert_eq!(library, before, "failed import is atomic");
    }
    #[test]
    fn gpl_import_validates_channels_and_supplies_unique_missing_names() {
        let mut library = ColorLibrary::default();
        library.apply(ColorLibrary::import_file(b"GIMP Palette\nName: Study\nColumns: 6\n# Colors\n255 0 0\n255 0 0 Untitled\n0 0 0 Ink\n255 255 255 ink\n", "Fallback").unwrap()).unwrap();
        assert_eq!(
            library
                .active_palette()
                .swatches
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["Coral", "Coral 2", "Ink", "ink 2"]
        );
        for bytes in [
            b"GIMP Palette\n256 0 0".as_slice(),
            b"GIMP Palette\n-1 0 0",
            b"GIMP Palette\n",
            b"not a palette",
        ] {
            assert!(ColorLibrary::import_file(bytes, "Bad").is_err());
        }
    }
    #[test]
    fn usage_history_is_bounded_deduplicated_and_survives_save() {
        let mut library = ColorLibrary::default();
        for i in 0..100 {
            library
                .record_use(RgbColor::new(RgbSpace::Srgb, [i as f32 / 100., 0., 0., 1.]).unwrap());
        }
        assert_eq!(library.history.len(), ColorLibrary::MAX_HISTORY);
        let color = library.history[10];
        library.record_use(color);
        assert_eq!(library.history[0], color);
        assert_eq!(library.history.iter().filter(|c| **c == color).count(), 1);
        let restored: ColorLibrary =
            serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
        assert_eq!(restored, library);
    }
    #[test]
    fn naming_an_unsaved_color_waits_for_explicit_add() {
        let mut library = ColorLibrary::default();
        let color = RgbColor::BLACK;
        library
            .apply(ColorLibraryAction::NameCurrent {
                color,
                name: "Outline".into(),
            })
            .unwrap();
        assert!(library.active_palette().swatches.is_empty());
        assert_eq!(library.current_name(color), Some("Outline"));
        library
            .apply(ColorLibraryAction::Store {
                palette: 1,
                name: String::new(),
                color,
            })
            .unwrap();
        assert_eq!(library.active_palette().swatches[0].name, "Outline");
        assert!(library.history.is_empty());
    }
    #[test]
    fn palettes_keep_definitions_and_validate_failed_changes_atomically() {
        let mut state = ColorState::default();
        let color = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 123. / 65535.]).unwrap();
        state
            .apply(ColorAction::Library {
                action: ColorLibraryAction::Store {
                    palette: 1,
                    name: "Wide red".into(),
                    color,
                },
            })
            .unwrap();
        let id = state.library.palettes[0].swatches[0].id;
        for space in RgbSpace::ALL {
            state.set_rgb_space(space).unwrap();
            state
                .apply(ColorAction::Library {
                    action: ColorLibraryAction::Use { id },
                })
                .unwrap();
            assert_eq!(state.definition(), color);
        }
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        state.validate().unwrap();
        assert_eq!(state.library.swatch(id).unwrap().color, color);
        let before = state.clone();
        assert!(
            state
                .apply(ColorAction::Library {
                    action: ColorLibraryAction::CreatePalette {
                        name: "MY COLORS".into()
                    }
                })
                .is_err()
        );
        assert_eq!(state, before);
        assert!(
            state
                .apply(ColorAction::Library {
                    action: ColorLibraryAction::RemovePalette { id: 1 }
                })
                .is_err()
        );
        assert_eq!(state, before);
        state
            .apply(ColorAction::Library {
                action: ColorLibraryAction::Rename {
                    id,
                    name: "P3 red".into(),
                },
            })
            .unwrap();
        state
            .apply(ColorAction::Library {
                action: ColorLibraryAction::Remove { id },
            })
            .unwrap();
        assert!(state.library.swatch(id).is_none());
        assert_eq!(state.definition(), color);
    }
}
