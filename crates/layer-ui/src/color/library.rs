//! Workspace-owned palettes. IDs survive rename/delete; every saved swatch
//! carries its own color definition and never stores a clipped display preview.
use super::*;

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
    next_id: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ColorLibraryAction {
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
        }
    }
}
fn name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 64 || value.chars().any(char::is_control) {
        return Err("Choose a name of 1–64 characters without control characters".into());
    }
    Ok(value.into())
}
impl ColorLibrary {
    pub const MAX_PALETTES: usize = 64;
    pub const MAX_SWATCHES: usize = 4096;
    pub(crate) fn validate(&self) -> Result<(), String> {
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
            if name(&palette.name)? != palette.name || !names.insert(palette.name.to_lowercase()) {
                return Err("Palette names must be unique".into());
            }
            if palette.id == 0 || palette.id >= self.next_id || !ids.insert(palette.id) {
                return Err("Invalid palette ID".into());
            }
            for swatch in &palette.swatches {
                name(&swatch.name)?;
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
            }
            ColorLibraryAction::Store {
                palette,
                name: value,
                color,
            } => {
                let name = name(&value)?;
                ColorState::validate_definition(color)?;
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
                self.next_id = next;
            }
            ColorLibraryAction::Rename { id, name: value } => {
                let name = name(&value)?;
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
            }
            ColorLibraryAction::Use { id } => {
                return Ok(Some(
                    self.swatch(id).ok_or("Swatch no longer exists")?.color,
                ));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
