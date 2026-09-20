// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_coefficient_decode.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* coefficient magnitude + sign decode for one sub-band.
//!
//! After GCLI decode, each group's coefficient magnitudes are carried in the
//! **data** substream as interleaved bit-planes — one nibble per bit-plane, its 4
//! bits feeding the 4 coefficients of the group (bit 3 → coeff 0 … bit 0 → coeff
//! 3), MSB-first. A group codes `gcli - gtli` bit-planes; the assembled magnitude
//! is then shifted left by `gtli` to restore the implicit truncated low bits.
//!
//! The **sign** substream then carries one bit per *non-zero* coefficient (`1` =
//! negate); zero coefficients consume no sign bit.

use super::bit_reader::BitReader;

/// Unpack coefficient magnitudes for one sub-band from the data substream.
///
/// `gcli_values` holds `num_groups` per-group GCLIs (from `gcli_decode`). Writes
/// `num_groups * 4` magnitudes to `coefficients_out` (zeroing it first). A group
/// with `gcli <= gtli` contributes four zeros and reads nothing.
///
/// Faithful to the reference `unpack_coefficient_magnitudes`.
// Consumed by `precinct_decode` (not yet ported).
#[cfg_attr(not(test), allow(dead_code))]
pub fn unpack_coefficient_magnitudes(data_reader: &mut BitReader, gcli_values: &[u8], gtli: i32, num_groups: usize, coefficients_out: &mut [i32]) {
  let total = num_groups * 4;
  coefficients_out[..total].fill(0);

  for g in 0..num_groups {
    let num_bitplanes = gcli_values[g] as i32 - gtli;
    if num_bitplanes <= 0 {
      continue; // all four coefficients in this group are zero
    }

    // Assemble magnitudes bit-plane by bit-plane, MSB first. Each nibble's bits
    // 3,2,1,0 feed coefficients 0,1,2,3 respectively.
    let mut m = [0i32; 4];
    for _ in 0..num_bitplanes {
      let nibble = data_reader.read_bits(4);
      m[0] = (m[0] << 1) | ((nibble >> 3) & 1) as i32;
      m[1] = (m[1] << 1) | ((nibble >> 2) & 1) as i32;
      m[2] = (m[2] << 1) | ((nibble >> 1) & 1) as i32;
      m[3] = (m[3] << 1) | (nibble & 1) as i32;
    }

    // Restore the implicit-zero low bits truncated at encode time.
    coefficients_out[g * 4] = m[0] << gtli;
    coefficients_out[g * 4 + 1] = m[1] << gtli;
    coefficients_out[g * 4 + 2] = m[2] << gtli;
    coefficients_out[g * 4 + 3] = m[3] << gtli;
  }
}

/// Apply sign bits to `coefficients` in place.
///
/// Reads one bit per non-zero coefficient from the sign substream (`1` negates);
/// zero coefficients consume no bit. Processes the whole slice.
///
/// Faithful to the reference `apply_sign_bits`.
// Consumed by `precinct_decode` (not yet ported).
#[cfg_attr(not(test), allow(dead_code))]
pub fn apply_sign_bits(sign_reader: &mut BitReader, coefficients: &mut [i32]) {
  for c in coefficients.iter_mut() {
    if *c == 0 {
      continue; // zero coefficients consume no sign bit
    }
    if sign_reader.read_bits(1) == 1 {
      *c = -*c;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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

  fn from_hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
  }

  #[test]
  fn zero_group_when_gcli_le_gtli() {
    // gcli == gtli -> 0 bit-planes -> all zero, no data read.
    let data = [0xFFu8; 4];
    let mut dr = BitReader::new(&data);
    let mut out = [0i32; 4];
    unpack_coefficient_magnitudes(&mut dr, &[5], 5, 1, &mut out);
    assert_eq!(out, [0, 0, 0, 0]);
    assert_eq!(dr.bytes_read(), 0, "no nibbles consumed");
  }

  #[test]
  fn single_bitplane_distributes_nibble_bits() {
    // gcli=1, gtli=0 -> 1 bit-plane. One nibble 0b1010 -> coeff0=1,c1=0,c2=1,c3=0.
    let data = pack_bits(&[1, 0, 1, 0]);
    let mut dr = BitReader::new(&data);
    let mut out = [0i32; 4];
    unpack_coefficient_magnitudes(&mut dr, &[1], 0, 1, &mut out);
    assert_eq!(out, [1, 0, 1, 0]);
  }

  #[test]
  fn multi_bitplane_msb_first_then_shift() {
    // gcli=3, gtli=1 -> 2 bit-planes. Coeff 0 gets bit3 of each nibble.
    // nibble0=0b1000, nibble1=0b1000 -> m[0] = 0b11 = 3; <<gtli(1) = 6.
    // coeff1 gets bit2: nibble0 bit2=0, nibble1 bit2=0 -> 0.
    let data = pack_bits(&[1, 0, 0, 0, /* nibble0 */ 1, 0, 0, 0 /* nibble1 */]);
    let mut dr = BitReader::new(&data);
    let mut out = [0i32; 4];
    unpack_coefficient_magnitudes(&mut dr, &[3], 1, 1, &mut out);
    assert_eq!(out, [6, 0, 0, 0]);
  }

  #[test]
  fn sign_bits_skip_zeros() {
    // coeffs [4,0,7,0,2]; non-zero at 0,2,4. Sign bits 1,0,1 -> negate 0 and 4.
    let mut coeffs = [4i32, 0, 7, 0, 2];
    let sign = pack_bits(&[1, 0, 1]);
    let mut sr = BitReader::new(&sign);
    apply_sign_bits(&mut sr, &mut coeffs);
    assert_eq!(coeffs, [-4, 0, 7, 0, -2]);
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder).
  //
  // Captured from the reference decoding real HE file DSC_8070.NEF: precinct 0,
  // line-block 0. The 6 sub-bands share ONE data reader and ONE sign reader
  // (state persists across sub-bands), so the fixture replays all 6 in order:
  // for each sub-band, unpack magnitudes (data reader) then apply signs (sign
  // reader), asserting the final signed coefficients match. Per-sub-band gcli
  // values and gtli are captured alongside. See tools/nikon_he_oracle README.
  #[test]
  fn oracle_lb0_dsc8070_bit_exact() {
    let fixture = include_str!("testdata/coeff_lb0_8070.txt");
    let mut data_hex = "";
    let mut sign_hex = "";
    // (ng, gtli, gcli_values, expected signed coeffs)
    let mut subs: Vec<(usize, i32, Vec<u8>, Vec<i32>)> = Vec::new();
    for line in fixture.lines() {
      if let Some(h) = line.strip_prefix("DATA=") {
        data_hex = h;
      } else if let Some(h) = line.strip_prefix("SIGN=") {
        sign_hex = h;
      } else if let Some(rest) = line.strip_prefix("SB ") {
        // <sb> <ng> <gtli> gcli=<hex> coeffs=<hex u32,...>
        let ng: usize = rest.split_whitespace().nth(1).unwrap().parse().unwrap();
        let gtli: i32 = rest.split_whitespace().nth(2).unwrap().parse().unwrap();
        let gcli_hex = rest.split("gcli=").nth(1).unwrap().split_whitespace().next().unwrap();
        let coeffs_hex = rest.split("coeffs=").nth(1).unwrap().trim();
        let gcli = from_hex(gcli_hex);
        assert_eq!(gcli.len(), ng);
        let coeffs: Vec<i32> = coeffs_hex
          .split(',')
          .filter(|s| !s.is_empty())
          .map(|s| u32::from_str_radix(s, 16).unwrap() as i32)
          .collect();
        assert_eq!(coeffs.len(), ng * 4);
        subs.push((ng, gtli, gcli, coeffs));
      }
    }
    assert_eq!(subs.len(), 6, "LB0 has 6 sub-bands");

    let data = from_hex(data_hex);
    let sign = from_hex(sign_hex);
    let mut dr = BitReader::new(&data);
    let mut sr = BitReader::new(&sign);

    for (idx, (ng, gtli, gcli, expected)) in subs.iter().enumerate() {
      let mut coeffs = vec![0i32; ng * 4];
      unpack_coefficient_magnitudes(&mut dr, gcli, *gtli, *ng, &mut coeffs);
      apply_sign_bits(&mut sr, &mut coeffs);
      assert_eq!(&coeffs, expected, "sub-band {idx} (ng={ng}, gtli={gtli}) mismatch vs reference");
    }
  }
}
