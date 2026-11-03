//! The paper library lives in the raster worker's origin store, where library
//! policy decodes, converts and encodes papers. The owner captures a request,
//! exchanges compressed tiles with the worker and adopts the result.
use super::*;
use layer_core::{PortableId, color::source::{SourceImage, SourceInterpretation}, raster::TileBlob};
use layer_ui::paper_library::{PaperCapture, PaperEntry, PaperLibraryError, PaperMaterial, PaperRequest, PaperResult};
use std::{rc::Rc, sync::Arc};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

const MAX_IMAGE_BYTES: f64 = 512. * 1024. * 1024.;
const FULL: &str = "paper:full";
const UNSUPPORTED: &str = "paper:unsupported";

#[derive(Serialize, Deserialize)]
struct Transfer {
    extent: [u32; 2],
    resolution: Option<layer_core::ImageResolution>,
    interpretation: SourceInterpretation,
    tiles: Vec<[u32; 2]>,
}
impl Transfer {
    fn send(source: &SourceImage, buffers: &js_sys::Array) -> Result<Self, String> {
        for tile in source.tiles.values() { buffers.push(&js_sys::Uint8Array::from(tile.compressed()?.as_ref())); }
        Ok(Self { extent: source.extent, resolution: source.resolution, interpretation: source.interpretation.clone(), tiles: source.tiles.keys().copied().collect() })
    }
    fn receive(self, buffers: &js_sys::Array, first: u32) -> Result<SourceImage, String> {
        let descriptor = self.interpretation.descriptor();
        let tiles = self.tiles.into_iter().zip(first..).map(|(coordinate, index)| {
            let bytes = buffers.get(index).dyn_into::<js_sys::Uint8Array>().map_err(|_| "Missing paper tile".to_string())?.to_vec();
            Ok((coordinate, Arc::new(TileBlob::from_verified_resource(PortableId::random(), descriptor, bytes.into())?)))
        }).collect::<Result<_, String>>()?;
        Ok(SourceImage { extent: self.extent, resolution: self.resolution, interpretation: self.interpretation, tiles })
    }
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum WorkerPaper {
    Import { name: String, stored: Vec<PaperEntry> },
    Store { name: String, stored: Vec<PaperEntry>, paper: Transfer },
    Load { id: String, retained: Option<Transfer> },
    Name { name: String },
}

fn library_error(error: PaperLibraryError) -> JsValue {
    match error {
        PaperLibraryError::Full => js(FULL),
        PaperLibraryError::Unsupported => js(UNSUPPORTED),
        PaperLibraryError::Failed(detail) => js(detail),
    }
}

/// Library policy for the worker store: one request in, metadata and buffers out.
#[wasm_bindgen]
pub fn raster_worker_paper(metadata: &str, buffers: js_sys::Array,file:Option<web_sys::Blob>) -> Result<js_sys::Object, JsValue> {
    let (metadata, buffers) = match serde_json::from_str(metadata).map_err(js)? {
        WorkerPaper::Import { name, stored } => {
            let input=blob::Backing::new(file.ok_or_else(||js(UNSUPPORTED))?)?;
            if input.source.byte_len()>MAX_IMAGE_BYTES as u64 {return Err(js(UNSUPPORTED));}
            let cancelled=Default::default();
            let reader=layer_core::package::transport::BackingReader::new(&input.source,&cancelled);
            let mut images = layer_ui::ImageImportBatch::new(Default::default(), layer_core::color::RgbSpace::ProPhoto, layer_color::photo::DecodeLimits::from_memory_budget(artwork_transfer::photo_memory_budget()));
            images.read(reader, &name, &cancelled).map_err(js)?;
            let sources = images.take_sources(false).map_err(js)?;
            let (_, source) = sources.first().ok_or_else(|| js(UNSUPPORTED))?;
            let paper = layer_ui::paper_library::library_paper(source, &|| false).map_err(library_error)?;
            prepared(&paper, &name, &stored)?
        }
        WorkerPaper::Store { name, stored, paper } => prepared(&paper.receive(&buffers, 0).map_err(js)?, &name, &stored)?,
        WorkerPaper::Load { id, retained } => {
            let png = buffers.get(0).dyn_into::<js_sys::Uint8Array>()?.to_vec();
            let paper = layer_ui::paper_library::stored_paper(&id, &png).map_err(js)?;
            let retained = retained.map(|transfer| transfer.receive(&buffers, 1)).transpose().map_err(js)?;
            let kept = retained.as_ref().map(|image| layer_core::Image::with_id(layer_ui::paper_library::paper_image_id(&id).unwrap_or_else(PortableId::random), Arc::new(image.clone())));
            let canonical = layer_ui::paper_library::canonical_source(&id, paper, kept.as_slice()).map_err(js)?;
            let out = js_sys::Array::new();
            let paper = if kept.is_some() { None } else { Some(Transfer::send(&canonical, &out).map_err(js)?) };
            (serde_json::json!({"id": id, "retained": kept.is_some(), "paper": paper}), out)
        }
        WorkerPaper::Name { name } => (serde_json::json!({"name": layer_ui::paper_library::paper_name(&name).ok_or_else(|| js("Name the paper"))?}), js_sys::Array::new()),
    };
    let result = js_sys::Object::new();
    js_sys::Reflect::set(&result, &js("metadata"), &js_sys::JSON::parse(&metadata.to_string())?)?;
    js_sys::Reflect::set(&result, &js("buffers"), &buffers)?;
    Ok(result)
}

fn prepared(paper: &SourceImage, name: &str, stored: &[PaperEntry]) -> Result<(serde_json::Value, js_sys::Array), JsValue> {
    let id = layer_ui::paper_library::paper_identity(paper).map_err(js)?;
    let buffers = js_sys::Array::new();
    let exists = stored.iter().any(|entry| entry.id == id);
    let named = if exists { None } else {
        let stored = layer_ui::paper_library::prepare_paper(&id, paper, name, stored).map_err(library_error)?;
        buffers.push(&js_sys::Uint8Array::from(stored.png.as_slice()));
        buffers.push(&js_sys::Uint8Array::from(stored.preview.as_slice()));
        Some(stored.name)
    };
    let paper = Transfer::send(paper, &buffers).map_err(js)?;
    Ok((serde_json::json!({"id": id, "exists": exists, "name": named, "paper": paper}), buffers))
}

#[wasm_bindgen]
pub struct WebPaperTask {
    capture: Rc<PaperCapture>,
    localization: Arc<layer_ui::Localizer>,
}
#[wasm_bindgen]
impl WebPaperTask {
    pub fn imports(&self) -> bool { matches!(self.capture.action, Some(PaperRequest::Import { .. })) }
}
#[wasm_bindgen]
pub struct WebPaperResult {
    result: Option<Result<PaperResult, String>>,
}

fn entries(reply: &JsValue) -> Result<Vec<PaperEntry>, String> {
    let listed: Vec<PaperEntry> = serde_wasm_bindgen::from_value(js_sys::Reflect::get(reply, &js("entries")).map_err(|_| "Missing paper entries")?)
        .map_err(|e| e.to_string())?;
    let previews = js_sys::Reflect::get(reply, &js("previews")).map_err(|_| "Missing paper previews")?.dyn_into::<js_sys::Array>().map_err(|_| "Missing paper previews")?;
    let size = layer_core::paper::PREVIEW_SIZE * layer_core::paper::PREVIEW_SIZE;
    Ok(listed.into_iter().zip(previews.iter()).filter_map(|(entry, preview)| {
        let preview = preview.dyn_into::<js_sys::Uint8Array>().ok()?.to_vec();
        (preview.len() == size).then(|| PaperEntry { preview: preview.into(), ..entry })
    }).collect())
}

fn paper(reply: &JsValue) -> Result<SourceImage, String> {
    let metadata = js_sys::Reflect::get(reply, &js("metadata")).map_err(|_| "Missing paper")?;
    let transfer: Transfer = serde_wasm_bindgen::from_value(js_sys::Reflect::get(&metadata, &js("paper")).map_err(|_| "Missing paper")?).map_err(|e| e.to_string())?;
    let tiles = js_sys::Reflect::get(reply, &js("tiles")).map_err(|_| "Missing paper")?.dyn_into::<js_sys::Array>().map_err(|_| "Missing paper")?;
    transfer.receive(&tiles, 0)
}

async fn library(request: serde_json::Value, buffers: js_sys::Array,file:Option<web_sys::Blob>, localization: &layer_ui::Localizer) -> Result<JsValue, String> {
    let packet=js_sys::Object::new();
    js_sys::Reflect::set(&packet,&js("operation"),&js("paper-library")).map_err(|error|format!("{error:?}"))?;
    js_sys::Reflect::set(&packet,&js("metadata"),&js(request.to_string())).map_err(|error|format!("{error:?}"))?;
    js_sys::Reflect::set(&packet,&js("buffers"),&buffers).map_err(|error|format!("{error:?}"))?;
    if let Some(file)=file {js_sys::Reflect::set(&packet,&js("file"),&file).map_err(|error|format!("{error:?}"))?;}
    JsFuture::from(raster_worker::request(&packet).map_err(|e|format!("{e:?}"))?).await.map_err(|error| {
        let text = color_preferences::diagnostic_text(&error);
        match text.as_str() {
            FULL => PaperLibraryError::Full.message(localization),
            UNSUPPORTED => PaperLibraryError::Unsupported.message(localization),
            _ => text,
        }
    })
}

async fn load(id: &str, retained: &[layer_core::Image], localization: &layer_ui::Localizer) -> Result<SourceImage, String> {
    let buffers = js_sys::Array::new();
    let parent = layer_ui::paper_library::paper_image_id(id).ok_or("Unknown paper")?;
    let retained = retained.iter().find(|image| image.id() == parent);
    let transfer = retained.map(|image| Transfer::send(image.storage(), &buffers)).transpose()?;
    let reply = library(serde_json::json!({"operation": "load", "id": id, "retained": transfer}), buffers,None, localization).await?;
    let kept = js_sys::Reflect::get(&reply, &js("metadata")).ok().and_then(|m| js_sys::Reflect::get(&m, &js("retained")).ok()).and_then(|v| v.as_bool()).unwrap_or(false);
    match retained.filter(|_| kept) { Some(image) => Ok(image.storage().as_ref().clone()), None => paper(&reply) }
}

/// The texture New chose; a stored paper the library cannot read falls back with a warning.
pub(super) async fn texture(material: &PaperMaterial, retained: &[layer_core::Image], localization: &layer_ui::Localizer) -> (layer_core::TextureSource, Option<String>) {
    let PaperMaterial::Stored { id, fallback } = material else { return (material.fallback(), None) };
    match (layer_ui::paper_library::paper_image_id(id), load(id, retained, localization).await) {
        (Some(parent), Ok(image)) => (layer_core::TextureSource::Image(layer_core::Image::with_id(parent, Arc::new(image))), None),
        _ => (fallback.clone(), Some(layer_ui::file_open_failure(localization, &format!("{id}.png")))),
    }
}

async fn run(capture: &PaperCapture, file: JsValue, localization: &layer_ui::Localizer) -> Result<PaperResult, String> {
    let none = js_sys::Array::new;
    Ok(match &capture.action {
        None => PaperResult::Entries(entries(&library(serde_json::json!({"operation": "list"}), none(),None, localization).await?)?),
        Some(PaperRequest::Load { id }) => PaperResult::Loaded(id.clone(), load(id, &capture.retained, localization).await?),
        Some(PaperRequest::Import { layer }) => {
            let failed = || PaperLibraryError::Unsupported.message(localization);
            if file.is_null() || file.is_undefined() { return Err(failed()); }
            let name = js_sys::Reflect::get(&file, &js("name")).ok().and_then(|n| n.as_string()).unwrap_or_default();
            let name = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem).to_string();
            let file=file.dyn_into::<web_sys::Blob>().map_err(|_|failed())?;
            let size=file.size();if !size.is_finite() || size<0. || size>MAX_IMAGE_BYTES {return Err(failed());}
            let reply=library(serde_json::json!({"operation":"import","name":name}),none(),Some(file),localization).await?;
            let metadata = js_sys::Reflect::get(&reply, &js("metadata")).map_err(|_| failed())?;
            let id = js_sys::Reflect::get(&metadata, &js("id")).ok().and_then(|id| id.as_string()).ok_or_else(failed)?;
            let image = layer_ui::paper_library::canonical_source(&id, paper(&reply)?, &capture.retained)?;
            PaperResult::Imported(*layer, id, image, entries(&reply)?)
        }
        Some(PaperRequest::Add { name, .. }) => {
            let image = capture.image.as_ref().ok_or_else(|| PaperLibraryError::Unsupported.message(localization))?;
            let buffers = none();
            let transfer = Transfer::send(image, &buffers)?;
            PaperResult::Entries(entries(&library(serde_json::json!({"operation": "store", "name": name, "paper": transfer}), buffers,None, localization).await?)?)
        }
        Some(PaperRequest::Rename { id, name }) => PaperResult::Entries(entries(&library(serde_json::json!({"operation": "rename", "id": id, "name": name}), none(),None, localization).await?)?),
        Some(PaperRequest::Delete { id }) => PaperResult::Entries(entries(&library(serde_json::json!({"operation": "delete", "id": id}), none(),None, localization).await?)?),
    })
}

#[wasm_bindgen]
impl WebApp {
    pub fn paper_task(&self, request: Option<u32>) -> Result<WebPaperTask, JsValue> {
        Ok(WebPaperTask { capture: Rc::new(self.session.capture_paper(request).map_err(js)?), localization: self.session.localization().clone() })
    }
    pub fn paper_run(&self, task: &WebPaperTask, file: JsValue) -> js_sys::Promise {
        let (capture, localization) = (task.capture.clone(), task.localization.clone());
        future_to_promise(async move { Ok(WebPaperResult { result: Some(run(&capture, file, &localization).await) }.into()) })
    }
    pub fn paper_adopt(&mut self, task: &WebPaperTask, result: &mut WebPaperResult, cancelled: bool) -> Result<JsValue, JsValue> {
        let result = result.result.take().ok_or_else(|| js("This paper request already finished"))?;
        let change = self.session.adopt_paper(&task.capture, result, cancelled).map_err(js)?;
        change.map_or(Ok(JsValue::NULL), |change| serialize(&change))
    }
    /// Imports a paper into the library alone, for New drawing.
    pub fn paper_library_import(&self) -> WebPaperTask {
        WebPaperTask { capture: Rc::new(self.session.capture_library_import()), localization: self.session.localization().clone() }
    }
}
