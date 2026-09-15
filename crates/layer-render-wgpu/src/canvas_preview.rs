//! Navigator samples derived display pixels, never rebuilds a scene. Native
//! Float32 tiles reduce into a bounded coarse image before preview sampling.
use super::*;
use layer_render::CanvasPreview;
use thumbnails::UiImageTarget;

pub(super) struct CanvasOverview {
    target: Option<UiImageTarget>,
    mips: Option<display_mips::Image>,
    pipeline: Option<wgpu::RenderPipeline>,
    tx: mpsc::Sender<Result<CanvasPreview, GpuRasterError>>,
    rx: mpsc::Receiver<Result<CanvasPreview, GpuRasterError>>,
    pending: bool,
}
impl CanvasOverview {
    pub fn storage_bytes(&self) -> u64 {
        self.target.as_ref().map_or(0, UiImageTarget::storage_bytes)
            + self
                .mips
                .as_ref()
                .map_or(0, display_mips::Image::storage_bytes)
    }
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            target: None,
            mips: None,
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
        let Some(mut source) = self
            .live_display
            .as_ref()
            .map(|cache| cache.coarse.binding.clone())
            .or_else(|| self.composite_bind_group.clone())
        else {
            return Ok(false);
        };
        if known_revision == Some(revision) {
            let _ = self.canvas_preview.tx.send(Ok(CanvasPreview {
                revision,
                image: None,
            }));
        } else {
            let [width, height] = self.document_extent;
            let native = self.device.working_format() == wgpu::TextureFormat::Rgba32Float;
            if self.live_display.is_some() {
                self.canvas_preview.mips = None;
            }
            if native {
                let pipelines = self
                    .display_pipelines
                    .get_or_insert_with(|| display_mips::Pipelines::new(&self.device));
                if let Some(startup) = &self.startup {
                    startup.compiler.check()?;
                    startup.compiler.pipeline(&pipelines.reduce, startup::OTHER);
                    startup.compiler.start();
                    if !pipelines.reduce.ready() {
                        return Ok(false);
                    }
                }
            }
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
            let pipeline = self
                .canvas_preview
                .pipeline
                .get_or_insert_with(|| {
                    let shader = self
                        .device
                        .create_shader_module(wgpu::ShaderModuleDescriptor {
                            label: Some("Navigator downsample"),
                            source: wgpu::ShaderSource::Wgsl(
                                format!(
                                    "{}\n{}\n{}",
                                    view_color::shader(
                                        self.device.working_space(),
                                        layer_core::color::RgbSpace::Srgb
                                    ),
                                    include_str!("overview_sample.wgsl"),
                                    if native {
                                        include_str!("canvas_preview_native.wgsl")
                                    } else {
                                        include_str!("canvas_preview.wgsl")
                                    }
                                )
                                .into(),
                            ),
                        });
                    let layout =
                        self.device
                            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                                label: Some("Navigator pipeline layout"),
                                bind_group_layouts: &[Some(if native {
                                    &self.display_pipelines.as_ref().unwrap().image_layout
                                } else {
                                    &self.texture_layout
                                })],
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
                })
                .clone();
            let mut encoder = crate::submission::CommandEncoder::new(
                &self.device,
                &wgpu::CommandEncoderDescriptor {
                    label: Some("Navigator preview"),
                },
            );
            if native && self.live_display.is_none() {
                let plan = display_mips::Plan::new(self.document_extent)?;
                let mut image = self
                    .canvas_preview
                    .mips
                    .take()
                    .filter(|image| image.plan == plan)
                    .unwrap_or_else(|| {
                        display_mips::Image::new(
                            self,
                            self.display_pipelines.as_ref().unwrap(),
                            plan,
                        )
                    });
                let result = (|| {
                    for coordinate in page_coordinates(PixelRect::full(self.document_extent)) {
                        image.write_tile(
                            &self.device,
                            self.display_pipelines.as_ref().unwrap(),
                            &mut encoder,
                            self.composite_texture.as_ref().unwrap(),
                            coordinate.map(|v| v * PAGE_SIZE),
                            coordinate,
                        )?;
                    }
                    Ok::<_, GpuRasterError>(())
                })();
                source = image.binding.clone();
                self.canvas_preview.mips = Some(image);
                result?;
            }
            let target = self.canvas_preview.target.as_ref().unwrap();
            target.encode(&mut encoder, &pipeline, &source);
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
