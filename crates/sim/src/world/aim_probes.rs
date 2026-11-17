//! The probes of the crosshair line (`kjk sim probes --only aim`): a
//! player's shots go where the middle of its screen looks, whatever its
//! camera (`Command::aim`, `combat::mind::crosshair`), on the proving
//! ground, no install:
//! - the round trip: a camera's place turned into a command's aim
//!   (`mind::aim_for`) gives a crosshair line within a step of the camera's
//!   own, for JA's third-person camera (behind the line it looks along),
//!   the own eyes (below it and beside it) and a camera far off;
//! - a bolt: the player's pistol fired with its own eyes' aim, and with
//!   none (the third-person camera's), flies at where each line meets the
//!   level, and the two meet it apart (the eyes' line runs lower).
//!
//! And the probes of hold to aim (`Shooting::aim`), no world:
//! - the gun is fully raised after `aim::IN_MS` held, not before, and
//!   fully lowered `aim::OUT_MS` after the release, not before;
//! - the aim and a Sniper Shot's steadying round-trip through a snapshot,
//!   and a snapshot from before hold to aim reads the gun lowered;
//! - putting the gun away lowers it and ends a steadying;
//! - the scripted words `alt` and `feat`, and `Command::other_mode`, hold
//!   the aim and feat attack bits.
//!
//! And the probes of hold to aim in the world, on the proving ground:
//! - a repeating blaster held up to aim, still and walking, spreads its shots by
//!   `weapons.bloom.stance` "aiming" once fully raised, and by the normal
//!   spread before;
//! - a character aiming walks as a held [Walk] does, under Force Speed
//!   too, and never jumps or rolls;
//! - every blaster aims, and no blade or bare hands do;
//! - only a body a seat plays aims: a creature's held aim does nothing;
//! - a weapon switch, the holster, a stun, a knockdown and a medpac each
//!   lower the gun on the next step;
//! - a predicted step raises the gun as the authority's does.

use super::{Fighting, SimWorld};
use crate::combat::bloom::shot_cone;
use crate::combat::sheet::{Blaster, Delivery, SaberShape};
use crate::combat::specials::Shooting;
use crate::combat::{Fighter, Sheath, Sheet, mind, weapons};
use crate::input::{Command, Seat, TickInput, buttons, holds};
use crate::math::{self, Vec3};
use crate::motion::{Anim, MotionState};
use crate::moves::Script;
use crate::probe::{Checks, check};
use crate::wire::{Reader, Versions, Writer};
use crate::{StepReason, WorldConfig};
use collision::{Contents, Extents, UNIT};

const MSEC: u32 = 8;
/// The eyes' aim the bolt is fired with: 2 units right, 9 below the
/// third-person line, as a first-person camera stood in a playtest.
const EYES: [i8; 3] = [0, -4, -18];
/// The walk axis a held [Walk] sends (JA's client).
const WALK_AXIS: i8 = rules::minds::tasks::WALK_AXIS;

pub fn probes() -> Checks {
    vec![
        ("aim_round_trip", round_trip()),
        ("bolt_flies_at_the_camera_line", bolt()),
        ("aim_level", aim_level()),
        ("aim_round_trips", aim_round_trips()),
        ("old_snapshot_aim", old_snapshot_aim()),
        ("interrupt_drops_aim", interrupt_drops_aim()),
        ("moves_words", moves_words()),
        ("other_mode", other_mode()),
        ("aim_spread", aim_spread()),
        ("aim_walk_spread", aim_walk_spread()),
        ("aim_walks", aim_walks()),
        ("aim_no_jump", aim_no_jump()),
        ("aim_no_roll", aim_no_roll()),
        ("aim_force_speed", aim_force_speed()),
        ("aim_blasters", aim_blasters()),
        ("aim_not_blades", aim_not_blades()),
        ("aim_players_only", aim_players_only()),
        ("aim_drops", aim_drops()),
        ("aim_predicts", aim_predicts()),
    ]
}

/// The dev soldier (a blaster pistol in the right hand, a vibroblade in the
/// left) with `delivery` in the right hand instead, or nothing there.
fn holding(delivery: Option<Delivery>) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    match delivery {
        Some(delivery) => {
            if let Some(weapon) = sheet.weapons[0].as_mut() {
                weapon.delivery = delivery;
            }
        }
        None => sheet.weapons[0] = None,
    }
    sheet
}

/// The way the player faces: along the proving ground's wall across, from
/// its entry, where the room is longest (6 m clear).
const YAW: f32 = crate::determinism::TURN_DEGREES + 90.0;
/// How long a walk or a run is measured: a run this long stays clear of the
/// far wall.
const MOVE_MS: u32 = 700;

/// A command looking down the room with `buttons` pressed and `held` held.
fn keys(buttons: u16, held: u8) -> Command {
    Command {
        angles: crate::moves::view_angles(2.0, YAW),
        buttons,
        holds: held,
        ..Command::default()
    }
}

/// [Aim] held, nothing else.
fn aiming() -> Command {
    keys(0, holds::AIM)
}

/// [Aim] held with the stick pushed fully forward.
fn aim_forward() -> Command {
    Command {
        forward: 127,
        ..aiming()
    }
}

/// [Walk] held with the stick as far forward as a walk sends.
fn walk_forward() -> Command {
    Command {
        forward: WALK_AXIS,
        ..keys(buttons::WALKING, 0)
    }
}

/// The player on the proving ground with its weapon drawn, stepped one
/// command at a time.
struct Ground {
    world: SimWorld,
    reason: StepReason,
    ps: MotionState,
    fighter: Fighter,
    now: u64,
}

impl Ground {
    fn new(sheet: Sheet, reason: StepReason) -> Result<Ground, String> {
        let config = WorldConfig {
            sheet,
            ..WorldConfig::one_seat()
        };
        let level = crate::determinism::proving_ground();
        let world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
        let mut ground = Ground {
            world,
            reason,
            ps: MotionState::standing([0.0; 3], 0.0),
            fighter: Fighter::fresh(1, 0),
            now: 0,
        };
        ground.poke(|fighter, _| fighter.sheath = Sheath::Drawn);
        ground.hold(MSEC, keys(0, 0))?;
        Ok(ground)
    }

    fn drawn(sheet: Sheet) -> Result<Ground, String> {
        Ground::new(sheet, StepReason::Authority)
    }

    /// `ms` of steps with `command`; `each` sees the player after each.
    fn watch(
        &mut self,
        ms: u32,
        command: Command,
        mut each: impl FnMut(&MotionState, &Fighter),
    ) -> Result<(), String> {
        for _ in 0..ms / MSEC {
            let tick = self.world.tick() + 1;
            let mut input = TickInput::default();
            input.insert(Seat(0), command);
            let snapshot = crate::step(&mut self.world, tick, &input, MSEC, self.reason);
            let me = snapshot.body(Seat(0)).ok_or("no player")?;
            self.ps = me.player.ok_or("no body")?;
            self.fighter = me.fighter.ok_or("no fighter")?;
            self.now = snapshot.time_ms;
            each(&self.ps, &self.fighter);
        }
        Ok(())
    }

    fn hold(&mut self, ms: u32, command: Command) -> Result<(), String> {
        self.watch(ms, command, |_, _| ())
    }

    /// Changes the player's fighter between steps (`now` is the last step's
    /// time).
    fn poke(&mut self, change: impl Fn(&mut Fighter, u64)) {
        let now = self.now;
        let mut query = self.world.world.query::<&mut Fighting>();
        for mut fighting in query.iter_mut(&mut self.world.world) {
            change(&mut fighting.0, now);
        }
    }

    fn aim(&self) -> u32 {
        self.fighter.shooting.aim
    }

    /// The spread the crosshair shows for the player's next shot.
    fn cone(&self, blaster: Blaster) -> f32 {
        shot_cone(&self.ps, &self.fighter.shooting, blaster, self.now)
    }

    /// How far (units, over the floor) `command` held for `ms` carries
    /// the player, after the same command held still for 300 ms.
    fn covers(&mut self, ms: u32, command: Command) -> Result<f32, String> {
        let still = Command {
            forward: 0,
            ..command
        };
        self.hold(300, still)?;
        let from = self.ps.origin;
        self.hold(ms, command)?;
        Ok(math::flat_distance(self.ps.origin, from))
    }
}

fn armed(blaster: Blaster) -> Result<Ground, String> {
    Ground::drawn(holding(Some(Delivery::Blaster(blaster))))
}

fn pistol() -> Result<Ground, String> {
    armed(Blaster::Pistol)
}

/// The gun the spread probes hold: the repeating blaster, whose base spread
/// is not 0 (the pistol's is), so the aiming stance shows on it.
const SPREADING: Blaster = Blaster::Repeater;

/// Held 300 ms, the gun's spread is its base times the aiming stance. Held
/// 100 ms, the gun is not yet fully raised and the spread is the base.
fn aim_spread() -> Result<String, String> {
    let base = weapons::primary(SPREADING).spread;
    let aimed = base * rules::weapons::bloom::STANCE[1];
    let mut ground = armed(SPREADING)?;
    ground.hold(100, aiming())?;
    let rising = ground.cone(SPREADING);
    ground.hold(200, aiming())?;
    let up = ground.cone(SPREADING);
    check(
        (rising - base).abs() < 0.001 && (up - aimed).abs() < 0.001,
        format!(
            "after 100 ms {rising:.3}° (base {base:.3}°); after 300 ms {up:.3}° (wanted {aimed:.3}°)"
        ),
    )
}

/// Walking while aimed, the spread is the base and the walking spread,
/// times the aiming stance.
fn aim_walk_spread() -> Result<String, String> {
    let base = weapons::primary(SPREADING).spread;
    let walking = rules::weapons::bloom::PACE_SPREAD[0];
    let wanted = (base + walking) * rules::weapons::bloom::STANCE[1];
    let mut ground = armed(SPREADING)?;
    ground.hold(MOVE_MS, aim_forward())?;
    let cone = ground.cone(SPREADING);
    check(
        ground.fighter.shooting.aiming && (cone - wanted).abs() < 0.001,
        format!(
            "aiming {}, {:.0} units/s: {cone:.3}°, wanted {wanted:.3}°",
            ground.fighter.shooting.aiming,
            math::flat_length(ground.ps.velocity)
        ),
    )
}

/// The most of a plain run's distance an aimed walk may cover. A held
/// [Walk] from standing covers a little over half of what a run does in
/// `MOVE_MS`, because a run takes longer to reach its speed.
const WALK_SHARE: f32 = 0.65;

/// Aiming with the stick fully forward covers what a held [Walk] does in
/// the same time (within 3%), and less than `WALK_SHARE` of a plain run.
fn aim_walks() -> Result<String, String> {
    let aimed = pistol()?.covers(MOVE_MS, aim_forward())?;
    let walked = pistol()?.covers(MOVE_MS, walk_forward())?;
    let run = pistol()?.covers(
        MOVE_MS,
        Command {
            forward: 127,
            ..keys(0, 0)
        },
    )?;
    check(
        (aimed / walked - 1.0).abs() <= 0.03 && aimed < run * WALK_SHARE,
        format!("aimed {aimed:.1} units, walked {walked:.1}, ran {run:.1}"),
    )
}

/// Whether the body was off the ground on any step of `ms` of `command`,
/// after `held` alone held for 100 ms.
fn leaves_ground(held: u8, command: Command, ms: u32) -> Result<bool, String> {
    let mut ground = pistol()?;
    ground.hold(100, keys(0, held))?;
    let mut left = false;
    ground.watch(ms, command, |ps, _| left |= ps.ground.is_none())?;
    Ok(left)
}

/// Aiming, then holding jump as well for 500 ms, the body stays on the
/// ground. Without aim, the same jump leaves it. The aim is held for 100 ms
/// first: the walk limit begins on the step after the gun starts to rise.
fn aim_no_jump() -> Result<String, String> {
    let jump = |held| Command {
        up: 127,
        ..keys(0, held)
    };
    let aimed = leaves_ground(holds::AIM, jump(holds::AIM), 500)?;
    let plain = leaves_ground(0, jump(0), 500)?;
    check(
        !aimed && plain,
        format!("left the ground aiming: {aimed}; without aim: {plain}"),
    )
}

/// What the legs did when, with `held` held, the player ran forward for
/// 600 ms and then pressed crouch for 300 ms.
struct Legs {
    /// The legs played a roll.
    rolled: bool,
    /// The legs played a run cycle.
    ran: bool,
    /// The fastest the body went over the floor, units a second.
    fastest: f32,
}

fn legs_of(held: u8) -> Result<Legs, String> {
    let mut ground = pistol()?;
    let run = Command {
        forward: 127,
        ..keys(0, held)
    };
    let mut legs = Legs {
        rolled: false,
        ran: false,
        fastest: 0.0,
    };
    let mut note = |ps: &MotionState, _: &Fighter| {
        legs.fastest = legs.fastest.max(math::flat_length(ps.velocity));
        legs.ran |= crate::motion::anim::is_run(ps.legs.anim);
        legs.rolled |= matches!(
            ps.legs.anim,
            Anim::BOTH_ROLL_F | Anim::BOTH_ROLL_B | Anim::BOTH_ROLL_L | Anim::BOTH_ROLL_R
        );
    };
    ground.watch(600, run, &mut note)?;
    ground.watch(300, Command { up: -127, ..run }, &mut note)?;
    Ok(legs)
}

/// Aiming, running forward and pressing crouch never rolls. A roll starts
/// only from a run cycle at `movement.rolls.from_speed_squared` or faster.
/// Aiming, the body never reaches that speed. Without aim, the same keys
/// reach both. On the proving ground the plain keys do not start a roll
/// either, so the probe checks the roll's speed condition instead.
fn aim_no_roll() -> Result<String, String> {
    let roll_speed = rules::movement::rolls::FROM_SPEED_SQUARED.sqrt();
    let (aimed, plain) = (legs_of(holds::AIM)?, legs_of(0)?);
    check(
        !aimed.rolled && aimed.fastest < roll_speed && plain.ran && plain.fastest >= roll_speed,
        format!(
            "aiming: rolled {}, ran {}, at most {:.0} units/s; without aim: ran {}, at most \
             {:.0} units/s; a roll starts from {roll_speed:.0} units/s",
            aimed.rolled, aimed.ran, aimed.fastest, plain.ran, plain.fastest
        ),
    )
}

/// How far `command` carries the pistol's player in `MOVE_MS`, with Force
/// Speed running or not.
fn covers_with_speed(command: Command, speed: bool) -> Result<f32, String> {
    let mut ground = pistol()?;
    if speed {
        ground.poke(|fighter, now| {
            fighter.force.speed_until = now + 60_000;
            fighter.force.speed_level = 1;
        });
    }
    ground.covers(MOVE_MS, command)
}

/// Force Speed makes the aimed walk faster by the same factor (within 3%)
/// as it makes a held [Walk]'s walk faster.
fn aim_force_speed() -> Result<String, String> {
    let factor = |command| -> Result<f32, String> {
        Ok(covers_with_speed(command, true)? / covers_with_speed(command, false)?)
    };
    let (aimed, walked) = (factor(aim_forward())?, factor(walk_forward())?);
    check(
        (aimed / walked - 1.0).abs() <= 0.03,
        format!("Force Speed: aimed walk ×{aimed:.3}, held [Walk] ×{walked:.3}"),
    )
}

const BLASTERS: [Blaster; 7] = [
    Blaster::Pistol,
    Blaster::Rifle,
    Blaster::Repeater,
    Blaster::Bowcaster,
    Blaster::Disruptor,
    Blaster::Ion,
    Blaster::Sonic,
];

/// The player's (aim, aiming) after `ms` of [Aim] held with `delivery` in
/// hand, or nothing.
fn aimed_with(delivery: Option<Delivery>, ms: u32) -> Result<(u32, bool), String> {
    let mut ground = Ground::drawn(holding(delivery))?;
    ground.hold(ms, aiming())?;
    Ok((ground.aim(), ground.fighter.shooting.aiming))
}

/// Every blaster is fully raised after `weapons.aim.in_ms` of [Aim] held.
fn aim_blasters() -> Result<String, String> {
    let seen = BLASTERS
        .iter()
        .map(|&blaster| {
            let (_, up) = aimed_with(Some(Delivery::Blaster(blaster)), rules::weapons::aim::IN_MS)?;
            Ok((blaster, up))
        })
        .collect::<Result<Vec<_>, String>>()?;
    check(
        seen.iter().all(|(_, up)| *up),
        format!("aiming after {} ms: {seen:?}", rules::weapons::aim::IN_MS),
    )
}

/// A lightsaber, a vibroblade and bare hands never aim.
fn aim_not_blades() -> Result<String, String> {
    let hands = [
        ("lightsaber", Some(Delivery::Saber(SaberShape::Single))),
        ("vibroblade", Some(Delivery::Melee)),
        ("bare hands", None),
    ];
    let seen = hands
        .iter()
        .map(|&(name, delivery)| Ok((name, aimed_with(delivery, 300)?.0)))
        .collect::<Result<Vec<_>, String>>()?;
    check(
        seen.iter().all(|(_, aim)| *aim == 0),
        format!("aim after 300 ms held: {seen:?}"),
    )
}

/// A creature whose command holds [Aim] with the stick forward and jump
/// held: its gun stays lowered, and its command keeps its run and jump.
/// The same fighter played by a seat raises its gun.
fn aim_players_only() -> Result<String, String> {
    let held = Command {
        forward: 127,
        up: 127,
        ..aiming()
    };
    let mut creature = Fighter::fresh(10, 0);
    creature.last_command = held;
    let mut player = creature;
    for _ in 0..300 / MSEC {
        super::firing::raise_to_aim(&mut creature, true, MSEC);
        super::firing::raise_to_aim(&mut player, false, MSEC);
    }
    let ps = MotionState::standing([0.0; 3], 0.0);
    let command = super::command_for(&TickInput::default(), None, &ps, &creature, 0);
    check(
        creature.shooting.aim == 0 && command == held && player.shooting.aiming,
        format!(
            "creature: aim {}, command {command:?}; player aiming {}",
            creature.shooting.aim, player.shooting.aiming
        ),
    )
}

/// One way to lower an aimed gun, done to the fighter between steps, or
/// by the command held for the next step.
type Drop = (&'static str, fn(&mut Fighter, u64), Command);

fn untouched(_: &mut Fighter, _: u64) {}

/// The ways a raised gun is lowered at once: the weapon switched to the
/// left hand's vibroblade, the gun holstered, a stun, a knockdown, a medpac
/// used.
fn drops() -> [Drop; 5] {
    let switch = Command {
        weapon: 1,
        ..aiming()
    };
    let medpac = keys(buttons::MEDPAC, holds::AIM);
    [
        ("weapon switched", untouched, switch),
        (
            "holstered",
            |fighter, _| fighter.sheath = Sheath::Holstered,
            aiming(),
        ),
        (
            "stunned",
            |fighter, now| fighter.force.stunned_until = now + 5000,
            aiming(),
        ),
        (
            "knocked down",
            |fighter, now| fighter.saber.knock_down(now, 2000),
            aiming(),
        ),
        (
            "medpac used",
            |fighter, _| fighter.hp = fighter.hp.min(100),
            medpac,
        ),
    ]
}

/// Aimed, then each of `drops`: on the next step the gun is down and the
/// walk limit gone.
fn aim_drops() -> Result<String, String> {
    let seen = drops()
        .iter()
        .map(|&(name, change, command)| {
            let mut ground = pistol()?;
            ground.hold(300, aiming())?;
            let up = ground.fighter.shooting.aiming;
            ground.poke(change);
            ground.hold(MSEC, command)?;
            Ok((name, up, ground.aim(), ground.fighter.shooting.slowed()))
        })
        .collect::<Result<Vec<_>, String>>()?;
    check(
        seen.iter()
            .all(|&(_, up, aim, slowed)| up && aim == 0 && !slowed),
        format!("(how, aimed before, aim after, slowed after): {seen:?}"),
    )
}

/// The same 400 ms of [Aim] pressed, let go and pressed again, stepped as
/// the authority and as a prediction for the avatar: the aim is the same
/// on every step.
fn aim_predicts() -> Result<String, String> {
    let sheet = || holding(Some(Delivery::Blaster(Blaster::Pistol)));
    let mut worlds = [
        Ground::new(sheet(), StepReason::Authority)?,
        Ground::new(sheet(), StepReason::Predict)?,
    ];
    let mut differ = Vec::new();
    for step in 0..400 / MSEC {
        let command = if (10..20).contains(&step) {
            keys(0, 0)
        } else {
            aiming()
        };
        for ground in &mut worlds {
            ground.hold(MSEC, command)?;
        }
        let [authority, predicted] = [worlds[0].aim(), worlds[1].aim()];
        if authority != predicted {
            differ.push((step, authority, predicted));
        }
    }
    check(
        differ.is_empty() && worlds[0].fighter.shooting.aiming,
        format!(
            "steps that differ (step, authority, predicted): {differ:?}; aiming at the end {}",
            worlds[0].fighter.shooting.aiming
        ),
    )
}

/// 25 steps of 8 ms held raise the gun fully, and 24 do not. 19 steps of
/// 8 ms after the release lower it fully, and 18 do not.
fn aim_level() -> Result<String, String> {
    let mut s = Shooting::default();
    for _ in 0..24 {
        s.aim_step(true, MSEC);
    }
    let rising = !s.aiming && s.aim > 0 && s.slowed();
    let risen_at = s.aim;
    s.aim_step(true, MSEC);
    let up = s.aiming && s.aim == Shooting::AIM_FULL && s.aim_share() == 1.0;
    for _ in 0..18 {
        s.aim_step(false, MSEC);
    }
    let lowering = s.aiming;
    let lowered_at = s.aim;
    s.aim_step(false, MSEC);
    let down = !s.aiming && s.aim == 0 && !s.slowed();
    check(
        rising && up && lowering && down,
        format!(
            "after 24 steps held: aim {risen_at}; after 25: up {up}; \
             after 18 released: aim {lowered_at}, aiming {lowering}; after 19: down {down}"
        ),
    )
}

fn aimed_sample() -> Shooting {
    Shooting {
        aim: 12345,
        aiming: true,
        steadying: true,
        ..Shooting::default()
    }
}

fn aim_round_trips() -> Result<String, String> {
    let mut out = Writer::default();
    aimed_sample().encode(&mut out);
    let read = Shooting::decode(&mut Reader::new("probe", &out.0));
    check(read.as_ref() == Ok(&aimed_sample()), format!("{read:?}"))
}

/// The bytes a build before hold to aim wrote, with the old Sniper Shot's
/// aim flag set, read as a lowered gun and no steadying.
fn old_snapshot_aim() -> Result<String, String> {
    let mut out = Writer::default();
    Shooting {
        aiming: true,
        ..Shooting::default()
    }
    .encode(&mut out);
    let versions = Versions {
        snapshot: crate::since::snapshot::FEAT_GUNPLAY_AIM.1 - 1,
        ..Versions::THIS_BUILD
    };
    // The older layout has no aim at the end.
    let cut = out.0.len() - 4;
    let mut input = Reader::new("probe", &out.0[..cut]).with_versions(versions);
    let read = Shooting::decode(&mut input).map(|s| (s.aim, s.aiming, s.steadying));
    let ok = read == Ok((0, false, false)) && input.finish().is_ok();
    check(
        ok,
        format!("older snapshot reads (aim, aiming, steadying) {read:?}"),
    )
}

fn interrupt_drops_aim() -> Result<String, String> {
    let mut s = aimed_sample();
    s.interrupt();
    check(
        s.aim == 0 && !s.aiming && !s.steadying,
        format!(
            "after interrupt: aim {}, aiming {}, steadying {}",
            s.aim, s.aiming, s.steadying
        ),
    )
}

/// `alt` holds the alternate attack and the aim. `feat` holds the feat
/// attack and not the alternate attack.
fn moves_words() -> Result<String, String> {
    let held = |text: &str| Script::parse(text).map(|s| s.command(0));
    let (alt, feat) = (held("1s alt")?, held("1s feat")?);
    let ok = alt.buttons & buttons::ALT_ATTACK != 0
        && alt.holds & holds::AIM != 0
        && feat.holds & holds::FEAT != 0
        && feat.buttons & buttons::ALT_ATTACK == 0;
    check(
        ok,
        format!(
            "alt: buttons {:#x}, holds {:#x}; feat: buttons {:#x}, holds {:#x}",
            alt.buttons, alt.holds, feat.buttons, feat.holds
        ),
    )
}

fn other_mode() -> Result<String, String> {
    let mut command = Command::default();
    command.other_mode();
    check(
        command.buttons & buttons::ALT_ATTACK != 0 && command.holds & holds::FEAT != 0,
        format!("buttons {:#x}, holds {:#x}", command.buttons, command.holds),
    )
}

/// Each camera's place, turned into an aim, lies within a step of the
/// crosshair line that aim gives (the line runs along the view through
/// it).
fn round_trip() -> Result<String, String> {
    let mut ps = crate::motion::MotionState::standing([0.0, 0.0, 24.0], 30.0);
    ps.view = [12.0, 30.0, 0.0];
    let (forward, ..) = math::angle_vectors(ps.view);
    let line = |at: Vec3| {
        mind::crosshair(&crate::motion::MotionState {
            command_aim: mind::aim_for(&ps, at),
            ..ps
        })
    };
    let off_line = |camera: Vec3| {
        let from = line(camera);
        let off = math::sub(camera, from);
        math::length(math::mad(off, -math::dot(off, forward), forward))
    };
    let base = mind::crosshair(&ps);
    let cameras = [
        ("third person", math::mad(base, -80.0, forward)),
        ("own eyes", math::add(base, [1.3, -0.8, -9.2])),
        ("far off", math::add(base, [30.0, -20.0, 40.0])),
    ];
    let misses: Vec<(&str, f32)> = cameras
        .iter()
        .map(|&(name, at)| (name, off_line(at)))
        .collect();
    // A step on each axis, rounded: half a step's diagonal at most.
    let most = mind::AIM_STEP * 0.5 * 3f32.sqrt();
    check(
        misses.iter().all(|(_, miss)| *miss <= most),
        format!("off the camera's line (units, at most {most:.2}): {misses:?}"),
    )
}

/// Fires the player's pistol with `aim` (facing +X): its first bolt's
/// way, the point its line meets the level, and the bolt's start.
fn fire(aim: [i8; 3]) -> Result<(Vec3, Vec3, Vec3), String> {
    let level = crate::determinism::proving_ground();
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let me = world
        .snapshot()
        .own(Seat(0))
        .map(|entity| entity.id)
        .ok_or("no player")?;
    let command = Command {
        angles: crate::moves::view_angles(2.0, 0.0),
        buttons: buttons::ATTACK,
        aim,
        ..Command::default()
    };
    for tick in 1..400 {
        let mut input = TickInput::default();
        input.insert(Seat(0), command);
        let snapshot = crate::step(&mut world, tick, &input, MSEC, StepReason::Authority);
        let bolt = snapshot
            .entities
            .iter()
            .find_map(|entity| entity.missile.filter(|missile| missile.owner == me));
        let ps = snapshot.own(Seat(0)).and_then(|entity| entity.player);
        if let (Some(bolt), Some(ps)) = (bolt, ps) {
            let from = mind::crosshair(&ps);
            let (forward, ..) = math::angle_vectors(ps.view);
            let far = math::mad(from, crate::combat::weapons::HITSCAN_RANGE, forward);
            let states = world.level().states_in(&snapshot);
            let met = world.level().with(states.as_deref(), |level| {
                level.trace(
                    math::scale(from, UNIT),
                    math::scale(far, UNIT),
                    &Extents::POINT,
                    Contents::SOLID,
                )
            });
            return Ok((bolt.velocity, math::scale(met.end, 1.0 / UNIT), bolt.base));
        }
    }
    Err("no bolt fired".into())
}

/// The angle between two ways (degrees).
fn between(mut a: Vec3, mut b: Vec3) -> f32 {
    math::normalize(&mut a);
    math::normalize(&mut b);
    math::dot(a, b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// With the own eyes' aim and with none, the bolt flies from the gun at
/// where its line meets the level (within 0.05°), and the lines meet it
/// apart.
fn bolt() -> Result<String, String> {
    let (eyes_way, eyes_point, eyes_from) = fire(EYES)?;
    let (third_way, third_point, third_from) = fire([0; 3])?;
    let eyes_off = between(eyes_way, math::sub(eyes_point, eyes_from));
    let third_off = between(third_way, math::sub(third_point, third_from));
    let apart = math::length(math::sub(eyes_point, third_point));
    check(
        eyes_off < 0.05 && third_off < 0.05 && apart > 8.0,
        format!(
            "own eyes: {eyes_off:.3}° off its line's point; third person: {third_off:.3}°; the \
             points {apart:.1} units apart"
        ),
    )
}
