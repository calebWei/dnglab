// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-production), src/decoders/nikon_he/
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! GCLI prediction lookup table.
//!
//! Maps `(gtli, previous_gcli, unary_code)` → predicted GCLI, used during GCLI
//! (Greatest Coded Line Index) decode for prediction modes. 8192 entries
//! (16 × 16 × 32), indexed `(gtli << 9) | (previous_gcli << 5) | unary_code`.
//!
//! `m_top = max(previous_gcli, gtli)` is the baseline; the unary code encodes a
//! zigzag delta within `[−threshold, +threshold]` (threshold = m_top − gtli),
//! then a one-sided escape beyond `2*threshold`.

use std::sync::OnceLock;

pub const PREDICTION_LUT_SIZE: usize = 8192;
pub const PREDICTION_LUT_SENTINEL: u8 = 0xFF;

fn build() -> [u8; PREDICTION_LUT_SIZE] {
  let mut lut = [0u8; PREDICTION_LUT_SIZE];
  for gtli in 0..16i32 {
    for previous_gcli in 0..16i32 {
      let m_top = previous_gcli.max(gtli);
      let threshold = m_top - gtli;
      let max_stored = 15 + threshold;
      for unary_code in 0..32i32 {
        let delta = if unary_code == 0 {
          0
        } else if unary_code <= 2 * threshold {
          // zigzag: odd -> negative, even -> positive
          if unary_code & 1 != 0 {
            -((unary_code + 1) / 2)
          } else {
            unary_code / 2
          }
        } else {
          unary_code - threshold
        };
        let gcli = m_top + delta;
        let index = ((gtli << 9) | (previous_gcli << 5) | unary_code) as usize;
        lut[index] = if gcli > max_stored { PREDICTION_LUT_SENTINEL } else { gcli as u8 };
      }
    }
  }
  lut
}

/// Return the cached prediction table (built once).
pub fn prediction_lut() -> &'static [u8; PREDICTION_LUT_SIZE] {
  static LUT: OnceLock<[u8; PREDICTION_LUT_SIZE]> = OnceLock::new();
  LUT.get_or_init(build)
}

#[inline]
pub fn lookup_prediction(lut: &[u8; PREDICTION_LUT_SIZE], gtli: i32, previous_gcli: i32, unary_code: i32) -> u8 {
  let index = ((gtli << 9) | (previous_gcli << 5) | unary_code) as usize;
  lut[index]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn zero_prediction_escape() {
    let lut = prediction_lut();
    // gtli=0, prev=0: m_top=0, threshold=0 -> u==0 delta 0; u>0 escape delta=u.
    assert_eq!(lookup_prediction(lut, 0, 0, 0), 0);
    assert_eq!(lookup_prediction(lut, 0, 0, 5), 5);
    assert_eq!(lookup_prediction(lut, 0, 0, 15), 15);
    assert_eq!(lookup_prediction(lut, 0, 0, 16), PREDICTION_LUT_SENTINEL); // 16 > max_stored 15
  }

  #[test]
  fn zigzag_pattern() {
    let lut = prediction_lut();
    // gtli=0, prev=3: m_top=3, threshold=3, max_stored=18.
    assert_eq!(lookup_prediction(lut, 0, 3, 0), 3); // delta 0
    assert_eq!(lookup_prediction(lut, 0, 3, 1), 2); // -1
    assert_eq!(lookup_prediction(lut, 0, 3, 2), 4); // +1
    assert_eq!(lookup_prediction(lut, 0, 3, 3), 1); // -2
    assert_eq!(lookup_prediction(lut, 0, 3, 4), 5); // +2
    assert_eq!(lookup_prediction(lut, 0, 3, 5), 0); // -3
    assert_eq!(lookup_prediction(lut, 0, 3, 6), 6); // +3
    assert_eq!(lookup_prediction(lut, 0, 3, 7), 7); // escape: 7-3=4 -> 3+4
  }

  #[test]
  fn stable_across_calls() {
    assert_eq!(prediction_lut().as_ptr(), prediction_lut().as_ptr());
  }
}
