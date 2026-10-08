//! One reserved request and one source allowance for the complete browser batch.
use super::*;
use layer_ui::{DocumentRequest, HostRequestKind, ImagePlacementContext};
use std::sync::{Arc, Mutex};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[wasm_bindgen]
pub struct WebImageImport {
    pub(super) id: u32,
    pub(super) context: ImagePlacementContext,
    pub(super) lost: Arc<Mutex<Option<String>>>,
}

#[wasm_bindgen]
pub struct WebPreparedImages {
    pub(super) request: WebImageImport,
    pub(super) sources: Vec<(String, layer_core::color::source::SourceImage)>,
    pub(super) clip: Option<layer_ui::PixelClip>,
    pub(super) control: layer_render_wgpu::snapshot::CaptureControl,
}

fn active(session: &UiSession<AttachedRenderer>, id: u32) -> Result<(), JsValue> {
    if session.state().requests.iter().any(|r| {
        r.id == id
            && matches!(
                r.kind,
                HostRequestKind::Document {
                    request: DocumentRequest::Place | DocumentRequest::Paste { .. }
                }
            )
    }) {
        Ok(())
    } else {
        Err(js("The image import request is no longer active"))
    }
}

async fn read_source(file: JsValue, photo_policy: layer_ui::PhotoOpenPolicy, source_bytes: usize,
    localization: &layer_ui::Localizer, control: &layer_render_wgpu::snapshot::CaptureControl,
) -> Result<(String, layer_core::color::source::SourceImage), JsValue> {
    let name = js_sys::Reflect::get(&file, &js("name"))?
        .as_string()
        .unwrap_or_else(|| "Image".into());
    let size = js_sys::Reflect::get(&file, &js("size"))?
        .as_f64()
        .ok_or_else(|| js("Invalid image file"))?;
    if size > 512. * 1024. * 1024. {
        return Err(js("Image file exceeds 512 MiB"));
    }
    let read = js_sys::Reflect::get(&file, &js("arrayBuffer"))?
        .dyn_into::<js_sys::Function>()?;
    let buffer = JsFuture::from(js_sys::Promise::resolve(&read.call0(&file)?)).await?;
    output::cancelled(control)?;
    let bytes = js_sys::Uint8Array::new(&buffer);
    let imported = artwork_transfer::open(
        bytes,
        artwork_transfer::OpenOptions {
            dimension: layer_core::ProjectLimits::default().dimension,
            photo_policy,
            names: layer_ui::photo_document_names(&name, localization),
            intent: layer_ui::ImportIntent::Place,
            source_bytes: Some(source_bytes),
        },
    )
    .await?;
    let project=match imported {
        artwork_transfer::Opened::Editable {document,..}=>document.project,
        artwork_transfer::Opened::Package{..}=>{
            layer_ui::ImportSource::identify(b"PK\x03\x04",layer_ui::ImportIntent::Place).map_err(js)?;
            unreachable!()
        }
    };
    output::cancelled(control)?;
    let scene=project.scene();
    let (occurrence,source)=scene.order().iter().find_map(|&handle| {
        let source=scene.paint_source(handle)?.base.as_ref()?.image.storage();
        Some((scene.occurrence(handle)?,source))
    }).ok_or_else(|| js("The selected file is not a photo"))?;
    Ok((occurrence.name.to_string(), (**source).clone()))
}

#[wasm_bindgen]
impl WebApp {
    pub fn photo_formats(&self) -> Result<JsValue, JsValue> {
        serialize(&layer_color::photo::formats().collect::<Vec<_>>())
    }
    pub fn image_layer_drop(&self, target: u64, fraction: f32) -> Result<JsValue, JsValue> {
        serialize(&self.session.image_layer_drop_hint(target, fraction))
    }
    pub fn capture_image_import(
        &self,
        id: u32,
        screen: JsValue,
        destination: JsValue,
    ) -> Result<WebImageImport, JsValue> {
        active(&self.session, id)?;
        let screen = serde_wasm_bindgen::from_value(screen).map_err(js)?;
        let destination = serde_wasm_bindgen::from_value(destination).map_err(js)?;
        let context = self
            .session
            .image_placement_context(screen, destination)
            .map_err(js)?;
        let lost = self.gpu_owner().ok_or_else(|| js("Wait for the canvas"))?;
        Ok(WebImageImport { id, context, lost })
    }
    pub fn prepare_images(
        &self,
        request: &WebImageImport,
        files: JsValue,
        interpret: js_sys::Function,
        control: &output::WebCaptureControl,
    ) -> Result<js_sys::Promise, JsValue> {
        active(&self.session, request.id)?;
        self.session
            .validate_image_placement(&request.context)
            .map_err(js)?;
        let request = WebImageImport {
            id: request.id,
            context: request.context,
            lost: request.lost.clone(),
        };
        let localization = self.session.localization().clone();
        let photo_policy = self.session.state().settings.photo_open;
        let color = self.session.engine().document().composition().color;
        let mode = match self.session.document_request(request.id).map_err(js)? { DocumentRequest::Paste { mode } => Some(*mode), _ => None };
        let clip = files.as_string().map(|nonce| self.documents.clip.capture(&nonce, &localization).map_err(js)).transpose()?;
        let files: js_sys::Array = if clip.is_some() { js_sys::Array::new() } else { files.dyn_into()? };
        let control = control.inner.clone();
        Ok(future_to_promise(async move {
            if let Some(mut clip) = clip {
                if !matches!(mode, Some(layer_ui::PasteMode::Into | layer_ui::PasteMode::NewImage)) && let Some(layers) = &clip.layers {
                    let mut document = layer_core::Document::from_artwork(layers.scene.artwork.clone()).map_err(js)?;
                    for change in layer_ui::clipboard_color_changes(document.composition().color, color) {
                        document = artwork_transfer::convert_color(&document, change, &control).await?.0;
                    }
                    let mut scene = document.snapshot();
                    Arc::make_mut(&mut scene).context = layers.scene.context.clone();
                    let mut layers = (**layers).clone(); layers.scene = scene;
                    clip.layers = Some(Arc::new(layers));
                }
                output::cancelled(&control)?;
                return Ok(WebPreparedImages { request, sources: Vec::new(), clip: Some(clip), control }.into());
            }
            if files.length() == 0 {
                return Err(js("Choose at least one image"));
            }
            let mut images = layer_ui::ImageImportBatch::new(photo_policy, color.space,
                layer_color::photo::DecodeLimits::from_memory_budget(artwork_transfer::photo_memory_budget()));
            for item in files.iter() {
                let candidates = if js_sys::Array::is_array(&item) { js_sys::Array::from(&item) } else { js_sys::Array::of1(&item) };
                let mut decoded = Err(js("No readable clipboard image"));
                for candidate in candidates.iter() {
                    output::cancelled(&control)?;
                    if request.lost.lock().unwrap().is_some() { return Err(js("The canvas was lost while importing; try again")); }
                    decoded = async {
                        let file = if let Some(load) = candidate.dyn_ref::<js_sys::Function>() {
                            JsFuture::from(js_sys::Promise::resolve(&load.call0(&JsValue::NULL)?)).await?
                        } else { candidate };
                        output::cancelled(&control)?;
                        read_source(file, photo_policy, images.limits().source_bytes, &localization, &control).await
                    }.await;
                    if decoded.is_ok() { break; }
                }
                output::cancelled(&control)?;
                let (name, source) = decoded?;
                images.append(name, source, control.is_cancelled()).map_err(js)?;
                if let Some(source) = images.pending_source() {
                    let choice = interpret.call1(&JsValue::NULL, &serialize(&source.interpretation)?)?;
                    let choice = JsFuture::from(js_sys::Promise::resolve(&choice)).await?;
                    if choice.is_null() || choice.is_undefined() { control.cancel(); }
                    output::cancelled(&control)?;
                    images.interpret(serde_wasm_bindgen::from_value(choice).map_err(js)?, control.is_cancelled()).map_err(js)?;
                }
            }
            output::cancelled(&control)?;
            Ok(WebPreparedImages {
                request, clip: None,
                sources: images.take_sources(control.is_cancelled()).map_err(js)?,
                control,
            }
            .into())
        }))
    }
    pub fn adopt_images(&mut self, prepared: WebPreparedImages) -> Result<JsValue, JsValue> {
        output::cancelled(&prepared.control)?;
        let request = prepared.request;
        active(&self.session, request.id)?;
        self.session
            .validate_image_placement(&request.context)
            .map_err(js)?;
        let lost = self.gpu_owner().ok_or_else(|| js("Canvas unavailable"))?;
        if !Arc::ptr_eq(&lost, &request.lost) || lost.lock().unwrap().is_some() {
            return Err(js("The canvas changed while importing; try again"));
        }
        let mode = self.session.state().requests.iter().find_map(|r| match &r.kind {
            HostRequestKind::Document { request: DocumentRequest::Paste { mode } } if r.id == request.id => Some(*mode),
            _ => None,
        });
        if let (Some(clip), Some(mode)) = (prepared.clip.as_ref(), mode) { self.session.paste_clip(clip, mode) } else { match mode {
            Some(mode) => self.session.paste_layer_sources(prepared.sources, mode, &request.context),
            None => self.session.place_layer_sources(prepared.sources, request.context.center, request.context.destination),
        }}.map_err(js)?;
        let mut change = self
            .session
            .complete_document_request(request.id, Ok(true))
            .map_err(js)?;
        change.canvas_wake = true;
        // Placement publishes tool selection and controls as well as document rows.
        change.regions |= 255;
        serialize(&change)
    }
}
