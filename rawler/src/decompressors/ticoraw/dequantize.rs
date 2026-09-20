// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_dequantize.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* coefficient dequantization.
//!
//! The encoder truncates the bottom `gtli` bits of every coefficient. The decoder
//! reconstructs a deadzone-midpoint estimate of those bits by scaling the coded
//! magnitude by a 16-entry constant table indexed by the number of coded
//! bit-planes, then applies the image-wide tone shift `w4 = 4`.
//!
//! Note: although the reference keeps a separate `dequantize_ll_coefficient` for
//! the LL bands (sub-bands 12 & 23), `dequantize_coefficient_array` applies the
//! uniform (non-LL) formula to *all* sub-bands — the `is_ll_band` flag is ignored.
//! The LL helper is ported for API fidelity but is not used on the decode path.

/// Deadzone-midpoint scaling constants, indexed by `bit_plane_count - 1`
/// (`bit_plane_count = gcli - gtli`). Index 15 is a sentinel (unreachable).
const MIDPOINT_SCALE_TABLE: [i32; 16] = [
  87381, 74898, 69905, 67650, 66576, 66052, 65793, 65664, 65600, 65568, 65552, 65544, 65540, 65538, 65537, 0,
];

/// Z9 HE image-wide tone shift.
const W4_SHIFT: i32 = 4;

/// Dequantize one (non-LL) coefficient.
///
/// `gcli` is the group's GCLI; `gtli` is the sub-band's GTLI (both the encoded and
/// target threshold are the same value in this format). Faithful to the reference
/// `dequantize_coefficient`, including the `u64` intermediate product.
pub fn dequantize_coefficient(coefficient: i32, gcli: i32, gtli: i32) -> i32 {
  if coefficient == 0 || gcli <= gtli {
    return 0; // zero, or the group was insignificant
  }
  let bit_plane_count = gcli - gtli;
  if !(1..=15).contains(&bit_plane_count) {
    return 0;
  }
  let magnitude = coefficient.unsigned_abs() >> gtli;
  let product = magnitude as u64 * MIDPOINT_SCALE_TABLE[(bit_plane_count - 1) as usize] as u64;
  let shifted = (product >> (16 - gtli)) as i32;
  let result = shifted << W4_SHIFT;
  if coefficient < 0 { -result } else { result }
}

/// Dequantize one LL-band coefficient.
///
/// `result = sign(coef) * ((|coef| >> gtli) * ((1 << w4) + 1)) << w4`. Ported for
/// API fidelity; the array path does not call this (see the module docs).
#[cfg_attr(not(test), allow(dead_code))]
pub fn dequantize_ll_coefficient(coefficient: i32, gcli: i32, gtli: i32) -> i32 {
  if coefficient == 0 || gcli <= gtli {
    return 0;
  }
  let magnitude = coefficient.unsigned_abs() >> gtli;
  let factor = (1 << W4_SHIFT) + 1; // = 17
  let result = ((magnitude as i32) * factor) << W4_SHIFT;
  if coefficient < 0 { -result } else { result }
}

/// Dequantize a sub-band's coefficient array in place.
///
/// Each group's 4 coefficients are dequantized with the group's GCLI and the
/// sub-band's `target_gtli`. `is_ll_band` is accepted for API parity but ignored
/// (the reference applies the uniform formula to all bands).
// Consumed by `precinct_decode` (not yet ported).
#[cfg_attr(not(test), allow(dead_code))]
pub fn dequantize_coefficient_array(
  coefficients: &mut [i32],
  coefficient_count: usize,
  gcli_values: &[u8],
  num_groups: usize,
  target_gtli: i32,
  _is_ll_band: bool,
) {
  for g in 0..num_groups {
    let gcli = gcli_values[g] as i32;
    for c in 0..4 {
      let idx = g * 4 + c;
      if idx >= coefficient_count {
        break;
      }
      coefficients[idx] = dequantize_coefficient(coefficients[idx], gcli, target_gtli);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn from_hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
  }

  #[test]
  fn zero_and_insignificant_return_zero() {
    assert_eq!(dequantize_coefficient(0, 8, 3), 0); // zero coefficient
    assert_eq!(dequantize_coefficient(100, 3, 3), 0); // gcli == gtli
    assert_eq!(dequantize_coefficient(100, 2, 3), 0); // gcli < gtli
  }

  #[test]
  fn matches_reference_formula_spotvalues() {
    // Recompute the reference formula by hand for a few (coef, gcli, gtli).
    let cases = [(824i32, 10, 3), (-824, 10, 3), (48, 6, 4), (17, 5, 4)];
    for (coef, gcli, gtli) in cases {
      let bpc = gcli - gtli;
      let mag = (coef.unsigned_abs()) >> gtli;
      let prod = mag as u64 * MIDPOINT_SCALE_TABLE[(bpc - 1) as usize] as u64;
      let expect_mag = ((prod >> (16 - gtli)) as i32) << W4_SHIFT;
      let expect = if coef < 0 { -expect_mag } else { expect_mag };
      assert_eq!(dequantize_coefficient(coef, gcli, gtli), expect, "coef={coef} gcli={gcli} gtli={gtli}");
    }
  }

  #[test]
  fn sign_is_preserved() {
    let pos = dequantize_coefficient(824, 10, 3);
    let neg = dequantize_coefficient(-824, 10, 3);
    assert!(pos > 0 && neg == -pos);
  }

  #[test]
  fn ll_coefficient_formula() {
    // (|coef|>>gtli) * 17 << 4. coef=64, gtli=3 -> (8)*17<<4 = 136<<4 = 2176.
    assert_eq!(dequantize_ll_coefficient(64, 8, 3), (8 * 17) << 4);
    assert_eq!(dequantize_ll_coefficient(-64, 8, 3), -((8 * 17) << 4));
    assert_eq!(dequantize_ll_coefficient(64, 3, 3), 0); // gcli == gtli
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder).
  //
  // Captured from the reference decoding real HE file DSC_8070.NEF: precinct 0,
  // line-block 0. For each of the 6 sub-bands the fixture holds the pre-dequant
  // (post-sign) coefficients as input and the post-dequant coefficients as
  // expected output, alongside gcli values and gtli. Dequant is per-coefficient
  // (no reader/cross-band state), so each sub-band is independent. See the
  // tools/nikon_he_oracle README for the capture method.
  #[test]
  fn oracle_lb0_dsc8070_bit_exact() {
    let fixture = include_str!("testdata/dequant_lb0_8070.txt");
    let mut count = 0;
    for line in fixture.lines() {
      let Some(rest) = line.strip_prefix("SB ") else { continue };
      // <sb> <ng> <gtli> gcli=<hex> in=<u32,...> out=<u32,...>
      let ng: usize = rest.split_whitespace().nth(1).unwrap().parse().unwrap();
      let gtli: i32 = rest.split_whitespace().nth(2).unwrap().parse().unwrap();
      let gcli = from_hex(rest.split("gcli=").nth(1).unwrap().split_whitespace().next().unwrap());
      let parse_ints = |s: &str| -> Vec<i32> {
        s.split(',')
          .filter(|x| !x.is_empty())
          .map(|x| u32::from_str_radix(x, 16).unwrap() as i32)
          .collect()
      };
      let mut coeffs = parse_ints(rest.split("in=").nth(1).unwrap().split_whitespace().next().unwrap());
      let expected = parse_ints(rest.split("out=").nth(1).unwrap().trim());
      assert_eq!(gcli.len(), ng);
      assert_eq!(coeffs.len(), ng * 4);
      assert_eq!(expected.len(), ng * 4);

      dequantize_coefficient_array(&mut coeffs, ng * 4, &gcli, ng, gtli, false);
      assert_eq!(coeffs, expected, "sub-band ng={ng} gtli={gtli} mismatch vs reference");
      count += 1;
    }
    assert_eq!(count, 6, "LB0 has 6 sub-bands");
  }
}
