// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_idwt_horizontal.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* horizontal inverse 5/3 (LeGall) DWT.
//!
//! Three layers:
//!
//! 1. [`idwt53_inverse_one_level`] — one synthesis level: interleave the L
//!    (even) and H (odd) samples, then apply the inverse UPDATE (even sites) and
//!    inverse PREDICT (odd sites) lifting steps with whole-sample-symmetric
//!    boundary extension.
//! 2. [`idwt53_horizontal_lift_all`] — the full multi-level IDWT for one lift
//!    line-block, deepest level first, seeded from the deepest LL. Uses two
//!    ping-pong buffers (`out` / `work`) chosen so the final result lands in
//!    `out` by parity.
//! 3. [`idwt_horizontal_pass`] — one pass (A or B) of a precinct: runs the lift
//!    line-blocks and memcpies the LL line-block, scattering into the LB-ordered
//!    output stripe (`h_out`).
//!
//! ## Porting notes
//!
//! * The reference reads `config[0].layout_info`; this port takes the
//!   [`LayoutInfo`] explicitly (matching [`super::subband_config`], which returns
//!   it by value alongside the config).
//! * The reference header notes a side effect ("`in_buf[0..n/2)` is overwritten
//!   with state for the vertical IDWT"). The actual reference code never writes
//!   `in_buf` (`lift_all` only reads it and ping-pongs between `out`/`work`), and
//!   the vertical stage reads `h_out`, not `in_buf`. This port matches the code:
//!   the input buffer is left untouched.

use super::subband_config::{LayoutInfo, SubbandConfig};

const NUM_SUBBANDS: usize = 26;
/// Pass-A LL sub-band (memcpy line-block for pass A).
const PASS_A_LL: usize = 12;
/// Pass-B LL sub-band (memcpy line-block for pass B).
const PASS_B_LL: usize = 23;

#[inline]
fn round_up(x: i32, m: i32) -> i32 {
  ((x + m - 1) / m) * m
}

/// One synthesis level of the LeGall 5/3 inverse DWT.
///
/// `l` (the `n_L` low-pass samples) go to even output positions, `h` (the `n_H`
/// high-pass samples) to odd positions; `out` must be exactly `n_L + n_H` long.
/// Inverse UPDATE then inverse PREDICT, with whole-sample-symmetric extension.
#[cfg_attr(not(test), allow(dead_code))]
pub fn idwt53_inverse_one_level(l: &[i32], h: &[i32], out: &mut [i32]) {
  let n_l = l.len();
  let n_h = h.len();
  let n = n_l + n_h;
  if n == 0 {
    return;
  }
  debug_assert_eq!(out.len(), n, "out must be exactly n_L + n_H long");

  // Step 1: L at even positions, H at odd positions.
  for (i, &v) in l.iter().enumerate() {
    let pos = 2 * i;
    if pos < n {
      out[pos] = v;
    }
  }
  for (i, &v) in h.iter().enumerate() {
    let pos = 2 * i + 1;
    if pos < n {
      out[pos] = v;
    }
  }

  // Step 2: inverse UPDATE on even sites — s[i] -= (s[i-1] + s[i+1] + 2) >> 2,
  // with s[-1] = s[1] and s[n] = s[n-2] (whole-sample-symmetric).
  let mut i = 0;
  while i < n {
    let left = if i > 0 {
      out[i - 1]
    } else if n > 1 {
      out[1]
    } else {
      0
    };
    let right = if i + 1 < n {
      out[i + 1]
    } else if i > 0 {
      out[i - 1]
    } else {
      0
    };
    out[i] -= (left + right + 2) >> 2;
    i += 2;
  }

  // Step 3: inverse PREDICT on odd sites — s[i] += (s[i-1] + s[i+1]) >> 1.
  let mut i = 1;
  while i < n {
    let left = out[i - 1];
    let right = if i + 1 < n { out[i + 1] } else { out[i - 1] };
    out[i] += (left + right) >> 1;
    i += 2;
  }
}

/// Full multi-level horizontal 5/3 IDWT for one lift line-block.
///
/// `in_buf` holds the LL band at `[0..N[levels])` and each `H_k` at
/// `[hl_offsets[k]..)` (index 0 of `hl_offsets` is the unused end-of-LB
/// sentinel). `n` is the reconstructed sample count, `levels` the decomposition
/// depth (5 for pass A, 1 for pass B). The result lands in `out[0..n)`; `work`
/// is scratch of at least `n` ints. Parity of `levels` selects the seed buffer
/// so the final swap leaves the result in `out`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn idwt53_horizontal_lift_all(in_buf: &[i32], n: usize, levels: usize, hl_offsets: &[usize], out: &mut [i32], work: &mut [i32]) {
  if levels < 1 || n < 1 {
    return;
  }

  // N[k]: sample count at level k. N[0] = n; N[k] = ceil(N[k-1] / 2).
  let mut nn = vec![0usize; levels + 1];
  nn[0] = n;
  for k in 1..=levels {
    nn[k] = (nn[k - 1] + 1) / 2;
  }

  // Ping-pong so the final result lands in `out` (see reference parity choice).
  let (mut cur, mut dst): (&mut [i32], &mut [i32]) = if levels & 1 == 1 { (work, out) } else { (out, work) };

  // Seed `cur` with the deepest LL.
  let ll_size = nn[levels];
  cur[..ll_size].copy_from_slice(&in_buf[..ll_size]);

  // Lift up, deepest level first.
  for k in (1..=levels).rev() {
    let n_l = nn[k];
    let n_h = nn[k - 1] - nn[k];
    let h = &in_buf[hl_offsets[k]..hl_offsets[k] + n_h];
    idwt53_inverse_one_level(&cur[..n_l], h, &mut dst[..n_l + n_h]);
    core::mem::swap(&mut cur, &mut dst);
  }
  // By parity, `cur` now aliases the caller's `out` buffer, holding the result.
}

/// Build the `levels + 1` horizontal `H`-offsets (in ints) for one lift LB.
///
/// `cum[i] = Σ_{j≤i} round_up(ng_lift[j], 8)`; `out[k] = 4 * cum[levels - k]`.
/// Index 0 is the end-of-LB sentinel (unused as a read offset).
fn build_hl_offsets(levels: usize, ng_lift: &[i32]) -> Vec<usize> {
  let mut cum = vec![0i32; levels + 1];
  for i in 0..=levels {
    cum[i] = if i == 0 { 0 } else { cum[i - 1] } + round_up(ng_lift[i], 8);
  }
  (0..=levels).map(|k| 4 * cum[levels - k] as usize).collect()
}

/// One horizontal pass (A or B) of a precinct.
///
/// Pass A (LBs 0-3): LB 0/1/3 are 5-level lifts, LB 2 is the memcpy LL (sb 12).
/// Pass B (LBs 4-7): LB 4/5/7 are 1-level lifts, LB 6 is the memcpy LL (sb 23).
/// Reconstructs into `out_stripe` in the LB-ordered layout used by the vertical
/// stage; `work` is scratch (≥ `n` ints). `buf` is `bufA` for pass A, `bufB` for
/// pass B.
#[cfg_attr(not(test), allow(dead_code))]
pub fn idwt_horizontal_pass(buf: &[i32], li: &LayoutInfo, config: &[SubbandConfig; NUM_SUBBANDS], is_pass_a: bool, out_stripe: &mut [i32], work: &mut [i32]) {
  let lift_lb = li.lift_lb;
  let memcpy_lb = li.memcpy_lb;
  let n = (li.ng_ll * 4) as usize; // W = ng_LL * 4 output samples per LB
  let lift_st = (lift_lb * 4) as usize;
  let memcpy_st = (memcpy_lb * 4) as usize;

  // LB-ordered in/out offsets (identical for both passes).
  let lb0 = 0;
  let lb1 = lift_st;
  let lb2 = 2 * lift_st;
  let lb3 = 2 * lift_st + memcpy_st;

  let (levels, hl_offsets, ll_sb) = if is_pass_a {
    (5usize, build_hl_offsets(5, &li.ng_lift), PASS_A_LL)
  } else {
    // Pass B: 1-level lift with 2 conceptual sub-bands, both ng_max.
    (1usize, build_hl_offsets(1, &[li.ng_max, li.ng_max]), PASS_B_LL)
  };

  // Lift LB 0/1.
  idwt53_horizontal_lift_all(&buf[lb0..], n, levels, &hl_offsets, &mut out_stripe[lb0..lb0 + n], work);
  idwt53_horizontal_lift_all(&buf[lb1..], n, levels, &hl_offsets, &mut out_stripe[lb1..lb1 + n], work);

  // Memcpy LL LB (sb 12 for pass A, sb 23 for pass B).
  let ll_start = (config[ll_sb].x24 * 4) as usize;
  let ll_len = (config[ll_sb].ng * 4) as usize;
  out_stripe[lb2..lb2 + ll_len].copy_from_slice(&buf[ll_start..ll_start + ll_len]);

  // Lift LB 3.
  idwt53_horizontal_lift_all(&buf[lb3..], n, levels, &hl_offsets, &mut out_stripe[lb3..lb3 + n], work);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::decompressors::ticoraw::picture_header::parse_picture_header;
  use crate::decompressors::ticoraw::precinct_decode::decode_precinct;
  use crate::decompressors::ticoraw::predecessor::PrecinctPredecessorState;
  use crate::decompressors::ticoraw::predict_lut::prediction_lut;
  use crate::decompressors::ticoraw::subband_config::compute_subband_layout;

  fn from_hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
  }

  fn expand_sparse(spec: &str, len: usize) -> Vec<i32> {
    let mut out = vec![0i32; len];
    for pair in spec.split(',').filter(|s| !s.is_empty()) {
      let (i, v) = pair.split_once(':').expect("idx:val");
      out[i.parse::<usize>().unwrap()] = v.parse::<i32>().unwrap();
    }
    out
  }

  // A single inverse level must invert a single forward 5/3 level exactly.
  #[test]
  fn one_level_round_trips_forward_5_3() {
    // Forward 5/3 (LeGall) analysis of a known signal, then inverse.
    let x: [i32; 8] = [10, 13, 9, 12, 40, 38, 7, 6];
    let n = x.len();
    // Forward: d[i] = x[2i+1] - ((x[2i] + x[2i+2]) >> 1) (PREDICT),
    //          s[i] = x[2i] + ((d[i-1] + d[i] + 2) >> 2)   (UPDATE).
    let n_l = n.div_ceil(2);
    let n_h = n / 2;
    let mut d = vec![0i32; n_h];
    for i in 0..n_h {
      let x2i = x[2 * i];
      let x2i2 = if 2 * i + 2 < n { x[2 * i + 2] } else { x[2 * i] };
      d[i] = x[2 * i + 1] - ((x2i + x2i2) >> 1);
    }
    let mut s = vec![0i32; n_l];
    for i in 0..n_l {
      let dm1 = if i > 0 { d[i - 1] } else { d[0] };
      let di = if i < n_h { d[i] } else { d[n_h - 1] };
      s[i] = x[2 * i] + ((dm1 + di + 2) >> 2);
    }
    let mut out = vec![0i32; n];
    idwt53_inverse_one_level(&s, &d, &mut out);
    assert_eq!(out, x, "inverse of forward 5/3 must reproduce the signal");
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder).
  //
  // Chains the real Rust `decode_precinct` on precinct 0 of DSC_8070.NEF to
  // build bufA/bufB, then runs both horizontal passes and asserts the LB-ordered
  // output stripe matches the reference's `h_out` exactly (pass A and pass B).
  #[test]
  fn oracle_precinct0_dsc8070_bit_exact() {
    let fixture = include_str!("testdata/idwt_horizontal_p0_8070.txt");
    let mut width = 0i32;
    let mut header = Vec::new();
    let mut raw = Vec::new();
    let mut houtlen = 0usize;
    let mut houta = "";
    let mut houtb = "";
    for line in fixture.lines() {
      if let Some(v) = line.strip_prefix("WIDTH=") {
        width = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("HEADER=") {
        header = from_hex(v.trim());
      } else if let Some(v) = line.strip_prefix("RAW=") {
        raw = from_hex(v.trim());
      } else if let Some(v) = line.strip_prefix("HOUTLEN=") {
        houtlen = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("HOUTA=") {
        houta = v.trim();
      } else if let Some(v) = line.strip_prefix("HOUTB=") {
        houtb = v.trim();
      }
    }

    // Build bufA/bufB via the validated entropy decode of precinct 0.
    let ph = parse_picture_header(&header).expect("picture header");
    let (li, config) = compute_subband_layout(width / 2);
    let mut pred_state = PrecinctPredecessorState::new(&config);
    let predict_lut = prediction_lut();
    let pass_a_stride = 3 * li.lift_lb + li.memcpy_lb;
    let kbuf = (4 * pass_a_stride) as usize;
    let mut buf_a = vec![0i32; kbuf];
    let mut buf_b = vec![0i32; kbuf];
    let res = decode_precinct(&raw, width, &ph, &config, &mut pred_state, predict_lut, &mut buf_a, &mut buf_b);
    assert!(res.success, "precinct decode must succeed");

    // Horizontal pass A and B into LB-ordered stripes.
    let mut h_out = vec![0i32; houtlen];
    let mut h_work = vec![0i32; houtlen];
    idwt_horizontal_pass(&buf_a, &li, &config, true, &mut h_out, &mut h_work);
    assert_eq!(h_out, expand_sparse(houta, houtlen), "pass A h_out mismatch");

    let mut h_out_b = vec![0i32; houtlen];
    let mut h_work_b = vec![0i32; houtlen];
    idwt_horizontal_pass(&buf_b, &li, &config, false, &mut h_out_b, &mut h_work_b);
    assert_eq!(h_out_b, expand_sparse(houtb, houtlen), "pass B h_out mismatch");
  }
}
