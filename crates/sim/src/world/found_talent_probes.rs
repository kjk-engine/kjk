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
//!   found still uses it, and the party then has one medpac fewer;
//! - a companion's hook that uses a feat `GetCreatureTalentBest` found still
//!   has that feat in its intent, as the engine chooses no feats yet;
//! - `GetIdFromTalent`, `GetTypeFromTalent`, `GetCategoryFromTalent` and
//!   `GetIsTalentValid` read a found power and a found feat as they read
//!   the same talent made with `TalentSpell` or `TalentFeat`.

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
use crate::script::actions::Use;
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

/// The feat both creatures have (a `feat.2da` row).
const FEAT: u16 = 8;

/// The talent categories the scripts ask for (`Talent::category`).
const POWERS: i32 = 1;
const HEALING: i32 = 2;
const FEATS: i32 = 4;

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
// 5
talent TalentSpell(int nSpell);
// 6
talent TalentFeat(int nFeat);
// 7
int GetIdFromTalent(talent tTalent);
// 8
int GetTypeFromTalent(talent tTalent);
// 9
int GetCategoryFromTalent(talent tTalent);
// 10
int GetIsTalentValid(talent tTalent);
// 11
void SetLocalNumber(object oObject, int nIndex, int nValue);
"#;

/// The routines that read a talent, two to a script (`READ_IDS`,
/// `READ_KINDS`). K1 keeps 8 local numbers an object, so a script keeps
/// the reads of two routines.
const ID_READERS: [&str; 2] = ["GetIdFromTalent", "GetTypeFromTalent"];
const KIND_READERS: [&str; 2] = ["GetCategoryFromTalent", "GetIsTalentValid"];

/// The scripts, each an AI hook (its heartbeat) of the creature that runs
/// it: a power found and used on the enemy, a power named and cast at the
/// enemy, a power found and used on the player, a medpac found and used on
/// the creature itself, and a feat found and used on the enemy. `READ_IDS`
/// and `READ_KINDS` keep each of their two routines' reads of the power and
/// the feat, found and named, in the creature's local numbers: for routine
/// `r`, power found at `4r`, power named at `4r + 1`, feat found at
/// `4r + 2` and feat named at `4r + 3`.
const FOUND_ON_FOE: &str = "probe_found_on_foe";
const NAMED_ON_FOE: &str = "probe_named_on_foe";
const FOUND_ON_PC: &str = "probe_found_on_pc";
const FOUND_MEDPAC: &str = "probe_found_medpac";
const FOUND_FEAT: &str = "probe_found_feat";
const READ_IDS: &str = "probe_read_ids";
const READ_KINDS: &str = "probe_read_kinds";

/// The local numbers a reads script keeps.
const LOCALS: i32 = 8;
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
    let talents = || {
        [
            found(POWERS),
            call("TalentSpell", [Expr::Int(i32::from(POWER))]),
            found(FEATS),
            call("TalentFeat", [Expr::Int(i32::from(FEAT))]),
        ]
    };
    let reads = |readers: [&str; 2]| {
        (0..)
            .zip(
                readers
                    .into_iter()
                    .flat_map(|reader| talents().map(|talent| (reader, talent))),
            )
            .map(|(at, (reader, talent))| {
                call(
                    "SetLocalNumber",
                    [Expr::Me, Expr::Int(at), call(reader, [talent])],
                )
            })
            .collect()
    };
    [
        (FOUND_ON_FOE, vec![used(found(POWERS), foe())]),
        (NAMED_ON_FOE, vec![named]),
        (
            FOUND_ON_PC,
            vec![used(found(POWERS), call("GetFirstPC", []))],
        ),
        (FOUND_MEDPAC, vec![used(found(HEALING), Expr::Me)]),
        (FOUND_FEAT, vec![used(found(FEATS), foe())]),
        (READ_IDS, reads(ID_READERS)),
        (READ_KINDS, reads(KIND_READERS)),
        (IMPACT, Vec::new()),
    ]
    .into_iter()
    .map(|(name, statements)| Ok((name, script(&catalog, &statements)?)))
    .collect()
}

/// A Jedi who knows `POWER` and has `FEAT`.
fn jedi() -> Sheet {
    let mut jedi = Sheet::dev_jedi();
    jedi.powers = Rows::default().with(POWER);
    jedi.feats = jedi.feats.with(FEAT);
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
    /// The caller's intent used `FEAT`.
    feat: bool,
    /// The medpacs the party had at the end.
    medpacs: u16,
    /// The caller's local numbers 0 to `LOCALS - 1` at the end (`READ_IDS`,
    /// `READ_KINDS`).
    locals: Vec<Option<i32>>,
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
    let mut feats = vec![Talent::default(); usize::from(FEAT) + 1];
    feats[usize::from(FEAT)] = Talent {
        category: FEATS as u16,
        maxcr: 1,
        ..Talent::default()
    };
    let talents = Talents {
        spells,
        impacts,
        feats,
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
        feat: false,
        medpacs: MEDPACS,
        locals: Vec::new(),
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
        seen.feat |= state
            .intents
            .get(&caller.0)
            .and_then(|intent| intent.talent)
            == Some(Use::Feat(FEAT));
        seen.medpacs = state.items.get(&medpacs.0).map_or(0, |item| item.stack);
    }
    let locals = &world.script_state().locals;
    seen.locals = (0..LOCALS)
        .map(|at| {
            locals
                .iter()
                .find(|((_, kind, index), _)| *kind == 1 && *index == at)
                .map(|(_, value)| *value)
        })
        .collect();
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

/// Whether every routine read each found talent as it read the same talent
/// named.
fn reads_alike(seen: &Seen) -> bool {
    seen.locals.len() == LOCALS as usize
        && seen
            .locals
            .chunks(2)
            .all(|pair| pair[0].is_some() && pair[0] == pair[1])
}

/// `reads_alike`, and `GetIdFromTalent` read `POWER` and `FEAT`.
fn ids_alike(seen: &Seen) -> bool {
    let ids = [POWER, POWER, FEAT, FEAT].map(|row| Some(i32::from(row)));
    reads_alike(seen) && seen.locals.get(..4) == Some(&ids[..])
}

const CASES: [Case; 7] = [
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
    (
        "companion_keeps_found_feat",
        FOUND_FEAT,
        true,
        false,
        |seen| seen.feat,
    ),
    (
        "found_talent_id_and_type_as_named",
        READ_IDS,
        true,
        false,
        ids_alike,
    ),
    (
        "found_talent_category_and_valid_as_named",
        READ_KINDS,
        true,
        false,
        reads_alike,
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
                        "talent queued {}, cast queued {}, feat intended {}, \
                         medpacs {} of {MEDPACS}, locals {:?}",
                        seen.talent, seen.cast, seen.feat, seen.medpacs, seen.locals
                    ),
                )
            });
            (name, result)
        })
        .collect()
}
