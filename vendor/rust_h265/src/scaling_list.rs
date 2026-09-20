//! HEVC scaling list parsing and default list construction (spec 7.3.4).
//!
//! The scaling list data structure mirrors FFmpeg's `ScalingList`:
//! - `sl[4][6][64]`: 4 size classes × 6 matrix IDs × up to 64 entries.
//!   - `sl[0]`: 4×4 (16 entries used, indexed in raster order)
//!   - `sl[1]`: 8×8 (64 entries)
//!   - `sl[2]`: 16×16 (64 entries; the 8×8 matrix is upsampled at decode time)
//!   - `sl[3]`: 32×32 (64 entries; only matrix IDs 0 and 3 are used)
//! - `sl_dc[2][6]`: DC coefficients for 16×16 and 32×32.
//!   - `sl_dc[0]`: 16×16 DC values (size_id=2)
//!   - `sl_dc[1]`: 32×32 DC values (size_id=3)
//!
//! Matrix ID mapping (for size_id 1/2): `3 * is_inter + c_idx`.
//!   - 0 = intra Y, 1 = intra Cb, 2 = intra Cr
//!   - 3 = inter Y, 4 = inter Cb, 5 = inter Cr
//!
//! For size_id 3 (32×32), only IDs 0 (intra Y) and 3 (inter Y) exist.

use crate::bitstream::BitstreamReader;
use crate::error::DecodeError;

/// Default 8×8 scaling list for intra prediction (HEVC spec table 7-3).
#[rustfmt::skip]
const DEFAULT_SCALING_LIST_INTRA: [u8; 64] = [
    16, 16, 16, 16, 17, 18, 21, 24,
    16, 16, 16, 16, 17, 19, 22, 25,
    16, 16, 17, 18, 20, 22, 25, 29,
    16, 16, 18, 21, 24, 27, 31, 36,
    17, 17, 20, 24, 30, 35, 41, 47,
    18, 19, 22, 27, 35, 44, 54, 65,
    21, 22, 25, 31, 41, 54, 70, 88,
    24, 25, 29, 36, 47, 65, 88, 115,
];

/// Default 8×8 scaling list for inter prediction (HEVC spec table 7-4).
#[rustfmt::skip]
const DEFAULT_SCALING_LIST_INTER: [u8; 64] = [
    16, 16, 16, 16, 17, 18, 20, 24,
    16, 16, 16, 17, 18, 20, 24, 25,
    16, 16, 17, 18, 20, 24, 25, 28,
    16, 17, 18, 20, 24, 25, 28, 33,
    17, 18, 20, 24, 25, 28, 33, 41,
    18, 20, 24, 25, 28, 33, 41, 54,
    20, 24, 25, 28, 33, 41, 54, 71,
    24, 25, 28, 33, 41, 54, 71, 91,
];

/// 4×4 diagonal scan: x coordinates (same as in residual_coding.rs).
#[rustfmt::skip]
const DIAG_SCAN_4X4_X: [usize; 16] = [
    0, 0, 1, 0, 1, 2, 0, 1, 2, 3, 1, 2, 3, 2, 3, 3,
];

/// 4×4 diagonal scan: y coordinates.
#[rustfmt::skip]
const DIAG_SCAN_4X4_Y: [usize; 16] = [
    0, 1, 0, 2, 1, 0, 3, 2, 1, 0, 3, 2, 1, 3, 2, 3,
];

/// 8×8 diagonal scan: x coordinates.
#[rustfmt::skip]
const DIAG_SCAN_8X8_X: [usize; 64] = [
    0, 0, 1, 0, 1, 2, 0, 1, 2, 3, 0, 1, 2, 3, 4, 0,
    1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5, 6, 0, 1, 2, 3,
    4, 5, 6, 7, 1, 2, 3, 4, 5, 6, 7, 2, 3, 4, 5, 6,
    7, 3, 4, 5, 6, 7, 4, 5, 6, 7, 5, 6, 7, 6, 7, 7,
];

/// 8×8 diagonal scan: y coordinates.
#[rustfmt::skip]
const DIAG_SCAN_8X8_Y: [usize; 64] = [
    0, 1, 0, 2, 1, 0, 3, 2, 1, 0, 4, 3, 2, 1, 0, 5,
    4, 3, 2, 1, 0, 6, 5, 4, 3, 2, 1, 0, 7, 6, 5, 4,
    3, 2, 1, 0, 7, 6, 5, 4, 3, 2, 1, 7, 6, 5, 4, 3,
    2, 7, 6, 5, 4, 3, 7, 6, 5, 4, 7, 6, 5, 7, 6, 7,
];

/// HEVC scaling list data (mirrors FFmpeg's `ScalingList`).
#[derive(Debug, Clone)]
pub struct ScalingList {
    /// `sl[size_id][matrix_id][coeff_idx]`.
    /// - size_id 0 (4×4): 16 entries per matrix, 6 matrices
    /// - size_id 1 (8×8): 64 entries per matrix, 6 matrices
    /// - size_id 2 (16×16): 64 entries per matrix, 6 matrices
    /// - size_id 3 (32×32): 64 entries per matrix, 6 matrices (only 0,3 used)
    pub sl: [[[u8; 64]; 6]; 4],
    /// DC coefficients for 16×16 (`sl_dc[0]`) and 32×32 (`sl_dc[1]`).
    pub sl_dc: [[u8; 6]; 2],
}

impl ScalingList {
    /// Construct a `ScalingList` with all default values (HEVC spec tables 7-3..7-6).
    /// This matches FFmpeg's `set_default_scaling_list_data`.
    pub fn default_scaling_list() -> Self {
        let mut sl = [[[0u8; 64]; 6]; 4];
        // DC defaults to 16 for both 16×16 (sl_dc[0]) and 32×32 (sl_dc[1]).
        let sl_dc = [[16u8; 6]; 2];

        // Size ID 0 (4×4): flat 16 for all 6 matrices.
        for matrix in sl[0].iter_mut() {
            for entry in matrix.iter_mut().take(16) {
                *entry = 16;
            }
        }

        // Size IDs 1, 2, 3: intra (matrix IDs 0, 1, 2) + inter (3, 4, 5).
        for size_group in sl[1..4].iter_mut() {
            for (matrix_id, matrix) in size_group.iter_mut().enumerate() {
                *matrix = if matrix_id < 3 {
                    DEFAULT_SCALING_LIST_INTRA
                } else {
                    DEFAULT_SCALING_LIST_INTER
                };
            }
        }

        ScalingList { sl, sl_dc }
    }
}

/// Parse `scaling_list_data()` syntax (HEVC spec 7.3.4).
///
/// The `sl` parameter should be pre-initialized with default values before
/// calling this function (matching FFmpeg's approach where `set_default_scaling_list_data`
/// is called first).
pub fn parse_scaling_list_data(
    r: &mut BitstreamReader,
    sl: &mut ScalingList,
) -> Result<(), DecodeError> {
    for size_id in 0u8..4 {
        let step = if size_id == 3 { 3 } else { 1 };
        let mut matrix_id = 0usize;
        while matrix_id < 6 {
            let scaling_list_pred_mode_flag = r.read_bit()? == 1;
            if !scaling_list_pred_mode_flag {
                // Copy from a previous matrix.
                let delta = r.read_ue()? as usize;
                if delta != 0 {
                    let actual_delta = delta * step;
                    if matrix_id < actual_delta {
                        return Err(DecodeError::InvalidSyntax(
                            "invalid delta in scaling list data",
                        ));
                    }
                    let src_matrix = matrix_id - actual_delta;
                    // Copy the matrix.
                    let num = if size_id > 0 { 64 } else { 16 };
                    let src = sl.sl[size_id as usize][src_matrix];
                    sl.sl[size_id as usize][matrix_id][..num].copy_from_slice(&src[..num]);
                    // Copy DC coefficient for 16×16 and 32×32.
                    if size_id > 1 {
                        sl.sl_dc[(size_id - 2) as usize][matrix_id] =
                            sl.sl_dc[(size_id - 2) as usize][src_matrix];
                    }
                }
                // delta == 0 means use the default already in the array.
            } else {
                // Explicitly coded coefficients.
                let mut next_coef: i32 = 8;
                let coef_num = 64usize.min(1usize << (4 + (size_id as usize * 2)));

                if size_id > 1 {
                    let scaling_list_dc_coef_minus8 = r.read_se()?;
                    if !(-7..=247).contains(&scaling_list_dc_coef_minus8) {
                        return Err(DecodeError::InvalidSyntax(
                            "scaling_list_dc_coef_minus8 out of range",
                        ));
                    }
                    let dc_val = (scaling_list_dc_coef_minus8 + 8) as u8;
                    sl.sl_dc[(size_id - 2) as usize][matrix_id] = dc_val;
                    next_coef = dc_val as i32;
                }

                for i in 0..coef_num {
                    let pos = if size_id == 0 {
                        4 * DIAG_SCAN_4X4_Y[i] + DIAG_SCAN_4X4_X[i]
                    } else {
                        8 * DIAG_SCAN_8X8_Y[i] + DIAG_SCAN_8X8_X[i]
                    };

                    let scaling_list_delta_coef = r.read_se()?;
                    // Wrapping mod-256 arithmetic matching FFmpeg's
                    // `(next_coef + 256U + scaling_list_delta_coef) % 256`.
                    next_coef = (next_coef + scaling_list_delta_coef + 256).rem_euclid(256);
                    sl.sl[size_id as usize][matrix_id][pos] = next_coef as u8;
                }
            }
            matrix_id += step;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_scaling_list() {
        let sl = ScalingList::default_scaling_list();

        // 4×4: all 16 (first 16 entries per matrix).
        for (matrix_id, matrix) in sl.sl[0].iter().enumerate() {
            for (j, entry) in matrix.iter().take(16).enumerate() {
                assert_eq!(*entry, 16, "4x4 [{matrix_id}][{j}]");
            }
        }

        // 8×8 intra (matrix 0,1,2): matches default intra list.
        for matrix in &sl.sl[1][0..3] {
            assert_eq!(&matrix[..], &DEFAULT_SCALING_LIST_INTRA[..]);
        }
        // 8×8 inter (matrix 3,4,5): matches default inter list.
        for matrix in &sl.sl[1][3..6] {
            assert_eq!(&matrix[..], &DEFAULT_SCALING_LIST_INTER[..]);
        }

        // DC coefficients default to 16.
        for row in &sl.sl_dc {
            for &v in row {
                assert_eq!(v, 16);
            }
        }
    }
}
