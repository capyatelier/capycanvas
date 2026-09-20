//! Twelve-bit Rust AV1 encoding, with a pixel-exact path for alpha and gains.
use super::*;
use rav1e::prelude::*;

pub(super) struct Coded {
    pub extent: [u32; 2],
    pub config: Vec<u8>,
    pub bytes: Vec<u8>,
}

pub(super) fn encode(
    extent: [u32; 2],
    color: Option<Color>,
    quality: u8,
    budget: usize,
    cancel: &AtomicBool,
    mut pixel: impl FnMut(u32, u32) -> [u16; 3],
) -> Result<Coded, String> {
    codec::check(cancel)?;
    validate_extent(extent, 1024)?;
    if !(1..=100).contains(&quality) {
        return Err("Invalid AVIF quality".into());
    }
    let pixels = u64::from(extent[0].div_ceil(64) * 64) * u64::from(extent[1].div_ceil(64) * 64);
    if pixels * 192 + 16 * 1024 * 1024 > budget as u64 {
        return Err("AVIF encoder exceeds the codec budget".into());
    }
    if quality == 100 {
        use oxideav_av1::encoder::{
            ChromaFormat, RateModel, YuvFrame, encode_gop_yuv_seg_extras_tuned,
            inter_frame::GopTuning,
        };
        if extent.into_iter().any(|n| n % 8 != 0) {
            return Err("Lossless AVIF cells require 8-pixel alignment".into());
        }
        let mono = color.is_none();
        let mut frame = YuvFrame::filled(
            extent[0],
            extent[1],
            12,
            if mono {
                ChromaFormat::Monochrome
            } else {
                ChromaFormat::Yuv444
            },
            0,
        );
        for y in 0..extent[1] {
            codec::check(cancel)?;
            for x in 0..extent[0] {
                let values = pixel(x, y);
                let at = y as usize * extent[0] as usize + x as usize;
                for (plane, &value) in [&mut frame.y, &mut frame.u, &mut frame.v]
                    .into_iter()
                    .zip(&values)
                    .take(if mono { 1 } else { 3 })
                {
                    if value > 4095 {
                        return Err("AVIF encoder sample exceeds 12-bit precision".into());
                    }
                    plane[at] = value;
                }
            }
        }
        // The pinned library exposes its cheaper rate model through the GOP
        // API. One frame runs the same still encoder, with quantizer zero for
        // exact samples. Skip the default entropy-cost search: it dominates
        // alpha/gain encoding without improving their reconstruction.
        let mut encoded = encode_gop_yuv_seg_extras_tuned(
            std::slice::from_ref(&frame),
            0,
            &[],
            &[],
            false,
            None,
            GopTuning {
                model: RateModel::Heuristic,
                ..Default::default()
            },
        )
        .map_err(err)?
        .gop;
        codec::check(cancel)?;
        // The general YUV encoder defaults to limited range and unspecified
        // CICP. Full-range alpha must also be declared in the AV1 sequence;
        // container nclx cannot override its range in independent decoders.
        // These color-description fields do not alter coded plane samples.
        encoded.seq.color_config.color_range = true;
        if let Some(color) = color {
            let cc = &mut encoded.seq.color_config;
            cc.color_description_present_flag = true;
            cc.color_primaries = color.cicp[0] as u8;
            cc.transfer_characteristics = color.cicp[1] as u8;
            cc.matrix_coefficients = color.cicp[2] as u8;
        }
        use oxideav_av1::encoder::{
            obu::{ObuFrame, build_temporal_unit},
            sequence_obu::write_sequence_header_obu,
        };
        use oxideav_av1::obu::{ObuIter, ObuType};
        let mut frames = Vec::new();
        if encoded.temporal_units.len() != 1 {
            return Err("Unexpected lossless AVIF encoder temporal units".into());
        }
        for obu in ObuIter::new(&encoded.temporal_units[0]) {
            let obu = obu.map_err(err)?;
            if obu.obu_type == ObuType::Frame {
                frames.push(ObuFrame::new(ObuType::Frame, obu.payload.to_vec()));
            }
        }
        if frames.len() != 1 {
            return Err("Unexpected lossless AVIF encoder frame".into());
        }
        let bytes = build_temporal_unit(Some(&write_sequence_header_obu(&encoded.seq)), &frames);
        if bytes.is_empty() || bytes.len() > budget {
            return Err("Invalid AVIF encoder output".into());
        }
        let op = &encoded.seq.operating_points[0];
        let cc = &encoded.seq.color_config;
        return Ok(Coded {
            extent,
            config: vec![
                0x81,
                (encoded.seq.seq_profile << 5) | op.seq_level_idx,
                (op.seq_tier << 7)
                    | 0x60
                    | (u8::from(mono) << 4)
                    | (u8::from(cc.subsampling_x) << 3)
                    | (u8::from(cc.subsampling_y) << 2)
                    | cc.chroma_sample_position,
                0,
            ],
            bytes,
        });
    }
    let mut options = EncoderConfig::with_speed_preset(10);
    options.width = extent[0] as usize;
    options.height = extent[1] as usize;
    options.bit_depth = 12;
    options.chroma_sampling = if color.is_some() {
        ChromaSampling::Cs444
    } else {
        ChromaSampling::Cs400
    };
    options.pixel_range = PixelRange::Full;
    options.color_description = color.map(|c| ColorDescription {
        color_primaries: match c.cicp[0] {
            9 => ColorPrimaries::BT2020,
            _ => ColorPrimaries::Unspecified,
        },
        transfer_characteristics: match c.cicp[1] {
            13 => TransferCharacteristics::SRGB,
            _ => TransferCharacteristics::Unspecified,
        },
        matrix_coefficients: MatrixCoefficients::Identity,
    });
    options.still_picture = true;
    options.low_latency = true;
    options.quantizer = (1 + (u32::from(100 - quality) * 254 + 49) / 99) as usize;
    options.min_quantizer = options.quantizer as u8;
    options.tune = Tune::Psnr;
    let mut context: Context<u16> = Config::new()
        .with_encoder_config(options)
        .with_threads(1)
        .new_context()
        .map_err(err)?;
    let config = context.container_sequence_header();
    let mut frame = context.new_frame();
    for y in 0..extent[1] {
        codec::check(cancel)?;
        for x in 0..extent[0] {
            let values = pixel(x, y);
            for (plane, &value) in frame
                .planes
                .iter_mut()
                .zip(&values)
                .take(if color.is_some() { 3 } else { 1 })
            {
                if value > 4095 {
                    return Err("AVIF encoder sample exceeds 12-bit precision".into());
                }
                let stride = plane.cfg.stride;
                let origin = plane.cfg.yorigin * stride + plane.cfg.xorigin;
                plane.data[origin + y as usize * stride + x as usize] = value;
            }
        }
    }
    codec::check(cancel)?;
    context.send_frame(frame).map_err(err)?;
    context.flush();
    let mut bytes = Vec::new();
    loop {
        codec::check(cancel)?;
        match context.receive_packet() {
            Ok(packet) => {
                if !bytes.is_empty() || packet.frame_type != FrameType::KEY {
                    return Err("Unexpected AVIF encoder frame".into());
                }
                bytes = packet.data;
            }
            Err(EncoderStatus::Encoded) => continue,
            Err(EncoderStatus::LimitReached) => break,
            Err(e) => return Err(format!("AVIF encoding failed: {e}")),
        }
    }
    codec::check(cancel)?;
    if bytes.is_empty() || bytes.len() > budget {
        return Err("Invalid AVIF encoder output".into());
    }
    Ok(Coded {
        extent,
        config,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossless_twelve_bit_planes_roundtrip_through_rav1d() {
        let cancel = AtomicBool::new(false);
        for mono in [false, true] {
            let extent = [32, 24];
            let color = (!mono).then_some(Color {
                cicp: [9, 13, 0],
                full_range: true,
            });
            let sample = |x: u32, y: u32| {
                std::array::from_fn(|c| {
                    ((x * [83, 97, 71][c] + y * [51, 17, 37][c] + [1, 13, 39][c]) % 4096) as u16
                })
            };
            let encoded = encode(extent, color, 100, 64 * 1024 * 1024, &cancel, sample).unwrap();
            let decoded =
                codec::decode(&encoded.bytes, &[], extent, 64 * 1024 * 1024, &cancel).unwrap();
            assert_eq!(decoded.depth, 12);
            assert_eq!(decoded.layout, if mono { 0 } else { 3 });
            assert!(decoded.full_range);
            if let Some(color) = color {
                assert_eq!(decoded.cicp, color.cicp);
            }
            let mut maximum = 0;
            for y in 0..extent[1] {
                for x in 0..extent[0] {
                    for c in 0..if mono { 1 } else { 3 } {
                        maximum = maximum.max(decoded.sample(c, x, y).abs_diff(sample(x, y)[c]));
                    }
                }
            }
            assert_eq!(maximum, 0, "Alpha and gain-map samples must be lossless");
        }
    }
}
