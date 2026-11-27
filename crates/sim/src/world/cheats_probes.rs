//! The probes of `cheats` (`kjk sim probes`): the console's orders on the
//! proving ground (`world::cheats`). Each order changes what it should
//! under a rule that lets the seat cheat, and nothing under one that does
//! not. A seat never changes another player's character. The first order
//! outside an arena level marks the game as cheated, and an arena level
//! never does. Every order survives its encoding, a world with cheat
//! orders steps to the same hash again, and a snapshot keeps the rule and
//! the mark.

use fmt_odyssey::ncs::Op;

use super::SimWorld;
use super::arena::Rules;
use super::arena_probes::{act, mine};
use super::cheats::{Cheats, Outcome, Rule, limits};
use crate::action::{Action, ArenaOrder, CheatOrder, Mark};
use crate::combat::Sheet;
use crate::combat::sheet::flags;
use crate::input::Seat;
use crate::inventory::{Catalog, ItemFacts};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::notice::Notice;
use crate::probe::check;
use crate::snapshot::{Kind, SimId, Snapshot};
use crate::{Guests, WorldConfig};

/// The item template the probes give.
const ITEM: &str = "probe_item";
/// The script the probes run.
const SCRIPT: &str = "probe_script";
/// The tag of the creature the probes kill.
const FOE: &str = "probe_foe";
/// The tag of a creature the story needs (its plot flag set).
const PLOT: &str = "probe_plot";
/// The number global the probes set.
const GLOBAL: &str = "PROBE_GLOBAL";

/// The proving ground for `seats` players, each with its own character,
/// an arena level when `arena`. A creature tagged `FOE` stands 3 m from
/// the arrival, and one tagged `PLOT` 4 m from it. The library knows the
/// item `ITEM` and the empty script `SCRIPT`. The world, and the placement
/// indexes of the two creatures.
fn ground(seats: u8, arena: bool) -> Result<(SimWorld, [u32; 2]), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry;
    let [x, y, z] = entry.position;
    let first = data.placements.len() as u32;
    for (tag, at, plot) in [(FOE, 3.0, false), (PLOT, 4.0, true)] {
        let mut sheet = Sheet::dev_soldier();
        if plot {
            sheet.flags |= flags::PLOT;
        }
        let creature = ObjectData::creature(tag, tag, [x, y + at, z], sheet);
        data.placements.push(Placement::Own(creature));
    }
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Retn);
    let library = crate::probe::library("", [(SCRIPT, asm.build())])
        .with_items(Catalog::new([ItemFacts::plain(ITEM)]));
    let mut level = ground.with_module(data).with_scripts(library);
    if arena {
        level = level.with_arena(Rules {
            respawn: entry.position,
            yaw: entry.yaw,
            pit: -100.0,
            respawn_ms: 500,
            terminal: None,
        });
    }
    let config = WorldConfig {
        seats,
        guests: Guests::Avatars,
        ..WorldConfig::one_seat()
    };
    let world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
    Ok((world, [first, first + 1]))
}

/// The rule `rule` sent by the first seat, then `orders` from their seats
/// on the next step: the snapshot after `steps` steps, and each cheat
/// notice with its seat.
fn under(
    world: &mut SimWorld,
    seats: u8,
    rule: Rule,
    orders: &[(Seat, Action)],
    steps: u32,
) -> (Snapshot, Vec<(Seat, Outcome)>) {
    let policy = [(Seat(0), Action::Cheat(CheatOrder::Policy(rule)))];
    act(world, seats, &policy, 2);
    let (snapshot, notices) = act(world, seats, orders, steps);
    let told = (notices.into_iter())
        .filter_map(|(seat, notice)| match notice {
            Notice::Cheat(outcome) => Some((seat, outcome)),
            _ => None,
        })
        .collect();
    (snapshot, told)
}

fn cheat(seat: u8, order: CheatOrder) -> (Seat, Action) {
    (Seat(seat), Action::Cheat(order))
}

/// How many of the item `ITEM` the party carries.
fn carried(snapshot: &Snapshot) -> u32 {
    (snapshot.goods.items.iter())
        .filter(|(_, item)| item.template == ITEM)
        .map(|(_, item)| u32::from(item.stack))
        .sum()
}

/// Whether the creature at placement `index` is alive.
fn alive(snapshot: &Snapshot, index: u32) -> bool {
    (snapshot.entities.iter())
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, index))
        .and_then(|entity| entity.fighter)
        .is_some_and(|fighter| fighter.alive())
}

/// The player of `seat`'s hit points and most hit points (hundredths).
fn hp(snapshot: &Snapshot, seat: Seat) -> Option<(i32, i32)> {
    let me = snapshot.own(seat)?;
    Some((me.fighter?.hp, i32::from(me.sheet?.max_hp) * 100))
}

pub fn probes() -> crate::probe::Checks {
    vec![
        ("cheats_orders_change_the_game", orders_change()),
        ("cheats_credits_add", credits_add()),
        ("cheats_heal", heal()),
        ("cheats_kill", kill()),
        ("cheats_off_refuses", off_refuses()),
        ("cheats_host_only", host_only()),
        ("cheats_never_another_player", never_another()),
        ("cheats_mark", mark()),
        ("cheats_warp_marks", warp_marks()),
        ("cheats_off_arena_orders", off_arena_orders()),
        ("cheats_bad_orders", bad_orders()),
        ("cheats_round_trip", round_trip()),
        ("cheats_same_hash", same_hash()),
        ("cheats_snapshot_keeps", snapshot_keeps()),
    ]
}

/// Under the rule Host, the host's item, credits, experience, global and
/// script orders change what they should. An order carried out sends no
/// notice.
fn orders_change() -> Result<String, String> {
    let (mut world, _) = ground(1, false)?;
    let orders = [
        cheat(
            0,
            CheatOrder::Item {
                template: ITEM.into(),
                count: 3,
            },
        ),
        cheat(0, CheatOrder::Credits(1234)),
        cheat(0, CheatOrder::Xp(5000)),
        cheat(
            0,
            CheatOrder::Global {
                name: GLOBAL.into(),
                value: 7,
            },
        ),
        cheat(
            0,
            CheatOrder::Script {
                resref: SCRIPT.into(),
                who: None,
            },
        ),
    ];
    let (after, told) = under(&mut world, 1, Rule::Host, &orders, 3);
    let items = carried(&after);
    let global = world.script_state().numbers.get(GLOBAL).copied();
    check(
        items == 3
            && after.goods.gold == 1234
            && after.progress.xp == 5000
            && global == Some(7)
            && told.is_empty(),
        format!(
            "items {items}, credits {}, experience {}, global {global:?}, told {told:?}",
            after.goods.gold, after.progress.xp
        ),
    )
}

/// Credits orders add to the party's credits. A total below 0 stays at 0,
/// and a total past the limit stays at the limit.
fn credits_add() -> Result<String, String> {
    let (mut world, _) = ground(1, false)?;
    let mut seen = Vec::new();
    for (amount, want) in [
        (300, 300),
        (200, 500),
        (-9_999, 0),
        (limits::CREDITS_MOST, limits::CREDITS_MOST),
        (1, limits::CREDITS_MOST),
    ] {
        let orders = [cheat(0, CheatOrder::Credits(amount))];
        let (after, _) = under(&mut world, 1, Rule::Host, &orders, 2);
        seen.push(format!("{amount:+} to {}", after.goods.gold));
        if after.goods.gold != want {
            return Err(format!("{}; wanted {want}", seen.join(", ")));
        }
    }
    Ok(seen.join(", "))
}

/// A heal with no one named fills the player's hit points.
fn heal() -> Result<String, String> {
    let (mut world, _) = ground(1, false)?;
    let mut start = world.snapshot();
    let at = (start.entities.iter())
        .position(|entity| entity.kind == Kind::Avatar(Seat(0)))
        .ok_or("no player")?;
    if let Some(fighter) = start.entities[at].fighter.as_mut() {
        fighter.hp /= 3;
    }
    let mut world = SimWorld::from_snapshot(&start, world.level().clone())
        .map_err(|error| error.to_string())?;
    let before = hp(&world.snapshot(), Seat(0)).ok_or("no hit points")?;
    let orders = [cheat(0, CheatOrder::Heal { who: None })];
    let (after, _) = under(&mut world, 1, Rule::Host, &orders, 3);
    let (now, most) = hp(&after, Seat(0)).ok_or("no hit points")?;
    check(
        before.0 < before.1 && now == most,
        format!("{} of {} before, {now} of {most} after", before.0, before.1),
    )
}

/// A kill by tag kills the creature. A creature with the plot flag and
/// the player's own character are refused and stay alive.
fn kill() -> Result<String, String> {
    let (mut world, [foe, plot]) = ground(1, false)?;
    let (me, _) = mine(&world.snapshot(), Seat(0))?;
    let orders = [
        cheat(
            0,
            CheatOrder::Kill {
                who: Mark::Tag(FOE.into()),
            },
        ),
        cheat(
            0,
            CheatOrder::Kill {
                who: Mark::Tag(PLOT.into()),
            },
        ),
        cheat(0, CheatOrder::Kill { who: Mark::Id(me) }),
    ];
    let (after, told) = under(&mut world, 1, Rule::Host, &orders, 10);
    let outcomes: Vec<Outcome> = told.iter().map(|(_, outcome)| *outcome).collect();
    let me_alive = hp(&after, Seat(0)).is_some_and(|(hp, _)| hp > 0);
    check(
        !alive(&after, foe)
            && alive(&after, plot)
            && me_alive
            && outcomes == [Outcome::Protected, Outcome::Protected],
        format!(
            "foe alive {}, plot creature alive {}, player alive {me_alive}, told {outcomes:?}",
            alive(&after, foe),
            alive(&after, plot)
        ),
    )
}

/// The console's `warp` order changes nothing but the mark. The host's
/// marks the game. Under Host only a guest's is refused and leaves the game
/// unmarked.
fn warp_marks() -> Result<String, String> {
    let (mut guest_first, _) = ground(2, false)?;
    let (_, guest_told) = under(
        &mut guest_first,
        2,
        Rule::Host,
        &[cheat(1, CheatOrder::Warped)],
        2,
    );
    let guest_marked = guest_first.cheats().used;
    let (mut world, _) = ground(1, false)?;
    let before = world.snapshot().goods.gold;
    let (after, told) = under(
        &mut world,
        1,
        Rule::Host,
        &[cheat(0, CheatOrder::Warped)],
        2,
    );
    let marked = world.cheats().used;
    check(
        marked
            && after.goods.gold == before
            && told.is_empty()
            && !guest_marked
            && guest_told == [(Seat(1), Outcome::HostOnly)],
        format!(
            "the host's warp marked {marked}, told {told:?}; a guest's marked {guest_marked}, told {guest_told:?}"
        ),
    )
}

/// Every order the rule Off meets changes nothing and is refused.
fn off_refuses() -> Result<String, String> {
    let (mut world, [foe, _]) = ground(1, false)?;
    let (before, _) = under(&mut world, 1, Rule::Off, &[], 2);
    let (me, sheet) = mine(&before, Seat(0))?;
    let orders = [
        cheat(
            0,
            CheatOrder::Item {
                template: ITEM.into(),
                count: 1,
            },
        ),
        cheat(0, CheatOrder::Credits(999)),
        cheat(0, CheatOrder::Xp(999)),
        cheat(
            0,
            CheatOrder::Kill {
                who: Mark::Tag(FOE.into()),
            },
        ),
        (
            Seat(0),
            Action::Arena(ArenaOrder::Ability {
                who: me,
                ability: 0,
                score: 30,
            }),
        ),
    ];
    let (after, told) = under(&mut world, 1, Rule::Off, &orders, 5);
    let (_, now) = mine(&after, Seat(0))?;
    let unchanged = carried(&after) == 0
        && after.goods.gold == before.goods.gold
        && after.progress.xp == before.progress.xp
        && alive(&after, foe)
        && now == sheet;
    let refused = told.len() == orders.len() && told.iter().all(|(_, o)| *o == Outcome::Off);
    check(
        unchanged && refused && !world.cheats().used,
        format!(
            "unchanged {unchanged}, told {told:?}, marked {}",
            world.cheats().used
        ),
    )
}

/// Under the rule Host, the second seat's order is refused and the
/// first seat's is carried out.
fn host_only() -> Result<String, String> {
    let (mut world, _) = ground(2, false)?;
    let orders = [cheat(1, CheatOrder::Credits(500))];
    let (guest, told_guest) = under(&mut world, 2, Rule::Host, &orders, 3);
    let orders = [cheat(0, CheatOrder::Credits(700))];
    let (host, _) = under(&mut world, 2, Rule::Host, &orders, 3);
    check(
        guest.goods.gold != 500
            && told_guest == [(Seat(1), Outcome::HostOnly)]
            && host.goods.gold == 700,
        format!(
            "credits after the guest's order {}, told {told_guest:?}; after the host's {}",
            guest.goods.gold, host.goods.gold
        ),
    )
}

/// Under the rule All, the guest changes its own character but neither
/// heals nor changes the host's, and the guest cannot set the rule.
fn never_another() -> Result<String, String> {
    let (mut world, _) = ground(2, false)?;
    let (start, _) = under(&mut world, 2, Rule::All, &[], 2);
    let (host, host_sheet) = mine(&start, Seat(0))?;
    let (guest, _) = mine(&start, Seat(1))?;
    let strength = |who| {
        Action::Arena(ArenaOrder::Ability {
            who,
            ability: 0,
            score: 30,
        })
    };
    let orders = [
        (Seat(1), strength(host)),
        (Seat(1), strength(guest)),
        cheat(1, CheatOrder::Heal { who: Some(host) }),
        cheat(1, CheatOrder::Policy(Rule::Off)),
    ];
    let (after, told) = under(&mut world, 2, Rule::All, &orders, 3);
    let (_, host_after) = mine(&after, Seat(0))?;
    let (_, guest_after) = mine(&after, Seat(1))?;
    let outcomes: Vec<Outcome> = told.iter().map(|(_, outcome)| *outcome).collect();
    let expected = [Outcome::HostOnly, Outcome::NotYours, Outcome::NotYours];
    check(
        host_after == host_sheet
            && guest_after.abilities[0] == 30
            && world.cheats().rule == Rule::All
            && outcomes.len() == expected.len()
            && expected.iter().all(|outcome| outcomes.contains(outcome)),
        format!(
            "host's Strength {} (was {}), guest's {}, rule {:?}, told {outcomes:?}",
            host_after.abilities[0],
            host_sheet.abilities[0],
            guest_after.abilities[0],
            world.cheats().rule
        ),
    )
}

/// The first order carried out outside an arena level marks the game as
/// cheated. Setting the rule does not, and an arena level never does.
fn mark() -> Result<String, String> {
    let (mut world, _) = ground(1, false)?;
    under(&mut world, 1, Rule::Host, &[], 2);
    let after_rule = world.cheats().used;
    let orders = [cheat(0, CheatOrder::Credits(10))];
    under(&mut world, 1, Rule::Host, &orders, 2);
    let after_order = world.cheats().used;
    under(&mut world, 1, Rule::Off, &[], 2);
    let kept = world.cheats().used;
    let (mut arena, _) = ground(1, true)?;
    let (me, _) = mine(&arena.snapshot(), Seat(0))?;
    let orders = [
        cheat(0, CheatOrder::Credits(10)),
        (Seat(0), Action::Arena(ArenaOrder::God)),
        (
            Seat(0),
            Action::Arena(ArenaOrder::Skill {
                who: me,
                skill: 0,
                ranks: 4,
            }),
        ),
    ];
    let (arena_after, _) = under(&mut arena, 1, Rule::Off, &orders, 2);
    let arena_marked = arena.cheats().used;
    check(
        !after_rule && after_order && kept && !arena_marked && arena_after.goods.gold == 10,
        format!(
            "marked after the rule {after_rule}, after an order {after_order}, kept {kept}; on an arena level {arena_marked}"
        ),
    )
}

/// Outside an arena level, the cheat rule lets `god` and the character
/// page's orders through, and refuses `clearfoes`, `restore` and `down`.
fn off_arena_orders() -> Result<String, String> {
    let (mut world, [foe, _]) = ground(1, false)?;
    let (me, _) = mine(&world.snapshot(), Seat(0))?;
    let orders = [
        (Seat(0), Action::Arena(ArenaOrder::God)),
        (
            Seat(0),
            Action::Arena(ArenaOrder::Alignment {
                who: me,
                good_evil: 10,
            }),
        ),
        (Seat(0), Action::Arena(ArenaOrder::Clear)),
        (Seat(0), Action::Arena(ArenaOrder::Down(3))),
    ];
    let (after, told) = under(&mut world, 1, Rule::Host, &orders, 3);
    let (_, sheet) = mine(&after, Seat(0))?;
    let outcomes: Vec<Outcome> = told.iter().map(|(_, outcome)| *outcome).collect();
    check(
        sheet.flags & flags::IMMORTAL != 0
            && sheet.good_evil == 10
            && alive(&after, foe)
            && outcomes == [Outcome::ArenaOnly, Outcome::ArenaOnly],
        format!(
            "god {}, alignment {}, foe alive {}, told {outcomes:?}",
            sheet.flags & flags::IMMORTAL != 0,
            sheet.good_evil,
            alive(&after, foe)
        ),
    )
}

/// An unknown template, script or tag changes nothing and is refused as
/// unknown. Amounts past the limits are held to them.
fn bad_orders() -> Result<String, String> {
    let (mut world, _) = ground(1, false)?;
    let orders = [
        cheat(
            0,
            CheatOrder::Item {
                template: "no_such_item".into(),
                count: 5,
            },
        ),
        cheat(
            0,
            CheatOrder::Script {
                resref: "no_such_script".into(),
                who: None,
            },
        ),
        cheat(
            0,
            CheatOrder::Kill {
                who: Mark::Tag("no_such_tag".into()),
            },
        ),
        cheat(
            0,
            CheatOrder::Kill {
                who: Mark::Id(SimId(u32::MAX - 1)),
            },
        ),
        (Seat(0), Action::Arena(ArenaOrder::Spawn("no_such".into()))),
        cheat(0, CheatOrder::Credits(i32::MAX)),
        cheat(0, CheatOrder::Xp(-5)),
        cheat(
            0,
            CheatOrder::Item {
                template: ITEM.into(),
                count: u16::MAX,
            },
        ),
    ];
    let (after, told) = under(&mut world, 1, Rule::Host, &orders, 3);
    let unknown = (told.iter())
        .filter(|(_, outcome)| *outcome == Outcome::Unknown)
        .count();
    check(
        unknown == 5
            && after.goods.gold == limits::CREDITS_MOST
            && after.progress.xp == 0
            && carried(&after) == u32::from(limits::ITEMS_MOST),
        format!(
            "{unknown} refused as unknown, credits {}, experience {}, items {}",
            after.goods.gold,
            after.progress.xp,
            carried(&after)
        ),
    )
}

/// Every cheat order is read back as it was written, and an unknown order
/// code fails to decode.
fn round_trip() -> Result<String, String> {
    let who = Some(SimId(9));
    let orders = Rule::ALL.map(CheatOrder::Policy).into_iter().chain([
        CheatOrder::Item {
            template: ITEM.into(),
            count: 4,
        },
        CheatOrder::Credits(-3),
        CheatOrder::Xp(70_000),
        CheatOrder::Heal { who },
        CheatOrder::Heal { who: None },
        CheatOrder::Kill {
            who: Mark::Id(SimId(3)),
        },
        CheatOrder::Kill {
            who: Mark::Tag(FOE.into()),
        },
        CheatOrder::Global {
            name: GLOBAL.into(),
            value: -1,
        },
        CheatOrder::Script {
            resref: SCRIPT.into(),
            who,
        },
        CheatOrder::Script {
            resref: SCRIPT.into(),
            who: None,
        },
        CheatOrder::Warped,
    ]);
    for order in orders {
        let action = Action::Cheat(order);
        let back = Action::from_bytes(&action.to_bytes()).map_err(|error| error.to_string())?;
        if back != action {
            return Err(format!("{action:?} came back as {back:?}"));
        }
    }
    let mut bad = Action::Cheat(CheatOrder::Credits(1)).to_bytes();
    if let Some(code) = bad.get_mut(1) {
        *code = 200;
    }
    check(
        Action::from_bytes(&bad).is_err(),
        "every order came back as written; an unknown order code is refused".into(),
    )
}

/// Two worlds given the same cheat orders on the same steps reach the
/// same hash.
fn same_hash() -> Result<String, String> {
    let orders = [
        cheat(0, CheatOrder::Credits(55)),
        cheat(
            0,
            CheatOrder::Kill {
                who: Mark::Tag(FOE.into()),
            },
        ),
        cheat(
            0,
            CheatOrder::Item {
                template: ITEM.into(),
                count: 2,
            },
        ),
    ];
    let mut hashes = Vec::new();
    for _ in 0..2 {
        let (mut world, _) = ground(1, false)?;
        let (after, _) = under(&mut world, 1, Rule::Host, &orders, 20);
        hashes.push(after.hash());
    }
    check(
        hashes.windows(2).all(|pair| pair[0] == pair[1]),
        format!("{} runs, one hash", hashes.len()),
    )
}

/// A snapshot keeps the rule and the mark, and a world made from it has
/// them.
fn snapshot_keeps() -> Result<String, String> {
    let (mut world, _) = ground(1, false)?;
    let orders = [cheat(0, CheatOrder::Credits(1))];
    let (after, _) = under(&mut world, 1, Rule::All, &orders, 2);
    let back = Snapshot::decode(&after.encode()).map_err(|error| error.to_string())?;
    let restored =
        SimWorld::from_snapshot(&back, world.level().clone()).map_err(|error| error.to_string())?;
    let expected = Cheats {
        rule: Rule::All,
        used: true,
    };
    check(
        after.cheats == expected && back.cheats == expected && restored.cheats() == expected,
        format!(
            "snapshot {:?}, decoded {:?}, restored {:?}",
            after.cheats,
            back.cheats,
            restored.cheats()
        ),
    )
}
