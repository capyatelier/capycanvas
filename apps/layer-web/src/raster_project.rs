//! Transfer immutable backing across independent Wasm memories in bounded blocks.
//! This is a private worker message, not a second project-file format. Only the
//! shared Project reader/writer handles persisted bytes and verifies integrity.
use super::*;
use layer_core::{
    Document, LayerId, Project, ProjectAsset, ProjectAssetFormat, ProjectLimits,
    color::PixelDescriptor,
    raster::{RasterData, RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};
use wasm_bindgen_futures::JsFuture;

const BLOCK: usize = 4 * 1024 * 1024;
fn limits(dimension: u32) -> ProjectLimits {
    ProjectLimits {
        dimension,
        asset_bytes: 256 * 1024 * 1024,
        raster_bytes: 512 * 1024 * 1024,
        tiles: 8192,
        ..Default::default()
    }
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    document: Document,
    rasters: Vec<Raster>,
    blobs: Vec<Blob>,
    sources: Vec<Source>,
    originals: Vec<Original>,
}
#[derive(Serialize, Deserialize)]
struct Raster {
    target: LayerId,
    tiles: Vec<(TileKey, usize)>,
    watercolor: Option<RasterWatercolor>,
}
#[derive(Serialize, Deserialize)]
struct Blob {
    descriptor: PixelDescriptor,
    digest: [u8; 32],
    data: usize,
}
#[derive(Serialize, Deserialize)]
struct Source {
    id: AssetId,
    extent: [u32; 2],
    format: ProjectAssetFormat,
    data: Vec<usize>,
}
#[derive(Serialize, Deserialize)]
struct Original {
    layers: Vec<LayerId>,
    kind: layer_core::color::source::SourceKind,
    extent: [u32; 2],
    resolution: Option<layer_core::ImageResolution>,
    interpretation: layer_core::color::source::SourceInterpretation,
    tiles: Vec<([u32; 2], usize)>,
}
struct Part {
    bytes: Arc<[u8]>,
    range: Range<usize>,
}

pub(super) async fn wait_backing(project: &Project) -> Result<(), JsValue> {
    let start = js_sys::Date::now();
    loop {
        let mut ready = true;
        for layer in &project.document.layers {
            for raster in std::iter::once(&layer.raster).chain(layer.mask.iter().map(|m| &m.raster))
            {
                match raster.try_data() {
                    None => ready = false,
                    Some(Err(e)) => return Err(js(e)),
                    Some(Ok(data)) => {
                        for tile in data.tiles.values() {
                            match tile.try_backing() {
                                None => ready = false,
                                Some(Err(e)) => return Err(js(e)),
                                Some(Ok(_)) => {}
                            }
                        }
                    }
                }
            }
        }
        if ready {
            return Ok(());
        }
        if js_sys::Date::now() - start > 30_000. {
            return Err(js("Raster backing timed out"));
        }
        documents::yield_browser().await?;
    }
}

fn describe(project: Project) -> Result<(Metadata, Vec<Part>), String> {
    let project = project.pruned()?;
    project.validate(limits(ProjectLimits::default().dimension))?;
    let mut metadata = Metadata {
        document: project.document.clone(),
        rasters: Vec::new(),
        blobs: Vec::new(),
        sources: Vec::new(),
        originals: Vec::new(),
    };
    let mut parts = Vec::new();
    let mut dedup = BTreeMap::new();
    for layer in &project.document.layers {
        for (target, root) in std::iter::once((layer.id, &layer.raster))
            .chain(layer.mask.iter().map(|m| (m.id, &m.raster)))
        {
            let data = root.wait_data()?;
            let mut tiles = Vec::new();
            for (key, tile) in &data.tiles {
                let blob = tile.wait_backing()?;
                let index = push_blob(&blob, &mut metadata.blobs, &mut parts, &mut dedup);
                tiles.push((*key, index));
            }
            metadata.rasters.push(Raster {
                target,
                tiles,
                watercolor: data.watercolor,
            });
        }
    }
    let mut originals = BTreeMap::new();
    for layer in &project.document.layers {
        let Some(source) = &layer.source else { continue };
        let identity = Arc::as_ptr(source) as usize;
        if let Some(&index) = originals.get(&identity) {
            let original: &mut Original = &mut metadata.originals[index];
            original.layers.push(layer.id);
        } else {
            let tiles = source.tiles.iter().map(|(coordinate, blob)| {
                (*coordinate, push_blob(blob, &mut metadata.blobs, &mut parts, &mut dedup))
            }).collect();
            originals.insert(identity, metadata.originals.len());
            metadata.originals.push(Original {
                layers: vec![layer.id], kind: source.kind, extent: source.extent,
                resolution: source.resolution, interpretation: source.interpretation.clone(), tiles,
            });
        }
    }
    for (id, asset) in &project.assets {
        let mut data = Vec::new();
        for start in (0..asset.bytes.len()).step_by(BLOCK) {
            data.push(parts.len());
            parts.push(Part {
                bytes: asset.bytes.clone(),
                range: start..(start + BLOCK).min(asset.bytes.len()),
            });
        }
        metadata.sources.push(Source {
            id: id.clone(),
            extent: asset.extent,
            format: asset.format,
            data,
        });
    }
    Ok((metadata, parts))
}

fn push_blob(blob: &Arc<TileBlob>, blobs: &mut Vec<Blob>, parts: &mut Vec<Part>,
    dedup: &mut BTreeMap<[u8; 32], usize>) -> usize {
    *dedup.entry(blob.digest).or_insert_with(|| {
        let index = blobs.len();
        blobs.push(Blob { descriptor: blob.descriptor, digest: blob.digest, data: parts.len() });
        parts.push(Part { bytes: blob.compressed_owned(), range: 0..blob.compressed().len() });
        index
    })
}

pub(super) async fn pack(project: Project) -> Result<JsValue, JsValue> {
    wait_backing(&project).await?;
    let (metadata, parts) = describe(project).map_err(js)?;
    let buffers = js_sys::Array::new();
    let mut copied = 0;
    for part in parts {
        copied += part.range.len();
        buffers.push(&js_sys::Uint8Array::from(&part.bytes[part.range]));
        if copied >= BLOCK {
            copied = 0;
            documents::yield_browser().await?;
        }
    }
    let result = js_sys::Object::new();
    js_sys::Reflect::set(
        &result,
        &js("metadata"),
        &js(serde_json::to_string(&metadata).map_err(js)?),
    )?;
    js_sys::Reflect::set(&result, &js("buffers"), &buffers)?;
    Ok(result.into())
}

fn part(buffers: &js_sys::Array, index: usize) -> Result<Vec<u8>, JsValue> {
    let value = buffers.get(index as u32);
    let bytes = value
        .dyn_into::<js_sys::Uint8Array>()
        .map_err(|_| js("Missing project worker block"))?;
    if bytes.length() as usize > BLOCK {
        return Err(js("Oversized project worker block"));
    }
    buffers.set(index as u32, JsValue::UNDEFINED);
    Ok(bytes.to_vec())
}

pub(super) async fn unpack(
    metadata: &str,
    buffers: js_sys::Array,
    verified: bool,
) -> Result<Project, JsValue> {
    if metadata.len() > 64 * 1024 * 1024 {
        return Err(js("Oversized project metadata"));
    }
    let metadata: Metadata = serde_json::from_str(metadata).map_err(js)?;
    let mut project = Project {
        document: metadata.document,
        assets: BTreeMap::new(),
    };
    let budget = limits(ProjectLimits::default().dimension);
    if metadata.blobs.len() > budget.tiles || metadata.rasters.len() > budget.layers * 2
        || metadata.originals.len() > budget.layers {
        return Err(js("Oversized project worker index"));
    }
    let mut tiles = Vec::new();
    let mut copied = 0;
    for blob in metadata.blobs {
        let bytes = part(&buffers, blob.data)?;
        copied += bytes.len();
        let tile = if verified {
            TileBlob::from_verified_worker(blob.descriptor, blob.digest, bytes.into())
        } else {
            TileBlob::from_compressed(blob.descriptor, blob.digest, bytes.into())
        }
        .map_err(js)?;
        tiles.push(Arc::new(tile));
        if copied >= BLOCK {
            copied = 0;
            documents::yield_browser().await?;
        }
    }
    let mut seen = BTreeSet::new();
    for raster in metadata.rasters {
        if !seen.insert(raster.target) {
            return Err(js("Duplicate project raster"));
        }
        let mut data = RasterData {
            tiles: BTreeMap::new(),
            watercolor: raster.watercolor,
        };
        for (key, index) in raster.tiles {
            let tile = tiles.get(index).ok_or_else(|| js("Missing project tile"))?;
            if data.tiles.insert(key, RasterTile::backed_shared(tile.clone())).is_some() {
                return Err(js("Duplicate project tile"));
            }
        }
        let root = RasterRevision::backed(data);
        let mut found = false;
        for layer in &mut project.document.layers {
            if layer.id == raster.target {
                layer.raster = root.clone();
                found = true;
                break;
            }
            if let Some(mask) = &mut layer.mask {
                if mask.id == raster.target {
                    mask.raster = root.clone();
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Err(js("Missing project raster target"));
        }
    }
    if seen.len()
        != project
            .document
            .layers
            .iter()
            .map(|l| 1 + usize::from(l.mask.is_some()))
            .sum::<usize>()
    {
        return Err(js("Incomplete project raster transfer"));
    }
    let mut source_layers = BTreeSet::new();
    for original in metadata.originals {
        let mut source = layer_core::color::source::SourceImage {
            kind: original.kind, extent: original.extent, resolution: original.resolution,
            interpretation: original.interpretation, tiles: BTreeMap::new(),
        };
        for (coordinate, index) in original.tiles {
            let tile = tiles.get(index).ok_or_else(|| js("Missing original source tile"))?;
            if source.tiles.insert(coordinate, tile.clone()).is_some() {
                return Err(js("Duplicate original source tile"));
            }
        }
        source.validate().map_err(js)?;
        let source = Arc::new(source);
        for id in original.layers {
            if !source_layers.insert(id) { return Err(js("Duplicate original source layer")); }
            let layer = project.document.layers.iter_mut().find(|l| l.id == id)
                .ok_or_else(|| js("Missing original source layer"))?;
            layer.source = Some(source.clone());
        }
    }
    let mut total = 0u64;
    for source in metadata.sources {
        let size =
            source.extent[0] as u64 * source.extent[1] as u64 * source.format.channels() as u64;
        total = total
            .checked_add(size)
            .filter(|v| *v <= budget.asset_bytes)
            .ok_or_else(|| js("Project source budget exceeded"))?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size as usize).map_err(js)?;
        for index in source.data {
            let block = part(&buffers, index)?;
            if bytes.len() + block.len() > size as usize {
                return Err(js("Oversized project source"));
            }
            bytes.extend_from_slice(&block);
            documents::yield_browser().await?;
        }
        if bytes.len() != size as usize {
            return Err(js("Incomplete project source"));
        }
        if project
            .assets
            .insert(
                source.id,
                ProjectAsset {
                    extent: source.extent,
                    format: source.format,
                    bytes: bytes.into(),
                },
            )
            .is_some()
        {
            return Err(js("Duplicate project source"));
        }
    }
    project.validate(budget).map_err(js)?;
    Ok(project)
}

pub(super) async fn save(project: Project) -> Result<JsValue, JsValue> {
    let wire = pack(project).await?;
    let metadata = js_sys::Reflect::get(&wire, &js("metadata"))?
        .as_string()
        .unwrap();
    let buffers = js_sys::Reflect::get(&wire, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
    JsFuture::from(raster_worker::call("write", &metadata, &buffers)?).await
}

#[derive(Serialize, Deserialize)]
pub(super) struct OpenOptions {
    pub dimension: u32,
    pub photo_policy: layer_ui::PhotoOpenPolicy,
    pub name: String,
    pub recovered: bool,
}
pub(super) async fn open(bytes: js_sys::Uint8Array, options: OpenOptions) -> Result<Project, JsValue> {
    let buffers = js_sys::Array::new();
    buffers.push(&bytes);
    let wire = JsFuture::from(raster_worker::call(
        "read", &serde_json::to_string(&options).map_err(js)?, &buffers,
    )?).await?;
    let metadata = js_sys::Reflect::get(&wire, &js("metadata"))?
        .as_string().ok_or_else(|| js("Missing project worker metadata"))?;
    let buffers = js_sys::Reflect::get(&wire, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
    unpack(&metadata, buffers, true).await
}

#[wasm_bindgen]
pub async fn raster_worker_read(options: &str, bytes: Vec<u8>) -> Result<JsValue, JsValue> {
    let options: OpenOptions = serde_json::from_str(options).map_err(js)?;
    let project = if bytes.starts_with(b"CAPY") {
        Project::read(bytes.as_slice(), limits(options.dimension)).map_err(js)?
    } else {
        if options.recovered { return Err(js("Recovery file is not a native drawing")); }
        let source = layer_color::photo::read_photo(std::io::Cursor::new(&bytes), layer_color::photo::DecodeLimits::from_memory_budget(photo_memory_budget())).map_err(js)?;
        let depth = options.photo_policy.editing_depth(source.interpretation.depth);
        let name = options.name.rsplit_once('.').map_or(options.name.as_str(), |(stem, _)| stem);
        layer_color::photo_project(source, name, depth).map_err(js)?
    };
    drop(bytes);
    pack(project).await
}

#[wasm_bindgen]
pub async fn raster_worker_write(
    metadata: String,
    buffers: js_sys::Array,
) -> Result<Vec<u8>, JsValue> {
    let project = unpack(&metadata, buffers, false).await?;
    let mut bytes = Vec::new();
    project.write(&mut bytes).map_err(js)?;
    Ok(bytes)
}

pub(super) async fn save_recovery(project: Project, key: String) -> Result<JsValue, JsValue> {
    let wire = pack(project).await?;
    let metadata = js_sys::Reflect::get(&wire, &js("metadata"))?
        .as_string()
        .ok_or_else(|| js("Missing project metadata"))?;
    let buffers = js_sys::Reflect::get(&wire, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
    let metadata =
        serde_json::to_string(&serde_json::json!({"key":key,"project":metadata})).map_err(js)?;
    JsFuture::from(raster_worker::call("recover-write", &metadata, &buffers)?).await
}


/// Browser admission allowance, not a measurement of free RAM. The capacity
/// hint only selects a bounded file-worker policy; absent hints retain the
/// codec's conservative fallback. A worker runs one file job at a time.
pub(super) fn photo_memory_budget() -> layer_color::photo::PhotoMemoryBudget {
    use layer_color::photo::PhotoMemoryBudget;
    let capacity = js_sys::Reflect::get(&js_sys::global(), &js("navigator"))
        .ok()
        .and_then(|navigator| js_sys::Reflect::get(&navigator, &js("deviceMemory")).ok())
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && *value > 0.);
    let Some(gib) = capacity else { return PhotoMemoryBudget::current(); };
    let capacity = (gib.min(8.) * (1024_u64.pow(3) as f64)) as u64;
    PhotoMemoryBudget {
        source_bytes: (capacity / 32) as usize,
        decode_bytes: (capacity / 8) as usize,
        encode_bytes: (capacity / 16 * 3) as usize,
    }
}
