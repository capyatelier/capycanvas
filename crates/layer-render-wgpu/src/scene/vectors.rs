use super::*;
use crate::vector::{Batch, Layer, Pool, tiles::{self, Frame, LevelGeometry, TILE}};
use layer_core::authored::{Affine64, VectorHandle, vector::{ItemId, PaintEntry, PathItem, Record, VectorObject}};
use std::collections::{BTreeSet, HashMap, HashSet};

const CELL: f64 = 256.;
const POOL_BYTES: u64 = 128 << 20;
const DERIVED_BYTES: usize = 256 << 20;
const MOTION_PIXELS: u64 = 4 << 20;
const DERIVE_TIME: std::time::Duration = std::time::Duration::from_millis(6);
type Colors = Arc<[[f32; 4]]>;
type PreviewGeometry = ((usize, u32, [u64; 2]), u32, Arc<LevelGeometry>, Colors);
type ReplacementKey = (ItemId, u32);
/// A replacement path's derivation at a level and its colors, with the
/// geometry, style and offset they were made from.
struct Replacement { geometry: Arc<LevelGeometry>, base: Option<u32>, colors: Vec<[f32; 4]>, used: u64, path: PathItem, offset: [u64; 2] }
impl Replacement {
    fn derives(&self, path: &PathItem, offset: [u64; 2]) -> bool {
        Arc::ptr_eq(&self.path.geometry, &path.geometry) && self.path.fill_rule == path.fill_rule && self.offset == offset
            && (Arc::ptr_eq(&self.path.style, &path.style) || same_outline(&self.path.style, &path.style))
    }
}

struct Derived { level: u32, offset: [i64; 2], geometry: Arc<LevelGeometry>, base: Option<u32>, used: u64 }
struct Item { path: PathItem, chunk: usize, index: u32, bounds: [f64; 4], cells: [i32; 4], colors: Colors, derived: Vec<Derived>, seen: u64 }
impl Item {
    fn derived(&self, level: u32, offset: [i64; 2]) -> Option<usize> { self.derived.iter().position(|d| d.level == level && d.offset == offset) }
    fn derived_at(&mut self, level: u32, offset: [i64; 2], frame: &Frame, bytes: &mut usize, paths: &mut u64) -> usize {
        if let Some(at) = self.derived(level, offset) { return at; }
        let geometry = Arc::new(tiles::derive(&self.path, frame));
        *bytes += geometry.bytes();
        *paths += 1;
        self.derived.push(Derived { level, offset, geometry, base: None, used: 0 });
        self.derived.len() - 1
    }
}

#[derive(Default)]
struct LayerCache {
    root: VectorObject,
    items: Vec<Option<Item>>,
    ids: HashMap<ItemId, u32>,
    free: Vec<u32>,
    grid: HashMap<[i32; 2], Vec<u32>>,
    chunks: HashMap<usize, u32>,
    palette: Option<(layer_core::color::RgbSpace, layer_core::BlendSpace)>,
    pending: Change,
    change: Change,
    derived_bytes: usize,
    derived_paths: u64,
}
#[derive(Default)]
struct Change { damage: Vec<[f64; 4]>, appended: Vec<u32>, touched: Vec<u32>, removed: Vec<Item>, mixed: bool, full: bool }
impl Change {
    fn appended(&self) -> Option<&[u32]> { (!self.full && !self.mixed && !self.appended.is_empty()).then_some(&self.appended) }
    fn damage(&self) -> Option<&[[f64; 4]]> { (!self.full && !self.damage.is_empty()).then_some(&self.damage) }
}

fn chunk_key(chunk: &layer_core::authored::Resource<layer_core::authored::vector::VectorChunk>) -> usize { Arc::as_ptr(chunk.storage()) as usize }
fn cells(bounds: [f64; 4]) -> [i32; 4] {
    [(bounds[0] / CELL).floor() as i32, (bounds[1] / CELL).floor() as i32, (bounds[2] / CELL).floor() as i32 + 1, (bounds[3] / CELL).floor() as i32 + 1]
}
fn same(a: &PathItem, b: &PathItem) -> bool {
    Arc::ptr_eq(&a.geometry, &b.geometry) && Arc::ptr_eq(&a.style, &b.style) && a.fill_rule == b.fill_rule && a.hidden == b.hidden
}
fn colors(path: &PathItem, palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> impl Iterator<Item = [f32; 4]> + '_ {
    path.style.entries.iter().map(move |entry| {
        let paint = match entry { PaintEntry::Fill(f) => f.paint.rgb(), PaintEntry::Stroke(s) => s.paint.rgb(), PaintEntry::Opaque(_) => None };
        paint.and_then(|c| c.linear_in(palette.0).ok()).map_or([0.; 4], |[r, g, b, a]| palette.1.composite(palette.0, [r * a, g * a, b * a, a]))
    })
}
/// Whether two styles derive the same coverage, differing at most in paint.
fn same_outline(a: &layer_core::authored::vector::Style, b: &layer_core::authored::vector::Style) -> bool {
    a.entries.len() == b.entries.len() && a.entries.iter().zip(&b.entries).all(|pair| match pair {
        (PaintEntry::Fill(_), PaintEntry::Fill(_)) | (PaintEntry::Opaque(_), PaintEntry::Opaque(_)) => true,
        (PaintEntry::Stroke(a), PaintEntry::Stroke(b)) => a.width == b.width && a.cap == b.cap && a.join == b.join && a.miter_limit == b.miter_limit && a.profile == b.profile,
        _ => false,
    })
}

impl LayerCache {
    fn insert(&mut self, path: PathItem, chunk: usize, index: u32, palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> [f64; 4] {
        let bounds = path.bounds();
        let item = Item { colors: colors(&path, palette).collect(), cells: cells(bounds), bounds, chunk, index, derived: Vec::new(), seen: 0, path };
        let slot = self.free.pop().unwrap_or_else(|| { self.items.push(None); self.items.len() as u32 - 1 });
        let [x0, y0, x1, y1] = item.cells;
        for y in y0..y1 { for x in x0..x1 { self.grid.entry([x, y]).or_default().push(slot); } }
        self.ids.insert(item.path.id, slot);
        self.items[slot as usize] = Some(item);
        bounds
    }
    fn remove(&mut self, slot: u32, pool: &mut Pool) -> Item {
        let item = self.items[slot as usize].take().unwrap();
        let [x0, y0, x1, y1] = item.cells;
        for y in y0..y1 { for x in x0..x1 {
            if let Some(list) = self.grid.get_mut(&[x, y]) {
                if let Some(i) = list.iter().position(|s| *s == slot) { list.swap_remove(i); }
                if list.is_empty() { self.grid.remove(&[x, y]); }
            }
        }}
        for derived in &item.derived {
            if let Some(base) = derived.base { pool.retire(base, &derived.geometry); }
            self.derived_bytes = self.derived_bytes.saturating_sub(derived.geometry.bytes());
        }
        self.ids.remove(&item.path.id);
        self.free.push(slot);
        item
    }
    fn update(&mut self, object: &VectorObject, palette: (layer_core::color::RgbSpace, layer_core::BlendSpace), pool: &mut Pool) {
        if self.root.same_root(object) && self.palette == Some(palette) { return; }
        let first = self.root.chunks().is_empty() && self.items.iter().all(Option::is_none);
        let mut added = Vec::new();
        let mut other = !first && self.palette.is_some() && self.palette != Some(palette);
        let mut damage = Vec::new();
        if self.palette != Some(palette) {
            self.palette = Some(palette);
            for item in self.items.iter_mut().flatten() { item.colors = colors(&item.path, palette).collect(); damage.push(item.bounds); }
        }
        let previous_root = std::mem::take(&mut self.root);
        let old: HashSet<usize> = previous_root.chunks().iter().map(chunk_key).collect();
        let new: HashSet<usize> = object.chunks().iter().map(chunk_key).collect();
        let mut previous: HashMap<ItemId, (&PathItem, usize)> = HashMap::new();
        let mut rank = 0usize;
        for chunk in previous_root.chunks().iter().filter(|c| !new.contains(&chunk_key(c))) {
            for record in chunk.records() { if let Record::Path(path) = record { previous.insert(path.id, (path, rank)); rank += 1; } }
        }
        self.chunks = object.chunks().iter().enumerate().map(|(i, c)| (chunk_key(c), i as u32)).collect();
        let mut common = Vec::new();
        let mut seen = HashSet::new();
        for (chunk, records) in object.chunks().iter().filter(|c| !old.contains(&chunk_key(c))).map(|c| (chunk_key(c), c.records())) {
            for (index, record) in records.iter().enumerate() {
                let Record::Path(path) = record else { continue };
                seen.insert(path.id);
                match (previous.get(&path.id), self.ids.get(&path.id).copied()) {
                    (Some((before, rank)), Some(slot)) if same(before, path) => {
                        let item = self.items[slot as usize].as_mut().unwrap();
                        item.chunk = chunk; item.index = index as u32; item.path = path.clone();
                        common.push((*rank, slot));
                    }
                    (_, Some(slot)) => {
                        other = true;
                        let removed = self.remove(slot, pool);
                        damage.push(removed.bounds);
                        self.pending.removed.push(removed);
                        damage.push(self.insert(path.clone(), chunk, index as u32, palette));
                        added.push(path.id);
                    }
                    (_, None) => { damage.push(self.insert(path.clone(), chunk, index as u32, palette)); added.push(path.id); }
                }
            }
        }
        for id in previous.keys().filter(|id| !seen.contains(*id)) {
            if let Some(slot) = self.ids.get(id).copied() {
                other = true;
                let removed = self.remove(slot, pool);
                damage.push(removed.bounds);
                self.pending.removed.push(removed);
            }
        }
        let mut ranks: Vec<usize> = common.iter().map(|(rank, _)| *rank).collect();
        if !ranks.is_sorted() {
            other = true;
            ranks.sort_unstable();
            for ((rank, slot), expected) in common.iter().zip(ranks) {
                if *rank != expected { damage.push(self.items[*slot as usize].as_ref().unwrap().bounds); self.pending.touched.push(*slot); }
            }
        }
        let set: HashSet<ItemId> = added.iter().copied().collect();
        let leading = object.records().take_while(|(_, record)| record.id().is_some_and(|id| set.contains(&id))).count();
        let slots: Vec<u32> = added.iter().filter_map(|id| self.ids.get(id).copied()).collect();
        let pending = &mut self.pending;
        pending.full |= first;
        if !other && !added.is_empty() && leading == added.len() { pending.appended.extend(&slots); } else { pending.mixed = true; }
        pending.touched.extend(slots);
        pending.damage.extend(damage);
        self.root = object.clone();
    }
    fn order(&self, item: &Item) -> u64 { u64::from(self.chunks.get(&item.chunk).copied().unwrap_or(0)) << 32 | u64::from(item.index) }
}

pub(super) fn coalesce(rects: Vec<PixelRect>) -> Vec<PixelRect> {
    if rects.len() <= 32 { return rects; }
    let mut pages = std::collections::BTreeSet::new();
    for rect in rects.iter().filter(|r| !r.is_empty()) { pages.extend(page_coordinates(*rect).map(|[x, y]| [y, x])); }
    let mut spans: Vec<PixelRect> = Vec::new();
    for [y, x] in pages {
        let page = page_rect([x, y]);
        match spans.last_mut() {
            Some(last) if last.min_y() == page.min_y() && last.max_x() == page.min_x() => *last = last.union(page),
            _ => spans.push(page),
        }
    }
    spans
}
pub(crate) fn preview_bounds(scene: SceneView<'_>, preview: &layer_render::VectorPreview) -> DocRect {
    let offset = scene.occurrence_offset64(preview.owner);
    let [x0, y0, x1, y1] = preview.path.bounds();
    DocRect { min: [(x0 + offset[0]).floor() as i64 - 16, (y0 + offset[1]).floor() as i64 - 16], max: [(x1 + offset[0]).ceil() as i64 + 16, (y1 + offset[1]).ceil() as i64 + 16] }
}
fn half_widths(path: &PathItem) -> Option<(f64, &layer_core::authored::vector::Subpath, Option<&layer_core::authored::vector::WidthProfile>)> {
    let stroke = path.style.strokes().next()?;
    Some((stroke.width * 0.5, path.geometry.subpaths().first()?, stroke.profile.as_deref()))
}
pub(crate) fn preview_damage(scene: SceneView<'_>, old: Option<&layer_render::VectorPreview>, new: Option<&layer_render::VectorPreview>) -> Vec<DocRect> {
    let whole = |p: &layer_render::VectorPreview| preview_bounds(scene, p);
    let (Some(old), Some(new)) = (old, new) else { return old.into_iter().chain(new).map(whole).collect() };
    let changed_style = old.owner != new.owner || old.path.style.entries.len() != new.path.style.entries.len()
        || old.path.style.entries.iter().zip(new.path.style.entries.iter()).any(|(a, b)| match (a, b) {
            (PaintEntry::Stroke(a), PaintEntry::Stroke(b)) => a.paint != b.paint || a.cap != b.cap || a.join != b.join,
            (a, b) => a != b,
        });
    let (Some((old_half, old_line, old_profile)), Some((new_half, new_line, new_profile)), false) = (half_widths(&old.path), half_widths(&new.path), changed_style) else { return vec![whole(old), whole(new)] };
    let width = |half: f64, profile: Option<&layer_core::authored::vector::WidthProfile>, i: usize| profile.map_or([half; 2], |p| p.at(i as u32, 0.).map(|f| f * half));
    let shared = old_line.len().min(new_line.len());
    let first = (0..shared).find(|&i| old_line.anchor(i) != new_line.anchor(i) || width(old_half, old_profile, i) != width(new_half, new_profile, i)).unwrap_or(shared).saturating_sub(1);
    let offset = scene.occurrence_offset64(new.owner);
    let mut bounds: Option<[f64; 4]> = None;
    for (line, half, profile) in [(old_line, old_half, old_profile), (new_line, new_half, new_profile)] {
        for i in first..line.len() {
            let p = line.anchor(i).point();
            let r = width(half, profile, i).into_iter().fold(0., f64::max) + 2.;
            bounds = Some(layer_core::authored::vector::union(bounds, [p[0] - r, p[1] - r, p[0] + r, p[1] + r]));
        }
    }
    bounds.map(|[x0, y0, x1, y1]| DocRect { min: [(x0 + offset[0]).floor() as i64 - 1, (y0 + offset[1]).floor() as i64 - 1], max: [(x1 + offset[0]).ceil() as i64 + 1, (y1 + offset[1]).ceil() as i64 + 1] }).into_iter().collect()
}
fn level_frame(offset: [f64; 2], level: u32, extent: [u32; 2]) -> Frame {
    let side = 1i64 << level;
    let tiles = extent.map(|v| (i64::from(v).div_euclid(side) as i32).div_euclid(TILE) + 1);
    Frame { scale: 1. / side as f64, origin: offset, clip: [0, 0, tiles[0], tiles[1]] }
}
pub(super) fn edit_damage(before: &VectorObject, after: &VectorObject) -> Vec<[f64; 4]> {
    if before.same_root(after) { return Vec::new(); }
    let old: HashSet<usize> = before.chunks().iter().map(chunk_key).collect();
    let new: HashSet<usize> = after.chunks().iter().map(chunk_key).collect();
    let mut previous: HashMap<ItemId, (&PathItem, usize)> = HashMap::new();
    for (rank, path) in before.chunks().iter().filter(|c| !new.contains(&chunk_key(c))).flat_map(|c| c.records()).filter_map(Record::path).enumerate() { previous.insert(path.id, (path, rank)); }
    let mut damage = Vec::new();
    let mut ranks = Vec::new();
    let mut seen = HashSet::new();
    for path in after.chunks().iter().filter(|c| !old.contains(&chunk_key(c))).flat_map(|c| c.records()).filter_map(Record::path) {
        seen.insert(path.id);
        match previous.get(&path.id) {
            Some((before, rank)) if same(before, path) => ranks.push((*rank, path.bounds())),
            Some((before, _)) => { damage.push(before.bounds()); damage.push(path.bounds()); }
            None => damage.push(path.bounds()),
        }
    }
    damage.extend(previous.iter().filter(|(id, _)| !seen.contains(*id)).map(|(_, (path, _))| path.bounds()));
    if !ranks.is_sorted_by_key(|(rank, _)| *rank) { damage.extend(ranks.into_iter().map(|(_, bounds)| bounds)); }
    damage
}

#[derive(Default)]
pub(super) struct Vectors {
    layers: HashMap<VectorHandle, LayerCache>,
    pool: Option<Pool>,
    frame: u64,
    stamp: u64,
    preview: Option<PreviewGeometry>,
    replacements: HashMap<ReplacementKey, Replacement>,
    pub(super) motion: Option<MotionImage>,
    spare_motion: Option<super::Image>,
    motion_charge: Option<crate::residency::Charge>,
    tier: Arc<crate::residency::Tier>,
    motion_revision: u64,
    batches: crate::vector::Arena,
    evicted: u64,
    trimmed: usize,
    levels: HashMap<(VectorHandle, u32), u64>,
    deadline: Option<web_time::Instant>,
    deferred: bool,
    waiting: Vec<Waiting>,
    baking: Vec<Baking>,
    baked: HashMap<VectorHandle, u64>,
    derived_paths: u64,
}
#[derive(Clone, Copy, PartialEq)]
struct Waiting { owner: OccurrenceHandle, origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2] }
type Baked = Vec<(u32, Arc<layer_core::authored::vector::PathGeometry>, LevelGeometry)>;
/// Full-resolution geometry a bake draws, derived off the canvas thread.
struct Baking { key: (VectorHandle, [i64; 2], [u32; 2]), results: std::sync::mpsc::Receiver<Baked> }
fn derive_lanes() -> usize {
    if cfg!(target_arch = "wasm32") { return 1; }
    std::thread::available_parallelism().map_or(1, |count| count.get().saturating_sub(2).clamp(1, 4))
}
fn derive_together(paths: &[&PathItem], frame: &Frame, geometry: &mut [LevelGeometry]) {
    let lane = paths.len().div_ceil(derive_lanes()).max(256);
    let derive = |paths: &[&PathItem], geometry: &mut [LevelGeometry]| {
        let mut scratch = tiles::Scratch::default();
        for (path, geometry) in paths.iter().zip(geometry) { tiles::derive_into(path, frame, geometry, &mut scratch); }
    };
    if paths.len() <= lane { return derive(paths, geometry); }
    std::thread::scope(|scope| for (paths, geometry) in paths.chunks(lane).zip(geometry.chunks_mut(lane)) { scope.spawn(move || derive(paths, geometry)); });
}
fn derive_off_thread(paths: Vec<(u32, PathItem)>, frame: Frame) -> std::sync::mpsc::Receiver<Baked> {
    let (send, results) = std::sync::mpsc::channel();
    let lanes = derive_lanes();
    std::thread::Builder::new().name("capy-vector-bake".into()).spawn(move || std::thread::scope(|scope| {
        for lane in paths.chunks(paths.len().div_ceil(lanes).max(1)) {
            let send = send.clone();
            scope.spawn(move || {
                let mut scratch = tiles::Scratch::default();
                for chunk in lane.chunks(256) {
                    let baked = chunk.iter().map(|(slot, path)| {
                        let mut geometry = LevelGeometry::default();
                        tiles::derive_into(path, &frame, &mut geometry, &mut scratch);
                        (*slot, path.geometry.clone(), geometry)
                    }).collect();
                    if send.send(baked).is_err() { return; }
                }
            });
        }
    })).expect("vector bake worker");
    results
}
/// The paths a motion moves, compared by identity.
#[derive(Clone, Debug)]
pub(crate) struct Moving(pub Arc<BTreeSet<ItemId>>);
impl PartialEq for Moving { fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) } }
impl Eq for Moving {}
impl PartialOrd for Moving { fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) } }
impl Ord for Moving { fn cmp(&self, other: &Self) -> std::cmp::Ordering { Arc::as_ptr(&self.0).cmp(&Arc::as_ptr(&other.0)) } }
impl std::hash::Hash for Moving { fn hash<H: std::hash::Hasher>(&self, state: &mut H) { Arc::as_ptr(&self.0).hash(state) } }
/// The paths a motion draws instead of the moving ones, compared by identity.
#[derive(Clone, Debug)]
pub(crate) struct Replaced(pub Option<Arc<[PathItem]>>);
impl PartialEq for Replaced { fn eq(&self, other: &Self) -> bool { self.0.as_ref().map(Arc::as_ptr) == other.0.as_ref().map(Arc::as_ptr) } }
pub(super) struct MotionImage { pub key: (OccurrenceHandle, Moving, u32, [i64; 2], Replaced), pub window: DocRect, pub image: super::Image, pub revision: u64 }
#[derive(Clone)]
pub(super) struct VectorJob { pub batch: Batch, pub output: wgpu::TextureView, pub base: Option<(wgpu::TextureView, [i32; 2])> }

impl Scene {
    pub(crate) fn restart_vector_uploads(&mut self) { self.vectors.restart(); }
    pub(super) fn vectors_deferred(&self) -> bool { self.vectors.deferred }
    pub(super) fn vector_palette(r: &WgpuRasterizer, packet: FramePacket<'_>) -> (layer_core::color::RgbSpace, layer_core::BlendSpace) {
        (r.device.working_space(), packet.blend_space)
    }
    pub(super) fn vector_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, owner: OccurrenceHandle, coordinate: [u32; 2]) -> Result<usize, GpuRasterError> {
        let origin = coordinate.map(|v| i64::from(v * PAGE_SIZE));
        let preview = r.vector_preview.as_ref().filter(|p| self.object_display && p.owner == owner).map(|p| p.path.clone());
        let batch = self.vectors.batch_with(packet.scene, owner, None, origin, 0, [PAGE_SIZE; 2], packet.document_extent, Self::vector_palette(r, packet), preview.as_ref(), false);
        let output = self.alloc(r, wgpu::Color::TRANSPARENT)?;
        if batch.tile_count() > 0 { self.jobs.push(Job::Vector(Box::new(VectorJob { batch, output: self.pool[output].view.clone(), base: None }))); }
        Ok(output)
    }
    pub(super) fn vector_thumbnail(&mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, owner: OccurrenceHandle, grid: display_mips::Plan,
        encoder: &mut crate::submission::CommandEncoder) -> Result<(wgpu::Texture, wgpu::TextureView), GpuRasterError> {
        let target = create_color_target(&r.device, [PAGE_SIZE; 2], "vector layer thumbnail");
        r.encode_clear(encoder, &target.1, "vector layer thumbnail");
        let batch = self.vectors.batch_with(packet.scene, owner, None, [0, 0], grid.level, grid.size, packet.document_extent, (r.device.working_space(), layer_core::BlendSpace::Linear), None, false);
        if batch.tile_count() > 0 { self.vectors.encode(r, encoder, &VectorJob { batch, output: target.1.clone(), base: None })?; }
        Ok(target)
    }
    pub(super) fn vector_damage(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>) {
        self.vectors.begin_frame();
        self.vectors.retain(packet.scene);
        let palette = Self::vector_palette(r, packet);
        let complete = self.vectors.advance(packet.scene, palette);
        for &owner in packet.scene.order() {
            let mut rects = self.vectors.damage(packet.scene, owner, palette);
            rects.extend(complete.iter().filter(|(window, _)| *window == owner).map(|(_, rect)| *rect));
            if rects.is_empty() { continue; }
            let extent = packet.document_extent;
            let rects = coalesce(rects.into_iter().map(|rect| rect.in_frame(extent)).filter(|rect| !rect.is_empty()).collect());
            let damage = self.scale_sources.object_damage.entry(Some(owner)).or_insert(scale::Damage::EMPTY);
            damage.regions.extend(rects.into_iter().map(|rect| rect.intersect(PixelRect::full(extent))));
        }
        self.vectors.trim();
        crate::performance_trace::counter(c"Capy vector paths derived", self.vectors.derived_paths());
    }
    pub(crate) fn presented_motion(&self, motion: &layer_render::VectorMotion) -> Option<(&wgpu::TextureView, DocRect, u32, [u32; 2], u64)> {
        let image = self.vectors.motion.as_ref().filter(|m| m.key.0 == motion.owner && Arc::ptr_eq(&m.key.1.0, &motion.items))?;
        Some((&image.image.view, image.window, image.image.plan.level, [image.image.texture.width(), image.image.texture.height()], image.revision))
    }
    pub(super) fn prepare_presented_motion(&mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        let presented = r.vector_motion_drawn.as_ref().filter(|_| r.vector_motion_split.is_some()).map(|(motion, bounds)| (motion.owner, Moving(motion.items.clone()), *bounds))
            .or_else(|| r.vector_prepared.as_ref().map(|((_, (owner, items), bounds), ..)| (*owner, items.clone(), *bounds)));
        let paths = r.vector_motion_paths_drawn.clone().filter(|_| r.vector_motion_drawn.is_some());
        if paths.is_none() { self.vectors.release_replacements(); }
        let Some((owner, items, bounds)) = presented else {
            if r.vector_motion_drawn.is_none() { self.vectors.release_motion(); }
            return Ok(());
        };
        let extent = packet.document_extent;
        let area = bounds.in_frame(extent);
        let display = r.scale_display.as_ref().map_or(0, |cache| cache.plan.level);
        let level = (display..display_mips::MAX_LEVEL).find(|level| area.area() >> (2 * level) <= MOTION_PIXELS).unwrap_or(display_mips::MAX_LEVEL);
        let key = (owner, items, level, packet.scene.occurrence_offset64(owner).map(|v| v as i64), Replaced(paths.clone()));
        if self.vectors.motion.as_ref().is_some_and(|m| m.key == key) || area.is_empty() { return Ok(()); }
        let palette = Self::vector_palette(r, packet);
        let shown = self.vectors.motion.as_ref().is_some_and(|m| m.key.0 == key.0 && m.key.1 == key.1 && m.key.3 == key.3 && m.key.4 == key.4);
        if shown && !self.vectors.motion_ready(packet.scene, owner, &key.1.0, level, extent, palette) {
            self.vectors.deferred = true;
            return Ok(());
        }
        let plan = display_mips::Plan::window(extent, level, DocRect::from(area).aligned(1 << level).in_frame(extent));
        let image = self.vectors.motion_image(r, plan);
        let batch = match &paths {
            Some(paths) => self.vectors.replacement_batch(paths, packet.scene.occurrence_offset64(owner), plan.doc_bounds.min, level, plan.size, extent, palette),
            None => self.vectors.motion_batch(packet.scene, owner, &key.1.0, plan.doc_bounds.min, level, plan.size, extent, palette),
        };
        r.encode_clear(encoder, &image.view, "vector motion");
        if batch.tile_count() > 0 { self.vectors.encode(r, encoder, &VectorJob { batch, output: image.view.clone(), base: None })?; }
        self.vectors.replace_motion(MotionImage { key, window: plan.doc_bounds, image, revision: 0 });
        Ok(())
    }
}

impl Vectors {
    /// Whether every path baking `owner` within `extent` draws has its
    /// full-resolution geometry. The rest is derived off the canvas thread, or
    /// within the frame's derivation time where there are no threads.
    pub fn bake_ready(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> bool {
        let (Some(handle), Some(object)) = (scene.vector_handle(owner), scene.vector_layer(owner)) else { return true };
        let offset = scene.occurrence_offset64(owner);
        let integer = offset.map(|v| v as i64);
        let key = (handle, integer, extent);
        let frame_number = self.frame;
        self.baked.insert(handle, frame_number);
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let layer = self.layers.entry(handle).or_default();
        layer.update(object, palette, pool);
        let mut running = false;
        if let Some(at) = self.baking.iter().position(|baking| baking.key == key) {
            loop {
                match self.baking[at].results.try_recv() {
                    Ok(baked) => for (slot, geometry, derived) in baked {
                        let Some(item) = layer.items.get_mut(slot as usize).and_then(Option::as_mut)
                            .filter(|item| Arc::ptr_eq(&item.path.geometry, &geometry) && item.derived(0, integer).is_none()) else { continue };
                        let geometry = Arc::new(derived);
                        layer.derived_bytes += geometry.bytes();
                        item.derived.push(Derived { level: 0, offset: integer, geometry, base: None, used: frame_number });
                    },
                    Err(std::sync::mpsc::TryRecvError::Empty) => { running = true; break; }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.baking.swap_remove(at); break; }
                }
            }
        }
        let mut missing = Vec::new();
        for (slot, item) in layer.items.iter_mut().enumerate().filter_map(|(slot, item)| Some((slot as u32, item.as_mut()?))) {
            match item.derived(0, integer) { Some(at) => item.derived[at].used = frame_number, None => missing.push((slot, item.path.clone())) }
        }
        if missing.is_empty() || running { return missing.is_empty(); }
        let frame = level_frame(offset, 0, extent);
        if cfg!(target_arch = "wasm32") {
            let deadline = web_time::Instant::now() + DERIVE_TIME;
            let LayerCache { items, derived_bytes, derived_paths, .. } = layer;
            return missing.into_iter().all(|(slot, _)| web_time::Instant::now() < deadline
                && { items[slot as usize].as_mut().unwrap().derived_at(0, integer, &frame, derived_bytes, derived_paths); true });
        }
        self.baking.push(Baking { key, results: derive_off_thread(missing, frame) });
        false
    }
    /// Paths the canvas thread derived or waited for.
    pub fn derived_paths(&self) -> u64 { self.derived_paths + self.layers.values().map(|layer| layer.derived_paths).sum::<u64>() }
    #[cfg(test)]
    pub fn storage_bytes(&self) -> u64 {
        self.pool.as_ref().map_or(0, Pool::storage_bytes) + self.batches.storage_bytes()
            + self.motion.as_ref().map_or(0, |motion| motion.image.bytes()) + self.spare_motion.as_ref().map_or(0, super::Image::bytes)
    }
    pub(super) fn motion_image(&mut self, r: &WgpuRasterizer, plan: display_mips::Plan) -> super::Image {
        let size = plan.size.map(|n| n.div_ceil(256) * 256);
        let fits = |image: &super::Image| image.texture.width() >= plan.size[0] && image.texture.height() >= plan.size[1]
            && u64::from(image.texture.width()) * u64::from(image.texture.height()) <= 4 * u64::from(size[0]) * u64::from(size[1]);
        if self.spare_motion.as_ref().is_none_or(|image|!fits(image)) {
            self.spare_motion = Some(super::Image::new(r, display_mips::Plan { size, ..plan }, "vector motion"));
            self.account_motion();
        }
        let image = self.spare_motion.as_ref().unwrap().clone();
        super::Image { plan, ..image }
    }
    pub(super) fn replace_motion(&mut self, motion: MotionImage) {
        if self.spare_motion.as_ref().is_some_and(|image|image.texture==motion.image.texture) { self.spare_motion = None; }
        self.motion_revision += 1;
        let texture = motion.image.texture.clone();
        if let Some(old) = self.motion.replace(MotionImage { revision: self.motion_revision, ..motion })
            && old.image.texture != texture { self.spare_motion = Some(old.image); }
        self.account_motion();
    }
    fn release_motion(&mut self) {
        if let Some(old) = self.motion.take() { self.spare_motion = Some(old.image); }
        self.account_motion();
    }
    fn account_motion(&mut self) {
        self.motion_charge = Some(self.tier.charge(self.motion.as_ref().map_or(0, |motion| motion.image.bytes()) + self.spare_motion.as_ref().map_or(0, super::Image::bytes)));
    }
    pub(super) fn charged(tier: &Arc<crate::residency::Tier>) -> Self {
        Self { tier: tier.clone(), batches: crate::vector::Arena::charged(tier), ..Default::default() }
    }
    /// Whether every path of `items` is derived at `level`, deriving within
    /// the frame's derivation time.
    pub fn motion_ready(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, items: &BTreeSet<ItemId>, level: u32, extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> bool {
        let (Some(handle), Some(object)) = (scene.vector_handle(owner), scene.vector_layer(owner)) else { return true };
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let layer = self.layers.entry(handle).or_default();
        layer.update(object, palette, pool);
        let slots = items.iter().filter_map(|id| layer.ids.get(id).copied()).collect();
        self.derive_within_deadline(handle, slots, scene.occurrence_offset64(owner), level, extent)
    }
    fn derive_within_deadline(&mut self, handle: VectorHandle, slots: Vec<u32>, offset: [f64; 2], level: u32, extent: [u32; 2]) -> bool {
        let (frame, integer, frame_number) = (level_frame(offset, level, extent), offset.map(|v| v as i64), self.frame);
        self.levels.insert((handle, level), frame_number);
        let Some(layer) = self.layers.get_mut(&handle) else { return true };
        for slot in slots {
            let item = layer.items[slot as usize].as_mut().unwrap();
            if item.derived(level, integer).is_some() { continue; }
            let now = web_time::Instant::now();
            if self.deadline.is_some_and(|deadline| now > deadline) { return false; }
            self.deadline.get_or_insert(now + DERIVE_TIME);
            let at = item.derived_at(level, integer, &frame, &mut layer.derived_bytes, &mut layer.derived_paths);
            item.derived[at].used = frame_number;
        }
        true
    }
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.deadline = None;
        self.deferred = false;
    }
    pub fn retain(&mut self, scene: SceneView<'_>) {
        let frame = self.frame;
        self.baked.retain(|_, used| *used + 2 >= frame);
        let live: HashSet<VectorHandle> = scene.order().iter().filter_map(|h| scene.vector_handle(*h)).chain(self.baked.keys().copied()).collect();
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let dropped = &mut self.derived_paths;
        self.layers.retain(|handle, layer| {
            let keep = live.contains(handle);
            if !keep {
                *dropped += layer.derived_paths;
                for item in layer.items.iter().flatten() { for d in &item.derived { if let Some(b) = d.base { pool.retire(b, &d.geometry); } } }
            }
            keep
        });
    }
    fn advance(&mut self, scene: SceneView<'_>, palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> Vec<(OccurrenceHandle, DocRect)> {
        let mut complete = Vec::new();
        for waiting in std::mem::take(&mut self.waiting) {
            if self.deadline.is_some_and(|deadline| web_time::Instant::now() > deadline) {
                self.deferred = true;
                self.waiting.push(waiting);
                continue;
            }
            let Some((handle, slots, offset)) = self.window_slots(scene, waiting, palette, None) else { continue };
            if self.entries(handle, slots, offset, waiting.level, waiting.extent, true).1 {
                self.deferred = true;
                self.waiting.push(waiting);
            } else {
                let span = waiting.size.map(|v| i64::from(v) << waiting.level);
                complete.push((waiting.owner, DocRect { min: waiting.origin, max: [waiting.origin[0] + span[0], waiting.origin[1] + span[1]] }));
            }
        }
        complete
    }
    fn window_slots(&mut self, scene: SceneView<'_>, at: Waiting, palette: (layer_core::color::RgbSpace, layer_core::BlendSpace), excluded: Option<&BTreeSet<ItemId>>) -> Option<(VectorHandle, Vec<u32>, [f64; 2])> {
        let handle = scene.vector_handle(at.owner)?;
        let object = scene.vector(handle)?;
        let offset = scene.occurrence_offset64(at.owner);
        self.stamp += 1;
        let stamp = self.stamp;
        let side = (1i64 << at.level) as f64;
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let layer = self.layers.entry(handle).or_default();
        layer.update(object, palette, pool);
        let x0 = (at.origin[0] - offset[0].floor() as i64) as f64;
        let y0 = (at.origin[1] - offset[1].floor() as i64) as f64;
        let window = [x0, y0, x0 + f64::from(at.size[0]) * side, y0 + f64::from(at.size[1]) * side];
        let [cx0, cy0, cx1, cy1] = cells([window[0] - 16., window[1] - 16., window[2] + 16., window[3] + 16.]);
        let mut slots = Vec::new();
        for y in cy0..cy1 { for x in cx0..cx1 {
            for slot in layer.grid.get(&[x, y]).into_iter().flatten() {
                let item = layer.items[*slot as usize].as_mut().unwrap();
                if item.seen == stamp { continue; }
                item.seen = stamp;
                let b = item.bounds;
                if excluded.is_some_and(|excluded| excluded.contains(&item.path.id)) || b[2] < window[0] - 16. || b[0] > window[2] + 16. || b[3] < window[1] - 16. || b[1] > window[3] + 16. { continue; }
                slots.push(*slot);
            }
        }}
        Some((handle, slots, offset))
    }
    /// Whether every path a window draws is derived at its level, deriving
    /// within the frame's derivation time.
    #[expect(clippy::too_many_arguments, reason = "A window names its layer, placement, level, extent and color domain")]
    pub fn window_ready(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> bool {
        let Some((handle, slots, offset)) = self.window_slots(scene, Waiting { owner, origin, level, size, extent }, palette, None) else { return true };
        self.derive_within_deadline(handle, slots, offset, level, extent)
    }
    pub fn damage(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> Vec<DocRect> {
        let Some(handle) = scene.vector_handle(owner) else { return Vec::new() };
        let Some(object) = scene.vector(handle) else { return Vec::new() };
        let offset = scene.occurrence_offset64(owner);
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let layer = self.layers.entry(handle).or_default();
        layer.update(object, palette, pool);
        layer.change = std::mem::take(&mut layer.pending);
        layer.change.damage.iter().map(|[x0, y0, x1, y1]| DocRect {
            min: [(x0 + offset[0]).floor() as i64 - 16, (y0 + offset[1]).floor() as i64 - 16],
            max: [(x1 + offset[0]).ceil() as i64 + 16, (y1 + offset[1]).ceil() as i64 + 16],
        }).collect()
    }
    pub fn bounds(scene: SceneView<'_>, owner: OccurrenceHandle) -> DocRect {
        let Some(object) = scene.vector_layer(owner) else { return DocRect::default() };
        let offset = scene.occurrence_offset64(owner);
        object.bounds().map_or(DocRect::default(), |[x0, y0, x1, y1]| DocRect {
            min: [(x0 + offset[0]).floor() as i64 - 16, (y0 + offset[1]).floor() as i64 - 16],
            max: [(x1 + offset[0]).ceil() as i64 + 16, (y1 + offset[1]).ceil() as i64 + 16],
        })
    }
    #[expect(clippy::too_many_arguments, reason = "A window names its layer, the paths it leaves out, placement, level, extent and color domain")]
    pub fn batch(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, excluded: Option<&BTreeSet<ItemId>>, origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> Batch {
        self.batch_with(scene, owner, excluded, origin, level, size, extent, palette, None, true)
    }
    #[expect(clippy::too_many_arguments, reason = "A window names its layer, the paths it leaves out, placement, level, extent and color domain")]
    pub fn append_batch(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, excluded: Option<&BTreeSet<ItemId>>, origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> Option<Batch> {
        let handle = scene.vector_handle(owner)?;
        let object = scene.vector(handle)?;
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let layer = self.layers.get_mut(&handle)?;
        layer.update(object, palette, pool);
        let slots: Vec<u32> = layer.change.appended()?.iter().copied().filter(|slot| layer.items[*slot as usize].as_ref().is_some_and(|item| excluded.is_none_or(|excluded| !excluded.contains(&item.path.id)))).collect();
        let (entries, _) = self.entries(handle, slots, scene.occurrence_offset64(owner), level, extent, false);
        let layers: Vec<Layer> = entries.iter().map(|(geometry, base, colors)| Layer { geometry, base: *base, colors }).collect();
        let mut batch = Batch::default();
        let side = 1i64 << level;
        batch.build(&layers, [origin[0].div_euclid(side), origin[1].div_euclid(side)], size);
        Some(batch)
    }
    /// Redraw only the tiles of a window that the layer's changes cover, or
    /// with `leaving` the tiles those paths cover, over a copy of the window.
    #[expect(clippy::too_many_arguments, reason = "A window names its layer, the paths it leaves out, placement, level, extent and color domain")]
    pub fn repaint_batch(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, excluded: Option<&BTreeSet<ItemId>>, leaving: Option<&BTreeSet<ItemId>>, origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> Option<Batch> {
        let handle = scene.vector_handle(owner)?;
        let object = scene.vector(handle)?;
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let layer = self.layers.get_mut(&handle)?;
        layer.update(object, palette, pool);
        if leaving.is_none() { layer.change.damage()?; }
        let offset = scene.occurrence_offset64(owner);
        let side = 1i64 << level;
        let tile = i64::from(TILE);
        let level_origin = origin.map(|v| v.div_euclid(side));
        let t0 = level_origin.map(|v| v.div_euclid(tile) as i32);
        let t1 = [0, 1].map(|i| (level_origin[i] + i64::from(size[i]) + tile - 1).div_euclid(tile) as i32);
        let columns = (t1[0] - t0[0]).max(0) as usize;
        let rows = (t1[1] - t0[1]).max(0) as usize;
        let mut mask = vec![false; columns * rows];
        let frame = level_frame(offset, level, extent);
        let integer_offset = offset.map(|v| v as i64);
        let window = [0, 1].map(|i| (origin[i] as f64 - offset[i], origin[i] as f64 - offset[i] + f64::from(size[i]) * side as f64));
        let inside = |b: [f64; 4]| b[2] >= window[0].0 - 16. && b[0] <= window[0].1 + 16. && b[3] >= window[1].0 - 16. && b[1] <= window[1].1 + 16.;
        let LayerCache { items, ids, change, derived_bytes, derived_paths, .. } = layer;
        let mut mark = |item: &mut Item| if inside(item.bounds) {
            let at = item.derived_at(level, integer_offset, &frame, derived_bytes, derived_paths);
            for t in item.derived[at].geometry.tile_range(t0[1]..t1[1], t0[0]..t1[0]) { mask[(t.tile[1] - t0[1]) as usize * columns + (t.tile[0] - t0[0]) as usize] = true; }
        };
        match leaving {
            Some(leaving) => for id in leaving { if let Some(item) = ids.get(id).and_then(|slot| items[*slot as usize].as_mut()) { mark(item); } },
            None => {
                for item in change.removed.iter_mut() { mark(item); }
                for slot in &change.touched { if let Some(item) = items[*slot as usize].as_mut() { mark(item); } }
            }
        }
        if mask.iter().filter(|m| **m).count() * 2 > mask.len() { return None; }
        let mut covered = vec![0u32; (columns + 1) * (rows + 1)];
        for y in 0..rows { for x in 0..columns {
            covered[(y + 1) * (columns + 1) + x + 1] = u32::from(mask[y * columns + x]) + covered[y * (columns + 1) + x + 1] + covered[(y + 1) * (columns + 1) + x] - covered[y * (columns + 1) + x];
        }}
        let masked = |bounds: [f64; 4]| {
            let tile_of = |v: f64, i: usize| (((v + offset[i]) / side as f64).floor() as i64).div_euclid(tile) as i32;
            let bounds = [bounds[0] - side as f64, bounds[1] - side as f64, bounds[2] + side as f64, bounds[3] + side as f64];
            let [x0, y0] = [tile_of(bounds[0], 0).max(t0[0]) - t0[0], tile_of(bounds[1], 1).max(t0[1]) - t0[1]].map(|v| v.max(0) as usize);
            let [x1, y1] = [((tile_of(bounds[2], 0) + 1).min(t1[0]) - t0[0]).max(0) as usize, ((tile_of(bounds[3], 1) + 1).min(t1[1]) - t0[1]).max(0) as usize];
            x0 < x1 && y0 < y1 && covered[y1 * (columns + 1) + x1] + covered[y0 * (columns + 1) + x0] > covered[y0 * (columns + 1) + x1] + covered[y1 * (columns + 1) + x0]
        };
        self.stamp += 1;
        let stamp = self.stamp;
        let layer = self.layers.get_mut(&handle)?;
        let [cx0, cy0, cx1, cy1] = cells([window[0].0 - 16., window[1].0 - 16., window[0].1 + 16., window[1].1 + 16.]);
        let mut slots = Vec::new();
        for y in cy0..cy1 { for x in cx0..cx1 {
            for slot in layer.grid.get(&[x, y]).into_iter().flatten() {
                let item = layer.items[*slot as usize].as_mut().unwrap();
                if item.seen == stamp { continue; }
                item.seen = stamp;
                if excluded.is_none_or(|excluded| !excluded.contains(&item.path.id)) && masked(item.bounds) { slots.push(*slot); }
            }
        }}
        let (entries, _) = self.entries(handle, slots, offset, level, extent, false);
        let layers: Vec<Layer> = entries.iter().map(|(geometry, base, colors)| Layer { geometry, base: *base, colors }).collect();
        let mut batch = Batch::default();
        batch.build_within(&layers, level_origin, size, Some(&mask));
        Some(batch)
    }
    #[expect(clippy::too_many_arguments, reason = "A window names its layer, items, placement, level, extent and color domain")]
    pub fn motion_batch(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, items: &BTreeSet<ItemId>, origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> Batch {
        let mut batch = Batch::default();
        let (Some(handle), Some(object)) = (scene.vector_handle(owner), scene.vector_layer(owner)) else { return batch };
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let layer = self.layers.entry(handle).or_default();
        layer.update(object, palette, pool);
        let slots = items.iter().filter_map(|id| layer.ids.get(id).copied()).collect();
        let (entries, _) = self.entries(handle, slots, scene.occurrence_offset64(owner), level, extent, false);
        let layers: Vec<Layer> = entries.iter().map(|(geometry, base, colors)| Layer { geometry, base: *base, colors }).collect();
        let side = 1i64 << level;
        batch.build(&layers, origin.map(|v| v.div_euclid(side)), size);
        batch
    }
    fn entries(&mut self, handle: VectorHandle, slots: Vec<u32>, offset: [f64; 2], level: u32, extent: [u32; 2], budgeted: bool) -> (Vec<(Arc<LevelGeometry>, u32, Colors)>, bool) {
        let frame = level_frame(offset, level, extent);
        let integer_offset = offset.map(|v| v as i64);
        let frame_number = self.frame;
        self.levels.insert((handle, level), frame_number);
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let Some(layer) = self.layers.get_mut(&handle) else { return (Vec::new(), false) };
        let mut skipped = false;
        let mut keyed: Vec<(u64, u32)> = slots.into_iter().map(|s| (layer.order(layer.items[s as usize].as_ref().unwrap()), s)).collect();
        keyed.sort_unstable_by_key(|(order, _)| std::cmp::Reverse(*order));
        let mut entries = Vec::with_capacity(keyed.len());
        for (_, slot) in keyed {
            let item = layer.items[slot as usize].as_mut().unwrap();
            if budgeted && item.derived(level, integer_offset).is_none() {
                let now = web_time::Instant::now();
                if self.deadline.is_some_and(|deadline| now > deadline) { skipped = true; continue; }
                self.deadline.get_or_insert(now + DERIVE_TIME);
            }
            let at = item.derived_at(level, integer_offset, &frame, &mut layer.derived_bytes, &mut layer.derived_paths);
            let derived = &mut item.derived[at];
            derived.used = frame_number;
            if derived.base.is_none() { derived.base = pool.allocate(&derived.geometry); }
            if derived.base.is_none() && self.evicted < frame_number {
                self.evicted = frame_number;
                let geometry = derived.geometry.clone();
                for other in layer.items.iter_mut().flatten() {
                    for d in other.derived.iter_mut().filter(|d| d.used < frame_number) { if let Some(b) = d.base.take() { pool.release(b, &d.geometry); } }
                }
                let item = layer.items[slot as usize].as_mut().unwrap();
                item.derived[at].base = pool.allocate(&geometry);
            }
            let item = layer.items[slot as usize].as_ref().unwrap();
            let derived = &item.derived[at];
            let Some(base) = derived.base else { continue };
            entries.push((derived.geometry.clone(), base, item.colors.clone()));
        }
        (entries, skipped)
    }
    #[expect(clippy::too_many_arguments, reason = "A window names its layer, the paths it leaves out, placement, level, extent, color domain and provisional path")]
    fn batch_with(&mut self, scene: SceneView<'_>, owner: OccurrenceHandle, excluded: Option<&BTreeSet<ItemId>>, origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace), preview: Option<&Arc<PathItem>>, display: bool) -> Batch {
        let mut batch = Batch::default();
        let at = Waiting { owner, origin, level, size, extent };
        let Some((handle, slots, offset)) = self.window_slots(scene, at, palette, excluded) else { return batch };
        let side = 1i64 << level;
        let level_origin = [origin[0].div_euclid(side), origin[1].div_euclid(side)];
        let (mut entries, skipped) = self.entries(handle, slots, offset, level, extent, display);
        if skipped && !self.waiting.contains(&at) {
            self.deferred = true;
            self.waiting.push(at);
        }
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let tiles = extent.map(|v| (i64::from(v).div_euclid(side) as i32).div_euclid(TILE) + 1);
        let frame = Frame { scale: 1. / side as f64, origin: offset, clip: [0, 0, tiles[0], tiles[1]] };
        if let Some(path) = preview {
            let key = (Arc::as_ptr(path) as usize, level, offset.map(f64::to_bits));
            if self.preview.as_ref().is_none_or(|p| p.0 != key) {
                if let Some(previous) = self.preview.take() { pool.retire(previous.1, &previous.2); }
                let geometry = Arc::new(tiles::derive(path, &frame));
                if let Some(base) = pool.allocate(&geometry) { self.preview = Some((key, base, geometry, colors(path, palette).collect())); }
            }
            if let Some((_, base, geometry, colors)) = &self.preview { entries.push((geometry.clone(), *base, colors.clone())); }
        }
        let layers: Vec<Layer> = entries.iter().map(|(geometry, base, colors)| Layer { geometry, base: *base, colors }).collect();
        batch.build(&layers, level_origin, size);
        batch
    }
    #[expect(clippy::too_many_arguments, reason = "A preview window names its path, placement, level, extent and color domain")]
    pub fn preview_batch(&mut self, path: &Arc<PathItem>, offset: [f64; 2], origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> Batch {
        let side = 1i64 << level;
        let key = (Arc::as_ptr(path) as usize, level, offset.map(f64::to_bits));
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        if self.preview.as_ref().is_none_or(|p| p.0 != key) {
            if let Some(previous) = self.preview.take() { pool.retire(previous.1, &previous.2); }
            let tiles = extent.map(|v| (i64::from(v).div_euclid(side) as i32).div_euclid(TILE) + 1);
            let geometry = Arc::new(tiles::derive(path, &Frame { scale: 1. / side as f64, origin: offset, clip: [0, 0, tiles[0], tiles[1]] }));
            let Some(base) = pool.allocate(&geometry) else { return Batch::default() };
            self.preview = Some((key, base, geometry, colors(path, palette).collect()));
        }
        let (_, base, geometry, colors) = self.preview.as_ref().unwrap();
        let mut batch = Batch::default();
        batch.build(&[Layer { geometry, base: *base, colors }], [origin[0].div_euclid(side), origin[1].div_euclid(side)], size);
        batch
    }
    /// A batch drawing `paths`, in layer order, instead of the layer's own.
    /// A path keeps its derivation while its geometry and outline stay the
    /// same, and a changed one is derived again into its own buffers.
    #[expect(clippy::too_many_arguments, reason = "A replacement batch names its paths, placement, level, window, extent and color domain")]
    pub fn replacement_batch(&mut self, paths: &[PathItem], offset: [f64; 2], origin: [i64; 2], level: u32, size: [u32; 2], extent: [u32; 2], palette: (layer_core::color::RgbSpace, layer_core::BlendSpace)) -> Batch {
        let frame = level_frame(offset, level, extent);
        let (frame_number, offset) = (self.frame, offset.map(f64::to_bits));
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let shown: Vec<&PathItem> = paths.iter().rev().filter(|path| !path.hidden).collect();
        let mut missing = Vec::new();
        for path in &shown {
            match self.replacements.get_mut(&(path.id, level)) {
                Some(entry) if entry.derives(path, offset) => {
                    if !Arc::ptr_eq(&entry.path.style, &path.style) {
                        entry.colors.clear();
                        entry.colors.extend(colors(path, palette));
                        entry.path = (*path).clone();
                    }
                    entry.used = frame_number;
                }
                entry => {
                    let buffer = entry.and_then(|entry| {
                        if let Some(base) = entry.base.take() { pool.retire(base, &entry.geometry); }
                        Arc::get_mut(&mut entry.geometry).map(std::mem::take)
                    });
                    missing.push((*path, buffer.unwrap_or_default()));
                }
            }
        }
        let (derived, mut geometry): (Vec<&PathItem>, Vec<LevelGeometry>) = missing.into_iter().unzip();
        derive_together(&derived, &frame, &mut geometry);
        self.derived_paths += derived.len() as u64;
        for (path, geometry) in derived.into_iter().zip(geometry) {
            let entry = self.replacements.entry((path.id, level)).or_insert_with(|| Replacement {
                geometry: Arc::default(), base: None, colors: Vec::new(), used: 0, path: path.clone(), offset });
            match Arc::get_mut(&mut entry.geometry) { Some(owned) => *owned = geometry, None => entry.geometry = Arc::new(geometry) }
            entry.base = pool.allocate(&entry.geometry);
            entry.colors.clear();
            entry.colors.extend(colors(path, palette));
            entry.path = path.clone();
            entry.offset = offset;
            entry.used = frame_number;
        }
        self.replacements.retain(|_, entry| {
            if entry.used == frame_number { return true; }
            if let Some(base) = entry.base { pool.retire(base, &entry.geometry); }
            false
        });
        let layers: Vec<Layer> = shown.iter().filter_map(|path| self.replacements.get(&(path.id, level)))
            .filter_map(|entry| Some(Layer { geometry: &entry.geometry, base: entry.base?, colors: &entry.colors })).collect();
        let side = 1i64 << level;
        let mut batch = Batch::default();
        batch.build(&layers, origin.map(|v| v.div_euclid(side)), size);
        batch
    }
    #[cfg(test)]
    pub(crate) fn replacement_count(&self) -> usize { self.replacements.len() }
    fn release_replacements(&mut self) {
        let Some(pool) = self.pool.as_mut() else { return };
        for (_, entry) in self.replacements.drain() { if let Some(base) = entry.base { pool.retire(base, &entry.geometry); } }
    }
    pub fn encode(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, job: &VectorJob) -> Result<(), GpuRasterError> {
        if job.batch.tile_count() > 0 && !r.compilation.require([r.scene_pipelines.vector.pipeline()], startup::DOCUMENT) {
            return Err(GpuRasterError::NotReady);
        }
        r.metrics.vector_tiles += job.batch.tile_count() as u64;
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        let buffer = pool.flush(&r.device, encoder, &mut r.uploads)?;
        r.scene_pipelines.vector.encode(&r.device, encoder, &mut r.uploads, &mut self.batches, &job.batch, &buffer, job.base.as_ref().map(|(view, origin)| (view, *origin)), &r.empty_view, &job.output)
    }
    pub fn restart(&mut self) {
        self.batches.restart();
        if let Some(pool) = &mut self.pool { pool.restart(); }
    }
    pub fn trim(&mut self) {
        let total: usize = self.layers.values().map(|layer| layer.derived_bytes).sum();
        self.trimmed = self.trimmed.min(total);
        let frame = self.frame;
        let stale = self.levels.values().any(|used| used + 2 < frame);
        if total <= DERIVED_BYTES || (!stale && total < self.trimmed + DERIVED_BYTES / 8) { return; }
        let pool = self.pool.get_or_insert_with(|| Pool::charged(POOL_BYTES, &self.tier));
        for layer in self.layers.values_mut() {
            let mut freed = 0usize;
            for item in layer.items.iter_mut().flatten() {
                item.derived.retain(|d| {
                    let keep = d.used + 2 >= frame;
                    if !keep { freed += d.geometry.bytes(); if let Some(b) = d.base { pool.release(b, &d.geometry); } }
                    keep
                });
            }
            layer.derived_bytes = layer.derived_bytes.saturating_sub(freed);
        }
        self.trimmed = self.layers.values().map(|layer| layer.derived_bytes).sum();
        self.levels.retain(|_, used| *used + 2 >= frame);
    }
}

#[cfg(test)]
#[path = "vector_tests.rs"]
mod tests;

pub(crate) fn moved_bounds(transform: Affine64, bounds: DocRect) -> DocRect {
    let corners = [[bounds.min[0], bounds.min[1]], [bounds.max[0], bounds.min[1]], [bounds.min[0], bounds.max[1]], [bounds.max[0], bounds.max[1]]]
        .map(|p| transform.map(p.map(|v| v as f64)));
    DocRect {
        min: [0, 1].map(|i| corners.iter().map(|c| c[i]).fold(f64::INFINITY, f64::min).floor() as i64),
        max: [0, 1].map(|i| corners.iter().map(|c| c[i]).fold(f64::NEG_INFINITY, f64::max).ceil() as i64),
    }
}
pub(crate) fn level_transform(motion: &layer_render::VectorMotion, level: u32) -> Affine64 {
    let [a, b, c, d, x, y] = motion.transform.0;
    let side = f64::from(1u32 << level);
    if [a, b, c, d] == [1., 0., 0., 1.] { Affine64([a, b, c, d, (x / side).round() * side, (y / side).round() * side]) } else { motion.transform }
}
pub(crate) type MotionSplit = Arc<(layer_core::authored::SceneScope, layer_core::authored::SceneScope)>;
pub(crate) type ActiveSplit = Arc<[layer_core::authored::SceneScope; 3]>;
fn plain(scene: SceneView<'_>, h: OccurrenceHandle) -> bool {
    scene.occurrence(h).is_some_and(|o| o.blend == layer_core::LayerBlend::Normal && !o.attachment.is_clip() && !o.mask.as_ref().is_some_and(|m| m.enabled)
        && o.opacity == 1.) && scene.attached_effects(h).is_empty()
}
pub(crate) fn motion_split(scene: SceneView<'_>, owner: OccurrenceHandle) -> Option<MotionSplit> {
    if !plain(scene, owner) { return None; }
    let (_, above, below) = split_above(scene, owner)?;
    Some(Arc::new((layer_core::authored::SceneScope::Members(above.into()), layer_core::authored::SceneScope::Members(below.into()))))
}
pub(crate) fn active_split(scene: SceneView<'_>, owner: OccurrenceHandle) -> Option<ActiveSplit> {
    use layer_core::authored::OccurrenceContent::{Objects, Paint, Vector};
    if scene.occurrence(owner).is_none_or(|o| o.blend != layer_core::LayerBlend::Normal || o.attachment.is_clip() || !matches!(o.content, Paint(_) | Objects(_) | Vector(_)))
        || !scene.attached_effects(owner).is_empty() { return None; }
    let (ancestors, above, mut below) = split_above(scene, owner)?;
    below.retain(|h| *h != owner);
    let active = ancestors.into_iter().chain([owner]).collect();
    Some(Arc::new([above.into(), active, below.into()].map(layer_core::authored::SceneScope::Members)))
}
fn split_above(scene: SceneView<'_>, owner: OccurrenceHandle) -> Option<(Vec<OccurrenceHandle>, Vec<OccurrenceHandle>, Vec<OccurrenceHandle>)> {
    let ancestors: Vec<_> = std::iter::successors(scene.parent(owner), |h| scene.parent(*h)).collect();
    if !ancestors.iter().all(|h| plain(scene, *h)) { return None; }
    let position = scene.position(owner)?;
    let (mut above, mut below) = (Vec::new(), Vec::new());
    for (at, &h) in scene.order().iter().enumerate() {
        if ancestors.contains(&h) { above.push(h); below.push(h); continue; }
        if at >= position { below.push(h); continue; }
        let drawn = scene.visible(h) && scene.occurrence(h).is_some_and(|o| o.is_artwork());
        let reads_below = scene.effect(h).is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
            || scene.occurrence(h).is_some_and(|o| o.blend != layer_core::LayerBlend::Normal)
            || scene.clipping_base(h).and_then(|base| scene.position(base)).is_some_and(|base| base >= position);
        if drawn && reads_below { return None; }
        above.push(h);
    }
    Some((ancestors, above, below))
}
pub(crate) type Prepared = (MotionSplit, (OccurrenceHandle, Moving), DocRect);
pub(crate) fn paths_bounds(scene: SceneView<'_>, owner: OccurrenceHandle, paths: &[PathItem]) -> DocRect {
    let offset = scene.occurrence_offset64(owner);
    paths.iter().filter(|path| !path.hidden).map(PathItem::bounds).fold(DocRect::default(), |bounds, [x0, y0, x1, y1]| bounds.union(DocRect {
        min: [(x0 + offset[0]).floor() as i64 - 16, (y0 + offset[1]).floor() as i64 - 16],
        max: [(x1 + offset[0]).ceil() as i64 + 16, (y1 + offset[1]).ceil() as i64 + 16],
    }))
}
pub(crate) fn motion_bounds(scene: SceneView<'_>, owner: OccurrenceHandle, items: &BTreeSet<ItemId>) -> DocRect {
    let Some(object) = scene.vector_layer(owner) else { return DocRect::default() };
    let offset = scene.occurrence_offset64(owner);
    object.items_bounds(items).map_or(DocRect::default(), |[x0, y0, x1, y1]| DocRect {
        min: [(x0 + offset[0]).floor() as i64 - 16, (y0 + offset[1]).floor() as i64 - 16],
        max: [(x1 + offset[0]).ceil() as i64 + 16, (y1 + offset[1]).ceil() as i64 + 16],
    })
}
/// What dragging `selection` needs prepared: its layer split around it, the
/// selected paths and their bounds, with `previous` reused while the layer and
/// selection stay the same.
pub(crate) fn prepared(scene: SceneView<'_>, selection: &layer_core::authored::VectorItemSelection, previous: Option<&(Prepared, VectorObject, [f64; 2])>) -> Option<(Prepared, VectorObject, [f64; 2])> {
    let object = scene.vector_layer(selection.layer)?;
    let offset = scene.occurrence_offset64(selection.layer);
    let split = motion_split(scene, selection.layer)?;
    let bounds = previous.filter(|((_, (owner, items), _), root, at)| *owner == selection.layer && Arc::ptr_eq(&items.0, &selection.items) && root.same_root(object) && *at == offset)
        .map_or_else(|| motion_bounds(scene, selection.layer, &selection.items), |((_, _, bounds), ..)| *bounds);
    Some(((split, (selection.layer, Moving(selection.items.clone())), bounds), object.clone(), offset))
}
impl WgpuRasterizer {
    /// Whether the vector layers of the drawing's pending bakes have the
    /// geometry their full-resolution tiles draw.
    pub(crate) fn bake_vectors_ready(&mut self, packet: FramePacket<'_>) -> bool {
        let working = self.device.working_space();
        let Some(scene) = self.scene.as_mut() else { return true };
        let mut ready = true;
        for target in crate::source_access::placed_targets(packet.scene) {
            for operation in packet.scene.operations(target).into_iter().flatten() {
                let (layer_core::RasterOperationKind::Bake { scene: snapshot, scope, offset }
                    | layer_core::RasterOperationKind::FrequencyDetail { scene: snapshot, scope, offset, .. }) = &operation.kind else { continue };
                let view = snapshot.view().with_scope(scope).with_offset(*offset);
                let extent = packet.scene.target_extent(target);
                for &owner in view.order().iter().filter(|owner| view.vector_layer(**owner).is_some() && view.visible(**owner)) {
                    ready &= scene.vectors.bake_ready(view, owner, extent, (working, view.composition().blend));
                }
            }
        }
        ready
    }
}
