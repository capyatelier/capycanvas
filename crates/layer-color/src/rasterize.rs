//! Explicit source commitment. Existing paint overrides, masks, placement and
//! full image extent are independent and remain owned by the document layer.
use crate::{OutputStatistics, WorkingDecoder, WorkingEncoder};
use layer_core::color::{ColorProfile, DocumentColor, source::*};

/// Worker-only bounded conversion. A rasterized image uses document RGBA codes;
/// retaining the tiled base avoids allocating/quantizing unchanged paint pages.
/// Cancellation never mutates the supplied original or returns partial content.
pub fn rasterize_source(
    source: &SourceImage,
    color: DocumentColor,
    max_bytes: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<(SourceImage, OutputStatistics), String> {
    let check = |cancelled: &mut dyn FnMut() -> bool| {
        if cancelled() {
            Err("Rasterization cancelled".to_string())
        } else {
            Ok(())
        }
    };
    check(&mut cancelled)?;
    source.validate()?;
    if !source.is_original() {
        return Err("This image is already rasterized".into());
    }
    let target = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: color.depth,
        profile: ColorProfile::Builtin(color.space),
        profile_assumed: false,
    };
    if source.interpretation.channels == target.channels
        && source.interpretation.depth == target.depth
        && source.interpretation.profile == target.profile
    {
        if source.resident_bytes() > max_bytes {
            return Err("Rasterized image exceeds the memory budget".into());
        }
        let mut result = source.clone();
        result.kind = SourceKind::Rasterized;
        result.interpretation.profile_assumed = false;
        check(&mut cancelled)?;
        return Ok((result, Default::default()));
    }
    let decoder = WorkingDecoder::new(&source.interpretation, color.space, Default::default())?;
    let encoder = WorkingEncoder::new(color.space, &target, Default::default())?;
    let mut builder = SourceBuilder::new(source.extent, target.clone(), max_bytes)?;
    let mut rows = source.rows();
    let mut original = vec![0; source.row_bytes()];
    let mut linear = vec![[0.; 4]; source.extent[0] as usize];
    let mut encoded = vec![0; source.extent[0] as usize * target.pixel_bytes()];
    let mut statistics = OutputStatistics::default();
    for y in 0..source.extent[1] {
        check(&mut cancelled)?;
        rows.read(y, &mut original)?;
        decoder.decode_pixels(&original, &mut linear)?;
        statistics.clipped_channels += encoder
            .encode_straight(&linear, &mut encoded, None, [0, y])?
            .clipped_channels;
        builder.push_row(&encoded)?;
    }
    check(&mut cancelled)?;
    let mut result = builder.finish()?;
    result.kind = SourceKind::Rasterized;
    result.validate()?;
    Ok((result, statistics))
}

#[cfg(test)]
mod tests;
