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
//!
//! And the probes of the feat attack button and Sniper Shot, on the proving
//! ground:
//! - [Feat attack] fires a blaster's feat attack. [Alt attack] fires
//!   nothing with a blaster.
//! - [Feat attack] held together with [Attack] fires no plain shot.
//! - A Sniper Shot steadied long enough fires one shot on the release. The
//!   shot has no spread, costs one feat attack's stamina, and lowers the
//!   shooter's Defense.
//! - A Sniper Shot let go too soon fires nothing and costs nothing. It
//!   steadies faster while the gun is aimed.
//! - A Sniper Shot can steady while the gun cools down after a shot. Let
//!   go once steady, it leaves on the first step the gun is ready, so
//!   chained Sniper Shots leave at the gun's own rate. Let go too soon, it
//!   fires nothing and costs nothing. A shot let go once steady stays
//!   steady while it waits, even when the gun leaves the aimed state.
//! - [Next feat attack] pressed during a Sniper Shot's steadying, while a
//!   steady shot waits for the gun, or during a Power Blast's charge ends
//!   it. Nothing fires from it, and nothing is paid twice.
//! - A character steadying a Sniper Shot walks and never jumps. A stun or
//!   a weapon switch, the holster, a knockdown or a medpac ends the
//!   steadying, also while a steady shot waits for the gun.
//! - Without the stamina for a feat attack, [Feat attack] fires as
//!   [Attack] does.
//! - A creature's mind fires Sniper Shots without ever aiming.
//! - A feat attack chosen from the action bar is the one [Feat attack]
//!   fires.
//! - [Feat attack] counts as an attack everywhere an attack matters. A
//!   companion whose mind holds the other mode fires nothing while a
//!   conversation holds the world. [Feat attack] alone ends stealth mode,
//!   breaks off a revive and a walk the target block steers, and the
//!   scripts see it as an attack. With a grenade readied, it does not raise
//!   the holstered gun.

use super::{Fighting, SimWorld};
use crate::action::Action;
use crate::combat::bar::FeatLine;
use crate::combat::bloom::shot_cone;
use crate::combat::missiles::Missile;
use crate::combat::picks::Picks;
use crate::combat::sheet::{Blaster, Delivery, SaberShape, feats};
use crate::combat::specials::{self, Shooting, Special};
use crate::combat::{Fighter, Sheath, Sheet, mind, weapons};
use crate::input::{Command, Seat, TickInput, buttons, holds};
use crate::math::{self, Vec3};
use crate::motion::{Anim, MotionState};
use crate::moves::Script;
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;
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
        ("feat_button", feat_button()),
        ("feat_over_attack", feat_over_attack()),
        ("steady_fires", steady_fires()),
        ("steady_early", steady_early()),
        ("steady_in_refire", steady_in_refire()),
        ("steady_chained", steady_chained()),
        ("steady_early_refire", steady_early_refire()),
        ("steady_lowered", steady_lowered()),
        ("feat_change", feat_change()),
        ("steady_aimed", steady_aimed()),
        ("steady_straight", steady_straight()),
        ("steady_walks", steady_walks()),
        ("steady_broke", steady_broke()),
        ("steady_drops", steady_drops()),
        ("mind_snipes", mind_snipes()),
        ("bar_feat_slot", bar_feat_slot()),
        ("feat_held_scene", super::scene::probes::party_feat_held()),
        ("feat_ends_stealth", feat_ends_stealth()),
        ("feat_breaks_revive", feat_breaks_revive()),
        ("feat_attacking", feat_attacking()),
        ("feat_grenade_readied", feat_grenade_readied()),
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
    /// The bolts the player fired, in the order they left.
    fired: Vec<Missile>,
    /// The last step's snapshot.
    last: Option<Snapshot>,
}

impl Ground {
    fn new(sheet: Sheet, reason: StepReason) -> Result<Ground, String> {
        Ground::on(crate::determinism::proving_ground(), sheet, reason)
    }

    /// The player as `sheet` on `level`.
    fn on(level: super::Level, sheet: Sheet, reason: StepReason) -> Result<Ground, String> {
        let config = WorldConfig {
            sheet,
            ..WorldConfig::one_seat()
        };
        let world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
        let mut ground = Ground {
            world,
            reason,
            ps: MotionState::standing([0.0; 3], 0.0),
            fighter: Fighter::fresh(1, 0),
            now: 0,
            fired: Vec::new(),
            last: None,
        };
        ground.poke(|fighter, _| fighter.sheath = Sheath::Drawn);
        ground.hold(MSEC, keys(0, 0))?;
        Ok(ground)
    }

    fn drawn(sheet: Sheet) -> Result<Ground, String> {
        Ground::new(sheet, StepReason::Authority)
    }

    /// One step with `command`, and `action` sent with it if there is one.
    fn step(&mut self, command: Command, action: Option<Action>) -> Result<(), String> {
        let tick = self.world.tick() + 1;
        let mut input = TickInput::default();
        input.insert(Seat(0), command);
        if let Some(action) = action {
            input.act(Seat(0), action);
        }
        let snapshot = crate::step(&mut self.world, tick, &input, MSEC, self.reason);
        let me = snapshot.body(Seat(0)).ok_or("no player")?;
        self.ps = me.player.ok_or("no body")?;
        self.fighter = me.fighter.ok_or("no fighter")?;
        self.now = snapshot.time_ms;
        let id = me.id;
        self.fired.extend(
            snapshot
                .entities
                .iter()
                .filter_map(|entity| entity.missile)
                .filter(|bolt| bolt.owner == id && bolt.from_ms == snapshot.time_ms),
        );
        self.last = Some(snapshot);
        Ok(())
    }

    /// `ms` of steps with `command`; `each` sees the player after each.
    fn watch(
        &mut self,
        ms: u32,
        command: Command,
        mut each: impl FnMut(&MotionState, &Fighter),
    ) -> Result<(), String> {
        for _ in 0..ms / MSEC {
            self.step(command, None)?;
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

    /// Where the player's crosshair line meets the level after the last
    /// step.
    fn crosshair_point(&self) -> Result<Vec3, String> {
        let snapshot = self.last.as_ref().ok_or("no step yet")?;
        let from = mind::crosshair(&self.ps);
        let (forward, ..) = math::angle_vectors(self.ps.view);
        let far = math::mad(from, weapons::HITSCAN_RANGE, forward);
        let level = self.world.level();
        let states = level.states_in(snapshot);
        let met = level.with(states.as_deref(), |level| {
            level.trace(
                math::scale(from, UNIT),
                math::scale(far, UNIT),
                &Extents::POINT,
                Contents::SOLID,
            )
        });
        Ok(math::scale(met.end, 1.0 / UNIT))
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
fn leaves_ground(mut ground: Ground, held: u8, command: Command, ms: u32) -> Result<bool, String> {
    ground.hold(100, keys(0, held))?;
    let mut left = false;
    ground.watch(ms, command, |ps, _| left |= ps.ground.is_none())?;
    Ok(left)
}

/// Aiming for 100 ms, then holding jump as well for 500 ms, the body stays
/// on the ground. Aim and jump pressed on the same step keep it on the
/// ground too. Without aim, the same jump leaves it.
fn aim_no_jump() -> Result<String, String> {
    let jump = |held| Command {
        up: 127,
        ..keys(0, held)
    };
    let aimed = leaves_ground(pistol()?, holds::AIM, jump(holds::AIM), 500)?;
    let together = leaves_ground(pistol()?, 0, jump(holds::AIM), 500)?;
    let plain = leaves_ground(pistol()?, 0, jump(0), 500)?;
    check(
        !aimed && !together && plain,
        format!(
            "left the ground aiming: {aimed}; pressing aim and jump together: {together}; \
             without aim: {plain}"
        ),
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
    let sheet = Sheet::dev_soldier();
    let mut creature = Fighter::fresh(10, 0);
    creature.last_command = held;
    creature.sheath = Sheath::Drawn;
    let mut player = creature;
    for _ in 0..300 / MSEC {
        super::firing::raise_to_aim(&mut creature, &sheet, true, 0, MSEC);
        super::firing::raise_to_aim(&mut player, &sheet, false, 0, MSEC);
    }
    let ps = MotionState::standing([0.0; 3], 0.0);
    let command = super::command_for(&TickInput::default(), None, &ps, &creature, 0);
    let command = super::slowed_to_aim(command, &creature, None, 0);
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

/// How long a Sniper Shot steadies: from the hip, and while aimed.
const STEADY_MS: [u64; 2] = rules::feats::specials::sniper_shot::STEADY_MS;

/// A hold a little over `ms`, two steps past it.
fn past(ms: u64) -> u32 {
    (ms as u32).next_multiple_of(MSEC) + 2 * MSEC
}

/// A hold well short of `ms`, three fifths of it.
fn short_of(ms: u64) -> u32 {
    (ms * 3 / 5) as u32
}

/// [Feat attack] held with `buttons` pressed.
fn feat(buttons: u16) -> Command {
    keys(buttons, holds::FEAT)
}

/// Nothing pressed.
fn idle() -> Command {
    keys(0, 0)
}

/// The dev soldier with `blaster` in the right hand, knowing the feat rows
/// `known`.
fn trained(blaster: Blaster, known: &[u16]) -> Sheet {
    let mut sheet = holding(Some(Delivery::Blaster(blaster)));
    sheet.feats = known.iter().fold(sheet.feats, |rows, row| rows.with(*row));
    sheet
}

/// The player with `blaster` and the first Sniper Shot feat.
fn sniping(blaster: Blaster) -> Sheet {
    trained(blaster, &feats::SNIPER_SHOT[..1])
}

/// The player with a blaster pistol and the first Power Blast feat.
fn blasting() -> Sheet {
    trained(Blaster::Pistol, &feats::POWER_BLAST[..1])
}

/// Whether `bolt` is a Sniper Shot from the right hand's gun of `sheet`:
/// its critical threat range is the one Sniper Shot gives that gun.
fn sniped(bolt: &Missile, sheet: &Sheet) -> bool {
    sheet.weapons[0].is_some_and(|plain| {
        bolt.weapon.crit_threat == Special::SniperShot(1).shot(plain).crit_threat
    })
}

/// A pistol with Power Blast: [Feat attack] held 1.1 s and let go fires
/// one Power Blast. [Alt attack] held as long fires nothing.
fn feat_button() -> Result<String, String> {
    let fired = |held: Command| -> Result<Vec<Missile>, String> {
        let mut ground = Ground::drawn(blasting())?;
        ground.hold(1100, held)?;
        ground.hold(200, idle())?;
        Ok(ground.fired)
    };
    let (feat, alt) = (fired(feat(0))?, fired(keys(buttons::ALT_ATTACK, 0))?);
    check(
        matches!(&feat[..], [blast] if blast.power) && alt.is_empty(),
        format!(
            "[Feat attack]: {} bolts, Power Blast {:?}; [Alt attack]: {} bolts",
            feat.len(),
            feat.iter().map(|bolt| bolt.power).collect::<Vec<_>>(),
            alt.len()
        ),
    )
}

/// A pistol with Power Blast: [Attack] and [Feat attack] held together for
/// 600 ms and let go fire no plain shot, only the Power Blast.
fn feat_over_attack() -> Result<String, String> {
    let mut ground = Ground::drawn(blasting())?;
    ground.hold(600, feat(buttons::ATTACK))?;
    ground.hold(200, idle())?;
    let plain = ground.fired.iter().filter(|bolt| !bolt.power).count();
    let blasts = ground.fired.len() - plain;
    check(
        plain == 0 && blasts == 1,
        format!("{plain} plain bolts, {blasts} Power Blasts"),
    )
}

/// A pistol with Sniper Shot: [Feat attack] held a little past the
/// steadying time and let go fires exactly one Sniper Shot. Nothing is paid
/// while the shot steadies. The release pays one feat attack's cost and
/// lowers the shooter's Defense by the rule's penalty.
fn steady_fires() -> Result<String, String> {
    let sheet = sniping(Blaster::Pistol);
    let cost = specials::cost(&sheet);
    let mut ground = Ground::drawn(sheet)?;
    ground.hold(200, idle())?;
    let before = ground.fighter.fp;
    ground.hold(past(STEADY_MS[0]), feat(0))?;
    let held = ground.fighter.fp;
    ground.hold(MSEC, idle())?;
    let spent = held - ground.fighter.fp;
    let (_, defence) = ground.fighter.shooting.exposed(ground.now);
    let wanted = rules::feats::specials::sniper_shot::DEFENCE_PENALTY;
    check(
        matches!(&ground.fired[..], [bolt] if sniped(bolt, &sheet))
            && held == before
            && spent == cost
            && cost > 0
            && defence == wanted,
        format!(
            "{} bolts, {} Sniper Shots; stamina {before} before, {held} while held, \
             {spent} spent on the release (cost {cost}); Defense {defence} (wanted {wanted})",
            ground.fired.len(),
            ground
                .fired
                .iter()
                .filter(|bolt| sniped(bolt, &sheet))
                .count()
        ),
    )
}

/// A Sniper Shot let go well before it is steady fires nothing. The
/// stamina and the weapon's timer are the same as before the hold.
fn steady_early() -> Result<String, String> {
    let mut ground = Ground::drawn(sniping(Blaster::Pistol))?;
    ground.hold(200, idle())?;
    let (pool, timer) = (ground.fighter.fp, ground.fighter.weapon_wait_ms);
    ground.hold(short_of(STEADY_MS[0]), feat(0))?;
    ground.hold(MSEC, idle())?;
    let after = (ground.fighter.fp, ground.fighter.weapon_wait_ms);
    ground.hold(200, idle())?;
    check(
        ground.fired.is_empty() && after == (pool, timer),
        format!(
            "{} bolts; (stamina, timer) {:?} before, {after:?} after the release",
            ground.fired.len(),
            (pool, timer)
        ),
    )
}

/// The player with a blaster pistol and Sniper Shot, after one plain shot:
/// the plain bolt is the only one fired.
fn after_plain_shot() -> Result<Ground, String> {
    let mut ground = Ground::drawn(sniping(Blaster::Pistol))?;
    ground.hold(200, idle())?;
    ground.hold(MSEC, keys(buttons::ATTACK, 0))?;
    match ground.fired.len() {
        1 => Ok(ground),
        bolts => Err(format!("{bolts} bolts from one press of [Attack]")),
    }
}

/// The time between two shots of `blaster` (ms).
fn refire(blaster: Blaster) -> u64 {
    u64::from(weapons::primary(blaster).refire_ms)
}

/// A pistol fires a plain shot. [Feat attack] is pressed on the next step,
/// while the gun is still cooling down, held a little past the steadying
/// time and let go before the gun is ready. The Sniper Shot leaves on the
/// first step the gun is ready, one refire after the plain shot. No stamina
/// is spent before it leaves.
fn steady_in_refire() -> Result<String, String> {
    let sheet = sniping(Blaster::Pistol);
    let mut ground = after_plain_shot()?;
    let pool = ground.fighter.fp;
    ground.hold(past(STEADY_MS[0]), feat(0))?;
    let released = ground.now;
    let mut spent_early = false;
    while ground.fired.len() < 2 && ground.now < released + 2000 {
        ground.hold(MSEC, idle())?;
        spent_early |= ground.fired.len() < 2 && ground.fighter.fp != pool;
    }
    let gap = match &ground.fired[..] {
        [plain, snipe] if !sniped(plain, &sheet) && sniped(snipe, &sheet) => {
            snipe.from_ms - plain.from_ms
        }
        other => return Err(format!("bolts {other:?}")),
    };
    let wanted = refire(Blaster::Pistol);
    let let_go = released - ground.fired[0].from_ms;
    check(
        let_go < wanted && gap.abs_diff(wanted) < u64::from(MSEC) && !spent_early,
        format!(
            "let go {let_go} ms after the plain shot; the Sniper Shot left {gap} ms after it \
             (refire {wanted} ms); stamina spent before it left: {spent_early}"
        ),
    )
}

/// A blaster rifle held up to aim fires Sniper Shots in a chain. [Feat
/// attack] is pressed again on the step after each shot and let go once the
/// shot is steady. The aimed steadying time is shorter than the rifle's
/// refire, so the shots leave one refire apart (within a step).
fn steady_chained() -> Result<String, String> {
    let sheet = sniping(Blaster::Rifle);
    let wanted = refire(Blaster::Rifle);
    let mut ground = Ground::drawn(sheet)?;
    ground.hold(300, aiming())?;
    for _ in 0..3000 / MSEC {
        if ground.fired.len() >= 3 {
            break;
        }
        let now = ground.now;
        let steady = ground
            .fighter
            .shooting
            .charging_from
            .is_some_and(|from| now >= from + STEADY_MS[1]);
        let feat = if steady { 0 } else { holds::FEAT };
        ground.hold(MSEC, keys(0, holds::AIM | feat))?;
    }
    let all_sniped = ground.fired.iter().all(|bolt| sniped(bolt, &sheet));
    let gaps: Vec<u64> = ground
        .fired
        .windows(2)
        .map(|pair| pair[1].from_ms - pair[0].from_ms)
        .collect();
    check(
        STEADY_MS[1] + 2 * u64::from(MSEC) < wanted
            && all_sniped
            && gaps.len() == 2
            && gaps
                .iter()
                .all(|gap| gap.abs_diff(wanted) < u64::from(MSEC)),
        format!(
            "{} Sniper Shots {gaps:?} ms apart; refire {wanted} ms, aimed steadying {} ms",
            ground.fired.len(),
            STEADY_MS[1]
        ),
    )
}

/// A pistol fires a plain shot, and a Sniper Shot steadies while the gun is
/// still cooling down. It is let go well before it is steady. Nothing more
/// fires in the next second, and no stamina is spent.
fn steady_early_refire() -> Result<String, String> {
    let mut ground = after_plain_shot()?;
    let pool = ground.fighter.fp;
    ground.hold(short_of(STEADY_MS[0]), feat(0))?;
    let steadied = ground.fighter.shooting.steadying;
    ground.hold(1000, idle())?;
    check(
        steadied && ground.fired.len() == 1 && ground.fighter.fp == pool,
        format!(
            "steadying before the release {steadied}; {} bolts; stamina {pool} before, {} after",
            ground.fired.len(),
            ground.fighter.fp
        ),
    )
}

/// A pistol held up to aim fires a plain shot. A Sniper Shot is steadied
/// during the refire a little past the aimed steadying time, and [Aim] and
/// [Feat attack] are let go together. The gun leaves the aimed state
/// before it is ready, and the Sniper Shot still leaves on the first step
/// the gun is ready.
fn steady_lowered() -> Result<String, String> {
    let sheet = sniping(Blaster::Pistol);
    let mut ground = Ground::drawn(sheet)?;
    ground.hold(300, aiming())?;
    ground.hold(MSEC, keys(buttons::ATTACK, holds::AIM))?;
    ground.hold(past(STEADY_MS[1]), keys(0, holds::AIM | holds::FEAT))?;
    let (aimed, released) = (ground.fighter.shooting.aiming, ground.now);
    let mut lowered = false;
    while ground.fired.len() < 2 && ground.now < released + 2000 {
        lowered |= !ground.fighter.shooting.aiming;
        ground.hold(MSEC, idle())?;
    }
    let shots: Vec<bool> = ground
        .fired
        .iter()
        .map(|bolt| sniped(bolt, &sheet))
        .collect();
    check(
        aimed && lowered && shots == [false, true],
        format!(
            "aimed at the release {aimed}; out of the aimed state before the gun was ready \
             {lowered}; bolts (Sniper Shot or not) {shots:?}"
        ),
    )
}

/// The player with a blaster pistol knowing Power Blast and Sniper Shot,
/// with `line` chosen.
fn both_lines(line: FeatLine) -> Sheet {
    let mut sheet = trained(
        Blaster::Pistol,
        &[feats::POWER_BLAST[0], feats::SNIPER_SHOT[0]],
    );
    Picks::set(&mut sheet, 0, line);
    sheet
}

/// The proving ground with an empty script library. A seat's actions run
/// with the scripts' step, which needs a library.
fn acting_level() -> super::Level {
    let library = crate::probe::library::<String>("void ActionWait(float fSeconds);", []);
    crate::determinism::proving_ground().with_scripts(library)
}

/// [Next feat attack] pressed in three states of a pistol that knows Power
/// Blast and Sniper Shot. In the first, a Sniper Shot steadies past its
/// time with [Feat attack] held, which stays held for 300 ms after the
/// press and is then let go. The press starts a new Power Blast charge,
/// which pays its cost and fires on the release. In the second, a plain
/// shot was fired, and a steady Sniper Shot was let go during the refire,
/// so it waits for the gun. The press drops it, and nothing fires. In the
/// third, a Power Blast charges for 600 ms, and [Feat attack] stays held
/// for 300 ms after the press. The charge's cost stays spent. The new
/// Sniper Shot steadies from the press, so the release is too soon and
/// fires nothing.
fn feat_change() -> Result<String, String> {
    let cases = [
        ("steadying", FeatLine::SniperShot, false, (1, 0, 1)),
        ("waiting for the gun", FeatLine::SniperShot, true, (0, 0, 0)),
        ("charging", FeatLine::PowerBlast, false, (0, 0, 1)),
    ];
    let seen = cases
        .iter()
        .map(|&(name, line, waiting, wanted)| {
            let sheet = both_lines(line);
            let mut ground = Ground::on(acting_level(), sheet, StepReason::Authority)?;
            let cost = specials::cost(&sheet);
            ground.hold(200, idle())?;
            if waiting {
                ground.hold(MSEC, keys(buttons::ATTACK, 0))?;
            }
            let plain = ground.fired.len();
            // The stamina spent is summed from each step's drop, because the
            // stamina comes back between the steps.
            let (mut spent, mut last) = (0, ground.fighter.fp);
            let mut tally = |fighter: &Fighter| {
                spent += (last - fighter.fp).max(0);
                last = fighter.fp;
            };
            let charge = match line {
                FeatLine::SniperShot => past(STEADY_MS[0]),
                _ => 600,
            };
            ground.watch(charge, feat(0), |_, fighter| tally(fighter))?;
            let command = if waiting { idle() } else { feat(0) };
            ground.watch(MSEC, command, |_, fighter| tally(fighter))?;
            let under_way = ground.fighter.shooting.charging_from.is_some()
                && (!waiting || ground.fighter.weapon_wait_ms > 0);
            ground.step(command, Some(Action::NextFeat { hand: 0 }))?;
            tally(&ground.fighter);
            ground.watch(300, command, |_, fighter| tally(fighter))?;
            ground.watch(1000, idle(), |_, fighter| tally(fighter))?;
            let after = &ground.fired[plain..];
            let blasts = after.iter().filter(|bolt| bolt.power).count();
            let snipes = after.iter().filter(|bolt| sniped(bolt, &sheet)).count();
            let ok = under_way
                && cost > 0
                && (blasts, snipes, spent) == (wanted.0, wanted.1, wanted.2 * cost);
            Ok((name, ok, under_way, blasts, snipes, spent, cost))
        })
        .collect::<Result<Vec<_>, String>>()?;
    check(
        seen.iter().all(|&(_, ok, ..)| ok),
        format!(
            "(state, as wanted, under way before the press, Power Blasts, Sniper Shots, \
             spent, cost): {seen:?}"
        ),
    )
}

/// With the gun aimed first, a Sniper Shot held a little past the aimed
/// steadying time fires one bolt. Held as long from the hip, it fires none.
fn steady_aimed() -> Result<String, String> {
    let hold = past(STEADY_MS[1]);
    let mut aimed = Ground::drawn(sniping(Blaster::Pistol))?;
    aimed.hold(300, aiming())?;
    let up = aimed.fighter.shooting.aiming;
    aimed.hold(hold, keys(0, holds::AIM | holds::FEAT))?;
    aimed.hold(200, aiming())?;
    let mut hip = Ground::drawn(sniping(Blaster::Pistol))?;
    hip.hold(hold, feat(0))?;
    hip.hold(200, idle())?;
    check(
        up && aimed.fired.len() == 1 && hip.fired.is_empty(),
        format!(
            "held {hold} ms: aimed {up}, {} bolts; from the hip, {} bolts",
            aimed.fired.len(),
            hip.fired.len()
        ),
    )
}

/// The runs `steady_straight` makes, each started a step later than the
/// last, so each meets other spread dice.
const SEEDS: u32 = 20;

/// The repeating blaster's player walks forward, fires 6 plain shots to
/// grow the spread, waits for the gun to be ready, then steadies a Sniper
/// Shot and lets go, in each of
/// `SEEDS` runs. Each Sniper Shot's bolt flies from the muzzle along the
/// crosshair line, within 0.01°. The way the line gives is rounded as the
/// sim rounds a bolt's velocity.
fn steady_straight() -> Result<String, String> {
    let sheet = sniping(SPREADING);
    let speed = weapons::primary(SPREADING).speed.ok_or("no bolt speed")?;
    let walk = |held: u8, pressed: u16| Command {
        forward: WALK_AXIS,
        ..keys(buttons::WALKING | pressed, held)
    };
    let mut offs = Vec::new();
    for seed in 0..SEEDS {
        let mut ground = Ground::drawn(sheet)?;
        ground.hold(seed * MSEC, idle())?;
        for _ in 0..2000 / MSEC {
            if ground.fired.len() >= 6 {
                break;
            }
            ground.hold(MSEC, walk(0, buttons::ATTACK))?;
        }
        let plain = ground.fired.len();
        // The steadying begins once the gun is ready again.
        while ground.fighter.weapon_wait_ms > 0 {
            ground.hold(MSEC, walk(0, 0))?;
        }
        ground.hold(past(STEADY_MS[0]), walk(holds::FEAT, 0))?;
        ground.hold(MSEC, walk(0, 0))?;
        let bolt = match &ground.fired[plain..] {
            [bolt] if sniped(bolt, &sheet) && plain == 6 => *bolt,
            other => return Err(format!("run {seed}: {plain} plain bolts, then {other:?}")),
        };
        let point = ground.crosshair_point()?;
        let (pitch, yaw) = mind::aim(bolt.base, point);
        let (way, ..) = math::angle_vectors([pitch, yaw, 0.0]);
        let wanted = math::scale(way, speed).map(f32::round_ties_even);
        offs.push(between(bolt.velocity, wanted));
    }
    let worst = offs.iter().copied().fold(0.0, f32::max);
    check(
        offs.len() == SEEDS as usize && worst <= 0.01,
        format!(
            "{} Sniper Shots, at most {worst:.4}° off the crosshair line",
            offs.len()
        ),
    )
}

/// With Sniper Shot, [Feat attack] held with the stick fully forward covers
/// what a held [Walk] does (within 3%), and jump held with it never leaves
/// the ground.
fn steady_walks() -> Result<String, String> {
    let ground = || Ground::drawn(sniping(Blaster::Pistol));
    let steadied = ground()?.covers(
        MOVE_MS,
        Command {
            forward: 127,
            ..feat(0)
        },
    )?;
    let walked = ground()?.covers(MOVE_MS, walk_forward())?;
    let jump = Command { up: 127, ..feat(0) };
    let left = leaves_ground(ground()?, holds::FEAT, jump, 500)?;
    check(
        (steadied / walked - 1.0).abs() <= 0.03 && !left,
        format!("steadying {steadied:.1} units, walked {walked:.1}; left the ground {left}"),
    )
}

/// With no stamina, [Feat attack] held for 1 s fires the same plain shots
/// as [Attack] held as long, and none of them is a Sniper Shot.
fn steady_broke() -> Result<String, String> {
    let sheet = sniping(Blaster::Pistol);
    let shots = |command: Command| -> Result<Vec<Missile>, String> {
        let mut ground = Ground::drawn(sheet)?;
        for _ in 0..1000 / MSEC {
            ground.poke(|fighter, _| fighter.fp = 0);
            ground.hold(MSEC, command)?;
        }
        Ok(ground.fired)
    };
    let (feat, attack) = (shots(feat(0))?, shots(keys(buttons::ATTACK, 0))?);
    let sniped = feat.iter().filter(|bolt| sniped(bolt, &sheet)).count();
    let at = |bolts: &[Missile]| bolts.iter().map(|bolt| bolt.from_ms).collect::<Vec<_>>();
    check(
        feat.len() >= 2 && at(&feat) == at(&attack) && sniped == 0,
        format!(
            "[Feat attack] bolts at {:?}, {sniped} Sniper Shots; [Attack] bolts at {:?}",
            at(&feat),
            at(&attack)
        ),
    )
}

/// Whether `fighter` cannot fire at `now`: stunned, knocked down, changing
/// weapons or using an item.
fn stopped(fighter: &Fighter, now: u64) -> bool {
    fighter.force.stunned(now)
        || fighter.saber.floored()
        || fighter.saber.change != crate::combat::saber::Change::None
        || fighter.pack.using.is_some()
}

/// The gun being put away (`Sheath::Holstering`), before the holster takes
/// it.
fn holstering() -> Drop {
    (
        "holstering",
        |fighter, _| fighter.sheath = Sheath::Holstering,
        aiming(),
    )
}

/// A Sniper Shot in two states, then each of `drops` and `holstering`. In
/// the first state
/// the shot steadies with [Feat attack] held, which stays held. In the
/// second, a plain shot was fired, and the Sniper Shot was steadied during
/// the refire and let go, so it waits for the gun to be ready. On the step
/// after the drop nothing steadies. A weapon switch only starts once the
/// gun is ready, so for the waiting shot it is checked on that step. [Feat
/// attack] stays as it was until the character can fire again, and is let
/// go one step later. No Sniper Shot leaves and no stamina is spent.
fn steady_drops() -> Result<String, String> {
    let seen = (drops().iter().chain([&holstering()]))
        .flat_map(|drop| [false, true].map(|waiting| (drop, waiting)))
        .map(|(&(name, change, command), waiting)| {
            let command = Command {
                holds: if waiting { 0 } else { holds::FEAT },
                ..command
            };
            let (mut ground, plain) = if waiting {
                let mut ground = after_plain_shot()?;
                ground.hold(past(STEADY_MS[0]), feat(0))?;
                ground.hold(MSEC, idle())?;
                (ground, 1)
            } else {
                let mut ground = Ground::drawn(sniping(Blaster::Pistol))?;
                ground.hold(200, idle())?;
                ground.hold(short_of(STEADY_MS[1]), feat(0))?;
                (ground, 0)
            };
            let pool = ground.fighter.fp;
            let before = ground.fighter.shooting.steadying && ground.fired.len() == plain;
            ground.poke(change);
            ground.hold(MSEC, command)?;
            // A weapon switch waits for the gun to be ready, as it does
            // after any shot, and drops the shot on that step.
            for _ in 0..1000 / MSEC {
                let shooting = ground.fighter.shooting;
                if !(waiting && shooting.steadying && ground.fighter.weapon_wait_ms > 0) {
                    break;
                }
                ground.hold(MSEC, command)?;
            }
            let after = ground.fighter.shooting;
            for _ in 0..10_000 / MSEC {
                if !stopped(&ground.fighter, ground.now) {
                    break;
                }
                ground.hold(MSEC, command)?;
            }
            ground.hold(MSEC, command)?;
            ground.hold(
                200,
                Command {
                    holds: 0,
                    ..command
                },
            )?;
            Ok((
                name,
                waiting,
                before,
                after.charging_from,
                after.steadying,
                ground.fired.len() - plain,
                pool - ground.fighter.fp,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    check(
        seen.iter()
            .all(|&(.., before, charging, after, bolts, spent)| {
                before && charging.is_none() && !after && bolts == 0 && spent == 0
            }),
        format!(
            "(how, waiting for the gun, steadying before, charging after, steadying after, \
             Sniper Shots, spent): {seen:?}"
        ),
    )
}

/// A hostile creature with a blaster rifle and Sniper Shot, 5 m from the
/// player: within 5 s it fires a Sniper Shot, and its gun is never aimed.
fn mind_snipes() -> Result<String, String> {
    use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
    use crate::module::{ObjectData, ObjectKind, Placement, Repute};
    let mut sheet = sniping(Blaster::Rifle);
    sheet.weapons[1] = None;
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Creature,
        tag: "sniper".into(),
        sheet: Some(Sheet {
            faction: HOSTILE_1,
            max_hp: 500,
            ..sheet
        }),
        ..ObjectData::unknown(
            "sniper",
            crate::determinism::ground_point(-4.0, 3.0, 0.0),
            0.0,
        )
    }));
    data.repute = Repute::at_war([PLAYER, HOSTILE_1]);
    let config = WorldConfig {
        sheet: Sheet {
            max_hp: 5000,
            ..Sheet::dev_soldier()
        },
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, ground.with_module(data)).map_err(|e| e.to_string())?;
    let (mut bolts, mut snipes, mut aim) = (0, 0, 0);
    for tick in 1..=5000 / u64::from(MSEC) {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let snapshot = crate::step(&mut world, tick, &input, MSEC, StepReason::Authority);
        let me = snapshot.own(Seat(0)).map(|me| me.id);
        for entity in &snapshot.entities {
            if Some(entity.id) != me
                && let Some(fighter) = entity.fighter
            {
                aim = aim.max(fighter.shooting.aim);
            }
            let Some(bolt) = entity.missile else { continue };
            if Some(bolt.owner) != me && bolt.from_ms == snapshot.time_ms {
                bolts += 1;
                snipes += usize::from(sniped(&bolt, &sheet));
            }
        }
        if snipes > 0 {
            break;
        }
    }
    check(
        snipes > 0 && aim == 0,
        format!("{bolts} bolts, {snipes} Sniper Shots; the creature's aim at most {aim}"),
    )
}

/// A pistol with Power Blast, its player in stealth mode and wearing a
/// stealth unit. One step with [Feat attack] alone held ends stealth mode.
/// One step with nothing pressed leaves it on.
fn feat_ends_stealth() -> Result<String, String> {
    let mut sheet = blasting();
    sheet.flags |= crate::combat::sheet::flags::STEALTH_UNIT;
    let stealthy = |command: Command| -> Result<bool, String> {
        let mut ground = Ground::drawn(sheet)?;
        ground.poke(|fighter, _| fighter.covert.stealth = true);
        ground.hold(MSEC, command)?;
        Ok(ground.fighter.covert.stealth)
    };
    let (still, fired) = (stealthy(idle())?, stealthy(feat(0))?);
    check(
        still && !fired,
        format!("in stealth after a step with nothing pressed {still}, with [Feat attack] {fired}"),
    )
}

/// [Use] held standing still holds a revive (`falls::holding`). With
/// [Feat attack] held as well, it does not. A walk the target block steers
/// and a revive's hold both end when the player holds [Feat attack] alone
/// (`target::steer`).
fn feat_breaks_revive() -> Result<String, String> {
    let mut reviver = Fighter::fresh(10, 0);
    reviver.last_command = keys(buttons::USE_HELD, 0);
    let revives = super::falls::holding(&reviver);
    reviver.last_command = keys(buttons::USE_HELD, holds::FEAT);
    let with_feat = super::falls::holding(&reviver);
    let piloted = Some(keys(buttons::USE_HELD, 0));
    let broken = [crate::target::WALK_BREAKS, crate::target::HOLD_BREAKS]
        .map(|breaks| crate::target::steer(piloted, feat(0), breaks).1);
    check(
        revives && !with_feat && broken == [true; 2],
        format!(
            "revives with [Use] {revives}, with [Use] and [Feat attack] {with_feat}; \
             [Feat attack] ends (the walk, the revive's hold) {broken:?}"
        ),
    )
}

/// A pistol with Power Blast, on the proving ground with a script library:
/// after two steps with [Feat attack] alone held, the scripts see the
/// player attacking (`Character::attacking`). After two steps with nothing
/// pressed, they do not.
fn feat_attacking() -> Result<String, String> {
    let attacking = |command: Command| -> Result<bool, String> {
        let mut ground = Ground::on(acting_level(), blasting(), StepReason::Authority)?;
        ground.hold(2 * MSEC, command)?;
        let me = (ground.last.as_ref())
            .and_then(|snapshot| snapshot.body(Seat(0)))
            .map(|me| me.id)
            .ok_or("no player")?;
        let scripts = ground.world.world.resource::<super::scripts::Scripts>();
        (scripts.things.iter())
            .find(|thing| thing.id.0 == me.0)
            .and_then(|thing| thing.character.as_ref())
            .map(|character| character.attacking)
            .ok_or_else(|| "the scripts see no player".to_string())
    };
    let (still, fired) = (attacking(idle())?, attacking(feat(0))?);
    check(
        !still && fired,
        format!(
            "attacking to the scripts with nothing pressed {still}, with [Feat attack] {fired}"
        ),
    )
}

/// The dev soldier's pistol and frag grenades: a grenade readied puts the
/// gun in its holster. [Feat attack] held for 300 ms then leaves the gun
/// in its holster and the grenade readied, and fires nothing.
fn feat_grenade_readied() -> Result<String, String> {
    use crate::action::GrenadeOrder;
    use crate::combat::items::Readied;
    let mut ground = pistol()?;
    ground.step(idle(), Some(Action::Grenade(GrenadeOrder::Toggle)))?;
    ground.hold(300, idle())?;
    let state = |ground: &Ground| {
        (
            ground.fighter.pack.readied == Readied::On,
            ground.fighter.weapon_away(ground.now),
        )
    };
    let before = state(&ground);
    ground.hold(300, feat(0))?;
    let after = state(&ground);
    check(
        before == (true, true) && after == (true, true) && ground.fired.is_empty(),
        format!(
            "(grenade readied, gun away) before {before:?}, after 300 ms of [Feat attack] \
             {after:?}; {} bolts",
            ground.fired.len()
        ),
    )
}

/// A pistol knowing Power Blast and Rapid Shot, with Rapid Shot chosen:
/// Power Blast chosen from the action bar, then [Feat attack] held 1.1 s
/// and let go fires one Power Blast.
fn bar_feat_slot() -> Result<String, String> {
    let mut sheet = trained(
        Blaster::Pistol,
        &[feats::POWER_BLAST[0], feats::RAPID_SHOT[0]],
    );
    let rapid = Picks::set(&mut sheet, 0, FeatLine::RapidShot);
    let mut ground = Ground::on(acting_level(), sheet, StepReason::Authority)?;
    let chosen = Action::Feat {
        hand: 0,
        line: FeatLine::PowerBlast,
    };
    ground.step(idle(), Some(chosen))?;
    ground.hold(200, idle())?;
    let special = ground
        .last
        .as_ref()
        .and_then(|snapshot| snapshot.body(Seat(0)))
        .and_then(|me| me.sheet)
        .and_then(|sheet| Special::of(&sheet, Blaster::Pistol));
    ground.hold(1100, feat(0))?;
    ground.hold(200, idle())?;
    check(
        rapid && matches!(&ground.fired[..], [blast] if blast.power),
        format!(
            "Rapid Shot chosen first {rapid}; the pistol's feat attack after the bar's \
             choice {special:?}; {} bolts, Power Blast {:?}",
            ground.fired.len(),
            ground
                .fired
                .iter()
                .map(|bolt| bolt.power)
                .collect::<Vec<_>>()
        ),
    )
}
