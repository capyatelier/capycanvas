//! Shared guide orchestration; hosts schedule idle work and own cancellation.
use super::*;

impl SnapshotRenderer {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn local_tone_guide(
        &mut self,
    ) -> Result<Arc<layer_core::color::hdr::LocalToneGuide>, String> {
        pollster::block_on(self.local_tone_guide_async())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn gpu_local_tone_guide(&mut self) -> Result<Arc<crate::local_tone::GpuToneGuide>, String> {
        pollster::block_on(self.gpu_local_tone_guide_async())
    }
    /// GPU analysis shared by previews and CPU delivery codecs. Only codecs
    /// download the bounded guide; full-resolution image analysis stays on GPU.
    pub async fn local_tone_guide_async(
        &mut self,
    ) -> Result<Arc<layer_core::color::hdr::LocalToneGuide>, String> {
        self.check_cancelled().map_err(|e| e.to_string())?;
        if let Some(guide) = &self.local_tone {
            return Ok(guide.clone());
        }
        let guide = Arc::new(
            self.gpu_local_tone_guide_async()
                .await?
                .download_async(&self.renderer.queue)
                .await?,
        );
        self.check_cancelled().map_err(|e| e.to_string())?;
        self.local_tone = Some(guide.clone());
        Ok(guide)
    }
    /// Construct a document-space guide on the capture device. Source regions
    /// are composed and reduced in one submission with no image readback. Wait
    /// between regions and range anchors so cancellation cannot leave a whole
    /// document's work queued ahead of a newly arriving pen stroke.
    pub async fn gpu_local_tone_guide_async(
        &mut self,
    ) -> Result<Arc<crate::local_tone::GpuToneGuide>, String> {
        use crate::local_tone::{Builder, range_async, wait_async};
        self.check_cancelled().map_err(|e| e.to_string())?;
        if let Some(guide) = &self.gpu_local_tone {
            return Ok(guide.clone());
        }
        let reserved = Builder::allocation_bound(self.extent)?;
        if reserved > self.limits.planned_pixel_bytes {
            return Err(GpuRasterError::CaptureBudget {
                required: reserved,
                limit: self.limits.planned_pixel_bytes,
            }
            .to_string());
        }
        Builder::prepare_pipelines(&self.renderer.device).await?;
        self.check_cancelled().map_err(|e| e.to_string())?;
        let builder = Builder::new(&self.renderer.device, self.extent, self.color().space)?;
        let device = self.renderer.device.clone();
        let queue = self.renderer.queue.clone();
        // Tile-aligned windows bound transient composite storage and queue work.
        // Existing scene capture supplies masks, blend modes and filter halos.
        for y in (0..self.extent[1]).step_by(512) {
            for x in (0..self.extent[0]).step_by(512) {
                let mut regions = vec![[
                    x,
                    y,
                    512.min(self.extent[0] - x),
                    512.min(self.extent[1] - y),
                ]];
                while let Some(region) = regions.pop() {
                    self.check_cancelled().map_err(|e| e.to_string())?;
                    match self.capture_region_gpu(region, reserved, |_, texture, encoder| {
                        builder.reduce(encoder, texture, [region[0], region[1]])
                    }) {
                        Err(GpuRasterError::CaptureBudget { .. })
                            if region[2].max(region[3]) > 16 =>
                        {
                            let axis = if region[2] > region[3] { 0 } else { 1 };
                            let mut first = region;
                            first[axis + 2] /= 2;
                            let mut second = region;
                            second[axis] += first[axis + 2];
                            second[axis + 2] -= first[axis + 2];
                            regions.push(second);
                            regions.push(first);
                        }
                        Err(e) => return Err(e.to_string()),
                        Ok(()) => wait_async(&device, &queue).await?,
                    }
                }
            }
        }
        self.check_cancelled().map_err(|e| e.to_string())?;
        let mut encoder = submission::CommandEncoder::new(&device, &Default::default());
        let statistics = builder.statistics(&mut encoder);
        builder.prepare(&mut encoder);
        encoder.submit(&queue);
        let (low, step, intervals, peak) = range_async(&device, &queue, &statistics).await?;
        for index in 0..=intervals {
            self.check_cancelled().map_err(|e| e.to_string())?;
            let mut encoder = submission::CommandEncoder::new(&device, &Default::default());
            builder.anchor(&mut encoder, low, step, intervals, index);
            encoder.submit(&queue);
            wait_async(&device, &queue).await?;
        }
        self.check_cancelled().map_err(|e| e.to_string())?;
        let mut encoder = submission::CommandEncoder::new(&device, &Default::default());
        let guide = builder.finish(&mut encoder, peak);
        encoder.submit(&queue);
        wait_async(&device, &queue).await?;
        self.check_cancelled().map_err(|e| e.to_string())?;
        self.gpu_local_tone = Some(guide.clone());
        Ok(guide)
    }
}
