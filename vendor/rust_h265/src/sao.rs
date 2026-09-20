//! HEVC Sample Adaptive Offset filter (spec 8.7.3).
//!
//! SAO is the second in-loop filter, applied AFTER deblocking. It corrects
//! banding and ringing artifacts via per-CTB classification + signed offset.
//!
//! Two modes per CTB per plane (luma + Cb + Cr):
//!   - **Band offset (BO)**: 4 contiguous bands of 8 intensity levels each
//!     (32 bands total). Each of the 4 bands gets a signed offset that's
//!     added to all pixels falling in that band.
//!   - **Edge offset (EO)**: per-pixel classification into 5 categories
//!     (0..4) based on neighbor comparison along one of 4 directions
//!     (horizontal, vertical, 45° diagonal, 135° diagonal). Categories
//!     1..4 get a signed offset (category 0 is "no edge", no offset).
//!
//! Reference: FFmpeg `libavcodec/hevc/hevcdec.c::hls_sao_param`,
//! `libavcodec/hevc/cabac.c::ff_hevc_sao_*`,
//! `libavcodec/h26x/h2656_sao_template.c::sao_band_filter` /
//! `sao_edge_filter`.

use crate::cabac::{CabacContexts, CabacReader};
use crate::cabac_tables::ctx;
use crate::cu_tree::PictureState;
use crate::pixel::Pixel;
use crate::slice::SliceHeader;
use crate::sps::Sps;

/// Per-CTB SAO type per plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaoType {
    #[default]
    NotApplied,
    Band,
    Edge,
}

/// Per-CTB SAO parameters for one CTB (covers all 3 planes for 4:2:0).
#[derive(Debug, Clone, Default)]
pub struct SaoParams {
    /// Per-plane filter type (Y, Cb, Cr).
    pub type_idx: [SaoType; 3],
    /// Per-plane signed offset values, indexed by category (cat 0 → 0).
    pub offset_val: [[i16; 5]; 3],
    /// Per-plane edge offset class (only used when `type_idx == Edge`).
    pub eo_class: [u8; 3],
    /// Per-plane band offset starting band (only used when `type_idx == Band`).
    pub band_position: [u8; 3],
}

/// EO class — direction of neighbor comparison.
const SAO_EO_HORIZ: u8 = 0;
const SAO_EO_VERT: u8 = 1;
const SAO_EO_135D: u8 = 2;
const SAO_EO_45D: u8 = 3;

// ---- CABAC SAO syntax decoders ----

fn decode_sao_merge_flag(cabac: &mut CabacReader, contexts: &mut CabacContexts) -> u32 {
    cabac.decode_bin(&mut contexts.state[ctx::SAO_MERGE_FLAG])
}

fn decode_sao_type_idx(cabac: &mut CabacReader, contexts: &mut CabacContexts) -> SaoType {
    if cabac.decode_bin(&mut contexts.state[ctx::SAO_TYPE_IDX]) == 0 {
        SaoType::NotApplied
    } else if cabac.decode_bypass() == 0 {
        SaoType::Band
    } else {
        SaoType::Edge
    }
}

fn decode_sao_offset_abs(cabac: &mut CabacReader, bit_depth: u8) -> u32 {
    // Truncated unary, max value `(1 << min(bit_depth, 10) - 5) - 1` = 7 for 8-bit.
    let length = (1u32 << (bit_depth.min(10) - 5)) - 1;
    let mut i = 0u32;
    while i < length && cabac.decode_bypass() != 0 {
        i += 1;
    }
    i
}

fn decode_sao_offset_sign(cabac: &mut CabacReader) -> u32 {
    cabac.decode_bypass()
}

fn decode_sao_band_position(cabac: &mut CabacReader) -> u32 {
    let mut value = cabac.decode_bypass();
    for _ in 0..4 {
        value = (value << 1) | cabac.decode_bypass();
    }
    value
}

fn decode_sao_eo_class(cabac: &mut CabacReader) -> u32 {
    let hi = cabac.decode_bypass();
    let lo = cabac.decode_bypass();
    (hi << 1) | lo
}

/// Decode per-CTB SAO parameters at CTB raster position `(rx, ry)`. Mirrors
/// FFmpeg `hls_sao_param`. Resolves merge flags by copying from the left
/// or upper neighbor.
#[allow(clippy::too_many_arguments)]
pub fn decode_sao_param<P: Pixel>(
    cabac: &mut CabacReader,
    contexts: &mut CabacContexts,
    state: &mut PictureState<P>,
    sps: &Sps,
    sh: &SliceHeader,
    rx: usize,
    ry: usize,
) {
    if !sh.slice_sao_luma_flag && !sh.slice_sao_chroma_flag {
        return;
    }

    let pic_w_in_ctbs = sps
        .pic_width_in_luma_samples
        .div_ceil(1u32 << sps.ctb_log2_size_y) as usize;
    let mut sao = SaoParams::default();
    let mut sao_merge_left = 0u32;
    let mut sao_merge_up = 0u32;

    // SAO merge flags are only decoded when the neighbor CTB is available
    // (same slice and same tile). Spec 7.3.8.4 / FFmpeg hls_sao_param gates
    // these on ctb_left_flag / ctb_up_flag from hls_decode_neighbour.
    let ctb_rs = ry * pic_w_in_ctbs + rx;
    let cur_slice = state.tab_slice_addr_rs[ctb_rs];

    let left_avail = rx > 0 && state.tab_slice_addr_rs[ctb_rs - 1] == cur_slice;
    let up_avail = ry > 0 && state.tab_slice_addr_rs[ctb_rs - pic_w_in_ctbs] == cur_slice;

    if left_avail {
        sao_merge_left = decode_sao_merge_flag(cabac, contexts);
    }
    if up_avail && sao_merge_left == 0 {
        sao_merge_up = decode_sao_merge_flag(cabac, contexts);
    }

    if sao_merge_left != 0 {
        sao = state.sao_params[ry * pic_w_in_ctbs + (rx - 1)].clone();
    } else if sao_merge_up != 0 {
        sao = state.sao_params[(ry - 1) * pic_w_in_ctbs + rx].clone();
    } else {
        // Decode parameters from the bitstream.
        for c_idx in 0..3 {
            let plane_enabled = if c_idx == 0 {
                sh.slice_sao_luma_flag
            } else {
                sh.slice_sao_chroma_flag
            };
            if !plane_enabled {
                sao.type_idx[c_idx] = SaoType::NotApplied;
                continue;
            }
            if c_idx == 2 {
                // Cb/Cr share type_idx and eo_class.
                sao.type_idx[2] = sao.type_idx[1];
                sao.eo_class[2] = sao.eo_class[1];
            } else {
                sao.type_idx[c_idx] = decode_sao_type_idx(cabac, contexts);
            }
            if sao.type_idx[c_idx] == SaoType::NotApplied {
                continue;
            }

            let mut offset_abs = [0u32; 4];
            for v in offset_abs.iter_mut() {
                *v = decode_sao_offset_abs(cabac, sps.bit_depth_luma);
            }

            if sao.type_idx[c_idx] == SaoType::Band {
                let mut offset_sign = [0u32; 4];
                for (i, &abs) in offset_abs.iter().enumerate() {
                    if abs != 0 {
                        offset_sign[i] = decode_sao_offset_sign(cabac);
                    }
                }
                sao.band_position[c_idx] = decode_sao_band_position(cabac) as u8;
                sao.offset_val[c_idx][0] = 0;
                for (i, &abs) in offset_abs.iter().enumerate() {
                    let mut v = abs as i16;
                    if offset_sign[i] != 0 {
                        v = -v;
                    }
                    sao.offset_val[c_idx][i + 1] = v;
                }
            } else {
                // Edge offset path. Cb/Cr inherits eo_class but each plane
                // still has its own offset values.
                if c_idx != 2 {
                    sao.eo_class[c_idx] = decode_sao_eo_class(cabac) as u8;
                }
                sao.offset_val[c_idx][0] = 0;
                for (i, &abs) in offset_abs.iter().enumerate() {
                    let mut v = abs as i16;
                    if i > 1 {
                        v = -v;
                    }
                    sao.offset_val[c_idx][i + 1] = v;
                }
            }
        }
    }

    state.sao_params[ry * pic_w_in_ctbs + rx] = sao;
}

// ---- Filter kernels ----

/// Band offset filter: each sample is mapped to one of 32 bands; if its
/// band is in the 4 active bands starting at `sao_left_class`, the
/// corresponding offset is added.
#[allow(clippy::too_many_arguments)]
fn sao_band_filter<P: Pixel>(
    dst: &mut [P],
    src: &[P],
    stride_dst: usize,
    stride_src: usize,
    offset_val: &[i16; 5],
    sao_left_class: u8,
    width: usize,
    height: usize,
    x0: usize,
    y0: usize,
    bit_depth: u8,
) {
    // Build a 32-entry offset table — only the 4 active bands have offsets.
    let mut offset_table = [0i16; 32];
    for k in 0..4 {
        offset_table[(k + sao_left_class as usize) & 31] = offset_val[k + 1];
    }
    let shift = bit_depth - 5;
    for y in 0..height {
        for x in 0..width {
            let sample = src[(y0 + y) * stride_src + (x0 + x)].to_i32();
            let band = (sample >> shift) & 31;
            let new_val = sample + offset_table[band as usize] as i32;
            dst[(y0 + y) * stride_dst + (x0 + x)] = P::from_i32_clamped(new_val, bit_depth);
        }
    }
}

/// Compare-with-neighbor sign function used by edge offset.
#[inline]
fn cmp(a: i32, b: i32) -> i32 {
    (a > b) as i32 - (a < b) as i32
}

/// Edge offset filter: per-pixel category (1..4) drives the offset.
/// Pixels at the picture borders (or slice/tile boundaries when
/// `no_cross_left/right/top/bottom` restrict the accessible area) along
/// the EO direction are skipped.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::needless_range_loop)]
fn sao_edge_filter<P: Pixel>(
    dst: &mut [P],
    src: &[P],
    stride_dst: usize,
    stride_src: usize,
    offset_val: &[i16; 5],
    eo: u8,
    width: usize,
    height: usize,
    x0: usize,
    y0: usize,
    pic_w: usize,
    pic_h: usize,
    no_cross_left: bool,
    no_cross_right: bool,
    no_cross_top: bool,
    no_cross_bottom: bool,
    bit_depth: u8,
) {
    // edge_idx maps (1 + cmp(a) + cmp(b)) → category.
    // FFmpeg uses { 1, 2, 0, 3, 4 } indexed by [2 + diff0 + diff1].
    static EDGE_IDX: [usize; 5] = [1, 2, 0, 3, 4];
    // Direction offsets for the two neighbors (a and b).
    let (a_dx, a_dy, b_dx, b_dy): (i32, i32, i32, i32) = match eo {
        SAO_EO_HORIZ => (-1, 0, 1, 0),
        SAO_EO_VERT => (0, -1, 0, 1),
        SAO_EO_135D => (-1, -1, 1, 1),
        SAO_EO_45D => (1, -1, -1, 1),
        _ => (0, 0, 0, 0),
    };

    // Determine which rows/cols of the CTB to actually process — skip
    // pixels whose neighbors would lie outside the picture or across a
    // restricted slice/tile boundary.
    let init_x = if a_dx == -1 || b_dx == -1 {
        if x0 == 0 || no_cross_left { 1 } else { 0 }
    } else {
        0
    };
    let end_x = if a_dx == 1 || b_dx == 1 {
        if x0 + width >= pic_w || no_cross_right {
            width - 1
        } else {
            width
        }
    } else {
        width
    };
    let init_y = if a_dy == -1 || b_dy == -1 {
        if y0 == 0 || no_cross_top { 1 } else { 0 }
    } else {
        0
    };
    let end_y = if a_dy == 1 || b_dy == 1 {
        if y0 + height >= pic_h || no_cross_bottom {
            height - 1
        } else {
            height
        }
    } else {
        height
    };

    for y in init_y..end_y {
        for x in init_x..end_x {
            let cur_x = x0 + x;
            let cur_y = y0 + y;
            let cur = src[cur_y * stride_src + cur_x].to_i32();
            let a_x = (cur_x as i32 + a_dx) as usize;
            let a_y = (cur_y as i32 + a_dy) as usize;
            let b_x = (cur_x as i32 + b_dx) as usize;
            let b_y = (cur_y as i32 + b_dy) as usize;
            let a = src[a_y * stride_src + a_x].to_i32();
            let b = src[b_y * stride_src + b_x].to_i32();
            let diff0 = cmp(cur, a);
            let diff1 = cmp(cur, b);
            let cat = EDGE_IDX[(2 + diff0 + diff1) as usize];
            let new_val = cur + offset_val[cat] as i32;
            dst[cur_y * stride_dst + cur_x] = P::from_i32_clamped(new_val, bit_depth);
        }
    }
}

/// Check if two adjacent CTBs (by raster address) are in the same slice,
/// and if not, whether loop filtering across that boundary is allowed.
/// Returns true if the boundary should be treated as "no filtering across".
#[inline]
fn sao_skip_slice_boundary<P: Pixel>(state: &PictureState<P>, rs_a: usize, rs_b: usize) -> bool {
    if state.tab_slice_addr_rs[rs_a] == state.tab_slice_addr_rs[rs_b] {
        return false;
    }
    !state.filter_slice_edges[rs_a] || !state.filter_slice_edges[rs_b]
}

/// Apply SAO to the entire reconstructed picture, after deblocking.
/// Per-CTB SAO parameters must already be in `state.sao_params`.
pub fn apply_sao_picture<P: Pixel>(state: &mut PictureState<P>, sps: &Sps, sh: &SliceHeader) {
    if !sh.slice_sao_luma_flag && !sh.slice_sao_chroma_flag {
        return;
    }
    let pic_w = state.width as usize;
    let pic_h = state.height as usize;
    let pic_w_c = (state.width / 2) as usize;
    let pic_h_c = (state.height / 2) as usize;
    let ctb_size = 1usize << sps.ctb_log2_size_y;
    let pic_w_in_ctbs = pic_w.div_ceil(ctb_size);
    let pic_h_in_ctbs = pic_h.div_ceil(ctb_size);

    // Snapshot the deblocked planes for SAO source reads.
    let y_src = state.y_plane.clone();
    let u_src = state.u_plane.clone();
    let v_src = state.v_plane.clone();

    for ry in 0..pic_h_in_ctbs {
        for rx in 0..pic_w_in_ctbs {
            let ctb_rs = ry * pic_w_in_ctbs + rx;
            let sao = state.sao_params[ctb_rs].clone();

            // Compute slice-boundary "no cross" flags for this CTB.
            let no_cross_left = rx > 0 && sao_skip_slice_boundary(state, ctb_rs, ctb_rs - 1);
            let no_cross_right =
                rx + 1 < pic_w_in_ctbs && sao_skip_slice_boundary(state, ctb_rs, ctb_rs + 1);
            let no_cross_top =
                ry > 0 && sao_skip_slice_boundary(state, ctb_rs, ctb_rs - pic_w_in_ctbs);
            let no_cross_bottom = ry + 1 < pic_h_in_ctbs
                && sao_skip_slice_boundary(state, ctb_rs, ctb_rs + pic_w_in_ctbs);

            // Luma
            if sh.slice_sao_luma_flag && sao.type_idx[0] != SaoType::NotApplied {
                let x0 = rx * ctb_size;
                let y0 = ry * ctb_size;
                let w = (x0 + ctb_size).min(pic_w) - x0;
                let h = (y0 + ctb_size).min(pic_h) - y0;
                let bit_depth_y = sps.bit_depth_luma;
                match sao.type_idx[0] {
                    SaoType::Band => sao_band_filter::<P>(
                        &mut state.y_plane,
                        &y_src,
                        state.y_stride,
                        state.y_stride,
                        &sao.offset_val[0],
                        sao.band_position[0],
                        w,
                        h,
                        x0,
                        y0,
                        bit_depth_y,
                    ),
                    SaoType::Edge => sao_edge_filter::<P>(
                        &mut state.y_plane,
                        &y_src,
                        state.y_stride,
                        state.y_stride,
                        &sao.offset_val[0],
                        sao.eo_class[0],
                        w,
                        h,
                        x0,
                        y0,
                        pic_w,
                        pic_h,
                        no_cross_left,
                        no_cross_right,
                        no_cross_top,
                        no_cross_bottom,
                        bit_depth_y,
                    ),
                    SaoType::NotApplied => {}
                }
            }

            // Chroma (4:2:0): operates on the 8x8 chroma CTU grid.
            if sh.slice_sao_chroma_flag && sps.chroma_format_idc == 1 {
                let x0_c = rx * (ctb_size / 2);
                let y0_c = ry * (ctb_size / 2);
                let w_c = (x0_c + (ctb_size / 2)).min(pic_w_c) - x0_c;
                let h_c = (y0_c + (ctb_size / 2)).min(pic_h_c) - y0_c;
                for c_idx in 1..=2 {
                    if sao.type_idx[c_idx] == SaoType::NotApplied {
                        continue;
                    }
                    let (dst_plane, src_plane) = if c_idx == 1 {
                        (&mut state.u_plane, &u_src)
                    } else {
                        (&mut state.v_plane, &v_src)
                    };
                    let bit_depth_c = sps.bit_depth_chroma;
                    match sao.type_idx[c_idx] {
                        SaoType::Band => sao_band_filter::<P>(
                            dst_plane,
                            src_plane,
                            state.uv_stride,
                            state.uv_stride,
                            &sao.offset_val[c_idx],
                            sao.band_position[c_idx],
                            w_c,
                            h_c,
                            x0_c,
                            y0_c,
                            bit_depth_c,
                        ),
                        SaoType::Edge => sao_edge_filter::<P>(
                            dst_plane,
                            src_plane,
                            state.uv_stride,
                            state.uv_stride,
                            &sao.offset_val[c_idx],
                            sao.eo_class[c_idx],
                            w_c,
                            h_c,
                            x0_c,
                            y0_c,
                            pic_w_c,
                            pic_h_c,
                            no_cross_left,
                            no_cross_right,
                            no_cross_top,
                            no_cross_bottom,
                            bit_depth_c,
                        ),
                        SaoType::NotApplied => {}
                    }
                }
            }
        }
    }
}
