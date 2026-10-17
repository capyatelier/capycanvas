pub(crate) mod outline;
pub(crate) mod tiles;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod bench;

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
    pub fn build(&mut self, layers: &[Layer], origin: [i64; 2], size: [u32; 2]) {
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
                if reference < self.floors[s] { continue; }
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
                if end > start {
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

pub(crate) struct Pool {
    buffer: Option<wgpu::Buffer>,
    capacity: u32,
    limit: u32,
    free: BTreeMap<u32, u32>,
    pending: Vec<(u32, std::sync::Arc<LevelGeometry>)>,
}
impl Pool {
    pub fn new(limit_bytes: u64) -> Self { Self { buffer: None, capacity: 0, limit: (limit_bytes / 4).min(u64::from(u32::MAX)) as u32, free: BTreeMap::new(), pending: Vec::new() } }
    #[cfg(test)]
    pub fn used_words(&self) -> u32 { self.capacity - self.free.values().sum::<u32>() }
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
    pub fn release(&mut self, offset: u32, geometry: &LevelGeometry) {
        let mut offset = offset;
        let mut len = (geometry.words.len() as u32).max(1);
        if let Some((&before, &size)) = self.free.range(..offset).next_back() && before + size == offset { self.free.remove(&before); offset = before; len += size; }
        if let Some(&after) = self.free.get(&(offset + len)) { self.free.remove(&(offset + len)); len += after; }
        self.free.insert(offset, len);
        self.pending.retain(|(o, _)| *o < offset || *o >= offset + len);
    }
    pub fn flush(&mut self, device: &PipelineDevice, encoder: &mut wgpu::CommandEncoder, uploads: &mut Uploads) -> Result<wgpu::Buffer, GpuRasterError> {
        let bytes = u64::from(self.capacity.max(1024)) * 4;
        if self.buffer.as_ref().is_none_or(|b| b.size() < bytes) {
            let grown = device.create_buffer(&wgpu::BufferDescriptor { label: Some("vector geometry"), size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
            if let Some(old) = &self.buffer { encoder.copy_buffer_to_buffer(old, 0, &grown, 0, old.size()); }
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
}
const USAGES: [wgpu::BufferUsages; 4] = [wgpu::BufferUsages::UNIFORM, wgpu::BufferUsages::STORAGE, wgpu::BufferUsages::STORAGE, wgpu::BufferUsages::STORAGE];
fn write_words(uploads: &mut Uploads, encoder: &mut wgpu::CommandEncoder, buffer: &wgpu::Buffer, words: impl Iterator<Item = u32>, len: usize) -> Result<(), GpuRasterError> {
    uploads.write_mapped(encoder, buffer, 0, (len * 4) as u64, |mapped| {
        let mut block = [0u8; 4096];
        let (mut filled, mut offset) = (0usize, 0usize);
        for word in words {
            block[filled..filled + 4].copy_from_slice(&word.to_le_bytes());
            filled += 4;
            if filled == block.len() { mapped.slice(offset..offset + filled).copy_from_slice(&block); offset += filled; filled = 0; }
        }
        if filled > 0 { mapped.slice(offset..offset + filled).copy_from_slice(&block[..filled]); }
    })
}
impl Kernel {
    pub fn new(device: &PipelineDevice) -> Self {
        let stage = wgpu::ShaderStages::COMPUTE;
        let read = wgpu::BufferBindingType::Storage { read_only: true };
        let layout = crate::bindings::layout(device, "vector coverage", &[
            crate::bindings::buffer(0, stage, wgpu::BufferBindingType::Uniform, false, wgpu::BufferSize::new(32)),
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
        Self { layout, pipeline }
    }
    pub fn pipeline(&self) -> &Deferred<wgpu::ComputePipeline> { &self.pipeline }
    fn buffer(buffers: &mut [Option<wgpu::Buffer>; 4], device: &PipelineDevice, index: usize, size: u64) -> wgpu::Buffer {
        let size = size.max(32).next_power_of_two();
        if let Some(buffer) = &buffers[index] && buffer.size() >= size { return buffer.clone(); }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("vector coverage data"), size, usage: USAGES[index] | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        buffers[index] = Some(buffer.clone());
        buffer
    }
    #[expect(clippy::too_many_arguments, reason = "One dispatch binds its batch, scratch buffers, geometry and both textures")]
    pub fn encode(&self, device: &PipelineDevice, encoder: &mut wgpu::CommandEncoder, uploads: &mut Uploads, scratch: &mut [Option<wgpu::Buffer>; 4], batch: &Batch, pool: &wgpu::Buffer,
        base: Option<&wgpu::TextureView>, empty: &wgpu::TextureView, output: &wgpu::TextureView) -> Result<(), GpuRasterError> {
        if batch.tiles.is_empty() { return Ok(()); }
        let tiles = batch.tiles.len() as u32;
        let columns = tiles.min(32768);
        let rows = tiles.div_ceil(columns);
        let params: [u32; 8] = [batch.size[0], batch.size[1], tiles, columns, u32::from(base.is_some()), 0, batch.origin[0] as i32 as u32, batch.origin[1] as i32 as u32];
        let sizes = [32, batch.tiles.len() * 16, batch.commands.len() * 16, batch.colors.len().max(1) * 16];
        let buffers: Vec<wgpu::Buffer> = (0..4).map(|i| Self::buffer(scratch, device, i, sizes[i] as u64)).collect();
        write_words(uploads, encoder, &buffers[0], params.into_iter(), 8)?;
        write_words(uploads, encoder, &buffers[1], batch.tiles.iter().flat_map(|t| t.map(|v| v as u32)), batch.tiles.len() * 4)?;
        write_words(uploads, encoder, &buffers[2], batch.commands.iter().flatten().copied(), batch.commands.len() * 4)?;
        if !batch.colors.is_empty() { write_words(uploads, encoder, &buffers[3], batch.colors.iter().flatten().map(|v| v.to_bits()), batch.colors.len() * 4)?; }
        let binding = crate::bindings::group(device, "vector coverage", &self.layout, buffers.iter().map(|b| b.as_entire_binding())
            .chain([pool.as_entire_binding(), wgpu::BindingResource::TextureView(base.unwrap_or(empty)), wgpu::BindingResource::TextureView(output)]).collect::<Vec<_>>());
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("vector coverage"), timestamp_writes: None });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(columns, rows, 1);
        Ok(())
    }
}
