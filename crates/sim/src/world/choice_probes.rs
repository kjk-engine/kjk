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
//!   it at the same time as on the base commit, and never a stim;
//! - a companion with Heal that a script tells to attack casts nothing in 3
//!   seconds while a conversation holds the world, though the player is at
//!   30% health;
//! - a companion with Heal that the player leads casts nothing in 3
//!   seconds, though the player is at 30% health;
//! - a companion at 30% health with Revitalize casts nothing in 3 seconds
//!   while nobody has fallen;
//! - a companion with Revitalize casts it within 3 seconds when a second
//!   companion has fallen;
//! - a companion whose combat script picks Power Attack every round still
//!   casts Heal within 6 seconds when the player is at 30% health;
//! - a companion with 5 rockets and no wrist launcher fires none at four
//!   enemies in 30 seconds;
//! - a companion who knows Energy Resistance and Force Valor casts Energy
//!   Resistance and not Valor in 6 seconds against one enemy with a blaster;
//! - the same companion casts neither in 6 seconds against one enemy with a
//!   vibroblade;
//! - a companion who knows Resist Energy 1 and 2 and Master Energy
//!   Resistance casts only Master Energy Resistance in 6 seconds against
//!   one enemy with a blaster;
//! - a companion who knows Force Valor, with Valor from the player already
//!   running on it, does not cast Valor in 10 seconds against four enemies;
//! - a companion who knows Force Valor and Battle Meditation casts exactly
//!   one of them once in 20 seconds against four enemies, an intense fight;
//! - a companion who knows Force Barrier casts it within 6 seconds against
//!   one enemy with a vibroblade;
//! - the same companion, with Force Barrier already running on it, does
//!   not cast it in 6 seconds.

use script_ncs::asm::{Expr, call, script};
use script_ncs::catalog::Catalog;
use script_ncs::vm::ObjectId;

use super::{Fighting, Id, SimWorld};
use crate::combat::effects::{self, Lasting, Modifies};
use crate::combat::sheet::feats::POWER_ATTACK;
use crate::combat::sheet::{Rows, factions};
use crate::combat::spells::{
    FORCE_POWER_BATTLE_MEDITATION_PC, FORCE_POWER_FORCE_BARRIER, FORCE_POWER_FORCE_VALOR,
    FORCE_POWER_HEAL, FORCE_POWER_MASTER_ENERGY_RESISTANCE, FORCE_POWER_RESIST_ENERGY_1,
    FORCE_POWER_RESIST_ENERGY_2, FORCE_POWER_REVITALIZE, ITEM_ABILITY_ADRENAL_SHOT_STRENGTH,
    ITEM_ABILITY_GRENADE_FRAGMENTATION, ITEM_ABILITY_MED_PACK, ITEM_ABILITY_ROCKET_EXPLOSIVE,
};
use crate::combat::{ROUND_MS, Sheet};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::inventory::{Catalog as Items, ItemFacts, ItemSpell, PARTY};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::dialogue::Conversation;
use crate::script::party::Npc;
use crate::script::{Menu, Pending, Talent, Talents};
use crate::snapshot::SimId;
use crate::{StepReason, WorldConfig};

/// A step's milliseconds.
const MSEC: u32 = 50;

const HEAL: u16 = FORCE_POWER_HEAL;
const REVITALIZE: u16 = FORCE_POWER_REVITALIZE;
const RESIST: u16 = FORCE_POWER_RESIST_ENERGY_1;
const RESIST_2: u16 = FORCE_POWER_RESIST_ENERGY_2;
const RESIST_MASTER: u16 = FORCE_POWER_MASTER_ENERGY_RESISTANCE;
const VALOR: u16 = FORCE_POWER_FORCE_VALOR;
const BARRIER: u16 = FORCE_POWER_FORCE_BARRIER;
const BATTLE_MEDITATION: u16 = FORCE_POWER_BATTLE_MEDITATION_PC;
/// The powers a companion may know in a case. Each costs 1 Force point.
const POWERS: [u16; 8] = [
    HEAL,
    REVITALIZE,
    RESIST,
    RESIST_2,
    RESIST_MASTER,
    VALOR,
    BARRIER,
    BATTLE_MEDITATION,
];
/// The powers that only their impact script casts (`force::scripted`).
/// Each is on the character's own menu of powers.
const SCRIPTED: [u16; 4] = [REVITALIZE, RESIST_MASTER, BARRIER, BATTLE_MEDITATION];
const FRAG: u16 = ITEM_ABILITY_GRENADE_FRAGMENTATION;
const MEDPAC: u16 = ITEM_ABILITY_MED_PACK;
const STIM: u16 = ITEM_ABILITY_ADRENAL_SHOT_STRENGTH;
/// A KOTOR 2 rocket, which only a character wearing a wrist launcher fires.
const ROCKET: u16 = ITEM_ABILITY_ROCKET_EXPLOSIVE;
/// The melee feat the feat script picks.
const FEAT: u16 = POWER_ATTACK[0];

/// The party's goods, in the order a case lists their counts.
const GOODS: [(&str, u16); 4] = [
    ("frag", FRAG),
    ("medpac", MEDPAC),
    ("stim", STIM),
    ("rocket", ROCKET),
];

/// The talent categories the library sets for the rows
/// (`Talent::category`). The medpac script asks for `HEALING`, and the
/// stim is not in that category.
const HEALING: u16 = 2;
const ENHANCING: u16 = 3;
const HOSTILE_AREA: u16 = 5;

/// How long the enemies stay stunned, and how long their fight with the
/// player lasts (sim ms). This is longer than any case.
const FOREVER: u64 = 1_000_000;

/// The enemies' hit points. A companion's shots kill none of the enemies
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
/// The companion's combat script that uses the feat `FEAT` on the first
/// enemy.
const PICK_FEAT: &str = "probe_pick_feat";
/// The companion's script that tells it to attack the first enemy, as a
/// scene's script does while a conversation holds the world.
const ATTACK_FOE: &str = "probe_attack_foe";
/// The impact script of every item row, which does nothing. An item is
/// usable only when its spell has an impact script.
const IMPACT: &str = "probe_impact";
/// Battle Meditation's impact script. It raises the caster's Strength by 1
/// for `LASTING_S` seconds, so that the power's effect runs on the caster
/// as the game's own script leaves it.
const LASTING_IMPACT: &str = "probe_lasting_impact";
/// How long the lasting impact's effect runs (seconds). This is longer than
/// any case.
const LASTING_S: f32 = 1000.0;
/// `DURATION_TYPE_TEMPORARY`, and Strength's ability number.
const TEMPORARY: i32 = 1;
const STRENGTH: i32 = 0;

/// The companion's own combat script, if it runs one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    None,
    /// `FIND_MEDPAC` runs once at the start.
    FindMedpac,
    /// `PICK_FEAT` runs at the start of every round.
    PickFeat,
    /// `ATTACK_FOE` runs once at the start.
    AttackFoe,
}

const CATALOG: &str = r#"
int TALENT_TYPE_FORCE = 0;
// 0
talent GetCreatureTalentBest(int nCategory, int nCRMax, object oCreature, int nInclusion, int nExcludeType, int nExcludeId);
// 1
void ActionUseTalentOnObject(talent tChosenTalent, object oTarget);
// 2
talent TalentFeat(int nFeat);
// 3
object GetObjectByTag(string sTag, int nNth);
// 4
void ActionAttack(object oAttackee, int bPassive);
// 5
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration);
// 6
effect EffectAbilityIncrease(int nAbilityToIncrease, int nModifyBy);
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
    let foe = || {
        call(
            "GetObjectByTag",
            [Expr::String(FOES[0].into()), Expr::Int(0)],
        )
    };
    let feat = call("TalentFeat", [Expr::Int(i32::from(FEAT))]);
    let picked = call("ActionUseTalentOnObject", [feat, foe()]);
    let attack = call("ActionAttack", [foe(), Expr::Int(0)]);
    let strength = call("EffectAbilityIncrease", [Expr::Int(STRENGTH), Expr::Int(1)]);
    let lasting = call(
        "ApplyEffectToObject",
        [
            Expr::Int(TEMPORARY),
            strength,
            Expr::Me,
            Expr::Float(LASTING_S),
        ],
    );
    [
        (FIND_MEDPAC, vec![used]),
        (PICK_FEAT, vec![picked]),
        (ATTACK_FOE, vec![attack]),
        (IMPACT, Vec::new()),
        (LASTING_IMPACT, vec![lasting]),
    ]
    .into_iter()
    .map(|(name, statements)| Ok((name, script(&catalog, &statements)?)))
    .collect()
}

/// How long a table indexed by `rows` must be to hold the highest of them.
fn rows_up_to(rows: impl Iterator<Item = u16>) -> usize {
    rows.map(usize::from).max().map_or(0, |highest| highest + 1)
}

/// The talent facts of the rows the cases use.
fn talents() -> Talents {
    let facts = [
        (HEAL, HEALING, false),
        (REVITALIZE, HEALING, false),
        (BARRIER, ENHANCING, false),
        (RESIST_MASTER, ENHANCING, false),
        (BATTLE_MEDITATION, ENHANCING, false),
        (FRAG, HOSTILE_AREA, true),
        (MEDPAC, HEALING, false),
        (STIM, ENHANCING, false),
        (ROCKET, HOSTILE_AREA, true),
    ];
    let size = rows_up_to(facts.iter().map(|(row, ..)| *row));
    let mut spells = vec![Talent::default(); size];
    let mut impacts = vec![String::new(); size];
    for (row, category, hostile) in facts {
        let menu = if SCRIPTED.contains(&row) {
            Menu::Friendly
        } else {
            Menu::None
        };
        spells[usize::from(row)] = Talent {
            category,
            maxcr: 1,
            hostile,
            menu,
            ..Talent::default()
        };
        let impact = match row {
            HEAL => None,
            BATTLE_MEDITATION => Some(LASTING_IMPACT),
            _ => Some(IMPACT),
        };
        if let Some(impact) = impact {
            impacts[usize::from(row)] = impact.to_string();
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

/// A case. It says how many companions there are and the powers each
/// knows, and how many enemies they face. It lists the party's goods
/// (`GOODS`' counts). It sets the player's and the companions' hit points
/// as a part of their maximum. It names the first companion's own combat
/// script. It says whether a conversation holds the world, and whether the
/// player leads the first companion. It says how long the case runs and
/// what it must see.
struct Case {
    name: &'static str,
    mates: usize,
    powers: &'static [u16],
    foes: usize,
    goods: [u16; 4],
    player_health: f32,
    mate_health: f32,
    script: Script,
    held: bool,
    led: bool,
    /// The second companion has fallen at the start.
    fallen: bool,
    /// The player's health is set back to `player_health` on every step,
    /// so that a heal never makes the next one needless.
    keep_hurt: bool,
    /// The enemies hold only a vibroblade. Otherwise each enemy holds a
    /// blaster pistol.
    foes_melee: bool,
    /// A power whose effect runs on the first companion for the whole case.
    /// The player applied the effect.
    running: Option<u16>,
    seconds: u64,
    wanted: fn(&Ran) -> bool,
}

/// What a case saw.
#[derive(Debug, Default)]
struct Ran {
    /// When the companion used a power, an item or a grenade of its own
    /// choice (sim ms). A use is a step at which its wait for the next use
    /// was set anew.
    uses: Vec<u64>,
    /// When the companion first pressed one of its powers (sim ms).
    cast_at: Option<u64>,
    /// The powers the companion pressed, each once, in the order of their
    /// first press.
    cast: Vec<u16>,
    /// The party's goods at the end (`GOODS`' counts).
    goods: [u16; 4],
    /// When the party first had a medpac fewer (sim ms).
    medpac_at: Option<u64>,
    /// When any companion began to cast Heal (sim ms). A cast begins on a
    /// step on which the companion presses Heal and did not on the step
    /// before.
    heals: Vec<u64>,
    /// The first step at which a companion counted more frags left than
    /// will remain. The three numbers are the time (sim ms), what the
    /// companion counted, and the number that will remain.
    overseen: Option<(u64, u16, u16)>,
}

/// Whether a companion of `mates` counts more frags left than will remain,
/// at `now`, among the `party` (each member's id, pack and sheet) whose
/// inventory holds `stack` frags.
///
/// A companion that may choose is not charging a throw of its own. It
/// counts the frags its sheet shows, less its own uses and every other
/// member's uses and coming throws that the inventory still counts
/// (`Pack::not_yet_taken`), as `world::minds` counts them. The frags that
/// will remain are the inventory's, less the same uses and throws of every
/// member. The two agree only while each sheet shows what the inventory
/// holds at the same step.
fn overseen(
    party: &[(SimId, crate::combat::items::Pack, Sheet)],
    mates: &[SimId],
    stack: u16,
    now: u64,
) -> Option<(u64, u16, u16)> {
    let untaken: u16 = party
        .iter()
        .map(|(_, pack, sheet)| u16::from(pack.not_yet_taken(sheet, FRAG, now)))
        .sum();
    // The truth, read from the packs' own fields rather than through
    // `not_yet_taken`: each member's frags used and not yet taken, and its
    // frag about to be thrown.
    let spent: u16 = party
        .iter()
        .map(|(_, pack, sheet)| {
            let used = (sheet.items.iter().zip(pack.used))
                .filter(|(carried, _)| carried.is_some_and(|carried| carried.row == FRAG))
                .map(|(_, used)| u16::from(used))
                .sum::<u16>();
            used + u16::from(pack.throwing(sheet, now) == Some(FRAG))
        })
        .sum();
    let remain = stack.saturating_sub(spent);
    party
        .iter()
        .filter(|(id, ..)| mates.contains(id))
        .find_map(|(_, _, sheet)| {
            let shown = (sheet.items.iter().flatten())
                .find(|carried| carried.row == FRAG)
                .map_or(0, |carried| u16::from(carried.count));
            let counted = shown.saturating_sub(untaken);
            (counted > remain).then_some((now, counted, remain))
        })
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
    data.force_costs = vec![0; rows_up_to(POWERS.into_iter())];
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
    // The dev soldier holds its blaster pistol in its first hand, and its
    // vibroblade is its second weapon. A melee enemy keeps only the blade.
    for (tag, at) in FOES.into_iter().zip(FOES_AT).take(case.foes) {
        let foe = Sheet {
            faction: factions::HOSTILE_1,
            max_hp: FOE_HP,
            weapons: if case.foes_melee {
                [bare.weapons[1], None]
            } else {
                bare.weapons
            },
            ..bare
        };
        data.placements.push(creature(tag, at, foe));
    }
    let items = Items::new(GOODS.map(item));
    let library =
        crate::probe::library_with(CATALOG, scripts()?, talents()).with_items(items.clone());
    // The energy resistance line's prerequisites, as `spells.2da` lists
    // them. Resist Energy 2 needs Resist Energy 1, and Master Energy
    // Resistance needs both.
    let library = library.with_classes(crate::levels::Classes {
        prerequisites: [
            (RESIST_2, [Some(RESIST), None]),
            (RESIST_MASTER, [Some(RESIST_2), Some(RESIST)]),
        ]
        .into(),
        ..Default::default()
    });
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
    if case.led {
        state.party.leader = Some(0);
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
    let once = match case.script {
        Script::FindMedpac => Some(FIND_MEDPAC),
        Script::AttackFoe => Some(ATTACK_FOE),
        Script::None | Script::PickFeat => None,
    };
    if let Some(once) = once {
        state.pending.push(Pending::new(once, ObjectId(mate.0)));
    }
    // In a held case, the player talks in a conversation that holds the
    // whole party. Its line waits on a reply, so it never ends of itself.
    if case.held {
        let pc = ObjectId(player.0);
        state.talk.conversations.push(Conversation {
            owner: pc,
            pc,
            listener: pc,
            until_ms: u64::MAX,
            party: true,
            ..Default::default()
        });
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
            if case.fallen && mates.get(1) == Some(&id.0) {
                fighter.fall(0);
            }
            // The effect a power's impact script leaves on its target: here
            // only the power's icon, from the player, for the whole case.
            if let Some(spell) = case.running.filter(|_| id.0 == mate) {
                let effect = Lasting {
                    modifies: Modifies::Icon,
                    amount: 0,
                    of: 0,
                    until: FOREVER,
                    spell,
                    by: player.0,
                    id: 1,
                    at: 0,
                    third: 0,
                };
                effects::add(fighter, effect, 0);
            }
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
    // The companions that pressed Heal on the step before.
    let mut healing: Vec<SimId> = Vec::new();
    let steps = case.seconds * 1000 / u64::from(MSEC);
    for step in 1..=steps {
        let before = (step - 1) * u64::from(MSEC);
        if case.script == Script::PickFeat && before % ROUND_MS == 0 {
            let mut state = world.script_state().clone();
            state
                .pending
                .push(Pending::new(PICK_FEAT, ObjectId(mate.0)));
            world.set_script_state(state);
        }
        if case.keep_hurt {
            let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
            for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
                if id.0 == player {
                    // Hit points are kept in hundredths.
                    fighting.0.hp = (player_hp as f32 * 100.0 * case.player_health) as i32;
                }
            }
        }
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
        let presses_heal = |fighter: &crate::combat::Fighter| {
            let command = fighter.last_command;
            command.buttons & buttons::FORCE_POWER != 0 && command.force == HEAL
        };
        let now_healing: Vec<SimId> = (snapshot.entities.iter())
            .filter(|entity| mates.contains(&entity.id))
            .filter(|entity| entity.fighter.as_ref().is_some_and(presses_heal))
            .map(|entity| entity.id)
            .collect();
        for _ in now_healing.iter().filter(|id| !healing.contains(id)) {
            ran.heals.push(now);
        }
        healing = now_healing;
        let command = fighter.last_command;
        if command.buttons & buttons::FORCE_POWER != 0 && case.powers.contains(&command.force) {
            ran.cast_at.get_or_insert(now);
            if !ran.cast.contains(&command.force) {
                ran.cast.push(command.force);
            }
        }
        if count(&world, "medpac") < case.goods[1] {
            ran.medpac_at.get_or_insert(now);
        }
        let party: Vec<_> = snapshot
            .entities
            .iter()
            .filter(|entity| entity.id == player || mates.contains(&entity.id))
            .filter_map(|entity| Some((entity.id, entity.fighter?.pack, entity.sheet?)))
            .collect();
        if let Some(over) = overseen(&party, &mates, count(&world, "frag"), now) {
            ran.overseen.get_or_insert(over);
        }
    }
    ran.goods = GOODS.map(|(name, _)| count(&world, name));
    Ok(ran)
}

/// When the first companion's script uses its medpac on the commit before
/// the companions chose their own powers and grenades (sim ms). The script's
/// talent is carried out as before, so the time must not move.
const BASE_MEDPAC_AT: u64 = 1650;

/// What every case starts from. One companion with no powers faces one
/// enemy. The party has no goods, and everyone is at full health. The
/// companion runs no script, nothing holds the world, and the player leads
/// nobody. The case runs for 30 seconds.
const PLAIN: Case = Case {
    name: "",
    mates: 1,
    powers: &[],
    foes: 1,
    goods: [0, 0, 0, 0],
    player_health: 1.0,
    mate_health: 1.0,
    script: Script::None,
    held: false,
    led: false,
    fallen: false,
    keep_hurt: false,
    foes_melee: false,
    running: None,
    seconds: 30,
    wanted: |_| false,
};

/// A Jedi companion who knows Energy Resistance and Force Valor faces one
/// enemy with a blaster for 6 seconds, which is past the opening of the
/// fight. Other cases change it.
const BUFFS: Case = Case {
    name: "companion_resists_energy_from_gunner",
    powers: &[RESIST, VALOR],
    seconds: 6,
    wanted: |ran| ran.cast == [RESIST],
    ..PLAIN
};

/// The case `companion_heals_hurt_ally`, which other cases change.
const HEALS: Case = Case {
    name: "companion_heals_hurt_ally",
    powers: &[HEAL],
    player_health: 0.3,
    seconds: 3,
    wanted: |ran| ran.cast_at.is_some(),
    ..PLAIN
};

/// Whether the companion cast and used nothing.
fn idle(ran: &Ran) -> bool {
    ran.cast_at.is_none() && ran.uses.is_empty()
}

/// The cases (see the module documentation).
const CASES: [Case; 22] = [
    HEALS,
    BUFFS,
    Case {
        name: "companion_casts_no_buff_against_blade",
        foes_melee: true,
        wanted: idle,
        ..BUFFS
    },
    Case {
        name: "companion_casts_only_master_energy_resistance",
        powers: &[RESIST, RESIST_2, RESIST_MASTER],
        wanted: |ran| ran.cast == [RESIST_MASTER],
        ..BUFFS
    },
    Case {
        name: "companion_keeps_valor_from_ally_in_intense_fight",
        powers: &[VALOR],
        foes: 4,
        running: Some(VALOR),
        seconds: 10,
        wanted: idle,
        ..BUFFS
    },
    Case {
        name: "companion_keeps_one_intense_buff_up",
        powers: &[VALOR, BATTLE_MEDITATION],
        foes: 4,
        seconds: 20,
        wanted: |ran| ran.cast.len() == 1 && ran.uses.len() == 1,
        ..BUFFS
    },
    Case {
        name: "companion_raises_barrier_against_blade",
        powers: &[BARRIER],
        foes_melee: true,
        wanted: |ran| ran.cast == [BARRIER],
        ..BUFFS
    },
    Case {
        name: "companion_keeps_running_barrier",
        powers: &[BARRIER],
        foes_melee: true,
        running: Some(BARRIER),
        wanted: idle,
        ..BUFFS
    },
    Case {
        name: "held_companion_casts_nothing",
        held: true,
        script: Script::AttackFoe,
        wanted: idle,
        ..HEALS
    },
    Case {
        name: "led_companion_casts_nothing",
        led: true,
        wanted: idle,
        ..HEALS
    },
    Case {
        name: "hurt_companion_keeps_revitalize_while_nobody_fell",
        powers: &[REVITALIZE],
        player_health: 1.0,
        mate_health: 0.3,
        wanted: idle,
        ..HEALS
    },
    Case {
        name: "companion_raises_fallen_companion",
        mates: 2,
        powers: &[REVITALIZE],
        player_health: 1.0,
        fallen: true,
        ..HEALS
    },
    Case {
        name: "companion_heals_past_its_scripts_feat",
        script: Script::PickFeat,
        seconds: 6,
        ..HEALS
    },
    Case {
        name: "companion_without_launcher_keeps_rockets",
        foes: 4,
        goods: [0, 0, 0, 5],
        wanted: |ran| ran.goods[3] == 5 && ran.uses.is_empty(),
        ..PLAIN
    },
    Case {
        name: "companion_keeps_grenades_from_one_weak_enemy",
        goods: [5, 0, 0, 0],
        wanted: |ran| ran.goods[0] == 5,
        ..PLAIN
    },
    Case {
        name: "companion_throws_grenade_at_four_enemies",
        foes: 4,
        goods: [5, 0, 0, 0],
        seconds: 8,
        wanted: |ran| ran.goods[0] == 4,
        ..PLAIN
    },
    Case {
        name: "companion_keeps_last_two_grenades",
        foes: 4,
        goods: [2, 0, 0, 0],
        wanted: |ran| ran.goods[0] == 2,
        ..PLAIN
    },
    Case {
        name: "two_companions_keep_last_two_grenades",
        mates: 2,
        foes: 4,
        goods: [3, 0, 0, 0],
        wanted: |ran| ran.goods[0] == 2 && ran.overseen.is_none(),
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
        goods: [5, 0, 0, 0],
        player_health: 0.3,
        wanted: |ran| ran.uses.len() >= 2 && ran.least_gap().is_some_and(|gap| gap >= APART_MS),
        ..PLAIN
    },
    Case {
        name: "healthy_companion_keeps_medpacs_and_stims",
        foes: 4,
        goods: [0, 10, 10, 0],
        wanted: |ran| ran.goods == [0, 10, 10, 0] && ran.uses.is_empty(),
        ..PLAIN
    },
    Case {
        name: "hurt_companion_uses_scripts_medpac_not_stim",
        foes: 4,
        goods: [0, 10, 10, 0],
        mate_health: 0.2,
        script: Script::FindMedpac,
        seconds: 10,
        wanted: |ran| ran.goods == [0, 9, 10, 0] && ran.medpac_at == Some(BASE_MEDPAC_AT),
        ..PLAIN
    },
];

/// The case of the approved test `choice_side_waits` (`side_wait_probes`).
/// Two companions who know Heal stand beside the player, who is kept at 5%
/// health for 30 seconds. They heal at least twice in all, and no two heals come less
/// than the side's wait for a heal apart.
const SIDE_WAIT_CASES: [Case; 1] = [Case {
    name: "one_heal_per_side_wait",
    mates: 2,
    player_health: 0.05,
    keep_hurt: true,
    seconds: 30,
    wanted: |ran| {
        let wait = crate::combat::choose::side_wait_ms(crate::combat::choose::SideKind::Heal);
        ran.heals.len() >= 2 && ran.heals.windows(2).all(|pair| pair[1] - pair[0] >= wait)
    },
    ..HEALS
}];

pub fn probes() -> Checks {
    checked(&CASES)
}

/// The staged case of the approved test `choice_side_waits`
/// (`SIDE_WAIT_CASES`).
pub fn side_wait_probes() -> Checks {
    checked(&SIDE_WAIT_CASES)
}

/// Each case's check.
fn checked(cases: &[Case]) -> Checks {
    cases
        .iter()
        .map(|case| {
            let result = run(case).and_then(|ran| {
                check(
                    (case.wanted)(&ran),
                    format!(
                        "uses at {:?} ms, a power first pressed at {:?} ms, the powers \
                         pressed {:?}, frags, medpacs, stims and rockets left {:?}, a medpac \
                         first used at {:?} ms, frags first counted above what remains (ms, \
                         counted, remains) {:?}, heals by any companion at {:?} ms",
                        ran.uses,
                        ran.cast_at,
                        ran.cast,
                        ran.goods,
                        ran.medpac_at,
                        ran.overseen,
                        ran.heals
                    ),
                )
            });
            (case.name, result)
        })
        .collect()
}
