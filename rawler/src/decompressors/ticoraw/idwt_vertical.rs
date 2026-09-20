// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
//   nikon_he_idwt_vertical.{h,cpp}.
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* vertical inverse 5/3 DWT — a per-LB-component state machine.
//!
//! After the horizontal IDWT, each precinct yields the line-blocks' horizontally
//! reconstructed rows. The vertical IDWT lifts these across precincts (i.e. down
//! the image) with one small state machine per LB component, whose carry buffers
//! (`x2`, `x3`) persist between calls. The tile stage drives one call per
//! LB per pass per precinct row (plus a partial-tile tail).
//!
//! Two entry paths select the initial state:
//!
//! * **Path A** (tile 0): `2 → 5 → 7 → 8 → (7 → 8)* → 9 → 11`.
//! * **Path B** (tile > 0): `0 → 1 → 4 → 7 → 8 → (7 → 8)* → 9 → 11`.
//!
//! Only states 7, 8 and 9 write to the output (`x1`). The tail states (7 and 9
//! reached with no input row, `x0 = None`) flush the remaining carry. Every loop
//! body is element-wise independent across lanes, and all shifts are arithmetic
//! on signed `i32` (matching the C++ `>>` on `int32_t`). When `n == 0` (the
//! memcpy LB) the bodies are no-ops but the state still advances.

/// LB components processed by the vertical IDWT (LB 0/1/3 lift, LB 2 memcpy).
#[cfg_attr(not(test), allow(dead_code))]
pub const VER_COMPONENTS: usize = 4;

/// Vertical-lift phase-counter states.
pub mod ver_lift_state {
  /// Path A entry (tile 0).
  pub const INIT2: i32 = 2;
  /// Path B entry (tile > 0).
  pub const INIT0: i32 = 0;
  pub const INIT1: i32 = 1;
  pub const INIT4: i32 = 4;
  pub const STATE5: i32 = 5;
  /// First x1 write / steady-state even tick.
  pub const STATE7: i32 = 7;
  /// Steady-state odd tick (main 5/3 lift body).
  pub const STATE8: i32 = 8;
  /// Partial-tile tail flush.
  pub const STATE9: i32 = 9;
  /// Done (no further writes).
  pub const STATE11: i32 = 11;
}

/// Per-LB carry + phase state for the vertical IDWT.
///
/// Unlike the reference (which holds raw pointers into tile-wide carry arrays),
/// this owns its two carry buffers; the tile stage keeps one `VerLiftStatePerLb`
/// per component. `x2`/`x3` are sized `lift_LB * 4` (the lift LBs; the memcpy LB
/// uses `n = 0` and never touches them).
#[cfg_attr(not(test), allow(dead_code))]
pub struct VerLiftStatePerLb {
  pub x2_carry: Vec<i32>,
  pub x3_carry: Vec<i32>,
  pub state: i32,
}

#[cfg_attr(not(test), allow(dead_code))]
impl VerLiftStatePerLb {
  /// A component's carry buffers of `size` ints, at the given initial `state`.
  pub fn new(size: usize, state: i32) -> Self {
    Self {
      x2_carry: vec![0i32; size],
      x3_carry: vec![0i32; size],
      state,
    }
  }
}

/// Run one vertical-lift step for one LB component.
///
/// `x0` is this LB's horizontal-lift output row (`None` for a partial-tile tail
/// flush); `x1_out` receives the reconstructed row when the current state writes
/// (states 7, 8, 9); `n` is the sample count (`lift_LB * 4` for lift LBs, `0`
/// for the memcpy LB, which still advances state). The state and carry buffers
/// in `st` are updated in place.
///
/// Faithful to the reference `ver_lift_lb_step` transition table.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ver_lift_lb_step(x0: Option<&[i32]>, x1_out: &mut [i32], n: usize, st: &mut VerLiftStatePerLb) {
  use ver_lift_state::*;
  let x2 = &mut st.x2_carry;
  let x3 = &mut st.x3_carry;

  match st.state {
    INIT0 => {
      // Path B entry: x2[i] = x0[i].
      let x0 = x0.unwrap_or(&[]);
      for i in 0..n {
        x2[i] = x0[i];
      }
      st.state = INIT1;
    }
    INIT1 => {
      // x2[i] = (x0[i] << 2) - x2[i].
      let x0 = x0.unwrap_or(&[]);
      for i in 0..n {
        x2[i] = (x0[i] << 2) - x2[i];
      }
      st.state = INIT4;
    }
    INIT4 => {
      // w = x2 - x0; x3 = (w+1)>>2; x2 = x0.
      let x0 = x0.unwrap_or(&[]);
      for i in 0..n {
        let w = x2[i] - x0[i];
        x3[i] = (w + 1) >> 2;
        x2[i] = x0[i];
      }
      st.state = STATE7;
    }
    INIT2 => {
      // Path A entry: x2[i] = x0[i] << 2.
      let x0 = x0.unwrap_or(&[]);
      for i in 0..n {
        x2[i] = x0[i] << 2;
      }
      st.state = STATE5;
    }
    STATE5 => {
      // w = x2 - 2*x0; x3 = (w+1)>>2; x2 = x0.
      let x0 = x0.unwrap_or(&[]);
      for i in 0..n {
        let w = x2[i] - 2 * x0[i];
        x3[i] = (w + 1) >> 2;
        x2[i] = x0[i];
      }
      st.state = STATE7;
    }
    STATE7 => {
      match x0 {
        None => {
          // Partial-tile tail: x1 = x3.
          x1_out[..n].copy_from_slice(&x3[..n]);
          st.state = STATE9;
        }
        Some(x0) => {
          // tmp = x3; x1 = tmp; x3 = tmp + 2*x2; x2 = (x0<<2) - x2.
          for i in 0..n {
            let tmp = x3[i];
            x1_out[i] = tmp;
            x3[i] = tmp + 2 * x2[i];
            x2[i] = (x0[i] << 2) - x2[i];
          }
          st.state = STATE8;
        }
      }
    }
    STATE8 => {
      // w = (x2 - x0 + 1) >> 2; x1 = (w + x3) >> 1; x2 = x0; x3 = w.
      let x0 = x0.unwrap_or(&[]);
      for i in 0..n {
        let w = (x2[i] - x0[i] + 1) >> 2;
        x1_out[i] = (w + x3[i]) >> 1;
        x2[i] = x0[i];
        x3[i] = w;
      }
      st.state = STATE7;
    }
    STATE9 => {
      // Tail flush: x1 = x3 + x2.
      for i in 0..n {
        x1_out[i] = x3[i] + x2[i];
      }
      st.state = STATE11;
    }
    STATE11 => {}
    _ => {}
  }
}

/// Initialize path-A (tile 0) state: all components start at state 2.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ver_lift_init_path_a(st: &mut [VerLiftStatePerLb]) {
  for s in st.iter_mut() {
    s.state = ver_lift_state::INIT2;
  }
}

/// Initialize path-B (tile > 0) state: all components start at state 0.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ver_lift_init_path_b(st: &mut [VerLiftStatePerLb]) {
  for s in st.iter_mut() {
    s.state = ver_lift_state::INIT0;
  }
}

/// Whether a ver_lift loop should advance the x1 write offset (pre-tick state of
/// LB 0 is > 5).
#[cfg_attr(not(test), allow(dead_code))]
pub fn ver_lift_should_advance_offset(st: &[VerLiftStatePerLb]) -> bool {
  !st.is_empty() && st[0].state > 5
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parse_lanes(s: &str) -> Vec<i32> {
    s.split(',').filter(|t| !t.is_empty()).map(|t| t.parse().unwrap()).collect()
  }

  // ORACLE CROSS-CHECK (bit-exact vs the reference decoder).
  //
  // Replays a real per-call trace of ver_lift_lb_step captured while decoding
  // DSC_8070.NEF: for each call we feed the recorded state_in and carry buffers
  // (x2in/x3in) and the input row (x0, or None), then assert the output row
  // (x1, for writing states), the mutated carries (x2out/x3out) and the new
  // state all match. The trace covers every transition — 0→1, 1→4, 2→5, 4→7,
  // 5→7, 7→8, 7→9 (tail), 8→7, 9→11, 11→11 — on real coefficients. The state
  // machine is element-wise independent, so 8 lanes fully exercise the math.
  #[test]
  fn oracle_ver_lift_calls_dsc8070_bit_exact() {
    let fixture = include_str!("testdata/idwt_vertical_calls_8070.txt");
    let mut n_calls = 0usize;
    let mut seen_states = std::collections::BTreeSet::new();
    for line in fixture.lines() {
      let rest = match line.strip_prefix("VL|") {
        Some(r) => r,
        None => continue,
      };
      // Layout: state_in|n|has_x0|state_out|<x0;x2i;x3i;x1;x2o;x3o>
      let mut hp = rest.splitn(5, '|');
      let state_in: i32 = hp.next().unwrap().parse().unwrap();
      let n: usize = hp.next().unwrap().parse().unwrap();
      let has_x0: i32 = hp.next().unwrap().parse().unwrap();
      let state_out: i32 = hp.next().unwrap().parse().unwrap();
      let body = hp.next().unwrap();
      let mut b = body.splitn(6, ';');
      let x0v = parse_lanes(b.next().unwrap());
      let x2i = parse_lanes(b.next().unwrap());
      let x3i = parse_lanes(b.next().unwrap());
      let x1e = parse_lanes(b.next().unwrap());
      let x2o = parse_lanes(b.next().unwrap());
      let x3o = parse_lanes(b.next().unwrap());

      // Only the first F lanes were captured; the state machine is lane-wise
      // independent, so replay with n = F (0 for the memcpy LB). `n` from the
      // header is the real (2944 / 0) width — used only to distinguish n==0.
      let f = if n == 0 { 0 } else { x2i.len() };

      let mut st = VerLiftStatePerLb {
        x2_carry: x2i.clone(),
        x3_carry: x3i.clone(),
        state: state_in,
      };
      let x0_opt = if has_x0 == 1 { Some(&x0v[..f]) } else { None };
      let mut x1 = vec![0i32; f];
      ver_lift_lb_step(x0_opt, &mut x1, f, &mut st);

      assert_eq!(st.state, state_out, "state transition from {state_in}");
      let writes = matches!(state_in, 7 | 8 | 9);
      for i in 0..f {
        if writes {
          assert_eq!(x1[i], x1e[i], "x1 lane {i}, state {state_in}");
        }
        assert_eq!(st.x2_carry[i], x2o[i], "x2 lane {i}, state {state_in}");
        assert_eq!(st.x3_carry[i], x3o[i], "x3 lane {i}, state {state_in}");
      }
      seen_states.insert(state_in);
      n_calls += 1;
    }
    assert!(n_calls >= 200, "expected the full trace, got {n_calls}");
    // Every entry/steady/tail state must be present.
    for s in [0, 1, 2, 4, 5, 7, 8, 9, 11] {
      assert!(seen_states.contains(&s), "state {s} missing from trace");
    }
  }

  // n == 0 (memcpy LB): no writes, but the state still advances.
  #[test]
  fn zero_n_advances_state_without_writing() {
    let mut st = VerLiftStatePerLb::new(0, ver_lift_state::INIT2);
    let mut x1: [i32; 0] = [];
    ver_lift_lb_step(Some(&[]), &mut x1, 0, &mut st);
    assert_eq!(st.state, ver_lift_state::STATE5);
    ver_lift_lb_step(Some(&[]), &mut x1, 0, &mut st);
    assert_eq!(st.state, ver_lift_state::STATE7);
  }

  #[test]
  fn path_init_helpers_set_states() {
    let mut st = vec![VerLiftStatePerLb::new(4, 99), VerLiftStatePerLb::new(4, 99)];
    ver_lift_init_path_a(&mut st);
    assert!(st.iter().all(|s| s.state == 2));
    assert!(ver_lift_should_advance_offset(&st) == false);
    ver_lift_init_path_b(&mut st);
    assert!(st.iter().all(|s| s.state == 0));
  }
}
