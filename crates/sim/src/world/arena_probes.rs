//! The probes of `arena` (`kjk sim probes`): on the proving ground as an
//! arena, a foe called in stands ahead hostile and is taken away by a
//! clear, god mode turns on and off, and a player in the pit goes down and
//! comes back whole at the respawn point.

use std::sync::Arc;

use super::SimWorld;
use super::arena::{Rules, SPAWN_AHEAD, TERMINAL_TAG, TERMINAL_TEMPLATE};
use crate::action::{Action, ArenaOrder};
use crate::combat::Sheet;
use crate::combat::sheet::{factions, flags};
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind};
use crate::probe::check;
use crate::snapshot::{Kind, SimId, Snapshot};
use crate::{StepReason, WorldConfig};

/// The companion's tag in `restore_companion`.
const MATE: &str = "probe_mate";
/// The foe's template, friendly as written: the arena turns it.
const FOE: &str = "probe_foe";
/// How long a downed player lies in the probes' arena (ms).
const RESPAWN_MS: u64 = 500;

/// Four classes of twenty levels alike (`coop_items::classes`): rows 0 to
/// 3, so that row 3, K1's Jedi Guardian, may be taken on top of row 0
/// (`Classes::second_classes`).
fn classes() -> crate::levels::Classes {
    let base = super::coop_items::classes();
    crate::levels::Classes {
        classes: vec![base.classes[0].clone(); 4],
        ..base
    }
}

/// The proving ground as an arena for one seat (`arena_for`).
fn arena() -> Result<(SimWorld, Rules), String> {
    arena_for(1, true, false).map(|(world, rules, _)| (world, rules))
}

/// The proving ground for `seats` players, an arena when `rules`: the
/// foe's template, two classes and the Debug Mode
/// terminal's template in its library. The arena's step makes the terminal
/// 1 m from where the players arrive. With `mate`, a creature tagged
/// `MATE` stands 2 m from the arrival (`companion` makes it one). The
/// world, its rules and the terminal's object index (the first one play
/// makes).
fn arena_for(seats: u8, rules: bool, mate: bool) -> Result<(SimWorld, Rules, u32), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry;
    if mate {
        let [x, y, z] = entry.position;
        let sheet = Sheet::dev_soldier();
        let creature = ObjectData::creature(MATE, MATE, [x, y + 2.0, z], sheet);
        data.placements
            .push(crate::module::Placement::Own(creature));
    }
    let terminal = data.placements.len() as u32;
    let [x, y, z] = entry.position;
    let arena = Rules {
        respawn: entry.position,
        yaw: entry.yaw,
        pit: -3.0,
        respawn_ms: RESPAWN_MS,
        terminal: Some(super::arena::Terminal {
            at: [x + 1.0, y, z],
            yaw: entry.yaw,
            facts: Arc::new(ObjectData {
                useable: true,
                template: TERMINAL_TEMPLATE.into(),
                ..ObjectData::bare(ObjectKind::Placeable, TERMINAL_TAG, [0.0; 3])
            }),
        }),
    };
    let mut foe = ObjectData::creature(FOE, FOE, [0.0; 3], Sheet::dev_soldier());
    if let Some(sheet) = foe.sheet.as_mut() {
        sheet.faction = factions::PLAYER;
        sheet.flags |= flags::PLOT;
    }
    let library = crate::probe::library("", std::iter::empty::<(String, _)>())
        .with_creatures([(FOE.to_string(), Arc::new(foe))].into())
        .with_classes(classes());
    let mut level = ground.with_module(data).with_scripts(library);
    if rules {
        level = level.with_arena(arena.clone());
    }
    let config = WorldConfig {
        seats,
        guests: crate::Guests::Avatars,
        ..WorldConfig::one_seat()
    };
    let world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
    Ok((world, arena, terminal))
}

/// `steps` steps of seat 0 standing, `orders` given on the first.
fn run(world: &mut SimWorld, orders: &[ArenaOrder], steps: u32) -> Snapshot {
    let orders: Vec<_> = orders
        .iter()
        .map(|order| (Seat(0), order.clone()))
        .collect();
    run_seats(world, 1, &orders, steps).0
}

/// `steps` steps of `seats` seats standing, each order given by its seat
/// on the first: the last snapshot, and every notice with its seat.
fn run_seats(
    world: &mut SimWorld,
    seats: u8,
    orders: &[(Seat, ArenaOrder)],
    steps: u32,
) -> (Snapshot, Vec<(Seat, crate::notice::Notice)>) {
    let actions: Vec<_> = orders
        .iter()
        .map(|(seat, order)| (*seat, Action::Arena(order.clone())))
        .collect();
    act(world, seats, &actions, steps)
}

/// `run_seats` with any actions.
fn act(
    world: &mut SimWorld,
    seats: u8,
    actions: &[(Seat, Action)],
    steps: u32,
) -> (Snapshot, Vec<(Seat, crate::notice::Notice)>) {
    let mut last = world.snapshot();
    let mut notices = Vec::new();
    for step in 0..steps {
        let mut input = TickInput::default();
        for seat in 0..seats {
            input.insert(Seat(seat), Command::default());
        }
        if step == 0 {
            for (seat, action) in actions {
                input.act(*seat, action.clone());
            }
        }
        let tick = world.tick() + 1;
        last = crate::step(world, tick, &input, 8, StepReason::Authority);
        notices.extend(world.take_notices());
    }
    (last, notices)
}

/// The creatures standing in `snapshot` that no seat plays.
fn foes(snapshot: &Snapshot) -> Vec<&crate::snapshot::EntityState> {
    snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)))
        .filter(|entity| entity.fighter.is_some_and(|fighter| fighter.alive()))
        .collect()
}

pub fn probes() -> crate::probe::Checks {
    vec![
        ("arena_spawn_and_clear", spawn_and_clear()),
        ("arena_god", god()),
        ("arena_pit_and_respawn", pit_and_respawn()),
        ("arena_down_gets_up_by_its_side", down_gets_up()),
        ("arena_terminal_sets_one_field", sets_one_field()),
        ("arena_terminal_constitution", constitution()),
        ("arena_terminal_rebuild", rebuild()),
        ("arena_terminal_second_class", second_class()),
        ("arena_terminal_restore", restore()),
        ("arena_terminal_restore_companion", restore_companion()),
        ("arena_terminal_seats", seats_rule()),
        ("arena_terminal_off_arena", off_arena()),
        ("arena_terminal_made_once", made_once()),
        ("arena_terminal_opens", opens()),
        ("arena_terminal_round_trip", round_trip()),
    ]
}

/// Seat `seat`'s character's id and sheet in `snapshot`.
fn mine(snapshot: &Snapshot, seat: Seat) -> Result<(SimId, Sheet), String> {
    let me = snapshot.own(seat).ok_or("no player")?;
    Ok((me.id, me.sheet.ok_or("no sheet")?))
}

/// Each sheet order changes the field it names and nothing else, and a
/// number past a limit is held to the limit.
fn sets_one_field() -> Result<String, String> {
    use super::arena::edits::limits;
    let (mut world, _) = arena()?;
    let (me, start) = mine(&run(&mut world, &[], 2), Seat(0))?;
    type Expect = fn(&Sheet) -> Sheet;
    let cases: [(&str, ArenaOrder, Expect); 5] = [
        (
            "Strength 17",
            ArenaOrder::Ability {
                who: me,
                ability: 0,
                score: 17,
            },
            |s| {
                let mut abilities = s.abilities;
                abilities[0] = 17;
                Sheet { abilities, ..*s }
            },
        ),
        (
            "Dexterity past the most",
            ArenaOrder::Ability {
                who: me,
                ability: 1,
                score: u8::MAX,
            },
            |s| {
                let mut abilities = s.abilities;
                abilities[1] = limits::ABILITY_MOST;
                Sheet { abilities, ..*s }
            },
        ),
        (
            "Repair 7",
            ArenaOrder::Skill {
                who: me,
                skill: 5,
                ranks: 7,
            },
            |s| {
                let mut skills = s.skills;
                skills[5] = 7;
                Sheet { skills, ..*s }
            },
        ),
        (
            "Security past the most",
            ArenaOrder::Skill {
                who: me,
                skill: 6,
                ranks: u8::MAX,
            },
            |s| {
                let mut skills = s.skills;
                skills[6] = limits::SKILL_MOST;
                Sheet { skills, ..*s }
            },
        ),
        (
            "alignment past the most",
            ArenaOrder::Alignment {
                who: me,
                good_evil: u8::MAX,
            },
            |s| Sheet {
                good_evil: limits::ALIGNMENT_MOST,
                ..*s
            },
        ),
    ];
    let mut seen = Vec::new();
    for (name, order, expect) in cases {
        let (mut world, _) = arena()?;
        let (_, after) = mine(&run(&mut world, &[order], 3), Seat(0))?;
        let right = after == expect(&start);
        seen.push(format!("{name}: {right}"));
        if !right {
            return Err(seen.join("; "));
        }
    }
    Ok(seen.join("; "))
}

/// A Constitution raised by 2 raises the most hit points by 1 a level.
fn constitution() -> Result<String, String> {
    let (mut world, _) = arena()?;
    let (me, start) = mine(&run(&mut world, &[], 2), Seat(0))?;
    let raised = ArenaOrder::Ability {
        who: me,
        ability: 2,
        score: start.abilities[2] + 2,
    };
    let (_, after) = mine(&run(&mut world, &[raised], 3), Seat(0))?;
    let rise = i32::from(after.max_hp) - i32::from(start.max_hp);
    check(
        rise == i32::from(start.level),
        format!("level {}: most hit points up by {rise}", start.level),
    )
}

/// A rebuild to class 1 at level 4 is a level-4 character whose career
/// is four levels of that class, at full hit points, with the party's XP
/// what the level needs. A second rebuild gives the same sheet. A rebuild
/// at level 1 is character creation's first level of the class.
fn rebuild() -> Result<String, String> {
    let (mut world, _) = arena()?;
    let (me, start) = mine(&run(&mut world, &[], 2), Seat(0))?;
    let order = |level| ArenaOrder::Rebuild {
        who: me,
        class: 1,
        level,
        second: None,
    };
    let once = run(&mut world, &[order(4)], 3);
    let (_, sheet) = mine(&once, Seat(0))?;
    let hp = once.own(Seat(0)).and_then(|me| me.fighter).map(|f| f.hp);
    let career = world.script_state().careers.get(&me.0).cloned();
    let classes = classes();
    let xp = world.script_state().xp(crate::inventory::PARTY);
    let (_, twice) = mine(&run(&mut world, &[order(4)], 3), Seat(0))?;
    let (_, first) = mine(&run(&mut world, &[order(1)], 3), Seat(0))?;
    let created = crate::creation::first_level(
        &classes,
        classes.classes[1].recommended,
        start.faction,
        &crate::levels::Choices {
            class: 1,
            ..Default::default()
        },
    )
    .ok_or("no first level")?
    .0;
    let numbers = |s: &Sheet| {
        (
            s.abilities,
            s.level,
            s.bab,
            s.saves,
            s.max_hp,
            s.feats,
            s.skills,
        )
    };
    check(
        sheet.level == 4
            && career == Some(vec![(1, 4)])
            && hp == Some(i32::from(sheet.max_hp) * 100)
            && xp >= classes.next(3).unwrap_or(0)
            && twice == sheet
            && numbers(&first) == numbers(&created),
        format!(
            "level {}, career {career:?}, hit points {hp:?} of {}, XP {xp}; again the same: {}; level 1 as created: {}",
            sheet.level,
            sheet.max_hp,
            twice == sheet,
            numbers(&first) == numbers(&created)
        ),
    )
}

/// A rebuild as class 0 at level 6 with two levels of class 3 (a Jedi on a
/// soldier) is a level-6 character whose career is four levels of class 0
/// then two of class 3. A second class the first may not take (class 1 on
/// class 0) changes nothing.
fn second_class() -> Result<String, String> {
    let (mut world, _) = arena()?;
    let (me, _) = mine(&run(&mut world, &[], 2), Seat(0))?;
    let order = |second| ArenaOrder::Rebuild {
        who: me,
        class: 0,
        level: 6,
        second: Some(second),
    };
    let (_, sheet) = mine(&run(&mut world, &[order((3, 2))], 3), Seat(0))?;
    let career = world.script_state().careers.get(&me.0).cloned();
    let (_, refused) = mine(&run(&mut world, &[order((1, 2))], 3), Seat(0))?;
    let kept = world.script_state().careers.get(&me.0).cloned();
    check(
        sheet.level == 6
            && career == Some(vec![(0, 4), (3, 2)])
            && refused == sheet
            && kept == career,
        format!(
            "level {}, career {career:?}; a second class not allowed keeps the sheet: {}, career {kept:?}",
            sheet.level,
            refused == sheet
        ),
    )
}

/// A restore makes the character the starting one again, at full hit
/// points.
fn restore() -> Result<String, String> {
    let (mut world, _) = arena()?;
    let (me, start) = mine(&run(&mut world, &[], 2), Seat(0))?;
    let jedi = Sheet::dev_jedi();
    let changed = [
        ArenaOrder::Rebuild {
            who: me,
            class: 0,
            level: 3,
            second: None,
        },
        ArenaOrder::Restore {
            who: me,
            sheet: Box::new(jedi),
        },
    ];
    let back = run(&mut world, &changed, 3);
    let (_, sheet) = mine(&back, Seat(0))?;
    let hp = back.own(Seat(0)).and_then(|me| me.fighter).map(|f| f.hp);
    let numbers = |s: &Sheet| {
        (
            s.abilities,
            s.level,
            s.max_hp,
            s.max_fp,
            s.feats,
            s.powers,
            s.skills,
        )
    };
    check(
        numbers(&sheet) == numbers(&jedi)
            && sheet.faction == start.faction
            && hp == Some(i32::from(sheet.max_hp) * 100),
        format!(
            "as the starting character: {}; faction kept: {}; hit points {hp:?} of {}",
            numbers(&sheet) == numbers(&jedi),
            sheet.faction == start.faction,
            sheet.max_hp
        ),
    )
}

/// A companion can be edited at the terminal, but a restore naming it
/// changes nothing: the order holds the starting player character, and
/// the terminal keeps no starting state of a companion's.
fn restore_companion() -> Result<String, String> {
    let (mut world, ..) = arena_for(1, true, true)?;
    let start = run(&mut world, &[], 2);
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let mate = (start.entities.iter())
        .find(|entity| {
            objects
                .of_entity(entity)
                .is_some_and(|data| data.tag == MATE)
        })
        .map(|entity| entity.id)
        .ok_or("no companion")?;
    // In the party table, following seat 0, as `AddPartyMember` leaves it.
    let mut state = world.script_state().clone();
    state.party.available.insert(
        0,
        crate::script::party::Npc {
            template: MATE.into(),
            object: Some(mate.0),
            kept: None,
            template_data: None,
        },
    );
    state.party.members.push(0);
    world.set_script_state(state);
    let mut fighters = world.world.query::<(&super::Id, &mut super::Fighting)>();
    for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
        if id.0 == mate {
            fighting.0.mind.leader = Some(Seat(0));
        }
    }
    // A sheet nothing changed yet is its template's.
    let template = (start.get(mate))
        .and_then(|entity| objects.of_entity(entity))
        .and_then(|facts| facts.sheet);
    let sheet_of =
        |snapshot: &Snapshot| (snapshot.get(mate).and_then(|entity| entity.sheet)).or(template);
    let before = sheet_of(&run(&mut world, &[], 2)).ok_or("no sheet")?;
    let orders = [
        ArenaOrder::Ability {
            who: mate,
            ability: 0,
            score: 30,
        },
        ArenaOrder::Restore {
            who: mate,
            sheet: Box::new(Sheet::dev_jedi()),
        },
    ];
    let after = sheet_of(&run(&mut world, &orders, 3)).ok_or("no sheet")?;
    let edited = after.abilities[0] == 30;
    let restored = after.level != before.level || after.powers != before.powers;
    check(
        edited && !restored,
        format!(
            "Strength set to {}; made the starting Jedi: {restored}",
            after.abilities[0]
        ),
    )
}

/// An order from seat 1 that names seat 0's character changes nothing. The
/// same order naming seat 1's own character changes it.
fn seats_rule() -> Result<String, String> {
    let (mut world, ..) = arena_for(2, true, false)?;
    let (start, _) = run_seats(&mut world, 2, &[], 2);
    let (host, host_sheet) = mine(&start, Seat(0))?;
    let (guest, _) = mine(&start, Seat(1))?;
    let strength = |who| ArenaOrder::Ability {
        who,
        ability: 0,
        score: 30,
    };
    let orders = [(Seat(1), strength(host)), (Seat(1), strength(guest))];
    let (after, _) = run_seats(&mut world, 2, &orders, 3);
    let (_, host_after) = mine(&after, Seat(0))?;
    let (_, guest_after) = mine(&after, Seat(1))?;
    check(
        host_after == host_sheet && guest_after.abilities[0] == 30,
        format!(
            "host's Strength {} (was {}), guest's {}",
            host_after.abilities[0], host_sheet.abilities[0], guest_after.abilities[0]
        ),
    )
}

/// The terminal's orders on a level without arena rules change nothing.
fn off_arena() -> Result<String, String> {
    let (mut world, ..) = arena_for(1, false, false)?;
    let start = run(&mut world, &[], 2);
    let (me, sheet) = mine(&start, Seat(0))?;
    let orders = [
        ArenaOrder::Ability {
            who: me,
            ability: 0,
            score: 30,
        },
        ArenaOrder::Alignment {
            who: me,
            good_evil: 0,
        },
    ];
    let after = run(&mut world, &orders, 3);
    let (_, now) = mine(&after, Seat(0))?;
    check(now == sheet, format!("sheet kept: {}", now == sheet))
}

/// The arena's step makes the terminal once, and a level without arena
/// rules has none.
fn made_once() -> Result<String, String> {
    let made = |snapshot: &Snapshot| {
        (snapshot.spawned.iter())
            .filter(|spawned| spawned.template == TERMINAL_TEMPLATE)
            .count()
    };
    let (mut world, ..) = arena_for(1, true, false)?;
    let arena = made(&run(&mut world, &[], 20));
    let (mut world, ..) = arena_for(1, false, false)?;
    let other = made(&run(&mut world, &[], 20));
    check(
        arena == 1 && other == 0,
        format!("terminals made in the arena: {arena}; elsewhere: {other}"),
    )
}

/// Seat 0 using the terminal opens its screen for seat 0 and no other.
fn opens() -> Result<String, String> {
    use crate::notice::{Notice, Screen};
    let (mut world, _, terminal) = arena_for(2, true, false)?;
    run_seats(&mut world, 2, &[], 2);
    let used = Action::Use {
        object: terminal,
        verb: crate::action::Verb::Use,
    };
    let (_, notices) = act(&mut world, 2, &[(Seat(0), used)], 30);
    let told: Vec<Seat> = notices
        .iter()
        .filter(|(_, notice)| *notice == Notice::Show(Screen::DebugTerminal))
        .map(|(seat, _)| *seat)
        .collect();
    check(told == [Seat(0)], format!("told to {told:?}"))
}

/// Every terminal order is read back as it was written.
fn round_trip() -> Result<String, String> {
    let who = SimId(7);
    let orders = [
        ArenaOrder::Rebuild {
            who,
            class: 3,
            level: 12,
            second: Some((11, 4)),
        },
        ArenaOrder::Rebuild {
            who,
            class: 0,
            level: 2,
            second: None,
        },
        ArenaOrder::Ability {
            who,
            ability: 2,
            score: 18,
        },
        ArenaOrder::Skill {
            who,
            skill: 4,
            ranks: 9,
        },
        ArenaOrder::Alignment { who, good_evil: 20 },
        ArenaOrder::Restore {
            who,
            sheet: Box::new(Sheet::dev_jedi()),
        },
    ];
    for order in orders {
        let action = Action::Arena(order);
        let mut out = crate::wire::Writer::default();
        action.encode(&mut out);
        let back = Action::decode(&mut crate::wire::Reader::new("action", &out.0))
            .map_err(|error| error.to_string())?;
        if back != action {
            return Err(format!("{action:?} came back as {back:?}"));
        }
    }
    Ok("every order came back as written".into())
}

/// `down N` for a death that leaves the body on its back (1) and one on its
/// front (4): it plays that death, lies past the respawn, and gets up where
/// it lies in the get-up for that side (`death::getup`).
fn down_gets_up() -> Result<String, String> {
    use crate::motion::Anim;
    let mut seen = Vec::new();
    for (n, death, getup) in [
        (1, Anim::BOTH_DEATH1, Anim::BOTH_GETUP4),
        (4, Anim::BOTH_DEATH4, Anim::BOTH_GETUP3),
    ] {
        let (mut world, _) = arena()?;
        let start = run(&mut world, &[], 2)
            .own(Seat(0))
            .ok_or("no player")?
            .position;
        let fell = run(&mut world, &[ArenaOrder::Down(n)], 2);
        let me = fell.own(Seat(0)).ok_or("no player")?;
        let lies = me.fighter.is_some_and(|f| f.downed())
            && me.player.is_some_and(|p| p.legs.anim == death);
        let steps = super::falls::ARENA_REVIVE_MS / 8 + 2;
        let up = run(&mut world, &[], steps);
        let me = up.own(Seat(0)).ok_or("no player")?;
        let rose = me.fighter.is_some_and(|f| f.alive())
            && me.player.is_some_and(|p| p.legs.anim == getup);
        let moved = (me.position[0] - start[0]).hypot(me.position[1] - start[1]);
        seen.push(format!(
            "down {n}: lay in {death:?} {lies}, up in {getup:?} {rose}, {moved:.2} m from where it fell"
        ));
        if !lies || !rose || moved > 0.1 {
            return Err(seen.join("; "));
        }
    }
    Ok(seen.join("; "))
}

/// Two foes called in stand `SPAWN_AHEAD` ahead, hostile and killable
/// though their template is a friendly plot creature; a clear takes both.
fn spawn_and_clear() -> Result<String, String> {
    let (mut world, _) = arena()?;
    let start = world.snapshot();
    let me = start.own(Seat(0)).ok_or("no player")?.position;
    let spawn = ArenaOrder::Spawn(FOE.to_string());
    let called = run(&mut world, &[spawn.clone(), spawn], 30);
    let standing = foes(&called);
    let apart: Vec<f32> = standing
        .iter()
        .map(|foe| (foe.position[0] - me[0]).hypot(foe.position[1] - me[1]))
        .collect();
    let hostile = standing.iter().all(|foe| {
        foe.sheet.is_some_and(|sheet| {
            sheet.faction == factions::HOSTILE_1 && sheet.flags & flags::PLOT == 0
        })
    });
    let cleared = run(&mut world, &[ArenaOrder::Clear], 30);
    let left = foes(&cleared).len();
    check(
        standing.len() == 2
            && apart.iter().all(|m| (m - SPAWN_AHEAD).abs() < 0.5)
            && hostile
            && left == 0,
        format!(
            "{} called in at {apart:?} m (hostile, no plot: {hostile}); {left} left after a clear",
            standing.len()
        ),
    )
}

/// God mode on, then off again.
fn god() -> Result<String, String> {
    let (mut world, _) = arena()?;
    let immortal = |snapshot: &Snapshot| {
        snapshot
            .own(Seat(0))
            .and_then(|me| me.sheet)
            .is_some_and(|sheet| sheet.flags & flags::IMMORTAL != 0)
    };
    let on = immortal(&run(&mut world, &[ArenaOrder::God], 3));
    let off = immortal(&run(&mut world, &[ArenaOrder::God], 3));
    check(on && !off, format!("on: {on}; off again: {}", !off))
}

/// The player under the pit's floor goes down, then after `RESPAWN_MS`
/// stands at the respawn point whole, and the arena is not lost.
fn pit_and_respawn() -> Result<String, String> {
    let (mut world, rules) = arena()?;
    let mut start = world.snapshot();
    let at = start
        .entities
        .iter()
        .position(|entity| entity.kind == Kind::Avatar(Seat(0)))
        .ok_or("no player")?;
    if let Some(player) = start.entities[at].player.as_mut() {
        player.origin[2] = (rules.pit - 1.0) / collision::UNIT;
    }
    if let Some(fighter) = start.entities[at].fighter.as_mut() {
        fighter.hp /= 2;
    }
    let mut world = SimWorld::from_snapshot(&start, world.level().clone())
        .map_err(|error| error.to_string())?;
    let fell = run(&mut world, &[], 2);
    let down = fell.entities[at].fighter.is_some_and(|f| f.downed());
    let back = run(&mut world, &[], (RESPAWN_MS / 8) as u32 + 10);
    let me = &back.entities[at];
    let whole = me.fighter.is_some_and(|f| f.alive())
        && me.sheet.is_some_and(|sheet| {
            me.fighter
                .is_some_and(|f| f.hp == i32::from(sheet.max_hp) * 100)
        });
    let [x, y, _] = me.position;
    let home = (x - rules.respawn[0]).hypot(y - rules.respawn[1]);
    check(
        down && whole && home < 0.1 && back.lost.is_none(),
        format!(
            "down after the fall: {down}; back whole: {whole}, {home:.2} m from the respawn point; lost: {:?}",
            back.lost
        ),
    )
}
