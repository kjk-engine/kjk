# Phase 3: Unified Hybrid Character Lighting & Tolerant MDL Matching

Date: 2026-11-27  
Branch: `feat/visual-phase3-hybrid-lighting` (based on `origin/visual-phase2-verified` `1e2843978`)

---

## Summary of Changes

Phase 3 eliminates the legacy flat probe look on characters, companions, doors, and unbaked placeables by separating **baked indirect room GI** from **live direct lights & suns**, adding vertical 3D probe resolution, and tolerating minor MDL re-save drift in bake fingerprints:

### 1. Multi-Height Indirect-Only 3D Probes (`crates/launcher/src/detail_bake/gpu.rs`)
- `probe_places` places dual-height probe layers above walkable floors (`0.6 m` and `1.8 m`) spaced `1.0 m` apart, casting an upward ceiling check (`CEILING_CLEARANCE = 0.25 m`) so upper probes are skipped under low ceilings and lower probes clamp safely inside alcoves.
- `bake_lights` splits probe irradiance into:
  - **`probe_indirect_linear`:** sky hemisphere fill + sky/sun bounces + 3-bounce fitted-light bounces + emissive glow (scaled per room just like `.klm` texels), excluding direct fitted lights and direct suns.
  - **`direct_scale`:** each room's linear fitted-light scale multiplier so live direct shading on characters matches the `.klm` room walls/floors 1:1.
  - **`sun1_vis` / `sun2_vis`:** per-probe direct visibility to primary and twin suns (e.g., Tatooine `tat_m17aa`).
- Emits `probes-hybrid.txt` alongside `.klm` lightmaps in set `b/`, and updates `lighting_bakes::shipped` so `kjk lighting pack` includes `probes-hybrid.txt` (deflated) in lighting downloads.

### 2. 3D Vertical Probe Interpolation & Loader (`crates/game/src/light_probes.rs`, `crates/game/src/viewer.rs`)
- `RoomProbes::blend` performs separable 3D interpolation across `(x, y)` within `BLEND_REACH = 1.6 m` and vertical `z` within `BLEND_Z = 1.35 m` inside the same storey (`BLEND_STOREY = 1.5 m`).
- `ProbeState` caches per-model rest-mesh midpoint heights (`0.4 m` to `1.8 m`) so short crates/droids bias toward the lower probe layer while taller characters/Wookiees/doors sample higher.
- `viewer::baked_probes` loads `probes-hybrid.txt` automatically whenever present on a baked area (disable via `KJK_HYBRID_PROBES=off` for instant A/B comparison), preserving full backwards compatibility with v1 `.png` bakes, legacy `KJK_BAKED_PROBES=on`, and CPU floor-readback probes for unbaked areas.

### 3. Live Direct Fitted Lights & Twin Suns in `crates/game/src/odyssey.wgsl`
- `ProbeList` carries `sun2_way`, `sun2_colour`, and per-slot `direct_scale`, `sun1_vis`, `sun2_vis`, `room`, and the `hybrid` marker bit.
- When `probe_is_hybrid(owner)` is true on a probed non-room object (`characters`, `doors`, `unbaked placeables`):
  - **Indirect GI:** reads linear HDR 6-axis `probe_indirect_linear`, modulated by material ambient and multi-bounce SSAO (`decode_22(room_kept)`), without `PROBE_FLOOR` lifting or `BAKED_GAIN_PROBES = 2.0`.
  - **Live Direct Lights:** skips BioWare legacy non-combat area lamps (`LIGHT_COMBAT` stays active for sabers/bolts/flashes) and evaluates `fitted_lights` with `wrap = 0.0` (matching the Phase 2 baker's `WRAP = 0.0`) scaled by `direct_scale`, plus live Sun 1 and Sun 2 direct lighting attenuated by `sun1_vis` / `sun2_vis` and dynamic character shadows.
  - **HW RT Room Occlusion:** under Vulkan Ultra (`shadows.rays != 0`), `fitted_visibility` traces `RAY_WALLS` (`scene_rays` rooms + other doors + nearby casters) so interior walls and closed doors block fitted lights from adjacent rooms while still lighting through open doorways.
  - **HDR Overflow:** radiance exceeding `1.0` adds into `hybrid_over` for smooth AgX rolloff and Bloom (on both linear main pass and MSAA fallback).

### 4. Tolerant Geometry Matching (`crates/game/src/bake_fingerprint.rs`)
- Keeps `EXACT = 1` backwards-compatible while computing quantized world-space triangle digests (`lightmap-approx` and `model-approx` at `1 mm` position / `1/2048` UV tolerance, discarding degenerate triangles).
- `fit()` accepts matching exact digests first, falling back to `*-approx` geometry digests and small GIT placement tolerances (`5 mm` position, `0.01 rad` facing) when MDLEdit/re-saved mods introduce sub-millimetre float drift or mesh reordering.
