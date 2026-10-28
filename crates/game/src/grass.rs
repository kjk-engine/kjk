//! The area's grass, drawn as the games' executables draw it (EXE-NOTES
//! `GRASS_*`): one mesh per room (`viewer::GrassPatch`), a quad per cluster
//! standing upright on its point at the cluster's own turn (not facing the
//! camera), two-sided, a quarter of the area's texture each, alpha tested at
//! 0.4 and blended with depth writes on (`grass.wgsl`).
//!
//! Its light is the texture times the room's lightmap under the cluster,
//! white where the room has no lightmap (the executable takes the room
//! mesh's vertex colours there). The area's grass colours are carried but
//! not applied: every K1 area with grass gives `Grass_Ambient` and
//! `Grass_Diffuse` 0 (danm14aa, kas_m25aa, unk_m41aa), and multiplying by
//! them would draw every tuft black, so the material's colours cannot reach
//! the texture's in K1. TSL's (white ambient and diffuse, a grey emissive,
//! handed to its vertex program) [verify].
//!
//! Each cluster fades by its own point's distance, whole to 15 m and linear
//! in the squared distance to nothing at 30 m (the executable fades a bin at
//! a time); past 30 m it is not drawn at all. Within 15 m its top edge sways
//! in the area's wind, easing to still from 12 m out: a gust in the vertex
//! shader rather than the executable's spring per quad. A room's grass shows
//! when the room does (`Culling::room_visible`) and the grass option
//! (`Display::grass`, on by default) is on; off, the patches are only hidden.

use asset_odyssey::grass::Tuft;
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::camera::primitives::Aabb;
use bevy::mesh::{
    Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexFormat,
};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendState, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use console::{AnimationTime, DumpDue, DumpFacts};

use crate::display::Display;
#[cfg(feature = "probes")]
use crate::probe_kit::{Checks, check, not_installed};
use crate::scene::{self, Culling, SceneContent, SceneRoot};
use crate::viewer::{LoadedScene, SceneGrass};

const SHADER: Handle<Shader> = uuid_handle!("470ea61e-cb4d-4fad-b6d0-102c4586b024");

/// Each vertex's share of the sway: 0 at the bottom edge, 1 at the top.
pub const SWAY: MeshVertexAttribute =
    MeshVertexAttribute::new("GrassSway", 988_540_917, VertexFormat::Float32);
/// The cluster's point, the same on its four vertices: what its fade, sway
/// and drawing at all go by.
pub const BASE: MeshVertexAttribute =
    MeshVertexAttribute::new("GrassBase", 988_540_918, VertexFormat::Float32x3);

/// Whole within this distance (metres), and swaying (EXE-NOTES
/// `GRASS_FADE`, `GRASS_WIND`).
const NEAR_M: f32 = 15.0;
/// Gone from this distance (`GRASS_DISTANCE_M`).
const FAR_M: f32 = 30.0;
/// `GrassParams::shape.w` flags.
const FOGGED: u32 = 1;
const LIGHTMAPPED: u32 = 2;
/// How far the bounds stand out round the tufts, in quad sizes, at the
/// least: a top edge 1.2 quads tall.
const REACH_QUADS: f32 = 1.2;

pub struct GrassPlugin;

impl Plugin for GrassPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "grass.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<GrassMaterial>::default())
            .add_systems(
                Update,
                // Before the warm-up, which reads the patches' meshes.
                spawn
                    .after(scene::spawn_scene)
                    .before(crate::warmup::start)
                    .run_if(resource_exists_and_changed::<SceneContent>),
            )
            .add_systems(
                Update,
                (option.before(show), show.after(scene::RoomsCulled), blow),
            );
    }
}

/// A room's grass mesh: the room's index in `LoadedScene::rooms`.
#[derive(Component)]
pub struct GrassOf(pub usize);

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct GrassParams {
    pub ambient: Vec4,
    pub diffuse: Vec4,
    pub emissive: Vec4,
    pub fog_color: Vec4,
    pub fog_range: Vec4,
    /// x: quad size, y: far radius² (900: 30 m), z: near radius² (225:
    /// 15 m), w: flags (1 = fogged, 2 = lightmap).
    pub shape: Vec4,
    /// xy: the area's wind direction × strength (`WindPower` 0, 1, 2 along
    /// +X+Y, game space), z: seconds (the sway's clock), w: unused.
    pub wind: Vec4,
    /// The lightning's light (`weather::Flash`): xyz where (Bevy world
    /// space), w its radius (0: none).
    pub flash_at: Vec4,
    /// Its colour: its keyed colour × its multiplier, added to the stored
    /// (gamma-encoded) values the shader works on.
    pub flash_colour: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct GrassMaterial {
    #[uniform(0)]
    pub params: GrassParams,
    #[texture(1)]
    #[sampler(2)]
    pub art: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub lightmap: Option<Handle<Image>>,
}

impl Material for GrassMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// `GRASS_BLEND`: blended by the source alpha, depth writes on, culling
    /// off; the alpha test is the shader's.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(2),
            SWAY.at_shader_location(3),
            BASE.at_shader_location(4),
        ])?];
        descriptor.primitive.cull_mode = None;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::ALPHA_BLENDING);
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(true);
        }
        Ok(())
    }
}

/// The patch's quads in game space (`GRASS_SHAPE`): with `d` the tuft's
/// turn, the bottom edge `quad` wide on its point, the top edge `quad ×
/// size` wide and as high; a quarter of the texture each (the variant's:
/// u from 0.5 when odd, v from 0.5 from the third), its v rising up the
/// quad as the game's do (the other way round stands the tufts on their
/// heads: flowers at the ground).
fn patch_mesh(tufts: &[Tuft], quad: f32) -> Mesh {
    let mut positions = Vec::with_capacity(tufts.len() * 4);
    let mut uvs = Vec::with_capacity(tufts.len() * 4);
    let mut indices = Vec::with_capacity(tufts.len() * 6);
    for (index, tuft) in tufts.iter().enumerate() {
        let p = Vec3::from_array(tuft.position);
        let d = Vec3::new(tuft.yaw.cos(), tuft.yaw.sin(), 0.0);
        let (bottom, top) = (quad / 2.0, quad * tuft.size / 2.0);
        let rise = Vec3::Z * quad * tuft.size;
        positions.extend(
            [
                p - d * bottom,
                p + d * bottom,
                p - d * top + rise,
                p + d * top + rise,
            ]
            .map(|corner| corner.to_array()),
        );
        let u = if tuft.variant % 2 == 1 { 0.5 } else { 0.0 };
        let v = if tuft.variant >= 2 { 0.5 } else { 0.0 };
        uvs.extend([[u, v], [u + 0.5, v], [u, v + 0.5], [u + 0.5, v + 0.5]]);
        let first = index as u32 * 4;
        indices.extend([0, 1, 2, 1, 3, 2].map(|corner| first + corner));
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_1,
        tufts
            .iter()
            .flat_map(|tuft| [tuft.lightmap_uv; 4])
            .collect::<Vec<_>>(),
    )
    .with_inserted_attribute(
        SWAY,
        tufts
            .iter()
            .flat_map(|_| [0.0f32, 0.0, 1.0, 1.0])
            .collect::<Vec<_>>(),
    )
    .with_inserted_attribute(
        BASE,
        tufts
            .iter()
            .flat_map(|tuft| [tuft.position; 4])
            .collect::<Vec<_>>(),
    )
    .with_inserted_indices(Indices::U32(indices))
}

/// The tufts' points grown on every side by a top edge's height, or by how
/// far the wind pushes a top edge aside (`wind` quads at most) and a little.
fn bounds(tufts: &[Tuft], quad: f32, wind: f32) -> Aabb {
    let points = tufts.iter().map(|tuft| Vec3::from_array(tuft.position));
    let (low, high) = points.fold((Vec3::MAX, Vec3::MIN), |(low, high), point| {
        (low.min(point), high.max(point))
    });
    let reach = Vec3::splat(quad * REACH_QUADS.max(wind + 0.2));
    Aabb::from_min_max(low - reach, high + reach)
}

/// The area's parameters for a patch, its lightmap's flag set when it has
/// one.
fn params(grass: &SceneGrass, scene: &LoadedScene, lightmapped: bool) -> GrassParams {
    let colour = |rgb: Vec3| rgb.extend(1.0);
    let mut flags = 0;
    if scene.fog.is_some() {
        flags |= FOGGED;
    }
    if lightmapped {
        flags |= LIGHTMAPPED;
    }
    let wind = Vec2::ONE.normalize() * grass.wind;
    GrassParams {
        ambient: colour(grass.ambient),
        diffuse: colour(grass.diffuse),
        emissive: colour(grass.emissive),
        fog_color: scene
            .fog
            .map_or(Vec4::ZERO, |fog| colour(Vec3::from_array(fog.color))),
        fog_range: scene
            .fog
            .map_or(Vec4::ZERO, |fog| Vec4::new(fog.near, fog.far, 0.0, 0.0)),
        shape: Vec4::new(
            grass.quad_size,
            FAR_M * FAR_M,
            NEAR_M * NEAR_M,
            flags as f32,
        ),
        wind: wind.extend(0.0).extend(0.0),
        flash_at: Vec4::ZERO,
        flash_colour: Vec4::ZERO,
    }
}

/// One entity per room patch under the new scene's root.
fn spawn(
    mut commands: Commands,
    scene: Res<SceneContent>,
    roots: Query<Entity, With<SceneRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let loaded = &scene.0;
    let (Some(grass), Ok(root)) = (&loaded.grass, roots.single()) else {
        return;
    };
    // A texture that did not decode draws nothing rather than white quads.
    let Some(art) = loaded.textures.get(&grass.texture) else {
        return;
    };
    let art = images.add(scene::image(art));
    let mut lightmaps = std::collections::BTreeMap::new();
    for patch in &grass.patches {
        let lightmap = patch.lightmap.as_ref().and_then(|name| {
            let texture = loaded.textures.get(name)?;
            Some(
                lightmaps
                    .entry(name)
                    .or_insert_with(|| images.add(scene::material_image(texture)))
                    .clone(),
            )
        });
        let material = materials.add(GrassMaterial {
            params: params(grass, loaded, lightmap.is_some()),
            art: Some(art.clone()),
            lightmap,
        });
        let entity = commands
            .spawn((
                GrassOf(patch.room),
                Mesh3d(meshes.add(patch_mesh(&patch.tufts, grass.quad_size))),
                MeshMaterial3d(material),
                bounds(&patch.tufts, grass.quad_size, grass.wind),
                Transform::IDENTITY,
                Visibility::Inherited,
            ))
            .id();
        commands.entity(root).add_child(entity);
    }
}

/// Each room's grass shown when the room is (and the option is on: while
/// it is off `option` keeps every patch hidden), and the grass in the dump.
fn show(
    scene: Option<Res<SceneContent>>,
    culling: Res<Culling>,
    display: Option<Res<Display>>,
    mut patches: Query<(&GrassOf, &mut Visibility)>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
) {
    let Some(scene) = scene else { return };
    let on = display.is_none_or(|display| display.grass);
    let mut shown = 0;
    for (room, mut visibility) in &mut patches {
        let visible = on && culling.shows(room.0);
        shown += usize::from(visible);
        if on {
            visibility.set_if_neq(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
    if dump_due.0
        && let Some(grass) = &scene.0.grass
    {
        facts.0.insert(
            "grass".into(),
            serde_json::json!({
                "texture": grass.texture,
                "missing_texture": grass.missing_texture,
                "shown": shown,
                "patches": grass
                    .patches
                    .iter()
                    .map(|patch| serde_json::json!({ "room": patch.room, "tufts": patch.tufts.len() }))
                    .collect::<Vec<_>>(),
            }),
        );
    }
}

/// The options' grass: hidden while it is off, and let go (shown, for
/// `show` to narrow to the visible rooms) when it comes on, at once and
/// for patches that have just arrived. Only `Visibility` changes: nothing
/// is despawned or rebuilt.
fn option(
    display: Option<Res<Display>>,
    mut patches: Query<&mut Visibility, With<GrassOf>>,
    arrived: Query<(), Added<GrassOf>>,
) {
    let Some(display) = display else { return };
    if !display.is_changed() && arrived.is_empty() {
        return;
    }
    let wanted = if display.grass {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut patches {
        visibility.set_if_neq(wanted);
    }
}

/// The sway's clock, written to each patch's material each frame (none in
/// a still area).
fn blow(
    patches: Query<&MeshMaterial3d<GrassMaterial>, With<GrassOf>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
    (time, held): (Res<Time>, Res<AnimationTime>),
) {
    let seconds = scene::animation_seconds(&time, &held);
    for handle in &patches {
        if materials
            .get(&handle.0)
            .is_some_and(|material| material.params.wind.xy() != Vec2::ZERO)
            && let Some(mut material) = materials.get_mut(&handle.0)
        {
            material.params.wind.z = seconds;
        }
    }
}

/// The option: the setting reads back, and the option system only shows
/// and hides (the same entity, whatever the option does).
#[cfg(feature = "probes")]
fn option_probes() -> Checks {
    let mut settings = crate::settings::Settings::defaults();
    let on = settings.display().grass;
    settings.apply("display.grass", "false");
    let read = check(on && !settings.display().grass, format!("{on} then off"));

    #[derive(Component)]
    struct Marker;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Display>()
        .add_systems(Update, option);
    let grass = app
        .world_mut()
        .spawn((GrassOf(0), Marker, Visibility::Inherited))
        .id();
    let step = |app: &mut App, shown: bool| {
        app.world_mut().resource_mut::<Display>().grass = shown;
        app.update();
        (
            app.world().get::<Visibility>(grass).copied(),
            app.world().get::<Marker>(grass).is_some(),
        )
    };
    let (off, kept_off) = step(&mut app, false);
    let (back, kept_back) = step(&mut app, true);
    let toggled = check(
        off == Some(Visibility::Hidden)
            && back == Some(Visibility::Inherited)
            && kept_off
            && kept_back,
        format!(
            "off {off:?}, back {back:?}, same entity {}",
            kept_off && kept_back
        ),
    );
    vec![
        ("grass_option", read),
        ("grass_option_hides_not_rebuilds", toggled),
    ]
}

/// The grass a module's entry area is drawn with, built without an app:
/// placed the same twice, none in an area without grass, and no patches
/// when the install lacks the area's texture; and the option's own checks.
#[cfg(feature = "probes")]
pub fn probes() -> Checks {
    let mut checks = option_probes();
    checks.extend(scene_probes());
    checks
}

#[cfg(feature = "probes")]
fn scene_probes() -> Checks {
    use asset_odyssey::module::ModuleIr;
    use vfs::Game;
    use vfs::resolver::Resolver;

    use crate::viewer;
    let discovery = crate::discover();
    if discovery.get(Game::Kotor1).is_none() {
        return vec![("grass_in_scenes", Ok(not_installed(Game::Kotor1)))];
    }
    let opened = |module: &str| -> Result<(Resolver, ModuleIr, usize), String> {
        let resolver = viewer::resolver(&discovery, Game::Kotor1, Some(module))?;
        let ir = ModuleIr::build(module, &viewer::ResolverSource(&resolver))
            .map_err(|refusal| format!("{module}: {refusal:?}"))?;
        let area = ir
            .areas
            .iter()
            .position(|area| area.resref == ir.entry_area)
            .ok_or(format!("{module}: no entry area"))?;
        Ok((resolver, ir, area))
    };
    let scene = |(resolver, ir, area): &(Resolver, ModuleIr, usize)| {
        viewer::module_scene(resolver, Game::Kotor1, ir, *area)
    };
    let tufts = |scene: &LoadedScene| -> usize {
        scene
            .grass
            .iter()
            .flat_map(|grass| &grass.patches)
            .map(|patch| patch.tufts.len())
            .sum()
    };
    let seeded = opened("danm14aa").and_then(|dan| {
        let (first, second) = (scene(&dan)?, scene(&dan)?);
        let patches = first.grass.as_ref().map_or(0, |grass| grass.patches.len());
        let shown = first.grass.as_ref().is_some_and(|grass| {
            !grass.missing_texture && grass.patches.iter().any(|patch| !patch.tufts.is_empty())
        });
        let same = first.grass.as_ref().map(|grass| &grass.patches)
            == second.grass.as_ref().map(|grass| &grass.patches);
        check(
            shown && same,
            format!(
                "danm14aa: {patches} patches, {} tufts, the same twice: {same}",
                tufts(&first)
            ),
        )
    });
    let none = opened("end_m01aa").and_then(|end| {
        let built = scene(&end)?;
        check(
            built.grass.is_none(),
            format!("end_m01aa: grass {}", built.grass.is_some()),
        )
    });
    let missing = opened("danm14aa").and_then(|(resolver, mut ir, area)| {
        let grass = ir.areas[area].grass.as_mut().ok_or("danm14aa: no grass")?;
        grass.texture = "nosuchtexture".into();
        let built = scene(&(resolver, ir, area))?;
        let grass = built.grass.as_ref().ok_or("no grass drawn")?;
        check(
            grass.missing_texture && grass.patches.is_empty(),
            format!(
                "nosuchtexture: missing {}, {} patches",
                grass.missing_texture,
                grass.patches.len()
            ),
        )
    });
    vec![
        ("grass_scene_seeded", seeded),
        ("grass_scene_none", none),
        ("grass_missing_texture", missing),
    ]
}
