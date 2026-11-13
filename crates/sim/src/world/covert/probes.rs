//! The probes of `world::covert` (`sim::stealth::probes`; the approved
//! test `stealth_spot`): stealth mode on the proving ground.

use crate::combat::sheet::{flags, skills};
use crate::combat::{Sheath, Sheet};
use crate::input::{Seat, buttons};
use crate::notice::Notice;
use crate::probe::{check, failed};
use crate::world::locks::lines;
use crate::{Command, StepReason, TickInput};

/// The soldier with `stealth` ranks, wearing a stealth unit or not.
pub(crate) fn sneak(stealth: u8, unit: bool) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.skills[skills::STEALTH] = stealth;
    if unit {
        sheet.flags |= flags::STEALTH_UNIT;
    }
    sheet
}

fn world(sheet: Sheet) -> Result<crate::world::SimWorld, String> {
    world_on(crate::determinism::proving_ground(), sheet)
}

/// One seat playing `sheet` alone on `level`.
fn world_on(level: crate::Level, sheet: Sheet) -> Result<crate::world::SimWorld, String> {
    crate::world::SimWorld::new(
        crate::WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet,
            guests: crate::Guests::Avatars,
        },
        level,
    )
    .map_err(|error| error.to_string())
}

/// The player's fighter in `snapshot`.
fn player(snapshot: &crate::Snapshot) -> Option<crate::combat::Fighter> {
    snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Avatar(_)))
        .and_then(|entity| entity.fighter)
}

/// What steps with `command` (its buttons on the first only, `held` on
/// every one) saw: the notices, the last snapshot.
fn steps(
    world: &mut crate::world::SimWorld,
    from: u64,
    count: u64,
    command: Command,
    held: u16,
) -> (Vec<Notice>, crate::Snapshot) {
    let mut notices = Vec::new();
    let mut last = world.snapshot();
    for tick in from..from + count {
        let mut input = TickInput::default();
        let mut sent = command;
        sent.buttons = held | if tick == from { command.buttons } else { 0 };
        input.insert(Seat(0), sent);
        last = crate::step(world, tick, &input, 8, StepReason::Authority);
        notices.extend(world.take_notices().into_iter().map(|(_, notice)| notice));
    }
    (notices, last)
}

fn press(button: u16) -> Command {
    Command {
        buttons: button,
        ..Command::default()
    }
}

/// Stealth's toggle and what ends it (S11).
pub(crate) fn stealth_mode() -> Vec<(&'static str, Result<String, String>)> {
    let mut checks = Vec::new();
    let mut run = |name: &'static str, test: &dyn Fn() -> Result<(bool, String), String>| {
        checks.push((
            name,
            match test() {
                Ok((ok, what)) => check(ok, what),
                Err(why) => Err(why),
            },
        ));
    };
    let on = |snapshot: &crate::Snapshot| player(snapshot).is_some_and(|f| f.covert.stealth);
    let refused = |notices: &[Notice], line: u32| notices.contains(&Notice::Line(line));
    run("stealth_needs_unit", &|| {
        let mut world = world(sneak(2, false))?;
        let (told, last) = steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
        Ok((
            !on(&last) && refused(&told, lines::CANNOT_USE_SKILL),
            format!("{told:?}"),
        ))
    });
    run("stealth_on_and_off", &|| {
        let mut world = world(sneak(2, true))?;
        let (told, last) = steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
        let (again, after) = steps(&mut world, 4, 3, press(buttons::STEALTH), 0);
        Ok((
            on(&last)
                && refused(&told, lines::STEALTH_ON)
                && !on(&after)
                && refused(&again, lines::STEALTH_OFF),
            format!("{told:?} {again:?}"),
        ))
    });
    run("stealth_needs_ranks", &|| {
        let mut world = world(sneak(0, true))?;
        let (told, last) = steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
        Ok((
            !on(&last) && refused(&told, lines::CANNOT_USE_SKILL),
            format!("{told:?}"),
        ))
    });
    run("stealth_not_in_combat", &|| {
        let mut world = world(sneak(2, true))?;
        set_combat(&mut world, 60_000);
        let (told, last) = steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
        Ok((
            !on(&last) && refused(&told, lines::STEALTH_IN_COMBAT),
            format!("{told:?}"),
        ))
    });
    run("stealth_restrict", &|| {
        let mut world = world(sneak(2, true))?;
        let mut state = world.script_state().clone();
        state.party.restrict = true;
        world.set_script_state(state);
        let (told, last) = steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
        Ok((
            !on(&last) && refused(&told, lines::CANNOT_USE_SKILL),
            format!("{told:?}"),
        ))
    });
    run("attack_ends_stealth", &|| {
        let mut world = world(sneak(2, true))?;
        steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
        let (_, last) = steps(&mut world, 4, 1, press(buttons::ATTACK), 0);
        let opener = player(&last).map(|f| f.covert.opener_until);
        Ok((
            !on(&last) && opener == Some(last.time_ms + crate::stealth::OPENER_MS),
            format!("opener {opener:?} at {}", last.time_ms),
        ))
    });
    run("unit_off_ends_stealth", &|| {
        let mut world = world(sneak(2, true))?;
        steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
        set_flags(&mut world, 0);
        let (_, last) = steps(&mut world, 4, 2, Command::default(), 0);
        Ok((!on(&last), "unit taken off".into()))
    });
    run("combat_ends_stealth", &|| {
        let mut world = world(sneak(2, true))?;
        steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
        set_combat(&mut world, 60_000);
        let (_, last) = steps(&mut world, 4, 2, Command::default(), 0);
        Ok((!on(&last), "a fight".into()))
    });
    // KOTOR's "Run Disabled": a run held in stealth goes at a walk.
    run("stealth_walks", &|| {
        let speed = |stealthed: bool| -> Result<f32, String> {
            let mut world = world(sneak(2, true))?;
            let from = if stealthed {
                steps(&mut world, 1, 3, press(buttons::STEALTH), 0);
                4
            } else {
                1
            };
            // In the open, 4 m from the door, running away from it.
            let start = crate::determinism::ground_point(-4.0, 0.0, 0.0);
            let mut bodies = world
                .world
                .query::<(&crate::world::Avatar, &mut crate::world::Player)>();
            for (_, mut body) in bodies.iter_mut(&mut world.world) {
                body.0.origin[0] = start[0] / collision::UNIT;
                body.0.origin[1] = start[1] / collision::UNIT;
            }
            let forward = Command {
                angles: [0, i16::MIN, 0],
                forward: 127,
                ..Command::default()
            };
            let (_, last) = steps(&mut world, from, 50, forward, 0);
            let entity = last
                .entities
                .iter()
                .find(|entity| matches!(entity.kind, crate::Kind::Avatar(_)))
                .ok_or("no player")?;
            let v = entity.player.ok_or("no body")?.velocity;
            Ok((v[0] * v[0] + v[1] * v[1]).sqrt())
        };
        let (walked, ran) = (speed(true)?, speed(false)?);
        Ok((
            walked < 140.0 && ran > 200.0,
            format!("{walked:.0} u/s stealthed, {ran:.0} u/s not"),
        ))
    });
    checks
}

fn set_combat(world: &mut crate::world::SimWorld, until: u64) {
    let mut fighters = world
        .world
        .query::<(&crate::world::Avatar, &mut crate::world::Fighting)>();
    for (_, mut fighting) in fighters.iter_mut(&mut world.world) {
        fighting.0.combat_until = until;
    }
}

fn set_flags(world: &mut crate::world::SimWorld, bits: u8) {
    let mut sheets = world
        .world
        .query::<(&crate::world::Avatar, &mut crate::world::PlayerSheet)>();
    for (_, mut sheet) in sheets.iter_mut(&mut world.world) {
        sheet.0.flags = bits;
    }
}

/// Where the guard stands in the arrival half, and the player 6 m in front
/// of it (the room is 16 m by 12 m).
const GUARD_AT: [f32; 2] = [-7.0, 0.0];
const SNEAK_AT: [f32; 2] = [-1.0, 0.0];
/// The guard's number among the ground's objects (after its door and
/// trigger), and the dummy's it fights in the opener's runs.
const GUARD: u32 = 2;
const DUMMY: u32 = 3;
/// Where the dummy stands: 1.5 m to the guard's side, so the guard
/// turning to it does not put the player behind it.
const DUMMY_AT: [f32; 2] = [-7.0, 1.5];

/// The proving ground with a guard of a faction at war with the player's
/// (senses 20 m, Awareness `awareness`), the scripts' step running (for
/// perception) if `scripted`; one seat playing `sheet`.
fn guarded(sheet: Sheet, awareness: u8, scripted: bool) -> Result<crate::world::SimWorld, String> {
    guarded_with(sheet, awareness, scripted, false)
}

/// `guarded`, with a dummy of the player's faction behind the guard if
/// `dummy` (one it may fight).
fn guarded_with(
    sheet: Sheet,
    awareness: u8,
    scripted: bool,
    dummy: bool,
) -> Result<crate::world::SimWorld, String> {
    guarded_by(
        sheet,
        (awareness, crate::combat::sheet::factions::HOSTILE_1, 0.0),
        scripted,
        dummy,
    )
}

/// `guarded_with`, the guard (Awareness, faction, facing in degrees from
/// +X) as given.
fn guarded_by(
    sheet: Sheet,
    guard: (u8, u8, f32),
    scripted: bool,
    dummy: bool,
) -> Result<crate::world::SimWorld, String> {
    guarded_on(
        crate::determinism::proving_ground(),
        sheet,
        guard,
        scripted,
        dummy,
    )
}

/// `guarded_by` on `ground`, the proving ground or a copy of it with other
/// animations.
fn guarded_on(
    ground: crate::Level,
    sheet: Sheet,
    (awareness, faction, yaw): (u8, u8, f32),
    scripted: bool,
    dummy: bool,
) -> Result<crate::world::SimWorld, String> {
    use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
    use crate::module::{ObjectData, Placement, Repute, Senses};
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let mut guard_sheet = Sheet {
        faction,
        max_hp: 500,
        ..Sheet::dev_soldier()
    };
    guard_sheet.skills[skills::AWARENESS] = awareness;
    data.placements.push(Placement::Own(ObjectData {
        tag: "guard".into(),
        sheet: Some(guard_sheet),
        senses: Some(Senses {
            sight: 20.0,
            hearing: 20.0,
        }),
        ..ObjectData::unknown(
            "guard",
            crate::determinism::ground_point(GUARD_AT[0], GUARD_AT[1], 0.0),
            yaw,
        )
    }));
    if dummy {
        data.placements.push(Placement::Own(ObjectData {
            tag: "dummy".into(),
            sheet: Some(Sheet {
                faction: PLAYER,
                max_hp: 5000,
                ..Sheet::commoner()
            }),
            ..ObjectData::unknown(
                "dummy",
                crate::determinism::ground_point(DUMMY_AT[0], DUMMY_AT[1], 0.0),
                0.0,
            )
        }));
    }
    data.area_stealth = crate::module::AreaStealth {
        mod_listen: 0,
        xp_max: 100,
        xp_current: 60,
        xp_loss: 20,
        xp_enabled: true,
    };
    let war = [usize::from(PLAYER), usize::from(HOSTILE_1)];
    data.repute = Repute {
        rows: (0..7)
            .map(|a| {
                (0..7)
                    .map(|b| match (a, b) {
                        _ if a == b => 100,
                        _ if war.contains(&a) && war.contains(&b) => 0,
                        _ => 50,
                    })
                    .collect()
            })
            .collect(),
    };
    let level = ground.with_module(data);
    let level = if scripted {
        level.with_scripts(crate::probe::library(
            "// 0\nvoid SendMessageToPC(object oPlayer, string szMessage);\n",
            Vec::<(String, fmt_odyssey::ncs::Ncs)>::new(),
        ))
    } else {
        level
    };
    world_on(level, sheet)
}

/// Puts the player at `at` (metres) and the guard at its place.
fn stand(world: &mut crate::world::SimWorld, at: [f32; 2]) {
    let mut bodies = world.world.query::<(
        Option<&crate::world::Avatar>,
        Option<&crate::world::Object>,
        &mut crate::world::Player,
    )>();
    for (avatar, object, mut body) in bodies.iter_mut(&mut world.world) {
        let to = match (avatar, object) {
            (Some(_), None) => at,
            (None, Some(object)) if object.index == GUARD => GUARD_AT,
            (None, Some(object)) if object.index == DUMMY => DUMMY_AT,
            _ => continue,
        };
        let to = crate::determinism::ground_point(to[0], to[1], 0.0);
        body.0.origin[0] = to[0] / collision::UNIT;
        body.0.origin[1] = to[1] / collision::UNIT;
        if avatar.is_some() {
            body.0.view_offset = [0; 3];
        }
    }
}

/// The guard's and the player's ids in `snapshot`.
fn ids(snapshot: &crate::Snapshot) -> Option<(u32, u32)> {
    let id = |pick: &dyn Fn(&crate::Kind) -> bool| {
        snapshot
            .entities
            .iter()
            .find(|entity| pick(&entity.kind))
            .map(|entity| entity.id.0)
    };
    Some((
        id(&|kind| matches!(kind, crate::Kind::Object(_, GUARD)))?,
        id(&|kind| matches!(kind, crate::Kind::Avatar(_)))?,
    ))
}

/// Sets both bodies' perception rolls to their middle, not drawn again for
/// 20 s.
fn mean_rolls(world: &mut crate::world::SimWorld, bodies: (u32, u32)) {
    let mut state = world.script_state().clone();
    for id in [bodies.0, bodies.1] {
        state
            .perception
            .insert(id, (0, crate::stealth::Rolls::MEAN));
    }
    world.set_script_state(state);
}

/// Steps `count` from `from` with the player at `at`, stealth pressed on
/// the first if `sneak`; how many steps until the guard saw the player
/// (none if it never did).
fn watched(
    world: &mut crate::world::SimWorld,
    from: u64,
    count: u64,
    at: [f32; 2],
) -> Result<Option<u64>, String> {
    let (guard, player) = ids(&world.snapshot()).ok_or("no guard or player")?;
    for step in 0..count {
        stand(world, at);
        crate::step(
            world,
            from + step,
            &TickInput::default(),
            8,
            StepReason::Authority,
        );
        let seen = world
            .script_state()
            .perceived
            .get(&guard)
            .is_some_and(|perceived| perceived.seen.contains(&player));
        if seen {
            return Ok(Some(step + 1));
        }
    }
    Ok(None)
}

/// Turns stealth on (three steps at `at`).
fn hide(world: &mut crate::world::SimWorld, at: [f32; 2]) {
    for tick in 1..=3 {
        stand(world, at);
        let mut input = TickInput::default();
        input.insert(Seat(0), press(if tick == 1 { buttons::STEALTH } else { 0 }));
        crate::step(world, tick, &input, 8, StepReason::Authority);
    }
}

/// Perception with stealth at KOTOR's cadence (S12, S13).
pub(crate) fn perception() -> Vec<(&'static str, Result<String, String>)> {
    let mut checks = Vec::new();
    let mut run = |name: &'static str, test: &dyn Fn() -> Result<(bool, String), String>| {
        checks.push((
            name,
            match test() {
                Ok((ok, what)) => check(ok, what),
                Err(why) => Err(why),
            },
        ));
    };
    // Not stealthed, 6 m in front: seen within 0.7 s.
    run("plain_seen_fast", &|| {
        let mut world = guarded(sneak(0, false), 2, true)?;
        let seen = watched(&mut world, 1, 88, SNEAK_AT)?;
        Ok((seen.is_some(), format!("seen after {seen:?} steps")))
    });
    // Stealthed (Stealth 8, DEX +2), still, 6 m in front of a still guard
    // with Awareness 2, the rolls their middle: 2 + 10 - 10 - 15 - 5 + 5 is
    // below 0: not seen for 5 s, nor heard.
    run("stealth_unseen", &|| {
        let mut world = guarded(sneak(8, true), 2, true)?;
        hide(&mut world, SNEAK_AT);
        let bodies = ids(&world.snapshot()).ok_or("no bodies")?;
        mean_rolls(&mut world, bodies);
        let seen = watched(&mut world, 4, 625, SNEAK_AT)?;
        let heard = world
            .script_state()
            .perceived
            .get(&bodies.0)
            .is_some_and(|perceived| perceived.heard.contains(&bodies.1));
        Ok((
            seen.is_none() && !heard,
            format!("seen {seen:?}, heard {heard}"),
        ))
    });
    // The same against Awareness 20: 20 + 10 - 10 - 15 - 5 + 5 = 5.
    run("stealth_seen_alert", &|| {
        let mut world = guarded(sneak(8, true), 20, true)?;
        hide(&mut world, SNEAK_AT);
        let bodies = ids(&world.snapshot()).ok_or("no bodies")?;
        mean_rolls(&mut world, bodies);
        let seen = watched(&mut world, 4, 88, SNEAK_AT)?;
        Ok((seen.is_some(), format!("seen after {seen:?} steps")))
    });
    // The guard looks every 300 to 699 ms: 14 to 34 looks in 10 s.
    run("cadence", &|| {
        let mut world = guarded(sneak(0, false), 2, true)?;
        let (guard, _) = ids(&world.snapshot()).ok_or("no bodies")?;
        let mut looks = 0;
        let mut next = None;
        for tick in 1..=1250 {
            stand(&mut world, SNEAK_AT);
            crate::step(
                &mut world,
                tick,
                &TickInput::default(),
                8,
                StepReason::Authority,
            );
            let now = world
                .script_state()
                .perception
                .get(&guard)
                .map(|(at, _)| *at);
            if now != next {
                looks += 1;
                next = now;
            }
        }
        Ok(((14..=34).contains(&looks), format!("{looks} looks in 10 s")))
    });
    // Without scripts the guard picks its own foes: not the stealthed
    // player at 6 m, the plain one.
    run("minds_respect_stealth", &|| {
        let target = |hidden: bool| -> Result<bool, String> {
            let mut world = guarded(sneak(8, true), 2, false)?;
            let from = if hidden {
                hide(&mut world, SNEAK_AT);
                4
            } else {
                1
            };
            let mut last = world.snapshot();
            for tick in from..from + 125 {
                stand(&mut world, SNEAK_AT);
                last = crate::step(
                    &mut world,
                    tick,
                    &TickInput::default(),
                    8,
                    StepReason::Authority,
                );
            }
            Ok(last
                .entities
                .iter()
                .find(|entity| matches!(entity.kind, crate::Kind::Object(_, GUARD)))
                .and_then(|entity| entity.fighter)
                .is_some_and(|fighter| fighter.target.is_some()))
        };
        let (hidden, plain) = (target(true)?, target(false)?);
        Ok((
            !hidden && plain,
            format!("hidden targeted {hidden}, plain {plain}"),
        ))
    });
    checks
}

/// A bare-handed sneak (Stealth 8, a unit, sneak attack's first tier):
/// fists land on the proving ground as `fists_brawl`'s do.
fn fist_sneak() -> Sheet {
    let mut sheet = sneak(8, true);
    sheet.weapons = [None; 2];
    sheet.feats = sheet
        .feats
        .with(crate::combat::sheet::feats::SNEAK_ATTACK[0]);
    sheet
}

/// Keeps the guard fighting the dummy (its target, aware long since, in
/// combat), so only the opener can make it unaware of the player.
fn busy_guard(world: &mut crate::world::SimWorld) {
    let mut fighters = world.world.query::<(
        &crate::world::Id,
        &crate::world::Object,
        &mut crate::world::Fighting,
    )>();
    let dummy = fighters
        .iter(&world.world)
        .find(|(_, object, _)| object.index == DUMMY)
        .map(|(id, ..)| id.0);
    for (_, object, mut fighting) in fighters.iter_mut(&mut world.world) {
        if object.index == GUARD {
            fighting.0.target = dummy;
            fighting.0.mind.aware_at = 0;
            fighting.0.combat_until = u64::MAX / 2;
        }
    }
}

/// The step on which an opener's run with the weapon put away first
/// presses attack: the guard's first looks at the stealthed player have
/// passed by then.
const PRESS: u64 = 30;
/// The length of an opener run's step (ms).
const STEP_MS: u32 = 8;

/// The step on which an opener's run first presses attack: `PRESS` with
/// the weapon put away, or `SWING_AT` in a run that presses the draw or
/// holster key on `DRAW_AT` first.
fn press_at(drawn: bool) -> u64 {
    if drawn { SWING_AT } else { PRESS }
}

/// How an opener's run attacks.
#[derive(Debug, Clone, Copy)]
struct Attack {
    /// The guard is kept fighting the dummy.
    busy: bool,
    /// The player aims along its own eyes' line (`mind::aim_for`), which
    /// meets the guard's head, so its shots fly at the guard. Otherwise the
    /// player aims along the third-person line, which runs over the guard's
    /// head.
    eyes: bool,
    /// The player presses the draw or holster key on `DRAW_AT`.
    drawn: bool,
}

/// Runs 900 steps of 8 ms (7.2 s) with the player at `at`, facing the
/// guard (as `fists_brawl` faces its foe). Stealth is pressed on the first
/// step, and attack is held from `press_at(attack.drawn)` on. `each` sees
/// the world and the snapshot after every step, with the step's tick and
/// whether the step told the player "Sneak Attack!". Returns how many
/// "Sneak Attack!" lines the player was told.
fn attack_run(
    world: &mut crate::world::SimWorld,
    at: [f32; 2],
    Attack { busy, eyes, drawn }: Attack,
    mut each: impl FnMut(&crate::world::SimWorld, u64, &crate::Snapshot, bool),
) -> usize {
    let (from, to) = (
        crate::determinism::ground_point(at[0], at[1], 0.0),
        crate::determinism::ground_point(GUARD_AT[0], GUARD_AT[1], 0.0),
    );
    let yaw = crate::math::atan2_deg(to[1] - from[1], to[0] - from[0]);
    let facing = Command {
        angles: crate::moves::view_angles(0.0, yaw),
        ..Command::default()
    };
    let press = press_at(drawn);
    let mut sneaks = 0;
    for tick in 1..=900 {
        stand(world, at);
        if busy {
            busy_guard(world);
        }
        let aim = if eyes { own_eyes(world) } else { [0; 3] };
        let mut input = TickInput::default();
        let buttons = match tick {
            STEALTH_AT => buttons::STEALTH,
            t if t >= press => buttons::ATTACK,
            _ => 0,
        };
        input.insert(
            Seat(0),
            Command {
                buttons,
                aim,
                ..facing
            },
        );
        if drawn && tick == DRAW_AT {
            input.act(Seat(0), crate::action::Action::Holster);
        }
        let snapshot = crate::step(world, tick, &input, STEP_MS, StepReason::Authority);
        let told = world
            .take_notices()
            .iter()
            .filter(|(_, notice)| *notice == Notice::Line(lines::SNEAK_ATTACK))
            .count();
        sneaks += told;
        each(world, tick, &snapshot, told > 0);
    }
    sneaks
}

/// The `Command::aim` of the player's own eyes.
fn own_eyes(world: &mut crate::world::SimWorld) -> [i8; 3] {
    let mut bodies = world
        .world
        .query::<(&crate::world::Avatar, &crate::world::Player)>();
    bodies
        .iter(&world.world)
        .next()
        .map_or([0; 3], |(_, body)| {
            crate::combat::mind::aim_for(&body.0, crate::combat::mind::eye(&body.0))
        })
}

/// The player stealthed 1 m from the guard (Awareness `awareness`), the
/// guard fighting a dummy, then the player attacking for 7 s (fists land
/// 3 s apart: three blows): the "Sneak Attack!" lines it was told, and its
/// opener after.
fn opener_run(awareness: u8) -> Result<(usize, Option<u64>), String> {
    let mut world = guarded_with(fist_sneak(), awareness, true, true)?;
    let close = [GUARD_AT[0] + 1.0, GUARD_AT[1]];
    let mut opener = None;
    let sneaks = attack_run(
        &mut world,
        close,
        Attack {
            busy: true,
            eyes: false,
            drawn: false,
        },
        |_, _, snapshot, _| {
            opener = player(snapshot).map(|f| f.covert.opener_until);
        },
    );
    Ok((sneaks, opener))
}

/// What the sneak holds in an armed opener's run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Armed {
    Fists,
    Vibroblade,
    Saber,
    Blaster,
}

impl Armed {
    pub const ALL: [Armed; 4] = [
        Armed::Fists,
        Armed::Vibroblade,
        Armed::Saber,
        Armed::Blaster,
    ];

    /// The weapon in its right hand: the dev soldier's vibroblade or
    /// blaster pistol, or the dev Jedi's lightsaber.
    pub(crate) fn weapon(self) -> Option<crate::combat::sheet::Weapon> {
        match self {
            Armed::Fists => None,
            Armed::Vibroblade => Sheet::dev_soldier().weapons[1],
            Armed::Saber => Sheet::dev_jedi().weapons[0],
            Armed::Blaster => Sheet::dev_soldier().weapons[0],
        }
    }

    /// How far behind the guard it stands (metres): 1 m with a blade or
    /// fists, 6 m with a blaster. The wall across the proving ground
    /// stands 7 m from the guard, so a shooter cannot stand farther off
    /// in the guard's half.
    fn behind_m(self) -> f32 {
        match self {
            Armed::Blaster => 6.0,
            _ => 1.0,
        }
    }

    /// Its name in the armed openers' rows.
    fn name(self) -> &'static str {
        match self {
            Armed::Fists => "fists",
            Armed::Vibroblade => "vibroblade",
            Armed::Saber => "saber",
            Armed::Blaster => "blaster",
        }
    }
}

/// What an armed opener's run saw. `sneaks` counts the "Sneak Attack!"
/// lines told. `press_to_hit_ms` is the time from the attack press to the
/// player's first blow on the guard, and `perceived_at_hit` says whether
/// the guard perceived the player on the step that blow landed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenerRun {
    pub sneaks: usize,
    pub press_to_hit_ms: Option<u64>,
    pub perceived_at_hit: bool,
    /// The step the first blow landed on told the player "Sneak Attack!".
    pub first_blow_sneak: bool,
    /// The player was in Stealth Mode on the last step before the attack
    /// press.
    pub stealth_before_press: bool,
    /// The player's weapon was in hand by its draw (`Sheath::Drawn`, not
    /// away) on the last step before the attack press.
    pub in_hand_before_press: bool,
    /// The guard perceived the player on some step before the attack press.
    pub perceived_before_press: bool,
    /// The time from the attack press to the first step on or after it on
    /// which the guard perceived the player.
    pub press_to_perceived_ms: Option<u64>,
}

/// The guard's facing in the armed openers' runs: toward -X, so its back
/// is to the player standing on the +X side of it.
const AWAY_YAW: f32 = 180.0;

/// Where an armed opener's run puts the guard and the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scene {
    /// An ordinary guard (Awareness 2, not kept busy) faces away, and the
    /// player stands behind it at `Armed::behind_m`.
    Behind,
    /// An alert guard (Awareness 20) faces the player, who stands 1 m in
    /// front of it. The guard is kept fighting the dummy, so it is neither
    /// unaware of a fight nor turned away from the player, and only
    /// perception decides the sneak attack.
    Watched,
    /// The ordinary guard faces away, and the player stands 4 m behind it,
    /// out of a blade's reach.
    OutOfReach,
}

impl Scene {
    /// The guard's Awareness and facing, and whether the dummy stands
    /// beside it and the guard is kept fighting the dummy.
    fn guard(self) -> (u8, f32, bool) {
        match self {
            Scene::Behind | Scene::OutOfReach => (2, AWAY_YAW, false),
            Scene::Watched => (20, 0.0, true),
        }
    }

    /// How far from the guard the player stands (metres, on the +X side).
    fn distance_m(self, weapon: Armed) -> f32 {
        match self {
            Scene::Behind => weapon.behind_m(),
            Scene::Watched => 1.0,
            Scene::OutOfReach => 4.0,
        }
    }
}

/// Whether `snapshot` shows a blow by `me` on `guard` this step. Any event
/// on the guard by the player except a visual one counts as a blow, which
/// assumes the guard, a hostile soldier with no lightsaber, never parries,
/// deflects or refuses the player's attacks.
fn struck(snapshot: &crate::Snapshot, guard: u32, me: u32) -> bool {
    snapshot
        .entities
        .iter()
        .find(|entity| entity.id.0 == guard)
        .and_then(|entity| entity.fighter)
        .is_some_and(|fighter| {
            fighter.events.iter().flatten().any(|event| {
                event.by.map(|by| by.0) == Some(me)
                    && event.result != crate::combat::Outcome::Visual
            })
        })
}

/// The player stealthed behind an ordinary guard (`Scene::Behind`),
/// holding `weapon`, then attacking for 7 s, on the proving ground with
/// `anims`. The weapon starts put away, or, if `drawn`, is drawn with the
/// draw or holster key in Stealth Mode 2 s before the attack. Bare hands
/// have nothing to draw, so `Armed::Fists` with `drawn` is an error.
pub fn armed_opener(
    anims: &std::sync::Arc<crate::motion::AnimTable>,
    weapon: Armed,
    drawn: bool,
) -> Result<OpenerRun, String> {
    opener_in(anims, weapon, drawn, Scene::Behind)
}

/// `armed_opener` in `scene`.
fn opener_in(
    anims: &std::sync::Arc<crate::motion::AnimTable>,
    weapon: Armed,
    drawn: bool,
    scene: Scene,
) -> Result<OpenerRun, String> {
    if drawn && weapon == Armed::Fists {
        return Err("bare hands have no weapon to draw".into());
    }
    let mut sheet = fist_sneak();
    sheet.weapons = [weapon.weapon(), None];
    let (awareness, yaw, busy) = scene.guard();
    let mut world = guarded_on(
        ja_ground(anims)?,
        sheet,
        (awareness, crate::combat::sheet::factions::HOSTILE_1, yaw),
        true,
        busy,
    )?;
    let (guard, me) = ids(&world.snapshot()).ok_or("no guard or player")?;
    let at = [GUARD_AT[0] + scene.distance_m(weapon), GUARD_AT[1]];
    let press = press_at(drawn);
    let ms = |tick: u64| tick.saturating_sub(press) * u64::from(STEP_MS);
    let mut run = OpenerRun::default();
    let sneaks = attack_run(
        &mut world,
        at,
        Attack {
            busy,
            eyes: true,
            drawn,
        },
        |world, tick, snapshot, sneaked| {
            let perceived = crate::world::scripts::perceived(world.script_state(), guard, me);
            if tick < press {
                run.perceived_before_press |= perceived;
            } else if perceived && run.press_to_perceived_ms.is_none() {
                run.press_to_perceived_ms = Some(ms(tick));
            }
            if tick + 1 == press {
                let fighter = player(snapshot);
                run.stealth_before_press = fighter.is_some_and(|f| f.covert.stealth);
                run.in_hand_before_press = fighter
                    .is_some_and(|f| f.sheath == Sheath::Drawn && !f.weapon_away(snapshot.time_ms));
            }
            if tick >= press && run.press_to_hit_ms.is_none() && struck(snapshot, guard, me) {
                run.press_to_hit_ms = Some(ms(tick));
                run.perceived_at_hit = perceived;
                run.first_blow_sneak = sneaked;
            }
        },
    );
    Ok(OpenerRun { sneaks, ..run })
}

/// The armed openers' rows, on the proving ground with `anims` (JA's
/// animations and skeleton, which a blade needs to land a blow).
///
/// Each `Armed` value has a row with its weapon put away at the start.
/// That row is a measurement, and it fails only when no blow landed. Each
/// weapon also has a `_drawn` row, drawn in Stealth Mode before the
/// attack. That row checks that Stealth Mode held until the press, that
/// the first blow was a sneak attack on a guard that had not perceived
/// the player, and that the blow landed sooner than with the weapon put
/// away, with the weapon in hand before the press. `armed_opener_watched`
/// checks that a guard that perceived the player before the attack takes
/// no sneak attack. `armed_opener_out_of_reach` checks that a guard
/// perceives a player whose blow never lands at its first look after the
/// Sneak Attack Window ends, and not before.
pub fn armed_openers(
    anims: &std::sync::Arc<crate::motion::AnimTable>,
) -> Vec<(String, Result<String, String>)> {
    let mut rows = Vec::new();
    for weapon in Armed::ALL {
        let put_away = armed_opener(anims, weapon, false);
        let away_ms = put_away.as_ref().ok().and_then(|run| run.press_to_hit_ms);
        rows.push((
            format!("armed_opener_{}", weapon.name()),
            put_away
                .and_then(|run| check(run.press_to_hit_ms.is_some(), format!("measured {run:?}"))),
        ));
        if weapon == Armed::Fists {
            continue;
        }
        let drawn = armed_opener(anims, weapon, true).and_then(|run| {
            let sooner = run
                .press_to_hit_ms
                .zip(away_ms)
                .is_some_and(|(drawn, away)| drawn < away);
            let drew = if run.in_hand_before_press {
                ""
            } else {
                "the weapon was not in hand before the press; "
            };
            check(
                run.in_hand_before_press
                    && run.stealth_before_press
                    && run.first_blow_sneak
                    && !run.perceived_at_hit
                    && sooner,
                format!("{drew}{run:?}; put away, the first blow took {away_ms:?} ms"),
            )
        });
        rows.push((format!("armed_opener_{}_drawn", weapon.name()), drawn));
    }
    let watched = opener_in(anims, Armed::Vibroblade, true, Scene::Watched).and_then(|run| {
        check(
            run.perceived_before_press && run.press_to_hit_ms.is_some() && !run.first_blow_sneak,
            format!("{run:?}"),
        )
    });
    // The guard looks again at most the least wait between its perception
    // checks plus their whole spread after the window closes.
    let window = crate::stealth::OPENER_MS;
    let next_look = rules::stealth::perception::ACTIVE_LEAST_MS
        + u64::from(rules::stealth::perception::ACTIVE_SPREAD_MS);
    let missed = opener_in(anims, Armed::Vibroblade, true, Scene::OutOfReach).and_then(|run| {
        let after_window = run
            .press_to_perceived_ms
            .is_some_and(|ms| ms > window && ms <= window + next_look);
        check(
            run.press_to_hit_ms.is_none() && !run.perceived_before_press && after_window,
            format!(
                "{run:?}; the window lasts {window} ms, and the guard looks again within {next_look} ms"
            ),
        )
    });
    rows.extend([
        ("armed_opener_watched".into(), watched),
        ("armed_opener_out_of_reach".into(), missed),
    ]);
    rows
}

/// The proving ground with `anims` (JA's animations and skeleton, which a
/// blade needs to swing and land a blow).
fn ja_ground(anims: &std::sync::Arc<crate::motion::AnimTable>) -> Result<crate::Level, String> {
    let ground = crate::determinism::proving_ground();
    Ok(
        crate::Level::new(ground.name(), ground.collision().clone(), anims.clone())
            .with_module((**ground.module().ok_or("the ground has no module")?).clone()),
    )
}

/// The lightsabers a sneak holds in the unlit lightsaber runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hilts {
    One,
    Pair,
    Double,
}

impl Hilts {
    pub const ALL: [Hilts; 3] = [Hilts::One, Hilts::Pair, Hilts::Double];

    /// Its right and left hands: the dev Jedi's lightsaber alone, one in
    /// each hand, or that lightsaber made double-bladed.
    pub fn weapons(self) -> [Option<crate::combat::sheet::Weapon>; 2] {
        use crate::combat::sheet::{Delivery, SaberShape};
        let saber = Armed::Saber.weapon();
        match self {
            Hilts::One => [saber, None],
            Hilts::Pair => [saber, saber],
            Hilts::Double => [
                saber.map(|weapon| crate::combat::sheet::Weapon {
                    delivery: Delivery::Saber(SaberShape::Staff),
                    ..weapon
                }),
                None,
            ],
        }
    }
}

/// What the player's seat does on one step of a keyed run.
#[derive(Debug, Clone, Copy, Default)]
pub struct Keys {
    pub buttons: u16,
    /// It presses the draw or holster key (`Action::Holster`).
    pub draw: bool,
    /// The weapon slot it wants in hand (`Command::weapon`).
    pub weapon: u8,
}

/// What a keyed run saw of the player after one step.
#[derive(Debug, Clone, Copy)]
pub struct Seen {
    pub tick: u64,
    pub sheath: crate::combat::Sheath,
    /// The blade in hand is lit, or out of its sheath (`SaberState::on`).
    pub on: bool,
    /// The second blade of a pair or of a double-bladed lightsaber is off.
    pub second_off: bool,
    /// `Fighter::weapon_away`.
    pub away: bool,
    pub stealth: bool,
    /// The weapon slot in hand.
    pub hand: u8,
    pub saber_move: crate::combat::saber::Move,
    /// The step lit a lightsaber (`motion::events::SABER_ON`).
    pub lit_event: bool,
}

/// The sneak (`fist_sneak`) holding `weapons`, alone on the proving ground
/// with `anims`.
pub fn sneak_holding(
    anims: &std::sync::Arc<crate::motion::AnimTable>,
    weapons: [Option<crate::combat::sheet::Weapon>; 2],
) -> Result<crate::world::SimWorld, String> {
    let mut sheet = fist_sneak();
    sheet.weapons = weapons;
    world_on(ja_ground(anims)?, sheet)
}

/// Runs steps 1 to `ticks` of `STEP_MS` with the player standing still and
/// its seat pressing what `keys` gives for each tick. `before` may change
/// the world before each step. Returns what each step saw of the player.
/// A probe draws a lightsaber unlit in Stealth Mode with `buttons::STEALTH`
/// on one tick and `draw` on a later one, and swings it with
/// `buttons::ATTACK` on a tick after that.
pub fn keyed_run(
    world: &mut crate::world::SimWorld,
    ticks: u64,
    keys: impl Fn(u64) -> Keys,
    mut before: impl FnMut(&mut crate::world::SimWorld, u64),
) -> Result<Vec<Seen>, String> {
    let mut seen = Vec::new();
    for tick in 1..=ticks {
        before(world, tick);
        let Keys {
            buttons,
            draw,
            weapon,
        } = keys(tick);
        let mut input = TickInput::default();
        input.insert(
            Seat(0),
            Command {
                buttons,
                weapon,
                ..Command::default()
            },
        );
        if draw {
            input.act(Seat(0), crate::action::Action::Holster);
        }
        let snapshot = crate::step(world, tick, &input, STEP_MS, StepReason::Authority);
        let entity = snapshot
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, crate::Kind::Avatar(_)))
            .ok_or("the player is gone")?;
        let fighter = entity.fighter.ok_or("the player has no fight")?;
        let body = entity.player.ok_or("the player has no body")?;
        seen.push(Seen {
            tick,
            sheath: fighter.sheath,
            on: fighter.saber.on,
            second_off: fighter.saber.second_off,
            away: fighter.weapon_away(snapshot.time_ms),
            stealth: fighter.covert.stealth,
            hand: fighter.hand,
            saber_move: fighter.saber.current_move,
            lit_event: body.events & crate::motion::events::SABER_ON != 0,
        });
    }
    Ok(seen)
}

/// When the unlit lightsaber runs press stealth, draw and attack (ticks of
/// `STEP_MS`): the draw a moment after Stealth Mode begins, the attack 2 s
/// after the draw. A run looks on for 2 s after the attack.
const STEALTH_AT: u64 = 1;
const DRAW_AT: u64 = 10;
const SWING_AT: u64 = DRAW_AT + 250;
const LOOKED_TO: u64 = SWING_AT + 250;
/// When a run that swung its lightsaber at no one on its first step
/// presses stealth: 3 s after the swing, when the fight is over but the
/// blade is still lit.
const SWUNG_STEALTH_AT: u64 = 375;

/// The step at `tick`.
fn at(seen: &[Seen], tick: u64) -> Option<Seen> {
    seen.iter().find(|step| step.tick == tick).copied()
}

/// Whether a lightsaber's draw flourish plays on any step from `SWING_AT`
/// on.
fn flourish_after_swing_at(seen: &[Seen]) -> bool {
    seen.iter()
        .any(|step| step.tick >= SWING_AT && step.saber_move == crate::combat::saber::Move::DRAW)
}

/// The keys of a run that presses stealth on `STEALTH_AT`, the draw or
/// holster key on `DRAW_AT`, and attack on `SWING_AT` when `swings`.
fn stealth_draw_keys(swings: bool) -> impl Fn(u64) -> Keys {
    move |tick| Keys {
        buttons: match tick {
            STEALTH_AT => buttons::STEALTH,
            SWING_AT if swings => buttons::ATTACK,
            _ => 0,
        },
        draw: tick == DRAW_AT,
        weapon: 0,
    }
}

/// A sneak holding `hilts` enters Stealth Mode, draws, and attacks 2 s
/// later. The first row checks the draw: every step between the draw and
/// the attack has the blades off, Stealth Mode on and no lighting, and the
/// hilt is in hand (`Sheath::Drawn`, not away). The second row checks the
/// attack's own step: the blades are lit (both, for a pair or a
/// double-bladed lightsaber), a swing has begun with no draw flourish, and
/// Stealth Mode has ended.
fn stealth_swing(
    anims: &std::sync::Arc<crate::motion::AnimTable>,
    hilts: Hilts,
) -> Result<[Result<String, String>; 2], String> {
    let mut world = sneak_holding(anims, hilts.weapons())?;
    let seen = keyed_run(&mut world, LOOKED_TO, stealth_draw_keys(true), |_, _| {})?;
    let unlit = seen
        .iter()
        .filter(|step| step.tick > DRAW_AT && step.tick < SWING_AT)
        .all(|step| !step.on && step.stealth && !step.lit_event);
    let held = at(&seen, SWING_AT - 1);
    let in_hand = held.is_some_and(|step| step.sheath == Sheath::Drawn && !step.away);
    let swing = at(&seen, SWING_AT);
    let both = hilts == Hilts::One || swing.is_some_and(|step| !step.second_off);
    let swung = swing.is_some_and(|step| {
        let swings = step.saber_move.start() || step.saber_move.attack();
        step.on && swings && !step.stealth && step.lit_event
    });
    let flourish = flourish_after_swing_at(&seen);
    Ok([
        check(
            unlit && in_hand,
            format!("unlit between the draw and the attack {unlit}; before the attack {held:?}"),
        ),
        check(
            swung && both && !flourish,
            format!("the attack's step {swing:?}; a draw flourish after it {flourish}"),
        ),
    ])
}

/// A sneak holding a lit lightsaber presses stealth: drawn by its key
/// (`Sheath::Drawn`) 2 s before, or swung at no one 3 s before
/// (`Sheath::Fight`, out of combat but within the 8 s the blade stays lit).
/// The step after the press has the blade off, the sheath `Sheath::Drawn`
/// and Stealth Mode on, and no step from the press on plays the put-away
/// flourish or lights the blade.
fn lit_enters_stealth(
    anims: &std::sync::Arc<crate::motion::AnimTable>,
    drawn: bool,
) -> Result<String, String> {
    use crate::combat::saber::Move;
    let press = if drawn { SWING_AT } else { SWUNG_STEALTH_AT };
    let mut world = sneak_holding(anims, Hilts::One.weapons())?;
    let seen = keyed_run(
        &mut world,
        press + 250,
        |tick| Keys {
            buttons: match tick {
                1 if !drawn => buttons::ATTACK,
                t if t == press => buttons::STEALTH,
                _ => 0,
            },
            draw: drawn && tick == DRAW_AT,
            weapon: 0,
        },
        |_, _| {},
    )?;
    let lit = at(&seen, press);
    let next = at(&seen, press + 1);
    let off = next
        .is_some_and(|step| !step.on && step.sheath == Sheath::Drawn && !step.away && step.stealth);
    let quiet = seen
        .iter()
        .filter(|step| step.tick > press)
        .all(|step| step.saber_move != Move::PUTAWAY && !step.on);
    check(
        lit.is_some_and(|step| step.on) && off && quiet,
        format!("at the press {lit:?}; the next step {next:?}; no put-away after {quiet}"),
    )
}

/// A sneak draws its lightsaber unlit in Stealth Mode, then combat begins
/// (`Fighter::combat_until`): Stealth Mode ends and the blade lights by the
/// normal rule, its draw flourish played, within 2 s.
fn combat_lights(anims: &std::sync::Arc<crate::motion::AnimTable>) -> Result<String, String> {
    let mut world = sneak_holding(anims, Hilts::One.weapons())?;
    let seen = keyed_run(
        &mut world,
        LOOKED_TO,
        stealth_draw_keys(false),
        |world, tick| {
            if tick == SWING_AT {
                set_combat(world, u64::MAX / 2);
            }
        },
    )?;
    let before = at(&seen, SWING_AT - 1);
    let lit = seen
        .iter()
        .find(|step| step.tick >= SWING_AT && step.on)
        .copied();
    let flourish = flourish_after_swing_at(&seen);
    check(
        before.is_some_and(|step| !step.on && step.stealth)
            && lit.is_some_and(|step| !step.stealth && step.lit_event)
            && flourish,
        format!("before combat {before:?}; first lit {lit:?}; draw flourish {flourish}"),
    )
}

/// A sneak holding a blaster and a lightsaber draws the blaster in Stealth
/// Mode, then changes to the lightsaber: 2 s later the lightsaber is in
/// hand (`Sheath::Drawn`, not away), unlit, and Stealth Mode holds.
fn change_in_stealth(anims: &std::sync::Arc<crate::motion::AnimTable>) -> Result<String, String> {
    let saber = Hilts::One.weapons()[0];
    let mut world = sneak_holding(anims, [Armed::Blaster.weapon(), saber])?;
    let keys = stealth_draw_keys(false);
    let seen = keyed_run(
        &mut world,
        LOOKED_TO,
        |tick| Keys {
            weapon: u8::from(tick >= SWING_AT),
            ..keys(tick)
        },
        |_, _| {},
    )?;
    let drawn = at(&seen, SWING_AT - 1);
    let last = at(&seen, LOOKED_TO);
    let lit = seen
        .iter()
        .find(|step| step.tick >= SWING_AT && (step.on || step.lit_event || !step.stealth));
    check(
        drawn.is_some_and(|step| step.sheath == Sheath::Drawn && !step.away)
            && last
                .is_some_and(|step| step.hand == 1 && step.sheath == Sheath::Drawn && !step.away)
            && lit.is_none(),
        format!("the blaster drawn {drawn:?}; 2 s after the change {last:?}; lit {lit:?}"),
    )
}

/// A lightsaber drawn in Stealth Mode, on the proving ground with `anims`
/// (JA's animations and skeleton, which a swing needs). Two rows for each
/// of `Hilts` (`stealth_swing`), two for a lit blade entering Stealth Mode
/// (`lit_enters_stealth`), one for combat ending Stealth Mode
/// (`combat_lights`) and one for a change of weapons (`change_in_stealth`).
pub fn unlit_sabers(
    anims: &std::sync::Arc<crate::motion::AnimTable>,
) -> Vec<(&'static str, Result<String, String>)> {
    let mut rows = Vec::new();
    for hilts in Hilts::ALL {
        let names = match hilts {
            Hilts::One => ["unlit_saber_in_stealth", "unlit_saber_lights_on_swing"],
            Hilts::Pair => ["unlit_pair_in_stealth", "unlit_pair_lights_on_swing"],
            Hilts::Double => ["unlit_double_in_stealth", "unlit_double_lights_on_swing"],
        };
        match stealth_swing(anims, hilts) {
            Ok(checks) => rows.extend(names.into_iter().zip(checks)),
            Err(why) => rows.extend(failed(names, why)),
        }
    }
    rows.extend([
        (
            "drawn_saber_enters_stealth",
            lit_enters_stealth(anims, true),
        ),
        (
            "fight_saber_enters_stealth",
            lit_enters_stealth(anims, false),
        ),
        ("unlit_saber_lights_in_combat", combat_lights(anims)),
        ("weapon_change_in_stealth", change_in_stealth(anims)),
    ]);
    rows
}

/// The opener out of stealth (S14).
pub(crate) fn opener() -> Vec<(&'static str, Result<String, String>)> {
    // A guard fighting someone else, not perceiving the stealthed player:
    // the first blow is a sneak attack, the rest not (it is not behind,
    // held or unaware otherwise); the opener spent.
    let unseen = opener_run(2);
    // A guard that spots the player through its stealth (Awareness 40):
    // no opener.
    let seen = opener_run(40);
    vec![
        (
            "opener_sneaks_once",
            check(
                unseen
                    .as_ref()
                    .is_ok_and(|(sneaks, opener)| *sneaks == 1 && *opener == Some(0)),
                format!("{unseen:?}"),
            ),
        ),
        (
            "opener_seen_no_sneak",
            check(
                seen.as_ref().is_ok_and(|(sneaks, _)| *sneaks == 0),
                format!("{seen:?}"),
            ),
        ),
    ]
}

/// The area's stealth XP now.
fn stealth_xp(world: &crate::world::SimWorld) -> Option<i32> {
    world
        .script_state()
        .stealth_xp
        .values()
        .next()
        .map(|xp| xp.current)
}

/// Stealth XP's loss (S15): a guard newly seeing the player costs the
/// area's loss 6 s later; one that attacks meanwhile costs it at once.
pub(crate) fn stealth_xp_loss() -> Vec<(&'static str, Result<String, String>)> {
    let watched_by = |faction: u8| -> Result<(Option<i32>, Option<i32>), String> {
        let mut world = guarded_by(sneak(0, false), (2, faction, 0.0), true, false)?;
        let mut before = None;
        for tick in 1..=800 {
            stand(&mut world, SNEAK_AT);
            crate::step(
                &mut world,
                tick,
                &TickInput::default(),
                8,
                StepReason::Authority,
            );
            if tick == 700 {
                before = stealth_xp(&world);
            }
        }
        Ok((before, stealth_xp(&world)))
    };
    let seen_run = watched_by(crate::combat::sheet::factions::HOSTILE_1);
    // One of the player's own faction (a companion's) seeing it costs none.
    let friend_run = watched_by(crate::combat::sheet::factions::PLAYER);
    let attacked_run = (|| -> Result<Option<i32>, String> {
        let mut world = guarded(sneak(0, false), 2, true)?;
        let (_, player) = ids(&world.snapshot()).ok_or("no bodies")?;
        for tick in 1..=100 {
            stand(&mut world, SNEAK_AT);
            let mut fighters = world
                .world
                .query::<(&crate::world::Object, &mut crate::world::Fighting)>();
            for (object, mut fighting) in fighters.iter_mut(&mut world.world) {
                if object.index == GUARD && tick > 10 {
                    fighting.0.target = Some(crate::SimId(player));
                    fighting.0.combat_until = u64::MAX / 2;
                }
            }
            crate::step(
                &mut world,
                tick,
                &TickInput::default(),
                8,
                StepReason::Authority,
            );
        }
        Ok(stealth_xp(&world))
    })();
    vec![
        (
            "stealth_xp_lost_after_6s",
            check(
                seen_run
                    .as_ref()
                    .is_ok_and(|(before, after)| *before == Some(60) && *after == Some(40)),
                format!("{seen_run:?}"),
            ),
        ),
        (
            "stealth_xp_kept_by_friends",
            check(
                // Untouched: the area's 60 (its state made on a change).
                friend_run
                    .as_ref()
                    .is_ok_and(|(_, after)| matches!(after, None | Some(60))),
                format!("{friend_run:?}"),
            ),
        ),
        (
            "stealth_xp_lost_on_attack",
            check(
                attacked_run.as_ref().is_ok_and(|xp| *xp == Some(40)),
                format!("{attacked_run:?}"),
            ),
        ),
    ]
}
