//! The material for Odyssey meshes (see `odyssey.wgsl`): the diffuse lit
//! by its lightmap, self-illumination, the area's ambient and the nearest
//! lights, then the area's fog, combined as the games' fixed-function path
//! does (the texture modulated by the lightmap). A mesh without a lightmap
//! binds Bevy's white fallback image.
//!
//! The lights are not the material's own: it reads a light list held once
//! on the GPU (`AREA_LIGHTS`, kept by `scene::pick_lights` and by
//! `weather::light` for the lightning's flash, or a 3D view's stage), so a new set is one small upload and a material made later sees
//! the lights of the moment.
//!
//! What changes every frame (a scrolling texture's offset, a clip's keyed
//! alpha and self-illumination) is not the material's either: a material
//! animated so takes a slot of a list held once on the GPU (`ANIMATED`,
//! `Animated`), and the frame writes the list. A material written anew is
//! prepared and bound anew; the list is one small upload into the same
//! buffer.
//!
//! How a surface blends follows xoreos (blending always on):
//! opaque surfaces draw first, alpha-blended when their textures carry
//! alpha; transparent ones (a transparency hint above 1, or decal textures) draw
//! after, far to near, without writing depth. An additive texture (TXI
//! `blending additive`, with alpha or without) adds itself by its alpha to
//! what is behind, as KOTOR's executable sets it (`GL_SRC_ALPHA, GL_ONE`
//! from its blend table; xoreos multiplied by the colour instead).

use asset_odyssey::texture::{Blending, TextureFacts};
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorWrites,
    RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, VertexFormat,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::{ShaderDefVal, ShaderRef};

const SHADER: Handle<Shader> = uuid_handle!("5d0f3b8e-3f7c-4d8e-9a51-0b7c2e61d4a3");
const PREPASS_SHADER: Handle<Shader> = uuid_handle!("e2a7c5d1-0b84-4f3e-9c26-7d1f8a3b5e90");
/// A danglymesh's vertex stage in the camera's prepass: where each vertex
/// was last frame from `PREVIOUS_POSITION`, as `dangly::sway` left it.
const DANGLY_PREPASS_SHADER: Handle<Shader> = uuid_handle!("41c8e6b2-9d37-4a05-b8f1-6e2a0c7d93b4");
/// A motion proxy's vertex stage outside the prepass: every vertex off the
/// screen, so it draws nothing there (`motion_proxy`).
const NOWHERE_SHADER: Handle<Shader> = uuid_handle!("d93a0f57-2b6e-4c18-a7d4-85e1b3c6f209");
/// An emitter's particles' vertex stage: their places, texture coordinates
/// and colours read from its own list (`OdysseyMaterial::particles`).
const PARTICLES_SHADER: Handle<Shader> = uuid_handle!("a4d81f37-6c2e-4b95-8f03-1e7b9c5d2a68");
/// `kjk::lighting`, the flash and the fog this material and the grass share.
const LIGHTING: Handle<Shader> = uuid_handle!("c3a7e215-8d4b-4f6e-b0d2-5e91f7a4c836");

/// The area's lights, which every material reads unless given others.
pub const AREA_LIGHTS: Handle<ShaderBuffer> = uuid_handle!("b6e2a9d4-71c3-4f08-8e5a-3d9c0f6b2a17");

/// The animated materials' slots (`Animated`), which every material reads.
pub const ANIMATED: Handle<ShaderBuffer> = uuid_handle!("8f4b2c61-d93e-4a75-b018-6e2f9c3d7a54");

/// No particles' list: a material's own vertices place it.
pub const NO_PARTICLES: Handle<ShaderBuffer> = uuid_handle!("3e9b7d52-0a1c-4f86-b4e7-6d2c8f1a5b39");

/// A particles' list of one, for the pipeline warm-up's stand-ins
/// (`warmup`): a material reading any list but `NO_PARTICLES` takes the
/// particles' pipeline.
pub const STAND_IN_PARTICLES: Handle<ShaderBuffer> =
    uuid_handle!("7c15e8a3-94d2-4b60-a1f7-2e8d3b6c9f04");

/// How many materials can be animated through `ANIMATED` at once (slot 0
/// is none); past that, a material is written itself. Our chosen cap (the
/// shader's list is this long).
pub const ANIMATED_SLOTS: usize = 1024;

/// A danglymesh's vertices where `dangly::sway` had them the frame before
/// (its mesh's node space), for the motion the camera's prepass writes.
pub const PREVIOUS_POSITION: MeshVertexAttribute =
    MeshVertexAttribute::new("PreviousPosition", 988_540_930, VertexFormat::Float32x3);
/// Its shader location in the prepass (Bevy's own end at 7).
const PREVIOUS_POSITION_LOCATION: u32 = 8;

/// Where a motion proxy (`motion_proxy`) writes depth and motion: where the
/// surface it stands for is this opaque or more. Its surface blends by its
/// alpha with the depth written, and what the prepass covers hides what lies
/// behind from every later draw, so the cut is high: what a covered texel
/// leaves of the world behind it is at most 1 − this, too little to see.
pub const PROXY_SOLID: f32 = 0.98;

/// Shader flags (`OdysseyParams::flags`).
const DISCARD_ZERO_ALPHA: u32 = 1;
const OPAQUE_ALPHA: u32 = 2;
const ENVMAP: u32 = 4;
const HAS_LIGHTMAP: u32 = 8;
const STATIC: u32 = 16;
const UNLIT: u32 = 32;
const VERTEX_ALPHA: u32 = 64;
const FOG: u32 = 128;
const OPAQUE_TEXTURE: u32 = 256;
/// An added surface whose tint's alpha scales what it adds (a clip fades
/// it: `faded_additive`).
const ADD_FADE: u32 = 512;
/// Shadows fall on it (`shadows`): an opaque surface of a lit scene.
const SHADOWED: u32 = 1024;
/// An opaque surface of a room: its light is baked, and only the combat
/// lights (`LIGHT_COMBAT`) add to it, measured from each fragment.
const ROOM: u32 = 2048;
/// Its texture's hue is the tint's (`recoloured`).
const RECOLOUR: u32 = 4096;
/// Its env map is a flat texture, sphere-mapped (`sphere_envmap`).
const SPHERE_ENVMAP: u32 = 8192;
/// Lit evenly (`OdysseyMaterial::mine_dome`): the area's ambient whatever
/// the mesh's ambient colour, and every lamp's light whichever way the
/// surface faces.
const EVEN_LIGHT: u32 = 16384;

/// Most lights a surface is lit by at once: the nearest to the camera win.
/// Our chosen cap (the shader's light list is this long).
pub const MAX_LIGHTS: usize = 32;

/// A light as the shader reads it, in Bevy world space.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuLight {
    pub position_radius: Vec4,
    pub color_multiplier: Vec4,
    /// x: ambient only (0/1), y: dynamic type, z: `LIGHT_COMBAT` for a
    /// fight's short light (a bolt, a flash, a blade), which rooms take too.
    pub kind: UVec4,
    /// The line a fight's light lights from, such as a flying bolt's. The
    /// xyz part is the line's direction and w is half its length (metres).
    /// A light at a point has all four at 0.
    pub way: Vec4,
    /// The rooms a fight's light does not reach. Bit `r` (counting through
    /// x, then y, z and w) stands for room `r`, which a room's meshes carry
    /// as their `MeshTag` (`r + 1`). `scene::pick_lights` sets the bit of
    /// each room not linked with the light's own (`fight_rooms::linked`).
    /// Rooms past `VIS_ROOMS` are always reached.
    pub unseen: UVec4,
}

/// How many rooms `GpuLight::unseen` can keep a light from.
pub const VIS_ROOMS: usize = 128;

/// `GpuLight::kind.z` of a fight's light: it lights the rooms' baked
/// surfaces as well as what moves.
pub const LIGHT_COMBAT: u32 = 1;
/// `GpuLight::kind.y` of a light that reaches everything, static meshes too
/// (`odyssey.wgsl`).
pub const LIGHT_DYNAMIC_ALL: u32 = 1;

/// The lights a material is lit by, as the shader reads them: the
/// lightning's flash, and the first `count` of `lights`.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct LightList {
    /// The lightning's light (`weather::Flash`): xyz where (Bevy world
    /// space), w its radius (0: none).
    pub flash_at: Vec4,
    /// Its colour: its keyed colour × its multiplier, added to the stored
    /// (gamma-encoded) values the shader works on.
    pub flash_colour: Vec4,
    pub count: u32,
    /// 1 under the options' Enhanced lighting (`rolloff`), else 0. The
    /// shader then adds a fight's lights as linear light and lets the
    /// effects that glow pass white.
    pub enhanced: u32,
    pub lights: [GpuLight; MAX_LIGHTS],
}

/// What `AREA_LIGHTS` holds: the lights `scene::pick_lights` chose and the
/// lightning's flash (`weather`). Each changes its own part here and puts
/// the whole list up (`upload`), one small write either way.
#[derive(Resource, Default)]
pub struct AreaLights(pub LightList);

impl AreaLights {
    pub fn upload(&self, buffers: &mut Assets<ShaderBuffer>) {
        if let Some(mut buffer) = buffers.get_mut(&AREA_LIGHTS) {
            buffer.set_data(self.0);
        }
    }
}

impl LightList {
    /// The first `MAX_LIGHTS` of `lights`.
    pub fn of(lights: impl IntoIterator<Item = GpuLight>) -> LightList {
        let mut list = LightList::default();
        for (slot, light) in list.lights.iter_mut().zip(lights) {
            *slot = light;
            list.count += 1;
        }
        list
    }
}

/// A slot of `ANIMATED`, over its material's own values: its UV offset
/// added, its alpha times the tint's, its self-illumination in place of
/// the material's where a clip keys it.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct GpuAnimated {
    /// rgb: the self-illumination keyed, a: multiplies the tint's alpha.
    pub illum_alpha: Vec4,
    /// xy: added to the UVs, z: 1 where the self-illumination is keyed.
    pub uv: Vec4,
}

impl GpuAnimated {
    /// Leaves its material as it is.
    const NONE: GpuAnimated = GpuAnimated {
        illum_alpha: Vec4::new(0.0, 0.0, 0.0, 1.0),
        uv: Vec4::ZERO,
    };
}

/// A particle quad's vertex as `particles.wgsl` reads it: place (its
/// emitter's parent's space), texture coordinate and colour.
#[derive(ShaderType, Clone, Copy, Default)]
pub struct GpuParticle {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub colour: [f32; 4],
}

/// `GpuParticle`'s size in the list.
pub const PARTICLE_BYTES: usize = 36;

/// The materials animated every frame and their slots in `ANIMATED`,
/// which `upload_animated` writes when they change. A material takes a
/// slot as it is first animated (the one write of it); a copy of it keeps
/// its slot, and so its offset and keys, until it is animated itself. A
/// material that is gone gives its slot back (`prune`); a new scene's
/// materials take them afresh (`forget`).
#[derive(Resource)]
pub struct Animated {
    slots: Box<[GpuAnimated; ANIMATED_SLOTS]>,
    by_material: bevy::platform::collections::HashMap<AssetId<OdysseyMaterial>, u32>,
    /// Slots given back, taken before fresh ones.
    free: Vec<u32>,
    changed: bool,
}

impl Default for Animated {
    fn default() -> Animated {
        Animated {
            slots: Box::new([GpuAnimated::NONE; ANIMATED_SLOTS]),
            by_material: default(),
            free: Vec::new(),
            changed: false,
        }
    }
}

impl Animated {
    /// `material`'s slot, taken now if it has none (`first` made of the
    /// material as it takes it); none when every slot is taken.
    fn slot(
        &mut self,
        material: AssetId<OdysseyMaterial>,
        materials: &mut Assets<OdysseyMaterial>,
        first: impl FnOnce(&mut OdysseyMaterial),
    ) -> Option<usize> {
        if let Some(&slot) = self.by_material.get(&material) {
            return Some(slot as usize);
        }
        let fresh = self.by_material.len() + self.free.len() + 1;
        let slot = self.free.last().map_or(fresh, |&slot| slot as usize);
        if slot >= ANIMATED_SLOTS {
            return None;
        }
        let mut written = materials.get_mut(material)?;
        if self.free.last() == Some(&(slot as u32)) {
            self.free.pop();
        }
        // A copy goes on from where its original's slot is.
        self.slots[slot] = match written.params.slot as usize {
            0 => GpuAnimated::NONE,
            was => self.slots[was.min(ANIMATED_SLOTS - 1)],
        };
        written.params.slot = slot as u32;
        first(&mut written);
        self.by_material.insert(material, slot as u32);
        self.changed = true;
        Some(slot)
    }

    /// `material`'s texture scrolled by `offset`; false when no slot was
    /// left for it.
    pub fn scroll(
        &mut self,
        material: AssetId<OdysseyMaterial>,
        offset: Vec2,
        materials: &mut Assets<OdysseyMaterial>,
    ) -> bool {
        let Some(slot) = self.slot(material, materials, |_| {}) else {
            return false;
        };
        let uv = &mut self.slots[slot].uv;
        if Vec2::new(uv.x, uv.y) != offset {
            (uv.x, uv.y) = (offset.x, offset.y);
            self.changed = true;
        }
        true
    }

    /// `material` shown at `alpha` and glowing `illum` (a clip's keys): its
    /// own alpha set aside for them as it takes its slot; false when no
    /// slot was left for it.
    pub fn light(
        &mut self,
        material: AssetId<OdysseyMaterial>,
        (alpha, illum): (f32, Vec3),
        materials: &mut Assets<OdysseyMaterial>,
    ) -> bool {
        let Some(slot) = self.slot(material, materials, |m| m.params.tint.w = 1.0) else {
            return false;
        };
        let slot = &mut self.slots[slot];
        let now = GpuAnimated {
            illum_alpha: illum.extend(alpha),
            uv: Vec4::new(slot.uv.x, slot.uv.y, 1.0, 0.0),
        };
        if *slot != now {
            *slot = now;
            self.changed = true;
        }
        true
    }

    /// The slots of materials that are gone (their models despawned) are
    /// free again.
    pub fn prune(&mut self, materials: &Assets<OdysseyMaterial>) {
        let free = &mut self.free;
        self.by_material.retain(|&material, &mut slot| {
            let kept = materials.contains(material);
            if !kept {
                free.push(slot);
            }
            kept
        });
    }

    /// A new scene's: its materials take slots afresh.
    pub fn forget(&mut self) {
        self.by_material.clear();
        self.free.clear();
    }
}

/// Writes `ANIMATED` when a slot changed.
pub fn upload_animated(mut animated: ResMut<Animated>, mut buffers: ResMut<Assets<ShaderBuffer>>) {
    if std::mem::take(&mut animated.changed)
        && let Some(mut buffer) = buffers.get_mut(&ANIMATED)
    {
        buffer.set_data(*animated.slots);
    }
}

/// A mesh's own lighting inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    pub self_illum: [f32; 3],
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    /// The alpha controller; multiplies the output alpha (xoreos
    /// `fraggle.a *= _alpha`), and below 1 makes the mesh transparent
    /// (as reone treats a mesh).
    pub alpha: f32,
    pub is_static: bool,
    /// Room geometry is not lit at runtime: diffuse × lightmap, or the
    /// diffuse alone (xoreos). The owner's screenshots show unlightmapped
    /// room meshes (the ceiling strips) at full brightness, where reone's
    /// static-object rule darkens them.
    pub is_room: bool,
    /// The model's header lets the area's fog draw over it.
    pub fogged: bool,
}

/// The area's distance fog (ARE `SunFog*`): its colour, and the distances
/// from the eye where it begins and where it is whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub color: [f32; 3],
    pub near: f32,
    pub far: f32,
}

/// What the area lights every mesh with: its ambient colour for objects
/// without lightmaps, and its fog; and whether its lights' shadows fall on
/// its opaque surfaces (a module's world, not a 3D view's stage).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaLight {
    pub ambient: [f32; 3],
    pub fog: Option<Fog>,
    pub shadowed: bool,
}

impl AreaLight {
    /// No fog, no shadows.
    pub fn ambient(ambient: [f32; 3]) -> AreaLight {
        AreaLight {
            ambient,
            fog: None,
            shadowed: false,
        }
    }
}

impl Lighting {
    /// For the material cache.
    pub fn bits(&self) -> [u32; 11] {
        let [a, b, c] = self.self_illum.map(f32::to_bits);
        let [d, e, f] = self.ambient.map(f32::to_bits);
        let [g, h, i] = self.diffuse.map(f32::to_bits);
        [
            a,
            b,
            c,
            d,
            e,
            f,
            g,
            h,
            i,
            self.alpha.to_bits(),
            u32::from(self.is_static)
                | (u32::from(self.is_room) << 1)
                | (u32::from(self.fogged) << 2),
        ]
    }
}

pub struct OdysseyMaterialPlugin;

impl Plugin for OdysseyMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, LIGHTING, "lighting.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, SHADER, "odyssey.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, PARTICLES_SHADER, "particles.wgsl", Shader::from_wgsl);
        load_internal_asset!(
            app,
            PREPASS_SHADER,
            "odyssey_prepass.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            DANGLY_PREPASS_SHADER,
            "dangly_prepass.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(app, NOWHERE_SHADER, "nowhere.wgsl", Shader::from_wgsl);
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_LIGHTS, ShaderBuffer::from(LightList::default()));
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(
                &ANIMATED,
                ShaderBuffer::from([GpuAnimated::NONE; ANIMATED_SLOTS]),
            );
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&NO_PARTICLES, ShaderBuffer::from([GpuParticle::default()]));
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(
                &STAND_IN_PARTICLES,
                ShaderBuffer::from([GpuParticle::default()]),
            );
        app.init_resource::<AreaLights>()
            .init_resource::<Animated>()
            .add_systems(Last, upload_animated)
            .add_plugins((
                MaterialPlugin::<OdysseyMaterial>::default(),
                crate::motion_proxy::MotionProxyPlugin,
            ));
    }
}

/// How a surface is drawn. A new one goes in `Surface::ALL` too, so the
/// pipeline warm-up (`warmup`) compiles it before play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Surface {
    /// No alpha: replaces what is behind.
    #[default]
    Opaque,
    /// Opaque pass, blended by the texture's alpha; alpha 0 is discarded
    /// (reone) so it does not hide what is drawn later.
    OpaqueAlpha,
    /// An env-mapped texture's alpha is the env map's mask, not coverage:
    /// opaque, with the env map added where alpha is low.
    OpaqueEnvMasked,
    /// Transparent pass, alpha-blended, no depth writes.
    Blend,
    /// Transparent pass, `src × src alpha + dst` (the executable's blend
    /// table for a TXI `blending additive`: `GL_SRC_ALPHA, GL_ONE`), no
    /// depth writes.
    Additive,
    /// Transparent pass, `src + dst` (Quake 3 `blendFunc GL_ONE GL_ONE`,
    /// JA's saber glow and core), unlit, no depth writes.
    AddOne,
    /// Transparent pass, alpha-blended by the texture's alpha times the
    /// vertex alpha and tinted by the vertex colour (Quake 3 `blendFunc
    /// GL_SRC_ALPHA GL_ONE_MINUS_SRC_ALPHA` with `rgbGen vertex`, which
    /// brings `alphaGen vertex`: JA's sprites that fade by alpha), unlit,
    /// no depth writes.
    BlendVertex,
    /// Transparent pass, `src × src alpha + dst`: KOTOR's `lighten`
    /// particles, white flipbooks whose shape is their alpha, tinted and
    /// faded by the vertex colour and alpha; unlit, no depth writes. (A
    /// max blend left the character sheet's red fog far darker than the
    /// game's.)
    Lighten,
    /// Transparent pass, `src × dst + dst × src` (Quake 3 `blendFunc
    /// GL_DST_COLOR GL_SRC_COLOR`: JA's saber burn marks), unlit, no depth
    /// writes: twice the texture times what is behind, on the encoded
    /// values as the game's framebuffer had them.
    Modulate,
}

impl Surface {
    pub const ALL: [Surface; 9] = [
        Surface::Opaque,
        Surface::OpaqueAlpha,
        Surface::OpaqueEnvMasked,
        Surface::Blend,
        Surface::Additive,
        Surface::AddOne,
        Surface::BlendVertex,
        Surface::Lighten,
        Surface::Modulate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Surface::Opaque => "opaque",
            Surface::OpaqueAlpha => "opaque_alpha",
            Surface::OpaqueEnvMasked => "opaque_envmap_masked",
            Surface::Blend => "blend",
            Surface::Additive => "additive",
            Surface::AddOne => "add_one",
            Surface::BlendVertex => "blend_vertex",
            Surface::Lighten => "lighten",
            Surface::Modulate => "modulate",
        }
    }
}

/// A mesh's surface from its transparency hint and the facts of the
/// textures that loaded (diffuse and lightmap), as xoreos combines them,
/// but for alpha: the diffuse's alpha is coverage (and `alphamean` not 1),
/// whatever the lightmap's; a lightmap's own only when no diffuse loaded
/// (Manaan's `m26mg_01a`, as before). xoreos asks every texture for alpha, so a
/// lightmap (never with alpha) made a lightmapped mesh opaque; the game
/// cuts it out by its diffuse (High Quality Skyboxes II's Taris building
/// cards, `m02aa_10b`, lightmapped planes whose DXT5 hides a black field and
/// the author's mark). Decal only when every texture is one; additive when any texture says so, with
/// alpha or without, as KOTOR's executable reads a texture's TXI (xoreos
/// only without). KOTOR meshes always carry a hint, so a hint above 1 or a
/// decal makes a mesh transparent, but not a skinned one (`skinned`): a
/// character's skin is one closed mesh that a draw without depth shows
/// through itself (TSL's `pfhc06` carries 2 on its whole head, and its mouth
/// and nose showed through its face), so it blends with the depth kept
/// (`OpaqueAlpha`). Hint 1 does not either: TSL's heads carry it on the
/// whole face (`head_geo`, its hair 2), which drawn without depth showed its
/// mouth and eye sockets through it; as reone has it, the texture's alpha
/// decides (`OpaqueAlpha`), the depth kept. Deviation: an additive mesh without a hint is drawn in
/// the transparent pass too (xoreos draws it in the opaque pass, in object
/// order); the coverage report counts them. A bump-mapped texture's alpha
/// is its bump's mask, not coverage: opaque, as reone has it (xoreos ignores the bump map and
/// blends by it, so the Hutts and the rancor showed through); reone still
/// discards its alpha-0 texels, which would leave pinholes, so none are.
pub fn surface(
    (hint, skinned): (u32, bool),
    mesh_alpha: f32,
    (diffuse, loaded): (Option<&TextureFacts>, &[&TextureFacts]),
) -> Surface {
    if loaded.is_empty() {
        return Surface::Opaque;
    }
    let alpha = diffuse
        .or(loaded.first().copied())
        .is_some_and(|facts| facts.alpha && facts.alpha_mean != Some(1.0));
    let decal = loaded.iter().all(|facts| facts.decal);
    let additive = loaded
        .iter()
        .any(|facts| facts.blending == Blending::Additive);
    let envmap = loaded.iter().any(|facts| facts.envmap.is_some());
    let bumpmap = loaded.iter().any(|facts| facts.bumpmap.is_some());
    if additive {
        Surface::Additive
    } else if decal || mesh_alpha < 1.0 || (hint > 1 && !skinned) {
        Surface::Blend
    } else if alpha && envmap {
        Surface::OpaqueEnvMasked
    } else if hint > 1 || (alpha && !bumpmap) {
        Surface::OpaqueAlpha
    } else {
        Surface::Opaque
    }
}

/// A weapon's surface, as kjk draws it (`rules::weapons_in_hand::look::SOLID`):
/// a texture with alpha that no TXI gives a meaning to (no env map, bump
/// map, blending, decal or animation) is drawn solid. A kjk choice, not
/// KOTOR's: its executable blends such a mesh by its alpha over what is
/// behind it, and mods that leave out their TXIs (High Quality Blasters,
/// whose alpha is an env-map mask) showed the world through their guns
/// (issue 171). Any other surface, and every surface of a model that is not
/// a weapon (`weapon`), is `surface` unchanged.
pub fn weapon_surface(surface: Surface, loaded: &[&TextureFacts], weapon: bool) -> Surface {
    let untold = |facts: &&TextureFacts| {
        facts.blending == Blending::Default
            && !facts.decal
            && facts.envmap.is_none()
            && facts.bumpmap.is_none()
            && facts.odd_blending.is_none()
            && facts.cycle.is_none()
    };
    if rules::weapons_in_hand::look::SOLID
        && weapon
        && surface == Surface::OpaqueAlpha
        && loaded.iter().all(untold)
    {
        Surface::Opaque
    } else {
        surface
    }
}

/// Whether `model` is a weapon's: every weapon model the games' items name
/// (`baseitems.2da` `defaultmodel`, a variation added) begins `w_`.
pub fn is_weapon_model(model: &str) -> bool {
    model
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("w_"))
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct OdysseyParams {
    /// Multiplies the diffuse; stored values, not linear.
    pub tint: Vec4,
    pub self_illum: Vec4,
    pub ambient: Vec4,
    pub diffuse: Vec4,
    pub world_ambient: Vec4,
    pub fog_color: Vec4,
    /// x: where the fog begins, y: where it is whole (eye distance).
    pub fog_range: Vec4,
    /// Added to the diffuse UVs (`animateuv`).
    pub uv_offset: Vec2,
    pub flags: u32,
    /// Its slot of `ANIMATED` (`Animated`); 0 for none.
    pub slot: u32,
    /// x: what the colour is multiplied by under the options' Enhanced
    /// lighting. It is 1 on everything but the effects that glow, such as
    /// a lightsaber's blade (`OdysseyMaterial::glowing`).
    pub glow: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(OdysseyKey)]
pub struct OdysseyMaterial {
    #[uniform(0)]
    pub params: OdysseyParams,
    #[texture(1)]
    #[sampler(2)]
    pub base: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub lightmap: Option<Handle<Image>>,
    /// A cube map. KOTOR's env-mapped surfaces add it, weighted by one minus
    /// the diffuse alpha (xoreos and reone both add it, `color += envmap × (1 − a)`).
    #[texture(5, dimension = "cube")]
    #[sampler(6)]
    pub envmap: Option<Handle<Image>>,
    /// The lights it is lit by: `AREA_LIGHTS` unless set.
    #[storage(7, read_only)]
    pub lights: Handle<ShaderBuffer>,
    /// The characters casting shadows now, and their capsules
    /// (`shadows::AREA_SHADOWS`).
    #[storage(8, read_only)]
    pub shadows: Handle<ShaderBuffer>,
    /// The placeables' boxes (`shadows::AREA_BOXES`).
    #[storage(13, read_only)]
    pub boxes: Handle<ShaderBuffer>,
    /// The animated materials' slots (`ANIMATED`).
    #[storage(9, read_only)]
    pub animated: Handle<ShaderBuffer>,
    /// An emitter's particles, four `GpuParticle` vertices a quad, which
    /// place its mesh's vertices by their index (`model_fx::Spray`):
    /// written in place each frame, where a mesh written anew is freed and
    /// allocated again on the GPU. `NO_PARTICLES` for the rest.
    #[storage(10, read_only)]
    pub particles: Handle<ShaderBuffer>,
    /// A flat env map, sphere-mapped (`sphere_envmap`), where `envmap` is
    /// none.
    #[texture(11)]
    #[sampler(12)]
    pub sphere_envmap: Option<Handle<Image>>,
    /// The shadow tiles' atlas (`shadow_tiles::ATLAS`), a layer for the
    /// characters' tiles and one for the placeables', read through a
    /// comparison sampler.
    #[texture(14, sample_type = "depth", dimension = "2d_array")]
    #[sampler(15, sampler_type = "comparison")]
    pub shadow_tiles: Option<Handle<Image>>,
    pub surface: Surface,
    /// Its back faces are not drawn (reone and xoreos cull a KOTOR mesh's
    /// back faces, a decal texture's excepted): a camera behind a wall or
    /// under a floor sees through it, or inside a character's head through
    /// it. Set for every model's mesh but decals (`scene`); effects' cards
    /// and particles are drawn from both sides.
    pub culled: bool,
    /// An emitter's particles, and their place among their model's
    /// emitters: drawn after every other surface, in that order
    /// (`EMITTERS_LAST`).
    pub emitted: Option<u32>,
    /// A motion proxy's (`motion_only`).
    pub motion_only: bool,
}

impl OdysseyMaterial {
    /// Drawn added (`src + dst`), unlit, by its alpha (a clip fades it): a
    /// glow card a clip keys (the swoop's thruster cards, its start lights'
    /// flares). reone adds such a texture's colour, its alpha taken from
    /// its brightness, which comes to the same; the texture's own alpha is
    /// not coverage here.
    pub fn faded_additive(mut self) -> OdysseyMaterial {
        self.surface = Surface::AddOne;
        self.params.flags &= !(DISCARD_ZERO_ALPHA | OPAQUE_ALPHA | VERTEX_ALPHA);
        self.params.flags |= UNLIT | ADD_FADE;
        self
    }

    /// Drawn whole, unblended and unlit, where its texture's alpha (times
    /// the vertex alpha) is above nothing: KOTOR's punch-through particles,
    /// which its executable draws with blending off and its alpha test at
    /// its default (`GL_GREATER`, 0).
    pub fn punch_through(mut self) -> OdysseyMaterial {
        self.surface = Surface::Opaque;
        self.params.flags &= !OPAQUE_ALPHA;
        self.params.flags |= UNLIT | DISCARD_ZERO_ALPHA | VERTEX_ALPHA;
        self
    }

    /// Its texture's hue replaced by `colour`, its light and white kept: each
    /// texel's whitest part (its least channel) stays white, the rest of
    /// its brightness takes `colour`. A JA blade in a colour JA has none of
    /// (TSL's silver, cyan, viridian, bronze), coloured by KOTOR's own blade
    /// texture (`weapons::own_colour`).
    pub fn recoloured(mut self, colour: [f32; 3]) -> OdysseyMaterial {
        self.params.tint = Vec4::new(colour[0], colour[1], colour[2], self.params.tint.w);
        self.params.flags |= RECOLOUR;
        self
    }

    /// The diffuse texture has no alpha (a DXT1, an RGB image): it is taken
    /// as 1, so a DXT1 kept in blocks samples its transparent texel as the
    /// games drew it, opaque black.
    pub fn opaque_texture(mut self, opaque: bool) -> OdysseyMaterial {
        if opaque {
            self.params.flags |= OPAQUE_TEXTURE;
        }
        self
    }

    pub fn new(
        tint: Vec4,
        base: Option<Handle<Image>>,
        lightmap: Option<Handle<Image>>,
        envmap: Option<Handle<Image>>,
        surface: Surface,
        lighting: &Lighting,
        area: &AreaLight,
    ) -> OdysseyMaterial {
        let mut flags = surface_flags(surface);
        if envmap.is_some() {
            flags |= ENVMAP;
        }
        if lightmap.is_some() {
            flags |= HAS_LIGHTMAP;
        }
        if lighting.is_static {
            flags |= STATIC;
        }
        // As reone has it, lighting is off when the diffuse is additive.
        if matches!(
            surface,
            Surface::Additive
                | Surface::AddOne
                | Surface::BlendVertex
                | Surface::Lighten
                | Surface::Modulate
        ) || lighting.is_room
        {
            flags |= UNLIT;
        }
        // reone fogs the opaque pass only, for models
        // whose header asks for it.
        let fog = area.fog.filter(|_| {
            lighting.fogged
                && matches!(
                    surface,
                    Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked
                )
        });
        if fog.is_some() {
            flags |= FOG;
        }
        let opaque = matches!(
            surface,
            Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked
        );
        if area.shadowed && opaque {
            flags |= SHADOWED;
        }
        if lighting.is_room && opaque {
            flags |= ROOM;
        }
        let colour = |rgb: [f32; 3]| Vec4::new(rgb[0], rgb[1], rgb[2], 1.0);
        // An added surface is drawn unlit, but the games show it in its
        // self-illumination's colour (the energy shields' spheres, red or
        // blue on a grey texture). A mine's dome is lit instead
        // (`mine_dome`).
        let glow = if matches!(surface, Surface::Additive | Surface::AddOne)
            && !lighting.is_room
            && lighting.self_illum != [0.0; 3]
        {
            colour(lighting.self_illum)
        } else {
            Vec4::ONE
        };
        OdysseyMaterial {
            params: OdysseyParams {
                tint: tint * glow * Vec4::new(1.0, 1.0, 1.0, lighting.alpha),
                self_illum: colour(lighting.self_illum),
                ambient: colour(lighting.ambient),
                diffuse: colour(lighting.diffuse),
                world_ambient: colour(area.ambient),
                fog_color: fog.map_or(Vec4::ZERO, |fog| colour(fog.color)),
                fog_range: fog.map_or(Vec4::ZERO, |fog| Vec4::new(fog.near, fog.far, 0.0, 0.0)),
                uv_offset: Vec2::ZERO,
                flags,
                slot: 0,
                glow: Vec4::ONE,
            },
            base,
            lightmap,
            envmap,
            lights: AREA_LIGHTS,
            shadows: crate::shadows::AREA_SHADOWS,
            boxes: crate::shadows::AREA_BOXES,
            animated: ANIMATED,
            particles: NO_PARTICLES,
            sphere_envmap: None,
            shadow_tiles: Some(crate::shadow_tiles::ATLAS),
            surface,
            culled: false,
            emitted: None,
            motion_only: false,
        }
    }

    /// Env-mapped by a flat texture rather than a cube map: KOTOR's
    /// executable sphere-maps an env map that is not a cube (fixed-function
    /// sphere-map texture coordinates), the texture's alpha its mask as a
    /// cube's.
    pub fn sphere_envmap(mut self, envmap: Option<Handle<Image>>) -> OdysseyMaterial {
        if envmap.is_some() {
            self.params.flags |= ENVMAP | SPHERE_ENVMAP;
        }
        self.sphere_envmap = envmap;
        self
    }

    /// The same light on no surface of its own: opaque, untextured, without
    /// its glow, and in its vertices' colours (a wound's cap, lit as the body
    /// it is cut from).
    pub fn bare(mut self) -> OdysseyMaterial {
        self.params.tint = Vec4::ONE;
        self.params.self_illum = Vec4::new(0.0, 0.0, 0.0, 1.0);
        self.params.uv_offset = Vec2::ZERO;
        self.params.slot = 0;
        self.params.flags &= STATIC | UNLIT | FOG | SHADOWED;
        self.base = None;
        self.lightmap = None;
        self.envmap = None;
        self.sphere_envmap = None;
        self.surface = Surface::Opaque;
        self.culled = true;
        self
    }

    /// The same as another surface, with the flags `new` gives that surface
    /// (the pipeline warm-up's stand-ins: `warmup`).
    pub fn as_surface(mut self, surface: Surface) -> OdysseyMaterial {
        let all = DISCARD_ZERO_ALPHA | OPAQUE_ALPHA | VERTEX_ALPHA;
        self.params.flags = (self.params.flags & !all) | surface_flags(surface);
        self.surface = surface;
        self
    }

    /// The same surface as a motion proxy's (`motion_proxy`): drawn only
    /// into the camera's prepass, its depth and motion where the surface is
    /// solid (`PROXY_SOLID`), and nowhere else.
    pub fn motion_only(mut self) -> OdysseyMaterial {
        self.lightmap = None;
        self.envmap = None;
        self.motion_only = true;
        self
    }

    /// The same material, `glow` times as bright under the options'
    /// Enhanced lighting. Above 1 it passes white there, and bloom spreads
    /// what is above white (`rolloff`). Under Classic lighting it changes
    /// nothing.
    pub fn glowing(mut self, glow: f32) -> OdysseyMaterial {
        self.params.glow.x = glow;
        self
    }

    /// Whether its texture's alpha is its coverage: a surface cut out by
    /// its alpha (leaves, a grate, hair), whose alpha is not an env map's
    /// mask and whose texture has alpha at all. Its shadow tile is cut by
    /// the alpha too (`shadow_tiles`).
    pub fn alpha_is_coverage(&self) -> bool {
        self.surface == Surface::OpaqueAlpha
            && self.base.is_some()
            && self.params.flags & (OPAQUE_TEXTURE | ENVMAP | SPHERE_ENVMAP) == 0
    }

    /// Whether a motion proxy stands for a mesh drawn with it: a blended
    /// opaque surface (`OpaqueAlpha`) writes depth in the sorted phase,
    /// which Bevy's prepass never draws.
    pub fn wants_proxy(&self) -> bool {
        self.surface == Surface::OpaqueAlpha
            && !self.motion_only
            && self.particles.id() == NO_PARTICLES.id()
    }

    /// What of it a motion proxy's material copies (`motion_only`): when
    /// this changes, the proxy's material is made anew.
    pub fn motion_facts(&self) -> MotionFacts {
        MotionFacts {
            tint_alpha: self.params.tint.w.to_bits(),
            uv_offset: self.params.uv_offset.to_array().map(f32::to_bits),
            flags: self.params.flags,
            slot: self.params.slot,
            base: self.base.as_ref().map(Handle::id),
            surface: self.surface,
            culled: self.culled,
        }
    }

    /// A mine's dome, drawn as the games draw it. Its texture is added to
    /// what is behind it, as any added surface is. Unlike kjk's other added
    /// surfaces it is lit: the texture is scaled by the mesh's
    /// self-illumination plus the light of the place, and that sum stops at
    /// full. So in daylight the dome is the texture's own pale colour, and
    /// in a dark place it keeps the self-illumination's red.
    ///
    /// The light of the place reaches the dome evenly (`EVEN_LIGHT`). The
    /// dome takes the area's ambient colour although its mesh's own ambient
    /// colour is black, and it takes each lamp's light whichever way a facet
    /// faces. This is read off the owner's screenshot of KOTOR 2 (Khoonda's
    /// west minefield, the player's own frag mines), not off the program.
    /// There every facet of the near dome has the same pale colour, also
    /// the facets turned away from the area's one far lamp. The dome adds
    /// the texture's own colour to the grass behind it, about its mean once
    /// for each layer, which is what full light gives.
    ///
    /// Its back faces are culled, although its texture is a decal: the dome
    /// is two shells, one facing out and one facing in, so the eye sees two
    /// layers of it, not four.
    pub fn mine_dome(&mut self) {
        self.params.flags &= !UNLIT;
        self.params.flags |= EVEN_LIGHT;
        self.params.tint = Vec3::ONE.extend(self.params.tint.w);
        self.culled = true;
    }

    /// Its back faces culled (`culled`), or not.
    pub fn culled(mut self, culled: bool) -> OdysseyMaterial {
        self.culled = culled;
        self
    }

    /// An emitter's particles', `rank`th among their model's emitters
    /// (`emitted`).
    pub fn emitted(mut self, rank: u32) -> OdysseyMaterial {
        self.emitted = Some(rank);
        self
    }
}

/// What a motion proxy's material copies of its mesh's (`motion_facts`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MotionFacts {
    tint_alpha: u32,
    uv_offset: [u32; 2],
    flags: u32,
    slot: u32,
    base: Option<AssetId<Image>>,
    surface: Surface,
    culled: bool,
}

/// The flags a surface draws with (`OdysseyMaterial::new`).
fn surface_flags(surface: Surface) -> u32 {
    match surface {
        Surface::OpaqueAlpha => DISCARD_ZERO_ALPHA,
        Surface::OpaqueEnvMasked => OPAQUE_ALPHA,
        Surface::BlendVertex | Surface::Lighten => VERTEX_ALPHA,
        _ => 0,
    }
}

/// `depth_bias` of a blended opaque surface (one that writes depth: this
/// material's `OpaqueAlpha`, the grass): farther than anything in view,
/// keeping their order among themselves.
pub const OPAQUE_FIRST: f32 = -1.0e5;

/// `depth_bias` of an emitter's particles: nearer than anything in view,
/// so they are drawn after the meshes, as KOTOR draws its emitters after
/// its models; the depth the meshes wrote still hides those behind them.
/// A model's emitters then go in their rank's order (`EMITTER_STEP` apart,
/// more than any distance in view), as KOTOR orders them, not by distance:
/// two of the main menu's at one distance swapped as they were sorted, and
/// the smoke in front of its figure came and went.
pub const EMITTERS_LAST: f32 = 1.0e5;
/// How far apart two ranks of emitters sort (`EMITTERS_LAST`).
const EMITTER_STEP: f32 = 1.0e3;

/// What a material's pipeline depends on: its blend and whether it writes
/// depth. Surfaces that agree on both (opaque and env-masked opaque; blend
/// and vertex-alpha blend) share a pipeline rather than each compiling one.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct OdysseyKey {
    blend: Option<BlendState>,
    depth_write: bool,
    culled: bool,
    particles: bool,
    /// It discards alpha 0 (`DISCARD_ZERO_ALPHA`): its prepass does too.
    cut: bool,
    motion_only: bool,
}

impl From<&OdysseyMaterial> for OdysseyKey {
    fn from(material: &OdysseyMaterial) -> OdysseyKey {
        let surface = material.surface;
        OdysseyKey {
            blend: blend(surface),
            depth_write: writes_depth(surface),
            culled: material.culled,
            particles: material.particles.id() != NO_PARTICLES.id(),
            cut: material.params.flags & DISCARD_ZERO_ALPHA != 0,
            motion_only: material.motion_only,
        }
    }
}

/// Whether a surface writes depth: the opaque ones, and the blended ones
/// drawn as the opaque pass is (`OpaqueAlpha`); what is truly see-through
/// does not, so its order among the see-through decides what shows.
pub fn writes_depth(surface: Surface) -> bool {
    !matches!(
        surface,
        Surface::Blend
            | Surface::Additive
            | Surface::AddOne
            | Surface::BlendVertex
            | Surface::Lighten
            | Surface::Modulate
    )
}

/// How a surface blends with what is drawn: none for an opaque one.
pub fn blend(surface: Surface) -> Option<BlendState> {
    match surface {
        Surface::Opaque | Surface::OpaqueEnvMasked => None,
        Surface::OpaqueAlpha | Surface::Blend | Surface::BlendVertex => {
            Some(BlendState::ALPHA_BLENDING)
        }
        Surface::AddOne => Some(BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        }),
        Surface::Modulate => Some(BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::Dst,
                dst_factor: BlendFactor::Src,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        }),
        Surface::Additive | Surface::Lighten => Some(BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        }),
    }
}

impl Material for OdysseyMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    /// Picks the phase: transparent surfaces go to Bevy's sorted
    /// transparent phase; the blend itself is set in `specialize`.
    fn alpha_mode(&self) -> AlphaMode {
        // A motion proxy is cut where it is solid, so Bevy's prepass draws
        // it (it skips every blended mode).
        if self.motion_only {
            return AlphaMode::Mask(PROXY_SOLID);
        }
        match self.surface {
            // Blended opaque surfaces go to the sorted phase as well, with
            // depth writes kept: xoreos draws its opaque pass far to near,
            // and Bevy's opaque phase has no fixed order, so blending there
            // changed from run to run (the tour's Dantooine foliage).
            Surface::Blend | Surface::OpaqueAlpha | Surface::BlendVertex | Surface::Modulate => {
                AlphaMode::Blend
            }
            Surface::Additive | Surface::AddOne | Surface::Lighten => AlphaMode::Add,
            _ => AlphaMode::Opaque,
        }
    }

    /// Blended opaque surfaces come first in the sorted phase: as the opaque
    /// pass they are, before everything blended, however near. Sorted among
    /// the blended, one nearer than a cell's force field by its middle drew
    /// over the field (TSL's Telos cells: the Exile's bare body, whose texture
    /// has alpha, not Kreia's robe, which has none).
    fn depth_bias(&self) -> f32 {
        match self.surface {
            Surface::OpaqueAlpha => OPAQUE_FIRST,
            _ => self
                .emitted
                .map_or(0.0, |rank| EMITTERS_LAST + rank as f32 * EMITTER_STEP),
        }
    }

    /// A camera with a temporal resolve (`temporal`) draws depth and motion
    /// first: each opaque surface as its main pass does, the blended opaque
    /// ones through their motion proxies (`motion_proxy`).
    fn enable_prepass() -> bool {
        true
    }

    /// No light draws a shadow map. Characters and placeables cast from
    /// capsules and boxes (`shadows`).
    fn enable_shadows() -> bool {
        false
    }

    /// In a camera's prepass a surface covers what its main pass does: an
    /// alpha-tested one (hair, grates) the shape its texture cuts out
    /// (`odyssey_prepass.wgsl`).
    fn prepass_fragment_shader() -> ShaderRef {
        ShaderRef::Handle(PREPASS_SHADER)
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let key = key.bind_group_data;
        let defs = &descriptor.vertex.shader_defs;
        let prepass = defs.contains(&"PREPASS_PIPELINE".into());
        let cull_mode = key
            .culled
            .then_some(bevy::render::render_resource::Face::Back);
        if prepass {
            // What the main pass draws, from the same faces and vertices:
            // the main pass tests against this depth.
            descriptor.primitive.cull_mode = cull_mode;
            let skinned = defs.contains(&"SKINNED".into());
            if key.particles {
                descriptor.vertex.shader = PARTICLES_SHADER;
            } else if !skinned && layout.0.contains(PREVIOUS_POSITION) {
                let previous = layout.0.get_layout(&[
                    PREVIOUS_POSITION.at_shader_location(PREVIOUS_POSITION_LOCATION)
                ])?;
                if let Some(buffer) = descriptor.vertex.buffers.first_mut() {
                    buffer.attributes.extend(previous.attributes);
                }
                descriptor.vertex.shader = DANGLY_PREPASS_SHADER;
            }
            if let Some(fragment) = descriptor.fragment.as_mut() {
                if key.cut {
                    fragment.shader_defs.push("CUT_ZERO_ALPHA".into());
                }
                if key.motion_only {
                    fragment.shader_defs.push("MOTION_PROXY".into());
                    fragment.shader_defs.push(ShaderDefVal::UInt(
                        "PROXY_SOLID_PERMILLE".into(),
                        (PROXY_SOLID * 1000.0).round() as u32,
                    ));
                }
            }
            return Ok(());
        }
        if key.motion_only {
            // The main pass is the mesh's own: its proxy draws nothing here.
            descriptor.vertex.shader = NOWHERE_SHADER;
            if let Some(fragment) = descriptor.fragment.as_mut() {
                for target in fragment.targets.iter_mut().flatten() {
                    target.write_mask = ColorWrites::empty();
                }
            }
            if let Some(depth) = descriptor.depth_stencil.as_mut() {
                depth.depth_write_enabled = Some(false);
            }
            return Ok(());
        }
        if key.particles {
            descriptor.vertex.shader = PARTICLES_SHADER;
        }
        descriptor.primitive.cull_mode = cull_mode;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = key.blend;
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(key.depth_write);
        }
        Ok(())
    }
}
