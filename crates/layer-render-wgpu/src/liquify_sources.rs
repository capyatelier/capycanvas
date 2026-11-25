use super::*;
use crate::residency::Class;
use layer_core::{MemoryBudget, MemoryReservation, authored::{PaintBase, PaintSource}, color::{ColorProfile, RgbSpace, source::{SourceChannels, SourceImage}}, raster::{TileBlob, TileKey}};
use object_image_mips::PreparedImageWork;
use scene::sources::PreparedSourcePixels;
use std::{collections::HashMap, sync::Weak};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Leaf { Image(usize, [u32; 2]), Raster(u64) }

enum Input { Image(Arc<SourceImage>, [u32; 2]), Raster(Arc<TileBlob>) }
impl Input {
    fn leaf(&self) -> Leaf {
        match self { Self::Image(source, at) => Leaf::Image(Arc::as_ptr(source) as usize, *at), Self::Raster(blob) => Leaf::Raster(blob.owner_identity()) }
    }
    fn bytes(&self) -> (u64, bool) {
        let (page, native) = (u64::from(PAGE_SIZE).pow(2), cfg!(not(target_arch = "wasm32")));
        match self {
            Self::Image(source, _) => {
                let interpretation = &source.interpretation;
                if !matches!(interpretation.profile, ColorProfile::Builtin(_)) || interpretation.channels == SourceChannels::Cmyk { return (scene::sources::NORMALIZED_PAGE_BYTES, false); }
                (page * 4 * interpretation.depth.bytes() as u64, native && interpretation.channels == SourceChannels::Rgba)
            },
            Self::Raster(blob) => (page * 4 * blob.descriptor.depth().bytes() as u64,
                native && (blob.descriptor.channels == 4 || object_image_mips::native_scalar(blob.descriptor))),
        }
    }
}

struct Entry { input: Input, pixels: Option<PreparedSourcePixels>, requested: bool, failed: bool }
impl Entry {
    fn missing(&self) -> bool { self.pixels.is_none() && !self.failed }
    fn charged(&self) -> u64 { let (bytes, shared) = self.input.bytes(); if self.failed || shared && self.pixels.is_some() { 0 } else { bytes } }
    fn read_elsewhere(&self) -> Option<(Weak<Vec<u8>>, u64)> {
        let (bytes, shared) = self.input.bytes();
        let (PreparedSourcePixels::NativeSamples(pixels) | PreparedSourcePixels::PremultipliedWorkingPixels(pixels)) = self.pixels.as_ref().filter(|_| !shared)?;
        (Arc::strong_count(pixels) > 1).then(|| (Arc::downgrade(pixels), bytes))
    }
}

#[derive(PartialEq)]
struct Identity { owner: u64, raster: u64, base: Option<PaintBase>, original: Option<usize> }

pub(crate) struct LiquifySources {
    target: Option<SourceTarget>,
    space: RgbSpace,
    identity: Option<Identity>,
    entries: HashMap<Leaf, Entry>,
    order: Vec<Leaf>,
    retired: Vec<(Weak<Vec<u8>>, u64)>,
    missing: usize,
    charged: u64,
    reservation: Option<MemoryReservation>,
    synced: bool,
    yielded: bool,
}

fn cover(budget: &MemoryBudget, reservation: &MemoryReservation, bytes: u64) -> bool {
    match bytes.checked_sub(reservation.reserved_bytes()) {
        Some(0) => true,
        Some(more) => budget.admit_covered_import(more).is_some_and(|more| reservation.merge(more).is_ok()),
        None => reservation.resize(bytes),
    }
}

fn leaves(source: &PaintSource, wanted: &mut Vec<([u32; 2], Input)>) -> bool {
    let data = match source.raster.try_data() { Some(Ok(data)) => data, Some(Err(_)) => return true, None => return false };
    let mut known = true;
    for coordinate in page_coordinates(PixelRect::full(source.domain)) {
        match data.tiles.get(&TileKey { plane: RasterPlane::Color, coordinate }).map(|tile| tile.try_backing()) {
            Some(Some(Ok(blob))) => wanted.push((coordinate, Input::Raster(blob))),
            Some(None) => known = false,
            Some(Some(Err(_))) => {},
            None => if let Some(base) = source.base.as_ref().filter(|base| source_access::paint_base_contains(base, coordinate)) {
                wanted.extend(page_coordinates(source_access::paint_base_source_bounds(base, coordinate)).map(|at| (coordinate, Input::Image(base.image.storage().clone(), at))));
            },
        }
    }
    known
}

impl LiquifySources {
    fn new(target: Option<SourceTarget>, space: RgbSpace) -> Self {
        Self { target, space, identity: None, entries: HashMap::new(), order: Vec::new(), retired: Vec::new(), missing: 0, charged: 0, reservation: None, synced: false, yielded: false }
    }
    fn reset(&mut self, target: Option<SourceTarget>, space: RgbSpace) {
        let entries = std::mem::take(&mut self.entries);
        self.retired.extend(entries.values().filter_map(Entry::read_elsewhere));
        *self = Self { retired: std::mem::take(&mut self.retired), reservation: self.reservation.take(), ..Self::new(target, space) };
        self.charged = self.retired.iter().map(|(_, bytes)| bytes).sum();
        self.shrink();
    }
    pub(crate) fn release(&mut self) -> bool {
        let held = self.reservation.as_ref().map_or(0, MemoryReservation::reserved_bytes);
        self.reset(self.target, self.space);
        self.yielded = true;
        self.reservation.as_ref().map_or(0, MemoryReservation::reserved_bytes) < held
    }
    fn shrink(&mut self) {
        if self.charged == 0 { self.reservation = None; }
        else if let Some(reservation) = self.reservation.as_ref().filter(|reservation| reservation.reserved_bytes() != self.charged) { reservation.resize(self.charged); }
    }
    fn reap(&mut self) {
        let charged = self.charged;
        self.retired.retain(|(pixels, bytes)| pixels.strong_count() != 0 || { self.charged -= bytes; false });
        if self.charged != charged { self.shrink(); }
    }
    fn filling(&self) -> bool { self.reservation.is_some() && self.missing != 0 }
    pub(crate) fn due(&self) -> bool { self.target.is_some() && !self.yielded && (!self.synced || self.filling()) }
    pub(crate) fn image(&self, source: &Arc<SourceImage>, coordinate: [u32; 2], destination: RgbSpace) -> Option<PreparedSourcePixels> {
        self.entries.get(&Leaf::Image(Arc::as_ptr(source) as usize, coordinate)).filter(|_| destination == self.space)?.pixels.clone()
    }
    pub(crate) fn raster(&self, blob: &TileBlob, space: RgbSpace, destination: RgbSpace) -> Option<PreparedSourcePixels> {
        let entry = self.entries.get(&Leaf::Raster(blob.owner_identity())).filter(|_| space == self.space && destination == self.space)?;
        matches!(&entry.input, Input::Raster(held) if held.descriptor == blob.descriptor).then(|| entry.pixels.clone()).flatten()
    }
    pub(crate) fn capture(&mut self, space: RgbSpace, prepared: &PreparedImageWork) -> bool {
        if space != self.space || self.reservation.is_none() { return false; }
        let (leaf, pixels) = match prepared {
            PreparedImageWork::Image(tile) => (Leaf::Image(Arc::as_ptr(&tile.source) as usize, tile.coordinate), &tile.pixels),
            PreparedImageWork::Raster(tile) if !tile.scalar && tile.space == space && tile.destination == space => (Leaf::Raster(tile.blob.owner_identity()), &tile.pixels),
            _ => return false,
        };
        let Some(entry) = self.entries.get_mut(&leaf).filter(|entry| match (&entry.input, prepared) {
            (Input::Raster(held), PreparedImageWork::Raster(tile)) => held.descriptor == tile.blob.descriptor, _ => true,
        }) else { return false; };
        if !entry.missing() { return entry.pixels.is_some(); }
        let before = entry.charged();
        match pixels { Ok(pixels) => entry.pixels = Some(pixels.clone()), Err(_) => entry.failed = true }
        let (released, captured) = (before - entry.charged(), entry.pixels.is_some());
        self.missing -= 1;
        self.charged -= released;
        if released != 0 { self.shrink(); }
        captured
    }
    fn select(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, drawing: bool) {
        let (scene, space, budget) = (packet.scene, r.document_color().space, r.factory.memory_budget());
        if space != self.space { self.reset(self.target, space); }
        self.synced = true;
        let Some(SourceTarget::Paint(handle)) = self.target else { return; };
        let Some(current) = scene.paint(handle) else { self.reset(self.target, space); self.synced = true; return; };
        let alpha_locked = scene.source_owner(SourceTarget::Paint(handle)).and_then(|owner| scene.occurrence(owner)).is_some_and(|occurrence| occurrence.alpha_locked);
        let original = current.raster.liquify().filter(|state| state.compatible(current, alpha_locked)).map(|state| state.original.clone());
        let identity = Identity { owner: scene.owner(), raster: current.raster.identity(), base: current.base.clone(), original: original.as_ref().map(|source| Arc::as_ptr(source) as usize) };
        if self.identity.as_ref() != Some(&identity) {
            let mut wanted = Vec::new();
            let known = [Some(current), original.as_deref()].into_iter().flatten().fold(true, |known, source| leaves(source, &mut wanted) && known);
            let centre = r.scale_display.as_ref().and_then(scene::scale::Cache::pointer).unwrap_or_else(|| {
                let shown = display_mips::view_bounds(packet.view, packet.document_extent, 0).unwrap_or(PixelRect::EMPTY);
                [(shown.min_x() + shown.max_x()) as f32 / 2., (shown.min_y() + shown.max_y()) as f32 / 2.]
            });
            wanted.sort_by_key(|(at, _)| { let [x, y] = at.map(|value| (value * PAGE_SIZE + PAGE_SIZE / 2) as f32); ((x - centre[0]).powi(2) + (y - centre[1]).powi(2)) as u64 });
            let mut entries = HashMap::with_capacity(wanted.len());
            self.order.clear();
            for (_, input) in wanted {
                let leaf = input.leaf();
                if entries.contains_key(&leaf) { continue; }
                entries.insert(leaf, self.entries.remove(&leaf).unwrap_or(Entry { input, pixels: None, requested: false, failed: false }));
                self.order.push(leaf);
            }
            let replaced = std::mem::replace(&mut self.entries, entries);
            self.retired.extend(replaced.values().filter_map(Entry::read_elsewhere));
            self.missing = self.entries.values().filter(|entry| entry.missing()).count();
            self.charged = self.entries.values().map(Entry::charged).sum::<u64>() + self.retired.iter().map(|(_, bytes)| bytes).sum::<u64>()
                + (self.entries.capacity() * std::mem::size_of::<(Leaf, Entry)>() + self.order.capacity() * std::mem::size_of::<Leaf>()) as u64;
            self.identity = known.then_some(identity);
            if self.reservation.as_ref().is_some_and(|reservation| !cover(budget, reservation, self.charged)) { self.release(); return; }
        }
        if !drawing && self.reservation.is_none() && self.missing != 0 && budget.import_headroom_bytes() >= self.charged { self.reservation = budget.admit_covered_import(self.charged); }
    }
    fn fill(&mut self, r: &mut WgpuRasterizer) {
        if !self.filling() { return; }
        let idle = r.moving_decode.as_ref().is_none_or(|(_, queue)| !queue.pending()) && r.source_results.is_empty();
        for leaf in &self.order {
            let entry = self.entries.get_mut(leaf).unwrap();
            entry.requested &= !idle;
            if !entry.missing() || entry.requested { continue; }
            let requested = match &entry.input {
                Input::Image(source, at) => source.tiles.get(at).ok_or(GpuRasterError::InvalidImage)
                    .and_then(|tile| r.request_image_decode(tile.resource_id(), source.clone(), *at, Class::Prefetch)),
                Input::Raster(blob) => r.request_raster_decode_to(blob.clone(), self.space, self.space, Class::Prefetch),
            };
            match requested {
                Ok(true) => entry.requested = true,
                Ok(false) => break,
                Err(_) => { self.charged -= entry.charged(); entry.failed = true; self.missing -= 1; },
            }
        }
        self.shrink();
    }
}

impl WgpuRasterizer {
    pub(crate) fn select_liquify_target(&mut self, target: Option<SourceTarget>) {
        let space = self.document_color().space;
        let sources = self.liquify_sources.get_or_insert_with(|| LiquifySources::new(None, space));
        if sources.target != target { sources.reset(target, space); }
    }
    pub(crate) fn prepare_liquify_sources(&mut self, packet: FramePacket<'_>) {
        let Some(mut sources) = self.liquify_sources.take() else { return; };
        sources.reap();
        let drawing = sources.target.zip(self.liquify.as_ref()).is_some_and(|(target, liquify)| liquify.drawing(target));
        if packet.dab_batches.is_empty() && !sources.yielded {
            if !drawing && self.memory.reclaim_unanswered() { sources.release(); } else { sources.select(self, packet, drawing); }
        }
        sources.fill(self);
        self.liquify_sources = Some(sources);
    }
    pub(crate) fn liquify_sources_due(&self) -> bool {
        self.liquify_sources.as_ref().is_some_and(LiquifySources::due)
            && self.moving_decode.as_ref().is_none_or(|(_, queue)| !queue.pending()) && self.source_results.is_empty()
    }
}
