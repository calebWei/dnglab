// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_tile.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* per-tile orchestrator.
//!
//! One tile spans 64 image rows (32 output stripes). [`decode_tile`] drives the
//! full per-precinct pipeline over the tile's precincts and assembles the
//! LB-ordered coefficient stripes:
//!
//! 1. entropy decode → `bufA` / `bufB` ([`decode_precinct`]);
//! 2. horizontal IDWT of each pass → `h_out` ([`idwt_horizontal_pass`]);
//! 3. copy the memcpy LL line-block of each pass into the stripe buffer;
//! 4. vertical lift of each pass ([`ver_lift_lb_step`]), a stateful loop whose
//!    per-LB carry persists across precincts.
//!
//! After the precinct loop a 2-step partial-tile tail flushes the vertical
//! state, the first 32 stripes are copied to this tile's region of the
//! image-wide `tile_coeff_buf`, and the next 2 stripes are saved as
//! `overflow_carry` for the following tile (a 2-stripe cross-tile overlap;
//! prediction state does **not** carry — the caller uses a fresh `pred_state`
//! per tile).
//!
//! Tile 0 enters the vertical state machine on **path A** (state 2); later tiles
//! on **path B** (state 0), seeding their first 2 stripes from `overflow_carry`
//! and starting `x1_write_offset` at 8. The pass-A vertical lift is skipped for
//! precinct 0 of a non-first tile (its rows came from the overlap).
//!
//! Porting note: the reference reads `config[0].layout_info`; this port takes
//! [`LayoutInfo`] explicitly, and each [`VerLiftStatePerLb`] owns its carry
//! buffers instead of pointing into one tile-wide array.

use super::idwt_horizontal::idwt_horizontal_pass;
use super::idwt_vertical::{ver_lift_lb_step, ver_lift_state, VerLiftStatePerLb};
use super::picture_header::PictureHeader;
use super::precinct_decode::decode_precinct;
use super::predecessor::PrecinctPredecessorState;
use super::predict_lut::PREDICTION_LUT_SIZE;
use super::subband_config::{LayoutInfo, SubbandConfig};

const NUM_SUBBANDS: usize = 26;
/// Output stripes per tile.
pub const STRIPES_PER_TILE: usize = 32;
/// Precincts consumed per tile (16 own + 2 overlap into the next tile).
#[cfg_attr(not(test), allow(dead_code))]
pub const PRECINCTS_PER_TILE: usize = 18;
/// Working stripes held in `tile_buf` (32 + tail + headroom).
const MAX_STRIPES: usize = 40;
/// Vertical-lift LB components (LB 0/1/3 lift, LB 2 memcpy).
const VER_COMPONENTS: usize = 4;

/// Stripe stride in ints: `4 * (3 * lift_LB + memcpy_LB)`.
#[cfg_attr(not(test), allow(dead_code))]
#[inline]
pub fn compute_buf_stripe_ints(li: &LayoutInfo) -> usize {
  let pass_a_stride = 3 * li.lift_lb + li.memcpy_lb;
  (4 * pass_a_stride) as usize
}

/// Per-band stride in ints: `lift_LB * 4`.
#[cfg_attr(not(test), allow(dead_code))]
#[inline]
pub fn compute_kband(li: &LayoutInfo) -> usize {
  (li.lift_lb * 4) as usize
}

/// Outcome of decoding one tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileDecodeResult {
  pub precincts_decoded: i32,
  pub success: bool,
}

/// Run one vertical-lift loop over the 4 LB components, writing reconstructed
/// rows into `tile_buf` starting at `x1_base_off`.
///
/// `x0_h` is the horizontal-lift output (`h_out`) for this pass, or `None` for a
/// partial-tile tail flush. LB strides are `[lift_st, lift_st, 0, lift_st]`: the
/// memcpy LB (k = 2) has `n = 0` — it ticks state without writing, and does not
/// advance the write cursor, so LB 3 lands in the slot after LB 1 (LB 2's data
/// arrives via the separate `memcpy_cursor` copy).
fn run_one_ver_lift_loop(
  tile_buf: &mut [i32],
  x1_base_off: usize,
  x0_h: Option<&[i32]>,
  lift_st: usize,
  memcpy_st: usize,
  ver_st: &mut [VerLiftStatePerLb; VER_COMPONENTS],
) {
  let lb_stride = [lift_st, lift_st, 0usize, lift_st];
  let x0_off = [Some(0usize), Some(lift_st), None, Some(2 * lift_st + memcpy_st)];
  let mut x1 = x1_base_off;
  for k in 0..VER_COMPONENTS {
    let n = lb_stride[k];
    let x0k: Option<&[i32]> = match (x0_h, x0_off[k]) {
      (Some(h), Some(off)) => Some(&h[off..off + n]),
      _ => None,
    };
    ver_lift_lb_step(x0k, &mut tile_buf[x1..x1 + n], n, &mut ver_st[k]);
    x1 += n;
  }
}

/// Decode one tile into its region of the image-wide `tile_coeff_buf`.
///
/// `precinct_data` holds one raw byte slice per precinct (its length is the
/// precinct count). `overflow_carry` (`2 * stripe_ints` ints) carries the
/// 2-stripe overlap in and out. Faithful to the reference `decode_tile`.
#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
pub fn decode_tile(
  precinct_data: &[&[u8]],
  image_width: i32,
  ph: &PictureHeader,
  li: &LayoutInfo,
  config: &[SubbandConfig; NUM_SUBBANDS],
  pred_state: &mut PrecinctPredecessorState,
  predict_lut: &[u8; PREDICTION_LUT_SIZE],
  tile_coeff_buf: &mut [i32],
  tile_index: usize,
  overflow_carry: &mut [i32],
  is_first_tile: bool,
  _is_last_tile: bool,
) -> TileDecodeResult {
  let mut result = TileDecodeResult::default();

  let lift_lb = li.lift_lb as usize;
  let memcpy_lb = li.memcpy_lb as usize;
  let pass_a_stride = 3 * lift_lb + memcpy_lb;
  let stripe_ints = 4 * pass_a_stride;
  let lift_st = lift_lb * 4;
  let memcpy_st = memcpy_lb * 4;
  let kband = lift_st;

  // Per-LB vertical-lift state (each owns lift_st-sized carry buffers).
  let init_state = if is_first_tile { ver_lift_state::INIT2 } else { ver_lift_state::INIT0 };
  let mut ver_st: [VerLiftStatePerLb; VER_COMPONENTS] = [
    VerLiftStatePerLb::new(lift_st, init_state),
    VerLiftStatePerLb::new(lift_st, init_state),
    VerLiftStatePerLb::new(lift_st, init_state),
    VerLiftStatePerLb::new(lift_st, init_state),
  ];

  // Per-precinct working buffers.
  let kbuf = 4 * pass_a_stride;
  let mut buf_a = vec![0i32; kbuf];
  let mut buf_b = vec![0i32; kbuf];
  let mut h_out = vec![0i32; 4 * lift_st];
  let mut h_work = vec![0i32; 4 * lift_st + 16];

  // Tile-wide working stripes.
  let mut tile_buf = vec![0i32; MAX_STRIPES * stripe_ints];

  // For tiles > 0, seed the first 2 stripes from the previous tile's overflow.
  if !is_first_tile {
    tile_buf[..2 * stripe_ints].copy_from_slice(&overflow_carry[..2 * stripe_ints]);
  }

  let mut x1_write_offset = if is_first_tile { 0 } else { 8 };
  let mut memcpy_cursor = 0usize;

  for (p, prec) in precinct_data.iter().enumerate() {
    // 1. Entropy → bufA, bufB.
    buf_a.iter_mut().for_each(|v| *v = 0);
    buf_b.iter_mut().for_each(|v| *v = 0);
    let prec_res = decode_precinct(prec, image_width, ph, config, pred_state, predict_lut, &mut buf_a, &mut buf_b);
    if !prec_res.success {
      break;
    }
    result.precincts_decoded += 1;

    // 2. Pass A horizontal IDWT → h_out.
    h_out.iter_mut().for_each(|v| *v = 0);
    h_work.iter_mut().for_each(|v| *v = 0);
    idwt_horizontal_pass(&buf_a, li, config, true, &mut h_out, &mut h_work);

    // 3. Copy pass-A memcpy LL LB into the stripe buffer.
    tile_buf[memcpy_cursor + 3 * kband..memcpy_cursor + 3 * kband + memcpy_st].copy_from_slice(&h_out[2 * lift_st..2 * lift_st + memcpy_st]);
    memcpy_cursor += stripe_ints;

    // 4. ver_lift pass A (skipped for precinct 0 of a non-first tile).
    let skip_pass_a_verlift = !is_first_tile && p == 0;
    if !skip_pass_a_verlift {
      let pre_tick = ver_st[0].state;
      let x1_base = x1_write_offset * pass_a_stride;
      run_one_ver_lift_loop(&mut tile_buf, x1_base, Some(&h_out), lift_st, memcpy_st, &mut ver_st);
      if pre_tick > 5 {
        x1_write_offset += 4;
      }
    }

    // 5. Pass B horizontal IDWT → h_out.
    h_out.iter_mut().for_each(|v| *v = 0);
    h_work.iter_mut().for_each(|v| *v = 0);
    idwt_horizontal_pass(&buf_b, li, config, false, &mut h_out, &mut h_work);

    // 6. Copy pass-B memcpy LL LB into the stripe buffer.
    tile_buf[memcpy_cursor + 3 * kband..memcpy_cursor + 3 * kband + memcpy_st].copy_from_slice(&h_out[2 * lift_st..2 * lift_st + memcpy_st]);
    memcpy_cursor += stripe_ints;

    // 7. ver_lift pass B.
    {
      let pre_tick = ver_st[0].state;
      let x1_base = x1_write_offset * pass_a_stride;
      run_one_ver_lift_loop(&mut tile_buf, x1_base, Some(&h_out), lift_st, memcpy_st, &mut ver_st);
      if pre_tick > 5 {
        x1_write_offset += 4;
      }
    }
  }

  // Partial-tile tail: 2 extra ver_lift loops with x0 = None (flush 7/8 → 9 → 11).
  for _ in 0..2 {
    let pre_tick = ver_st[0].state;
    let x1_base = x1_write_offset * pass_a_stride;
    run_one_ver_lift_loop(&mut tile_buf, x1_base, None, lift_st, memcpy_st, &mut ver_st);
    if pre_tick > 5 {
      x1_write_offset += 4;
    }
  }

  // Copy the first 32 stripes to this tile's region of tile_coeff_buf.
  let tile_out_off = tile_index * STRIPES_PER_TILE * stripe_ints;
  let n_copy = STRIPES_PER_TILE * stripe_ints;
  tile_coeff_buf[tile_out_off..tile_out_off + n_copy].copy_from_slice(&tile_buf[..n_copy]);

  // Save the next 2 stripes as the overflow for the next tile.
  overflow_carry[..2 * stripe_ints].copy_from_slice(&tile_buf[n_copy..n_copy + 2 * stripe_ints]);

  result.success = true;
  result
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::decompressors::ticoraw::picture_header::parse_picture_header;
  use crate::decompressors::ticoraw::predict_lut::prediction_lut;
  use crate::decompressors::ticoraw::subband_config::compute_subband_layout;

  fn from_hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
  }

  fn le_i32s(bytes: &[u8]) -> Vec<i32> {
    assert!(bytes.len() % 4 == 0, "binary length must be a multiple of 4");
    bytes.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder).
  //
  // Decodes tile 0 of DSC_8070.NEF — the full stripe orchestration over 18
  // precincts (entropy → horizontal → vertical, memcpy cursor, x1 write-offset,
  // path-A init, 2-step tail) — and asserts the entire tile-0 coefficient buffer
  // (372 736 ints) and the cross-tile overflow (23 296 ints) match the
  // reference's golden output exactly. Inputs and golden outputs are stored as
  // binary fixtures; a small text sidecar carries the layout and precinct spans.
  #[test]
  fn oracle_tile0_dsc8070_bit_exact() {
    let meta = include_str!("testdata/tile0_8070.meta.txt");
    let precinct_blob = include_bytes!("testdata/tile0_8070.precincts.bin");
    let coeff_golden = le_i32s(include_bytes!("testdata/tile0_8070.coeff.bin"));
    let overflow_golden = le_i32s(include_bytes!("testdata/tile0_8070.overflow.bin"));

    let mut width = 0i32;
    let mut header = Vec::new();
    let mut stripe_ints = 0usize;
    let mut coefflen = 0usize;
    let mut spans: Vec<(usize, usize)> = Vec::new(); // (offset, len) within precinct_blob
    for line in meta.lines() {
      if let Some(v) = line.strip_prefix("WIDTH=") {
        width = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("HEADER=") {
        header = from_hex(v.trim());
      } else if let Some(v) = line.strip_prefix("STRIPE_INTS=") {
        stripe_ints = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("COEFFLEN=") {
        coefflen = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("SPANS=") {
        for pair in v.trim().split(',').filter(|s| !s.is_empty()) {
          let (o, l) = pair.split_once(':').unwrap();
          spans.push((o.parse().unwrap(), l.parse().unwrap()));
        }
      }
    }
    assert_eq!(spans.len(), 18, "tile 0 has 18 precincts");

    let precincts: Vec<&[u8]> = spans.iter().map(|&(o, l)| &precinct_blob[o..o + l]).collect();

    let ph = parse_picture_header(&header).expect("picture header");
    let (li, config) = compute_subband_layout(width / 2);
    let predict_lut = prediction_lut();
    let mut pred_state = PrecinctPredecessorState::new(&config);

    // tile_coeff_buf need only hold tile 0 here (tile_index 0).
    let mut tile_coeff_buf = vec![0i32; coefflen];
    let mut overflow = vec![0i32; 2 * stripe_ints];

    let res = decode_tile(
      &precincts,
      width,
      &ph,
      &li,
      &config,
      &mut pred_state,
      predict_lut,
      &mut tile_coeff_buf,
      0,
      &mut overflow,
      true,
      false,
    );

    assert!(res.success, "tile decode must succeed");
    assert_eq!(res.precincts_decoded, 18, "all 18 precincts decoded");
    assert_eq!(tile_coeff_buf, coeff_golden, "tile-0 coefficient buffer mismatch");
    assert_eq!(overflow, overflow_golden, "overflow_carry mismatch");
  }
}
