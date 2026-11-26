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
use bevy::ecs::system::SystemParamItem;
use bevy::ecs::system::lifetimeless::SRes;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, AsBindGroupError, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BlendComponent, BlendFactor, BlendOperation, BlendState,
    BufferBindingType, ColorWrites, CompareFunction, PipelineCache, PreparedBindGroup,
    RenderPipelineDescriptor, ShaderStages, ShaderType, SpecializedMeshPipelineError,
    UnpreparedBindGroup,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::storage::ShaderBuffer;
use bevy::shader::{ShaderDefVal, ShaderRef};

const SHADER: Handle<Shader> = uuid_handle!("5d0f3b8e-3f7c-4d8e-9a51-0b7c2e61d4a3");
/// `odyssey.wgsl` with `enable wgpu_ray_query;` put in front of it. That
/// line must be a shader's first, and Bevy's shader composer reads it
/// before `#ifdef`, so it cannot be left out of one file for a card
/// without ray queries. The main pass uses this copy only when the
/// acceleration structures are available and the view blends its frames
/// (`specialize`), so a card without ray queries never compiles it.
const RAYS_SHADER: Handle<Shader> = uuid_handle!("9e3c6a14-58d2-4b7f-a0e9-2d4f81c5b736");
const PREPASS_SHADER: Handle<Shader> = uuid_handle!("e2a7c5d1-0b84-4f3e-9c26-7d1f8a3b5e90");
const DANGLY_SHADER: Handle<Shader> = uuid_handle!("41c8e6b2-9d37-4a05-b8f1-6e2a0c7d93b4");
/// A motion proxy's vertex stage outside the prepass: every vertex off the
/// screen, so it draws nothing there (`motion_proxy`).
const NOWHERE_SHADER: Handle<Shader> = uuid_handle!("d93a0f57-2b6e-4c18-a7d4-85e1b3c6f209");
/// An emitter's particles' vertex stage: their places, texture coordinates
/// and colours read from its own list (`OdysseyMaterial::particles`).
pub(crate) const PARTICLES_SHADER: Handle<Shader> =
    uuid_handle!("a4d81f37-6c2e-4b95-8f03-1e7b9c5d2a68");
/// It has a handle of its own so that Effects Quality does not take these
/// quads for particles.
const LISTED_QUADS_SHADER: Handle<Shader> = uuid_handle!("5b0e3f92-7a41-4c6d-9e18-c2d4a7f60b35");
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
/// A model of an area, lit under Enhanced lighting by the light probes
/// where it stands (`light_probes`) instead of the area's flat ambient.
/// Screens that draw a model in their own light do not set it.
const PROBED: u32 = 32768;
/// A room's lamp that the files do not set to glow
/// (`fitted_lights::switched_on`): its glow shows only on its texture's
/// bright texels, its tubes, and its housing keeps its baked light.
const LAMP_TEXELS: u32 = 65536;
/// A room's surface with no lightmap, in an area whose bake asks for its
/// unlit rooms to be dark (`AreaLight::dark_unlit`). Under the baked trial
/// it takes `odyssey.wgsl`'s `BAKED_UNLIT_LIGHT` and the fitted lights, not
/// half of full light.
const DARK_UNLIT: u32 = 131072;
/// A room's surface with a lightmap, opaque or not. Under the baked trial
/// its lightmap is the bake, which a blended surface needs too: the cliffs
/// of 401dxn blend (their alpha is under 1), and they showed their bake at
/// a quarter of its light, black beside the baked ground.
const ROOM_LIGHTMAP: u32 = 262144;
/// A placeable's mesh whose vertex colours hold its baked light
/// (`placeable_light`). Under the baked trial the light replaces the
/// probes' light.
const VERTEX_LIGHT: u32 = 524288;
/// Such a mesh takes none of the game's own lamps
/// (`placeable_light::alone`).
const VERTEX_LIGHT_ALONE: u32 = 1048576;
/// A punch-through surface (`Surface::Cutout`): its texels whose alpha is
/// not above `PUNCH_THROUGH_CUT` are left out.
const PUNCH_THROUGH: u32 = 2097152;
/// Water (`OdysseyMaterial::water`): its env map alone, as its bump map
/// turns it, at full light. The base texture is the bump map's frame.
const WATER: u32 = 4194304;

/// The alpha a punch-through texel must be above to be drawn. Both games'
/// executables draw a texture whose TXI says `blending punchthrough` with
/// the alpha test "greater than 0.35", and every other texture with
/// "greater than 0". The shaders keep the same number (`odyssey.wgsl`,
/// `odyssey_prepass.wgsl`). At a distance, where the GPU reads a smaller mip
/// level, the small holes of a cut-out (the gaps in Telos's tree crowns)
/// average to a faint alpha. Cut at 0, they showed the colours stored in the
/// holes as dark and blue blotches.
pub const PUNCH_THROUGH_CUT: f32 = 0.35;

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
    /// The way a light fitting's trial light shines (the light kinds'
    /// trial, `KJK_FOUND_LIGHTS`). The xyz part is the way, and w is how
    /// narrow the cone is: 0 for a light that shines all ways, 0.2 for the
    /// half of space in front of a panel, 0.7 for a spotlight.
    pub aim: Vec4,
}

/// How many rooms `GpuLight::unseen` can keep a light from.
pub const VIS_ROOMS: usize = 128;

/// `GpuLight::kind.z` of a fight's light: it lights the rooms' baked
/// surfaces as well as what moves.
pub const LIGHT_COMBAT: u32 = 1;
/// `GpuLight::kind.y` of a light that reaches everything, static meshes too
/// (`odyssey.wgsl`).
pub const LIGHT_DYNAMIC_ALL: u32 = 1;

/// The `GpuLight::kind` of a fight's light. It reaches everything, the
/// rooms' baked surfaces too. An ambient-only light lights from every side.
pub fn fight_kind(ambient_only: bool) -> UVec4 {
    UVec4::new(u32::from(ambient_only), LIGHT_DYNAMIC_ALL, LIGHT_COMBAT, 0)
}

/// The `GpuLight::kind` of a lamp with its model's dynamic type. The rooms'
/// baked surfaces never take it.
pub fn lamp_kind(ambient_only: bool, dynamic_type: u32) -> UVec4 {
    UVec4::new(u32::from(ambient_only), dynamic_type, 0, 0)
}

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
    /// Room geometry: drawn unlit as diffuse × lightmap, with fights' lights
    /// added to it (`ROOM`), and never lit by the light probes.
    pub is_room: bool,
    /// A room mesh of an area that has no lightmap. It is lit, not drawn
    /// at full light.
    ///
    /// KOTOR's executable draws the rooms in a pass of their own. For that
    /// pass it sets the scene's ambient light to black and turns on only
    /// the lamps of dynamic type 1 (`STATIC`). Each mesh gives its
    /// self-illumination as the colour it emits. So a room mesh without a
    /// lightmap shows as bright as its self-illumination, plus what the few
    /// lamps of type 1 add. (This was read in KOTOR 2's Mac program, which
    /// keeps its function names. KOTOR 1's program sets the ambient light
    /// to black around its room pass in the same way.)
    ///
    /// The models are made for this rule. KOTOR 1's light panels, its signs
    /// and Kashyyyk's grass have a self-illumination of 1, and still show at
    /// full light. Kashyyyk's bark without a lightmap has 0.29, and
    /// the bench seats of the Manaan hotel have 0.13 (issue 557). Kashyyyk's
    /// root trunk and its vines have none, and show dark (issues 545 and
    /// 546).
    pub room_flat: bool,
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
    /// Its models take the light probes' light in place of `ambient` under
    /// Enhanced lighting (`PROBED`). Rooms never do.
    pub probed: bool,
    /// Its bake asks for its rooms' surfaces with no lightmap to be dark
    /// (`DARK_UNLIT`, `unlit.txt` in the bake's folder).
    pub dark_unlit: bool,
    /// The area's fitted lights shine (`fitted_lights::wanted_in`), so a
    /// room's lamp that the files do not set to glow may glow
    /// (`fitted_lights::switched_on`).
    pub fitted: bool,
}

impl AreaLight {
    /// No fog, no shadows.
    pub fn ambient(ambient: [f32; 3]) -> AreaLight {
        AreaLight {
            ambient,
            fog: None,
            shadowed: false,
            probed: false,
            dark_unlit: false,
            fitted: false,
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
                | (u32::from(self.fogged) << 2)
                | (u32::from(self.room_flat) << 3),
        ]
    }
}

pub struct OdysseyMaterialPlugin;

impl Plugin for OdysseyMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, LIGHTING, "lighting.wgsl", crate::shader_checks::wgsl);
        load_internal_asset!(app, SHADER, "odyssey.wgsl", crate::shader_checks::wgsl);
        let rays = crate::shader_checks::wgsl(
            concat!("enable wgpu_ray_query;\n", include_str!("odyssey.wgsl")),
            std::path::Path::new(file!())
                .parent()
                .map_or_else(Default::default, |dir| dir.join("odyssey_rays.wgsl"))
                .to_string_lossy()
                .into_owned(),
        );
        let _ = app
            .world_mut()
            .resource_mut::<Assets<Shader>>()
            .insert(&RAYS_SHADER, rays);
        load_internal_asset!(
            app,
            PARTICLES_SHADER,
            "particles.wgsl",
            crate::shader_checks::wgsl
        );
        load_internal_asset!(
            app,
            LISTED_QUADS_SHADER,
            "particles.wgsl",
            crate::shader_checks::wgsl
        );
        load_internal_asset!(
            app,
            PREPASS_SHADER,
            "odyssey_prepass.wgsl",
            crate::shader_checks::wgsl
        );
        load_internal_asset!(
            app,
            DANGLY_SHADER,
            "dangly.wgsl",
            crate::shader_checks::wgsl
        );
        load_internal_asset!(
            app,
            NOWHERE_SHADER,
            "nowhere.wgsl",
            crate::shader_checks::wgsl
        );
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_LIGHTS, ShaderBuffer::from(LightList::default()));
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(
                &crate::fitted_lights::NO_GRID,
                ShaderBuffer::new(
                    &crate::fitted_lights::empty_grid(),
                    bevy::asset::RenderAssetUsages::default(),
                ),
            );
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
        crate::dangly::init_sway(app);
        app.init_resource::<AreaLights>()
            .init_resource::<Animated>()
            .add_systems(Last, upload_animated)
            .add_plugins((
                MaterialPlugin::<OdysseyMaterial>::default(),
                crate::motion_proxy::MotionProxyPlugin,
            ));
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render.add_systems(
                bevy::render::Render,
                free_rebound
                    .in_set(bevy::render::RenderSystems::PrepareAssets)
                    .before(
                        bevy::render::erased_render_asset::prepare_erased_assets::<
                            MeshMaterial3d<OdysseyMaterial>,
                        >,
                    ),
            );
        }
    }
}

/// Frees the bind group of each material about to be prepared again.
///
/// A material that makes its bind group itself (`as_bind_group`) is given
/// a new bind group each time it changes. Bevy 0.19 then keeps the old one
/// too, with its uniform buffer and the textures it binds. It frees the old
/// one only for a material whose bind group Bevy makes. This material makes
/// its own on a card with ray queries (`ray_structures::available`). So every
/// change to a material kept another bind group and buffer on the GPU for
/// good, and every area's textures stayed alive after the area was left.
/// On Peragus, a minute of standing still kept about 18,000 more bind
/// groups and 18,000 more buffers, since a few materials change every
/// frame. An area change to the next level and back kept about 4,000 more
/// of each and 145 more textures. Only going back to the main menu, which
/// starts the program again, let them go (issue 445). This frees
/// the old bind group first, as Bevy does for the materials whose bind
/// groups it makes.
fn free_rebound(
    extracted: Res<
        bevy::render::erased_render_asset::ExtractedAssets<MeshMaterial3d<OdysseyMaterial>>,
    >,
    mut bindings: ResMut<bevy::pbr::RenderMaterialBindings>,
    mut allocators: ResMut<bevy::pbr::MaterialBindGroupAllocators>,
) {
    if extracted.extracted.is_empty() {
        return;
    }
    let Some(allocator) = allocators.get_mut(&std::any::TypeId::of::<OdysseyMaterial>()) else {
        return;
    };
    for (id, _) in &extracted.extracted {
        if let Some(binding) = bindings.remove(&id.untyped()) {
            allocator.free(binding);
        }
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
    /// A punch-through texture (TXI `blending punchthrough`): opaque, not
    /// blended, with its texels of alpha 0.35 or less left out
    /// (`PUNCH_THROUGH_CUT`). KOTOR's executable blends nothing for a
    /// punch-through texture and draws it with the alpha test "greater than
    /// 0.35", so its texels are either solid or gone.
    Cutout,
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
    pub const ALL: [Surface; 10] = [
        Surface::Opaque,
        Surface::OpaqueAlpha,
        Surface::OpaqueEnvMasked,
        Surface::Cutout,
        Surface::Blend,
        Surface::Additive,
        Surface::AddOne,
        Surface::BlendVertex,
        Surface::Lighten,
        Surface::Modulate,
    ];

    /// Whether it is one of the opaque pass's surfaces, which write depth,
    /// whether or not they blend by their alpha.
    pub fn is_opaque(self) -> bool {
        matches!(
            self,
            Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked | Surface::Cutout
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            Surface::Opaque => "opaque",
            Surface::OpaqueAlpha => "opaque_alpha",
            Surface::OpaqueEnvMasked => "opaque_envmap_masked",
            Surface::Cutout => "cutout",
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
/// only without). KOTOR meshes always carry a hint. A hint above 1 makes a
/// mesh transparent when its texture has alpha, and so does a decal. A
/// skinned mesh (`skinned`) is not made transparent by its hint. A
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
/// A punch-through diffuse with alpha is cut out (`Cutout`), as KOTOR's
/// executable draws it: the Wookiees' fur. Blended, its faint edges wrote
/// depth and hid the parts of the body drawn after them.
pub fn surface(
    (hint, skinned): (u32, bool),
    mesh_alpha: f32,
    (diffuse, loaded): (Option<&TextureFacts>, &[&TextureFacts]),
) -> Surface {
    if loaded.is_empty() {
        return if mesh_alpha < 1.0 {
            Surface::Blend
        } else {
            Surface::Opaque
        };
    }
    let coverage = diffuse.or(loaded.first().copied());
    let alpha = coverage.is_some_and(|facts| facts.alpha && facts.alpha_mean != Some(1.0));
    let punch_through = coverage.is_some_and(|facts| facts.blending == Blending::PunchThrough);
    let decal = loaded.iter().all(|facts| facts.decal);
    let additive = loaded
        .iter()
        .any(|facts| facts.blending == Blending::Additive);
    let envmap = loaded.iter().any(|facts| facts.envmap.is_some());
    let bumpmap = loaded.iter().any(|facts| facts.bumpmap.is_some());
    let water = water(diffuse).is_some();
    if additive {
        Surface::Additive
    } else if water || decal || mesh_alpha < 1.0 || (hint > 1 && !skinned && alpha) {
        Surface::Blend
    } else if alpha && envmap {
        Surface::OpaqueEnvMasked
    } else if alpha && punch_through {
        Surface::Cutout
    } else if (hint > 1 && skinned) || (alpha && !bumpmap) {
        Surface::OpaqueAlpha
    } else {
        Surface::Opaque
    }
}

/// A mesh's self-illumination as the games light it. A mesh whose diffuse
/// texture's TXI says `decal 1` glows at full light, whatever the model
/// gives it. KOTOR 2's executable sets the emitted colour of such a mesh
/// to white when it sets up the mesh's material. So a decal is drawn at its
/// texture's own colours, in a dark place too (a mod's glowing blade
/// texture, `decal 1` with `envmaptexture`).
pub fn decal_glow(self_illum: [f32; 3], diffuse: Option<&TextureFacts>) -> [f32; 3] {
    if diffuse.is_some_and(|facts| facts.decal) {
        [1.0; 3]
    } else {
        self_illum
    }
}

/// A diffuse texture drawn as water (`OdysseyMaterial::water`): its TXI's
/// `wateralpha` and the bump map it names, when it also names an env map.
pub fn water(diffuse: Option<&TextureFacts>) -> Option<(f32, &str)> {
    let facts = diffuse.filter(|facts| facts.envmap.is_some())?;
    facts.water_alpha.zip(facts.bumpmap.as_deref())
}

/// Whether a mesh is drawn, given whether it is part of a room, whether it
/// names a diffuse texture and a lightmap, and whether each of them loaded.
/// A room's mesh that names neither (its diffuse is `NULL`) is drawn
/// untextured in its material's colours. KOTOR 2's executable draws such a
/// mesh on its flat path, because its trimesh draw never asks for a
/// texture. The Ebon Hawk's rooms lie over such a plane. With the plane left
/// out, the hyperspace sky showed blue through the seams of the floor (issue
/// 441). Such a mesh that casts shadows (`shadow`) is not drawn: it is a
/// shadow caster, not a surface. Each room of the Dantooine enclave has one,
/// a box around the room whose faces look in. Drawn, its lid hid the sky
/// above the council chamber's round opening, where the original game shows
/// the sky. The Ebon Hawk's plane casts no shadow. A mesh that names a
/// texture of which none loaded is not drawn, as xoreos loads textures. An
/// untextured mesh of a creature, a placeable or a door is not drawn either,
/// a kjk choice until the original is compared.
pub fn mesh_drawn(
    (room, shadow): (bool, bool),
    (texture_named, texture_loaded): (bool, bool),
    (lightmap_named, lightmap_loaded): (bool, bool),
) -> bool {
    let names_none = !texture_named && !lightmap_named;
    (room && !shadow && names_none) || texture_loaded || lightmap_loaded
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
    solid_if_untold(
        surface,
        loaded,
        rules::weapons_in_hand::look::SOLID && weapon,
    )
}

/// A creature's body or head (`worn`: its appearance asks for an env map,
/// `viewer::WornEnvMap`) drawn as kjk draws it (`rules::creatures::look::SOLID`):
/// a texture with alpha that no TXI gives a meaning to is drawn solid, in an
/// area with no env map for the alpha to mask. A kjk choice, not KOTOR's:
/// its executable sets no env map on such a creature and blends by the
/// alpha. Blended, the alpha (an env map's mask, mostly 0.85 to 1) kept the
/// motion proxy (`PROXY_SOLID`) from writing depth and motion there, and
/// under a temporal resolve the world behind showed through the creature
/// as it moved (Bastila's shoulders in the Taris Sith base). A mesh asking
/// to be transparent (`hint`, its transparency hint) keeps its surface, as
/// does any other model.
pub fn worn_surface(surface: Surface, loaded: &[&TextureFacts], worn: bool, hint: u32) -> Surface {
    solid_if_untold(
        surface,
        loaded,
        rules::creatures::look::SOLID && worn && hint == 0,
    )
}

/// `surface` drawn solid when `solid` and it blends by a texture alpha that
/// no TXI gives a meaning to (no env map, bump map, blending, decal or
/// animation).
fn solid_if_untold(surface: Surface, loaded: &[&TextureFacts], solid: bool) -> Surface {
    let untold = |facts: &&TextureFacts| {
        facts.blending == Blending::Default
            && !facts.decal
            && facts.envmap.is_none()
            && facts.bumpmap.is_none()
            && facts.odd_blending.is_none()
            && facts.cycle.is_none()
    };
    if solid && surface == Surface::OpaqueAlpha && loaded.iter().all(untold) {
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

/// An Odyssey mesh's material. Its bind group is made by hand
/// (`AsBindGroup` below). It binds what `OdysseyBound` binds. When the
/// structures are available (`ray_structures::available`), it also binds
/// the scene's acceleration structure and the numbers of the merged
/// placeables' and doors' geometries.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct OdysseyMaterial {
    pub params: OdysseyParams,
    pub base: Option<Handle<Image>>,
    pub lightmap: Option<Handle<Image>>,
    /// A cube map. KOTOR's env-mapped surfaces add it, weighted by one minus
    /// the diffuse alpha (xoreos and reone both add it, `color += envmap × (1 − a)`).
    pub envmap: Option<Handle<Image>>,
    /// The lights it is lit by: `AREA_LIGHTS` unless set.
    pub lights: Handle<ShaderBuffer>,
    /// The characters casting shadows now, and their capsules
    /// (`shadows::AREA_SHADOWS`).
    pub shadows: Handle<ShaderBuffer>,
    /// The placeables' boxes (`shadows::AREA_BOXES`).
    pub boxes: Handle<ShaderBuffer>,
    /// The animated materials' slots (`ANIMATED`).
    pub animated: Handle<ShaderBuffer>,
    /// An emitter's particles, four `GpuParticle` vertices a quad, which
    /// place its mesh's vertices by their index (`model_fx::Spray`):
    /// written in place each frame, where a mesh written anew is freed and
    /// allocated again on the GPU. `NO_PARTICLES` for the rest.
    pub particles: Handle<ShaderBuffer>,
    /// A flat env map, sphere-mapped (`sphere_envmap`), where `envmap` is
    /// none.
    pub sphere_envmap: Option<Handle<Image>>,
    /// The shadow tiles' atlas (`shadow_tiles::ATLAS`), a layer for the
    /// characters' tiles and one for the placeables', read through a
    /// comparison sampler.
    pub shadow_tiles: Option<Handle<Image>>,
    /// The light probes' light of each placed model
    /// (`light_probes::AREA_PROBES`). Only `odyssey.wgsl` reads it.
    pub probes: Handle<ShaderBuffer>,
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
    /// Where a blended opaque surface (`OpaqueAlpha`) sorts among the
    /// others. A room's mesh has a rank from 0 to 1, which is the share of
    /// its texture that is not opaque. Every other model's mesh has
    /// `OBJECTS_RANK`.
    /// KOTOR draws the textures with the least alpha last, so a patch of
    /// sand cut out by its alpha lies over the ground it sits on, and not
    /// the other way round (`depth_bias`).
    pub rank: f32,
    /// A motion proxy's (`motion_only`).
    pub motion_only: bool,
    pub listed_quads: bool,
    /// How the mesh is drawn while its alpha is below 1 and while it is 1,
    /// for a mesh whose alpha a clip keys (`Looks`). `None` for the rest.
    pub looks: Option<Looks>,
    /// Drawn over everything already on its camera's target: it neither
    /// tests nor writes depth. The quads of the cameras that draw over the
    /// world (the HUD, the loading screen, a movie, the fade) are, so that
    /// those cameras can keep the world's depth for ReShade (`reshade`).
    pub on_top: bool,
}

/// What a surface changes in a material: the surface, the shader flags and
/// the fog, which only the opaque pass takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub surface: Surface,
    flags: u32,
    fog_color: Vec4,
    fog_range: Vec4,
}

impl Look {
    fn of(material: &OdysseyMaterial) -> Look {
        Look {
            surface: material.surface,
            flags: material.params.flags,
            fog_color: material.params.fog_color,
            fog_range: material.params.fog_range,
        }
    }
}

/// A mesh's two looks: blended while its alpha is below 1 (`faded`) and
/// drawn with its opaque surface while it is 1 (`whole`). KOTOR's
/// executable blends a mesh only while its alpha is below 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Looks {
    pub faded: Look,
    pub whole: Look,
}

/// What `OdysseyMaterial` binds, as Bevy's derive makes it. Bevy's derive
/// has no attribute for an acceleration structure, so the material makes
/// its own bind group from this one's and adds the structure to it.
///
/// Only the lists a vertex stage reads are bound to the vertex stage. Metal
/// gives a stage a fixed number of buffer slots, and the vertex stage
/// shares them with the mesh's vertex buffers.
#[derive(AsBindGroup)]
pub struct OdysseyBound {
    #[uniform(0)]
    params: OdysseyParams,
    #[texture(1)]
    #[sampler(2)]
    base: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    lightmap: Option<Handle<Image>>,
    #[texture(5, dimension = "cube")]
    #[sampler(6)]
    envmap: Option<Handle<Image>>,
    #[storage(7, read_only, visibility(fragment))]
    lights: Handle<ShaderBuffer>,
    #[storage(8, read_only, visibility(fragment))]
    shadows: Handle<ShaderBuffer>,
    #[storage(13, read_only, visibility(fragment))]
    boxes: Handle<ShaderBuffer>,
    #[storage(9, read_only, visibility(fragment))]
    animated: Handle<ShaderBuffer>,
    #[storage(10, read_only)]
    particles: Handle<ShaderBuffer>,
    #[texture(11)]
    #[sampler(12)]
    sphere_envmap: Option<Handle<Image>>,
    #[texture(14, sample_type = "depth", dimension = "2d_array")]
    #[sampler(15, sampler_type = "comparison")]
    shadow_tiles: Option<Handle<Image>>,
    #[storage(16, read_only, visibility(fragment))]
    probes: Handle<ShaderBuffer>,
    #[storage(19, read_only, visibility(vertex))]
    sway: Handle<ShaderBuffer>,
    /// The fitted lights' grid (`fitted_lights::grid_handle`). Only the fragment
    /// stage reads it. Metal gives a vertex stage 31 buffer slots, and one
    /// more buffer seen by the vertex stage left the prepass's vertex
    /// buffers no slot, so no pipeline built on a Mac.
    #[storage(20, read_only, visibility(fragment))]
    fitted: Handle<ShaderBuffer>,
}

/// The binding of the scene's acceleration structure (`odyssey.wgsl`'s
/// `scene_rays`, `ray_structures`).
const RAYS_BINDING: u32 = 17;

/// The binding of the numbers of the merged placeables' and doors'
/// geometries (`odyssey.wgsl`'s `ray_numbers`, `RayStructures::numbers`).
const NUMBERS_BINDING: u32 = 18;

impl OdysseyMaterial {
    fn bound(&self) -> OdysseyBound {
        OdysseyBound {
            params: self.params,
            base: self.base.clone(),
            lightmap: self.lightmap.clone(),
            envmap: self.envmap.clone(),
            lights: self.lights.clone(),
            shadows: self.shadows.clone(),
            boxes: self.boxes.clone(),
            animated: self.animated.clone(),
            particles: self.particles.clone(),
            sphere_envmap: self.sphere_envmap.clone(),
            shadow_tiles: self.shadow_tiles.clone(),
            probes: self.probes.clone(),
            sway: crate::dangly::SWAY,
            fitted: crate::fitted_lights::grid_handle(),
        }
    }
}

/// With the structures not available, the bind group is `OdysseyBound`'s
/// alone, made as Bevy makes a derived one. With them available, Bevy's
/// bind groups cannot hold an acceleration structure, so the material makes
/// its bind group itself (`as_bind_group`). Its layout has the structure at
/// `RAYS_BINDING` and the numbers of the merged placeables' and doors'
/// geometries at `NUMBERS_BINDING`.
impl AsBindGroup for OdysseyMaterial {
    type Data = OdysseyKey;
    type Param = (
        <OdysseyBound as AsBindGroup>::Param,
        Option<SRes<crate::ray_structures::RayStructures>>,
    );

    fn label() -> &'static str {
        "OdysseyMaterial"
    }

    fn bind_group_data(&self) -> OdysseyKey {
        self.into()
    }

    fn unprepared_bind_group(
        &self,
        layout: &BindGroupLayout,
        device: &RenderDevice,
        param: &mut SystemParamItem<'_, '_, Self::Param>,
        force_no_bindless: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        if crate::ray_structures::available() {
            return Err(AsBindGroupError::CreateBindGroupDirectly);
        }
        self.bound()
            .unprepared_bind_group(layout, device, &mut param.0, force_no_bindless)
    }

    fn as_bind_group(
        &self,
        layout_descriptor: &BindGroupLayoutDescriptor,
        device: &RenderDevice,
        cache: &PipelineCache,
        param: &mut SystemParamItem<'_, '_, Self::Param>,
    ) -> Result<PreparedBindGroup, AsBindGroupError> {
        let layout = cache.get_bind_group_layout(layout_descriptor);
        let UnpreparedBindGroup { bindings } =
            self.bound()
                .unprepared_bind_group(&layout, device, &mut param.0, false)?;
        let mut entries: Vec<BindGroupEntry> = bindings
            .iter()
            .map(|(index, binding)| BindGroupEntry {
                binding: *index,
                resource: binding.get_binding(),
            })
            .collect();
        if crate::ray_structures::available() {
            let Some(structures) = param.1.as_ref() else {
                return Err(AsBindGroupError::RetryNextUpdate);
            };
            entries.push(BindGroupEntry {
                binding: RAYS_BINDING,
                resource: structures.tlas().as_binding(),
            });
            entries.push(BindGroupEntry {
                binding: NUMBERS_BINDING,
                resource: structures.numbers().as_entire_binding(),
            });
        }
        let bind_group = device.create_bind_group(Self::label(), &layout, &entries);
        Ok(PreparedBindGroup {
            bindings,
            bind_group,
        })
    }

    fn bind_group_layout_entries(
        device: &RenderDevice,
        force_no_bindless: bool,
    ) -> Vec<BindGroupLayoutEntry> {
        let mut entries = OdysseyBound::bind_group_layout_entries(device, force_no_bindless);
        if crate::ray_structures::available() {
            entries.push(BindGroupLayoutEntry {
                binding: RAYS_BINDING,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::AccelerationStructure {
                    vertex_return: false,
                },
                count: None,
            });
            entries.push(BindGroupLayoutEntry {
                binding: NUMBERS_BINDING,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            });
        }
        entries
    }
}

impl OdysseyMaterial {
    /// Drawn added (`src + dst`), unlit, by its alpha (a clip fades it): a
    /// glow card a clip keys (the swoop's thruster cards, its start lights'
    /// flares). reone adds such a texture's colour, its alpha taken from
    /// its brightness, which comes to the same; the texture's own alpha is
    /// not coverage here.
    pub fn faded_additive(mut self) -> OdysseyMaterial {
        self.surface = Surface::AddOne;
        self.params.flags &= !(DISCARD_ZERO_ALPHA | PUNCH_THROUGH | OPAQUE_ALPHA | VERTEX_ALPHA);
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

    /// A room's lamp that shines although the files do not set it to glow
    /// (`LAMP_TEXELS`).
    pub fn lamp_texels(mut self, lamp: bool) -> OdysseyMaterial {
        if lamp {
            self.params.flags |= LAMP_TEXELS;
        }
        self
    }

    /// A placeable's mesh whose vertex colours hold its baked light
    /// (`VERTEX_LIGHT`).
    pub fn vertex_light(mut self, baked: bool) -> OdysseyMaterial {
        if baked {
            self.params.flags |= VERTEX_LIGHT;
            if crate::placeable_light::alone() {
                self.params.flags |= VERTEX_LIGHT_ALONE;
            }
        }
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
        // An added surface that glows (its self-illumination above black) is
        // lit as the games light it. Its texture is scaled by the
        // self-illumination plus the light of the place, which reaches it
        // evenly, and that sum stops at full (read off a mine's dome in
        // KOTOR 2, `mine_dome`). In a dark place an energy shield's grey
        // sphere keeps its self-illumination's red or blue. A blade's blue
        // glow with a green self-illumination keeps its blue, where scaling
        // by the green alone left a flickering green line. Other added
        // surfaces are drawn unlit, as reone draws every added surface. A
        // room is unlit, but for its meshes without a lightmap.
        // A room surface with no lightmap in an area whose bake asks for it
        // to be dark (Nar Shaddaa's tunnels under Enhanced Lightmaps) keeps
        // the bake's own rule (`DARK_UNLIT`), not `room_flat`'s.
        let dark_unlit = area.dark_unlit && lighting.is_room && lightmap.is_none();
        let lit_glow = matches!(surface, Surface::Additive | Surface::AddOne)
            && !lighting.is_room
            && lighting.self_illum != [0.0; 3];
        if lit_glow {
            flags |= EVEN_LIGHT;
        } else if matches!(
            surface,
            Surface::Additive
                | Surface::AddOne
                | Surface::BlendVertex
                | Surface::Lighten
                | Surface::Modulate
        ) || (lighting.is_room && !lighting.room_flat)
            || dark_unlit
        {
            flags |= UNLIT;
        }
        // reone fogs the opaque pass only, for models
        // whose header asks for it.
        let fog = area.fog.filter(|_| lighting.fogged && surface.is_opaque());
        if fog.is_some() {
            flags |= FOG;
        }
        let opaque = surface.is_opaque();
        if area.shadowed && opaque {
            flags |= SHADOWED;
        }
        if lighting.is_room && opaque {
            flags |= ROOM;
        }
        if area.probed && !lighting.is_room {
            flags |= PROBED;
        }
        if dark_unlit {
            flags |= DARK_UNLIT;
        }
        if lighting.is_room && lightmap.is_some() {
            flags |= ROOM_LIGHTMAP;
        }
        let colour = |rgb: [f32; 3]| Vec4::new(rgb[0], rgb[1], rgb[2], 1.0);
        OdysseyMaterial {
            params: OdysseyParams {
                tint: tint * Vec4::new(1.0, 1.0, 1.0, lighting.alpha),
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
            probes: crate::light_probes::AREA_PROBES,
            surface,
            culled: false,
            emitted: None,
            rank: 0.0,
            motion_only: false,
            listed_quads: false,
            looks: None,
            on_top: false,
        }
    }

    /// Its looks when its alpha is keyed (`Looks`): `whole` the same mesh's
    /// material made with its opaque surface, the material itself blended.
    /// A mesh whose own alpha is below 1 is made blended (`surface`), but a
    /// clip can key it up to 1: the Star Forge captives' drained bodies
    /// (`plc_endcorps`'s `torso2` and `torso01`, issue 408).
    pub fn whole_as(mut self, whole: &OdysseyMaterial) -> OdysseyMaterial {
        self.looks = Some(Looks {
            faded: Look::of(&self),
            whole: Look::of(whole),
        });
        self
    }

    /// Its looks for a mesh that is opaque at rest and whose alpha a clip
    /// keys: the material as it is while whole, and blended, the rest kept,
    /// while faded. Its own looks (`whole_as`) if it has them; `None` for a
    /// surface that is neither.
    pub fn keyed_looks(&self) -> Option<Looks> {
        self.looks.or_else(|| {
            self.surface.is_opaque().then(|| {
                let whole = Look::of(self);
                Looks {
                    faded: Look {
                        surface: Surface::Blend,
                        ..whole
                    },
                    whole,
                }
            })
        })
    }

    /// Its surface, flags and fog drawn whole (`whole`) or faded by
    /// `looks`: the flags the two looks differ in are taken from the look,
    /// and any other flag set on it is kept.
    fn with_look(&self, looks: &Looks, whole: bool) -> (Surface, u32, Vec4, Vec4) {
        let look = if whole { looks.whole } else { looks.faded };
        let differ = looks.whole.flags ^ looks.faded.flags;
        let flags = (self.params.flags & !differ) | (look.flags & differ);
        (look.surface, flags, look.fog_color, look.fog_range)
    }

    /// Whether it is drawn otherwise than `with_look` has it.
    pub fn wants_look(&self, looks: &Looks, whole: bool) -> bool {
        let p = &self.params;
        self.with_look(looks, whole) != (self.surface, p.flags, p.fog_color, p.fog_range)
    }

    /// Drawn whole (`whole`) or faded by `looks` (`with_look`).
    pub fn show_look(&mut self, looks: &Looks, whole: bool) {
        let (surface, flags, fog_color, fog_range) = self.with_look(looks, whole);
        self.surface = surface;
        self.params.flags = flags;
        self.params.fog_color = fog_color;
        self.params.fog_range = fog_range;
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
        self.params.flags &= STATIC | UNLIT | FOG | SHADOWED | PROBED;
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
        let all = DISCARD_ZERO_ALPHA | PUNCH_THROUGH | OPAQUE_ALPHA | VERTEX_ALPHA;
        self.params.flags = (self.params.flags & !all) | surface_flags(surface);
        self.surface = surface;
        self
    }

    /// The same, discarding alpha 0 or not (`DISCARD_ZERO_ALPHA`), whatever
    /// its surface. A faded look keeps the flag of the surface it fades, so
    /// a blended surface can discard too (the pipeline warm-up's stand-ins:
    /// `warmup`).
    pub fn cutting(mut self, cut: bool) -> OdysseyMaterial {
        if cut {
            self.params.flags |= DISCARD_ZERO_ALPHA;
        } else {
            self.params.flags &= !DISCARD_ZERO_ALPHA;
        }
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
    /// what is behind it, as any added surface is. Like every added surface
    /// that glows (`new`), it is lit: the texture is scaled by the mesh's
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

    /// Drawn as water, as both games' executables draw a texture whose TXI
    /// says `wateralpha` below 1 and names a bump map and an env map
    /// (`bumpyshinytexture`). The diffuse texture is not drawn. The env map
    /// alone is drawn, reflected about the surface as its bump map turns
    /// it, at full light and whatever the place's light. It is blended by
    /// `alpha` (the TXI's `wateralpha`) times the env map's own alpha, and
    /// the mesh's own alpha takes no part. The bump map is a flip-book of
    /// heights, so the reflection ripples, and the mesh's scrolling UVs move
    /// it along. (Read in KOTOR 2's Mac program: its bump-mapped env map
    /// pass skips the diffuse pass for such a texture, and its fragment
    /// program writes the env map's colour and its alpha times `wateralpha`.
    /// KOTOR 1's program reads the same TXI word.)
    ///
    /// `bump` is the bump map's first frame; the flip-book's other frames
    /// take its place in turn (`scene::cycle_flipbooks`).
    pub fn water(mut self, bump: Option<Handle<Image>>, alpha: f32) -> OdysseyMaterial {
        self.base = bump;
        self.params.tint = Vec4::new(1.0, 1.0, 1.0, alpha);
        self.params.flags &= !OPAQUE_TEXTURE;
        self.params.flags |= WATER | UNLIT;
        self
    }

    /// `water` when the mesh is water (`material::water`), else the same.
    pub fn water_if(self, water: Option<(Option<Handle<Image>>, f32)>) -> OdysseyMaterial {
        match water {
            Some((bump, alpha)) => self.water(bump, alpha),
            None => self,
        }
    }

    /// Its sort rank (`rank`).
    pub fn ranked(mut self, rank: f32) -> OdysseyMaterial {
        self.rank = rank;
        self
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
        Surface::Cutout => DISCARD_ZERO_ALPHA | PUNCH_THROUGH,
        Surface::OpaqueEnvMasked => OPAQUE_ALPHA,
        Surface::BlendVertex | Surface::Lighten => VERTEX_ALPHA,
        _ => 0,
    }
}

/// `kjk sim probes`' cut-out group. The prepass decides which pixels a
/// cut-out surface covers, and the main pass then draws only those pixels.
/// Both must read the texture's alpha at the same mip level. If the prepass
/// covers a texel that the main pass discards, nothing is drawn there and
/// the clear colour shows. Under TAA the main pass reads with the view's mip
/// bias, so Tatooine's shrubs showed the pale fog colour through their
/// branches (issues 443 and 515).
#[cfg(feature = "probes")]
pub fn cutout_probes() -> sim::probe::Checks {
    vec![(
        "the prepass reads a cut-out texture as the main pass does",
        prepass_reads_as_main(),
    )]
}

/// Checks `TextureIr::sort_key` (the rule is in `asset_odyssey::texture`)
/// on built textures (`kjk sim probes --only "texture sort key"`; proposed
/// 2026-11-23).
#[cfg(feature = "probes")]
pub fn sort_key_probes() -> sim::probe::Checks {
    use asset_odyssey::texture::TextureIr;
    use fmt_odyssey::texture::{Mip, PixelFormat, Texture};
    use fmt_odyssey::txi::Txi;

    let key = |format: PixelFormat, data: Vec<u8>, txi: &str, header: Option<f32>| {
        let texture = Texture {
            format,
            layers: vec![vec![Mip {
                width: 4,
                height: 4,
                data,
            }]],
            cube: false,
            txi: Txi::parse(txi.as_bytes()),
            bottom_up: true,
            missing_mips: 0,
            declared_size: None,
            mips_stored: true,
            header_key: header,
        };
        TextureIr::from_texture(&texture, None).map(|ir| ir.sort_key())
    };
    let half: Vec<u8> = (0..16)
        .flat_map(|texel| [10, 20, 30, if texel < 8 { 255 } else { 0 }])
        .collect();
    let is = |name, got: Option<f32>, want: f32| {
        (
            name,
            sim::probe::check(got == Some(want), format!("{got:?}, expected {want}")),
        )
    };
    vec![
        is(
            "the header's float beats the TXI's alphamean and the pixels",
            key(
                PixelFormat::Rgba8,
                half.clone(),
                "alphamean 0.25",
                Some(0.75),
            ),
            0.75,
        ),
        is(
            "the TXI's alphamean is used when there is no header float",
            key(PixelFormat::Rgba8, half.clone(), "alphamean 0.25", None),
            0.25,
        ),
        is(
            "an uncompressed texture with neither averages its alpha",
            key(PixelFormat::Rgba8, half.clone(), "", None),
            0.5,
        ),
        is(
            "an uncompressed texture without alpha is 1",
            key(PixelFormat::Rgb8, vec![9; 48], "", None),
            1.0,
        ),
        is(
            "a compressed texture with neither is 1, not its alpha mean",
            key(PixelFormat::Dxt5, vec![0; 16], "", None),
            1.0,
        ),
        is(
            "a header float of -1 counts as none, so a compressed texture is 1",
            key(PixelFormat::Dxt5, vec![0; 16], "", Some(-1.0)),
            1.0,
        ),
        is(
            "a header float of -1 counts as none, so an uncompressed texture averages its alpha",
            key(PixelFormat::Rgba8, half.clone(), "", Some(-1.0)),
            0.5,
        ),
        is(
            "a header float of -1 does not fall back to the TXI's alphamean",
            key(
                PixelFormat::Rgba8,
                half.clone(),
                "alphamean 0.25",
                Some(-1.0),
            ),
            0.5,
        ),
        is(
            "a negative number other than -1 is used as it is",
            key(PixelFormat::Rgba8, half, "", Some(-0.5)),
            -0.5,
        ),
    ]
}

/// `kjk sim probes`' transparency hint group. KOTOR's executable never
/// reads the hint to blend. It blends every mesh by its texture's alpha and
/// writes depth, so a mesh whose texture has no alpha is solid. TSL's
/// Dantooine slab (`brok04` of `605dank`, hint 4, a texture with no alpha)
/// was drawn blended and without depth, and it hid the ivy (`brok05`) that
/// grows on it.
#[cfg(feature = "probes")]
pub fn hint_probes() -> sim::probe::Checks {
    vec![(
        "a hint blends only a mesh whose texture has alpha",
        hint_blends(),
    )]
}

#[cfg(feature = "probes")]
fn hint_blends() -> Result<String, String> {
    let with_alpha = TextureFacts {
        alpha: true,
        ..TextureFacts::default()
    };
    let without = TextureFacts::default();
    let cases = [
        (4, false, &without, Surface::Opaque),
        (4, false, &with_alpha, Surface::Blend),
        (4, true, &without, Surface::OpaqueAlpha),
        (4, true, &with_alpha, Surface::OpaqueAlpha),
        (0, false, &without, Surface::Opaque),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|&(hint, skinned, facts, wanted)| {
            let got = surface((hint, skinned), 1.0, (Some(facts), &[facts]));
            (got != wanted).then(|| {
                format!(
                    "hint {hint}, skinned {skinned}, alpha {}: {got:?}, wanted {wanted:?}",
                    facts.alpha
                )
            })
        })
        .collect();
    sim::probe::check(
        wrong.is_empty(),
        format!("{} cases, wrong: {wrong:?}", cases.len()),
    )
}

#[cfg(feature = "probes")]
fn prepass_reads_as_main() -> Result<String, String> {
    // Every read of the base texture in a shader, as written.
    fn reads(source: &str) -> Vec<&str> {
        source
            .match_indices("textureSample")
            .filter_map(|(at, _)| {
                let call = &source[at..];
                let open = call.find('(')?;
                let close = call.find(')')?;
                call[open + 1..]
                    .starts_with("base_texture,")
                    .then(|| &call[..=close])
            })
            .collect()
    }
    let main = reads(include_str!("odyssey.wgsl"));
    let prepass = reads(include_str!("odyssey_prepass.wgsl"));
    sim::probe::check(
        !prepass.is_empty() && prepass.iter().all(|read| main.contains(read)),
        format!("main pass {main:?}, prepass {prepass:?}"),
    )
}

/// `depth_bias` of a blended opaque surface (one that writes depth: this
/// material's `OpaqueAlpha`, the grass): farther than anything in view,
/// keeping their order among themselves: by their texture's alpha
/// (`OdysseyMaterial::rank`), then by distance.
pub const OPAQUE_FIRST: f32 = -1.0e5;
/// How far apart the ranks 0 and 1 of blended opaque surfaces sort
/// (`OdysseyMaterial::rank`): more than any distance in view.
pub const RANK_STEP: f32 = 1.0e4;
/// The `OdysseyMaterial::rank` of a blended opaque surface of any model that
/// is not a room, such as a creature, a placeable or a door. KOTOR draws every
/// mesh of a room, and its grass, before any of them. They therefore rank
/// after the room's ranks (0 to 1) and the grass. A creature's half
/// see-through texels write depth, so one drawn before the room hid the wall
/// behind it.
pub const OBJECTS_RANK: f32 = 2.0;
/// `depth_bias` of the grass. It sorts after every blended opaque surface of
/// a room, whose ranks run from 0 to 1.
pub const GRASS_AFTER_MESHES: f32 = OPAQUE_FIRST + RANK_STEP;

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
    listed_quads: bool,
    /// It discards alpha 0 (`DISCARD_ZERO_ALPHA`) and has a prepass, which
    /// then discards it too.
    cut: bool,
    motion_only: bool,
    on_top: bool,
}

impl From<&OdysseyMaterial> for OdysseyKey {
    fn from(material: &OdysseyMaterial) -> OdysseyKey {
        let surface = material.surface;
        OdysseyKey {
            blend: blend(surface),
            depth_write: writes_depth(surface),
            culled: material.culled,
            particles: material.particles.id() != NO_PARTICLES.id(),
            listed_quads: material.listed_quads,
            // Only a prepass reads it, and Bevy draws a blended or added
            // surface without a prepass. Such a surface therefore takes the
            // same pipeline whether it is set or not.
            cut: material.params.flags & DISCARD_ZERO_ALPHA != 0
                && matches!(
                    material.alpha_mode(),
                    AlphaMode::Opaque | AlphaMode::Mask(_)
                ),
            motion_only: material.motion_only,
            on_top: material.on_top,
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
        Surface::Opaque | Surface::OpaqueEnvMasked | Surface::Cutout => None,
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

fn swayed(
    descriptor: &mut RenderPipelineDescriptor,
    layout: &MeshVertexBufferLayoutRef,
) -> Result<(), SpecializedMeshPipelineError> {
    if crate::dangly::slot_attribute(descriptor, layout)? {
        descriptor.vertex.shader = DANGLY_SHADER;
    }
    Ok(())
}

fn list_shader(key: OdysseyKey) -> Handle<Shader> {
    if key.listed_quads {
        LISTED_QUADS_SHADER
    } else {
        PARTICLES_SHADER
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
            // Cut out in the prepass too: an opaque surface's prepass draws
            // no fragments, so its depth would hide what shows through it.
            // The shader cuts at `PUNCH_THROUGH_CUT` (`PUNCH_THROUGH`),
            // whatever the mask's number.
            Surface::Cutout => AlphaMode::Mask(0.5),
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
            Surface::OpaqueAlpha => OPAQUE_FIRST + self.rank * RANK_STEP,
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
        let mesh_key = key.mesh_key;
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
            if key.particles {
                descriptor.vertex.shader = list_shader(key);
            } else {
                swayed(descriptor, layout)?;
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
            descriptor.vertex.shader = list_shader(key);
        } else {
            swayed(descriptor, layout)?;
        }
        descriptor.primitive.cull_mode = cull_mode;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            // With the structures available, a view that blends its frames
            // (TAA or DLSS, which jitter) traces rays against them under
            // Ultra. A view under MSAA never does, and keeps the shader
            // without ray queries, as on a card that has none.
            let jittered = mesh_key.contains(bevy::pbr::MeshPipelineKey::TEMPORAL_JITTER);
            if crate::ray_structures::available() && jittered {
                fragment.shader = RAYS_SHADER;
                fragment.shader_defs.push("RAY_QUERIES".into());
            }
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = key.blend;
            }
            // The fitted lights and their trials, only in a run that may
            // show them (`fitted_lights::in_shaders`). An emitter's
            // particles never take them. Every particle is unlit (`new`,
            // `punch_through`), and the trials change nothing on an unlit
            // surface that is not a room's. Their code still made the
            // particles cost more: under Enhanced Lightmaps the crowd
            // brawl's 930 particles took 0.75 ms of GPU time against 0.15 ms.
            if crate::fitted_lights::in_shaders() && !key.particles {
                fragment.shader_defs.push("FITTED_LIGHTS".into());
            }
            // A surface of the opaque pass is in the prepass the occlusion
            // is worked out from, so it takes the occlusion. What is truly
            // see-through is not, and takes none (`ambient_occlusion`).
            if key.depth_write
                && mesh_key.contains(bevy::pbr::MeshPipelineKey::SCREEN_SPACE_AMBIENT_OCCLUSION)
            {
                fragment
                    .shader_defs
                    .extend_from_slice(crate::ambient_occlusion::shader_defs());
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(key.depth_write && !key.on_top);
            if key.on_top {
                depth.depth_compare = Some(CompareFunction::Always);
            }
        }
        Ok(())
    }
}
