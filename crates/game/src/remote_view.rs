//! The seats' remote droids drawn (`docs/superpowers/specs/2026-10-29-remote-droid-design.md`):
//!
//! - **Its model**: TSL's remote (Bao-Dur's, `p_remote.utc`), the model of
//!   the `appearance.2da` row labelled `Droid_SensorBall` in TSL's data (K2
//!   is required), found by label, not by number. It hovers where the sim
//!   has it, bobbing a little, turned to its seat's yaw; not drawn while it
//!   waits out a hold (`Remote::hidden`).
//! - **Its kit**: a press is seen as a button's ready time moving on
//!   (`Remote::ready_ms`): a flash at the droid and a sound (the shock
//!   bolt KOTOR's ion zap, the Sonic Blast the sonic grenade's burst, the
//!   stim dart `gui_inject`).
//!
//! Its camera is `play_view`'s (`cast::remote_at`); its buttons are the HUD's
//! (`hud`).

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_ja::fx::sprite_particle;
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use sim::Seat;
use sim::remote::Remote;

use crate::combat_fx::Added;
use crate::fx_model::{Spawned, Spawning};
use crate::heat_haze::HeatHazeMaterial;
use crate::scene::{ModelAssets, SceneContent, SceneRoot, Spawner};
use crate::script_fx::{Loaded, Read};
use crate::sim_host::{PresentedSnapshot, SceneSwap, view_ms};
use crate::sound::{Cue, Cues};

const UNIT: f32 = collision::UNIT;
/// The droid's box, middle to side (metres): the model's lowest point sits
/// this far under the droid's place.
const HALF_SIZE_M: f32 = rules::remote::flight::HALF_SIZE_M;
/// TSL's remote's `appearance.2da` label.
pub const APPEARANCE: &str = "Droid_SensorBall";
/// How far it bobs (metres) and how often (bobs a second).
const BOB_M: f32 = 0.05;
const BOBS: f32 = 0.6;
/// What each button sounds (KOTOR resrefs, read with `power_fx`'s).
const SOUNDS: [&str; 3] = ["cb_gr_ion", "cb_gr_sonic", "gui_inject"];
/// Each button's flash: its size (units) and colour.
const FLASHES: [(f32, [f32; 3]); 3] = [
    (8.0, [0.4, 0.7, 1.0]),
    (60.0, [0.6, 0.8, 1.0]),
    (6.0, [0.5, 1.0, 0.5]),
];
const GLOW: &str = "gfx/effects/sabers/yellow_glow";

pub struct RemoteViewPlugin;

impl Plugin for RemoteViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemoteView>().add_systems(
            PostUpdate,
            draw.run_if(resource_exists::<PresentedSnapshot>)
                .after(crate::play_view::Drawn)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// The droids drawn, their model, and each seat's kit as last seen.
#[derive(Resource, Default)]
pub struct RemoteView {
    generation: u32,
    model: Option<Loaded<Read>>,
    live: BTreeMap<Seat, Spawned>,
    ready: BTreeMap<Seat, [u64; 3]>,
}

/// TSL's remote and its textures, from TSL's own data.
fn read_remote() -> Read {
    let install = crate::installs::open(&crate::discover(), vfs::Game::Kotor2).ok()?;
    let resolver = install.base().ok()?;
    let table = resolver.two_da("appearance")?;
    let row = (0..table.cells.len())
        .find(|&row| table.text(row, "label").as_deref() == Some(APPEARANCE))?;
    let name = table.resref(row, "race")?;
    let model = crate::fx_model::load(resolver, &name, false)?;
    let textures = crate::fx_model::textures(resolver, std::iter::once(&model));
    Some((Arc::new(model), Arc::new(textures)))
}

/// Where `remote` is drawn at `ms` (metres): ahead along its way since its
/// snapshot, bobbing.
fn drawn_at(remote: &Remote, since_ms: f32, ms: f32) -> Vec3 {
    let ahead = (since_ms / 1000.0).clamp(0.0, 0.1);
    let at = Vec3::from_array(remote.at) + Vec3::from_array(remote.velocity) * ahead;
    let bob = BOB_M * (std::f32::consts::TAU * BOBS * ms / 1000.0 + f32::from(remote.seat.0)).sin();
    at + Vec3::Z * bob
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut view: ResMut<RemoteView>,
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    scene: Res<SceneContent>,
    roots: Query<Entity, With<SceneRoot>>,
    (mut assets, mut hazes): (ModelAssets, ResMut<Assets<HeatHazeMaterial>>),
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    seen: Query<&InheritedVisibility>,
    (mut added, mut cues): (ResMut<Added>, ResMut<Cues>),
    (mut facts, dump_due): (ResMut<console::DumpFacts>, Res<console::DumpDue>),
    mut commands: Commands,
) {
    let view = &mut *view;
    if swap.generation != view.generation {
        view.generation = swap.generation;
        // Their entities went with the old scene; the model stays read.
        view.live.clear();
        view.ready.clear();
    }
    let current = presented.0.current();
    let flying: BTreeMap<Seat, Remote> = current
        .remotes
        .iter()
        .map(|remote| (remote.seat, *remote))
        .collect();
    let gone: Vec<Seat> = (view.live.keys())
        .filter(|seat| !flying.contains_key(seat))
        .copied()
        .collect();
    for seat in gone {
        if let Some(spawned) = view.live.remove(&seat) {
            spawned.despawn(&mut commands);
        }
    }
    view.ready.retain(|seat, _| flying.contains_key(seat));
    let now = view_ms(&presented.0) as f32;
    // A press: a flash and a sound where the droid is.
    for (seat, remote) in &flying {
        let before = view.ready.insert(*seat, remote.ready_ms);
        let Some(before) = before else { continue };
        for kit in 0..3 {
            if remote.ready_ms[kit] <= before[kit] {
                continue;
            }
            let at = Vec3::from_array(remote.at) / UNIT;
            cues.0.push(Cue::at(format!("k1/{}", SOUNDS[kit]), at, now));
            let (size, colour) = FLASHES[kit];
            let colour = Vec3::from_array(colour);
            added.0.push(asset_ja::fx::Spawned {
                rgb_start: colour,
                rgb_end: colour,
                ..sprite_particle(
                    at,
                    (size, size * 1.5),
                    (1.0, 0.0),
                    now,
                    250.0,
                    GLOW,
                    asset_ja::fx::Flags::NONE,
                )
            });
        }
    }
    let model = view.model.get_or_insert_with(|| {
        Loaded::Loading(AsyncComputeTaskPool::get().spawn(async { read_remote() }))
    });
    let model = model.poll().cloned().flatten();
    if let (Some((model, textures)), Some(root)) = (&model, roots.iter().next()) {
        for seat in flying.keys() {
            if view.live.contains_key(seat) {
                continue;
            }
            let (images, stores) = assets.split();
            let mut spawning = Spawning {
                spawner: Spawner::new(textures, scene.0.area_light(), images),
                stores,
                hazes: &mut hazes,
                textures,
                area: scene.0.area_light(),
                lit: scene.0.lit,
                root,
                seed: u32::from(seat.0) | 1,
                commands: &mut commands,
            };
            view.live.insert(*seat, spawning.spawn(model.copy(), true));
        }
    }
    let since = now - current.time_ms as f32;
    let mut shown = Vec::new();
    for (seat, spawned) in &view.live {
        let Some(remote) = flying.get(seat) else {
            continue;
        };
        // The model stands on its root as a creature does (TSL's remote
        // floats a metre and more over it): its middle where the droid is.
        let at = drawn_at(remote, since, now) - Vec3::Z * (spawned.model.floor + HALF_SIZE_M);
        if let Ok(mut transform) = transforms.get_mut(spawned.root) {
            *transform = Transform::from_translation(at)
                .with_rotation(Quat::from_rotation_z(remote.yaw.to_radians()));
        }
        if let Ok(mut seen) = visibility.get_mut(spawned.root) {
            *seen = if remote.hidden {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
        shown.push(serde_json::json!({
            "seat": seat.0,
            "floor_m": spawned.model.floor,
            "visible": seen.get(spawned.root).ok().map(|seen| seen.get()),
            "at_m": at.to_array(),
            "hidden": remote.hidden,
            "ready_ms": remote.ready_ms,
        }));
    }
    if dump_due.0 {
        facts.0.insert(
            "remotes".into(),
            serde_json::json!({
                "model": model.as_ref().map(|(model, _)| model.placed.ir.name.clone()),
                "in_snapshot": flying.keys().map(|seat| seat.0).collect::<Vec<_>>(),
                "drawn": shown,
            }),
        );
    }
}
