//! Pure Rust H.265 / HEVC video decoder.
//!
//! Decodes Main-profile 8-bit 4:2:0 HEVC bitstreams end-to-end, including
//! I/P/B slices, hierarchical B-frames, WPP, tiles, dependent slice segments,
//! SAO, deblocking, AQ, scaling lists, sign-data hiding, weighted prediction,
//! PCM, transform skip, and transquant bypass. Byte-exact against FFmpeg on
//! all in-tree fixtures plus real 1080p Big Buck Bunny at x265 presets
//! ultrafast / medium / slow.
//!
//! # Quick start
//!
//! ```no_run
//! use rust_h265::{Decoder, Frame, DecodeError, parse_annex_b, NalUnitType};
//!
//! let data = std::fs::read("input.h265").unwrap();
//! let nals = parse_annex_b(&data);
//! let mut decoder = Decoder::new();
//! let mut frames = Vec::new();
//!
//! for nal in &nals {
//!     match decoder.decode_nal(nal) {
//!         Ok(Some(frame)) => frames.push(frame),
//!         Ok(None) => {}
//!         Err(e) => eprintln!("decode error: {e:?}"),
//!     }
//! }
//! while let Some(frame) = decoder.flush() {
//!     frames.push(frame);
//! }
//! // Sort by POC for display order.
//! frames.sort_by_key(|f| f.pic_order_cnt);
//! ```

// --- Internal modules (not part of the public API) ---
// allow(dead_code): internal modules contain utility functions, struct fields,
// and table constants kept for spec completeness / future use.
#[allow(dead_code)]
mod bitstream;
#[allow(dead_code)]
mod cabac;
#[allow(dead_code)]
mod cabac_tables;
#[allow(dead_code)]
mod cu_tree;
#[allow(dead_code)]
mod deblock;
#[allow(dead_code)]
mod inter_pred;
#[allow(dead_code)]
mod intra_pred;
#[allow(dead_code)]
mod inverse_transform;
#[allow(dead_code)]
pub(crate) mod pixel;
#[allow(dead_code)]
mod pps;
#[allow(dead_code)]
mod profile_tier_level;
#[allow(dead_code)]
mod residual_coding;
#[allow(dead_code)]
mod sao;
#[allow(dead_code)]
mod scaling_list;
#[allow(dead_code)]
mod slice;
#[allow(dead_code)]
mod sps;
#[allow(dead_code)]
mod vps;

// --- Internal but needs pub(crate) access from decoder ---
#[allow(dead_code)]
pub(crate) mod dpb;

// --- Public modules ---
pub mod decoder;
pub mod error;
pub mod nal;

// --- Top-level re-exports for convenience ---
pub use decoder::{Decoder, DecoderLimits, Frame, SequenceInfo};
pub use error::DecodeError;
pub use nal::{NalUnit, NalUnitType, parse_annex_b};
pub use pixel::{Pixel, PixelData};

#[cfg(test)]
mod parameter_set_tests {
    use crate::nal::{NalUnitType, parse_annex_b};

    /// Decode VPS+SPS+PPS from a real x265-encoded fixture and assert the
    /// fields a downstream slice/CTU loop will need.
    ///
    /// Fixture command (run from the crate root):
    /// ```text
    /// ffmpeg -hide_banner -loglevel error \
    ///   -f lavfi -i color=gray:size=16x16:rate=30:duration=0.04 \
    ///   -frames:v 1 -pix_fmt yuv420p -f rawvideo testdata/tiny_input.yuv
    /// x265 --input-res 16x16 --fps 30 --frames 1 --keyint 1 --bframes 0 \
    ///   --ctu 16 --input testdata/tiny_input.yuv -o testdata/tiny_intra.h265
    /// ```
    #[test]
    fn parse_tiny_intra_parameter_sets() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/tiny_intra.h265");
        let data = std::fs::read(path).expect("read fixture");
        let nals = parse_annex_b(&data);

        // Find the parameter set NALs. The fixture also contains a PrefixSei
        // (the x265 build banner) and the IDR slice NAL — we don't need them.
        let vps_nal = nals
            .iter()
            .find(|n| n.nal_unit_type == NalUnitType::Vps)
            .expect("VPS NAL");
        let sps_nal = nals
            .iter()
            .find(|n| n.nal_unit_type == NalUnitType::Sps)
            .expect("SPS NAL");
        let pps_nal = nals
            .iter()
            .find(|n| n.nal_unit_type == NalUnitType::Pps)
            .expect("PPS NAL");

        // ---- VPS ----
        let vps = crate::vps::parse_vps(&vps_nal.rbsp).expect("parse VPS");
        assert_eq!(vps.vps_video_parameter_set_id, 0);
        assert_eq!(vps.vps_max_layers_minus1, 0);
        assert_eq!(vps.vps_max_sub_layers_minus1, 0);
        assert!(vps.vps_temporal_id_nesting_flag);
        // Main Still Picture profile, level 1.0
        assert_eq!(vps.profile_tier_level.general_profile_idc, 3);
        assert_eq!(vps.profile_tier_level.general_level_idc, 30);
        assert!(!vps.profile_tier_level.general_tier_flag);

        // ---- SPS ----
        let sps = crate::sps::parse_sps(&sps_nal.rbsp).expect("parse SPS");
        assert_eq!(sps.sps_video_parameter_set_id, 0);
        assert_eq!(sps.sps_seq_parameter_set_id, 0);
        assert_eq!(sps.chroma_format_idc, 1);
        assert_eq!(sps.pic_width_in_luma_samples, 16);
        assert_eq!(sps.pic_height_in_luma_samples, 16);
        assert_eq!(sps.bit_depth_luma, 8);
        assert_eq!(sps.bit_depth_chroma, 8);
        // log2_max_pic_order_cnt_lsb_minus4 = 4 -> MaxPicOrderCntLsb = 256
        assert_eq!(sps.log2_max_pic_order_cnt_lsb, 8);

        // CTU derivation: log2_min_cb_minus3=0, log2_diff=1
        // -> MinCbLog2SizeY=3, CtbLog2SizeY=4, CtbSizeY=16.
        assert_eq!(sps.min_cb_log2_size_y, 3);
        assert_eq!(sps.ctb_log2_size_y, 4);
        assert_eq!(sps.ctb_size_y, 16);
        assert_eq!(sps.pic_width_in_ctbs_y(), 1);
        assert_eq!(sps.pic_height_in_ctbs_y(), 1);

        // TU derivation: min_tb_minus2=0, diff=2 -> 2..=4 (4x4..16x16)
        assert_eq!(sps.min_tb_log2_size_y, 2);
        assert_eq!(sps.max_tb_log2_size_y, 4);

        assert!(!sps.scaling_list_enabled_flag);
        assert!(!sps.amp_enabled_flag);
        assert!(!sps.sample_adaptive_offset_enabled_flag);
        assert!(!sps.pcm_enabled_flag);
        assert_eq!(sps.num_short_term_ref_pic_sets, 0);
        assert!(!sps.long_term_ref_pics_present_flag);
        assert!(sps.sps_temporal_mvp_enabled_flag);
        assert!(!sps.strong_intra_smoothing_enabled_flag);

        // ---- PPS ----
        let pps = crate::pps::parse_pps(&pps_nal.rbsp).expect("parse PPS");
        assert_eq!(pps.pps_pic_parameter_set_id, 0);
        assert_eq!(pps.pps_seq_parameter_set_id, 0);
        assert!(!pps.dependent_slice_segments_enabled_flag);
        assert!(!pps.output_flag_present_flag);
        assert_eq!(pps.num_extra_slice_header_bits, 0);
        assert!(!pps.sign_data_hiding_enabled_flag);
        assert!(!pps.cabac_init_present_flag);
        assert_eq!(pps.num_ref_idx_l0_default_active_minus1, 0);
        assert_eq!(pps.num_ref_idx_l1_default_active_minus1, 0);
        assert_eq!(pps.init_qp, 26);
        assert!(pps.cu_qp_delta_enabled_flag);
        assert_eq!(pps.diff_cu_qp_delta_depth, 0);
        assert_eq!(pps.pps_cb_qp_offset, 0);
        assert_eq!(pps.pps_cr_qp_offset, 0);
        assert!(!pps.weighted_pred_flag);
        assert!(!pps.weighted_bipred_flag);
        assert!(!pps.tiles_enabled_flag);
        assert!(!pps.entropy_coding_sync_enabled_flag);
        assert!(pps.deblocking_filter_control_present_flag);
        assert!(!pps.deblocking_filter_override_enabled_flag);
        assert!(pps.pps_deblocking_filter_disabled_flag);
        assert!(!pps.lists_modification_present_flag);
        assert_eq!(pps.log2_parallel_merge_level_minus2, 0);
        assert!(!pps.slice_segment_header_extension_present_flag);
    }
}
