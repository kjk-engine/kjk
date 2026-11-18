//! The probes of the Force powers and grenades a companion chooses itself
//! (`kjk sim probes --only "companion choices"`; `combat::choose`,
//! `world::minds`). Each case runs on the proving ground with a script
//! library, as the games' own levels have, and needs no install. The player
//! stands at one end of the room's first half with a companion beside it.
//! One to four enemies stand together at the other end, stunned for the
//! whole case and each aiming at the player, so that the fight is the
//! party's and the enemies never move. The cases:
//! - a companion with Heal casts it within 3 seconds when the player is at
//!   30% health;
//! - a companion with 5 fragmentation grenades throws none at one weak
//!   enemy in 30 seconds;
//! - the same companion throws one at four enemies standing together, and
//!   the party then has 4;
//! - a companion with 2 grenades throws none at four enemies, as the party
//!   keeps its last 2;
//! - two companions with the party's 3 grenades, facing four enemies, leave
//!   the party exactly 2 after 30 seconds;
//! - a companion with no powers and no items uses nothing in 30 seconds;
//! - a companion with Heal and grenades uses at least two things in 30
//!   seconds, never two within 5 seconds of each other;
//! - a companion at full health in an intense fight uses none of the
//!   party's 10 medpacs and 10 stims in 30 seconds;
//! - a companion at 20% health whose own combat script finds a medpac uses
//!   it at the same time as on the base commit, and never a stim.

use script_ncs::asm::{Expr, call, script};
use script_ncs::catalog::Catalog;
use script_ncs::vm::ObjectId;

use super::{Fighting, Id, SimWorld};
use crate::combat::Sheet;
use crate::combat::sheet::{Rows, factions};
use crate::combat::spells::{
    FORCE_POWER_HEAL, ITEM_ABILITY_ADRENAL_SHOT_STRENGTH, ITEM_ABILITY_GRENADE_FRAGMENTATION,
    ITEM_ABILITY_MED_PACK,
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

const HEAL: u16 = FORCE_POWER_HEAL;
const FRAG: u16 = ITEM_ABILITY_GRENADE_FRAGMENTATION;
const MEDPAC: u16 = ITEM_ABILITY_MED_PACK;
const STIM: u16 = ITEM_ABILITY_ADRENAL_SHOT_STRENGTH;

/// The party's goods, in the order a case lists their counts.
const GOODS: [(&str, u16); 3] = [("frag", FRAG), ("medpac", MEDPAC), ("stim", STIM)];

/// The talent categories the library gives the rows (`Talent::category`).
/// The medpac script asks for `HEALING`, which the stim is not.
const HEALING: u16 = 2;
const ENHANCING: u16 = 3;
const HOSTILE_AREA: u16 = 5;

/// How long the enemies stay stunned, and how long their fight with the
/// player lasts (sim ms): longer than any case.
const FOREVER: u64 = 1_000_000;

/// The enemies' hit points: enough that a companion's shots kill none
/// before it has chosen.
const FOE_HP: u16 = 60;

/// The least time between two uses of a companion's own choice (sim ms).
const APART_MS: u64 = 5000;

/// The companion's Force points, enough for every power it casts.
const MATE_FP: u16 = 60;

/// The companions. Most cases have only the first. The uses a case records
/// are the first companion's.
const MATES: [&str; 2] = ["probe_mate_a", "probe_mate_b"];
const FOES: [&str; 4] = ["probe_foe_a", "probe_foe_b", "probe_foe_c", "probe_foe_d"];

/// Where the player, the companions and the enemies stand (metres in the
/// room's frame). The enemies are 1 m apart, about 7 m from the party.
const PLAYER_AT: (f32, f32) = (-2.0, -4.0);
const MATES_AT: [(f32, f32); 2] = [(-1.0, -4.5), (-2.5, -5.5)];
const FOES_AT: [(f32, f32); 4] = [(-2.5, 3.0), (-1.5, 3.0), (-2.5, 4.0), (-1.5, 4.0)];

/// The companion's combat script, which finds a healing talent and uses it
/// on the companion itself.
const FIND_MEDPAC: &str = "probe_find_medpac";
/// The impact script of every item row, which does nothing. An item is
/// usable only when its spell has an impact script.
const IMPACT: &str = "probe_impact";

const CATALOG: &str = r#"
int TALENT_TYPE_FORCE = 0;
// 0
talent GetCreatureTalentBest(int nCategory, int nCRMax, object oCreature, int nInclusion, int nExcludeType, int nExcludeId);
// 1
void ActionUseTalentOnObject(talent tChosenTalent, object oTarget);
"#;

fn scripts() -> Result<Vec<(&'static str, fmt_odyssey::ncs::Ncs)>, String> {
    let catalog = Catalog::parse(CATALOG).map_err(|e| format!("{e:?}"))?;
    let found = call(
        "GetCreatureTalentBest",
        [
            Expr::Int(i32::from(HEALING)),
            Expr::Int(20),
            Expr::Me,
            Expr::Int(0),
            Expr::Int(-1),
            Expr::Int(-1),
        ],
    );
    let used = call("ActionUseTalentOnObject", [found, Expr::Me]);
    [(FIND_MEDPAC, vec![used]), (IMPACT, Vec::new())]
        .into_iter()
        .map(|(name, statements)| Ok((name, script(&catalog, &statements)?)))
        .collect()
}

/// The talent facts of the rows the cases use.
fn talents() -> Talents {
    let size = usize::from(HEAL.max(FRAG).max(MEDPAC).max(STIM)) + 1;
    let mut spells = vec![Talent::default(); size];
    let mut impacts = vec![String::new(); size];
    let facts = [
        (HEAL, HEALING, false),
        (FRAG, HOSTILE_AREA, true),
        (MEDPAC, HEALING, false),
        (STIM, ENHANCING, false),
    ];
    for (row, category, hostile) in facts {
        spells[usize::from(row)] = Talent {
            category,
            maxcr: 1,
            hostile,
            ..Talent::default()
        };
        if row != HEAL {
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

/// A case: how many companions there are and the powers each knows, how
/// many enemies they face, the party's goods (`GOODS`' counts), the
/// player's and the companions' hit points as a part of their maximum,
/// whether the first companion's own script finds a medpac once at the
/// start, how long it runs, and what it must see.
struct Case {
    name: &'static str,
    mates: usize,
    powers: &'static [u16],
    foes: usize,
    goods: [u16; 3],
    player_health: f32,
    mate_health: f32,
    script: bool,
    seconds: u64,
    wanted: fn(&Ran) -> bool,
}

/// What a case saw.
#[derive(Debug, Default)]
struct Ran {
    /// When the companion used a power, an item or a grenade of its own
    /// choice (sim ms): each step its wait for the next was set anew.
    uses: Vec<u64>,
    /// When the companion first pressed Heal (sim ms).
    healed_at: Option<u64>,
    /// The party's goods at the end (`GOODS`' counts).
    goods: [u16; 3],
    /// When the party first had a medpac fewer (sim ms).
    medpac_at: Option<u64>,
}

impl Ran {
    /// The shortest time between two uses (sim ms).
    fn least_gap(&self) -> Option<u64> {
        self.uses.windows(2).map(|pair| pair[1] - pair[0]).min()
    }
}

/// The ground set up for `case`, run for its time.
fn run(case: &Case) -> Result<Ran, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
    data.entry.position = crate::determinism::ground_point(PLAYER_AT.0, PLAYER_AT.1, 0.0);
    let costs = usize::from(HEAL) + 1;
    data.force_costs = vec![0; costs];
    data.force_costs[usize::from(HEAL)] = 1;
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
    let mate = Sheet {
        faction: factions::PLAYER,
        powers: case
            .powers
            .iter()
            .fold(Rows::default(), |rows, row| rows.with(*row)),
        max_fp: if case.powers.is_empty() { 0 } else { MATE_FP },
        ..bare
    };
    for (tag, at) in MATES.into_iter().zip(MATES_AT).take(case.mates) {
        data.placements.push(creature(tag, at, mate));
    }
    for (tag, at) in FOES.into_iter().zip(FOES_AT).take(case.foes) {
        let foe = Sheet {
            faction: factions::HOSTILE_1,
            max_hp: FOE_HP,
            ..bare
        };
        data.placements.push(creature(tag, at, foe));
    }
    let items = Items::new(GOODS.map(item));
    let library =
        crate::probe::library_with(CATALOG, scripts()?, talents()).with_items(items.clone());
    let nav = super::party_fights_probes::ground_nav(&ground);
    let level = ground.with_module(data).with_scripts(library).with_nav(nav);
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let player = start.body(Seat(0)).ok_or("no player")?;
    let (player, player_hp) = (
        player.id,
        player.sheet.map_or(0, |sheet| i32::from(sheet.max_hp)),
    );
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
    let tagged_all = |tags: &[&str]| {
        tags.iter()
            .map(|tag| tagged(tag))
            .collect::<Result<Vec<SimId>, String>>()
    };
    let mates = tagged_all(&MATES[..case.mates])?;
    let mate = *mates.first().ok_or("no companion")?;
    let foes = tagged_all(&FOES[..case.foes])?;
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
    let goods = GOODS
        .iter()
        .zip(case.goods)
        .filter(|(_, count)| *count > 0)
        .map(|((name, _), count)| {
            state
                .make_item(&items, name, PARTY, count, None)
                .map(|id| (*name, id))
                .ok_or(format!("no {name}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if case.script {
        state
            .pending
            .push(Pending::new(FIND_MEDPAC, ObjectId(mate.0)));
    }
    world.set_script_state(state);
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
        let fighter = &mut fighting.0;
        // Hit points are kept in hundredths.
        let hp = |health: f32, max: i32| (max as f32 * 100.0 * health) as i32;
        if mates.contains(&id.0) {
            fighter.mind.leader = Some(Seat(0));
            fighter.hp = hp(case.mate_health, i32::from(bare.max_hp));
        } else if id.0 == player {
            fighter.hp = hp(case.player_health, player_hp);
        } else if foes.contains(&id.0) {
            fighter.target = Some(player);
            fighter.combat_until = FOREVER;
            fighter.force.stunned_until = FOREVER;
        }
    }
    let count = |world: &SimWorld, name: &str| {
        goods
            .iter()
            .find(|(of, _)| *of == name)
            .and_then(|(_, id)| world.script_state().items.get(&id.0))
            .map_or(0, |item| item.stack)
    };
    let mut ran = Ran::default();
    let mut waited = 0;
    let steps = case.seconds * 1000 / u64::from(MSEC);
    for step in 1..=steps {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let snapshot = crate::step(&mut world, step, &input, MSEC, StepReason::Authority);
        let now = step * u64::from(MSEC);
        let fighter = snapshot
            .entities
            .iter()
            .find(|entity| entity.id == mate)
            .and_then(|entity| entity.fighter)
            .ok_or("the companion went")?;
        if fighter.mind.cast_after != waited {
            waited = fighter.mind.cast_after;
            ran.uses.push(now);
        }
        let command = fighter.last_command;
        if command.buttons & buttons::FORCE_POWER != 0 && command.force == HEAL {
            ran.healed_at.get_or_insert(now);
        }
        if count(&world, "medpac") < case.goods[1] {
            ran.medpac_at.get_or_insert(now);
        }
    }
    ran.goods = GOODS.map(|(name, _)| count(&world, name));
    Ok(ran)
}

/// When the first companion's script uses its medpac on the commit before
/// the companions chose their own powers and grenades (sim ms). The script's
/// talent is carried out as before, so the time must not move.
const BASE_MEDPAC_AT: u64 = 1650;

/// What every case starts from: one companion with no powers, one enemy,
/// no goods, everyone at full health, no script, 30 seconds.
const PLAIN: Case = Case {
    name: "",
    mates: 1,
    powers: &[],
    foes: 1,
    goods: [0, 0, 0],
    player_health: 1.0,
    mate_health: 1.0,
    script: false,
    seconds: 30,
    wanted: |_| false,
};

/// The cases (see the module documentation).
const CASES: [Case; 9] = [
    Case {
        name: "companion_heals_hurt_ally",
        powers: &[HEAL],
        player_health: 0.3,
        seconds: 3,
        wanted: |ran| ran.healed_at.is_some(),
        ..PLAIN
    },
    Case {
        name: "companion_keeps_grenades_from_one_weak_enemy",
        goods: [5, 0, 0],
        wanted: |ran| ran.goods[0] == 5,
        ..PLAIN
    },
    Case {
        name: "companion_throws_grenade_at_four_enemies",
        foes: 4,
        goods: [5, 0, 0],
        seconds: 8,
        wanted: |ran| ran.goods[0] == 4,
        ..PLAIN
    },
    Case {
        name: "companion_keeps_last_two_grenades",
        foes: 4,
        goods: [2, 0, 0],
        wanted: |ran| ran.goods[0] == 2,
        ..PLAIN
    },
    Case {
        name: "two_companions_keep_last_two_grenades",
        mates: 2,
        foes: 4,
        goods: [3, 0, 0],
        wanted: |ran| ran.goods[0] == 2,
        ..PLAIN
    },
    Case {
        name: "companion_with_nothing_chooses_nothing",
        foes: 4,
        wanted: |ran| ran.uses.is_empty(),
        ..PLAIN
    },
    Case {
        name: "companion_waits_between_uses",
        powers: &[HEAL],
        foes: 4,
        goods: [5, 0, 0],
        player_health: 0.3,
        wanted: |ran| ran.uses.len() >= 2 && ran.least_gap().is_some_and(|gap| gap >= APART_MS),
        ..PLAIN
    },
    Case {
        name: "healthy_companion_keeps_medpacs_and_stims",
        foes: 4,
        goods: [0, 10, 10],
        wanted: |ran| ran.goods == [0, 10, 10] && ran.uses.is_empty(),
        ..PLAIN
    },
    Case {
        name: "hurt_companion_uses_scripts_medpac_not_stim",
        foes: 4,
        goods: [0, 10, 10],
        mate_health: 0.2,
        script: true,
        seconds: 10,
        wanted: |ran| ran.goods == [0, 9, 10] && ran.medpac_at == Some(BASE_MEDPAC_AT),
        ..PLAIN
    },
];

pub fn probes() -> Checks {
    CASES
        .iter()
        .map(|case| {
            let result = run(case).and_then(|ran| {
                check(
                    (case.wanted)(&ran),
                    format!(
                        "uses at {:?} ms, Heal first pressed at {:?} ms, frags, medpacs and \
                         stims left {:?}, a medpac first used at {:?} ms",
                        ran.uses, ran.healed_at, ran.goods, ran.medpac_at
                    ),
                )
            });
            (case.name, result)
        })
        .collect()
}
