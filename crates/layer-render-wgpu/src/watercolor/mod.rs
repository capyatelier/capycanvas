use crate::{deferred::Deferred, submission::CommandEncoder, PipelineDevice};
use std::sync::Arc;
use wgpu::util::DeviceExt;

#[cfg(test)]
mod tests;

pub(crate) const SLOT_CELLS: u32 = 64;
pub(crate) const CELL_PIXELS: u32 = 4;
pub(crate) const TILE_PIXELS: u32 = SLOT_CELLS * CELL_PIXELS;
const ATLAS_SLOTS: u32 = 32;
const ACTIVITY: u64 = 4;
const MISSING: u32 = u32::MAX;
const GROWTH_HZ: u32 = 15;
const SOURCE: &str = include_str!("sim.wgsl");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Rate { Thirty, FortyFive, Sixty, Ninety, OneTwenty }
impl Rate {
    pub(crate) fn per_second(self) -> u32 {
        match self { Rate::Thirty => 30, Rate::FortyFive => 45, Rate::Sixty => 60, Rate::Ninety => 90, Rate::OneTwenty => 120 }
    }
    pub(crate) fn blocking(self) -> f32 {
        match self { Rate::Thirty => 0.55, Rate::FortyFive => 0.70, Rate::Sixty => 0.75, Rate::Ninety => 0.84, Rate::OneTwenty => 0.88 }
    }
    pub(crate) fn growth_interval(self) -> u32 { self.per_second() / GROWTH_HZ }
    pub(crate) fn for_display(hertz: f32) -> Self {
        if hertz >= 105. { Rate::OneTwenty } else if hertz >= 80. { Rate::Ninety } else { Rate::Sixty }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Look {
    pub spread: f32,
    pub drying: f32,
    pub wet_edge: f32,
    pub rim_seconds: f32,
    pub rewet_lift: f32,
}
impl Default for Look {
    fn default() -> Self { Self { spread: 1., drying: 1., wet_edge: 4., rim_seconds: 1. / 120., rewet_lift: 6. } }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dab {
    pub center: [f32; 2],
    pub radius: f32,
    pub water: f32,
    pub absorbance: [f32; 3],
    pub pickup: f32,
    pub flow: f32,
}
impl Dab {
    fn bytes(&self) -> [f32; 12] {
        let [x, y] = self.center;
        let [r, g, b] = self.absorbance;
        [x, y, self.radius, self.water, r, g, b, self.pickup, self.flow, 0., 0., 0.]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PaperMapping { pub grain: [f32; 4], pub axes: [f32; 4] }

#[derive(Default)]
struct Params { dt: f32, blocking: f32, evaporation: f32, deposit: f32, wet_edge: f32, rim_decay: f32, lift: f32, serial: u32, paper: PaperMapping }
impl Params {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        for value in [self.dt, self.blocking, self.evaporation, self.deposit, self.wet_edge, self.rim_decay, self.lift] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.serial.to_le_bytes());
        for value in self.paper.grain.iter().chain(&self.paper.axes) {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }
}

#[derive(Clone, Copy)]
enum Entry { FillPaper, Links, Step, Grow, Inject, Bake }
const ENTRIES: [(&str, &[Binding]); 6] = [
    ("fill_paper", &[Binding::Slots, Binding::Dispatch, Binding::Params, Binding::PaperOut, Binding::PaperSource, Binding::PaperSampler]),
    ("links", &[Binding::Slots, Binding::Dispatch, Binding::Paper, Binding::LinkOut]),
    ("step", &[Binding::Slots, Binding::Dispatch, Binding::Params, Binding::State, Binding::Pops, Binding::Deposit, Binding::Words,
        Binding::Link, Binding::StateOut, Binding::PopsOut, Binding::DepositOut, Binding::Activity]),
    ("grow", &[Binding::Slots, Binding::Dispatch, Binding::State, Binding::Words, Binding::Link, Binding::Paper, Binding::WordsOut]),
    ("inject", &[Binding::Slots, Binding::Dispatch, Binding::Params, Binding::State, Binding::Pops, Binding::Words, Binding::Paper,
        Binding::StateOut, Binding::PopsOut, Binding::WordsOut, Binding::Dabs]),
    ("bake", &[Binding::Slots, Binding::Dispatch, Binding::State, Binding::Deposit, Binding::Words, Binding::Link, Binding::Paper,
        Binding::Base, Binding::PaintOut]),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Slots, Dispatch, Params, State, Pops, Deposit, Words, Link, Paper, StateOut, PopsOut, DepositOut, WordsOut, LinkOut, PaperOut,
    Activity, Dabs, Base, PaintOut, PaperSource, PaperSampler,
}
impl Binding {
    fn number(self) -> u32 { self as u32 }
    fn entry(self) -> wgpu::BindGroupLayoutEntry {
        use crate::bindings::{buffer, sampler, storage_texture, texture, texture_of};
        let stage = wgpu::ShaderStages::COMPUTE;
        let n = self.number();
        let read = |n| buffer(n, stage, wgpu::BufferBindingType::Storage { read_only: true }, false, None);
        let write = |n, format| storage_texture(n, stage, format, wgpu::StorageTextureAccess::WriteOnly);
        match self {
            Binding::Slots | Binding::Dispatch | Binding::Dabs => read(n),
            Binding::Params => buffer(n, stage, wgpu::BufferBindingType::Uniform, false, None),
            Binding::Activity => buffer(n, stage, wgpu::BufferBindingType::Storage { read_only: false }, false, None),
            Binding::State | Binding::Pops | Binding::Deposit | Binding::Paper | Binding::Base => texture(n, stage, false),
            Binding::Words | Binding::Link => texture_of(n, stage, wgpu::TextureSampleType::Uint, wgpu::TextureViewDimension::D2),
            Binding::PaperSource => texture(n, stage, true),
            Binding::PaperSampler => sampler(n, stage, wgpu::SamplerBindingType::Filtering),
            Binding::StateOut | Binding::PopsOut | Binding::DepositOut => write(n, wgpu::TextureFormat::Rgba16Float),
            Binding::WordsOut | Binding::LinkOut => write(n, wgpu::TextureFormat::R32Uint),
            Binding::PaperOut => write(n, wgpu::TextureFormat::Rgba8Unorm),
            Binding::PaintOut => write(n, wgpu::TextureFormat::Rgba32Float),
        }
    }
}

pub(crate) struct Pipelines {
    layouts: Vec<wgpu::BindGroupLayout>,
    pipelines: Vec<Deferred<wgpu::ComputePipeline>>,
    sampler: wgpu::Sampler,
}
impl Pipelines {
    fn new(device: &PipelineDevice) -> Self {
        let module = Deferred::wgsl(device, "Watercolor simulation", SOURCE);
        let mut layouts = Vec::new();
        let mut pipelines = Vec::new();
        for (name, bindings) in ENTRIES {
            let entries: Vec<_> = bindings.iter().map(|b| b.entry()).collect();
            let layout = crate::bindings::layout(device, name, &entries);
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(name), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            pipelines.push(Deferred::compute(device, name, &pipeline_layout, &module, name));
            layouts.push(layout);
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Watercolor paper"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { layouts, pipelines, sampler }
    }
    pub(crate) async fn prepare(&self, device: &PipelineDevice) -> Result<(), String> {
        device.compilation().map_err(|e| e.to_string())?.prepare_all(&self.pipelines).await
    }
}

pub(crate) fn pipelines(device: &PipelineDevice) -> &Arc<Pipelines> {
    device.watercolor_pipelines.get_or_init(|| Arc::new(Pipelines::new(device)))
}

fn field(device: &PipelineDevice, label: &str, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

struct Fields {
    state: wgpu::Texture,
    pops: wgpu::Texture,
    deposit: wgpu::Texture,
    words: wgpu::Texture,
}
impl Fields {
    fn new(device: &PipelineDevice, cells: [u32; 2]) -> Self {
        Self {
            state: field(device, "Watercolor state", cells, wgpu::TextureFormat::Rgba16Float),
            pops: field(device, "Watercolor populations", cells, wgpu::TextureFormat::Rgba16Float),
            deposit: field(device, "Watercolor deposit", cells, wgpu::TextureFormat::Rgba16Float),
            words: field(device, "Watercolor extent", cells, wgpu::TextureFormat::R32Uint),
        }
    }
    fn textures(&self) -> [&wgpu::Texture; 4] { [&self.state, &self.pops, &self.deposit, &self.words] }
}

struct Atlas {
    rows: u32,
    fields: [Fields; 2],
    link: wgpu::Texture,
    paper: wgpu::Texture,
}
impl Atlas {
    fn new(device: &PipelineDevice, rows: u32) -> Self {
        let cells = [ATLAS_SLOTS * SLOT_CELLS, rows * SLOT_CELLS];
        Self {
            rows,
            fields: [Fields::new(device, cells), Fields::new(device, cells)],
            link: field(device, "Watercolor links", cells, wgpu::TextureFormat::R32Uint),
            paper: field(device, "Watercolor paper", [cells[0], cells[1] * CELL_PIXELS], wgpu::TextureFormat::Rgba8Unorm),
        }
    }
    fn capacity(&self) -> u32 { self.rows * ATLAS_SLOTS }
    fn textures(&self) -> Vec<&wgpu::Texture> {
        self.fields.iter().flat_map(Fields::textures).chain([&self.link, &self.paper]).collect()
    }
}

pub(crate) fn slot_origin(index: u32) -> [u32; 2] { [index % ATLAS_SLOTS * SLOT_CELLS, index / ATLAS_SLOTS * SLOT_CELLS] }

const AXIS: [[i32; 2]; 4] = [[1, 0], [0, 1], [-1, 0], [0, -1]];
const DIAGONAL: [[i32; 2]; 4] = [[1, 1], [-1, 1], [-1, -1], [1, -1]];

pub(crate) struct WetLayer {
    device: PipelineDevice,
    pipelines: Arc<Pipelines>,
    atlas: Atlas,
    tiles: std::collections::BTreeMap<[u32; 2], u32>,
    occupants: Vec<Option<[u32; 2]>>,
    generations: Vec<u32>,
    parity: usize,
    word_parity: usize,
    steps: u64,
    slots: wgpu::Buffer,
    activity: wgpu::Buffer,
    zeros: wgpu::Buffer,
    topology_changed: bool,
    links_dirty: bool,
    fresh: Vec<u32>,
}

impl WetLayer {
    pub(crate) fn new(device: &PipelineDevice) -> Self {
        let pipelines = pipelines(device).clone();
        let atlas = Atlas::new(device, 1);
        let capacity = atlas.capacity();
        Self {
            device: device.clone(),
            pipelines,
            slots: Self::slot_buffer(device, capacity),
            activity: Self::activity_buffer(device, capacity),
            zeros: device.create_buffer(&wgpu::BufferDescriptor { label: Some("Watercolor clear"), size: u64::from(SLOT_CELLS * SLOT_CELLS * 8),
                usage: wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }),
            atlas,
            tiles: Default::default(),
            occupants: vec![None; capacity as usize],
            generations: vec![0; capacity as usize],
            parity: 0,
            word_parity: 0,
            steps: 0,
            topology_changed: false,
            links_dirty: false,
            fresh: Vec::new(),
        }
    }
    fn slot_buffer(device: &PipelineDevice, capacity: u32) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some("Watercolor slots"), size: u64::from(capacity) * 48,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
    }
    fn activity_buffer(device: &PipelineDevice, capacity: u32) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some("Watercolor activity"), size: u64::from(capacity) * ACTIVITY * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
    }
    pub(crate) fn steps(&self) -> u64 { self.steps }
    pub(crate) fn tiles(&self) -> impl Iterator<Item = ([u32; 2], u32)> + '_ { self.tiles.iter().map(|(&tile, &slot)| (tile, slot)) }
    pub(crate) fn slot(&self, tile: [u32; 2]) -> Option<u32> { self.tiles.get(&tile).copied() }
    pub(crate) fn activity(&self) -> &wgpu::Buffer { &self.activity }
    pub(crate) fn reset_activity(&self, encoder: &mut CommandEncoder) { encoder.clear_buffer(&self.activity, 0, None); }
    pub(crate) fn growth_due(&self, rate: Rate) -> bool { self.steps % u64::from(rate.growth_interval()) == 0 }

    pub(crate) fn allocate(&mut self, encoder: &mut CommandEncoder, tile: [u32; 2]) -> u32 {
        if let Some(&slot) = self.tiles.get(&tile) { return slot; }
        let slot = match self.occupants.iter().position(Option::is_none) {
            Some(slot) => slot as u32,
            None => { self.grow_atlas(encoder); self.occupants.iter().position(Option::is_none).unwrap() as u32 }
        };
        self.occupants[slot as usize] = Some(tile);
        self.generations[slot as usize] += 1;
        self.tiles.insert(tile, slot);
        for texture in self.atlas.fields.iter().flat_map(Fields::textures).chain([&self.atlas.link]) {
            clear_slot(encoder, &self.zeros, texture, slot);
        }
        self.fresh.push(slot);
        self.topology_changed = true;
        slot
    }
    pub(crate) fn release(&mut self, tile: [u32; 2]) {
        if let Some(slot) = self.tiles.remove(&tile) {
            self.occupants[slot as usize] = None;
            self.topology_changed = true;
        }
    }
    fn grow_atlas(&mut self, encoder: &mut CommandEncoder) {
        let old_capacity = self.atlas.capacity();
        let atlas = Atlas::new(&self.device, self.atlas.rows * 2);
        for (from, to) in self.atlas.textures().into_iter().zip(atlas.textures()) {
            encoder.copy_texture_to_texture(from.as_image_copy(), to.as_image_copy(), from.size());
        }
        let capacity = atlas.capacity();
        let activity = Self::activity_buffer(&self.device, capacity);
        encoder.copy_buffer_to_buffer(&self.activity, 0, &activity, 0, u64::from(old_capacity) * ACTIVITY * 4);
        self.atlas = atlas;
        self.activity = activity;
        self.slots = Self::slot_buffer(&self.device, capacity);
        self.occupants.resize(capacity as usize, None);
        self.generations.resize(capacity as usize, 0);
        self.topology_changed = true;
    }
    fn neighbour(&self, tile: [u32; 2], offset: [i32; 2]) -> u32 {
        let x = i64::from(tile[0]) + i64::from(offset[0]);
        let y = i64::from(tile[1]) + i64::from(offset[1]);
        if x < 0 || y < 0 { return MISSING; }
        self.tiles.get(&[x as u32, y as u32]).copied().unwrap_or(MISSING)
    }
    fn upload_slots(&mut self, encoder: &mut CommandEncoder) {
        if !self.topology_changed { return; }
        let mut bytes = vec![0u8; self.atlas.capacity() as usize * 48];
        for (&tile, &slot) in &self.tiles {
            let mut words = [tile[0], tile[1], self.generations[slot as usize], 0, 0, 0, 0, 0, 0, 0, 0, 0];
            for (i, offset) in AXIS.iter().enumerate() { words[4 + i] = self.neighbour(tile, *offset); }
            for (i, offset) in DIAGONAL.iter().enumerate() { words[8 + i] = self.neighbour(tile, *offset); }
            let start = slot as usize * 48;
            for (i, word) in words.iter().enumerate() { bytes[start + i * 4..start + i * 4 + 4].copy_from_slice(&word.to_le_bytes()); }
        }
        let staging = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("Watercolor slot upload"), contents: &bytes, usage: wgpu::BufferUsages::COPY_SRC });
        encoder.copy_buffer_to_buffer(&staging, 0, &self.slots, 0, bytes.len() as u64);
        self.topology_changed = false;
        self.links_dirty = true;
    }
    fn list(&self, slots: &[u32]) -> wgpu::Buffer {
        let bytes: Vec<u8> = slots.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("Watercolor dispatch"), contents: &bytes, usage: wgpu::BufferUsages::STORAGE })
    }
    fn params(&self, params: Params) -> wgpu::Buffer {
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("Watercolor parameters"), contents: &params.bytes(), usage: wgpu::BufferUsages::UNIFORM })
    }
    fn active(&self) -> Vec<u32> { self.tiles.values().copied().collect() }
    fn view(texture: &wgpu::Texture) -> wgpu::TextureView { texture.create_view(&Default::default()) }

    fn dispatch(&self, encoder: &mut CommandEncoder, entry: Entry, resources: &[(Binding, wgpu::BindingResource<'_>)], groups: [u32; 3]) {
        if groups[2] == 0 { return; }
        let (name, bindings) = ENTRIES[entry as usize];
        let entries: Vec<wgpu::BindGroupEntry> = bindings.iter().map(|binding| {
            let resource = resources.iter().find(|(b, _)| b == binding).unwrap_or_else(|| panic!("{name} needs binding {}", binding.number())).1.clone();
            wgpu::BindGroupEntry { binding: binding.number(), resource }
        }).collect();
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(name), layout: &self.pipelines.layouts[entry as usize], entries: &entries });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(name), timestamp_writes: None });
        pass.set_pipeline(self.pipelines.pipelines[entry as usize].compile());
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
    }

    pub(crate) fn prepare_fresh(&mut self, encoder: &mut CommandEncoder, paper: &wgpu::TextureView, mapping: PaperMapping) {
        self.upload_slots(encoder);
        if !self.fresh.is_empty() {
            let fresh = self.list(&self.fresh);
            let params = self.params(Params { paper: mapping, ..Default::default() });
            let out = Self::view(&self.atlas.paper);
            self.dispatch(encoder, Entry::FillPaper, &[
                (Binding::Slots, self.slots.as_entire_binding()), (Binding::Dispatch, fresh.as_entire_binding()), (Binding::Params, params.as_entire_binding()),
                (Binding::PaperOut, wgpu::BindingResource::TextureView(&out)), (Binding::PaperSource, wgpu::BindingResource::TextureView(paper)),
                (Binding::PaperSampler, wgpu::BindingResource::Sampler(&self.pipelines.sampler)),
            ], [SLOT_CELLS / 16, SLOT_CELLS * CELL_PIXELS / 16, self.fresh.len() as u32]);
            self.fresh.clear();
        }
        if !self.links_dirty { return; }
        self.links_dirty = false;
        let active = self.active();
        let list = self.list(&active);
        let paper_view = Self::view(&self.atlas.paper);
        let link_out = Self::view(&self.atlas.link);
        self.dispatch(encoder, Entry::Links, &[
            (Binding::Slots, self.slots.as_entire_binding()), (Binding::Dispatch, list.as_entire_binding()),
            (Binding::Paper, wgpu::BindingResource::TextureView(&paper_view)), (Binding::LinkOut, wgpu::BindingResource::TextureView(&link_out)),
        ], [SLOT_CELLS / 16, SLOT_CELLS / 16, active.len() as u32]);
    }

    pub(crate) fn inject(&mut self, encoder: &mut CommandEncoder, dabs: &[Dab]) {
        if dabs.is_empty() { return; }
        self.upload_slots(encoder);
        let touched: Vec<u32> = self.touched(dabs);
        if touched.is_empty() { return; }
        let bytes: Vec<u8> = dabs.iter().flat_map(|d| d.bytes()).flat_map(f32::to_le_bytes).collect();
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("Watercolor dabs"), contents: &bytes, usage: wgpu::BufferUsages::STORAGE });
        let list = self.list(&touched);
        let params = self.params(Params { serial: self.steps as u32, ..Default::default() });
        let current = &self.atlas.fields[self.parity];
        let spare = &self.atlas.fields[1 - self.parity];
        let words = &self.atlas.fields[self.word_parity].words;
        let spare_words = &self.atlas.fields[1 - self.word_parity].words;
        let views = [Self::view(&current.state), Self::view(&current.pops), Self::view(words), Self::view(&self.atlas.paper),
            Self::view(&spare.state), Self::view(&spare.pops), Self::view(spare_words)];
        self.dispatch(encoder, Entry::Inject, &[
            (Binding::Slots, self.slots.as_entire_binding()), (Binding::Dispatch, list.as_entire_binding()), (Binding::Params, params.as_entire_binding()),
            (Binding::State, wgpu::BindingResource::TextureView(&views[0])), (Binding::Pops, wgpu::BindingResource::TextureView(&views[1])),
            (Binding::Words, wgpu::BindingResource::TextureView(&views[2])), (Binding::Paper, wgpu::BindingResource::TextureView(&views[3])),
            (Binding::StateOut, wgpu::BindingResource::TextureView(&views[4])), (Binding::PopsOut, wgpu::BindingResource::TextureView(&views[5])),
            (Binding::WordsOut, wgpu::BindingResource::TextureView(&views[6])), (Binding::Dabs, buffer.as_entire_binding()),
        ], [SLOT_CELLS / 16, SLOT_CELLS / 16, touched.len() as u32]);
        for &slot in &touched {
            for (source, target) in [(&spare.state, &current.state), (&spare.pops, &current.pops), (spare_words, words)] {
                copy_slot(encoder, source, target, slot);
            }
        }
    }
    fn touched(&self, dabs: &[Dab]) -> Vec<u32> {
        let mut touched = std::collections::BTreeSet::new();
        for dab in dabs {
            let reach = dab.radius * 1.07 + 4.;
            let low = [((dab.center[0] - reach) / TILE_PIXELS as f32).floor().max(0.) as u32, ((dab.center[1] - reach) / TILE_PIXELS as f32).floor().max(0.) as u32];
            let high = [((dab.center[0] + reach) / TILE_PIXELS as f32).floor().max(0.) as u32, ((dab.center[1] + reach) / TILE_PIXELS as f32).floor().max(0.) as u32];
            for y in low[1]..=high[1] {
                for x in low[0]..=high[0] {
                    if let Some(&slot) = self.tiles.get(&[x, y]) { touched.insert(slot); }
                }
            }
        }
        touched.into_iter().collect()
    }

    pub(crate) fn step(&mut self, encoder: &mut CommandEncoder, rate: Rate, look: Look, exchange: bool) {
        self.upload_slots(encoder);
        let active = self.active();
        if active.is_empty() { return; }
        let list = self.list(&active);
        let groups = [SLOT_CELLS / 16, SLOT_CELLS / 16, active.len() as u32];
        let words_in = Self::view(&self.atlas.fields[self.word_parity].words);
        let input = &self.atlas.fields[self.parity];
        let output = &self.atlas.fields[1 - self.parity];
        let state_in = Self::view(&input.state);
        let link = Self::view(&self.atlas.link);
        let grow = self.growth_due(rate);
        if grow {
            let words_out = Self::view(&self.atlas.fields[1 - self.word_parity].words);
            let paper = Self::view(&self.atlas.paper);
            self.dispatch(encoder, Entry::Grow, &[
                (Binding::Slots, self.slots.as_entire_binding()), (Binding::Dispatch, list.as_entire_binding()),
                (Binding::State, wgpu::BindingResource::TextureView(&state_in)), (Binding::Words, wgpu::BindingResource::TextureView(&words_in)),
                (Binding::Link, wgpu::BindingResource::TextureView(&link)), (Binding::Paper, wgpu::BindingResource::TextureView(&paper)),
                (Binding::WordsOut, wgpu::BindingResource::TextureView(&words_out)),
            ], groups);
        }
        let dt = 1. / rate.per_second() as f32;
        let params = self.params(Params {
            dt,
            blocking: rate.blocking() * look.spread,
            evaporation: if exchange { look.drying } else { 0. },
            deposit: if exchange { 1. } else { 0. },
            wet_edge: if exchange { look.wet_edge } else { 0. },
            rim_decay: 1. - (-dt / look.rim_seconds.max(1. / 120.)).exp(),
            lift: if exchange { look.rewet_lift } else { 0. },
            serial: self.steps as u32,
            paper: PaperMapping::default(),
        });
        let views = [Self::view(&input.pops), Self::view(&input.deposit), Self::view(&output.state), Self::view(&output.pops), Self::view(&output.deposit)];
        self.dispatch(encoder, Entry::Step, &[
            (Binding::Slots, self.slots.as_entire_binding()), (Binding::Dispatch, list.as_entire_binding()), (Binding::Params, params.as_entire_binding()),
            (Binding::State, wgpu::BindingResource::TextureView(&state_in)), (Binding::Pops, wgpu::BindingResource::TextureView(&views[0])),
            (Binding::Deposit, wgpu::BindingResource::TextureView(&views[1])), (Binding::Words, wgpu::BindingResource::TextureView(&words_in)),
            (Binding::Link, wgpu::BindingResource::TextureView(&link)), (Binding::StateOut, wgpu::BindingResource::TextureView(&views[2])),
            (Binding::PopsOut, wgpu::BindingResource::TextureView(&views[3])), (Binding::DepositOut, wgpu::BindingResource::TextureView(&views[4])),
            (Binding::Activity, self.activity.as_entire_binding()),
        ], groups);
        self.parity = 1 - self.parity;
        if grow { self.word_parity = 1 - self.word_parity; }
        self.steps += 1;
    }

    pub(crate) fn bake(&mut self, encoder: &mut CommandEncoder, tiles: &[[u32; 2]], base: &wgpu::TextureView, out: &wgpu::TextureView) {
        self.upload_slots(encoder);
        let slots: Vec<u32> = tiles.iter().map(|tile| self.tiles[tile]).collect();
        let list = self.list(&slots);
        let fields = &self.atlas.fields[self.parity];
        let views = [Self::view(&fields.state), Self::view(&fields.deposit), Self::view(&self.atlas.fields[self.word_parity].words),
            Self::view(&self.atlas.link), Self::view(&self.atlas.paper)];
        self.dispatch(encoder, Entry::Bake, &[
            (Binding::Slots, self.slots.as_entire_binding()), (Binding::Dispatch, list.as_entire_binding()),
            (Binding::State, wgpu::BindingResource::TextureView(&views[0])), (Binding::Deposit, wgpu::BindingResource::TextureView(&views[1])),
            (Binding::Words, wgpu::BindingResource::TextureView(&views[2])), (Binding::Link, wgpu::BindingResource::TextureView(&views[3])),
            (Binding::Paper, wgpu::BindingResource::TextureView(&views[4])), (Binding::Base, wgpu::BindingResource::TextureView(base)),
            (Binding::PaintOut, wgpu::BindingResource::TextureView(out)),
        ], [TILE_PIXELS / 16, TILE_PIXELS / 16, slots.len() as u32]);
    }

    pub(crate) fn fields(&self) -> [&wgpu::Texture; 4] {
        let current = &self.atlas.fields[self.parity];
        [&current.state, &current.pops, &current.deposit, &self.atlas.fields[self.word_parity].words]
    }
}

fn slot_copy(texture: &wgpu::Texture, slot: u32) -> wgpu::TexelCopyTextureInfo<'_> {
    let [x, y] = slot_origin(slot);
    wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All }
}
const SLOT_EXTENT: wgpu::Extent3d = wgpu::Extent3d { width: SLOT_CELLS, height: SLOT_CELLS, depth_or_array_layers: 1 };

fn copy_slot(encoder: &mut CommandEncoder, source: &wgpu::Texture, target: &wgpu::Texture, slot: u32) {
    encoder.copy_texture_to_texture(slot_copy(source, slot), slot_copy(target, slot), SLOT_EXTENT);
}

fn clear_slot(encoder: &mut CommandEncoder, zeros: &wgpu::Buffer, texture: &wgpu::Texture, slot: u32) {
    let row = SLOT_CELLS * texture.format().block_copy_size(None).unwrap();
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo { buffer: zeros, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(SLOT_CELLS) } },
        slot_copy(texture, slot), SLOT_EXTENT);
}
