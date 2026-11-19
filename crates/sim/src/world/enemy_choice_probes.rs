//! The probes of the Force powers, items and grenades an enemy chooses
//! itself (`kjk sim probes --only "enemy choices"`; `combat::choose`,
//! `world::minds`). Each case runs on the proving ground with a script
//! library, as the games' own levels have, and needs no install. The player
//! stands at one end of the room, and in some cases a companion stands 1 m
//! beside the player. The companion is stunned for the whole case so that
//! it never moves. One enemy stands about 7 m away and fights the player.
//! The enemy carries its own items, which are held by the enemy and not by
//! the party. The cases:
//! - an enemy that knows Stun casts it on the player within 6 seconds;
//! - an enemy with 3 fragmentation grenades throws one at the player and
//!   the companion standing together within 10 seconds, and the grenade
//!   leaves the enemy's own pack, not the party's 5;
//! - an enemy with only 1 grenade throws it, because the rule about the
//!   last 2 grenades is a companion's rule;
//! - an enemy with an energy shield uses it within 6 seconds against the
//!   player, who holds a blaster. The probe's shield has an impact script
//!   that does nothing, so the case counts the shield used and not its
//!   effect;
//! - an enemy below half health with a medpac and an advanced medpac uses
//!   the medpac when the medpac restores all its missing hit points;
//! - the same enemy uses the advanced medpac when the medpac would not
//!   restore them all;
//! - a neutral creature at 40% health with grenades, medpacs and a shield,
//!   standing beside the enemy, uses nothing in 10 seconds;
//! - an enemy below half health uses no medpac when a script turns it
//!   neutral and clears its actions on the very step on which it chose the
//!   medpac. The case first runs without the script to find that step.

use script_ncs::asm::{Expr, call, script};
use script_ncs::catalog::Catalog;
use script_ncs::vm::ObjectId;

use super::{Fighting, Id, SimWorld};
use crate::combat::Sheet;
use crate::combat::items::healing;
use crate::combat::sheet::{Rows, factions};
use crate::combat::spells::{
    FORCE_POWER_FORCE_VALOR, FORCE_POWER_STUN, ITEM_ABILITY_ADVANCED_MED_PACK,
    ITEM_ABILITY_ENERGY_SHIELD, ITEM_ABILITY_GRENADE_FRAGMENTATION, ITEM_ABILITY_MED_PACK,
};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::inventory::{Catalog as Items, ItemFacts, ItemSpell, PARTY};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::party::Npc;
use crate::script::{Pending, Talent, Talents};
use crate::snapshot::SimId;
use crate::{StepReason, WorldConfig};

/// A step's milliseconds.
const MSEC: u32 = 50;

const STUN: u16 = FORCE_POWER_STUN;
const VALOR: u16 = FORCE_POWER_FORCE_VALOR;
/// The powers an enemy may know in a case. Each costs 1 Force point.
const POWERS: [u16; 2] = [STUN, VALOR];
const FRAG: u16 = ITEM_ABILITY_GRENADE_FRAGMENTATION;
const MEDPAC: u16 = ITEM_ABILITY_MED_PACK;
const ADVANCED: u16 = ITEM_ABILITY_ADVANCED_MED_PACK;
const SHIELD: u16 = ITEM_ABILITY_ENERGY_SHIELD;

/// The goods of the cases by item name, in the order a case lists their
/// counts.
const GOODS: [(&str, u16); 4] = [
    ("frag", FRAG),
    ("medpac", MEDPAC),
    ("advanced", ADVANCED),
    ("shield", SHIELD),
];

/// The talent categories the library sets for the rows
/// (`Talent::category`).
const HEALING: u16 = 2;
const ENHANCING: u16 = 3;
const HOSTILE_AREA: u16 = 5;
const HOSTILE_SINGLE: u16 = 6;

/// How long the companion stays stunned and how long the enemy's fight
/// with the player lasts (sim ms). This is longer than any case.
const FOREVER: u64 = 1_000_000;

/// The enemy's Force points, enough for every power it casts.
const ENEMY_FP: u16 = 60;

/// The hit points of the player and of each companion, so that no grenade
/// knocks one of them out and breaks up the pair the enemies aim at.
const PARTY_HP: u16 = 5000;

/// The party's own fragmentation grenades. The enemy must not throw one of
/// these.
const PARTY_FRAGS: u16 = 5;

const MATES: [&str; 2] = ["probe_mate_a", "probe_mate_b"];
const ENEMY: &str = "probe_enemy";
const ALLIES: [&str; 3] = ["probe_ally_a", "probe_ally_b", "probe_ally_c"];
const NEUTRAL: &str = "probe_neutral";

/// Where everyone stands (metres in the room's frame). The companions stand
/// 1 m from the player, and the enemy's allies 1 m from the enemy.
const PLAYER_AT: (f32, f32) = (-2.0, -4.0);
const MATES_AT: [(f32, f32); 2] = [(-1.0, -4.0), (-3.0, -4.0)];
const ENEMY_AT: (f32, f32) = (-2.0, 3.0);
const ALLIES_AT: [(f32, f32); 3] = [(-3.0, 3.0), (-2.0, 4.0), (-3.0, 4.0)];
const NEUTRAL_AT: (f32, f32) = (-1.0, 3.0);

/// The script that ends the enemy's fight. It turns the enemy neutral and
/// clears its actions, as a story script does when a fight ends at a set
/// health.
const END_FIGHT: &str = "probe_end_fight";
/// The impact script of every item row, which does nothing. An item is
/// usable only when its spell has an impact script.
const IMPACT: &str = "probe_impact";

const CATALOG: &str = r#"
// 0
void ChangeToStandardFaction(object oCreatureToChange, int nStandardFaction);
// 1
void ClearAllActions();
"#;

fn scripts() -> Result<Vec<(&'static str, fmt_odyssey::ncs::Ncs)>, String> {
    let catalog = Catalog::parse(CATALOG).map_err(|e| format!("{e:?}"))?;
    let neutral = Expr::Int(i32::from(factions::NEUTRAL));
    let end = vec![
        call("ChangeToStandardFaction", [Expr::Me, neutral]),
        call("ClearAllActions", []),
    ];
    [(END_FIGHT, end), (IMPACT, Vec::new())]
        .into_iter()
        .map(|(name, statements)| Ok((name, script(&catalog, &statements)?)))
        .collect()
}

/// The talent facts of the rows the cases use. Every item row has the
/// impact script `IMPACT`.
fn talents() -> Talents {
    let facts = [
        (STUN, HOSTILE_SINGLE, true),
        (VALOR, ENHANCING, false),
        (FRAG, HOSTILE_AREA, true),
        (MEDPAC, HEALING, false),
        (ADVANCED, HEALING, false),
        (SHIELD, ENHANCING, false),
    ];
    let size = facts
        .iter()
        .map(|(row, ..)| usize::from(*row))
        .max()
        .unwrap_or(0)
        + 1;
    let mut spells = vec![Talent::default(); size];
    let mut impacts = vec![String::new(); size];
    for (row, category, hostile) in facts {
        spells[usize::from(row)] = Talent {
            category,
            maxcr: 1,
            hostile,
            ..Talent::default()
        };
        if !POWERS.contains(&row) {
            impacts[usize::from(row)] = IMPACT.to_string();
        }
    }
    Talents {
        spells,
        impacts,
        ..Talents::default()
    }
}

/// An item of `row`'s spell, single use, stacking to ten.
fn item((name, row): (&str, u16)) -> ItemFacts {
    ItemFacts {
        stacking: 10,
        spell: Some(ItemSpell { row, cost: 1 }),
        ..super::containers::plain(name)
    }
}

/// How hurt the enemy is at the start.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hurt {
    /// At full health.
    No,
    /// Below half health, missing exactly what a medpac restores it.
    MedpacWorth,
    /// Below half health, missing more than a medpac restores it.
    MoreThanAMedpac,
}

/// A case. It lists the enemy's powers and its own goods (`GOODS`' counts),
/// and how hurt it is. It says how many companions stand beside the player,
/// how many allies stand beside the enemy with the goods each ally carries,
/// and whether a neutral creature stands in the room. It says whether a script ends the enemy's fight on the step on
/// which the enemy first chose. It says how long the case runs and what it
/// must see.
struct Case {
    name: &'static str,
    powers: &'static [u16],
    goods: [u16; 4],
    hurt: Hurt,
    mates: usize,
    allies: usize,
    ally_goods: [u16; 4],
    neutral: bool,
    end_fight: bool,
    seconds: u64,
    wanted: fn(&Ran) -> bool,
}

/// What a case saw.
#[derive(Debug, Default)]
struct Ran {
    /// The powers the enemy pressed, each once, in the order of their first
    /// press.
    cast: Vec<u16>,
    /// The enemy's own goods at the end (`GOODS`' counts).
    enemy: [u16; 4],
    /// The neutral creature's goods at the end (`GOODS`' counts).
    neutral: [u16; 4],
    /// The party's fragmentation grenades at the end.
    party_frags: u16,
    /// The step on which the enemy first set its wait anew after a choice.
    chose_at: Option<u64>,
    /// When the enemy's side, the enemy and its allies, chose to throw each
    /// of its grenades (sim ms). A choice is a step on which a creature of
    /// the side newly has a grenade queued for its throw.
    throws: Vec<u64>,
    /// When the enemy first was no longer alive (sim ms).
    gone_at: Option<u64>,
}

/// Each case's starting point. The enemy knows no powers and carries no
/// goods. It is at full health and faces the player alone. The case runs
/// for 10 seconds.
const PLAIN: Case = Case {
    name: "",
    powers: &[],
    goods: [0, 0, 0, 0],
    hurt: Hurt::No,
    mates: 0,
    allies: 0,
    ally_goods: [0, 0, 0, 0],
    neutral: false,
    end_fight: false,
    seconds: 10,
    wanted: |_| false,
};

const CASES: [Case; 8] = [
    Case {
        name: "dark_jedi_disables",
        powers: &[STUN],
        seconds: 6,
        wanted: |ran| ran.cast.contains(&STUN),
        ..PLAIN
    },
    Case {
        name: "grenade_from_its_own_pack",
        goods: [3, 0, 0, 0],
        mates: 1,
        wanted: |ran| ran.enemy[0] == 2 && ran.party_frags == PARTY_FRAGS,
        ..PLAIN
    },
    Case {
        name: "last_grenade_thrown",
        goods: [1, 0, 0, 0],
        mates: 1,
        wanted: |ran| ran.enemy[0] == 0 && ran.party_frags == PARTY_FRAGS,
        ..PLAIN
    },
    Case {
        name: "shield_against_a_blaster",
        goods: [0, 0, 0, 1],
        seconds: 6,
        wanted: |ran| ran.enemy[3] == 0,
        ..PLAIN
    },
    Case {
        name: "weakest_medpac_that_restores",
        goods: [0, 1, 1, 0],
        hurt: Hurt::MedpacWorth,
        wanted: |ran| ran.enemy[1] == 0 && ran.enemy[2] == 1,
        ..PLAIN
    },
    Case {
        name: "advanced_medpac_when_needed",
        goods: [0, 1, 1, 0],
        hurt: Hurt::MoreThanAMedpac,
        wanted: |ran| ran.enemy[1] == 1 && ran.enemy[2] == 0,
        ..PLAIN
    },
    Case {
        name: "neutral_uses_nothing",
        neutral: true,
        wanted: |ran| ran.neutral == NEUTRAL_GOODS,
        ..PLAIN
    },
    Case {
        name: "no_medpac_when_a_script_ends_the_fight",
        goods: [0, 1, 0, 0],
        hurt: Hurt::MoreThanAMedpac,
        end_fight: true,
        seconds: 5,
        wanted: |ran| ran.chose_at.is_some() && ran.enemy[1] == 1,
        ..PLAIN
    },
];

/// The cases of an intense fight for the enemy's side, which `intense_probes`
/// runs. The enemy knows Force Valor, whose reason is an intense fight. It
/// faces the player and two companions alone, an intense fight for its
/// side, and casts Valor. With three allies beside it against the player
/// alone, its side is the stronger, and it does not cast Valor.
const INTENSE_CASES: [Case; 2] = [
    Case {
        name: "valor_against_three",
        powers: &[VALOR],
        mates: 2,
        seconds: 6,
        wanted: |ran| ran.cast.contains(&VALOR),
        ..PLAIN
    },
    Case {
        name: "no_valor_with_three_allies",
        powers: &[VALOR],
        allies: 3,
        seconds: 6,
        wanted: |ran| !ran.cast.contains(&VALOR),
        ..PLAIN
    },
];

/// The case of the approved test `choice_side_waits` (`side_wait_probes`).
/// The enemy and three allies stand together, each with 3 fragmentation
/// grenades, and face the player and a companion standing together for 30
/// seconds. The side throws at least twice, and no two of its throws come
/// less than the side's wait for a grenade apart.
const SIDE_WAIT_CASES: [Case; 1] = [Case {
    name: "one_grenade_per_side_wait",
    goods: [3, 0, 0, 0],
    allies: 3,
    ally_goods: [3, 0, 0, 0],
    mates: 1,
    seconds: 30,
    wanted: |ran| {
        let wait = crate::combat::choose::side_wait_ms(crate::combat::choose::SideKind::Grenade);
        ran.throws.len() >= 2 && ran.throws.windows(2).all(|pair| pair[1] - pair[0] >= wait)
    },
    ..PLAIN
}];

/// The neutral creature's goods (`GOODS`' counts), and its health as a part
/// of its maximum.
const NEUTRAL_GOODS: [u16; 4] = [3, 2, 0, 1];
const NEUTRAL_HEALTH: f32 = 0.4;

/// The ground set up for `case`, run for its time. When `end_at` is a step,
/// the script `END_FIGHT` runs for the enemy on that step.
fn run(case: &Case, end_at: Option<u64>) -> Result<Ran, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
    data.entry.position = crate::determinism::ground_point(PLAYER_AT.0, PLAYER_AT.1, 0.0);
    data.force_costs = vec![
        0;
        POWERS
            .iter()
            .map(|row| usize::from(*row))
            .max()
            .unwrap_or(0)
            + 1
    ];
    for row in POWERS {
        data.force_costs[usize::from(row)] = 1;
    }
    let creature = |tag: &str, (x, y): (f32, f32), sheet: Sheet| {
        Placement::Own(ObjectData {
            sheet: Some(sheet),
            template: tag.to_string(),
            ..ObjectData::bare(
                ObjectKind::Creature,
                tag,
                crate::determinism::ground_point(x, y, 0.0),
            )
        })
    };
    let bare = Sheet {
        items: Default::default(),
        ..Sheet::dev_soldier()
    };
    // The medpac restores the enemy `restores` hit points. Below half
    // health, the enemy misses exactly that many, or 3 more.
    let restores = healing(
        &bare,
        rules::items::medpacs::POINTS[0],
        rules::items::medpacs::PER_RANK[0],
        false,
    );
    let (max_hp, hp) = match case.hurt {
        Hurt::No => (i32::from(bare.max_hp), i32::from(bare.max_hp)),
        Hurt::MedpacWorth => (2 * restores - 2, restores - 2),
        Hurt::MoreThanAMedpac => (2 * restores + 4, restores + 1),
    };
    let enemy = Sheet {
        faction: factions::HOSTILE_1,
        powers: case
            .powers
            .iter()
            .fold(Rows::default(), |rows, row| rows.with(*row)),
        max_fp: if case.powers.is_empty() { 0 } else { ENEMY_FP },
        max_hp: u16::try_from(max_hp).map_err(|error| error.to_string())?,
        ..bare
    };
    data.placements.push(creature(ENEMY, ENEMY_AT, enemy));
    let ally = Sheet {
        faction: factions::HOSTILE_1,
        ..bare
    };
    for (tag, at) in ALLIES.into_iter().zip(ALLIES_AT).take(case.allies) {
        data.placements.push(creature(tag, at, ally));
    }
    let mate = Sheet {
        faction: factions::PLAYER,
        max_hp: PARTY_HP,
        ..bare
    };
    for (tag, at) in MATES.into_iter().zip(MATES_AT).take(case.mates) {
        data.placements.push(creature(tag, at, mate));
    }
    if case.neutral {
        let neutral = Sheet {
            faction: factions::NEUTRAL,
            ..bare
        };
        data.placements.push(creature(NEUTRAL, NEUTRAL_AT, neutral));
    }
    let items = Items::new(GOODS.map(item));
    let library =
        crate::probe::library_with(CATALOG, scripts()?, talents()).with_items(items.clone());
    let nav = super::party_fights_probes::ground_nav(&ground);
    let level = ground.with_module(data).with_scripts(library).with_nav(nav);
    let config = WorldConfig {
        sheet: Sheet {
            max_hp: PARTY_HP,
            ..Sheet::dev_soldier()
        },
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let player = start.body(Seat(0)).ok_or("no player")?.id;
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let tagged = |tag: &str| {
        start
            .entities
            .iter()
            .find(|entity| {
                objects
                    .of_entity(entity)
                    .is_some_and(|data| data.tag == tag)
            })
            .map(|entity| entity.id)
            .ok_or(format!("no {tag}"))
    };
    let enemy = tagged(ENEMY)?;
    let tagged_all = |tags: &[&str]| {
        tags.iter()
            .map(|tag| tagged(tag))
            .collect::<Result<Vec<_>, _>>()
    };
    let mates = tagged_all(&MATES[..case.mates])?;
    let allies = tagged_all(&ALLIES[..case.allies])?;
    let neutral = case.neutral.then(|| tagged(NEUTRAL)).transpose()?;
    let mut state = world.script_state().clone();
    for (npc, (tag, id)) in MATES.into_iter().zip(&mates).enumerate() {
        let npc = u8::try_from(npc).map_err(|error| error.to_string())?;
        state.party.available.insert(
            npc,
            Npc {
                template: tag.to_string(),
                object: Some(id.0),
                kept: None,
                template_data: None,
            },
        );
        state.party.members.push(npc);
    }
    // Each holder's goods, made in the scripts' state.
    let mut make = |holder: u32, counts: [u16; 4]| {
        GOODS
            .iter()
            .zip(counts)
            .map(|((name, _), count)| {
                if count == 0 {
                    return Ok(None);
                }
                state
                    .make_item(&items, name, holder, count, None)
                    .map(Some)
                    .ok_or(format!("no {name}"))
            })
            .collect::<Result<Vec<Option<ObjectId>>, String>>()
    };
    let enemy_goods = make(enemy.0, case.goods)?;
    let ally_goods = (allies.iter())
        .map(|ally| make(ally.0, case.ally_goods))
        .collect::<Result<Vec<_>, String>>()?;
    let neutral_goods = match neutral {
        Some(neutral) => make(neutral.0, NEUTRAL_GOODS)?,
        None => vec![None; GOODS.len()],
    };
    let party_frags = make(PARTY, [PARTY_FRAGS, 0, 0, 0])?[0];
    world.set_script_state(state);
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
        let fighter = &mut fighting.0;
        if mates.contains(&id.0) {
            fighter.mind.leader = Some(Seat(0));
            fighter.force.stunned_until = FOREVER;
            // A stunned companion still aims at the enemy, so that it is in
            // the enemy's fight.
            fighter.target = Some(enemy);
            fighter.combat_until = FOREVER;
        } else if id.0 == enemy || allies.contains(&id.0) {
            fighter.target = Some(player);
            fighter.combat_until = FOREVER;
        }
        if id.0 == enemy {
            // Hit points are kept in hundredths.
            fighter.hp = hp * 100;
        } else if Some(id.0) == neutral {
            let max = f32::from(bare.max_hp) * 100.0;
            fighter.hp = (max * NEUTRAL_HEALTH) as i32;
        }
    }
    let count = |world: &SimWorld, of: &[Option<ObjectId>]| -> [u16; 4] {
        std::array::from_fn(|at| {
            of.get(at)
                .copied()
                .flatten()
                .and_then(|id| world.script_state().items.get(&id.0))
                .map_or(0, |item| item.stack)
        })
    };
    // The creatures of the enemy's side that had a grenade queued on the
    // step before.
    let side: Vec<SimId> = std::iter::once(enemy)
        .chain(allies.iter().copied())
        .collect();
    let mut queued: Vec<SimId> = Vec::new();
    let mut ran = Ran::default();
    let mut waited = 0;
    let steps = case.seconds * 1000 / u64::from(MSEC);
    for step in 1..=steps {
        if end_at == Some(step) {
            let mut state = world.script_state().clone();
            state
                .pending
                .push(Pending::new(END_FIGHT, ObjectId(enemy.0)));
            world.set_script_state(state);
        }
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let snapshot = crate::step(&mut world, step, &input, MSEC, StepReason::Authority);
        let now_queued: Vec<SimId> = (snapshot.entities.iter())
            .filter(|entity| side.contains(&entity.id))
            .filter(|entity| entity.fighter.is_some_and(|f| f.pack.queued == Some(FRAG)))
            .map(|entity| entity.id)
            .collect();
        for _ in now_queued.iter().filter(|id| !queued.contains(id)) {
            ran.throws.push(step * u64::from(MSEC));
        }
        queued = now_queued;
        // The enemy's own uses, while it lives.
        let Some(fighter) = (snapshot.entities.iter())
            .find(|entity| entity.id == enemy)
            .and_then(|entity| entity.fighter)
            .filter(|fighter| fighter.alive())
        else {
            ran.gone_at.get_or_insert(step * u64::from(MSEC));
            continue;
        };
        if fighter.mind.cast_after != waited {
            waited = fighter.mind.cast_after;
            ran.chose_at.get_or_insert(step);
        }
        let command = fighter.last_command;
        if command.buttons & buttons::FORCE_POWER != 0 && !ran.cast.contains(&command.force) {
            ran.cast.push(command.force);
        }
    }
    ran.enemy = count(&world, &enemy_goods);
    ran.neutral = count(&world, &neutral_goods);
    ran.party_frags = count(&world, &[party_frags])[0];
    Ok(ran)
}

/// A case's run. A case whose script ends the fight runs twice. The first
/// run, without the script, finds the step on which the enemy chose. The
/// second run ends the fight on that step.
fn ran(case: &Case) -> Result<Ran, String> {
    if !case.end_fight {
        return run(case, None);
    }
    let first = run(case, None)?;
    let at = first
        .chose_at
        .ok_or("the enemy chose nothing without the script")?;
    run(case, Some(at))
}

pub fn probes() -> Checks {
    checked(&CASES)
}

/// The staged case of the approved test `choice_side_waits`
/// (`SIDE_WAIT_CASES`).
pub fn side_wait_probes() -> Checks {
    checked(&SIDE_WAIT_CASES)
}

/// The staged cases of the approved test `intense_fights` (`INTENSE_CASES`).
pub fn intense_probes() -> Checks {
    checked(&INTENSE_CASES)
}

/// Each case's check.
fn checked(cases: &[Case]) -> Checks {
    cases
        .iter()
        .map(|case| {
            let result = ran(case).and_then(|ran| check((case.wanted)(&ran), format!("{ran:?}")));
            (case.name, result)
        })
        .collect()
}
