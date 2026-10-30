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
//! How a surface blends follows xoreos (blending always on):
//! opaque surfaces draw first, alpha-blended when their textures carry
//! alpha; transparent ones (a transparency hint, or decal textures) draw
//! after, far to near, without writing depth. An additive texture (TXI
//! `blending additive`, with alpha or without) adds itself by its alpha to
//! what is behind, as KOTOR's executable sets it (`GL_SRC_ALPHA, GL_ONE`
//! from its blend table; xoreos multiplied by the colour instead).

use asset_odyssey::texture::{Blending, TextureFacts};
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
    ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;

const SHADER: Handle<Shader> = uuid_handle!("5d0f3b8e-3f7c-4d8e-9a51-0b7c2e61d4a3");
const PREPASS_SHADER: Handle<Shader> = uuid_handle!("e2a7c5d1-0b84-4f3e-9c26-7d1f8a3b5e90");
/// `kjk::lighting`, the flash and the fog this material and the grass share.
const LIGHTING: Handle<Shader> = uuid_handle!("c3a7e215-8d4b-4f6e-b0d2-5e91f7a4c836");

/// The area's lights, which every material reads unless given others.
pub const AREA_LIGHTS: Handle<ShaderBuffer> = uuid_handle!("b6e2a9d4-71c3-4f08-8e5a-3d9c0f6b2a17");

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

/// Most lights a surface is lit by at once: the nearest to the camera win.
/// Our chosen cap (the shader's light list is this long).
pub const MAX_LIGHTS: usize = 32;

/// A light as the shader reads it, in Bevy world space.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuLight {
    pub position_radius: Vec4,
    pub color_multiplier: Vec4,
    /// x: ambient only (0/1), y: dynamic type.
    pub kind: UVec4,
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
        load_internal_asset!(
            app,
            PREPASS_SHADER,
            "odyssey_shadow.wgsl",
            Shader::from_wgsl
        );
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_LIGHTS, ShaderBuffer::from(LightList::default()));
        app.init_resource::<AreaLights>()
            .add_plugins(MaterialPlugin::<OdysseyMaterial>::default());
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
/// textures that loaded (diffuse and lightmap), as xoreos combines them:
/// alpha only when every texture has it (and `alphamean` is not 1), decal
/// only when every texture is one; additive when any texture says so, with
/// alpha or without, as KOTOR's executable reads a texture's TXI (xoreos
/// only without). KOTOR meshes always carry a hint, so a hint or a decal makes a
/// mesh transparent. Deviation: an additive mesh without a hint is drawn in
/// the transparent pass too (xoreos draws it in the opaque pass, in object
/// order); the coverage report counts them. A bump-mapped texture's alpha
/// is its bump's mask, not coverage: opaque, as reone has it (xoreos ignores the bump map and
/// blends by it, so the Hutts and the rancor showed through); reone still
/// discards its alpha-0 texels, which would leave pinholes, so none are.
pub fn surface(hint: u32, mesh_alpha: f32, loaded: &[&TextureFacts]) -> Surface {
    if loaded.is_empty() {
        return Surface::Opaque;
    }
    let alpha = loaded
        .iter()
        .all(|facts| facts.alpha && facts.alpha_mean != Some(1.0));
    let decal = loaded.iter().all(|facts| facts.decal);
    let additive = loaded
        .iter()
        .any(|facts| facts.blending == Blending::Additive);
    let envmap = loaded.iter().any(|facts| facts.envmap.is_some());
    let bumpmap = loaded.iter().any(|facts| facts.bumpmap.is_some());
    if additive {
        Surface::Additive
    } else if hint != 0 || decal || mesh_alpha < 1.0 {
        Surface::Blend
    } else if alpha && envmap {
        Surface::OpaqueEnvMasked
    } else if alpha && !bumpmap {
        Surface::OpaqueAlpha
    } else {
        Surface::Opaque
    }
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
    /// The lights casting shadows now (`shadows::AREA_SHADOWS`).
    #[storage(8, read_only)]
    pub shadows: Handle<ShaderBuffer>,
    pub surface: Surface,
    /// Its back faces are not drawn (reone and xoreos cull a KOTOR mesh's
    /// back faces, a decal texture's excepted): a camera behind a wall or
    /// under a floor sees through it, or inside a character's head through
    /// it. Set for every model's mesh but decals (`scene`); effects' cards
    /// and particles are drawn from both sides.
    pub culled: bool,
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
        let mut flags = match surface {
            Surface::OpaqueAlpha => DISCARD_ZERO_ALPHA,
            Surface::OpaqueEnvMasked => OPAQUE_ALPHA,
            Surface::BlendVertex | Surface::Lighten => VERTEX_ALPHA,
            _ => 0,
        };
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
        if area.shadowed
            && matches!(
                surface,
                Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked
            )
        {
            flags |= SHADOWED;
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
            },
            base,
            lightmap,
            envmap,
            lights: AREA_LIGHTS,
            shadows: crate::shadows::AREA_SHADOWS,
            surface,
            culled: false,
        }
    }

    /// The same light on no surface of its own: opaque, untextured, without
    /// its glow, and in its vertices' colours (a wound's cap, lit as the body
    /// it is cut from).
    pub fn bare(mut self) -> OdysseyMaterial {
        self.params.tint = Vec4::ONE;
        self.params.self_illum = Vec4::new(0.0, 0.0, 0.0, 1.0);
        self.params.uv_offset = Vec2::ZERO;
        self.params.flags &= STATIC | UNLIT | FOG | SHADOWED;
        self.base = None;
        self.lightmap = None;
        self.envmap = None;
        self.surface = Surface::Opaque;
        self.culled = true;
        self
    }

    /// Its back faces culled (`culled`), or not.
    pub fn culled(mut self, culled: bool) -> OdysseyMaterial {
        self.culled = culled;
        self
    }
}

/// What a material's pipeline depends on: its blend and whether it writes
/// depth. Surfaces that agree on both (opaque and env-masked opaque; blend
/// and vertex-alpha blend) share a pipeline rather than each compiling one.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct OdysseyKey {
    blend: Option<BlendState>,
    depth_write: bool,
    culled: bool,
}

impl From<&OdysseyMaterial> for OdysseyKey {
    fn from(material: &OdysseyMaterial) -> OdysseyKey {
        let surface = material.surface;
        OdysseyKey {
            blend: blend(surface),
            depth_write: !matches!(
                surface,
                Surface::Blend
                    | Surface::Additive
                    | Surface::AddOne
                    | Surface::BlendVertex
                    | Surface::Lighten
                    | Surface::Modulate
            ),
            culled: material.culled,
        }
    }
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

    fn enable_prepass() -> bool {
        false
    }

    /// Characters, doors and placeables cast (`shadows`); the rest is
    /// kept out of the maps.
    fn enable_shadows() -> bool {
        true
    }

    /// Alpha-tested surfaces (hair, grates) cast the shape their texture
    /// cuts out.
    fn prepass_fragment_shader() -> ShaderRef {
        ShaderRef::Handle(PREPASS_SHADER)
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let key = key.bind_group_data;
        let shadow_pass = descriptor
            .vertex
            .shader_defs
            .contains(&"PREPASS_PIPELINE".into());
        if shadow_pass {
            // Both faces cast: a cloak or a sleeve is one sheet.
            descriptor.primitive.cull_mode = None;
            return Ok(());
        }
        descriptor.primitive.cull_mode = key
            .culled
            .then_some(bevy::render::render_resource::Face::Back);
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
