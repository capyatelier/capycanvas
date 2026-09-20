//! One LZ4 block per tile. The descriptor bounds output; TileBlob checks integrity.
use super::{MAX_COMPRESSED_TILE_BYTES, MAX_TILE_BYTES};

pub(super) fn compress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_TILE_BYTES {
        return Err("Oversized raster tile".into());
    }
    Ok(lz4_flex::block::compress(bytes))
}

pub(super) fn decompress(bytes: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    if expected > MAX_TILE_BYTES || bytes.len() > MAX_COMPRESSED_TILE_BYTES {
        return Err("Oversized raster tile".into());
    }
    let mut decoded = vec![0; expected];
    let count = lz4_flex::block::decompress_into(bytes, &mut decoded).map_err(|e| e.to_string())?;
    if count != expected {
        return Err("Invalid raster tile byte count".into());
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        color::{DocumentColor, RgbSpace, SampleDepth, f16},
        raster::TileBlob,
    };

    #[test]
    fn tile_blocks_preserve_integer_and_float_samples() {
        for depth in [
            SampleDepth::U8,
            SampleDepth::U16,
            SampleDepth::F16,
            SampleDepth::F32,
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
            let tile = TileBlob::encode(descriptor, &pixels).unwrap();
            assert_eq!(tile.decode().unwrap(), pixels);
            let restored =
                TileBlob::from_compressed(descriptor, tile.digest, tile.compressed().unwrap())
                    .unwrap();
            assert_eq!(restored.decode().unwrap(), pixels);
            assert!(
                tile.compressed_len() < pixels.len() / 2,
                "Periodic planes must compress: {depth:?}"
            );
        }
    }

    #[test]
    fn tile_blocks_bound_noise_ramps_and_malformed_input() {
        let mut seed = 29u32;
        let noise: Vec<u8> = (0..MAX_TILE_BYTES)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        // Incompressible LZ4 can exceed the former Zstd bound of raw + 1024.
        assert!(compress(&noise).unwrap().len() > noise.len() + 1024);
        for bytes in [
            vec![0; 262144],
            (0..262144).map(|i| i as u8).collect(),
            noise,
        ] {
            let encoded = compress(&bytes).unwrap();
            assert_eq!(decompress(&encoded, bytes.len()).unwrap(), bytes);
            for len in [0, 1, encoded.len() / 2, encoded.len() - 1] {
                assert!(decompress(&encoded[..len], bytes.len()).is_err());
            }
            assert!(decompress(&encoded, bytes.len() - 1).is_err());
            assert!(decompress(&encoded, MAX_TILE_BYTES + 1).is_err());
            let mut trailing = encoded.clone();
            trailing.push(0);
            assert!(decompress(&trailing, bytes.len()).is_err());
            let mut concatenated = encoded.clone();
            concatenated.extend(&encoded);
            assert!(decompress(&concatenated, bytes.len()).is_err());
        }
        assert!(compress(&vec![0; MAX_TILE_BYTES + 1]).is_err());
        assert!(decompress(&vec![0; MAX_COMPRESSED_TILE_BYTES + 1], 1).is_err());
        // Invalid zero-distance backreference.
        assert!(decompress(&[0x10, 42, 0, 0, 0], 5).is_err());
    }
}
