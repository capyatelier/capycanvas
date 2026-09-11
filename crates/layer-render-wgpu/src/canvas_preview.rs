//! Navigator samples the existing document composition, never rebuilds a scene.
//! One persistent small GPU target/map buffer; unchanged cameras cost no GPU work.
use super::*;
use layer_render::CanvasPreview;
use thumbnails::UiImageTarget;

pub(super) struct CanvasOverview {
    target: Option<UiImageTarget>,
    pipeline: Option<wgpu::RenderPipeline>,
    tx: mpsc::Sender<Result<CanvasPreview, GpuRasterError>>,
    rx: mpsc::Receiver<Result<CanvasPreview, GpuRasterError>>,
    pending: bool,
}
impl CanvasOverview {
    pub fn storage_bytes(&self) -> u64 {
        self.target.as_ref().map_or(0, UiImageTarget::storage_bytes)
    }
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            target: None,
            pipeline: None,
            tx,
            rx,
            pending: false,
        }
    }
    pub fn take(&mut self) -> Option<Result<CanvasPreview, GpuRasterError>> {
        let image = self.rx.try_recv().ok()?;
        self.pending = false;
        Some(image)
    }
}
impl WgpuRasterizer {
    /// Changes only when document composition changes, never for camera motion.
    pub fn canvas_preview_revision(&self) -> u64 {
        self.composite_revision
    }
    pub fn canvas_preview_pending(&self) -> bool {
        self.canvas_preview.pending
    }

    pub(super) fn start_canvas_preview(
        &mut self,
        known_revision: Option<u64>,
    ) -> Result<bool, GpuRasterError> {
        if self.canvas_preview.pending {
            return Ok(false);
        }
        let revision = self.composite_revision;
        let Some(source) = &self.composite_bind_group else {
            return Ok(false);
        };
        if known_revision == Some(revision) {
            let _ = self.canvas_preview.tx.send(Ok(CanvasPreview {
                revision,
                image: None,
            }));
        } else {
            let [width, height] = self.document_extent;
            let scale = 256.0 / width.max(height).max(1) as f32;
            let size = [width, height].map(|n| (n as f32 * scale).round().max(1.0) as u32);
            if self
                .canvas_preview
                .target
                .as_ref()
                .is_none_or(|t| t.size() != size)
            {
                self.canvas_preview.target = Some(UiImageTarget::new(&self.device, size));
            }
            let pipeline = self.canvas_preview.pipeline.get_or_insert_with(|| {
                let shader = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("Navigator downsample"),
                        source: wgpu::ShaderSource::Wgsl(
                            concat!(
                                include_str!("overview_sample.wgsl"),
                                "\n",
                                include_str!("canvas_preview.wgsl")
                            )
                            .into(),
                        ),
                    });
                let layout = self
                    .device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("Navigator pipeline layout"),
                        bind_group_layouts: &[Some(&self.texture_layout)],
                        immediate_size: 0,
                    });
                fullscreen_pipeline(
                    &self.device,
                    &layout,
                    &shader,
                    "fragment_main",
                    None,
                    EXPORT_FORMAT,
                    "Navigator downsample",
                )
            });
            let target = self.canvas_preview.target.as_ref().unwrap();
            let mut encoder = crate::submission::CommandEncoder::new(
                &self.device,
                &wgpu::CommandEncoderDescriptor {
                    label: Some("Navigator preview"),
                },
            );
            target.encode(&mut encoder, pipeline, source);
            encoder.submit(&self.queue);
            let tx = self.canvas_preview.tx.clone();
            target.map(revision, move |image| {
                let _ = tx.send(image.map(|image| CanvasPreview {
                    revision,
                    image: Some(image),
                }));
            });
        }
        self.canvas_preview.pending = true;
        Ok(true)
    }
}
