//! Bevy's extract systems that have nothing to copy are skipped.
//!
//! The extract runs on the main thread while the render thread waits.
//! Bevy puts about 165 systems in it. In a fight about 80 of them find
//! nothing to copy, every frame. They are Bevy UI's, the 2D sprites' and
//! meshes', the atmosphere's, the volumetric fog's, the decals', the light
//! probes', the lightmaps' and the morph targets', and those of every
//! material that no mesh wears. Each one still costs the executor a
//! hand-over to a pool thread and back.
//!
//! Each such system gets a gate, a run condition. The gate is open while
//! the main world has an entity with one of the components that the system
//! reads, and for `TAIL` frames after the last one is gone. In those last
//! frames the system clears what it copied before. A system run with
//! nothing to copy leaves the render world as its first such run left it,
//! so skipping it changes nothing. A system that copies an asset type also
//! runs while asset messages of that type wait, so it misses no added or
//! changed asset. Bevy UI's systems sit in Bevy's own sets, and the sets
//! take the gate. With no UI node, Bevy UI would still make a UI view for
//! every camera each frame, and draw nothing into it.
//!
//! `KJK_EXTRACT_GATES=off` runs every system, as Bevy has it.

use std::any::type_name;

use bevy::asset::AssetEvent;
use bevy::ecs::message::Messages;
use bevy::ecs::query::QueryFilter;
use bevy::ecs::schedule::{BoxedCondition, ConditionWithAccess};
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{
    Atmosphere, ClusteredDecal, DirectionalLightTexture, EnvironmentMapLight, FogVolume,
    GeneratedEnvironmentMapLight, IrradianceVolume, PointLightTexture, SpotLightTexture,
    VolumetricFog, VolumetricLight,
};
use bevy::mesh::morph::MeshMorphWeights;
use bevy::pbr::decal::ForwardDecalMaterialExt;
use bevy::pbr::{ExtendedMaterial, Lightmap, Material, MeshMaterial3d};
use bevy::prelude::*;
use bevy::render::{ExtractSchedule, RenderApp};
use bevy::sprite_render::{
    ColorMaterial, Material2d, MeshMaterial2d, SpriteMaterial, TilemapChunkMaterial,
};
use bevy::ui_render::RenderUiSystems;

/// Frames a gate stays open after the last component it watches is gone.
/// Two runs with nothing to copy bring every gated system to rest (the
/// morph targets keep the frame before's weights for one more frame).
const TAIL: u64 = 2;

/// Whether each gate is open this frame, in the render world.
#[derive(Resource, Default)]
struct ExtractGates(Vec<bool>);

/// What a gate watches in the main world.
type Watch = Box<dyn FnMut(&mut World) -> bool + Send>;

struct Gate {
    watch: Watch,
    /// An earlier gate whose sighting also counts for this one.
    with: Option<usize>,
    last_seen: u64,
}

/// The gates, and which systems each one holds (by Bevy's system names).
#[derive(Default)]
struct Gates {
    gates: Vec<Gate>,
    systems: Vec<(String, usize)>,
    frame: u64,
}

impl Gates {
    fn add(&mut self, watch: Watch, systems: impl IntoIterator<Item = String>) -> usize {
        self.add_with(None, watch, systems)
    }

    fn add_with(
        &mut self,
        with: Option<usize>,
        watch: Watch,
        systems: impl IntoIterator<Item = String>,
    ) -> usize {
        let index = self.gates.len();
        self.gates.push(Gate {
            watch,
            with,
            last_seen: 0,
        });
        self.systems
            .extend(systems.into_iter().map(|name| (name, index)));
        index
    }

    /// Opens or closes every gate for this frame.
    fn update(&mut self, main: &mut World, open: &mut Vec<bool>) {
        self.frame += 1;
        open.resize(self.gates.len(), true);
        for (index, open) in open.iter_mut().enumerate() {
            let with = self.gates[index]
                .with
                .is_some_and(|with| self.gates[with].last_seen == self.frame);
            let gate = &mut self.gates[index];
            if with || (gate.watch)(main) {
                gate.last_seen = self.frame;
            }
            *open = self.frame - gate.last_seen <= TAIL;
        }
    }

    /// The gate of the system named `name`. A generic system's name is
    /// its path, then its type parameters in angle brackets.
    fn gate_of(&self, name: &str) -> Option<usize> {
        self.systems.iter().find_map(|(prefix, gate)| {
            let rest = name.strip_prefix(prefix.as_str())?;
            (rest.is_empty() || rest.starts_with(',') || rest.starts_with('>')).then_some(*gate)
        })
    }
}

/// Whether any entity matches `F`.
fn any<F: QueryFilter + 'static>() -> Watch {
    let mut query: Option<QueryState<(), F>> = None;
    Box::new(move |world| {
        let query = query.get_or_insert_with(|| QueryState::new(world));
        query.iter(world).next().is_some()
    })
}

fn either(mut first: Watch, mut second: Watch) -> Watch {
    Box::new(move |world| first(world) || second(world))
}

/// Whether asset messages of `A` wait to be read.
fn messages<A: Asset>() -> Watch {
    Box::new(|world| {
        world
            .get_resource::<Messages<AssetEvent<A>>>()
            .is_some_and(|messages| !messages.is_empty())
    })
}

fn named(path: &str, generic: &str) -> String {
    format!("{path}<{generic}")
}

fn material_3d<M: Material>(gates: &mut Gates) {
    let ty = type_name::<M>();
    let worn = gates.add(
        any::<With<MeshMaterial3d<M>>>(),
        [
            "bevy_pbr::material::extract_mesh_materials",
            "bevy_pbr::material::early_sweep_material_instances",
            "bevy_pbr::material::extract_entities_needs_specialization",
            "bevy_pbr::material::extract_entities_that_need_specializations_removed",
        ]
        .map(|path| named(path, ty)),
    );
    gates.add_with(
        Some(worn),
        messages::<M>(),
        [named(
            "bevy_render::erased_render_asset::extract_erased_render_asset",
            &format!("bevy_pbr::mesh_material::MeshMaterial3d<{ty}>"),
        )],
    );
}

fn material_2d<M: Material2d>(gates: &mut Gates) {
    let ty = type_name::<M>();
    let worn = gates.add(
        any::<With<MeshMaterial2d<M>>>(),
        [
            "bevy_sprite_render::mesh2d::material::extract_mesh_materials_2d",
            "bevy_sprite_render::mesh2d::material::extract_entities_needs_specialization",
            "bevy_sprite_render::mesh2d::material::extract_entities_that_need_specializations_removed",
        ]
        .map(|path| named(path, ty)),
    );
    gates.add_with(
        Some(worn),
        messages::<M>(),
        [named(
            "bevy_render::render_asset::extract_render_asset",
            &format!("bevy_sprite_render::mesh2d::material::PreparedMaterial2d<{ty}>"),
        )],
    );
}

fn gates() -> Gates {
    let mut gates = Gates::default();
    let one = |path: &str| [path.to_string()];
    gates.add(
        either(any::<With<Sprite>>(), any::<With<Text2d>>()),
        [
            "bevy_sprite_render::render::extract_sprites".to_string(),
            "bevy_sprite_render::text2d::extract_text2d_sprite".to_string(),
        ],
    );
    gates.add(
        any::<With<Camera2d>>(),
        [
            "bevy_core_pipeline::core_2d::extract_core_2d_camera_phases".to_string(),
            named(
                "bevy_render::extract_component::extract_components",
                type_name::<Camera2d>(),
            ),
        ],
    );
    gates.add(
        any::<With<Mesh2d>>(),
        one("bevy_sprite_render::mesh2d::mesh::extract_mesh2d"),
    );
    material_2d::<ColorMaterial>(&mut gates);
    material_2d::<SpriteMaterial>(&mut gates);
    material_2d::<TilemapChunkMaterial>(&mut gates);

    let atmosphere = gates.add(
        any::<With<Atmosphere>>(),
        one("bevy_pbr::atmosphere::extract_atmosphere"),
    );
    gates.add_with(
        Some(atmosphere),
        messages::<ScatteringMedium>(),
        [named(
            "bevy_render::render_asset::extract_render_asset",
            "bevy_pbr::medium::GpuScatteringMedium",
        )],
    );
    gates.add(
        any::<Or<(With<VolumetricFog>, With<FogVolume>, With<VolumetricLight>)>>(),
        one("bevy_pbr::volumetric_fog::render::extract_volumetric_fog"),
    );
    gates.add(
        any::<With<ClusteredDecal>>(),
        one("bevy_pbr::decal::clustered::extract_clustered_decal"),
    );
    gates.add(
        any::<
            Or<(
                With<ClusteredDecal>,
                With<SpotLightTexture>,
                With<PointLightTexture>,
                With<DirectionalLightTexture>,
            )>,
        >(),
        one("bevy_pbr::decal::clustered::extract_decals"),
    );
    gates.add(
        any::<With<EnvironmentMapLight>>(),
        [
            named(
                "bevy_pbr::light_probe::gather_light_probes",
                type_name::<EnvironmentMapLight>(),
            ),
            named(
                "bevy_render::extract_instances::extract_all",
                "bevy_pbr::light_probe::environment_map::EnvironmentMapIds",
            ),
        ],
    );
    gates.add(
        any::<With<IrradianceVolume>>(),
        [named(
            "bevy_pbr::light_probe::gather_light_probes",
            type_name::<IrradianceVolume>(),
        )],
    );
    gates.add(
        any::<With<GeneratedEnvironmentMapLight>>(),
        one("bevy_pbr::light_probe::generate::extract_generated_environment_map_entities"),
    );
    gates.add(
        any::<With<Lightmap>>(),
        one("bevy_pbr::lightmap::extract_lightmaps"),
    );
    gates.add(
        any::<With<MeshMorphWeights>>(),
        one("bevy_pbr::render::morph::extract_morphs"),
    );

    material_3d::<StandardMaterial>(&mut gates);
    material_3d::<ExtendedMaterial<StandardMaterial, ForwardDecalMaterialExt>>(&mut gates);
    material_3d::<crate::material::OdysseyMaterial>(&mut gates);
    material_3d::<crate::heat_haze::HeatHazeMaterial>(&mut gates);
    material_3d::<crate::temporal::MotionDebugMaterial>(&mut gates);
    material_3d::<crate::speed_blur::SpeedBlurMaterial>(&mut gates);
    material_3d::<crate::ui_material::UiMaterial>(&mut gates);
    material_3d::<crate::grass::GrassMaterial>(&mut gates);
    material_3d::<crate::shell::ShellMaterial>(&mut gates);
    material_3d::<crate::scene_fx::RefractionMaterial>(&mut gates);
    material_3d::<crate::glow_shell::GlowMaterial>(&mut gates);
    gates
}

/// Gives each gated extract system its gate's condition. It runs before
/// the extract schedule's first run, so the conditions are initialised
/// with the systems. Systems are found by their names, which are their
/// functions' paths. A system that Bevy renames is not gated and runs as
/// before, and the log's count of gated systems drops.
fn install(render: &mut World, gates: &Gates) {
    render.resource_scope(|_, mut schedules: Mut<Schedules>| {
        let Some(schedule) = schedules.get_mut(ExtractSchedule) else {
            return;
        };
        let systems = &mut schedule.graph_mut().systems;
        if systems.is_initialized() {
            warn!("extract gates: the extract schedule already ran, nothing is gated");
            return;
        }
        let gated: Vec<_> = systems
            .iter()
            .filter_map(|(key, system, _)| Some((key, gates.gate_of(&system.name().to_string())?)))
            .collect();
        for &(key, gate) in &gated {
            let condition: BoxedCondition =
                Box::new(IntoSystem::into_system(move |gates: Res<ExtractGates>| {
                    gates.0.get(gate).copied().unwrap_or(true)
                }));
            if let Some(conditions) = systems.get_conditions_mut(key) {
                conditions.push(ConditionWithAccess::new(condition));
            }
        }
        info!(
            "extract gates: {} of {} extract systems gated",
            gated.len(),
            systems.len()
        );
    });
}

pub struct ExtractGatesPlugin;

impl Plugin for ExtractGatesPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        if std::env::var("KJK_EXTRACT_GATES").is_ok_and(|value| value == "off") {
            return;
        }
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let mut gates = gates();
        let ui = gates.add(any::<With<Node>>(), []);
        render.init_resource::<ExtractGates>();
        render.configure_sets(
            ExtractSchedule,
            (
                RenderUiSystems::ExtractCameraViews,
                RenderUiSystems::ExtractBoxShadows,
                RenderUiSystems::ExtractBackgrounds,
                RenderUiSystems::ExtractImages,
                RenderUiSystems::ExtractTextureSlice,
                RenderUiSystems::ExtractBorders,
                RenderUiSystems::ExtractViewportNodes,
                RenderUiSystems::ExtractTextBackgrounds,
                RenderUiSystems::ExtractTextShadows,
                RenderUiSystems::ExtractText,
                RenderUiSystems::ExtractCursor,
                RenderUiSystems::ExtractDebug,
                RenderUiSystems::ExtractGradient,
            )
                .distributive_run_if(move |gates: Res<ExtractGates>| {
                    gates.0.get(ui).copied().unwrap_or(true)
                }),
        );
        let Some(mut extract) = render.take_extract() else {
            return;
        };
        let mut installed = false;
        render.set_extract(move |main, render| {
            if !installed {
                installed = true;
                install(render, &gates);
            }
            let mut open = std::mem::take(&mut render.resource_mut::<ExtractGates>().0);
            gates.update(main, &mut open);
            render.resource_mut::<ExtractGates>().0 = open;
            extract(main, render);
        });
    }
}
