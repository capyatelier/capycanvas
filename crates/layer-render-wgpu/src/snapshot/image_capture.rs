//! Captures that become one immutable document-depth image: Convert to Image
//! Layer trims a paint layer's raw appearance to its visible pixels, and bakes
//! that read image objects finish their canonical sampling here.
use super::*;

impl SnapshotGpu {
    pub async fn image_capture(&self, capture: layer_core::ImageCapture, control: CaptureControl)
        -> Result<Option<(Arc<layer_core::color::source::SourceImage>, [i64; 2])>, String> {
        let [x, y, width, height] = capture.window;
        let mut window = [x, y, x.saturating_add(width).min(capture.extent[0]), y.saturating_add(height).min(capture.extent[1])];
        if let Some(target) = capture.trim {
            let bounds = self.content_bounds(layer_core::ContentBoundsRequest {
                snapshot: capture.scene.clone(), scope: layer_core::ContentScope::Target(target), selection: None,
            }, control.clone()).await?;
            if bounds.is_empty() { return Ok(None); }
            let [min, max] = [[bounds.min.x, bounds.min.y], [bounds.max.x, bounds.max.y]];
            window = [window[0].max(min[0].floor().max(0.) as u32), window[1].max(min[1].floor().max(0.) as u32),
                window[2].min(max[0].ceil().max(0.) as u32), window[3].min(max[1].ceil().max(0.) as u32)];
        }
        if window[0] >= window[2] || window[1] >= window[3] { return Ok(None); }
        let region = [window[0], window[1], window[2] - window[0], window[3] - window[1]];
        let mut snapshot = SnapshotRenderer::construct(capture.scene.clone(), capture.scope.clone(), control.clone(), self)
            .map_err(|e| e.to_string())?;
        snapshot.extent = capture.extent;
        snapshot.offset = capture.offset;
        snapshot.renderer.ensure_document_metadata(capture.extent, snapshot.scene.view().with_scope(&snapshot.scope).with_offset(snapshot.offset))
            .map_err(|e| e.to_string())?;
        let mut rows = layer_color::SourceRows::new(layer_color::ClipRows {
            extent: [region[2], region[3]], origin: [region[0], region[1]], color: snapshot.color(),
            resolution: capture.scene.view().composition().resolution, rendition: None, source: true,
            limit: layer_color::photo::PhotoMemoryBudget::current().encode_bytes,
        })?;
        let mut row = region[1];
        while row < window[3] {
            control.check().map_err(|e| e.to_string())?;
            let (count, mut pixels) = snapshot.read_window_band_async(region, row).await.map_err(|e| e.to_string())?;
            if let Some(selection) = &capture.selection {
                let coverage = snapshot.selection_coverage_async(selection, [region[0], row, region[2], count]).await.map_err(|e| e.to_string())?;
                for (line, pixels) in pixels.chunks_exact_mut(region[2] as usize).enumerate() {
                    coverage.apply(region[0], row + line as u32, pixels);
                }
            }
            rows.push(&mut pixels)?;
            row += count;
        }
        control.check().map_err(|e| e.to_string())?;
        Ok(Some((Arc::new(rows.finish()?), [i64::from(region[0]), i64::from(region[1])])))
    }
}
