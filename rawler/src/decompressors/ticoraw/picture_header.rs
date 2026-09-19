// SPDX-License-Identifier: LGPL-2.1
// Ported from yogthos/LibRaw (nikon-he-decoder), src/decoders/nikon_he/
// Original: Copyright (C) 2026 Dmitri Sotnikov, LGPL-2.1 / CDDL.

//! Nikon HE / HE\* JPEG-XS picture-header parser.
//!
//! Walks the codestream marker segments (SOC/CAP/PIH/CDT/WGT/NLT/SLH), extracts
//! the picture-header fields plus the per-band WGT `gain`/`priority` weights, and
//! locates the precinct stream (first byte after the initial SLH segment).
//!
//! Nikon appends a vendor-specific PIH tail; only the common prefix is consumed,
//! the remainder skipped via the marker length.

pub const MAX_WGT_BANDS: usize = 64;
pub const NIKON_HE_WGT_BANDS: i32 = 25;

const SOC: u16 = 0xff10;
const EOC: u16 = 0xff11;
const PIH: u16 = 0xff12;
const CDT: u16 = 0xff13;
const WGT: u16 = 0xff14;
const NLT: u16 = 0xff16;
const SLH: u16 = 0xff20;
const CAP: u16 = 0xff50;

#[derive(Debug, Clone)]
pub struct PictureHeader {
  pub valid: bool,
  pub precinct_offset: usize,
  pub lcod: u32,
  pub hdr_width: u16,
  pub hdr_height: u16,
  pub precinct_width: u16,
  pub hsl: u16,
  pub comps_num: u8,
  pub coeff_group_size: u8,
  pub significance_group_size: u8,
  pub bw: u8,
  pub nbands: i32,
  pub gain: [u8; MAX_WGT_BANDS],
  pub priority: [u8; MAX_WGT_BANDS],
  pub has_nlt: bool,
}

impl Default for PictureHeader {
  fn default() -> Self {
    Self {
      valid: false,
      precinct_offset: 0,
      lcod: 0,
      hdr_width: 0,
      hdr_height: 0,
      precinct_width: 0,
      hsl: 0,
      comps_num: 0,
      coeff_group_size: 0,
      significance_group_size: 0,
      bw: 0,
      nbands: 0,
      gain: [0; MAX_WGT_BANDS],
      priority: [0; MAX_WGT_BANDS],
      has_nlt: false,
    }
  }
}

impl PictureHeader {
  /// `T[p,b] = clamp(Qp - gain[b] - (priority[b] < Rp), 0, 15)`.
  #[inline]
  pub fn gtli_from_weights(&self, band: i32, qp: i32, rp: i32) -> i32 {
    if band < 0 || band >= self.nbands {
      return 0;
    }
    let mut v = qp - self.gain[band as usize] as i32 - if (self.priority[band as usize] as i32) < rp { 1 } else { 0 };
    if v < 0 {
      v = 0;
    }
    if v > 15 {
      v = 15;
    }
    v
  }
}

#[inline]
fn be16(p: &[u8]) -> u16 {
  ((p[0] as u16) << 8) | p[1] as u16
}
#[inline]
fn be32(p: &[u8]) -> u32 {
  ((p[0] as u32) << 24) | ((p[1] as u32) << 16) | ((p[2] as u32) << 8) | p[3] as u32
}

/// Parse the JPEG-XS picture header from the raw strip. Returns `Some(ph)` with
/// `ph.valid` reflecting whether all required markers (CAP/PIH/CDT/WGT) were seen
/// before the first SLH; `None` on structural failure.
pub fn parse_picture_header(strip: &[u8]) -> Option<PictureHeader> {
  let mut out = PictureHeader::default();
  if strip.len() < 8 || be16(strip) != SOC {
    return None;
  }

  let mut i = 2usize;
  let (mut saw_cap, mut saw_pih, mut saw_cdt, mut saw_wgt) = (false, false, false, false);

  while i + 2 <= strip.len() {
    let marker = be16(&strip[i..]);
    if marker & 0xff00 != 0xff00 {
      return None;
    }
    if marker == EOC {
      return None;
    }
    if i + 4 > strip.len() {
      return None;
    }
    let lseg = be16(&strip[i + 2..]) as usize;
    if lseg < 2 || i + 2 + lseg > strip.len() {
      return None;
    }
    let body = &strip[i + 4..i + 2 + lseg];
    let body_len = lseg - 2;

    match marker {
      CAP => {
        if saw_cap || body_len < 16 || &body[..16] != b"CONTACT_INTOPIX_" {
          return None;
        }
        saw_cap = true;
      }
      PIH => {
        if saw_pih || body_len < 20 {
          return None;
        }
        out.lcod = be32(&body[0..]);
        out.hdr_width = be16(&body[8..]);
        out.hdr_height = be16(&body[10..]);
        out.precinct_width = be16(&body[12..]);
        out.hsl = be16(&body[14..]);
        out.comps_num = body[16];
        out.coeff_group_size = body[17];
        out.significance_group_size = body[18];
        out.bw = body[19];
        saw_pih = true;
      }
      CDT => {
        if saw_cdt || body_len == 0 {
          return None;
        }
        saw_cdt = true;
      }
      WGT => {
        if saw_wgt || (body_len & 1) != 0 {
          return None;
        }
        let n = body_len / 2;
        if n == 0 || n > MAX_WGT_BANDS {
          return None;
        }
        for b in 0..n {
          out.gain[b] = body[2 * b];
          out.priority[b] = body[2 * b + 1];
        }
        out.nbands = n as i32;
        saw_wgt = true;
      }
      NLT => out.has_nlt = true,
      _ => {}
    }

    i += 2 + lseg;

    if marker == SLH {
      out.precinct_offset = i;
      out.valid = saw_cap && saw_pih && saw_cdt && saw_wgt;
      return Some(out);
    }
  }
  None
}

/// The decoder implements one fixed 4-component, 25-WGT-band Nikon profile.
pub fn is_supported_picture_header(ph: &PictureHeader, strip_size: usize) -> bool {
  ph.valid
    && ph.lcod as usize == strip_size
    && ph.hdr_width > 0
    && (ph.hdr_width & 1) == 0
    && ph.hdr_height > 0
    && ph.precinct_width == 0
    && ph.hsl == 16
    && ph.comps_num == 4
    && ph.coeff_group_size == 4
    && ph.significance_group_size == 8
    && ph.bw == 18
    && ph.nbands == NIKON_HE_WGT_BANDS
    && !ph.has_nlt
}

#[cfg(test)]
mod tests {
  use super::*;

  // Build a minimal synthetic codestream: SOC, CAP, PIH, CDT, WGT(25 bands), SLH.
  fn synth() -> Vec<u8> {
    let mut v = Vec::new();
    let seg = |v: &mut Vec<u8>, marker: u16, body: &[u8]| {
      v.extend_from_slice(&marker.to_be_bytes());
      let lseg = (body.len() + 2) as u16;
      v.extend_from_slice(&lseg.to_be_bytes());
      v.extend_from_slice(body);
    };
    v.extend_from_slice(&SOC.to_be_bytes());
    // CAP
    let mut cap = b"CONTACT_INTOPIX_".to_vec();
    cap.extend_from_slice(&[0u8; 4]);
    seg(&mut v, CAP, &cap);
    // PIH: Lcod(4) reserved(4) W(2) H(2) precinctW(2) Hsl(2) comps ng ss Bw
    let mut pih = vec![0u8; 20];
    // width=5600, height=3728 at offsets 8,10
    pih[8..10].copy_from_slice(&5600u16.to_be_bytes());
    pih[10..12].copy_from_slice(&3728u16.to_be_bytes());
    pih[12..14].copy_from_slice(&0u16.to_be_bytes()); // precinct_width
    pih[14..16].copy_from_slice(&16u16.to_be_bytes()); // Hsl
    pih[16] = 4; // comps
    pih[17] = 4; // coeff group
    pih[18] = 8; // sig group
    pih[19] = 18; // Bw
    seg(&mut v, PIH, &pih);
    // CDT
    seg(&mut v, CDT, &[0u8; 8]);
    // WGT: 25 bands * (gain, priority)
    let mut wgt = Vec::new();
    for b in 0..25u8 {
      wgt.push(b % 6); // gain
      wgt.push(b); // priority
    }
    seg(&mut v, WGT, &wgt);
    // SLH (empty-ish body)
    seg(&mut v, SLH, &[0u8; 2]);
    v
  }

  #[test]
  fn parses_and_validates() {
    let cs = synth();
    let ph = parse_picture_header(&cs).expect("parse");
    assert!(ph.valid);
    assert_eq!(ph.hdr_width, 5600);
    assert_eq!(ph.hdr_height, 3728);
    assert_eq!(ph.comps_num, 4);
    assert_eq!(ph.nbands, 25);
    assert_eq!(ph.hsl, 16);
    assert_eq!(ph.bw, 18);
    // precinct_offset points just past the SLH segment (end of buffer here).
    assert_eq!(ph.precinct_offset, cs.len());
    // Lcod in synth is 0, so is_supported fails on the Lcod==strip_size check.
    assert!(!is_supported_picture_header(&ph, cs.len()));
  }

  #[test]
  fn gtli_formula() {
    let cs = synth();
    let ph = parse_picture_header(&cs).unwrap();
    // band 3: gain=3, priority=3. Qp=5, Rp=4 -> 5-3-(3<4?1:0)=5-3-1=1
    assert_eq!(ph.gtli_from_weights(3, 5, 4), 1);
    // Rp=3 -> priority(3) < 3 is false -> 5-3-0 = 2
    assert_eq!(ph.gtli_from_weights(3, 5, 3), 2);
    // clamps to 0
    assert_eq!(ph.gtli_from_weights(3, 0, 4), 0);
    // clamps to 15
    assert_eq!(ph.gtli_from_weights(0, 100, 0), 15);
    // out-of-range band -> 0
    assert_eq!(ph.gtli_from_weights(99, 5, 4), 0);
  }

  #[test]
  fn rejects_without_soc() {
    assert!(parse_picture_header(&[0u8; 16]).is_none());
  }
}
