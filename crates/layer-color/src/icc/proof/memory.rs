//! Conservative admission before parsed/profile stages can multiply allocations.
use super::*;
use moxcms::{LutStore, LutWarehouse, ToneReprCurve};

const SCRATCH_LIMIT: usize = 44 * 1024 * 1024;
const ERROR: &str = "This ICC profile exceeds the proof preparation memory limit; choose a smaller RGB or CMYK profile";

pub(super) fn preflight(profile: &ColorProfile) -> Result<usize, String> {
    let ColorProfile::Icc(bytes) = profile else {
        return Ok(1024 * 1024);
    };
    if bytes.len() > MAX_ICC_BYTES {
        return Err(ERROR.into());
    }
    if bytes.get(8).is_some_and(|major| !matches!(major, 2 | 4)) {
        return Err("Soft proofing supports ICC version 2 and version 4 profiles".into());
    }
    let word = |offset| {
        bytes
            .get(offset..offset + 4)
            .map(|v| u32::from_be_bytes(v.try_into().unwrap()) as usize)
            .ok_or_else(|| "ICC profile header is incomplete".to_string())
    };
    let count = word(128)?;
    if count > 256 {
        return Err(ERROR.into());
    }
    // Count aliases repeatedly: parsers need not share allocations across tags.
    let mut payload = 0usize;
    for i in 0..count {
        payload = payload.checked_add(word(132 + i * 12 + 8)?).ok_or(ERROR)?;
        if payload > 8 * 1024 * 1024 {
            return Err(ERROR.into());
        }
    }
    // Four bytes per encoded byte covers parsed tables, text/curve expansion and
    // parser temporaries. Fixed metadata/evaluator overhead is reserved below.
    Ok(payload * 4)
}

fn count(store: &LutStore) -> usize {
    match store {
        LutStore::Store8(v) => v.len(),
        LutStore::Store16(v) => v.len(),
    }
}
fn curve(curve: &ToneReprCurve) -> usize {
    match curve {
        ToneReprCurve::Lut(v) => v.len() * 4 + 128,
        ToneReprCurve::Parametric(_) => 128,
    }
}
fn stages(lut: &LutWarehouse) -> usize {
    match lut {
        LutWarehouse::Lut(lut) => {
            // Includes the temporary Float32 table before its Float64 evaluator.
            (count(&lut.input_table) + count(&lut.output_table)) * 12
                + count(&lut.clut_table) * 4
                + 1024
        }
        LutWarehouse::Multidimensional(lut) => {
            lut.a_curves
                .iter()
                .chain(&lut.b_curves)
                .chain(&lut.m_curves)
                .map(curve)
                .sum::<usize>()
                + lut.clut.as_ref().map_or(0, |c| count(c) * 4)
                + 1024
        }
    }
}
pub(super) fn validate(
    profile: &Profile,
    parsed: usize,
    intent: RenderingIntent,
) -> Result<(), String> {
    // The source matrix, matrix inverses/TRCs, allocator overhead, quality probes
    // and fixed structures fit the 4 MiB reserve. Matrix inverse evaluators have
    // a fixed 16,384-entry bound in the pinned parser/evaluator implementation.
    let mut bound = parsed + 4 * 1024 * 1024;
    for intent in [
        intent,
        RenderingIntent::RelativeColorimetric,
        RenderingIntent::Perceptual,
    ] {
        let index = match intent {
            RenderingIntent::Perceptual => 0,
            RenderingIntent::RelativeColorimetric | RenderingIntent::AbsoluteColorimetric => 1,
            RenderingIntent::Saturation => 2,
        };
        for tags in [
            [
                &profile.lut_a_to_b_perceptual,
                &profile.lut_a_to_b_colorimetric,
                &profile.lut_a_to_b_saturation,
            ],
            [
                &profile.lut_b_to_a_perceptual,
                &profile.lut_b_to_a_colorimetric,
                &profile.lut_b_to_a_saturation,
            ],
        ] {
            if let Some(lut) = tags[index].as_ref().or(tags[0].as_ref()) {
                bound += stages(lut);
            }
        }
    }
    if bound > SCRATCH_LIMIT {
        Err(ERROR.into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_tag_aliases_cannot_multiply_preparation_memory() {
        let mut bytes = profile_bytes(&ColorProfile::default()).unwrap();
        bytes.resize(132 + 257 * 12, 0);
        bytes[128..132].copy_from_slice(&257u32.to_be_bytes());
        assert!(
            preflight(&ColorProfile::Icc(bytes.into()))
                .unwrap_err()
                .contains("memory limit")
        );
        let mut bytes = vec![0; 156];
        bytes[8] = 4;
        bytes[128..132].copy_from_slice(&2u32.to_be_bytes());
        for offset in [140, 152] {
            bytes[offset..offset + 4].copy_from_slice(&(5u32 * 1024 * 1024).to_be_bytes());
        }
        assert!(
            preflight(&ColorProfile::Icc(bytes.into()))
                .unwrap_err()
                .contains("memory limit")
        );
    }
}
