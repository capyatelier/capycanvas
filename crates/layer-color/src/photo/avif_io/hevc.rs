//! HEIF coded items through heif-oxide's bounded Rust HEVC adapter.
use super::*;
use heif_oxide::hevc::{DecoderLimits, PixelData};
use std::sync::atomic::Ordering;

pub(super) fn decode(
    payload: &[u8],
    properties: &Properties<'_>,
    inherited: Option<Color>,
    alpha: bool,
    budget: usize,
    cancel: &AtomicBool,
) -> Result<RawImage, String> {
    codec::check(cancel)?;
    let extent = properties.extent.ok_or("Missing HEIF spatial extent")?;
    validate_extent(extent, 32768)?;
    if properties.config.is_some() {
        return Err("HEIF item has incompatible AV1 configuration".into());
    }
    let record = properties.hevc.ok_or("Missing HEIF HEVC configuration")?;
    if record.len().saturating_add(64 * 24) > budget {
        return Err("HEVC configuration exceeds the codec memory budget".into());
    }
    let config = heif_oxide::hevc::parse_hvcc(record).map_err(err)?;
    // Validate every length/header before rebuilding Annex B. This also bounds
    // the NAL vector and emulation-prevention bookkeeping before allocation.
    let valid_nal = |nal: &[u8]| -> Result<(), String> {
        if nal.len() < 2 || nal[0] & 0x80 != 0 || nal[1] & 7 == 0 {
            return Err("Invalid HEIF HEVC NAL header".into());
        }
        if nal[0] & 1 != 0 || nal[1] >> 3 != 0 {
            return Err("Multi-layer HEVC is not supported".into());
        }
        if nal[2..].windows(3).any(|v| v == [0, 0, 1]) {
            return Err("Unescaped HEVC start code inside a length-prefixed NAL".into());
        }
        Ok(())
    };
    for ps in &config.parameter_sets {
        valid_nal(ps)?;
    }
    let mut r = Reader::new(payload);
    let mut count = config.parameter_sets.len();
    let mut annex_size = config
        .parameter_sets
        .iter()
        .map(|v| v.len() + 4)
        .sum::<usize>();
    while r.left() != 0 {
        codec::check(cancel)?;
        let length = match config.nal_length_size {
            1 => r.u8()? as usize,
            2 => r.u16()? as usize,
            4 => r.u32()? as usize,
            _ => return Err("Unsupported HEVC NAL length size".into()),
        };
        valid_nal(r.take(length)?)?;
        count += 1;
        if count > 65536 {
            return Err("HEVC item has too many NAL units".into());
        }
        annex_size = annex_size
            .checked_add(length + 4)
            .ok_or("HEVC payload size overflow")?;
    }
    let pixels_bytes = u64::from(extent[0]) * u64::from(extent[1]) * 8;
    // Annex B, unescaped RBSP and escape-position vectors can coexist. Reserve
    // frame output separately from the decoder's picture/filter working set.
    // Vec growth can retain twice its used capacity. Six encoded lengths
    // cover Annex B, RBSP and u32 escape positions even in an escape-heavy NAL.
    let scratch = (annex_size as u64) * 6
        + (count as u64) * 128
        + record.len() as u64
        + pixels_bytes
        + 1024 * 1024;
    let remaining = (budget as u64)
        .checked_sub(scratch)
        .ok_or("HEVC exceeds the codec memory budget")?;
    let annex = heif_oxide::hevc::build_annex_b(&config, payload).map_err(err)?;
    let decoded = heif_oxide::hevc::decode_first_frame_with_limits(
        &annex,
        DecoderLimits {
            expected_extent: Some(extent),
            memory_bytes: remaining as usize,
            still_only: true,
        },
        &|| cancel.load(Ordering::Acquire),
    )
    .map_err(err)?;
    let frame = decoded.frame;
    if [frame.width, frame.height] != extent
        || frame.bit_depth != decoded.info.bit_depth
        || properties.bits.is_some_and(|v| v != frame.bit_depth)
    {
        return Err("HEVC samples disagree with HEIF spatial extent or precision".into());
    }
    if properties
        .channels
        .is_some_and(|v| v != 3 && !(alpha && v == 1) && !(!alpha && v == 4))
    {
        return Err("HEIF channels disagree with HEVC samples".into());
    }
    let sample = |data: &PixelData, index: usize| -> u16 {
        match data {
            PixelData::U8(v) => u16::from(v[index]),
            PixelData::U16(v) => v[index],
        }
    };
    let len = |data: &PixelData| match data {
        PixelData::U8(v) => v.len(),
        PixelData::U16(v) => v.len(),
    };
    let cw = extent[0].div_ceil(2) as usize;
    let ch = extent[1].div_ceil(2) as usize;
    if len(&frame.y) != extent[0] as usize * extent[1] as usize
        || len(&frame.u) != cw * ch
        || len(&frame.v) != cw * ch
    {
        return Err("Incomplete HEVC decoded planes".into());
    }
    let mut output = pixels(extent, budget)?;
    let maximum = (1u16 << frame.bit_depth) - 1;
    for (y, row) in output.chunks_exact_mut(extent[0] as usize).enumerate() {
        codec::check(cancel)?;
        for (x, p) in row.iter_mut().enumerate() {
            let chroma = y / 2 * cw + x / 2;
            *p = [
                sample(&frame.y, y * extent[0] as usize + x),
                sample(&frame.u, chroma),
                sample(&frame.v, chroma),
                maximum,
            ];
            if p.iter().any(|v| *v > maximum) {
                return Err("HEVC sample exceeds its declared precision".into());
            }
        }
    }
    let color = inherited.unwrap_or_else(|| {
        decoded.info.color.map_or(
            Color {
                cicp: [2, 2, 1],
                full_range: false,
            },
            |(cicp, full_range)| Color { cicp, full_range },
        )
    });
    Ok(RawImage {
        extent,
        depth: frame.bit_depth,
        layout: if alpha { 0 } else { 1 },
        color,
        chroma_location: decoded.info.chroma_location as u8,
        premultiplied: false,
        pixels: output,
    })
}
