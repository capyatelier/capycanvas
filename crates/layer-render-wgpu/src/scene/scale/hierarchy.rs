use super::*;

pub(crate) fn eligible(scene: SceneView<'_>) -> bool {
    !scene.order().iter().any(|handle| scene.object_layer(*handle).is_some())
}

#[expect(clippy::large_enum_variant, reason = "Window images remain inline while display caches change during motion")]
pub(super) enum Pixels {
    Window { root: Option<Image>, next: Option<Image> },
    Resident { levels: Arc<[Image]>, level: u32 },
}
impl Default for Pixels {
    fn default() -> Self { Self::Window { root: None, next: None } }
}
impl Pixels {
    pub fn root(&self) -> Option<&Image> {
        match self { Self::Window { root, .. } => root.as_ref(), Self::Resident { levels, level } => Some(&levels[*level as usize]) }
    }
    pub fn next(&self) -> Option<&Image> {
        match self { Self::Window { next, .. } => next.as_ref(), Self::Resident { levels, level } => Some(&levels[*level as usize + 1]) }
    }
    pub fn ensure_root(&mut self, r: &WgpuRasterizer, plan: display_mips::Plan, label: &'static str) {
        if let Self::Window { root, .. } = self { root.get_or_insert_with(|| Image::new(r, plan, label)); }
    }
    pub fn ensure(&mut self, r: &WgpuRasterizer, plan: display_mips::Plan) {
        self.ensure_root(r, plan, "composition level");
        if let Self::Window { next, .. } = self {
            next.get_or_insert_with(|| Image::new(r,
                display_mips::Plan::window(plan.extent, plan.level + 1, plan.bounds), "adjacent composition level"));
        }
    }
    pub fn shares_levels(&self, other: &Self) -> bool {
        matches!((self, other), (Self::Resident { levels: a, .. }, Self::Resident { levels: b, .. }) if Arc::ptr_eq(a, b))
    }
    pub fn storage_bytes(&self) -> u64 {
        match self {
            Self::Window { root, next } => root.iter().chain(next).map(Image::bytes).sum(),
            Self::Resident { .. } => 0,
        }
    }
}

const HALF_REGION: u32 = 256;

pub(super) struct Half {
    sources: [wgpu::Texture; 2],
    images: [(wgpu::Texture, wgpu::TextureView); 2],
    binding: wgpu::BindGroup,
    region: wgpu::Buffer,
    region_binding: wgpu::BindGroup,
}
impl Half {
    fn new(r: &WgpuRasterizer, sources: [&Image; 2], previous: Option<Self>) -> Self {
        let region = previous.as_ref().map_or_else(|| r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("half-float display region"), size: u64::from(HALF_REGION),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
        }), |half| half.region.clone());
        let region_binding = Commands::record_binding(r, &region);
        let mut previous = previous.map(|half| half.images.map(Some));
        let images = std::array::from_fn(|index| {
            let size = [sources[index].texture.width(), sources[index].texture.height()];
            previous.as_mut().and_then(|images| images[index].take()).filter(|(texture, _)| [texture.width(), texture.height()] == size).unwrap_or_else(|| {
                let texture = r.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("half-float display level"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = texture.create_view(&Default::default());
                (texture, view)
            })
        });
        let binding = crate::bindings::group(&r.device, "half-float display levels", &r.scene_pipelines.scale.half_inputs, [
            wgpu::BindingResource::TextureView(&sources[0].view),
            wgpu::BindingResource::TextureView(&sources[1].view),
            wgpu::BindingResource::TextureView(&images[0].1),
            wgpu::BindingResource::TextureView(&images[1].1),
        ]);
        Self { sources: sources.map(|image| image.texture.clone()), images, binding, region, region_binding }
    }
    pub(super) fn presents(&self, sources: [&Image; 2]) -> bool {
        self.sources[0] == sources[0].texture && self.sources[1] == sources[1].texture
    }
    /// Convert `region` of the coarser level, and the finer level's texels
    /// that reduce into it.
    fn convert(&self, pipeline: &wgpu::ComputePipeline, encoder: &mut wgpu::CommandEncoder, uploads: &mut crate::Uploads, region: PixelRect) -> Result<(), GpuRasterError> {
        uploads.write_at(encoder, &self.region, 0, &[region.min_x(), region.min_y(), region.width(), region.height()].map(u32::to_le_bytes).concat())?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("half-float display levels"), timestamp_writes: None });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.region_binding, &[0]);
        pass.set_bind_group(1, &self.binding, &[]);
        pass.dispatch_workgroups((region.width() * 2).div_ceil(8), (region.height() * 2 + region.height().div_ceil(2)).div_ceil(8), 1);
        Ok(())
    }
    pub fn bytes(&self) -> u64 { self.images.iter().map(|(texture, _)| texture_bytes(texture)).sum() }
}
fn half_bytes(sources: [&Image; 2]) -> u64 { sources.iter().map(|image| image.bytes() / 2).sum() }

pub(super) struct Hierarchy {
    levels: Arc<[Image]>,
    updates: display_mips::CompleteUpdates,
    refined: BTreeSet<[u32; 2]>,
    bytes: u64,
}
impl Hierarchy {
    pub(crate) fn budget(r: &WgpuRasterizer) -> u64 {
        let _trace = crate::performance_trace::Span::new(c"capy.hierarchy_budget");
        r.memory.scene_limit().saturating_sub(CACHE_BYTES)
    }
    pub fn fits(&self, r: &WgpuRasterizer) -> bool { self.bytes <= Self::budget(r) }
    fn new(r: &mut WgpuRasterizer, cache: &Cache, packet: FramePacket<'_>, encoder: &mut crate::submission::CommandEncoder) -> Option<Self> {
        if !eligible(packet.scene) { return None; }
        #[cfg(not(target_arch = "wasm32"))]
        if r.snapshot_worker { return None; }
        let budget = Self::budget(r);
        if budget == 0 { return None; }
        let extent = cache.plan.extent;
        let last = display_mips::Plan::new(extent).ok()?.level.max(5) + 1;
        let pages = extent.map(|n| u64::from(n.div_ceil(PAGE_SIZE))).into_iter().product::<u64>();
        let records = pages * u64::from(last) * u64::from(r.device.limits().min_uniform_buffer_offset_alignment.max(16));
        let plans: Vec<_> = (0..=last).map(|level| display_mips::Plan::at(extent, level)).collect();
        let bytes = records + plans.iter().map(|p| p.level_bytes(p.level)).sum::<u64>();
        if bytes > budget || extent.iter().any(|n| *n > r.device.limits().max_texture_dimension_2d) { return None; }
        if r.native_edit.as_ref().is_some_and(|native|
            windows::Plan::new(packet.scene, extent, native.image_pixel_budget(bytes), r.device.limits().max_texture_dimension_2d).is_err()) { return None; }
        let existing: Vec<_> = std::iter::once(cache).chain(cache.overview.as_deref())
            .flat_map(|c| c.pixels.root().into_iter().chain(c.pixels.next())).collect();
        let levels: Arc<[Image]> = plans.into_iter().map(|plan| {
            if let Some(image) = existing.iter().find(|i| i.plan == plan) { return (*image).clone(); }
            let image = Image::new(r, plan, "resident composition level");
            for old in existing.iter().filter(|i| i.plan.level == plan.level) {
                encoder.copy_texture_to_texture(old.texture.as_image_copy(), wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d { x: old.plan.bounds.min_x() >> plan.level, y: old.plan.bounds.min_y() >> plan.level, z: 0 },
                    ..image.texture.as_image_copy()
                }, wgpu::Extent3d { width: old.plan.size[0], height: old.plan.size[1], depth_or_array_layers: 1 });
            }
            image
        }).collect();
        let pipelines = r.display_pipelines.take().unwrap_or_else(|| display_mips::Pipelines::new(&r.device));
        let views: Vec<_> = levels.iter().map(|i| &i.view).collect();
        let updates = display_mips::CompleteUpdates::from_level(&r.device, &pipelines,
            display_mips::Plan::at(extent, last), 0, &views);
        r.display_pipelines = Some(pipelines);
        let refined = if cache.plan.level == 0 && cache.placed.is_none() { cache.refined.clone() } else { BTreeSet::new() };
        let mut hierarchy = Self { levels, updates, refined, bytes };
        for coordinate in &hierarchy.refined { hierarchy.updates.tile(encoder, *coordinate); }
        hierarchy.updates.flush(encoder);
        Some(hierarchy)
    }
    pub fn storage_bytes(&self) -> u64 { self.bytes }
    pub fn root(&self) -> &Image { &self.levels[0] }
    pub fn missing(&self) -> Option<[u32; 2]> {
        self.missing_pages().next()
    }
    pub fn missing_pages(&self) -> impl Iterator<Item = [u32; 2]> + '_ {
        page_coordinates(self.levels[0].plan.bounds).filter(|c| !self.refined.contains(c))
    }
    pub fn invalidate(&mut self, dirty: Damage) {
        self.refined.retain(|c| !dirty.intersects(page_rect(*c)));
    }
    pub fn write(&mut self, encoder: &mut crate::submission::CommandEncoder, source: &Image,
        coordinate: [u32; 2], region: PixelRect,
    ) {
        if !self.refined.insert(coordinate) { return; }
        if source.texture != self.root().texture { encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d { x: region.min_x() - source.plan.bounds.min_x(), y: region.min_y() - source.plan.bounds.min_y(), z: 0 },
            ..source.texture.as_image_copy()
        }, wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d { x: region.min_x(), y: region.min_y(), z: 0 },
            ..self.levels[0].texture.as_image_copy()
        }, wgpu::Extent3d { width: region.width(), height: region.height(), depth_or_array_layers: 1 }); }
        self.updates.tile(encoder, coordinate);
    }
    pub fn flush(&mut self, encoder: &mut crate::submission::CommandEncoder) {
        self.updates.flush(encoder);
    }
}

impl Cache {
    pub(super) fn half_reserve(&self) -> u64 {
        if self.plan.level == 0 || self.hierarchy.is_some() { 0 } else { (self.plan.level_bytes(self.plan.level) + self.plan.level_bytes(self.plan.level + 1)) / 2 }
    }
    /// Create the half-float copies of the presented levels when those levels
    /// change. Later writes accumulate in `half_stale` for the presenter.
    pub(super) fn present_half(&mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        let (Some(root), Some(next)) = (self.pixels.root(), self.pixels.next()) else { self.half = None; return Ok(()) };
        if self.plan.level == 0 || self.placed.is_some() { self.half = None; return Ok(()); }
        let sources = [root, next];
        if self.half.as_ref().is_some_and(|half| half.presents(sources)) { return Ok(()); }
        let previous = self.half.take();
        let admitted = match &self.hierarchy {
            Some(hierarchy) => hierarchy.bytes + half_bytes(sources) <= Hierarchy::budget(r),
            None => half_bytes(sources) <= self.auxiliary_budget(r, packet),
        };
        if !admitted { return Ok(()); }
        let half = Half::new(r, sources, previous);
        *self.half_stale.get_mut().unwrap() = PixelRect::EMPTY;
        r.metrics.half_level_passes += 1;
        encoder.reserve_passes(1);
        half.convert(&r.scene_pipelines.scale.half, encoder, &mut r.uploads, PixelRect::full([next.texture.width(), next.texture.height()]))?;
        self.half = Some(half);
        Ok(())
    }
    /// Convert the parts of the presented levels written since the last
    /// presentation into their half-float copies.
    pub(crate) fn refresh_half(&self, r: &WgpuRasterizer, encoder: &mut wgpu::CommandEncoder, uploads: &mut crate::Uploads) -> Result<(), GpuRasterError> {
        let (Some(half), Some(root), Some(next)) = (&self.half, self.pixels.root(), self.pixels.next()) else { return Ok(()) };
        if !half.presents([root, next]) { return Ok(()); }
        let stale = std::mem::replace(&mut *self.half_stale.lock().unwrap(), PixelRect::EMPTY);
        let region = PixelRect::new(stale.min_x() / 2, stale.min_y() / 2, stale.max_x().div_ceil(2), stale.max_y().div_ceil(2))
            .intersect(PixelRect::full([next.texture.width(), next.texture.height()]));
        if region.is_empty() { return Ok(()); }
        half.convert(&r.scene_pipelines.scale.half, encoder, uploads, region)
    }
    #[cfg(test)]
    pub(crate) fn cached_branches(&self) -> usize { self.graph.cached_branches() }
    #[cfg(test)]
    pub(crate) fn spare_branch_textures(&self) -> Vec<wgpu::Texture> { self.graph.spare_textures() }
    #[cfg(test)]
    pub(crate) fn branch_textures(&self) -> Vec<wgpu::Texture> { self.graph.branch_textures() }
    #[cfg(test)]
    pub(crate) fn prepared_pages(&self) -> usize {
        self.graph.prepared.iter().flatten().map(|half| page_coordinates(self.plan.bounds).filter(|page| self.graph.branch_valid(half, *page)).count()).sum()
    }
    pub(crate) fn presented_views(&self) -> [&wgpu::TextureView; 2] {
        match (&self.half, self.pixels.root(), self.pixels.next()) {
            (Some(half), Some(root), Some(next)) if self.placed.is_none() && half.presents([root, next]) => [&half.images[0].1, &half.images[1].1],
            _ => [self.view(), self.next_view()],
        }
    }
    pub(crate) fn resident_bytes(&self) -> u64 {
        self.hierarchy.as_ref().map_or(0, Hierarchy::storage_bytes)
    }
    pub(crate) fn working_bytes(&self) -> u64 {
        self.storage_bytes().saturating_sub(self.resident_bytes())
    }
    pub(super) fn output_plan(&self) -> display_mips::Plan {
        self.pixels.root().map_or(self.plan, |i| i.plan)
    }
    pub(super) fn admit_hierarchy(&mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, encoder: &mut crate::submission::CommandEncoder,
        commands: &mut Commands,
    ) -> Result<(), GpuRasterError> {
        if !self.residency_checked && self.hierarchy.is_none()
            && let Some(hierarchy) = Hierarchy::new(r, self, packet, encoder) {
                let materialized = self.pixels.root().is_some();
                self.share_levels(r, &hierarchy, false);
                self.hierarchy = Some(hierarchy);
                if materialized && self.placed.is_none() {
                    let [x, y, width, height] = paint_transform::texel_rect(self.plan.bounds, 1 << self.plan.level);
                    self.reduce_output(r, encoder, PixelRect::new(x, y, x + width, y + height), commands)?;
                }
                if let Some(overview) = &mut self.overview {
                    overview.reduce_output(r, encoder, PixelRect::full(overview.plan.size), commands)?;
                }
                self.spare = None;
                self.shifted = None;
        }
        self.residency_checked = true;
        Ok(())
    }
    pub(super) fn share_levels(&mut self, r: &WgpuRasterizer, hierarchy: &Hierarchy, restore: bool) {
        self.pixels = Pixels::Resident { levels: hierarchy.levels.clone(), level: self.plan.level };
        self.geometry = Self::geometry(r, self.output_plan(), hierarchy.levels.last().unwrap().plan.level);
        if restore {
            self.valid.clone_from(&hierarchy.refined);
            self.refined.clone_from(&hierarchy.refined);
            self.ready = hierarchy.missing().is_none();
            self.reuse_output = self.unchanged && self.output_complete();
        }
        if let Some(overview) = &mut self.overview {
            overview.unchanged = self.unchanged;
            overview.share_levels(r, hierarchy, restore);
        }
    }
    pub(in crate::scene) fn invalidate_hierarchy(&mut self, sources: &Sources, dirty: PixelRect, tiles: Option<&BTreeSet<[u32; 2]>>) {
        if !self.unchanged && let Some(hierarchy) = &mut self.hierarchy {
            hierarchy.invalidate(Damage::from_tiles(dirty, tiles).union(self.graph.root.as_ref().unwrap().damage(sources, self.plan)));
        }
    }
}
