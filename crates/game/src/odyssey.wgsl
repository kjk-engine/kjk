// Odyssey static geometry as the games' fixed-function path draws it: the
// diffuse texture modulated by the lightmap on the second UV set (the
// games multiply the two).
//
// Textures are bound as plain UNORM, so the multiply happens on the stored
// (gamma-encoded) values as on the original hardware, and the result is
// output as it is: the target keeps encoded values and blends them as the
// games' framebuffers did (`scene::encoded_view`).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::mesh_functions::{get_world_from_local, get_tag}
#import kjk::lighting::{flash, fog, decode, encode}

struct GpuLight {
    position_radius: vec4<f32>,
    color_multiplier: vec4<f32>,
    kind: vec4<u32>,
    // A fight's light that lights from a line. The xyz part is the line's
    // direction, and w is half its length. A point light has 0 here.
    way: vec4<f32>,
    // Bit r: room r takes none of this fight's light (`material.rs`).
    unseen: vec4<u32>,
}

struct OdysseyParams {
    tint: vec4<f32>,
    self_illum: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    world_ambient: vec4<f32>,
    fog_color: vec4<f32>,
    fog_range: vec4<f32>,
    uv_offset: vec2<f32>,
    flags: u32,
    // Its slot of `animated` (`material::Animated`); 0 for none.
    slot: u32,
    // x: what its colour is multiplied by under Enhanced lighting. It is 1
    // on everything but the effects that glow (`material::OdysseyParams`).
    glow: vec4<f32>,
}

// What changes every frame, over a material's own values
// (`material::GpuAnimated`): rgb the self-illumination keyed (where uv.z
// is 1), a times the tint's alpha; uv.xy added to the UVs.
struct Animated {
    illum_alpha: vec4<f32>,
    uv: vec4<f32>,
}

struct LightList {
    // The lightning's light (`weather::Flash`): xyz where, w its radius
    // (0: none); its colour.
    flash_at: vec4<f32>,
    flash_colour: vec4<f32>,
    count: u32,
    // 1 under the options' Enhanced lighting (`rolloff.rs`), else 0.
    enhanced: u32,
    lights: array<GpuLight, 32>,
}

// A character casting shadows (`shadows.rs`'s `GpuCaster`).
struct GpuCaster {
    // The sphere holding its capsules. The xyz part is its centre, and w
    // its radius.
    sphere: vec4<f32>,
    // Bit i of x is set when light i of the list casts its shadow. The y
    // part is its first capsule, and z how many capsules it has. The w part
    // is the model of its body (`OWNER_TAG`).
    lights: vec4<u32>,
    // Bit r is set when its shadow does not fall on room r.
    unseen: vec4<u32>,
    // Its shadow tiles. The x part is its first tile in `tiles`. Bit i of y
    // is set when light i of the list has a tile of it. Its tiles follow
    // one another in the order of their lights.
    tiles: vec4<u32>,
}

// A shadow tile (`shadows.rs`'s `GpuTile`): a depth picture of one caster
// seen from one light, in the tile atlas.
struct GpuTile {
    // The light's view and projection, fitted to the caster's sphere.
    view_proj: mat4x4<f32>,
    // The tile's place in the atlas: xy its corner and zw its size.
    rect: vec4<f32>,
    // The x part is how wide the tile is at the caster's centre (metres).
    // The y part is how wide a texel is in the atlas. The z part is the
    // atlas's layer: 0 for the characters', 1 for the placeables' kept
    // tiles.
    span: vec4<f32>,
    // The depth range's near and far ends (metres from the light), and 1
    // for the sun's flat projection or 0 for a lamp's.
    depth: vec4<f32>,
}

// One of a character's capsules. It is the segment from `a` to `b`, and
// `a.w` is its radius.
struct GpuCapsule {
    a: vec4<f32>,
    b: vec4<f32>,
}

struct ShadowList {
    // 1: the rooms' shadows alone are drawn (`KJK_SHADOW_DEBUG`).
    debug: u32,
    // How many of `casters` cast.
    count: u32,
    // How many of `near_boxes` are traced.
    box_count: u32,
    // How many of `tiles` are drawn.
    tile_count: u32,
    // 1 with Shadows at Low. A tile lookup then reads one depth comparison
    // (`tile_visibility`), and the room's light from overhead is not shaded
    // (`room_shadow`).
    low: u32,
    // A sphere holding every caster's reach, and one holding every near
    // box's reach. A point outside both traces nothing.
    reach: vec4<f32>,
    box_reach: vec4<f32>,
    casters: array<GpuCaster, 32>,
    capsules: array<GpuCapsule, 512>,
    // The placeables' boxes nearest the camera. The x part is a box's place
    // in `boxes`. Bit i of y is set when light i of the list casts its
    // shadow. The z part is its placeable's model (`OWNER_TAG`). The w part
    // holds, 6 bits each, the light of each of its kept tiles: its place in
    // the list plus 1, or 0 when that light is not in the list now.
    near_boxes: array<vec4<u32>, 64>,
    tiles: array<GpuTile, 64>,
}

// A placeable's box (`shadows.rs`'s `GpuBox`). The centre's w is the
// radius of the sphere holding it. Each axis's xyz is one of the box's
// axes, and its w is half the box's size along it.
struct GpuBox {
    centre: vec4<f32>,
    x: vec4<f32>,
    y: vec4<f32>,
    z: vec4<f32>,
    // Bit r is set when its shadow does not fall on room r.
    unseen: vec4<u32>,
    // Its kept tiles: x the first in `BoxList::tiles`, y how many. The z
    // part is `BOX_CASTS` when the box itself casts. A placeable too sparse
    // for its box casts from its tiles alone.
    tiles: vec4<u32>,
}

struct BoxList {
    count: u32,
    tile_count: u32,
    boxes: array<GpuBox, 256>,
    // The placeables' kept tiles, in the atlas's layer 1.
    tiles: array<GpuTile, 256>,
}

// A placed model's light from the light probes (`light_probes.rs`'s
// `GpuProbe`): the light arriving on a surface facing east, west, up, down,
// south and north (Bevy's +x, -x, +y, -y, +z, -z). The w part of the first
// is 1 when the entry holds light.
struct GpuProbe {
    light: array<vec4<f32>, 6>,
}

struct ProbeList {
    // 1 under `KJK_PROBES=only`: a probed model shows its probe light alone.
    mode: u32,
    entries: array<GpuProbe, 1024>,
}

// How many models `probe_list` has an entry for (`light_probes::PROBE_SLOTS`).
const PROBE_SLOTS: u32 = 1024u;
// The probe light never goes below this share of the area's flat ambient,
// so a model in a dark spot is never black. Tuned with captures.
const PROBE_FLOOR: f32 = 0.35;

// `GpuBox::tiles.z` when the box itself casts.
const BOX_CASTS: u32 = 1u;
// No kept tile.
const NO_TILE: u32 = 0xffffffffu;

// Flags: see `material.rs`.
const DISCARD_ZERO_ALPHA: u32 = 1u;
const OPAQUE_ALPHA: u32 = 2u;
const ENVMAP: u32 = 4u;
const HAS_LIGHTMAP: u32 = 8u;
const STATIC: u32 = 16u;
const UNLIT: u32 = 32u;
const VERTEX_ALPHA: u32 = 64u;
const FOG: u32 = 128u;
const OPAQUE_TEXTURE: u32 = 256u;
const ADD_FADE: u32 = 512u;
const SHADOWED: u32 = 1024u;
const ROOM: u32 = 2048u;
const RECOLOUR: u32 = 4096u;
const SPHERE_ENVMAP: u32 = 8192u;
// Lit evenly (a mine's dome): the area's ambient whatever the mesh's
// ambient colour, and every lamp's light whichever way the surface faces.
const EVEN_LIGHT: u32 = 16384u;
// A model of an area, lit by the light probes under Enhanced lighting.
const PROBED: u32 = 32768u;
const LIGHT_DYNAMIC_TYPE_ALL: u32 = 1u;
// `GpuLight::kind.z` of a fight's light (`material::LIGHT_COMBAT`).
const LIGHT_COMBAT: u32 = 1u;
// How much of a fight's light a surface turned away from it still takes.
// That is light thrown about a corridor.
// The share of a fight's light's radius within which it is at least half
// as strong as at the light.
// How strong a fight's light is on what it lights. Rooms and characters
// take the same strength.
const COMBAT_WRAP: f32 = 0.3;
const FIGHT_CORE: f32 = 0.45;
const FIGHT_GAIN: f32 = 1.5;
// The same under Enhanced lighting. It is lower, because the bolt, the
// blade or the flash is itself bright there.
const FIGHT_GAIN_ENHANCED: f32 = 0.8;
// How tight the highlight is that a fight's light leaves on a shiny
// surface, and how strong.
const HIGHLIGHT_POWER: f32 = 60.0;
const HIGHLIGHT_GAIN: f32 = 6.0;
// A fight's light lighting a point less than this is not traced for
// shadows there, nor added. It would not show.
const FAINT: f32 = 0.002;
// How many rooms a fight's light can be kept from (`material::VIS_ROOMS`).
const VIS_ROOMS: u32 = 128u;
// Rec. 601 luma: how bright a baked light reads.
const LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OdysseyParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var lightmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var envmap_texture: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var envmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var<storage, read> lights: LightList;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var<storage, read> shadows: ShadowList;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var<storage, read> boxes: BoxList;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var<storage, read> animated: array<Animated, 1024>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var sphere_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var sphere_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var tile_atlas: texture_depth_2d_array;
@group(#{MATERIAL_BIND_GROUP}) @binding(15) var tile_sampler: sampler_comparison;
@group(#{MATERIAL_BIND_GROUP}) @binding(16) var<storage, read> probe_list: ProbeList;

fn max3(v: vec3<f32>) -> f32 {
    return max(v.x, max(v.y, v.z));
}

// `shadows.rs`'s limits. At most `CASTERS` characters cast at once. At
// most `NEAR_CASTERS` of them are traced at one fragment, the ones whose
// reach holds it.
const CASTERS: u32 = 32u;
const NEAR_CASTERS: u32 = 6u;
const NEAR_BOXES: u32 = 64u;
// How far a character's shadow is traced past its bounding sphere
// (metres). Over the last `SHADOW_EASE` of that it eases away, so no edge
// shows where the trace stops. A shadow that far is faint already.
const SHADOW_REACH: f32 = 6.0;
const SHADOW_EASE: f32 = 2.0;
// How wide a light's source is, which sets how soft its shadows are. A
// lamp stands for a lamp or a light panel, not a point. Its source is
// `LAMP_SOURCE` metres across its half. The sun's source is an angle,
// `SUN_SOURCE` radians across its half. That is wider than the sun's own,
// for a shadow as soft as the games' lightmapped light. A fight's light is
// at least `FIGHT_SOURCE` metres across its half, and a bolt in flight is
// wider by half its line. No source is narrower than `LEAST_SOURCE`
// radians across its half. A lamp hung far above a ceiling lights the room
// through panels near it, and its shadow is as soft as theirs.
const LAMP_SOURCE: f32 = 0.35;
const LEAST_SOURCE: f32 = 0.08;
const SUN_SOURCE: f32 = 0.03;
const FIGHT_SOURCE: f32 = 0.15;
// A light this wide (metres) is the area's sun (`shadows.rs`'s
// `SUN_RADIUS`).
const SUN_RADIUS: f32 = 100.0;
// Where a character's body nearly meets a surface, the body keeps the light
// bounced about the room off it. The darkening reaches `CONTACT_REACH`
// metres past a capsule's surface. It takes at most `CONTACT_DARKNESS` of
// the light.
const CONTACT_REACH: f32 = 0.35;
const CONTACT_DARKNESS: f32 = 0.7;
// A room keeps at least this share of its baked light under a shadow.
const ROOM_KEPT: f32 = 0.2;
// A room's baked light is shared out among the lights that put it there
// (`room_shadow`). Each light weighs as much as it would light the point by
// fixed-function lighting. Light bounced about the room weighs
// `FILL_WEIGHT`, and no character blocks it.
const FILL_WEIGHT: f32 = 0.05;
// Outdoors, the sky lights a spot the sun does not reach. The sky weighs
// `SKY_FILL` times the sun. Tatooine's lightmaps have their shaded sand at
// some 56% of their sunlit sand, which this matches.
const SKY_FILL: f32 = 1.25;
// A room's baked light comes from its lamps and light panels, most of them
// overhead, and many of those are not in the light list. So a character
// also keeps part of the room's light off the floor under it. That light
// comes from a wide source straight above, which weighs `OVERHEAD_WEIGHT`
// on a floor. The source spans `OVERHEAD_SOURCE` radians across its half.
const OVERHEAD_WEIGHT: f32 = 0.05;
const OVERHEAD_SOURCE: f32 = 0.45;
// `cone_visibility`'s mask that traces every caster near.
const ALL_CASTERS: u32 = 0xffffffffu;

// A mesh's `MeshTag` with this bit set carries the model it is part of
// (`shadows.rs`'s `OWNER_TAG`). `NO_OWNER` stands for none.
const OWNER_TAG: u32 = 0x80000000u;
const NO_OWNER: u32 = 0xffffffffu;
// A caster whose lowest point stands more than this above a surface
// (metres) is on another floor, and its shadow does not fall there.
const STOREY: f32 = 2.0;
// A mesh without an owner in its tag (a weapon re-armed, a cut limb) is
// part of the character whose sphere holds its origin, within this many
// metres of the sphere's edge.
const UNTAGGED_REACH: f32 = 0.3;

// The characters near a fragment whose shadows are traced there, by their
// place in `shadows.casters`. The placeables' boxes near it, by their place
// in `shadows.near_boxes`.
struct Near {
    count: u32,
    at: array<u32, 6>,
    boxes: u32,
    box_at: array<u32, 4>,
}

// At most this many placeables' boxes are traced at one fragment.
const NEAR_BOX_SLOTS: u32 = 4u;
// A placeable's shadow is traced no farther than this past its sphere
// (metres). Placeables stand on the floor, and their shadows are short.
const BOX_REACH: f32 = 3.0;
// A box is traced at the points where the ray toward the light enters it,
// passes its middle and leaves it.
const BOX_STEPS: u32 = 3u;
// Contact darkening beside a box is this much of a capsule's.
const BOX_CONTACT: f32 = 0.6;

fn no_casters() -> Near {
    return Near(0u, array<u32, 6>(0u, 0u, 0u, 0u, 0u, 0u), 0u, array<u32, 4>(0u, 0u, 0u, 0u));
}

// How far `position` lies outside a box (metres). It is less than 0 inside.
fn box_distance(position: vec3<f32>, b: GpuBox) -> f32 {
    let d = position - b.centre.xyz;
    let local = vec3<f32>(dot(d, b.x.xyz), dot(d, b.y.xyz), dot(d, b.z.xyz));
    let out = abs(local) - vec3<f32>(b.x.w, b.y.w, b.z.w);
    return length(max(out, vec3<f32>(0.0))) + min(max3(out), 0.0);
}

// Where the ray from `position` along `direction` enters and leaves box
// `b`, along it (metres). When it misses the box, the two are where it
// passes nearest, and the first is past the second.
fn box_span(position: vec3<f32>, direction: vec3<f32>, b: GpuBox) -> vec2<f32> {
    let d = position - b.centre.xyz;
    let start = vec3<f32>(dot(d, b.x.xyz), dot(d, b.y.xyz), dot(d, b.z.xyz));
    let way = vec3<f32>(dot(direction, b.x.xyz), dot(direction, b.y.xyz), dot(direction, b.z.xyz));
    let half = vec3<f32>(b.x.w, b.y.w, b.z.w);
    let inverse = vec3<f32>(1.0) / select(vec3<f32>(1e-6), way, abs(way) > vec3<f32>(1e-6));
    let a = (-half - start) * inverse;
    let c = (half - start) * inverse;
    let enter = max3(min(a, c));
    let leave = min(max(a.x, c.x), min(max(a.y, c.y), max(a.z, c.z)));
    return vec2<f32>(enter, leave);
}

// How much of a light's source a box hides from `position`. The light lies
// along `direction`, `distance` away, and its source spans `source`
// radians across its half. The ray is clipped against the box's three
// slabs. It is tested where it enters the box, at the middle and where it
// leaves, or around where it passes nearest when it misses. At each point,
// the box's distance from the ray, over the width of the light's cone
// there, says how much of the source shows. A thin box cannot fall between
// two tests.
fn box_occlusion(position: vec3<f32>, direction: vec3<f32>, distance: f32, source: f32, b: GpuBox) -> f32 {
    let span = box_span(position, direction, b);
    let first = clamp(min(span.x, span.y), 0.02, distance);
    let last = clamp(max(span.x, span.y), 0.02, distance);
    if max(span.x, span.y) <= 0.02 || min(span.x, span.y) >= distance {
        return 0.0;
    }
    var lit = 1.0;
    for (var k = 0u; k < BOX_STEPS; k++) {
        let t = mix(first, last, f32(k) / f32(BOX_STEPS - 1u));
        let clear = box_distance(position + direction * t, b);
        lit = min(lit, saturate(0.5 + clear / (2.0 * t * source)));
    }
    return smoothstep(0.0, 1.0, 1.0 - lit);
}

// The model a mesh is part of, from its tag (`scene::owner_tag`).
// `NO_OWNER` for a room's mesh or a mesh with no tag.
fn mesh_owner(instance_index: u32) -> u32 {
    let tag = get_tag(instance_index);
    return select(NO_OWNER, tag & ~OWNER_TAG, (tag & OWNER_TAG) != 0u);
}

// The light probes' light of model `owner` on a surface facing `normal`:
// the six colours weighed by the normal's squared parts. A zero normal
// takes the mean of the six. The w part is 0 when the model has no entry
// with light, and the area's flat ambient then lights it.
fn probe_light(owner: u32, normal: vec3<f32>) -> vec4<f32> {
    if owner >= PROBE_SLOTS {
        return vec4<f32>(0.0);
    }
    let light = probe_list.entries[owner].light;
    if light[0].w < 0.5 {
        return vec4<f32>(0.0);
    }
    if dot(normal, normal) < 1e-8 {
        let mean = (light[0].rgb + light[1].rgb + light[2].rgb + light[3].rgb + light[4].rgb + light[5].rgb) / 6.0;
        return vec4<f32>(mean, 1.0);
    }
    let n = normalize(normal);
    let w = n * n;
    let x = select(light[1].rgb, light[0].rgb, n.x > 0.0);
    let y = select(light[3].rgb, light[2].rgb, n.y > 0.0);
    let z = select(light[5].rgb, light[4].rgb, n.z > 0.0);
    return vec4<f32>(x * w.x + y * w.y + z * w.z, 1.0);
}

// The model of the character whose sphere holds `origin`, nearest its
// centre in metres. `NO_OWNER` when there is none. Only a mesh with no tag
// asks this.
fn untagged_owner(origin: vec3<f32>) -> u32 {
    var owner = NO_OWNER;
    var best = 1e9;
    for (var c = 0u; c < min(shadows.count, CASTERS); c++) {
        let caster = shadows.casters[c];
        let apart = length(origin - caster.sphere.xyz);
        if apart < caster.sphere.w + UNTAGGED_REACH && apart < best {
            best = apart;
            owner = caster.lights.w;
        }
    }
    return owner;
}

// Whether a caster in sphere `sphere`, kept from the rooms in `unseen`, may
// shade a surface at `position` of room `room` facing `normal`. Its room
// must be linked with the surface's. It must not stand a storey above the
// surface. It must not stand behind the surface. A zero normal faces every
// way.
fn may_shade(sphere: vec4<f32>, unseen: vec4<u32>, position: vec3<f32>, normal: vec3<f32>, room: u32) -> bool {
    if room < VIS_ROOMS && (unseen[room / 32u] & (1u << (room % 32u))) != 0u {
        return false;
    }
    if sphere.y - sphere.w - position.y > STOREY {
        return false;
    }
    return dot(normal, sphere.xyz - position) >= -sphere.w;
}

// Puts caster `c`, `apart2` from the point (squared), among the nearest
// held in `at` and `far`, of which `count` are held and `slots` fit. When
// all are taken, it replaces the farthest if it is nearer.
fn hold_nearest(count: ptr<function, u32>, at: ptr<function, array<u32, 6>>, far: ptr<function, array<f32, 6>>, slots: u32, c: u32, apart2: f32) {
    if *count < slots {
        (*at)[*count] = c;
        (*far)[*count] = apart2;
        *count += 1u;
        return;
    }
    var worst = 0u;
    for (var k = 1u; k < slots; k++) {
        if (*far)[k] > (*far)[worst] {
            worst = k;
        }
    }
    if apart2 < (*far)[worst] {
        (*at)[worst] = c;
        (*far)[worst] = apart2;
    }
}

// Whether `position` falls inside one of `caster`'s tiles, seen from its
// light. A point past the tile's far end still counts, as its lookup
// reads it (`tile_visibility`).
fn in_tiles(position: vec3<f32>, caster: GpuCaster) -> bool {
    let count = countOneBits(caster.tiles.y);
    for (var t = 0u; t < count; t++) {
        let clip = shadows.tiles[caster.tiles.x + t].view_proj * vec4<f32>(position, 1.0);
        if clip.w > 1e-4 && abs(clip.x) < clip.w && abs(clip.y) < clip.w {
            return true;
        }
    }
    return false;
}

// The characters whose shadows may fall on `position`, the nearest
// `NEAR_CASTERS` of those within `SHADOW_REACH` of it. Also the placeables'
// boxes, the nearest `NEAR_BOX_SLOTS` of those within `BOX_REACH`. The
// mesh's own model `owner`, and any caster `may_shade` turns away, are left
// out.
fn near_casters(position: vec3<f32>, normal: vec3<f32>, room: u32, owner: u32) -> Near {
    var near = no_casters();
    let all = position - shadows.reach.xyz;
    if dot(all, all) < shadows.reach.w * shadows.reach.w {
        var at = array<u32, 6>(0u, 0u, 0u, 0u, 0u, 0u);
        var far = array<f32, 6>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        var count = 0u;
        for (var c = 0u; c < min(shadows.count, CASTERS); c++) {
            let caster = shadows.casters[c];
            let reach = caster.sphere.w + SHADOW_REACH;
            let off = position - caster.sphere.xyz;
            let apart2 = dot(off, off);
            if caster.lights.w == owner || apart2 >= reach * reach
                || !may_shade(caster.sphere, caster.unseen, position, normal, room) {
                continue;
            }
            // With Shadows at Low a caster shades only what its tiles
            // cover, so a point outside them all leaves it out.
            if shadows.low != 0u && !in_tiles(position, caster) {
                continue;
            }
            hold_nearest(&count, &at, &far, NEAR_CASTERS, c, apart2);
        }
        near.count = count;
        near.at = at;
    }
    let all_boxes = position - shadows.box_reach.xyz;
    if dot(all_boxes, all_boxes) < shadows.box_reach.w * shadows.box_reach.w {
        var at = array<u32, 6>(0u, 0u, 0u, 0u, 0u, 0u);
        var far = array<f32, 6>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        var count = 0u;
        for (var k = 0u; k < min(shadows.box_count, NEAR_BOXES); k++) {
            let near_box = shadows.near_boxes[k];
            let b = boxes.boxes[near_box.x];
            let reach = b.centre.w + BOX_REACH;
            let off = position - b.centre.xyz;
            let apart2 = dot(off, off);
            if near_box.z == owner || apart2 >= reach * reach
                || !may_shade(b.centre, b.unseen, position, normal, room) {
                continue;
            }
            hold_nearest(&count, &at, &far, NEAR_BOX_SLOTS, k, apart2);
        }
        near.boxes = count;
        for (var k = 0u; k < NEAR_BOX_SLOTS; k++) {
            near.box_at[k] = at[k];
        }
    }
    return near;
}

// How much of a light's source a capsule hides from `position`. The light
// lies along `direction`, `distance` away. Its source spans `source`
// radians across its half. The capsule's point nearest the ray to the
// light stands for the capsule, as a sphere. The share hidden is how much
// of the source's disc that sphere's disc covers, as seen from the point.
fn capsule_occlusion(position: vec3<f32>, direction: vec3<f32>, distance: f32, source: f32, capsule: GpuCapsule) -> f32 {
    let a = capsule.a.xyz - position;
    let ba = capsule.b.xyz - capsule.a.xyz;
    let radius = capsule.a.w;
    let along = dot(ba, direction);
    let span = dot(ba, ba);
    let across = span - along * along;
    var s = 0.0;
    if across > 1e-6 {
        s = clamp((along * dot(a, direction) - dot(a, ba)) / across, 0.0, 1.0);
    } else if span > 1e-8 {
        s = clamp(-dot(a, ba) / span, 0.0, 1.0);
    }
    let centre = a + ba * s;
    let ahead = dot(centre, direction);
    if ahead <= 0.0 || ahead >= distance {
        return 0.0;
    }
    let gap = max(length(centre), 1e-4);
    let blocker = asin(min(radius / gap, 1.0));
    let apart = acos(clamp(ahead / gap, -1.0, 1.0));
    let overlap = saturate((blocker + source - apart) / (2.0 * min(blocker, source)));
    let covered = min(1.0, (blocker * blocker) / (source * source));
    // A capsule only just in front of the surface, toward the light, hides
    // little, so it does not end at a hard line where it passes behind.
    let in_front = smoothstep(0.0, radius, ahead);
    return smoothstep(0.0, 1.0, overlap) * covered * in_front;
}

// A surface this much nearer the light than a tile's depth (in the tile's
// depth range) is not shaded by it.
const TILE_BIAS: f32 = 0.002;

// 1: a tile lookup first searches for what casts there (`tile_visibility`).
// 0 spreads the edge by the caster's centre, for measuring the search.
const TILE_SEARCH: u32 = 1u;

// Where a tile lookup reads around its point, as shares of how far the
// edge spreads. No two of the eight share a column or a row, so the widest
// gap between them across either axis is a quarter of the spread. A limb
// thinner than the spread (a leg under a low lamp) then always meets some
// of them. Four reads at the corners stepped over such a limb, and its
// shadow was dark along its edges and lit down its middle.
const TILE_TAPS: u32 = 8u;
const TILE_SPREAD: array<vec2<f32>, 8> = array<vec2<f32>, 8>(
    vec2<f32>(0.125, -0.375),
    vec2<f32>(-0.125, 0.375),
    vec2<f32>(0.625, 0.125),
    vec2<f32>(-0.375, -0.625),
    vec2<f32>(-0.625, 0.625),
    vec2<f32>(-0.875, -0.125),
    vec2<f32>(0.375, 0.875),
    vec2<f32>(0.875, -0.875),
);

// A depth of `tile` as a distance from its light (metres).
fn tile_metres(tile: GpuTile, depth: f32) -> f32 {
    let near = tile.depth.x;
    let far = tile.depth.y;
    if tile.depth.z > 0.5 {
        return near + depth * (far - near);
    }
    return far * near / max(far - depth * (far - near), 1e-4);
}

// How much of a light reaches `position` past the caster in `tile`. The
// caster's centre lies `ahead` metres from `position` toward the light,
// whose source spans `source` radians across its half. First the depths
// around the point's place in the tile are read, as far apart as the
// light's source would spread the edge from the caster's centre. Those
// nearer the light than the point are what casts there, and their mean
// distance in front of the point sets how far the edge spreads. So a
// shadow is sharp where it meets what casts it (a foot, a plank on the
// sand) and softer farther away. Then depth comparisons are read that far
// apart, each filtered over 2 by 2 texels. Both readings are spread as
// `TILE_SPREAD` says.
fn tile_visibility(position: vec3<f32>, ahead: f32, source: f32, tile: GpuTile) -> f32 {
    let clip = tile.view_proj * vec4<f32>(position, 1.0);
    if clip.w <= 1e-4 {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if abs(ndc.x) >= 1.0 || abs(ndc.y) >= 1.0 {
        return 1.0;
    }
    let uv = tile.rect.xy + vec2<f32>(ndc.x + 1.0, 1.0 - ndc.y) * 0.5 * tile.rect.zw;
    let depth = min(ndc.z, 1.0) - TILE_BIAS;
    let texel = tile.span.y;
    let low = tile.rect.xy + vec2<f32>(texel);
    let high = tile.rect.xy + tile.rect.zw - vec2<f32>(texel);
    let layer = i32(tile.span.z);
    // With Shadows at Low, one depth comparison at the point, filtered over
    // 2 by 2 texels, gives a hard edge.
    if shadows.low != 0u {
        return textureSampleCompareLevel(tile_atlas, tile_sampler, clamp(uv, low, high), layer, depth);
    }
    // How far the edge spreads behind a gap of `metres`, as a share of the
    // atlas.
    let widest = tile.rect.z * 0.125;
    let search = clamp(tan(source) * max(ahead, 0.0) / tile.span.x * tile.rect.z, texel, widest);
    let point = tile_metres(tile, depth);
    var reach = search;
    var blockers = 0.0;
    var gap = 0.0;
    for (var k = 0u; k < TILE_TAPS * TILE_SEARCH; k++) {
        let at = clamp(uv + TILE_SPREAD[k] * search, low, high);
        let found = textureLoad(tile_atlas, vec2<i32>(at / texel), layer, 0);
        if found < depth {
            blockers += 1.0;
            gap += point - tile_metres(tile, found);
        }
    }
    if TILE_SEARCH == 1u {
        if blockers == 0.0 {
            return 1.0;
        }
        let spread = tan(source) * (gap / blockers) / tile.span.x * tile.rect.z;
        reach = clamp(spread, texel, widest);
    }
    var lit = 0.0;
    for (var k = 0u; k < TILE_TAPS; k++) {
        lit += textureSampleCompareLevel(tile_atlas, tile_sampler, clamp(uv + TILE_SPREAD[k] * reach, low, high), layer, depth);
    }
    return smoothstep(0.0, 1.0, lit / f32(TILE_TAPS));
}

// The kept tile of `near_box`'s placeable under the light whose bit is
// `lights`, or `NO_TILE`.
fn kept_tile(near_box: vec4<u32>, b: GpuBox, lights: u32) -> u32 {
    let wanted = firstTrailingBit(lights) + 1u;
    for (var slot = 0u; slot < b.tiles.y; slot++) {
        if ((near_box.w >> (6u * slot)) & 63u) == wanted {
            return b.tiles.x + slot;
        }
    }
    return NO_TILE;
}

// How much of light `index` of the list reaches `position` past the
// characters near it. It is 1 when all of it does and 0 when none does.
// Only the characters the light casts from are traced
// (`GpuCaster::lights`).
fn light_visibility(index: u32, light: GpuLight, position: vec3<f32>, near: Near) -> f32 {
    if near.count == 0u && near.boxes == 0u {
        return 1.0;
    }
    let to_light = fight_point(light, position) - position;
    let distance = length(to_light);
    if distance < 1e-3 {
        return 1.0;
    }
    var source = max(atan(LAMP_SOURCE / distance), LEAST_SOURCE);
    if light.position_radius.w >= SUN_RADIUS {
        source = SUN_SOURCE;
    } else if light.kind.z == LIGHT_COMBAT {
        source = max(atan((FIGHT_SOURCE + 0.5 * light.way.w) / distance), LEAST_SOURCE);
    }
    return cone_visibility(position, to_light / distance, distance, source, near, 1u << index, light.kind.z == LIGHT_COMBAT);
}

// How much of a source along `direction`, `distance` away and `source`
// radians across its half, reaches `position` past the characters `near`
// it. Only the characters with a bit of `lights` set in their
// `GpuCaster::lights` are traced. `ALL_CASTERS` traces every one. A
// fight's light (`fight`) reads no placeable's kept tile.
fn cone_visibility(position: vec3<f32>, direction: vec3<f32>, distance: f32, source: f32, near: Near, lights: u32, fight: bool) -> f32 {
    var casters = near.at;
    var kept = 1.0;
    for (var k = 0u; k < near.count; k++) {
        let caster = shadows.casters[casters[k]];
        if lights != ALL_CASTERS && (caster.lights.x & lights) == 0u {
            continue;
        }
        // The cone toward the light must pass through the caster's sphere.
        let centre = caster.sphere.xyz - position;
        let ahead = dot(centre, direction);
        let off = length(centre - direction * clamp(ahead, 0.0, distance));
        if ahead <= -caster.sphere.w || off > caster.sphere.w + max(ahead, 0.0) * source {
            continue;
        }
        var lit = 1.0;
        if lights != ALL_CASTERS && (caster.tiles.y & lights) != 0u {
            // The light has a tile of this caster. The tiles of the lights
            // before it in the list come first. A fight's light has one only
            // with Shadows at Low.
            let tile = caster.tiles.x + countOneBits(caster.tiles.y & (lights - 1u));
            lit = tile_visibility(position, ahead, source, shadows.tiles[tile]);
        } else {
            let range = caster.lights.yz;
            for (var j = range.x; j < range.x + range.y; j++) {
                lit *= 1.0 - capsule_occlusion(position, direction, distance, source, shadows.capsules[j]);
            }
        }
        let gap = max(length(centre) - caster.sphere.w, 0.0);
        let ease = 1.0 - smoothstep(SHADOW_REACH - SHADOW_EASE, SHADOW_REACH, gap);
        kept *= 1.0 - (1.0 - lit) * ease;
    }
    var placeables = near.box_at;
    for (var k = 0u; k < near.boxes; k++) {
        let near_box = shadows.near_boxes[placeables[k]];
        if lights != ALL_CASTERS && (near_box.y & lights) == 0u {
            continue;
        }
        let b = boxes.boxes[near_box.x];
        // A light with a kept tile of the placeable reads the tile. The
        // rest trace its box, unless it casts from its tiles alone.
        var tile = NO_TILE;
        if !fight && lights != ALL_CASTERS {
            tile = kept_tile(near_box, b, lights);
        }
        // A point inside a box is under it or part of it, and the box does
        // not shade it. A box's base often sinks a little into the floor.
        if tile == NO_TILE && ((b.tiles.z & BOX_CASTS) == 0u || box_distance(position, b) <= 0.0) {
            continue;
        }
        let centre = b.centre.xyz - position;
        let ahead = dot(centre, direction);
        let off = length(centre - direction * clamp(ahead, 0.0, distance));
        if ahead <= -b.centre.w || off > b.centre.w + max(ahead, 0.0) * source {
            continue;
        }
        let gap = max(length(centre) - b.centre.w, 0.0);
        let ease = 1.0 - smoothstep(BOX_REACH - 1.0, BOX_REACH, gap);
        if tile != NO_TILE {
            let lit = tile_visibility(position, ahead, source, boxes.tiles[tile]);
            kept *= 1.0 - (1.0 - lit) * ease;
        } else {
            kept *= 1.0 - box_occlusion(position, direction, distance, source, b) * ease;
        }
    }
    return kept;
}

// What a surface keeps of the light bounced about the room where a
// character's body nearly meets it. That is under the feet, beside a body
// lying on the floor, and behind a back against a wall.
fn contact_shade(position: vec3<f32>, normal: vec3<f32>, near: Near) -> f32 {
    var casters = near.at;
    var kept = 1.0;
    for (var k = 0u; k < near.count; k++) {
        let caster = shadows.casters[casters[k]];
        let reach = caster.sphere.w + CONTACT_REACH;
        let off = position - caster.sphere.xyz;
        if dot(off, off) >= reach * reach {
            continue;
        }
        let first = caster.lights.y;
        for (var j = first; j < first + caster.lights.z; j++) {
            let capsule = shadows.capsules[j];
            let a = capsule.a.xyz;
            let ba = capsule.b.xyz - a;
            let s = saturate(dot(position - a, ba) / max(dot(ba, ba), 1e-8));
            let to_body = a + ba * s - position;
            let gap = max(length(to_body), 1e-4);
            let radius = capsule.a.w;
            let clearance = gap - radius;
            if clearance >= CONTACT_REACH {
                continue;
            }
            let facing = saturate(dot(normal, to_body / gap));
            let near_body = 1.0 - smoothstep(0.0, CONTACT_REACH, clearance);
            let seen = min(1.0, (radius * radius) / (gap * gap));
            kept *= 1.0 - CONTACT_DARKNESS * facing * near_body * seen;
        }
    }
    var placeables = near.box_at;
    for (var k = 0u; k < near.boxes; k++) {
        let b = boxes.boxes[shadows.near_boxes[placeables[k]].x];
        let clearance = box_distance(position, b);
        if (b.tiles.z & BOX_CASTS) == 0u || clearance >= CONTACT_REACH || clearance <= 0.0 {
            continue;
        }
        let facing = saturate(dot(normal, normalize(b.centre.xyz - position)));
        let near_box = 1.0 - smoothstep(0.0, CONTACT_REACH, clearance);
        kept *= 1.0 - CONTACT_DARKNESS * BOX_CONTACT * facing * near_box;
    }
    return kept;
}

// A room's baked light `lighting` at `position` of room `room`, with the
// characters' shadows on it. The lightmap is taken to hold the light of
// each light of the list that lights the point, and light bounced about
// the room. Each light's share of the lightmap is its weight over all the
// weights. A light's weight is how much it would light the point by
// fixed-function lighting, from its colour, falloff and facing. A shadow
// takes the share of each light it blocks, so it never takes more than the
// lightmap holds. It always leaves `ROOM_KEPT` of the baked light. A
// fight's light is not in the lightmap, so its shadow is taken where its
// light is added (`combat_lights`).
fn room_shadow(position: vec3<f32>, normal: vec3<f32>, room: u32, lighting: vec3<f32>, near: Near) -> vec3<f32> {
    if near.count == 0u && near.boxes == 0u {
        return lighting;
    }
    var weights = vec3<f32>(FILL_WEIGHT);
    var blocked = vec3<f32>(0.0);
    for (var i = 0u; i < lights.count; i++) {
        let light = lights.lights[i];
        if light.kind.x != 0u || light.kind.z == LIGHT_COMBAT || !reaches_room(light, room) {
            continue;
        }
        let radius = light.position_radius.w;
        let to_light = light.position_radius.xyz - position;
        let distance = length(to_light);
        if distance >= radius {
            continue;
        }
        let facing = max(0.0, dot(normal, to_light / max(1e-4, distance)));
        let falloff = (radius * radius) / ((radius + distance) * (radius + distance));
        let reach = 1.0 - smoothstep(0.9 * radius, radius, distance);
        let weight = light.color_multiplier.rgb * (light.color_multiplier.w * falloff * reach * facing);
        if max3(weight) < 1e-3 {
            continue;
        }
        weights = weights + weight;
        if radius >= SUN_RADIUS {
            weights = weights + weight * SKY_FILL;
        }
        blocked = blocked + weight * (1.0 - light_visibility(i, light, position, near));
    }
    // The room's light from overhead, kept off a surface that faces up. It
    // traces every capsule and box near, so Shadows at Low leaves it out.
    let up = OVERHEAD_WEIGHT * saturate(normal.y);
    if up > 0.0 && shadows.low == 0u {
        let overhead = cone_visibility(position, vec3<f32>(0.0, 1.0, 0.0), 1e4, OVERHEAD_SOURCE, near, ALL_CASTERS, false);
        weights = weights + vec3<f32>(up);
        blocked = blocked + vec3<f32>(up * (1.0 - overhead));
    }
    let taken = lighting * blocked / weights;
    // `KJK_SHADOW_DEBUG=2`: red is the share of the light blocked, green
    // the weight of all the lights, blue how many characters are near.
    if shadows.debug == 2u {
        return vec3<f32>(max3(blocked / weights), max3(weights), f32(near.count) / 6.0);
    }
    return max(lighting - taken, lighting * ROOM_KEPT) * contact_shade(position, normal, near);
}

// The point of a fight's light nearest `position`. For a light on a line
// (a flying bolt), it is the nearest point of the line. For any other light
// it is the light itself.
fn fight_point(light: GpuLight, position: vec3<f32>) -> vec3<f32> {
    let at = light.position_radius.xyz;
    let half = light.way.w;
    if half <= 0.0 {
        return at;
    }
    let along = clamp(dot(position - at, light.way.xyz), -half, half);
    return at + light.way.xyz * along;
}

// How strong a fight's light is at `distance` from it. It falls off as an
// inverse square around a core whose size is `FIGHT_CORE` of its `radius`.
// It is at full strength at the light and at half strength at the core's
// edge. Its long tail eases to nothing at the radius. The light is strong
// close by and never makes a flat disc.
fn fight_falloff(distance: f32, radius: f32) -> f32 {
    let x = distance / max(FIGHT_CORE * radius, 1e-4);
    let edge = distance / max(radius, 1e-4);
    let window = saturate(1.0 - edge * edge * edge * edge);
    return window * window / (1.0 + x * x);
}

// What one fight's light gives a surface at `position`, measured from the
// fragment itself. A surface turned away from the light keeps
// `COMBAT_WRAP` of it. A light that only brightens the ambient lights from
// all sides. So does any light on a mesh without normals.
fn fight_light(light: GpuLight, position: vec3<f32>, normal: vec3<f32>, has_normal: bool) -> vec3<f32> {
    let radius = light.position_radius.w;
    let to_light = fight_point(light, position) - position;
    let distance = length(to_light);
    if distance >= radius {
        return vec3<f32>(0.0);
    }
    var facing = 1.0;
    if has_normal && light.kind.x == 0u {
        let lambert = max(0.0, dot(normal, to_light / max(1e-4, distance)));
        facing = COMBAT_WRAP + (1.0 - COMBAT_WRAP) * lambert;
    }
    return light.color_multiplier.rgb * (light.color_multiplier.w * fight_falloff(distance, radius) * facing);
}

// The room a mesh instance is part of. The mesh's `MeshTag` is the room's
// index plus 1 (`scene::room_tag`). A mesh with no tag is in no room, and
// every fight's light reaches it.
fn mesh_room(instance_index: u32) -> u32 {
    let tag = get_tag(instance_index);
    return select(VIS_ROOMS, tag - 1u, tag != 0u && (tag & OWNER_TAG) == 0u);
}

// Whether a fight's light reaches the surfaces of room `room`. It reaches
// only the rooms linked with its own (`GpuLight::unseen`), so it stops at
// a wall.
fn reaches_room(light: GpuLight, room: u32) -> bool {
    if room >= VIS_ROOMS {
        return true;
    }
    return (light.unseen[room / 32u] & (1u << (room % 32u))) == 0u;
}

// What a fight's lights add to the surface at `position` of room `room`,
// past the shadows of the characters `near` it.
fn combat_lights(position: vec3<f32>, raw_normal: vec3<f32>, room: u32, near: Near) -> vec3<f32> {
    let has_normal = dot(raw_normal, raw_normal) > 1e-8;
    let normal = normalize(select(vec3<f32>(0.0, 0.0, 1.0), raw_normal, has_normal));
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < lights.count; i++) {
        let light = lights.lights[i];
        if light.kind.z != LIGHT_COMBAT || !reaches_room(light, room) {
            continue;
        }
        // A light too faint here to show is not traced.
        let lit = fight_light(light, position, normal, has_normal);
        if max3(lit) > FAINT {
            sum = sum + lit * light_visibility(i, light, position, near);
        }
    }
    return sum;
}

// The point of a fight's light that a shiny surface at `position` mirrors
// toward the eye. `mirrored` is the eye's line of sight mirrored about the
// surface. A bolt lights from a line, and the point is where that line
// passes nearest the mirrored line of sight. So a bolt's highlight is a
// streak along the way it flies. Any other light is a point.
fn highlight_point(light: GpuLight, position: vec3<f32>, mirrored: vec3<f32>) -> vec3<f32> {
    let at = light.position_radius.xyz;
    let half = light.way.w;
    if half <= 0.0 {
        return at;
    }
    let way = light.way.xyz;
    let to_middle = at - position;
    let together = dot(mirrored, way);
    let apart = 1.0 - together * together;
    var along = 0.0;
    if apart > 1e-4 {
        along = (together * dot(mirrored, to_middle) - dot(way, to_middle)) / apart;
    }
    return at + way * clamp(along, -half, half);
}

// The highlights a fight's lights leave on a shiny surface at `position`
// of room `room`, as linear light. A surface without normals has none.
fn fight_highlights(position: vec3<f32>, raw_normal: vec3<f32>, room: u32, near: Near) -> vec3<f32> {
    if dot(raw_normal, raw_normal) <= 1e-8 {
        return vec3<f32>(0.0);
    }
    let normal = normalize(raw_normal);
    let to_eye = normalize(view.world_position - position);
    let mirrored = reflect(-to_eye, normal);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < lights.count; i++) {
        let light = lights.lights[i];
        if light.kind.z != LIGHT_COMBAT || !reaches_room(light, room) {
            continue;
        }
        let radius = light.position_radius.w;
        let to_light = highlight_point(light, position, mirrored) - position;
        let distance = length(to_light);
        if distance >= radius {
            continue;
        }
        let halfway = normalize(to_light / max(distance, 1e-4) + to_eye);
        let shine = pow(max(dot(normal, halfway), 0.0), HIGHLIGHT_POWER);
        let strength = light.color_multiplier.w * fight_falloff(distance, radius) * shine;
        if strength > FAINT {
            sum = sum + decode(light.color_multiplier.rgb) * (strength * light_visibility(i, light, position, near));
        }
    }
    return sum * HIGHLIGHT_GAIN;
}

// The light `lighting` with a fight's light `added` on it. The fight's
// light fills what is left below full through a soft shoulder. Each
// channel becomes 1 − (1 − l)·e^(−a/(1 − l)). The middle of a pool keeps
// rising toward full and never goes flat, as it did under a clamp.
fn add_fight_light(lighting: vec3<f32>, added: vec3<f32>) -> vec3<f32> {
    let room = max(vec3<f32>(1.0) - lighting, vec3<f32>(1e-4));
    return vec3<f32>(1.0) - room * exp(-max(added, vec3<f32>(0.0)) / room);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Its slot's values over its own (slot 0, none, leaves them).
    let moving = animated[params.slot];
    let tint = vec4<f32>(params.tint.rgb, params.tint.a * moving.illum_alpha.a);
#ifdef VERTEX_UVS_A
    let uv = in.uv + params.uv_offset + moving.uv.xy;
#else
    let uv = vec2<f32>(0.0);
#endif
#ifdef VERTEX_UVS_B
    let uv2 = in.uv_b;
#else
    let uv2 = vec2<f32>(0.0);
#endif
    // The view's mip bias (none but under TAA or DLSS, which draw fewer
    // pixels a frame than they show) keeps the texture as sharp as the
    // frame shown. The lightmap and the env map take none: they are soft
    // light, not the surface's detail.
    var texel = textureSampleBias(base_texture, base_sampler, uv, view.mip_bias);
    // A texture without alpha (DXT1 kept in blocks): its transparent texel
    // is opaque black, as the games drew DXT1.
    if (params.flags & OPAQUE_TEXTURE) != 0u {
        texel.a = 1.0;
    }
    var shade = tint;
    // A recoloured texture: its whitest part stays white, the rest of its
    // brightness takes the tint's colour.
    if (params.flags & RECOLOUR) != 0u {
        let high = max(texel.r, max(texel.g, texel.b));
        let low = min(texel.r, min(texel.g, texel.b));
        texel = vec4<f32>(vec3<f32>(low) + (high - low) * tint.rgb, texel.a);
        shade = vec4<f32>(1.0, 1.0, 1.0, tint.a);
    }
    let base = texel * shade;
    // The second UV set's lightmap, read on every fragment so the sample
    // stays in uniform control flow (a mesh without one binds a stand-in).
    let lightmap = textureSample(lightmap_texture, lightmap_sampler, uv2).rgb;
    let self_lit = select(params.self_illum.rgb, moving.illum_alpha.rgb, moving.uv.z > 0.5);
    // A lightmapped surface starts from its baked light plus its own glow;
    // anything else from full light. An unlit surface keeps this as it is.
    var lighting = vec3<f32>(1.0);
    if (params.flags & HAS_LIGHTMAP) != 0u {
        lighting = min(vec3<f32>(1.0), self_lit + lightmap);
    }
    // The characters whose shadows may fall on this fragment.
    var near = no_casters();
    // Under `KJK_PROBES=only`, a probed model's probe light, shown alone in
    // place of the colour (its w then 1).
    var probe_only = vec4<f32>(0.0);
    // Under Enhanced lighting, what a fight's lights give this fragment.
    // It is added to the finished colour as linear light (below). Under
    // Classic lighting it stays at nothing.
    let enhanced = lights.enhanced != 0u;
    var fight_added = vec3<f32>(0.0);
    if (params.flags & UNLIT) == 0u {
        // Fixed-function lighting, per fragment: an ambient sum, which the
        // material's ambient colour scales, and a diffuse sum, which its
        // diffuse colour scales, starting from the surface's own glow.
        let is_static = (params.flags & STATIC) != 0u;
        let even = (params.flags & EVEN_LIGHT) != 0u;
        // What scales the ambient sum: the material's ambient colour, or
        // nothing on a surface lit evenly.
        let ambient_colour = select(params.ambient.rgb, vec3<f32>(1.0), even);
        var ambient_sum = vec3<f32>(0.0);
        var diffuse_sum = self_lit;
        let normal = normalize(in.world_normal);
        let has_normal = dot(in.world_normal, in.world_normal) > 1e-8;
        if (params.flags & HAS_LIGHTMAP) != 0u {
            diffuse_sum = self_lit + lightmap;
        } else if !is_static {
            // The area's ambient lights a moving mesh (a creature, a
            // placeable) that has no baked light of its own. Under Enhanced
            // lighting a model of an area takes the light probes' light of
            // where it stands instead, which the lightmaps hold without its
            // lamps (`light_probes.rs`), never below `PROBE_FLOOR` of the
            // flat ambient.
            var area_ambient = params.world_ambient.rgb;
            if enhanced && (params.flags & PROBED) != 0u {
                let probed = probe_light(mesh_owner(in.instance_index), select(vec3<f32>(0.0), normal, has_normal));
                if probed.w > 0.5 {
                    area_ambient = max(probed.rgb, params.world_ambient.rgb * PROBE_FLOOR);
                    if probe_list.mode == 1u {
                        probe_only = vec4<f32>(probed.rgb, 1.0);
                    }
                }
            }
            ambient_sum = ambient_sum + ambient_colour * area_ambient;
        }
        let fight_normal = select(vec3<f32>(0.0, 1.0, 0.0), normal, has_normal);
        let position = in.world_position.xyz;
        // The fight's lights are summed apart and added the way a room adds
        // them (below).
        var fight = vec3<f32>(0.0);
        // Whether a light reaches a mesh is judged once for the whole mesh,
        // from its origin, so a creature is lit or not as one piece. The
        // light then fades out over the last tenth of its radius. (The
        // Endar Spire's storage box: the owner's screenshot reads (61, 71,
        // 98), this rule (56, 65, 89), a cut-off at the radius from each
        // fragment (143, 148, 171).)
        let origin = get_world_from_local(in.instance_index)[3].xyz;
        // The characters whose shadows fall here, leaving out the mesh's
        // own character.
        if (params.flags & SHADOWED) != 0u {
            var owner = mesh_owner(in.instance_index);
            if owner == NO_OWNER {
                owner = untagged_owner(origin);
            }
            near = near_casters(position, select(vec3<f32>(0.0), normal, has_normal), VIS_ROOMS, owner);
        }
        for (var i = 0u; i < lights.count; i++) {
            let light = lights.lights[i];
            // A static mesh takes only the lights meant for everything.
            if is_static && light.kind.y != LIGHT_DYNAMIC_TYPE_ALL {
                continue;
            }
            // A fight's light lights a character the way it lights the floor
            // under it. It is measured from each fragment by the same curve.
            if light.kind.z == LIGHT_COMBAT {
                let colour = select(params.diffuse.rgb, params.ambient.rgb, light.kind.x != 0u);
                let lit = fight_light(light, position, fight_normal, has_normal);
                if max3(lit) > FAINT {
                    fight = fight + lit * colour * light_visibility(i, light, position, near);
                }
                continue;
            }
            let at = light.position_radius.xyz;
            let radius = light.position_radius.w;
            let to_light = at - position;
            let distance = length(to_light);
            let reach = 1.0 - smoothstep(0.9 * radius, radius, length(at - origin));
            if reach <= 0.0 {
                continue;
            }
            // The fragment's own distance sets only the falloff and the
            // direction: there is no cut-off but the reach above.
            let direction = to_light / max(1e-4, distance);
            let facing = select(max(0.0, dot(normal, direction)), 1.0, even);
            let falloff = (radius * radius) / ((radius + distance) * (radius + distance));
            let strength = ((reach * light.color_multiplier.w) * falloff) * light.color_multiplier.rgb;
            if light.kind.x != 0u {
                // A light that only brightens the ambient: no direction.
                ambient_sum = ambient_sum + strength * ambient_colour;
            } else {
                // Another character's shadow keeps this light off the side
                // that faces it.
                var kept = 1.0;
                if facing > 0.0 {
                    kept = light_visibility(i, light, position, near);
                }
                diffuse_sum = diffuse_sum + ((strength * facing) * kept) * params.diffuse.rgb;
            }
        }
        // The diffuse sum never goes below black; the total stops at full.
        lighting = min(vec3<f32>(1.0), ambient_sum + max(vec3<f32>(0.0), diffuse_sum));
        if enhanced {
            fight_added = fight * FIGHT_GAIN_ENHANCED;
        } else {
            let headroom = 1.0 - dot(lighting, LUMA);
            lighting = add_fight_light(lighting, fight * FIGHT_GAIN * headroom);
        }
    } else if (params.flags & SHADOWED) != 0u {
        // A room's light is baked, so a shadow takes the blocked lights'
        // share of it. A mesh without normals (some rooms' walls) takes no
        // shadow. A fight's light is shadowed where it is added (below).
        let has_normal = dot(in.world_normal, in.world_normal) > 1e-8;
        let normal = select(vec3<f32>(0.0), normalize(in.world_normal), has_normal);
        near = near_casters(in.world_position.xyz, normal, mesh_room(in.instance_index), NO_OWNER);
        var shaded = lighting;
        if dot(in.world_normal, in.world_normal) > 1e-8 {
            let room = mesh_room(in.instance_index);
            shaded = room_shadow(in.world_position.xyz, normalize(in.world_normal), room, lighting, near);
        }
        if shadows.debug == 2u {
            return vec4<f32>(shaded, 1.0);
        }
        // `KJK_SHADOW_DEBUG=3` and 4: the rooms show the tile atlas's
        // characters' layer or its placeables' over the screen, black where
        // a tile holds a caster.
        if shadows.debug == 3u || shadows.debug == 4u {
            let uv = in.position.xy / view.viewport.zw;
            let layer = i32(shadows.debug - 3u);
            return vec4<f32>(vec3<f32>(textureSampleCompareLevel(tile_atlas, tile_sampler, uv, layer, 0.9999)), 1.0);
        }
        if shadows.debug != 0u {
            return vec4<f32>(vec3<f32>(dot(shaded, LUMA) / max(dot(lighting, LUMA), 1e-4)), 1.0);
        }
        lighting = shaded;
    }
    // A fight's lights on a room's baked light: a bolt's pool on the floor,
    // a blade's glow on the walls, a deflection's flash.
    // They add as much as the baked light leaves room for: a pool shows in
    // a dark corridor, a lit hall only warms, never washes out to white.
    if (params.flags & ROOM) != 0u {
        let room = mesh_room(in.instance_index);
        let fight = combat_lights(in.world_position.xyz, in.world_normal, room, near);
        if enhanced {
            fight_added = fight * FIGHT_GAIN_ENHANCED;
        } else {
            let headroom = 1.0 - dot(lighting, LUMA);
            lighting = add_fight_light(lighting, fight * FIGHT_GAIN * headroom);
        }
    }
    // A lightning flash lights everything near it, rooms too, which the
    // light list leaves to their lightmaps (EXE-NOTES `LIGHTNING_LIGHT`).
    // Fixed function's light sum stops at full: the flash's light (colour
    // keys up to 1 × multiplier 100) shows a surface's own texture, not
    // white.
    // Without a flash nothing here runs: the look is as it was.
    if lights.flash_at.w > 0.0 {
        lighting = min(
            vec3<f32>(1.0),
            lighting + flash(in.world_position.xyz, lights.flash_at, lights.flash_colour.rgb)
        );
    }
    var colour = lighting * base.rgb;
    if (params.flags & ENVMAP) != 0u {
        // Reflection mapping as KOTOR sets it up, done per fragment: the
        // direction from the eye to the point, mirrored about the surface
        // normal, picks the cube map's texel in the game's world (z up), not
        // the eye's: its executable turns the eye-space reflection by the
        // camera's orientation (a texture matrix). So a floor mirrors the
        // part of its cube map above the horizon, as high as the eye looks
        // down. The base texture's alpha masks the shine (one minus it
        // weighs the cube's colour, added to the lit texel): an opaque
        // texel has none.
        let incident = normalize(in.world_position.xyz - view.world_position);
        let mirrored = reflect(incident, normalize(in.world_normal));
        // Bevy's world (y up) back to the game's (z up).
        let lookup = vec3<f32>(mirrored.x, -mirrored.z, mirrored.y);
        var shine = textureSample(envmap_texture, envmap_sampler, lookup).rgb;
        if (params.flags & SPHERE_ENVMAP) != 0u {
            // A flat env map, sphere-mapped as fixed function does it: the
            // eye-space reflection r picks (r.x, r.y) / m + 1/2, m being
            // 2 |r + (0, 0, 1)|.
            let eye = mat3x3<f32>(
                view.view_from_world[0].xyz,
                view.view_from_world[1].xyz,
                view.view_from_world[2].xyz
            );
            let r = eye * mirrored;
            let m = 2.0 * length(r + vec3<f32>(0.0, 0.0, 1.0));
            let at = r.xy / max(m, 1e-4) + vec2<f32>(0.5);
            shine = textureSample(sphere_texture, sphere_sampler, at).rgb;
        }
        colour = min(colour + shine * (1.0 - texel.a), vec3<f32>(1.0));
    }
    if enhanced {
        // The colour so far is the games' own, worked out on stored values
        // and stopped at 1. A fight's lights are added to it as linear
        // light, and the sum may pass 1. Their sum is read as a stored
        // level of light. So on a dark surface a pool looks as it does
        // under Classic lighting, and on a lit one it adds as one more
        // light would. A fragment that no fight's light reaches keeps its
        // stored colour bit for bit.
        var extra = vec3<f32>(0.0);
        if max3(fight_added) > 0.0 {
            extra = decode(base.rgb) * decode(fight_added);
        }
        // A shiny surface also mirrors the light itself. The texture's
        // alpha masks that as it masks the env map.
        let lit = (params.flags & ROOM) != 0u || (params.flags & UNLIT) == 0u;
        let shiny = 1.0 - texel.a;
        if lit && shiny > 0.0 && (params.flags & ENVMAP) != 0u {
            let room = mesh_room(in.instance_index);
            extra = extra + fight_highlights(in.world_position.xyz, in.world_normal, room, near) * shiny;
        }
        if max3(extra) > 0.0 {
            colour = encode(decode(colour) + extra);
        }
    }
    // `KJK_PROBES=only`: the probe light alone, no lamps and no texture.
    if probe_only.w > 0.0 {
        return probe_only;
    }
    if (params.flags & FOG) != 0u {
        colour = fog(colour, in.world_position.xyz, params.fog_range, params.fog_color.rgb);
    }
    // An env-mapped texture's alpha is the shine's mask, not coverage: a
    // see-through mesh takes only its own alpha (reone's OIT model does the
    // same), so Dantooine's crystals (alpha 0.9, DXT5 masks) stay solid.
    let coverage = select(base.a, tint.a, (params.flags & ENVMAP) != 0u);
    var alpha = select(coverage, 1.0, (params.flags & OPAQUE_ALPHA) != 0u);
#ifdef VERTEX_COLORS
    // Effect primitives carry a colour and fading alpha per vertex (the
    // saber trail); on additive surfaces the alpha scales what is added,
    // on vertex-alpha ones it fades the blend.
    if (params.flags & VERTEX_ALPHA) != 0u {
        colour = colour * in.color.rgb;
        alpha = alpha * in.color.a;
    } else {
        colour = colour * in.color.rgb * in.color.a;
    }
#endif
    // Alpha 0 (the vertex's included: a punch-through particle faded out)
    // hides nothing behind it.
    if (params.flags & DISCARD_ZERO_ALPHA) != 0u && alpha == 0.0 {
        discard;
    }
    // An added surface a clip fades (the swoop's thruster cards, its start
    // lights' flares) adds by the alpha it keys (`faded_additive`).
    if (params.flags & ADD_FADE) != 0u {
        colour = colour * clamp(tint.a, 0.0, 1.0);
    }
    // An effect that glows is brighter than white under Enhanced lighting,
    // and bloom spreads what is above white (`rolloff.rs`).
    if enhanced {
        colour = colour * params.glow.x;
    }
    return vec4<f32>(colour, alpha);
}
