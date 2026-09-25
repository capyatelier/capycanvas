use super::*;

fn palette(colors: &[(&str, RgbColor)]) -> ColorPalette {
    ColorPalette {
        id: 1,
        name: "Study".into(),
        swatches: colors
            .iter()
            .enumerate()
            .map(|(i, (name, color))| SavedColor {
                id: i as u64 + 2,
                name: (*name).into(),
                color: *color,
            })
            .collect(),
    }
}
fn imported(bytes: &[u8]) -> (String, Vec<(String, RgbColor)>) {
    match ColorLibrary::import_file(bytes, "Fallback").unwrap() {
        ColorLibraryAction::Import { name, swatches } => (name, swatches),
        _ => unreachable!(),
    }
}
fn close(color: RgbColor, space: RgbSpace, expected: [f32; 4], tolerance: f32) {
    let actual = color.encoded_in(space).unwrap();
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, e)| (a - e).abs() <= tolerance),
        "{actual:?} != {expected:?}"
    );
}
fn aco(version: u16, entries: &[(u16, [u16; 4], &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(version.to_be_bytes());
    out.extend((entries.len() as u16).to_be_bytes());
    for (space, values, name) in entries {
        out.extend(space.to_be_bytes());
        for v in values {
            out.extend(v.to_be_bytes());
        }
        if version == 2 {
            let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
            out.extend((units.len() as u32).to_be_bytes());
            for u in units {
                out.extend(u.to_be_bytes());
            }
        }
    }
    out
}

#[test]
fn srgb_formats_round_trip_names_order_and_values() {
    let colors = [
        (
            "Ink",
            RgbColor::new(RgbSpace::Srgb, [0.05, 0.05, 0.06, 1.]).unwrap(),
        ),
        (
            "Coral “warm”",
            RgbColor::new(RgbSpace::Srgb, [1., 0.5, 0.25, 1.]).unwrap(),
        ),
        (
            "日本の藍",
            RgbColor::new(RgbSpace::Srgb, [0.1, 0.2, 0.6, 1.]).unwrap(),
        ),
    ];
    let source = palette(&colors);
    for (format, tolerance, named) in [
        (PaletteFormat::Aco, 0.51 / 65535., "Fallback"),
        (PaletteFormat::Ase, 1e-7, "Study"),
        (PaletteFormat::Gpl, 0.51 / 255., "Study"),
    ] {
        let export = source.export(format).unwrap();
        assert!(export.notice.is_none(), "{format:?}");
        assert_eq!(export.file_name, format!("Study.{}", format.extension()));
        let (name, swatches) = imported(&export.bytes);
        assert_eq!(name, named, "{format:?}");
        assert_eq!(swatches.len(), colors.len());
        for ((name, color), (expected_name, expected)) in swatches.iter().zip(colors) {
            assert_eq!(name, expected_name);
            close(*color, RgbSpace::Srgb, expected.rgba, tolerance);
        }
    }
}

#[test]
fn capycolor_is_exact_and_other_exports_report_conversion() {
    let wide = RgbColor::from_linear(RgbSpace::DisplayP3, [4., 0.1, 0.3, 0.25]).unwrap();
    let source = palette(&[("HDR", wide), ("Plain", RgbColor::BLACK)]);
    let export = source.export(PaletteFormat::Capycolor).unwrap();
    assert!(export.notice.is_none());
    assert_eq!(imported(&export.bytes).1[0].1, wide);
    for format in [PaletteFormat::Aco, PaletteFormat::Ase, PaletteFormat::Gpl] {
        let notice = source.export(format).unwrap().notice.unwrap();
        assert_eq!(
            notice,
            "1 color outside sRGB was clipped; 1 color became opaque. Capycolor keeps exact colors."
        );
    }
}

#[test]
fn aco_reads_every_documented_model_and_both_versions() {
    let entries = [
        (0, [65535, 0, 0, 0], "Red"),
        (1, [21845, 65535, 65535, 0], "HSB green"),
        (2, [0, 65535, 65535, 65535], "Cyan ink"),
        (7, [10000, 0, 0, 0], "Lab white"),
        (8, [10000, 0, 0, 0], "Gray"),
    ];
    let mut file = aco(1, &entries);
    file.extend(aco(2, &entries));
    file.extend(b"8BIMphry\0\0\0\x04\0\0\0\x10");
    let (name, colors) = imported(&file);
    assert_eq!(name, "Fallback");
    assert_eq!(
        colors.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
        entries.map(|e| e.2)
    );
    let tolerance = 1e-4;
    close(colors[0].1, RgbSpace::Srgb, [1., 0., 0., 1.], tolerance);
    close(colors[1].1, RgbSpace::Srgb, [0., 1., 0., 1.], tolerance);
    close(colors[2].1, RgbSpace::Srgb, [0., 1., 1., 1.], tolerance);
    close(colors[3].1, RgbSpace::Srgb, [1., 1., 1., 1.], 2e-3);
    close(colors[4].1, RgbSpace::Srgb, [0., 0., 0., 1.], tolerance);
    let (_, unnamed) = imported(&aco(1, &entries));
    assert!(unnamed.iter().all(|c| c.0.is_empty()));
    let (_, named) = imported(&aco(2, &entries));
    assert_eq!(named[1].0, "HSB green");
    for bad in [
        aco(1, &[(3, [1, 2, 3, 0], "Pantone")]),
        aco(1, &[(9, [0, 0, 0, 10000], "Wide CMYK")]),
        aco(2, &entries)[..30].to_vec(),
        [aco(1, &entries), aco(2, &entries[..2])].concat(),
    ] {
        assert!(ColorLibrary::import_file(&bad, "Bad").is_err());
    }
}

#[test]
fn lab_keeps_wide_colors_in_the_smallest_containing_space() {
    let neutral = lab(50., 0., 0.).unwrap();
    assert_eq!(neutral.space, RgbSpace::Srgb);
    let saturated = lab(60., 90., 70.).unwrap();
    assert_ne!(saturated.space, RgbSpace::Srgb);
    assert!(!saturated.in_gamut(RgbSpace::Srgb).unwrap());
}

#[test]
fn ase_reads_groups_models_and_rejects_damage() {
    let color = |name: &str, model: &[u8; 4], values: &[f32]| {
        let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let mut body = (units.len() as u16).to_be_bytes().to_vec();
        units.iter().for_each(|u| body.extend(u.to_be_bytes()));
        body.extend(model);
        values.iter().for_each(|v| body.extend(v.to_be_bytes()));
        body.extend(2u16.to_be_bytes());
        [
            1u16.to_be_bytes().to_vec(),
            (body.len() as u32).to_be_bytes().to_vec(),
            body,
        ]
        .concat()
    };
    let blocks = [
        color("Loose", b"RGB ", &[0., 0., 1.]),
        color("Cyan", b"CMYK", &[1., 0., 0., 0.]),
        color("Mid", b"LAB ", &[0.5, 0., 0.]),
        color("Light", b"Gray", &[0.75]),
    ];
    let mut file = b"ASEF\0\x01\0\0".to_vec();
    file.extend((blocks.len() as u32).to_be_bytes());
    blocks.iter().for_each(|b| file.extend(b));
    let (name, colors) = imported(&file);
    assert_eq!(name, "Fallback", "ungrouped colors do not name the palette");
    close(colors[1].1, RgbSpace::Srgb, [0., 1., 1., 1.], 1e-6);
    close(
        colors[2].1,
        RgbSpace::Srgb,
        [0.4663, 0.4663, 0.4663, 1.],
        2e-3,
    );
    close(colors[3].1, RgbSpace::Srgb, [0.75, 0.75, 0.75, 1.], 1e-6);
    let mut unknown = b"ASEF\0\x01\0\0\0\0\0\x01".to_vec();
    unknown.extend(color("Odd", b"HSV ", &[0., 0., 0.]));
    for bad in [
        unknown,
        file[..file.len() - 3].to_vec(),
        b"ASEF\0\x02\0\0\0\0\0\0".to_vec(),
    ] {
        assert!(ColorLibrary::import_file(&bad, "Bad").is_err());
    }
}

#[test]
fn procreate_swatches_round_trip_and_read_both_generations() {
    let wide = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 0.5]).unwrap();
    let mut colors = vec![("Wide", wide)];
    let gray = RgbColor::new(RgbSpace::Srgb, [0.5, 0.25, 0.75, 1.]).unwrap();
    colors.extend(std::iter::repeat_n(("Gray", gray), 30));
    let export = palette(&colors).export(PaletteFormat::Swatches).unwrap();
    let notice = export.notice.unwrap();
    assert!(
        notice.contains("1 color outside sRGB")
            && notice.contains("1 color became opaque")
            && notice.contains("1 color after"),
        "{notice}"
    );
    let (name, swatches) = imported(&export.bytes);
    assert_eq!(name, "Study");
    assert_eq!(swatches.len(), 30);
    close(swatches[0].1, RgbSpace::Srgb, [1., 0., 0., 1.], 1e-6);
    close(swatches[1].1, RgbSpace::Srgb, [0.5, 0.25, 0.75, 1.], 1e-6);
    assert!(swatches.iter().all(|s| s.0.is_empty()));
    let modern = br#"{"name":"Dusk","swatches":[null,
        {"alpha":1,"origin":2,"colorSpace":1,"colorModel":0,"brightness":0.99,"components":[0.9882352941,0.6745098039,0.6745098039],"version":"5.0","saturation":0.32,"hue":0},
        {"hue":0.5,"saturation":1,"brightness":1,"colorspace":0,"origin":1,"components":[0.5,1,1]}],"colorProfiles":[]}"#;
    let file = archive::zip(&[("__MACOSX/._x.json", b"{}"), ("palette.json", modern)]).unwrap();
    let (name, swatches) = imported(&file);
    assert_eq!(name, "Dusk");
    assert_eq!(swatches[0].1.space, RgbSpace::DisplayP3);
    close(
        swatches[0].1,
        RgbSpace::DisplayP3,
        [252. / 255., 172. / 255., 172. / 255., 1.],
        1e-6,
    );
    close(swatches[1].1, RgbSpace::Srgb, [0., 1., 1., 1.], 1e-6);
}

#[test]
fn clip_studio_color_sets_read_names_and_skip_empty_cells() {
    let title = "Mix";
    let mut header = (title.len() as u16).to_le_bytes().to_vec();
    header.extend(title.as_bytes());
    header.extend([0; 4]);
    header.extend((title.len() as u16).to_le_bytes());
    header.extend(title.as_bytes());
    let mut table = Vec::new();
    for (rgba, name) in [
        ([0xff, 0x28, 0x3c, 0xff], ""),
        ([0, 0, 0, 0], ""),
        ([0, 0, 0, 0xff], "黒"),
    ] {
        let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut entry = rgba.to_vec();
        entry.extend(u32::from(!name.is_empty()).to_le_bytes());
        if !name.is_empty() {
            entry.extend((units.len() as u16).to_le_bytes());
            entry.extend(&units);
        }
        table.extend((entry.len() as u32).to_le_bytes());
        table.extend(entry);
    }
    let mut file = b"SLCC\0\x01".to_vec();
    file.extend((header.len() as u32).to_le_bytes());
    file.extend(&header);
    file.extend(4u32.to_le_bytes());
    file.extend(3u32.to_le_bytes());
    file.extend((table.len() as u32).to_le_bytes());
    file.extend(&table);
    let (name, colors) = imported(&file);
    assert_eq!(name, "Mix");
    assert_eq!(
        colors.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
        ["", "黒"]
    );
    close(
        colors[0].1,
        RgbSpace::Srgb,
        [1., 40. / 255., 60. / 255., 1.],
        1e-6,
    );
    assert!(ColorLibrary::import_file(&file[..file.len() - 1], "Bad").is_err());
}

#[test]
fn affinity_palettes_resolve_references_names_and_models() {
    let tag = |t: &[u8; 4]| [t[3], t[2], t[1], t[0]];
    let prop = |kind: u8, key: &[u8; 4]| [vec![kind], tag(key).to_vec()].concat();
    let mut stream = b"\0\xffKS\x02\0".to_vec();
    stream.extend(tag(b"PalV"));
    stream.extend([1, 0, 25, 0, 0, 0]);
    stream.extend(prop(0x2B, b"PlCN"));
    stream.extend(6u32.to_le_bytes());
    stream.extend(b"Denis ");
    stream.extend(prop(0xB1, b"PalV"));
    stream.extend(4u32.to_le_bytes());
    let fill = |stream: &mut Vec<u8>, id: u32, first: bool| {
        stream.push(1);
        stream.extend(id.to_le_bytes());
        if first {
            stream.push(0);
            stream.extend(tag(b"FilS"));
            stream.extend([1, 0, 0, 0]);
            stream.extend(tag(b"Fill"));
            stream.extend([0, 0, 0, 2]);
        } else {
            stream.push(1);
            stream.extend(tag(b"FilS"));
        }
        stream.extend(prop(0x31, b"Colr"));
    };
    fill(&mut stream, 0, true);
    stream.extend([1, 1, 0, 0, 0, 0]);
    stream.extend(tag(b"RGBA"));
    stream.extend([1, 0, 0, 2]);
    stream.extend(prop(0x44, b"_col"));
    for v in [1f32, 0.5, 0., 0.25] {
        stream.extend(v.to_le_bytes());
    }
    stream.extend([0, 0]);
    stream.extend([2, 0, 0, 0, 0]);
    fill(&mut stream, 2, false);
    stream.extend([1, 3, 0, 0, 0, 0]);
    stream.extend(tag(b"LABA"));
    stream.extend([1, 0, 0, 2]);
    stream.extend(prop(0x3C, b"_col"));
    for v in [65535u16, 32896, 32896, 65535] {
        stream.extend(v.to_le_bytes());
    }
    stream.extend([0, 0]);
    stream.push(1);
    stream.extend(4u32.to_le_bytes());
    stream.push(0);
    stream.extend(tag(b"FilG"));
    stream.extend([2, 0, 0, 2]);
    stream.extend(prop(0x2A, b"Type"));
    stream.extend([0; 4]);
    stream.push(0);
    let names = ["Orange", "Orange copy", "White", "Gradient"];
    let mut table = (names.len() as u32).to_le_bytes().to_vec();
    for name in names {
        table.extend((name.len() as u32).to_le_bytes());
        table.extend(name.as_bytes());
    }
    stream.extend(prop(0xAB, b"PaNV"));
    stream.extend((table.len() as u32 - 4).to_le_bytes());
    stream.extend(table);
    stream.push(0);
    let mut file = vec![0, 0xff, 0x4b, 0x41, 11, 0, 0, 0];
    file.extend(tag(b"Swth"));
    file.extend(b"#Inf");
    file.resize(0x20, 0);
    file.extend((stream.len() as u64).to_le_bytes());
    file.resize(0x48, 0);
    file.extend(b"#Fil");
    file.extend(&stream);
    file.extend([0xff; 4]);
    let (name, colors) = imported(&file);
    assert_eq!(name, "Denis ");
    assert_eq!(
        colors.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
        &names[..3]
    );
    close(colors[0].1, RgbSpace::Srgb, [1., 0.5, 0., 0.25], 1e-6);
    assert_eq!(colors[1].1, colors[0].1);
    close(colors[2].1, RgbSpace::Srgb, [1., 1., 1., 1.], 2e-3);
    for end in [0x30, 0x60, file.len() - 12] {
        assert!(ColorLibrary::import_file(&file[..end], "Bad").is_err());
    }
}

#[test]
fn krita_palettes_read_models_and_names_from_colorset_xml() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ColorSet version="2.0" name="Krita &amp; Friends" columns="16" rows="2" comment="">
 <ColorSetEntry name="Gray" id="3" spot="false" bitdepth="U8"><Gray g="0.5" space="Gray-D50"/><Position row="0" column="1"/></ColorSetEntry>
 <ColorSetEntry name="Red" id="1" spot="false" bitdepth="U8">
  <RGB r="1" g="0" b="0" space="sRGB-elle-V2-srgbtrc.icc"/>
  <Position row="0" column="0"/>
 </ColorSetEntry>
 <Group name="Inks" rows="1">
  <ColorSetEntry name="Linear" id="4" spot="false" bitdepth="F32"><RGB r="0.2158605307340622" g="0.2158605307340622" b="0.2158605307340622"/></ColorSetEntry>
  <ColorSetEntry name="Cyan" id="2" spot="false" bitdepth="U16"><CMYK c="1" m="0" y="0" k="0" space="Chemical proof"/><Position row="0" column="0"/></ColorSetEntry>
 </Group>
</ColorSet>"#;
    let file = archive::zip(&[
        ("mimetype", b"application/x-krita-palette"),
        ("colorset.xml", xml.as_bytes()),
    ])
    .unwrap();
    let (name, colors) = imported(&file);
    assert_eq!(name, "Krita & Friends");
    assert_eq!(
        colors.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
        ["Red", "Gray", "Cyan", "Linear"]
    );
    close(
        colors[3].1,
        RgbSpace::Srgb,
        [128. / 255., 128. / 255., 128. / 255., 1.],
        1e-4,
    );
    let legacy = xml.replace("version=\"2.0\"", "version=\"1.0\"").replace(
        r#"<Gray g="0.5" space="Gray-D50"/>"#,
        r#"<Lab L="1" a="0.50196" b="0.50196" space="Lab identity built-in"/>"#,
    );
    let file = archive::zip(&[("colorset.xml", legacy.as_bytes())]).unwrap();
    close(
        imported(&file).1[1].1,
        RgbSpace::Srgb,
        [1., 1., 1., 1.],
        2e-3,
    );
    close(colors[2].1, RgbSpace::Srgb, [0., 1., 1., 1.], 1e-6);
}

#[test]
fn archives_are_bounded_and_checked() {
    let json = format!(
        r#"[{{"name":"Big","swatches":[{}]}}]"#,
        vec!["null"; 2_000_000].join(",")
    );
    let file = archive::zip(&[("Swatches.json", json.as_bytes())]).unwrap();
    assert!(file.len() < ColorLibrary::MAX_IMPORT_BYTES);
    assert!(
        ColorLibrary::import_file(&file, "Bomb")
            .unwrap_err()
            .contains("8 MB")
    );
    let mut damaged = palette(&[("Gray", RgbColor::BLACK)])
        .export(PaletteFormat::Swatches)
        .unwrap()
        .bytes;
    damaged[50] ^= 0xff;
    assert!(ColorLibrary::import_file(&damaged, "Bad").is_err());
    let other = archive::zip(&[("readme.txt", b"hello")]).unwrap();
    assert!(ColorLibrary::import_file(&other, "Bad").is_err());
    assert!(ColorLibrary::import_file(b"PK\x03\x04", "Bad").is_err());
}

#[test]
fn imports_clip_foreign_names_and_detect_by_content() {
    let long = "x".repeat(100);
    let file = format!("GIMP Palette\nName: {long}\n0 0 0 {long}\n");
    let action = ColorLibrary::import_file(file.as_bytes(), "Fallback").unwrap();
    let mut library = ColorLibrary::default();
    library.apply(action).unwrap();
    assert_eq!(library.active_palette().name.chars().count(), 64);
    assert!(PaletteFormat::IMPORT_EXTENSIONS.contains(&"kpl"));
    assert_eq!(
        palette(&[]).export(PaletteFormat::Gpl).unwrap().file_name,
        "Study.gpl"
    );
    let mut odd = palette(&[]);
    odd.name = "a/b: c?".into();
    assert_eq!(
        odd.export(PaletteFormat::Ase).unwrap().file_name,
        "a_b_ c_.ase"
    );
}
