//! A character's meshes drawn again over themselves, as the game's
//! executable does for two of its `ProgFX` (read off the Mac build), each
//! shell sharing its mesh and skin (`glow_shell.wgsl`):
//!
//! - A glow (1401 to 1426: the energy shields, the Force powers' flashes,
//!   carbonite, stealth): each vertex pushed `PUSH_M` out along its normal,
//!   with the glow's texture by the meshes' own UVs (a flip-book, played at
//!   its rate), blended as the texture's TXI says (every one of them
//!   additive). Each shell is a child of the mesh it covers.
//! - A distortion (1800: `VFX_DUR_DISTORTION`, Stealth Mode's look, a
//!   cloak's removal): the executable draws the character's meshes into its
//!   stencil only (colour writes off) and its frame buffer pass shows the
//!   scene there displaced by its ring texture, so the character is a
//!   ripple in what is behind it. Here the meshes are hidden and their
//!   shells, beside them, show the scene behind (Bevy's view transmission
//!   texture) displaced by the same rings (`ring_distort.wgsl`).
//! - A hologram (KOTOR 2's creatures whose template sets `Hologram`): as
//!   TSL's executable draws one, each mesh is drawn in its place by its
//!   hologram programs, not lit. The fragment program takes the luminance
//!   of the mesh's own texture (weights 0.30, 0.59 and 0.11, its
//!   `program.env[7]`) times the filter texture `Holotex`, and the filter's
//!   alpha. The vertex program samples the filter at the vertex's world
//!   height (`v`) and at the length of its world x and z (`u`), in metres,
//!   so the filter's rows lie as horizontal scan lines on the figure. The pass blends `GL_ONE, GL_SRC_ALPHA`: the
//!   colour is added, and what is behind is dimmed by the filter's alpha.

use asset_odyssey::texture::{Blending, TextureIr};
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorWrites,
    RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;

use crate::material::{self, OdysseyMaterial, Surface};
use crate::scene;

const SHADER: Handle<Shader> = uuid_handle!("8b2f4c61-0d7e-4a39-b5c8-1e6f3a9d7b24");

/// How far a glow's shell stands out of its mesh (metres): the game's
/// vertex program's `program.env[12].x`.
pub const PUSH_M: f32 = 0.02;

pub struct GlowShellPlugin;

impl Plugin for GlowShellPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "glow_shell.wgsl", crate::shader_checks::wgsl);
        app.add_plugins(MaterialPlugin::<GlowMaterial>::default());
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(GlowKey)]
pub struct GlowMaterial {
    /// x: how far the shell stands out (metres), y: 1 for a distortion
    /// (none for a glow), z: a mark's (drawn within its square only), w: 1
    /// for a hologram (`texture` the mesh's own, `filter` the hologram
    /// filter).
    #[uniform(0)]
    push: Vec4,
    #[texture(1)]
    #[sampler(2)]
    texture: Option<Handle<Image>>,
    /// What its texture is multiplied by (a mark's stage's `rgbGen` and
    /// `alphaGen`, `body_marks`); white for a shell.
    #[uniform(3)]
    pub tint: Vec4,
    /// A hologram's filter texture (`Holotex`).
    #[texture(4)]
    #[sampler(5)]
    filter: Option<Handle<Image>>,
    /// Where a danglymesh's vertices are (`dangly::SWAY`).
    #[storage(6, read_only, visibility(vertex))]
    sway: Handle<ShaderBuffer>,
    surface: Surface,
    /// A hologram's depth shell (`Glow::hologram_material`): drawn with the
    /// opaque meshes, writing depth and no colour.
    depth_only: bool,
}

impl GlowMaterial {
    fn distorts(&self) -> bool {
        self.push.y > 0.0
    }

    fn hologram(&self) -> bool {
        self.push.w > 0.0
    }

    /// A mark's stage (`body_marks`): `texture` by the mark's own UVs,
    /// `push` metres out of the skin it lies on, blended as `surface`.
    pub fn mark(texture: Handle<Image>, push: f32, surface: Surface) -> GlowMaterial {
        GlowMaterial {
            push: Vec4::new(push, 0.0, 1.0, 0.0),
            texture: Some(texture),
            tint: Vec4::ONE,
            filter: None,
            surface,
            depth_only: false,
            sway: crate::dangly::SWAY,
        }
    }
}

/// The weights TSL's hologram fragment programs take a texel's luminance
/// by (`program.env[7]`).
pub const HOLOGRAM_LUMINANCE: [f32; 3] = [0.30, 0.59, 0.11];

/// The blend of TSL's hologram pass, `GL_ONE, GL_SRC_ALPHA`: the
/// hologram's colour added to what is behind, which its alpha dims.
pub fn hologram_blend() -> BlendState {
    BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::SrcAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            src_factor: BlendFactor::Zero,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
    }
}

/// What a shell's pipeline depends on: its blend.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlowKey {
    surface: Surface,
    hologram: bool,
    depth_only: bool,
}

impl From<&GlowMaterial> for GlowKey {
    fn from(material: &GlowMaterial) -> GlowKey {
        GlowKey {
            surface: material.surface,
            hologram: material.hologram(),
            depth_only: material.depth_only,
        }
    }
}

impl Material for GlowMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn reads_view_transmission_texture(&self) -> bool {
        self.distorts()
    }

    /// A distortion in the transmissive phase, drawn over the whole scene
    /// (`heat_haze::draw_after_transparent`); `specialize` sets its blend.
    fn alpha_mode(&self) -> AlphaMode {
        if self.distorts() || self.depth_only {
            AlphaMode::Opaque
        } else {
            AlphaMode::Add
        }
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if crate::dangly::slot_attribute(descriptor, layout)? {
            descriptor.vertex.shader_defs.push("DANGLY".into());
        }
        descriptor.primitive.cull_mode = None;
        let depth_only = key.bind_group_data.depth_only;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                if depth_only {
                    target.blend = None;
                    target.write_mask = ColorWrites::empty();
                    continue;
                }
                target.blend = match key.bind_group_data.hologram {
                    true => Some(hologram_blend()),
                    false => material::blend(key.bind_group_data.surface),
                };
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(depth_only);
        }
        Ok(())
    }
}

/// The scene's meshes: each one's mesh, material, skin and parent.
pub type SceneMeshes<'w, 's> = Query<
    'w,
    's,
    (
        &'static Mesh3d,
        &'static MeshMaterial3d<OdysseyMaterial>,
        Option<&'static SkinnedMesh>,
        &'static ChildOf,
    ),
>;

/// A glow texture's frames, one image each, and how many it plays a second
/// (none for a still texture).
pub type Frames = (Vec<Handle<Image>>, f32);

/// A mesh distortions hid: how it was shown before the first, and how many
/// hide it now (one ending while another lasts leaves it hidden).
#[derive(Component)]
struct Shrouded {
    was: Visibility,
    by: u32,
}

/// A glow or distortion on a character: its material, its texture's frames
/// and rate, the shells it has put over the character's meshes (the mesh,
/// the shell), and the meshes a distortion hid.
pub struct Glow {
    distorts: bool,
    /// A hologram's filter texture: each mesh gets a material of its own
    /// texture and this filter, drawn in its place.
    hologram: Option<Handle<Image>>,
    material: Handle<GlowMaterial>,
    frames: Vec<Handle<Image>>,
    fps: f32,
    shells: Vec<(Entity, Entity)>,
    hidden: Vec<Entity>,
}

impl Glow {
    fn of(material: Handle<GlowMaterial>, (frames, fps): Frames) -> Glow {
        Glow {
            distorts: false,
            hologram: None,
            material,
            frames,
            fps,
            shells: Vec::new(),
            hidden: Vec::new(),
        }
    }

    /// A glow's images for `texture`: one a frame, and the rate.
    pub fn images(texture: &TextureIr, images: &mut Assets<Image>) -> Frames {
        scene::flipbook_images(texture, images)
            .unwrap_or_else(|| (vec![images.add(scene::material_image(texture))], 0.0))
    }

    /// A glow of `texture`, drawn with its `frames` (`Glow::images`).
    pub fn new(
        texture: &TextureIr,
        (frames, fps): Frames,
        materials: &mut Assets<GlowMaterial>,
    ) -> Glow {
        // The game blends the shell pass by its texture's TXI (its blend
        // table): additive `GL_SRC_ALPHA, GL_ONE` (every glow's), punch-
        // through `GL_ONE, GL_ZERO`, else `GL_SRC_ALPHA,
        // GL_ONE_MINUS_SRC_ALPHA`.
        let surface = match texture.facts.blending {
            Blending::Additive => Surface::Lighten,
            Blending::PunchThrough => Surface::Opaque,
            Blending::Default => Surface::Blend,
        };
        let material = materials.add(GlowMaterial {
            push: Vec4::new(PUSH_M, 0.0, 0.0, 0.0),
            texture: Some(frames[0].clone()),
            tint: Vec4::ONE,
            filter: None,
            surface,
            depth_only: false,
            sway: crate::dangly::SWAY,
        });
        Glow::of(material, (frames, fps))
    }

    /// A distortion (ProgFX 1800: effect 8000, Stealth Mode's look) as the
    /// executable draws it: its meshes hidden, and shells in their place
    /// showing the scene behind, displaced by the executable's ring texture.
    pub fn distortion(materials: &mut Assets<GlowMaterial>) -> Glow {
        let material = materials.add(GlowMaterial {
            push: Vec4::new(0.0, 1.0, 0.0, 0.0),
            texture: None,
            tint: Vec4::ONE,
            filter: None,
            surface: Surface::Blend,
            depth_only: false,
            sway: crate::dangly::SWAY,
        });
        Glow {
            distorts: true,
            ..Glow::of(material, (Vec::new(), 0.0))
        }
    }

    /// A hologram as TSL's executable draws one: every mesh hidden, and in
    /// its place a shell drawn by the hologram pass, with the mesh's own
    /// texture and `filter` (the executable's `Holotex`).
    pub fn hologram(filter: Handle<Image>) -> Glow {
        Glow {
            distorts: true,
            hologram: Some(filter),
            ..Glow::of(Handle::default(), (Vec::new(), 0.0))
        }
    }

    /// The material of a hologram's shell over a mesh whose texture is
    /// `texture`. Each mesh gets two shells. The depth shell (`depth_only`)
    /// is drawn first, with the opaque meshes, and writes only depth. The
    /// hologram pass then draws only the surface nearest the camera, as the
    /// executable does. The back of the head and the inside of the mouth no
    /// longer show through the face.
    fn hologram_material(&self, texture: Option<Handle<Image>>, depth_only: bool) -> GlowMaterial {
        GlowMaterial {
            push: Vec4::new(0.0, 0.0, 0.0, 1.0),
            texture,
            tint: Vec4::ONE,
            filter: self.hologram.clone(),
            surface: Surface::Blend,
            depth_only,
            sway: crate::dangly::SWAY,
        }
    }

    /// Whether it is a hologram's (`Glow::hologram`).
    pub fn is_hologram(&self) -> bool {
        self.hologram.is_some()
    }

    /// Covers every solid mesh under `root` not covered yet (a head or a
    /// weapon put on since included): a shell sharing its mesh and skin, a
    /// glow's under the mesh, a distortion's beside it with the mesh hidden.
    /// Its own shells are not meshes of the scene's material.
    pub fn cover(
        &mut self,
        root: Entity,
        (children, meshes): (&Query<&Children>, &SceneMeshes),
        (odyssey, glows): (&Assets<OdysseyMaterial>, &mut Assets<GlowMaterial>),
        commands: &mut Commands,
    ) {
        for entity in std::iter::once(root).chain(children.iter_descendants(root)) {
            let Ok((mesh, made, skin, parent)) = meshes.get(entity) else {
                continue;
            };
            let made = odyssey.get(&made.0);
            // A hologram covers every mesh: the executable draws all of a
            // hologram's parts by its hologram pass.
            let solid = made.is_some_and(|made| made.surface.is_opaque());
            let wanted = solid || self.hologram.is_some();
            if !wanted || self.shells.iter().any(|(covered, _)| *covered == entity) {
                continue;
            }
            let materials = match &self.hologram {
                Some(_) => {
                    let texture = made.and_then(|made| made.base.clone());
                    [true, false]
                        .map(|depth_only| {
                            glows.add(self.hologram_material(texture.clone(), depth_only))
                        })
                        .to_vec()
                }
                None => vec![self.material.clone()],
            };
            for material in materials {
                self.shell(entity, (mesh, skin, parent), material, commands);
            }
        }
    }

    /// One shell over `entity`'s mesh, drawn with `material`.
    fn shell(
        &mut self,
        entity: Entity,
        (mesh, skin, parent): (&Mesh3d, Option<&SkinnedMesh>, &ChildOf),
        material: Handle<GlowMaterial>,
        commands: &mut Commands,
    ) {
        let mut shell = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            NoFrustumCulling,
        ));
        if let Some(skin) = skin {
            shell.insert(skin.clone());
        }
        let shell = shell.id();
        if self.distorts {
            commands.entity(parent.parent()).add_child(shell);
            commands
                .entity(entity)
                .queue_silenced(|mut mesh: EntityWorldMut| {
                    if let Some(mut shrouded) = mesh.get_mut::<Shrouded>() {
                        shrouded.by += 1;
                    } else {
                        let was = mesh.get::<Visibility>().copied();
                        mesh.insert((
                            Shrouded {
                                was: was.unwrap_or(Visibility::Inherited),
                                by: 1,
                            },
                            Visibility::Hidden,
                        ));
                    }
                });
            self.hidden.push(entity);
        } else {
            commands.entity(entity).add_child(shell);
        }
        self.shells.push((entity, shell));
    }

    /// How many shells it has put on.
    pub fn shells(&self) -> usize {
        self.shells.len()
    }

    /// Shows its texture's frame `seconds` into the glow.
    pub fn play(&self, seconds: f32, materials: &mut Assets<GlowMaterial>) {
        if self.frames.len() < 2 {
            return;
        }
        let wanted = &self.frames[scene::flipbook_frame(self.frames.len(), self.fps, seconds)];
        if materials
            .get(&self.material)
            .is_some_and(|material| material.texture.as_ref() != Some(wanted))
            && let Some(mut material) = materials.get_mut(&self.material)
        {
            material.texture = Some(wanted.clone());
        }
    }

    /// Its shells gone (those whose meshes went took theirs with them), the
    /// meshes it hid shown as they were once no other distortion hides them.
    pub fn remove(&self, commands: &mut Commands) {
        for &(_, shell) in &self.shells {
            if let Ok(mut shell) = commands.get_entity(shell) {
                shell.despawn();
            }
        }
        for &mesh in &self.hidden {
            commands
                .entity(mesh)
                .queue_silenced(|mut mesh: EntityWorldMut| {
                    let Some(mut shrouded) = mesh.get_mut::<Shrouded>() else {
                        return;
                    };
                    shrouded.by -= 1;
                    if shrouded.by == 0 {
                        let was = shrouded.was;
                        mesh.remove::<Shrouded>().insert(was);
                    }
                });
        }
    }
}

/// The probes of the hologram look (`kjk sim probes`, group "holograms"):
/// a hologram's shell material is drawn by the hologram pass, blended
/// `GL_ONE, GL_SRC_ALPHA`, through its filter; a glow's is not.
#[cfg(feature = "probes")]
pub fn hologram_probes() -> sim::probe::Checks {
    use sim::probe::check;
    let filter = Handle::<Image>::default();
    let glow = Glow::hologram(filter.clone());
    let shell = glow.hologram_material(None, false);
    let key = GlowKey::from(&shell);
    let mark = GlowMaterial::mark(Handle::default(), 0.0, Surface::Blend);
    let blend = key.hologram.then(hologram_blend);
    vec![
        (
            "a hologram's shell takes the hologram pass and its filter",
            check(
                glow.is_hologram() && shell.hologram() && shell.filter == Some(filter),
                format!("push {:?}", shell.push),
            ),
        ),
        (
            "the hologram pass adds its colour and dims what is behind by its alpha",
            check(
                blend.is_some_and(|blend| {
                    blend.color.src_factor == BlendFactor::One
                        && blend.color.dst_factor == BlendFactor::SrcAlpha
                }),
                format!("{blend:?}"),
            ),
        ),
        (
            "a mark is no hologram",
            check(!GlowKey::from(&mark).hologram, String::new()),
        ),
    ]
}
