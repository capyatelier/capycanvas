//! Asynchronous 32px previews. The GPU worker never waits for thumbnail maps.
use super::*;
pub(super) struct Thumbnails {
    tx: mpsc::Sender<Result<ReadbackImage, GpuRasterError>>,
    rx: mpsc::Receiver<Result<ReadbackImage, GpuRasterError>>,
    pending: usize,
}
impl Thumbnails {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, pending: 0 }
    }
    pub fn take(&mut self) -> Option<Result<ReadbackImage, GpuRasterError>> {
        let image = self.rx.try_recv().ok()?;
        self.pending = self.pending.saturating_sub(1);
        Some(image)
    }
}
impl WgpuRasterizer {
    pub fn thumbnails_pending(&self) -> bool {
        self.thumbnails.pending > 0
    }
    pub(super) fn start_thumbnail(
        &mut self,
        id: u64,
        target: LayerId,
    ) -> Result<(), GpuRasterError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("asynchronous layer preview"),
            });
        let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
        scene.begin_frame();
        let source = scene.thumbnail(self, target, &mut encoder)?;
        self.scene = Some(scene);
        let (texture, view) =
            create_target(&self.device, [32, 32], EXPORT_FORMAT, "thumbnail sRGB");
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("thumbnail color conversion"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines.export);
            pass.set_bind_group(0, &source.texture_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("32px thumbnail readback"),
            size: 256 * 32,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(32),
                },
            },
            wgpu::Extent3d {
                width: 32,
                height: 32,
                depth_or_array_layers: 1,
            },
        );
        self.uploads.finish(&encoder);
        self.queue.submit([encoder.finish()]);
        let ready = buffer.clone();
        let tx = self.thumbnails.tx.clone();
        self.thumbnails.pending += 1;
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let image = result
                    .map_err(|e| GpuRasterError::MapFailed(e.to_string()))
                    .and_then(|_| {
                        let data = ready
                            .slice(..)
                            .get_mapped_range()
                            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                        let mut bytes = Vec::with_capacity(32 * 32 * 4);
                        for row in data.chunks(256) {
                            bytes.extend_from_slice(&row[..128]);
                        }
                        drop(data);
                        ready.unmap();
                        Ok(ReadbackImage {
                            request_id: id,
                            width: 32,
                            height: 32,
                            stride: 128,
                            bytes,
                        })
                    });
                let _ = tx.send(image);
            });
        Ok(())
    }
}
