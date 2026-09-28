//! Clipboard copies of a document rectangle on the capture worker.
use super::*;

impl SnapshotRenderer {
    /// Copy `crop` in one pass: its composite rows, multiplied band by band by
    /// `coverage`, become a document-depth source (unless `source` is false)
    /// and an sRGB PNG.
    pub fn write_clip(
        &mut self,
        crop: [u32; 4],
        coverage: Option<&Arc<layer_core::Selection>>,
        source: bool,
        limit: usize,
    ) -> Result<(Option<layer_core::color::source::SourceImage>, Vec<u8>), String> {
        self.check_cancelled().map_err(|e| e.to_string())?;
        self.control.output_rows.store(0, Ordering::Relaxed);
        let guide = if self.sdr_rendition.is_some() { Some(self.local_tone_guide()?) } else { None };
        let clip = layer_color::ClipRows {
            extent: [crop[2], crop[3]],
            origin: [crop[0], crop[1]],
            color: self.color(),
            resolution: self.output_metadata.resolution,
            rendition: self.sdr_rendition.zip(guide.as_deref()),
            source,
            limit,
        };
        let width = crop[2] as usize;
        let control = self.control.clone();
        let (mut band, mut first, mut end) = (Vec::new(), 0, 0);
        layer_color::write_clip_rows(clip, |y, row| {
            control.check().map_err(|e| e.to_string())?;
            let document_y = crop[1] + y;
            if document_y >= end {
                band = Vec::new();
                let (rows, mut pixels) = self.read_window_band(crop, document_y).map_err(|e| e.to_string())?;
                if let Some(selection) = coverage {
                    let coverage = self
                        .selection_coverage(selection, [crop[0], document_y, crop[2], rows])
                        .map_err(|e| e.to_string())?;
                    for (line, pixels) in pixels.chunks_exact_mut(width).enumerate() {
                        coverage.apply(crop[0], document_y + line as u32, pixels);
                    }
                }
                (band, first, end) = (pixels, document_y, document_y + rows);
            }
            let start = (document_y - first) as usize * width;
            row.copy_from_slice(&band[start..start + width]);
            control.output_rows.store(y + 1, Ordering::Relaxed);
            Ok(())
        })
    }
}
