# KJK Next-Generation Lighting & Visuals Roadmap

Date: 2026-11-27

## End-State Architecture (3 Tiers)

| Tier | Target Hardware | Lighting & Materials | Mod / Area Coverage |
|---|---|---|---|
| **Classic** | Everything | Untouched 2003 gamma-encoded path (`Rgba8Unorm`, `Tonemapping::None`), zero extra cost | 100% |
| **Enhanced (Baked)** | GTX 1050 Ti / Steam Deck / non-RT GPUs | Linear HDR (`Rgba16Float`) + AgX/ACES Filmic Tonemapping + PBR/Normals/Specular + BC6H Baked Fitted-Light GI + Hybrid Character Lighting + XeGTAO + SSR/Probes + Volumetrics & Water | Baked areas + per-lightmap fallback |
| **Enhanced (RT / Next-Gen)** | RTX / RDNA2+ / Apple Silicon M3+ | Same Linear HDR + PBR + Volumetrics & Water, plus **live** fitted-light RT shadows (ReSTIR DI), RT reflections, and live RTGI (DDGI) — no pre-baked download needed | 100% (including Dune Sea & all mods) |

## Dependency Flow

```text
Phase 0 (Stabilize + 8-Scene Baseline)
   │
   ▼
Phase 1 (Linear HDR + Two-Texture Legacy Blends + Filmic Tonemapping)
   │
   ▼
Phase 2 (Linear Baker + BC6H Padded Mips + Fix Double/Triple Room Shadows)
   │
   ▼
Phase 3 (Hybrid Character Lighting: Indirect-Only 3D Probes + Live Fitted Lights)
   │
   ▼
Phase 4 (PBR Material Response + Bump/Normal Maps + Live Specular + Directional GI)
   │
   ├──────────────────────────┐
   ▼                          ▼
Phase 5 (Reflections,      Phase 6 (RT Tier: Live Fitted
 Water, Volumetrics,        ReSTIR DI + DDGI/RTGI)
 Contact Shadows)
```

---

## Phase 0 — Stabilize, Freeze Tuning & Build a Visual Baseline

- **Freeze per-module `bake.toml` art tweaks** until Phase 1 (Linear HDR) and Phase 2 (linear baker & shadow fixes) land so modules are not tuned twice on non-physical math.
- **Cold-cache stability:** `#722` (`hold_scene` on `warmup::LAYER`) already holds scene cameras during pipeline warm-up; close the remaining Ultra gap so `ray_structures::build` stays idle while `lifecycle_view::SceneHold` is active.
- **Grass lightmap gain:** already landed in `#799` (`crates/game/src/grass.wgsl`).
- **8-Scene Visual Baseline & Regression Harness (`tools/visual-baseline/`):** repeatable headless capture + `kjk compare-pictures` + GPU timing summary across 8 canary scenes (`end_m01aa`, `tar_m03ae`, `danm13`, `tat_m17aa`, `manm26aa`, `101per`, `903mal`, `tat_m18aa`).

## Phase 1 — True Linear HDR Pipeline + Filmic Tonemapping (Enhanced Only)

See detailed plan: `docs/superpowers/plans/2026-11-27-phase1-linear-hdr-tonemapping.md`.

- Convert Enhanced's scene camera target from gamma-encoded values in `Rgba16Float` to true linear HDR (`pow(2.2)` / sRGB display encode at tonemap).
- Preserve KOTOR/JA legacy blended particles, skies, smoke, and `Modulate` decals via the two-texture `(accum_colour, transmittance)` composite pass (preventing the Step 9 Dune Sea purple-sky regression) while keeping HDR combat glows (`OdysseyMaterial::glow`) additive in linear space.
- Run TAA/upscaler/DLSS and Bloom natively in linear HDR (retiring `linear_frame.rs`).
- Replace `rolloff.wgsl` (`KNEE = 0.9` per-channel exponential clamp) with AgX/ACES filmic tonemapping + display sRGB encode, and move `sharpen.rs` after tonemapping.

## Phase 2 — Fix Baker Radiometry, Lightmap Storage & Baked-Room Shadows

- **Linear baker math (`detail_bake.rs`, `detail_bake/gpu.rs`, `detail_bake/gpu.wgsl`):**
  - Remove `direct(sum) = (sum * 0.8)^2.2` and `odyssey.wgsl`'s `decode(fight_added)` so lights, suns, sky, and 3-bounce GI accumulate linearly.
  - Remove `WRAP = 0.3` from `detail_bake/gpu.wgsl` so direct light respects surface normals and 3-bounce GI provides natural fill.
- **BC6H HDR lightmaps + island-padded mipmaps:**
  - Replace halved 8-bit PNGs (`BAKED_GAIN = 2.0`) with BC6H HDR (8 bpp, ~4x smaller in VRAM than uncompressed `Rgba8` even with mipmaps) and island-aware padded mips.
  - Delete `BAKED_GAIN = 2.0` / `BAKED_GAIN_PROBES = 2.0` / `stored.txt` halving workarounds, and encode up to 2 sun-visibility channels cleanly.
- **Unify baked-room dynamic shadows (`odyssey.wgsl`, `shadows.rs`):**
  - Stop baked static placeables (`ShadowBox`) from casting a second/third dynamic shadow onto baked rooms (keep them for combat lights and unbaked areas).
  - Remove BioWare's legacy lamp loop from `room_shadow` on `baked_room`, drive dynamic shadows from fitted lights + baked sun(s) only, and route `fitted_visibility` through Ultra ray queries / High shadow tiles instead of capsule fallback.

## Phase 3 — Unified Hybrid Lighting for Characters, Doors & Dynamic Objects

- **Indirect-only 3D light probes:** bake bounce + sky irradiance directly in the GPU baker across a multi-height / sparse 3D grid with trilinear interpolation (keeping floor-readback probes as the fallback for unbaked/modded areas).
- **Live direct fitted lights + sun on dynamic models:** shade characters, doors, and unbaked placeables directly from `fitted_lights` + `bake.toml` sun(s), removing BioWare's legacy diffuse lamps in baked areas and pruning `PROBE_FLOOR` / `GLOW_BY_ROOM` workarounds.
- Complete tolerant geometry matching for re-saved MDLs (`2026-11-26-lighting-check-mod-compat.md` Step 7).

## Phase 4 — PBR Material Response, Normal Maps & Specular

- Enable KOTOR's existing `bumpmaptexture` height-to-tangent-normal shading on all meshes (not only `WATER`).
- Promote BioWare's envmap alpha mask (`1.0 - texel.a`) and material properties into a GGX microfacet PBR model (roughness, metalness, Schlick Fresnel) evaluated against live fitted lights, suns, and combat lights.
- Store a directional lightmap term (dominant direction or L1 SH) alongside diffuse GI so normal maps react to baked room lighting.
- Support optional Override companion PBR textures (`_n`, `_rm`) and subtle micro-detail normals.

## Phase 5 — Reflections, Modern Water, Volumetrics & Contact Grounding

- **Localized Reflections:** replace flat additive global cubemaps with Fresnel-weighted PBR IBL from per-room parallax-corrected reflection probes + Screen-Space Reflections (and 1-spp denoised RT reflections on Ultra).
- **Modern Water Shader:** depth-based absorption/refraction, dual-layer animated normals, Fresnel, sun/lamp specular, SSR/probe reflections, and shore contact foam.
- **Volumetric Fog, Light Shafts & Lit Soft Particles:** froxel volumetric fog / shafts driven by suns, roof skylights, and bright fitted lights, plus room-lit depth-softened smoke/gas particles.
- **Screen-Space Contact Shadows:** fast depth-buffer raymarch for grounding feet, crates, and props.

## Phase 6 — Real-Time RT Tier (Live Fitted Lights + Dynamic GI)

- **Live RT direct lighting & shadows (ReSTIR DI / ray queries):** shade rooms and characters live from `fitted_lights` + sun(s) with temporal/spatial resampling, supporting lightmap-less areas (Dune Sea) and dynamic doors/mods with zero bakes.
- **Live RT Global Illumination (DDGI / ReSTIR GI):** real-time indirect bounce from opening doors, lightsabers, blasters, and Force powers.
- **Unified Enhanced settings:** keep Enhanced (Baked) for non-RT GPUs and Enhanced (RT) for RT-capable hardware sharing the exact same PBR, Linear HDR, and post-processing pipeline.
