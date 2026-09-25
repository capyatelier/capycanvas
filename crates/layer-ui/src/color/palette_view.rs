use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum PaletteCommand {
    NewPalette,
    ImportPalette,
    RenamePalette { id: u64 },
    ExportPalette { id: u64, format: PaletteFormat },
    RemovePalette { id: u64 },
    RenameColor { id: u64 },
    Library { action: ColorLibraryAction },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaletteMenuTarget {
    Library,
    Palette { id: u64 },
    Color { id: u64 },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaletteMenuItem {
    pub label: &'static str,
    pub enabled: bool,
    pub command: Option<PaletteCommand>,
    pub sections: Vec<Vec<PaletteMenuItem>>,
}
impl PaletteMenuItem {
    fn new(label: &'static str, command: PaletteCommand, enabled: bool) -> Self {
        Self {
            label,
            enabled,
            command: Some(command),
            sections: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaletteTileView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub name: String,
    pub color: RgbColor,
    pub rgba: [f32; 4],
    pub detail: String,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaletteChoiceView {
    pub id: u64,
    pub name: String,
    pub preview: Vec<[f32; 4]>,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PalettePanelView {
    pub palette: u64,
    pub name: String,
    pub swatches: Vec<PaletteTileView>,
    pub history: Vec<PaletteTileView>,
    pub palettes: Vec<PaletteChoiceView>,
    pub color_name: String,
    pub color_detail: String,
    pub can_name: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

pub fn selected_swatch(
    palette: &ColorPalette,
    current: RgbColor,
    previous: Option<u64>,
) -> Option<u64> {
    palette
        .swatches
        .iter()
        .find(|s| Some(s.id) == previous && s.color == current)
        .or_else(|| palette.swatches.iter().find(|s| s.color == current))
        .map(|s| s.id)
}

impl ColorLibrary {
    pub fn check(&self, action: ColorLibraryAction) -> Result<(), String> {
        self.clone().apply(action).map(|_| ())
    }
    pub fn color_name(&self, current: RgbColor) -> String {
        self.current_name(current)
            .map_or_else(|| Self::suggested_name(current), str::to_owned)
    }
    pub fn color_detail(colors: &ColorState) -> String {
        let mut detail = Self::hex_preview(colors.picker_base());
        if colors.hdr_intensity() != 0. {
            detail.push_str(&format!(" · {:+.1} EV", colors.hdr_intensity()));
        }
        detail
    }
    pub fn tile_detail(name: &str, color: RgbColor) -> String {
        format!(
            "{name} · {} · {}",
            Self::hex_preview(color),
            color.space.name()
        )
    }
    pub fn preview_colors(palette: &ColorPalette) -> impl Iterator<Item = RgbColor> + '_ {
        let count = palette.swatches.len().min(5);
        (0..count).map(move |i| {
            let index = if count == 1 {
                0
            } else {
                i * (palette.swatches.len() - 1) / (count - 1)
            };
            palette.swatches[index].color
        })
    }
    pub fn menu(&self, target: PaletteMenuTarget) -> Result<Vec<Vec<PaletteMenuItem>>, String> {
        use PaletteCommand as C;
        Ok(match target {
            PaletteMenuTarget::Library => vec![vec![
                PaletteMenuItem::new(
                    "New Palette…",
                    C::NewPalette,
                    self.palettes.len() < Self::MAX_PALETTES,
                ),
                PaletteMenuItem::new(
                    "Import Palette…",
                    C::ImportPalette,
                    self.palettes.len() < Self::MAX_PALETTES,
                ),
            ]],
            PaletteMenuTarget::Palette { id } => {
                if !self.palettes.iter().any(|p| p.id == id) {
                    return Err("Palette no longer exists".into());
                }
                let formats = PaletteFormat::ALL.map(|format| {
                    PaletteMenuItem::new(format.label(), C::ExportPalette { id, format }, true)
                });
                vec![
                    vec![
                        PaletteMenuItem::new("Rename Palette…", C::RenamePalette { id }, true),
                        PaletteMenuItem {
                            label: "Export Palette",
                            enabled: true,
                            command: None,
                            sections: vec![formats[..1].to_vec(), formats[1..].to_vec()],
                        },
                    ],
                    vec![PaletteMenuItem::new(
                        "Remove Palette…",
                        C::RemovePalette { id },
                        self.palettes.len() > 1,
                    )],
                ]
            }
            PaletteMenuTarget::Color { id } => {
                let palette = self
                    .palettes
                    .iter()
                    .find(|p| p.swatches.iter().any(|s| s.id == id))
                    .ok_or("Swatch no longer exists")?
                    .id;
                let library = |action| C::Library { action };
                vec![
                    vec![
                        PaletteMenuItem::new("Rename Color…", C::RenameColor { id }, true),
                        PaletteMenuItem::new(
                            "Remove Color",
                            library(ColorLibraryAction::Remove { id }),
                            true,
                        ),
                    ],
                    vec![
                        PaletteMenuItem::new(
                            "Undo Color Reorder",
                            library(ColorLibraryAction::UndoReorder { palette }),
                            self.can_undo_reorder(palette, false),
                        ),
                        PaletteMenuItem::new(
                            "Redo Color Reorder",
                            library(ColorLibraryAction::RedoReorder { palette }),
                            self.can_undo_reorder(palette, true),
                        ),
                    ],
                ]
            }
        })
    }
}

impl PalettePanelView {
    pub fn new(
        colors: &ColorState,
        library: &ColorLibrary,
        preview: impl Fn(RgbColor) -> [f32; 4],
    ) -> Self {
        let current = colors.definition();
        let palette = library.active_palette();
        let tile = |id, name: &str, color: RgbColor| PaletteTileView {
            id,
            detail: ColorLibrary::tile_detail(name, color),
            name: name.into(),
            color,
            rgba: preview(color),
            current: color == current,
        };
        Self {
            palette: palette.id,
            name: palette.name.clone(),
            swatches: palette
                .swatches
                .iter()
                .map(|s| tile(Some(s.id), &s.name, s.color))
                .collect(),
            history: library
                .history
                .iter()
                .map(|c| tile(None, "Recently used", *c))
                .collect(),
            palettes: library
                .palettes
                .iter()
                .map(|p| PaletteChoiceView {
                    id: p.id,
                    name: p.name.clone(),
                    preview: ColorLibrary::preview_colors(p).map(&preview).collect(),
                    active: p.id == palette.id,
                })
                .collect(),
            color_name: library.color_name(current),
            color_detail: ColorLibrary::color_detail(colors),
            can_name: !colors.transparent(),
            can_undo: library.can_undo_reorder(palette.id, false),
            can_redo: library.can_undo_reorder(palette.id, true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menus_and_views_follow_library_state() {
        let mut library = ColorLibrary::default();
        let color = RgbColor::new(RgbSpace::DisplayP3, [1., 0.2, 0.1, 1.]).unwrap();
        for name in ["Red", "Also red"] {
            library
                .apply(ColorLibraryAction::Store {
                    palette: 1,
                    name: name.into(),
                    color,
                })
                .unwrap();
        }
        let [first, second] = [0, 1].map(|i| library.palettes[0].swatches[i].id);
        let items = library.menu(PaletteMenuTarget::Palette { id: 1 }).unwrap();
        assert!(!items[1][0].enabled, "the last palette cannot be removed");
        let formats: Vec<_> = items[0][1]
            .sections
            .iter()
            .flatten()
            .map(|i| i.command.clone().unwrap())
            .collect();
        assert_eq!(formats.len(), PaletteFormat::ALL.len());
        assert_eq!(
            formats[0],
            PaletteCommand::ExportPalette {
                id: 1,
                format: PaletteFormat::Capycolor
            }
        );
        let colors = library
            .menu(PaletteMenuTarget::Color { id: first })
            .unwrap();
        assert!(!colors[1][0].enabled && !colors[1][1].enabled);
        library
            .apply(ColorLibraryAction::Reorder {
                palette: 1,
                id: second,
                before: Some(first),
            })
            .unwrap();
        let colors = library
            .menu(PaletteMenuTarget::Color { id: first })
            .unwrap();
        assert_eq!(
            colors[1][0].command,
            Some(PaletteCommand::Library {
                action: ColorLibraryAction::UndoReorder { palette: 1 }
            })
        );
        assert!(colors[1][0].enabled);
        assert!(library.menu(PaletteMenuTarget::Color { id: 999 }).is_err());

        let mut state = ColorState::default();
        state.set_color(color).unwrap();
        let view = PalettePanelView::new(&state, &library, |c| state.preview(c));
        assert_eq!(view.swatches.iter().filter(|t| t.current).count(), 2);
        assert!(view.can_undo && !view.can_redo);
        assert_eq!(view.palettes[0].preview.len(), 2);
        assert_eq!(
            selected_swatch(library.active_palette(), color, Some(first)),
            Some(first)
        );
        assert_eq!(
            selected_swatch(library.active_palette(), color, None),
            Some(second)
        );
        assert_eq!(
            selected_swatch(library.active_palette(), RgbColor::WHITE, Some(first)),
            None
        );
        assert!(view.swatches[0].detail.ends_with("Display P3"));
        let json = serde_json::to_value(&view).unwrap();
        assert!(json["history"].as_array().unwrap().is_empty());
        assert!(json["swatches"][0]["id"].is_u64());
    }
}
