//! An explicit export owns its staging buffer, never the live renderer. Waiting
//! and packing rows belong to the file worker, after GPU submission by the owner.
use super::*;

impl WgpuRasterizer {
    /// The explicit API still owns its requested complete RGBA8 result/staging,
    /// but never requires a document-sized working composite. Interactive file
    /// output uses the streaming SnapshotRenderer instead of this whole-image API.
    pub(super) fn encode_artwork_readback(
        &mut self,
        destination: &wgpu::Texture,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let frame = self
            .artwork_frame
            .clone()
            .ok_or(GpuRasterError::InvalidExtent)?;
        let mut capture = artwork::Capture::default();
        let (tile, view) = create_target(
            &self.device,
            [PAGE_SIZE; 2],
            EXPORT_FORMAT,
            "bounded explicit readback conversion",
        );
        for (index, coordinate) in
            page_coordinates(PixelRect::full(self.document_extent)).enumerate()
        {
            let region = page_rect(coordinate).intersect(PixelRect::full(self.document_extent));
            let source = capture.region(
                self,
                frame.packet(self.document_extent),
                region,
                [PAGE_SIZE; 2],
                encoder,
            )?;
            let binding = create_texture_bind_group(
                &self.device,
                &self.texture_layout,
                &source.view,
                &self.sampler,
                "exact readback tile",
            );
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("exact readback color conversion"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
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
                pass.set_bind_group(0, &binding, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_texture(
                tile.as_image_copy(),
                wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d {
                        x: region.min_x(),
                        y: region.min_y(),
                        z: 0,
                    },
                    ..destination.as_image_copy()
                },
                wgpu::Extent3d {
                    width: region.width(),
                    height: region.height(),
                    depth_or_array_layers: 1,
                },
            );
            if index % 16 == 15 {
                scene::Scene::submit_chunk(self, encoder, "explicit artwork readback chunk")?;
            }
        }
        Ok(())
    }
}

pub struct ExportReadback {
    device: wgpu::Device,
    buffer: wgpu::Buffer,
    submission: wgpu::SubmissionIndex,
    receiver: mpsc::Receiver<Result<(), String>>,
    request_id: u64,
    extent: [u32; 2],
    padded_row_bytes: u32,
}
impl ExportReadback {
    pub(super) fn new(
        device: wgpu::Device,
        buffer: wgpu::Buffer,
        submission: wgpu::SubmissionIndex,
        receiver: mpsc::Receiver<Result<(), String>>,
        request_id: u64,
        extent: [u32; 2],
        padded_row_bytes: u32,
    ) -> Self {
        Self {
            device,
            buffer,
            submission,
            receiver,
            request_id,
            extent,
            padded_row_bytes,
        }
    }
    /// Worker only. The document can change or close after the ticket is issued;
    /// the copied texture and its queue position preserve the captured pixels.
    pub fn finish(self) -> Result<ReadbackImage, GpuRasterError> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(self.submission.clone()),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|e| GpuRasterError::WaitFailed(e.to_string()))?;
        self.receiver
            .recv_timeout(READBACK_TIMEOUT)
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .map_err(GpuRasterError::MapFailed)?;
        self.image()
    }
    /// Nonblocking completion for event-loop hosts. Yield before polling again
    /// so the browser can deliver WebGPU's map callback.
    pub fn try_finish(&mut self) -> Result<Option<ReadbackImage>, GpuRasterError> {
        self.device
            .poll(wgpu::PollType::Poll)
            .map_err(|e| GpuRasterError::WaitFailed(e.to_string()))?;
        match self.receiver.try_recv() {
            Ok(result) => {
                result.map_err(GpuRasterError::MapFailed)?;
                self.image().map(Some)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(e) => Err(GpuRasterError::MapFailed(e.to_string())),
        }
    }
    fn image(&self) -> Result<ReadbackImage, GpuRasterError> {
        let mapped = self
            .buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
        let [width, height] = self.extent;
        let stride = width * 4;
        let mut bytes = vec![0; stride as usize * height as usize];
        for (source, destination) in mapped
            .chunks_exact(self.padded_row_bytes as usize)
            .zip(bytes.chunks_exact_mut(stride as usize))
        {
            destination.copy_from_slice(&source[..stride as usize]);
        }
        drop(mapped);
        self.buffer.unmap();
        Ok(ReadbackImage {
            request_id: self.request_id,
            width,
            height,
            stride,
            bytes,
        })
    }
}
