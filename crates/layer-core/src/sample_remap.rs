//! Exact sample permutations and crops: every result sample is one source
//! sample in its own interpretation, or empty outside the source. Work is
//! bounded to one result tile and the at most four source tiles it reads.
use crate::color::source::SourceImage;
use crate::raster::{RasterData, RasterTile, TILE_SIZE, TileBlob, TileKey};
use std::{collections::{BTreeMap, BTreeSet}, sync::{Arc, atomic::AtomicBool}};

/// Result sample `(x, y)` reads source sample `origin + x * x_axis + y * y_axis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GridMap {
    pub x_axis: [i64; 2],
    pub y_axis: [i64; 2],
    pub origin: [i64; 2],
}

impl GridMap {
    pub const IDENTITY: Self = Self { x_axis: [1, 0], y_axis: [0, 1], origin: [0, 0] };

    pub fn translation(origin: [i64; 2]) -> Self {
        Self { origin, ..Self::IDENTITY }
    }

    /// The EXIF orientation `code` of a source with `extent`, and the result extent.
    pub fn exif(code: u16, extent: [u32; 2]) -> Option<(Self, [u32; 2])> {
        let [w, h] = extent.map(|v| i64::from(v) - 1);
        let turned = [extent[1], extent[0]];
        let (x_axis, y_axis, origin, size) = match code {
            1 => ([1, 0], [0, 1], [0, 0], extent),
            2 => ([-1, 0], [0, 1], [w, 0], extent),
            3 => ([-1, 0], [0, -1], [w, h], extent),
            4 => ([1, 0], [0, -1], [0, h], extent),
            5 => ([0, 1], [1, 0], [0, 0], turned),
            6 => ([0, -1], [1, 0], [0, h], turned),
            7 => ([0, -1], [-1, 0], [w, h], turned),
            8 => ([0, 1], [-1, 0], [w, 0], turned),
            _ => return None,
        };
        Some((Self { x_axis, y_axis, origin }, size))
    }

    pub fn source(&self, [x, y]: [i64; 2]) -> [i64; 2] {
        std::array::from_fn(|i| self.origin[i] + x * self.x_axis[i] + y * self.y_axis[i])
    }

    /// The result sample that reads source sample `p`.
    pub fn result(&self, p: [i64; 2]) -> [i64; 2] {
        let d = [p[0] - self.origin[0], p[1] - self.origin[1]];
        [d[0] * self.x_axis[0] + d[1] * self.x_axis[1], d[0] * self.y_axis[0] + d[1] * self.y_axis[1]]
    }

    pub fn swaps_axes(&self) -> bool {
        self.x_axis[0] == 0
    }

    pub fn is_permutation(&self) -> bool {
        let [a, b] = self.x_axis;
        let [c, d] = self.y_axis;
        [a, b, c, d].iter().all(|v| v.abs() <= 1) && (a * d - b * c).abs() == 1 && a * c + b * d == 0
    }

    /// Whether every result tile reads exactly one source tile.
    pub fn tile_aligned(&self) -> bool {
        let size = i64::from(TILE_SIZE);
        let first = self.source([0, 0]);
        let last = self.source([size - 1, size - 1]);
        self.is_permutation() && (0..2).all(|i| first[i].min(last[i]).rem_euclid(size) == 0)
    }

    /// Source samples `[min, max)` as result samples `[min, max)`.
    pub fn result_rect(&self, min: [i64; 2], max: [i64; 2]) -> [[i64; 2]; 2] {
        let a = self.result(min);
        let b = self.result([max[0] - 1, max[1] - 1]);
        [std::array::from_fn(|i| a[i].min(b[i])), std::array::from_fn(|i| a[i].max(b[i]) + 1)]
    }

    fn moves_whole_tiles(&self) -> bool {
        self.x_axis == [1, 0] && self.y_axis == [0, 1] && self.origin.iter().all(|v| v.rem_euclid(i64::from(TILE_SIZE)) == 0)
    }
}

fn cancelled_error(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) { Err("The image operation was cancelled".into()) } else { Ok(()) }
}

fn tile_range(extent: u32) -> u32 {
    extent.div_ceil(TILE_SIZE)
}

struct Decoded<'a> {
    tiles: Vec<([u32; 2], Arc<Vec<u8>>)>,
    read: Box<dyn FnMut([u32; 2]) -> Result<Option<Vec<u8>>, String> + 'a>,
}
impl Decoded<'_> {
    fn get(&mut self, coordinate: [u32; 2]) -> Result<Option<Arc<Vec<u8>>>, String> {
        if let Some((_, bytes)) = self.tiles.iter().find(|(c, _)| *c == coordinate) {
            return Ok(Some(bytes.clone()));
        }
        let Some(bytes) = (self.read)(coordinate)? else { return Ok(None) };
        let bytes = Arc::new(bytes);
        self.tiles.push((coordinate, bytes.clone()));
        Ok(Some(bytes))
    }
}

/// Copy the samples of result tile `tile` whose results lie inside `keep` and
/// whose sources lie inside `source_extent`.
fn fill_tile(
    output: &mut [u8], bpp: usize, tile: [u32; 2], keep: [[i64; 2]; 2], source_extent: [i64; 2], map: GridMap, decoded: &mut Decoded<'_>,
) -> Result<bool, String> {
    let size = i64::from(TILE_SIZE);
    let base = [i64::from(tile[0]) * size, i64::from(tile[1]) * size];
    let mut wrote = false;
    for ly in 0..size {
        let y = base[1] + ly;
        if y < keep[0][1] || y >= keep[1][1] { continue; }
        for lx in 0..size {
            let x = base[0] + lx;
            if x < keep[0][0] || x >= keep[1][0] { continue; }
            let [sx, sy] = map.source([x, y]);
            if sx < 0 || sy < 0 || sx >= source_extent[0] || sy >= source_extent[1] { continue; }
            let coordinate = [(sx / size) as u32, (sy / size) as u32];
            let Some(bytes) = decoded.get(coordinate)? else { continue };
            let from = ((sy % size) * size + sx % size) as usize * bpp;
            let to = (ly * size + lx) as usize * bpp;
            output[to..to + bpp].copy_from_slice(&bytes[from..from + bpp]);
            wrote = true;
        }
    }
    Ok(wrote)
}

/// A new immutable image of `extent` whose samples are `source`'s through
/// `map`, in the source's own interpretation. Samples outside the source are
/// zero. Unchanged whole tiles are shared with the source.
pub fn remap_image(source: &SourceImage, extent: [u32; 2], map: GridMap, max_bytes: usize, cancelled: &AtomicBool) -> Result<SourceImage, String> {
    if !map.is_permutation() {
        return Err("Image samples can only be permuted exactly".into());
    }
    let descriptor = source.interpretation.descriptor();
    let bpp = source.interpretation.pixel_bytes();
    let mut result = SourceImage {
        extent,
        resolution: source.resolution.map(|r| if map.swaps_axes() { r.swapped() } else { r }),
        interpretation: source.interpretation.clone(),
        tiles: BTreeMap::new(),
    };
    let size = i64::from(TILE_SIZE);
    let source_extent = source.extent.map(i64::from);
    let mut retained = 0usize;
    let mut output = vec![0; TILE_SIZE as usize * TILE_SIZE as usize * bpp];
    for ty in 0..tile_range(extent[1]) {
        for tx in 0..tile_range(extent[0]) {
            cancelled_error(cancelled)?;
            let valid = [extent[0] - tx * TILE_SIZE, extent[1] - ty * TILE_SIZE].map(|v| v.min(TILE_SIZE));
            if map.moves_whole_tiles() {
                let s = [map.origin[0] / size + i64::from(tx), map.origin[1] / size + i64::from(ty)];
                let shared = (0..2).all(|i| s[i] >= 0 && s[i] * size < source_extent[i] && (source_extent[i] - s[i] * size).min(size) == i64::from(valid[i]));
                if let Some(tile) = shared.then(|| source.tiles.get(&[s[0] as u32, s[1] as u32])).flatten() {
                    retained = retained.saturating_add(tile.resident_bytes());
                    result.tiles.insert([tx, ty], tile.clone());
                    continue;
                }
            }
            output.fill(0);
            let mut decoded = Decoded {
                tiles: Vec::with_capacity(4),
                read: Box::new(|coordinate| source.tiles.get(&coordinate).map(|tile| tile.decode()).transpose()),
            };
            let keep = [[0; 2], extent.map(i64::from)];
            fill_tile(&mut output, bpp, [tx, ty], keep, source_extent, map, &mut decoded)?;
            let blob = TileBlob::encode(descriptor, &output)?;
            retained = retained.saturating_add(blob.resident_bytes());
            if retained > max_bytes {
                return Err("The image exceeds the memory budget".into());
            }
            result.tiles.insert([tx, ty], Arc::new(blob));
        }
    }
    result.validate()?;
    Ok(result)
}

/// Paint or coverage tiles moved through a tile-aligned `map` into a target of
/// `extent`, keeping only results inside `keep`. Tiles no source reaches stay absent.
pub fn remap_raster(data: &RasterData, map: GridMap, extent: [u32; 2], keep: [[i64; 2]; 2], cancelled: &AtomicBool) -> Result<RasterData, String> {
    if !map.tile_aligned() {
        return Err("Layer pixels can only be permuted by whole tiles".into());
    }
    let size = i64::from(TILE_SIZE);
    let keep = [std::array::from_fn(|i| keep[0][i].max(0)), std::array::from_fn(|i| keep[1][i].min(i64::from(extent[i])))];
    let mut wanted = BTreeSet::new();
    for key in data.tiles.keys() {
        let min = key.coordinate.map(|v| i64::from(v) * size);
        let [low, high] = map.result_rect(min, [min[0] + size, min[1] + size]);
        let low = [low[0].max(keep[0][0]), low[1].max(keep[0][1])];
        let high = [high[0].min(keep[1][0]), high[1].min(keep[1][1])];
        if low[0] >= high[0] || low[1] >= high[1] { continue; }
        wanted.insert((key.plane, [(low[0] / size) as u32, (low[1] / size) as u32]));
    }
    let mut tiles = BTreeMap::new();
    for (plane, coordinate) in wanted {
        cancelled_error(cancelled)?;
        let base = coordinate.map(|v| i64::from(v) * size);
        let corner = [map.source(base), map.source([base[0] + size - 1, base[1] + size - 1])];
        let key = TileKey { plane, coordinate: std::array::from_fn(|i| (corner[0][i].min(corner[1][i]) / size) as u32) };
        let Some(tile) = data.tiles.get(&key) else { continue };
        let inside = (0..2).all(|i| keep[0][i] <= base[i] && base[i] + size <= keep[1][i]);
        if inside && map.moves_whole_tiles() {
            tiles.insert(TileKey { plane, coordinate }, tile.clone());
            continue;
        }
        let descriptor = tile.descriptor();
        let bpp = descriptor.bytes_per_pixel().ok_or("Unsupported raster pixels")?;
        let blob = tile.wait_backing_cancellable(cancelled)?;
        let bytes = blob.decode()?;
        let mut output = vec![0; TILE_SIZE as usize * TILE_SIZE as usize * bpp];
        let mut decoded = Decoded { tiles: vec![(key.coordinate, Arc::new(bytes))], read: Box::new(|_| Ok(None)) };
        fill_tile(&mut output, bpp, coordinate, keep, [i64::MAX; 2], map, &mut decoded)?;
        tiles.insert(TileKey { plane, coordinate }, RasterTile::backed(TileBlob::encode(descriptor, &output)?));
    }
    Ok(RasterData { tiles, watercolor: data.watercolor })
}

#[cfg(test)]
#[path = "sample_remap_tests.rs"]
mod tests;
