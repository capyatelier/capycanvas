//! Pixel-tight content bounds, found on the CPU. Each side of a raster target
//! decodes only its outermost column or row of tiles, and moves inward only
//! past tiles that are wholly transparent. Results are cached by tile content,
//! so asking again over unchanged pixels decodes nothing. Bounds within the
//! canvas decode the tiles its edges cross, too.
use crate::color::{AlphaAssociation, SampleType};
use crate::raster::{RasterPlane, RasterRevision, TILE_SIZE, TileBlob, TileKey};
use crate::*;
use std::collections::HashMap;
use std::sync::Mutex;

/// Float noise in a placement must not add a pixel to a window.
const TOLERANCE: f32 = 1e-3;

/// Which pixels count toward the bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentScope {
    /// What the canvas shows: visible layers within the canvas, limited by
    /// their masks.
    Canvas,
    /// Visible layers limited by their masks, including pixels beyond the
    /// canvas.
    Visible,
    /// Every layer's pixels, including hidden layers and masked pixels.
    All,
}

/// How much one scan may do before it returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanBudget {
    /// A worker thread: waits for pending pixels and reads parked tiles.
    Worker,
    /// The UI thread: decodes at most this many tiles and never waits.
    Tiles(usize),
}

/// A tile's content and the tile-local rectangle scanned in it.
type TileScan = ([u8; 32], [u16; 4]);

/// Covered pixels of decoded tiles, keyed by tile content and the part of
/// the tile scanned.
#[derive(Default)]
pub struct ContentBoundsCache {
    tiles: Mutex<HashMap<TileScan, Option<[u16; 4]>>>,
}
impl ContentBoundsCache {
    const CAPACITY: usize = 16384;
    fn get(&self, key: &TileScan) -> Option<Option<[u16; 4]>> {
        self.tiles.lock().ok()?.get(key).copied()
    }
    fn insert(&self, key: TileScan, covered: Option<[u16; 4]>) {
        if let Ok(mut tiles) = self.tiles.lock() {
            if tiles.len() >= Self::CAPACITY {
                tiles.clear();
            }
            tiles.insert(key, covered);
        }
    }
}

/// Pixels of one target: its covered raster pixels and a local rectangle
/// known to be covered, within a local window, placed in the document.
struct Source {
    transform: Affine,
    raster: Option<(RasterRevision, RasterPlane)>,
    known: Rect,
    window: Rect,
}

/// A layer's content, limited by the masks of the layer and its groups.
struct Part {
    content: Source,
    clips: Vec<Source>,
}

/// The rasters a document's content bounds depend on, captured without
/// copying pixels, so a worker can scan them while the document moves on.
pub struct ContentBoundsRequest {
    parts: Vec<Part>,
    canvas: Option<Rect>,
}

fn local_rect(extent: [u32; 2]) -> Rect {
    Rect { min: Point::default(), max: Point { x: extent[0] as f32, y: extent[1] as f32 } }
}

impl ContentBoundsRequest {
    pub fn new(document: &Document, scope: ContentScope) -> Self {
        let layers = &document.layers;
        let bounds = local_rect([document.width, document.height]);
        let canvas = (scope == ContentScope::Canvas).then_some(bounds);
        let source = |transform: Affine, raster, known| Source {
            transform,
            raster,
            known,
            window: canvas.and_then(|c| Some(transform.inverse()?.bounds(c))).unwrap_or(Rect::UNBOUNDED),
        };
        let visible = scope != ContentScope::All;
        let clip = |owner: &Layer| {
            let bounded = |m: &&LayerMask| {
                m.enabled && !m.inverted && m.default_coverage <= 0. && m.initial.as_ref().is_none_or(|s| !s.inverted)
            };
            owner.mask.as_ref().filter(|m| visible && bounded(m)).map(|mask| {
                let known = mask.initial.as_ref().map_or(Rect::EMPTY, Selection::coverage_bounds);
                source(target_transform(layers, mask.id), Some((mask.raster.clone(), RasterPlane::Mask)), known)
            })
        };
        let mut parts = Vec::new();
        for layer in layers {
            if visible && (!document.layer_is_visible(layer.id) || layer.opacity <= 0.) {
                continue;
            }
            let content = match layer.kind {
                LayerKind::Paint => source(
                    target_transform(layers, layer.id),
                    Some((layer.raster.clone(), RasterPlane::Color)),
                    layer.source.as_ref().map_or(Rect::EMPTY, |source| local_rect(source.extent)),
                ),
                LayerKind::Background => source(Affine::IDENTITY, None, bounds),
                LayerKind::Effect if layer.effect.as_ref().is_some_and(|e| e.program.kind == EffectKind::Generator) => {
                    source(Affine::IDENTITY, None, bounds)
                }
                LayerKind::Effect | LayerKind::Group | LayerKind::Selection => continue,
            };
            let mut clips: Vec<_> = clip(layer).into_iter().collect();
            let mut parent = layer.properties.parent;
            while let Some(group) = parent.and_then(|id| document.layer(id)) {
                clips.extend(clip(group));
                parent = group.properties.parent;
            }
            parts.push(Part { content, clips });
        }
        Self { parts, canvas }
    }

    /// The document bounds of the content, or None when `budget` ran out or
    /// pixels are still loading; asking again resumes from the cache. Empty
    /// when there is nothing.
    pub fn scan(&self, cache: &ContentBoundsCache, budget: ScanBudget) -> Result<Option<Rect>, String> {
        let mut scan = Scan { cache, budget, decoded: 0 };
        let mut bounds = Rect::EMPTY;
        for part in &self.parts {
            let Some(mut area) = scan.bounds(&part.content)? else { return Ok(None) };
            for clip in &part.clips {
                if area.is_empty() {
                    break;
                }
                let Some(clip) = scan.bounds(clip)? else { return Ok(None) };
                area = area.intersect(clip);
            }
            bounds = bounds.union(area);
        }
        Ok(Some(self.canvas.map_or(bounds, |canvas| bounds.intersect(canvas))))
    }
}

struct Scan<'a> {
    cache: &'a ContentBoundsCache,
    budget: ScanBudget,
    decoded: usize,
}
impl Scan<'_> {
    fn bounds(&mut self, source: &Source) -> Result<Option<Rect>, String> {
        let covered = match &source.raster {
            Some((raster, plane)) => match self.raster(raster, *plane, source.window)? {
                Some(covered) => covered,
                None => return Ok(None),
            },
            None => Rect::EMPTY,
        };
        let known = source.known.intersect(source.window);
        Ok(Some(source.transform.bounds(known.union(covered))))
    }

    /// Local bounds of a target's covered pixels within `window`.
    fn raster(&mut self, raster: &RasterRevision, plane: RasterPlane, window: Rect) -> Result<Option<Rect>, String> {
        let data = match (raster.try_data(), self.budget) {
            (Some(data), _) => data?,
            (None, ScanBudget::Worker) => raster.wait_data()?,
            (None, ScanBudget::Tiles(_)) => return Ok(None),
        };
        let pixel = |v: f32, round: fn(f32) -> f32| round(v).clamp(0., u32::MAX as f32) as u32;
        let low = [window.min.x, window.min.y].map(|v| pixel(v + TOLERANCE, f32::floor));
        let high = [window.max.x, window.max.y].map(|v| pixel(v - TOLERANCE, f32::ceil));
        let part = |c: [u32; 2]| -> [u16; 4] {
            let origin = c.map(|v| v * TILE_SIZE);
            let from = |axis: usize| low[axis].saturating_sub(origin[axis]).min(TILE_SIZE) as u16;
            let to = |axis: usize| high[axis].saturating_sub(origin[axis]).min(TILE_SIZE) as u16;
            [from(0), from(1), to(0), to(1)]
        };
        let mut columns = std::collections::BTreeMap::<u32, Vec<[u32; 2]>>::new();
        let mut rows = std::collections::BTreeMap::<u32, Vec<[u32; 2]>>::new();
        for key in data.tiles.keys().filter(|k| k.plane == plane) {
            let [x0, y0, x1, y1] = part(key.coordinate);
            if x0 < x1 && y0 < y1 {
                columns.entry(key.coordinate[0]).or_default().push(key.coordinate);
                rows.entry(key.coordinate[1]).or_default().push(key.coordinate);
            }
        }
        let mut tile = |c: [u32; 2]| {
            let tile = &data.tiles[&TileKey { plane, coordinate: c }];
            self.tile(tile, part(c))
        };
        let origin = |v: u32| v * TILE_SIZE;
        let sides = [
            edge(columns.values(), |c, b| origin(c[0]) + u32::from(b[0]), u32::min, &mut tile)?,
            edge(rows.values(), |c, b| origin(c[1]) + u32::from(b[1]), u32::min, &mut tile)?,
            edge(columns.values().rev(), |c, b| origin(c[0]) + u32::from(b[2]), u32::max, &mut tile)?,
            edge(rows.values().rev(), |c, b| origin(c[1]) + u32::from(b[3]), u32::max, &mut tile)?,
        ];
        let [Some(x0), Some(y0), Some(x1), Some(y1)] = sides else { return Ok(None) };
        let (Some(x0), Some(y0), Some(x1), Some(y1)) = (x0, y0, x1, y1) else { return Ok(Some(Rect::EMPTY)) };
        Ok(Some(Rect {
            min: Point { x: x0 as f32, y: y0 as f32 },
            max: Point { x: x1 as f32, y: y1 as f32 },
        }))
    }

    /// A tile's covered pixels within the tile-local `part`, or None when
    /// the tile can't be read within the budget.
    fn tile(&mut self, tile: &crate::raster::RasterTile, part: [u16; 4]) -> Result<Option<Option<[u16; 4]>>, String> {
        let worker = self.budget == ScanBudget::Worker;
        let blob = match tile.try_backing() {
            Some(blob) => blob?,
            None if worker => tile.wait_backing()?,
            None => return Ok(None),
        };
        let key = (blob.digest, part);
        if let Some(covered) = self.cache.get(&key) {
            return Ok(Some(covered));
        }
        if let ScanBudget::Tiles(limit) = self.budget
            && (self.decoded >= limit || !blob.compressed_ready()? || blob.resident_bytes() == 0)
        {
            return Ok(None);
        }
        self.decoded += 1;
        let covered = covered_pixels(&blob, part)?;
        self.cache.insert(key, covered);
        Ok(Some(covered))
    }
}

/// The extreme value along one side: lines of tiles in scan order, stopping
/// at the first line with a covered pixel. None when a tile can't be read;
/// Some(None) when every tile is transparent.
fn edge<'a>(
    lines: impl Iterator<Item = &'a Vec<[u32; 2]>>,
    value: impl Fn([u32; 2], [u16; 4]) -> u32,
    pick: fn(u32, u32) -> u32,
    tile: &mut impl FnMut([u32; 2]) -> Result<Option<Option<[u16; 4]>>, String>,
) -> Result<Option<Option<u32>>, String> {
    for line in lines {
        let mut found = None;
        for &c in line {
            match tile(c)? {
                None => return Ok(None),
                Some(Some(covered)) => {
                    let v = value(c, covered);
                    found = Some(found.map_or(v, |f| pick(f, v)));
                }
                Some(None) => {}
            }
        }
        if found.is_some() {
            return Ok(Some(found));
        }
    }
    Ok(Some(None))
}

/// The tile-local rectangle of pixels within `part` with nonzero alpha, or
/// nonzero coverage on a mask.
fn covered_pixels(blob: &TileBlob, [x0, y0, x1, y1]: [u16; 4]) -> Result<Option<[u16; 4]>, String> {
    let descriptor = blob.descriptor;
    let bytes = blob.decode()?;
    let stride = descriptor.bytes_per_pixel().ok_or("Unsupported raster pixels")?;
    let size = usize::from(descriptor.bits_per_channel / 8);
    let opaque = descriptor.alpha == AlphaAssociation::None && descriptor.channels > 1;
    let offset = (usize::from(descriptor.channels) - 1) * size;
    let covered = |pixel: &[u8]| {
        let sample = &pixel[offset..offset + size];
        match (descriptor.sample, size) {
            (SampleType::Float, 4) => f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) > 0.,
            (SampleType::Float, _) => {
                let bits = u16::from_le_bytes([sample[0], sample[1]]);
                bits & 0x8000 == 0 && bits & 0x7fff != 0
            }
            _ => sample.iter().any(|&b| b != 0),
        }
    };
    let side = TILE_SIZE as usize;
    let [x0, y0, x1, y1] = [x0, y0, x1, y1].map(usize::from);
    let mut bounds = [u16::MAX, u16::MAX, 0, 0];
    for (y, row) in bytes.chunks_exact(side * stride).enumerate().take(y1).skip(y0) {
        let span = &row[x0 * stride..x1 * stride];
        let Some(first) = span.chunks_exact(stride).position(|p| opaque || covered(p)) else { continue };
        let last = x1 - 1 - span.chunks_exact(stride).rev().position(|p| opaque || covered(p)).unwrap_or(0);
        bounds[0] = bounds[0].min((x0 + first) as u16);
        bounds[1] = bounds[1].min(y as u16);
        bounds[2] = bounds[2].max(last as u16 + 1);
        bounds[3] = y as u16 + 1;
    }
    Ok((bounds[0] != u16::MAX).then_some(bounds))
}

#[cfg(test)]
#[path = "content_bounds_tests.rs"]
mod tests;
