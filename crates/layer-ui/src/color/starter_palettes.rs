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
    pub(crate) fn ensure_starters(&mut self) {
        if self.starters_installed {
            self.upgrade_review_starters();
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

impl ColorLibrary {
    fn upgrade_review_starters(&mut self) {
        use sha2::{Digest, Sha256};
        // Only exact, untouched review defaults are replaced. User edits,
        // including reordering, must survive upgrades. Targets never grow.
        const PREVIOUS: &[(usize, &str, &str)] = &[
            (
                9,
                "Manga Ink",
                "7b6780db71eff82de1cec433d0a34c6c8f8a9c25a76dea6d3cad3be4bc04c227",
            ),
            (
                0,
                "Saltwater Postcard",
                "eec845ea8030731a109c064692b30916770c5af41683209d77e9984bc67ce028",
            ),
            (
                1,
                "Velvet Ember",
                "a4ed8698ce1efa61a85b8f9c0ab61a4846f055250ff929775bef017fbfd43bfb",
            ),
            (
                2,
                "Moss Cathedral",
                "c85fced517dddf47169574bfcbce809202675a894dbf0e57a5e49b0462614337",
            ),
            (
                3,
                "Golden Hour",
                "0a4455c291e44c8017f90366f62487d013e2199c4a089293bd6654020627af80",
            ),
            (
                4,
                "Petal Milk",
                "b77f852ae0f332dff1ffdef5fe413246d59ae989ca92d5f0b47f6fe2a727ded4",
            ),
            (
                5,
                "Risograph Club",
                "e7bc5e961a9c256f149677441e1faae1a25c219895f0915dbd3fd796b67cf2fb",
            ),
            (
                6,
                "After Hours",
                "8e7f4ccef722a9ddf2aac96d1a7fc8b6b0f1cae64f3e47603b9112bfc4889728",
            ),
            (
                7,
                "Moonlit Archive",
                "53dc8653aa9434fcca92b1f4b31c641bb33695d96fc0f8f3d609f13de67aad18",
            ),
            (
                8,
                "Winter Glass",
                "8063efed21220c51ab90a65b036876334f13b3b6eef73f8d35b25277d4cc03e6",
            ),
            (
                9,
                "Graphite & Linen",
                "db3058121f0c22c8a3b0ae30ea9bc6626426f1b8bc9ac0b649e9e53bfbe3677e",
            ),
            (
                1,
                "Earth Tones",
                "0c57f2b97f7eb657a623bb351f900e5ea4df5b3b9b9b989f47605f05bf6787cb",
            ),
            (
                2,
                "Forest Study",
                "940b0309cdf6434d4d899bd8132ff06152e34b55bc5d05be87f746bc5312fcb2",
            ),
            (
                3,
                "Skin Tones",
                "38fcca6c240b7d723a51a3ffcaa14616de61f939ad0527ad51882732818ecc57",
            ),
            (
                4,
                "Pastels",
                "4ca02a1e155dcb48648106867dd702488c5347a1f841bf7819ff0bdd45eaa3c6",
            ),
            (
                5,
                "Primary Colors",
                "68f7bd4fa8e262b1a5ee2d646e158e0bc806e408aca7b6c8bb042ab78cd208cb",
            ),
            (
                6,
                "Night Lights",
                "c0ee02c7ded2b2cede7fafb3c6475b91f7cfa0d0a56f3a5d182cef240352ec56",
            ),
            (
                7,
                "Autumn Leaves",
                "24989379df69d7b1ba5961193b3eb2e8e8a8828d4fd215c90857e5cd9a72a1da",
            ),
            (
                8,
                "Winter Light",
                "a4c5942344b4d886e9e534d0c6966f45eb2b660814d8782624257c8238780aee",
            ),
            (
                9,
                "Grayscale",
                "bee18d5f6975d0f172fc207cfb23bf00225b3853f014aadeaf135ce06fea6876",
            ),
        ];
        for &(index, old_name, fingerprint) in PREVIOUS {
            let Some(p) = self.palettes.iter().position(|p| p.name == old_name) else {
                continue;
            };
            let palette = &self.palettes[p];
            if palette
                .swatches
                .iter()
                .any(|s| s.color.space != RgbSpace::Srgb)
            {
                continue;
            }
            let mut hash = Sha256::new();
            for swatch in &palette.swatches {
                hash.update(swatch.name.as_bytes());
                hash.update([0]);
                for v in swatch.color.rgba {
                    hash.update(v.to_le_bytes());
                }
            }
            if format!("{:x}", hash.finalize()) != fingerprint {
                continue;
            }
            let (title, colors) = STARTERS[index];
            if colors.len() > palette.swatches.len() {
                continue;
            }
            let id = palette.id;
            self.forget_reorders(id);
            self.palettes[p].swatches.truncate(colors.len());
            self.palettes[p].name = Self::unique_name(
                title.into(),
                self.palettes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != p)
                    .map(|(_, p)| p.name.clone()),
            );
            for (swatch, &(name, hex)) in self.palettes[p].swatches.iter_mut().zip(colors) {
                swatch.name = name.into();
                swatch.color = RgbColor::new(
                    RgbSpace::Srgb,
                    [
                        ((hex >> 16) & 255) as f32 / 255.,
                        ((hex >> 8) & 255) as f32 / 255.,
                        (hex & 255) as f32 / 255.,
                        1.,
                    ],
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn earlier_review_palettes_upgrade_only_when_untouched() {
    let mut library = ColorLibrary::default();
    library.ensure_starters();
    let old = [
        ("Burnt Velvet", 0x302026u32),
        ("Wine Clay", 0x63363Du32),
        ("Kiln Red", 0xA65347u32),
        ("Terracotta", 0xCF8264u32),
        ("Apricot Ash", 0xEABAA0u32),
        ("Dark Umber", 0x352C24u32),
        ("Bark", 0x66503Cu32),
        ("Raw Ochre", 0xA98246u32),
        ("Honey Dust", 0xD8B575u32),
        ("Linen Light", 0xF1DFC0u32),
        ("Deep Slate", 0x25393Bu32),
        ("Juniper Smoke", 0x45615Eu32),
        ("Copper Patina", 0x71918Au32),
        ("Faded Sage", 0xA4B9A8u32),
        ("Pale Celadon", 0xD4DFCCu32),
        ("Saffron Spark", 0xF1BA4Bu32),
        ("Rose Chalk", 0xF3D5C9u32),
    ];
    library.palettes[0].name = "Velvet Ember".into();
    for (s, &(name, hex)) in library.palettes[0].swatches.iter_mut().zip(&old) {
        s.name = name.into();
        s.color = RgbColor::new(
            RgbSpace::Srgb,
            [
                ((hex >> 16) & 255) as f32 / 255.,
                ((hex >> 8) & 255) as f32 / 255.,
                (hex & 255) as f32 / 255.,
                1.,
            ],
        )
        .unwrap();
    }
    let mut edited = library.clone();
    edited.palettes[0].swatches[0].name = "My shadow".into();
    let before = edited.clone();
    edited.ensure_starters();
    assert_eq!(edited, before, "user edits prevent replacement");
    let ids: Vec<_> = library.palettes[0].swatches.iter().map(|s| s.id).collect();
    library.ensure_starters();
    assert_eq!(
        library.palettes[0].name, "Pixel Arcade 2",
        "existing names remain unique"
    );
    assert_eq!(
        library.palettes[0]
            .swatches
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        ids[..STARTERS[1].1.len()]
    );
    assert_eq!(library.palettes[0].swatches[0].name, STARTERS[1].1[0].0);
    library.validate().unwrap();
}

#[test]
fn shortened_review_palette_preserves_edits_and_surviving_ids() {
    let mut library = ColorLibrary::default();
    library.ensure_starters();
    let old = [
        ("Black", 0x202020),
        ("Dark Gray", 0x484848),
        ("Middle Gray", 0x757575),
        ("Gray", 0xA3A3A3),
        ("Light Gray", 0xCECECE),
        ("White", 0xF1F1F1),
        ("Warm Black", 0x292623),
        ("Warm Dark Gray", 0x514B44),
        ("Warm Middle Gray", 0x7E756A),
        ("Warm Gray", 0xAEA294),
        ("Warm Light Gray", 0xD4C9B8),
        ("Warm White", 0xEEE7DA),
        ("Cool Black", 0x252B32),
        ("Cool Dark Gray", 0x505D6A),
        ("Cool Middle Gray", 0x8593A0),
        ("Cool Light Gray", 0xB8C5CF),
        ("Cool White", 0xE3EBEF),
    ];
    library
        .apply(ColorLibraryAction::Import {
            name: "Grayscale".into(),
            swatches: old
                .into_iter()
                .map(|(name, hex)| {
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
        .unwrap();
    let index = library.palettes.len() - 1;
    let active = library.active;
    let ids: Vec<_> = library.palettes[index]
        .swatches
        .iter()
        .map(|s| s.id)
        .collect();
    for change in 0..4 {
        let mut edited = library.clone();
        let palette = &mut edited.palettes[index];
        match change {
            0 => palette.swatches.swap(0, 1),
            1 => palette.swatches[0].color.rgba[3] = 0.5,
            2 => {
                palette.swatches.pop();
            }
            _ => palette.name = "My Grayscale".into(),
        }
        let before = edited.clone();
        edited.ensure_starters();
        assert_eq!(edited, before, "edited palette must survive ({change})");
    }
    library.ensure_starters();
    let palette = &library.palettes[index];
    assert_eq!(library.active, active);
    assert_eq!(palette.name, "Ink 2");
    assert_eq!(
        palette.swatches.iter().map(|s| s.id).collect::<Vec<_>>(),
        ids[..11]
    );
    assert_eq!(palette.swatches.last().unwrap().color, RgbColor::WHITE);
    library.validate().unwrap();
    let before = library.clone();
    library.ensure_starters();
    assert_eq!(library, before, "upgrade is idempotent");
}

#[test]
fn ink_rename_preserves_existing_colors_and_ids() {
    let mut library = ColorLibrary::default();
    library.ensure_starters();
    let original = library.clone();
    library.palettes[9].name = "Manga Ink".into();
    library.ensure_starters();
    assert_eq!(library, original);
}
