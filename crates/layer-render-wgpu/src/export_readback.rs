//! Blocking whole-document sRGB readback for tests and benchmarks.
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

    pub fn readback_srgb_rgba8(&mut self) -> Result<Vec<u8>, GpuRasterError> {
        self.pipelines.export.compile();
        let [width, height] = self.document_extent;
        if width == 0 || height == 0 {
            return Err(GpuRasterError::InvalidExtent);
        }
        let row_bytes = width.checked_mul(4).ok_or(GpuRasterError::SizeOverflow)?;
        let padded_row_bytes = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let size = padded_row_bytes as u64 * height as u64;
        if size > self.device.limits().max_buffer_size {
            return Err(GpuRasterError::SizeOverflow);
        }
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer explicit readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let export_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer explicit sRGB export"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: EXPORT_FORMAT,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("layer explicit readback encoder"),
            },
        );
        self.encode_artwork_readback(&export_texture, &mut encoder)?;
        encoder.copy_texture_to_buffer(
            export_texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.uploads.finish(&encoder);
        encoder.submit(&self.queue);
        let (sender, receiver) = mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result.map_err(|error| error.to_string()));
            });
        raster::wait_mapping(&self.device, &receiver).map_err(GpuRasterError::MapFailed)?;
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
        let bytes = mapped
            .chunks_exact(padded_row_bytes as usize)
            .flat_map(|row| &row[..row_bytes as usize])
            .copied()
            .collect();
        drop(mapped);
        buffer.unmap();
        Ok(bytes)
    }
}
