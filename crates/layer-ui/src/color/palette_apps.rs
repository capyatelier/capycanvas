use super::*;

struct Le<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Le<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or("The palette file ends unexpectedly")?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn len32(&mut self) -> Result<usize, String> {
        Ok(self.u32()? as usize)
    }
    fn utf8(&mut self, count: usize) -> Result<String, String> {
        Ok(String::from_utf8_lossy(self.take(count)?).into_owned())
    }
    fn tag(&mut self) -> Result<[u8; 4], String> {
        let mut tag: [u8; 4] = self.take(4)?.try_into().unwrap();
        tag.reverse();
        Ok(tag)
    }
    fn f32s<const N: usize>(&mut self) -> Result<[f64; N], String> {
        let bytes = self.take(N * 4)?;
        Ok(std::array::from_fn(|i| {
            f64::from(f32::from_le_bytes(
                bytes[i * 4..i * 4 + 4].try_into().unwrap(),
            ))
        }))
    }
}

pub(super) fn read_cls(bytes: &[u8]) -> Result<(String, Vec<Imported>), String> {
    let mut r = Le { bytes, at: 4 };
    r.u16()?;
    let header = r.len32()?;
    let mut h = Le {
        bytes: r.take(header)?,
        at: 0,
    };
    let legacy = usize::from(h.u16()?);
    h.take(legacy)?;
    h.u32()?;
    let utf8 = usize::from(h.u16()?);
    let name = h.utf8(utf8)?;
    r.u32()?;
    let count = r.len32()?;
    let table = r.len32()?;
    let mut table = Le {
        bytes: r.take(table)?,
        at: 0,
    };
    let mut colors = Vec::new();
    for _ in 0..count {
        let length = table.len32()?;
        let mut entry = Le {
            bytes: table.take(length)?,
            at: 0,
        };
        let [red, green, blue, alpha]: [u8; 4] = entry.take(4)?.try_into().unwrap();
        let named = entry.bytes.len() >= 10 && entry.u32()? == 1;
        let title = if named {
            let length = usize::from(entry.u16()?);
            let units: Vec<u16> = entry
                .take(length)?
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        } else {
            String::new()
        };
        if alpha != 0 {
            let rgb = [red, green, blue].map(|v| f64::from(v) / 255.);
            push_color(&mut colors, title, srgb(rgb)?)?;
        }
    }
    Ok((name, colors))
}

enum Value {
    Text(String),
    Object(usize),
    Objects(Vec<usize>),
    Texts(Vec<String>),
    Data(Vec<u8>),
    Other,
}
struct Object {
    class: [u8; 4],
    properties: Vec<([u8; 4], Value)>,
}
impl Object {
    fn get(&self, key: &[u8; 4]) -> Option<&Value> {
        self.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
}
struct Stream<'a> {
    r: Le<'a>,
    objects: Vec<Option<Object>>,
    depth: usize,
}
impl Stream<'_> {
    fn object(&mut self) -> Result<usize, String> {
        match self.r.u8()? {
            2 => Ok(self.r.len32()?),
            1 => {
                let id = self.r.len32()?;
                if id != self.objects.len() || id > 65_536 {
                    return Err("Invalid Affinity palette object".into());
                }
                self.objects.push(None);
                let class = match self.r.u8()? {
                    0 => {
                        let class = self.r.tag()?;
                        let mut meta = self.r.take(4)?[3];
                        while meta == 0 {
                            self.r.tag()?;
                            meta = self.r.take(4)?[3];
                        }
                        if meta != 2 {
                            return Err("Invalid Affinity palette class".into());
                        }
                        class
                    }
                    1 => self.r.tag()?,
                    _ => return Err("Invalid Affinity palette class".into()),
                };
                self.depth += 1;
                if self.depth > 16 {
                    return Err("Invalid Affinity palette nesting".into());
                }
                let properties = self.properties()?;
                self.depth -= 1;
                self.objects[id] = Some(Object { class, properties });
                Ok(id)
            }
            _ => Err("Invalid Affinity palette object".into()),
        }
    }
    fn properties(&mut self) -> Result<Vec<([u8; 4], Value)>, String> {
        let mut properties = Vec::new();
        loop {
            let kind = self.r.u8()?;
            if kind == 0 {
                return Ok(properties);
            }
            let key = self.r.tag()?;
            let value = match kind {
                0x2B => {
                    let length = self.r.len32()?;
                    Value::Text(self.r.utf8(length)?)
                }
                0x31 => Value::Object(self.object()?),
                0xB1 => {
                    let count = self.r.len32()?;
                    if count > self.r.bytes.len() {
                        return Err("The palette file ends unexpectedly".into());
                    }
                    Value::Objects(
                        (0..count)
                            .map(|_| self.object())
                            .collect::<Result<_, _>>()?,
                    )
                }
                0xAB => {
                    self.r.u32()?;
                    let count = self.r.len32()?;
                    if count > self.r.bytes.len() {
                        return Err("The palette file ends unexpectedly".into());
                    }
                    let mut texts = Vec::with_capacity(count.min(4096));
                    for _ in 0..count {
                        let length = self.r.len32()?;
                        texts.push(self.r.utf8(length)?);
                    }
                    Value::Texts(texts)
                }
                0xA4 => {
                    let count = self.r.len32()?;
                    self.r
                        .take(count.checked_mul(16).ok_or("Invalid Affinity palette")?)?;
                    Value::Other
                }
                0x2A => {
                    self.r.take(4)?;
                    Value::Other
                }
                0x3C | 0x44 | 0x48 => Value::Data(
                    self.r
                        .take(match kind {
                            0x3C => 8,
                            0x44 => 16,
                            _ => 20,
                        })?
                        .to_vec(),
                ),
                _ => return Err("This Affinity palette uses an unsupported feature".into()),
            };
            properties.push((key, value));
        }
    }
}

pub(super) fn read_afpalette(bytes: &[u8]) -> Result<(String, Vec<Imported>), String> {
    let mut header = Le { bytes, at: 0x20 };
    let length = usize::try_from(u64::from(header.u32()?) | u64::from(header.u32()?) << 32)
        .map_err(|_| "The palette file ends unexpectedly")?;
    let body = Le { bytes, at: 0x4C }.take(length)?;
    if !body.starts_with(b"\0\xffKS") {
        return Err("This Affinity palette is compressed or damaged".into());
    }
    let mut stream = Stream {
        r: Le {
            bytes: body,
            at: 16,
        },
        objects: Vec::new(),
        depth: 0,
    };
    let root = Object {
        class: *b"PalV",
        properties: stream.properties()?,
    };
    let object = |id: usize| stream.objects.get(id).and_then(Option::as_ref);
    let name = match root.get(b"PlCN") {
        Some(Value::Text(name)) => name.clone(),
        _ => String::new(),
    };
    let names = match root.get(b"PaNV") {
        Some(Value::Texts(names)) => names.as_slice(),
        _ => &[],
    };
    let mut colors = Vec::new();
    let Some(Value::Objects(fills)) = root.get(b"PalV") else {
        return Ok((name, colors));
    };
    for (index, fill) in fills.iter().enumerate() {
        let color =
            object(*fill)
                .filter(|f| &f.class == b"FilS")
                .and_then(|f| match f.get(b"Colr") {
                    Some(Value::Object(id)) => object(*id),
                    _ => None,
                });
        let Some((class, Some(Value::Data(data)))) = color.map(|c| (c.class, c.get(b"_col")))
        else {
            continue;
        };
        let mut r = Le { bytes: data, at: 0 };
        let color = match (&class, data.len()) {
            (b"RGBA", 16) => {
                let [red, green, blue, alpha] = r.f32s::<4>()?;
                RgbColor::new(
                    RgbSpace::Srgb,
                    [red, green, blue, alpha.clamp(0., 1.)].map(|v| v as f32),
                )?
            }
            (b"CMYK", 20) => {
                let [c, m, y, k, _] = r.f32s::<5>()?;
                srgb(cmyk(c, m, y, k))?
            }
            (b"LABA", 8) => {
                let [l, a, b] = [r.u16()?, r.u16()?, r.u16()?].map(f64::from);
                lab(
                    l * 100. / 65535.,
                    a * 255. / 65535. - 128.,
                    b * 255. / 65535. - 128.,
                )?
            }
            _ => continue,
        };
        push_color(
            &mut colors,
            names.get(index).cloned().unwrap_or_default(),
            color,
        )?;
    }
    Ok((name, colors))
}
