//! Phase 3d-1: Decoded Picture Buffer (DPB) infrastructure.
//!
//! HEVC references pictures by POC (picture order count) rather than by
//! index. The DPB is a pool of recently decoded pictures, each marked as
//! "short-term reference", "long-term reference", or "unused for reference".
//! Pictures are removed from the DPB when no future slice in decode order
//! could reference them any more.
//!
//! **Phase 3d-1 scope**: only the data structures + insertion / lookup /
//! marking plumbing. No actual inter decoding uses the DPB yet — see Phase
//! 3d-2 / 3d-3 for that. The purpose of landing the DPB now is to give
//! non-IDR slice header parsing somewhere to look up references and mark
//! them.
//!
//! The lifetime shape intentionally uses `Rc<RefCell<...>>` so that the
//! same decoded picture can be simultaneously held by the DPB (for
//! marking / lookup) and later by a `RefPicList` entry during inter
//! decode, without resorting to raw pointers or indices that would be
//! invalidated when the DPB is compacted.

use std::cell::RefCell;
use std::rc::Rc;

use crate::pixel::PixelData;
use crate::sps::Sps;

/// How this picture is currently marked in the DPB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureReferenceStatus {
    /// No future slice can reference this picture.
    UnusedForReference,
    /// Short-term reference (POC delta relative to the current picture).
    ShortTerm,
    /// Long-term reference (identified by POC LSB).
    LongTerm,
}

/// One decoded picture's planes + metadata.
#[derive(Debug)]
pub struct DecodedPicture {
    /// Luma plane in raster order.
    pub y: PixelData,
    /// Cb plane.
    pub u: PixelData,
    /// Cr plane.
    pub v: PixelData,
    pub width: u32,
    pub height: u32,
    /// Signed picture order count (may be negative if a non-IDR with
    /// earlier LSB appears after an IDR — in practice this is only
    /// non-positive for RASL / RADL pictures, which Phase 3d-1 doesn't
    /// decode anyway).
    pub poc: i32,
    /// Reference marking state. Mutable since a picture's state changes
    /// over its lifetime in the DPB.
    pub reference_status: RefCell<PictureReferenceStatus>,
    /// Whether the picture has been output yet (emitted as a `Frame`).
    pub output: RefCell<bool>,

    /// Phase 3e: per min-PU motion field stored when inserting into the DPB.
    /// Used by temporal merge/AMVP candidate derivation (spec 8.5.3.2.3 / 8.5.3.2.7).
    pub tab_mvf: Vec<crate::cu_tree::MvField>,
    /// Phase 3e: `log2_min_pu_size` of this picture (needed to index `tab_mvf`).
    pub log2_min_pu_size: u8,
    /// Phase 3e: min-PU width = `width >> log2_min_pu_size`.
    pub min_pu_width: usize,
    /// Phase 3e: `log2_ctb_size` of this picture (needed for CTB row check in temporal candidate).
    pub log2_ctb_size: u8,
    /// Phase 3e: per-list reference POCs at the time this picture was decoded.
    /// Used to compute POC distances for temporal MV scaling.
    /// `ref_pic_list_pocs[0]` = L0 POCs, `ref_pic_list_pocs[1]` = L1 POCs.
    pub ref_pic_list_pocs: [Vec<i32>; 2],
}

impl DecodedPicture {
    pub fn new(
        y: PixelData,
        u: PixelData,
        v: PixelData,
        width: u32,
        height: u32,
        poc: i32,
    ) -> Self {
        Self {
            y,
            u,
            v,
            width,
            height,
            poc,
            reference_status: RefCell::new(PictureReferenceStatus::ShortTerm),
            output: RefCell::new(false),
            tab_mvf: Vec::new(),
            log2_min_pu_size: 2,
            min_pu_width: 0,
            log2_ctb_size: 0,
            ref_pic_list_pocs: [Vec::new(), Vec::new()],
        }
    }

    /// Create a `DecodedPicture` with temporal MVP data (tab_mvf and ref lists)
    /// for use in temporal merge/AMVP candidate derivation.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_mvf(
        y: PixelData,
        u: PixelData,
        v: PixelData,
        width: u32,
        height: u32,
        poc: i32,
        tab_mvf: Vec<crate::cu_tree::MvField>,
        log2_min_pu_size: u8,
        min_pu_width: usize,
        log2_ctb_size: u8,
        ref_pic_list_pocs: [Vec<i32>; 2],
    ) -> Self {
        Self {
            y,
            u,
            v,
            width,
            height,
            poc,
            reference_status: RefCell::new(PictureReferenceStatus::ShortTerm),
            output: RefCell::new(false),
            tab_mvf,
            log2_min_pu_size,
            min_pu_width,
            log2_ctb_size,
            ref_pic_list_pocs,
        }
    }

    pub fn mark(&self, status: PictureReferenceStatus) {
        *self.reference_status.borrow_mut() = status;
    }

    pub fn reference_status(&self) -> PictureReferenceStatus {
        *self.reference_status.borrow()
    }

    pub fn is_reference(&self) -> bool {
        !matches!(
            self.reference_status(),
            PictureReferenceStatus::UnusedForReference
        )
    }
}

/// The decoded picture buffer.
///
/// Holds a pool of recently decoded pictures. Phase 3d-1 does not
/// implement proper bumping / reorder-aware output — it only provides the
/// scaffolding so later phases can wire in actual inter decoding and
/// display ordering.
#[derive(Debug, Default)]
pub struct DecodedPictureBuffer {
    pictures: Vec<Rc<DecodedPicture>>,
    /// Maximum number of pictures that may sit in the DPB at once
    /// (`sps_max_dec_pic_buffering_minus1 + 1`). Zero means "unbounded";
    /// the caller will fill it in from the active SPS.
    max_dec_pic_buffering: usize,
    /// `sps_max_num_reorder_pics` — the maximum number of pictures that
    /// may sit in the DPB before the current picture in decode order
    /// precedes the current picture in output order. Used by `bump()`.
    max_num_reorder_pics: usize,
    /// `sps_max_latency_increase_plus1` — when non-zero, a latency bound
    /// on how long a picture may sit in the DPB awaiting output.
    #[allow(dead_code)]
    max_latency_increase_plus1: u32,
}

impl DecodedPictureBuffer {
    /// Create an empty DPB. Call `configure_from_sps` on the first slice
    /// of each new sequence to populate the sizing fields from the SPS.
    pub fn new() -> Self {
        Self::default()
    }

    /// Populate sizing fields from the active SPS. Should be called once
    /// per activated SPS (idempotent — calling twice is harmless).
    pub fn configure_from_sps(&mut self, sps: &Sps) {
        self.max_dec_pic_buffering = (sps.sps_max_dec_pic_buffering_minus1 + 1) as usize;
        self.max_num_reorder_pics = sps.sps_max_num_reorder_pics as usize;
        self.max_latency_increase_plus1 = sps.sps_max_latency_increase_plus1;
    }

    /// Insert a freshly decoded picture into the DPB. The new picture
    /// starts out marked as `ShortTerm` per HEVC conventions; callers
    /// that need a different initial state can `mark()` it afterwards.
    pub fn insert(&mut self, pic: Rc<DecodedPicture>) {
        self.pictures.push(pic);
    }

    /// Total number of pictures currently in the DPB.
    pub fn len(&self) -> usize {
        self.pictures.len()
    }

    /// Returns true when the DPB contains no pictures.
    pub fn is_empty(&self) -> bool {
        self.pictures.is_empty()
    }

    /// Find a picture by POC. Returns `Some(...)` for the first match,
    /// `None` if no picture in the DPB has the given POC.
    pub fn find_by_poc(&self, poc: i32) -> Option<Rc<DecodedPicture>> {
        self.pictures.iter().find(|p| p.poc == poc).cloned()
    }

    /// Find a short-term reference picture by POC. Like `find_by_poc`
    /// but skips pictures that aren't currently marked as ST references.
    pub fn find_st_ref(&self, poc: i32) -> Option<Rc<DecodedPicture>> {
        self.pictures
            .iter()
            .find(|p| p.poc == poc && p.reference_status() == PictureReferenceStatus::ShortTerm)
            .cloned()
    }

    /// Find a long-term reference picture by POC LSB. Checks the picture's
    /// full POC's low bits against `poc_lsb`, which mirrors how HEVC LT
    /// refs are identified (spec 8.3.2).
    pub fn find_lt_ref_by_lsb(&self, poc_lsb: u32, max_poc_lsb: i32) -> Option<Rc<DecodedPicture>> {
        self.pictures
            .iter()
            .find(|p| {
                p.reference_status() == PictureReferenceStatus::LongTerm
                    && (p.poc.rem_euclid(max_poc_lsb)) == poc_lsb as i32
            })
            .cloned()
    }

    /// Mark every reference picture in the DPB as `UnusedForReference`.
    /// Used when the current RPS has been fully resolved and we need to
    /// flip everything that wasn't referenced back to unused state.
    pub fn unmark_all_references(&self) {
        for p in &self.pictures {
            p.mark(PictureReferenceStatus::UnusedForReference);
        }
    }

    /// Remove every picture that is both "unused for reference" and has
    /// already been output. Mirrors FFmpeg's DPB cleanup step. Safe to
    /// call after every picture insertion.
    pub fn cleanup_unused(&mut self) {
        self.pictures
            .retain(|p| p.is_reference() || !*p.output.borrow());
    }

    /// Phase 3d-1 stub: return the next picture in output order if the
    /// DPB is "full enough" to bump one out, or `None` otherwise. Because
    /// we don't yet handle reorder-aware output, this always returns None
    /// — the `Decoder` emits each picture immediately after it is decoded.
    /// Phase 3d-3+ will replace this with a real bumping process.
    pub fn bump(&mut self) -> Option<Rc<DecodedPicture>> {
        None
    }

    /// Direct access to the pictures list, for iteration by the caller
    /// when deriving the reference picture set.
    pub fn pictures(&self) -> &[Rc<DecodedPicture>] {
        &self.pictures
    }
}

/// Phase 3d-1: derived reference picture set groupings per spec 8.3.2.
///
/// The five sets are the input to `RefPicList0` / `RefPicList1`
/// construction (which Phase 3d-1 doesn't yet do). We compute them here
/// so that inter-decode phases can slot straight in.
#[derive(Debug, Default, Clone)]
pub struct ReferencePictureSets {
    /// Short-term "before current picture" references that the current
    /// picture itself uses (used_by_curr_pic_s0_flag = 1).
    pub st_curr_before: Vec<i32>,
    /// Short-term "after current picture" references that the current
    /// picture itself uses.
    pub st_curr_after: Vec<i32>,
    /// Short-term references that the current picture does NOT use but
    /// which must still be kept around (they're referenced by later
    /// pictures according to the RPS).
    pub st_foll: Vec<i32>,
    /// Long-term references that the current picture uses. Each entry is
    /// a full POC (when `lt_poc_msb_present` is true for that index) or a
    /// POC LSB (when false).
    pub lt_curr: Vec<i32>,
    /// Long-term references that the current picture does not use but
    /// which later pictures will.
    pub lt_foll: Vec<i32>,
    /// Per-entry flag for `lt_curr` ++ `lt_foll` (in that order) indicating
    /// whether `delta_poc_msb_present_flag` was set. When true, the
    /// corresponding entry in `lt_curr`/`lt_foll` is a full POC and DPB
    /// matching uses exact POC. When false, it's a POC LSB and DPB matching
    /// uses `poc % MaxPicOrderCntLsb`.
    pub lt_poc_msb_present: Vec<bool>,
}

impl ReferencePictureSets {
    /// Total number of "current" references (= `NumPocTotalCurr` in the
    /// spec). Used to decide whether `ref_pic_lists_modification` can
    /// appear in the slice header, etc.
    pub fn num_poc_total_curr(&self) -> usize {
        self.st_curr_before.len() + self.st_curr_after.len() + self.lt_curr.len()
    }
}

/// Phase 3d-2: construct `RefPicListTemp0` per spec 8.3.2.
///
/// The temp list is built by concatenating (in order) the POCs in
/// `st_curr_before`, `st_curr_after`, and `lt_curr`, repeating the
/// concatenation if necessary until the list is at least
/// `num_rps_curr_temp_list` entries long. Pure function on POCs so it
/// can be tested without a real DPB.
pub fn build_ref_pic_list_temp0(
    rps: &ReferencePictureSets,
    num_rps_curr_temp_list: usize,
) -> Vec<i32> {
    build_ref_pic_list_temp(
        &[&rps.st_curr_before, &rps.st_curr_after, &rps.lt_curr],
        num_rps_curr_temp_list,
    )
}

/// Phase 3d-2: construct `RefPicListTemp1` per spec 8.3.2.
///
/// Same as `build_ref_pic_list_temp0` but with `st_curr_after` visited
/// **before** `st_curr_before` — for B-slice L1 construction.
pub fn build_ref_pic_list_temp1(
    rps: &ReferencePictureSets,
    num_rps_curr_temp_list: usize,
) -> Vec<i32> {
    build_ref_pic_list_temp(
        &[&rps.st_curr_after, &rps.st_curr_before, &rps.lt_curr],
        num_rps_curr_temp_list,
    )
}

/// Shared helper for L0 / L1 temp list construction. Takes a list of
/// sub-lists in visit order and concatenates them (wrapping around) until
/// the result has at least `target_len` entries.
fn build_ref_pic_list_temp(sub_lists: &[&Vec<i32>], target_len: usize) -> Vec<i32> {
    let mut out: Vec<i32> = Vec::with_capacity(target_len);
    if target_len == 0 {
        return out;
    }
    // Empty concatenation would loop forever; bail to avoid that (callers
    // should only ever build this list for P/B slices which by spec
    // require at least one reference).
    let total_in_sub_lists: usize = sub_lists.iter().map(|s| s.len()).sum();
    if total_in_sub_lists == 0 {
        return out;
    }
    while out.len() < target_len {
        for sub in sub_lists {
            for poc in sub.iter() {
                if out.len() >= target_len {
                    break;
                }
                out.push(*poc);
            }
            if out.len() >= target_len {
                break;
            }
        }
    }
    out
}

/// Phase 3d-2: apply `ref_pic_list_modification()` to a temp list to
/// produce `RefPicList{0,1}` (spec 8.3.2 eq. 8-8 / 8-10). When the
/// modification flag is false, returns the first `num_active` entries of
/// the temp list unchanged. When true, the i-th output entry is
/// `temp_list[list_entry[i]]`.
pub fn apply_ref_pic_list_modification(
    temp_list: &[i32],
    modification_flag: bool,
    list_entry: &[u32],
    num_active: usize,
) -> Result<Vec<i32>, crate::error::DecodeError> {
    let mut out: Vec<i32> = Vec::with_capacity(num_active);
    if modification_flag {
        if list_entry.len() < num_active {
            return Err(crate::error::DecodeError::InvalidSyntax(
                "ref_pic_list_modification has fewer entries than active ref count",
            ));
        }
        for &idx in list_entry.iter().take(num_active) {
            let idx = idx as usize;
            if idx >= temp_list.len() {
                return Err(crate::error::DecodeError::InvalidSyntax(
                    "list_entry index out of temp list bounds",
                ));
            }
            out.push(temp_list[idx]);
        }
    } else {
        if temp_list.len() < num_active {
            return Err(crate::error::DecodeError::InvalidSyntax(
                "temp list shorter than active ref count",
            ));
        }
        out.extend_from_slice(&temp_list[..num_active]);
    }
    Ok(out)
}

/// Phase 3d-2: resolve a list of reference-picture POCs to actual DPB
/// entries. Each POC in the list must be present in the DPB; an
/// unresolved POC is a hard error. Matches FFmpeg's `find_ref_idx`
/// behavior when no "generate missing ref" fallback is required.
/// Look up each POC in the DPB, returning the corresponding pictures.
///
/// Only returns pictures that are currently marked as a reference (ShortTerm
/// or LongTerm). This ensures that the ref lists only contain pictures that
/// the RPS marking step actually retained — matching FFmpeg's behavior where
/// `find_ref_idx` checks the marking status.
pub fn resolve_ref_pics(
    dpb: &DecodedPictureBuffer,
    pocs: &[i32],
) -> Result<Vec<Rc<DecodedPicture>>, crate::error::DecodeError> {
    let mut out: Vec<Rc<DecodedPicture>> = Vec::with_capacity(pocs.len());
    for poc in pocs {
        // First try ST, then LT, then any-status (fallback for edge cases
        // where the marking hasn't settled yet, e.g. first slice of a new
        // picture before apply_rps_marking runs at picture completion).
        let pic = dpb
            .find_st_ref(*poc)
            .or_else(|| {
                dpb.pictures()
                    .iter()
                    .find(|p| {
                        p.poc == *poc && p.reference_status() == PictureReferenceStatus::LongTerm
                    })
                    .cloned()
            })
            .or_else(|| dpb.find_by_poc(*poc));
        match pic {
            Some(p) => out.push(p),
            None => {
                return Err(crate::error::DecodeError::InvalidSyntax(
                    "reference picture POC not present in DPB",
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_lookup_by_poc() {
        let mut dpb = DecodedPictureBuffer::new();
        let pic = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![0; 16 * 16]),
            PixelData::U8(vec![0; 8 * 8]),
            PixelData::U8(vec![0; 8 * 8]),
            16,
            16,
            5,
        ));
        dpb.insert(pic);
        assert_eq!(dpb.len(), 1);
        assert!(dpb.find_by_poc(5).is_some());
        assert!(dpb.find_by_poc(4).is_none());
    }

    #[test]
    fn mark_short_term_reference() {
        let pic = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            10,
        ));
        assert_eq!(pic.reference_status(), PictureReferenceStatus::ShortTerm);
        pic.mark(PictureReferenceStatus::LongTerm);
        assert_eq!(pic.reference_status(), PictureReferenceStatus::LongTerm);
        pic.mark(PictureReferenceStatus::UnusedForReference);
        assert!(!pic.is_reference());
    }

    #[test]
    fn cleanup_drops_unreferenced_and_output() {
        let mut dpb = DecodedPictureBuffer::new();
        let a = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            0,
        ));
        a.mark(PictureReferenceStatus::UnusedForReference);
        *a.output.borrow_mut() = true;
        let b = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            1,
        ));
        // b is still a reference.
        dpb.insert(a);
        dpb.insert(b);
        dpb.cleanup_unused();
        assert_eq!(dpb.len(), 1);
        assert_eq!(dpb.pictures()[0].poc, 1);
    }

    #[test]
    fn num_poc_total_curr_sums_three_sets() {
        let rps = ReferencePictureSets {
            st_curr_before: vec![-1, -2],
            st_curr_after: vec![1],
            st_foll: vec![-4],
            lt_curr: vec![100],
            lt_foll: vec![],
            lt_poc_msb_present: vec![],
        };
        assert_eq!(rps.num_poc_total_curr(), 4);
    }

    #[test]
    fn ref_pic_list_temp0_order_before_then_after_then_lt() {
        // Spec 8.3.2: RefPicListTemp0 concatenates st_curr_before,
        // st_curr_after, lt_curr — in that order.
        let rps = ReferencePictureSets {
            st_curr_before: vec![3, 2],
            st_curr_after: vec![5, 6],
            st_foll: vec![],
            lt_curr: vec![100],
            lt_foll: vec![],
            lt_poc_msb_present: vec![],
        };
        // NumPocTotalCurr = 5, so with num_rps_curr_temp_list = 5 we expect
        // exactly the concatenation, no wrap.
        let temp = build_ref_pic_list_temp0(&rps, 5);
        assert_eq!(temp, vec![3, 2, 5, 6, 100]);
    }

    #[test]
    fn ref_pic_list_temp1_order_after_then_before_then_lt() {
        // Spec 8.3.2: RefPicListTemp1 concatenates st_curr_after,
        // st_curr_before, lt_curr — note After comes BEFORE Before.
        let rps = ReferencePictureSets {
            st_curr_before: vec![3, 2],
            st_curr_after: vec![5, 6],
            st_foll: vec![],
            lt_curr: vec![100],
            lt_foll: vec![],
            lt_poc_msb_present: vec![],
        };
        let temp = build_ref_pic_list_temp1(&rps, 5);
        assert_eq!(temp, vec![5, 6, 3, 2, 100]);
    }

    #[test]
    fn ref_pic_list_temp0_wraps_around_when_active_exceeds_rps() {
        // `num_ref_idx_l0_active > NumPocTotalCurr` → temp list wraps.
        // RPS = [3, 2], active = 5 → temp = [3, 2, 3, 2, 3].
        let rps = ReferencePictureSets {
            st_curr_before: vec![3, 2],
            st_curr_after: vec![],
            st_foll: vec![],
            lt_curr: vec![],
            lt_foll: vec![],
            lt_poc_msb_present: vec![],
        };
        let temp = build_ref_pic_list_temp0(&rps, 5);
        assert_eq!(temp, vec![3, 2, 3, 2, 3]);
    }

    #[test]
    fn ref_pic_list_modification_reorders_temp_list() {
        // list_entry_l0 = [1, 0] → result is [temp[1], temp[0]] = [42, 10].
        let temp = vec![10, 42, 99];
        let modified = apply_ref_pic_list_modification(&temp, true, &[1, 0], 2).expect("apply mod");
        assert_eq!(modified, vec![42, 10]);
    }

    #[test]
    fn ref_pic_list_modification_disabled_uses_temp_prefix() {
        let temp = vec![10, 42, 99, 7];
        let result = apply_ref_pic_list_modification(&temp, false, &[], 3).expect("no mod");
        assert_eq!(result, vec![10, 42, 99]);
    }

    #[test]
    fn ref_pic_list_modification_rejects_out_of_bounds_entry() {
        let temp = vec![10, 42];
        let err = apply_ref_pic_list_modification(&temp, true, &[5], 1);
        assert!(err.is_err());
    }

    #[test]
    fn resolve_ref_pics_looks_up_by_poc() {
        let mut dpb = DecodedPictureBuffer::new();
        let p0 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            2,
        ));
        let p1 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            5,
        ));
        dpb.insert(p0);
        dpb.insert(p1);
        let resolved = resolve_ref_pics(&dpb, &[5, 2]).expect("resolve");
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].poc, 5);
        assert_eq!(resolved[1].poc, 2);
    }

    #[test]
    fn resolve_ref_pics_errors_on_missing_poc() {
        let dpb = DecodedPictureBuffer::new();
        let err = resolve_ref_pics(&dpb, &[3]);
        assert!(err.is_err());
    }

    /// resolve_ref_pics prefers ST-marked pictures over unmarked ones.
    #[test]
    fn resolve_ref_pics_prefers_short_term() {
        let mut dpb = DecodedPictureBuffer::new();
        let p = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            7,
        ));
        // Default status is ShortTerm.
        assert_eq!(p.reference_status(), PictureReferenceStatus::ShortTerm);
        dpb.insert(p);
        let resolved = resolve_ref_pics(&dpb, &[7]).expect("resolve");
        assert_eq!(resolved[0].poc, 7);
        assert_eq!(
            resolved[0].reference_status(),
            PictureReferenceStatus::ShortTerm
        );
    }

    /// resolve_ref_pics finds LT-marked pictures by POC.
    #[test]
    fn resolve_ref_pics_finds_long_term() {
        let mut dpb = DecodedPictureBuffer::new();
        let p = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            7,
        ));
        p.mark(PictureReferenceStatus::LongTerm);
        dpb.insert(p);
        let resolved = resolve_ref_pics(&dpb, &[7]).expect("resolve");
        assert_eq!(
            resolved[0].reference_status(),
            PictureReferenceStatus::LongTerm
        );
    }

    /// resolve_ref_pics falls back to unmarked pictures (needed during
    /// the transition window before apply_rps_marking runs at picture
    /// completion).
    #[test]
    fn resolve_ref_pics_fallback_unmarked() {
        let mut dpb = DecodedPictureBuffer::new();
        let p = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            7,
        ));
        p.mark(PictureReferenceStatus::UnusedForReference);
        dpb.insert(p);
        let resolved = resolve_ref_pics(&dpb, &[7]).expect("resolve");
        assert_eq!(resolved[0].poc, 7);
    }

    /// apply_rps_marking marks ST refs and leaves others unused.
    #[test]
    fn apply_rps_marking_marks_st_and_unused() {
        use crate::decoder::Decoder;

        let mut dpb = DecodedPictureBuffer::new();
        let p0 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            0,
        ));
        let p4 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            4,
        ));
        let p8 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            8,
        ));
        dpb.insert(p0.clone());
        dpb.insert(p4.clone());
        dpb.insert(p8.clone());

        // RPS: only POC 0 and 8 are ST refs. POC 4 is not referenced.
        let rps = ReferencePictureSets {
            st_curr_before: vec![0],
            st_curr_after: vec![8],
            st_foll: vec![],
            lt_curr: vec![],
            lt_foll: vec![],
            lt_poc_msb_present: vec![],
        };
        Decoder::apply_rps_marking(&rps, &dpb, 8);

        assert_eq!(p0.reference_status(), PictureReferenceStatus::ShortTerm);
        assert_eq!(
            p4.reference_status(),
            PictureReferenceStatus::UnusedForReference
        );
        assert_eq!(p8.reference_status(), PictureReferenceStatus::ShortTerm);
    }

    /// apply_rps_marking: LT marking uses POC LSB and doesn't override ST.
    #[test]
    fn apply_rps_marking_lt_by_lsb_no_st_override() {
        use crate::decoder::Decoder;

        let mut dpb = DecodedPictureBuffer::new();
        // Two pictures: POC 3 (ST ref) and POC 19 (LT ref by LSB=3 with max_lsb=16).
        let p3 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            3,
        ));
        let p19 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            19,
        ));
        dpb.insert(p3.clone());
        dpb.insert(p19.clone());

        let rps = ReferencePictureSets {
            st_curr_before: vec![3],
            st_curr_after: vec![],
            st_foll: vec![],
            lt_curr: vec![3], // LSB=3, matches both POC 3 and POC 19 (19%16=3)
            lt_foll: vec![],
            lt_poc_msb_present: vec![false, false], // LSB-based matching for both
        };
        // log2_max_poc_lsb=4 → max_poc_lsb=16
        Decoder::apply_rps_marking(&rps, &dpb, 4);

        // POC 3 should be ST (ST takes priority over LT).
        assert_eq!(p3.reference_status(), PictureReferenceStatus::ShortTerm);
        // POC 19 (LSB=3) should be LT (not overridden by ST since POC 19 != 3).
        assert_eq!(p19.reference_status(), PictureReferenceStatus::LongTerm);
    }

    /// LT marking with poc_msb_present=true uses full POC matching (not LSB).
    #[test]
    fn apply_rps_marking_lt_full_poc_matching() {
        use crate::decoder::Decoder;

        let mut dpb = DecodedPictureBuffer::new();
        // Two pictures with the SAME POC LSB (3) but different full POCs.
        // log2_max_poc_lsb=4 → max_poc_lsb=16
        let p3 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            3,
        ));
        let p19 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            19,
        ));
        dpb.insert(p3.clone());
        dpb.insert(p19.clone());

        // LT ref with poc_msb_present=true and full POC=19 → should match
        // only POC 19, not POC 3 (even though both have LSB=3).
        let rps = ReferencePictureSets {
            st_curr_before: vec![],
            st_curr_after: vec![],
            st_foll: vec![],
            lt_curr: vec![19], // full POC (msb_present=true)
            lt_foll: vec![],
            lt_poc_msb_present: vec![true],
        };
        Decoder::apply_rps_marking(&rps, &dpb, 4);

        assert_eq!(
            p3.reference_status(),
            PictureReferenceStatus::UnusedForReference
        );
        assert_eq!(p19.reference_status(), PictureReferenceStatus::LongTerm);
    }

    /// LT marking with poc_msb_present=false uses LSB matching.
    #[test]
    fn apply_rps_marking_lt_lsb_matching() {
        use crate::decoder::Decoder;

        let mut dpb = DecodedPictureBuffer::new();
        // POC 19 has LSB=3 (19%16=3)
        let p19 = Rc::new(DecodedPicture::new(
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            PixelData::U8(vec![]),
            0,
            0,
            19,
        ));
        dpb.insert(p19.clone());

        // LT ref with poc_msb_present=false and LSB=3 → matches POC 19
        let rps = ReferencePictureSets {
            st_curr_before: vec![],
            st_curr_after: vec![],
            st_foll: vec![],
            lt_curr: vec![3], // POC LSB (msb_present=false)
            lt_foll: vec![],
            lt_poc_msb_present: vec![false],
        };
        Decoder::apply_rps_marking(&rps, &dpb, 4);

        assert_eq!(p19.reference_status(), PictureReferenceStatus::LongTerm);
    }
}
