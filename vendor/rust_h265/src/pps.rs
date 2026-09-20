//! HEVC Picture Parameter Set parsing (spec 7.3.2.3, semantics 7.4.3.3).
//!
//! For Phase 1 we extract the PPS fields needed by future slice-header parsing
//! and reject the more complex features (tiles, WPP, scaling lists, extensions)
//! with a clear `Unsupported` error.

use crate::bitstream::BitstreamReader;
use crate::error::DecodeError;
use crate::scaling_list::{ScalingList, parse_scaling_list_data};
use crate::sps::Sps;

#[derive(Debug, Clone)]
pub struct Pps {
    pub pps_pic_parameter_set_id: u32,
    pub pps_seq_parameter_set_id: u32,
    pub dependent_slice_segments_enabled_flag: bool,
    pub output_flag_present_flag: bool,
    pub num_extra_slice_header_bits: u8,
    pub sign_data_hiding_enabled_flag: bool,
    pub cabac_init_present_flag: bool,
    pub num_ref_idx_l0_default_active_minus1: u32,
    pub num_ref_idx_l1_default_active_minus1: u32,
    /// `init_qp = init_qp_minus26 + 26` (spec 7.4.3.3.1).
    pub init_qp: i32,
    pub constrained_intra_pred_flag: bool,
    pub transform_skip_enabled_flag: bool,
    pub cu_qp_delta_enabled_flag: bool,
    pub diff_cu_qp_delta_depth: u32,
    pub pps_cb_qp_offset: i32,
    pub pps_cr_qp_offset: i32,
    pub pps_slice_chroma_qp_offsets_present_flag: bool,
    pub weighted_pred_flag: bool,
    pub weighted_bipred_flag: bool,
    pub transquant_bypass_enabled_flag: bool,
    pub tiles_enabled_flag: bool,
    pub entropy_coding_sync_enabled_flag: bool,
    /// Number of tile columns in the picture. Always ≥ 1; equals 1 when
    /// `tiles_enabled_flag` is 0.
    pub num_tile_columns: usize,
    /// Number of tile rows in the picture. Always ≥ 1; equals 1 when
    /// `tiles_enabled_flag` is 0.
    pub num_tile_rows: usize,
    /// True if tile columns / rows are uniformly spaced (spec default).
    pub uniform_spacing_flag: bool,
    /// Raw `column_width_minus1[i]` values for `i < num_tile_columns - 1`.
    /// Only meaningful when `uniform_spacing_flag = 0`. The actual per-column
    /// widths in CTBs are derived by [`Pps::resolve_tile_geometry`] once the
    /// SPS is available.
    pub column_width_minus1: Vec<u32>,
    /// Raw `row_height_minus1[i]` values for `i < num_tile_rows - 1`.
    pub row_height_minus1: Vec<u32>,
    /// Whether loop filtering (deblock + SAO) is allowed to cross tile
    /// boundaries. Defaults to `true` (the spec default when the PPS does not
    /// explicitly set it, i.e. when `tiles_enabled_flag = 0`).
    pub loop_filter_across_tiles_enabled_flag: bool,
    /// Per-tile-column CTB width (in CTBs). Empty until
    /// [`Pps::resolve_tile_geometry`] has been called with the active SPS.
    /// When populated, `column_widths_in_ctbs.iter().sum()` equals
    /// `pic_width_in_ctbs_y`.
    pub column_widths_in_ctbs: Vec<u32>,
    /// Per-tile-row CTB height (in CTBs). Same lifecycle as
    /// `column_widths_in_ctbs`.
    pub row_heights_in_ctbs: Vec<u32>,
    pub pps_loop_filter_across_slices_enabled_flag: bool,
    pub deblocking_filter_control_present_flag: bool,
    pub deblocking_filter_override_enabled_flag: bool,
    pub pps_deblocking_filter_disabled_flag: bool,
    pub pps_beta_offset_div2: i32,
    pub pps_tc_offset_div2: i32,
    pub pps_scaling_list_data_present_flag: bool,
    /// PPS-level scaling list override. When present, takes priority over SPS scaling list.
    pub scaling_list: Option<ScalingList>,
    pub lists_modification_present_flag: bool,
    pub log2_parallel_merge_level_minus2: u32,
    pub slice_segment_header_extension_present_flag: bool,
    /// `chroma_qp_offset_list_enabled_flag` from PPS Range Extension.
    /// In Main Profile this is always false (range extensions not parsed).
    /// When true, the slice header may signal `cu_chroma_qp_offset_enabled_flag`.
    pub chroma_qp_offset_list_enabled_flag: bool,
    /// `chroma_qp_offset_list_len_minus1` from PPS Range Extension (0..5).
    pub chroma_qp_offset_list_len_minus1: u32,
    /// Per-index Cb QP offset list (up to 6 entries).
    pub cb_qp_offset_list: [i32; 6],
    /// Per-index Cr QP offset list (up to 6 entries).
    pub cr_qp_offset_list: [i32; 6],
}

impl Pps {
    /// Derive `column_widths_in_ctbs` / `row_heights_in_ctbs` from the raw
    /// PPS tile syntax plus the active SPS. Idempotent — safe to call more
    /// than once. Must be called before any tile-scan address mapping is
    /// attempted (spec 6.5.1).
    ///
    /// For `tiles_enabled_flag = 0` this produces a single-tile layout:
    /// `column_widths_in_ctbs = [pic_width_in_ctbs]`,
    /// `row_heights_in_ctbs = [pic_height_in_ctbs]`.
    pub fn resolve_tile_geometry(&mut self, sps: &Sps) -> Result<(), DecodeError> {
        let pic_w_ctbs = sps.pic_width_in_ctbs_y() as usize;
        let pic_h_ctbs = sps.pic_height_in_ctbs_y() as usize;

        if !self.tiles_enabled_flag {
            self.column_widths_in_ctbs = vec![pic_w_ctbs as u32];
            self.row_heights_in_ctbs = vec![pic_h_ctbs as u32];
            return Ok(());
        }

        if self.num_tile_columns == 0 || self.num_tile_rows == 0 {
            return Err(DecodeError::InvalidSyntax("num_tile_{columns,rows} == 0"));
        }
        if self.num_tile_columns > pic_w_ctbs {
            return Err(DecodeError::InvalidSyntax(
                "num_tile_columns exceeds pic_width_in_ctbs",
            ));
        }
        if self.num_tile_rows > pic_h_ctbs {
            return Err(DecodeError::InvalidSyntax(
                "num_tile_rows exceeds pic_height_in_ctbs",
            ));
        }

        let mut col_w = vec![0u32; self.num_tile_columns];
        let mut row_h = vec![0u32; self.num_tile_rows];
        if self.uniform_spacing_flag {
            // Spec eq. 6-4: uniform spacing formula.
            let n_cols = self.num_tile_columns;
            for (i, w_i) in col_w.iter_mut().enumerate() {
                *w_i = (((i + 1) * pic_w_ctbs) / n_cols - (i * pic_w_ctbs) / n_cols) as u32;
            }
            let n_rows = self.num_tile_rows;
            for (i, h_i) in row_h.iter_mut().enumerate() {
                *h_i = (((i + 1) * pic_h_ctbs) / n_rows - (i * pic_h_ctbs) / n_rows) as u32;
            }
        } else {
            // Non-uniform: the first N-1 columns/rows come from the raw
            // `column_width_minus1` / `row_height_minus1` arrays, and the
            // last one absorbs whatever's left so the totals match.
            let mut sum = 0u32;
            let last_col = self.num_tile_columns - 1;
            for (w_i, raw) in col_w[..last_col]
                .iter_mut()
                .zip(self.column_width_minus1.iter())
            {
                *w_i = *raw + 1;
                sum += *w_i;
            }
            if sum >= pic_w_ctbs as u32 {
                return Err(DecodeError::InvalidSyntax(
                    "tile column widths sum too large",
                ));
            }
            col_w[last_col] = pic_w_ctbs as u32 - sum;

            let mut sum = 0u32;
            let last_row = self.num_tile_rows - 1;
            for (h_i, raw) in row_h[..last_row]
                .iter_mut()
                .zip(self.row_height_minus1.iter())
            {
                *h_i = *raw + 1;
                sum += *h_i;
            }
            if sum >= pic_h_ctbs as u32 {
                return Err(DecodeError::InvalidSyntax("tile row heights sum too large"));
            }
            row_h[last_row] = pic_h_ctbs as u32 - sum;
        }

        self.column_widths_in_ctbs = col_w;
        self.row_heights_in_ctbs = row_h;
        Ok(())
    }
}

pub fn parse_pps(rbsp: &[u8]) -> Result<Pps, DecodeError> {
    let mut r = BitstreamReader::new(rbsp);

    let pps_pic_parameter_set_id = r.read_ue()?;
    let pps_seq_parameter_set_id = r.read_ue()?;
    if pps_pic_parameter_set_id > 63 || pps_seq_parameter_set_id > 15 {
        return Err(DecodeError::InvalidSyntax("HEVC PPS id out of range"));
    }
    let dependent_slice_segments_enabled_flag = r.read_bit()? == 1;
    let output_flag_present_flag = r.read_bit()? == 1;
    let num_extra_slice_header_bits = r.read_bits(3)? as u8;
    let sign_data_hiding_enabled_flag = r.read_bit()? == 1;
    let cabac_init_present_flag = r.read_bit()? == 1;
    let num_ref_idx_l0_default_active_minus1 = r.read_ue()?;
    let num_ref_idx_l1_default_active_minus1 = r.read_ue()?;
    if num_ref_idx_l0_default_active_minus1 > 14 || num_ref_idx_l1_default_active_minus1 > 14 {
        return Err(DecodeError::InvalidSyntax("HEVC default reference count out of range"));
    }
    let init_qp_minus26 = r.read_se()?;
    let init_qp = init_qp_minus26 + 26;
    let constrained_intra_pred_flag = r.read_bit()? == 1;
    let transform_skip_enabled_flag = r.read_bit()? == 1;

    let cu_qp_delta_enabled_flag = r.read_bit()? == 1;
    let diff_cu_qp_delta_depth = if cu_qp_delta_enabled_flag {
        r.read_ue()?
    } else {
        0
    };
    let pps_cb_qp_offset = r.read_se()?;
    let pps_cr_qp_offset = r.read_se()?;
    let pps_slice_chroma_qp_offsets_present_flag = r.read_bit()? == 1;
    let weighted_pred_flag = r.read_bit()? == 1;
    let weighted_bipred_flag = r.read_bit()? == 1;
    let transquant_bypass_enabled_flag = r.read_bit()? == 1;

    let tiles_enabled_flag = r.read_bit()? == 1;
    let entropy_coding_sync_enabled_flag = r.read_bit()? == 1;
    // `entropy_coding_sync_enabled_flag = 1` (WPP) is accepted and wired up in
    // `Decoder::decode_slice` as of Phase 3c-3. Sequential decode only — we
    // still decode rows one after another, but with the spec's per-row CABAC
    // reinit + state propagation.

    // Phase 3c-2: tile fields. We can only parse the raw syntax here; the
    // per-column/per-row widths in CTBs depend on the SPS `ctb_size_y` which
    // isn't available at PPS-parse time. The derivation happens lazily in
    // `Pps::resolve_tile_geometry` once the active SPS is known.
    let mut num_tile_columns: usize = 1;
    let mut num_tile_rows: usize = 1;
    let mut uniform_spacing_flag = true;
    let mut column_width_minus1: Vec<u32> = Vec::new();
    let mut row_height_minus1: Vec<u32> = Vec::new();
    // Spec default when `tiles_enabled_flag = 0`: loop filtering across tile
    // boundaries is allowed (vacuously, since there are no tile boundaries).
    let mut loop_filter_across_tiles_enabled_flag = true;
    if tiles_enabled_flag {
        let num_tile_columns_minus1 = r.read_ue()?;
        let num_tile_rows_minus1 = r.read_ue()?;
        if num_tile_columns_minus1 >= 20 || num_tile_rows_minus1 >= 22 {
            return Err(DecodeError::InvalidSyntax("HEVC tile count out of range"));
        }
        num_tile_columns = (num_tile_columns_minus1 + 1) as usize;
        num_tile_rows = (num_tile_rows_minus1 + 1) as usize;
        uniform_spacing_flag = r.read_bit()? == 1;
        if !uniform_spacing_flag {
            column_width_minus1.reserve(num_tile_columns.saturating_sub(1));
            for _ in 0..num_tile_columns.saturating_sub(1) {
                column_width_minus1.push(r.read_ue()?);
            }
            row_height_minus1.reserve(num_tile_rows.saturating_sub(1));
            for _ in 0..num_tile_rows.saturating_sub(1) {
                row_height_minus1.push(r.read_ue()?);
            }
        }
        loop_filter_across_tiles_enabled_flag = r.read_bit()? == 1;
    }

    let pps_loop_filter_across_slices_enabled_flag = r.read_bit()? == 1;

    let deblocking_filter_control_present_flag = r.read_bit()? == 1;
    let mut deblocking_filter_override_enabled_flag = false;
    let mut pps_deblocking_filter_disabled_flag = false;
    let mut pps_beta_offset_div2 = 0;
    let mut pps_tc_offset_div2 = 0;
    if deblocking_filter_control_present_flag {
        deblocking_filter_override_enabled_flag = r.read_bit()? == 1;
        pps_deblocking_filter_disabled_flag = r.read_bit()? == 1;
        if !pps_deblocking_filter_disabled_flag {
            pps_beta_offset_div2 = r.read_se()?;
            pps_tc_offset_div2 = r.read_se()?;
        }
    }

    let pps_scaling_list_data_present_flag = r.read_bit()? == 1;
    let pps_scaling_list = if pps_scaling_list_data_present_flag {
        let mut sl = ScalingList::default_scaling_list();
        parse_scaling_list_data(&mut r, &mut sl)?;
        Some(sl)
    } else {
        None
    };

    let lists_modification_present_flag = r.read_bit()? == 1;
    let log2_parallel_merge_level_minus2 = r.read_ue()?;
    let slice_segment_header_extension_present_flag = r.read_bit()? == 1;

    // pps_extension_present_flag and any extensions follow — Phase 1 stops
    // here. The next NAL start code bounds the PPS.
    let _pps_extension_present_flag = r.read_bit()?;

    Ok(Pps {
        pps_pic_parameter_set_id,
        pps_seq_parameter_set_id,
        dependent_slice_segments_enabled_flag,
        output_flag_present_flag,
        num_extra_slice_header_bits,
        sign_data_hiding_enabled_flag,
        cabac_init_present_flag,
        num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_default_active_minus1,
        init_qp,
        constrained_intra_pred_flag,
        transform_skip_enabled_flag,
        cu_qp_delta_enabled_flag,
        diff_cu_qp_delta_depth,
        pps_cb_qp_offset,
        pps_cr_qp_offset,
        pps_slice_chroma_qp_offsets_present_flag,
        weighted_pred_flag,
        weighted_bipred_flag,
        transquant_bypass_enabled_flag,
        tiles_enabled_flag,
        entropy_coding_sync_enabled_flag,
        num_tile_columns,
        num_tile_rows,
        uniform_spacing_flag,
        column_width_minus1,
        row_height_minus1,
        loop_filter_across_tiles_enabled_flag,
        column_widths_in_ctbs: Vec::new(),
        row_heights_in_ctbs: Vec::new(),
        pps_loop_filter_across_slices_enabled_flag,
        deblocking_filter_control_present_flag,
        deblocking_filter_override_enabled_flag,
        pps_deblocking_filter_disabled_flag,
        pps_beta_offset_div2,
        pps_tc_offset_div2,
        pps_scaling_list_data_present_flag,
        scaling_list: pps_scaling_list,
        lists_modification_present_flag,
        log2_parallel_merge_level_minus2,
        slice_segment_header_extension_present_flag,
        chroma_qp_offset_list_enabled_flag: false,
        chroma_qp_offset_list_len_minus1: 0,
        cb_qp_offset_list: [0i32; 6],
        cr_qp_offset_list: [0i32; 6],
    })
}
