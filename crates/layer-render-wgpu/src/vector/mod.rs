pub(crate) mod outline;
pub(crate) mod tiles;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod bench;

use crate::{GpuRasterError, Uploads, deferred::Deferred, pipeline_device::PipelineDevice};
use std::collections::BTreeMap;
use tiles::{ItemTile, LevelGeometry, SOLID, TILE};

pub(crate) struct Layer<'a> { pub geometry: &'a LevelGeometry, pub base: u32, pub colors: &'a [[f32; 4]] }

#[derive(Clone, Default)]
pub(crate) struct Batch {
    tiles: Vec<[i32; 4]>,
    commands: Vec<[u32; 4]>,
    colors: Vec<[f32; 4]>,
    counts: Vec<u32>,
    floors: Vec<u32>,
    starts: Vec<u32>,
    pub size: [u32; 2],
    pub origin: [i64; 2],
}
#[cfg(test)]
impl Batch {
    pub fn command_count(&self) -> usize { self.commands.len() }
    pub fn bytes(&self) -> usize { 32 + self.tiles.len() * 16 + self.commands.len() * 16 + self.colors.len() * 16 }
}
impl Batch {
    pub fn tile_count(&self) -> usize { self.tiles.len() }
    pub fn build(&mut self, layers: &[Layer], origin: [i64; 2], size: [u32; 2]) { self.build_within(layers, origin, size, None) }
    pub fn build_within(&mut self, layers: &[Layer], origin: [i64; 2], size: [u32; 2], mask: Option<&[bool]>) {
        self.tiles.clear(); self.commands.clear(); self.colors.clear();
        self.size = size;
        self.origin = origin;
        let side = i64::from(TILE);
        let t0 = [origin[0].div_euclid(side) as i32, origin[1].div_euclid(side) as i32];
        let t1 = [(origin[0] + i64::from(size[0]) + side - 1).div_euclid(side) as i32, (origin[1] + i64::from(size[1]) + side - 1).div_euclid(side) as i32];
        let columns = (t1[0] - t0[0]).max(0) as usize;
        let count = columns * (t1[1] - t0[1]).max(0) as usize;
        if count == 0 { return; }
        let slot = |tile: [i32; 2]| (tile[1] - t0[1]) as usize * columns + (tile[0] - t0[0]) as usize;
        self.counts.clear(); self.counts.resize(count, 0);
        self.floors.clear(); self.floors.resize(count, 0);
        let mut reference = 0u32;
        for layer in layers {
            for tile in layer.geometry.tile_range(t0[1]..t1[1], t0[0]..t1[0]) {
                reference += 1;
                let s = slot(tile.tile);
                if mask.is_some_and(|mask| !mask[s]) { continue; }
                if tile.kind == SOLID && layer.colors[tile.entry as usize][3] >= 1. { self.floors[s] = reference; self.counts[s] = 0; }
                self.counts[s] += 1;
            }
        }
        self.starts.clear(); self.starts.reserve(count + 1); self.starts.push(0);
        for s in 0..count { let next = self.starts[s] + self.counts[s]; self.starts.push(next); }
        self.commands.resize(self.starts[count] as usize, [0; 4]);
        let mut reference = 0u32;
        for layer in layers {
            let palette = self.colors.len() as u32;
            let mut used = false;
            for tile in layer.geometry.tile_range(t0[1]..t1[1], t0[0]..t1[0]) {
                reference += 1;
                let s = slot(tile.tile);
                if reference < self.floors[s] || mask.is_some_and(|mask| !mask[s]) { continue; }
                let at = self.starts[s] as usize;
                self.starts[s] += 1;
                self.commands[at] = command(tile, layer.base, palette);
                used = true;
            }
            if used { self.colors.extend_from_slice(layer.colors); }
        }
        let mut start = 0u32;
        for y in t0[1]..t1[1] {
            for x in t0[0]..t1[0] {
                let s = slot([x, y]);
                let end = self.starts[s];
                if end > start || mask.is_some_and(|mask| mask[s]) {
                    let offset = [i64::from(x) * side - origin[0], i64::from(y) * side - origin[1]];
                    self.tiles.push([offset[0] as i32, offset[1] as i32, start as i32, end as i32]);
                }
                start = end;
            }
        }
    }
}
fn command(tile: &ItemTile, base: u32, palette: u32) -> [u32; 4] {
    [tile.kind | tile.count << 8, base + tile.start, tile.backdrop as u32, palette + u32::from(tile.entry)]
}

/// Geometry for every layer's derived paths, uploaded from the encoder's
/// prologue.
pub(crate) struct Pool {
    buffer: Option<wgpu::Buffer>,
    capacity: u32,
    limit: u32,
    free: BTreeMap<u32, u32>,
    pending: Vec<(u32, std::sync::Arc<LevelGeometry>)>,
    retired: Vec<(u32, u32)>,
    charge: crate::residency::Charge,
}
impl Pool {
    #[cfg(test)]
    pub fn new(limit_bytes: u64) -> Self { Self::charged(limit_bytes, &Default::default()) }
    pub(crate) fn charged(limit_bytes: u64, tier: &std::sync::Arc<crate::residency::Tier>) -> Self { Self { buffer: None, capacity: 0, limit: (limit_bytes / 4).min(u64::from(u32::MAX)) as u32, free: BTreeMap::new(), pending: Vec::new(), retired: Vec::new(), charge: tier.charge(0) } }
    #[cfg(test)]
    pub fn used_words(&self) -> u32 { self.capacity - self.free.values().sum::<u32>() }
    #[cfg(test)]
    pub fn storage_bytes(&self) -> u64 { self.buffer.as_ref().map_or(0, wgpu::Buffer::size) }
    pub fn allocate(&mut self, geometry: &std::sync::Arc<LevelGeometry>) -> Option<u32> {
        let words = (geometry.words.len() as u32).max(1);
        let found = self.free.iter().find(|(_, len)| **len >= words).map(|(offset, len)| (*offset, *len));
        let offset = match found {
            Some((offset, len)) => { self.free.remove(&offset); if len > words { self.free.insert(offset + words, len - words); } offset }
            None => {
                let tail = self.free.last_key_value().filter(|(o, l)| **o + **l == self.capacity).map(|(o, _)| *o);
                let start = tail.unwrap_or(self.capacity);
                let needed = start.checked_add(words)?;
                if needed > self.limit { return None; }
                if let Some(tail) = tail { self.free.remove(&tail); }
                let grown = needed.max(self.capacity.saturating_mul(2)).max(1 << 20).min(self.limit);
                if grown > needed { self.free.insert(needed, grown - needed); }
                self.capacity = grown;
                start
            }
        };
        if !geometry.words.is_empty() { self.pending.push((offset, geometry.clone())); }
        Some(offset)
    }
    /// Free space that nothing this frame has read.
    pub fn release(&mut self, offset: u32, geometry: &LevelGeometry) { self.free_words(offset, (geometry.words.len() as u32).max(1)); }
    /// Free space this frame may have read once the next frame begins.
    pub fn retire(&mut self, offset: u32, geometry: &LevelGeometry) {
        self.retired.push((offset, (geometry.words.len() as u32).max(1)));
    }
    pub fn restart(&mut self) {
        for (offset, len) in std::mem::take(&mut self.retired) { self.free_words(offset, len); }
    }
    fn free_words(&mut self, offset: u32, len: u32) {
        let (mut offset, mut len) = (offset, len);
        if let Some((&before, &size)) = self.free.range(..offset).next_back() && before + size == offset { self.free.remove(&before); offset = before; len += size; }
        if let Some(&after) = self.free.get(&(offset + len)) { self.free.remove(&(offset + len)); len += after; }
        self.free.insert(offset, len);
        self.pending.retain(|(o, _)| *o < offset || *o >= offset + len);
    }
    pub fn flush(&mut self, device: &PipelineDevice, encoder: &mut crate::submission::CommandEncoder, uploads: &mut Uploads) -> Result<wgpu::Buffer, GpuRasterError> {
        let encoder = encoder.prologue();
        let bytes = u64::from(self.capacity.max(1024)) * 4;
        if self.buffer.as_ref().is_none_or(|b| b.size() < bytes) {
            let grown = device.create_buffer(&wgpu::BufferDescriptor { label: Some("vector geometry"), size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
            if let Some(old) = &self.buffer { encoder.copy_buffer_to_buffer(old, 0, &grown, 0, old.size()); }
            self.charge.resize(grown.size());
            self.buffer = Some(grown);
        }
        let buffer = self.buffer.clone().unwrap();
        self.pending.sort_unstable_by_key(|(o, _)| *o);
        let mut index = 0;
        while index < self.pending.len() {
            let start = self.pending[index].0;
            let mut end = index;
            let mut words = 0u32;
            while end < self.pending.len() && self.pending[end].0 == start + words { words += self.pending[end].1.words.len() as u32; end += 1; }
            let run = &self.pending[index..end];
            uploads.write_mapped(encoder, &buffer, u64::from(start) * 4, u64::from(words) * 4, |mapped| {
                let mut offset = 0usize;
                for (_, geometry) in run {
                    for chunk in geometry.words.chunks(1024) {
                        let mut block = [0u8; 4096];
                        for (i, w) in chunk.iter().enumerate() { block[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes()); }
                        mapped.slice(offset..offset + chunk.len() * 4).copy_from_slice(&block[..chunk.len() * 4]);
                        offset += chunk.len() * 4;
                    }
                }
            })?;
            index = end;
        }
        self.pending.clear();
        Ok(buffer)
    }
}

#[derive(Clone)]
pub(crate) struct Kernel {
    layout: wgpu::BindGroupLayout,
    pipeline: Deferred<wgpu::ComputePipeline>,
    resample_layout: wgpu::BindGroupLayout,
    resample: Deferred<wgpu::ComputePipeline>,
}
const ALIGNMENT: u64 = 256;
/// A frame's coverage batches, one after another in a reusable buffer that the
/// encoder's prologue fills.
#[derive(Default)]
pub(crate) struct Arena { buffer: Option<wgpu::Buffer>, used: u64, charge: Option<crate::residency::Charge> }
impl Arena {
    pub(crate) fn charged(tier: &std::sync::Arc<crate::residency::Tier>) -> Self { Self { charge: Some(tier.charge(0)), ..Default::default() } }
    pub fn restart(&mut self) { self.used = 0; }
    #[cfg(test)]
    pub fn storage_bytes(&self) -> u64 { self.buffer.as_ref().map_or(0, wgpu::Buffer::size) }
    fn place(&mut self, device: &PipelineDevice, bytes: u64) -> Result<(wgpu::Buffer, u64), GpuRasterError> {
        let limit = device.limits().max_buffer_size;
        if bytes > limit { return Err(GpuRasterError::SizeOverflow); }
        let capacity = self.buffer.as_ref().map_or(0, wgpu::Buffer::size);
        if self.used + bytes > capacity {
            let size = (capacity * 2).max(bytes).max(1 << 20).min(limit);
            if let Some(charge) = &mut self.charge { charge.resize(size); }
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor { label: Some("vector coverage batches"), size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
            self.used = 0;
        }
        let offset = self.used;
        self.used += bytes.next_multiple_of(ALIGNMENT);
        Ok((self.buffer.clone().unwrap(), offset))
    }
}
fn fill_words(mapped: &mut wgpu::BufferViewMut, start: usize, words: impl Iterator<Item = u32>) {
    let mut block = [0u8; 4096];
    let (mut filled, mut offset) = (0usize, start);
    for word in words {
        block[filled..filled + 4].copy_from_slice(&word.to_le_bytes());
        filled += 4;
        if filled == block.len() { mapped.slice(offset..offset + filled).copy_from_slice(&block); offset += filled; filled = 0; }
    }
    if filled > 0 { mapped.slice(offset..offset + filled).copy_from_slice(&block[..filled]); }
}
impl Kernel {
    pub fn new(device: &PipelineDevice) -> Self {
        let stage = wgpu::ShaderStages::COMPUTE;
        let read = wgpu::BufferBindingType::Storage { read_only: true };
        let layout = crate::bindings::layout(device, "vector coverage", &[
            crate::bindings::buffer(0, stage, wgpu::BufferBindingType::Uniform, false, wgpu::BufferSize::new(48)),
            crate::bindings::buffer(1, stage, read, false, None),
            crate::bindings::buffer(2, stage, read, false, None),
            crate::bindings::buffer(3, stage, read, false, None),
            crate::bindings::buffer(4, stage, read, false, None),
            crate::bindings::texture(5, stage, false),
            crate::bindings::storage_texture(6, stage, wgpu::TextureFormat::Rgba32Float, wgpu::StorageTextureAccess::WriteOnly),
        ]);
        let shader = Deferred::wgsl(device, "vector coverage", include_str!("coverage.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("vector coverage"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = Deferred::compute(device, "vector coverage", &pipeline_layout, &shader, "main");
        let resample_layout = crate::bindings::layout(device, "display image resample", &[
            crate::bindings::buffer(0, stage, wgpu::BufferBindingType::Uniform, false, wgpu::BufferSize::new(64)),
            crate::bindings::texture(1, stage, false),
            crate::bindings::storage_texture(2, stage, wgpu::TextureFormat::Rgba32Float, wgpu::StorageTextureAccess::WriteOnly),
        ]);
        let resample_shader = Deferred::wgsl(device, "display image resample", include_str!("resample.wgsl"));
        let resample_pipeline = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("display image resample"), bind_group_layouts: &[Some(&resample_layout)], immediate_size: 0 });
        let resample = Deferred::compute(device, "display image resample", &resample_pipeline, &resample_shader, "main");
        Self { layout, pipeline, resample_layout, resample }
    }
    pub fn pipeline(&self) -> &Deferred<wgpu::ComputePipeline> { &self.pipeline }
    pub fn pipelines(&self) -> [&Deferred<wgpu::ComputePipeline>; 2] { [&self.pipeline, &self.resample] }
}
/// A display image whose texel (0, 0) starts at document `origin` and
/// covers `side` document pixels.
pub(crate) struct Placed<'a> { pub view: &'a wgpu::TextureView, pub origin: [i64; 2], pub side: u32 }
impl Kernel {
    /// Resample `source` through `inverse` into the `texels` of `output`.
    pub fn resample(&self, device: &PipelineDevice, encoder: &mut crate::submission::CommandEncoder, source: Placed<'_>, output: Placed<'_>,
        texels: crate::pixel_rect::PixelRect, inverse: layer_core::authored::Affine64) {
        let [a, b, c, d, x, y] = inverse.0.map(|v| v as f32);
        let size = [texels.width(), texels.height()];
        let values = [a, b, c, d, x, y, output.side as f32, source.side as f32, output.origin[0] as f32, output.origin[1] as f32, source.origin[0] as f32, source.origin[1] as f32]
            .map(f32::to_bits).into_iter().chain([size[0], size[1], texels.min_x(), texels.min_y()]);
        let bytes: Vec<u8> = values.flat_map(u32::to_le_bytes).collect();
        let uniform = wgpu::util::DeviceExt::create_buffer_init(&**device, &wgpu::util::BufferInitDescriptor { label: Some("display image resample"), contents: &bytes, usage: wgpu::BufferUsages::UNIFORM });
        let binding = crate::bindings::group(device, "display image resample", &self.resample_layout, [uniform.as_entire_binding(),
            wgpu::BindingResource::TextureView(source.view), wgpu::BindingResource::TextureView(output.view)]);
        let pass = encoder.compute_pass();
        pass.set_pipeline(&self.resample);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
    }
    #[expect(clippy::too_many_arguments, reason = "One dispatch binds its batch, the frame's batches, geometry and both textures")]
    pub fn encode(&self, device: &PipelineDevice, encoder: &mut crate::submission::CommandEncoder, uploads: &mut Uploads, arena: &mut Arena, batch: &Batch, pool: &wgpu::Buffer,
        base: Option<(&wgpu::TextureView, [i32; 2])>, empty: &wgpu::TextureView, output: &wgpu::TextureView) -> Result<(), GpuRasterError> {
        if batch.tiles.is_empty() { return Ok(()); }
        let tiles = batch.tiles.len() as u32;
        let columns = tiles.min(32768);
        let rows = tiles.div_ceil(columns);
        let base_origin = base.map_or([0; 2], |(_, origin)| origin);
        let params: [u32; 12] = [batch.size[0], batch.size[1], tiles, columns, u32::from(base.is_some()), 0, batch.origin[0] as i32 as u32, batch.origin[1] as i32 as u32, base_origin[0] as u32, base_origin[1] as u32, 0, 0];
        let sizes = [48, batch.tiles.len() as u64 * 16, batch.commands.len().max(1) as u64 * 16, batch.colors.len().max(1) as u64 * 16];
        let starts: Vec<u64> = sizes.iter().scan(0, |at, size| { let start = *at; *at += size.next_multiple_of(ALIGNMENT); Some(start) }).collect();
        let total = starts[3] + sizes[3];
        let (buffer, offset) = arena.place(device, total)?;
        uploads.write_mapped(encoder.prologue(), &buffer, offset, total, |mapped| {
            fill_words(mapped, starts[0] as usize, params.into_iter());
            fill_words(mapped, starts[1] as usize, batch.tiles.iter().flat_map(|t| t.map(|v| v as u32)));
            fill_words(mapped, starts[2] as usize, batch.commands.iter().flatten().copied());
            fill_words(mapped, starts[3] as usize, batch.colors.iter().flatten().map(|v| v.to_bits()));
        })?;
        let parts = (0..4).map(|i| wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buffer, offset: offset + starts[i], size: wgpu::BufferSize::new(sizes[i]) }));
        let binding = crate::bindings::group(device, "vector coverage", &self.layout, parts
            .chain([pool.as_entire_binding(), wgpu::BindingResource::TextureView(base.map_or(empty, |(view, _)| view)), wgpu::BindingResource::TextureView(output)]).collect::<Vec<_>>());
        let pass = encoder.compute_pass();
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(columns, rows, 1);
        Ok(())
    }
}
