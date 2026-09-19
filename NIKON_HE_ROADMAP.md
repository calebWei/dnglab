# Nikon HE / HE\* (TicoRAW / JPEG-XS) decode — prototype roadmap

Branch: `feat/nikon-he-support`

## What this is

Nikon "High Efficiency" (HE) and "High Efficiency\*" (HE\*) raw compression —
used by EXPEED 7 bodies (Z 9, Z 8, Z 6III, Z f, Z 5II, …) — is intoPIX
**TicoRAW**, which is a concrete instance of the **JPEG-XS** codestream syntax
(ISO/IEC 21122-1). Previously `nef.rs` rejected these outright:

```rust
if matches!(nef_compression, Some(HighEfficency | HighEfficencyStar)) {
    return Err(... "not supported" ...);
}
```

## Evidence (from the two sample HE\* files)

TIFF raw SubIFD: `5600 × 3728`, 14-bit, CFA RGGB, TIFF compression `34713`,
`NefCompression = 14` (HighEfficencyStar). The strip is a JPEG-XS codestream:

```
FF10                                   SOC
FF50 0022 "CONTACT_INTOPIX_..."        CAP  (intoPIX capabilities)
FF12 0027 <picture header>             PIH
  Lcod = 0x00C71800 = 13047808  (== StripByteCounts)
  Wf   = 0x15E0     = 5600      (== raw width)
  Hf   = 0x0E90     = 3728      (== raw height)
  Nc   = 0x04       = 4         (Bayer RGGB → 4 components)
  Hsl  = 0x0010     = 16
...
FF11                                   EOC
```

All PIH fields cross-check against the TIFF geometry, so the container→codestream
mapping is verified.

## What's implemented so far

- `rawler/src/decompressors/ticoraw/mod.rs`
  - JPEG-XS marker walk (SOC/CAP/PIH/…/SLH/EOC).
  - `PictureHeader::parse` (Lcod, Wf, Hf, Nc, Hsl, profile, level, …).
  - `decode_ticoraw()` entry point — parses + validates the header, logs it,
    then returns a WIP error. Unit tests assert the parse against real
    HE\* header bytes.
- `nef.rs` routes HE/HE\* into `decode_ticoraw` (must precede the `34713`
  Huffman branch, since HE also reports TIFF compression 34713).

Status: `dnglab analyze --meta <HE.NEF>` now reaches the new decoder and reports
the parsed codestream instead of "not supported".

## Remaining work (the actual codec)

Port the clean-room JPEG-XS/HE decoder. Reference: **yogthos/LibRaw PR #826**
(`src/decoders/nikon_he/`), a clean-room 2D 5/3-wavelet decoder that matches
Adobe DNG Converter output for HE.

1. **CDT / WGT / CWD parsing** — component table, quantization weights, wavelet
   decomposition config (levels per component).
2. **Slice / precinct headers (SLH)** — per-slice band layout.
3. **Entropy decode** — GCLI (greatest coded line index) + coefficient bitplanes
   + sign coding, per code group (Ng=4).
4. **Dequantization** — per-subband, using the weights table.
5. **Inverse 5/3 DWT** — per component, per decomposition level, with per-line
   buffer/carry state.
6. **Bayer reconstruction** — interleave the 4 components back to a CFA plane;
   apply black/white levels (14-bit, max 16383).

### HE vs HE\* caveat

The samples here are **HE\*** (the lossy-er variant). In the reference decoder,
plain HE decodes cleanly but **HE\* was reverted due to substantial artifacts** —
i.e. HE\* is not fully solved in open source yet. Expect HE to be the realistic
first milestone; HE\* likely needs extra header handling (the y-g-jiang follow-up
PR to yogthos/LibRaw targets HE/HE\* header parsing specifically).

Recommended order: implement + validate **HE** first (find/borrow an HE sample),
then tackle HE\*.

## Validating output

`dnglab analyze --raw-checksum <file>` / `--full-pixel` can dump decoded pixels;
compare against Adobe DNG Converter output for the same file (the reference
decoder's methodology). No Nikon SDK material should be used.

## Validation harness (confirmed working)

Ground truth = Adobe DNG Converter output (mosaic CFA DNG, `Photometric=32803`,
5600×3728, RGGB, decodable by dnglab). Extract the reference CFA plane as PGM:

```sh
# NOTE: use a binary-safe shell (bash). PowerShell '>' corrupts binary (UTF-8 re-encode).
dnglab.exe analyze --raw-pixel DSC_8070.dng > gt/DSC_8070.pgm   # P5, 5600x3728, 16-bit BE
```

Test matrix (in `Example Photos/`):

| File       | Compression         | Role                          |
|------------|---------------------|-------------------------------|
| DSC_8070   | HE  (code 13)       | known-solvable **checkpoint** |
| DSC_0566   | HE\* (code 14)      | target                        |
| DSC_0567   | HE\* (code 14)      | target                        |

Loop: decode NEF via our path → dump CFA → diff vs `gt/<file>.pgm`
(max abs error, %% mismatched pixels). HE should reach 0; HE\* "as far as possible".

## Reference decoder architecture (yogthos/LibRaw `nikon-he-production`)

Cloned locally at `D:\_repos\_ref_libraw_he` (LGPL-2.1 / CDDL — compatible with
rawler's LGPL-2.1; clean-room, no Nikon SDK). `decode_nikon_he_image()` is a
3-pass pipeline over an image-wide coefficient buffer:

1. `compute_subband_layout(width/2)` → `config[26]`; `build_prediction_lookup_table()`.
2. Walk precinct stream: 24-bit big-endian `total_size_minus_12` prefix, full
   precinct = `sz+12` bytes, 6-byte pad after every 16th; `n_tiles=(H+63)/64`,
   16 precincts/tile with a 2-precinct overlap into the next tile.
3. **Pass 1** `decode_tile` (→ bit_reader, gcli_decode, coefficient_decode,
   dequantize, gtli_table, idwt_horizontal, precinct_header/decode, predecessor).
4. **Pass 2** `step1_merge_4_to_2` (subband merge / part of IDWT).
5. **Pass 3** `step2_bayer_rows` (vertical IDWT + Bayer interleave + linearization LUT).

### Port order (foundational → integration)

1. `nikon_he_bit_reader` (MSB bit pump)
2. `nikon_he_gtli_table`, `nikon_he_iqx_iqp_lut_data`, `nikon_he_predict_lut` (pure data/LUTs)
3. `nikon_he_subband_config` (layout math)
4. `nikon_he_gcli_decode`, `nikon_he_coefficient_decode`, `nikon_he_dequantize`
5. `nikon_he_precinct_header`, `nikon_he_precinct_decode`, `nikon_he_predecessor`
6. `nikon_he_idwt_horizontal`, `nikon_he_idwt_vertical`, `nikon_he_tile`, `nikon_he_bayer`
7. Top-level `decode_nikon_he_image` → hook into `decode_ticoraw`; then wire the
   linearization LUT + black/white levels from the NEF makernote.

The JPEG-XS header parse already implemented feeds `image_width/height` and
locates the precinct-stream body (after the marker segments).

## Repro of the analysis

Sample layout was extracted by walking the TIFF directly (SubIFD strip offset +
first bytes) — see the session notes. `dnglab analyze --structure <file>` dumps
the root IFD chain as JSON.
