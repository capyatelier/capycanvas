use super::*;
use layer_core::authored::{Affine64, ImageInterpolation, ImageObjectHandle, PortableId};
use layer_core::color::source::SourceImage;
use std::sync::Weak;

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
    pub preview: bool,
}
impl ObjectKey {
    pub fn source(&self) -> Option<Arc<SourceImage>> { self.source.0.upgrade() }
    pub fn same_authored(&self, other: &Self) -> bool { self.handle == other.handle && self.image == other.image && self.source == other.source && self.pose == other.pose && self.nearest == other.nearest }
    pub fn new(scene: SceneView<'_>, owner: OccurrenceHandle, handle: ImageObjectHandle) -> Self {
        let object = scene.object(handle).unwrap();
        let affine = placement(scene, owner, object.affine);
        let pose = placement(scene.with_offset64(scene.evaluation_offset64().map(|value| -value)), owner, object.affine).0;
        Self { handle, image: object.image.id(), source: SourceIdentity(Arc::downgrade(object.image.storage())),
            affine: affine.0.map(f64::to_bits), pose: pose.map(f64::to_bits), nearest: object.interpolation == ImageInterpolation::Nearest, preview: false }
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
    if before.artwork().objects.same_root(&after.artwork().objects) && before.artwork().object_layers.same_root(&after.artwork().object_layers) { return None; }
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
        let before_children = &before.object_layer(owner)?.children;
        let after_children = &after.object_layer(owner)?.children;
        let reordered = before_children != after_children;
        for scene in [before, after] {
            if !scene.visible(owner) { continue; }
            for &handle in &scene.object_layer(owner)?.children {
                let object = scene.object(handle)?;
                if !object.visible { continue; }
                if !reordered && before.object(handle)?.visible == after.object(handle)?.visible
                    && ObjectKey::new(before, owner, handle) == ObjectKey::new(after, owner, handle) { continue; }
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
    pub preview: bool,
    pub live: bool,
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
    pub preview: bool,
    pub live: bool,
    pub context: layer_core::color::RgbSpace,
    pub display: bool,
}
impl CollectionJob {
    pub fn same(&self, other: &Self) -> bool {
        self.owner == other.owner && self.window.origin == other.window.origin && self.window.side == other.window.side
            && self.window.size == other.window.size && self.blend == other.blend && self.preview == other.preview && self.context == other.context && self.display == other.display
            && (Arc::ptr_eq(&self.authored,&other.authored) || (self.authored.len() == other.authored.len() && self.authored.iter().zip(other.authored.iter()).all(|(a,b)| a.same_authored(b))))
    }
    pub fn child(&self, index: usize, output: wgpu::TextureView) -> Result<ObjectJob, GpuRasterError> {
        let key = self.keys[index].clone();
        let source = key.source().ok_or(GpuRasterError::InvalidExtent)?;
        let inverse = Affine64(key.pose.map(f64::from_bits)).inverse().ok_or(GpuRasterError::InvalidTransform("Invalid image object affine"))?
            .compose(Affine64([self.window.side,0.,0.,self.window.side,self.window.origin[0],self.window.origin[1]])).0;
        Ok(ObjectJob {nearest:key.nearest,key,source,inverse,size:self.window.size,output,preview:false,live:self.live})
    }
}

impl Scene {
    pub(crate) fn live_objects_cold(&mut self, r: &mut WgpuRasterizer, scene: SceneView<'_>, bounds: DocRect, side: f64) -> Result<bool, GpuRasterError> {
        if self.object_results.pending() { return Ok(true); }
        self.object_spatial.prepare(scene);
        let size = std::array::from_fn(|axis| ((bounds.max[axis] - bounds.min[axis]) as f64 / side).ceil().max(1.) as u32);
        let mapping = Affine64([side, 0., 0., side, bounds.min[0] as f64, bounds.min[1] as f64]);
        let level = side.max(1.).log2().ceil() as u32;
        for &owner in scene.order() {
            if !scene.visible(owner) || scene.object_layer(owner).is_none() { continue; }
            let Some(content) = self.object_spatial.content(scene, owner) else { continue; };
            for key in content.query_at(bounds, level) {
                let object = scene.object(key.handle).ok_or(GpuRasterError::InvalidExtent)?;
                let inverse = key.placement().inverse().ok_or(GpuRasterError::InvalidTransform("Invalid image object affine"))?.compose(mapping).0;
                let large = requires_refinement(inverse, size, key.nearest, object.image.extent)?;
                let preview = !key.nearest && (r.moving_layer == Some(owner) || large);
                if large && !preview { return Ok(true); }
                if preview {
                    if r.moving_images.lookup(key.image, object.image.storage(), crate::object_image_mips::MovingImages::level(inverse)).is_none() { return Ok(true); }
                } else {
                    let source = crate::object_sampling::ObjectSampler::source_bounds(inverse, size, key.nearest, false, object.image.extent)?;
                    for coordinate in page_coordinates(source) { if decode_live(r, key.image, object.image.storage(), coordinate)?.is_none() { return Ok(true); } }
                }
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
    pub(super) fn object_image(&mut self, r: &WgpuRasterizer, scene: SceneView<'_>, key: &ObjectKey,
        window: ObjectWindow, output: wgpu::TextureView,
    ) -> Result<(), GpuRasterError> {
        let ObjectWindow { origin, side, size } = window;
        let object = scene.object(key.handle).ok_or(GpuRasterError::InvalidExtent)?;
        let output_to_document = Affine64([side, 0., 0., side, origin[0], origin[1]]);
        let inverse = key.placement().inverse().ok_or(GpuRasterError::InvalidTransform("Invalid image object affine"))?
            .compose(output_to_document).0;
        let mut preview = key.preview && (!self.object_display || r.moving_images.admitted(key.image, object.image.storage()));
        if preview && self.object_display && let Some(source) = r.moving_images.lookup(key.image, object.image.storage(), crate::object_image_mips::MovingImages::level(inverse)) {
            let mapping = inverse.map(|value| value / f64::from(1u32 << source.level));
            preview = crate::object_sampling::ObjectSampler::dispatches_single_source(mapping, size, false, true, source.extent, 64)? <= 64;
        }
        let mut key = key.clone(); key.preview = preview;
        self.jobs.push(Job::Object(Box::new(ObjectJob { nearest: key.nearest, key, source: object.image.storage().clone(), inverse, size, output, preview, live: self.object_display || self.object_query })));
        Ok(())
    }

    pub(super) fn collection_job(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, owner: OccurrenceHandle,
        content: object_spatial::Content, window: ObjectWindow, preview: bool,
    ) -> CollectionJob {
        let level=window.side.max(1.).log2().ceil() as u32;
        let bounds=DocRect {min:window.origin.map(|v|v.floor() as i64),max:std::array::from_fn(|i|(window.origin[i]+f64::from(window.size[i])*window.side).ceil() as i64)};
        let keys=content.query_at(bounds,level).into();
        let offset=packet.scene.evaluation_offset64();
        let global=ObjectWindow {origin:std::array::from_fn(|i|window.origin[i]-offset[i]),..window};
        CollectionJob {owner,authored:content.keys(),keys,window:global,blend:packet.blend_space,preview,live:self.object_display||self.object_query,
            context:r.device.working_space(),display:self.object_display}
    }
    pub(super) fn collection_direct(&self, r: &WgpuRasterizer, job: &CollectionJob) -> Result<bool,GpuRasterError> {
        let mut passes=0;
        for index in 0..job.keys.len() {
            let child=job.child(index,r.empty_view.clone())?;
            if child.nearest {return Ok(false);}
            if job.preview {
                let Some(source)=r.moving_images.lookup(child.key.image,&child.source,crate::object_image_mips::MovingImages::level(child.inverse)) else {return Ok(false);};
                let inverse=child.inverse.map(|v|v/f64::from(1u32<<source.level));
                passes+=crate::object_sampling::ObjectSampler::dispatches_single_source(inverse,child.size,false,true,source.extent,64)?+2;
            } else {
                passes+=crate::object_sampling::ObjectSampler::dispatches(child.inverse,child.size,false,false,child.source.extent,8)?+2;
                if job.live {for coordinate in page_coordinates(crate::object_sampling::ObjectSampler::source_bounds(child.inverse,child.size,false,false,child.source.extent)?) {
                    if r.source_tiles.borrow().prepared_view(&child.source,coordinate).is_none() {return Ok(false);}
                }}
            }
            if passes>if job.preview {64} else {8} {return Ok(false);}
        }
        Ok(true)
    }
    pub(super) fn object_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, owner: OccurrenceHandle,
        coordinate: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let content=self.object_spatial.content(packet.scene,owner).ok_or(GpuRasterError::InvalidExtent)?;
        let window=ObjectWindow {origin:coordinate.map(|v|f64::from(v*PAGE_SIZE)),side:1.,size:[PAGE_SIZE;2]};
        let mut request=self.collection_job(r,packet,owner,content,window,false);
        if request.keys.is_empty() {return Ok(self.alloc(r,wgpu::Color::TRANSPARENT));}
        let canonical=if self.object_display||self.object_query {self.object_results.resolve_collection(r,packet.scene,&request)?}
            else {self.exact_object_results.resolve_collection(r,packet.scene,&request)?};
        if let Some(view)=canonical {
            let output=self.reserve(r);
            self.jobs.push(Job::Copy {source:view.texture().clone(),source_origin:[0;2],destination:self.pool[output].texture.clone(),origin:[0;2],width:PAGE_SIZE,height:PAGE_SIZE});
            return Ok(output);
        }
        request.preview=self.object_display;
        if !request.preview || !self.collection_direct(r,&request)? {
            self.discard_unencoded_jobs();return Err(GpuRasterError::DeferredObjectWork);
        }
        let mut output=self.alloc(r,wgpu::Color::TRANSPARENT);
        for key in request.keys.iter() {
            let input=self.reserve(r);
            let mut key=key.clone();key.preview=true;
            self.object_image(r,packet.scene,&key,window,self.pool[input].view.clone())?;
            let input=self.converted(r,input,Convert::layers(packet));
            output=self.combine(r,input,output,1.,layer_core::LayerBlend::Normal,false,packet.blend_space);
        }
        Ok(output)
    }

}

pub(super) fn encode(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, job: &ObjectJob) -> Result<(), GpuRasterError> {
    if job.live && job.nearest { return Err(GpuRasterError::DeferredObjectWork); }
    let sampler = r.scene_pipelines.objects.clone();
    if job.preview && !job.nearest && let Some(source) = r.moving_images.lookup(job.key.image, &job.source, crate::object_image_mips::MovingImages::level(job.inverse)) {
        let inverse = job.inverse.map(|value| value / f64::from(1u32 << source.level));
        let accumulation = if job.live && !r.snapshot_worker { sampler.create_transient(&r.device,encoder,crate::object_sampling::SamplingRequest {inverse,size:job.size,nearest:false,preview:true},&job.output,&r.empty_view)? }
            else { sampler.create(&r.device,crate::object_sampling::SamplingRequest {inverse,size:job.size,nearest:false,preview:true},&job.output,&r.empty_view)? };
        sampler.initialize(encoder, &accumulation);
        let mut steps = 0;
        encode_phase(r, encoder, &sampler, &accumulation, &crate::object_sampling::SamplingPhase::Denominator, &mut steps)?;
        encode_phase(r, encoder, &sampler, &accumulation, &crate::object_sampling::SamplingPhase::Contribute { source: &source.view, origin: [0; 2], extent: source.extent }, &mut steps)?;
        encode_phase(r, encoder, &sampler, &accumulation, &crate::object_sampling::SamplingPhase::Normalize, &mut steps)?;
        return Ok(());
    }
    let accumulation = if job.live && !r.snapshot_worker { sampler.create_transient(&r.device,encoder,crate::object_sampling::SamplingRequest {inverse:job.inverse,size:job.size,nearest:job.nearest,preview:job.preview},&job.output,&r.empty_view)? }
        else { sampler.create(&r.device,crate::object_sampling::SamplingRequest {inverse:job.inverse,size:job.size,nearest:job.nearest,preview:job.preview},&job.output,&r.empty_view)? };
    if job.nearest {
        let length = job.size[0].checked_mul(job.size[1]).ok_or(GpuRasterError::SizeOverflow)?;
        for first in (0..length).step_by(65536) {
            let bytes = crate::object_sampling::prepare_nearest_coordinates(job.inverse, job.size, first, (length - first).min(65536)).map_err(GpuRasterError::Color)?;
            sampler.set_nearest_coordinates(encoder, &mut r.uploads, &accumulation, first, &bytes)?;
        }
    }
    sampler.initialize(encoder, &accumulation);
    let mut steps = 0;
    encode_phase(r, encoder, &sampler, &accumulation, &crate::object_sampling::SamplingPhase::Denominator, &mut steps)?;
    let bounds = accumulation.source_bounds(job.source.extent);
    for coordinate in page_coordinates(bounds) {
        let tile = if job.live { decode_live(r, job.key.image, &job.source, coordinate)?.ok_or(GpuRasterError::DeferredObjectWork)? }
            else { decode(r, encoder, &job.source, coordinate)? };
        let origin = coordinate.map(|v| v * PAGE_SIZE);
        let extent = std::array::from_fn(|axis| (job.source.extent[axis] - origin[axis]).min(PAGE_SIZE));
        encode_phase(r, encoder, &sampler, &accumulation,
            &crate::object_sampling::SamplingPhase::Contribute { source: &tile.view, origin: origin.map(|v| v as i32), extent }, &mut steps)?;
    }
    encode_phase(r, encoder, &sampler, &accumulation, &crate::object_sampling::SamplingPhase::Normalize, &mut steps)?;
    Ok(())
}

fn encode_phase(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, sampler: &crate::object_sampling::ObjectSampler,
    accumulation: &crate::object_sampling::ObjectAccumulator, phase: &crate::object_sampling::SamplingPhase<'_>, steps: &mut u32,
) -> Result<(), GpuRasterError> {
    for step in 0.. {
        if r.snapshot_cancelled.as_ref().is_some_and(|cancelled| cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
            return Err(GpuRasterError::Color("Snapshot capture cancelled".into()));
        }
        if !sampler.encode_step(&r.device, encoder, &mut r.uploads, accumulation, phase, step)? { break; }
        *steps += 1;
        if r.snapshot_worker && *steps >= 8 { Scene::submit_chunk(r, encoder, "canonical image sampling chunk")?; *steps = 0; }
    }
    Ok(())
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

pub(super) fn requires_refinement(inverse: [f64; 6], size: [u32; 2], nearest: bool, extent: [u32; 2]) -> Result<bool, GpuRasterError> {
    Ok(nearest || crate::object_sampling::ObjectSampler::dispatches(inverse, size, nearest, false, extent, 8)? > 8)
}

pub(super) fn live_ready(r: &mut WgpuRasterizer, job: &ObjectJob) -> Result<bool, GpuRasterError> {
    if job.preview && !job.nearest {
        let Some(source) = r.moving_images.lookup(job.key.image, &job.source, crate::object_image_mips::MovingImages::level(job.inverse)) else { return Ok(false); };
        let inverse = job.inverse.map(|value| value / f64::from(1u32 << source.level));
        return Ok(crate::object_sampling::ObjectSampler::dispatches_single_source(inverse, job.size, false, true, source.extent, 64)? <= 64);
    }
    if requires_refinement(job.inverse, job.size, job.nearest, job.source.extent)? { return Ok(false); }
    let bounds = crate::object_sampling::ObjectSampler::source_bounds(job.inverse, job.size, job.nearest, job.preview, job.source.extent)?;
    for coordinate in page_coordinates(bounds) { if decode_live(r, job.key.image, &job.source, coordinate)?.is_none() { return Ok(false); } }
    Ok(true)
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
        if let Some(values) = pending.data { for (destination, value) in data.chunks_exact_mut(4).zip(values) { destination.copy_from_slice(&value.to_ne_bytes()); } }
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
