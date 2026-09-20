//! HEIF coded items decoded directly by the bounded Rust HEVC decoder.
use super::*;
use rust_h265::{Decoder, DecoderLimits, Frame, PixelData, SequenceInfo, parse_annex_b};
use std::sync::atomic::Ordering;

// Read the hvcC parameter sets with the shared bounded container reader. Borrow
// the NALs until the admitted Annex B allocation; do not copy a second container.
fn configuration(record: &[u8]) -> Result<(u8, Vec<&[u8]>), String> {
    if record.len() > 1024 * 1024 {
        return Err("hvcC exceeds parameter-set budget".into());
    }
    let mut r = Reader::new(record);
    if r.u8()? != 1 {
        return Err("Unsupported hvcC configuration version".into());
    }
    r.take(20)?;
    let length_size = (r.u8()? & 3) + 1;
    if !matches!(length_size, 1 | 2 | 4) {
        return Err("Unsupported HEVC NAL length size".into());
    }
    let arrays = r.u8()?;
    let mut parameters = Vec::new();
    for _ in 0..arrays {
        r.u8()?; // Array completeness and type; the NAL header is authoritative.
        let count = r.u16()?;
        for _ in 0..count {
            if parameters.len() == 64 {
                return Err("Too many HEVC parameter sets".into());
            }
            let length = usize::from(r.u16()?);
            parameters.push(r.take(length)?);
        }
    }
    Ok((length_size, parameters))
}

fn next_nal<'a>(reader: &mut Reader<'a>, length_size: u8) -> Result<&'a [u8], String> {
    let length = match length_size {
        1 => usize::from(reader.u8()?),
        2 => usize::from(reader.u16()?),
        4 => reader.u32()? as usize,
        _ => return Err("Unsupported HEVC NAL length size".into()),
    };
    reader.take(length)
}

// Keep the still-only orchestration and cancellation boundary formerly added by
// our heif-oxide patch. Decoder admission, VUI metadata and block/filter checks
// remain in rust_h265; a partial flush must never masquerade as a complete still.
pub(super) fn decode_still(
    annex: &[u8],
    limits: DecoderLimits,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Frame, SequenceInfo), String> {
    let check = || {
        if cancelled() {
            Err("HEVC decode cancelled".to_owned())
        } else {
            Ok(())
        }
    };
    check()?;
    let nals = parse_annex_b(annex);
    if nals.is_empty() || nals.len() > 65536 {
        return Err("Invalid HEVC NAL count".into());
    }
    let mut decoder = Decoder::with_limits(limits);
    let mut output = None;
    for nal in &nals {
        check()?;
        if output.is_some() {
            if nal.nal_unit_type.is_vcl() {
                return Err("Multiple HEVC pictures in one still item".into());
            }
            continue;
        }
        if let Some(frame) = decoder
            .decode_nal_with_cancel(nal, cancelled)
            .map_err(err)?
        {
            let info = decoder.sequence_info().ok_or("Missing HEVC sequence")?;
            output = Some((frame, info));
        }
    }
    check()?;
    output.ok_or_else(|| "Incomplete HEVC still picture".into())
}

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
    let (length_size, parameter_sets) = configuration(record)?;
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
    for ps in &parameter_sets {
        valid_nal(ps)?;
    }
    let mut r = Reader::new(payload);
    let mut count = parameter_sets.len();
    let mut annex_size = parameter_sets.iter().map(|v| v.len() + 4).sum::<usize>();
    while r.left() != 0 {
        codec::check(cancel)?;
        let nal = next_nal(&mut r, length_size)?;
        valid_nal(nal)?;
        count += 1;
        if count > 65536 {
            return Err("HEVC item has too many NAL units".into());
        }
        annex_size = annex_size
            .checked_add(nal.len() + 4)
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
    let mut annex = Vec::new();
    annex.try_reserve_exact(annex_size).map_err(err)?;
    for ps in parameter_sets {
        annex.extend_from_slice(&[0, 0, 0, 1]);
        annex.extend_from_slice(ps);
    }
    let mut r = Reader::new(payload);
    while r.left() != 0 {
        codec::check(cancel)?;
        annex.extend_from_slice(&[0, 0, 0, 1]);
        annex.extend_from_slice(next_nal(&mut r, length_size)?);
    }
    let (frame, info) = decode_still(
        &annex,
        DecoderLimits {
            expected_extent: Some(extent),
            memory_bytes: remaining as usize,
            still_only: true,
        },
        &|| cancel.load(Ordering::Acquire),
    )?;
    if [frame.width, frame.height] != extent
        || frame.bit_depth != info.bit_depth
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
        info.color.map_or(
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
        chroma_location: info.chroma_location as u8,
        premultiplied: false,
        pixels: output,
    })
}
