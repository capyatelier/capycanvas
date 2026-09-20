//! Annex B NAL unit parsing for HEVC.
//!
//! HEVC NAL header is 2 bytes (vs. 1 byte in H.264):
//!
//! ```text
//!   bit  0       1               7 8           13 14                  15
//!       +-+-------------------------+-------------+----------------------+
//!       |F|     nal_unit_type       | nuh_layer_id| nuh_temporal_id_plus1|
//!       +-+-------------------------+-------------+----------------------+
//!         (6 bits)                   (6 bits)      (3 bits)
//! ```
//!
//! - `forbidden_zero_bit` (F): must be 0.
//! - `nal_unit_type`: 6 bits, values 0..63 (HEVC spec table 7-1).
//! - `nuh_layer_id`: 6 bits. 0 for base layer; non-zero only for SHVC/MV-HEVC.
//! - `nuh_temporal_id_plus1`: 3 bits, must be > 0. `temporal_id = value - 1`.

use std::borrow::Cow;

/// HEVC NAL unit type (HEVC spec table 7-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalUnitType {
    // VCL (slice) NAL units
    TrailN,   // 0
    TrailR,   // 1
    TsaN,     // 2
    TsaR,     // 3
    StsaN,    // 4
    StsaR,    // 5
    RadlN,    // 6
    RadlR,    // 7
    RaslN,    // 8
    RaslR,    // 9
    BlaWLp,   // 16
    BlaWRadl, // 17
    BlaNLp,   // 18
    IdrWRadl, // 19
    IdrNLp,   // 20
    Cra,      // 21
    // Non-VCL NAL units
    Vps,       // 32
    Sps,       // 33
    Pps,       // 34
    Aud,       // 35
    Eos,       // 36
    Eob,       // 37
    Fd,        // 38
    PrefixSei, // 39
    SuffixSei, // 40
    Other(u8),
}

impl NalUnitType {
    /// True for slice NAL types (VCL: 0..31). The values 22..23 (RSV_IRAP_VCL)
    /// and 24..31 (RSV_VCL) are also slice types but reserved.
    pub fn is_vcl(self) -> bool {
        match self {
            NalUnitType::TrailN
            | NalUnitType::TrailR
            | NalUnitType::TsaN
            | NalUnitType::TsaR
            | NalUnitType::StsaN
            | NalUnitType::StsaR
            | NalUnitType::RadlN
            | NalUnitType::RadlR
            | NalUnitType::RaslN
            | NalUnitType::RaslR
            | NalUnitType::BlaWLp
            | NalUnitType::BlaWRadl
            | NalUnitType::BlaNLp
            | NalUnitType::IdrWRadl
            | NalUnitType::IdrNLp
            | NalUnitType::Cra => true,
            NalUnitType::Other(v) => v < 32,
            _ => false,
        }
    }

    /// True for IDR pictures (types 19, 20).
    pub fn is_idr(self) -> bool {
        matches!(self, NalUnitType::IdrWRadl | NalUnitType::IdrNLp)
    }

    /// True for IRAP (Intra Random Access Point) pictures: BLA, IDR, CRA.
    /// These are the only NAL types where decoding can start cleanly.
    pub fn is_irap(self) -> bool {
        matches!(
            self,
            NalUnitType::BlaWLp
                | NalUnitType::BlaWRadl
                | NalUnitType::BlaNLp
                | NalUnitType::IdrWRadl
                | NalUnitType::IdrNLp
                | NalUnitType::Cra
        )
    }
}

impl From<u8> for NalUnitType {
    fn from(val: u8) -> Self {
        match val {
            0 => NalUnitType::TrailN,
            1 => NalUnitType::TrailR,
            2 => NalUnitType::TsaN,
            3 => NalUnitType::TsaR,
            4 => NalUnitType::StsaN,
            5 => NalUnitType::StsaR,
            6 => NalUnitType::RadlN,
            7 => NalUnitType::RadlR,
            8 => NalUnitType::RaslN,
            9 => NalUnitType::RaslR,
            16 => NalUnitType::BlaWLp,
            17 => NalUnitType::BlaWRadl,
            18 => NalUnitType::BlaNLp,
            19 => NalUnitType::IdrWRadl,
            20 => NalUnitType::IdrNLp,
            21 => NalUnitType::Cra,
            32 => NalUnitType::Vps,
            33 => NalUnitType::Sps,
            34 => NalUnitType::Pps,
            35 => NalUnitType::Aud,
            36 => NalUnitType::Eos,
            37 => NalUnitType::Eob,
            38 => NalUnitType::Fd,
            39 => NalUnitType::PrefixSei,
            40 => NalUnitType::SuffixSei,
            v => NalUnitType::Other(v),
        }
    }
}

#[derive(Debug)]
pub struct NalUnit<'a> {
    pub nal_unit_type: NalUnitType,
    pub nuh_layer_id: u8,
    /// `temporal_id` derived as `nuh_temporal_id_plus1 - 1`. Always 0..6.
    pub temporal_id: u8,
    /// RBSP payload (the bytes after the 2-byte NAL header, with emulation
    /// prevention bytes removed). Borrowed from the input when no EPB are
    /// present, owned otherwise.
    pub rbsp: Cow<'a, [u8]>,
    /// NAL-space byte positions (relative to the NAL payload start — i.e.
    /// after the 2-byte NAL header) at which an emulation prevention byte
    /// (0x03) was removed. Empty when no EPBs were present. HEVC spec
    /// 7.4.7.1 `entry_point_offset_minus1[]` is defined in NAL-space units
    /// (it counts EPBs); consumers that index into `rbsp` using those
    /// offsets must subtract the count of EPBs whose position is ≤ the
    /// target NAL offset to convert NAL → RBSP addresses.
    pub epb_positions: Vec<u32>,
}

/// Split an Annex B bytestream into NAL units.
/// Handles both 3-byte (`00 00 01`) and 4-byte (`00 00 00 01`) start codes.
pub fn parse_annex_b(data: &[u8]) -> Vec<NalUnit<'_>> {
    let mut nals = Vec::new();
    // Find first start code
    let mut i = match find_start_code(data, 0) {
        Some((pos, _)) => pos,
        None => return nals,
    };

    loop {
        // i points to first byte after start code (the NAL header byte 0)
        if i >= data.len() {
            break;
        }

        // Find the next start code to determine where this NAL ends
        let nal_end = match find_start_code(data, i) {
            Some((pos, sc_start)) => {
                let end = sc_start;
                // Strip trailing zeros before the start code
                let mut e = end;
                while e > i && data[e - 1] == 0 {
                    e -= 1;
                }
                (e, Some(pos))
            }
            None => (data.len(), None),
        };

        let nal_data = &data[i..nal_end.0];
        // HEVC NAL header is 2 bytes; need at least that.
        if nal_data.len() >= 2 {
            let b0 = nal_data[0];
            let b1 = nal_data[1];
            // forbidden_zero_bit (MSB of byte 0) must be 0
            if b0 & 0x80 == 0 {
                let nal_unit_type = NalUnitType::from((b0 >> 1) & 0x3F);
                // nuh_layer_id is 6 bits split across the two header bytes:
                // 1 LSB of byte 0 (= MSB of layer_id) + top 5 bits of byte 1
                let nuh_layer_id = ((b0 & 0x01) << 5) | (b1 >> 3);
                let temporal_id_plus1 = b1 & 0x07;
                // nuh_temporal_id_plus1 must be > 0 (spec 7.4.2.2). Skip
                // malformed headers rather than panicking.
                if temporal_id_plus1 > 0 {
                    let temporal_id = temporal_id_plus1 - 1;
                    let (rbsp, epb_positions) = remove_emulation_prevention(&nal_data[2..]);
                    nals.push(NalUnit {
                        nal_unit_type,
                        nuh_layer_id,
                        temporal_id,
                        epb_positions,
                        rbsp,
                    });
                }
            }
        }

        match nal_end.1 {
            Some(pos) => i = pos,
            None => break,
        }
    }

    nals
}

/// Find the next start code starting from `offset`.
/// Returns (position after start code, position of start code beginning).
fn find_start_code(data: &[u8], offset: usize) -> Option<(usize, usize)> {
    let mut i = offset;
    while i + 2 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                return Some((i + 3, i));
            }
            if i + 3 < data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                return Some((i + 4, i));
            }
        }
        i += 1;
    }
    None
}

/// Remove emulation prevention bytes (0x03 in `00 00 03` sequences).
/// Returns a borrowed slice when no emulation prevention bytes are found
/// (the common case), avoiding allocation entirely. Also returns the
/// NAL-space byte positions where each EPB was located (empty on the
/// fast path).
fn remove_emulation_prevention(data: &[u8]) -> (Cow<'_, [u8]>, Vec<u32>) {
    // Fast path: scan for 00 00 03. If none found, return borrowed slice.
    let has_epb = data.windows(3).any(|w| w[0] == 0 && w[1] == 0 && w[2] == 3);
    if !has_epb {
        return (Cow::Borrowed(data), Vec::new());
    }

    // Slow path: copy with emulation prevention removal.
    let mut rbsp = Vec::with_capacity(data.len());
    let mut epb_positions = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if i + 2 < data.len() && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 3 {
            rbsp.push(0);
            rbsp.push(0);
            // Record the NAL-space position of the removed 0x03 byte.
            epb_positions.push((i + 2) as u32);
            i += 3; // skip the 0x03 byte
        } else {
            rbsp.push(data[i]);
            i += 1;
        }
    }
    (Cow::Owned(rbsp), epb_positions)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a 2-byte HEVC NAL header for the given type / layer / temporal_id.
    fn hdr(nut: u8, layer_id: u8, temporal_id: u8) -> [u8; 2] {
        let b0 = (nut & 0x3F) << 1 | ((layer_id >> 5) & 0x01);
        let b1 = ((layer_id & 0x1F) << 3) | ((temporal_id + 1) & 0x07);
        [b0, b1]
    }

    #[test]
    fn test_header_packing_round_trip() {
        // VPS, layer 0, temporal_id 0
        let h = hdr(32, 0, 0);
        assert_eq!(h, [0x40, 0x01]);

        // SPS, layer 0, temporal_id 0
        let h = hdr(33, 0, 0);
        assert_eq!(h, [0x42, 0x01]);

        // IDR_W_RADL = 19, layer 0, temporal_id 0
        let h = hdr(19, 0, 0);
        assert_eq!(h, [0x26, 0x01]);
    }

    #[test]
    fn test_parse_annex_b_mixed_start_codes() {
        // Build a stream with one VPS (4-byte start code) and one SPS
        // (3-byte start code), each with a small payload.
        let mut data: Vec<u8> = Vec::new();
        // VPS NAL: 4-byte start code, header, payload
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        data.extend_from_slice(&hdr(32, 0, 0));
        data.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        // SPS NAL: 3-byte start code, header, payload
        data.extend_from_slice(&[0x00, 0x00, 0x01]);
        data.extend_from_slice(&hdr(33, 0, 0));
        data.extend_from_slice(&[0xDD, 0xEE]);

        let nals = parse_annex_b(&data);
        assert_eq!(nals.len(), 2);

        assert_eq!(nals[0].nal_unit_type, NalUnitType::Vps);
        assert_eq!(nals[0].nuh_layer_id, 0);
        assert_eq!(nals[0].temporal_id, 0);
        assert_eq!(&*nals[0].rbsp, &[0xAA, 0xBB, 0xCC]);

        assert_eq!(nals[1].nal_unit_type, NalUnitType::Sps);
        assert_eq!(nals[1].nuh_layer_id, 0);
        assert_eq!(nals[1].temporal_id, 0);
        assert_eq!(&*nals[1].rbsp, &[0xDD, 0xEE]);
    }

    #[test]
    fn test_parse_annex_b_idr_with_temporal_id() {
        // IDR_W_RADL with temporal_id=2 should round-trip correctly.
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        data.extend_from_slice(&hdr(19, 0, 2));
        data.extend_from_slice(&[0x12, 0x34]);

        let nals = parse_annex_b(&data);
        assert_eq!(nals.len(), 1);
        assert_eq!(nals[0].nal_unit_type, NalUnitType::IdrWRadl);
        assert!(nals[0].nal_unit_type.is_idr());
        assert!(nals[0].nal_unit_type.is_irap());
        assert!(nals[0].nal_unit_type.is_vcl());
        assert_eq!(nals[0].temporal_id, 2);
    }

    #[test]
    fn test_parse_annex_b_strips_emulation_prevention() {
        // Build a NAL whose RBSP contains the EPB sequence 00 00 03 -> 00 00.
        // Use a non-zero header so the 00 00 in the payload doesn't accidentally
        // match a start code.
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        data.extend_from_slice(&hdr(33, 0, 0)); // SPS, [0x42, 0x01]
        // Payload: 0x00 0x00 0x03 0x42 0xAA  →  RBSP: 0x00 0x00 0x42 0xAA
        data.extend_from_slice(&[0x00, 0x00, 0x03, 0x42, 0xAA]);

        let nals = parse_annex_b(&data);
        assert_eq!(nals.len(), 1);
        assert_eq!(nals[0].nal_unit_type, NalUnitType::Sps);
        assert_eq!(&*nals[0].rbsp, &[0x00, 0x00, 0x42, 0xAA]);
        assert!(
            matches!(nals[0].rbsp, Cow::Owned(_)),
            "EPB removal should produce an owned buffer"
        );
    }

    #[test]
    fn test_parse_annex_b_zero_copy_when_no_epb() {
        // No emulation prevention bytes → rbsp should borrow from input.
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        data.extend_from_slice(&hdr(33, 0, 0));
        data.extend_from_slice(&[0x11, 0x22, 0x33]);

        let nals = parse_annex_b(&data);
        assert_eq!(nals.len(), 1);
        assert!(
            matches!(nals[0].rbsp, Cow::Borrowed(_)),
            "no EPB → should borrow"
        );
    }

    #[test]
    fn test_parse_annex_b_skips_invalid_forbidden_bit() {
        // forbidden_zero_bit = 1 → NAL must be skipped.
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        // First NAL: forbidden bit set, should be ignored
        data.extend_from_slice(&[0x80, 0x01, 0xAA]);
        data.extend_from_slice(&[0x00, 0x00, 0x01]);
        // Second NAL: valid VPS
        data.extend_from_slice(&hdr(32, 0, 0));
        data.extend_from_slice(&[0xBB]);

        let nals = parse_annex_b(&data);
        assert_eq!(nals.len(), 1);
        assert_eq!(nals[0].nal_unit_type, NalUnitType::Vps);
    }

    #[test]
    fn test_nuh_layer_id_packing() {
        // Layer ID 33 = 0b100001 → MSB into byte0 LSB, 5 LSBs into byte1 high.
        let h = hdr(33, 33, 0);
        // byte0 = (33 << 1) | 1 = 0x42 | 1 = 0x43
        // byte1 = (33 & 0x1F) << 3 | 1 = 1 << 3 | 1 = 0x09
        assert_eq!(h, [0x43, 0x09]);

        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        data.extend_from_slice(&h);
        data.extend_from_slice(&[0xAA]);

        let nals = parse_annex_b(&data);
        assert_eq!(nals.len(), 1);
        assert_eq!(nals[0].nuh_layer_id, 33);
    }
}
