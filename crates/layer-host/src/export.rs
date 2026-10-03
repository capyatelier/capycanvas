//! Profiled delivery copies rendered from immutable native snapshots on a
//! worker. The output recipe never edits the master document.
use crate::{Renderer, tasks::Preview};
use layer_core::color::RgbSpace;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu, SnapshotPreview, SnapshotRenderer};
use layer_ui::{ColorFeatureError, DocumentExport, ExportFormat, ExportRecipe, UiSession};
use serde_json::{Value, json};
use std::io::{BufWriter, Seek, Write};

/// Encodes the renderer's configured output extent and resolution.
pub fn write_recipe(
    renderer: &mut SnapshotRenderer,
    output: impl Write + Seek,
    recipe: &ExportRecipe,
) -> Result<layer_color::OutputStatistics, ColorFeatureError> {
    recipe.validate()?;
    let (target, matte, clip) = (
        recipe.interpretation(),
        recipe.background.matte(),
        recipe.format.maps_hdr_range(),
    );
    match recipe.format {
        ExportFormat::Exr => renderer.write_exr(output),
        ExportFormat::Png => renderer.write_png(output, &target, recipe.encoding, matte),
        ExportFormat::Tiff => renderer.write_tiff(output, &target, recipe.encoding, matte),
        ExportFormat::Jpeg => renderer.write_jpeg(
            output,
            &target,
            recipe.encoding,
            matte.ok_or(ColorFeatureError::ExportCombination)?,
            recipe.jpeg_quality,
        ),
        ExportFormat::Webp => renderer.write_webp(output, &target, recipe.encoding, matte),
        ExportFormat::PngHdr | ExportFormat::PngHdrMapped => renderer.write_hdr_png(output, clip),
        ExportFormat::JpegHdr | ExportFormat::JpegHdrMapped | ExportFormat::AvifHdr | ExportFormat::AvifHdrMapped => {
            let format = recipe.format.gainmap().ok_or(ColorFeatureError::ExportCombination)?;
            renderer.write_gainmap(output, format, recipe.jpeg_quality, matte, clip)
        }
    }.map_err(ColorFeatureError::from)
}

pub struct RecipePreview {
    pub after: SnapshotPreview,
    /// The decoded SDR base of a gain-map delivery.
    pub sdr_base: Option<SnapshotPreview>,
    /// Whether an OpenEXR delivery keeps any transparency.
    pub transparent: Option<bool>,
    pub clipped: u64,
    /// Unmapped HDR delivery refuses to clip out-of-range colors.
    pub range_blocked: bool,
}

/// Simulates the delivery at its output extent, reduced to `size`.
pub fn preview_recipe(
    renderer: &mut SnapshotRenderer,
    size: [u32; 2],
    space: RgbSpace,
    headroom: f32,
    recipe: &ExportRecipe,
) -> Result<RecipePreview, ColorFeatureError> {
    recipe.validate()?;
    renderer.set_output_extent(recipe.output_extent(renderer.extent())?)?;
    let matte = recipe.background.matte();
    let (after, sdr_base, transparent, statistics) = if let Some(format) = recipe.format.gainmap() {
        let (hdr, base, statistics) = renderer.preview_gainmap_output(
            size,
            space,
            headroom,
            format,
            recipe.jpeg_quality,
            matte,
        )?;
        (hdr, Some(base), None, statistics)
    } else if recipe.format == ExportFormat::Exr {
        let (preview, transparent) =
            renderer.preview_document_with_coverage(size, space, headroom)?;
        (preview, None, Some(transparent), Default::default())
    } else if recipe.format.is_hdr() {
        let (preview, statistics) = renderer.preview_hdr_output(size, space, headroom)?;
        (preview, None, None, statistics)
    } else {
        let (preview, statistics) = renderer.preview_output(
            size,
            space,
            &recipe.interpretation(),
            recipe.encoding,
            matte,
        )?;
        (preview, None, None, statistics)
    };
    let clipped = statistics.clipped_channels;
    Ok(RecipePreview {
        after,
        sdr_base,
        transparent,
        clipped,
        range_blocked: clipped > 0 && recipe.format.is_hdr() && !recipe.format.maps_hdr_range(),
    })
}

pub struct ExportTask {
    original: DocumentExport,
    gpu: SnapshotGpu,
    renderer: Option<SnapshotRenderer>,
    recipe: ExportRecipe,
    previews: Vec<Preview>,
    clipped: u64,
    range_blocked: bool,
    space: RgbSpace,
    name: String,
}

impl ExportTask {
    pub fn capture(
        session: &UiSession<Renderer>,
        request: u32,
        name: &str,
        space: RgbSpace,
    ) -> Result<Self, String> {
        Ok(Self {
            original: session.capture_project_export(request)?,
            gpu: crate::tasks::gpu(session)?.snapshot_gpu(),
            renderer: None,
            recipe: ExportRecipe::web_share(),
            previews: Vec::new(),
            clipped: 0,
            range_blocked: false,
            space,
            name: name.to_owned(),
        })
    }

    pub fn document(&self) -> &layer_core::Document {
        &self.original.project.document
    }

    pub fn recipe(&self) -> &ExportRecipe {
        &self.recipe
    }

    pub fn previews(&self) -> &[Preview] {
        &self.previews
    }

    pub fn configure(&mut self, recipe: ExportRecipe) -> Result<(), ColorFeatureError> {
        recipe.validate_for_document(self.document())?;
        self.recipe = recipe;
        self.previews.clear();
        self.clipped = 0;
        self.range_blocked = false;
        Ok(())
    }

    fn renderer(&mut self, control: CaptureControl) -> Result<&mut SnapshotRenderer, String> {
        let renderer = match self.renderer.take() {
            Some(renderer) => renderer,
            None => self
                .gpu
                .capture(
                    self.original.project.clone(),
                    self.original.background,
                    self.original.time,
                    control,
                )
                .map_err(|e| e.to_string())?,
        };
        Ok(self.renderer.insert(renderer))
    }

    /// Previews are the artwork, the delivery, and a gain-map's SDR base.
    pub fn compare(&mut self, control: CaptureControl) -> Result<(), ColorFeatureError> {
        let (space, recipe) = (self.space, self.recipe.clone());
        let renderer = self.renderer(control)?;
        let before = renderer.preview_document([512, 384], space)?;
        let output = preview_recipe(renderer, [512, 384], space, 1., &recipe)?;
        self.previews = [before, output.after]
            .into_iter()
            .chain(output.sdr_base)
            .map(|p| {
                Ok(Preview {
                    extent: p.extent,
                    pixels: p.encoded_bytes(space)?,
                })
            })
            .collect::<Result<_, String>>()?;
        self.clipped = output.clipped;
        self.range_blocked = output.range_blocked;
        Ok(())
    }

    #[cfg(test)]
    fn details(&self) -> Result<Value, String> { self.details_localized(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)) }

    pub fn copy_localized(&self, localizer: &layer_ui::Localizer, captions: &[layer_ui::ExportProfileCaption]) -> Value {
        let name = std::path::Path::new(&self.name).file_stem();
        json!({
            "copy": layer_ui::color_feature_copy::ExportCopy::new(localizer),
            "profile_names": captions.iter().map(|caption| caption.message(localizer)).collect::<Vec<_>>(),
            "recipe_profile_name": self.recipe.profile.display_name(localizer),
            "suggested_name": name.and_then(|s| s.to_str()).unwrap_or(localizer.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_EXPORT).as_ref()),
            "format_name": self.recipe.format.localized_name(localizer),
        })
    }

    pub fn details_localized(&self, localizer: &layer_ui::Localizer) -> Result<Value, String> {
        let document = self.document();
        let extent = [document.width, document.height];
        let name = std::path::Path::new(&self.name).file_stem();
        let mut form = json!(layer_ui::ExportForm::new_localized(document, localizer));
        form["recipe_profile_caption"] = json!(layer_ui::ExportProfileCaption::Profile { name:self.recipe.profile.name.clone() });
        form["recipe_profile_name"] = self.recipe.profile.display_name(localizer).into();
        Ok(json!({
            "color": document.color,
            "extent": extent,
            "resolution": document.resolution,
            "recipe": self.recipe,
            "form": form,
            "suggested_name": name.and_then(|s| s.to_str()).unwrap_or(localizer.text(layer_ui::MessageId::COLOR_FEATURES_EXPORT_EXPORT).as_ref()),
            "extension": self.recipe.format.extension(),
            "format_name": self.recipe.format.localized_name(localizer),
            "output_extent": self.recipe.size.extent(extent).map_err(|reason|reason.message(localizer))?,
            "clipped_channels": self.clipped,
            "has_sdr_preview": self.previews.len() == 3,
            "range_blocked": self.range_blocked,
            "sampled_time": document.has_animated_effects().then_some(self.original.time),
        }))
    }

    pub fn write(
        &mut self,
        stream: impl Write + Seek,
        control: CaptureControl,
    ) -> Result<(), ColorFeatureError> {
        if self.range_blocked {
            return Err(ColorFeatureError::HdrRangeBlocked);
        }
        let recipe = self.recipe.clone();
        let document = self.document();
        let extent = recipe.output_extent([document.width, document.height])?;
        let metadata = recipe.delivery_metadata(document)?;
        let renderer = self.renderer(control)?;
        renderer.set_output_extent(extent)?;
        renderer.set_output_metadata(metadata)?;
        let mut output = BufWriter::new(stream);
        let statistics = write_recipe(renderer, &mut output, &recipe)?;
        output.flush().map_err(|e| e.to_string())?;
        self.clipped = statistics.clipped_channels;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeHost;
    use layer_core::color::{
        ColorProfile, SampleDepth,
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
    };
    use layer_render_wgpu::WgpuRasterizer;
    use layer_ui::{CommandId, ExportDraftAction, UiAction};
    use std::io::Cursor;

    fn export(pixel: [f32; 4]) -> (NativeHost, ExportTask) {
        let mut document = layer_core::Document::new("Export", 8, 6, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.color.depth = SampleDepth::F32;
        let gpu = WgpuRasterizer::new_native_headless(document.color).unwrap();
        let mut host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        host.session = UiSession::new(
            Renderer(Some(gpu.into())),
            document,
            [8, 6],
            layer_ui::Platform::Mac,
        )
        .unwrap();
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F32,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        };
        let mut source = SourceBuilder::new([8, 6], interpretation, 1 << 20).unwrap();
        let row: Vec<u8> = pixel
            .repeat(8)
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        for _ in 0..6 {
            source.push_row(&row).unwrap();
        }
        host.session
            .import_layer_source("Photo", source.finish().unwrap())
            .unwrap();
        host.session.frame(0, 0).unwrap();
        host.dispatch(UiAction::Invoke {
            command: CommandId::ExportDocument,
        })
        .unwrap();
        let request = host.session.state().requests.last().unwrap().id;
        let task = ExportTask::capture(&host.session, request, "", RgbSpace::Srgb).unwrap();
        (host, task)
    }

    fn written(task: &mut ExportTask) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        task.write(Cursor::new(&mut bytes), Default::default()).map_err(|reason|reason.diagnostic())?;
        Ok(bytes)
    }

    #[test]
    fn export_task_blocks_clipped_unmapped_hdr_and_writes_mapped() {
        let (host, mut task) = export([100., -0.5, 2., 1.]);
        let format = |format| {
            ExportRecipe::web_share()
                .draft_canonical(ExportDraftAction::Format(format))
                .recipe
        };
        task.configure(format(ExportFormat::PngHdr)).unwrap();
        assert!(written(&mut task).is_err());
        task.compare(Default::default()).unwrap();
        let details = task.details().unwrap();
        assert_eq!(details["range_blocked"], true);
        assert_eq!(details["has_sdr_preview"], false);
        assert!(written(&mut task).is_err());
        task.configure(format(ExportFormat::PngHdrMapped)).unwrap();
        task.compare(Default::default()).unwrap();
        assert_eq!(task.details().unwrap()["range_blocked"], false);
        let photo = layer_color::photo::read_photo(
            Cursor::new(written(&mut task).unwrap()),
            Default::default(),
        )
        .unwrap();
        assert_eq!(photo.extent, [8, 6]);
        assert_eq!(photo.interpretation.depth, SampleDepth::F16);
        drop((task, host));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }

    #[test]
    fn webp_export_writes_lossless_rgba_and_refuses_encoder_limits() {
        let mut document = layer_core::Document::new("Export", 8, 6, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.resolution = Some(layer_core::ImageResolution::ppi(240));
        let artist_and_gps = [
            b"II\x2a\0\x08\0\0\0\x02\0".as_slice(),
            b"\x3b\x01\x02\0\x04\0\0\0Ada\0",
            b"\x25\x88\x04\0\x01\0\0\0\x26\0\0\0\0\0\0\0",
            b"\x01\0\x01\0\x02\0\x02\0\0\0N\0\0\0\0\0\0\0",
        ]
        .concat();
        document.metadata.exif = Some(artist_and_gps.into());
        for paper in document.layers.iter_mut().filter(|l| l.kind == layer_core::LayerKind::Background) {
            paper.visible = false;
        }
        let gpu = WgpuRasterizer::new_native_headless(document.color).unwrap();
        let mut host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        host.session = UiSession::new(Renderer(Some(gpu.into())), document, [8, 6], layer_ui::Platform::Mac).unwrap();
        let pixel = |x: u32, y: u32| [(x * 30) as u8, (y * 40) as u8, 200, if x < 4 { 255 } else { 0 }];
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        };
        let mut source = SourceBuilder::new([8, 6], interpretation, 1 << 20).unwrap();
        for y in 0..6 {
            source.push_row(&(0..8).flat_map(|x| pixel(x, y)).collect::<Vec<_>>()).unwrap();
        }
        host.session.import_layer_source("Photo", source.finish().unwrap()).unwrap();
        host.session.frame(0, 0).unwrap();
        host.dispatch(UiAction::Invoke { command: CommandId::ExportDocument }).unwrap();
        let request = host.session.state().requests.last().unwrap().id;
        let mut task = ExportTask::capture(&host.session, request, "Photo.capy", RgbSpace::Srgb).unwrap();
        let webp = ExportRecipe::web_share().draft_canonical(ExportDraftAction::Format(ExportFormat::Webp)).recipe;
        assert_eq!(webp.filename("Photo.capy"), "Photo.webp");
        task.configure(webp.clone()).unwrap();
        task.compare(Default::default()).unwrap();
        assert_eq!(task.details().unwrap()["format_name"], "WebP · lossless");
        let bytes = written(&mut task).unwrap();
        assert_eq!(&bytes[8..12], b"WEBP");
        let photo = layer_color::photo::read_photo(Cursor::new(&bytes), Default::default()).unwrap();
        assert_eq!(photo.extent, [8, 6]);
        assert_eq!(photo.interpretation.channels, SourceChannels::Rgba);
        assert_eq!(photo.interpretation.depth, SampleDepth::U8);
        assert_eq!(photo.resolution, Some(layer_core::ImageResolution::ppi(240)));
        let kept = layer_color::photo::read_photo_detailed(Cursor::new(&bytes), Default::default()).unwrap().metadata;
        let exif = kept.exif.expect("the artist is kept");
        assert!(exif.windows(4).any(|w| w == b"Ada\0"));
        assert!(!exif.windows(2).any(|w| w == b"N\0"), "location is removed by default");
        let none = ExportRecipe { metadata: layer_ui::ExportMetadata { keep: layer_ui::MetadataKeep::None, remove_location: true }, ..webp.clone() };
        task.configure(none).unwrap();
        let stripped = written(&mut task).unwrap();
        assert!(layer_color::photo::read_photo_detailed(Cursor::new(stripped), Default::default()).unwrap().metadata.is_empty());
        task.configure(webp.clone()).unwrap();
        let mut rows = photo.rows();
        let mut row = vec![0; photo.row_bytes()];
        for y in 0..6 {
            rows.read(y, &mut row).unwrap();
            for (x, actual) in row.chunks_exact(4).enumerate() {
                let expected = pixel(x as u32, y);
                assert_eq!(actual[3], expected[3], "({x},{y}) alpha");
                if expected[3] > 0 {
                    assert!(actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1), "({x},{y}): {actual:?}");
                }
            }
        }
        let mut oversized = webp;
        oversized.size = layer_ui::ExportSize::Fit { bounds: [20000, 20000], enlarge: true };
        let error = task.configure(oversized.clone()).unwrap_err();
        assert_eq!(error, ColorFeatureError::WebpLimit);
        oversized.format = ExportFormat::Png;
        task.configure(oversized).unwrap();
        drop((task, host));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }

    #[test]
    fn gainmap_previews_keep_before_after_and_base() {
        let (host, mut task) = export([4., 2., 1., 1.]);
        let recipe = ExportRecipe::web_share()
            .draft_for_color_canonical(
                task.document().color,
                ExportDraftAction::Format(ExportFormat::AvifHdr),
            )
            .recipe;
        task.configure(recipe).unwrap();
        task.compare(Default::default()).unwrap();
        assert_eq!(task.previews().len(), 3);
        assert!(
            task.previews()
                .iter()
                .all(|p| p.pixels.len() == (p.extent[0] * p.extent[1] * 4) as usize)
        );
        let details = task.details().unwrap();
        assert_eq!(details["has_sdr_preview"], true);
        assert_eq!(details["range_blocked"], false);
        let photo = layer_color::photo::read_photo(
            Cursor::new(written(&mut task).unwrap()),
            Default::default(),
        )
        .unwrap();
        assert!(photo.interpretation.depth.is_float());
        drop((task, host));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
