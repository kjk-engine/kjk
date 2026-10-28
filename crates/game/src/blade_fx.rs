//! The sabers' sounds, and what blades meeting blades and walls show, from
//! the presented snapshots, as JA's client makes them (`docs/SOUND.md`):
//!
//! - Lighting and going out: a character whose blades are lit in a snapshot
//!   and were not in the one before sounds its saber's `soundOn`, going
//!   with it; one put out (alive) sounds its `soundOff` where it was.
//! - The hum: every living character with a lit saber in its hand holds its
//!   saber's loop (`soundLoop`) where it stands, the camera's player at the
//!   camera, every frame (`sound::Loops`).
//! - Swings: a new attack (an attacking move other than the one before, not
//!   a kick) sounds one of `saberhup1–8` in the weapon slot, where it
//!   starts; any other weapon's, its KOTOR swing (`weapon_sounds`). The
//!   whooshes keyed to animation frames are `play_view`'s (`keyframed`).
//! - Blocks: a blow turned by a blade (a `Parried` event; a clash records
//!   one on each side, played once) sounds one of `saberblock1–9` (a blow
//!   not of a lightsaber: its KOTOR parry) and shows the clash flare where
//!   it happened (the event's point; else where the two blades pass
//!   closest). Its block effect is `combat_fx`'s.
//! - Walls: every lit blade as drawn (`scene::DrawnBlades`, whose blades
//!   `Wielders` tells) traced from hilt to tip against the level each
//!   client frame, as the cgame traces the blade it draws; where it meets
//!   a wall, sparks (`sparks/spark_nosnd`); meeting it two client frames
//!   running, one of `saberhitwall1–3`, at most every 100 ms a blade, and
//!   where it met the level itself a burn and a glow along the way it
//!   dragged (`saber_marks`). A client frame is a sixtieth of a second of
//!   the view's time (QUIRKS.md Q-0005): a drawn frame that ends none does
//!   nothing here, one that ends several does it once for all of them.
//! - Rolls: a character starting a roll (move_body's `ROLL`) sounds KOTOR's
//!   body whoosh in its body slot (`weapon_sounds`).

use std::collections::BTreeMap;

use asset_ja::events::{Channel as KeyChannel, Keyed, Play};
use asset_ja::fx::Rng;
use audio::Channel;
use bevy::prelude::*;
use collision::{Contents, Extents};
use sim::combat::Outcome;
use sim::combat::saber::{Kit, Move, blades};
use sim::{EntityState, Instances, Level, Seat, SimId, Snapshot, motion};

use crate::cast;
use crate::combat_fx::{Added, Burst, Bursts, FightDrawn};
use crate::lifecycle_view::SceneCamera;
use crate::saber_marks::{Cut, Cuts};
use crate::scene::DrawnBlades;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::sound::{Cue, Cues, Hold, Loops};
use crate::viewer_ja::Files;
use crate::weapon_sounds::{Swing, WeaponSounds};

const UNIT: f32 = asset_ja::UNIT;
/// JA's client frames.
const FRAME_MS: f32 = 1000.0 / 60.0;
/// A blade whose data gives no length (units).
const BLADE_LENGTH: f32 = 40.0;
/// The shortest time between one blade's wall sounds (ms).
const WALL_SOUND_MS: f32 = 100.0;
/// How long a clash's mirror event is looked for (ticks).
const MIRROR_TICKS: u64 = 1;

const FLARE: &str = "gfx/effects/saberFlare";
/// The shaders this draws with that no effect file names.
pub const SHADERS: &[&str] = &[FLARE];
const WALL_EFFECT: &str = "sparks/spark_nosnd";
/// JA's fists' and kicks' keyframed whooshes.
const BODY_WHOOSH: &str = "sound/weapons/melee/swing";
/// The effects this asks `combat_fx` for.
pub const EFFECTS: &[&str] = &[WALL_EFFECT];

/// A saber's sounds: lighting, the hum, going out.
#[derive(Clone)]
pub struct SaberSounds([String; 3]);

/// The two sabers' sounds (JA's `single_1` and `dual_1`).
#[derive(Clone)]
pub struct Setup {
    single: SaberSounds,
    staff: SaberSounds,
}

/// Each saber's sounds from its file (JA's defaults where it names none).
pub fn load(files: &Files, notes: &mut Vec<String>) -> Setup {
    let [single, staff] = blades::SABERS.map(|name| {
        let saber = files
            .read(&format!("ext_data/sabers/{name}.sab"))
            .and_then(|bytes| asset_ja::saber(&bytes, name));
        if saber.is_none() {
            notes.push(format!(
                "{name}.sab: not read; its sounds are JA's defaults"
            ));
        }
        SaberSounds(saber.map_or(asset_ja::SABER_SOUNDS.map(String::from), |s| s.sounds))
    });
    Setup { single, staff }
}

fn numbered(stem: &str, n: i32) -> String {
    format!("sound/weapons/{stem}{n}.wav")
}

impl Setup {
    /// Every sound this plays.
    pub fn sounds(&self) -> Vec<String> {
        let mut names: Vec<String> = [&self.single, &self.staff]
            .into_iter()
            .flat_map(|saber| saber.0.iter().cloned())
            .collect();
        let ranges = [
            ("saber/saberhup", 1..=8),
            ("saber/saberblock", 1..=9),
            ("saber/saberhitwall", 1..=3),
        ];
        for (stem, range) in ranges {
            names.extend(range.map(|n| numbered(stem, n)));
        }
        names
    }

    fn of(&self, double: bool) -> &SaberSounds {
        if double { &self.staff } else { &self.single }
    }
}

/// Its systems; a session comes in with `install`.
pub struct BladeFxPlugin;

/// A session's blade effects, put into `app`.
pub fn install(app: &mut World, setup: Setup) {
    app.insert_resource(Blades {
        setup,
        seen: BTreeMap::new(),
        walls: BTreeMap::new(),
        blocks: Vec::new(),
        flash: None,
        frame: i64::MIN,
        generation: 0,
    });
}

/// What each character's blades were at the last snapshot seen.
#[derive(Clone, Copy)]
struct Seen {
    lit: bool,
    saber_move: Move,
    next_event: u16,
}

/// How a blade last met a wall: where in the last client frame (units),
/// and when it last sounded one (the view's ms).
#[derive(Clone, Copy)]
struct Wall {
    met: Option<Vec3>,
    sounded: f32,
}

impl Wall {
    /// A blade that has not met a wall yet.
    const FRESH: Wall = Wall {
        met: None,
        sounded: f32::MIN,
    };
}

/// The lit blades in hand the view drew this frame: whose (character and
/// slot), and their index in `scene::DrawnBlades`.
#[derive(Resource, Default)]
pub struct Wielders(pub Vec<(SimId, usize, usize)>);

/// Blades meet walls at most this many client frames at once (a drawn
/// frame over a long stall).
const MOST_FRAMES: u32 = 8;

#[derive(Resource)]
struct Blades {
    setup: Setup,
    seen: BTreeMap<SimId, Seen>,
    /// Per blade (character, slot): how it last met a wall.
    walls: BTreeMap<(SimId, usize), Wall>,
    /// The blocks played lately: the pair (lower id first) and the tick.
    blocks: Vec<(SimId, SimId, u64)>,
    /// The last block's clash flare: where, from when (the view's ms).
    flash: Option<(Vec3, f32)>,
    /// The last client frame the walls were met in (`FRAME_MS`s of the
    /// view's time).
    frame: i64,
    generation: u32,
}

impl Plugin for BladeFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wielders>().add_systems(
            PostUpdate,
            sound
                .run_if(resource_exists::<Blades>)
                .after(crate::scene::BladesDrawn)
                .before(FightDrawn),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn sound(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    seat: Option<Res<OwnSeat>>,
    mut blades: ResMut<Blades>,
    (mut cues, mut loops, mut bursts, mut added): (
        ResMut<Cues>,
        ResMut<Loops>,
        ResMut<Bursts>,
        ResMut<Added>,
    ),
    (mut cuts, wielders, drawn): (ResMut<Cuts>, Res<Wielders>, Res<DrawnBlades>),
    cameras: Query<(&GlobalTransform, &Projection), SceneCamera>,
    kotor: Option<Res<WeaponSounds>>,
) {
    let kotor = kotor.as_deref();
    let blades = &mut *blades;
    if swap.generation != blades.generation {
        blades.generation = swap.generation;
        blades.seen.clear();
        blades.walls.clear();
        blades.blocks.clear();
        blades.frame = i64::MIN;
    }
    let level = &swap.level;
    let current = presented.0.current();
    let now = crate::sim_host::view_ms(&presented.0) as f32;
    for snapshot in &presented.1 {
        let snapshot: &Snapshot = snapshot;
        let out = &mut (&mut cues.0, &mut bursts.0);
        let objects = Instances::in_snapshot(level, snapshot);
        let at = (snapshot, &objects, kotor);
        for entity in &snapshot.entities {
            changes(blades, entity, at, level, out);
        }
        blades
            .blocks
            .retain(|&(.., tick)| tick + MIRROR_TICKS >= snapshot.tick);
    }
    let frame = (now / FRAME_MS).floor() as i64;
    // The first frame of a scene ends one client frame.
    let last = if blades.frame == i64::MIN {
        frame - 1
    } else {
        blades.frame
    };
    let frames = (frame - last).clamp(0, i64::from(MOST_FRAMES)) as u32;
    blades.frame = frame;
    if frames > 0 {
        let objects = Instances::in_snapshot(level, current);
        let out = (&mut cues.0, &mut bursts.0, &mut cuts.0);
        let met = (current, &objects, kotor);
        walls(
            blades,
            (&wielders.0, &drawn),
            met,
            level,
            (now, frames),
            out,
        );
    }
    let me = cast::me(current, seat.map_or(Seat(0), |seat| seat.0));
    let eye = cameras
        .iter()
        .next()
        .map(|(camera, _)| crate::scene_fx::eye_of(camera).eye);
    if let (Some((at, from)), Some((camera, projection))) = (blades.flash, cameras.iter().next()) {
        added.0.extend(flare(
            (at, now - from, now),
            camera,
            projection,
            level,
            presented.0.current(),
        ));
    }
    let objects = Instances::in_snapshot(level, current);
    for entity in &current.entities {
        let Some(saber) = held_saber(entity, &objects) else {
            continue;
        };
        let f = entity.fighter.expect("held_saber checks");
        // Put away while its user works a lock or a mine (`weapons::armed`).
        let hidden = f
            .acting
            .is_some_and(|acting| crate::acting::hides_items(&acting, current.time_ms as f64));
        if !f.saber.on || !f.alive() || f.saber.thrown.is_some() || hidden {
            continue;
        }
        let at = match eye {
            Some(eye) if me == Some(entity.id) => eye,
            _ => Vec3::from_array(entity.position) / UNIT,
        };
        loops
            .0
            .push(Hold::at(blades.setup.of(saber).0[1].clone(), at));
    }
}

/// The flare of a clash at `at` (game space, units) `since` ms ago, drawn
/// at `now` (ms): for 150 ms a sprite at the clash as large on the screen
/// as JA's screen-space flare, fading with its time and shrinking with its
/// distance. None behind the view, with the level in between, past 1200
/// units, or under a camera without a perspective.
fn flare(
    (at, since, now): (Vec3, f32, f32),
    camera: &GlobalTransform,
    projection: &Projection,
    level: &Level,
    snapshot: &Snapshot,
) -> Option<asset_ja::fx::Spawned> {
    if since <= 0.0 || since >= 150.0 {
        return None;
    }
    let to_game = crate::scene::game_to_bevy().inverse();
    let eye = crate::scene_fx::eye_of(camera).eye;
    let forward = to_game * camera.forward().as_vec3();
    let away = at - eye;
    if away.dot(forward) < 0.2 {
        return None;
    }
    let doors = level.states_in(snapshot);
    let hit = level.with(doors.as_deref(), |world| {
        world.trace(
            (eye * UNIT).to_array(),
            (at * UNIT).to_array(),
            &Extents::POINT,
            Contents::SOLID,
        )
    });
    let distance = away.length();
    if hit.fraction < 1.0 || distance > 1200.0 {
        return None;
    }
    let v = ((1.0 - since / 150.0) * ((1.0 - distance / 800.0) * 2.0 + 0.35)).max(0.001);
    let Projection::Perspective(perspective) = projection else {
        return None;
    };
    // Half the horizontal field of view.
    let wide = ((perspective.fov / 2.0).tan() * perspective.aspect_ratio).atan();
    let radius = v * 300.0 / 320.0 * distance * wide.tan();
    let sprite = asset_ja::fx::sprite_particle(
        at,
        (radius, radius),
        (1.0, 1.0),
        now,
        0.0,
        FLARE,
        asset_ja::fx::Flags::NONE,
    );
    Some(asset_ja::fx::Spawned {
        rgb_start: Vec3::splat(0.8),
        rgb_end: Vec3::splat(0.8),
        ..sprite
    })
}

/// Whether `entity` holds a lightsaber in its hand out, and a staff.
fn held_saber(entity: &EntityState, objects: &Instances) -> Option<bool> {
    let f = entity.fighter?;
    let blade = Kit::of(&cast::sheet(entity, objects)).blade(f.hand)?;
    blade.saber.then_some(blade.double)
}

type Out<'a> = (&'a mut Vec<Cue>, &'a mut Vec<Burst>);
/// A snapshot, its instances, and KOTOR's weapon sounds.
type Met<'a> = (&'a Snapshot, &'a Instances, Option<&'a WeaponSounds>);

/// What changed for one character since its last snapshot: lit or out, a
/// new attack, blocks.
fn changes(
    blades: &mut Blades,
    entity: &EntityState,
    (snapshot, objects, kotor): Met,
    level: &Level,
    (cues, _): &mut Out,
) {
    let Some(f) = entity.fighter else { return };
    let now = Seen {
        lit: f.saber.on,
        saber_move: f.saber.current_move,
        next_event: f.next_event,
    };
    let Some(before) = blades.seen.insert(entity.id, now) else {
        return;
    };
    let ms = snapshot.time_ms as f32;
    let at = Vec3::from_array(entity.position) / UNIT;
    let mut rng = crate::view_rng::ja_seed(entity.id.0, snapshot.tick as u32);
    let saber = held_saber(entity, objects);
    // KOTOR's row for what is not a lightsaber (`weapon_sounds`).
    let row = |of: &EntityState| kotor.and_then(|k| Some((k, k.row(of, snapshot, objects)?)));
    if entity
        .player
        .is_some_and(|player| player.events & motion::events::ROLL != 0)
        && let Some(kotor) = kotor
        && let Some(name) = kotor.swing(0, Swing::Short, &mut rng)
    {
        cues.push(Cue {
            entity: Some(entity.id),
            channel: Channel::Body,
            follows: true,
            ..Cue::at(name, at, ms)
        });
    }
    if let Some(double) = saber
        && now.lit != before.lit
        && (now.lit || f.alive())
    {
        let sounds = &blades.setup.of(double).0;
        cues.push(Cue {
            entity: Some(entity.id),
            follows: now.lit,
            ..Cue::at(sounds[if now.lit { 0 } else { 2 }].clone(), at, ms)
        });
    }
    let attack =
        now.saber_move.attack() && (!now.saber_move.kick() || now.saber_move == Move::HILT_BASH);
    if attack && now.saber_move != before.saber_move {
        let name = match (saber, row(entity)) {
            (Some(_), _) => Some(numbered("saber/saberhup", rng.roll_between(1, 8))),
            (None, Some((kotor, row))) => kotor.swing(row, Swing::Short, &mut rng),
            (None, None) => None,
        };
        cues.extend(name.map(|name| Cue {
            entity: Some(entity.id),
            channel: Channel::Weapon,
            ..Cue::at(name, at, ms)
        }));
    }
    let fresh = now.next_event.wrapping_sub(before.next_event);
    for event in f.events.iter().flatten() {
        if event.sequence.wrapping_sub(before.next_event) >= fresh
            || event.result != Outcome::Parried
        {
            continue;
        }
        let Some(by) = event.by else { continue };
        let pair = (entity.id.min(by), entity.id.max(by));
        if blades.blocks.iter().any(|&(a, b, _)| (a, b) == pair) {
            continue;
        }
        blades.blocks.push((pair.0, pair.1, snapshot.tick));
        let point = match event.at {
            Some(at) => Vec3::from_array(at.map(|v| v as f32)),
            None => meeting(entity, cast::entity(snapshot, by), snapshot, level),
        };
        // The striker's: a lightsaber's JA's, the rest KOTOR's parry.
        let name = match cast::entity(snapshot, by).and_then(row) {
            Some((kotor, row)) => kotor.parry(row, &mut rng),
            None => Some(numbered("saber/saberblock", rng.roll_between(1, 9))),
        };
        cues.extend(name.map(|name| Cue::at(name, point, ms)));
        // The flare starts 50 ms back (the client's flash time).
        blades.flash = Some((point, ms - 50.0));
    }
}

/// Each lit blade of `entity` (one of `objects`' snapshot) at `ms` (units):
/// hilt and tip.
pub(crate) fn blade_segments(
    entity: &EntityState,
    (objects, ms): (&Instances, u64),
    level: &Level,
) -> Vec<(usize, Vec3, Vec3)> {
    let Some(f) = entity.fighter else {
        return Vec::new();
    };
    let kit = Kit::of(&cast::sheet(entity, objects));
    let slots = blades::slots(&kit, &f.saber, f.hand);
    let data = level.anims().skeleton();
    slots
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| {
            let (slot, trail) = (slot.as_ref()?, blades::fresh(f.trails[i], ms)?);
            let length = slot
                .length
                .or_else(|| Some(data?.saber_blade(slot.id)?.length))
                .unwrap_or(BLADE_LENGTH);
            let base = Vec3::from_array(trail.muzzle);
            Some((i, base, base + Vec3::from_array(trail.direction) * length))
        })
        .collect()
}

/// Where a block between `entity` and `other` shows: midway between their
/// blades where they pass closest; or the nearest point of the one blade
/// there is to the other's chest.
fn meeting(
    entity: &EntityState,
    other: Option<&EntityState>,
    snapshot: &Snapshot,
    level: &Level,
) -> Vec3 {
    let chest = |e: &EntityState| {
        e.player.map_or(Vec3::from_array(e.position) / UNIT, |p| {
            Vec3::from_array(sim::combat::mind::chest(&p))
        })
    };
    let at = (&Instances::in_snapshot(level, snapshot), snapshot.time_ms);
    let mine = blade_segments(entity, at, level);
    let theirs = other.map_or(Vec::new(), |o| blade_segments(o, at, level));
    let pairs = mine.iter().flat_map(|a| theirs.iter().map(move |b| (a, b)));
    let closest = pairs
        .map(|(a, b)| {
            let (p, q) = closest_points((a.1, a.2), (b.1, b.2));
            (p.distance_squared(q), (p + q) / 2.0)
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, at)| at);
    let target = other.map_or(chest(entity), chest);
    closest.unwrap_or_else(|| {
        mine.iter()
            .chain(&theirs)
            .map(|&(_, a, b)| nearest_on((a, b), target))
            .min_by(|x, y| {
                x.distance_squared(target)
                    .total_cmp(&y.distance_squared(target))
            })
            .unwrap_or(chest(entity))
    })
}

fn nearest_on((a, b): (Vec3, Vec3), p: Vec3) -> Vec3 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
    a + ab * t
}

/// The points of two segments nearest each other.
fn closest_points((p1, q1): (Vec3, Vec3), (p2, q2): (Vec3, Vec3)) -> (Vec3, Vec3) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.length_squared(), d2.length_squared(), d2.dot(r));
    let (c, b) = (d1.dot(r), d1.dot(d2));
    let denominator = a * e - b * b;
    let mut s = if denominator > f32::EPSILON {
        ((b * f - c * e) / denominator).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut t = if e > f32::EPSILON {
        (b * s + f) / e
    } else {
        0.0
    };
    if t < 0.0 {
        t = 0.0;
        s = if a > f32::EPSILON {
            (-c / a).clamp(0.0, 1.0)
        } else {
            0.0
        };
    } else if t > 1.0 {
        t = 1.0;
        s = if a > f32::EPSILON {
            ((b - c) / a).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    (p1 + d1 * s, p2 + d2 * t)
}

/// The lit blades as drawn meeting the level, `frames` client frames on
/// at `ms` (the view's): sparks, a wall sound where a blade met it the
/// client frame before too, and marks where it met the level itself.
fn walls(
    blades: &mut Blades,
    (wielders, drawn): (&[(SimId, usize, usize)], &DrawnBlades),
    (snapshot, objects, kotor): Met,
    level: &Level,
    (ms, frames): (f32, u32),
    (cues, bursts, cuts): (&mut Vec<Cue>, &mut Vec<Burst>, &mut Vec<Cut>),
) {
    let states = level.states_in(snapshot);
    let point = Extents {
        mins: [0.0; 3],
        maxs: [0.0; 3],
    };
    let mut touched = Vec::new();
    for &(id, slot, spec) in wielders {
        let Some([hilt, tip]) = drawn.0.get(spec).copied().flatten() else {
            continue;
        };
        // Only a lightsaber's blade melts what it touches: a melee weapon's
        // meeting a wall shows and sounds nothing (its blows and parries do).
        let entity = cast::entity(snapshot, id);
        if entity.is_some_and(|entity| held_saber(entity, objects).is_none()) {
            continue;
        }
        let trace = level.with(states.as_deref(), |world| {
            world.trace(hilt.to_array(), tip.to_array(), &point, Contents::SOLID)
        });
        if trace.fraction >= 1.0 || trace.start_solid {
            continue;
        }
        let at = Vec3::from_array(trace.end) / UNIT;
        let normal = Vec3::from_array(trace.plane.normal)
            .try_normalize()
            .unwrap_or(Vec3::Y);
        bursts.push(Burst {
            effect: WALL_EFFECT,
            at,
            normal,
            ms,
            times: frames,
            axis: None,
        });
        let key = (id, slot);
        let mut wall = blades.walls.get(&key).copied().unwrap_or(Wall::FRESH);
        let tick = (ms / FRAME_MS) as u32;
        if let Some(from) = wall.met {
            if ms - wall.sounded >= WALL_SOUND_MS {
                let mut rng = Rng((id.0 ^ tick).wrapping_mul(31) as i32);
                let row = kotor
                    .zip(entity)
                    .and_then(|(k, entity)| Some((k, k.row(entity, snapshot, objects)?)));
                let name = match row {
                    Some((kotor, row)) => kotor.wall(row, trace.surface.material, &mut rng),
                    None => Some(numbered("saber/saberhitwall", rng.roll_between(1, 3))),
                };
                cues.extend(name.map(|name| Cue {
                    channel: Channel::Weapon,
                    ..Cue::at(name, at, ms)
                }));
                wall.sounded = ms;
            }
            // A strip from where it met last, on the level alone.
            if trace.entity.is_none() {
                cuts.push(Cut {
                    from,
                    to: at,
                    normal,
                    ms,
                    frames,
                    seed: (id.0 ^ ((slot as u32) << 16), tick),
                });
            }
        }
        touched.push((
            key,
            Wall {
                met: Some(at),
                ..wall
            },
        ));
    }
    // A blade off the walls this client frame starts afresh.
    for wall in blades.walls.values_mut() {
        wall.met = None;
    }
    blades.walls.extend(touched);
}

/// A keyframed sound (`asset_ja::events`) of `entity`'s, at `at` (units),
/// at the view's `ms`, unless its chance says no. With KOTOR's weapon
/// sounds and `entity`'s row (`None`: a lightsaber), the whooshes are its
/// weapon's: a saber whoosh or spin of any other weapon its KOTOR swing
/// (a fast one short, the rest long; a spin a twirl), and JA's fist and
/// kick whooshes KOTOR's unarmed swing.
pub fn keyframed(
    keyed: &Keyed,
    (entity, at, ms): (SimId, Vec3, f32),
    kotor: Option<(&WeaponSounds, Option<usize>)>,
) -> Option<Cue> {
    let mut rng = Rng((entity.0.wrapping_mul(0x85eb_ca6b) ^ (ms / FRAME_MS) as u32) as i32);
    let name = keyed.sound(&mut rng)?;
    let own = match (&keyed.play, kotor) {
        (Play::Swing(weight), Some((kotor, Some(row)))) => {
            let swing = if *weight == 0 {
                Swing::Short
            } else {
                Swing::Long
            };
            kotor.swing(row, swing, &mut rng)
        }
        (Play::Spin(_), Some((kotor, Some(row)))) => kotor.swing(row, Swing::Twirl, &mut rng),
        (Play::Sound { name, .. }, Some((kotor, _))) if name.starts_with(BODY_WHOOSH) => {
            kotor.swing(0, Swing::Short, &mut rng)
        }
        _ => None,
    };
    Some(Cue {
        entity: Some(entity),
        channel: match keyed.channel {
            KeyChannel::Auto => Channel::Auto,
            KeyChannel::Weapon => Channel::Weapon,
            KeyChannel::Body => Channel::Body,
            KeyChannel::Voice => Channel::Voice,
        },
        follows: !keyed.stays(),
        ..Cue::at(own.unwrap_or(name), at, ms)
    })
}
