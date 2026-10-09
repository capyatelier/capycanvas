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
    if before.artwork().objects.same_root(&after.artwork().objects) { return None; }
    if before.order() != after.order() || before.composition() != after.composition() { return None; }
    let mut damage = scale::Damage::EMPTY;
    let mut changed = false;
    for &owner in after.order() {
        if before.paint_source(owner).map(|p| p.raster.identity()) != after.paint_source(owner).map(|p| p.raster.identity())
            || before.mask(owner).map(|(_, p)| p.raster.identity()) != after.mask(owner).map(|(_, p)| p.raster.identity()) { return None; }
        let old = metadata::Metadata::new(before, owner);
        let new = metadata::Metadata::new(after, owner);
        if old == new { continue; }
        if after.object_layer(owner).is_none() || !old.same_object_layer(&new) { return None; }
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
    pub(crate) fn live_objects_cold(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, bounds: DocRect, side: f64) -> Result<bool, GpuRasterError> {
        self.object_spatial.prepare(scene);
        let mapping = Affine64([side, 0., 0., side, bounds.min[0] as f64, bounds.min[1] as f64]);
        let level = side.max(1.).log2().ceil() as u32;
        for &owner in scene.order() {
            if !scene.visible(owner) || scene.object_layer(owner).is_none() { continue; }
            let Some(content) = self.object_spatial.content(scene, owner) else { continue; };
            for key in content.query_at(bounds, level) {
                let object = scene.object(key.handle).ok_or(GpuRasterError::InvalidExtent)?;
                let inverse = key.placement().inverse().ok_or(GpuRasterError::InvalidTransform("Invalid image object affine"))?.compose(mapping).0;
                if Self::preview_source(r, key.image, object.image.storage(), key.nearest, inverse).is_none() { return Ok(true); }
            }
        }
        Ok(false)
    }
    pub(crate) fn invalidate_object_previews(&mut self, extent: [u32; 2]) { self.object_source_damage = PixelRect::full(extent).into(); }
    pub(crate) fn decode_prepared(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        source: &Arc<SourceImage>, coordinate: [u32; 2], pixels: sources::PreparedSourcePixels,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> { decode_prepared(r, encoder, source, coordinate, pixels) }
    pub(crate) fn objects_pending(&self) -> bool { self.object_results.pending() }
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
            context:r.device.working_space(),display:self.object_display}
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
                let cache = if self.object_display || self.object_query { &mut self.object_results } else { &mut self.exact_object_results };
                match cache.prepare_collection(r, packet.scene, &request) {
                    Ok(complete) => ready &= complete,
                    Err(GpuRasterError::SourceWorkingSetExceeded) => return Ok(false),
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(ready)
    }
    pub(super) fn canonical_cover(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, content: &object_spatial::Content,
        request: &CollectionJob,
    ) -> Result<Option<Vec<(wgpu::TextureView, DocRect)>>, GpuRasterError> {
        let (pieces, mut ready, missing) = self.object_results.cover(request);
        let offset = packet.scene.evaluation_offset64();
        for rect in missing {
            let size = rect.size()?.map(|v| (f64::from(v) / request.window.side) as u32);
            let window = ObjectWindow { origin: std::array::from_fn(|i| rect.min[i] as f64 + offset[i]), side: request.window.side, size };
            let mut part = self.collection_job(r, packet, content.owner(), content.clone(), window);
            part.live = request.live; part.display = request.display;
            if part.keys.is_empty() { continue; }
            self.object_results.resolve_collection(r, packet.scene, &part)?;
            ready = false;
        }
        Ok(ready.then_some(pieces))
    }
    fn preview_source(r: &WgpuRasterizer, id: PortableId, source: &Arc<SourceImage>, nearest: bool, inverse: [f64; 6]) -> Option<crate::object_image_mips::MovingSource> {
        let level = if nearest { 0 } else { crate::object_image_mips::MovingImages::level(inverse) };
        r.moving_images.lookup(id, source, level).filter(|mip| !nearest || mip.level == 0)
    }
    pub(super) fn collection_preview(r: &WgpuRasterizer, job: &CollectionJob) -> Result<Option<CollectionPreview>, GpuRasterError> {
        let mut identities: Vec<(PortableId, SourceIdentity, u32)> = Vec::new();
        let mut sources = Vec::new();
        let mut objects = Vec::with_capacity(job.keys.len());
        for index in 0..job.keys.len() {
            let child = job.child(index, job.window, r.empty_view.clone())?;
            let Some(mip) = Self::preview_source(r, child.key.image, &child.source, child.nearest, child.inverse) else { return Ok(None); };
            let inverse = child.inverse.map(|value| value / f64::from(1u32 << mip.level));
            if crate::object_sampling::mapping(inverse, job.window.size).is_err() { return Ok(None); }
            let identity = (child.key.image, child.key.source.clone(), mip.level);
            let source = identities.iter().position(|known| *known == identity).unwrap_or_else(|| { identities.push(identity); sources.push(mip.view.clone()); sources.len() - 1 });
            objects.push(crate::object_sampling::PreviewSource { source, inverse, extent: mip.extent, nearest: child.nearest });
        }
        Ok(Some((sources, objects)))
    }
    pub(crate) fn object_thumbnail(&mut self, r: &mut WgpuRasterizer, owner: OccurrenceHandle, grid: display_mips::Plan,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<(wgpu::Texture, wgpu::TextureView)>, GpuRasterError> {
        let frame = r.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
        let packet = frame.packet(r.document_extent);
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
        if let Some((sources, objects)) = Self::collection_preview(r, &request)? {
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
        let mut results = std::mem::take(&mut self.object_results);
        let advanced = results.advance(r, self, encoder, false);
        self.object_results = results;
        let canonical = advanced.and_then(|_| self.object_results.resolve_collection(r, packet.scene, &request));
        self.object_query = false;
        let Some(view) = canonical? else { return Ok(None); };
        encoder.copy_texture_to_texture(view.texture().as_image_copy(), target.0.as_image_copy(),
            wgpu::Extent3d { width: grid.size[0], height: grid.size[1], depth_or_array_layers: 1 });
        Ok(Some(target))
    }
    pub(super) fn object_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, owner: OccurrenceHandle,
        coordinate: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let content=self.object_spatial.content(packet.scene,owner).ok_or(GpuRasterError::InvalidExtent)?;
        let window=ObjectWindow {origin:coordinate.map(|v|f64::from(v*PAGE_SIZE)),side:1.,size:[PAGE_SIZE;2]};
        let request=self.collection_job(r,packet,owner,content,window);
        if request.keys.is_empty() {return Ok(self.alloc(r,wgpu::Color::TRANSPARENT));}
        let canonical=if self.object_display||self.object_query {self.object_results.resolve_collection(r,packet.scene,&request)?}
            else {self.exact_object_results.resolve_collection(r,packet.scene,&request)?};
        if let Some(view)=canonical {
            let output=self.reserve(r);
            self.jobs.push(Job::Copy {source:view.texture().clone(),source_origin:[0;2],destination:self.pool[output].texture.clone(),origin:[0;2],width:PAGE_SIZE,height:PAGE_SIZE});
            return Ok(output);
        }
        let Some((sources,objects))=(if self.object_display {Self::collection_preview(r,&request)?} else {None}) else {
            self.discard_unencoded_jobs();return Err(GpuRasterError::DeferredObjectWork);
        };
        let output=self.reserve(r);
        let scratch=(crate::object_sampling::CollectionPreview::passes(&objects)>1).then(||self.reserve(r));
        self.jobs.push(Job::Collection(Box::new(crate::object_sampling::CollectionPreview {sources,objects,size:window.size,
            output:self.pool[output].view.clone(),scratch:scratch.map(|slot|self.pool[slot].view.clone()),encode:packet.blend_space==layer_core::BlendSpace::Perceptual})));
        if let Some(slot)=scratch {self.free(slot);}
        Ok(output)
    }

}

pub(super) fn decode(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    source: &Arc<layer_core::color::source::SourceImage>, coordinate: [u32; 2],
) -> Result<crate::source_access::RawTile, GpuRasterError> {
    decode_inner(r, encoder, source, coordinate, None)
}

pub(super) fn decode_live(r: &mut WgpuRasterizer, id: PortableId, source: &Arc<SourceImage>, coordinate: [u32; 2],
) -> Result<Option<crate::source_access::RawTile>, GpuRasterError> {
    if let Some(view) = r.source_tiles.borrow().prepared_view(source, coordinate) {
        return Ok(Some(crate::source_access::RawTile { texture: view.texture().clone(), view: view.clone() }));
    }
    r.request_image_decode(id, source.clone(), coordinate)?;
    Ok(None)
}

pub(crate) fn decode_prepared(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    source: &Arc<SourceImage>, coordinate: [u32; 2], pixels: sources::PreparedSourcePixels,
) -> Result<crate::source_access::RawTile, GpuRasterError> {
    decode_inner(r, encoder, source, coordinate, Some(pixels))
}

fn decode_inner(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    source: &Arc<SourceImage>, coordinate: [u32; 2], pixels: Option<sources::PreparedSourcePixels>,
) -> Result<crate::source_access::RawTile, GpuRasterError> {
    let (tile, pending) = r.source_tiles.borrow_mut().plan(r, source, coordinate)?;
    if let Some(mut pending) = pending {
        pending.prepared_pixels = pixels;
        if r.source_tiles.borrow().uploads_full() { Scene::submit_source_uploads(r, encoder)?; }
        let records = r.device.create_buffer(&wgpu::BufferDescriptor { label: Some("object image decode records"),
            size: 144, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let binding = uniform_binding(&r.device, &r.scene_pipelines.uniforms, &records);
        let mut data = [0; 144];
        if let Some(values) = pending.data { for (destination, value) in data.as_chunks_mut::<4>().0.iter_mut().zip(values) { destination.copy_from_slice(&value.to_ne_bytes()); } }
        r.uploads.write_at(encoder, &records, 0, &data)?;
        let bytes = r.source_tiles.get_mut().encode(&r.device, &mut r.uploads, &r.scene_pipelines.source, encoder, &pending, &binding, 0)?;
        let in_flight = r.source_tiles.borrow().charge_upload(encoder, bytes);
        r.metrics.source_upload_peak_bytes = r.metrics.source_upload_peak_bytes.max(in_flight);
    }
    Ok(tile)
}

#[cfg(test)]
#[path = "objects_tests.rs"]
mod tests;
