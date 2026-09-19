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

## Repro of the analysis

Sample layout was extracted by walking the TIFF directly (SubIFD strip offset +
first bytes) — see the session notes. `dnglab analyze --structure <file>` dumps
the root IFD chain as JSON.
