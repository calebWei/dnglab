// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_gcli_decode.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* GCLI (greatest-coded-line-index) decode for one sub-band.
//!
//! Each sub-band's coefficients are grouped (Cw=4 coeffs per group); the GCLI is
//! the number of significant bit-planes for a group. GCLIs are entropy-coded
//! across two substreams:
//!
//! * the **significance** substream carries one bit per *block of 8 groups*:
//!   `1` = the whole block takes the baseline prediction (unary code 0);
//!   `0` = each group in the block carries its own unary delta in the GCLI
//!   substream.
//! * the **GCLI** substream carries the per-group unary deltas.
//!
//! Two prediction modes are used:
//!
//! * `0x71` — **zero-prediction** (all non-LL sub-bands). Baseline = `gtli`;
//!   per-element `gcli = gtli + unary`.
//! * `0x73` — **vertical prediction** (the LL bands, sub-bands 12 & 23), using the
//!   previous band's per-group GCLIs as context through the prediction LUT.
//!   Baseline = `max(prev, gtli)`; per-element via [`lookup_prediction`].

use super::bit_reader::BitReader;
use super::predict_lut::{lookup_prediction, PREDICTION_LUT_SIZE};

/// Zero-prediction mode (baseline = `gtli`).
pub const MODE_ZERO_PRED: i32 = 0x71;
/// Vertical-prediction mode (LL bands; baseline = `max(prev, gtli)`).
#[cfg_attr(not(test), allow(dead_code))]
pub const MODE_VERT_PRED: i32 = 0x73;

/// Decode the per-group GCLI values for one sub-band.
///
/// `mode` is `0x71` (zero-prediction) or `0x73` (vertical prediction). For mode
/// `0x73`, `previous_gcli` must hold at least `num_groups` entries (the previous
/// band's GCLIs); for `0x71` it is ignored. Writes exactly `num_groups` bytes to
/// `gcli_output`.
///
/// Faithful to the reference `decode_gcli_values`: significance is read one bit
/// per 8-group block; a set bit takes the baseline for the whole (possibly short
/// trailing) block, a clear bit reads a unary delta per group.
// Consumed by `precinct_decode` (not yet ported).
#[cfg_attr(not(test), allow(dead_code))]
pub fn decode_gcli_values(
  sig_reader: &mut BitReader,
  gcli_reader: &mut BitReader,
  mode: i32,
  num_groups: usize,
  gtli: i32,
  predict_lut: &[u8; PREDICTION_LUT_SIZE],
  previous_gcli: Option<&[u8]>,
  gcli_output: &mut [u8],
) {
  // One significance bit per block of 8 GCLI groups.
  let num_sig_blocks = num_groups.div_ceil(8);

  for block in 0..num_sig_blocks {
    let base = block * 8;
    let block_size = core::cmp::min(8, num_groups - base);

    let sig_bit = sig_reader.read_bits(1);

    if sig_bit == 1 {
      // "All-baseline" block: every GCLI = prediction with unary_code 0.
      if mode == MODE_ZERO_PRED {
        // predict(gtli, 0, 0) = gtli.
        for slot in &mut gcli_output[base..base + block_size] {
          *slot = gtli as u8;
        }
      } else {
        // predict(gtli, prev, 0) = max(prev, gtli).
        let prev = previous_gcli.expect("mode 0x73 requires previous_gcli");
        for i in 0..block_size {
          let p = prev[base + i] as i32;
          gcli_output[base + i] = core::cmp::max(p, gtli) as u8;
        }
      }
    } else {
      // Per-element decode: each GCLI carries its own unary delta.
      if mode == MODE_ZERO_PRED {
        // gcli = gtli + unary.
        for slot in &mut gcli_output[base..base + block_size] {
          let u = gcli_reader.read_unary();
          *slot = (gtli + u as i32) as u8;
        }
      } else {
        let prev = previous_gcli.expect("mode 0x73 requires previous_gcli");
        for i in 0..block_size {
          let u = gcli_reader.read_unary();
          let p = prev[base + i] as i32;
          gcli_output[base + i] = lookup_prediction(predict_lut, gtli, p, u as i32);
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::decompressors::ticoraw::predict_lut::prediction_lut;

  // Pack a sequence of unary codes MSB-first into bytes: value v -> v ones then a
  // zero (matches BitReader::read_unary).
  fn pack_unary(codes: &[u32]) -> Vec<u8> {
    let mut bits: Vec<u8> = Vec::new();
    for &c in codes {
      bits.resize(bits.len() + c as usize, 1);
      bits.push(0);
    }
    pack_bits(&bits)
  }

  // Pack MSB-first bits into bytes (zero-padding the final byte).
  fn pack_bits(bits: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cur = 0u8;
    let mut n = 0u8;
    for &b in bits {
      cur = (cur << 1) | (b & 1);
      n += 1;
      if n == 8 {
        out.push(cur);
        cur = 0;
        n = 0;
      }
    }
    if n > 0 {
      cur <<= 8 - n;
      out.push(cur);
    }
    out
  }

  #[test]
  fn mode71_all_baseline_block() {
    // 8 groups, sig bit = 1 -> all GCLIs = gtli. No GCLI substream reads.
    let sig = pack_bits(&[1]);
    let gcli = [0u8; 4];
    let lut = prediction_lut();
    let mut sr = BitReader::new(&sig);
    let mut gr = BitReader::new(&gcli);
    let mut out = [0u8; 8];
    decode_gcli_values(&mut sr, &mut gr, MODE_ZERO_PRED, 8, 5, lut, None, &mut out);
    assert_eq!(out, [5; 8]);
  }

  #[test]
  fn mode71_per_element_deltas() {
    // 8 groups, sig bit = 0 -> each group gcli = gtli + unary.
    let sig = pack_bits(&[0]);
    let codes = [0u32, 1, 2, 3, 0, 4, 1, 0];
    let gcli = pack_unary(&codes);
    let lut = prediction_lut();
    let mut sr = BitReader::new(&sig);
    let mut gr = BitReader::new(&gcli);
    let mut out = [0u8; 8];
    let gtli = 2;
    decode_gcli_values(&mut sr, &mut gr, MODE_ZERO_PRED, 8, gtli, lut, None, &mut out);
    let expected: Vec<u8> = codes.iter().map(|&u| (gtli as u32 + u) as u8).collect();
    assert_eq!(&out[..], &expected[..]);
  }

  #[test]
  fn mode71_short_trailing_block() {
    // 10 groups -> 2 sig blocks: block 0 (8, baseline), block 1 (2 groups, deltas).
    let sig = pack_bits(&[1, 0]);
    let codes = [3u32, 1]; // only the 2 trailing groups read deltas
    let gcli = pack_unary(&codes);
    let lut = prediction_lut();
    let mut sr = BitReader::new(&sig);
    let mut gr = BitReader::new(&gcli);
    let mut out = [0u8; 10];
    let gtli = 4;
    decode_gcli_values(&mut sr, &mut gr, MODE_ZERO_PRED, 10, gtli, lut, None, &mut out);
    assert_eq!(&out[0..8], &[4u8; 8]);
    assert_eq!(out[8], (4 + 3) as u8);
    assert_eq!(out[9], (4 + 1) as u8);
  }

  #[test]
  fn mode73_baseline_is_max_prev_gtli() {
    // sig bit = 1 -> gcli = max(prev, gtli).
    let sig = pack_bits(&[1]);
    let gcli = [0u8; 4];
    let prev = [0u8, 1, 2, 3, 4, 5, 6, 7];
    let lut = prediction_lut();
    let mut sr = BitReader::new(&sig);
    let mut gr = BitReader::new(&gcli);
    let mut out = [0u8; 8];
    let gtli = 3;
    decode_gcli_values(&mut sr, &mut gr, MODE_VERT_PRED, 8, gtli, lut, Some(&prev), &mut out);
    let expected: Vec<u8> = prev.iter().map(|&p| core::cmp::max(p as i32, gtli) as u8).collect();
    assert_eq!(&out[..], &expected[..]);
  }

  fn from_hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder).
  //
  // Captured from the reference (yogthos/LibRaw nikon-he-decoder + dx_sig_fix)
  // decoding the real HE file DSC_8070.NEF: precinct 0, line-block 0. The 6
  // sub-bands of LB 0 share ONE sig reader and ONE gcli reader (state persists
  // mid-byte across sub-bands), so the fixture replays all 6 in order against a
  // single pair of readers — exactly as `decode_precinct` does. LB 0 uses mode
  // 0x73 with an all-zero previous-band context. See tools/nikon_he_oracle
  // (NIKON_HE_GCLI_DUMP capture) and NIKON_HE_PROJECT.md §6a.
  #[test]
  fn oracle_lb0_dsc8070_bit_exact() {
    let fixture = include_str!("testdata/gcli_lb0_8070.txt");
    let mut sig_hex = "";
    let mut gcli_hex = "";
    // (ng, gtli, expected_out)
    let mut subs: Vec<(usize, i32, Vec<u8>)> = Vec::new();
    for line in fixture.lines() {
      if let Some(h) = line.strip_prefix("SIG=") {
        sig_hex = h;
      } else if let Some(h) = line.strip_prefix("GCLI=") {
        gcli_hex = h;
      } else if let Some(rest) = line.strip_prefix("SB ") {
        let f: Vec<&str> = rest.split_whitespace().collect();
        // fields: <sb> <ng> <gtli> <out_hex>
        let ng: usize = f[1].parse().unwrap();
        let gtli: i32 = f[2].parse().unwrap();
        let out = from_hex(f[3]);
        assert_eq!(out.len(), ng);
        subs.push((ng, gtli, out));
      }
    }
    assert_eq!(subs.len(), 6, "LB0 has 6 sub-bands");

    let sig = from_hex(sig_hex);
    let gcli = from_hex(gcli_hex);
    let lut = prediction_lut();
    let mut sr = BitReader::new(&sig);
    let mut gr = BitReader::new(&gcli);

    for (idx, (ng, gtli, expected)) in subs.iter().enumerate() {
      let prev = vec![0u8; *ng]; // LB 0 previous-band context is all zeros
      let mut out = vec![0u8; *ng];
      decode_gcli_values(&mut sr, &mut gr, MODE_VERT_PRED, *ng, *gtli, lut, Some(&prev), &mut out);
      assert_eq!(&out, expected, "sub-band {idx} (ng={ng}, gtli={gtli}) mismatch vs reference");
    }
  }

  #[test]
  fn mode73_per_element_matches_lut() {
    // sig bit = 0 -> gcli = lookup_prediction(gtli, prev, unary).
    let sig = pack_bits(&[0]);
    let codes = [0u32, 1, 2, 0, 1, 3, 0, 2];
    let gcli = pack_unary(&codes);
    let prev = [1u8, 1, 2, 3, 0, 4, 5, 2];
    let lut = prediction_lut();
    let mut sr = BitReader::new(&sig);
    let mut gr = BitReader::new(&gcli);
    let mut out = [0u8; 8];
    let gtli = 2;
    decode_gcli_values(&mut sr, &mut gr, MODE_VERT_PRED, 8, gtli, lut, Some(&prev), &mut out);
    for i in 0..8 {
      let expect = lookup_prediction(lut, gtli, prev[i] as i32, codes[i] as i32);
      assert_eq!(out[i], expect, "group {i}");
    }
  }
}
