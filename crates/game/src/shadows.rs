//! Dynamic shadows from capsules. Each character's body is a set of
//! capsules, the ones its ragdoll is fitted with (`ragdoll::Template`). The
//! play view places them on the character's bones every frame
//! (`play_view::cast_shadows`). This module sends them to the GPU each
//! frame, with one bounding sphere per character.
//!
//! `odyssey.wgsl` traces a cone from each fragment toward each light of the
//! light list, against the capsules of the characters near the fragment.
//! The cone is as wide as the light's source, so a shadow's edge widens
//! with its distance from the character. Every light of the list can cast,
//! fight lights included. A character also darkens the floor where its
//! feet meet it.
//!
//! A shadow takes from a surface the light it blocks. On a character, that
//! is the blocked light's own share of its lighting. On a room, whose light
//! is baked into its lightmap, it is an estimate of the light's share of
//! the lightmap. The estimate comes from the light's colour, falloff and
//! facing, as fixed-function lighting would light a character there. A
//! shadow never takes more than the lightmap holds, and never takes the
//! room below its fill light (`odyssey.wgsl`'s `room_shadow`).
//!
//! No shadow is cast through a wall. Each character casts only from the
//! lights that see it past the level's walls and floors (`Sight`). A fight's
//! light moves too fast to test every frame, so it casts from wherever it
//! is. The shader keeps any light out of the rooms not linked with the
//! light's own (`GpuLight::unseen`).

use std::collections::HashMap;

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::render::storage::ShaderBuffer;

use crate::material::{AreaLights, GpuLight, LIGHT_COMBAT};

/// The shadows' list, which every Odyssey material reads.
pub const AREA_SHADOWS: Handle<ShaderBuffer> = uuid_handle!("3c8f1d27-94b6-4e0a-b5d3-6a2e7f90c41b");

/// The most characters that cast at once (`odyssey.wgsl`'s `CASTERS`). The
/// ones nearest the camera cast.
pub const CASTERS: usize = 32;

/// The most capsules all the casting characters have together
/// (`odyssey.wgsl`'s `CAPSULES`). A humanoid has 15 and a beast at most 20.
pub const CAPSULES: usize = 512;

/// A light this wide (metres) is the area's sun. It is far off and over all
/// of the area (Taris's is 800 wide, some 290 m above the streets).
pub(crate) const SUN_RADIUS: f32 = 100.0;

/// How far a character or a light moves (metres) before the test of
/// whether the light sees the character is made again.
const SIGHT_MOVE: f32 = 0.5;

/// How far below the top of a character's bounding sphere its sight of a
/// light is tested from (metres). That is about the chest of a standing
/// humanoid.
const SIGHT_BELOW_TOP: f32 = 0.6;

/// The placeables' boxes, which every Odyssey material reads. They are
/// written when a placeable appears, moves or goes, not every frame.
pub const AREA_BOXES: Handle<ShaderBuffer> = uuid_handle!("8a41c6e2-5d73-4f19-a0b8-2e7c93d16f54");

/// The most placeables an area has boxes for (`odyssey.wgsl`'s `BoxList`).
pub const BOXES: usize = 256;

/// The most boxes traced in a frame, the ones nearest the camera
/// (`odyssey.wgsl`'s `NEAR_BOXES`).
pub const NEAR_BOXES: usize = 64;

/// A placeable whose box is no longer than this on its longest side
/// (metres) casts no shadow. A cup, a datapad or a small canister would not
/// show one.
const LEAST_BOX: f32 = 0.3;

/// A placeable whose box is longer than this on its longest side (metres)
/// casts no shadow. One box is a poor stand-in for a whole machine or a
/// ship's hull, and its shadow would fall where the model has none.
const MOST_BOX: f32 = 8.0;

/// A placeable lower than `FLAT_HEIGHT` metres, and lower than
/// `FLAT_SHARE` of its longest side, casts no shadow. Such a placeable is
/// a body lying on the floor, a mat or debris. It fills little of its box,
/// and the box's shadow would be a dark rectangle where the floor shows.
const FLAT_HEIGHT: f32 = 0.5;
const FLAT_SHARE: f32 = 0.35;

/// A placeable casts no shadow when its model covers less than this share
/// of its box's face, seen along any one of the box's three axes (`fill`).
/// Such a placeable is a pile of junk, a swoop bike or a speeder: a few
/// planks, poles and struts with open space between them. Its box would
/// throw a solid block of shadow, and shade the floor around it, where the
/// model shows only those few thin pieces.
const LEAST_FILL: f32 = 0.5;

/// `fill` draws a model's outline on a grid of this many cells a side.
const FILL_GRID: usize = 16;

/// A mesh's `MeshTag` with this bit set carries the model it is part of
/// in the other bits (`scene::owner_tag`). A character's capsules and a
/// placeable's box cast no shadow on the meshes of their own model. A room
/// mesh's tag is its room's index plus 1, without this bit.
pub(crate) const OWNER_TAG: u32 = 1 << 31;

/// How far past a character's sphere its shadow is traced (metres,
/// `odyssey.wgsl`'s `SHADOW_REACH`), and past a box's (`BOX_REACH`).
const SHADOW_REACH: f32 = 6.0;
const BOX_REACH: f32 = 3.0;

/// At most this many lines are traced through the level in a frame, for
/// the lights' sight of the characters and boxes. A pair not yet traced
/// casts no shadow until its turn comes. Entering an area traces every
/// pair, and this spreads that over a few frames.
const SIGHT_TESTS: usize = 48;

/// A line from a character's chest toward a light passes the test when it
/// gets this near the light (metres). It is traced that way only when the
/// line from the light starts inside something solid, such as the lamp's
/// wall fixture.
const LIGHT_NEAR: f32 = 0.5;

/// A light this near a character's body (metres from its capsules' skin,
/// or a placeable's box) does not cast that body's shadow. Such a light is
/// held by it or set on it: a welding torch's sparks, a lit blade, a
/// muzzle flash, a lamp on a placeable. Its shadow would be the body's own,
/// thrown wide over everything about it from a point at its skin. As the
/// sparks flare and die, that shadow flashed on and off (T3-M4 welding on
/// the Ebon Hawk).
const HELD_REACH: f32 = 0.4;

/// A lamp whose own line to a character is blocked is tested again from
/// this high above the character's chest (metres), under the lamp
/// (`Sight::sees`). Many rooms' lamps hang above the ceiling their light
/// panels are set in. Leviathan's hall lamps hang 20 to 33 m over its
/// floor. A line to the lamp itself meets the ceiling, so the line is
/// tested to a point under the lamp. That line still meets any wall
/// between the lamp and the character.
const SIGHT_ABOVE: f32 = 2.0;

/// A character as the shader reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuCaster {
    /// The sphere holding all its capsules: xyz its centre (Bevy world
    /// space), w its radius.
    pub sphere: Vec4,
    /// x has bit i set when light i of the light list casts its shadow. y
    /// is its first capsule, and z how many capsules it has. w is the model
    /// of its body (`OWNER_TAG`).
    pub lights: UVec4,
    /// Bit r is set when its shadow does not fall on room r. Those are the
    /// rooms not linked with the room it stands in (`fight_rooms::linked`).
    pub unseen: UVec4,
    /// The capsules a fight's light traces. x is the first and y how many.
    /// They are the same capsules as `lights` names unless Shadows is Low,
    /// which traces `COARSE_CAPSULES` merged ones.
    pub coarse: UVec4,
    /// Its shadow tiles (`shadow_tiles`). x is its first tile in
    /// `ShadowList::tiles`. Bit i of y is set when light i of the light
    /// list has a tile of it. Its tiles follow one another in the order of
    /// their lights in the list. A light with a tile reads the tile, and
    /// does not trace the capsules.
    pub tiles: UVec4,
}

/// A shadow tile as the shader reads it. A tile is a depth picture of one
/// caster seen from one light, drawn into the tile atlas
/// (`shadow_tiles::ATLAS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuTile {
    /// The light's view and projection, fitted to the caster's sphere.
    pub view_proj: Mat4,
    /// The tile's place in the atlas: xy its corner and zw its size, as a
    /// share of the atlas.
    pub rect: Vec4,
    /// x is how wide the tile is at the caster's centre (metres). y is how
    /// wide one of its texels is, as a share of the atlas. z is the atlas's
    /// layer (`shadow_tiles::CHARACTERS_LAYER` or `KEPT_LAYER`).
    pub span: Vec4,
    /// The depth range's near and far ends, as distances from the light
    /// (metres), and 1 for the sun's flat projection or 0 for a lamp's. The
    /// shader turns a depth into metres with them, to soften the shadow by
    /// how far it falls behind what casts it.
    pub depth: Vec4,
}

/// The most tiles drawn in a frame (`odyssey.wgsl`'s `ShadowList::tiles`).
pub const TILES: usize = 64;

/// The most characters with tiles, the ones nearest the camera. The rest
/// cast from capsules.
const TILE_CASTERS: usize = 12;

/// The most lights with a tile of one character, the ones that light it
/// most.
const TILE_LIGHTS: usize = 3;

/// A lamp nearer a character's centre than this many times its sphere's
/// radius cannot be fitted with a tile. That pair keeps its capsules, but
/// with Shadows at Low it gets a wide view (`tile_view`).
const TILE_FIT: f32 = 1.25;

/// How wide a wide view is to each side of the caster's centre (radians,
/// 60 degrees).
const NEAR_VIEW: f32 = std::f32::consts::FRAC_PI_3;

/// Where a wide view's depth range starts (metres from the light). A light
/// nearer the caster's centre than this gets no tile.
const NEAR_START: f32 = 0.1;

/// A tile the shadow pass draws: which model it shows, from where, and
/// where in the atlas's layer (`shadow_tiles`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlannedTile {
    pub model: usize,
    pub view_proj: Mat4,
    /// x and y its corner, z and w its size, in the atlas's texels.
    pub rect: UVec4,
}

/// The characters' tiles the shadow pass draws this frame. `upload` writes
/// it.
#[derive(Resource, Default)]
pub struct TilePlan(pub Vec<PlannedTile>);

/// The placeables' kept tiles, in the atlas's layer 1. `place_boxes`
/// writes it when a placeable appears, moves or goes, and the pass draws
/// the layer again only then.
#[derive(Resource, Default, PartialEq)]
pub struct KeptPlan(pub Vec<PlannedTile>);

/// The most kept tiles (`odyssey.wgsl`'s `BoxList::tiles`).
pub const KEPT_TILES: usize = 256;

/// The most lamps with a kept tile of one placeable: the ones that light
/// it most. Each sun gets one beside them (Tatooine has two), up to
/// `KEPT_LIGHTS` in all.
const KEPT_LAMPS: usize = 2;

/// The most kept tiles of one placeable (`ShadowList::near_boxes`'s w
/// holds the lights of 3).
const KEPT_LIGHTS: usize = 3;

/// A kept tile's texels a side for each metre of its placeable's radius,
/// before it is rounded up to a power of two between `LEAST_KEPT` and
/// `MOST_KEPT`. A placeable 1 m across gets 64.
const KEPT_TEXELS: f32 = 128.0;
const LEAST_KEPT: u32 = 64;
const MOST_KEPT: u32 = 256;

/// `GpuBox::tiles.z` when the box itself casts (`odyssey.wgsl`'s
/// `BOX_CASTS`).
const BOX_CASTS: u32 = 1;

/// A capsule as the shader reads it. It is the segment from `a` to `b`
/// (Bevy world space), and `a.w` is its radius.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuCapsule {
    pub a: Vec4,
    pub b: Vec4,
}

/// A placeable's box as the shader reads it. `centre`'s w is the radius of
/// the sphere holding the box. Each of `x`, `y` and `z` is one of the box's
/// axes (Bevy world space), and its w is half the box's size along it.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuBox {
    pub centre: Vec4,
    pub x: Vec4,
    pub y: Vec4,
    pub z: Vec4,
    /// Bit r is set when its shadow does not fall on room r, as
    /// `GpuCaster::unseen`.
    pub unseen: UVec4,
    /// Its kept tiles: x the first in `BoxList::tiles`, y how many. z is
    /// `BOX_CASTS` when the box itself casts. A placeable too sparse for its
    /// box casts from its tiles alone (`TileOnly`).
    pub tiles: UVec4,
}

/// The placeables' boxes and their kept tiles, as the shader reads them.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct BoxList {
    pub count: u32,
    pub tile_count: u32,
    pub boxes: [GpuBox; BOXES],
    pub tiles: [GpuTile; KEPT_TILES],
}

impl Default for BoxList {
    fn default() -> BoxList {
        BoxList {
            count: 0,
            tile_count: 0,
            boxes: [GpuBox::default(); BOXES],
            tiles: [GpuTile::default(); KEPT_TILES],
        }
    }
}

/// On a placeable's root beside its `ShadowBox`: it is too sparse to cast
/// as its box (`BoxTally::of`), so it casts from its kept tiles alone. With
/// no tiles (Shadows at Low, or no lamp lights it), it casts nothing.
#[derive(Component, Clone, Copy, Debug)]
pub struct TileOnly;

/// On a placeable's root: its box in its model's own frame, the lowest and
/// the highest corner. The scene puts it on the placeables that would show
/// a shadow (`BoxTally::of`).
#[derive(Component, Clone, Copy, Debug)]
pub struct ShadowBox {
    pub low: Vec3,
    pub high: Vec3,
}

/// How many placeables of a scene got a box, and how many were left out as
/// too small, too large, too flat or too sparse, for the dump.
#[derive(Default)]
pub struct BoxTally {
    boxed: usize,
    small: usize,
    large: usize,
    flat: usize,
    sparse: usize,
}

impl BoxTally {
    /// The box for a placeable whose model is made of `triangles` (in its
    /// own frame), unless it is too small, too large, too flat or too
    /// sparse to cast as one.
    pub fn of(&mut self, triangles: &[[Vec3; 3]]) -> Option<ShadowBox> {
        self.tiled(triangles)
            .and_then(|(shadow, boxed)| boxed.then_some(shadow))
    }

    /// The box for a placeable as `of` gives it, and whether the box
    /// itself casts. A placeable too sparse for its box still gets one,
    /// which does not cast: it casts from its tiles alone (`TileOnly`).
    pub fn tiled(&mut self, triangles: &[[Vec3; 3]]) -> Option<(ShadowBox, bool)> {
        let (low, high) = triangles.iter().flatten().fold(None, |bounds, &p| {
            Some(bounds.map_or((p, p), |(low, high): (Vec3, Vec3)| {
                (low.min(p), high.max(p))
            }))
        })?;
        let longest = (high - low).max_element();
        if longest <= LEAST_BOX {
            self.small += 1;
            return None;
        }
        if longest > MOST_BOX {
            self.large += 1;
            return None;
        }
        // The model's own frame is the game's, z up.
        let height = high.z - low.z;
        if height < FLAT_HEIGHT && height < FLAT_SHARE * longest {
            self.flat += 1;
            return None;
        }
        if fill(triangles, low, high).min_element() < LEAST_FILL {
            self.sparse += 1;
            return Some((ShadowBox { low, high }, false));
        }
        self.boxed += 1;
        Some((ShadowBox { low, high }, true))
    }

    /// The counts, for the dump's scene facts.
    pub fn facts(&self) -> serde_json::Value {
        serde_json::json!({
            "boxed": self.boxed,
            "too_small": self.small,
            "too_large": self.large,
            "flat": self.flat,
            "sparse": self.sparse,
        })
    }
}

/// How much of the box from `low` to `high` a model made of `triangles`
/// fills. The model is seen along each of the box's three axes in turn, and
/// its outline is drawn on a grid over the box's face there. The share of
/// the grid's cells the outline covers is how much of that face the model
/// fills, from 0 to 1. The fill's x is the share seen along x, its y along
/// y and its z from above. A crate fills nearly all of its box from every
/// side. A pile of planks fills its box from above, but little of it from
/// the side.
pub fn fill(triangles: &[[Vec3; 3]], low: Vec3, high: Vec3) -> Vec3 {
    let size = (high - low).max(Vec3::splat(1e-4));
    let cells = FILL_GRID as f32;
    Vec3::from_array(std::array::from_fn(|axis| {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut covered = [false; FILL_GRID * FILL_GRID];
        for triangle in triangles {
            let flat = triangle.map(|p| {
                let at = (p - low) / size * cells;
                Vec2::new(at[u], at[v])
            });
            cover(&mut covered, flat);
        }
        covered.iter().filter(|&&cell| cell).count() as f32 / covered.len() as f32
    }))
}

/// Marks the cells of a `FILL_GRID` grid whose middles lie in the triangle
/// `[a, b, c]` (in cells). A triangle seen edge on covers none.
fn cover(covered: &mut [bool], [a, b, c]: [Vec2; 3]) {
    let area = (b - a).perp_dot(c - a);
    if area.abs() < 1e-9 {
        return;
    }
    let last = FILL_GRID as i32 - 1;
    let first_cell = |x: f32| ((x - 0.5).ceil() as i32).clamp(0, last + 1);
    let last_cell = |x: f32| ((x - 0.5).floor() as i32).clamp(-1, last);
    let (low, high) = (a.min(b).min(c), a.max(b).max(c));
    for row in first_cell(low.y)..=last_cell(high.y) {
        for column in first_cell(low.x)..=last_cell(high.x) {
            let p = Vec2::new(column as f32 + 0.5, row as f32 + 0.5);
            let inside = [(a, b), (b, c), (c, a)]
                .iter()
                .all(|&(from, to)| (to - from).perp_dot(p - from) * area.signum() >= 0.0);
            if inside {
                covered[row as usize * FILL_GRID + column as usize] = true;
            }
        }
    }
}

/// The boxes of the shown placeables, as last written to `AREA_BOXES`, by
/// root and model, for picking the ones near the camera each frame. Each
/// also has the light of each of its kept tiles, as its place in the
/// scene's lights (`scene::SceneLights::all`). `placed` is each shown
/// placeable's root and box as last planned, to tell when one appears,
/// moves or goes.
#[derive(Resource, Default)]
struct Boxes {
    shown: Vec<(Entity, usize, GpuBox, [Option<usize>; 3])>,
    placed: Vec<(Entity, Vec4, Vec4, Vec4)>,
}

/// The casters and their capsules, as the shader reads them.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct ShadowList {
    /// 1 shows the rooms' shadows alone (`KJK_SHADOW_DEBUG`), for checking
    /// them. 2 shows how they are made (`odyssey.wgsl`'s `room_shadow`). 3
    /// shows the shadow tiles' atlas over the rooms.
    pub debug: u32,
    /// How many of `casters` cast.
    pub count: u32,
    /// How many of `near_boxes` are traced.
    pub box_count: u32,
    /// How many of `tiles` are drawn.
    pub tile_count: u32,
    /// 1 with Shadows at Low. A tile lookup then reads one depth
    /// comparison, and the room's light from overhead is not shaded.
    pub low: u32,
    /// A sphere holding every caster's reach (`SHADOW_REACH`), and one
    /// holding every near box's reach (`BOX_REACH`). A point outside both
    /// traces nothing.
    pub reach: Vec4,
    pub box_reach: Vec4,
    pub casters: [GpuCaster; CASTERS],
    pub capsules: [GpuCapsule; CAPSULES],
    /// The boxes nearest the camera. Each one's x is its place in
    /// `AREA_BOXES`. Bit i of its y is set when light i of the light list
    /// casts its shadow. Its z is its placeable's model (`OWNER_TAG`). Its
    /// w holds, 6 bits each, the light of each of its kept tiles: its place
    /// in the light list plus 1, or 0 when that light is not in the list
    /// now.
    pub near_boxes: [UVec4; NEAR_BOXES],
    /// The shadow tiles drawn this frame (`GpuCaster::tiles`).
    pub tiles: [GpuTile; TILES],
}

impl Default for ShadowList {
    fn default() -> ShadowList {
        ShadowList {
            debug: 0,
            count: 0,
            box_count: 0,
            tile_count: 0,
            low: 0,
            reach: Vec4::ZERO,
            box_reach: Vec4::ZERO,
            casters: [GpuCaster::default(); CASTERS],
            capsules: [GpuCapsule::default(); CAPSULES],
            near_boxes: [UVec4::ZERO; NEAR_BOXES],
            tiles: [GpuTile::default(); TILES],
        }
    }
}

/// The options' Shadows (`settings`, `display.shadows`). A fight's lights
/// are its lit blades, bolts and flashes. A character without a tile casts
/// from its capsules, which the shader traces at each pixel for every
/// light that casts. A fight with many lit blades costs the GPU most.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShadowSettings {
    /// Nothing casts.
    Off,
    /// Only the `LOW_TILE_CASTERS` characters nearest the camera cast. Each
    /// casts from a tile under each of the `LOW_TILE_LIGHTS` lamps, suns or
    /// fight's lights that light it most. A tile lookup reads one depth comparison, so the edge is hard.
    /// No capsule and no box is traced for a light, and placeables cast
    /// nothing. They are left out because their lookups would add to every
    /// pixel near them, where the Low before the tiles drew none for most
    /// of them.
    Low,
    /// The area's lamps, its sun and every one of a fight's lights cast.
    /// The characters nearest the camera cast from their own meshes under
    /// the lamps and the sun that light them most (`shadow_tiles`). The
    /// rest cast from every capsule.
    #[default]
    High,
}

impl ShadowSettings {
    /// The setting's value as the settings file holds it.
    pub fn named(value: &str) -> ShadowSettings {
        match value {
            "off" => ShadowSettings::Off,
            "low" => ShadowSettings::Low,
            _ => ShadowSettings::High,
        }
    }

    /// Whether anything casts.
    pub fn on(self) -> bool {
        self != ShadowSettings::Off
    }

    /// How many of a fight's lights may cast from one character.
    fn fight_lights(self) -> usize {
        match self {
            ShadowSettings::Off => 0,
            ShadowSettings::Low if old_low() => 2,
            ShadowSettings::Low => LOW_FIGHT_LIGHTS,
            ShadowSettings::High => usize::MAX,
        }
    }

    /// Whether Shadows is Low, drawn from tiles.
    fn low(self) -> bool {
        self == ShadowSettings::Low && !old_low()
    }

    /// How many characters nearest the camera get tiles, and under how many
    /// lights each.
    fn tiles(self) -> (usize, usize) {
        match self {
            ShadowSettings::High => (TILE_CASTERS, TILE_LIGHTS),
            ShadowSettings::Low if !old_low() => (LOW_TILE_CASTERS, LOW_TILE_LIGHTS),
            _ => (0, 0),
        }
    }
}

/// `KJK_CAPSULE_LOW=1` draws Shadows at Low as it was drawn before the
/// tiles: from every character's capsules and every placeable's box, with
/// soft edges. It is kept only so a benchmark can compare the old Low with
/// the new one in one build. Remove it, and the code only it reaches, once
/// that benchmark is done.
fn old_low() -> bool {
    static OLD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OLD.get_or_init(|| std::env::var("KJK_CAPSULE_LOW").as_deref() == Ok("1"))
}

/// How many of a fight's lights may take a character's tile with Shadows
/// at Low. The one that lights the character most is weighed against the
/// lamps and the sun.
const LOW_FIGHT_LIGHTS: usize = 1;

/// How many characters nearest the camera get a tile with Shadows at Low.
/// The rest cast no shadow from a light.
const LOW_TILE_CASTERS: usize = 4;

/// How many lights get a tile of one character with Shadows at Low: the
/// ones that light it most. A sun is always counted as lighting a character
/// most, so under Tatooine's two suns both cast, and one sun leaves room
/// for the strongest lamp or fight's light.
const LOW_TILE_LIGHTS: usize = 2;

/// How many capsules stand for a character under a fight's light with
/// Shadows at Low.
const COARSE_CAPSULES: usize = 3;

/// A character that may cast this frame. Its capsules are in Bevy world
/// space. Each is the segment between its two ends and its radius.
pub struct Caster {
    /// Its body's root, which tells one character from another between
    /// frames.
    pub root: Entity,
    /// Its body's model in the scene's models. Its meshes carry it in
    /// their tag (`OWNER_TAG`).
    pub model: usize,
    pub capsules: Vec<(Vec3, Vec3, f32)>,
}

/// The characters that may cast this frame, and what tells whether a light
/// sees them. The play view fills it each frame.
#[derive(Resource, Default)]
pub struct Casters {
    pub bodies: Vec<Caster>,
    pub sight: Option<Sight>,
}

/// The level's walls and floors, and its doors as they stand, for the
/// lines from the lights to the characters.
pub struct Sight {
    pub level: sim::Level,
    pub doors: Option<Vec<u8>>,
}

/// What a line between a light and a character met.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Line {
    Clear,
    Blocked,
    /// It started inside something solid both ways.
    Solid,
}

impl Sight {
    /// Whether a line from a light at `light` reaches within `near` of a
    /// character's chest at `chest` (Bevy world space) before it meets
    /// anything solid. The level's collision holds the characters' bodies
    /// too, so the line is traced from the light. A light inside its own
    /// fixture starts in solid, and then the line is traced from the chest
    /// and passes within `LIGHT_NEAR` of the light.
    fn line(&self, light: Vec3, chest: Vec3, near: f32) -> Line {
        let to_game = crate::scene::game_to_bevy().inverse();
        let trace = |from: Vec3, to: Vec3| {
            self.level.line(
                self.doors.as_deref(),
                (to_game * from).to_array(),
                (to_game * to).to_array(),
            )
        };
        let length = light.distance(chest);
        let down = trace(light, chest);
        if !down.start_solid {
            return match down.fraction * length >= length - near {
                true => Line::Clear,
                false => Line::Blocked,
            };
        }
        let up = trace(chest, light);
        match (up.start_solid, up.fraction * length >= length - LIGHT_NEAR) {
            (true, _) => Line::Solid,
            (false, true) => Line::Clear,
            (false, false) => Line::Blocked,
        }
    }

    /// What the line between `light` and a character's chest at `chest`
    /// meets, the character's sphere being `radius` wide. The sun is tested
    /// along its direction, not to where it hangs far over the area. A lamp
    /// is tested to where it hangs. A lamp that line does not reach, and
    /// that hangs more than `SIGHT_ABOVE` over the chest, is then tested to
    /// the point `SIGHT_ABOVE` over the chest under it. The lamp sees the
    /// character when either line is clear. The point under a lamp alone
    /// was not enough: under the Star Forge's high lamps it lies behind the
    /// hall's struts, and a character the lamp saw cast no shadow.
    fn sees(&self, light: &GpuLight, chest: Vec3, radius: f32) -> Line {
        sight_of(light, chest, |from| self.line(from, chest, radius))
    }
}

/// `Sight::sees`'s rule. `line` says what the line from a point to the
/// chest meets.
fn sight_of(light: &GpuLight, chest: Vec3, line: impl Fn(Vec3) -> Line) -> Line {
    let at = light.position_radius.truncate();
    if is_sun(light) {
        return line(chest + (at - chest).normalize_or_zero() * 60.0);
    }
    let met = line(at);
    let under = chest.y + SIGHT_ABOVE;
    if met == Line::Blocked && at.y > under {
        return line(at.with_y(under));
    }
    met
}

/// A room surface, for the rays that find where blaster bolts and blades
/// meet the rooms.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub corners: [Vec3; 3],
}

/// Each room's surfaces in Bevy world space, by room (`LoadedScene::rooms`).
#[derive(Resource, Default)]
pub struct RoomSurfaces(pub Vec<Vec<Surface>>);

/// Which shadow tiles Shadows at High draws: the characters' and the
/// placeables' (`KJK_SHADOW_TILES` unset), the characters' alone
/// (`characters`), or none (`off`), so a bench can compare them in one
/// build.
fn tiles_wanted() -> (bool, bool) {
    static WANTED: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
    *WANTED.get_or_init(|| match std::env::var("KJK_SHADOW_TILES").as_deref() {
        Ok("off") => (false, false),
        Ok("characters") => (true, false),
        _ => (true, true),
    })
}

/// `KJK_SHADOW_DEBUG` set. The rooms then show their shadows alone.
struct ShadowDebug(u32);

impl Default for ShadowDebug {
    fn default() -> ShadowDebug {
        ShadowDebug(match std::env::var("KJK_SHADOW_DEBUG").as_deref() {
            Ok("2") => 2,
            Ok("3") => 3,
            Ok("4") => 4,
            Ok("5") => 5,
            Ok(_) => 1,
            Err(_) => 0,
        })
    }
}

/// Whether each light saw each character when last tested. The key is the
/// character's root and the light's place rounded to `SIGHT_MOVE`, so a
/// light carried about is traced again only every half metre. The value is
/// where the character's chest was, whether the light saw it, and the
/// frame it was last asked for.
#[derive(Default)]
struct Seen(HashMap<(Entity, [i32; 3]), (Vec3, bool, u32)>);

/// The lines traced this frame and what they met, for the budget and the
/// dump.
#[derive(Default)]
struct Traced {
    tested: usize,
    blocked: usize,
    /// Lines that started in solid both ways. Such a light casts, as the
    /// test cannot tell.
    solid: usize,
    /// Pairs left for a later frame (`SIGHT_TESTS`).
    deferred: usize,
    /// Lights held by the body they would shade (`HELD_REACH`).
    held: usize,
}

pub struct ShadowsPlugin;

impl Plugin for ShadowsPlugin {
    fn build(&self, app: &mut App) {
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_SHADOWS, ShaderBuffer::from(ShadowList::default()));
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_BOXES, ShaderBuffer::from(BoxList::default()));
        app.init_resource::<RoomSurfaces>()
            .init_resource::<ShadowSettings>()
            .init_resource::<Casters>()
            .init_resource::<Boxes>()
            .init_resource::<TilePlan>()
            .init_resource::<KeptPlan>()
            .add_systems(
                PostUpdate,
                (place_boxes, upload.in_set(Uploaded))
                    .chain()
                    .after(bevy::transform::TransformSystems::Propagate)
                    .after(crate::play_view::ShadowsCast),
            );
    }
}

/// The casters and the tile plan are written for this frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Uploaded;

fn is_sun(light: &GpuLight) -> bool {
    light.position_radius.w >= SUN_RADIUS
}

/// The sphere holding `capsules`: its centre and radius.
fn bounds(capsules: &[(Vec3, Vec3, f32)]) -> (Vec3, f32) {
    let (low, high) = capsules.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), &(a, b, r)| (low.min(a.min(b) - r), high.max(a.max(b) + r)),
    );
    let centre = (low + high) * 0.5;
    let radius = capsules
        .iter()
        .map(|&(a, b, r)| centre.distance(a).max(centre.distance(b)) + r)
        .fold(0.0, f32::max);
    (centre, radius)
}

/// The smallest sphere holding spheres `one` and `other` (xyz the centre,
/// w the radius).
pub(crate) fn spheres_joined(one: Vec4, other: Vec4) -> Vec4 {
    let apart = one.truncate().distance(other.truncate());
    if apart + other.w <= one.w {
        return one;
    }
    if apart + one.w <= other.w {
        return other;
    }
    let radius = (apart + one.w + other.w) * 0.5;
    let toward = (other.truncate() - one.truncate()) / apart.max(1e-6);
    (one.truncate() + toward * (radius - one.w)).extend(radius)
}

/// The capsule a ragdoll's solid stands for, in its node's frame. A
/// capsule is itself. A droid's box is a capsule along its longest side,
/// as wide as the mean of its other two sides.
pub(crate) fn capsule_of(solid: &ragdoll::Solid) -> (Vec3, Vec3, f32) {
    match *solid {
        ragdoll::Solid::Capsule {
            ends: [a, b],
            radius,
        } => (a, b, radius),
        ragdoll::Solid::Cuboid { centre, turn, half } => {
            let mut sides = [(half.x, Vec3::X), (half.y, Vec3::Y), (half.z, Vec3::Z)];
            sides.sort_by(|a, b| b.0.total_cmp(&a.0));
            let radius = (sides[1].0 + sides[2].0) * 0.5;
            let along = turn * sides[0].1 * (sides[0].0 - radius).max(0.0);
            (centre - along, centre + along, radius)
        }
    }
}

/// How far `at` lies outside the skin of `capsules` (metres).
fn outside_capsules(capsules: &[(Vec3, Vec3, f32)], at: Vec3) -> f32 {
    capsules
        .iter()
        .map(|&(a, b, r)| {
            let ab = b - a;
            let along = ((at - a).dot(ab) / ab.length_squared().max(1e-8)).clamp(0.0, 1.0);
            at.distance(a + ab * along) - r
        })
        .fold(f32::MAX, f32::min)
}

/// How far `at` lies outside box `b` (metres).
fn outside_box(b: &GpuBox, at: Vec3) -> f32 {
    let d = at - b.centre.truncate();
    let local = Vec3::new(
        d.dot(b.x.truncate()),
        d.dot(b.y.truncate()),
        d.dot(b.z.truncate()),
    );
    let out = local.abs() - Vec3::new(b.x.w, b.y.w, b.z.w);
    out.max(Vec3::ZERO).length() + out.max_element().min(0.0)
}

/// The point of `light` nearest `to`: on its line for a bolt in flight,
/// else the light itself (`odyssey.wgsl`'s `fight_point`).
fn light_point(light: &GpuLight, to: Vec3) -> Vec3 {
    let at = light.position_radius.truncate();
    let half = light.way.w;
    if half <= 0.0 {
        return at;
    }
    let way = light.way.truncate();
    at + way * (to - at).dot(way).clamp(-half, half)
}

/// Which lights of `lights` cast a character's shadow, as bits. A light
/// casts when it reaches the character's sphere and, unless it is a
/// fight's light, when it sees the character's chest (`seen`). A light
/// that only brightens the ambient casts none. Of a fight's lights, the
/// `fights` that light the character most cast. A light `outside` says is
/// within `HELD_REACH` of the body casts none.
fn casting_lights(
    lights: &[GpuLight],
    (root, centre, radius): (Entity, Vec3, f32),
    (fights, outside): (usize, &dyn Fn(Vec3) -> f32),
    (sight, seen, frame): (Option<&Sight>, &mut Seen, u32),
    traced: &mut Traced,
) -> u32 {
    let chest = centre + Vec3::Y * (radius - SIGHT_BELOW_TOP).max(0.0);
    let mut bits = 0u32;
    // The fight's lights that cast, each with how much it lights the
    // character.
    let mut fight_lights: Vec<(f32, usize)> = Vec::new();
    for (index, light) in lights.iter().enumerate() {
        if light.kind.x != 0 {
            continue;
        }
        let fight = light.kind.z == LIGHT_COMBAT;
        if fight && fights == 0 {
            continue;
        }
        let at = light.position_radius.truncate();
        let reach = light.position_radius.w + light.way.w + radius;
        if at.distance_squared(centre) >= reach * reach {
            continue;
        }
        if outside(light_point(light, centre)) < HELD_REACH {
            traced.held += 1;
            continue;
        }
        let clear = fight
            || sight.is_none_or(|sight| {
                let key = (root, (at / SIGHT_MOVE).round().as_ivec3().to_array());
                match seen.0.get_mut(&key) {
                    Some((was, clear, asked)) if was.distance(chest) < SIGHT_MOVE => {
                        *asked = frame;
                        *clear
                    }
                    _ if traced.tested >= SIGHT_TESTS => {
                        traced.deferred += 1;
                        false
                    }
                    _ => {
                        traced.tested += 1;
                        let line = sight.sees(light, chest, radius);
                        traced.solid += usize::from(line == Line::Solid);
                        let clear = line != Line::Blocked;
                        seen.0.insert(key, (chest, clear, frame));
                        clear
                    }
                }
            });
        if !clear {
            traced.blocked += 1;
        } else if fight {
            let near = 1.0 - (at.distance(centre) / reach).min(1.0);
            let colour = light.color_multiplier;
            fight_lights.push((
                colour.truncate().max_element() * colour.w * near * near,
                index,
            ));
        } else {
            bits |= 1 << index;
        }
    }
    fight_lights.sort_by(|a, b| b.0.total_cmp(&a.0));
    for &(_, index) in fight_lights.iter().take(fights) {
        bits |= 1 << index;
    }
    bits
}

/// A tile's shape: x how wide it is at its caster's centre (metres), y
/// and z its depth range's near and far ends (metres from the light), w 1
/// for the sun's flat projection or 0 for a lamp's.
type TileShape = Vec4;

/// A light with a tile of a caster: its place in its list, its view and
/// projection, and the tile's shape.
type TileLight = (usize, Mat4, TileShape);

/// The tile the shader reads for a tile of `shape` at `rect` (texels) of
/// `layer`.
fn gpu_tile(view_proj: Mat4, rect: UVec4, shape: TileShape, layer: u32) -> GpuTile {
    let atlas = crate::shadow_tiles::ATLAS_SIZE as f32;
    GpuTile {
        view_proj,
        rect: rect.as_vec4() / atlas,
        span: Vec4::new(shape.x, 1.0 / atlas, layer as f32, 0.0),
        depth: Vec4::new(shape.y, shape.z, shape.w, 0.0),
    }
}

/// The light's view and projection for a tile of a caster in the sphere at
/// `centre` of `radius`. A lamp looks at the sphere and its view just holds
/// it. The sun looks along its direction and its view is a square as wide
/// as the sphere. Each depth range holds the sphere. The second value is
/// the tile's shape.
///
/// A lamp too near the sphere to fit it (`TILE_FIT`) gets no view, unless
/// `near_too` is set (Shadows at Low, where no capsules stand in). Its view
/// then looks at the sphere's centre, `NEAR_VIEW` radians to each side, and
/// its depth range starts `NEAR_START` metres from the light. The parts of
/// the caster outside that view cast nothing. A fight's light is most often
/// this near: an enemy's blade a metre from a character.
fn tile_view(
    light: &GpuLight,
    centre: Vec3,
    radius: f32,
    near_too: bool,
) -> Option<(Mat4, TileShape)> {
    let at = light.position_radius.truncate();
    let to_light = (at - centre).normalize_or_zero();
    if to_light == Vec3::ZERO || radius <= 0.0 {
        return None;
    }
    let up = if to_light.y.abs() > 0.99 {
        Vec3::X
    } else {
        Vec3::Y
    };
    if is_sun(light) {
        let eye = centre + to_light * radius * 2.0;
        let view = Mat4::look_at_rh(eye, centre, up);
        let projection =
            Mat4::orthographic_rh(-radius, radius, -radius, radius, radius, radius * 3.0);
        return Some((
            projection * view,
            Vec4::new(radius * 2.0, radius, radius * 3.0, 1.0),
        ));
    }
    let distance = at.distance(centre);
    let (half, near) = if distance >= radius * TILE_FIT {
        ((radius / distance).asin(), distance - radius)
    } else if near_too && distance > NEAR_START {
        (NEAR_VIEW, NEAR_START)
    } else {
        return None;
    };
    let far = distance + radius;
    let view = Mat4::look_at_rh(at, centre, up);
    let projection = Mat4::perspective_rh(half * 2.0, 1.0, near, far);
    Some((
        projection * view,
        Vec4::new(2.0 * distance * half.tan(), near, far, 0.0),
    ))
}

/// The lights of `bits` that get a tile of a caster in the sphere at
/// `centre` of `radius`, each with its view and the tile's shape: the
/// `most` lights that light it most, in the order of the light list. With
/// `low` set (Shadows at Low), a fight's light is weighed with the lamps
/// and the sun, and a light too near the caster gets a wide view
/// (`tile_view`). A fight's light's tile is seen from its point nearest the
/// caster. Without `low`, a fight's light keeps its capsules.
///
/// A sun always lights a caster most. With `low` set, the brightest of
/// several suns come first, so a weak sky light does not take a sun's
/// place (the Dune Sea has a sun 1 km wide beside its two suns).
fn tile_lights(
    lights: &[GpuLight],
    bits: u32,
    (centre, radius): (Vec3, f32),
    (most, low): (usize, bool),
) -> Vec<TileLight> {
    let mut chosen: Vec<((f32, f32), usize, Mat4, TileShape)> = lights
        .iter()
        .enumerate()
        .filter(|&(index, light)| bits & (1 << index) != 0 && (low || light.kind.z != LIGHT_COMBAT))
        .filter_map(|(index, light)| {
            let mut seen_from = *light;
            seen_from.position_radius = light_point(light, centre).extend(light.position_radius.w);
            let (view, width) = tile_view(&seen_from, centre, radius, low)?;
            let colour = light.color_multiplier;
            let bright = colour.truncate().max_element() * colour.w;
            let strength = if is_sun(light) {
                (f32::MAX, if low { bright } else { 0.0 })
            } else {
                let at = seen_from.position_radius.truncate();
                let near = 1.0 - (at.distance(centre) / light.position_radius.w).min(1.0);
                (bright * near * near, 0.0)
            };
            Some((strength, index, view, width))
        })
        .collect();
    chosen.sort_by(|a, b| b.0.0.total_cmp(&a.0.0).then(b.0.1.total_cmp(&a.0.1)));
    chosen.truncate(most);
    chosen.sort_by_key(|&(_, index, ..)| index);
    chosen
        .into_iter()
        .map(|(_, index, view, width)| (index, view, width))
        .collect()
}

/// The capsule holding both `one` and `other`. Its ends are the two of
/// their four ends farthest apart.
fn merged(one: (Vec3, Vec3, f32), other: (Vec3, Vec3, f32)) -> (Vec3, Vec3, f32) {
    let ends = [
        (one.0, one.2),
        (one.1, one.2),
        (other.0, other.2),
        (other.1, other.2),
    ];
    let (mut a, mut b, mut span) = (one.0, one.1, -1.0);
    for (i, &(from, _)) in ends.iter().enumerate() {
        for &(to, _) in &ends[i + 1..] {
            let apart = from.distance_squared(to);
            if apart > span {
                (a, b, span) = (from, to, apart);
            }
        }
    }
    let radius = ends
        .iter()
        .map(|&(end, r)| outside_capsules(&[(a, b, 0.0)], end) + r)
        .fold(0.0, f32::max);
    (a, b, radius)
}

/// A capsule's volume, taken as a cylinder with a ball's worth of caps.
fn volume((a, b, r): (Vec3, Vec3, f32)) -> f32 {
    std::f32::consts::PI * r * r * (a.distance(b) + r * 4.0 / 3.0)
}

/// At most `most` capsules that hold a character's `capsules`. The two
/// capsules whose merging adds the least volume are merged until `most`
/// are left, so limbs that lie together become one.
fn coarse(capsules: &[(Vec3, Vec3, f32)], most: usize) -> Vec<(Vec3, Vec3, f32)> {
    let mut kept = capsules.to_vec();
    while kept.len() > most.max(1) {
        let mut best = (0, 1, f32::MAX);
        for i in 0..kept.len() {
            for j in i + 1..kept.len() {
                let added = volume(merged(kept[i], kept[j])) - volume(kept[i]) - volume(kept[j]);
                if added < best.2 {
                    best = (i, j, added);
                }
            }
        }
        let (i, j, _) = best;
        kept[i] = merged(kept[i], kept[j]);
        kept.swap_remove(j);
    }
    kept
}

/// A placeable's box in Bevy world space, placed by its root.
pub(crate) fn world_box(shadow: &ShadowBox, placed: &GlobalTransform) -> GpuBox {
    let (scale, turn, _) = placed.to_scale_rotation_translation();
    let half = (shadow.high - shadow.low) * 0.5 * scale;
    let centre = placed.transform_point((shadow.low + shadow.high) * 0.5);
    GpuBox {
        centre: centre.extend(half.length()),
        x: (turn * Vec3::X).extend(half.x),
        y: (turn * Vec3::Y).extend(half.y),
        z: (turn * Vec3::Z).extend(half.z),
        unseen: UVec4::ZERO,
        tiles: UVec4::new(0, 0, BOX_CASTS, 0),
    }
}

/// The rooms the shadow of something whose lowest point is `low` (Bevy
/// world space) does not fall on: the rooms not linked with the room it
/// stands in. None when it stands in no room.
fn unseen_from(
    low: Vec3,
    scene: Option<&crate::scene::SceneContent>,
    culling: Option<&crate::scene::Culling>,
) -> UVec4 {
    let (Some(scene), Some(culling)) = (scene, culling) else {
        return UVec4::ZERO;
    };
    let at = crate::scene::game_to_bevy().inverse() * low;
    culling
        .fight
        .room_of(&scene.0.rooms, at)
        .map_or(UVec4::ZERO, |room| culling.fight.unseen(room))
}

/// A sphere holding every sphere of `spheres` grown by `reach`.
fn reach_of(spheres: impl Iterator<Item = Vec4> + Clone, reach: f32) -> Vec4 {
    let (low, high) = spheres.clone().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), s| (low.min(s.truncate() - s.w), high.max(s.truncate() + s.w)),
    );
    if low.x > high.x {
        return Vec4::ZERO;
    }
    let centre = (low + high) * 0.5;
    let radius = spheres
        .map(|s| centre.distance(s.truncate()) + s.w)
        .fold(0.0, f32::max);
    centre.extend(radius + reach)
}

/// Writes the shown placeables' boxes and kept tiles to `AREA_BOXES` when
/// one appears, moves, or is hidden or goes, and the kept tiles' plan for
/// the pass (`KeptPlan`). Most placeables never move, so this is done once
/// when an area is shown. It is done again when the Shadows setting
/// changes, the area's lights are set, or the level's walls arrive for the
/// lamps' sight of the placeables.
#[allow(clippy::type_complexity)]
fn place_boxes(
    placed: Query<(
        Entity,
        &ShadowBox,
        Has<TileOnly>,
        &crate::scene::SceneModel,
        &GlobalTransform,
        &InheritedVisibility,
    )>,
    (scene, culling, scene_lights): (
        Option<Res<crate::scene::SceneContent>>,
        Option<Res<crate::scene::Culling>>,
        Option<Res<crate::scene::SceneLights>>,
    ),
    (settings, casters): (Res<ShadowSettings>, Res<Casters>),
    (mut boxes, mut kept): (ResMut<Boxes>, ResMut<KeptPlan>),
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut planned_for: Local<Option<(usize, bool, ShadowSettings)>>,
) {
    let rooms_changed = culling.as_ref().is_some_and(|culling| culling.is_changed());
    let tiled = *settings == ShadowSettings::High && tiles_wanted().1;
    let all_lights = scene_lights
        .as_deref()
        .map_or(&[][..], |lights| &lights.all[..]);
    let lights_key = (all_lights.len(), casters.sight.is_some(), *settings);
    let mut now: Vec<(Entity, usize, GpuBox, [Option<usize>; 3])> = placed
        .iter()
        .filter(|&(_, _, only, .., shown)| {
            settings.on() && !settings.low() && shown.get() && (tiled || !only)
        })
        .map(|(entity, shadow, only, model, at, _)| {
            let mut gpu = world_box(shadow, at);
            if only {
                gpu.tiles.z = 0;
            }
            (entity, model.index, gpu, [None; 3])
        })
        .take(BOXES)
        .collect();
    let placed_now: Vec<(Entity, Vec4, Vec4, Vec4)> = now
        .iter()
        .map(|(entity, _, gpu, _)| (*entity, gpu.centre, gpu.x, gpu.y))
        .collect();
    if placed_now == boxes.placed && *planned_for == Some(lights_key) && !rooms_changed {
        return;
    }
    *planned_for = Some(lights_key);
    // The room each box stands in, found from the middle of its base.
    for (_, _, gpu, _) in &mut now {
        let half = Vec3::new(gpu.x.w, gpu.y.w, gpu.z.w);
        let up = Vec3::new(gpu.x.y, gpu.y.y, gpu.z.y).abs().dot(half);
        let base = gpu.centre.truncate() - Vec3::Y * up;
        gpu.unseen = unseen_from(base, scene.as_deref(), culling.as_deref());
    }
    let mut list = Box::new(BoxList::default());
    let mut plan = Vec::new();
    if tiled {
        plan_kept(
            all_lights,
            casters.sight.as_ref(),
            &mut now,
            &mut list,
            &mut plan,
        );
    }
    // A placeable that casts from its tiles alone and got none casts
    // nothing.
    now.retain(|(_, _, gpu, _)| gpu.tiles.z == BOX_CASTS || gpu.tiles.y > 0);
    list.count = now.len() as u32;
    for (slot, (_, _, gpu, _)) in list.boxes.iter_mut().zip(&now) {
        *slot = *gpu;
    }
    if let Some(mut buffer) = buffers.get_mut(&AREA_BOXES) {
        buffer.set_data(*list);
    }
    boxes.shown = now;
    boxes.placed = placed_now;
    kept.set_if_neq(KeptPlan(plan));
}

/// A kept tile's texels a side for a placeable of `radius` (metres).
fn kept_size(radius: f32) -> u32 {
    ((radius * KEPT_TEXELS).ceil().max(1.0) as u32)
        .next_power_of_two()
        .clamp(LEAST_KEPT, MOST_KEPT)
}

/// The free squares of a layer of the atlas, for packing kept tiles. Each
/// is its corner and its size, in texels. A square is split in four until
/// it is the size a tile needs.
struct Packer(Vec<UVec3>);

impl Packer {
    fn new() -> Packer {
        Packer(vec![UVec3::new(0, 0, crate::shadow_tiles::ATLAS_SIZE)])
    }

    /// The corner of a free square of `size` texels, or None when none is
    /// left.
    fn take(&mut self, size: u32) -> Option<UVec2> {
        let (index, _) = self
            .0
            .iter()
            .enumerate()
            .filter(|(_, free)| free.z >= size)
            .min_by_key(|(_, free)| free.z)?;
        let mut free = self.0.swap_remove(index);
        while free.z > size {
            let half = free.z / 2;
            for (x, y) in [(half, 0), (0, half), (half, half)] {
                self.0.push(UVec3::new(free.x + x, free.y + y, half));
            }
            free.z = half;
        }
        Some(free.xy())
    }
}

/// Plans the kept tiles of the placeables of `boxes`, from the area's
/// lights `all` (`kept_lights`). The largest placeables are packed first,
/// so when the layer is full the smallest go without. Writes each box's
/// tiles and their lights, the tiles to `list`, and the plan.
fn plan_kept(
    all: &[GpuLight],
    sight: Option<&Sight>,
    boxes: &mut [(Entity, usize, GpuBox, [Option<usize>; 3])],
    list: &mut BoxList,
    plan: &mut Vec<PlannedTile>,
) {
    let mut wanted: Vec<(usize, u32, Vec<TileLight>)> = boxes
        .iter()
        .enumerate()
        .filter_map(|(index, (_, _, gpu, _))| {
            let (centre, radius) = (gpu.centre.truncate(), gpu.centre.w);
            let lights = kept_lights(all, centre, radius, sight);
            (!lights.is_empty()).then(|| (index, kept_size(radius), lights))
        })
        .collect();
    wanted.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut packer = Packer::new();
    for (index, size, lights) in wanted {
        let first = list.tile_count;
        for (slot, (light, view_proj, shape)) in lights.into_iter().enumerate() {
            if list.tile_count as usize >= KEPT_TILES {
                break;
            }
            let Some(corner) = packer.take(size) else {
                break;
            };
            let rect = UVec4::new(corner.x, corner.y, size, size);
            list.tiles[list.tile_count as usize] =
                gpu_tile(view_proj, rect, shape, crate::shadow_tiles::KEPT_LAYER);
            plan.push(PlannedTile {
                model: boxes[index].1,
                view_proj,
                rect,
            });
            boxes[index].3[slot] = Some(light);
            list.tile_count += 1;
        }
        boxes[index].2.tiles.x = first;
        boxes[index].2.tiles.y = list.tile_count - first;
    }
}

/// The area's lights that get a kept tile of a placeable in the sphere at
/// `centre` of `radius`, each with its view and the tile's shape: its suns
/// and the `KEPT_LAMPS` lamps that light it most, those that see it past
/// the level's walls, `KEPT_LIGHTS` at most, in the order of `all`. The
/// sight test is `casting_lights`'s.
fn kept_lights(
    all: &[GpuLight],
    centre: Vec3,
    radius: f32,
    sight: Option<&Sight>,
) -> Vec<TileLight> {
    let chest = centre + Vec3::Y * (radius - SIGHT_BELOW_TOP).max(0.0);
    let sees = |light: &GpuLight| {
        sight.is_none_or(|sight| sight.sees(light, chest, radius) != Line::Blocked)
    };
    let mut lamps: Vec<(f32, usize)> = all
        .iter()
        .enumerate()
        .filter(|(_, light)| light.kind.x == 0 && light.kind.z != LIGHT_COMBAT && !is_sun(light))
        .filter_map(|(index, light)| {
            let at = light.position_radius.truncate();
            let reach = light.position_radius.w;
            let apart = at.distance(centre);
            if apart >= reach + radius {
                return None;
            }
            let colour = light.color_multiplier;
            let near = 1.0 - (apart / reach).min(1.0);
            Some((
                colour.truncate().max_element() * colour.w * near * near,
                index,
            ))
        })
        .collect();
    lamps.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut chosen: Vec<TileLight> = Vec::new();
    for (index, sun) in all.iter().enumerate() {
        if chosen.len() < KEPT_LIGHTS
            && sun.kind.x == 0
            && is_sun(sun)
            && let Some((view, width)) = tile_view(sun, centre, radius, false)
            && sees(sun)
        {
            chosen.push((index, view, width));
        }
    }
    let mut lamps_chosen = 0;
    for (_, index) in lamps {
        if lamps_chosen == KEPT_LAMPS || chosen.len() == KEPT_LIGHTS {
            break;
        }
        let light = &all[index];
        if let Some((view, width)) = tile_view(light, centre, radius, false)
            && sees(light)
        {
            chosen.push((index, view, width));
            lamps_chosen += 1;
        }
    }
    chosen.sort_by_key(|&(index, ..)| index);
    chosen
}

/// Writes the casters nearest the camera and their capsules for the
/// shader.
#[allow(clippy::too_many_arguments)]
fn upload(
    (casters, boxes, mut plan): (Res<Casters>, Res<Boxes>, ResMut<TilePlan>),
    held: Res<crate::shadow_tiles::HeldReach>,
    scene_lights: Option<Res<crate::scene::SceneLights>>,
    area: Res<AreaLights>,
    settings: Res<ShadowSettings>,
    cameras: Query<&GlobalTransform, With<crate::scene::FlyCamera>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut facts: ResMut<console::DumpFacts>,
    (mut last, mut seen, mut frame): (Local<Box<ShadowList>>, Local<Seen>, Local<u32>),
    (debug, mut doors): (Local<ShadowDebug>, Local<Option<Vec<u8>>>),
    (scene, culling): (
        Option<Res<crate::scene::SceneContent>>,
        Option<Res<crate::scene::Culling>>,
    ),
) {
    *frame = frame.wrapping_add(1);
    // A door opened or closed: every line is traced again.
    let now_doors = casters.sight.as_ref().and_then(|sight| sight.doors.clone());
    if *doors != now_doors {
        seen.0.clear();
        *doors = now_doors;
    }
    let eye = cameras
        .single()
        .map_or(Vec3::ZERO, GlobalTransform::translation);
    let lights = &area.0.lights[..(area.0.count as usize).min(area.0.lights.len())];
    let mut list = Box::new(ShadowList {
        debug: debug.0,
        low: u32::from(settings.low()),
        ..default()
    });
    // The lights about the casters nearest the camera, for the dump.
    let mut about: Vec<serde_json::Value> = Vec::new();
    let mut near: Vec<(&Caster, Vec3, f32)> = if settings.on() {
        casters
            .bodies
            .iter()
            .filter(|caster| !caster.capsules.is_empty())
            .map(|caster| {
                let (centre, radius) = bounds(&caster.capsules);
                (caster, centre, radius)
            })
            .collect()
    } else {
        Vec::new()
    };
    near.sort_by(|a, b| {
        a.1.distance_squared(eye)
            .total_cmp(&b.1.distance_squared(eye))
    });
    let mut placed = 0usize;
    let mut traced = Traced::default();
    plan.0.clear();
    for (caster, centre, radius) in near {
        let count = caster.capsules.len();
        if list.count as usize >= CASTERS {
            break;
        }
        // At Low drawn from capsules (`old_low`), a fight's light traces a
        // few merged capsules, written after the character's own.
        let merged_few = if *settings == ShadowSettings::Low && old_low() && count > COARSE_CAPSULES
        {
            coarse(&caster.capsules, COARSE_CAPSULES)
        } else {
            Vec::new()
        };
        // A character with too many capsules for what is left is skipped.
        // One with fewer may still fit.
        if placed + count + merged_few.len() > CAPSULES {
            continue;
        }
        // With Shadows at Low, a character without a tile casts from no
        // light, and its lights are not tested.
        let (tile_casters, most_tiles) = settings.tiles();
        let tiled = tiles_wanted().0 && (list.count as usize) < tile_casters;
        let mut bits = if settings.low() && !tiled {
            0
        } else {
            casting_lights(
                lights,
                (caster.root, centre, radius),
                (settings.fight_lights(), &|at| {
                    outside_capsules(&caster.capsules, at)
                }),
                (casters.sight.as_ref(), &mut seen, *frame),
                &mut traced,
            )
        };
        let low = caster
            .capsules
            .iter()
            .map(|&(a, b, r)| if a.y < b.y { a } else { b } - Vec3::Y * r)
            .fold(centre, |low, p| if p.y < low.y { p } else { low });
        let in_dump = facts.wanted() && about.len() < 4;
        if in_dump {
            let mut row = lights_about(lights, centre, radius, bits);
            let round = |v: Vec3| v.to_array().map(|x| (x * 100.0).round() / 100.0);
            row["capsules"] = caster
                .capsules
                .iter()
                .map(|&(a, b, r)| {
                    serde_json::json!([round(a), round(b), (r * 100.0).round() / 100.0])
                })
                .collect();
            about.push(row);
        }
        let unseen = unseen_from(low, scene.as_deref(), culling.as_deref());
        // The dump gives the nearest characters' lowest points and how many
        // rooms their shadows are kept from.
        let described = about.len() <= 4;
        if let Some(row) = about.last_mut()
            && described
            && row["rooms_kept_from"].is_null()
        {
            row["low"] = serde_json::json!(low.to_array());
            row["rooms_kept_from"] =
                serde_json::json!(unseen.to_array().map(u32::count_ones).iter().sum::<u32>());
        }
        // The characters nearest the camera get tiles under the lights
        // that light them most (`ShadowSettings::tiles`). A tile is fitted
        // to the capsules, a robe's hem and what the character holds. Its
        // sphere grows to match, as the shader passes over a caster whose
        // sphere a light's cone misses.
        let mut tile_bits = 0u32;
        let first_tile = list.tile_count;
        let mut sphere = centre.extend(radius);
        if tiled {
            let fitted = held
                .0
                .get(&caster.model)
                .map_or(centre.extend(radius + crate::shadow_tiles::HEM), |&held| {
                    spheres_joined(centre.extend(radius + crate::shadow_tiles::HEM), held)
                });
            let (centre, radius) = (fitted.truncate(), fitted.w);
            let chosen = tile_lights(lights, bits, (centre, radius), (most_tiles, settings.low()));
            for (index, view_proj, shape) in chosen {
                let slot = list.tile_count as usize;
                if slot >= TILES {
                    break;
                }
                let rect = crate::shadow_tiles::slot_rect(slot);
                list.tiles[slot] = gpu_tile(
                    view_proj,
                    rect,
                    shape,
                    crate::shadow_tiles::CHARACTERS_LAYER,
                );
                plan.0.push(PlannedTile {
                    model: caster.model,
                    view_proj,
                    rect,
                });
                tile_bits |= 1 << index;
                list.tile_count += 1;
            }
            if tile_bits != 0 {
                sphere = fitted;
            }
        }
        // With Shadows at Low, only the lights with a tile cast.
        if settings.low() {
            bits = tile_bits;
        }
        // The dump gives the lights with a tile of each of the nearest
        // characters, as bits of the light list.
        if let Some(row) = about.last_mut()
            && in_dump
        {
            row["tile_lights"] = serde_json::json!(tile_bits);
        }
        list.casters[list.count as usize] = GpuCaster {
            sphere,
            lights: UVec4::new(bits, placed as u32, count as u32, caster.model as u32),
            unseen,
            coarse: if merged_few.is_empty() {
                UVec4::new(placed as u32, count as u32, 0, 0)
            } else {
                UVec4::new((placed + count) as u32, merged_few.len() as u32, 0, 0)
            },
            tiles: UVec4::new(first_tile, tile_bits, 0, 0),
        };
        let written = caster.capsules.iter().chain(&merged_few);
        for (slot, &(a, b, r)) in list.capsules[placed..].iter_mut().zip(written) {
            *slot = GpuCapsule {
                a: a.extend(r),
                b: b.extend(0.0),
            };
        }
        placed += count + merged_few.len();
        list.count += 1;
    }
    // The boxes nearest the camera, each with the lights that cast its
    // shadow.
    let mut near_boxes: Vec<(usize, Vec3, f32, Entity, usize)> = boxes
        .shown
        .iter()
        .enumerate()
        .map(|(index, (root, model, gpu, _))| {
            (index, gpu.centre.truncate(), gpu.centre.w, *root, *model)
        })
        .collect();
    // Each scene light's place in the light list now, for the kept tiles'
    // lights.
    let active = scene_lights
        .as_deref()
        .and_then(|lights| lights.active.as_deref())
        .unwrap_or(&[]);
    let listed = |light: usize| {
        active
            .iter()
            .take(lights.len())
            .position(|&active| active == light)
    };
    near_boxes.sort_by(|a, b| {
        a.1.distance_squared(eye)
            .total_cmp(&b.1.distance_squared(eye))
    });
    for (slot, &(index, centre, radius, root, model)) in list.near_boxes.iter_mut().zip(&near_boxes)
    {
        let (_, _, gpu, tile_lights) = &boxes.shown[index];
        // A placeable that casts from its tiles alone casts nothing under a
        // fight's light.
        let fights = if gpu.tiles.z == BOX_CASTS {
            settings.fight_lights()
        } else {
            0
        };
        let bits = casting_lights(
            lights,
            (root, centre, radius),
            (fights, &|at| outside_box(gpu, at)),
            (casters.sight.as_ref(), &mut seen, *frame),
            &mut traced,
        );
        let mut packed = 0u32;
        for (tile, light) in tile_lights.iter().enumerate() {
            if let Some(at) = light.and_then(listed) {
                packed |= (at as u32 + 1) << (6 * tile);
            }
        }
        *slot = UVec4::new(index as u32, bits, model as u32, packed);
        list.box_count += 1;
    }
    list.reach = reach_of(
        list.casters[..list.count as usize]
            .iter()
            .map(|caster| caster.sphere),
        SHADOW_REACH,
    );
    list.box_reach = reach_of(
        list.near_boxes[..list.box_count as usize]
            .iter()
            .map(|near| boxes.shown[near.x as usize].2.centre),
        BOX_REACH,
    );
    // A test not asked for again this frame is of a character or a light
    // gone.
    let now = *frame;
    seen.0.retain(|_, (_, _, asked)| *asked == now);
    if **last != *list
        && let Some(mut buffer) = buffers.get_mut(&AREA_SHADOWS)
    {
        buffer.set_data(*list);
        *last = list;
    }
    if facts.wanted() {
        let shown = &last;
        // The boxes nearest the camera. Each gives its centre and its half
        // sizes (Bevy world space, metres).
        let round = |x: f32| (x * 100.0).round() / 100.0;
        let box_list: Vec<serde_json::Value> = near_boxes
            .iter()
            .take(8)
            .enumerate()
            .map(|(k, near)| {
                let (_, _, gpu, tile_lights) = &boxes.shown[near.0];
                let centre = gpu.centre.truncate().to_array().map(round);
                let half = [gpu.x.w, gpu.y.w, gpu.z.w].map(round);
                // Then the lights that cast its shadow (bits of the light
                // list), its kept tiles' lights in the list (6 bits each),
                // its tiles (first, count, whether its box casts) and its
                // tiles' lights in the scene's lights.
                let slot = shown.near_boxes[k];
                serde_json::json!([
                    centre,
                    half,
                    slot.y,
                    slot.w,
                    gpu.tiles.truncate().to_array(),
                    tile_lights
                ])
            })
            .collect();
        let pairs: u32 = shown.casters[..shown.count as usize]
            .iter()
            .map(|caster| caster.lights.x.count_ones())
            .sum();
        facts.0.insert(
            "shadows".into(),
            serde_json::json!({
                "candidates": casters.bodies.len(),
                "casters": shown.count,
                "capsules": placed,
                "boxes": boxes.shown.len(),
                "near_boxes": shown.box_count,
                "box_list": box_list,
                // The lights about the nearest box, as `nearest` gives them,
                // and each one's colour and strength.
                "box_lights": near_boxes.first().map(|near| {
                    let mut about = lights_about(lights, near.1, near.2, shown.near_boxes[0].y);
                    about["colours"] = lights
                        .iter()
                        .map(|light| light.color_multiplier.to_array().map(round))
                        .collect();
                    about
                }),
                "light_pairs": pairs,
                "tiles": shown.tile_count,
                "sight_tests": traced.tested,
                "blocked_pairs": traced.blocked,
                "solid_pairs": traced.solid,
                "deferred_pairs": traced.deferred,
                "held_lights": traced.held,
                "nearest": about,
            }),
        );
    }
}

/// The lights within reach of a caster, for the dump. Each gives its
/// place in the light list, its distance from the caster's centre and its
/// height over it (metres), its radius, whether it is a fight's light, and
/// whether it casts the caster's shadow.
fn lights_about(lights: &[GpuLight], centre: Vec3, radius: f32, bits: u32) -> serde_json::Value {
    let round = |x: f32| (x * 100.0).round() / 100.0;
    let rows: Vec<serde_json::Value> = lights
        .iter()
        .enumerate()
        .filter(|(_, light)| {
            let reach = light.position_radius.w + light.way.w + radius;
            light.kind.x == 0 && light.position_radius.truncate().distance(centre) < reach
        })
        .map(|(index, light)| {
            let at = light.position_radius.truncate();
            serde_json::json!([
                index,
                round(at.distance(centre)),
                round(at.y - centre.y),
                round(light.position_radius.w),
                light.kind.z == LIGHT_COMBAT,
                bits & (1 << index) != 0,
            ])
        })
        .collect();
    serde_json::json!({ "centre": centre.to_array().map(round), "radius": round(radius), "lights": rows })
}

#[cfg(feature = "probes")]
pub mod probes;

/// A room model's surfaces in Bevy world space, at rest. `frame` places it
/// in the game's space, and `to_bevy` turns that into Bevy's.
pub fn surfaces(placed: &crate::viewer::Placed, to_bevy: Quat, frame: Mat4) -> Vec<Surface> {
    let world = Mat4::from_quat(to_bevy) * frame;
    placed
        .ir
        .meshes
        .iter()
        .filter(|mesh| mesh.skin.is_none())
        .flat_map(|mesh| {
            let node = world * crate::scene::to_mat4(&placed.ir.nodes[mesh.node].world);
            let at = move |index: u32| {
                mesh.positions
                    .get(index as usize)
                    .map(|p| node.transform_point3(Vec3::from_array(*p)))
            };
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(move |&[a, b, c]| Some([at(a)?, at(b)?, at(c)?]))
        })
        .map(|corners| Surface { corners })
        .collect()
}

/// Where along `direction` from `origin` the ray meets `triangle`
/// (Möller–Trumbore), on either face.
pub(crate) fn ray_hit(origin: Vec3, direction: Vec3, [a, b, c]: &[Vec3; 3]) -> Option<f32> {
    let (ab, ac) = (*b - *a, *c - *a);
    let p = direction.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let to_origin = origin - *a;
    let u = to_origin.dot(p) / det;
    let q = to_origin.cross(ab);
    let v = direction.dot(q) / det;
    let t = ac.dot(q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
}
