//! Exact artwork queries own bounded captures, independently of presentation.
//! The retained frame contains composition metadata and current preview styles;
//! pixels remain in paint/source backing, never in a second document image.
use super::*;
use layer_render::ViewState;

#[derive(Clone)]
pub(super) struct Frame {
    pub layers: Vec<Layer>,
    pub view: ViewState,
    pub background: [f32; 4],
    pub time: f32,
    pub previews: Vec<DabBatch>,
    pub preview_records: Vec<usize>,
}
impl Frame {
    pub fn new(packet: FramePacket<'_>, background: [f32; 4]) -> Self {
        Self {
            layers: packet
                .layers
                .iter()
                .map(Layer::composite_snapshot)
                .collect(),
            view: packet.view,
            background,
            time: packet.time_seconds,
            previews: packet
                .dab_batches
                .iter()
                .filter(|b| b.kind == DabBatchKind::Preview)
                .cloned()
                .collect(),
            preview_records: packet
                .dab_batches
                .iter()
                .enumerate()
                .filter_map(|(index, b)| (b.kind == DabBatchKind::Preview).then_some(index))
                .collect(),
        }
    }
    pub fn packet(&self, extent: [u32; 2]) -> FramePacket<'_> {
        FramePacket {
            layers: &self.layers,
            view: self.view,
            document_extent: extent,
            time_seconds: self.time,
            dabs: &[],
            dab_batches: &self.previews,
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
        }
    }
}

#[derive(Default)]
pub(super) struct Capture {
    scene: Option<scene::Scene>,
    target: Option<(wgpu::Texture, wgpu::TextureView)>,
    window: Option<PixelRect>,
    pub peak_image_bytes: u64,
    pub image_limit: Option<u64>,
}
impl Capture {
    pub fn storage_bytes(&self) -> u64 {
        self.target.as_ref().map_or(0, |(t, _)| texture_bytes(t))
            + self.scene.as_ref().map_or(0, scene::Scene::scratch_bytes)
    }
    /// The caller consumes this target before requesting another region. Edges
    /// occupy its top-left prefix; no samples outside `region` are meaningful.
    pub fn region(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        region: PixelRect,
        size: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<source_access::RawTile, GpuRasterError> {
        if size.contains(&0)
            || size[0] > PAGE_SIZE
            || size[1] > PAGE_SIZE
            || region.width() > size[0]
            || region.height() > size[1]
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let window = scene::Scene::capture_window(packet.layers, region, packet.document_extent);
        let bytes = scene::Scene::capture_image_bound(packet.layers, window);
        let limit = self.image_limit.unwrap_or(256 * 1024 * 1024);
        if bytes > limit {
            return Err(GpuRasterError::Color(format!(
                "Artwork query requires {bytes} bytes of filter images; its limit is {limit} bytes"
            )));
        }
        let resized = self
            .target
            .as_ref()
            .is_some_and(|(t, _)| [t.width(), t.height()] != size);
        // Retire previous image windows before replacing their storage. Exact
        // query scheduling/cancellation will move these drains off interaction.
        if resized || (self.window.is_some_and(|old| old != window) && self.peak_image_bytes > 0) {
            scene::Scene::submit_chunk(r, encoder, "artwork query window")?;
            if let Some(scene) = &mut self.scene {
                scene.release_capture_window(window);
            }
        }
        if self.target.is_none() || resized {
            self.target = Some(create_color_target(
                &r.device,
                size,
                "bounded artwork query",
            ));
        }
        r.complete_preview_pages(encoder);
        let scene = self.scene.get_or_insert_with(|| scene::Scene::new(r));
        let (texture, view) = self.target.as_ref().unwrap();
        scene.capture_region(r, packet, texture, region, None, encoder)?;
        // The lightweight frontmost preview has no paint page. Replay its
        // retained GPU contacts into this exact crop instead of sampling the
        // display. Native previews and destination brushes already have pages.
        if r.preview_direct_to_composite && !packet.dab_batches.is_empty() {
            use wgpu::util::DeviceExt;
            let frame = r
                .artwork_frame
                .clone()
                .ok_or(GpuRasterError::InvalidExtent)?;
            let target = TargetGpu::new(
                [region.min_x(), region.min_y()],
                size,
                packet.document_extent,
            );
            let uniform = r
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("exact preview crop geometry"),
                    contents: target_bytes(&target),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            for (batch, index) in frame.previews.iter().zip(&frame.preview_records) {
                if batch.dab_count == 0
                    || !packet
                        .layers
                        .iter()
                        .any(|l| l.id == batch.layer_id && l.visible)
                    || batch_pixel_rect(batch, packet.document_extent)
                        .intersect(region)
                        .is_empty()
                {
                    continue;
                }
                r.prepare_selection(encoder, &batch.style)?;
                let coverage = if batch.style.selection.is_some() {
                    r.selection_clip.buffer.as_ref().unwrap()
                } else {
                    &r.unclipped
                };
                let binding =
                    create_target_bind_group(&r.device, &r.target_layout, &uniform, coverage);
                r.encode_batch_to_target(
                    encoder,
                    *index,
                    batch,
                    view,
                    PixelRect::new(0, 0, region.width(), region.height()),
                    &binding,
                    0,
                )?;
            }
        }
        self.window = Some(window);
        self.peak_image_bytes = self.peak_image_bytes.max(bytes);
        Ok(source_access::RawTile {
            texture: texture.clone(),
            view: view.clone(),
        })
    }
}

impl WgpuRasterizer {
    /// The fast single-batch destination preview writes its damage directly
    /// from persistent paint. The live compositor already clips that fork, but
    /// exact queries compose whole tiles. Complete their unchanged pixels once
    /// when queried, without adding a copy to ordinary prediction frames.
    fn complete_preview_pages(&mut self, encoder: &mut crate::submission::CommandEncoder) {
        if self.preview_full_pages || !self.preview_requires_base
            || self.preview_completion.as_ref().is_some_and(|v| v.load(std::sync::atomic::Ordering::Acquire)) {
            return;
        }
        if let Some(layer) = self.paint_layers.iter().find(|l| Some(l.id) == self.preview_layer_id) {
            for preview in &self.preview_pages {
                let Some(source) = layer.pages.iter().find(|p| p.coordinate == preview.coordinate) else {
                    // Direct prediction clears absent persistent pixels to zero.
                    continue;
                };
                let page = page_rect(preview.coordinate);
                for region in page.subtract(page.intersect(self.preview_damage)) {
                    if region.is_empty() { continue; }
                    let local = region.page_local(preview.coordinate);
                    let copy = |texture| wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: local.min_x(), y: local.min_y(), z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    };
                    encoder.copy_texture_to_texture(
                        copy(&source.active().texture), copy(&preview.active().texture),
                        wgpu::Extent3d { width: local.width(), height: local.height(), depth_or_array_layers: 1 },
                    );
                }
            }
        }
        // Failed/abandoned queries must not certify unsubmitted copies. Reuse
        // the same queue-order validity guard as the other renderer caches.
        let write = crate::submission::CacheWrite::new();
        self.preview_completion = Some(write.validity());
        write.track(encoder);
    }
}

#[cfg(test)]
mod tests;
