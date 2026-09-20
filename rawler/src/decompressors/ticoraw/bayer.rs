// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_bayer.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* Bayer reconstruction — the final decode stage.
//!
//! After tiling, each tile's `tile_coeff_buf` holds 32 stripes of 4 sub-band
//! planes at offsets `{0, kBand, 2·kBand, 3·kBand}` = `{LH, LL, HL, HH}`. Two
//! passes turn these into the 16-bit RGGB Bayer image:
//!
//! * [`step1_merge_4_to_2`] — a 2D inverse 5/3 merge of the 4 planes into two
//!   (`L`, `H`), with `>> 3` lifting coefficients and whole-sample-symmetric row
//!   boundaries.
//! * [`step2_bayer_rows`] — the final inverse plus the tone-curve LUT
//!   ([`iqx_iqp_lut`](super::iqx_iqp_lut_data::iqx_iqp_lut)), emitting **two Bayer
//!   rows per stripe row** with a 32768 midpoint bias and 14-bit clamping.
//!
//! ## Plane addressing
//!
//! The reference passes raw plane pointers into the image-wide `tile_coeff_buf`
//! / `step1_scratch`, so a tile's row `-1` / row `w_rows` legitimately read the
//! previous / next tile. To model that safely, this port takes each source
//! buffer as a whole slice plus per-plane **base offsets**, and indexes with
//! signed row arithmetic — so the driver can pass the image-wide buffers and the
//! cross-tile boundary reads land in the neighbouring tile's region.

use super::iqx_iqp_lut_data::IQX_IQP_LUT_SIZE;

/// LUT right-shift after lookup.
const LUT_SHIFT: i32 = 2;
/// LUT output bit width.
const LUT_BIT_WIDTH: i32 = 14;
/// LUT entry count (matches [`IQX_IQP_LUT_SIZE`]).
const LUT_SIZE: i32 = IQX_IQP_LUT_SIZE as i32;
/// Max clamped output value (16383).
const CLIP_MAX: i32 = (1 << LUT_BIT_WIDTH) - 1;

/// Read `buf[base + r·stride + c]` with a signed row index.
#[inline]
fn at(buf: &[i32], base: usize, r: isize, stride: usize, c: usize) -> i32 {
  buf[(base as isize + r * stride as isize + c as isize) as usize]
}

/// Step 1: merge the 4 sub-band planes (`LL`, `LH`, `HH`, `HL`) into `L`/`H` via
/// a 2D inverse 5/3 wavelet with `>> 3` lifting.
///
/// `src` holds all four input planes at `p1_base` (LL), `p2_base` (LH),
/// `p3_base` (HH), `p4_base` (HL); `p1`/`p4` use `stride1`, `p2`/`p3` use
/// `stride2`. `out` receives `L` at `out_l_base` and `H` at `out_h_base` (same
/// buffer, `stride_out`). Faithful to the reference `step1_merge_4_to_2`.
#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
pub fn step1_merge_4_to_2(
  src: &[i32],
  p1_base: usize,
  p2_base: usize,
  p3_base: usize,
  p4_base: usize,
  w_rows: usize,
  w_cols: usize,
  stride1: usize,
  stride2: usize,
  out: &mut [i32],
  out_l_base: usize,
  out_h_base: usize,
  stride_out: usize,
  is_first_tile: bool,
  is_last_tile: bool,
) {
  if w_rows == 0 || w_cols == 0 {
    return;
  }

  for r in 0..w_rows {
    let ri = r as isize;
    let prev_r = if r == 0 && is_first_tile { 0 } else { ri - 1 };
    let next_r = if r == w_rows - 1 && is_last_tile { ri } else { ri + 1 };
    let l_row = out_l_base + r * stride_out;
    let h_row = out_h_base + r * stride_out;

    // Column c = 0 (boundary).
    let cp0 = if w_cols == 1 { 0 } else { 1 };
    let hh_sum = at(src, p3_base, ri, stride2, 0) + at(src, p3_base, ri, stride2, cp0);
    let lh_predicted = at(src, p2_base, ri, stride2, 0) - ((hh_sum + at(src, p3_base, prev_r, stride2, 0) + at(src, p3_base, prev_r, stride2, cp0)) >> 3);
    let lh_next_predicted =
      at(src, p2_base, next_r, stride2, 0) - ((hh_sum + at(src, p3_base, next_r, stride2, 0) + at(src, p3_base, next_r, stride2, cp0)) >> 3);

    let ll_hl_sum = at(src, p1_base, ri, stride1, 0) + at(src, p4_base, ri, stride1, 0);
    out[l_row] = lh_predicted - ((ll_hl_sum + at(src, p1_base, ri, stride1, cp0) + at(src, p4_base, prev_r, stride1, 0)) >> 3);

    let pred_2x = (lh_predicted + lh_next_predicted) << 1;
    let hh_predicted = at(src, p3_base, ri, stride2, 0) - ((ll_hl_sum + at(src, p4_base, ri, stride1, 0) + at(src, p1_base, next_r, stride1, 0)) >> 3);
    out[h_row] = hh_predicted + (pred_2x >> 2);

    // Carry from c = 0 (identical formulae as the c=0 block above).
    let mut carry_pred = lh_predicted;
    let mut carry_next_pred = lh_next_predicted;

    // Columns c = 1 .. w_cols - 1.
    for c in 1..w_cols {
      let cp = if c == w_cols - 1 { c } else { c + 1 };

      let hh_col_sum = at(src, p3_base, ri, stride2, c) + at(src, p3_base, ri, stride2, cp);
      let cur_predict = at(src, p2_base, ri, stride2, c) - ((hh_col_sum + at(src, p3_base, prev_r, stride2, cp) + at(src, p3_base, prev_r, stride2, c)) >> 3);
      let next_predict =
        at(src, p2_base, next_r, stride2, c) - ((hh_col_sum + at(src, p3_base, next_r, stride2, c) + at(src, p3_base, next_r, stride2, cp)) >> 3);

      let ll_hl_col_sum = at(src, p1_base, ri, stride1, c) + at(src, p4_base, ri, stride1, c);
      out[l_row + c] = cur_predict - ((ll_hl_col_sum + at(src, p1_base, ri, stride1, cp) + at(src, p4_base, prev_r, stride1, c)) >> 3);

      let pred_sum_4way = carry_pred + cur_predict + next_predict + carry_next_pred;
      let hh_col_predicted =
        at(src, p3_base, ri, stride2, c) - ((ll_hl_col_sum + at(src, p4_base, ri, stride1, c - 1) + at(src, p1_base, next_r, stride1, c)) >> 3);
      out[h_row + c] = hh_col_predicted + (pred_sum_4way >> 2);

      carry_pred = cur_predict;
      carry_next_pred = next_predict;
    }
  }
}

/// Step 2: final inverse + tone-curve LUT → two `u16` Bayer rows per stripe row.
///
/// `coeff` holds `p1` (LL) at `p1_base` and `p4` (HL) at `p4_base`, both with
/// `stride13`; `step1` holds `p2` (L) at `p2_base` and `p3` (H) at `p3_base`,
/// both with `stride24`. `lut` is the tone-curve LUT. Output Bayer rows go to
/// `out_bayer` at `(tile_row_start + 2r{,+1}) * image_w`. Faithful to the
/// reference `step2_bayer_rows`.
#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
pub fn step2_bayer_rows(
  coeff: &[i32],
  p1_base: usize,
  p4_base: usize,
  step1: &[i32],
  p2_base: usize,
  p3_base: usize,
  w_rows: usize,
  w_cols: usize,
  stride13: usize,
  stride24: usize,
  lut: &[i32],
  out_bayer: &mut [u16],
  image_w: usize,
  tile_row_start: i32,
  is_first_tile: bool,
  is_last_tile: bool,
) {
  let lut_rounding: i32 = if LUT_SHIFT > 0 { 1 << (LUT_SHIFT - 1) } else { 0 };
  let midpoint_bias: i32 = 1 << (LUT_SHIFT + LUT_BIT_WIDTH - 1);

  let lookup = |v: i32| -> u16 {
    let idx = if v < 0 {
      0
    } else if v >= LUT_SIZE {
      (LUT_SIZE - 1) as usize
    } else {
      v as usize
    };
    let mut val = (lut[idx] + lut_rounding) >> LUT_SHIFT;
    if val < 0 {
      val = 0;
    }
    if val > CLIP_MAX {
      val = CLIP_MAX;
    }
    val as u16
  };

  for r in 0..w_rows {
    let ri = r as isize;
    let prev_r = if r == 0 && is_first_tile { 0 } else { ri - 1 };
    let next_r = if r == w_rows - 1 && is_last_tile { ri } else { ri + 1 };

    let bayer_row_top = tile_row_start + 2 * r as i32;
    let bayer_row_bot = tile_row_start + 2 * r as i32 + 1;
    let top_off = if bayer_row_top < 0 { None } else { Some(bayer_row_top as usize * image_w) };
    let bot_off = if bayer_row_bot < 0 { None } else { Some(bayer_row_bot as usize * image_w) };

    // Column c = 0.
    let cp0 = if w_cols == 1 { 0 } else { 1 };
    let hh_c = at(step1, p3_base, ri, stride24, 0);
    let hh_cp = at(step1, p3_base, ri, stride24, cp0);
    let lh_c = at(step1, p2_base, ri, stride24, 0);
    let lh_next_c = at(step1, p2_base, next_r, stride24, 0);
    let hl_c = at(coeff, p4_base, ri, stride13, 0);

    if let Some(t) = top_off {
      let sum_top0 = ((at(step1, p3_base, prev_r, stride24, 0) + lh_c + hh_c + lh_c) >> 2) + midpoint_bias + at(coeff, p1_base, ri, stride13, 0);
      out_bayer[t] = lookup(sum_top0);
      let sum_top1 = lh_c + midpoint_bias;
      out_bayer[t + 1] = lookup(sum_top1);
    }
    if let Some(b) = bot_off {
      let sum_bot0 = hh_c + midpoint_bias;
      out_bayer[b] = lookup(sum_bot0);
      let sum_bot1 = hl_c + midpoint_bias + ((hh_cp + hh_c + lh_c + lh_next_c) >> 2);
      out_bayer[b + 1] = lookup(sum_bot1);
    }

    // Columns c = 1 .. w_cols - 1.
    for c in 1..w_cols {
      let cp = if c == w_cols - 1 { c } else { c + 1 };

      let hh_val_c = at(step1, p3_base, ri, stride24, c);
      let hh_val_cp = at(step1, p3_base, ri, stride24, cp);
      let hh_plus_lh_c = hh_val_c + at(step1, p2_base, ri, stride24, c);
      let lh_next_c2 = at(step1, p2_base, next_r, stride24, c);
      let hl_val_c = at(coeff, p4_base, ri, stride13, c);

      if let Some(t) = top_off {
        let sum_top0 = ((at(step1, p2_base, ri, stride24, c - 1) + at(step1, p3_base, prev_r, stride24, c) + hh_plus_lh_c) >> 2)
          + midpoint_bias
          + at(coeff, p1_base, ri, stride13, c);
        out_bayer[t + 2 * c] = lookup(sum_top0);
        let sum_top1 = at(step1, p2_base, ri, stride24, c) + midpoint_bias;
        out_bayer[t + 2 * c + 1] = lookup(sum_top1);
      }
      if let Some(b) = bot_off {
        let sum_bot0 = hh_val_c + midpoint_bias;
        out_bayer[b + 2 * c] = lookup(sum_bot0);
        let sum_bot1 = hl_val_c + midpoint_bias + ((hh_val_cp + hh_plus_lh_c + lh_next_c2) >> 2);
        out_bayer[b + 2 * c + 1] = lookup(sum_bot1);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::decompressors::ticoraw::iqx_iqp_lut_data::iqx_iqp_lut;
  use crate::decompressors::ticoraw::subband_config::compute_subband_layout;
  use crate::decompressors::ticoraw::tile::{compute_buf_stripe_ints, compute_kband};

  fn parse_ints(s: &str) -> Vec<i32> {
    s.split(',').filter(|t| !t.is_empty()).map(|t| t.parse().unwrap()).collect()
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder).
  //
  // Runs step1 then step2 on an 8x16 window of tile 0's REAL coefficient planes
  // (from the committed tile0_8070.coeff.bin) with is_first = is_last = true — a
  // self-contained block that exercises both row clamps and the left/right
  // column boundaries — and asserts the step1 L/H outputs and the step2 Bayer
  // pixels match the reference's golden window exactly.
  #[test]
  fn oracle_bayer_window_dsc8070_bit_exact() {
    let coeff_bin = include_bytes!("testdata/tile0_8070.coeff.bin");
    let coeff: Vec<i32> = coeff_bin.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();

    let fixture = include_str!("testdata/bayer_window_8070.txt");
    let (mut r, mut c) = (0usize, 0usize);
    let (mut out_l_exp, mut out_h_exp, mut bayer_exp) = (Vec::new(), Vec::new(), Vec::new());
    for line in fixture.lines() {
      if let Some(v) = line.strip_prefix("R=") {
        r = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("C=") {
        c = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("OUTL=") {
        out_l_exp = parse_ints(v.trim());
      } else if let Some(v) = line.strip_prefix("OUTH=") {
        out_h_exp = parse_ints(v.trim());
      } else if let Some(v) = line.strip_prefix("BAYER=") {
        bayer_exp = parse_ints(v.trim());
      }
    }

    let (li, _config) = compute_subband_layout(2800);
    let stripe_ints = compute_buf_stripe_ints(&li);
    let kband = compute_kband(&li);
    // Plane offsets within tile 0 (tile_off = 0): LH=0, LL=kband, HL=2k, HH=3k.
    let (p_lh, p_ll, p_hl, p_hh) = (0, kband, 2 * kband, 3 * kband);

    // step1: p1=LL, p2=LH, p3=HH, p4=HL.
    let mut step1 = vec![0i32; 8 * stripe_ints];
    step1_merge_4_to_2(
      &coeff,
      p_ll,
      p_lh,
      p_hh,
      p_hl,
      r,
      c,
      stripe_ints,
      stripe_ints,
      &mut step1,
      0,
      kband,
      stripe_ints,
      true,
      true,
    );

    let mut got_l = Vec::new();
    let mut got_h = Vec::new();
    for rr in 0..r {
      for cc in 0..c {
        got_l.push(step1[rr * stripe_ints + cc]);
        got_h.push(step1[kband + rr * stripe_ints + cc]);
      }
    }
    assert_eq!(got_l, out_l_exp, "step1 out_L mismatch");
    assert_eq!(got_h, out_h_exp, "step1 out_H mismatch");

    // step2: p1=LL, p4=HL (coeff); p2=L, p3=H (step1).
    let lut = iqx_iqp_lut();
    let mut bayer = vec![0u16; 2 * r * 5600];
    step2_bayer_rows(
      &coeff,
      p_ll,
      p_hl,
      &step1,
      0,
      kband,
      r,
      c,
      stripe_ints,
      stripe_ints,
      lut,
      &mut bayer,
      5600,
      0,
      true,
      true,
    );

    let mut got_bayer = Vec::new();
    for row in 0..2 * r {
      for col in 0..2 * c {
        got_bayer.push(bayer[row * 5600 + col] as i32);
      }
    }
    assert_eq!(got_bayer, bayer_exp, "step2 Bayer mismatch");
  }
}
