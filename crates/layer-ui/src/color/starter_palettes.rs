//! Original sRGB starter palettes, grouped by hue, ink family, or value.
//! Research and ordering rationale: docs/development/color-palettes-panel.md.
use super::*;
const STARTERS: &[(&str, &[(&str, u32)])] = &[
    (
        "Ocean Study",
        &[
            ("Deep Water", 0x283A43),
            ("Tidal Blue", 0x395C65),
            ("Sea Glass", 0x598184),
            ("Mist", 0x93ABA2),
            ("Pale Sage", 0xC3CBB4),
            ("Sand", 0xE4DCC4),
            ("Burgundy", 0x462D32),
            ("Burnt Umber", 0x75413C),
            ("Terracotta", 0xB9684D),
            ("Clay", 0xD5946B),
            ("Apricot", 0xE9B788),
            ("Warm Light", 0xF2D4AF),
            ("Pine Shadow", 0x2D3730),
            ("Olive", 0x4B5941),
            ("Meadow", 0x71825B),
            ("Lichen", 0x9DA874),
            ("Straw", 0xC5C697),
        ],
    ),
    (
        "Pixel Arcade",
        &[
            ("Wine", 0x73264A),
            ("Red", 0xED3B55),
            ("Orange", 0xFF8845),
            ("Gold", 0xFFCC45),
            ("Peach", 0xF6B68D),
            ("Cream", 0xFFF0CB),
            ("Forest", 0x176B55),
            ("Green", 0x43BE69),
            ("Lime", 0xB3E65B),
            ("Navy", 0x20294F),
            ("Blue", 0x3863D9),
            ("Cyan", 0x51C8EB),
            ("Purple", 0x854FC4),
            ("Black", 0x161723),
            ("Slate", 0x69768F),
            ("White", 0xF4F4ED),
        ],
    ),
    (
        "Dark Fantasy",
        &[
            ("Coal", 0x12151D),
            ("Blue Black", 0x202A38),
            ("Iron", 0x354453),
            ("Slate", 0x586779),
            ("Ash", 0x879399),
            ("Bone", 0xD7D1B7),
            ("Black Cherry", 0x291923),
            ("Oxblood", 0x4B202D),
            ("Blood Red", 0x812F3D),
            ("Rust", 0xAD4F45),
            ("Black Green", 0x182923),
            ("Moss", 0x344C3A),
            ("Olive", 0x666C45),
            ("Bronze", 0x91804C),
            ("Old Gold", 0xBCA369),
        ],
    ),
    (
        "Pop Art",
        &[
            ("Deep Red", 0xB71524),
            ("Red", 0xF1282D),
            ("Orange", 0xFF741C),
            ("Yellow", 0xFFDD00),
            ("Lemon", 0xFFF176),
            ("Pink", 0xFF719A),
            ("Royal Blue", 0x153C9C),
            ("Blue", 0x176DE5),
            ("Sky Blue", 0x70C9EC),
            ("Ink", 0x151515),
            ("Paper", 0xFFFBEF),
        ],
    ),
    (
        "Candy Pastels",
        &[
            ("Rose", 0xF298B5),
            ("Pink", 0xF8BDD4),
            ("Peach", 0xFFCAB7),
            ("Apricot", 0xFAD8AF),
            ("Butter", 0xF8E69E),
            ("Pale Yellow", 0xFFF2C6),
            ("Pistachio", 0xCCE5B4),
            ("Mint", 0xB3E4D0),
            ("Aqua", 0xB7E8E5),
            ("Baby Blue", 0xB7D8F4),
            ("Periwinkle", 0xC9C8F1),
            ("Lilac", 0xE0C3EF),
            ("Plum", 0x695579),
            ("Mauve", 0xA985B1),
            ("Lavender", 0xD2B4DF),
            ("Pink White", 0xFBEAF3),
            ("Cream", 0xFFFAF0),
        ],
    ),
    (
        "Riso Print",
        &[
            ("Hot Pink", 0xFF4DA4),
            ("Pink", 0xFF93C4),
            ("Pale Pink", 0xFFD0DD),
            ("Navy", 0x22356A),
            ("Blue", 0x496CA6),
            ("Pale Blue", 0xA5C0D1),
            ("Violet", 0x68477D),
            ("Mauve", 0xAD88AB),
            ("Ink", 0x292634),
            ("Paper", 0xF1E3C9),
            ("Cream", 0xFCF3DD),
        ],
    ),
    (
        "Synthwave",
        &[
            ("Night", 0x0F1026),
            ("Deep Purple", 0x271146),
            ("Violet", 0x4D197C),
            ("Purple", 0x7E27B8),
            ("Magenta", 0xBA2ADD),
            ("Neon Pink", 0xEF4BCF),
            ("Deep Blue", 0x092940),
            ("Petrol", 0x145071),
            ("Electric Blue", 0x126ABF),
            ("Bright Blue", 0x348AFF),
            ("Cyan", 0x26CFE8),
            ("Ice", 0x9BF6F2),
            ("Hot Rose", 0xDE286D),
            ("Coral", 0xFF5264),
            ("Orange", 0xFF873D),
            ("Amber", 0xFFC14A),
            ("Sunlight", 0xFFE3A0),
        ],
    ),
    (
        "Seventies Print",
        &[
            ("Cocoa", 0x4E2D24),
            ("Brick", 0x8F3D2C),
            ("Burnt Orange", 0xC4582B),
            ("Tangerine", 0xE78137),
            ("Mustard", 0xD4A72E),
            ("Cream", 0xF3D9A0),
            ("Dark Olive", 0x465030),
            ("Avocado", 0x7B8338),
            ("Olive Gold", 0xB8B35A),
            ("Dusty Rose", 0xBA6660),
            ("Faded Pink", 0xC98D80),
        ],
    ),
    (
        "Woodblock",
        &[
            ("Ink", 0x141F2C),
            ("Indigo", 0x17364F),
            ("Prussian Blue", 0x20577E),
            ("Blue", 0x2F7B9C),
            ("Sky Blue", 0x70ADB9),
            ("Pale Blue", 0xB9D5CF),
            ("Red Brown", 0x81342D),
            ("Vermilion", 0xCA4B35),
            ("Ochre", 0xC79F65),
            ("Buff", 0xE0C69A),
            ("Paper", 0xF2E5C9),
        ],
    ),
    (
        "Ink",
        &[
            ("Black", 0x000000),
            ("Dark Charcoal", 0x161616),
            ("Charcoal", 0x303030),
            ("Dark Gray", 0x4D4D4D),
            ("Middle Gray", 0x686868),
            ("Gray", 0x858585),
            ("Light Gray", 0xA1A1A1),
            ("Pale Gray", 0xBDBDBD),
            ("Light Wash", 0xD9D9D9),
            ("Paper White", 0xECECEC),
            ("White", 0xFFFFFF),
        ],
    ),
];
impl ColorLibrary {
    /// Install once in GTK working state, preserving existing palettes and IDs.
    /// Copies are ordinary editable palettes; deleting one never resurrects it.
    pub fn ensure_starters(&mut self) {
        if self.starters_installed {
            return;
        }
        self.starters_installed = true;
        let pristine = self.palettes.len() == 1
            && self.palettes[0].name == "My colors"
            && self.palettes[0].swatches.is_empty()
            && self.next_id == 2;
        let count = STARTERS
            .iter()
            .map(|(_, colors)| colors.len())
            .sum::<usize>();
        if self.palettes.len() + STARTERS.len() - usize::from(pristine) > Self::MAX_PALETTES
            || self
                .palettes
                .iter()
                .map(|p| p.swatches.len())
                .sum::<usize>()
                + count
                > Self::MAX_SWATCHES
            || self
                .next_id
                .checked_add((count + STARTERS.len()) as u64)
                .is_none()
        {
            return;
        }
        let active = self.active;
        for &(title, colors) in STARTERS {
            self.apply(ColorLibraryAction::Import {
                name: title.into(),
                swatches: colors
                    .iter()
                    .map(|&(name, hex)| {
                        (
                            name.into(),
                            RgbColor::new(
                                RgbSpace::Srgb,
                                [
                                    ((hex >> 16) & 255) as f32 / 255.,
                                    ((hex >> 8) & 255) as f32 / 255.,
                                    (hex & 255) as f32 / 255.,
                                    1.,
                                ],
                            )
                            .unwrap(),
                        )
                    })
                    .collect(),
            })
            .expect("bounded starter palette");
        }
        if pristine {
            self.palettes.remove(0);
            self.active = self.palettes[0].id;
        } else {
            self.active = active;
        }
    }
}
#[cfg(test)]
#[test]
fn starters_are_bounded_unique_and_installed_only_once() {
    let mut library = ColorLibrary::default();
    library.ensure_starters();
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
        .apply(ColorLibraryAction::RemovePalette { id })
        .unwrap();
    let mut restored: ColorLibrary =
        serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
    restored.ensure_starters();
    assert_eq!(library, restored);
    let mut custom = ColorLibrary::default();
    custom
        .apply(ColorLibraryAction::Store {
            palette: 1,
            name: "My ink".into(),
            color: RgbColor::BLACK,
        })
        .unwrap();
    let swatch = custom.palettes[0].swatches[0].clone();
    custom.ensure_starters();
    assert_eq!(custom.active, 1);
    assert_eq!(custom.palettes[0].swatches[0], swatch);
}
