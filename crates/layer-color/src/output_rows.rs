//! One bounded delivery pipeline for GPU snapshot workers and browser file
//! workers. The reader supplies exact linear-premultiplied working pixels.
use crate::{OutputStatistics, RowResampler, WorkingEncoder};
use layer_core::color::{OutputEncoding, RgbSpace, source::SourceInterpretation};

pub fn build_local_tone_guide(
    extent: [u32; 2],
    space: RgbSpace,
    cancelled: impl Fn() -> bool,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<layer_core::color::hdr::LocalToneGuide, String> {
    let mut builder = layer_core::color::hdr::LocalToneBuilder::new(extent, space)?;
    let mut row = vec![[0.; 4]; extent[0] as usize];
    for y in 0..extent[1] {
        if cancelled() {
            return Err("Local tone mapping cancelled".into());
        }
        read(y, &mut row)?;
        builder.push(&row)?;
    }
    builder.finish(cancelled)
}

pub fn encode_working_rows_with_guide(
    working: RgbSpace,
    source_extent: [u32; 2],
    extent: [u32; 2],
    target: &SourceInterpretation,
    options: OutputEncoding,
    matte: Option<[f32; 3]>,
    rendition: Option<layer_core::color::hdr::SdrRendition>,
    guide: Option<&layer_core::color::hdr::LocalToneGuide>,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
    write: impl FnOnce(
        [u32; 2],
        &SourceInterpretation,
        &mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>,
    ) -> Result<(), String>,
) -> Result<OutputStatistics, String> {
    if let Some(r) = rendition {
        r.validate().map_err(str::to_string)?;
    }
    if rendition.is_some() && guide.is_none() {
        return Err("Local SDR rendition requires image analysis".into());
    }
    let encoder = WorkingEncoder::new(working, target, options)?.with_sdr_gamut(rendition);
    let mapper = rendition.map(|r| r.mapper(working, working));
    let mut resampler = (source_extent != extent)
        .then(|| RowResampler::new(source_extent, extent))
        .transpose()?;
    let mut pixels = vec![[0.; 4]; extent[0] as usize];
    let mut statistics = OutputStatistics::default();
    write(extent, encoder.interpretation(), &mut |y, row| {
        if let Some(resampler) = &mut resampler {
            resampler.read_row(y, &mut pixels, &mut read)?;
        } else {
            read(y, &mut pixels)?;
        }
        if let Some(r) = mapper {
            for (x, pixel) in pixels.iter_mut().enumerate() {
                if let Some(guide) = guide {
                    let position = [
                        (x as f32 + 0.5) * guide.document_extent[0] as f32 / extent[0] as f32,
                        (y as f32 + 0.5) * guide.document_extent[1] as f32 / extent[1] as f32,
                    ];
                    *pixel = r.tone_local_premultiplied(*pixel, position, guide);
                } else { *pixel = r.tone_premultiplied(*pixel); }
            }
        }
        statistics.clipped_channels += encoder
            .encode_premultiplied(&pixels, row, matte, [0, y])?
            .clipped_channels;
        Ok(())
    })?;
    Ok(statistics)
}

/// Decode the actual delivered integer rows before reducing for display. Profile,
/// matte, resize and dither have already been applied by the output row producer.
/// This intentionally excludes lossy codec compression artifacts.
pub fn preview_encoded_rows(
    extent: [u32; 2],
    bounds: [u32; 2],
    space: RgbSpace,
    actual: &SourceInterpretation,
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<([u32; 2], Vec<[f32; 4]>), String> {
    let decoder = crate::WorkingDecoder::new(actual, space, Default::default())?;
    let mut preview = crate::AreaPreview::new(extent, bounds)?;
    let mut bytes = vec![0; extent[0] as usize * actual.pixel_bytes()];
    let mut pixels = vec![[0.; 4]; extent[0] as usize];
    for y in 0..extent[1] {
        read(y, &mut bytes)?;
        decoder.decode_pixels(&bytes, &mut pixels)?;
        for p in &mut pixels {
            for c in 0..3 {
                p[c] *= p[3];
            }
        }
        preview.push(&pixels)?;
    }
    preview.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, SampleDepth, hdr::SdrRendition, source::SourceChannels};

    fn deliver(
        space: RgbSpace,
        profile: ColorProfile,
        pixels: &[[f32; 4]],
        rendition: Option<SdrRendition>,
    ) -> Vec<u16> {
        let target = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile,
            profile_assumed: false,
        };
        let mut bytes = vec![0; pixels.len() * 8];
        let read = |_, row: &mut [[f32; 4]]| {
            row.copy_from_slice(pixels);
            Ok(())
        };
        let guide = rendition
            .map(|_| build_local_tone_guide([pixels.len() as u32, 1], space, || false, read))
            .transpose()
            .unwrap();
        let _stats = encode_working_rows_with_guide(
            space,
            [pixels.len() as u32, 1],
            [pixels.len() as u32, 1],
            &target,
            Default::default(),
            None,
            rendition,
            guide.as_ref(),
            read,
            |_, _, read| read(0, &mut bytes),
        )
        .unwrap();
        bytes
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect()
    }
    #[test]
    fn resized_preview_uses_document_coordinates_of_supplied_guide() {
        let pixels: Vec<_> = (0..32).map(|x| [2f32.powf(x as f32 / 4. - 4.); 4])
            .map(|mut p| { p[3] = 1.; p }).collect();
        let guide = build_local_tone_guide([32, 1], RgbSpace::Srgb, || false,
            |_, row| { row.copy_from_slice(&pixels); Ok(()) }).unwrap();
        let target = SourceInterpretation { channels: SourceChannels::Rgba, depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false };
        let resized: Vec<_> = pixels.chunks_exact(4).map(|p| std::array::from_fn(|c| p.iter().map(|v| v[c]).sum::<f32>() / 4.)).collect();
        let encode = |source: &[[f32; 4]]| {
            let mut bytes = vec![0; 8 * 8];
            encode_working_rows_with_guide(RgbSpace::Srgb, [source.len() as u32, 1], [8, 1],
                &target, Default::default(), None, Some(SdrRendition::default()), Some(&guide),
                |_, row| { row.copy_from_slice(source); Ok(()) },
                |_, _, rows| rows(0, &mut bytes)).unwrap();
            bytes
        };
        assert_eq!(encode(&pixels), encode(&resized));
    }
    #[test]
    fn hdr_sdr_delivery_preserves_extended_brightness_and_coverage() {
        let pixels = [
            [0., 0., 0., 1.],
            [0.18, 0.18, 0.18, 1.],
            [1., 1., 1., 1.],
            [4., 4., 4., 1.],
            [16., 16., 16., 1.],
            [1., 1., 1., 0.25],
            [0.; 4],
        ];
        let output = deliver(
            RgbSpace::Srgb,
            ColorProfile::Builtin(RgbSpace::Srgb),
            &pixels,
            Some(SdrRendition::default()),
        );
        assert!(
            output[12] - output[8] > 7000,
            "+2 stops must survive SDR delivery"
        );
        let guide=build_local_tone_guide([7,1],RgbSpace::Srgb,||false,|_,row|{row.copy_from_slice(&pixels);Ok(())}).unwrap();
        let mapper=SdrRendition::default().mapper(RgbSpace::Srgb,RgbSpace::Srgb);
        for (i,p) in pixels.iter().enumerate() {
            let expected=mapper.map_local_premultiplied(*p,[i as f32+0.5,0.5],&guide);
            for c in 0..3 {
                let linear=if p[3]>0. {expected[c]/p[3]} else {0.};
                let code=(RgbSpace::Srgb.encode(linear as f64)*65535.).round() as u16;
                assert!(output[i*4+c].abs_diff(code)<=2);
            }
        }
        assert_eq!(output[23], 16384);
        assert_eq!(&output[24..28], &[0; 4]);
    }
    #[test]
    fn hdr_sdr_delivery_uses_destination_gamut_independently_of_working_space() {
        let physical = [[4., 1., 0.2], [0.1, 2., 0.3], [-0.15, 0.1, 2.]];
        for target in RgbSpace::ALL {
            let reference = deliver(
                RgbSpace::Srgb,
                ColorProfile::Builtin(target),
                &physical.map(|p| [p[0], p[1], p[2], 1.]),
                Some(SdrRendition::default()),
            );
            for working in RgbSpace::ALL {
                let matrix = RgbSpace::Srgb.linear_transform(working);
                let pixels = physical.map(|p| {
                    let p = matrix.map(|r| {
                        (r[0] * p[0] as f64 + r[1] * p[1] as f64 + r[2] * p[2] as f64) as f32
                    });
                    [p[0], p[1], p[2], 1.]
                });
                let result = deliver(
                    working,
                    ColorProfile::Builtin(target),
                    &pixels,
                    Some(SdrRendition::default()),
                );
                assert!(
                    result
                        .iter()
                        .zip(&reference)
                        .all(|(a, b)| a.abs_diff(*b) <= 1),
                    "{working:?} → {target:?}"
                );
            }
        }
    }
    #[test]
    fn sdr_rows_remain_unchanged_and_icc_hdr_rows_use_the_same_rendition() {
        let p = [
            [0.18, 0.18, 0.18, 1.],
            [0.5, 0.5, 0.5, 1.],
            [1., 1., 1., 1.],
        ];
        let sdr = deliver(
            RgbSpace::Srgb,
            ColorProfile::Builtin(RgbSpace::Srgb),
            &p,
            None,
        );
        assert_eq!(sdr[8], 65535);
        assert_eq!(sdr[4], (RgbSpace::Srgb.encode(0.5) * 65535.).round() as u16);
        let builtin = deliver(
            RgbSpace::Srgb,
            ColorProfile::Builtin(RgbSpace::Srgb),
            &p,
            Some(SdrRendition::default()),
        );
        let icc = ColorProfile::Icc(
            crate::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb))
                .unwrap()
                .into(),
        );
        let result = deliver(RgbSpace::Srgb, icc, &p, Some(SdrRendition::default()));
        assert!(
            result.iter().zip(builtin).all(|(a, b)| a.abs_diff(b) <= 3),
            "ICC neutral rendition differs: {result:?}"
        );
    }

    #[test]
    fn local_contrast_delivery_matches_viewing_and_icc_proof_input() {
        use layer_core::color::hdr::sdr_luminance_weights;
        let physical = [[8., 0., 0.], [0., 0., 16.], [-0.1, 3., 0.5], [0.18; 3]];
        for working in RgbSpace::ALL {
            let matrix = RgbSpace::Srgb.linear_transform(working);
            let pixels = physical.map(|p| {
                let p = layer_core::color::rgb::apply(matrix, p).map(|v| v as f32);
                [p[0] * 0.25, p[1] * 0.25, p[2] * 0.25, 0.25]
            });
            let guide=build_local_tone_guide([4,1],working,||false,|_,row|{row.copy_from_slice(&pixels);Ok(())}).unwrap();
            for (contrast,balance,highlight_color) in [(0.5,-1.,0.),(1.,0.,0.5),(2.,1.,1.)] {
                let recipe=SdrRendition {contrast,balance,highlight_color,..Default::default()};
                for output in RgbSpace::ALL {
                    let actual = deliver(
                        working,
                        ColorProfile::Builtin(output),
                        &pixels,
                        Some(recipe),
                    );
                    let mapper = recipe.mapper(working, output);
                    for (i, p) in pixels.iter().enumerate() {
                        let expected = mapper.map_local_premultiplied(*p,[i as f32+0.5,0.5],&guide);
                        for c in 0..3 {
                            let code = (output.encode(f64::from(expected[c] / expected[3]))
                                * 65535.)
                                .round() as u16;
                            assert!(
                                actual[i * 4 + c].abs_diff(code) <= 2,
                                "view/export mismatch {working:?} {output:?} {p:?}"
                            );
                        }
                        assert_eq!(actual[i * 4 + 3], 16384);
                    }
                }
                let profile = ColorProfile::Icc(
                    crate::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb))
                        .unwrap()
                        .into(),
                );
                let actual = deliver(working, profile.clone(), &pixels, Some(recipe));
                // Independent explicit proof preparation before ICC conversion.
                let bounded:Vec<_> = pixels.iter().enumerate().map(|(i,p)| {
                    let tone=recipe.mapper(working,working).tone_local_premultiplied(*p,[i as f32+0.5,0.5],&guide);
                    let rgb=layer_core::color::hdr::unified_sdr_gamut(
                        [tone[0]/p[3],tone[1]/p[3],tone[2]/p[3]],sdr_luminance_weights(working),highlight_color);
                    [rgb[0]*p[3],rgb[1]*p[3],rgb[2]*p[3],p[3]]
                }).collect();
                let expected = deliver(working, profile, &bounded, None);
                assert_eq!(
                    actual, expected,
                    "ICC must receive exactly the same bounded RGB as mapped print proofing"
                );
            }
        }
    }
}
