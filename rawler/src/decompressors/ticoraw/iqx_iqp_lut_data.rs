// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_iqx_iqp_lut_data.h.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* step1/step2 tone-curve LUT (piecewise-linear).
//!
//! The final Bayer reconstruction applies a fixed tone curve mapping a 17-bit-ish
//! coefficient domain (`0..81792`) to a 16-bit output. The curve is stored as 256
//! `(x_in, y_out)` breakpoints; the materialized 81792-entry table is
//! reconstructed by **integer** linear interpolation between consecutive
//! breakpoints. Per the reference, this PWL approximation deviates ≤ 1 LSB out of
//! 14 bits at the Bayer level — visually indistinguishable from the exact curve.
//!
//! The table is built lazily on first use and cached for the process lifetime
//! (mirrors the reference's static-local `std::array`). It is materialized into a
//! heap `Vec` rather than a stack array because it is ~320 KB.

use std::sync::OnceLock;

/// Number of entries in the materialized tone-curve LUT.
pub const IQX_IQP_LUT_SIZE: usize = 81792;

/// The 256 `(x_in, y_out)` piecewise-linear breakpoints, transcribed verbatim
/// from `kIqxIqpBreakpoints` in the reference header. `x` is monotonically
/// non-decreasing; the final breakpoint pins the tail out to `IQX_IQP_LUT_SIZE-1`.
static BREAKPOINTS: [[i32; 2]; 256] = [
  [0, 0],
  [349, 172],
  [845, 409],
  [1301, 620],
  [1672, 787],
  [1983, 924],
  [2369, 1090],
  [2834, 1284],
  [3334, 1485],
  [3750, 1646],
  [4206, 1817],
  [4660, 1981],
  [5157, 2153],
  [5601, 2300],
  [5976, 2420],
  [6396, 2549],
  [6837, 2679],
  [7287, 2805],
  [7703, 2916],
  [8023, 2998],
  [8459, 3105],
  [8936, 3215],
  [9447, 3325],
  [9845, 3405],
  [10266, 3485],
  [10613, 3546],
  [10934, 3600],
  [11349, 3665],
  [11769, 3725],
  [12151, 3775],
  [12562, 3824],
  [12938, 3864],
  [13259, 3895],
  [13632, 3927],
  [14064, 3959],
  [14498, 3985],
  [14901, 4004],
  [15293, 4018],
  [15643, 4026],
  [15980, 4031],
  [16626, 4035],
  [17132, 4051],
  [17667, 4082],
  [18186, 4126],
  [18709, 4184],
  [19245, 4258],
  [19756, 4342],
  [20300, 4446],
  [20840, 4564],
  [21346, 4688],
  [21884, 4834],
  [22420, 4994],
  [22951, 5167],
  [23486, 5356],
  [23993, 5548],
  [24557, 5777],
  [25087, 6007],
  [25584, 6236],
  [26118, 6495],
  [26628, 6757],
  [27101, 7011],
  [27559, 7268],
  [28035, 7546],
  [28545, 7857],
  [29005, 8149],
  [29479, 8460],
  [30010, 8823],
  [30507, 9176],
  [30964, 9511],
  [31418, 9854],
  [31931, 10255],
  [32442, 10667],
  [32447, 10672],
  [32894, 11043],
  [33406, 11481],
  [33898, 11915],
  [34402, 12371],
  [34417, 12386],
  [34831, 12771],
  [35334, 13250],
  [35370, 13286],
  [35753, 13660],
  [36247, 14153],
  [36739, 14658],
  [37177, 15117],
  [37700, 15678],
  [38193, 16220],
  [38734, 16828],
  [38752, 16850],
  [39116, 17267],
  [39446, 17653],
  [39716, 17971],
  [40201, 18553],
  [40697, 19161],
  [41176, 19760],
  [41658, 20374],
  [42156, 21021],
  [42538, 21526],
  [43000, 22146],
  [43006, 22155],
  [43155, 22357],
  [43611, 22983],
  [44207, 23818],
  [44720, 24551],
  [44730, 24566],
  [45133, 25151],
  [45539, 25749],
  [45553, 25770],
  [46028, 26480],
  [46347, 26964],
  [46670, 27459],
  [46943, 27882],
  [47308, 28452],
  [47577, 28878],
  [47866, 29338],
  [48105, 29722],
  [48432, 30252],
  [48605, 30535],
  [48935, 31078],
  [49031, 31238],
  [49306, 31696],
  [49714, 32383],
  [50159, 33142],
  [50660, 34008],
  [50665, 34018],
  [50979, 34567],
  [51460, 35419],
  [51480, 35455],
  [51803, 36034],
  [51966, 36328],
  [52285, 36908],
  [52435, 37182],
  [52816, 37884],
  [53272, 38733],
  [53277, 38743],
  [53385, 38946],
  [53735, 39607],
  [54160, 40418],
  [54165, 40428],
  [54519, 41111],
  [54544, 41159],
  [54863, 41780],
  [54889, 41832],
  [55103, 42251],
  [55132, 42309],
  [55236, 42514],
  [55495, 43026],
  [55601, 43238],
  [55932, 43899],
  [55947, 43931],
  [56023, 44082],
  [56030, 44098],
  [56046, 44129],
  [56125, 44289],
  [56230, 44501],
  [56549, 45149],
  [56568, 45189],
  [56581, 45214],
  [56585, 45224],
  [56612, 45277],
  [56613, 45281],
  [56626, 45306],
  [56634, 45324],
  [56647, 45349],
  [56658, 45373],
  [56732, 45524],
  [56856, 45779],
  [56887, 45843],
  [56897, 45862],
  [56908, 45886],
  [57027, 46131],
  [57243, 46578],
  [57363, 46828],
  [57426, 46959],
  [57728, 47591],
  [57789, 47720],
  [57795, 47731],
  [57808, 47759],
  [57947, 48052],
  [58151, 48484],
  [58520, 49270],
  [58542, 49318],
  [58769, 49805],
  [59050, 50412],
  [59408, 51191],
  [59451, 51286],
  [59477, 51342],
  [59531, 51461],
  [59883, 52235],
  [60267, 53087],
  [60269, 53093],
  [60295, 53150],
  [60636, 53913],
  [60642, 53927],
  [60815, 54317],
  [61153, 55082],
  [61664, 56251],
  [61839, 56654],
  [61862, 56708],
  [61895, 56783],
  [61896, 56787],
  [61933, 56871],
  [61934, 56875],
  [61975, 56968],
  [61976, 56972],
  [62013, 57056],
  [62014, 57060],
  [62044, 57128],
  [62045, 57132],
  [62079, 57209],
  [62080, 57213],
  [62397, 57950],
  [62400, 57958],
  [62436, 58041],
  [62437, 58045],
  [62474, 58130],
  [62475, 58134],
  [62513, 58221],
  [62514, 58225],
  [62530, 58261],
  [62531, 58265],
  [62578, 58374],
  [62587, 58396],
  [62813, 58927],
  [62815, 58933],
  [62832, 58971],
  [62833, 58975],
  [62870, 59061],
  [62875, 59074],
  [63029, 59437],
  [63031, 59443],
  [63064, 59520],
  [63065, 59524],
  [63078, 59553],
  [63080, 59559],
  [63302, 60086],
  [63597, 60791],
  [63603, 60806],
  [63767, 61200],
  [63804, 61289],
  [64102, 62009],
  [64118, 62047],
  [64124, 62062],
  [64193, 62230],
  [64224, 62305],
  [64429, 62804],
  [64707, 63484],
  [65110, 64477],
  [65130, 64527],
  [65400, 65197],
  [65456, 65337],
  [65493, 65429],
  [65518, 65491],
  [65530, 65521],
  [65535, 65534],
  [81791, 65534],
];

/// Materialize the 81792-entry PWL tone curve.
///
/// Faithful to the reference: walk `i` from 0, advancing the breakpoint index `k`
/// while `BREAKPOINTS[k+1].x <= i`, then interpolate with **integer** arithmetic
/// `ya + (yb - ya) * (i - xa) / (xb - xa)` (truncating division), or `ya` on a
/// zero-width segment. All operands stay well within `i32`, so this matches the
/// C++ `int` math bit-for-bit.
fn build() -> Vec<i32> {
  let mut arr = vec![0i32; IQX_IQP_LUT_SIZE];
  let mut k = 0usize;
  for (i, slot) in arr.iter_mut().enumerate() {
    let i = i as i32;
    while k < 255 && BREAKPOINTS[k + 1][0] <= i {
      k += 1;
    }
    if k >= 255 {
      // The final breakpoint is only reached at i == 81791 (its x). There xa == i,
      // so the interpolation degenerates to ya. The reference reads a would-be
      // out-of-bounds BREAKPOINTS[k+1] here, harmless in C++ only because the
      // (i - xa) == 0 factor zeroes it out; we take ya directly to stay in bounds.
      *slot = BREAKPOINTS[255][1];
      continue;
    }
    let xa = BREAKPOINTS[k][0];
    let xb = BREAKPOINTS[k + 1][0];
    let ya = BREAKPOINTS[k][1];
    let yb = BREAKPOINTS[k + 1][1];
    *slot = if xb == xa { ya } else { ya + (yb - ya) * (i - xa) / (xb - xa) };
  }
  arr
}

/// Return the cached tone-curve LUT (built once, `IQX_IQP_LUT_SIZE` entries).
// Consumed by the Bayer reconstruction / 3-pass driver (not yet ported).
#[cfg_attr(not(test), allow(dead_code))]
pub fn iqx_iqp_lut() -> &'static [i32] {
  static LUT: OnceLock<Vec<i32>> = OnceLock::new();
  LUT.get_or_init(build)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn size_and_caching() {
    let a = iqx_iqp_lut();
    let b = iqx_iqp_lut();
    assert_eq!(a.len(), IQX_IQP_LUT_SIZE);
    // Same cached allocation across calls.
    assert_eq!(a.as_ptr(), b.as_ptr());
  }

  #[test]
  fn breakpoints_are_exact_anchors() {
    // At each breakpoint x, the interpolated value equals that breakpoint's y —
    // the last segment starting at x is entered exactly at x (its xa==x, i-xa==0).
    let lut = iqx_iqp_lut();
    for bp in BREAKPOINTS.iter() {
      let (x, y) = (bp[0], bp[1]);
      if (x as usize) < IQX_IQP_LUT_SIZE {
        assert_eq!(lut[x as usize], y, "breakpoint x={x} should map to y={y}");
      }
    }
  }

  #[test]
  fn endpoints_and_monotonicity() {
    let lut = iqx_iqp_lut();
    assert_eq!(lut[0], 0);
    // Tail is pinned flat at 65534 from x=65535 through the end.
    assert_eq!(lut[IQX_IQP_LUT_SIZE - 1], 65534);
    assert_eq!(lut[65535], 65534);
    // The curve is non-decreasing (breakpoints are monotone; integer interp of a
    // non-decreasing PWL stays non-decreasing).
    let mut prev = i32::MIN;
    for &v in lut {
      assert!(v >= prev, "LUT must be non-decreasing");
      prev = v;
    }
  }

  #[test]
  fn interpolation_midpoint_matches_reference_formula() {
    // First segment: (0,0) -> (349,172). At i=174: 0 + 172*174/349 = 85 (trunc).
    let lut = iqx_iqp_lut();
    assert_eq!(lut[174], 172 * 174 / 349);
    // Second segment: (349,172) -> (845,409). At i=600:
    //   172 + (409-172)*(600-349)/(845-349) = 172 + 237*251/496.
    assert_eq!(lut[600], 172 + 237 * 251 / 496);
  }
}
