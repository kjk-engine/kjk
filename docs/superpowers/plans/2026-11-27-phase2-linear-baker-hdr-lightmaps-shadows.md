# Phase 2 Implementation Plan: Linear Atmospheric Baker, `.klm` HDR Lightmaps & Unified Room Shadows

Date: 2026-11-27  
Prerequisite: Phase 1 verified cleanly at Windows Checkpoint #1 (`feat/visual-phase0-baseline`).  
Target Scope: **Enhanced Lightmaps** (`LightingMode::Baked`) + **Enhanced** (`odyssey.wgsl` ray/shadow unification). **Classic** remains untouched and bit-identical.

---

## 1. Artistic Direction & Goals

Current v1 bakes had two structural flaws besides gamma/clipping:
1. **Unintended multi-layer room shadows:** baked rooms (`baked_room`) still ran BioWare's legacy local lamp loop (`room_shadow`), fake `OVERHEAD_WEIGHT`, and static `ShadowBox` casters on top of baked shadows, while placeables cast dynamic shadows onto lightmaps that already baked them.
2. **Flat / patchy mood inside dark interior spaces:** `WRAP = 0.3` bled light around back-facing geometry (flattening bump/corner relief), `direct(sum) = (sum * 0.8)^2.2` + `GAIN = 2.0` PNG storage clipped near bright fixtures (`> 2.0`) and crushed near-black tunnels (7-bit banding on Nar Shaddaa / Peragus / Malachor), and `eased_level` / forced stored fills tried to match BioWare's flat room means instead of embracing real light falloff + 3-bounce indirect fill.

Phase 2 shifts baked areas to an **atmospheric, linear HDR pipeline**:
- **Moody, legible dark interiors (Peragus, Nar Shaddaa tunnels, tombs):** `WRAP = 0.0` sharpens directional fixture contrast and architectural silhouette, while true physical linear 3-bounce GI + optional clean ambient floor keeps shadows readable in AgX without flat grey wash or 7-bit dark-crush banding.
- **Crisp sunlit exteriors (Tatooine, Dantooine, Manaan):** unclamped HDR sun + sky radiance (beyond `2.0`), preserving bright daylight highlights for bloom and AgX filmic rolloff.
- **No requirement to replicate v1 flat brightness:** rooms can stay moody and contrasty where fixture placement dictates.

---

## 2. Format Decision: `.klm` (`Rgba16Float`, `lz4_flex`-Compressed, Island-Padded Mipmaps)

Rather than taking on a C++/ISPC encoder dependency (e.g., `intel_tex_2` / `bc6h`) or losing the alpha channel:
- **Extension & Magic:** `<map>.klm` (`KJK1` header: `u32` width, `u32` height, `u32` mip_count, followed by per-mip `lz4_flex`-compressed `[f16; 4]` pixel payloads in **linear light**).
- **Channel layout:**
  - `rgb`: true unclamped linear HDR light (`f16` — 1024 steps per EV stop down to near-black, `> 65504` max; zero clamping, zero `BAKED_GAIN = 2.0` halving).
  - `a`: exact `sunlit` visibility share (`0.0..=1.0`, matching BioWare/v1 semantics so `room_shadow` and `fitted_lights` know how much sun a texel receives with **zero extra texture binding**).
- **Island-aware padded mipmaps:** generates full mip chains (`max(w, h) -> 1`) via UV/island-mask validity dilation so lower mip levels never bleed black/background texels across UV chart seams.
- **Backwards compatibility:**
  - V1 `.png` bakes continue loading with existing gamma/2.0-gain decode.
  - V2 `.klm` bakes set a runtime material flag `LINEAR_LIGHTMAP`, telling `odyssey.wgsl` and `grass.wgsl` to skip `decode_22(lightmap * BAKED_GAIN)` and read `lightmap_texel.rgb` directly as linear HDR radiance!

---

## 3. PR Breakdown

### PR 2A — Unified Baked-Room Shadows & Ultra RT Fitted Shadows (`odyssey.wgsl`)
1. **Eliminate double/triple shadows on `baked_room`:**
   - Skip baked static placeable boxes (`near.box_at` / `RAY_PROP` placeables) for baked-light, sun, and fitted-light shadows when `baked_room` is true (keeping them active for `combat_lights` and unbaked rooms).
   - In `room_shadow(...)`, when `baked_room` is true (under `#ifdef FITTED_LIGHTS`), skip BioWare's legacy local room lamps (`radius < SUN_RADIUS`), skip `OVERHEAD_WEIGHT`, and trace dynamic character casters only against suns (`radius >= SUN_RADIUS`) weighted by `sunlit` + `contact_shade` (characters only).
2. **Hardware RT Fitted Shadows (`#ifdef RAY_QUERIES`):**
   - Route `fitted_visibility(...)` through `scene_rays` (`RAY_CASTERS`) under `shadows.rays != 0u` using `ray_noise()` jitter across `source`, skipping static `RAY_PROP` placeables on `baked_room`.

### PR 2B — `.klm` (`Rgba16Float`) Format, Padded Mips & Game Runtime (`asset_odyssey`, `scene.rs`, `viewer.rs`, `odyssey.wgsl`, `grass.wgsl`)
1. Implement `.klm` encoder/decoder + island-aware mip generator (`asset_odyssey::lightmap` / `Encoding::Rgba16Float`), mapping to Bevy `TextureFormat::Rgba16Float`.
2. Update `lighting_bakes.rs` (`shipped` recognizes `.klm`, format version / manifest handling) and `viewer.rs::detail_baked` to prefer `<name>.klm` when present (falling back to `.png`), tagging materials/grass with `LINEAR_LIGHTMAP`.
3. Update `odyssey.wgsl` and `grass.wgsl` to branch on `LINEAR_LIGHTMAP`:
   - `baked` is already true linear HDR (`no decode_22`, `no * 2.0`), split cleanly at `1.0` or added directly into `linear_output` (`lighting_lin = baked * ao + extra`).

### PR 2C — Linear Atmospheric GPU Baker & Pack Support (`detail_bake.rs`, `detail_bake/gpu.rs`, `gpu.wgsl`, `lighting_pack.rs`)
1. Set `WRAP = 0.0` in `detail_bake/gpu.wgsl` (`facing = max(dot(normal, toward), 0.0)`) and matching `Fitted::gives` on CPU.
2. Accumulate direct lights, 3-bounce indirect GI, sky/sun, and emissive glow linearly (`LIVE_GAIN = 1.0`, no `^2.2` distortion or `eased_level` knee clamp).
3. Replace artificial flat stored fills with configurable `--ambient-floor` (subtle readable shadow floor in deep indoor areas, `0.0` default for pure physical GI + explicit per-module overrides in `bake.toml`) and dynamic HDR intensity range.
4. Write `<map>.klm` with valid-texel mask dilation & mipmaps, and update `lighting_pack.rs` to package `.klm` lightmaps cleanly.

---

## 4. Verification Plan
- **MacBook (CPU + M4 Pro Metal):**
  - `cargo clippy --all-targets --features probes -- -D warnings`
  - `kjk sim probes` covering `shader bounds`, `.klm` lossless/bounds round-trip + padded mip seam invariance, and linear lightmap shader-flag logic.
  - Classic `0.000` parity check preserved.
- **Windows Checkpoint #2 (RTX 4070 Ti SUPER):**
  - Re-bake test subset (`101per`, Nar Shaddaa / `tar_m03ae`, `danm13`, `tat_m17aa`, `end_m01aa`) with `kjk detail-bake` outputting `.klm`.
  - Capture & inspect dark-tunnel contrast, contact shadows without double placeable darkening, Ultra RT fitted-light shadows (`scene_rays`), and VRAM/load-time metrics via `capture.py` + `compare.py`.
