use super::*;
use layer_core::raster::{TILE_SIZE, TileBlob};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn exif(bytes: &[u8]) -> Result<u16, String> {
    let bytes = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
    if bytes.len() < 8 {
        return Err("Incomplete EXIF header".into());
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err("Invalid EXIF byte order".into()),
    };
    let u16_at = |at: usize| -> Result<u16, String> {
        let b: [u8; 2] = bytes
            .get(at..at.checked_add(2).ok_or("EXIF offset overflow")?)
            .ok_or("Incomplete EXIF value")?
            .try_into()
            .unwrap();
        Ok(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        let b: [u8; 4] = bytes
            .get(at..at.checked_add(4).ok_or("EXIF offset overflow")?)
            .ok_or("Incomplete EXIF value")?
            .try_into()
            .unwrap();
        Ok(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    if u16_at(2)? != 42 {
        return Err("Unsupported EXIF TIFF header".into());
    }
    let ifd = u32_at(4)? as usize;
    if ifd == 0 {
        return Ok(1);
    }
    let count = usize::from(u16_at(ifd)?);
    for index in 0..count {
        let at = ifd
            .checked_add(2 + index * 12)
            .ok_or("EXIF directory overflow")?;
        if u16_at(at)? == 274 {
            if u16_at(at + 2)? != 3 || u32_at(at + 4)? != 1 {
                return Err("Invalid EXIF orientation field".into());
            }
            let value = u16_at(at + 8)?;
            return if (1..=8).contains(&value) {
                Ok(value)
            } else {
                Err("Invalid EXIF orientation".into())
            };
        }
    }
    Ok(1)
}

/// Normalize the source's sample positions losslessly. At most four input tiles
/// and one output tile are decoded; no rotated full-size image is allocated.
pub(super) fn normalize(
    source: SourceImage,
    orientation: u16,
    max_bytes: usize,
) -> Result<SourceImage, String> {
    if orientation == 1 {
        return Ok(source);
    }
    if !(1..=8).contains(&orientation) {
        return Err("Invalid source orientation".into());
    }
    let [w, h] = source.extent;
    let extent = if orientation >= 5 { [h, w] } else { [w, h] };
    let bpp = source.interpretation.pixel_bytes();
    let descriptor = source.interpretation.descriptor();
    let mut result = SourceImage {
        kind: source.kind,
        extent,
        interpretation: source.interpretation.clone(),
        tiles: BTreeMap::new(),
    };
    let mut retained = 0;
    let mut output = vec![0; TILE_SIZE as usize * TILE_SIZE as usize * bpp];
    for ty in 0..extent[1].div_ceil(TILE_SIZE) {
        for tx in 0..extent[0].div_ceil(TILE_SIZE) {
            let mut decoded = BTreeMap::new();
            output.fill(0);
            for ly in 0..TILE_SIZE.min(extent[1] - ty * TILE_SIZE) {
                for lx in 0..TILE_SIZE.min(extent[0] - tx * TILE_SIZE) {
                    let (x, y) = (tx * TILE_SIZE + lx, ty * TILE_SIZE + ly);
                    let [sx, sy] = match orientation {
                        2 => [w - 1 - x, y],
                        3 => [w - 1 - x, h - 1 - y],
                        4 => [x, h - 1 - y],
                        5 => [y, x],
                        6 => [y, h - 1 - x],
                        7 => [w - 1 - y, h - 1 - x],
                        8 => [w - 1 - y, x],
                        _ => unreachable!(),
                    };
                    let coordinate = [sx / TILE_SIZE, sy / TILE_SIZE];
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        decoded.entry(coordinate)
                    {
                        entry.insert(
                            source
                                .tiles
                                .get(&coordinate)
                                .ok_or("Missing oriented source tile")?
                                .decode()?,
                        );
                    }
                    let from = ((sy % TILE_SIZE) * TILE_SIZE + sx % TILE_SIZE) as usize * bpp;
                    let to = (ly * TILE_SIZE + lx) as usize * bpp;
                    output[to..to + bpp].copy_from_slice(&decoded[&coordinate][from..from + bpp]);
                }
            }
            let blob = TileBlob::encode_source(descriptor, &output)?;
            retained += blob.resident_bytes();
            if retained > max_bytes {
                return Err("Oriented source exceeds the memory budget".into());
            }
            result.tiles.insert([tx, ty], Arc::new(blob));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_eight_orientations_preserve_numbered_pixel_positions() {
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Gray,
            depth: IntegerDepth::U8,
            profile: ColorProfile::default(),
            profile_assumed: true,
        };
        let mut builder = SourceBuilder::new([3, 2], interpretation, 1024 * 1024).unwrap();
        builder.push_row(&[1, 2, 3]).unwrap();
        builder.push_row(&[4, 5, 6]).unwrap();
        let source = builder.finish().unwrap();
        let expected = [
            [1, 2, 3, 4, 5, 6],
            [3, 2, 1, 6, 5, 4],
            [6, 5, 4, 3, 2, 1],
            [4, 5, 6, 1, 2, 3],
            [1, 4, 2, 5, 3, 6],
            [4, 1, 5, 2, 6, 3],
            [6, 3, 5, 2, 4, 1],
            [3, 6, 2, 5, 1, 4],
        ];
        for orientation in 1..=8 {
            let result = normalize(source.clone(), orientation, 1024 * 1024).unwrap();
            let mut rows = result.rows();
            let mut values = Vec::new();
            let mut row = vec![0; result.row_bytes()];
            for y in 0..result.extent[1] {
                rows.read(y, &mut row).unwrap();
                values.extend_from_slice(&row);
            }
            assert_eq!(values, expected[orientation as usize - 1]);
        }
    }
}
