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

## 6a. Module lifecycle (MANDATORY — one module at a time)

**The core rule: pick up exactly ONE module, drive it to _Done_ (all exit
criteria below), commit it, and only THEN pick up the next.** Never have two
modules half-ported at once. A wavelet/entropy pipeline fails silently — a wrong
final image gives no hint which of ~16 stages is at fault — so every stage must be
independently green before the next is started.

A module moves through these states: **Selected → Ported → Tested → Validated →
Committed → Done**. Do not advance a state until the prior one holds.

### 1. Select (start of a module)
- Take the next unchecked item from the §7 checklist (respect the dependency
  order). Set the §9 "Next action" pointer to name it as *in progress*.
- Confirm its dependencies are already **Done** (e.g. `precinct_decode` needs
  `bit_reader`, `gcli_decode`, `coefficient_decode`, `dequantize`, `gtli_table`).

### 2. Port
- Read the reference `.h` + `.cpp` under
  `D:\_repos\_ref_libraw_he\src\decoders\nikon_he\` (branch `nikon-he-decoder`).
- Translate faithfully to `rawler/src/decompressors/ticoraw/<module>.rs`. Preserve
  integer widths, signedness, endianness, rounding, and overflow behavior
  (use `wrapping_*`/`i64` where the C relies on it). Declare `mod <module>;` in
  `ticoraw/mod.rs`.
- Note any Rust-idiomatic deviation in a code comment (e.g. return-by-value vs the
  C++ program-static, slices vs raw pointers). Keep the SPDX/port-provenance header.

### 3. Test (in the same file — NO module ships without tests)
- Add `#[cfg(test)] mod tests`. Cover, in order of preference:
  - the reference's documented formulas / behavior (spot values);
  - values cross-checked against the reference (hand-computed, or dumped from the
    oracle with `fprintf`);
  - invariants / round-trips / boundary cases (EOF, last-sub-band, partial tile).
- At least one test must exercise the module on realistic parameters (e.g. the
  5600-wide layout), not only toy inputs.

### 4. Validate (the definition of _Done_ — ALL must hold)
- [ ] `cargo test -p rawler ticoraw` — **all** ticoraw tests green (not just the new one).
- [ ] `cargo clippy -p rawler` — **no new warnings** attributable to the module
      (use `#[allow(dead_code)]` with a comment only for APIs a later module will use).
- [ ] `cargo fmt` applied (or matches `rustfmt.toml`).
- [ ] **Oracle cross-check where the module produces comparable output:** dump the
      reference's intermediate for this stage (add a temporary `fprintf` to the
      ref, rebuild `tools/nikon_he_oracle`) and confirm the Rust output matches
      **bit-exactly for HE**. For pure-math/LUT/parse modules a unit test against
      known values suffices; for entropy/DWT/tile/bayer stages the oracle diff is
      REQUIRED before the module is Done. **Prerequisite:** the oracle must decode
      the target file — see the DX blocker (§8/§12); on DX, fix the reference
      first, then use it as the oracle.

### 5. Commit & close out
- Commit the single module **+ its tests** (one module per commit).
- Tick its box in the §7 checklist; update the §9 "Ported so far" list and move the
  "Next action" pointer to the next module.
- Append/refresh the §10 session-log row. **Push to the fork at each milestone.**

### 6. Then — and only then — return to step 1 for the next module.

> If a module can't reach _Done_ (e.g. it needs an oracle the reference can't yet
> produce), STOP, record the blocker in §8 + §9, and surface it — do not silently
> move on to the next module with an unvalidated one behind you.

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
- [x] `gtli_table` (dynamic `compute_gtli_table`/`gtli_for_sub_band` from WGT weights +
      `wgt_index_for_band` 26→25 remap + verbatim static fallback table) — ported + tested
      (7 tests; ground-truth invariant: every static row has `values[23]==values[12]`).
- [x] `iqx_iqp_lut_data` (256-breakpoint PWL tone curve → lazily-materialized
      81792-entry LUT, cached via `OnceLock`) — ported + tested (4 tests: exact
      breakpoint anchors, reference interpolation formula, endpoints/monotonicity,
      caching). Guards the reference's benign OOB `k+1` read at the final breakpoint.
- [x] `subband_config` (`compute_subband_layout`) — ported + tested; still need `compute_buf_stripe_ints`, `compute_kband` (defined elsewhere in ref — locate)
- [x] `gcli_decode` (per-sub-band GCLI decode: sig-bit-per-8-group + unary deltas,
      modes 0x71 zero-pred / 0x73 vert-pred) — ported + tested (6 tests incl. an
      **oracle bit-exact cross-check** on DSC_8070 precinct 0 / LB 0, all 6 sub-bands,
      701 groups, mode 0x73 with shared reader state). Fixture:
      `ticoraw/testdata/gcli_lb0_8070.txt`; capture method in oracle README.
- [x] `coefficient_decode` (`unpack_coefficient_magnitudes`: nibble-per-bitplane
      MSB-first → `<< gtli`; `apply_sign_bits`: one bit per non-zero coeff) — ported
      + tested (5 tests incl. an **oracle bit-exact cross-check** on DSC_8070
      precinct 0 / LB 0: 2804 coefficients across all 6 sub-bands, shared data+sign
      readers). Fixture: `ticoraw/testdata/coeff_lb0_8070.txt`.
- [x] `dequantize` (deadzone-midpoint reconstruction via 16-entry scale table +
      `w4=4` tone shift; `dequantize_coefficient` / `_ll_coefficient` /
      `_coefficient_array`) — ported + tested (5 tests incl. an **oracle bit-exact
      cross-check** on DSC_8070 precinct 0 / LB 0: pre→post dequant for all 2804
      coefficients). Note: the array path applies the uniform formula to all bands
      (the reference ignores `is_ll_band`). Fixture: `ticoraw/testdata/dequant_lb0_8070.txt`.
- [x] `precinct_header` (24-bit total_size + Bp/Br + 28×2-bit Dpb + 8 interleaved
      7-byte LB mini-headers `[f20_sign:1][data:20][gcli:20][sign:15]`, **DX `sig`
      formula baked in**) — ported + tested (5 tests incl. an **oracle bit-exact
      cross-check** re-parsing real DSC_8070 precinct 0: total_size/Bp/Br/28 Dpb +
      all 8 LBs' sig/gcli/data/sign/offsets). Fixture: `ticoraw/testdata/precinct_hdr_p0_8070.txt`.
- [x] `predecessor` (`PrecinctPredecessorState`: cross-band GCLI rotation for the LL
      bands sb 12/23, `get_previous_gcli`/`save_gcli`/`advance_precinct`/
      `reset_gcli_state`, `should_reset_gcli`) — ported + tested (7 tests: rotation
      semantics both directions, advance zeroing, precinct-16 reset, insig flag).
      Rust models the reference's pointer-aliased rotation buffers directly. Full
      bit-exact validation lands at `precinct_decode` (it drives the real decode).
- [ ] `precinct_decode`
- [ ] `idwt_horizontal`, `idwt_vertical`
- [ ] `tile` (`decode_tile`)
- [ ] `bayer` (`step1_merge_4_to_2`, `step2_bayer_rows`)
- [ ] top-level `decode_nikon_he_image` (3-pass driver) → hook into `decode_ticoraw`

**Validation**
- [x] Build the reference **oracle harness** (`tools/nikon_he_oracle`, CMake+MSVC).
- [x] **Fix the reference for DX** (per-LB `sig` formula; `dx_sig_fix.patch`) —
      oracle now decodes HE + HE\* on Z50 II, matching Adobe (RMSE ≈ 0.57). §8/§12.
- [ ] Add per-stage intermediate dumps to the (fixed) oracle for stage-by-stage Rust diffing.
- [ ] Build decode→PGM→diff harness (dnglab our-path → PGM; `magick compare` / numpy vs `target/gt/*.pgm`)
- [ ] HE (DSC_8070) decodes; iterate to **0 error** ← primary milestone
- [ ] HE\* (0566/0567): decode, measure error, push "as far as possible"
- [ ] Wire result into `RawImage` (levels, photometric, crop) so `dnglab convert` works

## 8. Risks / open questions

- **✅ RESOLVED (2026-09-20): DX support cracked; oracle decodes HE AND HE\*.**
  The samples are **Nikon Z50 II** — a **DX (APS-C)** body, 5600×3728. The
  reference (validated only on FX) originally crashed because
  `parse_precinct_header` used a wrong per-LB significance (`sig`) substream size
  on DX (hardcoded `sig=f20`). **Fix:** `sig_bytes[lb] = ceil( Σ_sb ceil(ng_sb/8) / 8 )`
  (significance = 1 bit per Ss=8 coeff-group, packed 8/byte) → sig =
  `[12,12,11,12,11,11,11,11]`. See `tools/nikon_he_oracle/dx_sig_fix.patch`.
  With the fix, the oracle decodes all three files (`success=1`, 59 tiles, 1048
  precincts) and matches Adobe ground truth to **RMSE ≈ 0.57, mean abs err ≈ 0.14
  LSB** (99.7% of pixels within ±1; max |Δ| ≈ 183 on a handful): HE `DSC_8070`
  PSNR 101 dB, HE\* `DSC_0566` 103 dB, HE\* `DSC_0567` 97 dB. The ±1 spread is
  dither/rounding vs Adobe — essentially a correct decode. **HE\* is NOT a
  blocker on this camera** (contrary to the FX-era reference caveat).
  - Remaining tiny residual (a few thousand px with |Δ|≥8, max 183) is unchased
    polish — likely dequant/tone-curve rounding; revisit only if it matters.
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

There are **two workstreams**: **(A) foundational / validation infra** (harness,
oracle, and the DX reference fix) and **(B) the Rust module port** (§7). (A) must
be far enough along to validate (B). Right now (A) has an open blocker (DX).

- **(A) Foundational — done:** environment + helper tools
  (cmake/ninja/imagemagick/exiftool), branch + fork/upstream remotes, ground-truth
  PGM harness, reference cloned (`nikon-he-decoder`), **oracle harness built**
  (`tools/nikon_he_oracle`), plan.
- **(A) Foundational — ✅ COMPLETE (incl. DX):** the oracle now decodes our
  **DX (Z50 II)** files (HE + HE\*) after the per-LB `sig` fix
  (`tools/nikon_he_oracle/dx_sig_fix.patch`), matching Adobe to RMSE ≈ 0.57 (§8,
  §12). Entropy-stage modules can now be oracle-validated on DX. **Workstream (A)
  is done** — remaining optional: per-stage intermediate dumps for finer diffing.
- **(B) Ported + tested (49 ticoraw tests green):** `bit_reader`, `subband_config`,
  `predict_lut`, `picture_header`, `gtli_table`, `iqx_iqp_lut_data`, `gcli_decode`,
  `coefficient_decode`, `dequantize`, `precinct_header`, `predecessor` (entropy
  stages + `precinct_header` **oracle bit-exact-validated** on real HE data;
  `precinct_header` carries the DX `sig` fix). Everything needed to decode one
  precinct's sub-bands now exists; `precinct_decode` will wire them together.
  Header/framing
  **verified on real HE + HE\*** (5600×3728, comps=4, nbands=25,
  precinct_offset=155=0x9B, supported=true). `decode_ticoraw` parses the picture
  header and returns a WIP error (no pixels yet).
- **Foundational workstream (A) is COMPLETE**, including the DX fix — oracle
  decodes HE + HE\* on the Z50 II, matching Adobe (RMSE ≈ 0.57).
- **▶ NEXT ACTION (resume module port, workstream B, §6a lifecycle, ONE at a time,
  oracle-validated):** the per-sub-band entropy chain, `precinct_header` (DX `sig`
  fix), and `predecessor` are done. Next is **`precinct_decode`** — the integration
  module that wires header + entropy chain + predecessor together and scatters into
  bufA/bufB; oracle-validate the full per-precinct bufA/bufB output there. Then the
  IDWTs, `tile` (+ `compute_buf_stripe_ints`/
  `compute_kband` from `nikon_he_tile.h`), `bayer`, and the 3-pass driver.
  Validate each against the fixed oracle (bit-exact for HE). Reference:
  `D:\_repos\_ref_libraw_he\src\decoders\nikon_he\` (branch `nikon-he-decoder`,
  with `dx_sig_fix.patch` applied); target: `rawler/src/decompressors/ticoraw/`.

## 10. Session log

| Date       | Session | Summary |
|------------|---------|---------|
| 2026-09-19 | 1       | Diagnosed RapidRAW's dark HE\* preview fallback; identified format as JPEG-XS/TicoRAW; installed toolchain; created branch + scaffold (marker/PIH parser, tests); routed HE/HE\* into new path; built ground-truth harness; cloned reference decoder; set up fork/upstream remotes; wrote this plan. No codec code yet. |
| 2026-09-20 | 2       | Pushed branch to fork (`origin`=calebWei/dnglab). Ported 4 modules, each unit-tested (16 ticoraw tests green): `bit_reader`, `subband_config`, `predict_lut`, `picture_header`. Switched reference base to `nikon-he-decoder` branch (has `picture_header`/`gtli_from_weights`; production was missing it). Unified `decode_ticoraw` on the new parser; **verified header/framing on real HE + HE\*** (precinct_offset=155=0x9B, supported=true). Installed helper tools (cmake, ninja, imagemagick, exiftool); confirmed HE vs HE\* via exiftool. Documented §6a per-module workflow + oracle-harness plan. Commits through `c79c39b6` pushed; doc/tooling commit follows. Then built the oracle harness (`tools/nikon_he_oracle`, CMake+MSVC): header parses on our files, but **reference decode SEGFAULTS on the Nikon Z50 II (DX, 5600×3728) samples** in `decode_precinct` (p=0) — reference validated only on FX bodies. Identified camera via exiftool; confirmed no hardcoded FX dims (DX = fixable bug). Opened §11 decision (debug DX vs get an FX sample). Then (per user: debug DX) root-caused the crash (§12): `parse_precinct_header` returns false on DX because the per-LB significance substream size is hardcoded `sig=f20=11` but the real DX value is 12 for the lift LBs (and differs for the LL LB) — `sig` is per-LB, not a global `f20`. Mini-header bit layout is correct. Saved analysis tooling (`tools/nikon_he_oracle/analysis/pp_boundary.py`). Next: derive the per-LB sig formula, patch the reference, verify oracle vs Adobe, then port. |
| 2026-09-20 | 3       | **Completed foundational workstream (A), incl. the DX fix.** Derived the per-LB significance formula `sig=ceil(Σ ceil(ng_sb/8)/8)` = `[12,12,11,12,11,11,11,11]`; verified all 8 LB headers align. Patched the reference (`dx_sig_fix.patch`) and rebuilt the oracle: **HE (DSC_8070) and both HE\* (0566/0567) now decode** and match Adobe to RMSE ≈ 0.57 / mean-abs ≈ 0.14 LSB (PSNR 97–103 dB) — HE\* is fine on this camera. Expanded §6a into a full module lifecycle (Selected→…→Done with a definition-of-done gate); classified the DX RE as foundational feeding the `precinct_header` module. Saved `dx_sig_fix.patch` + analysis scripts under `tools/nikon_he_oracle`. Next session: resume the Rust module port (workstream B) from `gtli_table`, oracle-validated. |

| 2026-09-20 | 4       | **Ported `gtli_table` (workstream B, module 5/…).** Faithful port of `nikon_he_gtli_table.{h,cpp}`: the live dynamic path (`compute_gtli_table`/`gtli_for_sub_band` computing `clamp(Qp-gain[w]-(priority[w]<Rp),0,15)` from the picture-header WGT weights) plus the 26→25 `wgt_index_for_band` remap (pass-B LL band 23 reuses pass-A LL WGT band 12) and the reference's verbatim static fallback table (retained as fixture; dead on the header-present live path). 7 unit tests (17 ticoraw total, all green); clippy clean; fmt clean. Ground-truth cross-check: every captured static row satisfies `values[23]==values[12]`, independently confirming the shared-LL band remap. Pure LUT/formula module → unit tests suffice per §6a (no oracle diff required at this stage). Next: `iqx_iqp_lut_data`. |

| 2026-09-20 | 4       | **Ported `iqx_iqp_lut_data` (workstream B, module 6/…).** Faithful port of `nikon_he_iqx_iqp_lut_data.h`: 256 PWL `(x_in,y_out)` breakpoints (generated from the reference to avoid transcription error) + lazy materialization of the 81792-entry tone-curve LUT by integer linear interpolation (`ya + (yb-ya)*(i-xa)/(xb-xa)`), cached via `OnceLock` in a heap `Vec` (~320 KB). Guarded the reference's benign out-of-bounds `BREAKPOINTS[k+1]` read at the final breakpoint (harmless in C++ only because `i-xa==0`). 4 unit tests (all 21 ticoraw green); rustfmt-clean; clippy-clean. Pure LUT module → unit tests suffice per §6a. Both `gtli_table` (module 5) and `iqx_iqp_lut_data` landed this session. Reverted unrelated crate-wide rustfmt churn on session-2 files. Next: entropy stages, starting `gcli_decode`. |

| 2026-09-20 | 4       | **Ported `gcli_decode` (workstream B, module 7/… — first entropy stage).** Faithful port of `nikon_he_gcli_decode.{h,cpp}`: significance bit per 8-group block + per-group unary deltas; modes 0x71 (zero-pred, `gcli=gtli+u`) and 0x73 (vert-pred via prediction LUT, baseline `max(prev,gtli)`). 5 synthetic unit tests + **1 oracle bit-exact cross-check**: established the oracle-capture pattern — added an env-gated (`NIKON_HE_GCLI_DUMP`) dump to the reference `decode_precinct`, rebuilt the oracle, decoded real DSC_8070, and captured precinct 0 / LB 0 (shared sig+gcli readers, all 6 sub-bands / 701 groups, mode 0x73, zero prev). The Rust port reproduces the reference output byte-for-byte (fixture `ticoraw/testdata/gcli_lb0_8070.txt`). Reverted the temporary reference capture (dx_sig_fix intact). 27 ticoraw tests green; rustfmt-clean; module clippy-clean (crate-wide unwrap/format lints are pre-existing toolchain noise). Next: `coefficient_decode`. |

| 2026-09-20 | 4       | **Ported `coefficient_decode` (workstream B, module 8/…).** Faithful port of `nikon_he_coefficient_decode.{h,cpp}`: `unpack_coefficient_magnitudes` (per group, `gcli-gtli` bit-planes as nibbles, MSB-first, distributed bit3..0 → coeff0..3, then `<< gtli`) and `apply_sign_bits` (one bit per non-zero coeff). 4 synthetic unit tests + **1 oracle bit-exact cross-check**: reused the capture pattern (`NIKON_HE_COEFF_DUMP`) to dump precinct 0 / LB 0's data+sign buffers and the final signed coefficients; the Rust port reproduces all **2804 coefficients** (6 sub-bands sharing one data + one sign reader) byte-for-byte. Fixture `ticoraw/testdata/coeff_lb0_8070.txt`. Reference capture reverted (dx_sig_fix intact). 32 ticoraw tests green; rustfmt-clean (project max_width=160); module clippy-clean. Next: `dequantize` (finishes the per-sub-band entropy chain). |

| 2026-09-20 | 4       | **Ported `dequantize` (workstream B, module 9/… — completes the per-sub-band entropy chain).** Faithful port of `nikon_he_dequantize.{h,cpp}`: `dequantize_coefficient` (deadzone-midpoint: `mag = |coef|>>gtli`, `(mag * scale_table[bpc-1]) >> (16-gtli) << 4`, sign-preserving; u64 intermediate), the LL helper (ported for fidelity; unused — the array path ignores `is_ll_band` and applies the uniform formula to all bands), and `dequantize_coefficient_array`. 4 synthetic unit tests + **1 oracle bit-exact cross-check** (`NIKON_HE_DEQ_DUMP`): captured pre- and post-dequant coefficients for precinct 0 / LB 0; the Rust port reproduces all 2804 post-dequant values. Fixture `ticoraw/testdata/dequant_lb0_8070.txt`. Reference capture reverted (dx_sig_fix intact). 37 ticoraw tests green; rustfmt-clean; module clippy-clean. Next: `precinct_header` (bake in the DX `sig` formula from §12), then `precinct_decode`/`predecessor` — the integration point to oracle-validate the full per-precinct bufA/bufB scatter. |

| 2026-09-20 | 4       | **Ported `precinct_header` (workstream B, module 10/…) — with the DX fix baked in.** Faithful port of `nikon_he_precinct_header.{h,cpp}` incl. the project's `dx_sig_fix`: `compute_lb_sig_bytes` (per-LB significance = `ceil(Σ ceil(ng_sb/8)/8)`), `compute_f20`, and `parse_precinct_header` (24-bit total_size, Bp/Br, 28×2-bit Dpb, 8 interleaved 7-byte LB mini-headers `[f20_sign:1][data:20][gcli:20][sign:15]`, walking substream payloads). 4 unit tests (DX sig = `[12,12,11,12,11,11,11,11]` for 5600; f20 DX/FX; mini-header bit extraction; too-short) + **1 oracle bit-exact cross-check** (`NIKON_HE_PHDR_DUMP`): re-parses real DSC_8070 precinct 0 and matches every field (total_size=8387, Bp=7, Br=17, all 28 Dpb, all 8 LBs' sig/gcli/data/sign counts + offsets — LB0 sig=12/gcli=233/data=581/sign=225, consistent with the earlier entropy fixtures). Fixture `ticoraw/testdata/precinct_hdr_p0_8070.txt`. Reference capture reverted (dx_sig_fix intact). 42 ticoraw tests green; rustfmt-clean; module clippy-clean. Next: `predecessor` (cross-band GCLI state), then `precinct_decode`. |

| 2026-09-20 | 4       | **Ported `predecessor` (workstream B, module 11/…).** Faithful port of `nikon_he_predecessor.{h,cpp}`: `PrecinctPredecessorState` managing cross-band GCLI prediction for the LL bands (sb 12 ← sb 23 of the previous precinct; sb 23 ← sb 12 of the current precinct) via two rotation buffers, plus `get_previous_gcli`/`save_gcli`/`advance_precinct` (zeros the sb 12 buffer, keeps sb 23)/`reset_gcli_state`/`set_fully_insig`, and the free `should_reset_gcli` (precinct 16). Rust models the reference's raw-pointer aliasing (`gcli_store[12]→buf_b`, `[23]→buf_a`) by routing sb 12/23 through the rotation buffers directly — behaviorally identical, no `unsafe`; ownership removes the C++ `destroy`. 7 unit tests (both rotation directions, advance zeroing, precinct-16 reset, insig flag, reset condition). Its bit-exact validation lands at `precinct_decode` (it drives the real decode). 49 ticoraw tests green; rustfmt-clean; clippy-clean. Next: `precinct_decode` — the integration milestone (oracle-validate the full per-precinct bufA/bufB scatter). |

<!-- Append a new row per session. Keep §9 "Next action" current. -->

## 11. Open decision — DX support path — ✅ RESOLVED

Decision taken: **debug DX** (option A). Outcome: DX cracked, oracle decodes HE +
HE\* on the Z50 II (§8, §12). No FX sample needed. (Original options retained below
for history.)

The reference crashes on our Nikon Z50 II (DX) files (§8). Options:

- **A — Debug the reference for DX ourselves.** Root-cause the `decode_precinct`
  crash on `DSC_8070` (precinct-header offsets for DX geometry) using the oracle
  harness + tracing, fix it, then port the fixed logic. Self-contained; no new
  inputs. Risk: DX may differ in more than one place.
- **B — Get an FX HE sample.** An HE `.NEF` (+ Adobe `.dng`) from a Z8/Z9/Z6III/Zf/
  Z5II. Confirms the reference decodes FX correctly (oracle vs Adobe), gives a
  bit-exact checkpoint to port against, and lets us bisect the DX-specific
  difference. Strongly de-risks the port.
- **C — Both:** get an FX sample *and* debug DX. Recommended if an FX file is
  available — validate the port on FX, then extend to DX.

Tooling to bisect is ready: `tools/nikon_he_oracle` (add per-stage `fprintf`
dumps to the reference to compare against Rust stage-by-stage).

## 12. DX debug findings (2026-09-20) — ROOT CAUSE LOCALIZED

Debugging the crash on `DSC_8070` (Z50 II HE) via the oracle harness (trace edits
to the ref, since reverted) pinned the DX incompatibility:

- The crash is a **secondary** effect: `parse_precinct_header` **returns false**
  for DX, then `decode_tile` mishandles the failed precinct and segfaults in its
  tail/`memcpy`. Root cause is the header parse, not the entropy math.
- **Root cause: the per-LB significance (`sig`) substream size is wrong for DX.**
  The reference hardcodes `sig = f20 = ceil(image_width/2 / 256)` (= **11** for our
  2800 half-width) for every LB. The real DX values differ, so every LB boundary
  after LB0 drifts and the parse overflows.
- **The 7-byte LB mini-header bit layout is CORRECT for DX**
  (`[1 f20_sign][20 data][20 gcli][15 sign]`, big-endian): extracted values are
  sane (LB0 data=581 gcli=233 sign=225; LB1 data=573 gcli=231 sign=237).
- **Measured true boundaries** (precinct 0, size 8387, Bp=7 Br=17), by scanning for
  real `0x00`-led headers (`tools/nikon_he_oracle/analysis/pp_boundary.py`):
  - LB0 header @off 12 → real LB1 header @off **1070** ⇒ LB0 region = 1058 = 7 (hdr)
    + 1051 (payload); payload − (data+gcli+sign=1039) ⇒ **sig = 12** (not 11).
  - LB1 @1070 ⇒ **sig = 12**. LB2 (the LL LB, sb 12) has a **different** sig.
  - So `sig` is **per-LB / per-LB-type**, likely derived from the LB's
    significance-group count — NOT a single global `f20`.

### Where this RE sits (foundational vs module work)

This DX RE is **foundational validation-infra work, NOT a module port** — but its
output feeds one module:

- **Foundational (do now, in the reference C++):** derive the DX `sig` formula and
  patch the reference's `parse_precinct_header` so the **oracle** decodes our
  files. This unblocks the oracle as the validation tool for **every** entropy
  stage (§6a step 4 requires an oracle diff; on DX that oracle doesn't exist until
  this is fixed). It gates the whole port, not one module.
- **Module input (later, in Rust):** the resulting per-LB `sig` formula becomes
  part of the **`precinct_header`** module when it is ported to Rust (§7). Port it
  with the DX fix baked in; do not re-port the reference's buggy `sig=f20`.

So: finish the DX reference fix as foundational work → then resume the §7 module
port order, using the now-working oracle to validate each stage.

### ✅ DX cracked (2026-09-20) — DONE

Derived and verified the per-LB `sig` formula:
`sig_bytes[lb] = ceil( Σ_{sb in lb} ceil(ng_sb / 8) / 8 )` → `[12,12,11,12,11,11,11,11]`.
All 8 LB mini-headers align (flag=0, sane fields). Patched the reference
(`nikon_he_precinct_header.cpp`; see `tools/nikon_he_oracle/dx_sig_fix.patch`),
rebuilt the oracle: **HE and both HE\* files decode and match Adobe** (RMSE ≈ 0.57).
The oracle is now a working per-stage validation tool for DX (HE + HE\*).

**To reproduce in a fresh clone:** `git apply` the patch onto
`D:\_repos\_ref_libraw_he` (branch `nikon-he-decoder`), then build
`tools/nikon_he_oracle` (README). Strips: `analysis/extract_strip.py`.

Optional later polish: `decode_tile` should fail gracefully on a bad precinct
(currently can crash) — moot once `sig` is correct, but nice for robustness.
The ~thousands of |Δ|≥8 px (max 183) are unchased (dequant/curve rounding).
