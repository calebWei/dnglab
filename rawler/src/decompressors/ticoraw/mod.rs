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
mod gcli_decode;
mod gtli_table;
mod iqx_iqp_lut_data;
mod picture_header;
mod predict_lut;
mod subband_config;

use crate::pixarray::PixU16;
use picture_header::{is_supported_picture_header, parse_picture_header};

/// Decode a Nikon HE / HE\* (JPEG-XS / TicoRAW) codestream into a 16-bit CFA image.
///
/// `src` is the whole raw strip (starting at the JPEG-XS `SOC` marker).
///
/// # Prototype
///
/// Parses and validates the picture header (markers + WGT weights), then returns
/// an error because the wavelet decode path is not implemented yet. The parsed
/// header is logged so the framing can be validated against real files.
pub fn decode_ticoraw(src: &[u8], width: usize, height: usize, bps: usize, _dummy: bool) -> Result<PixU16, String> {
  let ph = parse_picture_header(src).ok_or_else(|| "TicoRAW: failed to parse JPEG-XS picture header".to_string())?;

  // `src` may be zero-padded by the caller (subview_padded), so validate the
  // profile against the header-declared codestream length (Lcod), and require
  // the buffer to actually hold that many bytes.
  let strip_size = ph.lcod as usize;
  if src.len() < strip_size {
    return Err(format!("TicoRAW: buffer {} shorter than declared Lcod {}", src.len(), strip_size));
  }
  let supported = is_supported_picture_header(&ph, strip_size);
  log::info!(
    "Nikon HE/TicoRAW: {}x{}, {} comps, nbands={}, Hsl={}, Bw={}, Lcod={}, precinct_offset={}, supported={}",
    ph.hdr_width,
    ph.hdr_height,
    ph.comps_num,
    ph.nbands,
    ph.hsl,
    ph.bw,
    ph.lcod,
    ph.precinct_offset,
    supported
  );

  if ph.hdr_width as usize != width || ph.hdr_height as usize != height {
    log::warn!("Nikon HE: PIH dims {}x{} disagree with TIFF {}x{}", ph.hdr_width, ph.hdr_height, width, height);
  }

  Err(format!(
    "Nikon HE (JPEG-XS/TicoRAW) decode not yet implemented. \
     Parsed picture header OK: {}x{} bps={} comps={} nbands={} precinct_offset={} supported={}. \
     Remaining stages: precinct/GCLI/coefficient entropy decode, dequantization, inverse 5/3 DWT, Bayer reconstruction.",
    ph.hdr_width, ph.hdr_height, bps, ph.comps_num, ph.nbands, ph.precinct_offset, supported
  ))
}
