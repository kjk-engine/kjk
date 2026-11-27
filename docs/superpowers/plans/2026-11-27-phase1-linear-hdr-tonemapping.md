# Phase 1 Implementation Plan: True Linear HDR + Two-Texture Legacy Blends + Filmic Tonemapping

Date: 2026-11-27  
Prerequisite: Phase 0 (`tools/visual-baseline/` baseline captured under `classic` and `enhanced`).  
Scope: **Enhanced** and **Enhanced Lightmaps** (`LightingMode::Enhanced` / `LightingMode::Baked`). **Classic** (`LightingMode::Classic`) stays bit-identical on its 8-bit gamma-encoded `Rgba8Unorm` path.

---

## 1. Why Phase 1 Comes Before Re-Baking or PBR

Today, Enhanced mode renders into an `Rgba16Float` target (`Hdr`) that still stores **gamma-encoded** values (`scene::encoded_view`, `rolloff.rs`):
- `odyssey.wgsl:2486` computes `decode(base.rgb) * decode(fight_added)` even though `fight_added` is already a linear sum of falloffs, applying a `^2.2` curve to fight lights.
- `rolloff.wgsl:16` clamps per-channel above `KNEE = 0.9` with `min(v, 0.9) + 0.1 * (1 - exp(-(v - 0.9) / 0.1))`, causing hue shifts on bright lights and providing no filmic contrast curve.
- `bevy_post_process::bloom` runs on gamma-encoded values (`threshold: 1.0`).
- `sharpen.wgsl` runs before `rolloff` on unclamped values.
- As `docs/LIGHTING-PLAN.md` Step 9 proved, naive linear blending breaks KOTOR/JA's legacy transparent layers (e.g., Dune Sea's `Fx_GroundSand3x` sand particles turning the sky grey-purple under `KJK_LINEAR=on`), while `linear_frame.rs` (`KJK_LINEAR=temporal`) leaves the main pass gamma-encoded so no linear lighting, reflection, or volumetric pass can run between opaque and transparent draws.

Phase 1 converts Enhanced into a true **linear HDR** pipeline while preserving legacy blended layers exactly via Step 9's two-texture `(accum_colour, transmittance)` composite design.

---

## 2. Color-Space Convention & Runtime Gating Rules

### 2.1 Pure Power `2.2` Internally, sRGB at Display Tonemap Output
As measured in `docs/LIGHTING-PLAN.md` Step 9:
- BioWare's textures and lightmaps are stored without hardware sRGB flags (`Rgba8Unorm` / BC1 / BC3).
- Under a pure power `γ = 2.2` (`pow(max(v, 0.0), vec3(2.2))`), `pow(a * b, 2.2) == pow(a, 2.2) * pow(b, 2.2)` is **exact**, and the `Modulate` blend (`2 * src * dst`) preserves its algebraic trick under `pow(2.2)` (whereas the piecewise sRGB curve shifts dark texture × lightmap products by up to 7/255).
- **Rule:**
  - Inside `kjk::lighting` (`crates/game/src/lighting.wgsl`), provide `decode_22(v)` (`pow(max(v, 0.0), vec3(2.2))`) and `encode_22(v)` (`pow(max(v, 0.0), vec3(1.0 / 2.2))`) for Enhanced's surface math and two-texture legacy composite.
  - Keep `decode()` / `encode()` (piecewise sRGB) for final display conversion after tonemapping (and for `ui_composite` / `video_effect` compatibility).

### 2.2 Runtime Gating (Fixing Step 9's DLSS Mode-Switch Bug)
In Step 9, gating decode/encode via pipeline shader defs broke runtime Classic ↔ Enhanced switching under DLSS because the pipeline key didn't change.
- **Rule:** Gate all linear-vs-gamma shader behavior via runtime uniform/storage flags (`lights.enhanced != 0u` in `odyssey.wgsl`, a uniform flag in `grass.wgsl`, `glow_shell.wgsl`, `shell.wgsl`, and `rolloff` / `linear_composite` camera component presence via `LightingMode::enhanced() && camera.hdr`), **never** via static pipeline shader defs that drift on live mode toggles.

---

## 3. Target Render Graph Order (Core3d, Enhanced Only)

```text
1. Prepass (Depth / Normals / Motion Vectors)
2. XeGTAO (`ambient_occlusion.wgsl`)
3. Opaque Main Pass (`odyssey.wgsl`, `grass.wgsl`, `glow_shell.wgsl` depth_only)
   └─► Target: ViewTarget (`Rgba16Float`), cleared to linear fog (`pow(fog_srgb, 2.2)`).
   └─► Writes true linear HDR light (opaque diffuse, lightmaps, fitted/combat lights,
       combat highlights, AO, linear fog). Values may exceed 1.0.
4. Legacy Blended Pass -> Two-Texture MRT + Linear Composite (TAA / Upscaler / DLSS)
   ├─► 4a. Legacy blended surfaces (`Blend`, `BlendVertex`, `Additive`, `AddOne`,
   │       `Lighten`, `Modulate`, `glow_shell` glows/marks/holograms, `shell`,
   │       and `effects_quality` half-res particles) render into:
   │       - `accum_colour` (`Rgba16Float`, clear `0.0`)
   │       - `transmittance` (`Rgba16Float`, clear `1.0`)
   ├─► 4b. Composite pass over `ViewTarget` (linear HDR):
   │       Reads `bg_linear`, encodes `bg_gamma = encode_22(clamp(bg_linear, 0.0, 1.0))`,
   │       preserves `bg_over = max(bg_linear - 1.0, 0.0)`,
   │       applies `blended_gamma = transmittance.rgb * bg_gamma + accum_colour.rgb`,
   │       and writes `decode_22(blended_gamma) + bg_over * transmittance.rgb`.
   └─► 4c. Enhanced HDR Glows & Distortions:
           - `OdysseyMaterial::glow` > 1.0 (sabers, bolts, flashes) draw directly
             to `ViewTarget` with linear additive blending so bloom & tonemap
             receive true HDR radiance (> 1.0).
           - Distortions (`heat_haze.wgsl`, `refraction.wgsl`, `glow_shell` distort)
             run after composite on the linear `ViewTarget` (`screen_copy`).
5. `EarlyPostProcess`: TAA / `upscale.rs` / DLSS (natively on linear HDR; retire `linear_frame.rs`)
6. `Bloom` (`bevy_post_process::bloom`) on linear HDR
7. `PostProcess` — **Filmic Tonemap + sRGB Encode (`rolloff.rs` / `rolloff.wgsl`)**
   └─► AgX (default, with `KJK_TONEMAP=agx|agx_punchy|aces|rolloff` for A/B) + sRGB encode -> `[0, 1]`
8. `Sharpen` (`sharpen.rs` / `sharpen.wgsl`)
   └─► Runs **after** tonemapping + sRGB encode in perceptual `[0, 1]` space (prevents HDR CAS ringing).
9. `VideoEffectPass` (`video_effect.rs`) -> `UiComposite` (`ui_composite.rs`)
   └─► Unchanged; both receive `[0, 1]` gamma-encoded world frame.
```

---

## 4. Detailed Design per Subsystem

### 4.1 Opaque Linear Shading & Clear Color (`scene.rs`, `odyssey.wgsl`, `grass.wgsl`)
- **Clear color (`scene::clear_to_fog`):**
  - `scene.rs:2044` already uses `Color::srgb(red, green, blue)`, which Bevy automatically linearizes when clearing an `Hdr` (`Rgba16Float`) target. Verify under both Classic (`Rgba8Unorm`, `CompositingSpace::Srgb`) and Enhanced (`Rgba16Float`, `Hdr`) that the clear color matches `decode_22(fog_color)` on screen (adjust clear color or `fog()` in `odyssey.wgsl` / `grass.wgsl` so distant geometry blends seamlessly into the clear color with zero horizon seam).
- **`odyssey.wgsl` (`enhanced` branch):**
  - Decode `base_lin = decode_22(base.rgb)` and `lightmap_lin = decode_22(lighting)` (for opaque surfaces under Enhanced).
  - Remove the broken `decode(base.rgb) * decode(fight_added)` on line 2486; `fight_added` is already linear, so simply add `base_lin * fight_added + fight_highlights(...) * shiny`.
  - Compute `fog(...)` in linear space using `decode_22(params.fog_color.rgb)` (note: Step 9 observed linear fog appears slightly thicker in mid-distance; verify against `clear_to_fog` and tune only if needed, or preserve exact perceptual fog ramp `mix(colour, decode_22(fog_color), amount)` vs `decode_22(mix(encode_22(colour), fog_color, amount))` — document as a tunable switch during A/B).
  - Remove redundant `encode(decode(colour) + extra)` round-trips; output linear HDR `vec4<f32>(colour_lin, alpha)` directly.
  - Keep the `!enhanced` (`Classic`) branch 100% untouched.
- **`grass.wgsl`:**
  - Add a runtime `enhanced` flag in `GrassParams` (for example in `shape.w` flags bit `8`), so when Enhanced is active `fragment()` outputs `decode_22(...)` linear light (with linear flash and linear fog) without breaking Classic or runtime mode switches.

### 4.2 Two-Texture Legacy Blend Pass (`legacy_blend.rs` + `material.rs` / `effects_quality.rs`)
Why this works cleanly (Step 9 proof): every legacy blend in `material.rs:1925` transforms background `B` into `T * B + A`:
- `Blend` / `BlendVertex` (`src_alpha`, `1 - src_alpha`): `T = vec3(1.0 - src.a)`, `A = src.rgb * src.a`
- `Additive` / `Lighten` (`src_alpha`, `1`): `T = vec3(1.0)`, `A = src.rgb * src.a`
- `AddOne` (`1`, `1`): `T = vec3(1.0)`, `A = src.rgb`
- `Modulate` (`dst`, `src`): `T = 2.0 * src.rgb`, `A = vec3(0.0)`
- `Hologram` (`1`, `src_alpha` in `glow_shell.rs:144`): `T = vec3(src.a)`, `A = src.rgb`

Because composition of affine maps $(T_2, A_2) \circ (T_1, A_1)$ is associative—transforming $(T_1, A_1)$ to $(T_2 \cdot T_1,\; T_2 \cdot A_1 + A_2)$—hardware blending into two `Rgba16Float` attachments simultaneously (`@location(0) accum_colour`, `@location(1) transmittance`) computes the exact stacked result in one pass with zero frame copies:

| Surface | `@location(0) accum_colour` `(src, dst)` | `@location(1) transmittance` `(src, dst)` | Fragment Output Note |
|---|---|---|---|
| `Blend` / `BlendVertex` | `(SrcAlpha, OneMinusSrcAlpha)` | `(Zero, OneMinusSrcAlpha)` | Standard RGB + alpha |
| `Additive` / `Lighten` | `(SrcAlpha, One)` | `(Zero, One)` | Standard RGB + alpha |
| `AddOne` | `(One, One)` | `(Zero, One)` | Standard RGB |
| `Hologram` | `(One, SrcAlpha)` | `(Zero, SrcAlpha)` | Standard RGB + `Holotex` alpha |
| `Modulate` | `(Zero, Src)` | `(Zero, Src)` | Fragment outputs `2.0 * src.rgb` into unclamped `Rgba16Float`, multiplying both `accum` and `transmittance` by `2 * src` |
- **Enhanced HDR Glowing Draws (`OdysseyMaterial::glow.x > 1.0`):**
  - Lightsaber blades, blaster bolts, and muzzle flashes should **not** go into the gamma-clamped legacy blend pass — they are physical light emitters! They draw in the linear HDR pass (after composite, depth-tested against the scene depth buffer) with linear additive blending (`(One, One)` or `(SrcAlpha, One)` in linear light) so their radiance stays > 1.0 for Bloom and AgX highlight rolloff.
- **Interaction with `effects_quality` (Effects Quality = Low):**
  - `effects_quality.rs` already uses the exact same `(colour, alpha)` two-part factorization for half-res emitter particles! Under Enhanced, its half-res composite feeds into the same linear pipeline cleanly (or runs after opaque and before HDR glows).
- **MSAA Policy:**
  - Two-texture composite requires reading the resolved `ViewTarget` color. Under MSAA (if a user selects MSAA 4 + Enhanced), either:
    - *(Option A, recommended)* Resolve MSAA before the legacy-blend composite and composite onto a single-sample float target, or
    - *(Option B)* Document that Enhanced's linear two-texture path runs under TAA / Upscaler / DLSS (which are single-sample) and MSAA keeps the Step 9 fallback or forces resolve-before-composite. We will validate Option A on Metal/Vulkan in Phase 1 Step 2.

### 4.3 Native Linear TAA / Upscaler / DLSS & Bloom
- **Retire `linear_frame.rs` (`KJK_LINEAR=temporal`):** Once `ViewTarget` holds linear HDR out of the main pass, Bevy's TAA, `upscale.rs`, and NVIDIA DLSS (`HighDynamicRange | AutoExposure`) automatically receive linear HDR directly with zero extra decode/encode passes (saving ~0.05–0.10 ms CPU/GPU).
- **Retuned Linear Bloom (`rolloff.rs`):**
  - In gamma space, `threshold = 1.0` triggered only above `1.0^2.2 = 1.0`, while `intensity = 0.25` added 25% of encoded bloom.
  - In linear HDR, lightsabers, bolts, and super-bright specular highlights / terminal tubes can sit at linear `1.5–12.0`. Start with `threshold = 1.0`, `threshold_softness = 0.2`, `intensity = 0.08..0.15` (using `BloomCompositeMode::EnergyConserving` or tuned `Additive`) and calibrate on `endar_corridor`, `taris_cantina`, and an active saber duel.

### 4.4 Filmic Tonemapping (`rolloff.rs` / `rolloff.wgsl`) & Post-Tonemap Sharpening (`sharpen.rs`)
- **Filmic Tonemapper (`rolloff.wgsl`):**
  - Replace the per-channel `KNEE = 0.9` exponential curve with **AgX** (inset/outset 3×3 matrices + 6th-order polynomial sigmoid, plus optional `agx_punchy` contrast curve, and `aces` / legacy `rolloff` behind `KJK_TONEMAP` for side-by-side contact sheets).
  - Why AgX over naive ACES/per-channel rolloff: AgX naturally desaturates extreme lightsaber/blaster/lamp cores toward white without shifting red/orange toward yellow or blue toward purple (classic ACES/per-channel artifacts), keeping KOTOR's saturated saber hues intact while rolling highlights smoothly.
  - Encode the `[0, 1]` tonemapped linear result to sRGB (`encode(tonemapped)`) so `VideoEffectPass` and `UiComposite` continue operating in `[0, 1]` perceptual space without changing UI or security-camera grades.
- **Move `Sharpen` after Tonemapping (`sharpen.rs`):**
  - Currently `sharpen` runs `after(EarlyPostProcess).before(PostProcess)`. Moving it to `in_set(Core3dSystems::PostProcess).after(RollOffPass).before(VideoEffectPass)` lets CAS operate on `[0, 1]` tonemapped values instead of unbounded HDR values, eliminating dark/bright ringing halos around glowing sabers and lamp strips.

---

## 5. Step-by-Step PR Breakdown

1. **PR 1A — Linear Opaque + Clear + Runtime Gating (`lighting.wgsl`, `odyssey.wgsl`, `grass.wgsl`, `scene.rs`):**
   - Add `decode_22`/`encode_22`, runtime uniform gating for `grass.wgsl`, linear opaque lighting in `odyssey.wgsl` (removing `decode(fight_added)`), and verify clear color / fog matching.
   - Measure Classic parity (`compare.py` must report 0.000 diff for Classic).
2. **PR 1B — Two-Texture Legacy Blend Pass + Linear Composite (`legacy_blend.rs`, `material.rs`, `glow_shell.rs`, `shell.rs`, `effects_quality.rs`):**
   - Implement the `(accum_colour, transmittance)` pass for legacy transparents + `Modulate` (`2 * src` output) + HDR combat glow separation.
   - Verify `dune_sea_sky` (`tat_m18aa`), `peragus_tunnels` (`101per`), and `taris_cantina` (`tar_m03ae`) on the Phase 0 harness.
3. **PR 1C — AgX Filmic Tonemapper + Linear Bloom + Post-Tonemap Sharpen + Retire `linear_frame.rs` (`rolloff.rs`, `rolloff.wgsl`, `sharpen.rs`, `linear_frame.rs`):**
   - Replace `rolloff.wgsl` with AgX + sRGB encode (with `KJK_TONEMAP` A/B switch), tune Bloom in linear space, reorder `sharpen` after `RollOffPass`, and remove `linear_frame.rs`.
   - Generate AgX vs AgX Punchy vs ACES vs old Rolloff contact sheets across all 8 baseline scenes for owner sign-off.

---

## 6. Verification & Acceptance Criteria

Run on the Phase 0 baseline harness (`tools/visual-baseline/capture.py` + `compare.py`) and `kjk bench`:
1. **Classic parity:** `compare.py` between pre-Phase-1 and post-Phase-1 under `classic` reports `mean = 0.000` across all 8 scenes.
2. **Dune Sea regression fixed:** `dune_sea_sky` (`tat_m18aa`) sky color and sand haze match intended blue/tan palette (no Step 9 grey-purple shift).
3. **Runtime mode toggle:** Switching Classic ↔ Enhanced ↔ Classic live under TAA/upscaler/DLSS leaves zero stale gamma/linear shader artifacts.
4. **HDR saber/bolt/lamp behavior:** Sabers, bolts, and bright lamps desaturate cleanly toward white cores with smooth bloom halos and zero CAS ringing halos.
5. **GPU budget:** Total post-process (`AgX tonemap + bloom + composite`) stays within **≤ 0.25 ms** on desktop and does not regress `arena_fight` / `peragus_hangar` on M4 Pro beyond the budget of the removed `linear_frame` + old `rolloff` passes.

---

## 7. Open Questions Requiring Owner Visual Sign-Off (in PR 1C)

1. **Tonemapper look:** AgX Default vs AgX Punchy vs ACES (chosen from an 8-scene contact sheet).
2. **Linear fog ramp:** Exact linear fog vs perceptual-matched fog curve in `danm13` / `tat_m18aa`.
3. **Bloom intensity & threshold** in linear HDR on active saber combat + `taris_cantina` neon signs.
