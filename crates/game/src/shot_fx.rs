//! Where blaster shots end, and the disruptor's shot, from the presented
//! snapshots, drawn with KOTOR's effects (`context/artifacts/
//! kotor-blasters/SURVEY.md`); the fire, muzzle flashes and deflections
//! are `combat_fx`'s:
//!
//! - The disruptor, a hitscan, has nothing in flight: its shot shows as its
//!   shooter's weapon timer starting over in an attack. Its bolt is drawn
//!   as a tracer (`blaster_view`) from the gun's muzzle as drawn
//!   (`cast::Guns`; else where a rifle's bolt leaves the sim's gun) to where
//!   its aim line (a player's crosshair line, a creature's eye's) passes the
//!   body its shot struck that tick, else to where that line meets the
//!   level, which plays the wall impact.
//! - Ends: a bolt gone from the snapshots struck a body (its shooter's hit
//!   that tick by its weapon: the impact is `combat_fx`'s), was absorbed by
//!   a blade (a deflection that tick: `combat_fx` plays it), or, still in
//!   its life, met the level: the way from where it was last seen to where
//!   it would be now is traced with its box, and where that stops the wall
//!   impact plays.
//! - A wall impact: KOTOR's misses meet nothing, so this is the nearest the
//!   game has: `VFX_COM_BLASTER_IMPACT_GROUND` (`v_spksplat_imp`) on a
//!   floor, `VFX_COM_SPARKS_BLASTER` (`v_blaster_imp`) on the rest, and the
//!   bolt's impact sound for the surface (`WeaponSounds::bolt_wall`).

use std::collections::BTreeMap;

use bevy::prelude::*;
use collision::{Contents, Extents};
use sim::combat::sheet::Blaster;
use sim::combat::{Delivery, Missile, Outcome, mind, weapons};
use sim::motion::Anim;
use sim::{EntityState, Instances, Kind, Level, SimId, Snapshot};

use crate::blaster_view::{BlasterShots, Tracer};
use crate::cast;
use crate::combat_fx::FightDrawn;
use crate::script_fx::{HookVisual, HookVisuals, Place};
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::sound::{Cue, Cues};
use crate::weapon_sounds::WeaponSounds;

const UNIT: f32 = asset_ja::UNIT;
/// KOTOR's blaster impact on the ground (`VFX_COM_BLASTER_IMPACT_GROUND`)
/// and its blaster sparks (`VFX_COM_SPARKS_BLASTER`); a surface whose
/// normal rises steeper than `FLOOR_Z` is ground.
const GROUND: u16 = 4032;
const SPARKS: u16 = 4033;
const FLOOR_Z: f32 = 0.7;

pub struct ShotFxPlugin;

/// What the last snapshot showed: each bolt and when, each fighter's
/// weapon timer; and the beams and ends seen.
#[derive(Resource, Default)]
struct Shots {
    bolts: BTreeMap<SimId, (Missile, u64)>,
    timers: BTreeMap<SimId, i32>,
    generation: u32,
    beams: usize,
    walls: usize,
    absorbed: usize,
    /// The last wall impact (metres).
    last_wall: Option<[f32; 3]>,
    /// The own player's last wall impact (metres), and the camera as it
    /// was when that shot was fired, for how far the shot went off the
    /// screen's middle (`aim`); the own shots fired this frame.
    own_wall: Option<(Vec3, Option<GlobalTransform>)>,
    fired: bool,
    fired_camera: Option<GlobalTransform>,
}

impl Plugin for ShotFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shots>().add_systems(
            PostUpdate,
            shoot
                // Only in a session (the menu waits without one).
                .run_if(resource_exists::<crate::sim_host::PresentedSnapshot>)
                .after(bevy::transform::TransformSystems::Propagate)
                .before(FightDrawn),
        );
    }
}

/// What `shoot` asks of the other views, and the tables it reads.
type Asks<'w> = (
    Option<ResMut<'w, HookVisuals>>,
    ResMut<'w, Cues>,
    ResMut<'w, BlasterShots>,
    Option<Res<'w, WeaponSounds>>,
);

/// Where the impacts go: KOTOR's visuals, sounds, tracers; the tables.
struct Out<'a> {
    sparked: &'a mut Vec<HookVisual>,
    cues: &'a mut Vec<Cue>,
    tracers: &'a mut Vec<Tracer>,
    kotor: Option<&'a WeaponSounds>,
}

impl Out<'_> {
    /// A bolt of `base_item` met the level at `at` (units, its `normal`,
    /// the surface's `material`) at `ms`.
    fn wall(&mut self, base_item: u16, (at, normal, material): (Vec3, Vec3, u16), ms: f32) {
        let row = if normal.z > FLOOR_Z { GROUND } else { SPARKS };
        self.sparked.push(HookVisual {
            row,
            place: Place::At(at * UNIT),
        });
        if let Some(sound) = self.kotor.and_then(|k| k.bolt_wall(base_item, material)) {
            self.cues.push(Cue::at(sound, at, ms));
        }
    }
}

fn shoot(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    mut shots: ResMut<Shots>,
    (visuals, mut cues, mut blasters, kotor): Asks,
    (guns, places): (Res<cast::Guns>, Query<&GlobalTransform>),
    (seat, cameras): (
        Option<Res<crate::sim_host::OwnSeat>>,
        Query<&GlobalTransform, crate::lifecycle_view::SceneCamera>,
    ),
    mut facts: ResMut<console::DumpFacts>,
) {
    let own = seat.map_or(sim::Seat(0), |seat| seat.0);
    let shots = &mut *shots;
    if swap.generation != shots.generation {
        *shots = Shots {
            generation: swap.generation,
            ..Shots::default()
        };
    }
    let camera = cameras.iter().next().copied();
    let mut sparked = Vec::new();
    let out = &mut Out {
        sparked: &mut sparked,
        cues: &mut cues.0,
        tracers: &mut blasters.tracers,
        kotor: kotor.as_deref(),
    };
    for snapshot in &presented.1 {
        let me = cast::me(snapshot, own);
        shots.fired |= snapshot.entities.iter().any(|entity| {
            entity
                .missile
                .is_some_and(|missile| Some(missile.owner) == me)
                && !shots.bolts.contains_key(&entity.id)
        });
        bolts(shots, (snapshot, me), &swap.level, out);
        for entity in &snapshot.entities {
            // The gun its last shot left (`Shooting::hands`).
            let hand = entity
                .fighter
                .map_or(0, |fighter| u8::from(fighter.shooting.hands == 2));
            let gun = crate::combat_fx::gun_muzzle(&guns, &places, (entity.id, hand));
            let mine = Some(entity.id) == me;
            disruptor(shots, (entity, gun, mine), snapshot, &swap.level, out);
        }
    }
    if let Some(mut visuals) = visuals {
        visuals.0.append(&mut sparked);
    }
    if std::mem::take(&mut shots.fired) {
        shots.fired_camera = camera;
    }
    if let Some((_, seen @ None)) = &mut shots.own_wall {
        *seen = camera;
    }
    let aim = shots
        .own_wall
        .and_then(|(wall, camera)| Some(off_middle(&camera?, wall)));
    facts.0.insert(
        "shots".into(),
        serde_json::json!({
            "beams": shots.beams,
            "bolts": shots.bolts.len(),
            "wall_impacts": shots.walls,
            "absorbed": shots.absorbed,
            "last_wall_m": shots.last_wall,
            "aim": aim,
        }),
    );
}

/// How far the own last wall impact `wall` (game metres) is off the
/// screen's middle as `camera` looked when its shot was fired: how far
/// ahead, and up and right of the middle's line (centimetres).
fn off_middle(camera: &GlobalTransform, wall: Vec3) -> serde_json::Value {
    let to_game = crate::scene::game_to_bevy().inverse();
    let at = to_game * camera.translation();
    let (forward, up, right) = (
        to_game * camera.forward().as_vec3(),
        to_game * camera.up().as_vec3(),
        to_game * camera.right().as_vec3(),
    );
    let off = wall - at;
    serde_json::json!({
        "ahead_m": off.dot(forward),
        "up_cm": off.dot(up) * 100.0,
        "right_cm": off.dot(right) * 100.0,
    })
}

/// The bolts in `snapshot` kept; those gone ended (`me`: the own player).
fn bolts(
    shots: &mut Shots,
    (snapshot, me): (&Snapshot, Option<SimId>),
    level: &Level,
    out: &mut Out,
) {
    let mut seen = BTreeMap::new();
    for entity in &snapshot.entities {
        let Some(missile) = entity.missile.filter(|m| m.lob.is_none()) else {
            continue;
        };
        if matches!(missile.weapon.delivery, Delivery::Blaster(_)) {
            seen.insert(entity.id, (missile, snapshot.time_ms));
        }
    }
    let gone = std::mem::replace(&mut shots.bolts, seen);
    for (id, (missile, seen_ms)) in gone {
        if !shots.bolts.contains_key(&id) {
            let mine = Some(missile.owner) == me;
            ended(shots, (&missile, mine), seen_ms, snapshot, level, out);
        }
    }
}

/// How a bolt last seen at `seen_ms` and gone in `snapshot` ended.
fn ended(
    shots: &mut Shots,
    (missile, mine): (&Missile, bool),
    seen_ms: u64,
    snapshot: &Snapshot,
    level: &Level,
    out: &mut Out,
) {
    if !matches!(missile.weapon.delivery, Delivery::Blaster(_)) {
        return;
    }
    let now = snapshot.time_ms;
    if now > missile.until_ms {
        return;
    }
    let events = || {
        snapshot
            .entities
            .iter()
            .filter_map(|e| e.fighter)
            .flat_map(|f| f.events.into_iter().flatten())
            .filter(|e| e.by == Some(missile.owner) && e.tick == snapshot.tick)
    };
    if events().any(|e| e.weapon == Some(missile.weapon.delivery)) {
        return;
    }
    let way = Vec3::from_array(missile.velocity).normalize_or(Vec3::X);
    let from = Vec3::from_array(missile.at(seen_ms));
    let to = Vec3::from_array(missile.at(now));
    if events().any(|e| e.result == Outcome::Deflected) {
        shots.absorbed += 1;
        return;
    }
    let half = missile.half_size * UNIT;
    let extents = Extents {
        mins: [-half; 3],
        maxs: [half; 3],
    };
    let states = level.states_in(snapshot);
    let trace = level.with(states.as_deref(), |world| {
        world.trace(
            (from * UNIT).to_array(),
            (to * UNIT).to_array(),
            &extents,
            Contents::SOLID,
        )
    });
    if trace.fraction >= 1.0 && !trace.start_solid {
        return;
    }
    shots.walls += 1;
    shots.last_wall = Some(trace.end);
    if mine {
        shots.own_wall = Some((Vec3::from_array(trace.end), shots.fired_camera));
    }
    let normal = Vec3::from_array(trace.plane.normal).normalize_or(-way);
    let at = Vec3::from_array(trace.end) / UNIT;
    let met = (at, normal, trace.surface.material);
    out.wall(missile.weapon.base_item, met, now as f32);
}

/// A disruptor's shot in `snapshot`, if `entity` fired one: its tracer,
/// from its gun's muzzle as drawn (`gun`; else where a rifle's bolt leaves
/// the sim's gun), and its wall impact.
fn disruptor(
    shots: &mut Shots,
    (entity, gun, mine): (&EntityState, Option<Vec3>, bool),
    snapshot: &Snapshot,
    level: &Level,
    out: &mut Out,
) {
    let (Some(f), Some(p)) = (entity.fighter, entity.player) else {
        return;
    };
    let before = shots.timers.insert(entity.id, f.weapon_wait_ms);
    let weapon = cast::weapon(entity, &Instances::in_snapshot(level, snapshot));
    let shot = before.is_some_and(|before| f.weapon_wait_ms > before)
        && matches!(p.torso.anim, Anim::BOTH_ATTACK2 | Anim::BOTH_ATTACK3)
        && weapon.delivery == Delivery::Blaster(Blaster::Disruptor);
    if !shot {
        return;
    }
    shots.beams += 1;
    let ms = snapshot.time_ms as f32;
    // The line it aims along: a player's crosshair line (the sim shoots at
    // what it meets), a creature's view from its eye.
    let eye = Vec3::from_array(match entity.kind {
        Kind::Avatar(_) => mind::crosshair(&p),
        _ => mind::eye(&p),
    });
    let forward = Vec3::from_array(sim::math::angle_vectors(p.view).0);
    let muzzle = gun.unwrap_or_else(|| {
        Vec3::from_array(sim::muzzle(&p, weapons::primary(Blaster::Rifle).muzzle))
    });
    let struck = snapshot.entities.iter().find(|victim| {
        victim.fighter.is_some_and(|vf| {
            vf.events.iter().flatten().any(|event| {
                event.by == Some(entity.id)
                    && event.tick == snapshot.tick
                    && event.weapon == Some(Delivery::Blaster(Blaster::Disruptor))
            })
        })
    });
    let end = match struck.and_then(|victim| victim.player) {
        // Where the line passes the body struck, by its chest.
        Some(victim) => {
            let chest = Vec3::from_array(mind::chest(&victim));
            eye + forward * (chest - eye).dot(forward).max(0.0)
        }
        None => {
            let far = eye + forward * weapons::HITSCAN_RANGE;
            let states = level.states_in(snapshot);
            let trace = level.with(states.as_deref(), |world| {
                world.trace(
                    (eye * UNIT).to_array(),
                    (far * UNIT).to_array(),
                    &Extents::POINT,
                    Contents::SOLID,
                )
            });
            let end = Vec3::from_array(trace.end) / UNIT;
            if trace.fraction < 1.0 {
                shots.walls += 1;
                shots.last_wall = Some(trace.end);
                if mine {
                    // A beam meets the wall as it is fired: the camera's
                    // now (`shoot`).
                    shots.own_wall = Some((Vec3::from_array(trace.end), None));
                }
                let normal = Vec3::from_array(trace.plane.normal).normalize_or(-forward);
                out.wall(weapon.base_item, (end, normal, trace.surface.material), ms);
            }
            end
        }
    };
    let model = out
        .kotor
        .and_then(|kotor| kotor.ammo(weapon.base_item))
        .and_then(|ammo| ammo.models[0].clone());
    if let Some(model) = model {
        out.tracers.push(Tracer {
            model,
            from: muzzle * UNIT,
            to: end * UNIT,
            ms: f64::from(ms),
        });
    }
}
