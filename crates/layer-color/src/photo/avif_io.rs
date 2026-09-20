//! Shared Rust AVIF container, AV1 decode, and source color/geometry pipeline.
use super::*;
use std::{borrow::Cow, sync::atomic::AtomicBool};
mod codec;
mod color;
mod container;
mod encode;
mod gainmap;
mod mux;
mod output;
mod properties;
mod sequence;
use container::{Container, Reader};
use properties::{Color, Geometry, Properties};
pub(super) use output::{preview, write};

// Probe brands without buffering the image or selecting a host-specific codec.
pub(super) fn is_avif(
    input: &mut (impl BufRead + Seek),
    cancel: &AtomicBool,
) -> Result<bool, String> {
    let origin = input.stream_position().map_err(err)?;
    let result = (|| {
        let mut header = [0; 8];
        input.read_exact(&mut header).map_err(err)?;
        if &header[4..] != b"ftyp" {
            return Ok(false);
        }
        let mut size = u64::from(u32::from_be_bytes(header[..4].try_into().unwrap()));
        let header_size = if size == 1 {
            input.read_exact(&mut header).map_err(err)?;
            size = u64::from_be_bytes(header);
            16
        } else {
            8
        };
        let payload = size
            .checked_sub(header_size)
            .ok_or("Invalid HEIF/AVIF file type size")?;
        if payload < 8 || payload > 256 * 1024 || payload % 4 != 0 {
            return Err("Invalid HEIF/AVIF brand list".into());
        }
        let mut found = false;
        for index in 0..payload / 4 {
            codec::check(cancel)?;
            let mut brand = [0; 4];
            input.read_exact(&mut brand).map_err(err)?;
            if index != 1 && matches!(&brand, b"avif" | b"avis") {
                found = true;
            }
        }
        Ok(found)
    })();
    input.seek(std::io::SeekFrom::Start(origin)).map_err(err)?;
    result
}

// Chroma remains in its encoded sampling grid until all derived tiles are
// joined. Converting tiles independently would clamp bilinear filtering at
// each internal edge and introduce visible seams.
struct RawImage {
    extent: [u32; 2],
    depth: u8,
    layout: u32,
    color: Color,
    premultiplied: bool,
    pixels: Vec<[u16; 4]>,
}
impl RawImage {
    fn plane_extent(&self, plane: usize) -> [u32; 2] {
        if plane == 0 {
            self.extent
        } else {
            [
                if matches!(self.layout, 1 | 2) {
                    self.extent[0].div_ceil(2)
                } else {
                    self.extent[0]
                },
                if self.layout == 1 {
                    self.extent[1].div_ceil(2)
                } else {
                    self.extent[1]
                },
            ]
        }
    }
    fn sample(&self, plane: usize, x: u32, y: u32) -> u16 {
        let x = if plane != 0 && matches!(self.layout, 1 | 2) {
            x * 2
        } else {
            x
        };
        let y = if plane != 0 && self.layout == 1 {
            y * 2
        } else {
            y
        };
        self.pixels[y as usize * self.extent[0] as usize + x as usize][plane]
    }
}
fn pixels(extent: [u32; 2], budget: usize) -> Result<Vec<[u16; 4]>, String> {
    validate_extent(extent, 32768)?;
    let count = (extent[0] as usize)
        .checked_mul(extent[1] as usize)
        .ok_or("AVIF image size overflow")?;
    if count.checked_mul(8).is_none_or(|n| n > budget) {
        return Err("AVIF image exceeds the codec budget".into());
    }
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| "AVIF image allocation failed")?;
    out.resize(count, [0; 4]);
    Ok(out)
}
fn alpha_item(container: &Container<'_>, id: u32) -> Result<Option<u32>, String> {
    let mut alpha = None;
    for reference in &container.references {
        if &reference.kind != b"auxl" || !reference.to.contains(&id) {
            continue;
        }
        let p = Properties::read(container, reference.from)?;
        if matches!(
            p.aux,
            Some(b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha" | b"urn:mpeg:hevc:2015:auxid:1")
        ) {
            if alpha.replace(reference.from).is_some() {
                return Err("Multiple AVIF alpha images".into());
            }
        }
    }
    Ok(alpha)
}
fn config_depth(config: &[u8]) -> Result<u8, String> {
    let b = *config.get(2).ok_or("Incomplete AVIF codec configuration")?;
    Ok(if b & 0x40 == 0 {
        8
    } else if b & 0x20 == 0 {
        10
    } else {
        12
    })
}
fn decode_coded(
    payload: &[u8],
    p: &Properties<'_>,
    inherited: Option<Color>,
    alpha: bool,
    budget: usize,
    cancel: &AtomicBool,
) -> Result<RawImage, String> {
    let extent = p.extent.ok_or("Missing AVIF spatial extent")?;
    let config = p.config.ok_or("Missing AVIF AV1 configuration")?;
    let mut output = pixels(extent, budget)?;
    let remaining = budget
        .checked_sub(output.len() * 8)
        .ok_or("AVIF codec budget is too small")?;
    let plane = codec::decode(payload, &config[4..], extent, remaining, cancel)?;
    if config_depth(config)? != plane.depth || p.bits.is_some_and(|v| v != plane.depth) {
        return Err("AVIF precision disagrees with its AV1 data".into());
    }
    if alpha && plane.layout != 0 {
        return Err("AVIF alpha must be monochrome".into());
    }
    if p.channels
        .is_some_and(|n| n != if plane.layout == 0 { 1 } else { 3 })
    {
        return Err("AVIF channel count disagrees with its AV1 data".into());
    }
    let color = inherited.unwrap_or(Color {
        cicp: plane.cicp,
        full_range: plane.full_range,
    });
    for (y, row) in output.chunks_exact_mut(extent[0] as usize).enumerate() {
        codec::check(cancel)?;
        for (x, pixel) in row.iter_mut().enumerate() {
            let (x, y) = (x as u32, y as u32);
            let (cx, cy) = (
                if matches!(plane.layout, 1 | 2) {
                    x / 2
                } else {
                    x
                },
                if plane.layout == 1 { y / 2 } else { y },
            );
            *pixel = [
                plane.sample(0, x, y),
                if plane.layout == 0 {
                    0
                } else {
                    plane.sample(1, cx, cy)
                },
                if plane.layout == 0 {
                    0
                } else {
                    plane.sample(2, cx, cy)
                },
                (1u16 << plane.depth) - 1,
            ];
        }
    }
    Ok(RawImage {
        extent,
        depth: plane.depth,
        layout: plane.layout,
        color,
        premultiplied: false,
        pixels: output,
    })
}
fn decode_item(
    container: &Container<'_>,
    id: u32,
    parent_color: Option<Color>,
    alpha: bool,
    stack: &mut Vec<u32>,
    budget: usize,
    cancel: &AtomicBool,
) -> Result<RawImage, String> {
    codec::check(cancel)?;
    if stack.len() == 8 || stack.contains(&id) {
        return Err("Cyclic or excessively nested AVIF derivation".into());
    }
    stack.push(id);
    let result = (|| {
        let item = container.item(id)?;
        let p = Properties::read(container, id)?;
        let extent = p.extent.ok_or("Missing AVIF spatial extent")?;
        validate_extent(extent, 32768)?;
        let inherited = p.color.or(parent_color);
        let mut image = match &item.kind {
            b"av01" => {
                let payload = container.payload(id, budget)?;
                let owned = if matches!(payload, Cow::Owned(_)) {
                    payload.len()
                } else {
                    0
                };
                let budget = budget
                    .checked_sub(owned)
                    .ok_or("AVIF item exceeds the codec budget")?;
                decode_coded(&payload, &p, inherited, alpha, budget, cancel)?
            }
            b"grid" => {
                let payload = container.payload(id, 12)?;
                let mut r = Reader::new(&payload);
                if r.u8()? != 0 {
                    return Err("Unsupported AVIF grid version".into());
                }
                let flags = r.u8()?;
                if flags > 1 {
                    return Err("Invalid AVIF grid flags".into());
                }
                let (rows, columns) = (r.u8()? as u32 + 1, r.u8()? as u32 + 1);
                let grid_extent = if flags == 0 {
                    [r.u16()? as u32, r.u16()? as u32]
                } else {
                    [r.u32()?, r.u32()?]
                };
                r.end()?;
                if grid_extent != extent {
                    return Err("AVIF grid disagrees with its spatial extent".into());
                }
                let tiles = container.targets(id, b"dimg")?;
                if tiles.len() != (rows * columns) as usize {
                    return Err("AVIF grid tile count mismatch".into());
                }
                let mut output = pixels(extent, budget)?;
                let mut characteristics = None;
                let remaining = budget
                    .checked_sub(output.len() * 8)
                    .ok_or("AVIF grid exceeds the codec budget")?;
                for (i, &tile) in tiles.iter().enumerate() {
                    let tile_p = Properties::read(container, tile)?;
                    if tile_p.geometry != Geometry::default() {
                        return Err("Unsupported AVIF grid tile transformation".into());
                    }
                    let tile =
                        decode_item(container, tile, inherited, alpha, stack, remaining, cancel)?;
                    let current = (tile.extent, tile.depth, tile.layout, tile.color);
                    if characteristics.is_some_and(|v| v != current) {
                        return Err("Inconsistent AVIF grid tile representation".into());
                    }
                    characteristics = Some(current);
                    let [tw, th] = tile.extent;
                    if (columns > 1 && tw % 2 != 0 && matches!(tile.layout, 1 | 2))
                        || (rows > 1 && th % 2 != 0 && tile.layout == 1)
                    {
                        return Err("AVIF grid tile chroma alignment mismatch".into());
                    }
                    if (columns - 1) * tw >= extent[0]
                        || columns * tw < extent[0]
                        || (rows - 1) * th >= extent[1]
                        || rows * th < extent[1]
                    {
                        return Err("AVIF grid tiles do not cover its extent".into());
                    }
                    let (x, y) = ((i as u32 % columns) * tw, (i as u32 / columns) * th);
                    let width = tw.min(extent[0] - x) as usize;
                    for yy in 0..th.min(extent[1] - y) {
                        codec::check(cancel)?;
                        let at = (y + yy) as usize * extent[0] as usize + x as usize;
                        let source = yy as usize * tw as usize;
                        output[at..at + width]
                            .copy_from_slice(&tile.pixels[source..source + width]);
                    }
                }
                let (_, depth, layout, color) = characteristics.ok_or("Empty AVIF grid")?;
                if p.bits.is_some_and(|v| v != depth) {
                    return Err("AVIF grid precision mismatch".into());
                }
                RawImage {
                    extent,
                    depth,
                    layout,
                    color,
                    premultiplied: false,
                    pixels: output,
                }
            }
            b"iden" => {
                let source = container.targets(id, b"dimg")?;
                if source.len() != 1 {
                    return Err("Invalid AVIF identity derivation".into());
                }
                let image = decode_item(
                    container, source[0], inherited, alpha, stack, budget, cancel,
                )?;
                if image.extent != extent {
                    return Err("AVIF identity extent mismatch".into());
                }
                image
            }
            b"tmap" => return Err("AVIF tone-map integration is not yet available".into()),
            _ => return Err("Unsupported AVIF image item type".into()),
        };
        if !alpha {
            if let Some(alpha_id) = alpha_item(container, id)? {
                let alpha_p = Properties::read(container, alpha_id)?;
                if alpha_p.geometry != p.geometry {
                    return Err("AVIF color and alpha transformations disagree".into());
                }
                let remaining = budget
                    .checked_sub(image.pixels.len() * 8)
                    .ok_or("AVIF alpha exceeds the codec budget")?;
                let alpha = decode_item(container, alpha_id, None, true, stack, remaining, cancel)?;
                if alpha.extent != image.extent || alpha.depth != image.depth {
                    return Err("AVIF color and alpha geometry or precision disagree".into());
                }
                image.premultiplied = container.targets(id, b"prem")?.contains(&alpha_id);
                for (i, (pixel, alpha)) in image.pixels.iter_mut().zip(&alpha.pixels).enumerate() {
                    if i % extent[0] as usize == 0 {
                        codec::check(cancel)?;
                    }
                    pixel[3] = alpha[0];
                }
            }
        }
        Ok(image)
    })();
    stack.pop();
    result
}

pub(super) fn read(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
    cancel: &AtomicBool,
) -> Result<DecodedPhoto, String> {
    read_rendition(input, limits, cancel, true)
}

fn read_rendition(
    input: impl BufRead + Seek,
    limits: DecodeLimits,
    cancel: &AtomicBool,
    reconstruct: bool,
) -> Result<DecodedPhoto, String> {
    codec::check(cancel)?;
    let mut input = super::raster_io::Input::new(input, limits)?;
    let length = usize::try_from(input.length).map_err(|_| "AVIF input is too large")?;
    let mut encoded = super::raster_io::allocate(length)?;
    for chunk in encoded.chunks_mut(64 * 1024) {
        codec::check(cancel)?;
        input.read_exact(chunk).map_err(err)?;
    }
    let container = Container::parse(&encoded, limits.codec_bytes - length, cancel)?;
    let sequence = sequence::Sequence::parse(&container, cancel)?;
    let primary = container.primary.unwrap_or(0);
    let descriptor = if sequence.is_none() {
        gainmap::Descriptor::find(
            &container,
            primary,
            limits.codec_bytes - length - container.metadata_bytes,
            cancel,
        )?
    } else {
        None
    };
    let id = descriptor.as_ref().map_or(primary, |v| v.base);
    let item_properties = if sequence.is_none() {
        Some(Properties::read(&container, id)?)
    } else {
        None
    };
    let properties = sequence
        .as_ref()
        .map(|v| v.properties())
        .or(item_properties.as_ref())
        .unwrap();
    let raw_extent = properties.extent.ok_or("Missing AVIF spatial extent")?;
    limits.extent(raw_extent)?;
    let extent = properties.geometry.extent(raw_extent)?;
    limits.extent(extent)?;
    let band = extent[0] as usize * 8 * (layer_core::raster::TILE_SIZE as usize + 1);
    let budget = limits
        .codec_bytes
        .checked_sub(length)
        .and_then(|n| n.checked_sub(container.metadata_bytes))
        .and_then(|n| n.checked_sub(band))
        .and_then(|n| n.checked_sub(properties.icc.map_or(0, |v| v.len())))
        .ok_or("AVIF exceeds the codec budget")?;
    if properties
        .color
        .is_some_and(|c| matches!(c.cicp[1], 16 | 18))
    {
        return Err("HDR HEIF/AVIF needs an explicit SDR conversion before import".into());
    }
    let image = if let Some(sequence) = &sequence {
        sequence.decode(budget, cancel)?
    } else {
        decode_item(&container, id, None, false, &mut Vec::new(), budget, cancel)?
    };
    let (profile, profile_assumed) = color::profile(image.color, properties.icc)?;
    let channels = match crate::profile_channels(&profile)? {
        ProfileChannels::Gray => SourceChannels::GrayAlpha,
        ProfileChannels::Rgb => SourceChannels::Rgba,
        _ => return Err("AVIF pixels disagree with the embedded profile".into()),
    };
    let gainmap = descriptor.filter(|_| reconstruct)
        .map(|v| {
            v.decode(
                &container,
                &image,
                properties,
                &profile,
                budget - image.pixels.len() * 8,
                cancel,
            )
        })
        .transpose()?;
    let interpretation = if gainmap.is_some() {
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F16,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        }
    } else {
        SourceInterpretation {
            channels,
            depth: if image.depth == 8 {
                SampleDepth::U8
            } else {
                SampleDepth::U16
            },
            profile,
            profile_assumed,
        }
    };
    check_channels(interpretation.channels, &interpretation.profile)?;
    let remaining = budget
        .saturating_sub(image.pixels.len() * 8)
        .saturating_sub(gainmap.as_ref().map_or(0, |v| v.bytes()));
    let mut resolution = if let Some(sequence) = &sequence {
        sequence.resolution(&container, remaining, cancel)?
    } else {
        None
    };
    for reference in &container.references {
        if sequence.is_some() {
            break;
        }
        if &reference.kind != b"cdsc"
            || !reference.to.contains(&id)
            || &container.item(reference.from)?.kind != b"Exif"
        {
            continue;
        }
        let payload = container.payload(reference.from, remaining.min(crate::MAX_ICC_BYTES))?;
        let mut r = Reader::new(&payload);
        let offset = r.u32()? as usize;
        r.take(offset)?;
        resolution = super::metadata::exif(r.data)?.resolution;
    }
    if properties.geometry.rotation % 2 != 0 {
        resolution = resolution.map(|v| v.swapped());
    }
    let mut builder = SourceBuilder::new(extent, interpretation.clone(), limits.source_bytes)?;
    let mut row = super::raster_io::allocate(extent[0] as usize * interpretation.pixel_bytes())?;
    let crop = properties.geometry.crop(image.extent)?;
    let max = (1u32 << image.depth) - 1;
    let converter = color::Converter::new(image.color, image.layout)?;
    for y in 0..extent[1] {
        codec::check(cancel)?;
        let mut at = 0;
        for x in 0..extent[0] {
            let [sx, sy] = properties.geometry.source_pixel(crop, x, y);
            if let Some(gainmap) = &gainmap {
                let pixel = gainmap.pixel(&image, &converter, sx, sy)?;
                let bits = layer_core::color::hdr::encode_pixel(pixel).map_err(str::to_string)?;
                for value in bits {
                    row[at..at + 2].copy_from_slice(&value.to_le_bytes());
                    at += 2;
                }
                continue;
            }
            let mut pixel = converter.pixel(&image, sx, sy);
            pixel[3] = image.pixels[sy as usize * image.extent[0] as usize + sx as usize][3];
            if image.premultiplied {
                let alpha = u32::from(pixel[3]);
                for c in &mut pixel[..3] {
                    *c = if alpha == 0 {
                        0
                    } else {
                        ((u32::from(*c) * max + alpha / 2) / alpha).min(max) as u16
                    };
                }
            }
            for (c, value) in pixel.into_iter().enumerate() {
                if channels == SourceChannels::GrayAlpha && matches!(c, 1 | 2) {
                    continue;
                }
                if u32::from(value) > max {
                    return Err("AVIF sample exceeds its declared precision".into());
                }
                if image.depth == 8 {
                    row[at] = value as u8;
                    at += 1;
                } else {
                    row[at..at + 2].copy_from_slice(
                        &(((u32::from(value) * 65535 + max / 2) / max) as u16).to_le_bytes(),
                    );
                    at += 2;
                }
            }
        }
        builder.push_row(&row)?;
    }
    let mut source = builder.finish()?;
    source.resolution = resolution;
    codec::check(cancel)?;
    Ok(DecodedPhoto {
        source,
        first_frame: sequence.is_some(),
        primary_image: sequence.is_none()
            && container
                .items
                .iter()
                .filter(|v| !v.hidden && matches!(&v.kind, b"av01" | b"grid" | b"iden"))
                .filter(|v| {
                    v.id == id
                        || !container.references.iter().any(|r| {
                            (&r.kind == b"dimg" && r.to.contains(&v.id))
                                || (&r.kind == b"auxl" && r.from == v.id)
                        })
                })
                .count()
                > 1,
    })
}

#[cfg(test)]
mod tests;
