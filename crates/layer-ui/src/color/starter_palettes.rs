//! Original sRGB starter palettes, grouped by hue, ink family, or value.
//! Research and ordering rationale: docs/ui/color-palettes.md.
use super::*;
const STARTERS: &[(crate::MessageId, &[(crate::MessageId, u32)])] = &[
    (
        crate::MessageId::CREATION_PALETTE_OCEAN_STUDY,
        &[
            (crate::MessageId::CREATION_COLOR_DEEP_WATER, 0x283A43),
            (crate::MessageId::CREATION_COLOR_TIDAL_BLUE, 0x395C65),
            (crate::MessageId::CREATION_COLOR_SEA_GLASS, 0x598184),
            (crate::MessageId::CREATION_COLOR_MIST, 0x93ABA2),
            (crate::MessageId::CREATION_COLOR_PALE_SAGE, 0xC3CBB4),
            (crate::MessageId::CREATION_COLOR_SAND, 0xE4DCC4),
            (crate::MessageId::CREATION_COLOR_BURGUNDY, 0x462D32),
            (crate::MessageId::CREATION_COLOR_BURNT_UMBER, 0x75413C),
            (crate::MessageId::CREATION_COLOR_TERRACOTTA, 0xB9684D),
            (crate::MessageId::CREATION_COLOR_CLAY, 0xD5946B),
            (crate::MessageId::CREATION_COLOR_APRICOT, 0xE9B788),
            (crate::MessageId::CREATION_COLOR_WARM_LIGHT, 0xF2D4AF),
            (crate::MessageId::CREATION_COLOR_PINE_SHADOW, 0x2D3730),
            (crate::MessageId::CREATION_COLOR_OLIVE, 0x4B5941),
            (crate::MessageId::CREATION_COLOR_MEADOW, 0x71825B),
            (crate::MessageId::CREATION_COLOR_LICHEN, 0x9DA874),
            (crate::MessageId::CREATION_COLOR_STRAW, 0xC5C697),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_PIXEL_ARCADE,
        &[
            (crate::MessageId::CREATION_COLOR_WINE, 0x73264A),
            (crate::MessageId::CREATION_COLOR_RED, 0xED3B55),
            (crate::MessageId::CREATION_COLOR_ORANGE, 0xFF8845),
            (crate::MessageId::CREATION_COLOR_GOLD, 0xFFCC45),
            (crate::MessageId::CREATION_COLOR_PEACH, 0xF6B68D),
            (crate::MessageId::CREATION_COLOR_CREAM, 0xFFF0CB),
            (crate::MessageId::CREATION_COLOR_FOREST, 0x176B55),
            (crate::MessageId::CREATION_COLOR_GREEN, 0x43BE69),
            (crate::MessageId::CREATION_COLOR_LIME, 0xB3E65B),
            (crate::MessageId::CREATION_COLOR_NAVY, 0x20294F),
            (crate::MessageId::CREATION_COLOR_BLUE, 0x3863D9),
            (crate::MessageId::CREATION_COLOR_CYAN, 0x51C8EB),
            (crate::MessageId::CREATION_COLOR_PURPLE, 0x854FC4),
            (crate::MessageId::CREATION_COLOR_BLACK, 0x161723),
            (crate::MessageId::CREATION_COLOR_SLATE, 0x69768F),
            (crate::MessageId::CREATION_COLOR_WHITE, 0xF4F4ED),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_DARK_FANTASY,
        &[
            (crate::MessageId::CREATION_COLOR_COAL, 0x12151D),
            (crate::MessageId::CREATION_COLOR_BLUE_BLACK, 0x202A38),
            (crate::MessageId::CREATION_COLOR_IRON, 0x354453),
            (crate::MessageId::CREATION_COLOR_SLATE, 0x586779),
            (crate::MessageId::CREATION_COLOR_ASH, 0x879399),
            (crate::MessageId::CREATION_COLOR_BONE, 0xD7D1B7),
            (crate::MessageId::CREATION_COLOR_BLACK_CHERRY, 0x291923),
            (crate::MessageId::CREATION_COLOR_OXBLOOD, 0x4B202D),
            (crate::MessageId::CREATION_COLOR_BLOOD_RED, 0x812F3D),
            (crate::MessageId::CREATION_COLOR_RUST, 0xAD4F45),
            (crate::MessageId::CREATION_COLOR_BLACK_GREEN, 0x182923),
            (crate::MessageId::CREATION_COLOR_MOSS, 0x344C3A),
            (crate::MessageId::CREATION_COLOR_OLIVE, 0x666C45),
            (crate::MessageId::CREATION_COLOR_BRONZE, 0x91804C),
            (crate::MessageId::CREATION_COLOR_OLD_GOLD, 0xBCA369),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_POP_ART,
        &[
            (crate::MessageId::CREATION_COLOR_DEEP_RED, 0xB71524),
            (crate::MessageId::CREATION_COLOR_RED, 0xF1282D),
            (crate::MessageId::CREATION_COLOR_ORANGE, 0xFF741C),
            (crate::MessageId::CREATION_COLOR_YELLOW, 0xFFDD00),
            (crate::MessageId::CREATION_COLOR_LEMON, 0xFFF176),
            (crate::MessageId::CREATION_COLOR_PINK, 0xFF719A),
            (crate::MessageId::CREATION_COLOR_ROYAL_BLUE, 0x153C9C),
            (crate::MessageId::CREATION_COLOR_BLUE, 0x176DE5),
            (crate::MessageId::CREATION_COLOR_SKY_BLUE, 0x70C9EC),
            (crate::MessageId::CREATION_COLOR_INK, 0x151515),
            (crate::MessageId::CREATION_COLOR_PAPER, 0xFFFBEF),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_CANDY_PASTELS,
        &[
            (crate::MessageId::CREATION_COLOR_ROSE, 0xF298B5),
            (crate::MessageId::CREATION_COLOR_PINK, 0xF8BDD4),
            (crate::MessageId::CREATION_COLOR_PEACH, 0xFFCAB7),
            (crate::MessageId::CREATION_COLOR_APRICOT, 0xFAD8AF),
            (crate::MessageId::CREATION_COLOR_BUTTER, 0xF8E69E),
            (crate::MessageId::CREATION_COLOR_PALE_YELLOW, 0xFFF2C6),
            (crate::MessageId::CREATION_COLOR_PISTACHIO, 0xCCE5B4),
            (crate::MessageId::CREATION_COLOR_MINT, 0xB3E4D0),
            (crate::MessageId::CREATION_COLOR_AQUA, 0xB7E8E5),
            (crate::MessageId::CREATION_COLOR_BABY_BLUE, 0xB7D8F4),
            (crate::MessageId::CREATION_COLOR_PERIWINKLE, 0xC9C8F1),
            (crate::MessageId::CREATION_COLOR_LILAC, 0xE0C3EF),
            (crate::MessageId::CREATION_COLOR_PLUM, 0x695579),
            (crate::MessageId::CREATION_COLOR_MAUVE, 0xA985B1),
            (crate::MessageId::CREATION_COLOR_LAVENDER, 0xD2B4DF),
            (crate::MessageId::CREATION_COLOR_PINK_WHITE, 0xFBEAF3),
            (crate::MessageId::CREATION_COLOR_CREAM, 0xFFFAF0),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_RISO_PRINT,
        &[
            (crate::MessageId::CREATION_COLOR_HOT_PINK, 0xFF4DA4),
            (crate::MessageId::CREATION_COLOR_PINK, 0xFF93C4),
            (crate::MessageId::CREATION_COLOR_PALE_PINK, 0xFFD0DD),
            (crate::MessageId::CREATION_COLOR_NAVY, 0x22356A),
            (crate::MessageId::CREATION_COLOR_BLUE, 0x496CA6),
            (crate::MessageId::CREATION_COLOR_PALE_BLUE, 0xA5C0D1),
            (crate::MessageId::CREATION_COLOR_VIOLET, 0x68477D),
            (crate::MessageId::CREATION_COLOR_MAUVE, 0xAD88AB),
            (crate::MessageId::CREATION_COLOR_INK, 0x292634),
            (crate::MessageId::CREATION_COLOR_PAPER, 0xF1E3C9),
            (crate::MessageId::CREATION_COLOR_CREAM, 0xFCF3DD),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_SYNTHWAVE,
        &[
            (crate::MessageId::CREATION_COLOR_NIGHT, 0x0F1026),
            (crate::MessageId::CREATION_COLOR_DEEP_PURPLE, 0x271146),
            (crate::MessageId::CREATION_COLOR_VIOLET, 0x4D197C),
            (crate::MessageId::CREATION_COLOR_PURPLE, 0x7E27B8),
            (crate::MessageId::CREATION_COLOR_MAGENTA, 0xBA2ADD),
            (crate::MessageId::CREATION_COLOR_NEON_PINK, 0xEF4BCF),
            (crate::MessageId::CREATION_COLOR_DEEP_BLUE, 0x092940),
            (crate::MessageId::CREATION_COLOR_PETROL, 0x145071),
            (crate::MessageId::CREATION_COLOR_ELECTRIC_BLUE, 0x126ABF),
            (crate::MessageId::CREATION_COLOR_BRIGHT_BLUE, 0x348AFF),
            (crate::MessageId::CREATION_COLOR_CYAN, 0x26CFE8),
            (crate::MessageId::CREATION_COLOR_ICE, 0x9BF6F2),
            (crate::MessageId::CREATION_COLOR_HOT_ROSE, 0xDE286D),
            (crate::MessageId::CREATION_COLOR_CORAL, 0xFF5264),
            (crate::MessageId::CREATION_COLOR_ORANGE, 0xFF873D),
            (crate::MessageId::CREATION_COLOR_AMBER, 0xFFC14A),
            (crate::MessageId::CREATION_COLOR_SUNLIGHT, 0xFFE3A0),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_SEVENTIES_PRINT,
        &[
            (crate::MessageId::CREATION_COLOR_COCOA, 0x4E2D24),
            (crate::MessageId::CREATION_COLOR_BRICK, 0x8F3D2C),
            (crate::MessageId::CREATION_COLOR_BURNT_ORANGE, 0xC4582B),
            (crate::MessageId::CREATION_COLOR_TANGERINE, 0xE78137),
            (crate::MessageId::CREATION_COLOR_MUSTARD, 0xD4A72E),
            (crate::MessageId::CREATION_COLOR_CREAM, 0xF3D9A0),
            (crate::MessageId::CREATION_COLOR_DARK_OLIVE, 0x465030),
            (crate::MessageId::CREATION_COLOR_AVOCADO, 0x7B8338),
            (crate::MessageId::CREATION_COLOR_OLIVE_GOLD, 0xB8B35A),
            (crate::MessageId::CREATION_COLOR_DUSTY_ROSE, 0xBA6660),
            (crate::MessageId::CREATION_COLOR_FADED_PINK, 0xC98D80),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_WOODBLOCK,
        &[
            (crate::MessageId::CREATION_COLOR_INK, 0x141F2C),
            (crate::MessageId::CREATION_COLOR_INDIGO, 0x17364F),
            (crate::MessageId::CREATION_COLOR_PRUSSIAN_BLUE, 0x20577E),
            (crate::MessageId::CREATION_COLOR_BLUE, 0x2F7B9C),
            (crate::MessageId::CREATION_COLOR_SKY_BLUE, 0x70ADB9),
            (crate::MessageId::CREATION_COLOR_PALE_BLUE, 0xB9D5CF),
            (crate::MessageId::CREATION_COLOR_RED_BROWN, 0x81342D),
            (crate::MessageId::CREATION_COLOR_VERMILION, 0xCA4B35),
            (crate::MessageId::CREATION_COLOR_OCHRE, 0xC79F65),
            (crate::MessageId::CREATION_COLOR_BUFF, 0xE0C69A),
            (crate::MessageId::CREATION_COLOR_PAPER, 0xF2E5C9),
        ],
    ),
    (
        crate::MessageId::CREATION_PALETTE_INK,
        &[
            (crate::MessageId::CREATION_COLOR_BLACK, 0x000000),
            (crate::MessageId::CREATION_COLOR_DARK_CHARCOAL, 0x161616),
            (crate::MessageId::CREATION_COLOR_CHARCOAL, 0x303030),
            (crate::MessageId::CREATION_COLOR_DARK_GRAY, 0x4D4D4D),
            (crate::MessageId::CREATION_COLOR_MIDDLE_GRAY, 0x686868),
            (crate::MessageId::CREATION_COLOR_GRAY, 0x858585),
            (crate::MessageId::CREATION_COLOR_LIGHT_GRAY, 0xA1A1A1),
            (crate::MessageId::CREATION_COLOR_PALE_GRAY, 0xBDBDBD),
            (crate::MessageId::CREATION_COLOR_LIGHT_WASH, 0xD9D9D9),
            (crate::MessageId::CREATION_COLOR_PAPER_WHITE, 0xECECEC),
            (crate::MessageId::CREATION_COLOR_WHITE, 0xFFFFFF),
        ],
    ),
];
pub(crate) fn starter_palettes(localizer: &crate::Localizer) -> Vec<(String, Vec<(String, RgbColor)>)> {
    STARTERS.iter().map(|(title, colors)| (localizer.text(*title).to_string(), colors.iter().map(|&(name, hex)| {
        (localizer.text(name).to_string(), RgbColor::new(RgbSpace::Srgb, [
            ((hex >> 16) & 255) as f32 / 255., ((hex >> 8) & 255) as f32 / 255.,
            (hex & 255) as f32 / 255., 1.,
        ]).unwrap())
    }).collect())).collect()
}
impl ColorLibrary {
    pub(crate) fn ensure_starters_localized(&mut self, localizer: &crate::Localizer) {
        self.ensure_starters(starter_palettes(localizer), |stem, number| numbered_name(localizer, stem, number));
    }
    fn ensure_starters(&mut self, starters: Vec<(String, Vec<(String, RgbColor)>)>, numbered: impl Fn(&str, u32) -> String) {
        if self.starters_installed || starters.is_empty() {
            return;
        }
        let owned_default = self.fresh_default;
        let pristine = owned_default && self.palettes.len() == 1
            && self.palettes[0].swatches.is_empty()
            && self.next_id == 2;
        let count = starters
            .iter()
            .map(|(_, colors)| colors.len())
            .sum::<usize>();
        if self.palettes.len() + starters.len() - usize::from(pristine) > Self::MAX_PALETTES
            || self
                .palettes
                .iter()
                .map(|p| p.swatches.len())
                .sum::<usize>()
                + count
                > Self::MAX_SWATCHES
            || self
                .next_id
                .checked_add((count + starters.len()) as u64)
                .is_none()
        {
            return;
        }
        let mut names = UniqueNames { used: self.palettes.iter().skip(usize::from(pristine)).map(|palette| palette.name.to_lowercase()).collect(), ..Default::default() };
        let mut prepared = Vec::with_capacity(starters.len());
        for (title, swatches) in starters {
            let Ok(title) = checked_name(&title) else { return; };
            let title = names.claim(title, &numbered);
            let mut colors = UniqueNames::default();
            let mut prepared_colors = Vec::with_capacity(swatches.len());
            for (name, color) in swatches {
                let Ok(name) = checked_name(&name) else { return; };
                if ColorState::validate_definition(color).is_err() { return; }
                let name = colors.claim(name, &numbered);
                if checked_name(&name).is_err() { return; }
                prepared_colors.push((name, color));
            }
            if checked_name(&title).is_err() || prepared_colors.is_empty() { return; }
            prepared.push((title, prepared_colors));
        }
        self.starters_installed = true;
        self.fresh_default = false;
        let active = self.active;
        if pristine { self.palettes.clear(); }
        for (title, swatches) in prepared {
            self.apply(ColorLibraryAction::Import { name: title, swatches })
                .expect("bounded starter palette");
        }
        if pristine {
            self.active = self.palettes[0].id;
        } else {
            self.active = active;
        }
    }
}
#[cfg(test)]
#[test]
fn starters_are_bounded_unique_and_installed_only_once() {
    let mut library = ColorLibrary::canonical();
    library.ensure_starters(starter_palettes(&crate::Localizer::shared(crate::UiLanguage::English)), |stem, number| format!("{stem} {number}"));
    library.validate().unwrap();
    assert_eq!(library.palettes.len(), 10);
    assert!(
        library
            .palettes
            .iter()
            .all(|p| (11..=17).contains(&p.swatches.len()))
    );
    assert!(library.history.is_empty());
    let id = library.palettes[0].id;
    library
        .apply_canonical(ColorLibraryAction::RemovePalette { id })
        .unwrap();
    let mut restored: ColorLibrary =
        serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
    restored.ensure_starters(starter_palettes(&crate::Localizer::shared(crate::UiLanguage::English)), |stem, number| format!("{stem} {number}"));
    assert_eq!(library, restored);
    let mut custom = ColorLibrary::canonical();
    custom
        .apply_canonical(ColorLibraryAction::Store {
            palette: 1,
            name: "My ink".into(),
            color: RgbColor::BLACK,
        })
        .unwrap();
    let swatch = custom.palettes[0].swatches[0].clone();
    custom.ensure_starters(starter_palettes(&crate::Localizer::shared(crate::UiLanguage::English)), |stem, number| format!("{stem} {number}"));
    assert_eq!(custom.active, 1);
    assert_eq!(custom.palettes[0].swatches[0], swatch);
}

#[cfg(test)]
#[test]
fn fresh_ownership_is_independent_of_text_and_is_not_restored() {
    let starters = || starter_palettes(&crate::Localizer::shared(crate::UiLanguage::English));
    for name in ["My colors", "私の色", "내 색", "Custom ink"] {
        let mut fresh = ColorLibrary::fresh(name);
        fresh.apply(ColorLibraryAction::RenamePalette { id: 1, name: name.into() }).unwrap();
        fresh.apply(ColorLibraryAction::SelectPalette { id: 1 }).unwrap();
        assert!(fresh.apply(ColorLibraryAction::RenamePalette { id: 1, name: "\n".into() }).is_err());
        assert!(fresh.fresh_default);
        let bytes = serde_json::to_vec(&fresh).unwrap();
        assert!(!String::from_utf8(bytes.clone()).unwrap().contains("fresh_default"));
        let mut restored: ColorLibrary = serde_json::from_slice(&bytes).unwrap();
        restored.ensure_starters(starters(), |stem, number| format!("{stem} {number}"));
        assert_eq!(restored.palettes[0].name, name);
        assert_eq!(restored.palettes[0].id, 1);
        assert_eq!(restored.palettes.len(), 11);
        fresh.ensure_starters(starters(), |stem, number| format!("{stem} {number}"));
        assert_eq!(fresh.palettes.len(), 10);
        let installed = fresh.clone();
        fresh.ensure_starters(starters(), |stem, number| format!("{stem} {number}"));
        assert_eq!(fresh, installed);
    }
}
#[cfg(test)]
#[test]
fn successful_rename_retains_literal_default_looking_names() {
    let mut library = ColorLibrary::fresh("My colors");
    for name in ["私の色", "My colors"] {
        library.apply(ColorLibraryAction::RenamePalette { id: 1, name: name.into() }).unwrap();
    }
    assert!(!library.fresh_default);
    library.ensure_starters(starter_palettes(&crate::Localizer::shared(crate::UiLanguage::English)), |stem, number| format!("{stem} {number}"));
    assert_eq!(library.palettes[0].name, "My colors");
    assert_eq!(library.palettes[0].id, 1);
}
#[cfg(test)]
#[test]
fn translated_starters_preserve_numeric_colors_and_order() {
    let mut expected = None;
    for language in [crate::UiLanguage::English, crate::UiLanguage::Japanese, crate::UiLanguage::Korean] {
        let mut library = ColorLibrary::fresh("文字");
        library.ensure_starters(starter_palettes(&crate::Localizer::shared(language)), |stem, number| format!("{stem} {number}"));
        let values: Vec<_> = library.palettes.iter().map(|palette| (palette.id,
            palette.swatches.iter().map(|swatch| (swatch.id, swatch.color)).collect::<Vec<_>>())).collect();
        if let Some(expected) = &expected { assert_eq!(&values, expected); } else { expected = Some(values); }
        assert_eq!(library.palettes.iter().map(|palette| palette.swatches.len()).sum::<usize>(), 137);
    }
}

#[cfg(test)]
#[test]
fn creation_preparation_keeps_user_literals_and_rejects_invalid_colors() {
    let localizer = crate::Localizer::shared(crate::UiLanguage::Japanese);
    let mut library = ColorLibrary::fresh("文字");
    let action = library.prepare_creation(ColorLibraryAction::CreatePalette { name: String::new() }, &localizer).unwrap();
    library.apply(action).unwrap();
    assert_eq!(library.active_palette().name, localizer.text(crate::MessageId::CREATION_PALETTE_NEW).as_ref());
    let action = library.prepare_creation(ColorLibraryAction::Store { palette: library.active, name: "私の赤".into(), color: RgbColor::BLACK }, &localizer).unwrap();
    library.apply(action).unwrap();
    assert_eq!(library.active_palette().swatches[0].name, "私の赤");
    let before = library.clone();
    let mut invalid = RgbColor::BLACK;
    invalid.rgba[0] = f32::NAN;
    assert!(library.prepare_creation(ColorLibraryAction::Store { palette: library.active, name: String::new(), color: invalid }, &localizer).is_err());
    assert_eq!(library, before);
    let bytes = serde_json::to_vec(&library).unwrap();
    let restored: ColorLibrary = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored.palettes, library.palettes);
    assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
}

#[cfg(test)]
#[test]
fn fresh_editing_reset_does_not_rewrite_restored_names() {
    let localizer = crate::Localizer::shared(crate::UiLanguage::Japanese);
    let mut working = crate::EditingState::new_localized(&localizer);
    working.color_library.palettes[0].name = "My colors".into();
    working.color_library.palettes[0].swatches[0].name = "手描きの色".into();
    let bytes = serde_json::to_vec(&working).unwrap();
    let mut restored: crate::EditingState = serde_json::from_slice(&bytes).unwrap();
    restored.color_library.ensure_starters(starter_palettes(&localizer), |stem, number| format!("{stem} {number}"));
    assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    let reset = crate::EditingState::new_localized(&localizer);
    assert_eq!(reset.color_library.palettes[0].name, localizer.text(crate::MessageId::CREATION_PALETTE_OCEAN_STUDY).as_ref());
    assert_eq!(reset.color_library.palettes[0].id, working.color_library.palettes[0].id);
    assert_eq!(reset.color_library.palettes[0].swatches[0].color, working.color_library.palettes[0].swatches[0].color);
}

#[cfg(test)]
#[test]
fn bounded_import_names_are_prepared_once_and_data_only_apply_rejects_duplicates() {
    let localizer = crate::Localizer::shared(crate::UiLanguage::English);
    let mut library = ColorLibrary::fresh("Ink collection");
    let swatches = (0..ColorLibrary::MAX_SWATCHES).map(|_| ("手描き".into(), RgbColor::BLACK)).collect();
    let action = library.prepare_creation(ColorLibraryAction::Import { name: "Ink collection".into(), swatches }, &localizer).unwrap();
    library.apply(action).unwrap();
    assert_eq!(library.active_palette().swatches.len(), ColorLibrary::MAX_SWATCHES);
    assert_eq!(library.active_palette().swatches.last().unwrap().name, "手描き 4096");
    let mut fresh = ColorLibrary::fresh("Literal");
    let before = fresh.clone();
    assert!(fresh.apply(ColorLibraryAction::Import { name: "Literal imported".into(), swatches: vec![("Same".into(), RgbColor::BLACK), ("Same".into(), RgbColor::BLACK)] }).is_err());
    assert_eq!(fresh, before);
    assert!(fresh.fresh_default);
}

#[cfg(test)]
#[test]
fn restored_starter_name_collision_preserves_literal_and_numbers_generated_palette() {
    let localizer = crate::Localizer::shared(crate::UiLanguage::Japanese);
    let title = localizer.text(crate::MessageId::CREATION_PALETTE_OCEAN_STUDY).to_string();
    let fresh = ColorLibrary::fresh(&title);
    let mut restored: ColorLibrary = serde_json::from_slice(&serde_json::to_vec(&fresh).unwrap()).unwrap();
    let literal = restored.palettes[0].clone();
    restored.ensure_starters_localized(&localizer);
    assert_eq!(restored.palettes[0], literal);
    assert_eq!(restored.palettes[1].name, numbered_name(&localizer, &title, 2));
    assert!(restored.starters_installed);
    assert_eq!(restored.palettes.len(), 11);
    let after = restored.clone();
    restored.ensure_starters_localized(&localizer);
    assert_eq!(restored, after);
}

#[cfg(test)]
#[test]
fn fresh_starter_named_placeholder_and_rejected_install_are_atomic() {
    let localizer = crate::Localizer::shared(crate::UiLanguage::English);
    let mut fresh = ColorLibrary::fresh("Ocean Study");
    let before = serde_json::to_vec(&fresh).unwrap();
    fresh.ensure_starters(vec![("\n".into(), vec![("Ink".into(), RgbColor::BLACK)])], |stem, number| format!("{stem} {number}"));
    assert_eq!(serde_json::to_vec(&fresh).unwrap(), before);
    assert!(fresh.fresh_default);
    fresh.ensure_starters_localized(&localizer);
    assert_eq!(fresh.palettes[0].name, "Ocean Study");
    assert_eq!(fresh.palettes.len(), 10);
}
