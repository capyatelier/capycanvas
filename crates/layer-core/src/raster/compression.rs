//! Standard Zstd frames with bounded output and exactly one frame per tile.
//! The archive's shuffle and SHA-256 identity are independent of this codec.
use zrip_core::frame::{MAX_BLOCK_SIZE, header::parse_frame_header};

pub(super) fn compress(bytes: &[u8], interactive: bool) -> Result<Vec<u8>, String> {
    if bytes.len() > super::MAX_TILE_BYTES {
        return Err("Oversized raster tile".into());
    }
    if interactive {
        // Keep capture free of entropy coding while retaining the shorter
        // search stride needed by periodic multibyte raster samples.
        let mut params = zrip_encode::strategy::level_params_for_size(-4, bytes.len()).unwrap();
        params.force_raw_literals = true;
        zrip_encode::compress_with_params(bytes, &params).map_err(|e| e.to_string())
    } else {
        zrip_encode::compress(bytes, 1).map_err(|e| e.to_string())
    }
}

pub(super) fn decompress(bytes: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    if expected > super::MAX_TILE_BYTES {
        return Err("Oversized raster tile".into());
    }
    validate_frame(bytes, expected)?;
    let mut decoded =
        zrip_decode::decompress_with_limit(bytes, expected).map_err(|e| e.to_string())?;
    if decoded.len() != expected {
        return Err("Invalid raster tile byte count".into());
    }
    // The decoder reserves block scratch in this Vec. The shared tile cache
    // accounts capacity, so release that spare space before retaining samples.
    decoded.shrink_to_fit();
    Ok(decoded)
}

fn validate_frame(bytes: &[u8], expected: usize) -> Result<(), String> {
    let invalid = "Invalid compressed raster frame";
    let header = parse_frame_header(bytes).map_err(|_| invalid)?;
    if header.dict_id.is_some()
        || header
            .frame_content_size
            .is_some_and(|n| n != expected as u64)
    {
        return Err(invalid.into());
    }
    let mut at = header.header_size;
    loop {
        let block = bytes
            .get(at..at.checked_add(3).ok_or(invalid)?)
            .ok_or(invalid)?;
        let bits = u32::from_le_bytes([block[0], block[1], block[2], 0]);
        let size = (bits >> 3) as usize;
        if size > MAX_BLOCK_SIZE {
            return Err(invalid.into());
        }
        let stored = match (bits >> 1) & 3 {
            0 | 2 => size,
            1 => 1,
            _ => return Err(invalid.into()),
        };
        at = at
            .checked_add(3 + stored)
            .filter(|&end| end <= bytes.len())
            .ok_or(invalid)?;
        if bits & 1 != 0 {
            break;
        }
    }
    if header.content_checksum {
        at = at.checked_add(4).ok_or(invalid)?;
    }
    if at > bytes.len() {
        return Err(invalid.into());
    }
    if at != bytes.len() {
        return Err("Trailing compressed raster data".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_zstd_fixtures_keep_samples_and_tile_identities() {
        use crate::color::{DocumentColor, RgbSpace, SampleDepth, f16};
        use crate::raster::TileBlob;
        for (depth, encoded) in [
            (
                SampleDepth::U8,
                include_bytes!("../../tests/fixtures/native-zstd/u8.zst").as_slice(),
            ),
            (
                SampleDepth::U16,
                include_bytes!("../../tests/fixtures/native-zstd/u16.zst").as_slice(),
            ),
            (
                SampleDepth::F16,
                include_bytes!("../../tests/fixtures/native-zstd/f16.zst").as_slice(),
            ),
            (
                SampleDepth::F32,
                include_bytes!("../../tests/fixtures/native-zstd/f32.zst").as_slice(),
            ),
        ] {
            let descriptor = DocumentColor {
                space: RgbSpace::Srgb,
                depth,
            }
            .paint_descriptor();
            let pixels: Vec<u8> = (0..65536u32)
                .flat_map(|i| {
                    (0..4).flat_map(move |c| {
                        let value = if c == 3 {
                            1.
                        } else {
                            (i.wrapping_mul(c * 73 + 1) % 4096) as f32 / 1024.
                        };
                        match depth {
                            SampleDepth::U8 => vec![(value * 63.) as u8],
                            SampleDepth::U16 => ((value * 16383.) as u16).to_le_bytes().to_vec(),
                            SampleDepth::F16 => f16::from_f32(value).to_le_bytes().to_vec(),
                            SampleDepth::F32 => value.to_le_bytes().to_vec(),
                        }
                    })
                })
                .collect();
            let old = TileBlob::from_compressed(
                descriptor,
                TileBlob::digest(descriptor, &pixels),
                encoded.into(),
            )
            .unwrap();
            assert_eq!(old.decode().unwrap(), pixels);
            for new in [
                TileBlob::encode(descriptor, &pixels),
                TileBlob::encode_source(descriptor, &pixels),
            ] {
                let new = new.unwrap();
                assert_eq!(new.digest, old.digest);
                assert_eq!(new.decode().unwrap(), pixels);
                assert!(
                    new.compressed_len() < old.compressed_len() * 5,
                    "Periodic tiles must retain useful compression"
                );
            }
        }
    }

    #[test]
    fn tile_compression_preserves_ramps_noise_and_multibyte_planes() {
        let mut seed = 29u32;
        let planes: Vec<u8> = (0..super::super::MAX_TILE_BYTES)
            .map(|i| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                match i / 65536 % 4 {
                    0 | 1 => seed as u8,
                    2 => i as u8,
                    _ => 63,
                }
            })
            .collect();
        for bytes in [
            vec![0; 262144],
            (0..262144).map(|i| i as u8).collect(),
            planes,
        ] {
            for interactive in [true, false] {
                let encoded = compress(&bytes, interactive).unwrap();
                assert_eq!(decompress(&encoded, bytes.len()).unwrap(), bytes);
                assert!(
                    encoded.len() < bytes.len() * 3 / 4,
                    "Periodic and shuffled float planes must compress"
                );
                for len in [0, 1, 4, encoded.len() / 2, encoded.len() - 1] {
                    assert!(decompress(&encoded[..len], bytes.len()).is_err());
                }
                let mut trailing = encoded.clone();
                trailing.push(0);
                assert!(
                    decompress(&trailing, bytes.len())
                        .unwrap_err()
                        .contains("Trailing")
                );
                let mut concatenated = encoded.clone();
                concatenated.extend(compress(&[], true).unwrap());
                assert!(
                    decompress(&concatenated, bytes.len())
                        .unwrap_err()
                        .contains("Trailing")
                );
                assert!(decompress(&encoded, bytes.len() - 1).is_err());
                let mut corrupt = encoded;
                *corrupt.last_mut().unwrap() ^= 1;
                assert!(decompress(&corrupt, bytes.len()).is_err());
            }
        }
    }
}
