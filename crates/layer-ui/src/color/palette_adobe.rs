use super::*;

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    pub(super) fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
    pub(super) fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or("The palette file ends unexpectedly")?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f64, String> {
        Ok(f64::from(f32::from_be_bytes(
            self.take(4)?.try_into().unwrap(),
        )))
    }
    fn utf16(&mut self, units: usize) -> Result<String, String> {
        let units: Vec<u16> = self
            .take(units.checked_mul(2).ok_or("Invalid name length")?)?
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .take_while(|u| *u != 0)
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }
}

fn utf16_units(name: &str) -> Vec<u16> {
    name.encode_utf16().chain([0]).collect()
}
fn push_utf16(out: &mut Vec<u8>, units: &[u16]) {
    for unit in units {
        out.extend(unit.to_be_bytes());
    }
}

pub(super) fn read_aco(bytes: &[u8]) -> Result<Option<Vec<Imported>>, String> {
    if bytes.len() < 4 || !matches!(u16::from_be_bytes([bytes[0], bytes[1]]), 1 | 2) {
        return Ok(None);
    }
    let mut reader = Reader::new(bytes);
    let mut colors = aco_section(&mut reader)?;
    if reader.remaining() >= 4 && reader.bytes[reader.at..].starts_with(&[0, 2]) {
        let named = aco_section(&mut reader)?;
        if named.len() != colors.len() {
            return Err("The named and unnamed color lists differ".into());
        }
        colors = named;
    }
    Ok(Some(colors))
}

fn aco_section(reader: &mut Reader) -> Result<Vec<Imported>, String> {
    let version = reader.u16()?;
    if !matches!(version, 1 | 2) {
        return Err(format!("Unsupported swatch file version {version}"));
    }
    let count = usize::from(reader.u16()?);
    if count > reader.remaining() / 10 {
        return Err("The palette file ends unexpectedly".into());
    }
    let mut colors = Vec::with_capacity(count.min(ColorLibrary::MAX_SWATCHES));
    for _ in 0..count {
        let space = reader.u16()?;
        let v = [reader.u16()?, reader.u16()?, reader.u16()?, reader.u16()?];
        let name = if version == 2 {
            let units = reader.u32()? as usize;
            if units > reader.remaining() / 2 {
                return Err("The palette file ends unexpectedly".into());
            }
            reader.utf16(units)?
        } else {
            String::new()
        };
        let unit = |i: usize| f64::from(v[i]) / 65535.;
        let inverted_ink = |i: usize| 1. - unit(i);
        let gray_coverage = f64::from(v[0].min(10_000)) / 10_000.;
        let color = match space {
            0 => srgb([unit(0), unit(1), unit(2)])?,
            1 => srgb(hsb(f64::from(v[0]) / 65536., unit(1), unit(2)))?,
            2 => srgb(cmyk(inverted_ink(0), inverted_ink(1), inverted_ink(2), inverted_ink(3)))?,
            7 => lab(
                f64::from(v[0]) / 100.,
                f64::from(v[1] as i16) / 100.,
                f64::from(v[2] as i16) / 100.,
            )?,
            8 => srgb([1. - gray_coverage; 3])?,
            _ => return Err(
                "This palette uses color-book references (such as Pantone) that cannot be converted"
                    .into(),
            ),
        };
        push_color(&mut colors, name, color)?;
    }
    Ok(colors)
}

pub(super) fn write_aco(colors: &[(&str, [f32; 4])]) -> Result<Vec<u8>, String> {
    let count = u16::try_from(colors.len()).map_err(|_| too_many())?;
    let mut out = Vec::with_capacity(8 + colors.len() * 32);
    for version in [1u16, 2] {
        out.extend(version.to_be_bytes());
        out.extend(count.to_be_bytes());
        for (name, rgba) in colors {
            out.extend(0u16.to_be_bytes());
            for channel in &rgba[..3] {
                out.extend(((channel * 65535.).round() as u16).to_be_bytes());
            }
            out.extend(0u16.to_be_bytes());
            if version == 2 {
                let units = utf16_units(name);
                out.extend((units.len() as u32).to_be_bytes());
                push_utf16(&mut out, &units);
            }
        }
    }
    Ok(out)
}

const GROUP_START: u16 = 0xC001;
const GROUP_END: u16 = 0xC002;
const COLOR: u16 = 0x0001;
const PROCESS_COLOR: u16 = 2;

pub(super) fn read_ase(bytes: &[u8]) -> Result<(String, Vec<Imported>), String> {
    let mut reader = Reader::new(bytes);
    reader.take(4)?;
    let major = reader.u16()?;
    reader.u16()?;
    if major != 1 {
        return Err(format!("Unsupported Adobe Swatch Exchange version {major}"));
    }
    reader.u32()?;
    let mut ase = Ase::default();
    ase.blocks(&mut reader, 0)?;
    let name = if ase.groups.len() == 1 && !ase.ungrouped {
        ase.groups.pop().unwrap()
    } else {
        String::new()
    };
    Ok((name, ase.colors))
}

#[derive(Default)]
struct Ase {
    colors: Vec<Imported>,
    groups: Vec<String>,
    ungrouped: bool,
}
impl Ase {
    fn blocks(&mut self, reader: &mut Reader, depth: usize) -> Result<(), String> {
        let mut depth = depth;
        while reader.remaining() > 0 {
            let kind = reader.u16()?;
            if kind == GROUP_END && reader.remaining() == 0 {
                break;
            }
            let length = reader.u32()? as usize;
            let mut block = Reader::new(reader.take(length)?);
            let name = |block: &mut Reader| -> Result<String, String> {
                let units = usize::from(block.u16()?);
                block.utf16(units)
            };
            match kind {
                GROUP_START => {
                    self.groups.push(if length == 0 {
                        String::new()
                    } else {
                        name(&mut block)?
                    });
                    depth += 1;
                    if depth > 8 {
                        return Err("Invalid swatch group nesting".into());
                    }
                    self.blocks(&mut block, depth)?;
                }
                GROUP_END => depth = depth.saturating_sub(1),
                COLOR => {
                    self.ungrouped |= depth == 0;
                    let name = name(&mut block)?;
                    let model = block.take(4)?;
                    let color = match String::from_utf8_lossy(model)
                        .trim()
                        .to_ascii_uppercase()
                        .as_str()
                    {
                        "RGB" => srgb([block.f32()?, block.f32()?, block.f32()?])?,
                        "CMYK" => {
                            srgb(cmyk(block.f32()?, block.f32()?, block.f32()?, block.f32()?))?
                        }
                        "LAB" => {
                            let l = block.f32()?;
                            lab(
                                if l > 1.001 { l } else { l * 100. },
                                block.f32()?,
                                block.f32()?,
                            )?
                        }
                        "GRAY" => srgb([block.f32()?; 3])?,
                        other => return Err(format!("Unsupported color model “{other}”")),
                    };
                    push_color(&mut self.colors, name, color)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub(super) fn write_ase(title: &str, colors: &[(&str, [f32; 4])]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(64 + colors.len() * 48);
    out.extend(b"ASEF");
    out.extend(1u16.to_be_bytes());
    out.extend(0u16.to_be_bytes());
    out.extend((colors.len() as u32 + 2).to_be_bytes());
    let block = |out: &mut Vec<u8>, kind: u16, name: &str, body: &[u8]| {
        let units = utf16_units(name);
        out.extend(kind.to_be_bytes());
        out.extend((2 + units.len() as u32 * 2 + body.len() as u32).to_be_bytes());
        out.extend((units.len() as u16).to_be_bytes());
        push_utf16(out, &units);
        out.extend(body);
    };
    block(&mut out, GROUP_START, title, &[]);
    for (name, rgba) in colors {
        let mut body = b"RGB ".to_vec();
        for channel in &rgba[..3] {
            body.extend(channel.to_be_bytes());
        }
        body.extend(PROCESS_COLOR.to_be_bytes());
        block(&mut out, COLOR, name, &body);
    }
    out.extend(GROUP_END.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    Ok(out)
}
