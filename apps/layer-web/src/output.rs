//! Browser output keeps Float32 capture bands bounded and spools them to the
//! file worker. Its synchronous OPFS reader feeds the same streaming CMM,
//! resampler and codecs as native export, without a full-frame Wasm allocation.
use super::*;
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::{ExportFormat, ExportRecipe};
use std::io::{Seek, SeekFrom, Write};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

#[derive(serde::Serialize, serde::Deserialize)]
struct OutputMetadata {
    token: String,
    extent: [u32; 2],
    color: layer_core::color::DocumentColor,
    resolution: Option<layer_core::ImageResolution>,
    recipe: ExportRecipe,
    original: Option<String>,
}

#[wasm_bindgen]
impl WebApp {
    pub fn export_image(&self, id: u32, value: JsValue) -> Result<js_sys::Promise, JsValue> {
        let recipe: ExportRecipe = serde_wasm_bindgen::from_value(value).map_err(js)?;
        recipe.validate().map_err(js)?;
        let snapshot = self.session.capture_project_export(id).map_err(js)?;
        layer_color::WorkingEncoder::new(
            snapshot.project.document.color.space,
            &recipe.interpretation(),
            recipe.encoding,
        )
        .map_err(js)?;
        let gpu = self
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or_else(|| js("Wait for the canvas"))?
            .renderer
            .snapshot_gpu();
        Ok(future_to_promise(async move {
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
            };
            let mut capture = gpu
                .capture(
                    snapshot.project,
                    snapshot.background,
                    snapshot.time,
                    Default::default(),
                    CaptureControl::default(),
                )
                .map_err(js)?;
            let extent = metadata.recipe.size.extent(metadata.extent).map_err(js)?;
            let original = if extent == metadata.extent
                && metadata.recipe.encoding.conversion == Default::default()
                && metadata.recipe.background.matte().is_none()
            {
                capture.identity_source(&metadata.recipe.interpretation())
            } else {
                None
            };
            let buffers = if let Some(original) = original {
                let project = layer_color::photo_project(
                    (*original).clone(),
                    "Original",
                    metadata.color.depth,
                )
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
            metadata.token = JsFuture::from(raster_worker::call(
                "output-begin",
                "",
                &js_sys::Array::new(),
            )?)
            .await?
            .as_string()
            .ok_or_else(|| js("Missing output worker token"))?;
            let result = async {
                if metadata.original.is_none() {
                    let mut y = 0;
                    while y < metadata.extent[1] {
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
                        JsFuture::from(raster_worker::call(
                            "output-band",
                            &metadata.token,
                            &parts,
                        )?)
                        .await?;
                        y += rows;
                    }
                }
                drop(capture);
                JsFuture::from(raster_worker::call(
                    "output-encode",
                    &serde_json::to_string(&metadata).map_err(js)?,
                    &buffers,
                )?)
                .await
            }
            .await;
            if result.is_err() {
                let _ = JsFuture::from(raster_worker::call(
                    "output-close",
                    &metadata.token,
                    &js_sys::Array::new(),
                )?)
                .await;
            }
            result
        }))
    }
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
    let recipe = &metadata.recipe;
    recipe.validate().map_err(js)?;
    let target = recipe.interpretation();
    let extent = recipe.size.extent(metadata.extent).map_err(js)?;
    let output = WorkerFile {
        write,
        position: 0,
        length: 0,
    };
    let write = |extent, target: &_, rows: &mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>| {
        match recipe.format {
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
            ExportFormat::Jpeg => layer_color::photo::write_jpeg_rows(
                output,
                extent,
                target,
                metadata.resolution,
                recipe.jpeg_quality,
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
        let row_bytes = metadata.extent[0] as usize * 16;
        let mut bytes = vec![0; row_bytes];
        layer_color::encode_working_rows(
            metadata.color.space,
            metadata.extent,
            extent,
            &target,
            recipe.encoding,
            recipe.background.matte(),
            |y, pixels| {
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
            },
            write,
        )
        .map_err(js)?
        .clipped_channels
    };
    serialize(&serde_json::json!({"clipped_channels": clipped, "extent": extent}))
}
