// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_predecessor.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* cross-precinct GCLI predecessor state.
//!
//! Two LL sub-bands use vertical (cross-band) GCLI prediction (mode `0x73`):
//!
//! * **sb 12** (pass-A LL): its "previous band" is **sb 23 of the *previous*
//!   precinct**.
//! * **sb 23** (pass-B LL): its "previous band" is **sb 12 of the *current*
//!   precinct**.
//!
//! This is realized with two rotation buffers: `rotation_buf_a` carries sb 23
//! (feeding the next precinct's sb 12), `rotation_buf_b` carries the current
//! precinct's sb 12 (feeding sb 23). All other sub-bands use zero-prediction
//! (mode `0x71`) but still have their GCLIs stored.
//!
//! The reference aliases `gcli_store[12] → rotation_buf_b` and
//! `gcli_store[23] → rotation_buf_a` via raw pointers; this port routes sb 12/23
//! saves and reads through the rotation buffers directly (no aliasing), which is
//! behaviorally identical. Rust ownership makes the C++ `destroy()` unnecessary.
//!
//! Precinct-16 reset: at precinct index 16 all GCLI state is zeroed (see
//! [`should_reset_gcli`]).

use super::subband_config::SubbandConfig;

/// Pass-A LL sub-band (its previous band is sb 23 of the previous precinct).
const PASS_A_LL: usize = 12;
/// Pass-B LL sub-band (its previous band is sb 12 of the current precinct).
const PASS_B_LL: usize = 23;

const NUM_SUBBANDS: usize = 26;

/// Inter-precinct GCLI prediction state for the 26 sub-bands.
#[cfg_attr(not(test), allow(dead_code))]
pub struct PrecinctPredecessorState {
  ng: [usize; NUM_SUBBANDS],
  /// Per-sub-band last-saved GCLIs. Entries 12 and 23 are unused (empty) — those
  /// bands route through the rotation buffers.
  gcli_store: Vec<Vec<u8>>,
  /// Carries sb 23 → feeds sb 12's "previous" on the next precinct.
  rotation_buf_a: Vec<u8>,
  /// Carries the current precinct's sb 12 → feeds sb 23's "previous".
  rotation_buf_b: Vec<u8>,
  fully_insig: [bool; NUM_SUBBANDS],
  precinct_index: i32,
}

#[cfg_attr(not(test), allow(dead_code))]
impl PrecinctPredecessorState {
  /// Build predecessor storage from the sub-band layout (replaces the reference
  /// `init`; Rust drops the buffers, so there is no `destroy`).
  pub fn new(config: &[SubbandConfig; NUM_SUBBANDS]) -> Self {
    let mut ng = [0usize; NUM_SUBBANDS];
    let mut gcli_store = Vec::with_capacity(NUM_SUBBANDS);
    for (i, cfg) in config.iter().enumerate() {
      ng[i] = cfg.ng.max(0) as usize;
      // sb 12/23 live in the rotation buffers; give the others own storage.
      if i == PASS_A_LL || i == PASS_B_LL {
        gcli_store.push(Vec::new());
      } else {
        gcli_store.push(vec![0u8; ng[i]]);
      }
    }
    let rotation_buf_size = ng[PASS_A_LL].max(ng[PASS_B_LL]);
    Self {
      ng,
      gcli_store,
      rotation_buf_a: vec![0u8; rotation_buf_size],
      rotation_buf_b: vec![0u8; rotation_buf_size],
      fully_insig: [false; NUM_SUBBANDS],
      precinct_index: 0,
    }
  }

  /// The "previous band" GCLIs for `sb` (the context for mode-`0x73` prediction).
  ///
  /// sb 12 → sb 23 of the previous precinct; sb 23 → sb 12 of the current
  /// precinct; every other sub-band → its own last-saved GCLIs.
  pub fn get_previous_gcli(&self, sb: usize) -> &[u8] {
    if sb == PASS_A_LL {
      &self.rotation_buf_a
    } else if sb == PASS_B_LL {
      &self.rotation_buf_b
    } else if sb < NUM_SUBBANDS {
      &self.gcli_store[sb]
    } else {
      &[]
    }
  }

  /// Save decoded GCLIs for `sb` (updates the rotation buffers for sb 12/23).
  pub fn save_gcli(&mut self, sb: usize, gcli_values: &[u8]) {
    if sb >= NUM_SUBBANDS {
      return;
    }
    let n = self.ng[sb];
    if n == 0 || gcli_values.len() < n {
      return;
    }
    match sb {
      PASS_A_LL => self.rotation_buf_b[..n].copy_from_slice(&gcli_values[..n]),
      PASS_B_LL => self.rotation_buf_a[..n].copy_from_slice(&gcli_values[..n]),
      _ => self.gcli_store[sb][..n].copy_from_slice(&gcli_values[..n]),
    }
  }

  pub fn set_fully_insig(&mut self, sb: usize, flag: bool) {
    if sb < NUM_SUBBANDS {
      self.fully_insig[sb] = flag;
    }
  }

  pub fn is_fully_insig(&self, sb: usize) -> bool {
    sb < NUM_SUBBANDS && self.fully_insig[sb]
  }

  pub fn precinct_index(&self) -> i32 {
    self.precinct_index
  }

  /// Advance to the next precinct. Zeros the sb 12 buffer (consumed by sb 23);
  /// the sb 23 buffer is retained as the next precinct's sb 12 context.
  pub fn advance_precinct(&mut self) {
    self.precinct_index += 1;
    self.rotation_buf_b.fill(0);
  }

  /// Zero all GCLI prediction state (precinct-16 reset).
  pub fn reset_gcli_state(&mut self) {
    for i in 0..NUM_SUBBANDS {
      if i != PASS_A_LL && i != PASS_B_LL {
        self.gcli_store[i].fill(0);
      }
      self.fully_insig[i] = false;
    }
    self.rotation_buf_a.fill(0);
    self.rotation_buf_b.fill(0);
  }
}

/// Whether the precinct-16 GCLI reset applies. `Bp`/`Br` are quantizer values,
/// not position markers, so the condition is purely `precinct_index == 16`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn should_reset_gcli(precinct_index: i32, _bp: u8, _br: u8) -> bool {
  precinct_index == 16
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::decompressors::ticoraw::subband_config::compute_subband_layout;

  fn state() -> PrecinctPredecessorState {
    // Realistic 5600-wide layout (half-pass 2800): LL bands have ng 700.
    let (_li, config) = compute_subband_layout(2800);
    PrecinctPredecessorState::new(&config)
  }

  #[test]
  fn non_cross_band_saves_and_reads_back() {
    let mut s = state();
    let ng0 = s.ng[0];
    let vals: Vec<u8> = (0..ng0).map(|i| (i % 251) as u8).collect();
    s.save_gcli(0, &vals);
    assert_eq!(s.get_previous_gcli(0), &vals[..]);
  }

  #[test]
  fn sb23_prev_is_sb12_of_current_precinct() {
    let mut s = state();
    let ng12 = s.ng[PASS_A_LL];
    let v12: Vec<u8> = (0..ng12).map(|i| (1 + i % 9) as u8).collect();
    s.save_gcli(PASS_A_LL, &v12);
    // sb 23's previous band = current precinct's sb 12.
    assert_eq!(&s.get_previous_gcli(PASS_B_LL)[..ng12], &v12[..]);
  }

  #[test]
  fn sb12_prev_is_sb23_of_previous_precinct() {
    let mut s = state();
    let ng23 = s.ng[PASS_B_LL];
    let v23: Vec<u8> = (0..ng23).map(|i| (2 + i % 7) as u8).collect();
    // Decode precinct 0: save sb 23, then advance.
    s.save_gcli(PASS_B_LL, &v23);
    s.advance_precinct();
    // In precinct 1, sb 12's previous band = previous precinct's sb 23.
    assert_eq!(&s.get_previous_gcli(PASS_A_LL)[..ng23], &v23[..]);
  }

  #[test]
  fn advance_zeros_sb12_buffer_but_keeps_sb23() {
    let mut s = state();
    let ng12 = s.ng[PASS_A_LL];
    let ng23 = s.ng[PASS_B_LL];
    s.save_gcli(PASS_A_LL, &vec![9u8; ng12]);
    s.save_gcli(PASS_B_LL, &vec![5u8; ng23]);
    s.advance_precinct();
    // sb 23 context (for next sb 12) is retained; sb 12 buffer (for sb 23) zeroed.
    assert!(s.get_previous_gcli(PASS_A_LL)[..ng23].iter().all(|&b| b == 5));
    assert!(s.get_previous_gcli(PASS_B_LL)[..ng12].iter().all(|&b| b == 0));
    assert_eq!(s.precinct_index(), 1);
  }

  #[test]
  fn reset_zeros_everything() {
    let mut s = state();
    s.save_gcli(0, &vec![3u8; s.ng[0]]);
    s.save_gcli(PASS_A_LL, &vec![7u8; s.ng[PASS_A_LL]]);
    s.save_gcli(PASS_B_LL, &vec![8u8; s.ng[PASS_B_LL]]);
    s.set_fully_insig(5, true);
    s.reset_gcli_state();
    assert!(s.get_previous_gcli(0).iter().all(|&b| b == 0));
    assert!(s.get_previous_gcli(PASS_A_LL).iter().all(|&b| b == 0));
    assert!(s.get_previous_gcli(PASS_B_LL).iter().all(|&b| b == 0));
    assert!(!s.is_fully_insig(5));
  }

  #[test]
  fn fully_insig_flag_roundtrips() {
    let mut s = state();
    assert!(!s.is_fully_insig(7));
    s.set_fully_insig(7, true);
    assert!(s.is_fully_insig(7));
    s.set_fully_insig(7, false);
    assert!(!s.is_fully_insig(7));
  }

  #[test]
  fn reset_condition_is_precinct_16() {
    assert!(should_reset_gcli(16, 7, 17));
    assert!(!should_reset_gcli(15, 7, 17));
    assert!(!should_reset_gcli(0, 5, 0));
  }
}
