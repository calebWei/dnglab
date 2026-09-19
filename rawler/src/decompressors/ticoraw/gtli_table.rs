// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_gtli_table.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* GTLI (greatest-truncated-level-info) resolution.
//!
//! For each precinct the entropy decoder needs, per sub-band, the *GTLI*: the
//! number of least-significant bit-planes discarded at encode time (0..=15).
//! It is a function of the precinct's `(Bp, Br)` pair and the per-band WGT
//! `gain`/`priority` weights carried in the picture header.
//!
//! # Two paths (faithful to the reference)
//!
//! * **Dynamic (live path).** When a picture header with WGT weights is
//!   available — always the case for a real Nikon codestream — GTLI is computed
//!   as `clamp(Qp - gain[w] - (priority[w] < Rp), 0, 15)` (see
//!   [`PictureHeader::gtli_from_weights`]). This is the path the decoder runs.
//! * **Static fallback.** The reference also ships a table of `(Bp, Br)` → 26
//!   GTLI rows captured from real files, used only when *no* picture header is
//!   set. Our decoder always parses the header, so this table is never the live
//!   source; it is retained as a regression fixture and as ground-truth for the
//!   band-remap invariant test below.
//!
//! # 26 sub-bands vs 25 WGT bands
//!
//! The decoder indexes 26 sub-bands but WGT declares only 25. Sub-band 23 (the
//! pass-B LL band) reuses the WGT entry of sub-band 12 (the pass-A LL band);
//! bands above 23 shift down by one. See [`wgt_index_for_band`]. This is
//! independently confirmed by the static table: every captured row satisfies
//! `values[23] == values[12]` (see `static_rows_share_ll_band`).

use super::picture_header::PictureHeader;

/// Number of sub-bands the decoder resolves a GTLI for.
pub const NUM_SUBBANDS: usize = 26;

/// Pass-A LL sub-band index (has its own WGT entry).
const PASS_A_LL_BAND: i32 = 12;
/// Pass-B LL sub-band index (shares the pass-A LL WGT entry).
const PASS_B_LL_BAND: i32 = 23;

/// Map a 0..26 sub-band index to its 0..25 WGT weight index.
///
/// Bands below the pass-B LL band map 1:1; the pass-B LL band (23) reuses the
/// pass-A LL entry (12); bands above it shift down by one so the 26 sub-bands
/// fold onto the 25 WGT entries.
#[inline]
pub fn wgt_index_for_band(b: i32) -> i32 {
  if b < PASS_B_LL_BAND {
    b
  } else if b == PASS_B_LL_BAND {
    PASS_A_LL_BAND
  } else {
    b - 1
  }
}

/// Compute the full 26-entry GTLI array for `(bp, br)` from the header weights.
///
/// This mirrors the reference's active-picture-header branch of
/// `lookup_gtli_table`: it always computes (never consults the static table).
// Consumed by `precinct_decode` (not yet ported).
#[cfg_attr(not(test), allow(dead_code))]
pub fn compute_gtli_table(ph: &PictureHeader, bp: i32, br: i32) -> [u8; NUM_SUBBANDS] {
  let mut out = [0u8; NUM_SUBBANDS];
  for (b, slot) in out.iter_mut().enumerate() {
    let w = wgt_index_for_band(b as i32);
    *slot = ph.gtli_from_weights(w, bp, br) as u8;
  }
  out
}

/// GTLI for a single sub-band, computed from the header weights.
///
/// Equivalent to `compute_gtli_table(ph, bp, br)[sb]` but without materializing
/// the whole array. Mirrors the reference `lookup_gtli_for_sub_band` on its live
/// (picture-header-present) path; the reference's `0xFF`-unknown sentinel only
/// arises on the static-table path, which is dead when a header is present.
// Consumed by `precinct_decode` (not yet ported).
#[cfg_attr(not(test), allow(dead_code))]
#[inline]
pub fn gtli_for_sub_band(ph: &PictureHeader, bp: i32, br: i32, sb: usize) -> u8 {
  let w = wgt_index_for_band(sb as i32);
  ph.gtli_from_weights(w, bp, br) as u8
}

/// A captured `(Bp, Br)` → 26-GTLI row from the reference's static fallback.
#[cfg_attr(not(test), allow(dead_code))]
pub struct GtliRow {
  pub bp: i32,
  pub br: i32,
  pub values: [u8; NUM_SUBBANDS],
}

/// The reference static fallback table (used only when no picture header is set).
///
/// Retained verbatim from `nikon_he_gtli_table.cpp` as a regression fixture and
/// as ground-truth for the band-remap invariant. HE rows have `Bp` 4..5; HE\*
/// rows have `Bp` 1..3.
#[cfg_attr(not(test), allow(dead_code))]
pub static STATIC_GTLI_ROWS: &[GtliRow] = &[
  // Bp=4 rows (HE)
  GtliRow { bp: 4, br: 0, values: [1, 1, 2, 2, 3, 3, 2, 3, 3, 4, 4, 4, 4, 2, 3, 3, 4, 4, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 1, values: [1, 1, 2, 2, 3, 3, 2, 3, 3, 4, 4, 4, 4, 2, 3, 3, 3, 4, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 2, values: [1, 1, 2, 2, 3, 3, 2, 3, 3, 3, 4, 4, 4, 2, 3, 3, 3, 4, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 3, values: [0, 1, 2, 2, 3, 3, 2, 3, 3, 3, 4, 4, 4, 2, 3, 3, 3, 4, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 4, values: [0, 1, 2, 2, 3, 3, 2, 2, 3, 3, 4, 4, 4, 2, 3, 3, 3, 4, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 5, values: [0, 1, 2, 2, 3, 3, 2, 2, 3, 3, 4, 4, 4, 2, 2, 3, 3, 4, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 6, values: [0, 1, 2, 2, 3, 3, 1, 2, 3, 3, 4, 4, 4, 2, 2, 3, 3, 4, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 7, values: [0, 1, 2, 2, 3, 3, 1, 2, 3, 3, 4, 4, 4, 1, 2, 3, 3, 4, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 11, values: [0, 1, 1, 2, 2, 3, 1, 2, 3, 3, 3, 4, 4, 1, 2, 3, 3, 3, 4, 3, 4, 4, 4, 4, 4, 4] },
  GtliRow { bp: 4, br: 12, values: [0, 1, 1, 2, 2, 3, 1, 2, 3, 3, 3, 4, 3, 1, 2, 3, 3, 3, 4, 3, 4, 4, 4, 3, 4, 4] },
  // Bp=5 rows (HE)
  GtliRow { bp: 5, br: 12, values: [1, 2, 2, 3, 3, 4, 2, 3, 4, 4, 4, 5, 4, 2, 3, 4, 4, 4, 5, 4, 5, 5, 5, 4, 5, 5] },
  GtliRow { bp: 5, br: 13, values: [1, 2, 2, 3, 3, 4, 2, 3, 4, 4, 4, 5, 4, 2, 3, 4, 4, 4, 5, 4, 5, 5, 5, 4, 4, 5] },
  GtliRow { bp: 5, br: 14, values: [1, 2, 2, 3, 3, 4, 2, 3, 4, 4, 4, 4, 4, 2, 3, 4, 4, 4, 5, 4, 5, 5, 5, 4, 4, 5] },
  GtliRow { bp: 5, br: 15, values: [1, 2, 2, 3, 3, 4, 2, 3, 4, 4, 4, 4, 4, 2, 3, 4, 4, 4, 4, 4, 5, 5, 5, 4, 4, 5] },
  GtliRow { bp: 5, br: 16, values: [1, 2, 2, 3, 3, 4, 2, 3, 4, 4, 4, 4, 4, 2, 3, 4, 4, 4, 4, 4, 5, 4, 5, 4, 4, 5] },
  GtliRow { bp: 5, br: 20, values: [1, 1, 2, 3, 3, 4, 2, 3, 3, 4, 4, 4, 4, 2, 3, 3, 4, 4, 4, 4, 4, 4, 5, 4, 4, 5] },
  GtliRow { bp: 5, br: 21, values: [1, 1, 2, 3, 3, 4, 2, 3, 3, 4, 4, 4, 4, 2, 3, 3, 4, 4, 4, 3, 4, 4, 5, 4, 4, 5] },
  GtliRow { bp: 5, br: 22, values: [1, 1, 2, 3, 3, 3, 2, 3, 3, 4, 4, 4, 4, 2, 3, 3, 4, 4, 4, 3, 4, 4, 5, 4, 4, 5] },
  GtliRow { bp: 5, br: 23, values: [1, 1, 2, 2, 3, 3, 2, 3, 3, 4, 4, 4, 4, 2, 3, 3, 4, 4, 4, 3, 4, 4, 5, 4, 4, 5] },
  GtliRow { bp: 5, br: 24, values: [1, 1, 2, 2, 3, 3, 2, 3, 3, 4, 4, 4, 4, 2, 3, 3, 4, 4, 4, 3, 4, 4, 4, 4, 4, 5] },
  // HE* (Lossy_High_Efficiency_Star) rows
  GtliRow { bp: 1, br: 0, values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1] },
  GtliRow { bp: 1, br: 1, values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1] },
  GtliRow { bp: 1, br: 7, values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1] },
  GtliRow { bp: 1, br: 8, values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1] },
  GtliRow { bp: 1, br: 11, values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1] },
  GtliRow { bp: 2, br: 0, values: [0, 0, 0, 1, 1, 2, 0, 1, 1, 2, 2, 2, 2, 0, 1, 1, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2] },
  GtliRow { bp: 2, br: 1, values: [0, 0, 0, 1, 1, 2, 0, 1, 1, 2, 2, 2, 2, 0, 0, 1, 1, 2, 2, 1, 2, 2, 2, 2, 2, 2] },
  GtliRow { bp: 2, br: 3, values: [0, 0, 0, 0, 1, 1, 0, 1, 1, 2, 2, 2, 2, 0, 0, 1, 1, 2, 2, 1, 2, 2, 2, 2, 2, 2] },
  GtliRow { bp: 2, br: 4, values: [0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 2, 0, 0, 1, 1, 2, 2, 1, 2, 2, 2, 2, 2, 2] },
  GtliRow { bp: 2, br: 7, values: [0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 2, 0, 0, 1, 1, 1, 2, 1, 2, 2, 2, 2, 2, 2] },
  GtliRow { bp: 2, br: 8, values: [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 2, 2, 2, 0, 0, 1, 1, 1, 2, 1, 2, 2, 2, 2, 2, 2] },
  GtliRow { bp: 2, br: 10, values: [0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 2, 2, 0, 0, 1, 1, 1, 2, 1, 2, 2, 2, 2, 2, 2] },
  GtliRow { bp: 2, br: 11, values: [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 2, 2, 0, 0, 1, 1, 1, 2, 1, 2, 2, 2, 2, 2, 2] },
  GtliRow { bp: 2, br: 12, values: [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 2, 1, 0, 0, 1, 1, 1, 2, 1, 2, 2, 2, 1, 2, 2] },
  GtliRow { bp: 2, br: 13, values: [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 2, 1, 0, 0, 1, 1, 1, 2, 1, 2, 2, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 14, values: [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 2, 1, 2, 2, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 15, values: [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 16, values: [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 17, values: [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 2, 1, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 18, values: [0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 2, 1, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 20, values: [0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 21, values: [0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 23, values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 2, 1, 1, 2] },
  GtliRow { bp: 2, br: 24, values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 2] },
  GtliRow { bp: 3, br: 8, values: [0, 0, 1, 1, 2, 2, 0, 1, 2, 2, 2, 3, 3, 0, 1, 2, 2, 3, 3, 2, 3, 3, 3, 3, 3, 3] },
  GtliRow { bp: 3, br: 11, values: [0, 0, 0, 1, 1, 2, 0, 1, 2, 2, 2, 3, 3, 0, 1, 2, 2, 2, 3, 2, 3, 3, 3, 3, 3, 3] },
  GtliRow { bp: 3, br: 12, values: [0, 0, 0, 1, 1, 2, 0, 1, 2, 2, 2, 3, 2, 0, 1, 2, 2, 2, 3, 2, 3, 3, 3, 2, 3, 3] },
  GtliRow { bp: 3, br: 13, values: [0, 0, 0, 1, 1, 2, 0, 1, 2, 2, 2, 3, 2, 0, 1, 2, 2, 2, 3, 2, 3, 3, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 14, values: [0, 0, 0, 1, 1, 2, 0, 1, 2, 2, 2, 2, 2, 0, 1, 2, 2, 2, 3, 2, 3, 3, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 15, values: [0, 0, 0, 1, 1, 2, 0, 1, 2, 2, 2, 2, 2, 0, 1, 2, 2, 2, 2, 2, 3, 3, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 16, values: [0, 0, 0, 1, 1, 2, 0, 1, 2, 2, 2, 2, 2, 0, 1, 2, 2, 2, 2, 2, 3, 2, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 17, values: [0, 0, 0, 1, 1, 2, 0, 1, 2, 2, 2, 2, 2, 0, 1, 1, 2, 2, 2, 2, 3, 2, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 18, values: [0, 0, 0, 1, 1, 2, 0, 1, 1, 2, 2, 2, 2, 0, 1, 1, 2, 2, 2, 2, 3, 2, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 20, values: [0, 0, 0, 1, 1, 2, 0, 1, 1, 2, 2, 2, 2, 0, 1, 1, 2, 2, 2, 2, 2, 2, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 21, values: [0, 0, 0, 1, 1, 2, 0, 1, 1, 2, 2, 2, 2, 0, 1, 1, 2, 2, 2, 1, 2, 2, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 22, values: [0, 0, 0, 1, 1, 1, 0, 1, 1, 2, 2, 2, 2, 0, 1, 1, 2, 2, 2, 1, 2, 2, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 23, values: [0, 0, 0, 0, 1, 1, 0, 1, 1, 2, 2, 2, 2, 0, 1, 1, 2, 2, 2, 1, 2, 2, 3, 2, 2, 3] },
  GtliRow { bp: 3, br: 24, values: [0, 0, 0, 0, 1, 1, 0, 1, 1, 2, 2, 2, 2, 0, 1, 1, 2, 2, 2, 1, 2, 2, 2, 2, 2, 3] },
];

/// Look up the 26-entry GTLI row for `(bp, br)` in the static fallback table.
///
/// Returns `None` for an unknown combination (mirrors the reference's `nullptr`
/// / `0xFF` sentinel). Not used on the live decode path — see the module docs.
#[cfg_attr(not(test), allow(dead_code))]
pub fn lookup_static_gtli_table(bp: i32, br: i32) -> Option<&'static [u8; NUM_SUBBANDS]> {
  STATIC_GTLI_ROWS.iter().find(|r| r.bp == bp && r.br == br).map(|r| &r.values)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::decompressors::ticoraw::picture_header::MAX_WGT_BANDS;

  // A picture header carrying arbitrary per-band WGT weights, for exercising the
  // dynamic path in isolation (no codestream needed).
  fn ph_with_weights(gain: &[u8], priority: &[u8]) -> PictureHeader {
    let mut ph = PictureHeader::default();
    assert_eq!(gain.len(), priority.len());
    let n = gain.len();
    assert!(n <= MAX_WGT_BANDS);
    ph.gain[..n].copy_from_slice(gain);
    ph.priority[..n].copy_from_slice(priority);
    ph.nbands = n as i32;
    ph.valid = true;
    ph
  }

  #[test]
  fn band_remap_folds_26_onto_25() {
    // 1:1 below the pass-B LL band.
    assert_eq!(wgt_index_for_band(0), 0);
    assert_eq!(wgt_index_for_band(11), 11);
    assert_eq!(wgt_index_for_band(12), 12);
    assert_eq!(wgt_index_for_band(22), 22);
    // Pass-B LL (23) reuses pass-A LL (12).
    assert_eq!(wgt_index_for_band(23), 12);
    // Above it, shift down by one.
    assert_eq!(wgt_index_for_band(24), 23);
    assert_eq!(wgt_index_for_band(25), 24);
    // The 26 sub-bands must cover exactly WGT indices 0..=24.
    let mut seen = [false; 25];
    for b in 0..NUM_SUBBANDS as i32 {
      seen[wgt_index_for_band(b) as usize] = true;
    }
    assert!(seen.iter().all(|&s| s), "every WGT band 0..=24 is referenced");
  }

  #[test]
  fn dynamic_matches_gtli_formula() {
    // 25 bands; band w has gain=w%6, priority=w.
    let gain: Vec<u8> = (0..25u8).map(|w| w % 6).collect();
    let priority: Vec<u8> = (0..25u8).collect();
    let ph = ph_with_weights(&gain, &priority);

    let table = compute_gtli_table(&ph, 5, 4);
    // Spot-check sub-band 3 (w=3): clamp(5 - 3 - (3<4)) = clamp(5-3-1)=1.
    assert_eq!(table[3], 1);
    // Sub-band 6 (w=6, gain=0, priority=6): clamp(5-0-(6<4?)) = 5.
    assert_eq!(table[6], 5);
    // gtli_for_sub_band must agree with the array.
    for sb in 0..NUM_SUBBANDS {
      assert_eq!(gtli_for_sub_band(&ph, 5, 4, sb), table[sb]);
    }
  }

  #[test]
  fn dynamic_shares_ll_band() {
    // Pass-A LL (12) and pass-B LL (23) must resolve identically for any (Bp,Br)
    // because both use WGT band 12.
    let gain: Vec<u8> = (0..25u8).map(|w| (w * 7) % 5).collect();
    let priority: Vec<u8> = (0..25u8).map(|w| (w * 3) % 25).collect();
    let ph = ph_with_weights(&gain, &priority);
    for bp in 0..8 {
      for br in 0..25 {
        let t = compute_gtli_table(&ph, bp, br);
        assert_eq!(t[12], t[23], "LL bands must match at Bp={bp} Br={br}");
      }
    }
  }

  #[test]
  fn dynamic_clamps_to_0_and_15() {
    let ph = ph_with_weights(&[0u8; 25], &[0u8; 25]);
    // Qp below the floor -> 0 everywhere.
    assert!(compute_gtli_table(&ph, -3, 0).iter().all(|&v| v == 0));
    // Qp far above the ceiling -> 15 everywhere (gain=0, priority>=Rp so no -1).
    assert!(compute_gtli_table(&ph, 100, 0).iter().all(|&v| v == 15));
  }

  // GROUND-TRUTH INVARIANT: the static rows were captured from real files. Every
  // row must share the pass-A/pass-B LL value, independently confirming the
  // 26->25 band remap (sub-band 23 reuses WGT band 12).
  #[test]
  fn static_rows_share_ll_band() {
    for r in STATIC_GTLI_ROWS {
      assert_eq!(
        r.values[23], r.values[12],
        "static row Bp={} Br={} must have values[23]==values[12]",
        r.bp, r.br
      );
      // All GTLI values are in the valid 0..=15 range.
      assert!(r.values.iter().all(|&v| v <= 15));
    }
  }

  #[test]
  fn static_lookup_hit_and_miss() {
    // A known HE row.
    let row = lookup_static_gtli_table(4, 0).expect("Bp=4,Br=0 present");
    assert_eq!(row[0], 1);
    assert_eq!(row[12], 4);
    // A known HE* row.
    assert!(lookup_static_gtli_table(1, 0).is_some());
    // Unknown combination.
    assert!(lookup_static_gtli_table(9, 9).is_none());
  }
}
