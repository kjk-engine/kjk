# Windows Checkpoint #3 Handoff: Phase 3 Hybrid Character Lighting & Tolerant MDL Matching

Date: 2026-11-27  
Branch: `feat/visual-phase3-hybrid-lighting`  
Target hardware: Windows Desktop PC (RTX 4070 Ti SUPER — Vulkan HW ray tracing + DLSS + `kjk detail-bake`)

This checkpoint verifies Phase 3:
- **PR 3A (Multi-Height Indirect-Only 3D Probes):** `detail-bake` writes dual-height (`0.6 m` / `1.8 m`) ceiling-aware `probes-hybrid.txt` containing indirect-only linear GI, per-room `direct_scale`, and Sun 1 / Sun 2 visibility, and `kjk lighting pack` ships `probes-hybrid.txt`.
- **PR 3B (3D Blending & Automatic Loading):** `light_probes.rs` interpolates probes in 3D `(x, y, z)` at each model's midpoint height and uploads `ProbeList` + `sun2` metadata; `viewer.rs` loads `probes-hybrid.txt` automatically (toggle `KJK_HYBRID_PROBES=off` for A/B comparison).
- **PR 3C (Hybrid Character/Door/Prop Shading):** `odyssey.wgsl` shades characters/doors/unbaked placeables with linear indirect 3D probes + live `fitted_lights` (`wrap = 0.0`, `* direct_scale`) + live Twin Suns, with `RAY_WALLS` HW RT wall/pillar/door occlusion on Ultra.
- **PR 3D (Tolerant MDL Fingerprint Matching):** `bake_fingerprint.rs` writes & reads `lightmap-approx` / `model-approx` quantized digests alongside exact digests so harmless re-saved MDL drift keeps bakes active.

---

## 1. Sync & Build on Windows

```powershell
git fetch origin
git checkout feat/visual-phase3-hybrid-lighting
git pull

# Release build with DLSS (or omit --features dlss)
cargo build --release -p launcher --features dlss
```

*(Also run targeted probes with `--features probes`: `cargo run --release -p launcher --features probes -- sim probes --only "light probes,bake fingerprint,shader bounds,klm lightmap"`.)*

---

## 2. Re-Bake Canary Modules (`probes-hybrid.txt`)

Existing `.klm` sets from Checkpoint #2 will not yet contain `probes-hybrid.txt` (they will safely fall back to Phase 2 floor-readback probes until re-baked). Re-bake the 5 canary areas (`101per`, `tar_m03ae`, `end_m01aa`, `danm13`, `tat_m17aa`) using the same flags as Checkpoint #2 so `b\probes-hybrid.txt` is generated beside `*.klm`:

1. Verify `b\probes-hybrid.txt` exists in each module's output folder and contains `format 2`, dual-height probes, and `sun` / `sun2` lines on `danm13` / `tat_m17aa`.
2. Note any change in dump + bake time from the dual-height probe pass.

---

## 3. Visual & Technical Checks (Checkpoint #3 Sign-Off)

1. **A/B Comparison (`KJK_HYBRID_PROBES=on` vs `KJK_HYBRID_PROBES=off`):**
   - **Moody interiors (`101per`, `tar_m03ae`, `end_m01aa`):** walk hero/companions past light strips, wall panels, and dark corners.
   - Check that character brightness and hue match nearby walls/floors without glowing/floating in dark corners (`PROBE_FLOOR` lift removed).
   - Check `KJK_MODEL_LIGHT=probes`, `KJK_MODEL_LIGHT=fitted`, and `KJK_MODEL_LIGHT=whole` in one interior scene.
2. **Ultra HW RT Wall & Pillar Occlusion on Characters (`--shadows ultra` vs `--shadows high`):**
   - Stand a character behind an interior pillar, doorframe, or closed door near a bright lamp (`101per`, `end_m01aa`, `tar_m03ae`) and confirm Ultra RT `RAY_WALLS` cuts off direct lamp bleed cleanly without ankle/floor self-shadow artifacts.
3. **Outdoor & Twin Suns (`tat_m17aa`, `danm13`):**
   - Confirm hero/NPCs in Anchorhead (`tat_m17aa`) receive crisp directional shading from both twin suns in sunlight (`sun1_vis`, `sun2_vis`) and soft cool sky/bounce fill in alley shade, with dynamic character-on-character sun shadows.
4. **Tolerant MDL Matching & Pack Sanity:**
   - Check `100%` runtime fit on all 5 re-baked modules (`fingerprint.txt` now contains `lightmap-approx` and `model-approx` lines).
   - Run `kjk lighting pack` (with `--features probes`) on a small staging folder to confirm `probes-hybrid.txt` is packed cleanly.
5. **Classic Parity & Regression Baseline:**
   - Capture all 8 visual-baseline scenes in **Enhanced (High)**, **Enhanced (Ultra)**, **Enhanced + DLSS**, and **Classic**, and check `kjk compare-pictures` vs Checkpoint #2 for zero Classic regressions and expected GPU timings (`main_opaque` & `ray_structures` p50).
