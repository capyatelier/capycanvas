use super::*;
use layer_core::authored::{Affine64, ImageInterpolation, ImageObjectHandle, PortableId};
use layer_core::color::source::SourceImage;
use std::sync::Weak;

type CollectionPreview = (Vec<wgpu::TextureView>, Vec<crate::object_sampling::PreviewSource>);

#[derive(Clone)]
pub(super) struct SourceIdentity(Weak<SourceImage>);
impl PartialEq for SourceIdentity { fn eq(&self, other: &Self) -> bool { self.0.ptr_eq(&other.0) } }
impl Eq for SourceIdentity {}
impl PartialOrd for SourceIdentity { fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) } }
impl Ord for SourceIdentity { fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.0.as_ptr().cmp(&other.0.as_ptr()) } }
impl std::hash::Hash for SourceIdentity { fn hash<H: std::hash::Hasher>(&self, state: &mut H) { std::hash::Hash::hash(&self.0.as_ptr(), state); } }

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct ObjectKey {
    pub handle: ImageObjectHandle,
    pub image: PortableId,
    pub source: SourceIdentity,
    pub affine: [u64; 6],
    pub pose: [u64; 6],
    pub nearest: bool,
}
impl ObjectKey {
    pub fn source(&self) -> Option<Arc<SourceImage>> { self.source.0.upgrade() }
    pub fn same_authored(&self, other: &Self) -> bool { self.handle == other.handle && self.image == other.image && self.source == other.source && self.pose == other.pose && self.nearest == other.nearest }
    pub fn new(scene: SceneView<'_>, owner: OccurrenceHandle, handle: ImageObjectHandle) -> Self {
        let object = scene.object(handle).unwrap();
        let affine = placement(scene, owner, object.affine);
        let pose = placement(scene.with_offset64(scene.evaluation_offset64().map(|value| -value)), owner, object.affine).0;
        Self { handle, image: object.image.id(), source: SourceIdentity(Arc::downgrade(object.image.storage())),
            affine: affine.0.map(f64::to_bits), pose: pose.map(f64::to_bits), nearest: object.interpolation == ImageInterpolation::Nearest }
    }
    pub fn placement(&self) -> Affine64 { Affine64(self.affine.map(f64::from_bits)) }
}

pub(super) fn placement(scene: SceneView<'_>, owner: OccurrenceHandle, affine: Affine64) -> Affine64 {
    let offset = scene.occurrence_offset64(owner);
    Affine64([1., 0., 0., 1., offset[0], offset[1]]).compose(affine)
}
pub(super) fn object_bounds(scene: SceneView<'_>, key: &ObjectKey) -> DocRect {
    let Some(object) = scene.object(key.handle) else { return DocRect::default(); };
    let [min, max] = key.placement().bounds(object.image.extent);
    let [a, b, c, d, _, _] = key.placement().0;
    let halo = if key.nearest { [0.; 2] } else { [(a.abs() + c.abs()) * 0.5 + 2., (b.abs() + d.abs()) * 0.5 + 2.] };
    DocRect { min: std::array::from_fn(|axis| (min[axis] - halo[axis]).floor() as i64), max: std::array::from_fn(|axis| (max[axis] + halo[axis]).ceil() as i64) }
}
pub(super) fn edited_damage(before: SceneView<'_>, after: SceneView<'_>, extent: [u32; 2]) -> Option<scale::Damage> {
    if before.artwork().objects.same_root(&after.artwork().objects) && before.artwork().vectors.same_root(&after.artwork().vectors) { return None; }
    if before.order() != after.order() || before.composition() != after.composition() { return None; }
    let mut damage = scale::Damage::EMPTY;
    let mut changed = false;
    for &owner in after.order() {
        if before.paint_source(owner).map(|p| p.raster.identity()) != after.paint_source(owner).map(|p| p.raster.identity())
            || before.mask(owner).map(|(_, p)| p.raster.identity()) != after.mask(owner).map(|(_, p)| p.raster.identity()) { return None; }
        let old = metadata::Metadata::new(before, owner);
        let new = metadata::Metadata::new(after, owner);
        if old == new { continue; }
        if !old.same_object_layer(&new) { return None; }
        if let (Some(previous), Some(next)) = (before.vector_layer(owner), after.vector_layer(owner)) {
            changed = true;
            if !after.visible(owner) && !before.visible(owner) { continue; }
            let offset = after.occurrence_offset64(owner);
            let rects = super::vectors::edit_damage(previous, next).into_iter().map(|rect| {
                let rect = [rect[0] + offset[0] - 16., rect[1] + offset[1] - 16., rect[2] + offset[0] + 16., rect[3] + offset[1] + 16.];
                PixelRect::new(rect[0].floor().clamp(0., f64::from(extent[0])) as u32, rect[1].floor().clamp(0., f64::from(extent[1])) as u32,
                    rect[2].ceil().clamp(0., f64::from(extent[0])) as u32, rect[3].ceil().clamp(0., f64::from(extent[1])) as u32)
            }).filter(|rect| !rect.is_empty()).collect();
            damage.regions.extend(super::vectors::coalesce(rects).into_iter().map(|rect| rect.intersect(PixelRect::full(extent))));
            continue;
        }
        after.object_layer(owner)?;
        changed = true;
        for scene in [before, after] {
            if !scene.visible(owner) { continue; }
            {
                let object = scene.object_layer(owner)?;
                let affine = placement(scene, owner, object.affine);
                let [min, max] = affine.bounds(object.image.extent);
                let [a, b, c, d, _, _] = affine.0;
                let halo = (a.abs() + c.abs()).max(b.abs() + d.abs()) + 2.;
                damage.regions.push(PixelRect::new((min[0] - halo).floor().clamp(0., f64::from(extent[0])) as u32,
                    (min[1] - halo).floor().clamp(0., f64::from(extent[1])) as u32,
                    (max[0] + halo).ceil().clamp(0., f64::from(extent[0])) as u32,
                    (max[1] + halo).ceil().clamp(0., f64::from(extent[1])) as u32));
            }
        }
    }
    changed.then(|| damage.expand(stack::support(after, 0).unwrap_or(extent[0].max(extent[1])), extent))
}

#[derive(Clone)]
pub(super) struct ObjectJob {
    pub key: ObjectKey,
    pub source: Arc<layer_core::color::source::SourceImage>,
    pub inverse: [f64; 6],
    pub size: [u32; 2],
    pub nearest: bool,
    pub output: wgpu::TextureView,
    pub live: bool,
}

pub(super) const COLLECTION_PART: u32 = 512;
pub(super) const INLINE_SAMPLING_BYTES: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64 * 56;

pub(super) fn inline_sampling(r: &WgpuRasterizer) -> bool {
    #[cfg(target_arch = "wasm32")]
    { r.snapshot_worker && r.browser_image_decoder.is_none() }
    #[cfg(not(target_arch = "wasm32"))]
    { r.snapshot_worker }
}

#[derive(Clone)]
pub(super) enum SamplingStep {
    Begin { inverse: [f64; 6], size: [u32; 2], nearest: bool, extent: [u32; 2] },
    Contribute { source: wgpu::TextureView, origin: [i32; 2], extent: [u32; 2] },
    Normalize,
}

#[derive(Default)]
pub(super) struct InlineSampling {
    result: Option<(wgpu::Texture, wgpu::TextureView)>,
    uniform: Option<wgpu::Buffer>,
    accumulation: Option<crate::object_sampling::ObjectAccumulator>,
    pub taps: u64,
}
impl InlineSampling {
    pub fn encode(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, step: &SamplingStep) -> Result<(), GpuRasterError> {
        use crate::object_sampling::{SamplingPhase, SamplingRequest};
        let sampler = r.scene_pipelines.objects.clone();
        let phase = match step {
            SamplingStep::Begin { inverse, size, nearest, extent } => {
                let uniform = self.uniform.get_or_insert_with(|| crate::object_sampling::ObjectSampler::uniform(&r.device)).clone();
                let output = &self.result.as_ref().ok_or(GpuRasterError::InvalidExtent)?.1;
                let request = SamplingRequest { inverse: *inverse, size: *size, nearest: *nearest };
                let accumulation = sampler.reuse(&r.device, self.accumulation.take(), request, output, &r.empty_view, &uniform)?;
                let length = size[0] * size[1];
                for first in (0..length).step_by(65536).filter(|_| *nearest) {
                    let bytes = crate::object_sampling::prepare_nearest_coordinates(*inverse, *size, first, (length - first).min(65536)).map_err(GpuRasterError::Color)?;
                    sampler.set_nearest_coordinates(encoder, &mut r.uploads, &accumulation, first, &bytes)?;
                }
                sampler.initialize(encoder, &accumulation);
                self.accumulation = Some(accumulation);
                SamplingPhase::Denominator { extent: *extent }
            }
            SamplingStep::Contribute { source, origin, extent } => SamplingPhase::Contribute { source, origin: *origin, extent: *extent },
            SamplingStep::Normalize => SamplingPhase::Normalize,
        };
        let accumulation = self.accumulation.as_ref().ok_or(GpuRasterError::InvalidExtent)?;
        for step in 0.. {
            let Some(taps) = sampler.encode_step(&r.device, encoder, &mut r.uploads, accumulation, &phase, step)? else { break; };
            self.taps += taps.max(object_cache::PASS_TAPS);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) struct ObjectWindow { pub origin: [f64; 2], pub side: f64, pub size: [u32; 2] }

#[derive(Clone)]
pub(super) struct CollectionJob {
    pub owner: OccurrenceHandle,
    pub authored: Arc<[ObjectKey]>,
    pub keys: Arc<[ObjectKey]>,
    pub window: ObjectWindow,
    pub blend: layer_core::BlendSpace,
    pub live: bool,
    pub context: layer_core::color::RgbSpace,
    pub display: bool,
    pub format: wgpu::TextureFormat,
}
impl CollectionJob {
    pub fn same(&self, other: &Self) -> bool {
        self.same_content(other) && self.window.origin == other.window.origin && self.window.size == other.window.size
    }
    pub fn same_content(&self, other: &Self) -> bool {
        self.owner == other.owner && self.window.side == other.window.side && self.blend == other.blend && self.context == other.context && self.display == other.display
            && (Arc::ptr_eq(&self.authored,&other.authored) || (self.authored.len() == other.authored.len() && self.authored.iter().zip(other.authored.iter()).all(|(a,b)| a.same_authored(b))))
    }
    pub fn bounds(&self) -> DocRect {
        DocRect { min: self.window.origin.map(|v| v.floor() as i64),
            max: std::array::from_fn(|i| (self.window.origin[i] + f64::from(self.window.size[i]) * self.window.side).ceil() as i64) }
    }
    pub fn child(&self, index: usize, window: ObjectWindow, output: wgpu::TextureView) -> Result<ObjectJob, GpuRasterError> {
        let key = self.keys[index].clone();
        let source = key.source().ok_or(GpuRasterError::InvalidExtent)?;
        let inverse = Affine64(key.pose.map(f64::from_bits)).inverse().ok_or(GpuRasterError::InvalidTransform("Invalid image object affine"))?
            .compose(Affine64([window.side,0.,0.,window.side,window.origin[0],window.origin[1]])).0;
        Ok(ObjectJob {nearest:key.nearest,key,source,inverse,size:window.size,output,live:self.live})
    }
    pub fn parts(&self) -> u32 { self.window.size.map(|size| size.div_ceil(COLLECTION_PART)).into_iter().product() }
    pub fn part(&self, index: u32) -> ObjectWindow {
        let columns = self.window.size[0].div_ceil(COLLECTION_PART);
        let size: [u32; 2] = std::array::from_fn(|axis| self.window.size[axis].div_ceil(self.window.size[axis].div_ceil(COLLECTION_PART)));
        let cell = [index % columns, index / columns];
        ObjectWindow { origin: std::array::from_fn(|axis| self.window.origin[axis] + f64::from(cell[axis] * size[axis]) * self.window.side), side: self.window.side, size }
    }
}

impl Scene {
    pub(crate) fn capture_source_bytes(&mut self,scene:SceneView<'_>,region:PixelRect)->Result<u64,GpuRasterError> {
        if matches!(scene.scope(),Some(layer_core::SceneScope::Raw(_))) {return Ok(0);}
        self.object_spatial.prepare(scene);
        let mut bytes=0;let mut y=region.min_y();
        while y<region.max_y() {
            let rows=(PAGE_SIZE-y%PAGE_SIZE).min(region.max_y()-y);let mut x=region.min_x();
            while x<region.max_x() {
                let columns=(2*PAGE_SIZE-x%PAGE_SIZE).min(region.max_x()-x);
                bytes=bytes.max(self.capture_window_source_bytes(scene,PixelRect::new(x,y,x+columns,y+rows))?);
                x+=columns;
            }
            y+=rows;
        }
        Ok(bytes)
    }
    fn capture_window_source_bytes(&mut self,scene:SceneView<'_>,region:PixelRect)->Result<u64,GpuRasterError> {
        let raw=match scene.scope() {Some(layer_core::SceneScope::RawObjects(owner))=>Some(*owner),_=>None};
        let window=DocRect::from(region).aligned(PAGE_SIZE);
        let stages=images::stage_regions(scene,window,0,Some(&self.object_spatial));
        let mut tiles=std::collections::BTreeSet::new();
        for &owner in scene.order() {
            if raw.map_or_else(||!scene.visible(owner),|raw|raw!=owner) {continue;}
            let Some(content)=self.object_spatial.content(scene,owner) else {continue;};
            let window=stages.get(&owner).map_or(window,|regions|regions[0]).aligned(PAGE_SIZE);
            for key in content.query_at(window,0) {
                let source=key.source().ok_or(GpuRasterError::InvalidImage)?;
                let inverse=key.placement().inverse().ok_or(GpuRasterError::InvalidTransform("Invalid image object affine"))?
                    .compose(Affine64([1.,0.,0.,1.,window.min[0] as f64,window.min[1] as f64])).0;
                let bounds=crate::object_sampling::ObjectSampler::source_bounds(inverse,window.size()?,key.nearest,source.extent)?;
                tiles.extend(page_coordinates(bounds).filter(|coordinate|source.tiles.contains_key(coordinate)).map(|coordinate|(key.source.clone(),coordinate)));
            }
        }
        Ok((tiles.len() as u64).saturating_mul(sources::NORMALIZED_PAGE_BYTES))
    }
    pub(crate) fn invalidate_object_previews(&mut self, extent: [u32; 2]) { self.object_source_damage = PixelRect::full(extent).into(); }
    #[cfg(test)]
    pub(crate) fn decode_prepared(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        source: &Arc<SourceImage>, coordinate: [u32; 2], pixels: sources::PreparedSourcePixels,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> { decode_prepared(r, encoder, source, coordinate, pixels) }
    pub(crate) fn decode_prepared_for(r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,
        source:&Arc<SourceImage>,coordinate:[u32;2],pixels:sources::PreparedSourcePixels,use_:sources::SourceUse,class:Class,
    )->Result<crate::source_access::RawTile,GpuRasterError> {decode_prepared_for(r,encoder,source,coordinate,pixels,use_,class)}
    pub(crate) fn decode_prepared_raster(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        blob: &Arc<layer_core::raster::TileBlob>, space: layer_core::color::RgbSpace, destination: layer_core::color::RgbSpace,
        pixels: sources::PreparedSourcePixels, class: Class,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> { decode_prepared_raster(r, encoder, blob, space, destination, pixels, class) }
    pub(crate) fn compiling(&self) -> bool { !self.pending_pipelines.ready() }
    pub(crate) fn objects_pending(&self) -> bool { self.object_results.pending() || self.vectors_deferred() }
    pub(crate) fn objects_demanded(&self) -> bool { self.object_results.demanded() || self.vectors_deferred() }
    pub(crate) fn objects_waiting(&self) -> bool { self.object_results.waiting() && !self.vectors_deferred() }
    pub(crate) fn object_edit_damage(before: SceneView<'_>, after: SceneView<'_>, extent: [u32; 2]) -> Option<scale::Damage> {
        edited_damage(before, after, extent)
    }
    pub(super) fn collection_job(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, owner: OccurrenceHandle,
        content: object_spatial::Content, window: ObjectWindow,
    ) -> CollectionJob {
        let level=window.side.max(1.).log2().ceil() as u32;
        let bounds=DocRect {min:window.origin.map(|v|v.floor() as i64),max:std::array::from_fn(|i|(window.origin[i]+f64::from(window.size[i])*window.side).ceil() as i64)};
        let keys=content.query_at(bounds,level).into();
        let offset=packet.scene.evaluation_offset64();
        let global=ObjectWindow {origin:std::array::from_fn(|i|window.origin[i]-offset[i]),..window};
        CollectionJob {owner,authored:content.keys(),keys,window:global,blend:packet.blend_space,live:self.object_display||self.object_query,
            context:r.device.working_space(),display:self.object_display,format:crate::MovingProjection::format_for(r.moving_projection.as_ref(),packet.scene)}
    }
    pub(super) fn prepare_object_pages(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, raw: Option<OccurrenceHandle>,
        pages: impl IntoIterator<Item = [u32; 2]> + Clone,
    ) -> Result<bool, GpuRasterError> {
        self.object_spatial.prepare(packet.scene);
        let mut ready = true;
        for &owner in packet.scene.order() {
            if raw.is_some_and(|raw| raw != owner) || (raw.is_none() && !packet.scene.visible(owner)) { continue; }
            let Some(content) = self.object_spatial.content(packet.scene, owner) else { continue; };
            for coordinate in pages.clone() {
                let window = ObjectWindow { origin: coordinate.map(|v| f64::from(v * PAGE_SIZE)), side: 1., size: [PAGE_SIZE; 2] };
                let request = self.collection_job(r, packet, owner, content.clone(), window);
                if request.keys.is_empty() { continue; }
                let display = self.object_display || self.object_query;
                let requester = self.capture_requester();
                let cache = if display { &mut self.object_results } else { &mut self.exact_object_results };
                cache.set_requester(requester);
                ready &= cache.prepare_collection(r, packet.scene, &request,None)?;
            }
        }
        Ok(ready)
    }
    fn capture_requester(&self) -> Class { if self.object_live || self.object_query { Class::Visible } else { Class::Refinement } }
    pub(super) fn canonical_cover(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, content: &object_spatial::Content,
        request: &CollectionJob, accepted:&mut sources::AcceptedInputs, class: Class,
    ) -> Result<Option<Vec<(wgpu::TextureView, DocRect)>>, GpuRasterError> {
        let (pieces, mut ready, missing) = self.object_results.cover(request,Some(accepted));
        self.object_results.stamp(request, class);
        let offset = packet.scene.evaluation_offset64();
        let mut parts = Vec::new();
        for rect in missing {
            let size = rect.size()?.map(|v| (f64::from(v) / request.window.side) as u32);
            let window = ObjectWindow { origin: std::array::from_fn(|i| rect.min[i] as f64 + offset[i]), side: request.window.side, size };
            let mut part = self.collection_job(r, packet, content.owner(), content.clone(), window);
            part.live = request.live; part.display = request.display;
            if part.keys.is_empty() { continue; }
            parts.push(part);
            ready = false;
        }
        if accepted.declaring() {
            for part in parts { accepted.declare_object(part); }
        } else if !parts.is_empty() {
            self.object_results.admit(r, class.max(Class::Prefetch), &parts, Some(accepted))?;
        }
        Ok(ready.then_some(pieces))
    }
    fn preview_source(r: &WgpuRasterizer, id: PortableId, source: &Arc<SourceImage>, nearest: bool, format:wgpu::TextureFormat, inverse: [f64; 6], size: [u32;2]) -> Option<crate::object_image_mips::MovingSource> {
        let level = if nearest { 0 } else { crate::object_image_mips::MovingImages::level(inverse) };
        let format=if nearest {crate::object_image_mips::MovingImages::source_format(source)}else{format};
        let bounds=crate::object_sampling::ObjectSampler::source_bounds(inverse,size,nearest,source.extent).ok()?;
        r.moving_images.lookup(id,source,level,bounds,format,r.device.working_space())
            .filter(|mip|!nearest || mip.level==0)
    }
    pub(super) fn collection_preview(&mut self,r: &WgpuRasterizer, job: &CollectionJob) -> Result<Option<CollectionPreview>, GpuRasterError> {
        let mut identities: Vec<(PortableId, SourceIdentity, u32, [u32;2], [u32;2])> = Vec::new();
        let mut sources = Vec::new();
        let mut objects = Vec::with_capacity(job.keys.len());
        for index in 0..job.keys.len() {
            let child = job.child(index, job.window, r.empty_view.clone())?;
            let Some(mip) = Self::preview_source(r, child.key.image, &child.source, child.nearest, job.format, child.inverse, job.window.size) else {
                return Ok(None);
            };
            let mut inverse = child.inverse.map(|value| value / f64::from(1u32 << mip.level));
            inverse[4]-=f64::from(mip.origin[0])/f64::from(1u32<<mip.level);
            inverse[5]-=f64::from(mip.origin[1])/f64::from(1u32<<mip.level);
            if crate::object_sampling::mapping(inverse, job.window.size).is_err() { return Ok(None); }
            let identity = (child.key.image, child.key.source.clone(), mip.level, mip.origin, mip.extent);
            let source = identities.iter().position(|known| *known == identity).unwrap_or_else(|| { identities.push(identity); sources.push(mip.view.clone()); sources.len() - 1 });
            objects.push(crate::object_sampling::PreviewSource { source, inverse, extent: mip.extent, nearest: child.nearest, samples: mip.samples });
        }
        let required=compile::Needs {compute:objects.iter().map(|object|if object.samples.is_some_and(|samples|samples.format()==wgpu::TextureFormat::Rg32Uint) {
            &r.scene_pipelines.objects.preview_integer
        } else {&r.scene_pipelines.objects.preview}).filter(|pipeline|!pipeline.ready()).cloned().collect(),..Default::default()};
        if !required.require(&r.compilation,startup::DOCUMENT) {
            self.pending_pipelines=required;
            return Err(GpuRasterError::NotReady);
        }
        Ok(Some((sources, objects)))
    }
    pub(crate) fn object_thumbnail(&mut self, r: &mut WgpuRasterizer, owner: OccurrenceHandle, grid: display_mips::Plan,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<(wgpu::Texture, wgpu::TextureView)>, GpuRasterError> {
        let frame = r.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
        let packet = frame.packet(r.document_extent);
        if packet.scene.vector_layer(owner).is_some() { return self.vector_thumbnail(r, packet, owner, grid, encoder).map(Some); }
        self.object_spatial.prepare(packet.scene);
        let content = self.object_spatial.content(packet.scene, owner).ok_or(GpuRasterError::InvalidExtent)?;
        self.object_query = true;
        let mut request = self.collection_job(r, packet, owner, content, ObjectWindow { origin: [0.; 2], side: f64::from(1u32 << grid.level), size: grid.size });
        request.blend = layer_core::BlendSpace::Linear;
        let target = create_color_target(&r.device, [PAGE_SIZE; 2], "object layer thumbnail");
        if request.keys.is_empty() {
            self.object_query = false;
            r.encode_clear(encoder, &target.1, "empty object layer thumbnail");
            return Ok(Some(target));
        }
        if let Some((sources, objects)) = self.collection_preview(r, &request)? {
            self.object_query = false;
            let scratch = (crate::object_sampling::CollectionPreview::passes(&objects) > 1)
                .then(|| create_color_target(&r.device, [PAGE_SIZE; 2], "object layer thumbnail scratch"));
            let preview = crate::object_sampling::CollectionPreview { sources, objects, size: grid.size, output: target.1.clone(),
                scratch: scratch.map(|scratch| scratch.1), encode: false };
            r.scene_pipelines.objects.clone().preview(&r.device, encoder, &mut r.uploads, &preview, &r.empty_view)?;
            return Ok(Some(target));
        }
        r.drain_image_decodes(encoder)?;
        self.object_results.retain(packet.scene, request.blend, r.device.working_space());
        let mut results = std::mem::replace(&mut self.object_results,super::object_cache::ObjectCache::new(&self.tiers,Default::default(),false));
        let advanced = results.advance_with_memory(r, self, encoder, object_cache::IDLE_TAPS);
        self.object_results = results;
        self.object_results.set_requester(Class::Navigator);
        let mut accepted=sources::AcceptedInputs::default();
        let canonical = advanced.and_then(|_| self.object_results.resolve_collection(r, packet.scene, &request,Some(&mut accepted)));
        self.object_query = false;
        let Some(view) = canonical? else { return Ok(None); };
        accepted.track(encoder);
        encoder.copy_texture_to_texture(view.texture().as_image_copy(), target.0.as_image_copy(),
            wgpu::Extent3d { width: grid.size[0], height: grid.size[1], depth_or_array_layers: 1 });
        Ok(Some(target))
    }
    pub(super) fn object_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, owner: OccurrenceHandle,
        coordinate: [u32; 2],
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        if packet.scene.vector_layer(owner).is_some() {return self.vector_tile(r,packet,owner,coordinate);}
        let content=self.object_spatial.content(packet.scene,owner).ok_or(GpuRasterError::InvalidExtent)?;
        let window=ObjectWindow {origin:coordinate.map(|v|f64::from(v*PAGE_SIZE)),side:1.,size:[PAGE_SIZE;2]};
        let request=self.collection_job(r,packet,owner,content,window);
        if request.keys.is_empty() {return self.alloc(r,wgpu::Color::TRANSPARENT);}
        if inline_sampling(r) && !self.object_display && !self.object_query {return self.inline_object_tile(r,&request, accepted);}
        let display = self.object_display || self.object_query;
        let requester = self.capture_requester();
        let cache = if display { &mut self.object_results } else { &mut self.exact_object_results };
        cache.set_requester(requester);
        let canonical=cache.resolve_collection(r,packet.scene,&request,Some(accepted))?;
        if let Some(view)=canonical {
            let output=self.reserve(r)?;
            self.jobs.push(Job::Copy {source:view.texture().clone(),source_origin:[0;2],destination:self.pool[output].texture.clone(),origin:[0;2],width:PAGE_SIZE,height:PAGE_SIZE});
            return Ok(output);
        }
        let Some((sources,objects))=(if self.object_display {self.collection_preview(r,&request)?} else {None}) else {
            return Err(GpuRasterError::NotReady);
        };
        let output=self.reserve(r)?;
        let held=self.own(output);
        let scratch=(crate::object_sampling::CollectionPreview::passes(&objects)>1).then(||self.reserve(r)).transpose()?;
        self.jobs.push(Job::Collection(Box::new(crate::object_sampling::CollectionPreview {sources,objects,size:window.size,
            output:self.pool[output].view.clone(),scratch:scratch.map(|slot|self.pool[slot].view.clone()),encode:packet.blend_space==layer_core::BlendSpace::Perceptual})));
        if let Some(slot)=scratch {self.free(slot);}
        Ok(self.keep(held))
    }
    fn inline_object_tile(&mut self, r: &WgpuRasterizer, request: &CollectionJob, accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        let job=request.child(0,request.part(0),r.empty_view.clone())?;
        r.preflight_image_sampling(job.inverse,job.size,job.nearest)?;
        let (_,bounds,_)=object_cache::working_set(&job)?;
        let (texture,view)=if self.declaring {(r.declared_scratch[0].texture.clone(),r.declared_scratch[0].view.clone())}
            else {self.inline_objects.result.get_or_insert_with(||create_color_target(&r.device,[PAGE_SIZE;2],"inline object result")).clone()};
        self.jobs.push(Job::Sampling(Box::new(SamplingStep::Begin {inverse:job.inverse,size:job.size,nearest:job.nearest,extent:job.source.extent})));
        for coordinate in page_coordinates(bounds) {
            let read=accepted.image(r,&job.source,coordinate)?;
            let view=self.read_input(read);
            let origin=coordinate.map(|v|v*PAGE_SIZE);
            let extent=std::array::from_fn(|axis|(job.source.extent[axis]-origin[axis]).min(PAGE_SIZE));
            self.jobs.push(Job::Sampling(Box::new(SamplingStep::Contribute {source:view,origin:origin.map(|v|v as i32),extent})));
        }
        self.jobs.push(Job::Sampling(Box::new(SamplingStep::Normalize)));
        let output=self.reserve(r)?;
        if request.blend==layer_core::BlendSpace::Perceptual {
            let mut data=[0.;32];data[..4].copy_from_slice(&[0.,0.,PAGE_SIZE as f32,PAGE_SIZE as f32]);
            data[4..8].copy_from_slice(&[PAGE_SIZE as f32,PAGE_SIZE as f32,0.,0.]);
            data[8]=1.;data[9]=1.;data[31]=Convert::Encode.code();
            self.jobs.push(Job::Draw {target:self.pool[output].view.clone(),sources:[view,r.empty_view.clone(),r.empty_view.clone(), r.empty_paper_view.clone()],data,over:false,clip:None,source_target:None,edges:None,pipeline:None});
        } else {
            self.jobs.push(Job::Copy {source:texture,source_origin:[0;2],destination:self.pool[output].texture.clone(),origin:[0;2],width:PAGE_SIZE,height:PAGE_SIZE});
        }
        Ok(output)
    }

}

#[cfg(test)]
pub(crate) fn decode_prepared(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    source: &Arc<SourceImage>, coordinate: [u32; 2], pixels: sources::PreparedSourcePixels,
) -> Result<crate::source_access::RawTile, GpuRasterError> {
    decode_prepared_for(r,encoder,source,coordinate,pixels,sources::SourceUse::Display,Class::Visible)
}
pub(crate) fn decode_prepared_for(r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,
    source:&Arc<SourceImage>,coordinate:[u32;2],pixels:sources::PreparedSourcePixels,use_:sources::SourceUse,class:Class,
)->Result<crate::source_access::RawTile,GpuRasterError> {
    let mut requested=sources::Inputs::default();requested.image_for(source,coordinate,r.document_color().space,use_);
    let inputs=requested.accept_query(r,None)?;
    let read=sources::accepted_image(&inputs,source,coordinate)?;
    encode_prepared(r,encoder,inputs,read,pixels,class)
}

pub(crate) fn decode_prepared_raster(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    blob: &Arc<layer_core::raster::TileBlob>, space: layer_core::color::RgbSpace, destination: layer_core::color::RgbSpace,
    pixels: sources::PreparedSourcePixels, class: Class,
) -> Result<crate::source_access::RawTile, GpuRasterError> {
    let mut requested=sources::Inputs::default();requested.raster(blob.clone(),space,destination);
    let inputs=requested.accept_query(r,None)?;
    let read=sources::accepted_raster(&inputs,blob,space,destination)?;
    encode_prepared(r,encoder,inputs,read,pixels,class)
}

#[cfg(test)]
pub(crate) fn promote(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    tile: crate::source_access::RawTile,
) -> Result<crate::source_access::RawTile, GpuRasterError> {
    if tile.texture.format() != wgpu::TextureFormat::Rgba32Float
        && !r.compilation.require([r.scene_pipelines.source.for_format(tile.texture.format())], startup::VALIDATION) {
        return Err(GpuRasterError::NotReady);
    }
    let result = r.source_tiles.borrow_mut().promote(r, tile);
    r.source_tiles.get_mut().flush_retired(encoder);
    let (tile, pending) = result?;
    if let Some(pending) = pending { sources::encode_pending(r, encoder, pending, Class::Visible)?; }
    Ok(tile)
}

fn encode_prepared(r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,
    inputs:sources::AcceptedInputs,read:crate::source_access::SourceRead,pixels:sources::PreparedSourcePixels,class:Class,
) -> Result<crate::source_access::RawTile, GpuRasterError> {
    r.source_tiles.get_mut().flush_retired(encoder);
    for pending in &read.pending {pending.set_prepared(Some(pixels.clone()));}
    if read.pending.iter().any(|pending|!pending.recorded()) && r.source_tiles.borrow().uploads_full(class) {Scene::submit_source_uploads(r,encoder,class)?;}
    inputs.track(encoder);
    if !read.encode(r,encoder,class)? {return Err(GpuRasterError::NotReady);}
    Ok(read.tile)
}


#[cfg(test)]
#[path = "objects_tests.rs"]
mod tests;
