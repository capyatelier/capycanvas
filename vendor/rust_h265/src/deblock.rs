// The Q0/P0 = `0 * xstride` and `(-1) * xstride` patterns are intentional
// for clarity — they mirror the FFmpeg `P3 P2 P1 P0 Q0 Q1 Q2 Q3` macro layout.
#![allow(clippy::erasing_op)]
#![allow(clippy::identity_op)]
#![allow(clippy::neg_multiply)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::too_many_arguments)]

//! HEVC deblocking filter (spec 8.7.2).
//!
//! Filters the reconstructed picture in-place to soften block-edge artifacts.
//! For Phase 3b-1 we implement the intra-slice path: every internal TU/CU
//! 8×8 edge inside the picture gets boundary strength `bS = 2` (because both
//! sides are intra), and the strong/normal luma filters and chroma filter
//! are applied per spec 8.7.2.4 / 8.7.2.5.
//!
//! Reference: FFmpeg `libavcodec/hevc/filter.c::deblocking_filter_CTB` and
//! `libavcodec/hevc/dsp_template.c::hevc_loop_filter_*`.

use crate::cu_tree::PictureState;
use crate::pixel::Pixel;
use crate::pps::Pps;
use crate::slice::SliceHeader;
use crate::sps::Sps;

/// HEVC β table indexed by clipped QP, spec table 8-13.
#[rustfmt::skip]
static BETA_TABLE: [u8; 52] = [
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  6,  7,  8,  9,
    10, 11, 12, 13, 14, 15, 16, 17, 18, 20,
    22, 24, 26, 28, 30, 32, 34, 36, 38, 40,
    42, 44, 46, 48, 50, 52, 54, 56, 58, 60,
    62, 64,
];

/// HEVC tc table indexed by clipped (QP + 2*(bS-1) + tc_offset), spec table 8-14.
#[rustfmt::skip]
static TC_TABLE: [u8; 54] = [
    0, 0, 0, 0, 0, 0, 0,  0,  0,  0,  0,  0,  0,  0,  0,  0, 0, 0, 1,
    1, 1, 1, 1, 1, 1, 1,  1,  2,  2,  2,  2,  3,  3,  3,  3, 4, 4, 4,
    5, 5, 6, 6, 7, 8, 9, 10, 11, 13, 14, 16, 18, 20, 22, 24,
];

const MAX_QP: i32 = 51;

/// Compute β for the given QP and offset, scaled for bit depth.
fn beta_lookup(qp: i32, beta_offset: i32, bit_depth: u8) -> i32 {
    let idx = (qp + beta_offset).clamp(0, MAX_QP) as usize;
    (BETA_TABLE[idx] as i32) << (bit_depth - 8)
}

/// Compute tc for the given QP, boundary strength, and offset, scaled for bit depth.
fn tc_lookup(qp: i32, bs: i32, tc_offset: i32, bit_depth: u8) -> i32 {
    // For intra deblocking the offset is 2*(bS - 1) per spec 8.7.2.5.
    let idx = (qp + 2 * (bs - 1) + tc_offset).clamp(0, 53) as usize;
    (TC_TABLE[idx] as i32) << (bit_depth - 8)
}

/// Map a luma QP to the chroma QP via spec table 8-9 (chroma_format_idc=1).
fn chroma_qp_table(qp_i: i32) -> i32 {
    const QP_C: [i32; 14] = [29, 30, 31, 32, 33, 33, 34, 34, 35, 35, 36, 36, 37, 37];
    if qp_i < 30 {
        qp_i
    } else if qp_i > 43 {
        qp_i - 6
    } else {
        QP_C[(qp_i - 30) as usize]
    }
}

/// Compute chroma tc for an intra edge (bS == 2), scaled for bit depth.
fn chroma_tc(qp_y: i32, qp_offset: i32, tc_offset: i32, bit_depth: u8) -> i32 {
    let qp_i = (qp_y + qp_offset).clamp(0, 57);
    let qp = chroma_qp_table(qp_i);
    // Intra deblocking: bS - 1 = 1, so tc table index gets +2.
    let idx = (qp + 2 + tc_offset).clamp(0, 53) as usize;
    (TC_TABLE[idx] as i32) << (bit_depth - 8)
}

// ---- Inner luma kernels (per "filter unit" of 4 pixels along the edge) ----

/// Strong luma filter, modifies up to 3 pixels on each side.
/// `pix` points to the first sample. `xstride` is the across-edge stride
/// (1 for horizontal edge, plane stride for vertical edge). `ystride` is
/// the along-edge stride. `tc`, `tc2`, `tc3` are bounded by `tc << 0/1/?`.
#[allow(clippy::too_many_arguments)]
fn loop_filter_luma_strong<P: Pixel>(
    plane: &mut [P],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    tc2: i32,
    tc3: i32,
    bit_depth: u8,
) {
    for d in 0..4 {
        let line_off = d * ystride;
        let p3 = plane[(base as isize + line_off + (-4) * xstride) as usize].to_i32();
        let p2 = plane[(base as isize + line_off + (-3) * xstride) as usize].to_i32();
        let p1 = plane[(base as isize + line_off + (-2) * xstride) as usize].to_i32();
        let p0 = plane[(base as isize + line_off + (-1) * xstride) as usize].to_i32();
        let q0 = plane[(base as isize + line_off + 0 * xstride) as usize].to_i32();
        let q1 = plane[(base as isize + line_off + 1 * xstride) as usize].to_i32();
        let q2 = plane[(base as isize + line_off + 2 * xstride) as usize].to_i32();
        let q3 = plane[(base as isize + line_off + 3 * xstride) as usize].to_i32();

        let new_p0 = p0 + ((((p2 + 2 * p1 + 2 * p0 + 2 * q0 + q1 + 4) >> 3) - p0).clamp(-tc3, tc3));
        let new_p1 = p1 + ((((p2 + p1 + p0 + q0 + 2) >> 2) - p1).clamp(-tc2, tc2));
        let new_p2 = p2 + ((((2 * p3 + 3 * p2 + p1 + p0 + q0 + 4) >> 3) - p2).clamp(-tc, tc));
        let new_q0 = q0 + ((((p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) >> 3) - q0).clamp(-tc3, tc3));
        let new_q1 = q1 + ((((p0 + q0 + q1 + q2 + 2) >> 2) - q1).clamp(-tc2, tc2));
        let new_q2 = q2 + ((((2 * q3 + 3 * q2 + q1 + q0 + p0 + 4) >> 3) - q2).clamp(-tc, tc));

        plane[(base as isize + line_off + (-3) * xstride) as usize] =
            P::from_i32_clamped(new_p2, bit_depth);
        plane[(base as isize + line_off + (-2) * xstride) as usize] =
            P::from_i32_clamped(new_p1, bit_depth);
        plane[(base as isize + line_off + (-1) * xstride) as usize] =
            P::from_i32_clamped(new_p0, bit_depth);
        plane[(base as isize + line_off + 0 * xstride) as usize] =
            P::from_i32_clamped(new_q0, bit_depth);
        plane[(base as isize + line_off + 1 * xstride) as usize] =
            P::from_i32_clamped(new_q1, bit_depth);
        plane[(base as isize + line_off + 2 * xstride) as usize] =
            P::from_i32_clamped(new_q2, bit_depth);
    }
}

/// Weak (normal) luma filter.
#[allow(clippy::too_many_arguments)]
fn loop_filter_luma_weak<P: Pixel>(
    plane: &mut [P],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    nd_p: i32,
    nd_q: i32,
    bit_depth: u8,
) {
    let tc_2 = tc >> 1;
    for d in 0..4 {
        let line_off = d * ystride;
        let p2 = plane[(base as isize + line_off + (-3) * xstride) as usize].to_i32();
        let p1 = plane[(base as isize + line_off + (-2) * xstride) as usize].to_i32();
        let p0 = plane[(base as isize + line_off + (-1) * xstride) as usize].to_i32();
        let q0 = plane[(base as isize + line_off + 0 * xstride) as usize].to_i32();
        let q1 = plane[(base as isize + line_off + 1 * xstride) as usize].to_i32();
        let q2 = plane[(base as isize + line_off + 2 * xstride) as usize].to_i32();

        let mut delta0 = (9 * (q0 - p0) - 3 * (q1 - p1) + 8) >> 4;
        if delta0.abs() < 10 * tc {
            delta0 = delta0.clamp(-tc, tc);
            plane[(base as isize + line_off + (-1) * xstride) as usize] =
                P::from_i32_clamped(p0 + delta0, bit_depth);
            plane[(base as isize + line_off + 0 * xstride) as usize] =
                P::from_i32_clamped(q0 - delta0, bit_depth);
            if nd_p > 1 {
                let dp = (((p2 + p0 + 1) >> 1) - p1 + delta0) >> 1;
                let dp = dp.clamp(-tc_2, tc_2);
                plane[(base as isize + line_off + (-2) * xstride) as usize] =
                    P::from_i32_clamped(p1 + dp, bit_depth);
            }
            if nd_q > 1 {
                let dq = (((q2 + q0 + 1) >> 1) - q1 - delta0) >> 1;
                let dq = dq.clamp(-tc_2, tc_2);
                plane[(base as isize + line_off + 1 * xstride) as usize] =
                    P::from_i32_clamped(q1 + dq, bit_depth);
            }
        }
    }
}

/// Chroma weak filter (the only chroma filter HEVC has). 4 lines.
fn loop_filter_chroma_weak<P: Pixel>(
    plane: &mut [P],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    bit_depth: u8,
) {
    for d in 0..4 {
        let line_off = d * ystride;
        let p1 = plane[(base as isize + line_off + (-2) * xstride) as usize].to_i32();
        let p0 = plane[(base as isize + line_off + (-1) * xstride) as usize].to_i32();
        let q0 = plane[(base as isize + line_off + 0 * xstride) as usize].to_i32();
        let q1 = plane[(base as isize + line_off + 1 * xstride) as usize].to_i32();
        let delta0 = ((((q0 - p0) * 4) + p1 - q1 + 4) >> 3).clamp(-tc, tc);
        plane[(base as isize + line_off + (-1) * xstride) as usize] =
            P::from_i32_clamped(p0 + delta0, bit_depth);
        plane[(base as isize + line_off + 0 * xstride) as usize] =
            P::from_i32_clamped(q0 - delta0, bit_depth);
    }
}

// ---- Per-edge dispatch (8 pixels along the edge = 2 filter units of 4) ----

/// Filter one 8-pixel luma edge given strength flags `bs0`, `bs1` for the
/// two halves of the edge. `pix_base` is the index of the first sample
/// of the second-side first row (Q0[0]). `xstride` and `ystride` describe
/// the sample layout (vertical edge: xstride=1, ystride=plane_stride;
/// horizontal edge: xstride=plane_stride, ystride=1).
#[allow(clippy::too_many_arguments)]
fn filter_luma_edge<P: Pixel>(
    plane: &mut [P],
    pix_base: usize,
    xstride: isize,
    ystride: isize,
    qp_avg: i32,
    bs0: i32,
    bs1: i32,
    beta_offset: i32,
    tc_offset: i32,
    bit_depth: u8,
) {
    let beta = beta_lookup(qp_avg, beta_offset, bit_depth);
    let tcs = [
        if bs0 != 0 {
            tc_lookup(qp_avg, bs0, tc_offset, bit_depth)
        } else {
            0
        },
        if bs1 != 0 {
            tc_lookup(qp_avg, bs1, tc_offset, bit_depth)
        } else {
            0
        },
    ];

    for j in 0..2 {
        let tc = tcs[j];
        if tc == 0 {
            continue;
        }
        // Each j-half is 4 lines along the edge, starting at row j*4.
        let j_base = (pix_base as isize + (j as isize * 4) * ystride) as usize;

        // Compute d on lines 0 and 3 of this segment, per spec 8.7.2.5.
        let d0_p = (plane[(j_base as isize + 0 * ystride + (-3) * xstride) as usize].to_i32()
            - 2 * plane[(j_base as isize + 0 * ystride + (-2) * xstride) as usize].to_i32()
            + plane[(j_base as isize + 0 * ystride + (-1) * xstride) as usize].to_i32())
        .abs();
        let d0_q = (plane[(j_base as isize + 0 * ystride + 0 * xstride) as usize].to_i32()
            - 2 * plane[(j_base as isize + 0 * ystride + 1 * xstride) as usize].to_i32()
            + plane[(j_base as isize + 0 * ystride + 2 * xstride) as usize].to_i32())
        .abs();
        let d3_p = (plane[(j_base as isize + 3 * ystride + (-3) * xstride) as usize].to_i32()
            - 2 * plane[(j_base as isize + 3 * ystride + (-2) * xstride) as usize].to_i32()
            + plane[(j_base as isize + 3 * ystride + (-1) * xstride) as usize].to_i32())
        .abs();
        let d3_q = (plane[(j_base as isize + 3 * ystride + 0 * xstride) as usize].to_i32()
            - 2 * plane[(j_base as isize + 3 * ystride + 1 * xstride) as usize].to_i32()
            + plane[(j_base as isize + 3 * ystride + 2 * xstride) as usize].to_i32())
        .abs();
        let d0 = d0_p + d0_q;
        let d3 = d3_p + d3_q;
        let dp_total = d0_p + d3_p;
        let dq_total = d0_q + d3_q;
        let d = d0 + d3;

        if d < beta {
            let beta_3 = beta >> 3;
            let beta_2 = beta >> 2;
            let tc25 = (tc * 5 + 1) >> 1;

            // Strong filter check.
            let p3_l0 = plane[(j_base as isize + 0 * ystride + (-4) * xstride) as usize].to_i32();
            let p0_l0 = plane[(j_base as isize + 0 * ystride + (-1) * xstride) as usize].to_i32();
            let q0_l0 = plane[(j_base as isize + 0 * ystride + 0 * xstride) as usize].to_i32();
            let q3_l0 = plane[(j_base as isize + 0 * ystride + 3 * xstride) as usize].to_i32();
            let p3_l3 = plane[(j_base as isize + 3 * ystride + (-4) * xstride) as usize].to_i32();
            let p0_l3 = plane[(j_base as isize + 3 * ystride + (-1) * xstride) as usize].to_i32();
            let q0_l3 = plane[(j_base as isize + 3 * ystride + 0 * xstride) as usize].to_i32();
            let q3_l3 = plane[(j_base as isize + 3 * ystride + 3 * xstride) as usize].to_i32();

            let strong = (p3_l0 - p0_l0).abs() + (q3_l0 - q0_l0).abs() < beta_3
                && (p0_l0 - q0_l0).abs() < tc25
                && (p3_l3 - p0_l3).abs() + (q3_l3 - q0_l3).abs() < beta_3
                && (p0_l3 - q0_l3).abs() < tc25
                && (d0 << 1) < beta_2
                && (d3 << 1) < beta_2;

            if strong {
                let tc2 = tc << 1;
                loop_filter_luma_strong(plane, j_base, xstride, ystride, tc2, tc2, tc2, bit_depth);
            } else {
                let nd_p = if dp_total < ((beta + (beta >> 1)) >> 3) {
                    2
                } else {
                    1
                };
                let nd_q = if dq_total < ((beta + (beta >> 1)) >> 3) {
                    2
                } else {
                    1
                };
                loop_filter_luma_weak(plane, j_base, xstride, ystride, tc, nd_p, nd_q, bit_depth);
            }
        }
    }
}

/// Filter one 8-pixel chroma edge (in chroma samples). `bs0` and `bs1`
/// must both be 2 (otherwise this function isn't called).
#[allow(clippy::too_many_arguments)]
fn filter_chroma_edge<P: Pixel>(
    plane: &mut [P],
    pix_base: usize,
    xstride: isize,
    ystride: isize,
    qp0_avg: i32,
    qp1_avg: i32,
    qp_offset: i32,
    tc_offset: i32,
    bs0: i32,
    bs1: i32,
    bit_depth: u8,
) {
    let tcs = [
        if bs0 == 2 {
            chroma_tc(qp0_avg, qp_offset, tc_offset, bit_depth)
        } else {
            0
        },
        if bs1 == 2 {
            chroma_tc(qp1_avg, qp_offset, tc_offset, bit_depth)
        } else {
            0
        },
    ];
    for j in 0..2 {
        if tcs[j] == 0 {
            continue;
        }
        let j_base = (pix_base as isize + (j as isize * 4) * ystride) as usize;
        loop_filter_chroma_weak(plane, j_base, xstride, ystride, tcs[j], bit_depth);
    }
}

// ---- Top-level dispatch ----

/// Get the per-min-CB QP at luma sample position `(x, y)`.
fn get_qp_y<P: Pixel>(state: &PictureState<P>, x: i32, y: i32) -> i32 {
    let xc = (x.max(0) >> state.log2_min_cb_size) as usize;
    let yc = (y.max(0) >> state.log2_min_cb_size) as usize;
    state.tab_qp_y[yc * state.min_cb_width + xc] as i32
}

/// Read a boundary strength from `state.bs_vertical` (or `bs_horizontal`),
/// indexed by 4-pixel grid coordinates.
fn read_bs(bs: &[u8], pic_w: usize, x: usize, y: usize) -> i32 {
    bs[(y >> 2) * (pic_w >> 2) + (x >> 2)] as i32
}

/// Check whether a vertical edge at luma x crosses a slice boundary and
/// should be skipped because `slice_loop_filter_across_slices_enabled_flag`
/// is false on either side.
#[inline]
fn skip_vertical_slice_boundary<P: Pixel>(state: &PictureState<P>, x: usize, y: usize) -> bool {
    let log2_ctb = state.log2_ctb_size as usize;
    let left_ctb_col = (x - 1) >> log2_ctb;
    let right_ctb_col = x >> log2_ctb;
    if left_ctb_col == right_ctb_col {
        return false; // not at a CTB column boundary
    }
    let ctb_width = (state.width as usize).div_ceil(1 << log2_ctb);
    let ctb_row = y >> log2_ctb;
    let left_rs = ctb_row * ctb_width + left_ctb_col;
    let right_rs = ctb_row * ctb_width + right_ctb_col;
    if state.tab_slice_addr_rs[left_rs] == state.tab_slice_addr_rs[right_rs] {
        return false; // same slice
    }
    !state.filter_slice_edges[left_rs] || !state.filter_slice_edges[right_rs]
}

/// Check whether a horizontal edge at luma y crosses a slice boundary and
/// should be skipped.
#[inline]
fn skip_horizontal_slice_boundary<P: Pixel>(state: &PictureState<P>, x: usize, y: usize) -> bool {
    let log2_ctb = state.log2_ctb_size as usize;
    let top_ctb_row = (y - 1) >> log2_ctb;
    let bot_ctb_row = y >> log2_ctb;
    if top_ctb_row == bot_ctb_row {
        return false; // not at a CTB row boundary
    }
    let ctb_width = (state.width as usize).div_ceil(1 << log2_ctb);
    let ctb_col = x >> log2_ctb;
    let top_rs = top_ctb_row * ctb_width + ctb_col;
    let bot_rs = bot_ctb_row * ctb_width + ctb_col;
    if state.tab_slice_addr_rs[top_rs] == state.tab_slice_addr_rs[bot_rs] {
        return false; // same slice
    }
    !state.filter_slice_edges[top_rs] || !state.filter_slice_edges[bot_rs]
}

/// Apply the deblocking filter to the entire reconstructed picture.
///
/// Phase 3b-1 limitation: only the intra-slice path. The boundary strength
/// arrays must already be populated by the CU/TU decode (every internal
/// 8-aligned TU/CU edge inside the picture gets `bS = 2`).
pub fn deblock_picture<P: Pixel>(
    state: &mut PictureState<P>,
    sps: &Sps,
    pps: &Pps,
    sh: &SliceHeader,
) {
    let pic_w = state.width as usize;
    let pic_h = state.height as usize;
    let stride_y = state.y_stride;
    let stride_uv = state.uv_stride;
    let beta_offset = (sh.slice_beta_offset_div2 + pps.pps_beta_offset_div2) * 2;
    let tc_offset = (sh.slice_tc_offset_div2 + pps.pps_tc_offset_div2) * 2;
    let bit_depth_luma = sps.bit_depth_luma;
    let bit_depth_chroma = sps.bit_depth_chroma;

    // ---- Luma vertical edges ----
    let mut y = 0usize;
    while y < pic_h {
        let mut x = 8usize;
        while x < pic_w {
            if skip_vertical_slice_boundary(state, x, y) {
                x += 8;
                continue;
            }
            let bs0 = read_bs(&state.bs_vertical, pic_w, x, y);
            let bs1 = read_bs(&state.bs_vertical, pic_w, x, y + 4);
            if bs0 != 0 || bs1 != 0 {
                let qp_avg = (get_qp_y(state, x as i32 - 1, y as i32)
                    + get_qp_y(state, x as i32, y as i32)
                    + 1)
                    >> 1;
                let pix_base = y * stride_y + x;
                filter_luma_edge::<P>(
                    &mut state.y_plane,
                    pix_base,
                    1,
                    stride_y as isize,
                    qp_avg,
                    bs0,
                    bs1,
                    beta_offset,
                    tc_offset,
                    bit_depth_luma,
                );
            }
            x += 8;
        }
        y += 8;
    }

    // ---- Luma horizontal edges ----
    let mut y = 8usize;
    while y < pic_h {
        let mut x = 0usize;
        while x < pic_w {
            if skip_horizontal_slice_boundary(state, x, y) {
                x += 8;
                continue;
            }
            let bs0 = read_bs(&state.bs_horizontal, pic_w, x, y);
            let bs1 = read_bs(&state.bs_horizontal, pic_w, x + 4, y);
            if bs0 != 0 || bs1 != 0 {
                let qp_avg = (get_qp_y(state, x as i32, y as i32 - 1)
                    + get_qp_y(state, x as i32, y as i32)
                    + 1)
                    >> 1;
                let pix_base = y * stride_y + x;
                filter_luma_edge::<P>(
                    &mut state.y_plane,
                    pix_base,
                    stride_y as isize,
                    1,
                    qp_avg,
                    bs0,
                    bs1,
                    beta_offset,
                    tc_offset,
                    bit_depth_luma,
                );
            }
            x += 8;
        }
        y += 8;
    }

    // ---- Chroma (4:2:0): on the 16x16 luma grid ----
    //
    // To keep the borrow checker happy we collect (pix_base, qp0, qp1, bs0, bs1)
    // for every chroma edge before grabbing &mut state.u_plane / v_plane.
    if sps.chroma_format_idc == 1 {
        struct ChromaEdge {
            pix_base: usize,
            xstride: isize,
            ystride: isize,
            qp0_avg: i32,
            qp1_avg: i32,
            bs0: i32,
            bs1: i32,
        }
        let mut edges: Vec<ChromaEdge> = Vec::new();

        // Vertical edges, every 16 luma cols.
        let mut y_l = 0usize;
        while y_l < pic_h {
            let mut x_l = 16usize;
            while x_l < pic_w {
                if skip_vertical_slice_boundary(state, x_l, y_l) {
                    x_l += 16;
                    continue;
                }
                let bs0 = read_bs(&state.bs_vertical, pic_w, x_l, y_l);
                let bs1 = read_bs(&state.bs_vertical, pic_w, x_l, y_l + 8);
                if bs0 == 2 || bs1 == 2 {
                    let qp0_avg = (get_qp_y(state, x_l as i32 - 1, y_l as i32)
                        + get_qp_y(state, x_l as i32, y_l as i32)
                        + 1)
                        >> 1;
                    let qp1_avg = (get_qp_y(state, x_l as i32 - 1, y_l as i32 + 8)
                        + get_qp_y(state, x_l as i32, y_l as i32 + 8)
                        + 1)
                        >> 1;
                    let xc = x_l >> 1;
                    let yc = y_l >> 1;
                    edges.push(ChromaEdge {
                        pix_base: yc * stride_uv + xc,
                        xstride: 1,
                        ystride: stride_uv as isize,
                        qp0_avg,
                        qp1_avg,
                        bs0,
                        bs1,
                    });
                }
                x_l += 16;
            }
            y_l += 16;
        }
        // Horizontal edges.
        let mut y_l = 16usize;
        while y_l < pic_h {
            let mut x_l = 0usize;
            while x_l < pic_w {
                if skip_horizontal_slice_boundary(state, x_l, y_l) {
                    x_l += 16;
                    continue;
                }
                let bs0 = read_bs(&state.bs_horizontal, pic_w, x_l, y_l);
                let bs1 = read_bs(&state.bs_horizontal, pic_w, x_l + 8, y_l);
                if bs0 == 2 || bs1 == 2 {
                    let qp0_avg = (get_qp_y(state, x_l as i32, y_l as i32 - 1)
                        + get_qp_y(state, x_l as i32, y_l as i32)
                        + 1)
                        >> 1;
                    let qp1_avg = (get_qp_y(state, x_l as i32 + 8, y_l as i32 - 1)
                        + get_qp_y(state, x_l as i32 + 8, y_l as i32)
                        + 1)
                        >> 1;
                    let xc = x_l >> 1;
                    let yc = y_l >> 1;
                    edges.push(ChromaEdge {
                        pix_base: yc * stride_uv + xc,
                        xstride: stride_uv as isize,
                        ystride: 1,
                        qp0_avg,
                        qp1_avg,
                        bs0,
                        bs1,
                    });
                }
                x_l += 16;
            }
            y_l += 16;
        }

        // Now apply edges to each chroma plane.
        for c_idx in 1..=2 {
            let qp_offset = if c_idx == 1 {
                pps.pps_cb_qp_offset
            } else {
                pps.pps_cr_qp_offset
            };
            let plane = if c_idx == 1 {
                &mut state.u_plane
            } else {
                &mut state.v_plane
            };
            for e in &edges {
                filter_chroma_edge::<P>(
                    plane,
                    e.pix_base,
                    e.xstride,
                    e.ystride,
                    e.qp0_avg,
                    e.qp1_avg,
                    qp_offset,
                    tc_offset,
                    e.bs0,
                    e.bs1,
                    bit_depth_chroma,
                );
            }
        }
    }
}
