//! One reserved request and one source allowance for the complete browser batch.
use super::*;
use layer_ui::{DocumentRequest, HostRequestKind, ImagePlacementContext};
use std::sync::{Arc, Mutex};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[wasm_bindgen]
pub struct WebImageImport {
    id: u32,
    context: ImagePlacementContext,
    lost: Arc<Mutex<Option<String>>>,
}

#[wasm_bindgen]
pub struct WebPreparedImages {
    request: WebImageImport,
    sources: Vec<(String, layer_core::color::source::SourceImage)>,
    control: layer_render_wgpu::snapshot::CaptureControl,
}

fn active(session: &UiSession<WebRenderer>, id: u32) -> Result<(), JsValue> {
    if session.state().requests.iter().any(|r| {
        r.id == id
            && matches!(
                r.kind,
                HostRequestKind::Document {
                    request: DocumentRequest::Place | DocumentRequest::Paste
                }
            )
    }) {
        Ok(())
    } else {
        Err(js("The image import request is no longer active"))
    }
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
        let live = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Wait for the canvas"))?;
        Ok(WebImageImport {
            id,
            context,
            lost: live.lost.clone(),
        })
    }
    pub fn prepare_images(
        &self,
        request: &WebImageImport,
        files: js_sys::Array,
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
        let photo_policy = self.session.state().settings.photo_open;
        let working_space = self.session.engine().document().color.space;
        let control = control.inner.clone();
        Ok(future_to_promise(async move {
            if files.length() == 0 {
                return Err(js("Choose at least one image"));
            }
            let mut images = layer_ui::ImageImportBatch::new(photo_policy, working_space,
                layer_color::photo::DecodeLimits::from_memory_budget(raster_project::photo_memory_budget()));
            for file in files.iter() {
                output::cancelled(&control)?;
                if request.lost.lock().unwrap().is_some() {
                    return Err(js("The canvas was lost while importing; try again"));
                }
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
                output::cancelled(&control)?;
                let bytes = js_sys::Uint8Array::new(&buffer);
                let project = raster_project::open(
                    bytes,
                    raster_project::OpenOptions {
                        dimension: layer_core::ProjectLimits::default().dimension,
                        photo_policy,
                        name,
                        intent: layer_ui::ImportIntent::Place,
                        source_bytes: Some(images.limits().source_bytes),
                    },
                )
                .await?.project;
                output::cancelled(&control)?;
                let layer = project
                    .document
                    .layers
                    .iter()
                    .find(|l| l.source.is_some())
                    .ok_or_else(|| js("The selected file is not a photo"))?;
                let name = layer.name.to_string();
                images.append(name, (**layer.source.as_ref().unwrap()).clone(), control.is_cancelled()).map_err(js)?;
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
                request,
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
        let live = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Canvas unavailable"))?;
        if !Arc::ptr_eq(&live.lost, &request.lost) || live.lost.lock().unwrap().is_some() {
            return Err(js("The canvas changed while importing; try again"));
        }
        self.session
            .place_layer_sources(
                prepared.sources,
                request.context.center,
                request.context.destination,
            )
            .map_err(js)?;
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
