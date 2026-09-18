//! Bounded artwork/output comparisons. Output simulation precedes reduction;
//! thumbnail pixels cannot replace native editing, sampling or delivery data.
use super::{output::Rows, *};
use layer_core::color::{OutputEncoding, RgbSpace};

fn reduce(
    source: [u32; 2],
    bounds: [u32; 2],
    space: RgbSpace,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<SnapshotPreview, String> {
    if bounds.into_iter().any(|v| !(1..=1024).contains(&v)) {
        return Err("Preview bounds must be between 1 and 1024 pixels".into());
    }
    let scale = (f64::from(bounds[0]) / f64::from(source[0]))
        .min(f64::from(bounds[1]) / f64::from(source[1]))
        .min(1.);
    let extent = source.map(|v| (f64::from(v) * scale).round().max(1.) as u32);
    let mut sampler = layer_color::RowResampler::new(source, extent)?;
    let mut pixels = vec![[0.; 4]; (extent[0] * extent[1]) as usize];
    for (y, row) in pixels.chunks_exact_mut(extent[0] as usize).enumerate() {
        sampler.read_row(y as u32, row, &mut read)?;
    }
    Ok(SnapshotPreview {
        extent,
        space,
        pixels,
    })
}

impl SnapshotRenderer {
    /// Reduce the complete native composition before mapping its linear RGB to
    /// the requested viewing space. This does not apply delivery choices.
    pub fn preview_document(
        &mut self,
        bounds: [u32; 2],
        space: RgbSpace,
    ) -> Result<SnapshotPreview, String> {
        self.preview_document_for_display(bounds, space, 1.)
    }

    /// Same composition as the canvas. Headroom above one uses the HDR display
    /// shoulder; one uses the document's saved SDR appearance. Neither edits it.
    pub fn preview_document_for_display(
        &mut self,
        bounds: [u32; 2],
        space: RgbSpace,
        headroom: f32,
    ) -> Result<SnapshotPreview, String> {
        self.preview_document_with_coverage(bounds, space, headroom)
            .map(|v| v.0)
    }
    pub fn preview_document_with_coverage(
        &mut self,
        bounds: [u32; 2],
        space: RgbSpace,
        headroom: f32,
    ) -> Result<(SnapshotPreview, bool), String> {
        if !headroom.is_finite() || !(1. ..=100.).contains(&headroom) {
            return Err("Invalid display HDR headroom".into());
        }
        self.check_cancelled().map_err(|e| e.to_string())?;
        let extent = self.extent;
        let matrix = self.color().space.linear_transform(space);
        let rendition = self.sdr_rendition;
        let sdr = rendition.map(|r| r.mapper(self.color().space, space));
        let guide = if headroom <= 1. && rendition.is_some_and(|r| r.is_local()) {
            Some(self.local_tone_guide()?)
        } else {
            None
        };
        let weights = layer_core::color::hdr::sdr_luminance_weights(self.color().space);
        let mut rows = Rows::new(self);
        let mut transparent = false;
        let mut image = reduce(extent, bounds, space, |y, target| {
            target.copy_from_slice(rows.read(y)?);
            transparent |= target.iter().any(|p| p[3] < 1.);
            Ok(())
        })?;
        // Linear primary/adaptation matrices commute with area averaging and
        // associated alpha. No encoded or bounded sRGB intermediate is used.
        for (i, pixel) in image.pixels.iter_mut().enumerate() {
            if let Some(guide) = &guide {
                let position = [
                    (i as u32 % image.extent[0]) as f32 + 0.5,
                    (i as u32 / image.extent[0]) as f32 + 0.5,
                ];
                let position = std::array::from_fn(|c| {
                    position[c] * extent[c] as f32 / image.extent[c] as f32
                });
                *pixel = guide.adjust_with_weights(*pixel, position, weights, rendition.unwrap());
            }
            if rendition.is_some() {
                *pixel = if headroom > 1. {
                    layer_core::color::hdr::map_display_premultiplied(*pixel, headroom)
                } else {
                    *pixel = sdr.unwrap().map_premultiplied(*pixel);
                    continue;
                };
            }
            let rgb: [f64; 3] = std::array::from_fn(|c| f64::from(pixel[c]));
            for (out, row) in pixel[..3].iter_mut().zip(matrix) {
                *out = row.iter().zip(rgb).map(|(m, v)| m * v).sum::<f64>() as f32;
            }
        }
        Ok((image, transparent))
    }

    /// Decode the actual gain-map file and its actual SDR base before reducing.
    /// Both views share one encoding; switching HDR/SDR requires no new worker.
    pub fn preview_gainmap_output(
        &mut self,
        bounds: [u32; 2],
        space: RgbSpace,
        headroom: f32,
        format: layer_color::photo::GainMapFormat,
        quality: u8,
        matte: Option<[f32; 3]>,
    ) -> Result<
        (
            SnapshotPreview,
            SnapshotPreview,
            layer_color::OutputStatistics,
        ),
        String,
    > {
        let rendition = self
            .sdr_rendition
            .ok_or("Gain-map delivery requires HDR artwork")?;
        let control = self.control.clone();
        let mut result = None;
        let guide = if rendition.is_local() {
            Some(self.local_tone_guide()?)
        } else {
            None
        };
        let stats = self.hdr_rows(|extent, working, read| {
            let (extent, hdr, sdr, stats) = layer_color::photo::preview_gainmap_rows_with_guide(
                extent,
                bounds,
                working,
                rendition,
                guide.as_deref(),
                format,
                quality,
                matte,
                control.cancellation_flag(),
                read,
            )?;
            result = Some((extent, hdr, sdr));
            Ok(stats)
        })?;
        let (extent, mut hdr, mut sdr) = result.unwrap();
        let matrix = RgbSpace::Srgb.linear_transform(space);
        let mapper = rendition.mapper(RgbSpace::Srgb, space);
        for (i, p) in hdr.iter_mut().enumerate() {
            if headroom <= 1. {
                if let Some(guide) = &guide {
                    let position = [
                        (i as u32 % extent[0]) as f32 + 0.5,
                        (i as u32 / extent[0]) as f32 + 0.5,
                    ];
                    *p = guide.adjust(
                        *p,
                        std::array::from_fn(|c| {
                            position[c] * guide.document_extent[c] as f32 / extent[c] as f32
                        }),
                        RgbSpace::Srgb,
                        rendition,
                    );
                }
                *p = mapper.map_premultiplied(*p);
                continue;
            }
            *p = layer_core::color::hdr::map_display_premultiplied(*p, headroom);
            let rgb = layer_core::color::rgb::apply(matrix, [p[0], p[1], p[2]].map(f64::from));
            p[..3].copy_from_slice(&rgb.map(|v| v as f32));
        }
        for p in &mut sdr {
            let rgb = layer_core::color::rgb::apply(matrix, [p[0], p[1], p[2]].map(f64::from));
            p[..3].copy_from_slice(&rgb.map(|v| v as f32));
        }
        Ok((
            SnapshotPreview {
                extent,
                space,
                pixels: hdr,
            },
            SnapshotPreview {
                extent,
                space,
                pixels: sdr,
            },
            stats,
        ))
    }

    /// Preview resized PQ delivery, including actual 16-bit RGB/alpha codes and
    /// range failures. Reduction and display mapping happen after output coding.
    pub fn preview_hdr_output(
        &mut self,
        bounds: [u32; 2],
        space: RgbSpace,
        headroom: f32,
    ) -> Result<(SnapshotPreview, layer_color::OutputStatistics), String> {
        if !headroom.is_finite() || !(1. ..=100.).contains(&headroom) {
            return Err("Invalid display HDR headroom".into());
        }
        let mut result = None;
        let statistics = self.hdr_rows(|extent, working, read| {
            let (extent, pixels, stats) =
                layer_color::photo::preview_hdr_rows(extent, bounds, working, read)?;
            result = Some(SnapshotPreview {
                extent,
                space,
                pixels,
            });
            Ok(stats)
        })?;
        let mut image = result.expect("successful HDR preview produced pixels");
        let working = self.color().space;
        let to_working = RgbSpace::Srgb.linear_transform(working);
        let matrix = working.linear_transform(space);
        let rendition = self
            .sdr_rendition
            .unwrap_or_default()
            .mapper(working, space);
        let guide = if headroom <= 1. && self.sdr_rendition.is_some_and(|r| r.is_local()) {
            Some(self.local_tone_guide()?)
        } else {
            None
        };
        for (i, pixel) in image.pixels.iter_mut().enumerate() {
            let rgb = layer_core::color::rgb::apply(
                to_working,
                [pixel[0], pixel[1], pixel[2]].map(f64::from),
            );
            pixel[..3].copy_from_slice(&rgb.map(|v| v as f32));
            if let Some(guide) = &guide {
                let position = [
                    (i as u32 % image.extent[0]) as f32 + 0.5,
                    (i as u32 / image.extent[0]) as f32 + 0.5,
                ];
                *pixel = guide.adjust(
                    *pixel,
                    std::array::from_fn(|c| {
                        position[c] * guide.document_extent[c] as f32 / image.extent[c] as f32
                    }),
                    working,
                    self.sdr_rendition.unwrap(),
                );
            }
            *pixel = if headroom > 1. {
                layer_core::color::hdr::map_display_premultiplied(*pixel, headroom)
            } else {
                *pixel = rendition.map_premultiplied(*pixel);
                continue;
            };
            let rgb = layer_core::color::rgb::apply(
                matrix,
                [pixel[0], pixel[1], pixel[2]].map(f64::from),
            );
            pixel[..3].copy_from_slice(&rgb.map(|v| v as f32));
        }
        Ok((image, statistics))
    }

    /// Simulate the actual encoded output rows, including delivery resizing,
    /// profile, depth, dither and matte, then interpret those samples for viewing
    /// and reduce. Codec compression artifacts are deliberately excluded.
    pub fn preview_output(
        &mut self,
        bounds: [u32; 2],
        space: RgbSpace,
        target: &SourceInterpretation,
        options: OutputEncoding,
        matte: Option<[f32; 3]>,
    ) -> Result<(SnapshotPreview, layer_color::OutputStatistics), String> {
        let mut preview = None;
        let statistics = self.write_rows(target, options, matte, |extent, actual, read| {
            let (extent, pixels) =
                layer_color::preview_encoded_rows(extent, bounds, space, actual, read)?;
            preview = Some(SnapshotPreview {
                extent,
                space,
                pixels,
            });
            Ok(())
        })?;
        Ok((
            preview.expect("successful output consumer produced a preview"),
            statistics,
        ))
    }
}
