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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorLibrary {
    pub palettes: Vec<ColorPalette>,
    pub active: u64,
    pub selected: Option<u64>,
    pub history: Vec<RgbColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_name: Option<(RgbColor, String)>,
    starters_installed: bool,
    #[serde(skip)]
    fresh_default: bool,
    #[serde(skip)]
    reorders: reorder::ReorderHistory,
    next_id: u64,
}
impl PartialEq for ColorLibrary {
    fn eq(&self, other: &Self) -> bool {
        self.palettes == other.palettes && self.active == other.active && self.selected == other.selected
            && self.history == other.history && self.pending_name == other.pending_name
            && self.starters_installed == other.starters_installed && self.reorders == other.reorders
            && self.next_id == other.next_id
    }
}
#[derive(Default)]
struct UniqueNames {
    used: std::collections::BTreeSet<String>,
    suffixes: std::collections::BTreeMap<String, u32>,
}
impl UniqueNames {
    fn claim(&mut self, base: String, numbered: impl Fn(&str, u32) -> String) -> String {
        if self.used.insert(base.to_lowercase()) {
            return base;
        }
        let stem = base.chars().take(55).collect::<String>();
        let suffix = self.suffixes.entry(stem.to_lowercase()).or_insert(2);
        loop {
            let candidate = numbered(&stem, *suffix);
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
impl ColorLibrary {
    pub fn canonical() -> Self {
        Self::fresh(&crate::Localizer::shared(crate::UiLanguage::English).text(crate::MessageId::CREATION_PALETTE_MY_COLORS))
    }
    pub(crate) fn fresh(default_name: &str) -> Self {
        Self {
            palettes: vec![ColorPalette {
                id: 1,
                name: default_name.into(),
                swatches: Vec::new(),
            }],
            next_id: 2,
            starters_installed: false,
            fresh_default: true,
            reorders: Default::default(),
            active: 1,
            selected: None,
            history: Vec::new(),
            pending_name: None,
        }
    }
}
fn checked_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 64 || value.chars().any(char::is_control) {
        return Err("Choose a name of 1–64 characters without control characters".into());
    }
    Ok(value.split_whitespace().collect::<Vec<_>>().join(" "))
}
impl ColorLibrary {
    #[cfg(test)]
    pub(crate) fn apply_canonical(&mut self, action: ColorLibraryAction) -> Result<Option<RgbColor>, String> {
        let action = self.prepare_creation(action, &crate::Localizer::shared(crate::UiLanguage::English))?;
        self.apply(action)
    }
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
        self.fresh_default = false;
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
    pub fn suggested_name(color: RgbColor) -> crate::MessageId {
        let p = color.encoded_in(RgbSpace::Srgb).expect("validated color");
        let [r, g, b] = [p[0], p[1], p[2]].map(|v| v.clamp(0., 1.));
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        if max < 0.12 {
            crate::MessageId::CREATION_COLOR_INK
        } else if delta < 0.08 {
            if min > 0.9 {
                crate::MessageId::CREATION_COLOR_WHITE
            } else if max < 0.4 {
                crate::MessageId::CREATION_COLOR_CHARCOAL
            } else {
                crate::MessageId::CREATION_COLOR_GRAY
            }
        } else {
            match components([r, g, b, 1.], ColorSpace::Hsv, 0.)[0] as u32 {
                0..=19 | 345..=359 => {
                    if max < 0.65 {
                        crate::MessageId::CREATION_COLOR_BRICK
                    } else {
                        crate::MessageId::CREATION_COLOR_CORAL
                    }
                }
                20..=44 => {
                    if max < 0.65 {
                        crate::MessageId::CREATION_COLOR_UMBER
                    } else if delta < 0.45 {
                        crate::MessageId::CREATION_COLOR_SAND
                    } else {
                        crate::MessageId::CREATION_COLOR_AMBER
                    }
                }
                45..=69 => {
                    if delta < 0.4 {
                        crate::MessageId::CREATION_COLOR_LINEN
                    } else {
                        crate::MessageId::CREATION_COLOR_GOLD
                    }
                }
                70..=159 => crate::MessageId::CREATION_COLOR_GREEN,
                160..=194 => crate::MessageId::CREATION_COLOR_TEAL,
                195..=254 => crate::MessageId::CREATION_COLOR_BLUE,
                255..=284 => crate::MessageId::CREATION_COLOR_VIOLET,
                _ => crate::MessageId::CREATION_COLOR_ROSE,
            }
        }
    }
    pub fn current_name(&self, color: RgbColor) -> Option<&str> {
        self.pending_name
            .as_ref()
            .filter(|(c, _)| *c == color)
            .map(|(_, name)| name.as_str())
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
            checked_name(value)?;
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
            if checked_name(&palette.name).is_err() || !names.insert(palette.name.to_lowercase()) {
                return Err("Palette names must be unique".into());
            }
            if palette.id == 0 || palette.id >= self.next_id || !ids.insert(palette.id) {
                return Err("Invalid palette ID".into());
            }
            let mut swatch_names = std::collections::BTreeSet::new();
            for swatch in &palette.swatches {
                if checked_name(&swatch.name)? != swatch.name
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
        if self.selected.is_some_and(|id| self.swatch(id).is_none()) {
            return Err("Swatch no longer exists".into());
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
        let value = checked_name(value)?;
        if self
            .palettes
            .iter()
            .any(|p| Some(p.id) != id && p.name.to_lowercase() == value.to_lowercase())
        {
            return Err("A palette already uses that name".into());
        }
        Ok(value)
    }
    pub fn prepare_creation(&self, action: ColorLibraryAction, localizer: &crate::Localizer) -> Result<ColorLibraryAction, String> {
        match &action {
            ColorLibraryAction::NameCurrent { color, .. } | ColorLibraryAction::Store { color, .. } => ColorState::validate_definition(*color)?,
            ColorLibraryAction::Import { swatches, .. } => { for (_, color) in swatches { ColorState::validate_definition(*color)?; } }
            _ => {}
        }
        let numbered = |stem: &str, number: u32| numbered_name(localizer, stem, number);
        let unique = |base: String, names: Vec<String>| {
            UniqueNames { used: names.into_iter().map(|name| name.to_lowercase()).collect(), ..Default::default() }
                .claim(base, numbered)
        };
        let swatch_name = |palette: u64, id: Option<u64>, value: String, color: RgbColor| {
            if !value.trim().is_empty() { return value; }
            let base = self.current_name(color).map(str::to_owned)
                .unwrap_or_else(|| localizer.text(Self::suggested_name(color)).to_string());
            let names = self.palettes.iter().find(|p| p.id == palette).into_iter()
                .flat_map(|p| &p.swatches).filter(|s| Some(s.id) != id).map(|s| s.name.clone()).collect();
            unique(base, names)
        };
        Ok(match action {
            ColorLibraryAction::CreatePalette { name } if name.trim().is_empty() => ColorLibraryAction::CreatePalette {
                name: unique(localizer.text(crate::MessageId::CREATION_PALETTE_NEW).to_string(), self.palettes.iter().map(|p| p.name.clone()).collect()),
            },
            ColorLibraryAction::Import { name, swatches } => {
                let name = if name.trim().is_empty() { localizer.text(crate::MessageId::CREATION_PALETTE_IMPORTED).to_string() } else { name };
                let name = unique(checked_name(&name).unwrap_or(name), self.palettes.iter().map(|p| p.name.clone()).collect());
                let mut used = UniqueNames::default();
                let swatches = swatches.into_iter().map(|(name, color)| {
                    let name = if name.trim().is_empty() { localizer.text(Self::suggested_name(color)).to_string() } else { name };
                    let name = used.claim(checked_name(&name).unwrap_or(name), numbered); (name, color)
                }).collect();
                ColorLibraryAction::Import { name, swatches }
            }
            ColorLibraryAction::NameCurrent { color, name } => ColorLibraryAction::NameCurrent {
                color, name: swatch_name(self.active, None, name, color),
            },
            ColorLibraryAction::Store { palette, color, name } => ColorLibraryAction::Store {
                palette, color, name: swatch_name(palette, None, name, color),
            },
            ColorLibraryAction::Rename { id, name } => {
                if let Some((palette, color)) = self.palettes.iter().find_map(|p| p.swatches.iter().find(|s| s.id == id).map(|s| (p.id, s.color))) {
                    ColorLibraryAction::Rename { id, name: swatch_name(palette, Some(id), name, color) }
                } else { ColorLibraryAction::Rename { id, name } }
            }
            action => action,
        })
    }
    pub fn apply(&mut self, action: ColorLibraryAction) -> Result<Option<RgbColor>, String> {
        let fresh = self.fresh_default.then(|| (self.palettes[0].clone(), self.next_id, self.pending_name.clone()));
        match action {
            ColorLibraryAction::NameCurrent { color, name: value } => {
                ColorState::validate_definition(color)?;
                let name = self.swatch_name(self.active_palette().id, None, &value)?;
                self.pending_name = Some((color, name));
            }
            ColorLibraryAction::SelectPalette { id } => {
                if !self.palettes.iter().any(|p| p.id == id) {
                    return Err("Palette no longer exists".into());
                }
                self.active = id;
                return Ok(None);
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
                let base = checked_name(&value)?;
                let value = next.palette_name(None, &base)?;
                next.apply(ColorLibraryAction::CreatePalette { name: value })?;
                let end = next
                    .next_id
                    .checked_add(swatches.len() as u64)
                    .ok_or("Swatch IDs exhausted")?;
                let mut names = std::collections::BTreeSet::new();
                for (value, color) in swatches {
                    ColorState::validate_definition(color)?;
                    let base = checked_name(&value)?;
                    if !names.insert(base.to_lowercase()) { return Err("Another color in this palette already has that name".into()); }
                    let value = base;
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
                let name = self.palette_name(None, &name)?;
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
                if self.palettes[index].swatches.iter().any(|s| Some(s.id) == self.selected) { self.selected = None; }
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
                let name = self.swatch_name(palette, None, &value)?;
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
                self.selected = Some(self.next_id);
                self.next_id = next;
            }
            ColorLibraryAction::Rename { id, name: value } => {
                let palette = self
                    .palettes
                    .iter()
                    .find(|p| p.swatches.iter().any(|s| s.id == id))
                    .ok_or("Swatch no longer exists")?;
                let name = self.swatch_name(palette.id, Some(id), &value)?;
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
                if self.selected == Some(id) { self.selected = None; }
                let palette_id = palette.id;
                self.forget_reorders(palette_id);
            }
            ColorLibraryAction::Use { id } => {
                let color = self.swatch(id).ok_or("Swatch no longer exists")?.color;
                self.selected = Some(id);
                return Ok(Some(color));
            }
            ColorLibraryAction::Reorder {
                palette,
                id,
                before,
            } => self.reorder(palette, id, before)?,
            ColorLibraryAction::UndoReorder { palette } => self.restore_reorder(palette, false)?,
            ColorLibraryAction::RedoReorder { palette } => self.restore_reorder(palette, true)?,
        }
        if let Some((palette, next_id, pending_name)) = fresh {
            self.fresh_default &= self.palettes.len() == 1 && self.palettes[0] == palette
                && self.next_id == next_id && self.pending_name == pending_name;
        }
        Ok(None)
    }
    fn swatch_name(
        &self,
        palette: u64,
        id: Option<u64>,
        value: &str,
    ) -> Result<String, String> {
        let palette = self
            .palettes
            .iter()
            .find(|p| p.id == palette)
            .ok_or("Palette no longer exists")?;
        let others = || palette.swatches.iter().filter(|s| Some(s.id) != id);
        let value = checked_name(value)?;
        if others().any(|s| s.name.to_lowercase() == value.to_lowercase()) {
            return Err("Another color in this palette already has that name".into());
        }
        Ok(value)
    }
}

fn numbered_name(localizer: &crate::Localizer, stem: &str, number: u32) -> String {
    let mut args = crate::FluentArgs::new();
    args.set("name", stem); args.set("number", number);
    localizer.format(crate::MessageId::CREATION_NUMBERED_NAME, &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_imports_keep_ids_and_definitions() {
        let mut library = ColorLibrary::canonical();
        let color = RgbColor::from_linear(RgbSpace::DisplayP3, [4., 0.1, 0.3, 0.25]).unwrap();
        library
            .apply_canonical(ColorLibraryAction::Store {
                palette: 1,
                name: "  Warm   red ".into(),
                color,
            })
            .unwrap();
        let id = library.active_palette().swatches[0].id;
        let before = library.clone();
        assert!(
            library
                .apply_canonical(ColorLibraryAction::Store {
                    palette: 1,
                    name: "warm red".into(),
                    color
                })
                .is_err()
        );
        assert_eq!(library, before);
        library
            .apply_canonical(ColorLibraryAction::Store {
                palette: 1,
                name: String::new(),
                color,
            })
            .unwrap();
        library
            .apply_canonical(ColorLibraryAction::Store {
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
            .apply_canonical(ColorLibrary::import_file(&file.bytes, "Unused").unwrap())
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
                .apply_canonical(ColorLibraryAction::Import {
                    name: "Bad".into(),
                    swatches: vec![("OK".into(), color), ("bad\nname".into(), color)]
                })
                .is_err()
        );
        assert_eq!(library, before, "failed import is atomic");
    }
    #[test]
    fn gpl_import_validates_channels_and_supplies_unique_missing_names() {
        let mut library = ColorLibrary::canonical();
        library.apply_canonical(ColorLibrary::import_file(b"GIMP Palette\nName: Study\nColumns: 6\n# Colors\n255 0 0\n255 0 0 Untitled\n0 0 0 Ink\n255 255 255 ink\n", "Fallback").unwrap()).unwrap();
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
        let mut library = ColorLibrary::canonical();
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
        let mut library = ColorLibrary::canonical();
        let color = RgbColor::BLACK;
        library
            .apply_canonical(ColorLibraryAction::NameCurrent {
                color,
                name: "Outline".into(),
            })
            .unwrap();
        assert!(library.active_palette().swatches.is_empty());
        assert_eq!(library.current_name(color), Some("Outline"));
        library
            .apply_canonical(ColorLibraryAction::Store {
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
        let mut library = ColorLibrary::canonical();
        let color = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 123. / 65535.]).unwrap();
        library.apply_canonical(ColorLibraryAction::Store { palette: 1, name: "Wide red".into(), color }).unwrap();
        let id = library.palettes[0].swatches[0].id;
        for space in RgbSpace::ALL {
            state.set_rgb_space(space).unwrap();
            state.set_color(library.apply_canonical(ColorLibraryAction::Use { id }).unwrap().unwrap()).unwrap();
            assert_eq!(state.definition(), color);
        }
        library = serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
        library.validate().unwrap();
        assert_eq!(library.swatch(id).unwrap().color, color);
        let before = library.clone();
        assert!(library.apply_canonical(ColorLibraryAction::CreatePalette { name: "MY COLORS".into() }).is_err());
        assert_eq!(library, before);
        assert!(library.apply_canonical(ColorLibraryAction::RemovePalette { id: 1 }).is_err());
        assert_eq!(library, before);
        library.apply_canonical(ColorLibraryAction::Rename { id, name: "P3 red".into() }).unwrap();
        library.apply_canonical(ColorLibraryAction::Remove { id }).unwrap();
        assert!(library.swatch(id).is_none());
        assert_eq!(state.definition(), color);
    }
}
