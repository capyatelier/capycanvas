//! Profiled streaming output; resizing never changes native snapshot geometry.
use super::*;

impl SnapshotRenderer {
    pub fn set_output_resolution(
        &mut self,
        resolution: Option<layer_core::ImageResolution>,
    ) -> Result<(), String> {
        if let Some(resolution) = resolution {
            resolution.validate()?;
        }
        self.output_resolution = resolution;
        Ok(())
    }
    pub fn set_output_extent(&mut self, extent: [u32; 2]) -> Result<(), String> {
        if extent.into_iter().any(|v| v == 0 || v > 32768) {
            return Err("Image dimensions must be between 1 and 32768 pixels".into());
        }
        self.output_extent = extent;
        Ok(())
    }
    /// Profiled output writes a copy. Callers publish their temporary file only
    /// after success; cancellation, codec or capture failure may leave it partial.
    pub fn write_png(
        &mut self,
        output: impl std::io::Write,
        target: &SourceInterpretation,
        options: layer_core::color::OutputEncoding,
        matte: Option<[f32; 3]>,
    ) -> Result<layer_color::OutputStatistics, String> {
        let resolution = self.output_resolution;
        self.write_rows(target, options, matte, |extent, target, row| {
            layer_color::photo::write_png_rows(output, extent, target, resolution, row)
        })
    }
    pub fn write_tiff(
        &mut self,
        output: impl std::io::Write + std::io::Seek,
        target: &SourceInterpretation,
        options: layer_core::color::OutputEncoding,
        matte: Option<[f32; 3]>,
    ) -> Result<layer_color::OutputStatistics, String> {
        let resolution = self.output_resolution;
        self.write_rows(target, options, matte, |extent, target, row| {
            layer_color::photo::write_tiff_rows(output, extent, target, resolution, row)
        })
    }
    pub fn write_jpeg(
        &mut self,
        output: impl std::io::Write,
        target: &SourceInterpretation,
        options: layer_core::color::OutputEncoding,
        matte: [f32; 3],
        quality: u8,
    ) -> Result<layer_color::OutputStatistics, String> {
        let resolution = self.output_resolution;
        self.write_rows(target, options, Some(matte), |extent, target, row| {
            layer_color::photo::write_jpeg_rows(output, extent, target, resolution, quality, row)
        })
    }
    fn identity_source(
        &self,
        target: &SourceInterpretation,
    ) -> Option<Arc<layer_core::color::source::SourceImage>> {
        if self.background[3] != 0. {
            return None;
        }
        let mut visible = self
            .layers
            .iter()
            .filter(|l| l.visible && l.opacity > 0. && l.kind != LayerKind::Background);
        let layer = visible.next()?;
        if visible.next().is_some()
            || layer.kind != LayerKind::Paint
            || layer.opacity != 1.
            || layer.properties.parent.is_some()
            || layer.properties.offset != layer_core::Point::default()
            || layer.properties.blend != layer_core::LayerBlend::Normal
            || layer.properties.clipped
            || layer.mask.as_ref().is_some_and(|m| m.enabled)
            || layer.effect.is_some()
            || !self.backing[&layer.id].tiles.is_empty()
        {
            return None;
        }
        let source = layer.source.as_ref()?;
        (source.extent == self.extent
            && source.interpretation.channels == target.channels
            && source.interpretation.depth == target.depth
            && source.interpretation.profile == target.profile)
            .then(|| source.clone())
    }
    pub(super) fn write_rows(
        &mut self,
        target: &SourceInterpretation,
        options: layer_core::color::OutputEncoding,
        matte: Option<[f32; 3]>,
        write: impl FnOnce(
            [u32; 2],
            &SourceInterpretation,
            &mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>,
        ) -> Result<(), String>,
    ) -> Result<layer_color::OutputStatistics, String> {
        self.check_cancelled().map_err(|e| e.to_string())?;
        self.control.output_rows.store(0, Ordering::Relaxed);
        let encoder = layer_color::WorkingEncoder::new(self.color().space, target, options)?;
        let extent = self.output_extent;
        if extent == self.extent
            && options.conversion == Default::default()
            && matte.is_none()
            && let Some(source) = self.identity_source(target)
        {
            // Preserve exact integer samples, including hidden straight RGB,
            // when delivery does not require compositing or color conversion.
            let mut rows = source.rows();
            write(extent, encoder.interpretation(), &mut |y, row| {
                self.check_cancelled().map_err(|e| e.to_string())?;
                rows.read(y, row)?;
                self.control.output_rows.store(y + 1, Ordering::Relaxed);
                Ok(())
            })?;
            return Ok(Default::default());
        }
        let mut resampler = (extent != self.extent)
            .then(|| layer_color::RowResampler::new(self.extent, extent))
            .transpose()?;
        let mut resized = if resampler.is_some() {
            vec![[0.; 4]; extent[0] as usize]
        } else {
            Vec::new()
        };
        let control = self.control.clone();
        let mut source = Rows::new(self);
        let mut stats = layer_color::OutputStatistics::default();
        write(extent, encoder.interpretation(), &mut |y, row| {
            control.check().map_err(|e| e.to_string())?;
            let pixels = if let Some(resampler) = &mut resampler {
                resampler.read_row(y, &mut resized, |sy, target| {
                    target.copy_from_slice(source.read(sy)?);
                    Ok(())
                })?;
                &resized
            } else {
                source.read(y)?
            };
            stats.clipped_channels += encoder
                .encode_premultiplied(pixels, row, matte, [0, y])?
                .clipped_channels;
            control.output_rows.store(y + 1, Ordering::Relaxed);
            Ok(())
        })?;
        Ok(stats)
    }
}

/// Small source bands amortize GPU mappings while the resampler retains only
/// four filtered rows. No output size requires a full document readback.
pub(super) struct Rows<'a> {
    renderer: &'a mut SnapshotRenderer,
    band: Vec<[f32; 4]>,
    first: u32,
    end: u32,
}
impl<'a> Rows<'a> {
    pub(super) fn new(renderer: &'a mut SnapshotRenderer) -> Self {
        Self {
            renderer,
            band: Vec::new(),
            first: 0,
            end: 0,
        }
    }
    pub(super) fn read(&mut self, y: u32) -> Result<&[[f32; 4]], String> {
        self.renderer.check_cancelled().map_err(|e| e.to_string())?;
        let [width, height] = self.renderer.extent;
        if y >= height {
            return Err("Invalid source row".into());
        }
        if y < self.first || y >= self.end {
            self.band = Vec::new();
            self.first = y;
            self.end = (y + 16).min(height);
            self.band = self
                .renderer
                .read_region([0, y, width, self.end - y])
                .map_err(|e| e.to_string())?;
        }
        let start = (y - self.first) as usize * width as usize;
        Ok(&self.band[start..start + width as usize])
    }
}
