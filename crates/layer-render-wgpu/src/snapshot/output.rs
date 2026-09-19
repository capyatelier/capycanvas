//! Profiled streaming output; resizing never changes native snapshot geometry.
use super::*;
use layer_core::color::RgbSpace;

impl SnapshotRenderer {
    /// GPU analysis shared by previews and CPU delivery codecs. Only codecs
    /// download the bounded guide; full-resolution image analysis stays on GPU.
    pub fn local_tone_guide(
        &mut self,
    ) -> Result<Arc<layer_core::color::hdr::LocalToneGuide>, String> {
        self.check_cancelled().map_err(|e| e.to_string())?;
        if let Some(guide) = &self.local_tone {
            return Ok(guide.clone());
        }
        let guide = Arc::new(
            self.gpu_local_tone_guide()?
                .download(&self.renderer.queue)?,
        );
        self.local_tone = Some(guide.clone());
        Ok(guide)
    }
    /// Construct a document-space guide on the capture device. Source regions
    /// are composed and reduced in one submission with no image readback. Wait
    /// between regions and range anchors so cancellation cannot leave a whole
    /// document's work queued ahead of a newly arriving pen stroke.
    pub fn gpu_local_tone_guide(&mut self) -> Result<Arc<crate::local_tone::GpuToneGuide>, String> {
        use crate::local_tone::{Builder, range, wait};
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
                        Ok(()) => wait(&device, &queue)?,
                    }
                }
            }
        }
        self.check_cancelled().map_err(|e| e.to_string())?;
        let mut encoder = submission::CommandEncoder::new(&device, &Default::default());
        let statistics = builder.statistics(&mut encoder);
        builder.prepare(&mut encoder);
        encoder.submit(&queue);
        let (low, step, intervals, peak) = range(&device, &queue, &statistics)?;
        for index in 0..=intervals {
            self.check_cancelled().map_err(|e| e.to_string())?;
            let mut encoder = submission::CommandEncoder::new(&device, &Default::default());
            builder.anchor(&mut encoder, low, step, intervals, index);
            encoder.submit(&queue);
            wait(&device, &queue)?;
        }
        self.check_cancelled().map_err(|e| e.to_string())?;
        let mut encoder = submission::CommandEncoder::new(&device, &Default::default());
        let guide = builder.finish(&mut encoder, peak);
        encoder.submit(&queue);
        wait(&device, &queue)?;
        self.check_cancelled().map_err(|e| e.to_string())?;
        self.gpu_local_tone = Some(guide.clone());
        Ok(guide)
    }
    pub fn write_gainmap(
        &mut self,
        output: impl std::io::Write,
        format: layer_color::photo::GainMapFormat,
        quality: u8,
        matte: Option<[f32; 3]>,
        clip: bool,
    ) -> Result<layer_color::OutputStatistics, String> {
        let rendition = self
            .sdr_rendition
            .ok_or("Gain-map delivery requires an HDR document")?;
        let control = self.control.clone();
        let resolution = self.output_resolution;
        let guide = Some(self.local_tone_guide()?);
        self.hdr_rows(|extent, space, read| {
            layer_color::photo::write_gainmap_rows_with_guide(
                output,
                extent,
                space,
                rendition,
                guide.as_deref(),
                format,
                quality,
                resolution,
                matte,
                clip,
                control.cancellation_flag(),
                read,
            )
        })
    }
    pub fn write_exr(&mut self, output: impl std::io::Write + std::io::Seek) -> Result<layer_color::OutputStatistics, String> {
        let resolution = self.output_resolution;
        self.hdr_rows(|extent, space, read| layer_color::photo::write_exr_rows(output, extent, space, resolution, read).map(|_| Default::default()))?;
        Ok(Default::default())
    }
    pub fn write_hdr_png(
        &mut self,
        output: impl std::io::Write,
        clip: bool,
    ) -> Result<layer_color::OutputStatistics, String> {
        let resolution = self.output_resolution;
        self.hdr_rows(|extent, space, read| {
            layer_color::photo::write_hdr_png_rows(output, extent, space, resolution, clip, read)
        })
    }

    /// Exact edited D65 luminance peak for the saved SDR range.
    /// Traverse bounded bands; coverage is not brightness and hidden RGB is ignored.
    pub fn hdr_headroom(&mut self) -> Result<f32, String> {
        let mut peak = 1f32;
        self.hdr_rows(|extent, space, read| {
            let m = layer_core::color::hdr::to_bt2020(space);
            let mut row = vec![[0.; 4]; extent[0] as usize];
            for y in 0..extent[1] {
                read(y, &mut row)?;
                for p in &row {
                    if p[3] > 0. {
                        let v = layer_core::color::rgb::apply(
                            m,
                            [
                                p[0] as f64 / p[3] as f64,
                                p[1] as f64 / p[3] as f64,
                                p[2] as f64 / p[3] as f64,
                            ],
                        );
                        if v.iter().any(|c| !c.is_finite()) {
                            return Err("Cannot measure non-finite HDR data".into());
                        }
                        let measured = v.into_iter()
                            .zip(layer_core::color::hdr::BT2020_LUMA)
                            .map(|(v,w)|v*f64::from(w)).sum::<f64>();
                        peak = peak.max(measured as f32);
                    }
                }
            }
            Ok(Default::default())
        })?;
        let headroom = peak.log2();
        if headroom > 16. {
            return Err(
                "Edited HDR range exceeds the SDR mapper's 16-stop range. Adjust exposure first."
                    .into(),
            );
        }
        Ok(headroom)
    }
    pub fn inspect_hdr_output(&mut self) -> Result<layer_color::OutputStatistics, String> {
        self.hdr_rows(|extent, space, read| {
            layer_color::photo::inspect_hdr_rows(extent, space, read)
        })
    }

    pub(super) fn hdr_rows(
        &mut self,
        consume: impl FnOnce(
            [u32; 2],
            RgbSpace,
            &mut dyn FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
        ) -> Result<layer_color::OutputStatistics, String>,
    ) -> Result<layer_color::OutputStatistics, String> {
        self.check_cancelled().map_err(|e| e.to_string())?;
        if !self.color().depth.is_float() {
            return Err("HDR delivery requires an HDR document".into());
        }
        let extent = self.output_extent;
        let source_extent = self.extent;
        let space = self.color().space;
        let control = self.control.clone();
        let mut resampler = (source_extent != extent)
            .then(|| layer_color::RowResampler::new(source_extent, extent))
            .transpose()?;
        let mut source = Rows::new(self);
        consume(extent, space, &mut |y, row| {
            control.check().map_err(|e| e.to_string())?;
            if let Some(resampler) = &mut resampler {
                resampler.read_row(y, row, &mut |y, row: &mut [[f32; 4]]| {
                    row.copy_from_slice(source.read(y)?);
                    Ok(())
                })?;
            } else {
                row.copy_from_slice(source.read(y)?);
            }
            control.output_rows.store(y + 1, Ordering::Relaxed);
            Ok(())
        })
    }

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
        if let Some(source) = self.output_source(extent, target, options, matte) {
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
        let source_extent = self.extent;
        let working = self.color().space;
        let control = self.control.clone();
        let rendition = if target.depth.is_float() {
            None
        } else {
            self.sdr_rendition
        };
        let guide = if rendition.is_some() {
            Some(self.local_tone_guide()?)
        } else {
            None
        };
        let mut source = Rows::new(self);
        layer_color::encode_working_rows_with_guide(
            working,
            source_extent,
            extent,
            target,
            options,
            matte,
            rendition,
            guide.as_deref(),
            |y, row| {
                control.check().map_err(|e| e.to_string())?;
                row.copy_from_slice(source.read(y)?);
                Ok(())
            },
            |extent, target, read| {
                write(extent, target, &mut |y, row| {
                    control.check().map_err(|e| e.to_string())?;
                    read(y, row)?;
                    control.output_rows.store(y + 1, Ordering::Relaxed);
                    Ok(())
                })
            },
        )
    }
}

/// Bounded source bands amortize tile composition and GPU mappings while the
/// resampler retains only four filtered rows. Never a full document readback.
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
            let (rows, band) = self.renderer.read_band(y).map_err(|e| e.to_string())?;
            self.end = y + rows;
            self.band = band;
        }
        let start = (y - self.first) as usize * width as usize;
        Ok(&self.band[start..start + width as usize])
    }
}
