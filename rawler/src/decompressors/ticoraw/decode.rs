// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_decode.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* top-level image decoder — the 3-pass driver.
//!
//! Walks the precinct byte stream, then runs three passes over the tiles:
//!
//! 1. **Pass 1** — [`decode_tile`] for each tile → the image-wide
//!    `tile_coeff_buf` (entropy → horizontal → vertical IDWT).
//! 2. **Pass 2** — [`step1_merge_4_to_2`] over every tile → `step1_scratch`.
//! 3. **Pass 3** — [`step2_bayer_rows`] over every tile → the 16-bit RGGB Bayer
//!    image.
//!
//! Passes 2 and 3 read across tile boundaries (a tile's row `-1` / `w_rows`
//! reaches the previous / next tile's stripes), so all tiles are decoded before
//! any `step1`, and all `step1` before any `step2` — the buffers are image-wide.
//!
//! ## Precinct stream
//!
//! The 24-bit big-endian field at each precinct's start is its payload size
//! *after* the 12-byte prefix, so a precinct occupies `sz + 12` bytes. A 6-byte
//! alignment pad follows every 16th precinct (indices 15, 31, …). A tile
//! consumes 18 precincts at stride 16 — the last 2 of tile *T* are the first 2
//! of tile *T+1* (a 2-precinct overlap; prediction state does **not** carry, so
//! each tile gets a fresh [`PrecinctPredecessorState`]). A zero size marks the
//! end of the stream.

use super::bayer::{step1_merge_4_to_2, step2_bayer_rows};
use super::iqx_iqp_lut_data::iqx_iqp_lut;
use super::picture_header::PictureHeader;
use super::predecessor::PrecinctPredecessorState;
use super::predict_lut::prediction_lut;
use super::subband_config::compute_subband_layout;
use super::tile::{STRIPES_PER_TILE, compute_buf_stripe_ints, compute_kband, decode_tile};

/// File precincts consumed per tile (stride 16; 18 = 16 own + 2 overlap).
const PRECINCT_STRIDE: usize = 16;
const PRECINCTS_PER_TILE: usize = 18;
/// Image rows per tile (32 output stripes × 2 Bayer rows).
const ROWS_PER_TILE: usize = 64;

/// Result of a full image decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeDecodeResult {
  pub tiles_decoded: usize,
  pub total_precincts: usize,
}

/// Walk the precinct stream into `(offset, payload_size)` spans (the full
/// precinct occupies `payload_size + 12` bytes). Stops at a zero size, a short
/// buffer, or `max_precincts`.
fn walk_precincts(stream: &[u8], max_precincts: usize) -> Vec<(usize, usize)> {
  let mut spans = Vec::new();
  let mut cursor = 0usize;
  let mut remaining = stream.len();
  for p in 0..max_precincts {
    if remaining < 3 {
      break;
    }
    let sz = ((stream[cursor] as usize) << 16) | ((stream[cursor + 1] as usize) << 8) | (stream[cursor + 2] as usize);
    if sz == 0 {
      break; // end-of-stream sentinel
    }
    if sz + 12 > remaining {
      break;
    }
    spans.push((cursor, sz));
    cursor += sz + 12;
    remaining -= sz + 12;
    // 6-byte alignment pad after every 16th precinct (indices 15, 31, …).
    if (p & 0xF) == 15 && remaining >= 6 {
      cursor += 6;
      remaining -= 6;
    }
  }
  spans
}

/// Decode a full Nikon HE / HE\* image into `out_bayer` (`image_width ×
/// image_height` u16, row-major RGGB).
///
/// `stream` starts at the precinct byte stream (`strip[precinct_offset..]`);
/// `ph` supplies the WGT weights for GTLI. Faithful to the reference
/// `decode_nikon_he_image`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn decode_nikon_he_image(
  stream: &[u8],
  image_width: usize,
  image_height: usize,
  ph: &PictureHeader,
  out_bayer: &mut [u16],
) -> Result<HeDecodeResult, String> {
  if image_width == 0 || image_height == 0 {
    return Err("Nikon HE: zero image dimensions".to_string());
  }

  let (li, config) = compute_subband_layout((image_width / 2) as i32);
  let predict_lut = prediction_lut();
  let lut = iqx_iqp_lut();
  let stripe_ints = compute_buf_stripe_ints(&li);
  let kband = compute_kband(&li);

  let n_tiles = image_height.div_ceil(ROWS_PER_TILE);
  let max_precincts = n_tiles * PRECINCT_STRIDE + 2;

  let spans = walk_precincts(stream, max_precincts);
  let precinct_slices: Vec<&[u8]> = spans.iter().map(|&(s, sz)| &stream[s..s + sz + 12]).collect();
  let n_walked = precinct_slices.len();

  let tile_ints = STRIPES_PER_TILE * stripe_ints;
  let mut tile_coeff_buf = vec![0i32; n_tiles * tile_ints];
  let mut step1_scratch = vec![0i32; n_tiles * tile_ints];
  let mut overflow = vec![0i32; 2 * stripe_ints];

  let mut result = HeDecodeResult::default();

  // Pass 1: entropy + IDWT → tile_coeff_buf (all tiles before any step1).
  for t in 0..n_tiles {
    let base = t * PRECINCT_STRIDE;
    if base >= n_walked {
      break;
    }
    let count = (n_walked - base).min(PRECINCTS_PER_TILE);
    let is_first = t == 0;
    let is_last = t == n_tiles - 1;
    let mut pred_state = PrecinctPredecessorState::new(&config);
    let tile_res = decode_tile(
      &precinct_slices[base..base + count],
      image_width as i32,
      ph,
      &li,
      &config,
      &mut pred_state,
      predict_lut,
      &mut tile_coeff_buf,
      t,
      &mut overflow,
      is_first,
      is_last,
    );
    if !tile_res.success {
      return Err(format!("Nikon HE: tile {t} decode failed"));
    }
    result.tiles_decoded += 1;
    result.total_precincts += tile_res.precincts_decoded as usize;
  }

  let w_cols = image_width / 2;
  let last_partial = image_height % ROWS_PER_TILE;

  // Pass 2: step1 merge → step1_scratch (reads tile_coeff_buf across boundaries).
  for t in 0..n_tiles {
    let is_first = t == 0;
    let is_last = t == n_tiles - 1;
    let w_rows = if is_last && last_partial != 0 { last_partial / 2 } else { STRIPES_PER_TILE };
    let toff = t * tile_ints;
    step1_merge_4_to_2(
      &tile_coeff_buf,
      toff + kband,     // p1 = LL
      toff,             // p2 = LH
      toff + 3 * kband, // p3 = HH
      toff + 2 * kband, // p4 = HL
      w_rows,
      w_cols,
      stripe_ints,
      stripe_ints,
      &mut step1_scratch,
      toff,         // out_L
      toff + kband, // out_H
      stripe_ints,
      is_first,
      is_last,
    );
  }

  // Pass 3: step2 → out_bayer (reads tile_coeff_buf + step1_scratch).
  for t in 0..n_tiles {
    let is_first = t == 0;
    let is_last = t == n_tiles - 1;
    let w_rows = if is_last && last_partial != 0 { last_partial / 2 } else { STRIPES_PER_TILE };
    let toff = t * tile_ints;
    step2_bayer_rows(
      &tile_coeff_buf,
      toff + kband,     // p1 = LL
      toff + 2 * kband, // p4 = HL
      &step1_scratch,
      toff,         // p2 = L
      toff + kband, // p3 = H
      w_rows,
      w_cols,
      stripe_ints,
      stripe_ints,
      lut,
      out_bayer,
      image_width,
      (t * ROWS_PER_TILE) as i32,
      is_first,
      is_last,
    );
  }

  Ok(result)
}

#[cfg(test)]
mod tests {
  use super::*;

  // Hermetic unit test of the precinct-stream walk (the driver's unique parsing
  // logic): 24-bit size, sz+12 stride, 6-byte pad after every 16th precinct,
  // and the zero-size end sentinel.
  #[test]
  fn precinct_walk_stride_pad_and_sentinel() {
    // Build 18 precincts of payload size 4 (full = 16 bytes each), a 6-byte pad
    // after index 15, then a zero-size sentinel.
    let mut stream = Vec::new();
    let mut expected = Vec::new();
    let mut off = 0usize;
    for p in 0..18usize {
      expected.push((off, 4usize));
      // 24-bit size = 4, then 9 more prefix bytes + payload = 13 → total 16.
      stream.extend_from_slice(&[0x00, 0x00, 0x04]);
      stream.extend(std::iter::repeat_n(0u8, 13));
      off += 16;
      if (p & 0xF) == 15 {
        stream.extend_from_slice(&[0u8; 6]); // alignment pad
        off += 6;
      }
    }
    stream.extend_from_slice(&[0x00, 0x00, 0x00]); // end sentinel

    let spans = walk_precincts(&stream, 64);
    assert_eq!(spans, expected, "walk must reproduce spans incl. the pad after #15");
    // The pad shifts precinct 16's offset by 6 beyond the plain 16-byte stride.
    assert_eq!(spans[16].0, 16 * 16 + 6);
  }

  #[test]
  fn precinct_walk_stops_on_short_buffer() {
    // One valid precinct then a truncated size field.
    let mut stream = vec![0x00, 0x00, 0x04];
    stream.extend(std::iter::repeat_n(0u8, 13));
    stream.extend_from_slice(&[0x00, 0x10]); // sz claims 16 but buffer ends
    let spans = walk_precincts(&stream, 64);
    assert_eq!(spans, vec![(0, 4)]);
  }

  // OPT-IN END-TO-END ORACLE CHECK (bit-exact vs the reference decoder).
  //
  // Decodes a whole real HE/HE* strip in Rust and asserts every Bayer pixel
  // matches the reference C++ decoder's output. The strip is large (multi-MB) so
  // it is not embedded; point the test at local files and run with `--ignored`:
  //
  //   NIKON_HE_STRIP=…/strip_8070.bin \
  //   NIKON_HE_REF_PGM=…/bayer_8070.pgm \
  //   cargo test -p rawler --lib ticoraw::decode::tests::e2e -- --ignored
  //
  // The reference PGM is the oracle harness output (16-bit big-endian P5).
  #[test]
  #[ignore = "requires local strip + reference PGM via env vars"]
  fn e2e_full_image_matches_reference() {
    use crate::decompressors::ticoraw::picture_header::parse_picture_header;

    let strip_path = std::env::var("NIKON_HE_STRIP").expect("set NIKON_HE_STRIP");
    let pgm_path = std::env::var("NIKON_HE_REF_PGM").expect("set NIKON_HE_REF_PGM");
    let strip = std::fs::read(&strip_path).expect("read strip");
    let ph = parse_picture_header(&strip).expect("parse picture header");
    let (w, h) = (ph.hdr_width as usize, ph.hdr_height as usize);
    let stream = &strip[ph.precinct_offset..(ph.lcod as usize).min(strip.len())];

    let mut bayer = vec![0u16; w * h];
    let res = decode_nikon_he_image(stream, w, h, &ph, &mut bayer).expect("decode");
    eprintln!("decoded {} tiles, {} precincts ({w}x{h})", res.tiles_decoded, res.total_precincts);

    // Parse the reference PGM: "P5 <w> <h> 65535\n" then w*h big-endian u16.
    let pgm = std::fs::read(&pgm_path).expect("read ref pgm");
    let hdr_end = pgm.iter().position(|&b| b == b'\n').expect("pgm header newline") + 1;
    let header = std::str::from_utf8(&pgm[..hdr_end]).unwrap();
    let mut it = header.split_whitespace();
    assert_eq!(it.next(), Some("P5"));
    let pw: usize = it.next().unwrap().parse().unwrap();
    let phh: usize = it.next().unwrap().parse().unwrap();
    assert_eq!((pw, phh), (w, h), "ref PGM dimensions");
    let px = &pgm[hdr_end..];
    assert_eq!(px.len(), w * h * 2, "ref PGM pixel byte count");

    let mut mismatches = 0usize;
    let mut first: Option<(usize, u16, u16)> = None;
    for i in 0..w * h {
      let refv = ((px[2 * i] as u16) << 8) | px[2 * i + 1] as u16;
      if bayer[i] != refv {
        if first.is_none() {
          first = Some((i, bayer[i], refv));
        }
        mismatches += 1;
      }
    }
    assert_eq!(mismatches, 0, "bit-exact mismatch: {mismatches} px, first {first:?}");
  }
}
