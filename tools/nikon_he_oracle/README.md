# Nikon HE reference "oracle" harness

Builds the **reference** Nikon HE C++ decoder (yogthos/LibRaw, branch
`nikon-he-decoder`) into a standalone tool that decodes a raw NEF strip to a
16-bit PGM. Purpose: validate the Rust port in `rawler/src/decompressors/ticoraw`
against a ground truth, and — with added tracing — dump per-stage intermediates
to localize porting bugs. See `../../NIKON_HE_PROJECT.md`.

The reference C++ is **not vendored** here (LGPL clean-room; we port it to Rust
rather than ship it). Point the build at your local clone.

## Build (Windows, MSVC)

```sh
cmake="/c/Program Files/CMake/bin/cmake.exe"
cd tools/nikon_he_oracle
"$cmake" -S . -B build -G "Visual Studio 17 2022" -A x64 \
  -DNIKON_HE_REF_DIR="D:/_repos/_ref_libraw_he"
"$cmake" --build build --config Release
# -> build/Release/nikon_he_oracle.exe
```

## Extract a strip and run

The NEF raw strip is `StripOffset .. +StripByteCounts` of the CFA SubIFD
(Photometric 32803, Compression 34713). Extract with the TIFF walker in
`NIKON_HE_PROJECT.md` (or reuse the scratch `extract_strip.py`). Then:

```sh
./build/Release/nikon_he_oracle.exe strip_8070.bin bayer_8070.pgm
# compare to the Adobe DNG ground truth:
magick compare -metric PSNR bayer_8070.pgm ../../target/gt/DSC_8070.pgm null:
```

## STATUS (2026-09-20) — DX working

- Header parse matches the Rust port (precinct_offset=155).
- **DX FIX REQUIRED — apply `dx_sig_fix.patch` before building.** The stock
  reference (FX-only) crashes on the Nikon Z50 II (DX, 5600×3728) because its
  per-LB significance substream size is wrong (`sig=f20`). The patch computes the
  correct per-LB `sig = ceil(Σ_sb ceil(ng_sb/8) / 8)` = `[12,12,11,12,11,11,11,11]`.
  Apply it in the reference clone:
  ```sh
  cd D:/_repos/_ref_libraw_he
  git apply D:/_repos/dnglab/tools/nikon_he_oracle/dx_sig_fix.patch
  ```
- With the patch, the oracle decodes **HE and both HE\*** samples and matches the
  Adobe DNG ground truth to RMSE ≈ 0.57 (PSNR 97–103 dB; ~99.7% of pixels within
  ±1 LSB). See `../../NIKON_HE_PROJECT.md` §8/§12.
