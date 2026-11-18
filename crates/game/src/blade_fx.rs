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
//! - Perfect blocks: under the Advanced rules, a block made in the same
//!   step as a fighter's new perfect block (`Duel::perfect_at`) sounds one
//!   of `saberbounce1–3` at 4.2 times its gain. It plays in place of
//!   `saberblock1–9`, and together with KOTOR's parry for a blow not of a
//!   lightsaber. Its flare is twice as large, at full brightness, and shows
//!   for 400 ms instead of 150 ms. Every player who sees and hears the
//!   block gets these. When the body that the player controls made the
//!   perfect block, that player's camera also shakes for 250 ms of real
//!   time (`ClashShake`).
//! - Walls: every lit blade as drawn (`scene::DrawnBlades`, whose blades
//!   `Wielders` tells) traced from hilt to tip against the level each
//!   client frame, as the cgame traces the blade it draws; where it meets
//!   a wall, sparks (`sparks/spark_nosnd`); meeting it two client frames
//!   running, one of `saberhitwall1–3`, at most every 100 ms a blade, and
//!   where it met the level itself a burn and a glow along the way it
//!   dragged (`saber_marks`). A client frame is a sixtieth of a second of
//!   the view's time (QUIRKS.md Q-0005): a drawn frame that ends none does
//!   nothing here, one that ends several does it once for all of them.

use std::collections::BTreeMap;

use asset_ja::events::{Channel as KeyChannel, Keyed, Play};
use asset_ja::fx::Rng;
use audio::Channel;
use bevy::prelude::*;
use collision::{Contents, Extents};
use sim::combat::Outcome;
use sim::combat::saber::throw::Flight;
use sim::combat::saber::{Kit, Move, blades};
use sim::{EntityState, Instances, Level, Seat, SimId, Snapshot};

use crate::cast;
use crate::combat_fx::{Added, Burst, Bursts, FightDrawn};
use crate::lifecycle_view::SceneCamera;
use crate::saber_marks::{Cut, Cuts};
use crate::scene::DrawnBlades;
use crate::script_fx::Shake;
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

/// How a clash's flare looks. `size` is its size at full strength on JA's
/// virtual screen, which is 640 pixels wide, as a share of half that width
/// (320 pixels). `bright` is the brightness of its colour, from 0 to 1.
/// `ms` is how long the flare shows.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FlareLook {
    size: f32,
    bright: f32,
    ms: f32,
}

/// An ordinary block's flare: 300 pixels on JA's screen, at 0.8 brightness,
/// for JA's 150 ms.
const BLOCK_FLARE: FlareLook = FlareLook {
    size: 300.0 / 320.0,
    bright: 0.8,
    ms: 150.0,
};
/// A perfect block's flare: twice as large as an ordinary block's, at full
/// brightness. It shows for 400 ms, which is 2.7 times as long as an
/// ordinary block's flare, so that it is still seen after the slow motion.
const PERFECT_FLARE: FlareLook = FlareLook {
    size: 600.0 / 320.0,
    bright: 1.0,
    ms: 400.0,
};

/// An ordinary block's sound: one of `saberblock1–9`.
const BLOCK_SOUNDS: (&str, i32) = ("saber/saberblock", 9);
/// A perfect block's sound: one of `saberbounce1–3`, JA's lower and harder
/// ring of a blade knocked back.
const PERFECT_SOUNDS: (&str, i32) = ("saber/saberbounce", 3);
/// The gain of a perfect block's sound. The RMS of `saberbounce1–3` is on
/// average 0.142 of full scale, and that of `saberblock1–9` 0.290. At 4.2
/// times its own gain, a perfect block is about twice as loud as an
/// ordinary one, which is 6 dB louder. Its peaks then go over full scale,
/// and the mix's limiter (`audio::mixer`, from -3 dB at 20 to 1) holds
/// them, as it holds the peaks of `saberblock1–9`, which already reach
/// full scale.
const PERFECT_GAIN: f32 = 4.2;

/// The camera of a player whose own body makes a perfect block shakes for
/// 250 ms of real time. Real time is used so that the perfect block's own
/// slow motion does not stretch the shake (`docs/SABER-RULES.md` 3). The
/// shake turns the camera by at most about 1.4 degrees, which is twice as
/// far as a script's shake of scale 1.
const PERFECT_SHAKE_MS: u64 = 250;
const PERFECT_SHAKE_SCALE: f32 = 2.0;

/// The shake of a perfect block that the player's own body made, seen at
/// `real_ms` (the real clock, ms).
fn perfect_shake(real_ms: u64) -> Shake {
    Shake {
        from: real_ms,
        total: PERFECT_SHAKE_MS,
        scale: PERFECT_SHAKE_SCALE,
    }
}

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

/// One of the numbered sounds `stem` 1 to `last`, at random.
fn one_of((stem, last): (&str, i32), rng: &mut Rng) -> String {
    numbered(stem, rng.roll_between(1, last))
}

impl Setup {
    /// Every sound this plays.
    pub fn sounds(&self) -> Vec<String> {
        let mut names: Vec<String> = [&self.single, &self.staff]
            .into_iter()
            .flat_map(|saber| saber.0.iter().cloned())
            .chain([asset_ja::SABER_SPIN, asset_ja::SABER_CATCH].map(String::from))
            .collect();
        let ranges = [
            ("saber/saberhup", 1..=8),
            (BLOCK_SOUNDS.0, 1..=BLOCK_SOUNDS.1),
            (PERFECT_SOUNDS.0, 1..=PERFECT_SOUNDS.1),
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
    app.insert_resource(Blades::new(setup));
}

/// What each character's blades were at the last snapshot seen.
#[derive(Clone, Copy)]
struct Seen {
    lit: bool,
    /// A staff's second blade or a pair's second saber put out (the style
    /// button).
    second_off: bool,
    /// Its thrown saber coming back to the hand.
    coming_back: bool,
    saber_move: Move,
    next_event: u16,
    /// Its last perfect block (`Duel::perfect_at`).
    perfect_at: u64,
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
    /// The last block's clash flare: where, from when (the view's ms), and
    /// how it looks.
    flash: Option<(Vec3, f32, FlareLook)>,
    /// The last client frame the walls were met in (`FRAME_MS`s of the
    /// view's time).
    frame: i64,
    generation: u32,
}

impl Blades {
    /// A session's blade effects before its first snapshot.
    fn new(setup: Setup) -> Blades {
        Blades {
            setup,
            seen: BTreeMap::new(),
            walls: BTreeMap::new(),
            blocks: Vec::new(),
            flash: None,
            frame: i64::MIN,
            generation: 0,
        }
    }
}

/// The camera shake of the last perfect block that the body the player
/// controls made, which `play_view` adds to the camera's turn. Its times
/// are on the real clock (`Time<Real>`, ms), not the world's.
#[derive(Resource, Default)]
pub struct ClashShake(pub Option<Shake>);

impl Plugin for BladeFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wielders>()
            .init_resource::<ClashShake>()
            .add_systems(
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
    (seat, mut clash, real): (Option<Res<OwnSeat>>, ResMut<ClashShake>, Res<Time<Real>>),
    mut blades: ResMut<Blades>,
    (mut cues, mut loops, mut bursts, mut added): (
        ResMut<Cues>,
        ResMut<Loops>,
        ResMut<Bursts>,
        ResMut<Added>,
    ),
    (mut cuts, wielders, drawn, doors): (
        ResMut<Cuts>,
        Res<Wielders>,
        Res<DrawnBlades>,
        Res<crate::sim_host::PresentedDoors>,
    ),
    cameras: Query<(&GlobalTransform, &Projection), SceneCamera>,
    kotor: Option<Res<WeaponSounds>>,
) {
    let kotor = kotor.as_deref();
    let seat = seat.map_or(Seat(0), |seat| seat.0);
    let blades = &mut *blades;
    if swap.generation != blades.generation {
        blades.generation = swap.generation;
        blades.seen.clear();
        blades.walls.clear();
        blades.blocks.clear();
        blades.frame = i64::MIN;
        clash.0 = None;
    }
    let level = &swap.level;
    let current = presented.0.current();
    let now = crate::sim_host::view_ms(&presented.0) as f32;
    for snapshot in &presented.1 {
        let snapshot: &Snapshot = snapshot;
        let out = &mut (&mut cues.0, &mut bursts.0);
        let objects = Instances::in_snapshot(level, snapshot);
        if follow(blades, (snapshot, &objects, kotor), level, seat, out) {
            clash.0 = Some(perfect_shake(real.elapsed().as_millis() as u64));
        }
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
    let objects = Instances::in_snapshot(level, current);
    if frames > 0 {
        let out = (&mut cues.0, &mut bursts.0, &mut cuts.0);
        let met = (current, &objects, kotor);
        walls(
            blades,
            (&wielders.0, &drawn),
            met,
            (level, doors.of(&presented.0, &swap).as_deref()),
            (now, frames),
            out,
        );
    }
    let me = cast::me(current, seat);
    let eye = cameras
        .iter()
        .next()
        .map(|(camera, _)| crate::scene_fx::eye_of(camera).eye);
    if let (Some((at, from, look)), Some((camera, projection))) =
        (blades.flash, cameras.iter().next())
    {
        added.0.extend(flare(
            (at, now - from, now, look),
            camera,
            projection,
            (level, doors.of(&presented.0, &swap).as_deref()),
        ));
    }
    for entity in &current.entities {
        let Some(saber) = held_saber(entity, &objects) else {
            continue;
        };
        let f = entity.fighter.expect("held_saber checks");
        // Put away while its user works a lock or a mine, or throws a
        // grenade (`weapons::armed`).
        let hidden = crate::acting::hides_weapons(&f, current.time_ms as f64);
        // A thrown saber sounds where it flies (`at` is in units already):
        // out, its spin (JA's `saberSpinSound` as the thrown saber's
        // `loopSound`); coming back, its own hum (`soundLoop`).
        if let Some(thrown) = f
            .saber
            .thrown
            .filter(|thrown| f.saber.on && thrown.flight.flies())
        {
            let name = if thrown.flight == Flight::Back {
                blades.setup.of(saber).0[1].clone()
            } else {
                asset_ja::SABER_SPIN.to_string()
            };
            loops.0.push(Hold::at(name, Vec3::from_array(thrown.at)));
        }
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

impl FlareLook {
    /// The flare's radius (units) `since` ms after its clash and `distance`
    /// units away, under a view whose half horizontal field of view has the
    /// tangent `wide`. It fades with its time and shrinks with its distance.
    fn radius(&self, since: f32, distance: f32, wide: f32) -> f32 {
        let v = ((1.0 - since / self.ms) * ((1.0 - distance / 800.0) * 2.0 + 0.35)).max(0.001);
        v * self.size * distance * wide
    }
}

/// The flare of a clash at `at` (game space, units) `since` ms ago, drawn
/// at `now` (ms) as `look` says: for the look's time a sprite at the clash as large
/// on the screen as JA's screen-space flare times its size. None behind the
/// view, with the level in between, past 1200 units, or under a camera
/// without a perspective.
fn flare(
    (at, since, now, look): (Vec3, f32, f32, FlareLook),
    camera: &GlobalTransform,
    projection: &Projection,
    (level, doors): (&Level, Option<&[u8]>),
) -> Option<asset_ja::fx::Spawned> {
    if since <= 0.0 || since >= look.ms {
        return None;
    }
    let to_game = crate::scene::game_to_bevy().inverse();
    let eye = crate::scene_fx::eye_of(camera).eye;
    let forward = to_game * camera.forward().as_vec3();
    let away = at - eye;
    if away.dot(forward) < 0.2 {
        return None;
    }
    let hit = level.with(doors, |world| {
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
    let Projection::Perspective(perspective) = projection else {
        return None;
    };
    // Half the horizontal field of view.
    let wide = ((perspective.fov / 2.0).tan() * perspective.aspect_ratio).atan();
    let radius = look.radius(since, distance, wide.tan());
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
        rgb_start: Vec3::splat(look.bright),
        rgb_end: Vec3::splat(look.bright),
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

/// The sounds of one block at `point` and `ms`. `parry` is the striker's
/// KOTOR parry when its weapon is not a lightsaber (`Some(None)` when its
/// row names none), and None for a lightsaber. A lightsaber's block sounds
/// one of `saberblock1–9`, and another weapon's its parry. A perfect block
/// plays its louder ring in place of `saberblock1–9`, and together with the
/// parry of another weapon.
fn block_cues(
    perfect: bool,
    parry: Option<Option<String>>,
    rng: &mut Rng,
    (point, ms): (Vec3, f32),
) -> Vec<Cue> {
    let block = match (perfect, parry) {
        (_, Some(parry)) => parry,
        (false, None) => Some(one_of(BLOCK_SOUNDS, rng)),
        (true, None) => None,
    };
    let ring = perfect.then(|| Cue {
        gain: PERFECT_GAIN,
        ..Cue::at(one_of(PERFECT_SOUNDS, rng), point, ms)
    });
    (block.map(|name| Cue::at(name, point, ms)).into_iter())
        .chain(ring)
        .collect()
}

/// One presented snapshot's lightings, swings and blocks (`changes`). A
/// block is perfect when one of its two fighters made a new perfect block
/// in this snapshot (`perfect_blocks`). Whether the body that `seat`
/// controls made one, so that the player's camera shakes.
fn follow(blades: &mut Blades, met: Met, level: &Level, seat: Seat, out: &mut Out) -> bool {
    let snapshot = met.0;
    let perfect = perfect_blocks(&blades.seen, snapshot);
    for entity in &snapshot.entities {
        changes(blades, entity, met, level, out, &perfect);
    }
    blades
        .blocks
        .retain(|&(.., tick)| tick + MIRROR_TICKS >= snapshot.tick);
    let own = snapshot.body(seat).map(|own| own.id);
    perfect.iter().any(|&(id, _)| Some(id) == own)
}

/// The fighters of `snapshot` whose last perfect block (`Duel::perfect_at`)
/// is new since `seen`, and when they made it. None under Jedi Academy's
/// rules, where no block is perfect.
fn perfect_blocks(seen: &BTreeMap<SimId, Seen>, snapshot: &Snapshot) -> Vec<(SimId, u64)> {
    if !snapshot.saber_rules.duel() {
        return Vec::new();
    }
    (snapshot.entities.iter())
        .filter_map(|entity| {
            let at = entity.fighter?.saber.duel.perfect_at;
            let before = seen.get(&entity.id)?.perfect_at;
            (at != 0 && at != before).then_some((entity.id, at))
        })
        .collect()
}

/// What changed for one character since its last snapshot: lit or out, a
/// new attack, blocks. A block is perfect when either of its fighters is
/// in `perfect`.
fn changes(
    blades: &mut Blades,
    entity: &EntityState,
    (snapshot, objects, kotor): Met,
    level: &Level,
    (cues, _): &mut Out,
    perfect: &[(SimId, u64)],
) {
    let Some(f) = entity.fighter else { return };
    let now = Seen {
        lit: f.saber.on,
        second_off: f.saber.second_off,
        coming_back: f
            .saber
            .thrown
            .is_some_and(|thrown| thrown.flight == Flight::Back),
        saber_move: f.saber.current_move,
        next_event: f.next_event,
        perfect_at: f.saber.duel.perfect_at,
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
    // Lit or out, and a lit saber's second blade (or a pair's second
    // saber) lit or put out by the style button (`Cmd_SaberAttackCycle_f`'s
    // `soundOn` / `soundOff`).
    let lighting = if now.lit != before.lit && (now.lit || f.alive()) {
        Some(now.lit)
    } else if now.lit && before.lit && now.second_off != before.second_off {
        Some(!now.second_off)
    } else {
        None
    };
    // Coming back and now in the hand: caught.
    if before.coming_back && f.saber.thrown.is_none() && f.alive() {
        cues.push(Cue {
            entity: Some(entity.id),
            ..Cue::at(asset_ja::SABER_CATCH.to_string(), at, ms)
        });
    }
    if let Some(double) = saber
        && let Some(on) = lighting
    {
        let sounds = &blades.setup.of(double).0;
        cues.push(Cue {
            entity: Some(entity.id),
            follows: on,
            ..Cue::at(sounds[if on { 0 } else { 2 }].clone(), at, ms)
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
        let perfect = (perfect.iter()).any(|&(id, _)| id == entity.id || id == by);
        let parry = (cast::entity(snapshot, by).and_then(row))
            .map(|(kotor, row)| kotor.parry(row, &mut rng));
        cues.extend(block_cues(perfect, parry, &mut rng, (point, ms)));
        let look = if perfect { PERFECT_FLARE } else { BLOCK_FLARE };
        // The flare starts 50 ms back (the client's flash time).
        blades.flash = Some((point, ms - 50.0, look));
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
            let full = data.and_then(|data| Some(data.saber_blade(slot.id)?.length));
            let length = slot.length(full.or(Some(BLADE_LENGTH)))?;
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
    (level, states): (&Level, Option<&[u8]>),
    (ms, frames): (f32, u32),
    (cues, bursts, cuts): (&mut Vec<Cue>, &mut Vec<Burst>, &mut Vec<Cut>),
) {
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
        let trace = level.with(states, |world| {
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

/// The probes (`kjk sim probes --only "perfect block feedback"`): what an
/// ordinary block and a perfect block sound like, how their flares look,
/// and whose camera shakes, and how far.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    vec![
        ("block_feedback", block_feedback()),
        ("block_sounds", block_sounds()),
        ("flare_sizes", flare_sizes()),
        ("flare_colours", flare_colours()),
        ("perfect_shake", perfect_shake_probe()),
    ]
}

/// The real time (ms) at which the probes see a block.
#[cfg(feature = "probes")]
const PROBE_REAL_MS: u64 = 5000;

/// A sound as the probes write it: its name without its number, and its
/// gain.
#[cfg(feature = "probes")]
fn heard(cue: &Cue) -> (String, f32) {
    let name = cue.name.trim_end_matches(".wav");
    let name = name.trim_end_matches(|c: char| c.is_ascii_digit());
    (name.to_string(), cue.gain)
}

/// What the first block of a staged fight makes for the player of one seat:
/// its sounds, how its flare looks, and the shake of that player's camera.
#[cfg(feature = "probes")]
#[derive(Debug, PartialEq)]
struct Felt {
    sounds: Vec<(String, f32)>,
    flare: FlareLook,
    shake: Option<Shake>,
}

/// The first block of `snapshots` as the player of `seat` sees and hears it
/// (`follow`), with the block seen at `PROBE_REAL_MS` of real time. None if
/// no block was made.
#[cfg(feature = "probes")]
fn first_block(snapshots: &[Snapshot], seat: Seat) -> Option<Felt> {
    let level = Level::arena();
    let saber = SaberSounds(asset_ja::SABER_SOUNDS.map(String::from));
    let mut blades = Blades::new(Setup {
        single: saber.clone(),
        staff: saber,
    });
    snapshots.iter().find_map(|snapshot| {
        let objects = Instances::in_snapshot(&level, snapshot);
        let (mut cues, mut bursts) = (Vec::new(), Vec::new());
        let met = (snapshot, &objects, None);
        let own = follow(
            &mut blades,
            met,
            &level,
            seat,
            &mut (&mut cues, &mut bursts),
        );
        let (.., flare) = blades.flash?;
        let blocks = [BLOCK_SOUNDS.0, PERFECT_SOUNDS.0];
        Some(Felt {
            sounds: (cues.iter().map(heard))
                .filter(|(name, _)| blocks.iter().any(|block| name.contains(block)))
                .collect(),
            flare,
            shake: own.then(|| perfect_shake(PROBE_REAL_MS)),
        })
    })
}

/// The first block in three staged fights, for the player of each seat, on
/// the host and on a guest that decodes the host's snapshots. In the fight
/// where seat 1 turned its camera to the left, seat 1 makes a perfect block
/// against seat 0. Both players hear one of `saberbounce1–3` at 4.2 times
/// its gain and see the perfect block's flare, and only seat 1's camera
/// shakes. The same holds for seat 1's perfect block of seat 0's spin,
/// which cannot be blocked otherwise. In the fight where seat 1 has not
/// turned its camera, the block is ordinary: one of `saberblock1–9`, the
/// ordinary flare, and no shake. Under Jedi Academy's rules no block is
/// perfect.
#[cfg(feature = "probes")]
fn block_feedback() -> Result<String, String> {
    use sim::combat::saber::SaberRules;
    use sim::trace::duelling_probes::targeted::{Stage, staged};
    let perfect = staged(Stage::TurnedLeft)?;
    let ordinary = staged(Stage::Unturned)?;
    let spin = staged(Stage::Spin)?;
    let ja: Vec<Snapshot> = (perfect.iter())
        .map(|s| {
            let mut s = s.clone();
            s.saber_rules = SaberRules::Ja;
            s
        })
        .collect();
    let at = (perfect.iter())
        .map(|s| crate::block_reticle_view::perfect_at(s, Seat(1)))
        .find(|&at| at != 0)
        .ok_or("seat 1 makes no perfect block")?;
    let felt = |perfect: bool, shaken: bool| Felt {
        sounds: vec![match perfect {
            true => (format!("sound/weapons/{}", PERFECT_SOUNDS.0), PERFECT_GAIN),
            false => (format!("sound/weapons/{}", BLOCK_SOUNDS.0), 1.0),
        }],
        flare: if perfect { PERFECT_FLARE } else { BLOCK_FLARE },
        shake: shaken.then(|| perfect_shake(PROBE_REAL_MS)),
    };
    let rows: [(&str, &[Snapshot], Seat, Felt); 7] = [
        (
            "a perfect block, for the blocker",
            &perfect,
            Seat(1),
            felt(true, true),
        ),
        (
            "a perfect block, for the attacker",
            &perfect,
            Seat(0),
            felt(true, false),
        ),
        (
            "a perfect block of a spin, for the blocker",
            &spin,
            Seat(1),
            felt(true, true),
        ),
        (
            "a perfect block of a spin, for the attacker",
            &spin,
            Seat(0),
            felt(true, false),
        ),
        (
            "an ordinary block, for the blocker",
            &ordinary,
            Seat(1),
            felt(false, false),
        ),
        (
            "an ordinary block, for the attacker",
            &ordinary,
            Seat(0),
            felt(false, false),
        ),
        (
            "Jedi Academy's rules, for the blocker",
            &ja,
            Seat(1),
            felt(false, false),
        ),
    ];
    let decoded = |snapshots: &[Snapshot]| -> Result<Vec<Snapshot>, String> {
        (snapshots.iter())
            .map(|s| Snapshot::decode(&s.encode()).map_err(|e| format!("{e:?}")))
            .collect()
    };
    let mut wrong = Vec::new();
    for (name, snapshots, seat, want) in &rows {
        for (whose, snapshots) in [("host", snapshots.to_vec()), ("guest", decoded(snapshots)?)] {
            let got = first_block(&snapshots, *seat);
            if got.as_ref() != Some(want) {
                wrong.push(format!(
                    "{name}, on the {whose}: {got:?}, expected {want:?}"
                ));
            }
        }
    }
    match wrong.is_empty() {
        true => Ok(format!(
            "{} rows right on the host and on a guest, the perfect block at {at} ms",
            rows.len()
        )),
        false => Err(wrong.join("; ")),
    }
}

/// The sounds of a block (`block_cues`) by the striker's weapon. A
/// lightsaber's ordinary block sounds `saberblock`, and its perfect block
/// `saberbounce` at 4.2 times its gain. Another weapon's ordinary block
/// sounds its KOTOR parry, and its perfect block that parry together with
/// `saberbounce`. A weapon whose row names no parry sounds only the ring
/// when perfect, and nothing otherwise.
#[cfg(feature = "probes")]
fn block_sounds() -> Result<String, String> {
    let parry = "sound/weapons/cm_parry";
    let block = format!("sound/weapons/{}", BLOCK_SOUNDS.0);
    let ring = format!("sound/weapons/{}", PERFECT_SOUNDS.0);
    /// A row's name, whether the block is perfect, the striker's parry, and
    /// the sounds expected.
    type Row = (
        &'static str,
        bool,
        Option<Option<String>>,
        Vec<(String, f32)>,
    );
    let rows: [Row; 6] = [
        ("a lightsaber, ordinary", false, None, vec![(block, 1.0)]),
        (
            "a lightsaber, perfect",
            true,
            None,
            vec![(ring.clone(), PERFECT_GAIN)],
        ),
        (
            "another weapon, ordinary",
            false,
            Some(Some(parry.into())),
            vec![(parry.into(), 1.0)],
        ),
        (
            "another weapon, perfect",
            true,
            Some(Some(parry.into())),
            vec![(parry.into(), 1.0), (ring.clone(), PERFECT_GAIN)],
        ),
        (
            "a weapon with no parry, ordinary",
            false,
            Some(None),
            vec![],
        ),
        (
            "a weapon with no parry, perfect",
            true,
            Some(None),
            vec![(ring, PERFECT_GAIN)],
        ),
    ];
    let wrong: Vec<String> = (rows.into_iter())
        .filter_map(|(name, perfect, parry, want)| {
            let cues = block_cues(perfect, parry, &mut Rng(1), (Vec3::ZERO, 0.0));
            let got: Vec<(String, f32)> = cues.iter().map(heard).collect();
            (got != want).then(|| format!("{name}: {got:?}, expected {want:?}"))
        })
        .collect();
    match wrong.is_empty() {
        true => Ok("6 rows right".into()),
        false => Err(wrong.join("; ")),
    }
}

/// The flares' sizes and brightness, 50 ms after the clash and 100 units
/// away, under a view whose half horizontal field of view has the tangent 1.
/// The ordinary flare keeps the size it had before perfect blocks had their
/// own, 131.25 units, and its brightness of 0.8. The perfect block's flare
/// is twice as large at full strength and fades over 400 ms instead of
/// 150 ms, so 50 ms after the clash it is 344.53 units, at full brightness.
#[cfg(feature = "probes")]
fn flare_sizes() -> Result<String, String> {
    let rows = [
        ("an ordinary block", BLOCK_FLARE, (131.25, 0.8)),
        ("a perfect block", PERFECT_FLARE, (344.531_25, 1.0)),
    ];
    let wrong: Vec<String> = (rows.iter())
        .map(|(name, look, want)| (name, (look.radius(50.0, 100.0, 1.0), look.bright), want))
        .filter(|(_, got, want)| (got.0 - want.0).abs() > 0.001 || got.1 != want.1)
        .map(|(name, got, want)| {
            format!("{name}: radius and brightness {got:?}, expected {want:?}")
        })
        .collect();
    match wrong.is_empty() {
        true => Ok(format!("{rows:?}")),
        false => Err(wrong.join("; ")),
    }
}

/// The colour of the flare that `flare` spawns, seen from seat 0's eye at a
/// point at seat 1's chest in the first snapshot of a staged fight in the
/// arena. Its colour, at the start and at the end, is the look's brightness
/// in each channel: 0.8 for an ordinary block and 1.0 for a perfect one.
#[cfg(feature = "probes")]
fn flare_colours() -> Result<String, String> {
    use sim::trace::duelling_probes::targeted::{Stage, staged};
    let snapshot = staged(Stage::Unturned)?
        .into_iter()
        .next()
        .ok_or("the fight has no snapshot")?;
    let level = Level::arena();
    let place = |seat: u8, up: f32| -> Result<Vec3, String> {
        let body = snapshot.body(Seat(seat)).ok_or("a seat has no body")?;
        Ok(Vec3::from_array(body.position) / UNIT + Vec3::Z * up)
    };
    let (eye, at) = (place(0, 40.0)?, place(1, 24.0)?);
    let to_bevy = crate::scene::game_to_bevy();
    let camera = GlobalTransform::from(
        Transform::from_translation(to_bevy * eye * UNIT)
            .looking_at(to_bevy * at * UNIT, to_bevy * Vec3::Z),
    );
    let projection = Projection::Perspective(PerspectiveProjection::default());
    let rows = [
        ("an ordinary block", BLOCK_FLARE),
        ("a perfect block", PERFECT_FLARE),
    ];
    let got: Vec<(&str, Option<[Vec3; 2]>)> = (rows.iter())
        .map(|(name, look)| {
            let spawned = flare(
                (at, 50.0, 0.0, *look),
                &camera,
                &projection,
                (&level, level.states_in(&snapshot).as_deref()),
            );
            (
                *name,
                spawned.map(|spawned| [spawned.rgb_start, spawned.rgb_end]),
            )
        })
        .collect();
    let wrong: Vec<String> = (rows.iter().zip(&got))
        .filter(|((_, look), (_, got))| *got != Some([Vec3::splat(look.bright); 2]))
        .map(|((name, look), (_, got))| format!("{name}: colour {got:?}, expected {}", look.bright))
        .collect();
    match wrong.is_empty() {
        true => Ok(format!("the colours at start and end: {got:?}")),
        false => Err(wrong.join("; ")),
    }
}

/// The perfect block's shake (`perfect_shake`), sampled every 10
/// microseconds of real time. It turns the camera by at most 1.395 degrees,
/// within 0.005, and stops 250 ms after it starts.
#[cfg(feature = "probes")]
fn perfect_shake_probe() -> Result<String, String> {
    let shake = perfect_shake(PROBE_REAL_MS);
    let turn = |since: f64| shake.angles(PROBE_REAL_MS as f64 + since, [true, false])[0].abs();
    let samples = PERFECT_SHAKE_MS as u32 * 100;
    let peak = (0..samples)
        .map(|i| turn(f64::from(i) / 100.0))
        .fold(0.0, f32::max);
    let end = PERFECT_SHAKE_MS as f64;
    let after = turn(end);
    match (peak - 1.395).abs() <= 0.005 && after == 0.0 {
        true => Ok(format!("peak {peak:.4} degrees, nothing at {end} ms")),
        false => Err(format!(
            "peak {peak:.4} degrees and {after} at {end} ms, expected 1.395 and 0"
        )),
    }
}
