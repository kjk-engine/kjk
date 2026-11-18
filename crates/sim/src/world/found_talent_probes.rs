//! The probes of the talent a combat script finds (`kjk sim probes --only
//! "found talents"`), on the proving ground, no install. A companion of the
//! player's party and an enemy each run one AI hook once:
//! - a companion's hook that uses a power `GetCreatureTalentBest` found
//!   queues no talent, as the engine chooses its powers;
//! - a companion's hook that casts a power it names with
//!   `ActionCastSpellAtObject` still casts it, as a set piece;
//! - an enemy's hook that uses a power `GetCreatureTalentBest` found still
//!   queues it;
//! - a hurt companion's hook that uses a medpac `GetCreatureTalentBest`
//!   found still uses it, and the party then has one medpac fewer.

use script_ncs::asm::{Expr, call, script};
use script_ncs::catalog::Catalog;
use script_ncs::vm::ObjectId;

use super::SimWorld;
use crate::combat::Sheet;
use crate::combat::sheet::Rows;
use crate::combat::spells::{ITEM_ABILITY_MED_PACK, K1_FORCE_POWERS};
use crate::input::{Command, Seat, TickInput};
use crate::inventory::{Catalog as Items, ItemFacts, ItemSpell, PARTY};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::script::{Pending, Talent, Talents};
use crate::snapshot::{EntityState, Kind};
use crate::{StepReason, WorldConfig};

const MSEC: u32 = 50;

/// The steps each case runs: 2.75 seconds. The hook runs once, before its
/// own heartbeat first comes due at 3 seconds.
const STEPS: usize = 55;

/// The power both creatures know, and the medpac's row.
const POWER: u16 = K1_FORCE_POWERS[0];
const MEDPAC: u16 = ITEM_ABILITY_MED_PACK;

/// The talent categories the scripts ask for (`Talent::category`).
const POWERS: i32 = 1;
const HEALING: i32 = 2;

/// The medpacs the party carries at the start.
const MEDPACS: u16 = 3;

/// The companion's hit points in the medpac case.
const HURT_HP: i32 = 5;

const MATE: &str = "probe_mate";
const FOE: &str = "probe_foe";

/// The routines the scripts call. The talent types are needed: without
/// them `GetCreatureTalentBest`'s excluded type of -1 matches every talent.
const CATALOG: &str = r#"
int TALENT_TYPE_FORCE = 0;
int TALENT_TYPE_FEAT = 2;
// 0
talent GetCreatureTalentBest(int nCategory, int nCRMax, object oCreature, int nInclusion, int nExcludeType, int nExcludeId);
// 1
void ActionUseTalentOnObject(talent tChosenTalent, object oTarget);
// 2
void ActionCastSpellAtObject(int nSpell, object oTarget, int nMetaMagic, int bCheat, int nDomainLevel, int nProjectilePathType, int bInstantSpell);
// 3
object GetObjectByTag(string sTag, int nNth);
// 4
object GetFirstPC();
"#;

/// The scripts, each an AI hook (its heartbeat) of the creature that runs
/// it: a power found and used on the enemy, a power named and cast at the
/// enemy, a power found and used on the player, and a medpac found and used
/// on the creature itself.
const FOUND_ON_FOE: &str = "probe_found_on_foe";
const NAMED_ON_FOE: &str = "probe_named_on_foe";
const FOUND_ON_PC: &str = "probe_found_on_pc";
const FOUND_MEDPAC: &str = "probe_found_medpac";
/// The medpac's impact script, which does nothing. An item is used only
/// when its spell has an impact script.
const IMPACT: &str = "probe_impact";

fn scripts() -> Result<Vec<(&'static str, fmt_odyssey::ncs::Ncs)>, String> {
    let catalog = Catalog::parse(CATALOG).map_err(|e| format!("{e:?}"))?;
    let found = |category: i32| {
        call(
            "GetCreatureTalentBest",
            [
                Expr::Int(category),
                Expr::Int(20),
                Expr::Me,
                Expr::Int(0),
                Expr::Int(-1),
                Expr::Int(-1),
            ],
        )
    };
    let foe = || call("GetObjectByTag", [Expr::String(FOE.into()), Expr::Int(0)]);
    let used = |talent: Expr, on: Expr| call("ActionUseTalentOnObject", [talent, on]);
    let named = call(
        "ActionCastSpellAtObject",
        [
            Expr::Int(i32::from(POWER)),
            foe(),
            Expr::Int(0),
            Expr::Int(0),
            Expr::Int(0),
            Expr::Int(0),
            Expr::Int(0),
        ],
    );
    [
        (FOUND_ON_FOE, vec![used(found(POWERS), foe())]),
        (NAMED_ON_FOE, vec![named]),
        (
            FOUND_ON_PC,
            vec![used(found(POWERS), call("GetFirstPC", []))],
        ),
        (FOUND_MEDPAC, vec![used(found(HEALING), Expr::Me)]),
        (IMPACT, Vec::new()),
    ]
    .into_iter()
    .map(|(name, statements)| Ok((name, script(&catalog, &statements)?)))
    .collect()
}

/// A Jedi who knows `POWER`.
fn jedi() -> Sheet {
    let mut jedi = Sheet::dev_jedi();
    jedi.powers = Rows::default().with(POWER);
    jedi
}

/// The medpac's item: single use, stacking to ten.
fn medpac() -> ItemFacts {
    ItemFacts {
        stacking: 10,
        spell: Some(ItemSpell {
            row: MEDPAC,
            cost: 1,
        }),
        ..super::containers::plain("medpac")
    }
}

/// What a case saw.
struct Seen {
    /// The caller queued a talent.
    talent: bool,
    /// The caller queued a cast.
    cast: bool,
    /// The medpacs the party had at the end.
    medpacs: u16,
}

/// The proving ground with the companion `MATE` in the player's party and
/// `FOE` beside it, each knowing `POWER`, and the party carrying `MEDPACS`
/// medpacs. `script` is the heartbeat of the one `on_mate` names, and runs
/// as it once at the start, the companion first hurt to `HURT_HP` if
/// `hurt`.
fn run(script: &'static str, on_mate: bool, hurt: bool) -> Result<Seen, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let size = usize::from(POWER.max(MEDPAC)) + 1;
    data.force_costs = vec![0; size];
    data.force_costs[usize::from(POWER)] = 1;
    let [x, y, z] = data.entry.position;
    let first = u32::try_from(data.placements.len()).map_err(|error| error.to_string())?;
    for (n, tag) in [MATE, FOE].into_iter().enumerate() {
        let hooks = if (tag == MATE) == on_mate {
            vec![("ScriptHeartbeat".to_string(), script.to_string())]
        } else {
            Vec::new()
        };
        let at = [x + 1.5 * (n as f32 + 1.0), y, z];
        data.placements.push(Placement::Own(ObjectData {
            scripts: hooks,
            ..ObjectData::creature(tag, tag, at, jedi())
        }));
    }
    let mut spells = vec![Talent::default(); size];
    spells[usize::from(POWER)] = Talent {
        category: POWERS as u16,
        maxcr: 1,
        hostile: true,
        reach_dm: 300,
        ..Talent::default()
    };
    spells[usize::from(MEDPAC)] = Talent {
        category: HEALING as u16,
        maxcr: 1,
        ..Talent::default()
    };
    let mut impacts = vec![String::new(); size];
    impacts[usize::from(MEDPAC)] = IMPACT.to_string();
    let talents = Talents {
        spells,
        impacts,
        ..Talents::default()
    };
    let items = Items::new([medpac()]);
    let library =
        crate::probe::library_with(CATALOG, scripts()?, talents).with_items(items.clone());
    let level = ground.with_module(data).with_scripts(library);
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level.clone()).map_err(|error| error.to_string())?;
    // The companion joins the party, as `AddPartyMember` leaves it.
    let mut start = world.snapshot();
    let mut state = world.script_state().clone();
    let find = |number: u32| {
        move |entity: &&mut EntityState| entity.kind == Kind::Object(ObjectKind::Creature, number)
    };
    let mate = start
        .entities
        .iter_mut()
        .find(find(first))
        .ok_or("no companion")?;
    let fighter = mate.fighter.as_mut().ok_or("a companion without a fight")?;
    fighter.mind.leader = Some(Seat(0));
    fighter.faction = Some(crate::combat::sheet::factions::PLAYER);
    if hurt {
        fighter.hp = HURT_HP;
    }
    let mate = mate.id;
    state.party.available.insert(
        0,
        crate::script::party::Npc {
            template: MATE.to_string(),
            object: None,
            kept: None,
            template_data: None,
        },
    );
    if !state.party.join(0, mate.0) {
        return Err("the companion did not join".into());
    }
    let foe = start
        .entities
        .iter_mut()
        .find(find(first + 1))
        .ok_or("no enemy")?
        .id;
    let medpacs = state
        .make_item(&items, "medpac", PARTY, MEDPACS, None)
        .ok_or("no medpac")?;
    let caller = if on_mate { mate } else { foe };
    state.pending.push(Pending::new(script, ObjectId(caller.0)));
    let mut world = SimWorld::from_snapshot(&start, level).map_err(|error| error.to_string())?;
    world.set_script_state(state);
    let mut seen = Seen {
        talent: false,
        cast: false,
        medpacs: MEDPACS,
    };
    for _ in 0..STEPS {
        step(&mut world);
        let state = world.script_state();
        let queue = state.actions.get(&caller.0).map_or(&[][..], Vec::as_slice);
        // Queued at the end of a step, or begun and done within one.
        let did = |kind: &str| {
            queue.iter().any(|queued| queued.action.kind() == kind)
                || world.script_report().actions.contains_key(kind)
        };
        seen.talent |= did("talent");
        seen.cast |= did("cast");
        seen.medpacs = state.items.get(&medpacs.0).map_or(0, |item| item.stack);
    }
    Ok(seen)
}

fn step(world: &mut SimWorld) {
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority);
}

/// The cases: each its name, its script, whether the companion runs it,
/// whether the companion is hurt first, and what it must see.
type Case = (&'static str, &'static str, bool, bool, fn(&Seen) -> bool);

const CASES: [Case; 4] = [
    (
        "companion_drops_found_talent",
        FOUND_ON_FOE,
        true,
        false,
        |seen| !seen.talent,
    ),
    (
        "companion_casts_named_power",
        NAMED_ON_FOE,
        true,
        false,
        |seen| seen.cast,
    ),
    (
        "enemy_keeps_found_talent",
        FOUND_ON_PC,
        false,
        false,
        |seen| seen.talent,
    ),
    (
        "companion_uses_found_medpac",
        FOUND_MEDPAC,
        true,
        true,
        |seen| seen.talent && seen.medpacs + 1 == MEDPACS,
    ),
];

pub fn probes() -> Checks {
    CASES
        .iter()
        .map(|&(name, script, on_mate, hurt, wanted)| {
            let result = run(script, on_mate, hurt).and_then(|seen| {
                check(
                    wanted(&seen),
                    format!(
                        "talent queued {}, cast queued {}, medpacs {} of {MEDPACS}",
                        seen.talent, seen.cast, seen.medpacs
                    ),
                )
            });
            (name, result)
        })
        .collect()
}
