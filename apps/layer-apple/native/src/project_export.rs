//! Profiled delivery copies use immutable native snapshots on the file worker.
use super::*;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu, SnapshotRenderer};
use layer_ui::{DocumentExport, ExportFormat, ExportRecipe};

pub(super) struct Task {
    original: DocumentExport,
    gpu: SnapshotGpu,
    renderer: Option<SnapshotRenderer>,
    recipe: ExportRecipe,
    pub(super) previews: Vec<color::Preview>,
    clipped: u64,
}
impl Task {
    pub(super) fn capture(session: &UiSession<Renderer>, request: u32) -> Result<Self, String> {
        Ok(Self {
            original: session.capture_project_export(request)?,
            gpu: session.engine().backend().0.as_ref().ok_or("Canvas unavailable")?.snapshot_gpu(),
            renderer: None,
            recipe: ExportRecipe::web_share(),
            previews: Vec::new(),
            clipped: 0,
        })
    }
    fn configure(&mut self, recipe: ExportRecipe) -> Result<(), String> {
        let document = &self.original.project.document;
        recipe.validate_for_document(document)?;
        self.recipe = recipe;
        self.previews.clear();
        self.clipped = 0;
        Ok(())
    }
    fn renderer(&mut self, control: CaptureControl) -> Result<&mut SnapshotRenderer, String> {
        if self.renderer.is_none() {
            self.renderer = Some(self.gpu.capture(self.original.project.clone(), self.original.background,
                self.original.time, Default::default(), control).map_err(|e| e.to_string())?);
        }
        let document = &self.original.project.document;
        let renderer = self.renderer.as_mut().unwrap();
        renderer.set_output_extent(self.recipe.size.extent([document.width, document.height])?)?;
        renderer.set_output_resolution(self.recipe.output_resolution(document.resolution)?)?;
        Ok(renderer)
    }
    pub(super) fn compare(&mut self, control: CaptureControl) -> Result<(), String> {
        let recipe = self.recipe.clone();
        let renderer = self.renderer(control)?;
        let before = renderer.preview_document([512, 384], crate::DISPLAY_SPACE)?;
        let (after, base, statistics) = if let Some(format) = recipe.format.gainmap() {
            let (hdr, sdr, stats) = renderer.preview_gainmap_output([512,384], crate::DISPLAY_SPACE,
                1., format, recipe.jpeg_quality, recipe.background.matte())?;
            (hdr, Some(sdr), stats)
        } else if recipe.format == ExportFormat::Exr {
            (renderer.preview_document([512,384], crate::DISPLAY_SPACE)?, None, Default::default())
        } else if recipe.format.is_hdr() {
            let (image, stats) = renderer.preview_hdr_output([512,384],crate::DISPLAY_SPACE,1.)?;
            (image, None, stats)
        } else {
            let (image, stats) = renderer.preview_output([512, 384], crate::DISPLAY_SPACE,
                &recipe.interpretation(), recipe.encoding, recipe.background.matte())?;
            (image, None, stats)
        };
        self.previews = [before, after].into_iter().chain(base).map(|p| {
            Ok(color::Preview { extent: p.extent, pixels: p.encoded_bytes(crate::DISPLAY_SPACE)? })
        }).collect::<Result<_, String>>()?;
        self.clipped = statistics.clipped_channels;
        Ok(())
    }
    pub(super) fn details(&self) -> Result<serde_json::Value, String> {
        let document = &self.original.project.document;
        let extent = [document.width, document.height];
        Ok(serde_json::json!({"color":document.color,"extent":extent,
            "resolution":document.resolution,"recipe":self.recipe,"form":layer_ui::ExportForm::new(document),
            "output_extent":self.recipe.size.extent(extent)?,"clipped_channels":self.clipped,
            "has_sdr_preview":self.previews.len() == 3,
            "range_blocked":self.recipe.format.is_hdr() && self.recipe.format != ExportFormat::Exr && !self.recipe.format.maps_hdr_range() && self.clipped>0,
            "sampled_time":document.has_animated_effects().then_some(self.original.time)}))
    }
    pub(super) fn write(&mut self, stream: impl Write + Seek, control: CaptureControl) -> Result<(), String> {
        let recipe = self.recipe.clone();
        recipe.validate()?;
        let renderer = self.renderer(control)?;
        let mut output = std::io::BufWriter::new(stream);
        let target = recipe.interpretation();
        let statistics = match recipe.format {
            ExportFormat::Exr => renderer.write_exr(&mut output),
            ExportFormat::PngHdr | ExportFormat::PngHdrMapped => renderer.write_hdr_png(&mut output, recipe.format.maps_hdr_range()),
            ExportFormat::JpegHdr | ExportFormat::JpegHdrMapped | ExportFormat::AvifHdr | ExportFormat::AvifHdrMapped => renderer.write_gainmap(&mut output, recipe.format.gainmap().unwrap(),recipe.jpeg_quality,recipe.background.matte(),recipe.format.maps_hdr_range()),
            ExportFormat::Png => renderer.write_png(&mut output, &target, recipe.encoding, recipe.background.matte()),
            ExportFormat::Tiff => renderer.write_tiff(&mut output, &target, recipe.encoding, recipe.background.matte()),
            ExportFormat::Jpeg => renderer.write_jpeg(&mut output, &target, recipe.encoding,
                recipe.background.matte().ok_or("Choose a JPEG background")?, recipe.jpeg_quality),
        }?;
        output.flush().map_err(|e| e.to_string())?;
        self.clipped = statistics.clipped_channels;
        Ok(())
    }
}

/// # Safety
/// Worker only, before comparison or writing. The output recipe never edits the master.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_export_options(task: *const CapyProjectTask, recipe: *const c_char) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    let recipe = unsafe { read_title(recipe) }.map(str::to_owned);
    task.perform(|payload| match payload {
        Payload::Export(export) => export.configure(serde_json::from_str(&recipe?).map_err(|e| e.to_string())?),
        _ => Err("Not an export task".into()),
    })
}
