# Windows Checkpoint #2 Handoff: Phase 2 Linear Baker, `.klm` HDR Lightmaps & Unified Room Shadows

Date: 2026-11-27  
Branch: `feat/visual-phase0-baseline`  
Target hardware: Windows Desktop PC (RTX 4070 Ti SUPER — Vulkan HW ray tracing + `kjk detail-bake --hdr`)

This checkpoint verifies Phase 2:
- **PR 2A (Unified Baked-Room Shadows & Ultra RT Fitted Shadows):** baked rooms skip BioWare's legacy local lamp loop, `OVERHEAD_WEIGHT` blobs, and duplicate static placeable shadows; dynamic characters cast RT/capsule fitted-light shadows via `scene_rays` (`#ifdef RAY_QUERIES`) on Ultra.
- **PR 2B (`.klm` `Rgba16Float` HDR Lightmaps + Island-Padded Mips):** uncompressed-on-GPU HDR lightmaps (`KLM2`, LZ4-compressed on disk) preserve unclamped radiance above `1.0`, clean near-black tonal resolution, UV island-padded mipmaps (zero dark-gutter seam bleeding), and the `sunlit` visibility mask in `.a` — while staying 100% backwards-compatible with existing v1 `.png` bakes.
- **PR 2C (Linear Atmospheric GPU Baker):** `kjk detail-bake --hdr` (`WRAP = 0.0`, linearized fitted lights, linear 3-bounce GI, no `^2.2` / `B_KNEE` clipping, no forced flat `B_MEAN` / `even` ambient wash, configurable `--exposure` and `--ambient-floor`).

---

## 1. Sync & Build on Windows

```powershell
git fetch origin
git checkout feat/visual-phase0-baseline
git pull

# Build with DLSS (or omit --features dlss for standard build)
$env:DLSS_SDK = "<path-to-DLSS-SDK>"
cargo build --release -p launcher --features dlss
```

*(Also build with `--features probes` if you want the live `KJK_GPU_TIMES` / probe / `detail-bake` pack commands — note `kjk detail-bake` itself does not require `probes`, while `kjk lighting pack` does.)*

---

## 2. Quick Verification on Existing Bakes (PR 2A Shadow Fix)

Before baking anything new, run a quick capture or walk through a baked area (`101per`, `danm13`, or `manm26aa`) under **Enhanced Lightmaps** (`--shadows ultra` and `--shadows high`):
1. Stand a character near a static placeable crate/console in a baked room:
   - Static placeables should no longer cast extra double/triple dynamic shadow boxes/blobs onto baked floors.
   - Characters should cast clean directional shadows from nearby fitted lamps (and sharp/jittered HW RT shadows under Ultra), plus subtle character contact shading near feet/walls.

---

## 3. Test-Baking `.klm` HDR Atmospheric Areas (`kjk detail-bake --hdr`)

Rather than copying the old flat v1 bakes, `--hdr` embraces moody, directional interior contrast with crisp fixture pools, natural 3-bounce indirect GI, and a subtle `--ambient-floor` (`0.008` default) so dark corridors stay readable under AgX without turning flat grey.

Pick 4–5 diverse areas (suggested: Peragus `101per`, Nar Shaddaa tunnels e.g. `301nar` / `303nar`, Taris Cantina `tar_m03ae`, Dantooine `danm13` with `--sun`, and Endar Spire `end_m01aa`):

### Step 3a: Dump fitted lights & bake to a scratch folder (`target\hdr-bakes`)
For each `<module>` (`101per`, `301nar`, `tar_m03ae`, etc.):

```powershell
# 1. Dump the module's fitted lights once
$env:KJK_FITTED_DUMP = "target\hdr-bakes\<module>.lights.txt"
.\target\release\kjk.exe play --stage <module> --walk --headless --cmds "wait world; wait 10f; quit"
Remove-Item Env:KJK_FITTED_DUMP

# 2. Run the linear HDR GPU bake (.klm output in target\hdr-bakes\<module>\b\)
.\target\release\kjk.exe detail-bake <module> `
  --lights target\hdr-bakes\<module>.lights.txt `
  --out target\hdr-bakes\<module> `
  --hdr `
  --exposure 1.0 `
  --ambient-floor 0.008 `
  --placeables --placeable-light
```

*(For outdoor/skylight areas like `danm13` or `tat_m17aa`, pass the existing `--sun ...` / `--skylights ...` flags from `data/lights/bake.toml` alongside `--hdr`.)*

### Step 3b: Load the new `.klm` bakes live in `kjk`
Point `KJK_BAKED_LIGHTMAPS` (or check `<module>\b` folder discovery) to your test bake folder and run with `Enhanced Lightmaps` (`display.lighting = "baked"`):

```powershell
# Launch interactively or capture screenshots against target\hdr-bakes\<module>\b
$env:KJK_BAKED_LIGHTMAPS = "target\hdr-bakes\<module>\b"
.\target\release\kjk.exe play --stage <module> --walk
```

Verify `.klm` files were produced (`target\hdr-bakes\<module>\b\*.klm`, zero `.png` files for lightmapped rooms) and loaded without errors in the log.

---

## 4. What to Evaluate Visually (Checkpoint #2 Sign-Off)

1. **Mood & contrast in dark indoor areas (`101per`, Nar Shaddaa tunnels, `tar_m03ae`):**
   - `WRAP = 0.0` removes the washed-out wraparound light — check whether directional lamp pools, recessed walls, and architectural silhouettes feel genuinely atmospheric.
   - Check dark corridors for **smooth near-black gradients** (zero 7-bit PNG contour banding!).
   - Tune `--ambient-floor` (`0.0` = pure 3-bounce physical GI, `0.008` = default subtle fill, `0.003–0.015` if tunnels feel too dark/bright) and `--exposure` (`0.8–1.5`) as needed.
2. **Bright lamps & outdoor highlights (`end_m01aa`, `danm13`, `tat_m17aa`):**
   - Fixtures and sunlit areas can now exceed `1.0` naturally in `.klm` without `GAIN = 2.0` / `B_KNEE` clamping — verify bloom and AgX roll off fixture hot-spots smoothly.
3. **Mipmap chart seams:**
   - Look closely along floor/wall trim edges at oblique angles and far distances (`101per`, `end_m01aa`) to confirm island-padded `.klm` mipmaps prevent black UV-gutter bleeding.
4. **Stamp & package sanity (optional spot check):**
   - Run `detail-bake --stamp target\hdr-bakes\<module>\b` and `kjk lighting pack` (built with `--features probes`) on a small test folder to confirm `.klm` files pack cleanly with `Stored` compression.

---

## 5. Questions to Bring Back Before Phase 3

1. Preferred baseline `--exposure` and `--ambient-floor` defaults for dark interiors (`101per` / Nar Shaddaa) vs bright interiors (`end_m01aa`).
2. Whether any rooms still need per-room scale multipliers in `--levels` (under `--hdr`, `--levels` multiplies each room's linear output directly rather than normalizing to `B_MEAN`).
3. Whether `bake.toml` defaults should be updated before kicking off full K1/TSL `.klm` re-bakes alongside **Phase 3 (Hybrid Character Lighting: 3D Bounce Probes + Live Fitted Direct Lights)**.
