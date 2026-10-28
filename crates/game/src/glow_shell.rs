//! A character's meshes drawn again over themselves, as the game's
//! executable does for two of its `ProgFX` (read off the Mac build), each
//! shell sharing its mesh and skin (`glow_shell.wgsl`):
//!
//! - A glow (1401 to 1426: the energy shields, the Force powers' flashes,
//!   carbonite, stealth): each vertex pushed `PUSH_M` out along its normal,
//!   with the glow's texture by the meshes' own UVs (a flip-book, played at
//!   its rate), blended as the texture's TXI says (every one of them
//!   additive). Each shell is a child of the mesh it covers.
//! - A distortion (1800: `VFX_DUR_DISTORTION`, a cloak's removal): the
//!   executable draws the character's meshes into its stencil only (colour
//!   writes off) and its frame buffer pass bends the scene there, so the
//!   character is a ripple in what is behind it. Here the meshes are hidden
//!   and their shells, beside them, show the scene behind (Bevy's view
//!   transmission texture) moved by their normals, `BEND` of the screen at
//!   most [verify by eye: the game's pass and its strength].

use asset_odyssey::texture::{Blending, TextureIr};
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::material::{self, OdysseyMaterial, Surface};
use crate::scene;

const SHADER: Handle<Shader> = uuid_handle!("8b2f4c61-0d7e-4a39-b5c8-1e6f3a9d7b24");

/// How far a glow's shell stands out of its mesh (metres): the game's
/// vertex program's `program.env[12].x`.
pub const PUSH_M: f32 = 0.02;
/// How far a distortion moves the scene behind it at most (a share of the
/// screen).
const BEND: f32 = 0.03;

pub struct GlowShellPlugin;

impl Plugin for GlowShellPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "glow_shell.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<GlowMaterial>::default());
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(GlowKey)]
pub struct GlowMaterial {
    /// x: how far the shell stands out (metres), y: a distortion's bend
    /// (none for a glow), z: a mark's (drawn within its square only).
    #[uniform(0)]
    push: Vec4,
    #[texture(1)]
    #[sampler(2)]
    texture: Option<Handle<Image>>,
    /// What its texture is multiplied by (a mark's stage's `rgbGen` and
    /// `alphaGen`, `body_marks`); white for a shell.
    #[uniform(3)]
    pub tint: Vec4,
    surface: Surface,
}

impl GlowMaterial {
    fn distorts(&self) -> bool {
        self.push.y > 0.0
    }

    /// A mark's stage (`body_marks`): `texture` by the mark's own UVs,
    /// `push` metres out of the skin it lies on, blended as `surface`.
    pub fn mark(texture: Handle<Image>, push: f32, surface: Surface) -> GlowMaterial {
        GlowMaterial {
            push: Vec4::new(push, 0.0, 1.0, 0.0),
            texture: Some(texture),
            tint: Vec4::ONE,
            surface,
        }
    }
}

/// What a shell's pipeline depends on: its blend.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlowKey {
    surface: Surface,
}

impl From<&GlowMaterial> for GlowKey {
    fn from(material: &GlowMaterial) -> GlowKey {
        GlowKey {
            surface: material.surface,
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

    fn alpha_mode(&self) -> AlphaMode {
        if self.distorts() {
            AlphaMode::Blend
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
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = material::blend(key.bind_group_data.surface);
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
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
    material: Handle<GlowMaterial>,
    frames: Vec<Handle<Image>>,
    fps: f32,
    shells: Vec<(Entity, Entity)>,
    hidden: Vec<Entity>,
}

impl Glow {
    fn of(material: Handle<GlowMaterial>, (frames, fps): (Vec<Handle<Image>>, f32)) -> Glow {
        Glow {
            distorts: false,
            material,
            frames,
            fps,
            shells: Vec::new(),
            hidden: Vec::new(),
        }
    }

    /// A glow's images for `texture`: one a frame, and the rate.
    pub fn images(texture: &TextureIr, images: &mut Assets<Image>) -> (Vec<Handle<Image>>, f32) {
        scene::flipbook_images(texture, images)
            .unwrap_or_else(|| (vec![images.add(scene::material_image(texture))], 0.0))
    }

    /// A glow of `texture`, drawn with its `frames` (`Glow::images`).
    pub fn new(
        texture: &TextureIr,
        (frames, fps): (Vec<Handle<Image>>, f32),
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
            surface,
        });
        Glow::of(material, (frames, fps))
    }

    /// A distortion: shells in the meshes' place showing the scene behind,
    /// bent, its meshes hidden.
    pub fn distortion(materials: &mut Assets<GlowMaterial>) -> Glow {
        let material = materials.add(GlowMaterial {
            push: Vec4::new(0.0, BEND, 0.0, 0.0),
            texture: None,
            tint: Vec4::ONE,
            surface: Surface::Blend,
        });
        Glow {
            distorts: true,
            ..Glow::of(material, (Vec::new(), 0.0))
        }
    }

    /// Covers every solid mesh under `root` not covered yet (a head or a
    /// weapon put on since included): a shell sharing its mesh and skin, a
    /// glow's under the mesh, a distortion's beside it with the mesh hidden.
    /// Its own shells are not meshes of the scene's material.
    pub fn cover(
        &mut self,
        root: Entity,
        (children, meshes): (&Query<&Children>, &SceneMeshes),
        odyssey: &Assets<OdysseyMaterial>,
        commands: &mut Commands,
    ) {
        for entity in std::iter::once(root).chain(children.iter_descendants(root)) {
            let Ok((mesh, made, skin, parent)) = meshes.get(entity) else {
                continue;
            };
            let solid = odyssey.get(&made.0).is_some_and(|made| {
                matches!(
                    made.surface,
                    Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked
                )
            });
            if !solid || self.shells.iter().any(|(covered, _)| *covered == entity) {
                continue;
            }
            let mut shell = commands.spawn((
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(self.material.clone()),
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
