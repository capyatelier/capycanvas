//! AVIF tone-map item selection and reconstruction in linear application RGB.
use super::*;
use crate::photo::gainmap::Metadata;

pub(super) struct Descriptor {
    pub base: u32,
    gain: u32,
    alternate: u32,
    metadata: Metadata,
}

fn preferred(
    container: &Container<'_>,
    alternate: u32,
    base: u32,
    cancel: &AtomicBool,
) -> Result<bool, String> {
    let mut result = None;
    for view in container::boxes(container.groups.unwrap_or_default()) {
        codec::check(cancel)?;
        let view = view?;
        if &view.kind != b"altr" {
            continue;
        }
        let mut r = Reader::new(view.data);
        if r.full()? != (0, 0) {
            return Err("Unsupported AVIF alternative group".into());
        }
        r.u32()?; // group id
        let count = r.u32()? as usize;
        if count > 4096 || count.checked_mul(4) != Some(r.left()) {
            return Err("Invalid AVIF alternative count".into());
        }
        let mut found = false;
        for _ in 0..count {
            let id = r.u32()?;
            if id == alternate {
                found = true;
            }
            if id == base && result.replace(found).is_some() {
                return Err("Duplicate AVIF alternative membership".into());
            }
        }
    }
    Ok(result.unwrap_or(false))
}

fn metadata(bytes: &[u8]) -> Result<Option<Metadata>, String> {
    let mut r = Reader::new(bytes);
    // Forward-compatible unsupported versions leave the SDR primary usable.
    if r.u8()? != 0 || r.u16()? != 0 {
        return Ok(None);
    }
    let writer = r.u16()?;
    let flags = r.u8()?;
    if flags & 0x3f != 0 {
        return Err("Invalid AVIF gain-map flags".into());
    }
    let mut fraction = |signed| -> Result<f32, String> {
        let n = r.u32()?;
        let d = r.u32()?;
        if d == 0 {
            return Err("Invalid AVIF gain-map denominator".into());
        }
        let n = if signed {
            f64::from(n as i32)
        } else {
            f64::from(n)
        };
        Ok((n / f64::from(d)) as f32)
    };
    let mut m = Metadata {
        min: [0.; 3],
        max: [0.; 3],
        gamma: [1.; 3],
        base_offset: [0.; 3],
        alternate_offset: [0.; 3],
        base_headroom: fraction(false)?,
        alternate_headroom: fraction(false)?,
        use_base_space: flags & 0x40 != 0,
    };
    let channels = if flags & 0x80 != 0 { 3 } else { 1 };
    for c in 0..channels {
        m.min[c] = fraction(true)?;
        m.max[c] = fraction(true)?;
        m.gamma[c] = fraction(false)?;
        m.base_offset[c] = fraction(true)?;
        m.alternate_offset[c] = fraction(true)?;
    }
    if channels == 1 {
        for values in [
            &mut m.min,
            &mut m.max,
            &mut m.gamma,
            &mut m.base_offset,
            &mut m.alternate_offset,
        ] {
            let first = values[0];
            values.fill(first);
        }
    }
    if writer == 0 {
        r.end()?;
    }
    Ok(Some(m.validate()?))
}

impl Descriptor {
    pub fn find(
        container: &Container<'_>,
        primary: u32,
        budget: usize,
        cancel: &AtomicBool,
    ) -> Result<Option<Self>, String> {
        let mut found = None;
        for item in &container.items {
            codec::check(cancel)?;
            if &item.kind != b"tmap" {
                continue;
            }
            let refs = container.targets(item.id, b"dimg")?;
            if refs.first() != Some(&primary) && item.id != primary {
                continue;
            }
            if refs.len() != 2 || refs[0] == refs[1] || refs.contains(&item.id) {
                return Err("Invalid AVIF tone-map derivation".into());
            }
            if item.id != primary && !preferred(container, item.id, primary, cancel)? {
                continue;
            }
            let payload = container.payload(item.id, budget.min(64 * 1024))?;
            let Some(metadata) = metadata(&payload)? else {
                if item.id == primary {
                    return Err("Unsupported primary AVIF tone-map version".into());
                }
                continue;
            };
            let descriptor = Self {
                base: refs[0],
                gain: refs[1],
                alternate: item.id,
                metadata,
            };
            if found.replace(descriptor).is_some() {
                return Err("Ambiguous AVIF HDR alternatives".into());
            }
        }
        Ok(found)
    }
    pub fn decode(
        self,
        container: &Container<'_>,
        base: &RawImage,
        base_properties: &Properties<'_>,
        profile: &ColorProfile,
        budget: usize,
        cancel: &AtomicBool,
    ) -> Result<GainMap, String> {
        let alternate = Properties::read(container, self.alternate)?;
        let gain = Properties::read(container, self.gain)?;
        if alternate.extent != Some(base.extent) || alternate.geometry != Geometry::default() {
            return Err("Invalid AVIF tone-map geometry".into());
        }
        if gain.geometry != base_properties.geometry
            || (gain.extent != Some(base.extent) && gain.geometry != Geometry::default())
        {
            return Err("AVIF gain-map geometry disagrees with its base".into());
        }
        if alpha_item(container, self.gain)?.is_some() {
            return Err("AVIF gain map must not have auxiliary alpha".into());
        }
        let budget = budget
            .checked_sub(alternate.icc.map_or(0, |v| v.len()))
            .ok_or("AVIF gain-map profile exceeds the codec budget")?;
        let application = if self.metadata.use_base_space {
            None
        } else {
            let mut color = alternate.color.unwrap_or(base.color);
            if color.cicp[0] == 2 {
                color.cicp[0] = base.color.cicp[0];
            }
            // GainMapColor creates a linear application profile. The alternate
            // transfer describes delivery, not the linear gain application.
            color.cicp[1] = 13;
            Some(color::profile(color, alternate.icc)?.0)
        };
        let color = crate::icc::GainMapColor::new(profile, application.as_ref())?;
        let image = decode_item(
            container,
            self.gain,
            None,
            false,
            &mut Vec::new(),
            budget,
            cancel,
        )?;
        let converter = color::Converter::new(image.color, image.layout)?;
        Ok(GainMap {
            image,
            metadata: self.metadata,
            color,
            converter,
        })
    }
}

pub(super) struct GainMap {
    image: RawImage,
    metadata: Metadata,
    color: crate::icc::GainMapColor,
    converter: color::Converter,
}
impl GainMap {
    pub fn bytes(&self) -> usize {
        self.image.pixels.len() * 8
    }
    pub fn pixel(
        &self,
        base: &RawImage,
        converter: &color::Converter,
        x: u32,
        y: u32,
    ) -> Result<[f32; 4], String> {
        let encoded = converter.pixel_at_depth(base, x, y, 16);
        let alpha = f32::from(base.pixels[y as usize * base.extent[0] as usize + x as usize][3])
            / ((1u32 << base.depth) - 1) as f32;
        let rgb = std::array::from_fn(|c| {
            let value = f32::from(encoded[c]) / 65535.;
            if base.premultiplied {
                if alpha == 0. {
                    0.
                } else {
                    (value / alpha).min(1.)
                }
            } else {
                value
            }
        });
        // Sample the gain grid at base pixel centers, clamping its borders.
        let coordinate = |pixel: u32, base: u32, gain: u32| {
            let v = ((pixel as f64 + 0.5) * f64::from(gain) / f64::from(base) - 0.5)
                .clamp(0., f64::from(gain - 1));
            (
                v.floor() as u32,
                (v.floor() as u32 + 1).min(gain - 1),
                (v - v.floor()) as f32,
            )
        };
        let (x0, x1, wx) = coordinate(x, base.extent[0], self.image.extent[0]);
        let (y0, y1, wy) = coordinate(y, base.extent[1], self.image.extent[1]);
        let samples = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
            .map(|(x, y)| self.converter.pixel_at_depth(&self.image, x, y, 16));
        let gain = std::array::from_fn(|c| {
            let v = samples.map(|p| f32::from(p[c]) / 65535.);
            (v[0] * (1. - wx) + v[1] * wx) * (1. - wy) + (v[2] * (1. - wx) + v[3] * wx) * wy
        });
        let linear = self.metadata.reconstruct(self.color.linear_base(rgb), gain);
        let rgb = self.color.to_srgb(linear);
        if rgb
            .iter()
            .any(|v| !v.is_finite() || v.abs() > layer_core::color::hdr::MAX_LINEAR)
        {
            return Err("AVIF gain map reconstructs pixels outside the HDR range".into());
        }
        Ok([rgb[0], rgb[1], rgb[2], alpha])
    }
}
