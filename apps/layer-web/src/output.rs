//! Browser output keeps Float32 capture bands bounded and spools them to the
//! file worker. Its synchronous OPFS reader feeds the same streaming CMM,
//! resampler and codecs as native export. Gain-map codecs admit their complete
//! image working sets against the browser's per-operation memory allowance.
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
    #[serde(default)]
    clip: Option<ClipMetadata>,
    /// Buffers holding the document's Exif, XMP and IPTC blocks.
    #[serde(default)]
    photo: [Option<u32>; 3],
}

/// A clipboard copy of `extent` document pixels at `origin`.
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct ClipMetadata {
    pub origin: [u32; 2],
    pub document: [u32; 2],
    /// Write the document-depth source as well as the PNG.
    pub source: bool,
}

pub(super) fn clip_metadata(
    token: &str,
    extent: [u32; 2],
    document: layer_core::SceneView<'_>,
    rendition: Option<layer_core::color::hdr::SdrRendition>,
    guide: Option<([u32; 2], f32)>,
    clip: ClipMetadata,
) -> String {
    serde_json::to_string(&OutputMetadata {
        token: token.into(),
        extent,
        color: document.composition().color,
        resolution: document.composition().resolution,
        recipe: ExportRecipe::web_share(),
        original: None,
        preview: false,
        rendition,
        flatten: None,
        guide: guide.map(|(extent, peak)| GuideMetadata { extent, peak, buffer: 0 }),
        clip: Some(clip),
        photo: [None; 3],
    })
    .unwrap()
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

    pub fn export_image(
        &self,
        id: u32,
        value: JsValue,
        control: &WebCaptureControl,
        preview: Option<bool>,
    ) -> Result<js_sys::Promise, JsValue> {
        let preview = preview.unwrap_or(false);
        let (snapshot, recipe) = if value.is_null() || value.is_undefined() {
            (self.session.export_dialog_source(id).map_err(js)?,
                self.session.export_dialog_recipe(id).map_err(color_preferences::color_feature_rejection)?)
        } else {
            (self.session.capture_project_export(id).map_err(js)?, serde_wasm_bindgen::from_value::<ExportRecipe>(value).map_err(js)?)
        };
        recipe.validate().map_err(color_preferences::color_feature_rejection)?;
        recipe.output_extent(snapshot.composition().size).map_err(color_preferences::color_feature_rejection)?;
        recipe.output_resolution(snapshot.composition().resolution).map_err(color_preferences::color_feature_rejection)?;
        let format = recipe.format;
        let localization = self.session.localization().clone();
        let gpu = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Wait for the canvas"))?
            .snapshot_gpu();
        let control = control.inner.clone();
        Ok(future_to_promise(async move {
            let result = render_output(gpu, snapshot, recipe, control, preview, None).await.map_err(|error| {
                if js_sys::Reflect::get(&error, &js("name")).ok().and_then(|name| name.as_string()).as_deref() == Some("AbortError") {
                    error
                } else {
                    color_preferences::color_feature_rejection(color_preferences::color_feature_reason(error))
                }
            })?;
            if preview {
                let clipped = serde_wasm_bindgen::from_value::<u64>(js_sys::Reflect::get(&result, &js("clipped_channels"))?).map_err(js)? > 0;
                js_sys::Reflect::set(&result, &js("range_exceeded"), &JsValue::from_bool(clipped && format.is_hdr() && !format.maps_hdr_range()))?;
                js_sys::Reflect::set(&result, &js("status"), &js(&layer_ui::color_feature_copy::export_preview_status(&localization, format, clipped)))?;
            }
            Ok(result)
        }))
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
    artwork_transfer::wait_backing(&snapshot.capture.artwork).await?;
    let mut metadata = OutputMetadata {
        token: String::new(),
        extent: snapshot.composition().size,
        color: snapshot.composition().color,
        resolution: recipe
            .output_resolution(snapshot.composition().resolution)
            .map_err(color_preferences::color_feature_rejection)?,
        recipe,
        original: None,
        preview,
        rendition: snapshot.composition().color.depth.is_float().then_some(snapshot.output().sdr),
        flatten,
        guide: None,
        clip: None,
        photo: [None; 3],
    };
    let photo = snapshot.capture.artwork.metadata.clone();
    let mut capture = gpu
        .capture(
            snapshot.capture,
            control.clone(),
        )
        .map_err(js)?;
    let extent = metadata.recipe.output_extent(metadata.extent).map_err(color_preferences::color_feature_rejection)?;
    let original = (!preview && flatten.is_none() && !metadata.recipe.format.is_hdr()).then(|| capture.output_source(
        extent, &metadata.recipe.interpretation(), metadata.recipe.encoding, metadata.recipe.background.matte(),
    )).flatten();
    let mut packed = if let Some(original) = original {
        let project =
            layer_color::photo_project((*original).clone(), Default::default(), layer_core::DocumentNames { paint: "Original".into(), paper: "".into() }, metadata.color.depth)
                .map_err(js)?;
        let packed = artwork_transfer::pack(project.artwork).await?;
        metadata.original = Some(
            js_sys::Reflect::get(&packed, &js("metadata"))?
                .as_string()
                .ok_or_else(|| js("Missing original metadata"))?,
        );
        packed
    } else {
        let packed = artwork_transfer::PackedArtwork::empty();
        js_sys::Reflect::set(&packed, &js("buffers"), &js_sys::Array::new())?;
        packed
    };
    for (index, block) in metadata.photo.iter_mut().zip(photo.blocks()) {
        if let Some(bytes) = block {
            *index = Some(packed.push_buffer(bytes.as_ref())?);
        }
    }
    // Codecs consume pixels on their file worker, but illumination analysis is
    // always the shared GPU algorithm. Only its bounded guide crosses to CPU.
    if metadata.rendition.is_some() && (preview || metadata.recipe.format.gainmap().is_some() ||
        (!metadata.recipe.format.is_hdr() && !flatten.is_some_and(|c| c.depth.is_float()))) {
        let guide = capture.local_tone_guide_async().await.map_err(js)?;
        cancelled(&control)?;
        let samples: Vec<u8> = guide.samples.as_flattened().iter().flat_map(|value| value.to_le_bytes()).collect();
        metadata.guide = Some(GuideMetadata {
            extent: guide.extent, peak: guide.peak, buffer: packed.push_buffer(&samples)?,
        });
    }
    let buffers = js_sys::Reflect::get(&packed, &js("buffers"))?.dyn_into::<js_sys::Array>()?;
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
    Ok(result)
}

/// The worker owns the synchronous OPFS file. Rust controls codec seek/write
/// positions. Capture is spooled in bands; gain-map codecs separately admit
/// their complete image buffers before allocation.
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

async fn write_sdr_rows(output:impl Write+Seek,extent:[u32;2],target:&layer_core::color::source::SourceInterpretation,recipe:&ExportRecipe,delivery:&layer_color::photo::DeliveryMetadata,rows:&mut impl layer_color::photo::RowCursor)->Result<(),String> {
        match recipe.format {
            ExportFormat::Exr | ExportFormat::PngHdr | ExportFormat::PngHdrMapped | ExportFormat::JpegHdr | ExportFormat::JpegHdrMapped | ExportFormat::AvifHdr | ExportFormat::AvifHdrMapped => Err("HDR delivery is not enabled on this host".into()),
            ExportFormat::Png => layer_color::photo::write_png_rows_async(
                output,
                extent,
                target,
                delivery,
                rows,
            ).await,
            ExportFormat::Tiff => layer_color::photo::write_tiff_rows_async(
                output,
                extent,
                target,
                delivery,
                rows,
            ).await,
            ExportFormat::Jpeg => layer_color::photo::write_jpeg_rows_async(
                output,
                extent,
                target,
                delivery,
                layer_color::photo::JpegEncodeOptions::from_memory_budget(recipe.jpeg_quality, artwork_transfer::photo_memory_budget()),
                rows,
            ).await,
            ExportFormat::Webp => layer_color::photo::write_webp_rows_async(
                output,
                extent,
                target,
                delivery,
                artwork_transfer::photo_memory_budget().encode_bytes,
                rows,
            ).await,
        }
}

#[wasm_bindgen]
pub async fn raster_worker_output(
    metadata: &str,
    buffers: js_sys::Array,
    read: js_sys::Function,
    write: js_sys::Function,
) -> Result<JsValue, JsValue> {
    artwork_transfer::seed_memory_budget();
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
            extent: wire.extent,
            document_extent: metadata.clip.as_ref().map_or(metadata.extent, |clip| clip.document),
            samples,
            peak: wire.peak,
        })
    }).transpose()?;
    let recipe = &metadata.recipe;
    recipe.validate().map_err(color_preferences::color_feature_rejection)?;
    let [exif, xmp, iptc] = metadata.photo.map(|index| {
        index.map(|index| js_sys::Uint8Array::new(&buffers.get(index)).to_vec().into())
    });
    let delivery = layer_color::photo::DeliveryMetadata {
        resolution: metadata.resolution,
        photo: layer_core::PhotoMetadata { exif, xmp, iptc },
        policy: recipe.metadata,
    };
    delivery.photo.validate().map_err(js)?;
    let target = recipe.interpretation();
    let extent = recipe.output_extent(metadata.extent).map_err(color_preferences::color_feature_rejection)?;
    let output = WorkerFile {
        write,
        position: 0,
        length: 0,
    };
    let row_bytes = metadata.extent[0] as usize * 16;
    let mut bytes = vec![0; row_bytes];
    let transparent = std::cell::Cell::new(false);
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
                    if metadata.preview && pixel[3] < 1. { transparent.set(true); }
                }
                Ok(())
            };
    if let Some(clip) = &metadata.clip {
        let rows = layer_color::ClipRows {
            extent: metadata.extent,
            origin: clip.origin,
            color: metadata.color,
            resolution: metadata.resolution,
            rendition: metadata.rendition.zip(guide.as_ref()),
            source: clip.source,
            limit: artwork_transfer::photo_memory_budget().encode_bytes,
        };
        let (source, png) = layer_color::write_clip_rows(rows, &mut read_row).map_err(js)?;
        let mut result = match source {
            Some(source) => {
                let mut document = layer_core::Document::new(layer_core::authored::PortableId::random(), source.extent[0], source.extent[1], layer_core::DocumentNames { paint: "".into(), paper: "".into() });
                let root=document.artwork.root; document.artwork.compositions.get_mut(root).unwrap().color=metadata.color;
                let layer_core::SourceTarget::Paint(paint)=document.working.target.unwrap() else {unreachable!()};
                document.artwork.paint.get_mut(paint).unwrap().base=Some(layer_core::authored::PaintBase {image:std::sync::Arc::new(source).into(),offset:[0;2],policy:layer_core::authored::PaintBasePolicy::WorkingPixels});
                let stack=document.composition().result; document.artwork.stacks.get_mut(stack).unwrap().entries.truncate(1);
                artwork_transfer::pack(document.artwork).await?
            }
            None => {
                let empty = artwork_transfer::PackedArtwork::empty();
                js_sys::Reflect::set(&empty, &js("buffers"), &js_sys::Array::new())?;
                empty
            }
        };
        let png=result.push_buffer(png.as_slice())?;
        js_sys::Reflect::set(&result,&js("png"),&JsValue::from(png))?;
        return Ok(result.into_js());
    }
    let before = if metadata.preview {
        Some(if metadata.rendition.is_some() {
            mapped_preview(metadata.extent, metadata.color.space, metadata.rendition, guide.as_ref(), &mut read_row)
        } else {
            layer_ui::color_comparison::master_preview(metadata.extent, metadata.color.space, &mut read_row)
                .map(|(extent,pixels)|layer_render_wgpu::snapshot::SnapshotPreview {extent,pixels,space:layer_core::color::RgbSpace::Srgb})
        }.map_err(js)?)
    } else { None };
    if recipe.format.is_hdr() {
        if !metadata.color.depth.is_float() { return Err(js("HDR output requires HDR artwork")); }
        let mut resampler = layer_color::RowResampler::new(metadata.extent, extent).map_err(js)?;
        let mut rows = |y,pixels: &mut [[f32;4]]| {if y==0 {resampler=layer_color::RowResampler::new(metadata.extent,extent)?;}resampler.read_row(y,pixels,&mut read_row)};
        if let Some(format) = recipe.format.gainmap() {
            let rendition = metadata.rendition.ok_or_else(|| js("Missing SDR rendition"))?;
            let guide = guide.as_ref().ok_or_else(|| js("Missing output illumination guide"))?;
            let options = layer_color::photo::GainMapEncodeOptions::from_memory_budget(
                recipe.jpeg_quality, artwork_transfer::photo_memory_budget(),
            );
            // The editor cancels the isolated worker from its own event loop,
            // including during synchronous codec calls. No shared Wasm memory
            // or browser-provided photo decoder is required.
            let cancel = std::sync::atomic::AtomicBool::new(false);
            if metadata.preview {
                let (size, mut hdr, sdr, stats) = layer_color::photo::preview_gainmap_rows(
                    layer_color::photo::GainMapRender { extent, space: metadata.color.space, rendition, guide, matte: recipe.background.matte(), clip: true },
                    layer_ui::color_comparison::RASTER_BOUNDS,
                    format,
                    options,
                    &cancel,
                    &mut rows,
                ).map_err(js)?;
                let space = layer_core::color::RgbSpace::Srgb;
                let mapper = rendition.mapper(space, space);
                for (i, p) in hdr.iter_mut().enumerate() {
                    let position = [i as u32 % size[0], i as u32 / size[0]];
                    *p = mapper.map_local_premultiplied(*p,
                        std::array::from_fn(|c| (position[c] as f32 + 0.5)
                            * metadata.extent[c] as f32 / size[c] as f32), guide);
                }
                let image = |pixels| layer_render_wgpu::snapshot::SnapshotPreview { extent: size, space, pixels };
                let result = serialize(&serde_json::json!({"clipped_channels": stats.clipped_channels, "extent": extent, "transparent": transparent.get()}))?;
                let previews = js_sys::Array::new();
                previews.push(&output_preview_value(before.as_ref().ok_or_else(|| js("Missing artwork preview"))?)?);
                previews.push(&output_preview_value(&image(hdr))?);
                js_sys::Reflect::set(&result, &js("previews"), &previews)?;
                js_sys::Reflect::set(&result, &js("sdr_preview"), &output_preview_value(&image(sdr))?)?;
                return Ok(result);
            }
            let stats = layer_color::photo::write_gainmap_rows(
                output,
                layer_color::photo::GainMapRender { extent, space: metadata.color.space, rendition, guide, matte: recipe.background.matte(), clip: recipe.format.maps_hdr_range() },
                format,
                options,
                &delivery,
                &cancel,
                rows,
            ).map_err(js)?;
            return serialize(&serde_json::json!({"clipped_channels": stats.clipped_channels, "extent": extent, "transparent": transparent.get()}));
        }
        if recipe.format == ExportFormat::Exr {
            let after=if metadata.preview {
                Some(mapped_preview(extent,metadata.color.space,metadata.rendition,guide.as_ref(),&mut rows).map_err(js)?)
            } else {
                layer_color::photo::write_exr_rows(output,extent,metadata.color.space,metadata.resolution,rows).map_err(js)?;None
            };
            let result=serialize(&serde_json::json!({"clipped_channels":0,"extent":extent,"transparent":transparent.get()}))?;
            if let Some(after)=after {let values=js_sys::Array::new();values.push(&output_preview_value(&before.unwrap())?);values.push(&output_preview_value(&after)?);js_sys::Reflect::set(&result,&js("previews"),&values)?;}
            return Ok(result);
        }
        let (preview, stats) = if metadata.preview {
            let (size, mut pixels, stats) = layer_color::photo::preview_hdr_rows(extent,layer_ui::color_comparison::RASTER_BOUNDS,metadata.color.space,&mut rows).map_err(js)?;
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
            (None,layer_color::photo::write_hdr_png_rows(output,extent,metadata.color.space,&delivery,recipe.format.maps_hdr_range(),rows).map_err(js)?)
        };
        let result=serialize(&serde_json::json!({"clipped_channels":stats.clipped_channels,"extent":extent,"transparent":transparent.get()}))?;
        if let Some(after)=preview { let values=js_sys::Array::new(); values.push(&output_preview_value(&before.unwrap())?);values.push(&output_preview_value(&after)?);js_sys::Reflect::set(&result,&js("previews"),&values)?; }
        return Ok(result);
    }
    let mut preview = None;
    let mut flattened = None;
    let clipped = if let Some(original) = metadata.original {
        let project = artwork_transfer::unpack(&original, buffers).await?;
        let source = project.paint.iter().find_map(|(_,_,paint)| paint.base.as_ref().map(|base| base.image.storage()))
            .ok_or_else(|| js("Missing original source"))?;
        if source.extent != extent
            || source.interpretation.channels != target.channels
            || source.interpretation.depth != target.depth
            || source.interpretation.profile != target.profile
        {
            return Err(js("Original source does not match this output"));
        }
        let mut rows = source.rows();
        write_sdr_rows(output,extent,&target,recipe,&delivery,&mut rows).await.map_err(js)?;
        0
    } else {
    let write = |extent, target: &_, rows: &mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>| {
        if let Some(color) = metadata.flatten {
            flattened = Some(layer_color::flattened_document(
                extent,
                color,
                metadata.resolution,
                target,
                layer_color::photo::PhotoMemoryBudget::conversion_encode_limit(),
                rows,
            )?);
            return Ok(());
        }
        if metadata.preview {
            let (extent, pixels) = layer_color::preview_encoded_rows(
                extent,
                layer_ui::color_comparison::RASTER_BOUNDS,
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
        layer_core::package::archive::ready(write_sdr_rows(output,extent,target,recipe,&delivery,&mut layer_color::photo::ReadyRows(rows)))
    };
        layer_color::encode_working_rows_with_guide(
            metadata.color.space,
            metadata.extent,
            extent,
            layer_color::WorkingRowsOptions { target: &target, encoding: recipe.encoding, matte: recipe.background.matte(), rendition: if metadata.flatten.is_some_and(|c|c.depth.is_float()) { None } else { metadata.rendition }, guide: guide.as_ref() },
            &mut read_row,
            write,
        )
        .map_err(js)?
        .clipped_channels
    };
    if let Some(mut project) = flattened {
        project.artwork.metadata = std::sync::Arc::new(delivery.photo);
        let wire = artwork_transfer::pack(project.artwork).await?;
        js_sys::Reflect::set(&wire, &js("clipped"), &JsValue::from_f64(clipped as f64))?;
        return Ok(wire.into_js());
    }
    let result = serialize(&serde_json::json!({"clipped_channels": clipped, "extent": extent, "transparent": transparent.get()}))?;
    if let Some(preview) = preview {
        js_sys::Reflect::set(&result, &js("preview"), &output_preview_value(&preview)?)?;
        if let Some(before)=before { let values=js_sys::Array::new(); values.push(&output_preview_value(&before)?); values.push(&output_preview_value(&preview)?); js_sys::Reflect::set(&result,&js("previews"),&values)?; }
    }
    Ok(result)
}

fn output_preview_value(preview: &layer_render_wgpu::snapshot::SnapshotPreview) -> Result<JsValue, JsValue> {
    let checker = layer_ui::TRANSPARENCY_CHECKER.map(|gray| gray.linear()[0]);
    let pixels = preview.pixels.iter().enumerate().map(|(index,pixel)| {
        let rgb = layer_ui::color_comparison::over_checker(*pixel, [index as u32 % preview.extent[0], index as u32 / preview.extent[0]], checker);
        [rgb[0] as f32,rgb[1] as f32,rgb[2] as f32,1.]
    }).collect();
    preview_value(&layer_render_wgpu::snapshot::SnapshotPreview {extent:preview.extent,space:preview.space,pixels})
}

pub(super) fn preview_value(
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
    layer_color::encode_working_rows_with_guide(
        space,
        extent,
        extent,
        layer_color::WorkingRowsOptions { target: &target, encoding: Default::default(), matte: None, rendition, guide },
        read,
        |size,target,rows| {
        let (extent,pixels)=layer_color::preview_encoded_rows(size,layer_ui::color_comparison::RASTER_BOUNDS,layer_core::color::RgbSpace::Srgb,target,rows)?;
        preview=Some(layer_render_wgpu::snapshot::SnapshotPreview{extent,pixels,space:layer_core::color::RgbSpace::Srgb});Ok(())
    },
    )?;
    Ok(preview.unwrap())
}
