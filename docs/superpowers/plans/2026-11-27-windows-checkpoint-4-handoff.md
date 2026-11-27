# Windows Checkpoint #4 Handoff — Phase 4 (PBR Materials, Normal Maps & Directional GI)

**Branch:** `feat/visual-phase4-pbr-normals`  
**Target GPU:** RTX 4070 Ti SUPER (Vulkan + Hardware RT + DLSS)  

## What Changed in Phase 4
1. **All-Mesh Bump & Companion PBR Textures:** BioWare's `bumpmaptexture` + `bumpmapscaling` now drive cotangent-frame normal perturbation on all non-water meshes in Enhanced mode, with auto-detection for RGB tangent normal maps vs. grayscale heightmaps and optional `<diffuse>_n` / `<diffuse>_rm` Override companions.
2. **GGX Microfacet PBR Specular & Energy-Conserving IBL:** `ENVMAP` reflectivity masks (`1.0 - texel.a`) and `_rm` maps feed GGX specular across live fitted lights, Twin Suns, combat lights, and directional GI, plus Fresnel-weighted envmap IBL (keeping Classic untouched). Includes `KJK_PBR=off` for clean A/B comparison.
3. **Directional GI Lightmaps (`<name>.dir.png`):** GPU lightmap baker accumulates dominant incident direction across direct lights, suns/sky, and 3-bounce GI, outputting Bevy-space `<name>.dir.png` companion maps that sculpt normal maps under baked lighting without invalidating existing bakes (`bake_fingerprint::set_lightmaps` skips `*.dir.png`).

---

## Checkpoint #4 Verification Checklist (Windows)

### 1. Build, Lint & Deterministic Tests
```powershell
git fetch origin
git checkout feat/visual-phase4-pbr-normals
git pull

cargo clippy --workspace --all-targets -- -D warnings
cargo test -p approved_tests --features rig lighting
cargo run -p launcher --features probes -- sim probes --only "light probes,shader bounds,klm lightmap,texture sort key,pbr material"
```
Confirm `unclamped_reads` and `pbr material` pass with zero warnings/errors.

### 2. Vulkan Shader & Multi-Tier Runtime Smoke Check
Confirm `odyssey.wgsl` compiles cleanly on Vulkan across all modes (`Classic`, `Enhanced Baked`, `Enhanced Ultra + RAY_QUERIES`, and `DLSS`), with zero Naga validation errors or device lost events:
- Classic mode (Enhanced off): verify bit-identical appearance (zero normal perturbation / zero GGX / additive envmap preserved).
- Enhanced mode with `KJK_PBR=off` vs. default `KJK_PBR=on`.

### 3. Directional GI Rebake (`<name>.dir.png`) & `kjk lighting pack`
Re-bake at least one indoor metallic/bumped module and one outdoor sun module (e.g., `101per`, `003ebo`, `tar_m02af`, `tat_m17aa`, `danm13`) to generate `<lightmap>.dir.png` alongside `.klm` and `probes-hybrid.txt`:
```powershell
cargo run --release -p launcher -- detail-bake <module> --lights ...
```
Confirm:
- `<lightmap>.dir.png` files exist in `b/`.
- `kjk lighting pack` packs `.dir.png` files cleanly and `bake_fingerprint` matches `Whole` (`set_lightmaps` ignores `*.dir.png`).
- Older Phase 2/3 bakes without `.dir.png` still load and show GGX live fitted specular + normal maps cleanly.

### 4. Visual A/B Captures (`KJK_PBR=off` vs `KJK_PBR=on`) & Frame Time Check
Capture `8`-scene visual baseline and/or A/B pairs on:
- **`101per` / `end_m01aa`:** metallic wall panels, floor grates, and droids (`ENVMAP` + `bumpmaptexture` + live fitted GGX + `.dir.png` relief).
- **`003ebo` (Ebon Hawk):** brushed hull/console specular and normal relief under warm/cool interior fixtures.
- **`tar_m02af` (Taris Cantina):** moody metallic reflections without washed-out additive glow.
- **`tat_m17aa` / `danm13`:** crisp sunlight specular on metallic armor, droids, and sandstone relief; zero wall light-bleed on shadowed interiors.

Compare average FPS / GPU frame time in `101per` and `tat_m17aa` between `KJK_PBR=off` and `KJK_PBR=on`.

---

## Sign-off
Report any WGSL/Vulkan issues, visual artifacts (over-bright envmaps, bump inversion, seam artifacts on `.dir.png`), or frame-time regressions before merging/cutting Phase 5 (SSR/Planar Reflections, Volumetric God Rays, Contact Shadows & Modern Water).
