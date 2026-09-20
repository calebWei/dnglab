// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_precinct_decode.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* per-precinct entropy decode — the integration stage.
//!
//! Wires the parsed [precinct header](super::precinct_header) and the entropy
//! chain together to decode all 26 sub-bands of one precinct and scatter their
//! dequantized coefficients into the two pass buffers (`bufA` / `bufB`):
//!
//! ```text
//! parse header → (per sub-band) gtli → get_previous_gcli
//!              → decode_gcli_values → unpack_coefficient_magnitudes
//!              → apply_sign_bits → dequantize → save_gcli/set_fully_insig
//!              → scatter into bufA|bufB at config[sb].x24 * 4
//! ```
//!
//! ## Line-block walk & reader lifetime
//!
//! Sub-bands are decoded in index order, grouped by their line-block (LB):
//! `LB0 = sb0..5`, `LB1 = sb6..11`, `LB2 = sb12`, `LB3 = sb13..18`,
//! `LB4 = sb19..20`, `LB5 = sb21..22`, `LB6 = sb23`, `LB7 = sb24..25`. This
//! natural order also satisfies the cross-band dependency (sb 12 in LB2 is
//! decoded before sb 23 in LB6, within one precinct).
//!
//! **Critical:** one [`BitReader`] per substream *per LB*, persisted across all
//! of that LB's sub-bands. Bit state can be mid-byte at a sub-band boundary;
//! rebuilding readers from byte cursors would drop the partial-byte state and
//! desync the decode.

use super::bit_reader::BitReader;
use super::coefficient_decode::{apply_sign_bits, unpack_coefficient_magnitudes};
use super::dequantize::dequantize_coefficient_array;
use super::gcli_decode::{decode_gcli_values, MODE_ZERO_PRED};
use super::gtli_table::gtli_for_sub_band;
use super::picture_header::PictureHeader;
use super::precinct_header::{parse_precinct_header, LINE_BLOCKS_PER_PRECINCT};
use super::predecessor::{should_reset_gcli, PrecinctPredecessorState};
use super::predict_lut::PREDICTION_LUT_SIZE;
use super::subband_config::SubbandConfig;

/// Number of sub-bands in a precinct.
pub const NUM_SUBBANDS: usize = 26;
/// Number of `Dpb` entries in the header.
const DPB_COUNT: usize = 28;

/// Outcome of decoding one precinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrecinctDecodeResult {
  /// Sub-bands successfully decoded (26 on success).
  pub total_sub_bands_decoded: i32,
  /// `true` iff all 26 sub-bands were decoded.
  pub success: bool,
}

/// Map a sub-band index (0..26) to its `Dpb` index (16 primary + 12 secondary).
#[inline]
fn sb_to_dpb_index(sb: usize) -> Option<usize> {
  if sb < 26 {
    Some(sb)
  } else {
    None
  }
}

/// Inclusive-start / exclusive-end sub-band range for each line-block.
#[inline]
fn lb_sub_band_range(lb: usize) -> (usize, usize) {
  match lb {
    0 => (0, 6),
    1 => (6, 12),
    2 => (12, 13),
    3 => (13, 19),
    4 => (19, 21),
    5 => (21, 23),
    6 => (23, 24),
    7 => (24, 26),
    _ => (0, 0),
  }
}

/// Decode one precinct: header + all 26 sub-bands, scattered into `buf_a`/`buf_b`.
///
/// `precinct_data` starts at the precinct header; `image_width` is the full image
/// width (drives the DX significance sizes and `f20`). `ph` supplies the WGT
/// weights for the GTLI lookup. `buf_a` / `buf_b` must each be large enough to
/// hold the scatter (`config[sb].x24 * 4 + config[sb].ng * 4` int32s for every
/// sub-band; see `compute_buf_stripe_ints` in the tile stage). They are written,
/// not cleared, so the caller zeros them per precinct.
///
/// Faithful to the reference `decode_precinct`. On a header-parse failure returns
/// `{0, false}` without touching the buffers or `pred_state`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn decode_precinct(
  precinct_data: &[u8],
  image_width: i32,
  ph: &PictureHeader,
  config: &[SubbandConfig; NUM_SUBBANDS],
  pred_state: &mut PrecinctPredecessorState,
  predict_lut: &[u8; PREDICTION_LUT_SIZE],
  buf_a: &mut [i32],
  buf_b: &mut [i32],
) -> PrecinctDecodeResult {
  let mut result = PrecinctDecodeResult::default();

  // 1. Parse the precinct header (carries the DX per-LB significance sizes).
  let sizes = match parse_precinct_header(precinct_data, image_width) {
    Some(s) => s,
    None => return result,
  };
  let bp = sizes.bp as i32;
  let br = sizes.br as i32;

  // Precinct-16 GCLI reset (Bp/Br are quantizer values, so this is purely a
  // precinct-index condition).
  if should_reset_gcli(pred_state.precinct_index(), sizes.bp, sizes.br) {
    pred_state.reset_gcli_state();
  }

  // 2. Walk the 8 line-blocks; one BitReader-per-substream per LB.
  for lb in 0..LINE_BLOCKS_PER_PRECINCT {
    let sig_off = sizes.lb_sig_offset[lb] as usize;
    let sig_len = sizes.lb_sig_bytes[lb] as usize;
    let gcli_off = sig_off + sig_len;
    let gcli_len = sizes.lb_gcli_bytes[lb] as usize;
    let data_off = gcli_off + gcli_len;
    let data_len = sizes.lb_data_bytes[lb] as usize;
    let sign_off = data_off + data_len;
    let sign_len = sizes.lb_sign_bytes[lb] as usize;

    // parse_precinct_header validated that every substream lies within
    // `precinct_data`; guard anyway so a malformed precinct errs rather than
    // panics.
    if sign_off + sign_len > precinct_data.len() {
      return result;
    }

    let mut sig_reader = BitReader::new(&precinct_data[sig_off..gcli_off]);
    let mut gcli_reader = BitReader::new(&precinct_data[gcli_off..data_off]);
    let mut data_reader = BitReader::new(&precinct_data[data_off..sign_off]);
    let mut sign_reader = BitReader::new(&precinct_data[sign_off..sign_off + sign_len]);

    let (sb_start, sb_end) = lb_sub_band_range(lb);
    for sb in sb_start..sb_end {
      let ng = config[sb].ng.max(0) as usize;

      // Prediction mode from the header's Dpb field (`| 0x70`).
      let dpb_mode = match sb_to_dpb_index(sb) {
        Some(idx) if idx < DPB_COUNT => (sizes.dpb[idx] as i32) | 0x70,
        _ => MODE_ZERO_PRED,
      };

      // GTLI for this sub-band (live picture-header path: never the 0xFF
      // sentinel, which only arises on the dead static-table path).
      let gtli = gtli_for_sub_band(ph, bp, br, sb) as i32;

      // 3. GCLIs.
      let mut gcli_out = vec![0u8; ng];
      {
        let prev_gcli = pred_state.get_previous_gcli(sb);
        decode_gcli_values(
          &mut sig_reader,
          &mut gcli_reader,
          dpb_mode,
          ng,
          gtli,
          predict_lut,
          Some(prev_gcli),
          &mut gcli_out,
        );
      }

      // 4. Coefficient magnitudes.
      let coeff_count = ng * 4;
      let mut coeffs = vec![0i32; coeff_count];
      unpack_coefficient_magnitudes(&mut data_reader, &gcli_out, gtli, ng, &mut coeffs);

      // 5. Sign bits.
      apply_sign_bits(&mut sign_reader, &mut coeffs);

      // 6. Dequantize.
      let is_ll = sb == 12 || sb == 23;
      dequantize_coefficient_array(&mut coeffs, coeff_count, &gcli_out, ng, gtli, is_ll);

      // 7. Persist GCLIs (prediction context) and the fully-insignificant flag.
      pred_state.save_gcli(sb, &gcli_out);
      let all_zero = coeffs.iter().all(|&c| c == 0);
      pred_state.set_fully_insig(sb, all_zero);

      // 8. Scatter into bufA / bufB at x24 * 4.
      let target = if config[sb].buffer_idx == 0 { &mut *buf_a } else { &mut *buf_b };
      let start = config[sb].x24.max(0) as usize * 4;
      if start + coeff_count <= target.len() {
        target[start..start + coeff_count].copy_from_slice(&coeffs);
      }

      result.total_sub_bands_decoded += 1;
    }
  }

  // 9. Advance to the next precinct.
  pred_state.advance_precinct();

  result.success = result.total_sub_bands_decoded == NUM_SUBBANDS as i32;
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

  /// Expand a sparse `idx:val,idx:val,...` dump into a dense zero-filled buffer.
  fn expand_sparse(spec: &str, len: usize) -> Vec<i32> {
    let mut out = vec![0i32; len];
    for pair in spec.split(',').filter(|s| !s.is_empty()) {
      let (i, v) = pair.split_once(':').expect("idx:val");
      out[i.parse::<usize>().unwrap()] = v.parse::<i32>().unwrap();
    }
    out
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder + DX fix).
  //
  // The full integration milestone: decode precinct 0 of real DSC_8070.NEF —
  // header parse, all 26 sub-bands (gtli → gcli → coeffs → signs → dequant →
  // cross-band prediction), and the scatter into bufA/bufB — then assert both
  // buffers match the reference's post-decode buffers exactly. This exercises
  // every entropy module wired together on real data, including the sb-12→sb-23
  // LL cross-band prediction and the x24/buffer_idx scatter geometry.
  #[test]
  fn oracle_precinct0_dsc8070_bit_exact() {
    let fixture = include_str!("testdata/precinct_decode_p0_8070.txt");
    let mut width = 0i32;
    let mut header = Vec::new();
    let mut raw = Vec::new();
    let mut buflen = 0usize;
    let mut bufa_spec = "";
    let mut bufb_spec = "";
    for line in fixture.lines() {
      if let Some(v) = line.strip_prefix("WIDTH=") {
        width = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("HEADER=") {
        header = from_hex(v.trim());
      } else if let Some(v) = line.strip_prefix("RAW=") {
        raw = from_hex(v.trim());
      } else if let Some(v) = line.strip_prefix("BUFLEN=") {
        buflen = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("BUFA=") {
        bufa_spec = v.trim();
      } else if let Some(v) = line.strip_prefix("BUFB=") {
        bufb_spec = v.trim();
      }
    }

    // Reconstruct the real picture header (supplies the WGT weights for GTLI).
    let ph = parse_picture_header(&header).expect("parse picture header");
    assert!(ph.valid, "picture header must be valid");
    assert_eq!(ph.nbands, 25, "Nikon HE profile has 25 WGT bands");

    let (_li, config) = compute_subband_layout(width / 2);
    let mut pred_state = PrecinctPredecessorState::new(&config);
    let predict_lut = prediction_lut();

    let mut buf_a = vec![0i32; buflen];
    let mut buf_b = vec![0i32; buflen];
    let result = decode_precinct(&raw, width, &ph, &config, &mut pred_state, predict_lut, &mut buf_a, &mut buf_b);

    assert!(result.success, "all 26 sub-bands must decode");
    assert_eq!(result.total_sub_bands_decoded, 26);

    let expected_a = expand_sparse(bufa_spec, buflen);
    let expected_b = expand_sparse(bufb_spec, buflen);
    assert_eq!(buf_a, expected_a, "bufA mismatch vs reference");
    assert_eq!(buf_b, expected_b, "bufB mismatch vs reference");
  }
}
