//! Pen-up healing ([`heal.wgsl`]). A Healing or Spot Healing stroke paints a
//! clone or a tint while the pen is down. The pass that ends it rewrites every
//! page the stroke painted as its source plus a membrane that takes on the
//! stroke's surroundings, in the same submission, so the stroke's capture and
//! its one undo step include the result, and replays heal again.
use super::*;
use wgpu::util::DeviceExt;

/// Finest-level cells at most. Strokes whose pages hold more heal at half or
/// quarter resolution, and the membrane is upsampled.
const MAX_CELLS: u64 = 1 << 22;
/// Red-black relaxation sweeps at the finest level, run a round at a time
/// within the 16 x 16 blocks of each colour.
const SWEEPS: u32 = 32;
const BLOCK: u32 = 16;
const BLOCK_SWEEPS: u32 = 8;
/// Spot Healing's candidate sources: eight directions at two distances, in
/// the order that breaks ties.
const DISTANCES: [f32; 2] = [1.25, 2.];
const DIRECTIONS: [[i32; 2]; 8] = [[1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0], [-1, -1], [0, -1], [1, -1]];
const CANDIDATES: usize = DISTANCES.len() * DIRECTIONS.len();
/// Cost added per unit of a candidate window that overlaps the stroke or
/// leaves the image: the largest difference a pixel can score.
const PENALTY: f32 = 4.;
const PARAMS_BYTES: u64 = 64;
/// A candidate's page origin, padded to 16 bytes, then its source mapping.
const CANDIDATE_BYTES: u64 = 80;
/// Spot Healing's blocks of 32 x 32 pixels, and how many a page holds.
const SCORE_BLOCK: u32 = 32;
const SCORE_BLOCKS: u64 = (PAGE_SIZE as u64 / SCORE_BLOCK as u64).pow(2);
/// Before each page's dispatch-indirect record: the best cost so far and its
/// index, padded.
const CHOICE_WORDS: u64 = 4;
/// Plane buffers kept for the next stroke; larger ones are released.
const RETAINED_BYTES: u64 = 64 << 20;
const NONE: u32 = u32::MAX;

pub(in crate::retouch_sources) struct Pipelines {
    planes: wgpu::BindGroupLayout,
    parameters: wgpu::BindGroupLayout,
    pages: wgpu::BindGroupLayout,
    candidate: wgpu::BindGroupLayout,
    chosen: wgpu::BindGroupLayout,
    apply_planes: wgpu::BindGroupLayout,
    apply_pages: wgpu::BindGroupLayout,
    seed: Deferred<wgpu::ComputePipeline>,
    score: Deferred<wgpu::ComputePipeline>,
    pull: Deferred<wgpu::ComputePipeline>,
    push: Deferred<wgpu::ComputePipeline>,
    relax: Deferred<wgpu::ComputePipeline>,
    judge: Deferred<wgpu::ComputePipeline>,
    pick: Deferred<wgpu::ComputePipeline>,
    apply: Deferred<wgpu::RenderPipeline>,
}

impl Pipelines {
    pub fn new(device: &PipelineDevice) -> Self {
        let params = |stage| bindings::buffer(0, stage, wgpu::BufferBindingType::Uniform, true, NonZeroU64::new(PARAMS_BYTES));
        let storage = |binding, stage, read_only| {
            bindings::buffer(binding, stage, wgpu::BufferBindingType::Storage { read_only }, false, None)
        };
        let compute = wgpu::ShaderStages::COMPUTE;
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let mut entries = vec![params(compute), storage(1, compute, true)];
        entries.extend((2..=4).map(|binding| storage(binding, compute, false)));
        let planes = bindings::layout(device, "heal planes", &entries);
        let textures = |range: std::ops::RangeInclusive<u32>, stage| {
            range.map(|binding| bindings::texture(binding, stage, false)).collect::<Vec<_>>()
        };
        let pages = bindings::layout(device, "heal page fields", &textures(0..=2, compute));
        let mut entries = textures(0..=7, compute);
        entries.push(bindings::buffer(8, compute, wgpu::BufferBindingType::Uniform, true, NonZeroU64::new(CANDIDATE_BYTES)));
        let candidate = bindings::layout(device, "heal candidate source", &entries);
        let chosen = bindings::layout(
            device,
            "heal chosen source",
            &[bindings::storage_texture(0, compute, wgpu::TextureFormat::Rgba32Float, wgpu::StorageTextureAccess::WriteOnly)],
        );
        let apply_planes =
            bindings::layout(device, "heal membrane", &[params(fragment), storage(1, fragment, true), storage(5, fragment, true), storage(6, fragment, true)]);
        let apply_pages = bindings::layout(device, "heal page", &textures(1..=4, fragment));
        let parameters = bindings::layout(device, "heal parameters", &[params(compute)]);
        let layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: groups, immediate_size: 0 })
        };
        let plane_layout = layout("heal planes", &[Some(&planes)]);
        let page_layout = layout("heal page fields", &[Some(&planes), Some(&pages)]);
        let score_layout = layout("heal candidate scores", &[Some(&planes), Some(&pages), Some(&candidate)]);
        let pick_layout = layout("heal chosen source", &[Some(&parameters), None, Some(&candidate), Some(&chosen)]);
        let apply_layout = layout("heal apply", &[Some(&apply_planes), Some(&apply_pages)]);
        let shader = Deferred::wgsl(device, "heal", compose_wgsl(&[&working_color::shader(device), include_str!("retouch_sample.wgsl"), include_str!("heal.wgsl")]));
        let kernel = |layout: &wgpu::PipelineLayout, entry| Deferred::compute(device, entry, layout, &shader, entry);
        let apply = {
            let (device, layout, shader) = (device.clone(), apply_layout.clone(), shader.clone());
            Deferred::pipeline(move |mode| {
                fullscreen_pipeline_recipe(mode, &device, &layout, &shader, "apply_main", None, device.working_format(), "heal apply")
            })
        };
        Self {
            seed: kernel(&page_layout, "seed"),
            score: kernel(&score_layout, "score"),
            pick: kernel(&pick_layout, "pick"),
            pull: kernel(&plane_layout, "pull"),
            push: kernel(&plane_layout, "push"),
            relax: kernel(&plane_layout, "relax"),
            judge: kernel(&plane_layout, "judge"),
            apply,
            planes,
            parameters,
            pages,
            candidate,
            chosen,
            apply_planes,
            apply_pages,
        }
    }

    pub fn all(&self) -> ([&Deferred<wgpu::RenderPipeline>; 1], [&Deferred<wgpu::ComputePipeline>; 7]) {
        ([&self.apply], [&self.seed, &self.score, &self.pull, &self.push, &self.relax, &self.judge, &self.pick])
    }
}

/// Storage kept between strokes while a retouching tool is selected.
#[derive(Default)]
pub(in crate::retouch_sources) struct Buffers {
    planes: Option<Planes>,
}

/// Each cell's value and the finest membrane, four floats a cell, and a word
/// buffer: each cell's weight, then Spot Healing's scores and choice, which
/// draws read as indirect records.
struct Planes {
    values: wgpu::Buffer,
    membrane: wgpu::Buffer,
    words: wgpu::Buffer,
}

impl Buffers {
    pub fn storage_bytes(&self) -> u64 {
        self.planes.as_ref().map_or(0, Planes::bytes)
    }
}

impl Planes {
    fn bytes(&self) -> u64 {
        self.values.size() + self.membrane.size() + self.words.size()
    }
    fn fits(&self, cells: u64, finest: u64, words: u64) -> bool {
        self.values.size() >= cells * 16 && self.membrane.size() >= finest * 16 && self.words.size() >= words * 4
    }
    fn new(device: &wgpu::Device, cells: u64, finest: u64, words: u64) -> Self {
        let buffer = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | usage,
                mapped_at_creation: false,
            })
        };
        Self {
            values: buffer("heal values", cells * 16, wgpu::BufferUsages::empty()),
            membrane: buffer("heal membrane", finest * 16, wgpu::BufferUsages::empty()),
            words: buffer("heal weights and choice", words * 4, wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST),
        }
    }
}

/// One pyramid level: square tiles of cells on a grid of tile positions.
struct Level {
    tile: u32,
    grid: [u32; 2],
    tiles: Vec<[u32; 2]>,
    base: u64,
}

impl Level {
    fn cells(&self) -> u64 {
        self.tiles.len() as u64 * u64::from(self.tile * self.tile)
    }
}

/// The pyramid over a stroke's pages, whose first level has one tile per page,
/// in `pages` order, of `PAGE_SIZE / scale` cells a side.
struct Pyramid {
    scale: u32,
    levels: Vec<Level>,
}

impl Pyramid {
    fn new(pages: &[[u32; 2]], max_binding: u64) -> Self {
        let origin = [0, 1].map(|axis| pages.iter().map(|p| p[axis]).min().unwrap());
        let span = [0, 1].map(|axis| pages.iter().map(|p| p[axis]).max().unwrap() - origin[axis] + 1);
        let mut scale = 1;
        loop {
            let pyramid = Self::scaled(pages, origin, span, scale);
            let finest = pyramid.levels[0].cells();
            if (finest <= MAX_CELLS && pyramid.cells() * 16 <= max_binding) || scale == PAGE_SIZE {
                return pyramid;
            }
            scale *= 2;
        }
    }

    fn scaled(pages: &[[u32; 2]], origin: [u32; 2], span: [u32; 2], scale: u32) -> Self {
        let mut tiles: Vec<[u32; 2]> = pages.iter().map(|p| [p[0] - origin[0], p[1] - origin[1]]).collect();
        let (mut tile, mut grid, mut base) = (PAGE_SIZE / scale, span, 0);
        let mut levels = Vec::new();
        loop {
            let level = Level { tile, grid, tiles: tiles.clone(), base };
            base += level.cells();
            levels.push(level);
            if tile == 1 && grid == [1, 1] {
                return Self { scale, levels };
            }
            if tile > 1 {
                tile /= 2;
            } else {
                grid = grid.map(|n| n.div_ceil(2));
                tiles = tiles.iter().map(|t| [t[0] / 2, t[1] / 2]).collect::<BTreeSet<_>>().into_iter().collect();
            }
        }
    }

    fn cells(&self) -> u64 {
        self.levels.iter().map(Level::cells).sum()
    }

    /// Eight words per level, then the tables and tile lists they point to.
    fn words(&self) -> Vec<u32> {
        let mut words = vec![0; self.levels.len() * 8];
        let mut shared: Option<(usize, u32, u32)> = None;
        for (index, level) in self.levels.iter().enumerate() {
            let reused = shared.filter(|(previous, ..)| {
                let previous = &self.levels[*previous];
                previous.grid == level.grid && previous.tiles == level.tiles
            });
            let (table, list) = match reused {
                Some((_, table, list)) => (table, list),
                None => {
                    let table = words.len();
                    let mut entries = vec![NONE; (level.grid[0] * level.grid[1]) as usize];
                    for (i, t) in level.tiles.iter().enumerate() {
                        entries[(t[1] * level.grid[0] + t[0]) as usize] = i as u32;
                    }
                    words.extend(entries);
                    let list = words.len();
                    words.extend(level.tiles.iter().map(|t| t[0] | t[1] << 16));
                    (table as u32, list as u32)
                }
            };
            shared = Some((index, table, list));
            words[index * 8..index * 8 + 7].copy_from_slice(&[
                level.tile,
                level.grid[0],
                level.grid[1],
                table,
                list,
                level.base as u32,
                level.tiles.len() as u32,
            ]);
        }
        words
    }
}

#[derive(Clone, Copy, Default)]
struct Params {
    level: u32,
    parity: u32,
    tile: u32,
    scale: u32,
    window: [i32; 4],
    candidate: u32,
    pages: u32,
    candidates: u32,
    flags: u32,
    scores: u32,
    choice: u32,
    windows: u32,
    boxes: u32,
}

impl Params {
    fn words(self) -> [u32; 16] {
        [
            self.level,
            self.parity,
            self.tile,
            self.scale,
            self.window[0] as u32,
            self.window[1] as u32,
            self.window[2] as u32,
            self.window[3] as u32,
            self.candidate,
            self.pages,
            self.candidates,
            self.flags,
            self.scores,
            self.choice,
            self.windows,
            self.boxes,
        ]
    }
}

/// Parameter blocks at dynamic offsets of one uniform buffer.
struct Slots {
    stride: u64,
    bytes: Vec<u8>,
}

impl Slots {
    fn words(&mut self, words: impl IntoIterator<Item = u32>) -> u32 {
        let offset = self.bytes.len();
        self.bytes.extend(words.into_iter().flat_map(u32::to_le_bytes));
        self.bytes.resize(offset + self.stride as usize, 0);
        offset as u32
    }
    fn push(&mut self, params: Params) -> u32 {
        self.words(params.words())
    }
}

/// Spot Healing's candidate offsets, in the target's pixels, with the cost
/// of each window's overlap with the stroke or reach beyond the image.
fn candidates(damage: PixelRect, window: PixelRect, extent: [u32; 2]) -> Vec<([i32; 2], f32)> {
    let size = [damage.width() as f32, damage.height() as f32];
    let area = window.area().max(1) as f32;
    DISTANCES
        .iter()
        .flat_map(|distance| DIRECTIONS.iter().map(move |direction| (distance, direction)))
        .map(|(distance, direction)| {
            let offset: [i32; 2] = std::array::from_fn(|axis| (direction[axis] as f32 * distance * size[axis]).round() as i32);
            let shifted = |v: u32, axis: usize| (i64::from(v) + i64::from(offset[axis])).clamp(0, i64::from(u32::MAX)) as u32;
            let moved = PixelRect::new(
                shifted(window.min_x(), 0),
                shifted(window.min_y(), 1),
                shifted(window.max_x(), 0),
                shifted(window.max_y(), 1),
            );
            let inside = moved.intersect(PixelRect::full(extent)).area() as f32;
            let lost = area - inside.min(area);
            let overlap = moved.intersect(damage).area() as f32;
            (offset, PENALTY * (lost + overlap) / area)
        })
        .collect()
}

impl RetouchSources {
    /// Heal the stroke `batch` ends over every page it painted.
    pub(super) fn heal(
        &mut self,
        r: &mut WgpuRasterizer,
        batch: &DabBatch,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let Some(retouch) = batch.style.retouch.clone() else {
            return Ok(());
        };
        let pages: Vec<[u32; 2]> = r
            .paint_layers
            .iter()
            .find(|l| l.id == batch.layer_id)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?
            .coverage_pages
            .iter()
            .filter(|page| page.owner == Some(batch.stroke_id))
            .map(|page| page.coordinate)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if pages.is_empty() {
            return Ok(());
        }
        let spot = batch.style.execution == BrushExecution::SpotHeal;
        let device = r.device.clone();
        let pyramid = Pyramid::new(&pages, device.limits().max_storage_buffer_binding_size);
        let finest = pyramid.levels[0].cells();
        let mut layout = pyramid.words();
        let extent = r.target_extent(batch.layer_id);
        let damage: Vec<PixelRect> = self
            .stroke
            .as_ref()
            .filter(|s| s.id == batch.stroke_id)
            .map(|s| s.damage.values().copied().collect())
            .filter(|d: &Vec<PixelRect>| !d.is_empty())
            .unwrap_or_else(|| pages.iter().map(|p| page_rect(*p)).collect());
        let bounds = damage.iter().fold(PixelRect::EMPTY, |b, d| b.union(*d));
        let margin = (bounds.width().max(bounds.height()) / 4).clamp(8, 64).max(2 * pyramid.scale);
        let windows: Vec<PixelRect> = pages
            .iter()
            .map(|page| damage.iter().fold(PixelRect::EMPTY, |w, d| w.union(d.expand(margin, extent).intersect(page_rect(*page)))))
            .collect();
        let offsets = candidates(bounds, bounds.expand(margin, extent), extent);
        let candidate_words = layout.len() as u32;
        for (offset, penalty) in &offsets {
            layout.extend([offset[0] as u32, offset[1] as u32, penalty.to_bits()]);
        }
        let window_words = layout.len() as u32;
        let cell_windows: Vec<[u32; 4]> = pages
            .iter()
            .zip(&windows)
            .map(|(page, window)| {
                let local = window.page_local(*page);
                let scale = pyramid.scale;
                [local.min_x() / scale, local.min_y() / scale, local.max_x().div_ceil(scale), local.max_y().div_ceil(scale)]
            })
            .collect();
        let running = |count: fn(&[u32; 4]) -> u32| {
            let mut total = 0;
            std::iter::once(0).chain(cell_windows.iter().map(|w| {
                total += count(w);
                total
            }))
            .collect::<Vec<u32>>()
        };
        let window_cells = running(|w| (w[2] - w[0]) * (w[3] - w[1]));
        let window_blocks = running(|w| {
            let blocks = |lo: u32, hi: u32| if hi > lo { hi.div_ceil(BLOCK) - lo / BLOCK } else { 0 };
            blocks(w[0], w[2]) * blocks(w[1], w[3])
        });
        layout.extend(cell_windows.iter().flatten().chain(&window_cells).chain(&window_blocks));
        let locals: Vec<[i32; 4]> = pages
            .iter()
            .zip(&windows)
            .map(|(page, window)| {
                let local = window.page_local(*page);
                [local.min_x(), local.min_y(), local.max_x(), local.max_y()].map(|v| v as i32)
            })
            .collect();
        let boxes: Vec<[u32; 2]> = locals
            .iter()
            .map(|w| {
                [[w[0], w[2]], [w[1], w[3]]]
                    .map(|[lo, hi]| if hi > lo { (hi as u32).div_ceil(SCORE_BLOCK) - lo as u32 / SCORE_BLOCK } else { 0 })
            })
            .collect();
        let box_words = layout.len() as u32;
        layout.extend(boxes.iter().flatten());

        let cells = pyramid.cells();
        let scores_at = cells;
        let choice_at = scores_at + if spot { 2 * pages.len() as u64 * SCORE_BLOCKS } else { 0 };
        let words = choice_at + CHOICE_WORDS + 4 * pages.len() as u64;
        let stride = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(PARAMS_BYTES);
        let mut slots = Slots { stride, bytes: Vec::new() };
        let page_count = pages.len() as u32;
        let bounded = !r.document_color().depth.is_float();
        let perceptual = batch.style.blend_space == layer_core::BlendSpace::Perceptual;
        let flags = u32::from(spot)
            | u32::from(batch.style.alpha_locked) << 1
            | u32::from(bounded) << 2
            | u32::from(perceptual) << 3;
        let base = Params {
            flags,
            scale: pyramid.scale,
            pages: page_count,
            candidates: candidate_words,
            scores: scores_at as u32,
            choice: choice_at as u32,
            windows: window_words,
            boxes: box_words,
            ..Params::default()
        };
        let judges: Vec<u32> =
            (0..if spot { CANDIDATES as u32 } else { 0 }).map(|candidate| slots.push(Params { candidate, ..base })).collect();
        let seeds: Vec<u32> = (0..page_count).map(|tile| slots.push(Params { tile, window: locals[tile as usize], ..base })).collect();
        let top = pyramid.levels.len() - 1;
        let pulls: Vec<(u32, u64)> =
            (1..=top).map(|level| (slots.push(Params { level: level as u32, ..base }), pyramid.levels[level].cells())).collect();
        let pushes: Vec<(u32, u64)> = (0..top)
            .rev()
            .map(|level| {
                let cells = if level == 0 { u64::from(*window_cells.last().unwrap()) } else { pyramid.levels[level].cells() };
                (slots.push(Params { level: level as u32, ..base }), cells)
            })
            .collect();
        let colours = [0, 1].map(|parity| slots.push(Params { parity, ..base }));
        let applies: Vec<u32> =
            (0..page_count).map(|tile| slots.push(Params { tile, window: locals[tile as usize], ..base })).collect();

        if !self.heal.planes.as_ref().is_some_and(|p| p.fits(cells, finest, words)) {
            self.heal.planes = Some(Planes::new(&device, cells, finest, words));
        }
        let layout_bytes: Vec<u8> = layout.iter().flat_map(|w| w.to_le_bytes()).collect();
        let layout_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("heal layout"),
            contents: &layout_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("heal parameters"),
            contents: &slots.bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let planes = self.heal.planes.as_ref().unwrap();
        let (membrane, choice) = (planes.membrane.clone(), planes.words.clone());
        let k = self.pipelines.heal.clone();
        let uniform = || {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &params, offset: 0, size: NonZeroU64::new(PARAMS_BYTES) })
        };
        let plane_group = bindings::group(
            &device,
            "heal planes",
            &k.planes,
            [
                uniform(),
                layout_buffer.as_entire_binding(),
                planes.values.as_entire_binding(),
                membrane.as_entire_binding(),
                choice.as_entire_binding(),
            ],
        );
        let apply_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("heal membrane"),
            layout: &k.apply_planes,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform() },
                wgpu::BindGroupEntry { binding: 1, resource: layout_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: membrane.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: choice.as_entire_binding() },
            ],
        });

        r.ensure_material_gather();
        let fields = r.material_gather.as_ref().unwrap().fields.each_ref().map(|(_, view)| view.clone());
        let at = |i: usize, scale: [f32; 2], offset: [f32; 2]| Gather {
            region: windows[i],
            scale,
            offset,
            stroke: Some((batch.stroke_id, batch.layer_id, retouch.clone())),
        };
        let flip = retouch.flip.map(|f| if f { -1. } else { 1. });
        let coverage = |r: &WgpuRasterizer, page: [u32; 2]| {
            r.paint_layers
                .iter()
                .find(|l| l.id == batch.layer_id)
                .and_then(|l| l.coverage_pages.iter().find(|p| p.coordinate == page && p.owner == Some(batch.stroke_id)))
                .map_or_else(|| r.empty_scalar_view.clone(), |p| p.active().view.clone())
        };
        let page_group = |r: &WgpuRasterizer, page: [u32; 2], found: &wgpu::TextureView, copied: &wgpu::TextureView| {
            bindings::group(
                &device,
                "heal page fields",
                &k.pages,
                [found, copied, &coverage(r, page)].map(wgpu::BindingResource::TextureView),
            )
        };
        let dispatch = |encoder: &mut crate::submission::CommandEncoder, pipeline: &wgpu::ComputePipeline, slot: u32, page: Option<&wgpu::BindGroup>, groups: u32| {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("heal"), timestamp_writes: None });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &plane_group, &[slot]);
            if let Some(page) = page {
                pass.set_bind_group(1, page, &[]);
            }
            pass.dispatch_workgroups(groups, groups, 1);
        };
        let seed_groups = (PAGE_SIZE / pyramid.scale).div_ceil(16);
        let inside: Vec<bool> = windows.iter().map(|w| !w.is_empty()).collect();
        let identity = |i| at(i, [1.; 2], [0.; 2]);
        let (found, copied): (Vec<wgpu::TextureView>, Vec<wgpu::TextureView>) = if spot {
            let chosen = |_| create_target(&device, [PAGE_SIZE; 2], wgpu::TextureFormat::Rgba32Float, "spot healing source").1;
            (pages.iter().map(|_| Page::new(r, "spot healing destination").view).collect(), pages.iter().map(chosen).collect())
        } else {
            (vec![fields[0].clone(); pages.len()], vec![fields[1].clone(); pages.len()])
        };
        if spot {
            // Candidates are judged one at a time, each laid down only while
            // it is the best so far, so the pages a candidate reads stay
            // cached from its scores to its source.
            for i in (0..pages.len()).filter(|i| inside[*i]) {
                self.encode_gather(r, &found[i], identity(i), encoder)?;
            }
            let mut uniforms = Slots { stride, bytes: Vec::new() };
            let mut mappings = Vec::with_capacity(pages.len() * CANDIDATES);
            for (offset, _) in &offsets {
                for (i, page) in pages.iter().enumerate() {
                    let origin = page.map(|v| ((v * PAGE_SIZE) as f32).to_bits());
                    let mapping = if inside[i] {
                        self.mapping(r, &at(i, [1.; 2], offset.map(|v| v as f32)))?
                    } else {
                        Mapping::fixed(false)
                    };
                    mappings.push((uniforms.words(origin.into_iter().chain([0, 0]).chain(mapping.words)), mapping));
                }
            }
            let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("spot healing candidates"),
                contents: &uniforms.bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let fields: Vec<_> = pages.iter().enumerate().map(|(i, page)| page_group(r, *page, &found[i], &r.empty_view)).collect();
            let parameters = bindings::group(&device, "spot healing parameters", &k.parameters, [uniform()]);
            let chosen: Vec<_> = copied
                .iter()
                .map(|view| bindings::group(&device, "spot healing chosen source", &k.chosen, [wgpu::BindingResource::TextureView(view)]))
                .collect();
            let candidate = |this: &mut Self, r: &mut WgpuRasterizer, index: usize, encoder: &mut crate::submission::CommandEncoder| {
                let (offset, mapping) = &mappings[index];
                let views = this.pages(r, mapping, encoder)?.0.map(|view| view.unwrap_or_else(|| r.empty_view.clone()));
                let uniform = wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &uniforms, offset: 0, size: NonZeroU64::new(CANDIDATE_BYTES) });
                let resources = views.iter().map(wgpu::BindingResource::TextureView).chain([uniform]);
                Ok::<_, GpuRasterError>((bindings::group(&device, "spot healing candidate", &k.candidate, resources), *offset))
            };
            encoder.clear_buffer(&choice, scores_at * 4, Some(2 * pages.len() as u64 * SCORE_BLOCKS * 4));
            for (c, judge) in judges.iter().enumerate() {
                for i in (0..pages.len()).filter(|i| inside[*i]) {
                    let (group, offset) = candidate(self, r, c * pages.len() + i, encoder)?;
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("spot healing score"), timestamp_writes: None });
                    pass.set_pipeline(&k.score);
                    pass.set_bind_group(0, &plane_group, &[seeds[i]]);
                    pass.set_bind_group(1, &fields[i], &[]);
                    pass.set_bind_group(2, &group, &[offset]);
                    pass.dispatch_workgroups(boxes[i][0], boxes[i][1], 1);
                }
                dispatch(encoder, &k.judge, *judge, None, 1);
                for i in (0..pages.len()).rev().filter(|i| inside[*i]) {
                    let (group, offset) = candidate(self, r, c * pages.len() + i, encoder)?;
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("spot healing source"), timestamp_writes: None });
                    pass.set_pipeline(&k.pick);
                    pass.set_bind_group(0, &parameters, &[seeds[i]]);
                    pass.set_bind_group(2, &group, &[offset]);
                    pass.set_bind_group(3, &chosen[i], &[]);
                    pass.dispatch_workgroups_indirect(&choice, (choice_at + CHOICE_WORDS + 4 * i as u64) * 4);
                }
            }
        }
        for (i, page) in pages.iter().enumerate() {
            if !spot && inside[i] {
                self.encode_gather(r, &fields[0], identity(i), encoder)?;
                self.encode_gather(r, &fields[1], at(i, flip, retouch.offset), encoder)?;
            }
            dispatch(encoder, &k.seed, seeds[i], Some(&page_group(r, *page, &found[i], &copied[i])), seed_groups);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("heal membrane"), timestamp_writes: None });
            let mut run = |pipeline: &wgpu::ComputePipeline, slot: u32, invocations: u64| {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &plane_group, &[slot]);
                pass.dispatch_workgroups(invocations.div_ceil(256) as u32, 1, 1);
            };
            for &(slot, cells) in &pulls {
                run(&k.pull, slot, cells);
            }
            for &(slot, cells) in &pushes {
                run(&k.push, slot, cells);
            }
            for _ in 0..SWEEPS / BLOCK_SWEEPS {
                for slot in colours {
                    pass.set_pipeline(&k.relax);
                    pass.set_bind_group(0, &plane_group, &[slot]);
                    pass.dispatch_workgroups(*window_blocks.last().unwrap(), 1, 1);
                }
            }
        }
        let layer = r
            .paint_layers
            .iter()
            .position(|l| l.id == batch.layer_id)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
        for (tile, page) in pages.iter().enumerate().filter(|(i, _)| inside[*i]) {
            let start = match self.stroke.as_ref().filter(|s| s.id == batch.stroke_id).and_then(|s| s.pages.get(page)) {
                Some(Some(slot)) => self.pool[*slot].view.clone(),
                _ => r.empty_view.clone(),
            };
            let Some(index) = r.paint_layers[layer].pages.iter().position(|p| p.coordinate == *page) else {
                continue;
            };
            let painted = &r.paint_layers[layer].pages[index];
            let secondary = !painted.active_secondary;
            let coverage = coverage(r, *page);
            let entries: Vec<_> = (1..)
                .zip([&copied[tile], &coverage, &painted.active().view, &start])
                .map(|(binding, view)| wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) })
                .collect();
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("heal page"), layout: &k.apply_pages, entries: &entries });
            let mut pass = encoder.color_pass("heal apply", &painted.surface(secondary).view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(&k.apply);
            pass.set_bind_group(0, &apply_group, &[applies[tile]]);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);
            r.paint_layers[layer].pages[index].active_secondary = secondary;
        }
        self.counts.heals += 1;
        if self.heal.planes.as_ref().is_some_and(|p| p.bytes() > RETAINED_BYTES) {
            self.heal.planes = None;
        }
        Ok(())
    }
}

impl WgpuRasterizer {
    /// Heal the retouching stroke `batch` ends, after its last dabs.
    pub(crate) fn encode_heal(
        &mut self,
        batch: &DabBatch,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let mut sources = self.retouch_sources();
        let result = sources.heal(self, batch, encoder);
        self.retouch = Some(sources);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(list: &[[u32; 2]]) -> Pyramid {
        Pyramid::new(list, u64::MAX)
    }

    #[test]
    fn a_pyramid_halves_its_tiles_then_its_grid_down_to_one_cell() {
        let pyramid = pages(&[[3, 4], [5, 4], [5, 6]]);
        assert_eq!(pyramid.scale, 1);
        let tiles: Vec<_> = pyramid.levels.iter().map(|l| (l.tile, l.grid, l.tiles.len())).collect();
        assert_eq!(tiles[..9].iter().map(|t| t.0).collect::<Vec<_>>(), [256, 128, 64, 32, 16, 8, 4, 2, 1]);
        assert!(tiles[..9].iter().all(|t| t.1 == [3, 3] && t.2 == 3), "{tiles:?}");
        assert_eq!(tiles[9..], [(1, [2, 2], 3), (1, [1, 1], 1)]);
        assert_eq!(pyramid.levels[1].base, 3 * 65536);
        let words = pyramid.words();
        assert_eq!(words[0..7], [256, 3, 3, 88, 97, 0, 3], "levels share one table while the grid holds");
        assert_eq!(words[88..97], [0, NONE, 1, NONE, NONE, NONE, NONE, NONE, 2]);
        assert_eq!(words[97..100], [0, 2, 2 | 2 << 16]);
        assert_eq!(words[8 * 8 + 3..8 * 8 + 5], [88, 97]);
    }

    #[test]
    fn large_strokes_heal_at_reduced_resolution() {
        let wide: Vec<_> = (0..65).map(|x| [x, 0]).collect();
        assert_eq!(pages(&wide[..64]).scale, 1, "four megapixels heal at full resolution");
        assert_eq!(pages(&wide).scale, 2);
        let huge: Vec<_> = (0..300).map(|x| [x % 20, x / 20]).collect();
        assert_eq!(pages(&huge).scale, 4);
        assert!(pages(&huge).levels[0].cells() <= MAX_CELLS);
    }

    #[test]
    fn candidates_circle_the_stroke_and_cost_what_leaves_the_image() {
        let damage = PixelRect::new(100, 100, 140, 120);
        let window = damage.expand(10, [1000, 1000]);
        let found = candidates(damage, window, [1000, 1000]);
        assert_eq!(found.len(), CANDIDATES);
        assert_eq!(found[0].0, [50, 0]);
        assert_eq!(found[3].0, [-50, 25]);
        assert_eq!(found[8].0, [80, 0]);
        assert!(found[8..].iter().all(|(_, penalty)| *penalty == 0.), "{found:?}");
        assert!(found[2].1 > 0. && found[0].1 == 0., "a window over the stroke costs: {found:?}");
        let edge = candidates(damage, window, [150, 1000]);
        assert!(edge[0].1 > 0. && edge[4].1 == 0., "a window past the image costs");
    }
}
