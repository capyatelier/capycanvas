//! Browser worker transport. File bytes are validated in the file worker; live
//! capture compression shares the exact native tile codec and pixel descriptors.
use super::*;
use layer_core::{
    color::PixelDescriptor,
    raster::{TILE_SIZE, TileBlob},
};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen_futures::JsFuture;

thread_local! { static WORKER: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) }; }

#[wasm_bindgen]
pub fn configure_raster_worker(worker: js_sys::Function) {
    WORKER.with(|slot| *slot.borrow_mut() = Some(worker));
}

#[wasm_bindgen]
pub fn raster_worker_lookup(name:&str,bytes:&[u8])->Result<JsValue,JsValue> {
    let resource=layer_core::Lut3d::parse_cube_named(bytes,name).map_err(js)?;
    let result=js_sys::Object::new();
    js_sys::Reflect::set(&result,&js("metadata"),&js(serde_json::to_string(&(&resource,resource.admitted_spaces())).map_err(js)?))?;
    js_sys::Reflect::set(&result,&js("bytes"),&js_sys::Uint8Array::from(resource.payload().unwrap()))?;
    Ok(result.into())
}

pub(super) fn call(
    operation: &str,
    metadata: &str,
    buffers: &js_sys::Array,
) -> Result<js_sys::Promise, JsValue> {
    let request = js_sys::Object::new();
    js_sys::Reflect::set(&request, &js("operation"), &js(operation))?;
    js_sys::Reflect::set(&request, &js("metadata"), &js(metadata))?;
    js_sys::Reflect::set(&request, &js("buffers"), buffers)?;
    self::request(&request)
}

pub(super) fn request(request:&JsValue)->Result<js_sys::Promise,JsValue> {
    WORKER
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .ok_or_else(|| js("Raster worker unavailable"))?
                .call1(&JsValue::NULL, request)
        })?
        .dyn_into()
}

/// The callback stays alive until the isolated output worker has settled.
pub(super) async fn call_cancellable(
    operation: &str, metadata: &str, buffers: &js_sys::Array,
    control: layer_render_wgpu::snapshot::CaptureControl,
) -> Result<JsValue, JsValue> {
    let cancelled = wasm_bindgen::closure::Closure::<dyn Fn() -> bool>::new(move || control.is_cancelled());
    let request = js_sys::Object::new();
    js_sys::Reflect::set(&request, &js("operation"), &js(operation))?;
    js_sys::Reflect::set(&request, &js("metadata"), &js(metadata))?;
    js_sys::Reflect::set(&request, &js("buffers"), buffers)?;
    js_sys::Reflect::set(&request, &js("cancelled"), cancelled.as_ref())?;
    let promise = WORKER.with(|slot| slot.borrow().as_ref()
        .ok_or_else(|| js("Raster worker unavailable"))?.call1(&JsValue::NULL, &request))?;
    let result = JsFuture::from(promise.dyn_into::<js_sys::Promise>()?).await;
    drop(cancelled);
    result
}

pub(super) fn install(renderer: &mut WgpuRasterizer) {
    renderer.set_complete_display_allowance(artwork_transfer::photo_memory_budget().encode_bytes as u64);
    renderer.set_analysis_backing_waiter(Rc::new(|scene, control| Box::pin(async move {
        if control.is_cancelled() { return Err("Analysis cancelled".into()); }
        artwork_transfer::wait_backing_cancellable(&layer_render_wgpu::snapshot::SnapshotGpu::scoped_transfer_artwork(&scene,&[]), Some(&control)).await.map_err(|e| format!("{e:?}"))?;
        if control.is_cancelled() { return Err("Analysis cancelled".into()); }
        Ok(())
    })));
    renderer.set_browser_image_decoder(Rc::new(|source, coordinate, destination| Box::pin(async move {
        documents::yield_browser().await.map_err(|e|format!("{e:?}"))?;
        let tile=source.tiles.get(&coordinate).ok_or("Missing immutable image tile")?;
        let start=js_sys::Date::now();
        while !tile.compressed_ready()? {
            if js_sys::Date::now()-start>30_000. {return Err("Image backing timed out".into());}
            documents::yield_browser().await.map_err(|e|format!("{e:?}"))?;
        }
        let mut profiles=Vec::new();
        let profile=layer_core::color::ProfileReference::detach(&source.interpretation.profile,&mut profiles);
        let interpretation=source.interpretation.clone().with_profile(profile);
        let buffers=js_sys::Array::new();
        buffers.push(&js_sys::Uint8Array::from(tile.compressed()?.as_ref()));
        let mut chunks=Vec::new();
        for profile in profiles {
            let mut count=0;
            for block in profile.chunks(layer_core::package::MAX_RANGE_BYTES) {
                buffers.push(&js_sys::Uint8Array::from(block));count+=1;
                documents::yield_browser().await.map_err(|e|format!("{e:?}"))?;
            }
            chunks.push(count);
        }
        let metadata=serde_json::to_string(&ImageDecode {interpretation,destination,extent:source.extent,coordinate,profiles:chunks}).map_err(|e|e.to_string())?;
        let result=JsFuture::from(call("image-decode",&metadata,&buffers).map_err(|e|format!("{e:?}"))?).await.map_err(|e|format!("{e:?}"))?;
        let bytes=result.dyn_into::<js_sys::Uint8Array>().map_err(|_|"Missing prepared image pixels")?.to_vec();
        if matches!(source.interpretation.profile,layer_core::color::ColorProfile::Builtin(_)) {
            if bytes.len()!=(TILE_SIZE*TILE_SIZE) as usize*4*source.interpretation.depth.bytes() {return Err("Invalid prepared source tile extent".into());}
            Ok(layer_render_wgpu::PreparedImagePixels::NativeSamples(std::sync::Arc::new(bytes)))
        } else {
            if bytes.len()!=(TILE_SIZE*TILE_SIZE) as usize*16 {return Err("Invalid prepared working tile extent".into());}
            Ok(layer_render_wgpu::PreparedImagePixels::PremultipliedWorkingPixels(std::sync::Arc::new(bytes)))
        }
    })));
    renderer.set_browser_nearest_coordinate_decoder(Rc::new(|inverse,size,first,count| Box::pin(async move {
        if count>65536 {return Err("Oversized Nearest coordinate request".into());}
        let metadata=serde_json::to_string(&NearestCoordinates {inverse,size,first,count}).map_err(|e|e.to_string())?;
        let result=JsFuture::from(call("nearest-coordinates",&metadata,&js_sys::Array::new()).map_err(|e|format!("{e:?}"))?).await.map_err(|e|format!("{e:?}"))?;
        let bytes=result.dyn_into::<js_sys::Uint8Array>().map_err(|_|"Missing Nearest coordinates")?;
        if bytes.length() as usize!=count as usize*8 {return Err("Invalid prepared Nearest coordinate extent".into());}
        Ok(std::sync::Arc::new(bytes.to_vec()))
    })));
    renderer.set_snapshot_worker(Rc::new(|request, control| Box::pin(async move {
        if control.is_cancelled() { return Err("Operation cancelled".into()); }
        let mut transform = None;
        let mut additional = Vec::new();
        let (scene, selection, task) = match request {
            layer_render::SnapshotRequest::LevelsStatistics(query) => {
                let (scene, source) = query_scene(&query)?;
                (scene, query.selection, SnapshotTask::LevelsStatistics(source))
            }
            layer_render::SnapshotRequest::ArtworkStatistics(request) => {
                let (scene, source) = query_scene(&request.query)?;
                (scene, request.query.selection, SnapshotTask::ArtworkStatistics {source, preview:request.preview, selection:request.selection,waveform:request.waveform})
            }
            layer_render::SnapshotRequest::ArtworkSample(request) => {
                let (scene, source) = query_scene(&request.query)?;
                (scene, request.query.selection, SnapshotTask::ArtworkSample {source, position:request.position, width:request.width})
            }
            layer_render::SnapshotRequest::Bounds(request) => (layer_render_wgpu::snapshot::SnapshotGpu::bounds_source_scene(&request)?,request.selection,SnapshotTask::Bounds(request.scope)),
            layer_render::SnapshotRequest::TransformPixels(plan) => {
                let task = SnapshotTask::TransformPixels {target:plan.target, map:plan.map.clone(), source:plan.source};
                additional.extend(plan.paint.map(layer_core::SourceTarget::Paint).into_iter().chain(plan.coverage.map(layer_core::SourceTarget::Coverage)));
                transform = Some((plan.output, plan.paint, plan.coverage));
                let scene=std::sync::Arc::new((*plan.scene).clone().with_scope(layer_core::SceneScope::Raw(plan.target)));
                (scene, None, task)
            }
            layer_render::SnapshotRequest::Image(capture) => {
                let scene = std::sync::Arc::new((*capture.scene).clone().with_scope(capture.scope.clone()));
                let selection = capture.selection.as_deref().cloned();
                (scene, selection, SnapshotTask::Image {offset:capture.offset, extent:capture.extent, window:capture.window, trim:capture.trim})
            }
            layer_render::SnapshotRequest::Remap(plan) => {
                additional.extend(plan.targets());
                let first = *additional.first().ok_or("Missing moved layers")?;
                let scene = std::sync::Arc::new((*plan.scene).clone().with_scope(layer_core::SceneScope::Raw(first)));
                (scene, None, SnapshotTask::Remap(plan.specs.to_vec()))
            }
        };
        let frozen = FrozenScene::new(&scene);
        let artwork=layer_render_wgpu::snapshot::SnapshotGpu::scoped_transfer_artwork(&scene,&additional);
        let mut packing = std::pin::pin!(artwork_transfer::pack_scene(artwork, selection));
        let packed = std::future::poll_fn(|cx| {
            if control.is_cancelled() { return std::task::Poll::Ready(Err(js("Operation cancelled"))); }
            std::future::Future::poll(packing.as_mut(), cx)
        }).await.map_err(|e| format!("{e:?}"))?;
        let (metadata, buffers) = packed_parts(&packed).map_err(|e| format!("{e:?}"))?;
        let metadata = serde_json::to_string(&(metadata, frozen, &task)).map_err(|e| e.to_string())?;
        let result = call_cancellable("snapshot", &metadata, &buffers, control.clone()).await.map_err(|e| format!("{e:?}"))?;
        if control.is_cancelled() { return Err("Operation cancelled".into()); }
        match task {
            SnapshotTask::LevelsStatistics(..)=>serde_wasm_bindgen::from_value(result).map(layer_render::SnapshotResult::LevelsStatistics).map_err(|e|e.to_string()),
            SnapshotTask::ArtworkStatistics { waveform, .. } => {
                let mut histogram: layer_core::color::histogram::Histogram=serde_wasm_bindgen::from_value(result.clone()).map_err(|e|e.to_string())?;
                if waveform {
                    let counts=js_sys::Reflect::get(&result,&js("waveform_counts")).map_err(|e|format!("{e:?}"))?
                        .dyn_into::<js_sys::Uint32Array>().map_err(|_|"Missing waveform counts")?;
                    if counts.length() as usize!=layer_core::color::histogram::Waveform::WORDS {return Err("Invalid waveform extent".into());}
                    histogram.waveform=Some(layer_core::color::histogram::Waveform {counts:counts.to_vec()});
                }
                Ok(layer_render::SnapshotResult::ArtworkStatistics(histogram))
            },
            SnapshotTask::ArtworkSample { .. } => serde_wasm_bindgen::from_value(result).map(layer_render::SnapshotResult::ArtworkSample).map_err(|e| e.to_string()),
            SnapshotTask::Bounds(..) => serde_wasm_bindgen::from_value(result).map(layer_render::SnapshotResult::Bounds).map_err(|e| e.to_string()),
            SnapshotTask::TransformPixels {..} => {
                let (metadata, buffers) = packed_parts(&result).map_err(|e| format!("{e:?}"))?;
                let artwork = artwork_transfer::unpack(&metadata, buffers).await.map_err(|e| format!("{e:?}"))?;
                let (mut output, paint, coverage) = transform.ok_or("Missing transform output")?;
                install_transformed_rasters(&mut output, &artwork, paint, coverage)?;
                Ok(layer_render::SnapshotResult::TransformPixels(output))
            }
            SnapshotTask::Image {..} => {
                if result.is_null() { return Ok(layer_render::SnapshotResult::Image(None)); }
                let (metadata, buffers) = packed_parts(&result).map_err(|e| format!("{e:?}"))?;
                let artwork = artwork_transfer::unpack(&metadata, buffers).await.map_err(|e| format!("{e:?}"))?;
                let base = artwork.paint.iter().find_map(|(_, _, paint)| paint.base.clone()).ok_or("Missing captured image")?;
                Ok(layer_render::SnapshotResult::Image(Some((base.image.storage().clone(), base.offset.map(i64::from)))))
            }
            SnapshotTask::Remap(specs) => {
                let (metadata, buffers) = packed_parts(&result).map_err(|e| format!("{e:?}"))?;
                let artwork = artwork_transfer::unpack(&metadata, buffers).await.map_err(|e| format!("{e:?}"))?;
                specs.iter().map(|spec| {
                    let (raster, base) = match spec.target {
                        layer_core::SourceTarget::Paint(h) => artwork.paint.get(h).map(|p| (p.raster.clone(), p.base.as_ref())),
                        layer_core::SourceTarget::Coverage(h) => artwork.coverage.get(h).map(|c| (c.raster.clone(), None)),
                        layer_core::SourceTarget::Selection(_) => None,
                    }.ok_or("Missing moved layer")?;
                    let image = spec.base.and(base).map(|base| base.image.clone());
                    Ok((spec.target, image, raster))
                }).collect::<Result<Vec<_>, String>>().map(layer_render::SnapshotResult::Remap)
            }
        }
    })));
    renderer.set_browser_raster_encoder(Rc::new(|bytes, descriptors| {
        Box::pin(async move {
            let metadata = serde_json::to_string(&descriptors).map_err(|e| e.to_string())?;
            let buffers = js_sys::Array::new();
            buffers.push(&js_sys::Uint8Array::from(bytes.as_slice()));
            drop(bytes);
            let promise = call("encode", &metadata, &buffers).map_err(|e| format!("{e:?}"))?;
            let result = JsFuture::from(promise)
                .await
                .map_err(|e| format!("{e:?}"))?;
            let bytes = js_sys::Uint8Array::new(&result).to_vec();
            let mut offset = 0;
            let mut blobs = Vec::with_capacity(descriptors.len());
            for descriptor in descriptors {
                let header = bytes
                    .get(offset..offset + 4)
                    .ok_or("Incomplete raster worker result")?;
                let size = u32::from_le_bytes(header.try_into().unwrap()) as usize;
                offset += 4;
                let fingerprint=bytes.get(offset..offset+32).ok_or("Incomplete raster worker fingerprint")?.try_into().unwrap();
                offset+=32;
                let end = offset.checked_add(size).ok_or("Raster worker size overflow")?;
                let encoded = bytes
                    .get(offset..end)
                    .ok_or("Incomplete raster worker tile")?;
                blobs.push(TileBlob::from_verified_resource_with_encoded_fingerprint(
                    layer_core::PortableId::random(), descriptor, encoded.into(),Some(fingerprint),
                )?);
                offset = end;
            }
            if offset != bytes.len() {
                return Err("Trailing raster worker data".into());
            }
            Ok(blobs)
        })
    }));
}

#[derive(Serialize,Deserialize)]
struct ImageDecode {
    interpretation:layer_core::color::source::SourceInterpretation<layer_core::color::ProfileReference>,
    destination:layer_core::color::RgbSpace,
    extent:[u32;2],
    coordinate:[u32;2],
    profiles:Vec<usize>,
}
#[derive(Serialize,Deserialize)]
struct NearestCoordinates {inverse:[f64;6],size:[u32;2],first:u32,count:u32}
#[wasm_bindgen]
pub fn raster_worker_nearest_coordinates(metadata:&str)->Result<Vec<u8>,JsValue> {
    if metadata.len()>4096 {return Err(js("Oversized Nearest coordinate metadata"));}
    let request:NearestCoordinates=serde_json::from_str(metadata).map_err(js)?;
    layer_render_wgpu::prepare_nearest_coordinates(request.inverse,request.size,request.first,request.count)
        .map(|bytes|bytes.as_ref().clone()).map_err(js)
}
#[wasm_bindgen]
pub fn raster_worker_image_decode(metadata:&str,buffers:js_sys::Array)->Result<Vec<u8>,JsValue> {
    use layer_core::color::source::SourceImage;
    use std::sync::Arc;
    if metadata.len()>16*1024 {return Err(js("Oversized image preparation metadata"));}
    let request:ImageDecode=serde_json::from_str(metadata).map_err(js)?;
    if request.profiles.len()>1 || buffers.length()==0 {return Err(js("Invalid image preparation payloads"));}
    let expected=request.profiles.iter().try_fold(1usize,|sum,count|sum.checked_add(*count)).ok_or_else(||js("Image preparation chunk overflow"))?;
    if expected!=buffers.length() as usize {return Err(js("Incomplete image preparation payloads"));}
    let mut profiles=Vec::<Arc<[u8]>>::new();let mut at=1;
    for count in request.profiles {
        let mut bytes=Vec::new();
        for _ in 0..count {
            let block=buffers.get(at).dyn_into::<js_sys::Uint8Array>()?;at+=1;
            if block.length() as usize>layer_core::package::MAX_RANGE_BYTES || bytes.len()+block.length() as usize>layer_core::color::source::MAX_PROFILE_BYTES {return Err(js("Oversized image profile payload"));}
            bytes.extend_from_slice(&block.to_vec());
        }
        profiles.push(bytes.into());
    }
    let interpretation=request.interpretation.clone().with_profile(request.interpretation.profile.resolve(&profiles).map_err(js)?);
    let compressed=buffers.get(0).dyn_into::<js_sys::Uint8Array>()?;
    if compressed.length() as usize>layer_core::package::MAX_RANGE_BYTES {return Err(js("Oversized image tile payload"));}
    let tile=Arc::new(TileBlob::from_verified_resource(layer_core::PortableId::random(),interpretation.descriptor(),compressed.to_vec().into()).map_err(js)?);
    let source=SourceImage {extent:request.extent,resolution:None,interpretation,tiles:[(request.coordinate,tile)].into()};
    let pixels=layer_render_wgpu::prepare_image_tile(&source,request.coordinate,request.destination,
        &layer_core::raster::DecodedTileCache::new(4*1024*1024),None).map_err(js)?;
    match pixels {
        layer_render_wgpu::PreparedImagePixels::NativeSamples(bytes)|layer_render_wgpu::PreparedImagePixels::PremultipliedWorkingPixels(bytes)=>Ok(bytes.as_ref().clone()),
    }
}

#[wasm_bindgen]
pub fn raster_worker_encode(metadata: &str, bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    let descriptors: Vec<PixelDescriptor> = serde_json::from_str(metadata).map_err(js)?;
    let mut result = Vec::new();
    for tile in encode_tiles(bytes, descriptors).map_err(js)? {
        result.extend_from_slice(&(tile.compressed_len() as u32).to_le_bytes());
        result.extend_from_slice(&tile.encoded_fingerprint().ok_or_else(||js("Missing encoded raster fingerprint"))?);
        result.extend_from_slice(&tile.compressed().map_err(js)?);
    }
    Ok(result)
}

fn encode_tiles(bytes: &[u8], descriptors: Vec<PixelDescriptor>) -> Result<Vec<TileBlob>, String> {
    if bytes.len() > 16 * 1024 * 1024 || descriptors.len() > 256 {
        return Err("Raster worker chunk exceeds its budget".into());
    }
    let mut result = Vec::with_capacity(descriptors.len());
    let mut offset = 0;
    for descriptor in descriptors {
        let size = descriptor.byte_len([TILE_SIZE; 2]).ok_or("Unsupported raster pixels")?;
        let raw = bytes.get(offset..offset + size).ok_or("Incomplete raster worker chunk")?;
        result.push(TileBlob::encode(descriptor, raw)?);
        offset += size;
    }
    if offset != bytes.len() { return Err("Trailing raster worker input".into()); }
    Ok(result)
}

use layer_core::SnapshotSource;
fn query_scene(query: &layer_core::ArtworkQuery) -> Result<(std::sync::Arc<layer_core::SceneSnapshot>, SnapshotSource), String> {
    use layer_core::ArtworkSource::*;
    query.validate()?;
    let normalized=layer_render_wgpu::snapshot::SnapshotGpu::artwork_source_scene(query)?;
    Ok(match query.source {
        Visible => (normalized,SnapshotSource::Visible),
        Source(target) => (normalized,SnapshotSource::Source(target)),
        Objects(target) => (normalized,SnapshotSource::Objects(target)),
        EffectInput(target) => (normalized,SnapshotSource::EffectInput(target)),
        EffectChannels(target) => (std::sync::Arc::new((*query.snapshot).clone().with_scope(normalized.scope.clone())),SnapshotSource::EffectChannels(target)),
        Reference | EffectBaseline(_) => (normalized,SnapshotSource::Visible),
    })
}
#[derive(Serialize, Deserialize)]
enum SnapshotScope {
    All,
    Members(Vec<layer_core::OccurrenceHandle>),
    Raw(layer_core::SourceTarget),
    RawObjects(layer_core::OccurrenceHandle),
    EffectInput(layer_core::OccurrenceHandle),
}
#[derive(Serialize, Deserialize)]
struct FrozenScene {
    owner:u64,
    revision:u64,
    elapsed:f32,
    phases:Vec<(layer_core::EffectHandle,f32)>,
    scope:SnapshotScope,
    offset:[f64;2],
}
impl FrozenScene {
    fn new(scene:&layer_core::SceneSnapshot) -> Self {
        use layer_core::SceneScope::*;
        let scope = match &scene.scope {
            All => SnapshotScope::All,
            Members(handles) => SnapshotScope::Members(handles.to_vec()),
            Raw(target) => SnapshotScope::Raw(*target),
            RawObjects(target) => SnapshotScope::RawObjects(*target),
            EffectInput(target) => SnapshotScope::EffectInput(*target),
        };
        Self {owner:scene.owner,revision:scene.revision,elapsed:scene.context.elapsed,phases:scene.context.phases.as_ref().clone(),scope,offset:scene.offset}
    }
    fn snapshot(self, artwork:layer_core::Artwork) -> Result<std::sync::Arc<layer_core::SceneSnapshot>, String> {
        use layer_core::SceneScope::*;
        let scope = match self.scope {
            SnapshotScope::All => All,
            SnapshotScope::Members(handles) => Members(handles.into()),
            SnapshotScope::Raw(target) => Raw(target),
            SnapshotScope::RawObjects(target) => RawObjects(target),
            SnapshotScope::EffectInput(target) => EffectInput(target),
        };
        let index = std::sync::Arc::new(layer_core::SceneIndex::build(&artwork)?);
        let mut snapshot = layer_core::SceneSnapshot::new(artwork,index,self.owner,self.revision,
            layer_core::EvaluationContext {elapsed:self.elapsed,phases:self.phases.into()});
        snapshot.scope = scope;
        snapshot.offset = self.offset;
        Ok(std::sync::Arc::new(snapshot))
    }
}
#[derive(Serialize, Deserialize)]
enum SnapshotTask {
    LevelsStatistics(SnapshotSource),
    ArtworkStatistics {source:SnapshotSource, preview:bool, selection:bool, waveform:bool},
    ArtworkSample {source:SnapshotSource, position:[f32;2], width:u32},
    Bounds(layer_core::ContentScope),
    TransformPixels {target:layer_core::SourceTarget, map:layer_core::LayerPlacement, source:Option<layer_core::Rect>},
    Image {offset:layer_core::Point, extent:[u32;2], window:[u32;4], trim:Option<layer_core::SourceTarget>},
    Remap(Vec<layer_core::RemapSpec>),
}
fn install_transformed_rasters(edit:&mut layer_core::Edit, artwork:&layer_core::Artwork,
    paint:Option<layer_core::PaintHandle>, coverage:Option<layer_core::CoverageHandle>) -> Result<(), String> {
    match edit {
        layer_core::Edit::Paint(change) if Some(change.handle) == paint => {
            change.value.as_mut().ok_or("Missing transform paint output")?.raster = artwork.paint.get(change.handle).ok_or("Missing transformed paint")?.raster.clone();
        }
        layer_core::Edit::Coverage(change) if Some(change.handle) == coverage => {
            change.value.as_mut().ok_or("Missing transform coverage output")?.raster = artwork.coverage.get(change.handle).ok_or("Missing transformed coverage")?.raster.clone();
        }
        layer_core::Edit::Batch(edits) => for edit in edits { install_transformed_rasters(edit,artwork,paint,coverage)?; },
        _ => {}
    }
    Ok(())
}

fn packed_parts(packed: &JsValue) -> Result<(String, js_sys::Array), JsValue> {
    let metadata = js_sys::Reflect::get(packed, &js("metadata"))?.as_string().ok_or(js("Missing snapshot metadata"))?;
    let buffers = js_sys::Reflect::get(packed, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
    Ok((metadata, buffers))
}

thread_local! {
    static SNAPSHOT_GPU: RefCell<Option<(layer_core::color::DocumentColor, layer_render_wgpu::snapshot::SnapshotGpu)>> = const {RefCell::new(None)};
}
pub(super) async fn snapshot_gpu(color:layer_core::color::DocumentColor) -> Result<layer_render_wgpu::snapshot::SnapshotGpu,JsValue> {
    if let Some(gpu)=SNAPSHOT_GPU.with(|cache|cache.borrow().as_ref().filter(|(cached,_)|*cached==color).map(|(_,gpu)|gpu.clone())) {return Ok(gpu);}
    SNAPSHOT_GPU.with(|cache|*cache.borrow_mut()=None);
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
    let instance = wgpu::Instance::new(descriptor);
    let (adapter, device, queue) = request_device(&instance, None, None).await?;
    device.set_device_lost_callback(|reason, message| {
        gpu_diagnostics::report("worker", "device_lost", format!("{reason:?}: {message}"));
    });
    let mut renderer = WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue, color).map_err(js)?;
    renderer.set_browser_raster_encoder(Rc::new(|bytes, descriptors| Box::pin(async move { encode_tiles(&bytes, descriptors) })));
    let gpu=renderer.snapshot_gpu();
    SNAPSHOT_GPU.with(|cache|*cache.borrow_mut()=Some((color,gpu.clone())));
    Ok(gpu)
}

#[wasm_bindgen]
pub async fn raster_worker_snapshot(metadata: &str, buffers: js_sys::Array) -> Result<JsValue, JsValue> {
    let (metadata, frozen, task): (String, FrozenScene, SnapshotTask) = serde_json::from_str(metadata).map_err(js)?;
    let (artwork, selection) = artwork_transfer::unpack_scene(&metadata, buffers).await?;
    let scene = frozen.snapshot(artwork).map_err(js)?;
    let gpu=snapshot_gpu(scene.view().composition().color).await?;
    match task {
        SnapshotTask::LevelsStatistics(source) => {
            let query = layer_core::ArtworkQuery::from_snapshot(scene,source.artwork_source(),selection);
            let result = gpu.levels_statistics(query,Default::default()).await.map_err(js)?;
            serialize(&result)
        }
        SnapshotTask::ArtworkStatistics {source,preview,selection:restrict_selection,waveform} => {
            let query = layer_core::ArtworkQuery::from_snapshot(scene,source.artwork_source(),selection);
            let histogram = gpu.artwork_statistics(layer_core::ArtworkStatisticsRequest {query,preview,selection:restrict_selection,waveform},Default::default()).await.map_err(js)?;
            let result=serialize(&histogram)?;
            if let Some(waveform)=&histogram.waveform {
                js_sys::Reflect::set(&result,&js("waveform_counts"),&js_sys::Uint32Array::from(waveform.counts.as_slice()))?;
            }
            Ok(result)
        }
        SnapshotTask::ArtworkSample {source,position,width} => {
            let query = layer_core::ArtworkQuery::from_snapshot(scene,source.artwork_source(),selection);
            let result = gpu.artwork_sample(layer_core::ArtworkSampleRequest {query,position,width},Default::default()).await.map_err(js)?;
            serialize(&result)
        }
        SnapshotTask::Bounds(scope) => {
            let result = gpu.content_bounds(layer_core::ContentBoundsRequest {snapshot:scene,scope,selection},Default::default()).await.map_err(js)?;
            serialize(&result)
        }
        SnapshotTask::Image {offset,extent,window,trim} => {
            let scope = scene.scope.clone();
            let capture = layer_core::ImageCapture {scene, scope, offset, extent, window, trim, selection:selection.map(std::sync::Arc::new)};
            let Some((image, origin)) = gpu.image_capture(capture,Default::default()).await.map_err(js)? else { return Ok(JsValue::NULL); };
            let mut document = layer_core::Document::new(layer_core::authored::PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "".into(), paper: "".into() });
            let root=document.artwork.root; document.artwork.compositions.get_mut(root).unwrap().color=image_color(&image);
            let layer_core::SourceTarget::Paint(paint)=document.working.target.unwrap() else {unreachable!()};
            let offset = origin.map(|v| u32::try_from(v).map_err(js)).into_iter().collect::<Result<Vec<_>,_>>()?;
            document.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase {image:image.into(),offset:[offset[0],offset[1]],policy:layer_core::authored::PaintBasePolicy::WorkingPixels});
            let stack=document.composition().result; document.artwork.stacks.get_mut(stack).unwrap().entries.truncate(1);
            artwork_transfer::pack(document.artwork).await
        }
        SnapshotTask::Remap(specs) => {
            let plan = layer_core::RemapPlan {scene: scene.clone(), specs: specs.into()};
            let results = gpu.remap(plan, Default::default()).await.map_err(js)?;
            let mut artwork = scene.artwork.clone();
            for (target, image, raster) in results {
                match target {
                    layer_core::SourceTarget::Paint(h) => {
                        let source = artwork.paint.get_mut(h).ok_or_else(|| js("Missing moved layer"))?;
                        if let (Some(image), Some(base)) = (image, source.base.as_mut()) { base.image = image; }
                        source.raster = raster;
                    }
                    layer_core::SourceTarget::Coverage(h) => artwork.coverage.get_mut(h).ok_or_else(|| js("Missing moved mask"))?.raster = raster,
                    layer_core::SourceTarget::Selection(_) => return Err(js("Missing moved layer")),
                }
            }
            artwork_transfer::pack(artwork).await
        }
        SnapshotTask::TransformPixels {target,map,source} => {
            let mut document = layer_core::Document::from_artwork(scene.artwork.clone()).map_err(js)?;
            let mut plan = document.layer_transform_plan(target,&map,source,Default::default()).map_err(js)?;
            plan.scene = scene;
            let output = gpu.transform_pixels(plan,Default::default()).await.map_err(js)?;
            document.apply(output).map_err(js)?;
            artwork_transfer::pack(document.artwork).await
        }
    }
}

fn image_color(image:&layer_core::color::source::SourceImage)->layer_core::color::DocumentColor {
    let layer_core::color::ColorProfile::Builtin(space)=image.interpretation.profile else {return Default::default();};
    layer_core::color::DocumentColor {space,depth:image.interpretation.depth}
}
