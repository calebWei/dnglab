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
//! # Status
//!
//! Full decode is implemented: JPEG-XS framing + picture header parse, the
//! per-precinct entropy chain (GTLI / GCLI / coefficient / sign / dequantize),
//! cross-band prediction, precinct scatter, the horizontal and vertical inverse
//! 5/3 DWTs, tile assembly and Bayer reconstruction (tone-curve LUT). Ported
//! module-by-module from the clean-room decoder in yogthos/LibRaw (branch
//! `nikon-he-decoder`), each stage cross-checked bit-exactly against it; the
//! full-image output is byte-identical to the reference on real HE and HE\*
//! files. See `NIKON_HE_PROJECT.md` in the repository root.

mod bayer;
mod bit_reader;
mod coefficient_decode;
mod decode;
mod dequantize;
mod gcli_decode;
mod gtli_table;
mod idwt_horizontal;
mod idwt_vertical;
mod iqx_iqp_lut_data;
mod picture_header;
mod precinct_decode;
mod precinct_header;
mod predecessor;
mod predict_lut;
mod subband_config;
mod tile;

use crate::pixarray::PixU16;
use decode::decode_nikon_he_image;
use picture_header::{is_supported_picture_header, parse_picture_header};

/// Decode a Nikon HE / HE\* (JPEG-XS / TicoRAW) codestream into a 16-bit CFA image.
///
/// `src` is the whole raw strip (starting at the JPEG-XS `SOC` marker).
///
/// Parses and validates the picture header, then runs the full 3-pass decode
/// ([`decode_nikon_he_image`]) into a 16-bit CFA image. In `dummy` mode only the
/// dimensions are returned (a zeroed image), skipping the pixel decode.
pub fn decode_ticoraw(src: &[u8], width: usize, height: usize, bps: usize, dummy: bool) -> Result<PixU16, String> {
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

  // In dummy mode the caller only wants dimensions, not pixels.
  if dummy {
    return Ok(PixU16::new(width, height));
  }

  if ph.precinct_offset >= src.len() {
    return Err(format!("TicoRAW: precinct_offset {} beyond buffer {}", ph.precinct_offset, src.len()));
  }
  let stream = &src[ph.precinct_offset..strip_size.max(ph.precinct_offset)];

  let mut bayer = vec![0u16; width * height];
  let res = decode_nikon_he_image(stream, width, height, &ph, &mut bayer)?;
  log::info!(
    "Nikon HE/TicoRAW: decoded {} tiles, {} precincts (bps={})",
    res.tiles_decoded,
    res.total_precincts,
    bps
  );

  Ok(PixU16::new_with(bayer, width, height))
}
