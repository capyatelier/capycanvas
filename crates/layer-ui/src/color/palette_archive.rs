use super::*;
use std::io::{Read, Write};

const MAX_MEMBER: usize = 8 * 1024 * 1024;
pub(super) const PROCREATE_SLOTS: usize = 30;
const ZIP_VERSION: u16 = 20;
const DEFLATE: u16 = 8;
const DOS_DATE_1980: u16 = 0x21;

struct Member<'a> {
    name: &'a [u8],
    method: u16,
    crc: u32,
    size: usize,
    data: &'a [u8],
}

fn le16(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(corrupt)
}
fn le32(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(corrupt)
}
fn corrupt() -> String {
    "The palette archive is damaged".into()
}

fn members(bytes: &[u8]) -> Result<Vec<Member<'_>>, String> {
    let end = (0..=bytes.len().saturating_sub(22))
        .rev()
        .take(65_557)
        .find(|&at| bytes[at..].starts_with(b"PK\x05\x06"))
        .ok_or_else(corrupt)?;
    let count = usize::from(le16(bytes, end + 10)?);
    let mut at = le32(bytes, end + 16)? as usize;
    let mut members = Vec::with_capacity(count.min(256));
    for _ in 0..count {
        if le32(bytes, at)? != 0x0201_4b50 {
            return Err(corrupt());
        }
        let name_len = usize::from(le16(bytes, at + 28)?);
        let extra_len = usize::from(le16(bytes, at + 30)?);
        let skip = name_len + extra_len + usize::from(le16(bytes, at + 32)?);
        let mut sizes = [
            le32(bytes, at + 24)?,
            le32(bytes, at + 20)?,
            le32(bytes, at + 42)?,
        ]
        .map(u64::from);
        let mut extra = bytes
            .get(at + 46 + name_len..at + 46 + name_len + extra_len)
            .ok_or_else(corrupt)?;
        while extra.len() >= 4 {
            let (id, len) = (le16(extra, 0)?, usize::from(le16(extra, 2)?));
            let field = extra.get(4..4 + len).ok_or_else(corrupt)?;
            if id == 1 {
                let mut values = field
                    .chunks_exact(8)
                    .map(|c| u64::from_le_bytes(c.try_into().unwrap()));
                for size in sizes.iter_mut().filter(|s| **s == 0xFFFF_FFFF) {
                    *size = values.next().ok_or_else(corrupt)?;
                }
            }
            extra = &extra[4 + len..];
        }
        let [size, compressed, local] = sizes.map(|v| usize::try_from(v).unwrap_or(usize::MAX));
        if le32(bytes, local)? != 0x0403_4b50 {
            return Err(corrupt());
        }
        let start = local
            + 30
            + usize::from(le16(bytes, local + 26)?)
            + usize::from(le16(bytes, local + 28)?);
        members.push(Member {
            name: bytes.get(at + 46..at + 46 + name_len).ok_or_else(corrupt)?,
            method: le16(bytes, at + 10)?,
            crc: le32(bytes, at + 16)?,
            size,
            data: bytes
                .get(start..start.checked_add(compressed).ok_or_else(corrupt)?)
                .ok_or_else(corrupt)?,
        });
        at += 46 + skip;
    }
    Ok(members)
}

fn expand(member: &Member) -> Result<Vec<u8>, String> {
    if member.size > MAX_MEMBER {
        return Err("The palette archive expands beyond 8 MB".into());
    }
    let mut out = Vec::with_capacity(member.size);
    match member.method {
        0 => out.extend_from_slice(member.data),
        DEFLATE => {
            flate2::read::DeflateDecoder::new(member.data)
                .take(MAX_MEMBER as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|_| corrupt())?;
        }
        _ => return Err("The palette archive uses an unsupported compression method".into()),
    }
    let mut crc = flate2::Crc::new();
    crc.update(&out);
    if out.len() != member.size || crc.sum() != member.crc {
        return Err(corrupt());
    }
    Ok(out)
}

pub(super) fn read(bytes: &[u8]) -> Result<(String, Vec<Imported>), String> {
    let members = members(bytes)?;
    let find = |name: &[u8]| members.iter().find(|m| m.name.eq_ignore_ascii_case(name));
    let json = || {
        members.iter().find(|m| {
            m.name.len() > 5
                && m.name[m.name.len() - 5..].eq_ignore_ascii_case(b".json")
                && !m.name.starts_with(b"__MACOSX/")
        })
    };
    if let Some(member) = find(b"Swatches.json").or_else(json) {
        read_procreate(&expand(member)?)
    } else if let Some(member) = find(b"colorset.xml") {
        read_kpl(&expand(member)?)
    } else {
        Err("This archive is not a Procreate or Krita palette".into())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ProcreateFile {
    Many(Vec<ProcreatePalette>),
    One(ProcreatePalette),
}
#[derive(Deserialize)]
struct ProcreatePalette {
    #[serde(default)]
    name: String,
    swatches: Vec<Option<ProcreateSwatch>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcreateSwatch {
    hue: f64,
    saturation: f64,
    brightness: f64,
    #[serde(default, alias = "colorspace")]
    color_space: u8,
    #[serde(default)]
    origin: u8,
    #[serde(default)]
    components: Vec<f64>,
}

fn read_procreate(json: &[u8]) -> Result<(String, Vec<Imported>), String> {
    let palettes = match serde_json::from_slice(json)
        .map_err(|e| format!("Invalid Procreate palette: {e}"))?
    {
        ProcreateFile::Many(palettes) => palettes,
        ProcreateFile::One(palette) => vec![palette],
    };
    let mut colors = Vec::new();
    for swatch in palettes.iter().flat_map(|p| &p.swatches).flatten() {
        let rgb = match swatch.components[..] {
            [r, g, b] if swatch.origin == 2 => [r, g, b],
            _ => hsb(
                swatch.hue,
                swatch.saturation.clamp(0., 1.),
                swatch.brightness.clamp(0., 1.),
            ),
        };
        let space = if swatch.color_space == 1 {
            RgbSpace::DisplayP3
        } else {
            RgbSpace::Srgb
        };
        let color = RgbColor::new(space, [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.])?;
        push_color(&mut colors, String::new(), color)?;
    }
    Ok((
        palettes
            .into_iter()
            .next()
            .map(|p| p.name)
            .unwrap_or_default(),
        colors,
    ))
}

pub(super) fn write_swatches(name: &str, colors: &[(&str, [f32; 4])]) -> Result<Vec<u8>, String> {
    let mut slots = vec![serde_json::Value::Null; PROCREATE_SLOTS];
    for (slot, (_, rgba)) in slots.iter_mut().zip(colors) {
        let [r, g, b] = [rgba[0], rgba[1], rgba[2]].map(f64::from);
        let max = r.max(g).max(b);
        let delta = max - r.min(g).min(b);
        let hue = if delta == 0. {
            0.
        } else if max == r {
            ((g - b) / delta).rem_euclid(6.)
        } else if max == g {
            (b - r) / delta + 2.
        } else {
            (r - g) / delta + 4.
        } / 6.;
        *slot = serde_json::json!({
            "hue": hue,
            "saturation": if max == 0. { 0. } else { delta / max },
            "brightness": max,
            "alpha": 1,
            "colorSpace": 0,
        });
    }
    let json = serde_json::to_vec(&serde_json::json!([{ "name": name, "swatches": slots }]))
        .map_err(|e| e.to_string())?;
    zip(&[("Swatches.json", &json)])
}

pub(super) fn zip(files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in files {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).map_err(|e| e.to_string())?;
        let compressed = encoder.finish().map_err(|e| e.to_string())?;
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let offset = out.len() as u32;
        let fields = |header: &mut Vec<u8>| {
            header.extend(ZIP_VERSION.to_le_bytes());
            header.extend(0u16.to_le_bytes());
            header.extend(DEFLATE.to_le_bytes());
            header.extend(0u16.to_le_bytes());
            header.extend(DOS_DATE_1980.to_le_bytes());
            header.extend(crc.sum().to_le_bytes());
            header.extend((compressed.len() as u32).to_le_bytes());
            header.extend((data.len() as u32).to_le_bytes());
            header.extend((name.len() as u16).to_le_bytes());
            header.extend(0u16.to_le_bytes());
        };
        out.extend(0x0403_4b50u32.to_le_bytes());
        fields(&mut out);
        out.extend(name.as_bytes());
        out.extend(&compressed);
        directory.extend(0x0201_4b50u32.to_le_bytes());
        directory.extend(ZIP_VERSION.to_le_bytes());
        fields(&mut directory);
        let comment_disk_and_attributes = [0; 10];
        directory.extend(comment_disk_and_attributes);
        directory.extend(offset.to_le_bytes());
        directory.extend(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend(&directory);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((directory.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    Ok(out)
}

fn read_kpl(xml: &[u8]) -> Result<(String, Vec<Imported>), String> {
    use quick_xml::events::Event;
    let text = std::str::from_utf8(xml).map_err(|_| corrupt())?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut name = String::new();
    let mut legacy = false;
    let mut group = 0;
    let mut entries = Vec::new();
    let mut entry: Option<(String, String, Option<RgbColor>, [usize; 2])> = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("Invalid Krita palette: {e}"))?;
        let element = match &event {
            Event::Start(e) | Event::Empty(e) => e,
            Event::End(e) if e.local_name().as_ref() == b"ColorSetEntry" => {
                if let Some((title, _, Some(color), [row, column])) = entry.take() {
                    entries.push(((group, row, column, entries.len()), (title, color)));
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let mut attributes = std::collections::BTreeMap::new();
        for attribute in element.attributes() {
            let attribute = attribute.map_err(|_| corrupt())?;
            let value = attribute
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|_| corrupt())?
                .into_owned();
            attributes.insert(attribute.key.local_name().as_ref().to_vec(), value);
        }
        let text = |key: &[u8]| attributes.get(key).cloned().unwrap_or_default();
        let number = |key: &[u8]| -> Result<f64, String> {
            text(key).trim().parse::<f64>().map_err(|_| {
                format!(
                    "Invalid Krita color value “{}”",
                    String::from_utf8_lossy(key)
                )
            })
        };
        match element.local_name().as_ref() {
            b"ColorSet" => {
                name = text(b"name");
                legacy = text(b"version").trim() == "1.0";
            }
            b"Group" => group += 1,
            b"ColorSetEntry" if matches!(event, Event::Start(_)) => {
                entry = Some((text(b"name"), text(b"bitdepth"), None, [usize::MAX; 2]));
            }
            b"Position" => {
                if let Some((.., position)) = entry.as_mut() {
                    *position = [number(b"row")?, number(b"column")?].map(|v| v.max(0.) as usize);
                }
            }
            model @ (b"RGB" | b"sRGB" | b"CMYK" | b"Lab" | b"XYZ" | b"Gray") => {
                let Some((_, depth, slot @ None, _)) = entry.as_mut() else {
                    continue;
                };
                *slot = Some(match model {
                    b"sRGB" => srgb([number(b"r")?, number(b"g")?, number(b"b")?])?,
                    b"RGB" => {
                        let space = text(b"space");
                        let space =
                            if space.is_empty() && depth.as_str() != "U8" && !depth.is_empty() {
                                "g10"
                            } else {
                                &space
                            };
                        profiled_rgb(space, [number(b"r")?, number(b"g")?, number(b"b")?])?
                    }
                    b"CMYK" => srgb(cmyk(
                        number(b"c")?,
                        number(b"m")?,
                        number(b"y")?,
                        number(b"k")?,
                    ))?,
                    b"Lab" if legacy => lab(
                        number(b"L")? * 100.,
                        number(b"a")? * 255. - 128.,
                        number(b"b")? * 255. - 128.,
                    )?,
                    b"Lab" => lab(number(b"L")?, number(b"a")?, number(b"b")?)?,
                    b"XYZ" => xyz([number(b"x")?, number(b"y")?, number(b"z")?])?,
                    _ => srgb([number(b"g")?; 3])?,
                });
            }
            _ => {}
        }
    }
    entries.sort_by_key(|(order, _)| *order);
    let mut colors = Vec::with_capacity(entries.len());
    for (_, (title, color)) in entries {
        push_color(&mut colors, title, color)?;
    }
    Ok((name, colors))
}

fn profiled_rgb(profile: &str, rgb: [f64; 3]) -> Result<RgbColor, String> {
    let profile = profile.to_ascii_lowercase();
    let has = |names: &[&str]| names.iter().any(|n| profile.contains(n));
    let space = if has(&["p3"]) {
        RgbSpace::DisplayP3
    } else if has(&["adobe", "clayrgb"]) {
        RgbSpace::AdobeRgb
    } else if has(&["prophoto", "romm", "largergb"]) {
        RgbSpace::ProPhoto
    } else {
        RgbSpace::Srgb
    };
    let rgba = [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.];
    if has(&["g10", "linear"]) {
        RgbColor::from_linear(space, rgba)
    } else {
        RgbColor::new(space, rgba)
    }
}
