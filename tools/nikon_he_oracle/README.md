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

## Per-stage capture (oracle cross-check fixtures)

Entropy/DWT/tile/bayer Rust modules are validated **bit-exactly** against the
reference by capturing its exact stage I/O on a real file, then asserting the
Rust port reproduces it. Pattern (used for `gcli_decode`):

1. In the reference clone, add an **env-gated** `fprintf` dump to the relevant
   stage (e.g. `nikon_he_precinct_decode.cpp`), guarded by
   `getenv("NIKON_HE_..._DUMP") && pred_state.precinct_index() == 0` so it fires
   for the first precinct only. Dump the stage's inputs (raw substream bytes,
   params) **and** outputs as hex. Capture whole line-block buffers, not
   per-sub-band slices — the sig/gcli readers persist mid-byte across a LB's
   sub-bands, so a faithful fixture must replay all of a LB's sub-bands against
   one reader pair.
2. Rebuild the oracle, run it with the env var set, redirect stderr to a file.
   (Note: `precinct_index()==0` fires once per tile, so the dump repeats ~59×;
   take the first block.)
3. Distill the first block into a small fixture under
   `rawler/src/decompressors/ticoraw/testdata/` and add a `#[test]` that replays
   it through the Rust module (see `gcli_decode.rs::oracle_lb0_dsc8070_bit_exact`).
4. **Revert the temporary capture** from the reference clone (`git checkout --`),
   leaving `dx_sig_fix` intact (it lives in `nikon_he_precinct_header.cpp`).

Extract the HE strip first with `analysis/extract_strip.py` (writes
`_ref_libraw_he/oracle/strip_8070.bin`).

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
