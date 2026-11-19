//! The probes of the talents a companion's combat script queues (`kjk sim
//! probes --only "companion talents"`), on the proving ground, with no
//! install. A companion of the player's party and an enemy stand side by
//! side. One of them runs one script once. The cases:
//! - a companion's AI hook that uses a power it names with `TalentSpell`
//!   queues no talent, because the engine chooses its powers;
//! - the same is true of a power that `GetCreatureTalentBest` found;
//! - a companion's AI hook that casts a power it names with
//!   `ActionCastSpellAtObject` still casts it;
//! - an enemy's AI hook keeps a power it names and a power it found;
//! - a hurt companion's AI hook that uses a medpac still uses it, and the
//!   party then has one medpac fewer;
//! - a companion's AI hook that uses a feat still has that feat in its
//!   intent, because the engine chooses no feats yet;
//! - a script that is not the companion's AI hook and uses a power for the
//!   companion still queues the talent;
//! - a droid companion's AI hook keeps its stun ray, which is one of the
//!   droid's own weapons, and drops its energy shield.

use script_ncs::asm::{Expr, call, script};
use script_ncs::catalog::Catalog;
use script_ncs::vm::ObjectId;

use super::SimWorld;
use crate::combat::Sheet;
use crate::combat::sheet::{Rows, flags};
use crate::combat::spells::{
    DROID_ITEM_ENERGY_SHIELD_1, DROID_ITEM_STUN_RAY_1, ITEM_ABILITY_MED_PACK, K1_FORCE_POWERS,
};
use crate::input::{Command, Seat, TickInput};
use crate::inventory::{Catalog as Items, ItemFacts, ItemSpell, PARTY};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::script::actions::Use;
use crate::script::{Pending, Talent, Talents};
use crate::snapshot::{EntityState, Kind};
use crate::{StepReason, WorldConfig};

const MSEC: u32 = 50;

/// The steps each case runs, which is 2.75 seconds. The script runs once,
/// before the caller's own heartbeat first comes due at 3 seconds.
const STEPS: usize = 55;

/// The power both creatures know, the medpac's row, and the droid's stun
/// ray and energy shield.
const POWER: u16 = K1_FORCE_POWERS[0];
const MEDPAC: u16 = ITEM_ABILITY_MED_PACK;
const STUN_RAY: u16 = DROID_ITEM_STUN_RAY_1;
const SHIELD: u16 = DROID_ITEM_ENERGY_SHIELD_1;

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

/// The routines the scripts call. The talent types are needed. Without
/// them, the excluded type -1 of `GetCreatureTalentBest` matches every
/// talent.
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
"#;

/// The scripts. Each uses a talent or casts a power once.
const NAMED_ON_FOE: &str = "probe_named_on_foe";
const FOUND_ON_FOE: &str = "probe_found_on_foe";
const CAST_ON_FOE: &str = "probe_cast_on_foe";
const NAMED_ON_PC: &str = "probe_named_on_pc";
const FOUND_ON_PC: &str = "probe_found_on_pc";
const FOUND_MEDPAC: &str = "probe_found_medpac";
const FOUND_FEAT: &str = "probe_found_feat";
const STUN_RAY_ON_FOE: &str = "probe_stun_ray_on_foe";
const SHIELD_ON_SELF: &str = "probe_shield_on_self";

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
    let named = |row: u16| call("TalentSpell", [Expr::Int(i32::from(row))]);
    let foe = || call("GetObjectByTag", [Expr::String(FOE.into()), Expr::Int(0)]);
    let pc = || call("GetFirstPC", []);
    let used = |talent: Expr, on: Expr| vec![call("ActionUseTalentOnObject", [talent, on])];
    let cast = call(
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
        (NAMED_ON_FOE, used(named(POWER), foe())),
        (FOUND_ON_FOE, used(found(POWERS), foe())),
        (CAST_ON_FOE, vec![cast]),
        (NAMED_ON_PC, used(named(POWER), pc())),
        (FOUND_ON_PC, used(found(POWERS), pc())),
        (FOUND_MEDPAC, used(found(HEALING), Expr::Me)),
        (FOUND_FEAT, used(found(FEATS), foe())),
        (STUN_RAY_ON_FOE, used(named(STUN_RAY), foe())),
        (SHIELD_ON_SELF, used(named(SHIELD), Expr::Me)),
        (IMPACT, Vec::new()),
    ]
    .into_iter()
    .map(|(name, statements)| Ok((name, script(&catalog, &statements)?)))
    .collect()
}

/// A Jedi who knows `POWER` and has `FEAT`. A droid has the droid flag as
/// well.
fn jedi(droid: bool) -> Sheet {
    let mut jedi = Sheet::dev_jedi();
    jedi.powers = Rows::default().with(POWER);
    jedi.feats = jedi.feats.with(FEAT);
    if droid {
        jedi.flags |= flags::DROID;
    }
    jedi
}

/// The medpac's item. It is used once and stacks to ten.
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

/// Who runs a case's script.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Caller {
    Mate,
    Foe,
}

/// A case: its name, its script and who runs it, whether the script is the
/// caller's AI hook (its heartbeat) or a plain script, whether the companion
/// is hurt first, whether the companion is a droid, and what it must see.
struct Case {
    name: &'static str,
    script: &'static str,
    caller: Caller,
    hooked: bool,
    hurt: bool,
    droid: bool,
    wanted: fn(&Seen) -> bool,
}

/// What every case starts from: the companion runs the script as its AI
/// hook, at full health, and is not a droid.
const PLAIN: Case = Case {
    name: "",
    script: "",
    caller: Caller::Mate,
    hooked: true,
    hurt: false,
    droid: false,
    wanted: |_| false,
};

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
}

/// The proving ground with the companion `MATE` in the player's party and
/// `FOE` beside it. Each knows `POWER`, and the party carries `MEDPACS`
/// medpacs. The case's caller runs its script once at the start.
fn run(case: &Case) -> Result<Seen, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let size = usize::from(POWER.max(MEDPAC).max(STUN_RAY).max(SHIELD)) + 1;
    data.force_costs = vec![0; size];
    data.force_costs[usize::from(POWER)] = 1;
    let [x, y, z] = data.entry.position;
    let first = u32::try_from(data.placements.len()).map_err(|error| error.to_string())?;
    for (n, (tag, caller)) in [(MATE, Caller::Mate), (FOE, Caller::Foe)]
        .into_iter()
        .enumerate()
    {
        let hooks = if case.hooked && case.caller == caller {
            vec![("ScriptHeartbeat".to_string(), case.script.to_string())]
        } else {
            Vec::new()
        };
        let at = [x + 1.5 * (n as f32 + 1.0), y, z];
        let droid = case.droid && caller == Caller::Mate;
        data.placements.push(Placement::Own(ObjectData {
            scripts: hooks,
            ..ObjectData::creature(tag, tag, at, jedi(droid))
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
    if case.hurt {
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
    let caller = match case.caller {
        Caller::Mate => mate,
        Caller::Foe => foe,
    };
    state
        .pending
        .push(Pending::new(case.script, ObjectId(caller.0)));
    let mut world = SimWorld::from_snapshot(&start, level).map_err(|error| error.to_string())?;
    world.set_script_state(state);
    let mut seen = Seen {
        talent: false,
        cast: false,
        feat: false,
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
        seen.feat |= state
            .intents
            .get(&caller.0)
            .and_then(|intent| intent.talent)
            == Some(Use::Feat(FEAT));
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

/// The cases (see the module documentation).
const CASES: [Case; 10] = [
    Case {
        name: "companion_drops_named_power",
        script: NAMED_ON_FOE,
        wanted: |seen| !seen.talent,
        ..PLAIN
    },
    Case {
        name: "companion_drops_found_power",
        script: FOUND_ON_FOE,
        wanted: |seen| !seen.talent,
        ..PLAIN
    },
    Case {
        name: "companion_casts_named_power",
        script: CAST_ON_FOE,
        wanted: |seen| seen.cast,
        ..PLAIN
    },
    Case {
        name: "enemy_keeps_named_power",
        script: NAMED_ON_PC,
        caller: Caller::Foe,
        wanted: |seen| seen.talent,
        ..PLAIN
    },
    Case {
        name: "enemy_keeps_found_power",
        script: FOUND_ON_PC,
        caller: Caller::Foe,
        wanted: |seen| seen.talent,
        ..PLAIN
    },
    Case {
        name: "companion_uses_found_medpac",
        script: FOUND_MEDPAC,
        hurt: true,
        wanted: |seen| seen.talent && seen.medpacs + 1 == MEDPACS,
        ..PLAIN
    },
    Case {
        name: "companion_keeps_found_feat",
        script: FOUND_FEAT,
        wanted: |seen| seen.feat,
        ..PLAIN
    },
    Case {
        name: "companion_keeps_power_from_other_script",
        script: NAMED_ON_FOE,
        hooked: false,
        wanted: |seen| seen.talent,
        ..PLAIN
    },
    Case {
        name: "droid_keeps_stun_ray",
        script: STUN_RAY_ON_FOE,
        droid: true,
        wanted: |seen| seen.talent,
        ..PLAIN
    },
    Case {
        name: "droid_drops_shield",
        script: SHIELD_ON_SELF,
        droid: true,
        wanted: |seen| !seen.talent,
        ..PLAIN
    },
];

pub fn probes() -> Checks {
    CASES
        .iter()
        .map(|case| {
            let result = run(case).and_then(|seen| {
                check(
                    (case.wanted)(&seen),
                    format!(
                        "talent queued {}, cast queued {}, feat intended {}, medpacs {} of {MEDPACS}",
                        seen.talent, seen.cast, seen.feat, seen.medpacs
                    ),
                )
            });
            (case.name, result)
        })
        .collect()
}
