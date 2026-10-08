use super::Chromaticities;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Edid {
    pub vendor: String,
    pub product: u16,
    pub name: Option<String>,
    pub chromaticities: Chromaticities,
    pub gamma: Option<f32>,
    pub bt2020_signal: bool,
    pub pq_signal: bool,
    pub max_luminance: Option<f32>,
}

const HEADER: [u8; 8] = [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0];
const CTA_EXTENSION: u8 = 0x02;
const EXTENDED_TAG: u8 = 7;
const COLORIMETRY: u8 = 5;
const HDR_STATIC_METADATA: u8 = 6;
const PRODUCT_NAME: u8 = 0xfc;

pub fn parse(bytes: &[u8]) -> Result<Edid, String> {
    let base = bytes.get(..128).ok_or("EDID is shorter than one block")?;
    if base[..8] != HEADER {
        return Err("EDID header is missing".into());
    }
    if !checksum(base) {
        return Err("EDID checksum mismatch".into());
    }
    let id = u16::from_be_bytes([base[8], base[9]]);
    let vendor = [10, 5, 0]
        .map(|shift| char::from(b'@' + ((id >> shift) & 31) as u8))
        .iter()
        .collect();
    let mut edid = Edid {
        vendor,
        product: u16::from_le_bytes([base[10], base[11]]),
        name: None,
        chromaticities: chromaticities(base),
        gamma: (base[23] != 0xff).then(|| (f32::from(base[23]) + 100.) / 100.),
        bt2020_signal: false,
        pq_signal: false,
        max_luminance: None,
    };
    for descriptor in base[54..126].as_chunks::<18>().0 {
        if descriptor[..3] == [0, 0, 0] && descriptor[3] == PRODUCT_NAME {
            edid.name = text(&descriptor[5..]);
        }
    }
    for block in bytes[128..].as_chunks::<128>().0.iter().filter(|&b| b[0] == CTA_EXTENSION && checksum(b)) {
        cta_blocks(block, &mut edid);
    }
    Ok(edid)
}

fn checksum(block: &[u8]) -> bool {
    block.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) == 0
}

fn chromaticities(base: &[u8]) -> Chromaticities {
    let low = |byte: u8, shift: u8| u16::from((byte >> shift) & 3);
    let value = |high: u8, low: u16| f64::from((u16::from(high) << 2) | low) / 1024.;
    let (a, b) = (base[25], base[26]);
    Chromaticities {
        primaries: [
            [value(base[27], low(a, 6)), value(base[28], low(a, 4))],
            [value(base[29], low(a, 2)), value(base[30], low(a, 0))],
            [value(base[31], low(b, 6)), value(base[32], low(b, 4))],
        ],
        white: [value(base[33], low(b, 2)), value(base[34], low(b, 0))],
    }
}

fn text(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|b| *b == b'\n').unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn cta_blocks(block: &[u8], edid: &mut Edid) {
    let end = usize::from(block[2]).clamp(4, 127);
    let mut at = 4;
    while at < end {
        let (tag, len) = (block[at] >> 5, usize::from(block[at] & 31));
        let body = &block[(at + 1).min(end)..(at + 1 + len).min(end)];
        if tag == EXTENDED_TAG && body.len() >= 2 {
            match body[0] {
                COLORIMETRY => edid.bt2020_signal |= body[1] & 0x80 != 0,
                HDR_STATIC_METADATA => {
                    edid.pq_signal |= body[1] & 0x04 != 0;
                    edid.max_luminance = body
                        .get(3)
                        .filter(|v| **v != 0)
                        .map(|v| 50. * 2f32.powf(f32::from(*v) / 32.));
                }
                _ => (),
            }
        }
        at += 1 + len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CINTIQ: &[u8] = include_bytes!("../../tests/fixtures/edid/cintiq-pro-27.bin");
    const LG_TV: &[u8] = include_bytes!("../../tests/fixtures/edid/lg-tv.bin");

    #[test]
    fn wide_gamut_monitor_reports_native_primaries_and_hdr_peak() {
        let edid = parse(CINTIQ).unwrap();
        assert_eq!((edid.vendor.as_str(), edid.product), ("WAC", 0x1082));
        assert_eq!(edid.name.as_deref(), Some("Cintiq Pro 27"));
        let c = edid.chromaticities;
        assert_eq!(c.primaries[1], [0.2021484375, 0.73828125]);
        assert!((c.white[0] - 0.3135).abs() < 0.001);
        assert_eq!(edid.gamma, Some(2.2));
        assert!(edid.bt2020_signal && edid.pq_signal);
        assert_eq!(edid.max_luminance, Some(400.));
    }

    #[test]
    fn television_reports_signal_primaries_without_a_peak() {
        let edid = parse(LG_TV).unwrap();
        assert_eq!(edid.name.as_deref(), Some("LG TV SSCR2"));
        assert!(edid.chromaticities.near(Chromaticities::of(layer_core::color::RgbSpace::Srgb), 0.004));
        assert!(edid.bt2020_signal && edid.pq_signal);
        assert_eq!(edid.max_luminance, None);
    }

    #[test]
    fn rejects_truncated_or_corrupt_data() {
        assert!(parse(&CINTIQ[..100]).is_err());
        let mut corrupt = CINTIQ.to_vec();
        corrupt[30] ^= 1;
        assert!(parse(&corrupt).is_err());
        let mut extension = CINTIQ.to_vec();
        extension[140] ^= 1;
        let edid = parse(&extension).unwrap();
        assert_eq!(edid.max_luminance, None, "a corrupt extension is ignored");
    }
}
