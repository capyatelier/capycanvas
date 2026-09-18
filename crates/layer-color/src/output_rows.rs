//! One bounded delivery pipeline for GPU snapshot workers and browser file
//! workers. The reader supplies exact linear-premultiplied working pixels.
use crate::{OutputStatistics, RowResampler, WorkingEncoder};
use layer_core::color::{OutputEncoding, RgbSpace, source::SourceInterpretation};

pub fn encode_working_rows(
    working: RgbSpace,
    source_extent: [u32; 2],
    extent: [u32; 2],
    target: &SourceInterpretation,
    options: OutputEncoding,
    matte: Option<[f32; 3]>,
    rendition: Option<layer_core::color::hdr::SdrRendition>,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
    write: impl FnOnce(
        [u32; 2],
        &SourceInterpretation,
        &mut dyn FnMut(u32, &mut [u8]) -> Result<(), String>,
    ) -> Result<(), String>,
) -> Result<OutputStatistics, String> {
    if let Some(r) = rendition { r.validate().map_err(str::to_string)?; }
    let encoder = WorkingEncoder::new(working, target, options)?.with_hdr_gamut_mapping(rendition.is_some());
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
        if let Some(r) = mapper { for pixel in &mut pixels { *pixel = r.tone_premultiplied(*pixel); } }
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

    fn deliver(space: RgbSpace, profile: ColorProfile, pixels: &[[f32;4]], rendition: Option<SdrRendition>) -> Vec<u16> {
        let target=SourceInterpretation {channels:SourceChannels::Rgba,depth:SampleDepth::U16,profile,profile_assumed:false};
        let mut bytes=vec![0;pixels.len()*8];
        let stats=encode_working_rows(space,[pixels.len() as u32,1],[pixels.len() as u32,1],&target,Default::default(),None,rendition,
            |_,row| {row.copy_from_slice(pixels);Ok(())},
            |_,_,read|read(0,&mut bytes)).unwrap();
        assert_eq!(stats.clipped_channels,0,"HDR gamut rolloff should avoid hard output clipping");
        bytes.chunks_exact(2).map(|v|u16::from_le_bytes([v[0],v[1]])).collect()
    }
    #[test]
    fn hdr_sdr_delivery_keeps_middle_gray_highlight_separation_and_coverage() {
        let pixels=[[0.,0.,0.,1.],[0.18,0.18,0.18,1.],[1.,1.,1.,1.],[4.,4.,4.,1.],[16.,16.,16.,1.],[1.,1.,1.,0.25],[0.;4]];
        let output=deliver(RgbSpace::Srgb,ColorProfile::Builtin(RgbSpace::Srgb),&pixels,Some(SdrRendition::default()));
        // Independent curve landmarks: black, unchanged middle gray, broad shoulder.
        for (i,linear) in [0.,0.18,0.59,0.8550862069,0.9595913462].into_iter().enumerate() {
            let code=(RgbSpace::Srgb.encode(linear)*65535.).round() as u16;
            assert!(output[i*4].abs_diff(code)<=1,"{i}: {} vs {code}",output[i*4]);
        }
        assert!(output[12]-output[8]>7000,"+2 stops must survive SDR delivery");
        assert_eq!(&output[20..23],&output[12..15]);
        assert_eq!(output[23],16384);
        assert_eq!(&output[24..28],&[0;4]);
    }
    #[test]
    fn hdr_sdr_delivery_uses_destination_gamut_independently_of_working_space() {
        let physical=[[4.,1.,0.2],[0.1,2.,0.3],[-0.15,0.1,2.]];
        for target in RgbSpace::ALL {
            let reference=deliver(RgbSpace::Srgb,ColorProfile::Builtin(target),&physical.map(|p|[p[0],p[1],p[2],1.]),Some(SdrRendition::default()));
            for working in RgbSpace::ALL {
                let matrix=RgbSpace::Srgb.linear_transform(working);
                let pixels=physical.map(|p| {let p=matrix.map(|r|(r[0]*p[0] as f64+r[1]*p[1] as f64+r[2]*p[2] as f64) as f32);[p[0],p[1],p[2],1.]});
                let result=deliver(working,ColorProfile::Builtin(target),&pixels,Some(SdrRendition::default()));
                assert!(result.iter().zip(&reference).all(|(a,b)|a.abs_diff(*b)<=1),"{working:?} → {target:?}");
            }
        }
    }
    #[test]
    fn sdr_rows_remain_unchanged_and_icc_hdr_rows_use_the_same_rendition() {
        let p=[[0.18,0.18,0.18,1.],[0.5,0.5,0.5,1.],[1.,1.,1.,1.]];
        let sdr=deliver(RgbSpace::Srgb,ColorProfile::Builtin(RgbSpace::Srgb),&p,None);
        assert_eq!(sdr[8],65535);
        assert_eq!(sdr[4],(RgbSpace::Srgb.encode(0.5)*65535.).round() as u16);
        let builtin=deliver(RgbSpace::Srgb,ColorProfile::Builtin(RgbSpace::Srgb),&p,Some(SdrRendition::default()));
        let icc=ColorProfile::Icc(crate::profile_bytes(&ColorProfile::Builtin(RgbSpace::Srgb)).unwrap().into());
        let result=deliver(RgbSpace::Srgb,icc,&p,Some(SdrRendition::default()));
        assert!(result.iter().zip(builtin).all(|(a,b)|a.abs_diff(b)<=3),"ICC neutral rendition differs: {result:?}");
    }
}
