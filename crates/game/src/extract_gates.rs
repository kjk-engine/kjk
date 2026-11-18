//! Bevy's extract systems that have nothing to copy are skipped.
//!
//! The extract runs on the main thread while the render thread waits.
//! Bevy puts 166 systems in it. In a fight about 90 of them find nothing to
//! copy, every frame. They are Bevy UI's, the 2D sprites' and meshes', the
//! atmosphere's, the volumetric fog's, the decals', the light probes', the
//! lightmaps' and the morph targets'. They also include those of every
//! material that no mesh wears, and those of the camera features that no
//! camera has. Each one still costs the executor a hand-over to a pool
//! thread and back.
//!
//! Each such system gets a gate, which is a run condition. The gate is open
//! while the main world has an entity with one of the components that the
//! system reads. It stays open for `TAIL` frames after the last one is
//! gone. The first run with nothing to copy clears what the system copied
//! before. Later runs change nothing, so skipping them is safe.
//!
//! A system that copies an asset type also runs while asset messages of
//! that type wait, so it misses no new or changed asset. The two systems
//! that pass on a material's meshes whose pipelines need making again run
//! only while the main world's lists of those meshes have entries. A run
//! with empty lists does nothing, so these two need no tail.
//!
//! Bevy UI's systems sit in Bevy's own sets, and the sets take the gate.
//! With no UI node, Bevy UI would still make a UI view for every camera
//! each frame, and draw nothing into it.
//!
//! Every `REFRESH` frames every gate opens for one frame. A system's change
//! detection compares ticks that wrap after a few billion changes, and a
//! system that never ran for hours could see its change ticks wrap.
//!
//! `KJK_EXTRACT_GATES=off` runs every system, as Bevy has it.

use std::any::type_name;

use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::asset::AssetEvent;
use bevy::core_pipeline::oit::OrderIndependentTransparencySettings;
use bevy::core_pipeline::prepass::background_motion_vectors::NoBackgroundMotionVectors;
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
use bevy::pbr::deferred::PbrDeferredLightingDepthId;
use bevy::pbr::{
    ContactShadows, DistanceFog, ExtendedMaterial, Lightmap, Material, MeshMaterial3d,
    ScreenSpaceReflections, ScreenSpaceTransmission,
};
use bevy::prelude::*;
use bevy::render::gpu_readback::Readback;
use bevy::render::occlusion_culling::OcclusionCulling;
use bevy::render::{ExtractSchedule, RenderApp};
use bevy::sprite_render::{
    ColorMaterial, Material2d, MeshMaterial2d, SpriteMaterial, TilemapChunkMaterial,
};
use bevy::ui_render::RenderUiSystems;

/// Frames a gate stays open after the last component it watches is gone.
/// The morph targets keep the weights of the frame before for one more
/// frame, so they need two runs with nothing to copy to come to rest.
const TAIL: u64 = 2;

/// Every this many frames, every gate opens for one frame.
const REFRESH: u64 = 3600;

/// The gate of Bevy UI's sets. It is the first gate made.
const UI: usize = 0;

/// Whether each gate is open this frame, in the render world.
#[derive(Resource, Default)]
struct ExtractGates(Vec<bool>);

/// What a gate watches in the main world.
type Watch = Box<dyn FnMut(&mut World) -> bool + Send>;

/// Whether the main world has what a gate's systems are made for. A gate
/// whose systems are missing although this holds is logged.
type Expected = fn(&World) -> bool;

struct Gate {
    watch: Watch,
    /// An earlier gate whose sighting also counts for this one.
    with: Option<usize>,
    tail: u64,
    expected: Expected,
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
        self.push(
            Gate {
                watch,
                with: None,
                tail: TAIL,
                expected: |_| true,
                last_seen: 0,
            },
            systems,
        )
    }

    fn push(&mut self, gate: Gate, systems: impl IntoIterator<Item = String>) -> usize {
        let index = self.gates.len();
        self.gates.push(gate);
        self.systems
            .extend(systems.into_iter().map(|name| (name, index)));
        index
    }

    /// Opens or closes every gate for this frame.
    fn update(&mut self, main: &mut World, open: &mut Vec<bool>) {
        self.frame += 1;
        let refresh = self.frame.is_multiple_of(REFRESH);
        open.resize(self.gates.len(), true);
        for (index, open) in open.iter_mut().enumerate() {
            let with = self.gates[index]
                .with
                .is_some_and(|with| self.gates[with].last_seen == self.frame);
            let gate = &mut self.gates[index];
            if with || (gate.watch)(main) {
                gate.last_seen = self.frame;
            }
            *open = refresh || self.frame - gate.last_seen <= gate.tail;
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

/// A system's name: its function's path.
fn system(path: &str) -> String {
    path.to_string()
}

/// A generic system's name, up to its first type parameter.
fn generic(path: &str, parameter: &str) -> String {
    format!("{path}<{parameter}")
}

/// The condition that runs a system while gate `gate` is open.
fn open(gate: usize) -> impl Fn(Res<ExtractGates>) -> bool + Clone {
    move |gates: Res<ExtractGates>| gates.0.get(gate).copied().unwrap_or(true)
}

fn material_3d<M: Material>(gates: &mut Gates) {
    let ty = type_name::<M>();
    let expected: Expected = |world| world.contains_resource::<Assets<M>>();
    let worn = gates.push(
        Gate {
            watch: any::<With<MeshMaterial3d<M>>>(),
            with: None,
            tail: TAIL,
            expected,
            last_seen: 0,
        },
        [
            generic("bevy_pbr::material::extract_mesh_materials", ty),
            generic("bevy_pbr::material::early_sweep_material_instances", ty),
        ],
    );
    gates.push(
        Gate {
            watch: messages::<M>(),
            with: Some(worn),
            tail: TAIL,
            expected,
            last_seen: 0,
        },
        [generic(
            "bevy_render::erased_render_asset::extract_erased_render_asset",
            &format!("bevy_pbr::mesh_material::MeshMaterial3d<{ty}>"),
        )],
    );
    gates.push(
        Gate {
            watch: Box::new(|world| {
                world
                    .get_resource::<bevy::pbr::EntitiesNeedingSpecialization<M>>()
                    .is_some_and(|entities| !entities.changed.is_empty())
            }),
            with: None,
            tail: 0,
            expected,
            last_seen: 0,
        },
        [generic(
            "bevy_pbr::material::extract_entities_needs_specialization",
            ty,
        )],
    );
    gates.push(
        Gate {
            watch: Box::new(|world| {
                world
                    .get_resource::<bevy::pbr::EntitiesNeedingSpecialization<M>>()
                    .is_some_and(|entities| !entities.removed.is_empty())
            }),
            with: None,
            tail: 0,
            expected,
            last_seen: 0,
        },
        [generic(
            "bevy_pbr::material::extract_entities_that_need_specializations_removed",
            ty,
        )],
    );
}

fn material_2d<M: Material2d>(gates: &mut Gates) {
    let ty = type_name::<M>();
    let worn = gates.add(
        any::<With<MeshMaterial2d<M>>>(),
        [generic(
            "bevy_sprite_render::mesh2d::material::extract_mesh_materials_2d",
            ty,
        )],
    );
    gates.push(
        Gate {
            watch: messages::<M>(),
            with: Some(worn),
            tail: TAIL,
            expected: |_| true,
            last_seen: 0,
        },
        [generic(
            "bevy_render::render_asset::extract_render_asset",
            &format!("bevy_sprite_render::mesh2d::material::PreparedMaterial2d<{ty}>"),
        )],
    );
    gates.push(
        Gate {
            watch: Box::new(|world| {
                world
                    .get_resource::<bevy::sprite_render::EntitiesNeedingSpecialization<M>>()
                    .is_some_and(|entities| !entities.changed.is_empty())
            }),
            with: None,
            tail: 0,
            expected: |_| true,
            last_seen: 0,
        },
        [generic(
            "bevy_sprite_render::mesh2d::material::extract_entities_needs_specialization",
            ty,
        )],
    );
    gates.push(
        Gate {
            watch: Box::new(|world| {
                world
                    .get_resource::<bevy::sprite_render::EntitiesNeedingSpecialization<M>>()
                    .is_some_and(|entities| !entities.removed.is_empty())
            }),
            with: None,
            tail: 0,
            expected: |_| true,
            last_seen: 0,
        },
        [generic(
            "bevy_sprite_render::mesh2d::material::extract_entities_that_need_specializations_removed",
            ty,
        )],
    );
}

/// The gate of the camera feature `C`, which Bevy copies to the render
/// world with its `extract_components` system.
fn camera_feature<C: Component>(gates: &mut Gates) {
    gates.add(
        any::<With<C>>(),
        [generic(
            "bevy_render::extract_component::extract_components",
            type_name::<C>(),
        )],
    );
}

fn gates() -> Gates {
    let mut gates = Gates::default();
    let ui = gates.add(any::<With<Node>>(), []);
    debug_assert_eq!(ui, UI);

    gates.add(
        either(any::<With<Sprite>>(), any::<With<Text2d>>()),
        [
            system("bevy_sprite_render::render::extract_sprites"),
            system("bevy_sprite_render::text2d::extract_text2d_sprite"),
        ],
    );
    gates.add(
        any::<With<Camera2d>>(),
        [
            system("bevy_core_pipeline::core_2d::extract_core_2d_camera_phases"),
            generic(
                "bevy_render::extract_component::extract_components",
                type_name::<Camera2d>(),
            ),
        ],
    );
    gates.add(
        any::<With<Mesh2d>>(),
        [system("bevy_sprite_render::mesh2d::mesh::extract_mesh2d")],
    );
    material_2d::<ColorMaterial>(&mut gates);
    material_2d::<SpriteMaterial>(&mut gates);
    material_2d::<TilemapChunkMaterial>(&mut gates);

    let atmosphere = gates.add(
        any::<With<Atmosphere>>(),
        [system("bevy_pbr::atmosphere::extract_atmosphere")],
    );
    gates.push(
        Gate {
            watch: messages::<ScatteringMedium>(),
            with: Some(atmosphere),
            tail: TAIL,
            expected: |_| true,
            last_seen: 0,
        },
        [generic(
            "bevy_render::render_asset::extract_render_asset",
            "bevy_pbr::medium::GpuScatteringMedium",
        )],
    );
    gates.add(
        any::<Or<(With<VolumetricFog>, With<FogVolume>, With<VolumetricLight>)>>(),
        [system(
            "bevy_pbr::volumetric_fog::render::extract_volumetric_fog",
        )],
    );
    gates.add(
        any::<With<ClusteredDecal>>(),
        [system(
            "bevy_pbr::decal::clustered::extract_clustered_decal",
        )],
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
        [system("bevy_pbr::decal::clustered::extract_decals")],
    );
    gates.add(
        any::<With<EnvironmentMapLight>>(),
        [
            generic(
                "bevy_pbr::light_probe::gather_light_probes",
                type_name::<EnvironmentMapLight>(),
            ),
            generic(
                "bevy_render::extract_instances::extract_all",
                "bevy_pbr::light_probe::environment_map::EnvironmentMapIds",
            ),
        ],
    );
    gates.add(
        any::<With<IrradianceVolume>>(),
        [generic(
            "bevy_pbr::light_probe::gather_light_probes",
            type_name::<IrradianceVolume>(),
        )],
    );
    gates.add(
        any::<With<GeneratedEnvironmentMapLight>>(),
        [system(
            "bevy_pbr::light_probe::generate::extract_generated_environment_map_entities",
        )],
    );
    gates.add(
        any::<With<Lightmap>>(),
        [system("bevy_pbr::lightmap::extract_lightmaps")],
    );
    gates.add(
        any::<With<MeshMorphWeights>>(),
        [system("bevy_pbr::render::morph::extract_morphs")],
    );

    camera_feature::<Fxaa>(&mut gates);
    camera_feature::<Smaa>(&mut gates);
    camera_feature::<ContrastAdaptiveSharpening>(&mut gates);
    camera_feature::<OrderIndependentTransparencySettings>(&mut gates);
    camera_feature::<NoBackgroundMotionVectors>(&mut gates);
    camera_feature::<ContactShadows>(&mut gates);
    camera_feature::<PbrDeferredLightingDepthId>(&mut gates);
    camera_feature::<DistanceFog>(&mut gates);
    camera_feature::<ScreenSpaceReflections>(&mut gates);
    camera_feature::<ScreenSpaceTransmission>(&mut gates);
    camera_feature::<Readback>(&mut gates);
    camera_feature::<OcclusionCulling>(&mut gates);

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
/// functions' paths. A gate that finds none of its systems is logged, so a
/// system that Bevy renames shows in the log instead of running ungated
/// unnoticed.
fn install(main: &World, render: &mut World, gates: &Gates) {
    let mut schedules = render.resource_mut::<Schedules>();
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
        let condition: BoxedCondition = Box::new(IntoSystem::into_system(open(gate)));
        if let Some(conditions) = systems.get_conditions_mut(key) {
            conditions.push(ConditionWithAccess::new(condition));
        }
    }
    for (index, gate) in gates.gates.iter().enumerate() {
        let names: Vec<&str> = gates
            .systems
            .iter()
            .filter(|(_, of)| *of == index)
            .map(|(name, _)| name.as_str())
            .collect();
        if !names.is_empty() && (gate.expected)(main) && !gated.iter().any(|(_, of)| *of == index) {
            warn!("extract gates: no extract system found for {names:?}");
        }
    }
    info!(
        "extract gates: {} of {} extract systems gated",
        gated.len(),
        systems.len()
    );
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
                .distributive_run_if(open(UI)),
        );
        let Some(mut extract) = render.take_extract() else {
            return;
        };
        let mut installed = false;
        render.set_extract(move |main, render| {
            if !installed {
                installed = true;
                install(main, render, &gates);
            }
            let mut open = std::mem::take(&mut render.resource_mut::<ExtractGates>().0);
            gates.update(main, &mut open);
            render.resource_mut::<ExtractGates>().0 = open;
            extract(main, render);
        });
    }
}
