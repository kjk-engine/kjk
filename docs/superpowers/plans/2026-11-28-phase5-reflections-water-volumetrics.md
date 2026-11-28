# Phase 5: Reflections, Modern Water, Volumetrics & Contact Grounding

## Summary

Phase 5 adds four synergistic Enhanced-mode rendering upgrades on top of the Phase 1–4 linear HDR + PBR pipeline while keeping Classic rendering 100% bit-identical (`lights.enhanced == 0`):

1. **Screen-Space Contact Shadows (`crates/game/src/odyssey.wgsl`):**
   - 8-step temporally jittered screen-space raymarch against `depth_prepass_texture` along the dominant direct light direction (`Sun 1`, `dir_lightmap` dominant incident direction, `room_lights` probe direction, or overhead).
   - Grounds boots, droid legs, furniture feet, crates, and crevices onto floors and walls across both Baked and Ultra tiers without touching Classic or emissive surfaces (`!is_room_emissive`).
   - Respects `view.main_pass_viewport` under native, TAA, kjk Upscaler, and DLSS.

2. **Modern Physical Water (`crates/game/src/odyssey.wgsl`, `crates/game/src/legacy_blend.rs`, `crates/game/src/material.rs`):**
   - Routes `WATER` surfaces out of the gamma-encoded `legacy_blend` pass (`MODERN_WATER` shader def) so water renders in true linear HDR directly onto `ViewTarget`.
   - Dual-layer counter-scrolling bump normals + subtle world-space animated capillary wave motion (`globals.time`).
   - Depth-prepass thickness (`bg_depth - water_depth`) for Beer-Lambert depth absorption, zero-clipping shoreline feathering, and animated shore contact foam.
   - Schlick Fresnel (`F0 = 0.02`), cube envmap reflections modulated by area brightness, and HDR GGX specular from Twin Suns, fitted room lights, and combat/saber lights.

3. **Localized Screen-Space Reflections (`crates/game/src/ssr.rs`, `crates/game/src/ssr.wgsl`):**
   - Runs as `Core3dSystems::MainPass` (`SsrPass`) immediately after `main_opaque_pass_3d` and before transparents / `heat_haze` / `legacy_blend`.
   - Opaque smooth, metallic, or env-mapped surfaces (`OPAQUE_SURFACE`) pack `(r_bin << 4 | s_bin) + 2.0` (`[3..257]` exact `f16` integer) into `ViewTarget.a`. Non-reflective stone, sand, and skin keep `alpha <= 1.0` and skip SSR at near-zero cost.
   - `ssr.wgsl` executes a 20-step exponential raymarch + 4-step binary refinement against `depth_prepass_texture` and `normal_prepass_texture`, evaluates Fresnel, edge/distance/roughness/backface fades, blends linear HDR reflections, and restores `alpha = 1.0`.

4. **Volumetric Fog / Light Shafts & Soft Lit Smoke Particles (`crates/game/src/volumetrics.rs`, `crates/game/src/volumetrics.wgsl`, `crates/game/src/odyssey.wgsl`):**
   - Runs in `Core3d` after `Core3dSystems::MainPass` and before `Core3dSystems::EarlyPostProcess` (before TAA / kjk Upscaler / DLSS so the 16-step temporally jittered raymarch is temporally denoised at render resolution).
   - Driven by Twin Suns (`LoadedScene.probes.sun` / `sun2`) with screen-space depth occlusion probes (crisp outdoor sun shafts through arches/canopies) and up to 16 nearest luminous fitted/saber/level lights (soft indoor light cones with Henyey-Greenstein forward scattering), conservatively clamped (`<= 0.42`) so dark Peragus/Nar Shaddaa tunnels remain atmospheric and moody without washed-out haze.
   - Transparent non-water particles receive smooth depth-difference alpha fade (`smoothstep(0.0, 0.42, bg_dist - frag_dist)`) and subtle ambient tinting on `VERTEX_ALPHA` smoke/gas so dark rooms don't show glowing grey cards or harsh wall intersection seams.

## Feature & Debug Toggles

| Env Var | Default | Purpose |
|---|---|---|
| `KJK_PHASE5` | `on` | Master Phase 5 switch (`off` or `0` disables all Phase 5 features). |
| `KJK_CONTACT_SHADOWS` | `on` | `off` disables screen-space contact shadows. |
| `KJK_WATER` | `on` | `off` reverts water to Classic `water_shine` + `legacy_blend`. |
| `KJK_SSR` | `on` | `off` disables SSR pass and opaque alpha mask packing. |
| `KJK_SSR_DEBUG` | `0` | `1` visualizes isolated SSR contribution. |
| `KJK_VOLUMETRICS` | `on` | `off` disables volumetric light shafts & cones. |
| `KJK_VOLUMETRICS_STRENGTH` | `1.0` | Scalar multiplier for volumetric scattering intensity. |
| `KJK_VOLUMETRICS_DEBUG` | `0` | `1` visualizes isolated volumetric scattering. |
| `KJK_SOFT_PARTICLES` | `on` | `off` disables depth-softened & room-lit particles. |
