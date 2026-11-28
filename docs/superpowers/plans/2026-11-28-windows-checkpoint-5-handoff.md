# Windows Checkpoint #5 Handoff (`feat/visual-phase5-water-ssr-volumetrics`)

Run all visual & GPU verification on the Windows RTX 4070 Ti SUPER machine on branch `feat/visual-phase5-water-ssr-volumetrics`.

## 1. Automated Verification (Before / Alongside Visual Captures)

```powershell
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p approved_tests --features rig lighting
cargo run -p launcher --features probes -- sim probes --only "shader bounds,pbr material,phase5 visuals"
cargo xtask publish-check
```

## 2. Live Vulkan + RTX Shader & Pipeline Verification

Verify all new/updated shaders (`odyssey.wgsl`, `odyssey_rays.wgsl`, `ssr.wgsl`, `volumetrics.wgsl`) compile cleanly on Vulkan / RTX with checked shaders enabled:

```powershell
$env:KJK_CHECKED_SHADERS = "1"
# Boot Enhanced Ultra + DLSS scenes with active Phase 5
```

## 3. 8-Scene Classic vs Enhanced Baked vs Enhanced Ultra vs DLSS Captures

Capture and visually inspect all 8 baseline scenes (`tools/visual-baseline/capture.py`):
1. **Classic baseline (`--modes classic`):** Verify 100% pixel-identical parity with Phase 0–4 Classic (zero SSR, zero contact shadows, Classic water, zero volumetrics).
2. **Enhanced Ultra + DLSS (`--modes enhanced,baked`):**
   - **Dantooine (`danm13` / courtyard) & Manaan (`manm26aa`) & Lehon (`unk_m41aa`):**
     - Water shows dual-layer ripples, shoreline feathering + animated bank foam, Beer-Lambert depth absorption, Fresnel sky/cubemap reflections, and HDR GGX sun sparkles.
     - Archways/buildings cast subtle volumetric sun shafts (`KJK_VOLUMETRICS=on` vs `off`) without washing out contrast.
     - Character boots, droids, and courtyard props show crisp contact grounding shadows.
   - **Peragus (`101per`), Nar Shaddaa tunnels, Taris Cantina, Endar Spire (`end_m01aa`):**
     - Metallic floors, computer consoles, and polished panels reflect nearby corridor geometry & neon/lights (`SSR`), cleanly faded by roughness & screen edges, zero grain/flicker under DLSS.
     - Smoked/steam vents and gas particles fade smoothly where intersecting floors/walls (`KJK_SOFT_PARTICLES`).
     - Indoor fitted light cones & lightsaber volumetrics stay subtle and preserve deep atmospheric shadows (no grey ambient fog).

## 4. A/B Toggle & Debug Isolation Checks

Test each isolation knob to confirm clean toggle behavior and zero frame corruption:
- `KJK_SSR_DEBUG=1`: shows SSR only on smooth/metallic/env-mapped floors, panels, and armor; matte plaster/sand/skin stays pure black.
- `KJK_VOLUMETRICS_DEBUG=1`: shows sunlit shafts outdoors and subtle light-cone scattering indoors; shadow regions stay dark.
- `KJK_WATER=off`: water cleanly falls back through `legacy_blend`.
- `KJK_SSR=off` / `KJK_VOLUMETRICS=off` / `KJK_CONTACT_SHADOWS=off` / `KJK_SOFT_PARTICLES=off` / `KJK_PHASE5=off`: each disables cleanly at zero visual glitch.

## 5. Sign-Off Gate for Phase 6

Once Checkpoint #5 passes Vulkan shader compilation, 8-scene visual captures across Classic / Baked / Ultra / DLSS, and GPU frametime budget checks on RTX 4070 Ti SUPER, push any Windows fixes and cut Phase 6 (Real-Time RT Tier: Live Fitted Lights ReSTIR DI + Live RTGI / DDGI).
