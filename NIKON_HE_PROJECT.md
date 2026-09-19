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

- **Reference:** yogthos/LibRaw, branch `nikon-he-production`, dir
  `src/decoders/nikon_he/` (16 modules, ~90 KB C++). Cloned locally at
  **`D:\_repos\_ref_libraw_he`**. License: LGPL-2.1 / CDDL (compatible with
  rawler's LGPL-2.1; clean-room, no Nikon SDK). Do **not** commit reference code
  into this repo; port/rewrite in Rust.
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

## 7. Task checklist (port order)

Foundational → integration. Tick as completed; keep the "Next action" pointer (§9) in sync.

**Setup & framing**
- [x] Install toolchain, build rawler + dnglab
- [x] Identify format as JPEG-XS/TicoRAW; verify against sample bytes
- [x] Branch + scaffold `decompressors/ticoraw` (marker/PIH parser + tests)
- [x] Route HE/HE\* in `nef.rs` into `decode_ticoraw` (before the 34713 branch)
- [x] Validate ground-truth harness (Adobe DNG → PGM, 5600×3728)
- [x] Clone reference decoder locally; document architecture
- [ ] Locate precinct-stream body offset + linearization LUT + black/white levels from the NEF makernote

**Codec port (`decompressors/ticoraw/…`)**
- [ ] `bit_reader` (MSB bit pump)
- [ ] `gtli_table`, `iqx_iqp_lut_data`, `predict_lut` (pure data/LUTs)
- [ ] `subband_config` (layout math: `compute_subband_layout`, `compute_buf_stripe_ints`, `compute_kband`)
- [ ] `gcli_decode`, `coefficient_decode`, `dequantize`
- [ ] `precinct_header`, `precinct_decode`, `predecessor`
- [ ] `idwt_horizontal`, `idwt_vertical`
- [ ] `tile` (`decode_tile`)
- [ ] `bayer` (`step1_merge_4_to_2`, `step2_bayer_rows`)
- [ ] top-level `decode_nikon_he_image` (3-pass driver) → hook into `decode_ticoraw`

**Validation**
- [ ] Build decode→PGM→diff harness
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

- **Done:** environment, branch, JPEG-XS framing parser (tests green), decode
  routing, validated ground-truth harness, reference cloned, plan documented.
- **Commits on `feat/nikon-he-support`:** scaffold + roadmap + (this) plan.
- **`decode_ticoraw` today:** parses SOC/CAP/PIH, logs geometry, returns a WIP
  error. No pixels yet.
- **▶ NEXT ACTION:** begin the codec port at `bit_reader` (§7), then LUT tables
  and `subband_config`. Read the matching files under
  `D:\_repos\_ref_libraw_he\src\decoders\nikon_he\` and translate faithfully to
  Rust in `rawler/src/decompressors/ticoraw/`.

## 10. Session log

| Date       | Session | Summary |
|------------|---------|---------|
| 2026-09-19 | 1       | Diagnosed RapidRAW's dark HE\* preview fallback; identified format as JPEG-XS/TicoRAW; installed toolchain; created branch + scaffold (marker/PIH parser, tests); routed HE/HE\* into new path; built ground-truth harness; cloned reference decoder; set up fork/upstream remotes; wrote this plan. No codec code yet. |

<!-- Append a new row per session. Keep §9 "Next action" current. -->
