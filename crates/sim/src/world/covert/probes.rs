//! The probes of `world::covert` (`sim::stealth::probes`; the approved
//! test `stealth_spot`): stealth mode on the proving ground.

use crate::combat::Sheet;
use crate::combat::sheet::{flags, skills};
use crate::input::{Seat, buttons};
use crate::notice::Notice;
use crate::probe::check;
use crate::world::locks::lines;
use crate::{Command, StepReason, TickInput};

/// The soldier with `stealth` ranks, wearing a stealth unit or not.
fn sneak(stealth: u8, unit: bool) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.skills[skills::STEALTH] = stealth;
    if unit {
        sheet.flags |= flags::STEALTH_UNIT;
    }
    sheet
}

fn world(sheet: Sheet) -> Result<crate::world::SimWorld, String> {
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
        crate::determinism::proving_ground(),
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
        (awareness, crate::combat::sheet::factions::HOSTILE_1),
        scripted,
        dummy,
    )
}

/// `guarded_with`, the guard (Awareness, faction) as given.
fn guarded_by(
    sheet: Sheet,
    (awareness, faction): (u8, u8),
    scripted: bool,
    dummy: bool,
) -> Result<crate::world::SimWorld, String> {
    use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
    use crate::module::{ObjectData, Placement, Repute, Senses};
    let ground = crate::determinism::proving_ground();
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
            0.0,
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

/// The player stealthed 1 m from the guard (Awareness `awareness`), the
/// guard fighting a dummy, then the player attacking for 7 s (fists land
/// 3 s apart: three blows): the "Sneak Attack!" lines it was told, and its
/// opener after.
fn opener_run(awareness: u8) -> Result<(usize, Option<u64>), String> {
    let mut world = guarded_with(fist_sneak(), awareness, true, true)?;
    let close = [GUARD_AT[0] + 1.0, GUARD_AT[1]];
    // Facing the guard (as `fists_brawl` faces its foe).
    let (from, to) = (
        crate::determinism::ground_point(close[0], close[1], 0.0),
        crate::determinism::ground_point(GUARD_AT[0], GUARD_AT[1], 0.0),
    );
    let yaw = crate::math::atan2_deg(to[1] - from[1], to[0] - from[0]);
    let facing = Command {
        angles: crate::moves::view_angles(0.0, yaw),
        ..Command::default()
    };
    let mut sneaks = 0;
    let mut last = world.snapshot();
    for tick in 1..=900 {
        stand(&mut world, close);
        busy_guard(&mut world);
        let mut input = TickInput::default();
        let buttons = match tick {
            1 => buttons::STEALTH,
            // Let the guard's looks pass, then attack.
            t if t >= 30 => buttons::ATTACK,
            _ => 0,
        };
        input.insert(Seat(0), Command { buttons, ..facing });
        last = crate::step(&mut world, tick, &input, 8, StepReason::Authority);
        sneaks += world
            .take_notices()
            .iter()
            .filter(|(_, notice)| *notice == Notice::Line(lines::SNEAK_ATTACK))
            .count();
    }
    Ok((sneaks, player(&last).map(|f| f.covert.opener_until)))
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
        let mut world = guarded_by(sneak(0, false), (2, faction), true, false)?;
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
