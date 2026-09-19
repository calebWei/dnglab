// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-production), src/decoders/nikon_he/
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Sub-band layout configuration.
//!
//! Computes the spatial layout of 26 sub-bands within 8 line-blocks (LBs) for
//! one pass of a half-width image row. Drives scatter-address computation,
//! horizontal IDWT ordering and buffer assignment.
//!
//! Unlike the C++ original (which returns pointers into a program-lifetime
//! static `LayoutInfo`), this port returns the `LayoutInfo` by value; callers
//! pass it alongside the `[SubbandConfig; 26]`.

/// Static layout information shared across all sub-bands for one image width.
#[derive(Debug, Clone, Default)]
pub struct LayoutInfo {
  /// `ceil(W / 8)`.
  pub ng_max: i32,
  /// `W / 4`.
  pub ng_ll: i32,
  /// Per-LB `ng_lift` for the 6 five-level lift sub-bands.
  pub ng_lift: [i32; 6],
  /// `round_up(ng_LL, 32)`.
  pub memcpy_lb: i32,
  /// `memcpy_LB + round_up(ceil(ng_max/16), 32)`.
  pub lift_lb: i32,
  /// Cumsum of `round_up(ng_lift[i], 8)` for pass-A lift LBs.
  pub passa_hl_offset: [i32; 6],
  /// Pass-B HL offsets (2 sub-bands).
  pub passb_hl_offset: [i32; 2],
}

/// One entry per sub-band (0..25).
#[derive(Debug, Clone, Copy, Default)]
pub struct SubbandConfig {
  /// Number of coefficient groups (each group = 4 coefficients).
  pub ng: i32,
  /// Scatter offset into the line-buffer (in groups; write at `x24 * 4`).
  pub x24: i32,
  /// Processing priority (lower decodes earlier).
  pub priority: i32,
  /// Target buffer: 0 = bufA, 1 = bufB.
  pub buffer_idx: i32,
}

#[inline]
fn round_up(x: i32, m: i32) -> i32 {
  ((x + m - 1) / m) * m
}

/// Compute the full sub-band layout for a given half-pass width
/// (`half_pass_W = image_width / 2`).
pub fn compute_subband_layout(half_pass_w: i32) -> (LayoutInfo, [SubbandConfig; 26]) {
  // --- Core sizes ---
  let ng_max = (half_pass_w + 7) / 8; // ceil(W/8)
  let ng_ll = half_pass_w / 4;
  let n4 = (ng_max + 1) / 2; // ceil(ng_max/2)
  let n5 = (n4 + 1) / 2; // ceil(n4/2)
  let n_h5 = n4 - n5;

  // Per-sub-band ng for a 5-level lift LB.
  let ng_5level: [i32; 6] = [
    (n5 + 3) / 4,     // ceil(N5/4)
    (n_h5 + 3) / 4,   // ceil(N_H5/4)
    (ng_max + 7) / 8, // ceil(ng_max/8)
    (ng_max + 3) / 4, // ceil(ng_max/4)
    (ng_max + 1) / 2, // ceil(ng_max/2)
    ng_max,
  ];

  // --- LB region sizes ---
  let memcpy_lb = round_up(ng_ll, 32);
  let lift_lb = memcpy_lb + round_up((ng_max + 15) / 16, 32);

  // --- Sub-band definitions: (sb, lb, ng_val, priority) ---
  struct SbDef {
    sb: usize,
    lb: i32,
    ng_val: i32,
    priority: i32,
  }
  let defs: [SbDef; 26] = [
    // LB 0: sb 0-5, priorities 0-5
    SbDef { sb: 0, lb: 0, ng_val: ng_5level[0], priority: 0 },
    SbDef { sb: 1, lb: 0, ng_val: ng_5level[1], priority: 1 },
    SbDef { sb: 2, lb: 0, ng_val: ng_5level[2], priority: 2 },
    SbDef { sb: 3, lb: 0, ng_val: ng_5level[3], priority: 3 },
    SbDef { sb: 4, lb: 0, ng_val: ng_5level[4], priority: 4 },
    SbDef { sb: 5, lb: 0, ng_val: ng_5level[5], priority: 5 },
    // LB 1: sb 6-11, priorities 18-23
    SbDef { sb: 6, lb: 1, ng_val: ng_5level[0], priority: 18 },
    SbDef { sb: 7, lb: 1, ng_val: ng_5level[1], priority: 19 },
    SbDef { sb: 8, lb: 1, ng_val: ng_5level[2], priority: 20 },
    SbDef { sb: 9, lb: 1, ng_val: ng_5level[3], priority: 21 },
    SbDef { sb: 10, lb: 1, ng_val: ng_5level[4], priority: 22 },
    SbDef { sb: 11, lb: 1, ng_val: ng_5level[5], priority: 23 },
    // LB 2: sb 12, priority 36 (memcpy LL)
    SbDef { sb: 12, lb: 2, ng_val: ng_ll, priority: 36 },
    // LB 3: sb 13-18, priorities 54-59
    SbDef { sb: 13, lb: 3, ng_val: ng_5level[0], priority: 54 },
    SbDef { sb: 14, lb: 3, ng_val: ng_5level[1], priority: 55 },
    SbDef { sb: 15, lb: 3, ng_val: ng_5level[2], priority: 56 },
    SbDef { sb: 16, lb: 3, ng_val: ng_5level[3], priority: 57 },
    SbDef { sb: 17, lb: 3, ng_val: ng_5level[4], priority: 58 },
    SbDef { sb: 18, lb: 3, ng_val: ng_5level[5], priority: 59 },
    // LB 4: sb 19-20, priorities 6-7 (1-level lift, ng_max)
    SbDef { sb: 19, lb: 4, ng_val: ng_max, priority: 6 },
    SbDef { sb: 20, lb: 4, ng_val: ng_max, priority: 7 },
    // LB 5: sb 21-22, priorities 24-25
    SbDef { sb: 21, lb: 5, ng_val: ng_max, priority: 24 },
    SbDef { sb: 22, lb: 5, ng_val: ng_max, priority: 25 },
    // LB 6: sb 23, priority 36 (memcpy LL)
    SbDef { sb: 23, lb: 6, ng_val: ng_ll, priority: 36 },
    // LB 7: sb 24-25, priorities 60-61
    SbDef { sb: 24, lb: 7, ng_val: ng_max, priority: 60 },
    SbDef { sb: 25, lb: 7, ng_val: ng_max, priority: 61 },
  ];

  // --- HL offset tables ---
  let ng_lift_for_hl = ng_5level;
  let mut passa_hl_offset = [0i32; 6];
  let mut cumsum = 0;
  for i in 0..6 {
    passa_hl_offset[i] = cumsum;
    cumsum += round_up(ng_lift_for_hl[i], 8);
  }
  let passb_hl_offset = [0, round_up(ng_max, 8)];

  let layout = LayoutInfo {
    ng_max,
    ng_ll,
    ng_lift: ng_lift_for_hl,
    memcpy_lb,
    lift_lb,
    passa_hl_offset,
    passb_hl_offset,
  };

  // --- x24 scatter offsets (buffer-local; bufA/bufB accumulate independently) ---
  let lb_base = [0, lift_lb, 2 * lift_lb, 2 * lift_lb + memcpy_lb]; // per-buffer LB base
  let mut lb_sb_x24 = [0i32; 8];

  let mut out = [SubbandConfig::default(); 26];
  for d in &defs {
    let buffer_idx = if d.lb >= 4 { 1 } else { 0 };
    let local_lb = (if buffer_idx == 1 { d.lb - 4 } else { d.lb }) as usize;
    let x24 = lb_base[local_lb] + lb_sb_x24[d.lb as usize];
    lb_sb_x24[d.lb as usize] += round_up(d.ng_val, 8);
    out[d.sb] = SubbandConfig { ng: d.ng_val, x24, priority: d.priority, buffer_idx };
  }

  (layout, out)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn layout_sizes_for_5600_wide() {
    // image_width 5600 -> half_pass_W 2800
    let (li, cfg) = compute_subband_layout(2800);
    assert_eq!(li.ng_max, 350); // ceil(2800/8)
    assert_eq!(li.ng_ll, 700); // 2800/4
    assert_eq!(li.memcpy_lb, 704); // round_up(700,32)
    assert_eq!(li.lift_lb, 736); // 704 + round_up(ceil(350/16)=22,32)=32
    // priorities are a set of distinct-ish values; LL sub-bands share 36.
    assert_eq!(cfg[12].priority, 36);
    assert_eq!(cfg[23].priority, 36);
    assert_eq!(cfg[12].buffer_idx, 0);
    assert_eq!(cfg[23].buffer_idx, 1);
    // LB0 first sub-band sits at x24=0.
    assert_eq!(cfg[0].x24, 0);
    assert_eq!(cfg[0].ng, (((350 + 1) / 2 + 1) / 2 + 3) / 4);
  }
}
