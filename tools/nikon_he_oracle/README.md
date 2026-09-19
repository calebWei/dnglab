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

## KNOWN STATUS (2026-09-20)

- Header parse works on our files (matches the Rust port; precinct_offset=155).
- **The reference SEGFAULTS on the Nikon Z50 II (DX, 5600×3728) samples** inside
  `decode_precinct` on the first precinct. The reference was validated only on FX
  bodies (Z9/Z8/Z6III/Zf/Z5II). The code is width-parameterized (no hardcoded FX
  dims), so this is a **bug/gap for DX geometry**, not a fundamental limitation —
  but it must be root-caused (precinct-header byte-offset derivation is the prime
  suspect: `nikon_he_precinct_header.cpp`, `compute_f20`, the 20-bit
  `lb_gcli_bytes` field). An FX HE sample would let us confirm the reference works
  there and bisect the DX difference.
