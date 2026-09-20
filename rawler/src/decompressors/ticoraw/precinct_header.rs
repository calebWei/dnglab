// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_precinct_header.{h,cpp}, INCLUDING the DX significance fix
//   (dx_sig_fix.patch) developed for this project — see NIKON_HE_PROJECT.md §12.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* per-precinct header parser.
//!
//! Each precinct opens with a compact big-endian header describing the byte
//! layout of the 8 line-blocks (LBs) that follow. Layout:
//!
//! * bytes 0..2 — `total_size` (24-bit)
//! * byte 3 — `Bp`, byte 4 — `Br`
//! * bytes 5..11 — 28 × 2-bit `Dpb` fields
//! * then, for each of 8 LBs, a **7-byte mini-header interleaved with its
//!   substreams**: `[f20_sign:1][data_bytes:20][gcli_bytes:20][sign_bytes:15]`,
//!   immediately followed by that LB's sig, gcli, data and sign substreams. The
//!   next mini-header begins right after the previous LB's sign substream.
//!
//! ## DX significance fix
//!
//! The stock reference set every LB's significance-substream size to the global
//! `f20`, which is only correct for FX bodies. On DX (e.g. Nikon Z50 II, this
//! project's samples) the per-LB significance size is
//! `sig[lb] = ceil( Σ_subband ceil(ng_subband / 8) / 8 )`, giving
//! `[12,12,11,12,11,11,11,11]` at width 5600. See [`compute_lb_sig_bytes`].

/// Line-blocks per precinct pass.
pub const LINE_BLOCKS_PER_PRECINCT: usize = 8;
/// Number of `Dpb` (débit-par-bloc) entries.
pub const DPB_COUNT: usize = 28;

/// Minimum header prefix: 3 (total_size) + 1 (Bp) + 1 (Br) + 7 (Dpb) + at least
/// one 7-byte LB mini-header = 19. (LB mini-headers interleave with substreams,
/// so the full header length is not bounded up front.)
const MIN_HEADER_PREFIX: usize = 19;

/// Parsed precinct header — byte layout of the 8 line-blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecinctSizes {
  pub total_size: u32,
  pub bp: u8,
  pub br: u8,
  pub dpb: [u8; DPB_COUNT],
  /// `f20 = ceil(half_pass_width / 256)` (informational; the DX per-LB `sig`
  /// sizes replace it as the actual significance-substream lengths).
  pub f20: i32,
  pub lb_sig_bytes: [u32; LINE_BLOCKS_PER_PRECINCT],
  pub lb_data_bytes: [u32; LINE_BLOCKS_PER_PRECINCT],
  pub lb_gcli_bytes: [u32; LINE_BLOCKS_PER_PRECINCT],
  pub lb_sign_bytes: [u32; LINE_BLOCKS_PER_PRECINCT],
  pub lb_f20_sign: [u8; LINE_BLOCKS_PER_PRECINCT],
  /// Offset from the start of the precinct to each LB's sig substream (just past
  /// its 7-byte mini-header).
  pub lb_sig_offset: [u32; LINE_BLOCKS_PER_PRECINCT],
}

impl Default for PrecinctSizes {
  fn default() -> Self {
    Self {
      total_size: 0,
      bp: 0,
      br: 0,
      dpb: [0; DPB_COUNT],
      f20: 0,
      lb_sig_bytes: [0; LINE_BLOCKS_PER_PRECINCT],
      lb_data_bytes: [0; LINE_BLOCKS_PER_PRECINCT],
      lb_gcli_bytes: [0; LINE_BLOCKS_PER_PRECINCT],
      lb_sign_bytes: [0; LINE_BLOCKS_PER_PRECINCT],
      lb_f20_sign: [0; LINE_BLOCKS_PER_PRECINCT],
      lb_sig_offset: [0; LINE_BLOCKS_PER_PRECINCT],
    }
  }
}

#[inline]
fn ceil_div(a: i32, b: i32) -> i32 {
  (a + b - 1) / b
}

/// `f20 = ceil(half_pass_width / 256)` (FX W=4140 → 17; DX W=2704 → 11).
#[inline]
pub fn compute_f20(half_pass_width: i32) -> i32 {
  (half_pass_width + 255) / 256
}

/// Per-LB significance-substream byte sizes (the DX fix).
///
/// Significance is coded one bit per significance-group (`Ss = 8` coefficient
/// groups), packed 8 bits/byte:
/// `sig[lb] = ceil( Σ_subband ceil(ng_subband / 8) / 8 )`.
///
/// The 8 LBs are: LB0/1/3 = 5-level lift (6 sub-bands), LB2/6 = LL, LB4/5/7 =
/// 1-level lift (2 sub-bands). Verified against the Z50 II (DX) HE sample: at
/// width 5600 this yields `[12,12,11,12,11,11,11,11]`, aligning all 8 LB headers.
pub fn compute_lb_sig_bytes(image_width: i32) -> [u32; LINE_BLOCKS_PER_PRECINCT] {
  let half = image_width / 2;
  let ng_max = (half + 7) / 8;
  let ng_ll = half / 4;
  let n4 = (ng_max + 1) / 2;
  let n5 = (n4 + 1) / 2;
  let n_h5 = n4 - n5;
  let ng5 = [(n5 + 3) / 4, (n_h5 + 3) / 4, (ng_max + 7) / 8, (ng_max + 3) / 4, (ng_max + 1) / 2, ng_max];
  let groups_5level: i32 = ng5.iter().map(|&n| ceil_div(n, 8)).sum();
  let groups_ll = ceil_div(ng_ll, 8);
  let groups_1level = 2 * ceil_div(ng_max, 8);
  let groups_per_lb = [
    groups_5level,
    groups_5level,
    groups_ll,
    groups_5level,
    groups_1level,
    groups_1level,
    groups_ll,
    groups_1level,
  ];
  let mut out = [0u32; LINE_BLOCKS_PER_PRECINCT];
  for (o, &g) in out.iter_mut().zip(groups_per_lb.iter()) {
    *o = ceil_div(g, 8) as u32;
  }
  out
}

#[inline]
fn read_be(p: &[u8], bytes: usize) -> u32 {
  let mut v = 0u32;
  for &b in &p[..bytes] {
    v = (v << 8) | b as u32;
  }
  v
}

/// Parse a precinct header from raw bytes (MSB-first, big-endian).
///
/// `data` starts at the precinct; `image_width` is the full image width (used for
/// `f20` and the DX per-LB `sig` sizes). Returns `None` if the buffer is too short
/// for the prefix or for any LB mini-header + its declared substreams.
///
/// Faithful to the reference `parse_precinct_header` with the DX `sig` fix.
// Consumed by `precinct_decode` (not yet ported).
#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_precinct_header(data: &[u8], image_width: i32) -> Option<PrecinctSizes> {
  if data.len() < MIN_HEADER_PREFIX {
    return None;
  }

  let mut out = PrecinctSizes {
    f20: compute_f20(image_width / 2),
    ..Default::default()
  };
  let lb_sig = compute_lb_sig_bytes(image_width);

  out.total_size = read_be(data, 3);
  out.bp = data[3];
  out.br = data[4];

  // Bytes 5..11: 28 × 2-bit Dpb fields (MSB-first within each byte).
  for i in 0..7 {
    let b = data[5 + i];
    for j in 0..4 {
      let idx = i * 4 + j;
      if idx >= DPB_COUNT {
        break;
      }
      out.dpb[idx] = (b >> (6 - 2 * j)) & 0x03;
    }
  }

  // 8 LB mini-headers + substreams, interleaved.
  let mut cursor = 12usize; // after 3+1+1+7 prefix
  for lb in 0..LINE_BLOCKS_PER_PRECINCT {
    if cursor + 7 > data.len() {
      return None;
    }

    // 7-byte mini-header = 56 bits:
    //   [f20_sign:1][data_bytes:20][gcli_bytes:20][sign_bytes:15]
    let mut val = 0u64;
    for &b in &data[cursor..cursor + 7] {
      val = (val << 8) | b as u64;
    }
    out.lb_f20_sign[lb] = ((val >> 55) & 1) as u8;
    out.lb_data_bytes[lb] = ((val >> 35) & 0xFFFFF) as u32;
    out.lb_gcli_bytes[lb] = ((val >> 15) & 0xFFFFF) as u32;
    out.lb_sign_bytes[lb] = (val & 0x7FFF) as u32;
    out.lb_sig_bytes[lb] = lb_sig[lb]; // DX FIX (reference used the global f20)

    cursor += 7;
    out.lb_sig_offset[lb] = cursor as u32;

    let payload = out.lb_sig_bytes[lb] as usize + out.lb_gcli_bytes[lb] as usize + out.lb_data_bytes[lb] as usize + out.lb_sign_bytes[lb] as usize;
    if cursor + payload > data.len() {
      return None;
    }
    cursor += payload;
  }

  Some(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn from_hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
  }

  #[test]
  fn f20_values() {
    assert_eq!(compute_f20(2704), 11); // DX
    assert_eq!(compute_f20(4140), 17); // FX
    assert_eq!(compute_f20(2800), 11); // Z50 II half-pass (5600/2)
  }

  #[test]
  fn dx_sig_bytes_for_5600() {
    // The DX fix: Z50 II (5600 wide) per-LB significance sizes.
    assert_eq!(compute_lb_sig_bytes(5600), [12, 12, 11, 12, 11, 11, 11, 11]);
  }

  #[test]
  fn mini_header_bit_extraction() {
    // Build a minimal precinct: prefix (12 bytes) + one LB mini-header with known
    // fields + payload, and enough LBs to satisfy the 8-LB walk with tiny sizes.
    // We test field extraction on LB0 with f20_sign=0.
    // data_bytes=3, gcli_bytes=5, sign_bytes=7 -> pack into 56 bits.
    let f20_sign = 0u64;
    let data_bytes = 3u64;
    let gcli_bytes = 5u64;
    let sign_bytes = 7u64;
    let val: u64 = (f20_sign << 55) | (data_bytes << 35) | (gcli_bytes << 15) | sign_bytes;
    let mut hdr = vec![0u8; 12]; // prefix
    // width 5600 -> lb_sig[0] = 12
    let sig0 = 12usize;
    // LB0 mini-header (7 bytes, big-endian of val's low 56 bits)
    let mut mh = Vec::new();
    for k in (0..7).rev() {
      mh.push(((val >> (k * 8)) & 0xff) as u8);
    }
    hdr.extend_from_slice(&mh);
    hdr.extend(std::iter::repeat_n(0u8, sig0 + gcli_bytes as usize + data_bytes as usize + sign_bytes as usize));
    // Remaining 7 LBs: give them zero-size mini-headers (all fields 0) + sig only.
    // width 5600 sig sizes = [12,12,11,12,11,11,11,11]; provide payloads.
    let sig = compute_lb_sig_bytes(5600);
    for &s in sig.iter().skip(1) {
      hdr.extend(std::iter::repeat_n(0u8, 7)); // zero mini-header
      hdr.extend(std::iter::repeat_n(0u8, s as usize)); // sig payload (others 0)
    }

    let ps = parse_precinct_header(&hdr, 5600).expect("parse");
    assert_eq!(ps.lb_f20_sign[0], 0);
    assert_eq!(ps.lb_data_bytes[0], 3);
    assert_eq!(ps.lb_gcli_bytes[0], 5);
    assert_eq!(ps.lb_sign_bytes[0], 7);
    assert_eq!(ps.lb_sig_bytes[0], 12);
    assert_eq!(ps.lb_sig_offset[0], 19); // 12 prefix + 7 mini-header
  }

  #[test]
  fn too_short_returns_none() {
    assert!(parse_precinct_header(&[0u8; 10], 5600).is_none());
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder + DX fix).
  //
  // The raw bytes of precinct 0 from real DSC_8070.NEF, and the reference's
  // parsed PrecinctSizes for it. Re-parsing in Rust must reproduce every field
  // — total_size, Bp, Br, all 28 Dpb, and all 8 LBs' sig/gcli/data/sign byte
  // counts + sig offsets. This exercises the DX significance formula end to end.
  #[test]
  fn oracle_precinct0_dsc8070_bit_exact() {
    let fixture = include_str!("testdata/precinct_hdr_p0_8070.txt");
    let mut width = 0i32;
    let mut raw: Vec<u8> = Vec::new();
    let mut exp = PrecinctSizes::default();
    for line in fixture.lines() {
      if let Some(v) = line.strip_prefix("WIDTH=") {
        width = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("RAW=") {
        raw = from_hex(v.trim());
      } else if let Some(v) = line.strip_prefix("TOTAL_SIZE=") {
        exp.total_size = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("BP=") {
        exp.bp = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("BR=") {
        exp.br = v.trim().parse().unwrap();
      } else if let Some(v) = line.strip_prefix("DPB=") {
        for (i, x) in v.trim().split(',').filter(|s| !s.is_empty()).enumerate() {
          exp.dpb[i] = x.parse().unwrap();
        }
      } else if let Some(v) = line.strip_prefix("SIG=") {
        for (i, x) in v.trim().split(',').filter(|s| !s.is_empty()).enumerate() {
          exp.lb_sig_bytes[i] = x.parse().unwrap();
        }
      } else if let Some(v) = line.strip_prefix("GCLI=") {
        for (i, x) in v.trim().split(',').filter(|s| !s.is_empty()).enumerate() {
          exp.lb_gcli_bytes[i] = x.parse().unwrap();
        }
      } else if let Some(v) = line.strip_prefix("DATA=") {
        for (i, x) in v.trim().split(',').filter(|s| !s.is_empty()).enumerate() {
          exp.lb_data_bytes[i] = x.parse().unwrap();
        }
      } else if let Some(v) = line.strip_prefix("SIGN=") {
        for (i, x) in v.trim().split(',').filter(|s| !s.is_empty()).enumerate() {
          exp.lb_sign_bytes[i] = x.parse().unwrap();
        }
      } else if let Some(v) = line.strip_prefix("SIGOFF=") {
        for (i, x) in v.trim().split(',').filter(|s| !s.is_empty()).enumerate() {
          exp.lb_sig_offset[i] = x.parse().unwrap();
        }
      }
    }

    let ps = parse_precinct_header(&raw, width).expect("parse precinct 0");
    assert_eq!(ps.total_size, exp.total_size, "total_size");
    assert_eq!(ps.bp, exp.bp, "Bp");
    assert_eq!(ps.br, exp.br, "Br");
    assert_eq!(ps.dpb, exp.dpb, "Dpb");
    assert_eq!(ps.lb_sig_bytes, exp.lb_sig_bytes, "sig bytes");
    assert_eq!(ps.lb_gcli_bytes, exp.lb_gcli_bytes, "gcli bytes");
    assert_eq!(ps.lb_data_bytes, exp.lb_data_bytes, "data bytes");
    assert_eq!(ps.lb_sign_bytes, exp.lb_sign_bytes, "sign bytes");
    assert_eq!(ps.lb_sig_offset, exp.lb_sig_offset, "sig offsets");
  }
}
