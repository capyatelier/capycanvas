//! Lossless tiled source samples. A decoded full photograph need not coexist
//! with its GPU paint, save snapshot and export. Integer16 bytes are little endian.
use super::{AlphaAssociation, ColorProfile, IntegerDepth, PixelDescriptor, TransferEncoding};
use crate::raster::{TILE_SIZE, TileBlob};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceChannels {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
    Cmyk,
}
impl SourceChannels {
    pub fn count(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::GrayAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba | Self::Cmyk => 4,
        }
    }
    pub fn has_alpha(self) -> bool {
        matches!(self, Self::GrayAlpha | Self::Rgba)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInterpretation {
    pub channels: SourceChannels,
    pub depth: IntegerDepth,
    pub profile: ColorProfile,
    pub profile_assumed: bool,
}
impl SourceInterpretation {
    pub fn descriptor(&self) -> PixelDescriptor {
        PixelDescriptor {
            channels: self.channels.count() as u8,
            bits_per_channel: self.depth.bits(),
            encoding: TransferEncoding::Profile,
            alpha: if self.channels.has_alpha() {
                AlphaAssociation::Straight
            } else {
                AlphaAssociation::None
            },
        }
    }
    pub fn pixel_bytes(&self) -> usize {
        self.channels.count() * self.depth.bytes()
    }
}

#[derive(Clone, Debug)]
pub struct SourceImage {
    pub extent: [u32; 2],
    pub interpretation: SourceInterpretation,
    pub tiles: BTreeMap<[u32; 2], Arc<TileBlob>>,
}
impl SourceImage {
    pub fn resident_bytes(&self) -> usize {
        self.tiles.values().map(|t| t.resident_bytes()).sum()
    }
    pub fn row_bytes(&self) -> usize {
        self.extent[0] as usize * self.interpretation.pixel_bytes()
    }
    pub fn rows(&self) -> SourceRows<'_> {
        SourceRows {
            source: self,
            band: u32::MAX,
            tiles: Vec::new(),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let [w, h] = self.extent;
        if w == 0 || h == 0 || w > 32768 || h > 32768 {
            return Err("Unsupported source dimensions".into());
        }
        let columns = w.div_ceil(TILE_SIZE);
        let rows = h.div_ceil(TILE_SIZE);
        if self.tiles.len() != columns as usize * rows as usize {
            return Err("Incomplete source tiles".into());
        }
        for (&[x, y], tile) in &self.tiles {
            if x >= columns || y >= rows || tile.descriptor != self.interpretation.descriptor() {
                return Err("Invalid source tile layout".into());
            }
        }
        Ok(())
    }
}

/// At most one 256-row band is decoded. Color conversions and encoders consume
/// sequential rows through this cache, without decoding each tile 256 times.
pub struct SourceRows<'a> {
    source: &'a SourceImage,
    band: u32,
    tiles: Vec<Vec<u8>>,
}
impl SourceRows<'_> {
    pub fn read(&mut self, y: u32, output: &mut [u8]) -> Result<(), String> {
        if y >= self.source.extent[1] || output.len() != self.source.row_bytes() {
            return Err("Invalid source row".into());
        }
        let band = y / TILE_SIZE;
        let bpp = self.source.interpretation.pixel_bytes();
        if self.band != band {
            self.band = u32::MAX;
            self.tiles.clear();
            for x in 0..self.source.extent[0].div_ceil(TILE_SIZE) {
                let tile = self
                    .source
                    .tiles
                    .get(&[x, band])
                    .ok_or("Missing source tile")?;
                if tile.descriptor != self.source.interpretation.descriptor() {
                    return Err("Invalid source tile format".into());
                }
                self.tiles.push(tile.decode()?);
            }
            self.band = band;
        }
        let tile_row = (y % TILE_SIZE) as usize * TILE_SIZE as usize * bpp;
        for (pixels, tile) in output.chunks_mut(TILE_SIZE as usize * bpp).zip(&self.tiles) {
            pixels.copy_from_slice(&tile[tile_row..tile_row + pixels.len()]);
        }
        Ok(())
    }
    pub fn decoded_bytes(&self) -> usize {
        self.tiles.iter().map(Vec::len).sum()
    }
}

/// Source decoding accumulates one row band and compresses it before continuing.
/// `max_bytes` limits final compressed ownership; packing is one 256-row band
/// plus one tile (64.5 MiB at the 32768×RGBA16 limit).
pub struct SourceBuilder {
    image: SourceImage,
    next_y: u32,
    band: Vec<u8>,
    retained_bytes: usize,
    max_bytes: usize,
}
impl SourceBuilder {
    pub fn new(
        extent: [u32; 2],
        interpretation: SourceInterpretation,
        max_bytes: usize,
    ) -> Result<Self, String> {
        if extent.contains(&0) || extent.iter().any(|v| *v > 32768) {
            return Err("Unsupported source dimensions".into());
        }
        let row = extent[0] as usize * interpretation.pixel_bytes();
        let mut band = Vec::new();
        band.try_reserve_exact(row * TILE_SIZE as usize)
            .map_err(|_| "Source row-band allocation failed")?;
        Ok(Self {
            image: SourceImage {
                extent,
                interpretation,
                tiles: BTreeMap::new(),
            },
            next_y: 0,
            band,
            retained_bytes: 0,
            max_bytes,
        })
    }
    pub fn push_row(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.next_y >= self.image.extent[1] || bytes.len() != self.image.row_bytes() {
            return Err("Invalid decoded source row".into());
        }
        self.band.extend_from_slice(bytes);
        self.next_y += 1;
        if self.next_y % TILE_SIZE == 0 || self.next_y == self.image.extent[1] {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), String> {
        let bpp = self.image.interpretation.pixel_bytes();
        let descriptor = self.image.interpretation.descriptor();
        let mut tile = vec![0; TILE_SIZE as usize * TILE_SIZE as usize * bpp];
        for x in 0..self.image.extent[0].div_ceil(TILE_SIZE) {
            tile.fill(0);
            let offset = x as usize * TILE_SIZE as usize * bpp;
            let width = (self.image.row_bytes() - offset).min(TILE_SIZE as usize * bpp);
            for (row, source) in self.band.chunks_exact(self.image.row_bytes()).enumerate() {
                let start = row * TILE_SIZE as usize * bpp;
                tile[start..start + width].copy_from_slice(&source[offset..offset + width]);
            }
            let blob = TileBlob::encode_source(descriptor, &tile)?;
            self.retained_bytes += blob.resident_bytes();
            if self.retained_bytes > self.max_bytes {
                return Err("Decoded source exceeds the memory budget".into());
            }
            self.image
                .tiles
                .insert([x, (self.next_y - 1) / TILE_SIZE], Arc::new(blob));
        }
        self.band.clear();
        Ok(())
    }
    pub fn finish(self) -> Result<SourceImage, String> {
        if self.next_y != self.image.extent[1] {
            return Err("Source decoding is incomplete".into());
        }
        self.image.validate()?;
        Ok(self.image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_bands_preserve_integer16_hidden_rgb_and_partial_tiles() {
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: ColorProfile::default(),
            profile_assumed: false,
        };
        let mut builder = SourceBuilder::new([513, 259], interpretation, 16 * 1024 * 1024).unwrap();
        let row = |y: u32| {
            (0..513)
                .flat_map(|x: u32| {
                    [x.wrapping_mul(617) as u16, y as u16, (x ^ y) as u16, 0]
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                })
                .collect::<Vec<_>>()
        };
        for y in 0..259 {
            builder.push_row(&row(y)).unwrap();
        }
        let image = builder.finish().unwrap();
        let clone = image.clone();
        assert!(Arc::ptr_eq(
            image.tiles.values().next().unwrap(),
            clone.tiles.values().next().unwrap()
        ));
        let mut rows = image.rows();
        let mut decoded = vec![0; image.row_bytes()];
        for y in (0..259).rev() {
            rows.read(y, &mut decoded).unwrap();
            assert_eq!(decoded, row(y));
            assert_eq!(rows.decoded_bytes(), 3 * 256 * 256 * 8);
        }
    }
}
