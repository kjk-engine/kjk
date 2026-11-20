//! A grenade drawn from throw to burst (`docs/COMBAT.md`, "Grenades"):
//!
//! - **Its model**: the kind's `spells.2da` `projmodel` (`w_fraggren_001`,
//!   `w_thermdet_001`, …), read off the frame, spawned when it is thrown
//!   and gone when it bursts, where the sim has it at the moment drawn. It
//!   leaves the thrower's right hand as drawn and closes on its way over
//!   `LAUNCH_MS` (the arm comes over as the sim throws it, `acting::Throw`).
//!   In flight it tumbles about the level axis across its way, at most
//!   `SPIN` turns a second, rolling slower; at rest it lies as it fell, on
//!   the floor its box rests on.
//! - **Its fuse**: JA's thermal detonator's glow (`thermal_w_2`: its glow
//!   stage `rgbGen wave sin 0.6 0.4 0 3`, three pulses a second, yellow) as
//!   a sprite on it, pulsing three times as fast over the last half second,
//!   from JA's warning on (the quickening is ours).
//!
//! What it sounds is `power_fx`'s.

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_ja::fx::sprite_particle;
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use bevy::transform::helper::TransformHelper;
use sim::combat::{Location, Missile, grenades};
use sim::{SimId, Snapshot};

use crate::cast::Hitboxes;
use crate::combat_fx::Added;
use crate::fx_model::{Spawned, Spawning};
use crate::heat_haze::HeatHazeMaterial;
use crate::lifecycle::Installer;
use crate::lifecycle_view::SceneCamera;
use crate::scene::{ModelAssets, SceneContent, SceneRoot, Spawner, game_to_bevy};
use crate::script_fx::{Loaded, Read, ask_for, read};
use crate::sim_host::{PresentedSnapshot, SceneSwap, view_ms};

const UNIT: f32 = collision::UNIT;
/// How long a grenade takes from the hand to its own way (ms).
const LAUNCH_MS: f32 = 120.0;
/// The most a grenade turns in flight (turns a second), and its rolling
/// radius (metres: the models' half width).
const SPIN: f32 = 2.0;
const RADIUS_M: f32 = 0.05;
/// JA's thermal glow: its pulses a second, and over the last half second
/// (`WARNING_MS` before the burst, as JA's `warning` sounds) three times
/// as many; its brightness `BASE ± SWING`.
const PULSES: f32 = 3.0;
const WARNING_PULSES: f32 = 9.0;
pub const WARNING_MS: u64 = 500;
const BASE: f32 = 0.6;
const SWING: f32 = 0.4;
/// The glow sprite: JA's yellow glow, this wide (units) and drawn at
/// these colours (yellow, as JA's thermal glow texture).
const GLOW: &str = "gfx/effects/sabers/yellow_glow";
const GLOW_SIZE: f32 = 10.0;
/// The shaders this draws with that no effect file names.
pub const SHADERS: &[&str] = &[GLOW];

pub struct GrenadeViewPlugin;

impl Plugin for GrenadeViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            draw.run_if(resource_exists::<GrenadeView>)
                .run_if(resource_exists::<PresentedSnapshot>)
                .after(crate::play_view::Drawn)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// A grenade drawn: its model, who threw it and when, its turn, and the
/// frames it has stood (its entities stand from the second).
struct Live {
    spawned: Spawned,
    owner: SimId,
    thrown: u64,
    turn: Quat,
    drawn_ms: f32,
    frames: u32,
}

/// The view's state, put in with a session (`install`).
#[derive(Resource)]
pub struct GrenadeView {
    installer: Option<Arc<Installer>>,
    /// The scene swap its models belong to.
    generation: u32,
    /// `spells.2da` `projmodel` by grenade row.
    table: Option<Loaded<BTreeMap<u16, String>>>,
    models: BTreeMap<String, Loaded<Read>>,
    live: BTreeMap<SimId, Live>,
    /// Each readied grenade in its holder's right hand, by holder: its model
    /// and its row.
    held: BTreeMap<SimId, (Spawned, u16)>,
    /// The grenades drawn in this scene: by whom, when thrown (`dump`).
    drawn: Vec<(SimId, u64)>,
}

/// A session's grenades, their models read from `installer`.
pub fn install(world: &mut World, installer: Option<Arc<Installer>>) {
    world.insert_resource(GrenadeView {
        installer,
        generation: 0,
        table: None,
        models: BTreeMap::new(),
        live: BTreeMap::new(),
        held: BTreeMap::new(),
        drawn: Vec::new(),
    });
}

/// The grenade rows' `projmodel`, read for `module`.
fn table(installer: &Installer, module: &str) -> BTreeMap<u16, String> {
    let Some(spells) = installer
        .resolver(module)
        .ok()
        .and_then(|resolver| resolver.two_da("spells"))
    else {
        return BTreeMap::new();
    };
    (0..spells.cells.len())
        .filter_map(|row| {
            let row16 = u16::try_from(row).ok()?;
            grenades::is_grenade(row16).then_some(())?;
            let model = spells.resref(row, "projmodel")?;
            Some((row16, model))
        })
        .collect()
}

/// The grenades in `snapshot`, by id.
fn lobs(snapshot: &Snapshot) -> BTreeMap<SimId, Missile> {
    snapshot
        .entities
        .iter()
        .filter_map(|e| Some((e.id, e.missile.filter(|m| m.lob.is_some())?)))
        .collect()
}

/// How bright the glow is `since` ms after the throw (0 to 1).
fn glow(since: u64) -> f32 {
    let warning = grenades::FUSE_MS - WARNING_MS;
    let seconds = |ms: u64| ms as f32 / 1000.0;
    let pulses = if since < warning {
        PULSES * seconds(since)
    } else {
        PULSES * seconds(warning) + WARNING_PULSES * seconds(since - warning)
    };
    BASE + SWING * (std::f32::consts::TAU * pulses).sin()
}

/// The fuse's glow (`GLOW`) at `centre` (game units), `bright` (0 to 1),
/// drawn a little toward the `eye`, so the floor a grenade lies on and the
/// hand that holds one do not cut it.
fn glow_sprite(centre: Vec3, eye: Option<Vec3>, bright: f32, now: f32) -> asset_ja::fx::Spawned {
    let toward = eye.map_or(Vec3::ZERO, |eye| (eye - centre).normalize_or_zero());
    asset_ja::fx::Spawned {
        rgb_start: Vec3::splat(bright),
        rgb_end: Vec3::splat(bright),
        ..sprite_particle(
            centre + toward * GLOW_SIZE * 0.5,
            (GLOW_SIZE, GLOW_SIZE),
            (1.0, 1.0),
            now,
            0.0,
            GLOW,
            asset_ja::fx::Flags::NONE,
        )
    }
}

/// A grenade's model `read` spawned hidden under the scene's `root`, lit as
/// the `scene`'s area is; none while the model could not be read.
fn spawn_model(
    read: &Read,
    (assets, hazes): (&mut ModelAssets, &mut Assets<HeatHazeMaterial>),
    (scene, root, seed): (&SceneContent, Entity, u32),
    commands: &mut Commands,
) -> Option<Spawned> {
    let (model, textures) = read.as_ref()?;
    let (images, stores) = assets.split();
    let mut spawning = Spawning {
        spawner: Spawner::sharing(textures, scene.0.area_light(), images, &scene.0.images),
        stores,
        hazes,
        textures,
        area: scene.0.area_light(),
        lit: scene.0.lit,
        root,
        seed,
        commands,
    };
    Some(spawning.spawn(model.copy(), true))
}

/// The grenade each character has readied (`Pack::readied`) in `snapshot`:
/// its `spells.2da` row (the kind the grenade slot uses now,
/// `Pack::keyed`), and since when (sim ms) its throw is charged, if it is.
fn readied(snapshot: &Snapshot, level: &sim::Level) -> BTreeMap<SimId, (u16, Option<u64>)> {
    use sim::combat::items::{Readied, Shelf};
    snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let fighter = entity.fighter.as_ref()?;
            if !fighter.alive() || fighter.pack.readied != Readied::On {
                return None;
            }
            let sheet = crate::cast::worn_sheet(snapshot, entity, level);
            let (_, kind) = fighter
                .pack
                .keyed(&sheet, Shelf::Grenade, snapshot.time_ms)?;
            Some((entity.id, (kind.row, fighter.pack.charging_from)))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut view: ResMut<GrenadeView>,
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    scene: Res<SceneContent>,
    hitboxes: Option<Res<Hitboxes>>,
    roots: Query<Entity, With<SceneRoot>>,
    (mut assets, mut hazes): (ModelAssets, ResMut<Assets<HeatHazeMaterial>>),
    mut posing: ParamSet<(TransformHelper, Query<&mut Transform>)>,
    cameras: Query<&GlobalTransform, SceneCamera>,
    mut visibility: Query<&mut Visibility>,
    (mut added, mut facts, dump_due): (
        ResMut<Added>,
        ResMut<console::DumpFacts>,
        Res<console::DumpDue>,
    ),
    mut commands: Commands,
) {
    let view = &mut *view;
    if swap.generation != view.generation {
        view.generation = swap.generation;
        view.table = None;
        view.models.clear();
        // Their entities went with the old scene.
        view.live.clear();
        view.held.clear();
        view.drawn.clear();
    }
    let Some(installer) = view.installer.clone() else {
        return;
    };
    let (previous, current) = (presented.0.previous(), presented.0.current());
    let flying = lobs(current);
    let gone: Vec<SimId> = view
        .live
        .keys()
        .filter(|id| !flying.contains_key(id))
        .copied()
        .collect();
    for id in gone {
        if let Some(live) = view.live.remove(&id) {
            live.spawned.despawn(&mut commands);
        }
    }
    let module = swap.level.resources().to_string();
    let table = view.table.get_or_insert_with(|| {
        let (installer, module) = (installer.clone(), module.clone());
        Loaded::Loading(
            AsyncComputeTaskPool::get().spawn(async move { table(&installer, &module) }),
        )
    });
    let Some(table) = table.poll() else {
        return;
    };
    let model_of = |missile: &Missile| table.get(&missile.lob?.row);
    // Every kind's model read with the scene, ready for the first throw.
    if view.models.len() < table.len() {
        let names = table.values().cloned().collect();
        ask_for(&mut view.models, names, (&installer, &module), read);
    }
    for model in view.models.values_mut() {
        model.poll();
    }
    let Some(root) = roots.iter().next() else {
        return;
    };
    for (&id, missile) in &flying {
        if view.live.contains_key(&id) {
            continue;
        }
        let Some(Loaded::Ready(read)) = model_of(missile).and_then(|name| view.models.get(name))
        else {
            continue;
        };
        let place = (&*scene, root, id.0 | 1);
        let Some(spawned) = spawn_model(read, (&mut assets, &mut hazes), place, &mut commands)
        else {
            continue;
        };
        // A held scene's lob bursts as it lands, short of the fuse.
        let thrown = if missile.scripted {
            missile.from_ms
        } else {
            missile.until_ms.saturating_sub(grenades::FUSE_MS)
        };
        view.drawn.push((missile.owner, thrown));
        view.live.insert(
            id,
            Live {
                spawned,
                owner: missile.owner,
                thrown,
                turn: Quat::IDENTITY,
                drawn_ms: thrown as f32,
                frames: 0,
            },
        );
    }
    // Each readied grenade in its holder's right hand, as the kind its
    // grenade slot uses now; another kind picked takes its place.
    let readied = readied(current, &swap.level);
    let dropped: Vec<SimId> = (view.held.iter())
        .filter(|(id, (_, row))| readied.get(id).is_none_or(|(now, _)| now != row))
        .map(|(id, _)| *id)
        .collect();
    for id in dropped {
        if let Some((spawned, _)) = view.held.remove(&id) {
            spawned.despawn(&mut commands);
        }
    }
    for (&id, &(row, _)) in &readied {
        if view.held.contains_key(&id) {
            continue;
        }
        let Some(Loaded::Ready(read)) = table.get(&row).and_then(|name| view.models.get(name))
        else {
            continue;
        };
        let place = (&*scene, root, id.0 | 1);
        if let Some(spawned) = spawn_model(read, (&mut assets, &mut hazes), place, &mut commands) {
            view.held.insert(id, (spawned, row));
        }
    }
    let now = view_ms(&presented.0) as f32;
    // Where each holder's right hand is drawn and how it turns (game
    // metres), for the grenades readied.
    let holding: BTreeMap<SimId, (Vec3, Quat)> = {
        let helper = posing.p0();
        view.held
            .keys()
            .filter_map(|owner| {
                let spots = hitboxes.as_ref()?.0.get(owner)?;
                let hand = spots[Location::HandRight as usize]?;
                let global = helper.compute_global_transform(hand).ok()?;
                let to_game = game_to_bevy().inverse();
                Some((
                    *owner,
                    (to_game * global.translation(), to_game * global.rotation()),
                ))
            })
            .collect()
    };
    // Where each thrower's right hand is drawn (game metres).
    let hands: BTreeMap<SimId, Vec3> = {
        let helper = posing.p0();
        view.live
            .values()
            .filter(|live| now - (live.thrown as f32) < LAUNCH_MS)
            .filter_map(|live| {
                let spots = hitboxes.as_ref()?.0.get(&live.owner)?;
                let hand = spots[Location::HandRight as usize]?;
                let global = helper.compute_global_transform(hand).ok()?;
                Some((live.owner, game_to_bevy().inverse() * global.translation()))
            })
            .collect()
    };
    let earlier = lobs(previous);
    // Where the camera is (game units).
    let eye = cameras
        .iter()
        .next()
        .map(|camera| game_to_bevy().inverse() * camera.translation() / UNIT);
    let mut transforms = posing.p1();
    // A readied grenade is unlit in the hand, and glows yellow from the
    // moment its throw is charged, as it glows in flight.
    for (owner, (spawned, _)) in &view.held {
        let Some(&(at, turn)) = holding.get(owner) else {
            continue;
        };
        if let Ok(mut transform) = transforms.get_mut(spawned.root) {
            *transform = Transform::from_translation(at).with_rotation(turn);
        }
        if let Ok(mut seen) = visibility.get_mut(spawned.root) {
            *seen = Visibility::Inherited;
        }
        if let Some(Some(from)) = readied.get(owner).map(|(_, charging)| *charging) {
            let bright = glow((now.max(0.0) as u64).saturating_sub(from));
            added.0.push(glow_sprite(at / UNIT, eye, bright, now));
        }
    }
    let mut shown = Vec::new();
    for (id, live) in &mut view.live {
        let Some(missile) = flying.get(id) else {
            continue;
        };
        // Drawn before its last bounce: the way it had.
        let missile = match earlier.get(id) {
            Some(before) if now < missile.from_ms as f32 => before,
            _ => missile,
        };
        let ms = now.max(0.0) as u64;
        let resting = missile.lob.is_some_and(|lob| lob.resting);
        let mut at = Vec3::from_array(missile.at(ms)) * UNIT;
        if resting {
            at.z -= grenades::HALF_SIZE * UNIT + live.spawned.model.floor;
        }
        let since = now - live.thrown as f32;
        if let Some(hand) = hands.get(&live.owner) {
            let f = (since / LAUNCH_MS).clamp(0.0, 1.0);
            at = hand.lerp(at, f * f * (3.0 - 2.0 * f));
        }
        // A wrist launcher's shot (`grenades::homing`) flies nose first (a
        // KOTOR model's +Y) and has no fuse to glow; a thrown grenade
        // tumbles, rolled along its way as fast as it goes, at most `SPIN`.
        let homing = missile.lob.is_some_and(|lob| grenades::homing(lob.row));
        let dt = ((now - live.drawn_ms) / 1000.0).clamp(0.0, 0.1);
        live.drawn_ms = now;
        let velocity = Vec3::from_array(missile.velocity_at(ms));
        let way = Vec3::new(velocity.x, velocity.y, 0.0) * UNIT;
        if homing && let Some(nose) = velocity.try_normalize() {
            live.turn = Quat::from_rotation_arc(Vec3::Y, nose);
        } else if !resting && let Some(axis) = Vec3::Z.cross(way).try_normalize() {
            let rate = (way.length() / RADIUS_M).min(SPIN * std::f32::consts::TAU);
            live.turn = (Quat::from_axis_angle(axis, rate * dt) * live.turn).normalize();
        }
        if let Ok(mut transform) = transforms.get_mut(live.spawned.root) {
            *transform = Transform::from_translation(at).with_rotation(live.turn);
        }
        live.frames += 1;
        if let Ok(mut seen) = visibility.get_mut(live.spawned.root) {
            *seen = if live.frames > 1 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        let bright = glow(since.max(0.0) as u64);
        if !homing {
            added.0.push(glow_sprite(at / UNIT, eye, bright, now));
        }
        if !dump_due.0 {
            continue;
        }
        shown.push(serde_json::json!({
            "id": id.0,
            "model": live.spawned.model.placed.ir.name,
            "at_m": at.to_array(),
            "resting": resting,
            "glow": bright,
            "from_hand": hands.contains_key(&live.owner),
        }));
    }
    if dump_due.0 {
        facts.0.insert(
            "grenades".into(),
            serde_json::json!({
                "models": view.models.keys().collect::<Vec<_>>(),
                "live": shown,
                "drawn": view.drawn.iter().map(|(by, at)| (by.0, at)).collect::<Vec<_>>(),
                "readied": readied.iter().map(|(by, (row, charging))| (by.0, row, charging)).collect::<Vec<_>>(),
                "held": view.held.iter().map(|(by, (_, row))| (by.0, row, holding.get(by).map(|(at, _)| at.to_array()))).collect::<Vec<_>>(),
            }),
        );
    }
}
