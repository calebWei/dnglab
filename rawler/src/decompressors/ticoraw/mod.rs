// SPDX-License-Identifier: LGPL-2.1
// Copyright 2026 RapidRAW / dnglab contributors

//! Decompressor for Nikon "High Efficiency" (HE) and "High Efficiency\*" (HE\*)
//! compressed NEF raw images.
//!
//! # Background
//!
//! Nikon's HE / HE\* raw compression (introduced with the EXPEED 7 processor:
//! Z 9, Z 8, Z 6III, Z f, Z 5II, ...) is intoPIX **TicoRAW**, which is a
//! bit-exact instance of the **JPEG-XS** codestream syntax (ISO/IEC 21122-1).
//!
//! This is confirmed directly from the raw strip of a sample file, which begins
//! with the JPEG-XS `SOC` marker and carries an intoPIX capabilities string:
//!
//! ```text
//! FF10                      SOC  (Start of codestream)
//! FF50 0022 "CONTACT_INTOPIX_..."   CAP  (capabilities marker segment)
//! FF12 0027 ...                 PIH  (picture header)
//! ...
//! FF11                      EOC  (End of codestream)
//! ```
//!
//! The picture header (PIH) of that sample decodes to:
//!
//! | Field | Bytes            | Value      | Meaning                    |
//! |-------|------------------|------------|----------------------------|
//! | Lcod  | `00 C7 18 00`    | 13047808   | codestream length (bytes)  |
//! | Wf    | `15 E0`          | 5600       | image width                |
//! | Hf    | `0E 90`          | 3728       | image height               |
//! | Nc    | `04`             | 4          | components (Bayer RGGB)     |
//!
//! (Lcod matches the TIFF `StripByteCounts`, and Wf/Hf match the raw SubIFD
//! dimensions, so the mapping is verified.)
//!
//! # Status: PROTOTYPE
//!
//! This module currently parses the JPEG-XS codestream framing (markers +
//! picture header). The entropy decode, dequantization, inverse 5/3 DWT and
//! Bayer reconstruction stages are **not yet implemented**. See `ROADMAP` in
//! the repository root for the porting plan (reference: the clean-room decoder
//! in yogthos/LibRaw PR #826).

mod bit_reader;
mod predict_lut;
mod subband_config;

use crate::pixarray::PixU16;

/// JPEG-XS codestream markers (ISO/IEC 21122-1). Only the subset relevant to
/// the Nikon HE codestreams observed so far is enumerated; every other marker
/// segment is skipped generically using its length field.
#[allow(dead_code)]
mod marker {
  pub const SOC: u16 = 0xFF10; // Start of codestream (no length)
  pub const EOC: u16 = 0xFF11; // End of codestream (no length)
  pub const PIH: u16 = 0xFF12; // Picture header
  pub const CDT: u16 = 0xFF13; // Component table
  pub const WGT: u16 = 0xFF14; // Weights table
  pub const COM: u16 = 0xFF15; // Extension / comment
  pub const NLT: u16 = 0xFF16; // Nonlinearity
  pub const CWD: u16 = 0xFF17; // Wavelet decomposition
  pub const CTS: u16 = 0xFF18; // Colour transform
  pub const CRG: u16 = 0xFF19; // Component registration
  pub const SLH: u16 = 0xFF20; // Slice header
  pub const CAP: u16 = 0xFF50; // Capabilities
}

/// Parsed JPEG-XS picture header (PIH), enough to drive the decode.
#[derive(Debug, Clone, Default)]
pub struct PictureHeader {
  /// `Lcod`: total codestream length in bytes.
  pub codestream_len: u32,
  /// `Ppih`: profile.
  pub profile: u16,
  /// `Plev`: level / sublevel.
  pub level: u16,
  /// `Wf`: image width in samples.
  pub width: u16,
  /// `Hf`: image height in samples.
  pub height: u16,
  /// `Cw`: precinct/column width (0 = full width).
  pub column_width: u16,
  /// `Hsl`: slice height in precinct rows.
  pub slice_height: u16,
  /// `Nc`: number of components.
  pub components: u8,
  /// `Ng`: coefficients per code group.
  pub group_size: u8,
  /// `Ss`: significance group size.
  pub sig_group_size: u8,
}

impl PictureHeader {
  /// Parse a PIH marker payload (the bytes *after* the marker and 16-bit length).
  fn parse(payload: &[u8]) -> Result<Self, String> {
    // PIH layout (big-endian), per ISO/IEC 21122-1:
    //   Lcod(32) Ppih(16) Plev(16) Wf(16) Hf(16) Cw(16) Hsl(16)
    //   Nc(8) Ng(8) Ss(8) ...
    if payload.len() < 21 {
      return Err(format!("TicoRAW: PIH payload too short ({} bytes)", payload.len()));
    }
    let be16 = |o: usize| u16::from_be_bytes([payload[o], payload[o + 1]]);
    let be32 = |o: usize| u32::from_be_bytes([payload[o], payload[o + 1], payload[o + 2], payload[o + 3]]);
    Ok(Self {
      codestream_len: be32(0),
      profile: be16(4),
      level: be16(6),
      width: be16(8),
      height: be16(10),
      column_width: be16(12),
      slice_height: be16(14),
      components: payload[16],
      group_size: payload[17],
      sig_group_size: payload[18],
    })
  }
}

/// The intoPIX capabilities string embedded in the `CAP` marker, when present.
fn cap_string(payload: &[u8]) -> Option<String> {
  let end = payload.iter().position(|&b| b == 0).unwrap_or(payload.len());
  let s: String = payload[..end].iter().filter(|&&b| b.is_ascii_graphic()).map(|&b| b as char).collect();
  if s.is_empty() { None } else { Some(s) }
}

/// Result of walking the codestream framing.
#[derive(Debug, Default)]
pub struct Codestream {
  pub pih: PictureHeader,
  pub capabilities: Option<String>,
  /// Byte offset (into the source slice) of the first `SLH`/entropy-coded data.
  pub body_offset: Option<usize>,
}

/// Walk the JPEG-XS marker segments and extract the header information.
///
/// This does **not** decode pixels; it validates the framing and pulls out the
/// picture-header parameters that the (not yet implemented) entropy/DWT stages
/// will need.
pub fn parse_codestream(src: &[u8]) -> Result<Codestream, String> {
  if src.len() < 2 || u16::from_be_bytes([src[0], src[1]]) != marker::SOC {
    return Err("TicoRAW: missing SOC marker (not a JPEG-XS codestream)".to_string());
  }
  let mut cs = Codestream::default();
  let mut pos = 2usize;
  while pos + 2 <= src.len() {
    let m = u16::from_be_bytes([src[pos], src[pos + 1]]);
    if m == marker::EOC {
      break;
    }
    // SLH begins the slice/entropy data; record where and stop header scan.
    if m == marker::SLH {
      cs.body_offset = Some(pos);
      break;
    }
    if src[pos] != 0xFF {
      return Err(format!("TicoRAW: expected marker at offset {pos}, found 0x{:02x}", src[pos]));
    }
    if pos + 4 > src.len() {
      return Err(format!("TicoRAW: truncated marker segment at offset {pos}"));
    }
    let len = u16::from_be_bytes([src[pos + 2], src[pos + 3]]) as usize;
    if len < 2 || pos + 2 + len > src.len() {
      return Err(format!("TicoRAW: bad segment length {len} at offset {pos}"));
    }
    let payload = &src[pos + 4..pos + 2 + len];
    match m {
      marker::PIH => cs.pih = PictureHeader::parse(payload)?,
      marker::CAP => cs.capabilities = cap_string(payload),
      _ => {} // CDT/WGT/NLT/CWD/CTS/... skipped for now
    }
    pos += 2 + len;
  }
  Ok(cs)
}

/// Decode a Nikon HE / HE\* (JPEG-XS / TicoRAW) codestream into a 16-bit CFA image.
///
/// # Prototype
///
/// Parses and validates the codestream header, then returns an error because the
/// wavelet decode path is not implemented yet. The parsed header is logged so the
/// framing can be validated against real files.
pub fn decode_ticoraw(src: &[u8], width: usize, height: usize, bps: usize, _dummy: bool) -> Result<PixU16, String> {
  let cs = parse_codestream(src)?;
  log::info!(
    "Nikon HE/TicoRAW codestream: {}x{}, {} components, profile=0x{:04x} level=0x{:04x}, Lcod={}, cap={:?}",
    cs.pih.width,
    cs.pih.height,
    cs.pih.components,
    cs.pih.profile,
    cs.pih.level,
    cs.pih.codestream_len,
    cs.capabilities
  );

  // Sanity-check the header against the TIFF-reported geometry.
  if cs.pih.width as usize != width || cs.pih.height as usize != height {
    log::warn!(
      "Nikon HE: PIH dims {}x{} disagree with TIFF {}x{}",
      cs.pih.width,
      cs.pih.height,
      width,
      height
    );
  }

  Err(format!(
    "Nikon HE (JPEG-XS/TicoRAW) decode not yet implemented. \
     Parsed codestream OK: {}x{} bps={} components={} (cap={:?}). \
     Remaining stages: slice/precinct entropy decode, dequantization, inverse 5/3 DWT, Bayer reconstruction.",
    cs.pih.width, cs.pih.height, bps, cs.pih.components, cs.capabilities
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The exact header bytes captured from a real HE\* sample (DSC_0566.NEF),
  /// SOC + CAP + PIH.
  const SAMPLE_HEADER: &[u8] = &[
    0xff, 0x10, // SOC
    0xff, 0x50, 0x00, 0x22, // CAP, len=34
    0x43, 0x4f, 0x4e, 0x54, 0x41, 0x43, 0x54, 0x5f, 0x49, 0x4e, 0x54, 0x4f, 0x50, 0x49, 0x58, 0x5f, // "CONTACT_INTOPIX_"
    0xef, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // CAP padding
    0xff, 0x12, 0x00, 0x27, // PIH, len=39
    0x00, 0xc7, 0x18, 0x00, // Lcod = 13047808
    0x00, 0x00, // Ppih
    0x00, 0x00, // Plev
    0x15, 0xe0, // Wf = 5600
    0x0e, 0x90, // Hf = 3728
    0x00, 0x00, // Cw
    0x00, 0x10, // Hsl = 16
    0x04, // Nc = 4
    0x04, // Ng
    0x08, // Ss
    // remaining PIH bytes (not asserted here)
    0x12, 0x44, 0x10, 0x51, 0x14, 0x50, 0x88, 0x70, 0x83, 0xf0, 0x15, 0x23, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
  ];

  #[test]
  fn parses_intopix_jpegxs_header() {
    let cs = parse_codestream(SAMPLE_HEADER).expect("header should parse");
    assert_eq!(cs.pih.width, 5600);
    assert_eq!(cs.pih.height, 3728);
    assert_eq!(cs.pih.components, 4);
    assert_eq!(cs.pih.codestream_len, 13_047_808);
    assert_eq!(cs.pih.slice_height, 16);
    assert!(cs.capabilities.as_deref().unwrap_or("").contains("INTOPIX"));
  }

  #[test]
  fn rejects_non_jpegxs() {
    assert!(parse_codestream(&[0x00, 0x01, 0x02, 0x03]).is_err());
  }
}
