# Windows Checkpoint #1 Handoff: Phase 0 Baseline & Phase 1 Linear HDR Verification

Date: 2026-11-27  
Branch: `feat/visual-phase0-baseline`  
Target hardware: Windows Desktop PC (RTX 4070 Ti SUPER — DX12/Vulkan, DLSS/DLAA, Ultra RT warmup, `KJK_GPU_TIMES`)

This checkpoint verifies Phase 0 (8-scene capture harness + `ray_structures` `SceneHold` warmup fix) and Phase 1 (True Linear HDR main pass, two-texture `(accum_colour, transmittance)` composite via `legacy_blend`, `HDR_GLOW` radiance preservation, MSAA `DecodeBeforeBloom` fallback, AgX/ACES tonemapping, linear bloom, post-tonemap `sharpen`, and removal of `linear_frame`).

---

## 1. Sync & Build on Windows

1. Ensure KOTOR I and KOTOR II game data are configured as usual on the Windows machine (e.g. `KJK_K1` / `KJK_TSL` or standard install discovery).
2. Checkout the branch in your Windows clone:
   ```powershell
   git fetch origin
   git checkout feat/visual-phase0-baseline
   ```
3. Build release binaries (build **with `--features dlss`** if NVIDIA's Streamline/DLSS SDK is installed on the Windows box; otherwise build standard release):
   ```powershell
   # Standard release build
   cargo build --release -p launcher

   # With DLSS support (when NVIDIA SDK is configured)
   cargo build --release -p launcher --features dlss
   ```
   Binary path on Windows: `target\release\kjk.exe` (pass `--exe target\release\kjk.exe` to the Python harness scripts so `.exe` resolves cleanly).

---

## 2. Pre-Phase-1 vs Phase 1 Comparison (`tools/visual-baseline/`)

If you want a pixel-exact **Classic parity** diff against pre-Phase-1 `origin/main` (`b02e47c53`):
1. Build `origin/main` once into a scratch binary or worktree (say `..\kjk-main\target\release\kjk.exe`) and capture the pre-Phase-1 baseline:
   ```powershell
   python tools/visual-baseline/capture.py --label pre-phase1 --modes classic,enhanced --exe ..\kjk-main\target\release\kjk.exe --gpu-times --sheet
   ```
2. Capture Phase 1 from `feat/visual-phase0-baseline`:
   ```powershell
   python tools/visual-baseline/capture.py --label phase1-agx --modes classic,enhanced --exe target\release\kjk.exe --gpu-times --sheet
   ```
3. Compare `classic` and `enhanced` side by side:
   ```powershell
   python tools/visual-baseline/compare.py pre-phase1 phase1-agx --exe target\release\kjk.exe --out target\visual-baseline\diff-pre-vs-phase1
   ```
   **Acceptance criteria (Classic):**
   - `classic` reports **`largest: 0`, `mean: 0.000`, `changed: 0%`** across all 8 scenes (`endar_corridor`, `taris_cantina`, `dantooine_enclave`, `anchorhead_suns`, `manaan_water`, `peragus_tunnels`, `malachor_dark`, `dune_sea_sky`).

---

## 3. AgX vs AgX Punchy vs ACES vs Rolloff Contact Sheets

Phase 1 defaults to **AgX** (`KJK_TONEMAP=agx`, `KJK_BLOOM=0.12`). Capture Enhanced variants for visual sign-off:

```powershell
$env:KJK_TONEMAP = "agx_punchy"
python tools/visual-baseline/capture.py --label phase1-agx-punchy --modes enhanced --exe target\release\kjk.exe --sheet

$env:KJK_TONEMAP = "aces"
python tools/visual-baseline/capture.py --label phase1-aces --modes enhanced --exe target\release\kjk.exe --sheet

$env:KJK_TONEMAP = "rolloff"
python tools/visual-baseline/capture.py --label phase1-rolloff --modes enhanced --exe target\release\kjk.exe --sheet
Remove-Item Env:KJK_TONEMAP
```

If you want to tune bloom intensity (`threshold = 1.0`, `threshold_softness = 0.2`, `BloomCompositeMode::Additive`, default `0.12`):
```powershell
$env:KJK_BLOOM = "0.08"
python tools/visual-baseline/capture.py --label phase1-bloom-008 --modes enhanced --scenes endar_corridor,taris_cantina,peragus_tunnels --exe target\release\kjk.exe
Remove-Item Env:KJK_BLOOM
```

---

## 4. Focused Functional & Regression Checklist

Run through these checks interactively and via `capture.py`:

1. **Dune Sea regression (`dune_sea_sky` / `tat_m18aa`):**
   - Inspect `phase1-agx\enhanced\dune_sea_sky.png`.
   - Verify sky blue gradient and sand haze match Classic's warm desert/blue palette without Step 9's grey-purple shift (`legacy_blend` compositing in gamma 2.2 over linear sky/terrain).
2. **Dark scenes & specular strips (`endar_corridor`, `taris_cantina`, `malachor_dark`, `peragus_tunnels`):**
   - Check dark floor gradients (`malachor_dark`) for smoothness (no 7-bit gamma crushing).
   - Check strip lights (`endar_corridor`), signs (`taris_cantina`), and lightsaber blades/blaster bolts in active combat for clean white-core desaturation under AgX, smooth bloom, and **zero CAS ringing halos** (`sharpen` now runs after `RollOffPass`).
3. **Horizon clear-color / fog seam (`dantooine_enclave`, `anchorhead_suns`, `dune_sea_sky`):**
   - Verify distant geometry blends seamlessly into `ClearColor` (`sync_clear_color` updates `ClearColor` to `fog^2.2` when `linear_main_active`).
4. **MSAA 4x Enhanced fallback (`DecodeBeforeBloom`):**
   ```powershell
   python tools/visual-baseline/capture.py --label phase1-msaa4 --modes enhanced --msaa 4 --scenes dune_sea_sky,endar_corridor,taris_cantina --exe target\release\kjk.exe
   ```
   - Verify MSAA 4x + Enhanced clears cleanly, applies `decode_linear` before Bloom + AgX, and renders without wgpu validation warnings or black/blown-out frames.
5. **DLSS / DLAA & live runtime mode toggling (Windows RTX 4070 Ti SUPER):**
   - Capture or run interactively with `display.msaa = dlss_quality` / `dlss_performance` (or `--msaa dlss_performance` on `capture.py`).
   - Open Graphics Settings in-game and toggle Lighting live: **Enhanced → Classic → Enhanced → Classic → Enhanced** (plus switching TAA ↔ DLSS ↔ MSAA 4x).
   - Confirm zero stale linear/gamma shader state (`LightList::enhanced`, `grass.wgsl` `clock.z`, `ClearColor`, `LinearLegacyBlend`, and `DecodeBeforeBloom` update synchronously via runtime uniforms/markers).
6. **Ultra RT warmup hold (`SceneHold` / Phase 0 fix):**
   ```powershell
   python tools/visual-baseline/capture.py --label phase1-ultra --modes enhanced --shadows ultra --scenes dantooine_enclave,peragus_tunnels --exe target\release\kjk.exe
   ```
   - Verify no multi-second BVH rebuild stall or hold-screen hang during stage load under `--shadows ultra`.

---

## 5. GPU Performance Benchmark (`kjk bench`)

Measure GPU frame times on desktop with DX12/Vulkan and inspect the pass breakdown:

```powershell
# Enhanced baseline vs Phase 1 (use saved bench snapshots / preset flags)
target\release\kjk.exe bench enhanced --gpu-times
target\release\kjk.exe bench dlss-performance --gpu-times
target\release\kjk.exe bench shadows-ultra --gpu-times
```

Confirm in the `gpu` pass timings (from `--gpu-times` logs / dumps):
- `linear_frame` is gone (`0` cost; retired).
- New `legacy_blend` pass + `bloom` + `roll_off` (AgX) + `sharpen` fit comfortably within desktop GPU budgets (verify exact numbers on RTX 4070 Ti SUPER — expected sub-0.3 ms combined for `legacy_blend` + `roll_off` at 1440p/4K DLSS).

---

## 6. Sign-Off Decisions Before Phase 2

Record your answers before starting Phase 2 (Re-baking & PBR material upgrades) back on Mac/Windows:

1. **Preferred default tonemapper:** `agx` vs `agx_punchy` vs `aces` (inspect `phase1-*` contact sheets across all 8 scenes).
2. **Preferred default linear bloom strength:** `0.12` vs `0.08`–`0.15` (check `endar_corridor`, `taris_cantina`, and active lightsaber combat).
3. **Fog ramp in linear space:** Keep exact linear fog (`decode_22(fog_color)`) in `odyssey.wgsl` / `grass.wgsl`, or blend the fog factor perceptually if `dantooine_enclave` / `dune_sea_sky` haze feels slightly thicker in mid-distance.
4. **Any visual artifacts spotted** under DLSS, MSAA 4x, Ultra shadows, or Effects Quality Low/High switching.
