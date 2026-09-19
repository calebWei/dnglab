# Nikon HE / HE\* decoder — project plan & living status

> **Master document.** This is the source of truth for the Nikon High-Efficiency
> raw decoder effort. It is written to survive across multiple Claude Code
> sessions: a fresh session should be able to read this top-to-bottom and resume
> with zero prior context. Deep codec architecture lives in
> [`NIKON_HE_ROADMAP.md`](./NIKON_HE_ROADMAP.md); this file tracks **goal, plan,
> state, and how to continue**.
>
> **When you make progress, update this file** — tick the checklist, append to the
> Session Log, and move the "Next action" pointer.

---

## 1. Goal

Add a working decoder for Nikon **High Efficiency (HE)** and **High Efficiency\*
(HE\*)** compressed NEF raws to `rawler` (the raw library inside this dnglab
fork), so these files decode to real sensor data instead of being rejected.

- **Primary milestone (achievable):** bit-exact decode of **HE** (compression
  code 13), validated against Adobe DNG Converter output.
- **Stretch milestone (ambitious / not fully solved in open source):** decode
  **HE\*** (compression code 14) **"as far as possible"** — the agreed success
  bar. HE\* is the lossier variant; the public reference decoder does *not* yet
  solve it cleanly. We push until diminishing returns and document exactly how
  close we get and what remains.

Downstream motivation: RapidRAW consumes a `rawler` fork; today it falls back to
a darkened embedded JPEG preview for HE/HE\* files. A real decoder fixes that.

## 2. Why this is hard (the key finding)

Nikon HE/HE\* = intoPIX **TicoRAW**, which is a concrete instance of the
**JPEG-XS** codestream syntax (ISO/IEC 21122). Verified from real sample bytes:
the raw strip starts with the JPEG-XS `SOC` marker `FF10`, carries an intoPIX
`CAP` string, and its picture header (PIH) cross-checks against the TIFF
(Lcod == strip byte count, Wf/Hf == raw dims, Nc == 4 for RGGB). So decoding
means implementing a JPEG-XS-family wavelet codec: precinct/entropy decode →
dequantization → inverse 5/3 DWT → Bayer reconstruction.

Full evidence and the picture-header field breakdown are in `NIKON_HE_ROADMAP.md`.

## 3. Strategy

Port the clean-room reference decoder (C++) to Rust module-by-module, then
validate end-to-end against Adobe DNG ground truth using a tight decode→diff loop.

- **Reference:** yogthos/LibRaw, dir `src/decoders/nikon_he/`. Cloned locally at
  **`D:\_repos\_ref_libraw_he`**. License: LGPL-2.1 / CDDL (compatible with
  rawler's LGPL-2.1; clean-room, no Nikon SDK). Do **not** commit reference code
  into this repo; port/rewrite in Rust.
  - **Use branch `nikon-he-decoder` (checked out locally), NOT `nikon-he-production`.**
    Production is missing `nikon_he_picture_header.{h,cpp}` — the general
    **WGT-derived GTLI path** (`gtli_from_weights`) and HE/HE\* header parsing.
    The `nikon-he-decoder` branch has the complete 33-file set; foundational
    files (bit_reader, subband_config) are byte-identical between the two.
  - GTLI has two paths: general (compute from picture-header WGT weights) and a
    hardcoded fallback table (`kGtliTable`, captured combos incl. HE\* rows). The
    general path is required for robustness — the fallback only covers sampled
    `(Bp,Br)` combos.
- **Ground truth:** Adobe DNG Converter output (mosaic CFA DNG), decoded by
  dnglab into a 16-bit PGM and diffed against our decoder output.
- **Order:** foundational data/bit-IO first, integration last (see §7 checklist).

## 4. Environment & repo setup (already done)

- **Toolchain:** Rust 1.98.1 MSVC (`rustup`, via winget). VS 2022 C++ provides the
  linker. `~/.cargo/bin` may not be on the bash PATH — prepend it:
  `export PATH="$HOME/.cargo/bin:$PATH"` (bash) or
  `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"` (PowerShell).
- **Repo:** `D:\_repos\dnglab` (this is upstream `dnglab/dnglab` v0.8.0).
- **Working branch:** `feat/nikon-he-support` (local; **not pushed**).
- **Remotes:**
  - `origin` → `github.com/calebWei/dnglab.git` (the user's fork) — push here, **only when asked**.
  - `upstream` → `github.com/dnglab/dnglab.git` — **fetch only** (push URL disabled).
- **Reference clone:** `D:\_repos\_ref_libraw_he` (not a submodule; local only).
- **Samples:** `D:\_repos\dnglab\Example Photos\` (NEF + Adobe DNG pairs).

⚠️ **Windows gotcha:** PowerShell `>` corrupts binary output (UTF-8 re-encode +
BOM). Always capture binary pixel dumps via **bash** redirection.

### Installed tooling (absolute paths — winget PATH updates don't reach this session's shell)

A fresh terminal will have these on PATH; the agent shell here must call them by
absolute path.

| Tool | Path | Use |
|------|------|-----|
| Rust/cargo | `~/.cargo/bin` | build/test (prepend to PATH each shell) |
| MSVC `cl.exe` | VS2022 (14.44) — via a "x64 Native Tools" prompt or vcvars64.bat | build the C++ reference oracle |
| CMake | `C:\Program Files\CMake\bin\cmake.exe` | build the oracle harness |
| Ninja | `C:\Users\caleb\AppData\Local\Microsoft\WinGet\Packages\Ninja-build.Ninja_Microsoft.Winget.Source_8wekyb3d8bbwe\ninja.exe` | oracle build generator |
| ImageMagick | `C:\Program Files\ImageMagick-7.1.2-Q16-HDRI\magick.exe` | `magick compare -metric PSNR/RMSE/AE a.pgm b.pgm null:` + diff maps |
| exiftool | `C:\Users\caleb\AppData\Local\Programs\ExifTool\ExifTool.exe` | NEF/DNG makernote (`-NEFCompression`, `-LinearizationTable`, dims) |
| numpy (py 3.12) | on PATH | scripted per-pixel PGM diff (max abs err, %% mismatched) |

Confirmed via exiftool: `DSC_8070` = **High Efficiency**, `DSC_0566/0567` =
**High Efficiency\***. NEFs carry **no** `LinearizationTable` → HE uses the
reference's fixed built-in tone curve (`iqx_iqp_lut`), not a per-file table.

### Reference "oracle" harness (Tier-1 validation lever) — TODO, not yet built

Plan: compile the reference `nikon_he/*.cpp` (branch `nikon-he-decoder`) into a
tiny standalone exe with `cl.exe`/CMake that (a) proves the reference decodes
OUR HE sample vs Adobe, and (b) **dumps per-stage intermediate buffers**
(coefficients after entropy → dequant → horizontal IDWT → vertical IDWT → Bayer).
Then diff each Rust stage against the matching oracle dump to localize bugs to a
single module instead of only seeing a wrong final image. Build this before/while
porting the entropy+IDWT stages — it is the difference between per-stage green
checkpoints and end-to-end guesswork. (Falls back to raw `cl.exe` if CMake is
inconvenient.)

## 5. Build & validate (commands)

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cd /d/_repos/dnglab

# Build
cargo build -p rawler          # library only (~2 min cold)
cargo build -p dnglab          # CLI (needed for analyze/ground-truth)
cargo test  -p rawler ticoraw  # header-parser unit tests

# Ground truth: dump Adobe DNG CFA plane to PGM (P5, 5600x3728, 16-bit BE)
exe=./target/debug/dnglab.exe
mkdir -p target/gt
"$exe" analyze --raw-pixel "Example Photos/DSC_8070.dng" > target/gt/DSC_8070.pgm

# Our decode path (currently errors with WIP message after parsing the codestream)
"$exe" analyze --meta "Example Photos/DSC_8070.NEF"

# Inspect NEF compression mode (debug logging)
RUST_LOG=debug "$exe" analyze --meta "Example Photos/DSC_8070.NEF" 2>&1 | grep -i "compression mode"
```

Validation loop (to build): decode NEF via our path → dump CFA → compare to
`target/gt/<file>.pgm` reporting **max abs error** and **% mismatched pixels**.
A small `bin/` example or a Rust integration test can own this comparison.

## 6. Test matrix

| File (in `Example Photos/`) | Compression    | Raw dims  | Role                              |
|-----------------------------|----------------|-----------|-----------------------------------|
| `DSC_8070.NEF` + `.dng`     | HE  (code 13)  | 5600×3728 | **checkpoint** — must reach 0 err |
| `DSC_0566.NEF` + `.dng`     | HE\* (code 14) | 5600×3728 | HE\* target                       |
| `DSC_0567.NEF` + `.dng`     | HE\* (code 14) | 5600×3728 | HE\* target                       |

All are 14-bit, CFA RGGB, Adobe WhiteLevel 15892.

## 6a. Module porting workflow (MANDATORY discipline)

Work **one module at a time**. Do not start the next module until the current one
is **fully validated**. For each module:

1. **Read** the reference `.h` + `.cpp` under
   `D:\_repos\_ref_libraw_he\src\decoders\nikon_he\` (branch `nikon-he-decoder`).
2. **Port** it faithfully to `rawler/src/decompressors/ticoraw/<module>.rs`.
   Preserve integer widths, signedness, endianness, rounding. Note any
   Rust-idiomatic deviation in a code comment (e.g. return-by-value vs C++ static).
3. **Write tests in the same file** (`#[cfg(test)] mod tests`). Every module gets
   tests — no exceptions. Prefer:
   - spot-checks of the reference's documented behavior / formulas;
   - values cross-checked against the reference (hand-computed or oracle-dumped);
   - round-trip / invariants where applicable.
4. **Validate fully** before moving on:
   - `cargo test -p rawler ticoraw` is green (all tests, not just the new one);
   - `cargo clippy -p rawler` has no new warnings on the module;
   - once the oracle harness exists: **diff this stage's output against the oracle
     dump** and confirm it matches (bit-exact for HE) before proceeding.
5. **Commit** the single module (`+ its tests`), tick the §7 checklist, update the
   §9 "Next action" pointer and §10 session log. **Push at milestones.**

Rationale: a wavelet/entropy pipeline fails silently — a wrong final image gives
no hint which of 16 stages is at fault. Per-module validation (ideally against the
oracle) keeps every step green so bugs are caught where they're introduced.

## 7. Task checklist (port order)

Foundational → integration. Tick as completed; keep the "Next action" pointer (§9) in sync.
**One module at a time, fully validated (see §6a) before the next.**

**Setup & framing**
- [x] Install toolchain, build rawler + dnglab
- [x] Identify format as JPEG-XS/TicoRAW; verify against sample bytes
- [x] Branch + scaffold `decompressors/ticoraw` (marker/PIH parser + tests)
- [x] Route HE/HE\* in `nef.rs` into `decode_ticoraw` (before the 34713 branch)
- [x] Validate ground-truth harness (Adobe DNG → PGM, 5600×3728)
- [x] Clone reference decoder locally; document architecture
- [ ] Locate precinct-stream body offset + linearization LUT + black/white levels from the NEF makernote

**Codec port (`decompressors/ticoraw/…`)**
- [x] `bit_reader` (MSB bit pump) — ported + unit tested
- [x] `predict_lut` (GCLI prediction LUT) — ported + tested
- [x] `picture_header` (markers, WGT weights, `gtli_from_weights`, `is_supported`) —
      ported + tested; **validated on real files** (HE 8070 + HE\* 0566/0567 all parse:
      5600×3728, comps=4, nbands=25, precinct_offset=155=0x9B, supported=true).
      Supersedes the old scaffold parser in `mod.rs`.
- [ ] `gtli_table`, `iqx_iqp_lut_data` (pure data/LUTs)
- [x] `subband_config` (`compute_subband_layout`) — ported + tested; still need `compute_buf_stripe_ints`, `compute_kband` (defined elsewhere in ref — locate)
- [ ] `gcli_decode`, `coefficient_decode`, `dequantize`
- [ ] `precinct_header`, `precinct_decode`, `predecessor`
- [ ] `idwt_horizontal`, `idwt_vertical`
- [ ] `tile` (`decode_tile`)
- [ ] `bayer` (`step1_merge_4_to_2`, `step2_bayer_rows`)
- [ ] top-level `decode_nikon_he_image` (3-pass driver) → hook into `decode_ticoraw`

**Validation**
- [ ] Build the reference **oracle harness** (C++ per-stage intermediate dumps) — §4
- [ ] Build decode→PGM→diff harness (dnglab our-path → PGM; `magick compare` / numpy vs `target/gt/*.pgm`)
- [ ] HE (DSC_8070) decodes; iterate to **0 error** ← primary milestone
- [ ] HE\* (0566/0567): decode, measure error, push "as far as possible"
- [ ] Wire result into `RawImage` (levels, photometric, crop) so `dnglab convert` works

## 8. Risks / open questions

- **HE\* is not fully solved upstream.** The reference reverted HE\* for
  artifacts; a follow-up PR targets HE/HE\* header parsing. Expect HE\* to need
  extra reverse engineering beyond a straight port. Primary success = HE.
- **HE\* ground-truth fidelity:** we assume Adobe decodes HE\* correctly. If
  Adobe itself is approximate, "error vs Adobe" is only a proxy.
- **Endianness / signedness / integer-width** bugs are the usual porting hazards
  in the entropy + DWT stages — unit-test small pieces where possible.
- **Precinct-stream framing** (24-bit size prefix, +12 byte prefix, 6-byte pad
  every 16th, 2-precinct tile overlap) is subtle — verify against real bytes.

## 9. Current state & next action

- **Done:** environment + helper tools (cmake/ninja/imagemagick/exiftool), branch,
  ground-truth harness, reference cloned (`nikon-he-decoder` branch), plan.
- **Ported + tested (16 ticoraw tests green):** `bit_reader`, `subband_config`,
  `predict_lut`, `picture_header`. Header/framing **verified on real HE + HE\***
  files (5600×3728, comps=4, nbands=25, precinct_offset=155=0x9B, supported=true).
- **`decode_ticoraw` today:** parses the full picture header (markers + WGT),
  logs it, returns a WIP error. No pixels yet.
- **Pushed to fork through commit `c79c39b6`.** (Doc/tooling commit after that.)
- **▶ NEXT ACTION:** (per §6a — one module, fully validated) port `gtli_table`
  (uses the `gtli_from_weights` general path now available via `picture_header`,
  plus the hardcoded fallback rows). Then `iqx_iqp_lut_data` (tone-curve LUT), then
  the entropy stages (`gcli_decode`, `coefficient_decode`, `dequantize`). Consider
  building the oracle harness (§4) before the entropy stages so each can be diffed.
  `compute_buf_stripe_ints` / `compute_kband` are referenced by `decode.cpp` but
  not in `subband_config.cpp` — find their definitions (likely `tile`/`precinct`)
  while porting those. Reference: `D:\_repos\_ref_libraw_he\src\decoders\nikon_he\`
  (branch `nikon-he-decoder`); target: `rawler/src/decompressors/ticoraw/`.

## 10. Session log

| Date       | Session | Summary |
|------------|---------|---------|
| 2026-09-19 | 1       | Diagnosed RapidRAW's dark HE\* preview fallback; identified format as JPEG-XS/TicoRAW; installed toolchain; created branch + scaffold (marker/PIH parser, tests); routed HE/HE\* into new path; built ground-truth harness; cloned reference decoder; set up fork/upstream remotes; wrote this plan. No codec code yet. |
| 2026-09-20 | 2       | Pushed branch to fork (`origin`=calebWei/dnglab). Ported 4 modules, each unit-tested (16 ticoraw tests green): `bit_reader`, `subband_config`, `predict_lut`, `picture_header`. Switched reference base to `nikon-he-decoder` branch (has `picture_header`/`gtli_from_weights`; production was missing it). Unified `decode_ticoraw` on the new parser; **verified header/framing on real HE + HE\*** (precinct_offset=155=0x9B, supported=true). Installed helper tools (cmake, ninja, imagemagick, exiftool); confirmed HE vs HE\* via exiftool. Documented §6a per-module workflow + oracle-harness plan. Commits through `c79c39b6` pushed; doc/tooling commit follows. |

<!-- Append a new row per session. Keep §9 "Next action" current. -->
