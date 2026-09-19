//! Browser output keeps Float32 capture bands bounded and spools them to the
//! file worker. Its synchronous OPFS reader feeds the same streaming CMM,
//! resampler and codecs as native export, without a full-frame Wasm allocation.
use super::*;
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::{ExportFormat, ExportRecipe};
use std::io::{Seek, SeekFrom, Write};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[wasm_bindgen]
pub struct WebCaptureControl {
    pub(super) inner: CaptureControl,
}
#[wasm_bindgen]
impl WebCaptureControl {
    pub fn cancel(&self) {
        self.inner.cancel();
    }
    pub fn cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

pub(super) fn cancelled(control: &CaptureControl) -> Result<(), JsValue> {
    if control.is_cancelled() {
        let error = js_sys::Error::new("Image operation cancelled");
        error.set_name("AbortError");
        Err(error.into())
    } else {
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OutputMetadata {
    token: String,
    extent: [u32; 2],
    color: layer_core::color::DocumentColor,
    resolution: Option<layer_core::ImageResolution>,
    recipe: ExportRecipe,
    original: Option<String>,
    preview: bool,
    rendition: Option<layer_core::color::hdr::SdrRendition>,
    flatten: Option<layer_core::color::DocumentColor>,
    guide: Option<GuideMetadata>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GuideMetadata {
    extent: [u32; 2],
    peak: f32,
    buffer: u32,
}

#[wasm_bindgen]
impl WebApp {
    pub fn capture_control(&self) -> WebCaptureControl {
        WebCaptureControl {
            inner: CaptureControl::default(),
        }
    }

    pub fn histogram(&self, control: &WebCaptureControl) -> Result<js_sys::Promise, JsValue> {
        self.session.require_document_snapshot_idle().map_err(js)?;
        let project = self.session.capture_project_recovery().map_err(js)?;
        let epoch = self.session.state().document_file.epoch;
        let revision = project.document.revision;
        let background = self.session.engine().view().background_rgba_linear;
        let time = self.session.engine().animation_time();
        let sampled_time = project.document.has_animated_effects().then_some(time);
        let gpu = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Canvas unavailable"))?
            .renderer
            .snapshot_gpu();
        let control = control.inner.clone();
        Ok(future_to_promise(async move {
            raster_project::wait_backing(&project).await?;
            let mut renderer = gpu
                .capture(
                    project,
                    background,
                    time,
                    Default::default(),
                    control.clone(),
                )
                .map_err(js)?;
            let result = renderer.histogram_async().await.map_err(js)?;
            cancelled(&control)?;
            serialize(
                &serde_json::json!({"epoch":epoch,"revision":revision,"axis":result.axis(),"histogram":result,"sampled_time":sampled_time}),
            )
        }))
    }

    pub fn export_image(
        &self,
        id: u32,
        value: JsValue,
        control: &WebCaptureControl,
        preview: Option<bool>,
    ) -> Result<js_sys::Promise, JsValue> {
        let preview = preview.unwrap_or(false);
        let recipe: ExportRecipe = serde_wasm_bindgen::from_value(value).map_err(js)?;
        recipe.validate().map_err(js)?;
        let snapshot = self.session.capture_project_export(id).map_err(js)?;
        let gpu = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Wait for the canvas"))?
            .renderer
            .snapshot_gpu();
        let control = control.inner.clone();
        Ok(future_to_promise(render_output(
            gpu, snapshot, recipe, control, preview, None,
        )))
    }
}

pub(super) async fn render_output(
    gpu: layer_render_wgpu::snapshot::SnapshotGpu,
    snapshot: layer_ui::DocumentExport,
    recipe: ExportRecipe,
    control: CaptureControl,
    preview: bool,
    flatten: Option<layer_core::color::DocumentColor>,
) -> Result<JsValue, JsValue> {
    raster_project::wait_backing(&snapshot.project).await?;
    let mut metadata = OutputMetadata {
        token: String::new(),
        extent: [
            snapshot.project.document.width,
            snapshot.project.document.height,
        ],
        color: snapshot.project.document.color,
        resolution: recipe
            .output_resolution(snapshot.project.document.resolution)
            .map_err(js)?,
        recipe,
        original: None,
        preview,
        rendition: snapshot.project.document.color.depth.is_float().then_some(snapshot.project.document.sdr_rendition),
        flatten,
        guide: None,
    };
    let mut capture = gpu
        .capture(
            snapshot.project,
            snapshot.background,
            snapshot.time,
            Default::default(),
            control.clone(),
        )
        .map_err(js)?;
    let before = if preview && metadata.rendition.is_none() {
        Some(
            capture
                .preview_document_async([512, 384], layer_core::color::RgbSpace::Srgb)
                .await
                .map_err(js)?,
        )
    } else {
        None
    };
    let extent = metadata.recipe.size.extent(metadata.extent).map_err(js)?;
    let original = (flatten.is_none() && !metadata.recipe.format.is_hdr()).then(|| capture.output_source(
        extent, &metadata.recipe.interpretation(), metadata.recipe.encoding, metadata.recipe.background.matte(),
    )).flatten();
    let buffers = if let Some(original) = original {
        let project =
            layer_color::photo_project((*original).clone(), "Original", metadata.color.depth)
                .map_err(js)?;
        let packed = raster_project::pack(project).await?;
        metadata.original = Some(
            js_sys::Reflect::get(&packed, &js("metadata"))?
                .as_string()
                .ok_or_else(|| js("Missing original metadata"))?,
        );
        js_sys::Reflect::get(&packed, &js("buffers"))?.dyn_into::<js_sys::Array>()?
    } else {
        js_sys::Array::new()
    };
    // Codecs consume pixels on their file worker, but illumination analysis is
    // always the shared GPU algorithm. Only its bounded guide crosses to CPU.
    if metadata.rendition.is_some() && (preview ||
        (!metadata.recipe.format.is_hdr() && !flatten.is_some_and(|c| c.depth.is_float()))) {
        let guide = capture.local_tone_guide_async().await.map_err(js)?;
        cancelled(&control)?;
        metadata.guide = Some(GuideMetadata {
            extent: guide.extent, peak: guide.peak, buffer: buffers.length(),
        });
        let samples = js_sys::Float32Array::from(guide.samples.as_flattened());
        buffers.push(&js_sys::Uint8Array::new(&samples.buffer()));
    }
    metadata.token = raster_worker::call_cancellable(
        "output-begin", "", &js_sys::Array::new(), control.clone(),
    ).await?
    .as_string()
    .ok_or_else(|| js("Missing output worker token"))?;
    let result = async {
        if metadata.original.is_none() {
            let mut y = 0;
            while y < metadata.extent[1] {
                cancelled(&control)?;
                let (rows, pixels) = capture.read_band_async(y).await.map_err(js)?;
                let mut bytes = Vec::with_capacity(pixels.len() * 16);
                for pixel in pixels {
                    for channel in pixel {
                        bytes.extend_from_slice(&channel.to_le_bytes());
                    }
                }
                let parts = js_sys::Array::new();
                parts.push(&js_sys::Uint8Array::from(bytes.as_slice()));
                drop(bytes);
                JsFuture::from(raster_worker::call("output-band", &metadata.token, &parts)?)
                    .await?;
                y += rows;
            }
        }
        drop(capture);
        raster_worker::call_cancellable(
            "output-encode", &serde_json::to_string(&metadata).map_err(js)?,
            &buffers, control.clone(),
        ).await
    }
    .await;
    if result.is_err() || control.is_cancelled() {
        let _ = JsFuture::from(raster_worker::call(
            "output-close",
            &metadata.token,
            &js_sys::Array::new(),
        )?)
        .await;
    }
    cancelled(&control)?;
    let result = result?;
    if let Some(before) = before {
        let previews = js_sys::Array::new();
        previews.push(&preview_value(&before)?);
        previews.push(&js_sys::Reflect::get(&result, &js("preview"))?);
        js_sys::Reflect::set(&result, &js("previews"), &previews)?;
    }
    Ok(result)
}

/// The worker owns the synchronous OPFS file. Rust controls codec seek/write
/// positions; neither encoded output nor the Float32 photograph is buffered whole.
struct WorkerFile {
    write: js_sys::Function,
    position: u64,
    length: u64,
}
impl Write for WorkerFile {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = self
            .position
            .checked_add(bytes.len() as u64)
            .filter(|v| *v <= 4 * 1024 * 1024 * 1024)
            .ok_or_else(|| std::io::Error::other("Export exceeds its 4 GiB file budget"))?;
        let written = self
            .write
            .call2(
                &JsValue::NULL,
                &JsValue::from_f64(self.position as f64),
                &js_sys::Uint8Array::from(bytes),
            )
            .map_err(|e| std::io::Error::other(format!("{e:?}")))?
            .as_f64()
            .unwrap_or(0.) as usize;
        if written != bytes.len() {
            return Err(std::io::Error::other("Incomplete export write"));
        }
        self.position = end;
        self.length = self.length.max(end);
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Seek for WorkerFile {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        self.position = match from {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::Current(p) => self.position.checked_add_signed(p),
            SeekFrom::End(p) => self.length.checked_add_signed(p),
        }
        .filter(|v| *v <= 4 * 1024 * 1024 * 1024)
        .ok_or_else(|| std::io::Error::other("Invalid export seek"))?;
        Ok(self.position)
    }
}

#[wasm_bindgen]
pub async fn raster_worker_output(
    metadata: &str,
    buffers: js_sys::Array,
    read: js_sys::Function,
    write: js_sys::Function,
) -> Result<JsValue, JsValue> {
    let metadata: OutputMetadata = serde_json::from_str(metadata).map_err(js)?;
    let guide = metadata.guide.as_ref().map(|wire| {
        let bytes = js_sys::Uint8Array::new(&buffers.get(wire.buffer));
        if wire.extent.contains(&0) || wire.extent.iter().any(|n| *n > layer_core::color::hdr::LOCAL_GUIDE_EDGE)
            || bytes.length() as usize != wire.extent[0] as usize * wire.extent[1] as usize * 16 {
            return Err(js("Invalid output illumination guide"));
        }
        let mut samples = vec![[0.; 4]; bytes.length() as usize / 16];
        js_sys::Float32Array::new_with_byte_offset_and_length(&bytes.buffer(), bytes.byte_offset(), bytes.length()/4)
            .copy_to(samples.as_flattened_mut());
        if !wire.peak.is_finite() || samples.iter().flatten().any(|v| !v.is_finite()) {
            return Err(js("Non-finite output illumination guide"));
        }
        Ok(layer_core::color::hdr::LocalToneGuide {
            extent: wire.extent, document_extent: metadata.extent, samples, peak: wire.peak,
        })
    }).transpose()?;
    let recipe = &metadata.recipe;
    recipe.validate().map_err(js)?;
    let target = recipe.interpretation();
    let extent = recipe.size.extent(metadata.extent).map_err(js)?;
    let output = WorkerFile {
        write,
        position: 0,
        length: 0,
    };
    let row_bytes = metadata.extent[0] as usize * 16;
    let mut bytes = vec![0; row_bytes];
    let mut read_row = |y: u32, pixels: &mut [[f32;4]]| {
                let result = read
                    .call2(
                        &JsValue::NULL,
                        &JsValue::from_f64(y as f64 * row_bytes as f64),
                        &JsValue::from_f64(row_bytes as f64),
                    )
                    .map_err(|e| format!("{e:?}"))?;
                let data = js_sys::Uint8Array::new(&result);
                if data.length() as usize != row_bytes {
                    return Err("Incomplete output source row".into());
                }
                data.copy_to(&mut bytes);
                for (pixel, source) in pixels.iter_mut().zip(bytes.chunks_exact(16)) {
                    *pixel = std::array::from_fn(|c| {
                        f32::from_le_bytes(source[c * 4..c * 4 + 4].try_into().unwrap())
                    });
                }
                Ok(())
            };
    let before = if metadata.preview && metadata.rendition.is_some() {
        Some(mapped_preview(metadata.extent, metadata.color.space, metadata.rendition, guide.as_ref(), &mut read_row).map_err(js)?)
    } else { None };
    if recipe.format.is_hdr() {
        if !metadata.color.depth.is_float() { return Err(js("HDR output requires HDR artwork")); }
        if recipe.format.gainmap().is_some() { return Err(js("HDR gain-map output is unavailable in this browser; choose HDR PNG")); }
        let mut resampler = layer_color::RowResampler::new(metadata.extent, extent).map_err(js)?;
        let mut rows = |y,pixels: &mut [[f32;4]]| {if y==0 {resampler=layer_color::RowResampler::new(metadata.extent,extent)?;}resampler.read_row(y,pixels,&mut read_row)};
        if recipe.format == ExportFormat::Exr {
            let after=if metadata.preview {
                Some(mapped_preview(extent,metadata.color.space,metadata.rendition,guide.as_ref(),&mut rows).map_err(js)?)
            } else {
                layer_color::photo::write_exr_rows(output,extent,metadata.color.space,metadata.resolution,rows).map_err(js)?;None
            };
            let result=serialize(&serde_json::json!({"clipped_channels":0,"extent":extent}))?;
            if let Some(after)=after {let values=js_sys::Array::new();values.push(&preview_value(&before.unwrap())?);values.push(&preview_value(&after)?);js_sys::Reflect::set(&result,&js("previews"),&values)?;}
            return Ok(result);
        }
        let (preview, stats) = if metadata.preview {
            let (size, mut pixels, stats) = layer_color::photo::preview_hdr_rows(extent,[512,384],metadata.color.space,&mut rows).map_err(js)?;
            let guide = guide.as_ref().ok_or_else(|| js("Missing output illumination guide"))?;
            let to_working = layer_core::color::RgbSpace::Srgb.linear_transform(metadata.color.space);
            let mapper = metadata.rendition.unwrap_or_default().mapper(metadata.color.space,layer_core::color::RgbSpace::Srgb);
            for (i,p) in pixels.iter_mut().enumerate() {
                let rgb=layer_core::color::rgb::apply(to_working,[p[0],p[1],p[2]].map(f64::from));
                p[..3].copy_from_slice(&rgb.map(|v|v as f32));
                *p=mapper.map_local_premultiplied(*p,std::array::from_fn(|c| (if c==0 { i as u32%size[0] } else { i as u32/size[0] }) as f32 * metadata.extent[c] as f32/size[c] as f32 + 0.5*metadata.extent[c] as f32/size[c] as f32),&guide);
            }
            (Some(layer_render_wgpu::snapshot::SnapshotPreview {extent:size,pixels,space:layer_core::color::RgbSpace::Srgb}),stats)
        } else {
            (None,layer_color::photo::write_hdr_png_rows(output,extent,metadata.color.space,metadata.resolution,recipe.format.maps_hdr_range(),rows).map_err(js)?)
        };
        let result=serialize(&serde_json::json!({"clipped_channels":stats.clipped_channels,"extent":extent}))?;
        if let Some(after)=preview { let values=js_sys::Array::new(); values.push(&preview_value(&before.unwrap())?);values.push(&preview_value(&after)?);js_sys::Reflect::set(&result,&js("previews"),&values)?; }
        return Ok(result);
    }
    let mut preview = None;
    let mut flattened = None;
    let write = |extent, target: &_, rows: &mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>| {
        if let Some(color) = metadata.flatten {
            flattened = Some(layer_color::flattened_document(
                extent,
                color,
                metadata.resolution,
                target,
                512 * 1024 * 1024,
                rows,
            )?);
            return Ok(());
        }
        if metadata.preview {
            let (extent, pixels) = layer_color::preview_encoded_rows(
                extent,
                [512, 384],
                layer_core::color::RgbSpace::Srgb,
                target,
                rows,
            )?;
            preview = Some(layer_render_wgpu::snapshot::SnapshotPreview {
                extent,
                pixels,
                space: layer_core::color::RgbSpace::Srgb,
            });
            return Ok(());
        }
        match recipe.format {
            ExportFormat::Exr | ExportFormat::PngHdr | ExportFormat::PngHdrMapped | ExportFormat::JpegHdr | ExportFormat::JpegHdrMapped | ExportFormat::AvifHdr | ExportFormat::AvifHdrMapped => Err("HDR delivery is not enabled on this host".into()),
            ExportFormat::Png => layer_color::photo::write_png_rows(
                output,
                extent,
                target,
                metadata.resolution,
                rows,
            ),
            ExportFormat::Tiff => layer_color::photo::write_tiff_rows(
                output,
                extent,
                target,
                metadata.resolution,
                rows,
            ),
            ExportFormat::Jpeg => layer_color::photo::write_jpeg_rows_with_options(
                output,
                extent,
                target,
                metadata.resolution,
                layer_color::photo::JpegEncodeOptions::from_memory_budget(recipe.jpeg_quality, raster_project::photo_memory_budget()),
                rows,
            ),
        }
    };
    let clipped = if let Some(original) = metadata.original {
        let project = raster_project::unpack(&original, buffers, true).await?;
        let source = project
            .document
            .layers
            .iter()
            .find_map(|l| l.source.as_ref())
            .ok_or_else(|| js("Missing original source"))?;
        if source.extent != extent
            || source.interpretation.channels != target.channels
            || source.interpretation.depth != target.depth
            || source.interpretation.profile != target.profile
        {
            return Err(js("Original source does not match this output"));
        }
        let mut rows = source.rows();
        write(extent, &target, &mut |y, row| rows.read(y, row)).map_err(js)?;
        0
    } else {
        layer_color::encode_working_rows_with_guide(
            metadata.color.space,
            metadata.extent,
            extent,
            &target,
            recipe.encoding,
            recipe.background.matte(),
            if metadata.flatten.is_some_and(|c|c.depth.is_float()) { None } else { metadata.rendition },
            guide.as_ref(),
            &mut read_row,
            write,
        )
        .map_err(js)?
        .clipped_channels
    };
    if let Some(project) = flattened {
        let wire = raster_project::pack(project).await?;
        js_sys::Reflect::set(&wire, &js("clipped"), &JsValue::from_f64(clipped as f64))?;
        return Ok(wire);
    }
    let result = serialize(&serde_json::json!({"clipped_channels": clipped, "extent": extent}))?;
    if let Some(preview) = preview {
        js_sys::Reflect::set(&result, &js("preview"), &preview_value(&preview)?)?;
        if let Some(before)=before { let values=js_sys::Array::new(); values.push(&preview_value(&before)?); values.push(&preview_value(&preview)?); js_sys::Reflect::set(&result,&js("previews"),&values)?; }
    }
    Ok(result)
}

fn preview_value(
    preview: &layer_render_wgpu::snapshot::SnapshotPreview,
) -> Result<JsValue, JsValue> {
    let value = js_sys::Object::new();
    js_sys::Reflect::set(&value, &js("extent"), &serialize(&preview.extent)?)?;
    js_sys::Reflect::set(
        &value,
        &js("pixels"),
        &js_sys::Uint8Array::from(preview.srgb_bytes().map_err(js)?.as_slice()),
    )?;
    Ok(value.into())
}

fn mapped_preview(extent:[u32;2],space:layer_core::color::RgbSpace,rendition:Option<layer_core::color::hdr::SdrRendition>,guide:Option<&layer_core::color::hdr::LocalToneGuide>,read:&mut impl FnMut(u32,&mut [[f32;4]])->Result<(),String>) -> Result<layer_render_wgpu::snapshot::SnapshotPreview,String> {
    let target=layer_ui::ExportRecipe::web_share().interpretation();
    let mut preview=None;
    layer_color::encode_working_rows_with_guide(space,extent,extent,&target,Default::default(),None,rendition,guide,read,|size,target,rows| {
        let (extent,pixels)=layer_color::preview_encoded_rows(size,[512,384],layer_core::color::RgbSpace::Srgb,target,rows)?;
        preview=Some(layer_render_wgpu::snapshot::SnapshotPreview{extent,pixels,space:layer_core::color::RgbSpace::Srgb});Ok(())
    })?;
    Ok(preview.unwrap())
}
