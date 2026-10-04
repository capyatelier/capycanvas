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
    renderer.set_snapshot_worker(Rc::new(|request, control| Box::pin(async move {
        if control.is_cancelled() { return Err("Operation cancelled".into()); }
        let mut transform = None;
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
                let task = SnapshotTask::TransformPixels {target:plan.target, interpolation:plan.geometry.placement.interpolation};
                transform = Some((plan.output, plan.paint, plan.coverage));
                let scene=std::sync::Arc::new((*plan.scene).clone().with_scope(layer_core::SceneScope::Raw(plan.target)));
                (scene, None, task)
            }
        };
        let frozen = FrozenScene::new(&scene);
        let additional:Vec<_>=transform.as_ref().into_iter().flat_map(|(_,paint,coverage)|paint.map(layer_core::SourceTarget::Paint).into_iter().chain(coverage.map(layer_core::SourceTarget::Coverage))).collect();
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
                let end = offset.checked_add(size).ok_or("Raster worker size overflow")?;
                let encoded = bytes
                    .get(offset..end)
                    .ok_or("Incomplete raster worker tile")?;
                blobs.push(TileBlob::from_verified_resource(
                    layer_core::PortableId::random(), descriptor, encoded.into(), None,
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

#[wasm_bindgen]
pub fn raster_worker_encode(metadata: &str, bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    let descriptors: Vec<PixelDescriptor> = serde_json::from_str(metadata).map_err(js)?;
    let mut result = Vec::new();
    for tile in encode_tiles(bytes, descriptors).map_err(js)? {
        result.extend_from_slice(&(tile.compressed_len() as u32).to_le_bytes());
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
    EffectInput(layer_core::OccurrenceHandle),
}
#[derive(Serialize, Deserialize)]
struct FrozenScene {
    owner:u64,
    revision:u64,
    elapsed:f32,
    phases:Vec<(layer_core::EffectHandle,f32)>,
    scope:SnapshotScope,
    offset:layer_core::Point,
}
impl FrozenScene {
    fn new(scene:&layer_core::SceneSnapshot) -> Self {
        use layer_core::SceneScope::*;
        let scope = match &scene.scope {
            All => SnapshotScope::All,
            Members(handles) => SnapshotScope::Members(handles.to_vec()),
            Raw(target) => SnapshotScope::Raw(*target),
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
    TransformPixels {target:layer_core::SourceTarget, interpolation:layer_core::Interpolation},
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
async fn snapshot_gpu(color:layer_core::color::DocumentColor) -> Result<layer_render_wgpu::snapshot::SnapshotGpu,JsValue> {
    if let Some(gpu)=SNAPSHOT_GPU.with(|cache|cache.borrow().as_ref().filter(|(cached,_)|*cached==color).map(|(_,gpu)|gpu.clone())) {return Ok(gpu);}
    SNAPSHOT_GPU.with(|cache|*cache.borrow_mut()=None);
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
    let instance = wgpu::Instance::new(descriptor);
    let (adapter, device, queue) = request_device(&instance, None).await?;
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
        SnapshotTask::TransformPixels {target,interpolation} => {
            let mut document = layer_core::Document::from_artwork(scene.artwork.clone()).map_err(js)?;
            let mut plan = document.transform_pixels_plan(target,interpolation,Default::default()).map_err(js)?;
            plan.scene = scene;
            let output = gpu.transform_pixels(plan,Default::default()).await.map_err(js)?;
            document.apply(output).map_err(js)?;
            artwork_transfer::pack(document.artwork).await
        }
    }
}
