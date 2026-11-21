//! A medpac used under steady hits (`kjk sim probes --only "medpac hits"`),
//! on the proving ground with scripts, no install. A hurt character uses a
//! medpac through its script's item talent, as the games' AI script
//! `k_ai_master` uses one. The probe then takes a tenth of a hit point from it
//! every half second, and each loss runs its damaged script.
//!
//! The AI's scripts are written as `k_ai_master` is. Its damaged script
//! starts a new combat round only when the character has no attack target,
//! no attempted attack target and no attempted spell target. The new round
//! clears the character's actions and uses the medpac again. Its end of
//! round does the same with no test.
//!
//! - An enemy's medpac lands once its use is over. The hits do not break the
//!   use off, because the scripts see the use as the enemy's current action.
//! - A companion's medpac lands the same way.
//! - An enemy whose first hit comes late in a combat round still heals. Its
//!   round ends only once the use is over, so the round's own
//!   `ClearAllActions` does not break the use off.
//! - A story script that clears the enemy's actions on every hit, with no
//!   test, still breaks the use off. The enemy never heals and keeps its
//!   medpac.

use script_ncs::asm::{Expr, call, not, script, when};
use script_ncs::catalog::Catalog as Routines;

use super::{Clock, Fighting, Id, SimWorld};
use crate::combat::Sheet;
use crate::combat::items::heal_ms;
use crate::combat::sheet::{factions, skills};
use crate::combat::spells::ITEM_ABILITY_MED_PACK;
use crate::input::{Command, Seat, TickInput};
use crate::inventory::{Catalog, ItemFacts, ItemSpell, PARTY};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::party::Npc;
use crate::script::{Talent, Talents};
use crate::{StepReason, WorldConfig};

const MEDPAC: u16 = ITEM_ABILITY_MED_PACK;
/// How many medpacs the character carries.
const MEDPACS: u16 = 2;
const STEP_MS: u64 = 50;
/// How often a hit lands, and what it takes (hundredths of a hit point).
const HIT_EVERY_MS: u64 = 500;
const HIT: i32 = 10;
/// How long each run lasts.
const RUN_MS: u64 = 7000;
/// The character's hit points as the run begins (hundredths of its maximum).
const HURT_TO: i32 = 30;
const TAG: &str = "patient";
/// What the medpac's impact script heals (whole hit points).
const HEALS: i32 = 5;
/// The name of the medpac's impact script.
const IMPACT: &str = "pm_heal";

const ROUTINES: &str = r#"
// 0
void ClearAllActions();
// 1
talent TalentSpell(int nSpell);
// 2
void ActionUseTalentOnObject(talent tChosenTalent, object oTarget);
// 3
int GetIsObjectValid(object oObject);
// 4
object GetAttemptedAttackTarget();
// 5
object GetAttemptedSpellTarget();
// 6
object GetAttackTarget(object oCreature);
// 7
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration = 0.0f);
// 8
effect EffectHeal(int nDamageToHeal);
"#;

/// Who uses the medpac.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Who {
    Enemy,
    Companion,
}

/// What its scripts do.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Scripts {
    /// `k_ai_master`'s damaged script and end of round.
    Ai,
    /// A spawn script that uses the medpac, and a damaged script that only
    /// clears its actions.
    Story,
}

/// One run: who, what its scripts do, and when the first hit lands. In a
/// run whose first hit comes after `HIT_EVERY_MS`, the character is in
/// combat from the start. Its first combat round then ends during the use
/// that the first hit begins.
#[derive(Debug, Clone, Copy)]
struct Case {
    who: Who,
    scripts: Scripts,
    first_hit_ms: u64,
}

/// What a run saw: when each use began, when the first heal landed, the
/// medpacs left then (or at the end, with no heal), and how long a use of
/// the character's takes.
#[derive(Debug)]
struct Seen {
    begun: Vec<u64>,
    healed_at: Option<u64>,
    left: u16,
    use_ms: u64,
}

fn use_medpac() -> Expr {
    call(
        "ActionUseTalentOnObject",
        [
            call("TalentSpell", [Expr::Int(i32::from(MEDPAC))]),
            Expr::Me,
        ],
    )
}

fn invalid(object: Expr) -> Expr {
    not(call("GetIsObjectValid", [object]))
}

/// The medpac's impact script, as K1's `k_sup_healing` heals: `HEALS` to
/// its user at once.
fn impact() -> Result<fmt_odyssey::ncs::Ncs, String> {
    let routines = Routines::parse(ROUTINES).map_err(|e| format!("{e:?}"))?;
    let heal = call("EffectHeal", [Expr::Int(HEALS)]);
    let apply = call(
        "ApplyEffectToObject",
        [Expr::Int(0), heal, Expr::Me, Expr::Float(0.0)],
    );
    script(&routines, &[apply])
}

/// The name of the script a hook runs, such as `pm_damaged` for
/// `ScriptDamaged`.
fn resref(hook: &str) -> String {
    format!(
        "pm_{}",
        hook.trim_start_matches("Script").to_ascii_lowercase()
    )
}

/// The scripts of `scripts`, by hook.
fn hooks(scripts: Scripts) -> Result<Vec<(&'static str, fmt_odyssey::ncs::Ncs)>, String> {
    let routines = Routines::parse(ROUTINES).map_err(|e| format!("{e:?}"))?;
    let clear = || call("ClearAllActions", []);
    let round = || vec![clear(), use_medpac()];
    let made: Vec<(&str, Vec<Expr>)> = match scripts {
        Scripts::Ai => vec![
            (
                "ScriptDamaged",
                vec![when(
                    invalid(call("GetAttemptedAttackTarget", [])),
                    [when(
                        invalid(call("GetAttemptedSpellTarget", [])),
                        [when(invalid(call("GetAttackTarget", [Expr::Me])), round())],
                    )],
                )],
            ),
            ("ScriptEndRound", round()),
        ],
        Scripts::Story => vec![
            ("ScriptSpawn", vec![use_medpac()]),
            ("ScriptDamaged", vec![clear()]),
        ],
    };
    made.into_iter()
        .map(|(hook, statements)| Ok((hook, script(&routines, &statements)?)))
        .collect()
}

/// The proving ground with the character of `case`, hurt, carrying
/// `MEDPACS`; the world and the character's id.
fn stage(case: Case) -> Result<(SimWorld, u32), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
    let faction = match case.who {
        Who::Enemy => factions::HOSTILE_1,
        Who::Companion => factions::PLAYER,
    };
    let hooks = hooks(case.scripts)?;
    data.placements.push(Placement::Own(ObjectData {
        sheet: Some(Sheet {
            faction,
            ..Sheet::dev_soldier()
        }),
        template: TAG.into(),
        scripts: hooks
            .iter()
            .map(|(hook, _)| (hook.to_string(), resref(hook)))
            .collect(),
        ..ObjectData::bare(
            ObjectKind::Creature,
            TAG,
            crate::determinism::ground_point(-4.0, 0.0, 0.0),
        )
    }));
    let catalog = Catalog::new([ItemFacts {
        stacking: 10,
        spell: Some(ItemSpell {
            row: MEDPAC,
            cost: 1,
        }),
        ..super::containers::probes::plain("medpac")
    }]);
    let rows = usize::from(MEDPAC) + 1;
    let mut impacts = vec![String::new(); rows];
    impacts[usize::from(MEDPAC)] = IMPACT.into();
    let talents = Talents {
        spells: vec![Talent::default(); rows],
        impacts,
        ..Talents::default()
    };
    let scripts = hooks
        .into_iter()
        .map(|(hook, ncs)| (resref(hook), ncs))
        .chain([(IMPACT.to_string(), impact()?)]);
    let library =
        crate::probe::library_with(ROUTINES, scripts, talents).with_items(catalog.clone());
    let level = ground.with_module(data).with_scripts(library);
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let patient = start
        .entities
        .iter()
        .find(|entity| {
            objects
                .of_entity(entity)
                .is_some_and(|data| data.tag == TAG)
        })
        .map(|entity| entity.id.0)
        .ok_or("no patient")?;
    let mut state = world.script_state().clone();
    let holder = match case.who {
        Who::Enemy => patient,
        Who::Companion => {
            let npc = Npc {
                template: TAG.into(),
                object: Some(patient),
                kept: None,
                template_data: None,
            };
            state.party.available.insert(0, npc);
            state.party.members.push(0);
            PARTY
        }
    };
    state
        .make_item(&catalog, "medpac", holder, MEDPACS, None)
        .ok_or("no medpac")?;
    world.set_script_state(state);
    let late = case.first_hit_ms > HIT_EVERY_MS;
    with_patient(&mut world, patient, |fighter, max| {
        fighter.hp = max * HURT_TO;
        if late {
            fighter.combat_until = u64::MAX;
        }
        if case.who == Who::Companion {
            fighter.mind.leader = Some(Seat(0));
        }
    });
    Ok((world, patient))
}

/// `change` made to the patient's fighter, given its maximum hit points
/// (whole points); what `change` returns.
fn with_patient<T>(
    world: &mut SimWorld,
    patient: u32,
    change: impl FnOnce(&mut crate::combat::Fighter, i32) -> T,
) -> Option<T> {
    let max = i32::from(Sheet::dev_soldier().max_hp);
    let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
    fighters
        .iter_mut(&mut world.world)
        .find(|(id, _)| id.0.0 == patient)
        .map(|(_, mut fighting)| change(&mut fighting.0, max))
}

/// The medpacs left with whoever holds them.
fn medpacs_left(world: &SimWorld) -> u16 {
    (world.script_state().items.values())
        .filter(|item| item.template == "medpac")
        .map(|item| item.stack)
        .sum()
}

fn run(case: Case) -> Result<Seen, String> {
    let (mut world, patient) = stage(case)?;
    let host = world.snapshot().body(Seat(0)).ok_or("no host")?.id;
    let rank = Sheet::dev_soldier()
        .skill(skills::TREAT_INJURY)
        .clamp(0, 255) as u8;
    let mut seen = Seen {
        begun: Vec::new(),
        healed_at: None,
        left: 0,
        use_ms: heal_ms(rank),
    };
    let mut next_hit = case.first_hit_ms;
    for tick in 1..=RUN_MS / STEP_MS {
        let now = world.world.resource::<Clock>().time_ms;
        let hit = now >= next_hit;
        if hit {
            next_hit += HIT_EVERY_MS;
        }
        let before = with_patient(&mut world, patient, |fighter, _| {
            if hit {
                fighter.hp -= HIT;
                fighter.last_attacker = Some((host, now));
                fighter.combat_until = fighter.combat_until.max(now + crate::combat::COMBAT_MS);
            }
            fighter.hp
        })
        .ok_or("no patient")?;
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        crate::step(
            &mut world,
            tick,
            &input,
            STEP_MS as u32,
            StepReason::Authority,
        );
        let now = world.world.resource::<Clock>().time_ms;
        let (hp, last) = with_patient(&mut world, patient, |fighter, _| {
            (fighter.hp, fighter.pack.using.map(|_| fighter.pack.last.1))
        })
        .ok_or("no patient")?;
        if let Some(began) = last.filter(|began| seen.begun.last() != Some(began)) {
            seen.begun.push(began);
        }
        if hp >= before + 100 && seen.healed_at.is_none() {
            seen.healed_at = Some(now);
            seen.left = medpacs_left(&world);
        }
    }
    if seen.healed_at.is_none() {
        seen.left = medpacs_left(&world);
    }
    Ok(seen)
}

/// Whether the first use landed once its wind-up was over, no hit or round
/// having broken it off. Only one use began before the heal. The heal came
/// between the use's time and two steps more after the use began. One
/// medpac was spent.
fn healed_once(seen: &Seen) -> bool {
    let first = seen.begun.first().copied();
    let before_heal = |at: u64| seen.begun.iter().filter(|began| **began < at).count();
    match (first, seen.healed_at) {
        (Some(began), Some(at)) => {
            let took = at - began;
            before_heal(at) == 1
                && took >= seen.use_ms
                && took <= seen.use_ms + 2 * STEP_MS
                && seen.left == MEDPACS - 1
        }
        _ => false,
    }
}

/// Whether the use was broken off and never landed, the medpac kept.
fn broken_off(seen: &Seen) -> bool {
    !seen.begun.is_empty() && seen.healed_at.is_none() && seen.left == MEDPACS
}

pub fn probes() -> Checks {
    let cases: [(&str, Case, fn(&Seen) -> bool); 4] = [
        (
            "medpac_hits_enemy_heals",
            Case {
                who: Who::Enemy,
                scripts: Scripts::Ai,
                first_hit_ms: 0,
            },
            healed_once,
        ),
        (
            "medpac_hits_companion_heals",
            Case {
                who: Who::Companion,
                scripts: Scripts::Ai,
                first_hit_ms: 0,
            },
            healed_once,
        ),
        (
            "medpac_hits_late_in_round_heals",
            Case {
                who: Who::Enemy,
                scripts: Scripts::Ai,
                first_hit_ms: 2400,
            },
            healed_once,
        ),
        (
            "medpac_hits_story_clear_breaks_off",
            Case {
                who: Who::Enemy,
                scripts: Scripts::Story,
                first_hit_ms: 0,
            },
            broken_off,
        ),
    ];
    cases
        .into_iter()
        .map(|(name, case, wanted)| {
            let result =
                run(case).and_then(|seen| check(wanted(&seen), format!("{case:?}: {seen:?}")));
            (name, result)
        })
        .collect()
}
