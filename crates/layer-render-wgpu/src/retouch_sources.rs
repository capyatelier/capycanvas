//! What retouching strokes copy from, kept apart from the pages they paint.
//! Before a stroke first writes a target page, the page is copied GPU to GPU
//! as the stroke found it. A cache holds the composite of the reference layers
//! below the target. While the pen is down nothing here uploads or waits: a
//! page that isn't ready is a miss, and the stroke replays once contact ends.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[path = "heal.rs"]
mod heal;

/// Stroke-start pages kept ready while a retouching tool is selected.
const POOL_PAGES: usize = 16;
/// Cached reference pages, 1 MiB each.
pub(super) const REFERENCE_PAGES: usize = 96;
/// Reference pages captured per quiet frame before a stroke needs them.
const PREFETCH_PAGES: usize = 2;
/// Pages prefetched on each side of a focus point.
const PREFETCH_RING: i64 = 2;
/// A gather's first pixel, padded to 16 bytes, then its mapping.
const PARAMETER_BYTES: u64 = 80;

struct Page {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Page {
    fn new(r: &WgpuRasterizer, label: &'static str) -> Self {
        let (texture, view) = create_color_target(&r.device, [PAGE_SIZE; 2], label);
        Self { texture, view }
    }
}

pub(super) struct Pipelines {
    layout: wgpu::BindGroupLayout,
    gather: Deferred<wgpu::RenderPipeline>,
    copy: Deferred<wgpu::RenderPipeline>,
    heal: Arc<heal::Pipelines>,
}
impl Pipelines {
    fn new(device: &PipelineDevice) -> Self {
        let mut entries: Vec<_> =
            (0..8).map(|binding| bindings::texture(binding, wgpu::ShaderStages::FRAGMENT, false)).collect();
        entries.push(bindings::buffer(
            8,
            wgpu::ShaderStages::FRAGMENT,
            wgpu::BufferBindingType::Uniform,
            false,
            NonZeroU64::new(PARAMETER_BYTES),
        ));
        let layout = bindings::layout(device, "retouch sources", &entries);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retouch sources"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = Deferred::wgsl(
            device,
            "retouch sources",
            compose_wgsl(&[include_str!("retouch_sample.wgsl"), include_str!("retouch_sources.wgsl")]),
        );
        let pipeline = |entry: &'static str| {
            let (device, layout, shader) = (device.clone(), pipeline_layout.clone(), shader.clone());
            Deferred::pipeline(move |mode| {
                fullscreen_pipeline_recipe(mode, &device, &layout, &shader, entry, None, device.working_format(), "retouch sources")
            })
        };
        Self { layout, gather: pipeline("gather_main"), copy: pipeline("copy_main"), heal: Arc::new(heal::Pipelines::new(device)) }
    }
}

/// Work the sources did, cumulative. A capture that could block uploads
/// source pixels or waits for the GPU; it never runs during a contact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Counts {
    pub copies: u64,
    pub captures: u64,
    pub blocking: u64,
    pub misses: u64,
    pub heals: u64,
}

/// The latest retouching stroke's target pages as the stroke found them. None
/// marks a page that was empty when the stroke first painted it.
struct StrokePages {
    id: StrokeId,
    target: LayerId,
    retouch: layer_core::Retouch,
    pages: BTreeMap<[u32; 2], Option<usize>>,
    /// Bounds of the dabs the stroke laid down on each page, in the target's
    /// pixels.
    damage: BTreeMap<[u32; 2], PixelRect>,
}

/// The reference frame a cache was captured from: the member layers' raster,
/// placement and appearance. The target is not a member, so painting it keeps
/// the cache.
struct ReferenceKey {
    members: Arc<BTreeSet<LayerId>>,
    extent: [u32; 2],
    background: [f32; 4],
    layers: Vec<Layer>,
}
impl ReferenceKey {
    fn members<'a>(frame: &'a artwork::Frame, members: &'a BTreeSet<LayerId>) -> impl Iterator<Item = &'a Layer> {
        frame.layers.iter().filter(|l| members.contains(&l.id))
    }
    fn new(frame: &artwork::Frame, members: &Arc<BTreeSet<LayerId>>, extent: [u32; 2]) -> Self {
        Self {
            members: members.clone(),
            extent,
            background: frame.background,
            layers: Self::members(frame, members).cloned().collect(),
        }
    }
    fn matches(&self, frame: &artwork::Frame, members: &BTreeSet<LayerId>, extent: [u32; 2]) -> bool {
        *self.members == *members
            && self.extent == extent
            && self.background == frame.background
            && Self::members(frame, members).count() == self.layers.len()
            && Self::members(frame, members).zip(&self.layers).all(|(a, b)| artwork::same_layer(a, b))
    }
}

struct Slot {
    page: Page,
    coordinate: Option<[u32; 2]>,
    used: u64,
}

/// Composites of the reference layers below the target, by document page,
/// least recently used first out.
#[derive(Default)]
struct ReferenceCache {
    key: Option<ReferenceKey>,
    frame: Option<Arc<artwork::Frame>>,
    pages: BTreeMap<[u32; 2], usize>,
    slots: Vec<Slot>,
    clock: u64,
    failed: BTreeSet<[u32; 2]>,
}
impl ReferenceCache {
    fn validate(&mut self, frame: &artwork::Frame, members: &Arc<BTreeSet<LayerId>>, extent: [u32; 2]) {
        if self.key.as_ref().is_some_and(|key| key.matches(frame, members, extent)) {
            return;
        }
        self.key = Some(ReferenceKey::new(frame, members, extent));
        self.frame = Some(Arc::new(reference_frame(frame, members)));
        self.pages.clear();
        self.failed.clear();
        for slot in &mut self.slots {
            slot.coordinate = None;
        }
    }
    fn get(&mut self, coordinate: [u32; 2]) -> Option<usize> {
        let index = *self.pages.get(&coordinate)?;
        self.clock += 1;
        self.slots[index].used = self.clock;
        Some(index)
    }
    fn claim(&mut self, r: &WgpuRasterizer, coordinate: [u32; 2]) -> usize {
        self.clock += 1;
        let free = self.slots.iter().position(|s| s.coordinate.is_none());
        let index = match free {
            Some(index) => index,
            None if self.slots.len() < REFERENCE_PAGES => {
                self.slots.push(Slot { page: Page::new(r, "retouch reference page"), coordinate: None, used: 0 });
                self.slots.len() - 1
            }
            None => (0..self.slots.len()).min_by_key(|i| self.slots[*i].used).unwrap(),
        };
        if let Some(evicted) = self.slots[index].coordinate.replace(coordinate) {
            self.pages.remove(&evicted);
        }
        self.slots[index].used = self.clock;
        self.pages.insert(coordinate, index);
        index
    }
    fn release(&mut self, index: usize) {
        if let Some(coordinate) = self.slots[index].coordinate.take() {
            self.pages.remove(&coordinate);
        }
    }
}

/// The document frame with only `members` visible: references render without
/// the target or anything above it, over the paper only when it is a member.
fn reference_frame(frame: &artwork::Frame, members: &BTreeSet<LayerId>) -> artwork::Frame {
    let mut reference = frame.clone();
    reference.previews.clear();
    for layer in &mut reference.layers {
        layer.visible &= members.contains(&layer.id);
    }
    reference.view.background_rgba_linear = reference
        .layers
        .iter()
        .find(|l| l.kind == LayerKind::Background && l.visible)
        .map_or([0.; 4], |paper| {
            let mut color = reference.background;
            color[3] *= paper.opacity;
            color
        });
    reference
}

/// A lone, untransformed layer at full opacity is its own composite.
fn lone_layer(frame: &artwork::Frame) -> Option<LayerId> {
    let mut visible = frame.layers.iter().filter(|l| l.visible);
    let layer = visible.next()?;
    (visible.next().is_none()
        && layer.kind == LayerKind::Paint
        && layer.opacity == 1.
        && layer.mask.is_none()
        && layer.effect.is_none()
        && layer.properties.parent.is_none()
        && !layer.properties.clipped
        && layer.properties.blend == layer_core::LayerBlend::Normal
        && layer_core::target_transform(&frame.layers, layer.id) == layer_core::Affine::IDENTITY)
        .then_some(layer.id)
}

/// Pages within `PREFETCH_RING` of each point, nearest rings first.
fn rings(points: &[layer_core::Point], extent: [u32; 2]) -> Vec<[u32; 2]> {
    let pages = extent.map(|n| i64::from(n.div_ceil(PAGE_SIZE)));
    let mut ordered = Vec::new();
    for radius in 0..=PREFETCH_RING {
        for point in points {
            let center = [point.x, point.y].map(|v| (v / PAGE_SIZE as f32).floor() as i64);
            for y in center[1] - radius..=center[1] + radius {
                for x in center[0] - radius..=center[0] + radius {
                    let ring = (x - center[0]).abs().max((y - center[1]).abs()) == radius;
                    let coordinate = [x as u32, y as u32];
                    if ring && (0..pages[0]).contains(&x) && (0..pages[1]).contains(&y) && !ordered.contains(&coordinate) {
                        ordered.push(coordinate);
                    }
                }
            }
        }
    }
    ordered.truncate(REFERENCE_PAGES / 2);
    ordered
}

/// A region of the target to sample, in the target's pixels, and the source
/// point each destination pixel reads: `scale * destination + offset`. A
/// stroke's own gather names it, with its target and source; otherwise the
/// latest retouching stroke or the prepared tool is sampled.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Gather {
    pub region: PixelRect,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
    pub stroke: Option<(StrokeId, LayerId, layer_core::Retouch)>,
}

/// What `retouch_sample.wgsl` reads for a gather: sixteen words that map its
/// destination pixels to the source, and the 2x2 blocks of target and
/// reference pages they address.
pub(super) struct Mapping {
    pub words: [u32; 16],
    mode: Mode,
    target: LayerId,
    stroke: Option<StrokeId>,
    references: Option<Arc<BTreeSet<LayerId>>>,
    blocks: [[i64; 2]; 2],
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum Mode {
    None = 0,
    Target = 1,
    References = 2,
    Tint = 3,
}

impl Mapping {
    /// A source that reads no pages: nothing, or Spot Healing's live tint.
    pub fn fixed(tint: bool) -> Self {
        let mode = if tint { Mode::Tint } else { Mode::None };
        let mut words = [0; 16];
        words[13] = mode as u32;
        Self { words, mode, target: LayerId(0), stroke: None, references: None, blocks: [[0; 2]; 2] }
    }
}

pub(super) struct RetouchSources {
    pipelines: Pipelines,
    parameters: wgpu::Buffer,
    prepared: Option<layer_render::RetouchPreparation>,
    stroke: Option<StrokePages>,
    /// The retouching stroke whose contact continues past the latest frame.
    live: Option<StrokeId>,
    pool: Vec<Page>,
    free: Vec<usize>,
    cache: ReferenceCache,
    capture: artwork::Capture,
    missed: Option<StrokeId>,
    reported: Option<StrokeId>,
    view: Option<layer_render::ViewState>,
    pending: bool,
    heal: heal::Buffers,
    pub counts: Counts,
}

impl RetouchSources {
    fn new(r: &WgpuRasterizer) -> Self {
        Self {
            pipelines: Pipelines::new(&r.device),
            parameters: r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("retouch source mapping"),
                size: PARAMETER_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            prepared: None,
            stroke: None,
            live: None,
            pool: Vec::new(),
            free: Vec::new(),
            cache: ReferenceCache::default(),
            capture: artwork::Capture::default(),
            missed: None,
            reported: None,
            view: None,
            pending: false,
            heal: heal::Buffers::default(),
            counts: Counts::default(),
        }
    }

    /// The pipelines retouching strokes draw with, from pen-down to pen-up.
    pub fn pipelines(&self) -> (Vec<Deferred<wgpu::RenderPipeline>>, Vec<Deferred<wgpu::ComputePipeline>>) {
        let (render, compute) = self.pipelines.heal.all();
        ([&self.pipelines.gather, &self.pipelines.copy].into_iter().chain(render).cloned().collect(), compute.into_iter().cloned().collect())
    }

    pub fn prepared(&self) -> bool {
        self.prepared.is_some()
    }

    pub fn pending(&self) -> bool {
        self.pending
    }

    pub fn storage_bytes(&self) -> u64 {
        self.pool.iter().chain(self.cache.slots.iter().map(|s| &s.page)).map(|p| texture_bytes(&p.texture)).sum::<u64>()
            + self.capture.storage_bytes()
            + self.heal.storage_bytes()
            + PARAMETER_BYTES
    }

    #[cfg(test)]
    pub fn cached_pages(&self) -> usize {
        self.cache.pages.len()
    }

    fn pool_page(&mut self, r: &WgpuRasterizer) -> usize {
        self.free.pop().unwrap_or_else(|| {
            self.pool.push(Page::new(r, "retouch stroke-start page"));
            self.pool.len() - 1
        })
    }

    fn release_stroke(&mut self) {
        if let Some(stroke) = self.stroke.take() {
            self.free.extend(stroke.pages.into_values().flatten());
        }
    }

    /// Track which retouching stroke is in contact: one whose batches arrived
    /// without its end. A rebuild without it ends it, as a cancel does; any
    /// other frame without batches changes nothing.
    fn note_batches(&mut self, batches: &[DabBatch], reset: bool) {
        let Some(batch) = batches
            .iter()
            .find(|b| b.style.retouch.is_some() && !matches!(b.kind, DabBatchKind::LayerOperation(_)))
        else {
            if reset {
                self.live = None;
            }
            return;
        };
        let ended = batches.iter().any(|b| b.stroke_id == batch.stroke_id && b.stroke_end);
        self.live = (!ended).then_some(batch.stroke_id);
    }

    fn miss(&mut self) {
        self.counts.misses += 1;
        if let Some(stroke) = self.live
            && self.reported != Some(stroke)
        {
            self.missed = Some(stroke);
        }
    }

    pub fn take_miss(&mut self) -> Option<StrokeId> {
        let stroke = self.missed.take()?;
        self.reported = Some(stroke);
        Some(stroke)
    }

    pub fn retire_stroke(&mut self) {
        if self.live.is_none() {
            self.release_stroke();
        }
    }

    /// Keep target page `coordinate` as the stroke found it, before this
    /// frame's persistent batches write it.
    fn keep_page(
        &mut self,
        r: &WgpuRasterizer,
        layer: LayerId,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) {
        if self.stroke.as_ref().is_none_or(|s| s.pages.contains_key(&coordinate)) {
            return;
        }
        let page = r
            .paint_layers
            .iter()
            .find(|l| l.id == layer)
            .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate));
        let original = r.native_color_tile(layer, coordinate).is_ok_and(|tile| tile.is_some())
            || r.tiled_sources.get(&layer).is_some_and(|s| {
                coordinate[0] * PAGE_SIZE < s.extent[0] && coordinate[1] * PAGE_SIZE < s.extent[1]
            });
        let copy = page.filter(|page| !page.primary_needs_clear || original).map(|page| {
            let slot = self.pool_page(r);
            let source = &page.active().texture;
            encoder.copy_texture_to_texture(source.as_image_copy(), self.pool[slot].texture.as_image_copy(), source.size());
            self.counts.copies += 1;
            slot
        });
        self.stroke.as_mut().unwrap().pages.insert(coordinate, copy);
    }

    /// Capture reference page `coordinate` into the cache. Without `wait`, a
    /// capture that would upload or wait is skipped.
    fn capture_reference(
        &mut self,
        r: &mut WgpuRasterizer,
        coordinate: [u32; 2],
        wait: bool,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<usize>, GpuRasterError> {
        let frame = self.cache.frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
        let extent = r.document_extent;
        let region = page_rect(coordinate).intersect(PixelRect::full(extent));
        if region.is_empty() {
            return Ok(None);
        }
        if let Some(layer) = lone_layer(&frame) {
            return self.copy_lone_layer(r, layer, coordinate, wait, encoder);
        }
        let packet = frame.packet(extent);
        let blocking = self.capture.would_block(r, packet, region);
        if blocking && !wait {
            return Ok(None);
        }
        let slot = self.cache.claim(r, coordinate);
        let texture = self.cache.slots[slot].page.texture.clone();
        if let Err(error) = self.capture.region_into(r, packet, region, &texture, encoder) {
            self.cache.release(slot);
            return Err(error);
        }
        self.counts.captures += 1;
        self.counts.blocking += u64::from(blocking);
        Ok(Some(slot))
    }

    fn copy_lone_layer(
        &mut self,
        r: &mut WgpuRasterizer,
        layer: LayerId,
        coordinate: [u32; 2],
        wait: bool,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<usize>, GpuRasterError> {
        let resident = r
            .paint_layers
            .iter()
            .find(|l| l.id == layer)
            .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate))
            .map(|p| p.active().texture.clone());
        if let Some(texture) = resident {
            let slot = self.cache.claim(r, coordinate);
            encoder.copy_texture_to_texture(
                texture.as_image_copy(),
                self.cache.slots[slot].page.texture.as_image_copy(),
                texture.size(),
            );
            self.counts.captures += 1;
            return Ok(Some(slot));
        }
        let prepared = raw_prepared_view(r, layer, coordinate);
        let view = match prepared {
            Prepared::View(view) => Some(view),
            Prepared::Absent => None,
            Prepared::Decode if !wait => return Ok(None),
            Prepared::Decode => {
                self.counts.blocking += 1;
                r.raw_layer_tile(layer, coordinate, encoder)?.map(|tile| tile.view)
            }
        };
        let slot = self.cache.claim(r, coordinate);
        let destination = self.cache.slots[slot].page.view.clone();
        match view {
            Some(view) => {
                let mut sources = [&r.empty_view; 8];
                sources[0] = &view;
                self.draw(r, &self.pipelines.copy, &destination, [PAGE_SIZE; 2], sources, encoder)
            }
            None => drop(encoder.color_pass("retouch transparent reference", &destination, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT))),
        }
        self.counts.captures += 1;
        Ok(Some(slot))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        r: &WgpuRasterizer,
        pipeline: &wgpu::RenderPipeline,
        destination: &wgpu::TextureView,
        size: [u32; 2],
        sources: [&wgpu::TextureView; 8],
        encoder: &mut crate::submission::CommandEncoder,
    ) {
        let binding = bindings::group(
            &r.device,
            "retouch sources",
            &self.pipelines.layout,
            sources
                .into_iter()
                .map(wgpu::BindingResource::TextureView)
                .chain([self.parameters.as_entire_binding()]),
        );
        let mut pass = encoder.color_pass("retouch sources", destination, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        pass.set_scissor_rect(0, 0, size[0], size[1]);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &binding, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The target page as `stroke` found it: its stroke-start copy, or the
    /// live page when the stroke hasn't written it.
    fn target_page(
        &mut self,
        r: &mut WgpuRasterizer,
        target: LayerId,
        stroke: Option<StrokeId>,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<wgpu::TextureView>, GpuRasterError> {
        if let Some(pages) = self.stroke.as_ref().filter(|s| s.target == target && stroke.is_none_or(|id| id == s.id))
            && let Some(copy) = pages.pages.get(&coordinate)
        {
            return Ok(copy.map(|slot| self.pool[slot].view.clone()));
        }
        if let Some(page) = r
            .paint_layers
            .iter()
            .find(|l| l.id == target)
            .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate))
        {
            return Ok(Some(page.active().view.clone()));
        }
        match raw_prepared_view(r, target, coordinate) {
            Prepared::View(view) => Ok(Some(view)),
            Prepared::Absent => Ok(None),
            Prepared::Decode if self.live.is_some() => {
                self.miss();
                Ok(None)
            }
            Prepared::Decode => {
                self.counts.blocking += 1;
                Ok(r.raw_layer_tile(target, coordinate, encoder)?.map(|tile| tile.view))
            }
        }
    }

    fn reference_page(
        &mut self,
        r: &mut WgpuRasterizer,
        coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<wgpu::TextureView>, GpuRasterError> {
        if page_rect(coordinate).intersect(PixelRect::full(r.document_extent)).is_empty() {
            return Ok(None);
        }
        if let Some(slot) = self.cache.get(coordinate) {
            return Ok(Some(self.cache.slots[slot].page.view.clone()));
        }
        if self.live.is_some()
            && let Some(layer) = self.cache.frame.as_deref().and_then(lone_layer)
        {
            return Ok(match lone_page(r, layer, coordinate) {
                Prepared::View(view) => Some(view),
                Prepared::Absent => None,
                Prepared::Decode => {
                    self.miss();
                    None
                }
            });
        }
        let slot = self.capture_reference(r, coordinate, self.live.is_none(), encoder)?;
        if slot.is_none() && self.live.is_some() {
            self.miss();
        }
        Ok(slot.map(|slot| self.cache.slots[slot].page.view.clone()))
    }

    /// Map `gather`'s destination pixels to the target as the latest
    /// retouching stroke found it, over the reference composite below it.
    pub fn mapping(&self, r: &WgpuRasterizer, gather: &Gather) -> Result<Mapping, GpuRasterError> {
        let (target, retouch, stroke) = match (&gather.stroke, &self.stroke, &self.prepared) {
            (Some((stroke, target, retouch)), ..) => (*target, retouch, Some(*stroke)),
            (None, Some(stroke), _) => (stroke.target, &stroke.retouch, None),
            (None, None, Some(prepared)) => (prepared.target, &prepared.retouch, None),
            (None, None, None) => return Err(GpuRasterError::MissingPaintLayer(LayerId(0))),
        };
        let frame = r.artwork_frame.as_ref().ok_or(GpuRasterError::InvalidExtent)?;
        let layer_core::Affine([a, b, c, d, tx, ty]) = layer_core::target_transform(&frame.layers, target);
        let region = gather.region;
        if [a, b, c, d] != [1., 0., 0., 1.] || region.is_empty() || region.width().max(region.height()) > PAGE_SIZE {
            return Err(GpuRasterError::InvalidTransform("Retouch sources need an unrotated, unscaled layer"));
        }
        let references = (retouch.source == layer_core::RetouchSource::References && !retouch.references.is_empty())
            .then(|| retouch.references.clone());
        let ends = [[region.min_x(), region.max_x()], [region.min_y(), region.max_y()]];
        let block = |shift: [f32; 2]| {
            std::array::from_fn::<i64, 2, _>(|axis| {
                let [lo, hi] = ends[axis].map(|v| v as f32);
                let texels = [lo + 0.5, hi - 0.5].map(|p| gather.scale[axis] * p + gather.offset[axis] + shift[axis] - 0.5);
                (texels[0].min(texels[1]).floor() as i64).div_euclid(i64::from(PAGE_SIZE))
            })
        };
        let blocks = [block([0.; 2]), block([tx, ty])];
        let whole = |values: [f32; 2]| values.iter().all(|v| v.fract() == 0.);
        let exact = gather.scale.iter().all(|s| s.abs() == 1.)
            && whole(gather.offset)
            && (references.is_none() || whole([gather.offset[0] + tx, gather.offset[1] + ty]));
        let opacity = frame.layers.iter().find(|l| l.id == target).map_or(1., |l| l.opacity);
        let page = i64::from(PAGE_SIZE);
        let mode = if references.is_some() { Mode::References } else { Mode::Target };
        let mut words = [0u32; 16];
        for (word, value) in words.iter_mut().zip([gather.scale[0], gather.scale[1], gather.offset[0], gather.offset[1], tx, ty]) {
            *word = value.to_bits();
        }
        for (word, value) in words[6..12].iter_mut().zip([
            i64::from(r.document_extent[0]),
            i64::from(r.document_extent[1]),
            blocks[0][0] * page,
            blocks[0][1] * page,
            blocks[1][0] * page,
            blocks[1][1] * page,
        ]) {
            *word = (value as i32) as u32;
        }
        words[12] = opacity.to_bits();
        words[13] = mode as u32;
        words[14] = u32::from(exact);
        Ok(Mapping { words, mode, target, stroke, references, blocks })
    }

    /// The target and reference pages `mapping` reads, 2x2 each, and whether
    /// every page it needed was ready.
    pub fn pages(
        &mut self,
        r: &mut WgpuRasterizer,
        mapping: &Mapping,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<([Option<wgpu::TextureView>; 8], bool), GpuRasterError> {
        let mut views: [Option<wgpu::TextureView>; 8] = Default::default();
        if !matches!(mapping.mode, Mode::Target | Mode::References) {
            return Ok((views, true));
        }
        if let Some(references) = &mapping.references {
            let frame = r.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
            self.cache.validate(&frame, references, r.document_extent);
        }
        let misses = self.counts.misses;
        let [targets, references] = mapping.blocks.map(block_pages);
        for i in 0..4 {
            if let Some(coordinate) = targets[i] {
                views[i] = self.target_page(r, mapping.target, mapping.stroke, coordinate, encoder)?;
            }
            if mapping.references.is_some()
                && let Some(coordinate) = references[i]
            {
                views[4 + i] = self.reference_page(r, coordinate, encoder)?;
            }
        }
        Ok((views, self.counts.misses == misses))
    }

    /// Whether `pages` would read `mapping` without capturing or decoding a
    /// page, which could evict one that work encoded earlier still reads.
    pub fn resident(&self, r: &WgpuRasterizer, mapping: &Mapping) -> bool {
        if !matches!(mapping.mode, Mode::Target | Mode::References) {
            return true;
        }
        let stroke = self.stroke.as_ref().filter(|s| s.target == mapping.target && mapping.stroke.is_none_or(|id| id == s.id));
        let target = block_pages(mapping.blocks[0]).into_iter().flatten().all(|coordinate| {
            stroke.is_some_and(|s| s.pages.contains_key(&coordinate))
                || r.paint_layers
                    .iter()
                    .any(|l| l.id == mapping.target && l.pages.iter().any(|p| p.coordinate == coordinate))
                || !matches!(raw_prepared_view(r, mapping.target, coordinate), Prepared::Decode)
        });
        let lone = self.live.is_some() && self.cache.frame.as_deref().and_then(lone_layer).is_some();
        let references = mapping.references.as_ref().is_none_or(|members| {
            r.artwork_frame.as_ref().is_some_and(|frame| {
                self.cache.key.as_ref().is_some_and(|key| key.matches(frame, members, r.document_extent))
            }) && block_pages(mapping.blocks[1]).into_iter().flatten().all(|coordinate| {
                lone || page_rect(coordinate).intersect(PixelRect::full(r.document_extent)).is_empty()
                    || self.cache.pages.contains_key(&coordinate)
            })
        });
        target && references
    }

    /// Draw the source of `gather` into `destination`. Returns whether every
    /// page it needed was ready.
    fn encode_gather(
        &mut self,
        r: &mut WgpuRasterizer,
        destination: &wgpu::TextureView,
        gather: Gather,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<bool, GpuRasterError> {
        let mapping = self.mapping(r, &gather)?;
        let (views, complete) = self.pages(r, &mapping, encoder)?;
        let region = gather.region;
        let origin = [region.min_x() as f32, region.min_y() as f32].map(f32::to_bits);
        let bytes: Vec<u8> =
            origin.into_iter().chain([0, 0]).chain(mapping.words).flat_map(u32::to_le_bytes).collect();
        r.uploads.write(encoder, &self.parameters, &bytes)?;
        let views = views.map(|view| view.unwrap_or_else(|| r.empty_view.clone()));
        self.draw(r, &self.pipelines.gather, destination, [region.width(), region.height()], views.each_ref(), encoder);
        Ok(complete)
    }

    fn prefetch(
        &mut self,
        r: &mut WgpuRasterizer,
        frame: &artwork::Frame,
        quiet: bool,
        encoder: &mut crate::submission::CommandEncoder,
    ) {
        self.pending = false;
        let Some(prepared) = self.prepared.as_ref().filter(|p| {
            p.retouch.source == layer_core::RetouchSource::References && !p.retouch.references.is_empty()
        }) else {
            return;
        };
        if self.live.is_some() {
            return;
        }
        let members = prepared.retouch.references.clone();
        self.cache.validate(frame, &members, r.document_extent);
        let wanted: Vec<_> = rings(&prepared.points, r.document_extent)
            .into_iter()
            .filter(|c| !self.cache.pages.contains_key(c) && !self.cache.failed.contains(c))
            .collect();
        for &coordinate in wanted.iter().take(if quiet { PREFETCH_PAGES } else { 0 }) {
            if !matches!(self.capture_reference(r, coordinate, true, encoder), Ok(Some(_))) {
                self.cache.failed.insert(coordinate);
            }
        }
        self.pending = wanted
            .iter()
            .any(|c| !self.cache.pages.contains_key(c) && !self.cache.failed.contains(c));
        if !self.pending {
            self.capture = artwork::Capture::default();
        }
    }
}

/// The 2x2 pages of the block whose first page is `block`, where they exist.
fn block_pages(block: [i64; 2]) -> [Option<[u32; 2]>; 4] {
    [[0, 0], [1, 0], [0, 1], [1, 1]].map(|[dx, dy]| {
        let [x, y] = [block[0] + dx, block[1] + dy];
        (x >= 0 && y >= 0 && x < i64::from(u32::MAX / PAGE_SIZE) && y < i64::from(u32::MAX / PAGE_SIZE))
            .then_some([x as u32, y as u32])
    })
}

enum Prepared {
    View(wgpu::TextureView),
    Absent,
    Decode,
}

/// A lone reference layer's page as the renderer holds it: painted, or an
/// original or backed tile it already decoded.
fn lone_page(r: &WgpuRasterizer, layer: LayerId, coordinate: [u32; 2]) -> Prepared {
    r.paint_layers
        .iter()
        .find(|l| l.id == layer)
        .and_then(|l| l.pages.iter().find(|p| p.coordinate == coordinate))
        .map_or_else(|| raw_prepared_view(r, layer, coordinate), |page| Prepared::View(page.active().view.clone()))
}

/// A layer page the renderer can read without uploading: an original or
/// backed tile it already decoded. `Decode` when reading needs an upload.
fn raw_prepared_view(r: &WgpuRasterizer, layer: LayerId, coordinate: [u32; 2]) -> Prepared {
    let scene = r.scene.as_ref();
    match r.native_color_tile(layer, coordinate) {
        Ok(Some(blob)) => {
            return scene
                .and_then(|s| s.prepared_raster_view(&blob, r.document_color().space))
                .map_or(Prepared::Decode, |view| Prepared::View(view.clone()));
        }
        Err(_) => return Prepared::Decode,
        Ok(None) => {}
    }
    match r.tiled_sources.get(&layer) {
        Some(source)
            if coordinate[0] * PAGE_SIZE < source.extent[0] && coordinate[1] * PAGE_SIZE < source.extent[1] =>
        {
            scene
                .and_then(|s| s.prepared_source_view(source, coordinate))
                .map_or(Prepared::Decode, |view| Prepared::View(view.clone()))
        }
        _ => Prepared::Absent,
    }
}

impl WgpuRasterizer {
    fn retouch_sources(&mut self) -> Box<RetouchSources> {
        self.retouch.take().unwrap_or_else(|| Box::new(RetouchSources::new(self)))
    }

    /// Select or release retouching: prepare the stroke-start pool and warm
    /// the source pipelines before pen-down, or free every retouch page.
    pub(super) fn prepare_retouch_sources(&mut self, prepared: Option<&layer_render::RetouchPreparation>) {
        let Some(prepared) = prepared else {
            self.retouch = None;
            return;
        };
        let mut retouch = self.retouch_sources();
        retouch.pending = prepared.retouch.source == layer_core::RetouchSource::References
            && !prepared.retouch.references.is_empty();
        retouch.prepared = Some(prepared.clone());
        while retouch.pool.len() < POOL_PAGES {
            retouch.pool.push(Page::new(self, "retouch stroke-start page"));
            retouch.free.push(retouch.pool.len() - 1);
        }
        if let Some(startup) = &mut self.startup {
            let (render, compute) = retouch.pipelines();
            startup.require_brush(&render, &compute);
        }
        self.retouch = Some(retouch);
    }

    pub(super) fn note_retouch_batches(&mut self, batches: &[DabBatch], reset: bool) {
        if let Some(retouch) = &mut self.retouch {
            retouch.note_batches(batches, reset);
        } else if batches.iter().any(|b| b.style.retouch.is_some()) {
            let mut retouch = self.retouch_sources();
            retouch.note_batches(batches, reset);
            self.retouch = Some(retouch);
        }
    }

    /// Copy each target page a persistent retouching batch is about to write
    /// for the first time in its stroke. A stroke's first batch starts over,
    /// so a replay copies the restored pages again.
    pub(super) fn keep_stroke_start_pages(
        &mut self,
        batches: &[DabBatch],
        tiles: &[Vec<BrushTile>],
        encoder: &mut crate::submission::CommandEncoder,
    ) {
        let mut retouching = batches
            .iter()
            .zip(tiles)
            .filter(|(b, _)| b.kind == DabBatchKind::Persistent && b.style.retouch.is_some())
            .peekable();
        let Some((first, _)) = retouching.peek() else {
            return;
        };
        let mut retouch = self.retouch_sources();
        if first.stroke_start
            || retouch.stroke.as_ref().is_none_or(|s| s.id != first.stroke_id || s.target != first.layer_id)
        {
            retouch.release_stroke();
            retouch.stroke = Some(StrokePages {
                id: first.stroke_id,
                target: first.layer_id,
                retouch: first.style.retouch.clone().unwrap(),
                pages: BTreeMap::new(),
                damage: BTreeMap::new(),
            });
        }
        for (batch, tiles) in retouching {
            for tile in tiles {
                retouch.keep_page(self, batch.layer_id, tile.coordinate, encoder);
                let [x, y] = tile.coordinate.map(|v| v * PAGE_SIZE);
                let local = tile.local;
                let dabs = PixelRect::new(x + local.min_x(), y + local.min_y(), x + local.max_x(), y + local.max_y());
                let damage = retouch.stroke.as_mut().unwrap().damage.entry(tile.coordinate).or_default();
                *damage = damage.union(dabs);
            }
        }
        self.retouch = Some(retouch);
    }

    pub(super) fn prefetch_retouch(
        &mut self,
        frame: &artwork::Frame,
        moving: bool,
        encoder: &mut crate::submission::CommandEncoder,
    ) {
        let Some(mut retouch) = self.retouch.take() else {
            return;
        };
        let quiet = !moving && retouch.view == Some(frame.view);
        retouch.view = Some(frame.view);
        retouch.prefetch(self, frame, quiet, encoder);
        self.retouch = Some(retouch);
    }

    /// Map what the retouching batch copies onto the `local` part of page
    /// `coordinate`. Spot Healing lays a translucent tint until pen-up finds
    /// its source; a batch without a source copies nothing.
    pub(super) fn retouch_mapping(
        &mut self,
        batch: &DabBatch,
        coordinate: [u32; 2],
        local: PixelRect,
    ) -> Result<Mapping, GpuRasterError> {
        let [x, y] = coordinate.map(|v| v * PAGE_SIZE);
        let region = PixelRect::new(x + local.min_x(), y + local.min_y(), x + local.max_x(), y + local.max_y());
        match &batch.style.retouch {
            _ if batch.style.execution == BrushExecution::SpotHeal => Ok(Mapping::fixed(true)),
            Some(retouch) if !region.is_empty() => {
                let gather = Gather {
                    region,
                    scale: retouch.flip.map(|f| if f { -1. } else { 1. }),
                    offset: retouch.offset,
                    stroke: Some((batch.stroke_id, batch.layer_id, retouch.clone())),
                };
                let retouch = self.retouch_sources();
                let mapping = retouch.mapping(self, &gather);
                self.retouch = Some(retouch);
                mapping
            }
            _ => Ok(Mapping::fixed(false)),
        }
    }

    pub(super) fn retouch_resident(&self, mapping: &Mapping) -> bool {
        self.retouch.as_ref().is_some_and(|retouch| retouch.resident(self, mapping))
    }

    /// The pages `mapping` reads, empty where there are none.
    pub(super) fn retouch_pages(
        &mut self,
        mapping: &Mapping,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<[wgpu::TextureView; 8], GpuRasterError> {
        let mut retouch = self.retouch_sources();
        let pages = retouch.pages(self, mapping, encoder);
        self.retouch = Some(retouch);
        Ok(pages?.0.map(|view| view.unwrap_or_else(|| self.empty_view.clone())))
    }

    /// Draw what the latest retouching stroke samples into `destination`, a
    /// 256px working-format target on this device: for each pixel `p` of the
    /// `size` region at `origin` in the stroke's layer, the source at
    /// `scale * p + offset`. The target is read as the stroke found it, over
    /// the references below it. Returns whether every page was ready; during
    /// a contact a page that is not stays transparent.
    pub fn draw_retouch_source(
        &mut self,
        destination: &wgpu::TextureView,
        origin: [u32; 2],
        size: [u32; 2],
        scale: [f32; 2],
        offset: [f32; 2],
    ) -> Result<bool, GpuRasterError> {
        let region = PixelRect::new(origin[0], origin[1], origin[0].saturating_add(size[0]), origin[1].saturating_add(size[1]));
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor { label: Some("retouch source") },
        );
        let mut retouch = self.retouch_sources();
        let complete = retouch.encode_gather(self, destination, Gather { region, scale, offset, stroke: None }, &mut encoder);
        self.retouch = Some(retouch);
        let complete = complete?;
        self.uploads.finish(&encoder);
        self.last_submission = Some(encoder.submit(&self.queue));
        Ok(complete)
    }
}

#[cfg(test)]
#[path = "retouch_sources_tests.rs"]
mod tests;
