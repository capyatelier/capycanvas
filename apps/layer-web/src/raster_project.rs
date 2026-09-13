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
struct Part {
    bytes: Arc<[u8]>,
    range: Range<usize>,
}

async fn wait_backing(project: &Project) -> Result<(), JsValue> {
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
                let index = *dedup.entry(blob.digest).or_insert_with(|| {
                    let index = metadata.blobs.len();
                    metadata.blobs.push(Blob {
                        descriptor: blob.descriptor,
                        digest: blob.digest,
                        data: parts.len(),
                    });
                    parts.push(Part {
                        bytes: blob.compressed_owned(),
                        range: 0..blob.compressed().len(),
                    });
                    index
                });
                tiles.push((*key, index));
            }
            metadata.rasters.push(Raster {
                target,
                tiles,
                watercolor: data.watercolor,
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

async fn pack(project: Project) -> Result<JsValue, JsValue> {
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

async fn unpack(
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
    if metadata.blobs.len() > budget.tiles || metadata.rasters.len() > budget.layers * 2 {
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
        tiles.push(RasterTile::backed(tile));
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
            if data.tiles.insert(key, tile.clone()).is_some() {
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

pub(super) async fn open(bytes: js_sys::Uint8Array, dimension: u32) -> Result<Project, JsValue> {
    let buffers = js_sys::Array::new();
    buffers.push(&bytes);
    let wire = JsFuture::from(raster_worker::call(
        "read",
        &dimension.to_string(),
        &buffers,
    )?)
    .await?;
    let metadata = js_sys::Reflect::get(&wire, &js("metadata"))?
        .as_string()
        .ok_or_else(|| js("Missing project worker metadata"))?;
    let buffers = js_sys::Reflect::get(&wire, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
    unpack(&metadata, buffers, true).await
}

#[wasm_bindgen]
pub async fn raster_worker_read(dimension: u32, bytes: Vec<u8>) -> Result<JsValue, JsValue> {
    let project = Project::read(bytes.as_slice(), limits(dimension)).map_err(js)?;
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
