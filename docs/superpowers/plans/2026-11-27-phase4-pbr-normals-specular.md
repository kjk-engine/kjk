# Phase 4: PBR Material Response, Normal Maps, Live Specular & Directional GI

**Date:** 2026-11-27  
**Branch:** `feat/visual-phase4-pbr-normals` (cut from verified Phase 3 `ac766dc0f`)  
**Status:** Implemented on macOS (M4 Pro); awaiting Windows RTX 4070 Ti SUPER Checkpoint #4 validation and captures.

---

## Goals & Design Principles

1. **Zero Classic Regression:** Classic mode (`lights.enhanced == 0u`) stays 100% unchanged — original additive chrome, original water-only bump mapping, zero GGX or normal perturbation.
2. **A/B Verification Switch:** `KJK_PBR=off` disables Phase 4 normal perturbation, GGX specular, energy-conserving IBL, and directional GI modulation inside Enhanced mode (`PBR_BIT` in `LightList::enhanced`) so Windows captures can compare Phase 3 vs. Phase 4 cleanly.
3. **100% Backwards-Compatible Assets:**
   - Works out-of-the-box on vanilla KOTOR / TSL assets via existing `bumpmaptexture`, `bumpmapscaling`, and `ENVMAP` (`1.0 - texel.a` mask) TXI/texture metadata.
   - Optionally loads community Override companion textures `<diffuse>_n` (RGB tangent-space normal map) and `<diffuse>_rm` (R = roughness, G = metalness, B = AO/cavity) silently without triggering missing-texture warnings.
   - Works on both existing `.klm` / `.png` bakes (graceful fallback when no `.dir.png` exists) and re-baked Phase 4 `.dir.png` directional GI lightmaps.
4. **Metal / Vulkan Limit & Performance Discipline:**
   - Added 3 fragment-only texture bindings (`@binding(21)` `bump_normal_texture`, `@binding(22)` `pbr_rm_texture`, `@binding(23)` `dir_lightmap_texture`) reusing `base_sampler` and `lightmap_sampler` -> **0 new samplers** and **0 new vertex buffer slots** (preserving Metal's 31 vertex buffer slot limit).
   - Baked room fitted-light GGX specular skips ray queries (`no_casters()`) and masks against baked irradiance luminance (`room_spec_vis`) to prevent wall bleed at zero ray-trace cost.

---

## Architecture Breakdown

### 1. Texture Metadata & Companion Asset Plumbing (`asset_odyssey`, `game/src/{viewer,material,scene,rolloff}.rs`)
- **`crates/asset_odyssey/src/texture.rs`:**
  - `TextureFacts` now parses `bumpmapscaling` (`bump_scale: Option<f32>`) and `isbumpmap` (`is_bumpmap: bool`).
  - Added `looks_like_tangent_normal_map(&TextureIr)` (blue-dominant channel check on mip 0) to distinguish RGB tangent-space normal maps shipped by HD mod packs from BioWare's grayscale heightmaps.
- **`crates/game/src/viewer.rs` & `scene.rs`:**
  - `textures()` now loads `bumpmaptexture` for **all** diffuse materials (not only `WATER`), plus optional `<diffuse>_n` and `<diffuse>_rm` companions.
  - `BakeFolder` discovers `<lightmap>.dir.png` files alongside `.klm`/`.png` without adding them to `BakeFolder::lightmaps` or `bake_fingerprint::set_lightmaps`.
  - `SceneBuilder::mesh()` attaches `bump_normal`, `pbr_rm`, and `dir_lightmap` (`HAS_BUMPMAP`, `HAS_NORMAL_MAP`, `HAS_PBR_RM`, `HAS_DIR_LIGHTMAP`, storing `bump_scale` in `OdysseyParams::glow.y`).
- **`crates/game/src/material.rs` & `rolloff.rs`:**
  - `LightList::enhanced` bit `PBR_BIT = 4` is set when `lighting.enhanced() && crate::material::pbr_enabled()`.

### 2. All-Mesh Normal Mapping & GGX Microfacet PBR (`crates/game/src/odyssey.wgsl`)
- **`compute_shading_normal`:** Builds screen-space cotangent frame (`dpdx`/`dpdy` on `world_position` & `uv`). Evaluates either decoded tangent-space normals (`HAS_NORMAL_MAP`) or mip-0 height differences (`HAS_BUMPMAP`) scaled by `params.glow.y * BUMP_STRENGTH`. `geom_normal` is preserved for shadow-ray offsets (`surface_normal`), `near_casters`, and `room_shadow`.
- **`derive_pbr` & `ggx_spec`:**
  - Reads explicit `(roughness, metalness, cavity)` from `_rm` (`HAS_PBR_RM`), or infers `(roughness, metalness)` from `ENVMAP` reflectivity mask `1.0 - texel.a`, defaulting matte dielectrics to `(roughness = 0.82, metalness = 0.0, F0 = 0.04)`.
  - Evaluates Cook-Torrance GGX (`D_GGX * G_Smith * F_Schlick * ndotl`) across:
    1. Live **fitted lights** (both hybrid characters/objects and baked rooms).
    2. Live **Sun 1 + Sun 2** (masked by `sun1_vis`/`sun2_vis` on hybrid models and `.klm` `sunlit` alpha on baked rooms).
    3. Live **combat lights** (`fight_highlights_pbr` for saber/blaster specular across matte and metallic surfaces).
    4. **Directional GI** (`HAS_DIR_LIGHTMAP`) soft indirect specular lobe.
  - Energy conservation in Enhanced PBR mode scales diffuse albedo by `(1.0 - metalness * 0.72) * cavity` and weights `ENVMAP` reflections by Fresnel (`f_ibl`) and local irradiance luminance (`env_vis`) instead of unattenuated gamma-additive washing out.

### 3. GPU Directional GI Baker (`crates/launcher/src/detail_bake/gpu.wgsl`, `gpu.rs`, `crates/launcher/src/detail_bake.rs`)
- Expanded bake compute `results` stride from 3 to 4 `vec4<f32>` per point (`64` bytes/point).
- Accumulated luma-weighted incident direction vectors across:
  - Direct fitted lights (`MODE_DIRECT_SOFT`)
  - Suns & sky hemisphere rays (`MODE_SKY`)
  - Multi-bounce diffuse + emissive GI (`MODE_BOUNCE_NEW`, both lamp and sun bounces)
- Smoothed across planar surface neighbors (`smooth_values`), transformed from game space `(x, y, z)` to Bevy world space `(x, z, -y)`, island-dilated (`asset_odyssey::lightmap::dilate_invalid`), and written alongside each `.klm` as `<lightmap>.dir.png` (`RGB = dir * 0.5 + 0.5`, `A = |sum_dir| / total_luma`) via `asset_odyssey::lightmap::encode_dir_pixel`.
- Runtime modulation in `odyssey.wgsl` adjusts baked diffuse irradiance by `mix(1.0, clamp(ndotl_shade / ndotl_geom, 0.35, 1.85), dir_str * 0.85)` so bumped normal maps sculpt across baked room lighting.
