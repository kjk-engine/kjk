//! `kjk script fighting <game> [module]`: the routines the combat AI
//! (`k_ai_master`, `k_inc_generic`) calls most, checked end to end. Scripts
//! made here (`probe`) run as the module's first creature with a sheet and
//! a name (the Endar Spire by default), with the module's item templates:
//!
//! 1. Given two melee weapons and a ranged one, it equips the most damaging
//!    melee weapon, then the most damaging ranged one, passing over a more
//!    damaging blade it lacks the feats for (Brejik and Bastila's
//!    lightsaber on Taris); it attacks the
//!    player, and that attack is its last (`GetLastAttackAction`) with no
//!    combat feat; it is no party member, so `ActionFollowLeader` leaves it
//!    be, and it has used no power (`GetSpellTarget`).
//! 2. It flees the player, stood beside it: a move to a place a set
//!    distance from the player (or half that, where no way leads
//!    further), and after a few seconds it stands further off than it did.
//! 3. Damage it takes is what `GetTotalDamageDealt` answers.

use std::process::ExitCode;

use script_ncs::catalog::{Catalog, Value};
use serde_json::json;
use sim::combat::Weapon;
use sim::inventory::slots;
use vfs::Discovery;

use crate::probe::{Call, Probe, float, int, me, program, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// How far the creature is asked to flee (metres), and the damage dealt it.
const FLEE: f32 = 12.0;
const DAMAGE: i32 = 4;
/// How far from it the player stands for it to flee (metres).
const BESIDE: f32 = 1.5;

fn constant(routines: &Catalog, name: &str) -> Result<i32, String> {
    routines
        .constant_named(name)
        .and_then(|c| match c.value {
            Value::Int(v) => Some(v),
            _ => None,
        })
        .ok_or(format!("no {name}"))
}

/// `Set<kind>(name, <value>)`.
fn keep(setter: &'static str, name: &str, value: Vec<Call>) -> Vec<Call> {
    let mut calls = value;
    calls.extend([text(name), Call::Routine(setter, 2)]);
    calls
}

/// `OBJECT_INVALID`.
fn nothing() -> Call {
    Call::Op(fmt_odyssey::ncs::Op::Const(
        fmt_odyssey::ncs::Const::Object(1),
    ))
}

/// `GetTag(GetItemInSlot(slot, OBJECT_SELF))`.
fn wielded(slot: u8) -> Vec<Call> {
    vec![
        me(),
        int(i32::from(slot)),
        Call::Routine("GetItemInSlot", 2),
        Call::Routine("GetTag", 1),
    ]
}

/// Twice a weapon's average blow, as `ActionEquipMostDamaging…` weighs it.
fn average(weapon: &Weapon) -> i32 {
    let dice = |count: u8, sides: u8| i32::from(count) * (i32::from(sides) + 1);
    dice(weapon.dice, weapon.die)
        + dice(weapon.bonus_dice, weapon.bonus_die)
        + 2 * i32::from(weapon.damage_bonus)
}

pub fn fighting(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let attack_object = constant(&routines, "ACTION_ATTACKOBJECT")?;
        let invalid = constant(&routines, "ACTION_INVALID")?;
        let mut chosen = None;
        let mut armoury = None;
        let mut probe = Probe::new(discovery, game, module, |level| {
            let data = level.module().ok_or("no module")?;
            let mut seen = std::collections::BTreeMap::new();
            for object in data.objects() {
                if object.kind == sim::module::ObjectKind::Creature {
                    let nth = seen.entry(object.tag.clone()).or_insert(0);
                    if let Some(sheet) = object.sheet.as_ref().filter(|_| chosen.is_none())
                        && !object.name.is_empty()
                    {
                        chosen = Some((object.tag.clone(), *nth, sheet.feats));
                    }
                    *nth += 1;
                }
            }
            let items = level
                .scripts()
                .ok_or("the module has no scripts")?
                .items()
                .clone();
            // The module's hand weapons by kind, weakest first, and
            // whether the chosen creature is trained for each.
            let feats = chosen.as_ref().map(|(.., feats)| *feats).ok_or("no creature chosen")?;
            let mut weapons: Vec<(String, String, Weapon, bool)> = items
                .iter()
                .filter(|facts| facts.slots & 1 << slots::RIGHT_WEAPON != 0)
                .filter_map(|facts| {
                    let trained = sim::combat::passive::may_equip(&feats, &facts.needs);
                    Some((facts.template.clone(), facts.tag.clone(), facts.weapon?, trained))
                })
                .collect();
            weapons.sort_by_key(|(template, _, weapon, _)| (average(weapon), template.clone()));
            // The strongest blade it is not trained for, if the module has
            // one: never drawn, though the trained blades given beside it
            // deal less.
            let untrained = weapons
                .iter()
                .rev()
                .find(|(_, _, weapon, trained)| !trained && !weapon.delivery.ranged())
                .map(|(template, tag, weapon, _)| (template.clone(), tag.clone(), average(weapon)));
            let below = untrained.as_ref().map_or(i32::MAX, |(.., deals)| *deals);
            let kind = |ranged: bool| {
                weapons
                    .iter()
                    .filter(|(_, _, weapon, trained)| weapon.delivery.ranged() == ranged && *trained)
                    .map(|(template, tag, weapon, _)| (template.clone(), tag.clone(), *weapon))
                    .collect::<Vec<_>>()
            };
            let (mut melee, ranged) = (kind(false), kind(true));
            if melee.iter().filter(|(.., weapon)| average(weapon) < below).count() >= 2 {
                melee.retain(|(.., weapon)| average(weapon) < below);
            }
            let untrained = untrained
                .filter(|_| melee.iter().all(|(.., weapon)| average(weapon) < below))
                .map(|(template, tag, _)| (template, tag));
            let weakest = melee.first().ok_or("no melee weapon")?.clone();
            let strongest = melee.last().ok_or("no melee weapon")?.clone();
            let gun = ranged.last().ok_or("no ranged weapon")?.clone();
            if average(&weakest.2) == average(&strongest.2) {
                return Err("the module's melee weapons all deal alike".into());
            }
            let give = |template: &str| {
                vec![
                    int(1),
                    me(),
                    text(template),
                    Call::Routine("CreateItemOnObject", 3),
                ]
            };
            let player = vec![Call::Routine("GetFirstPC", 0)];
            let mut first = vec![
                untrained.as_ref().map_or_else(Vec::new, |(template, _)| give(template)),
                give(&weakest.0),
                give(&strongest.0),
                give(&gun.0),
                vec![
                    int(0),
                    nothing(),
                    Call::Routine("ActionEquipMostDamagingMelee", 2),
                ],
                keep("SetGlobalString", "kjk_melee", wielded(slots::RIGHT_WEAPON)),
                vec![nothing(), Call::Routine("ActionEquipMostDamagingRanged", 1)],
                keep(
                    "SetGlobalString",
                    "kjk_ranged",
                    wielded(slots::RIGHT_WEAPON),
                ),
            ];
            let mut attack = player.clone();
            attack.insert(0, int(0));
            attack.push(Call::Routine("ActionAttack", 2));
            first.push(attack);
            let second = vec![
                keep(
                    "SetGlobalNumber",
                    "kjk_last_action",
                    vec![me(), Call::Routine("GetLastAttackAction", 1)],
                ),
                keep(
                    "SetGlobalNumber",
                    "kjk_last_feat",
                    vec![me(), Call::Routine("GetLastCombatFeatUsed", 1)],
                ),
                keep(
                    "SetGlobalNumber",
                    "kjk_spell_target",
                    vec![
                        me(),
                        Call::Routine("GetSpellTarget", 1),
                        Call::Routine("GetIsObjectValid", 1),
                    ],
                ),
                vec![Call::Routine("ClearAllActions", 0)],
                vec![Call::Routine("ActionFollowLeader", 0)],
                // As text: ACTION_INVALID is past a number global's byte.
                keep(
                    "SetGlobalString",
                    "kjk_following",
                    vec![
                        me(),
                        Call::Routine("GetCurrentAction", 1),
                        Call::Routine("IntToString", 1),
                    ],
                ),
                {
                    let mut flee = vec![float(FLEE), int(1)];
                    flee.extend(player.clone());
                    flee.push(Call::Routine("ActionMoveAwayFromObject", 3));
                    flee
                },
            ];
            let third = vec![
                vec![int(0), me(), Call::Routine("SetPlotFlag", 2)],
                vec![
                    float(0.0),
                    me(),
                    int(0),
                    int(8),
                    int(DAMAGE),
                    Call::Routine("EffectDamage", 3),
                    int(0),
                    Call::Routine("ApplyEffectToObject", 4),
                ],
            ];
            let fourth = vec![keep(
                "SetGlobalNumber",
                "kjk_dealt",
                vec![Call::Routine("GetTotalDamageDealt", 0)],
            )];
            let mut scripts = std::collections::BTreeMap::new();
            for (name, calls) in [
                ("kjk_fight1", first),
                ("kjk_fight2", second),
                ("kjk_fight3", third),
                ("kjk_fight4", fourth),
            ] {
                scripts.insert(name.to_string(), program(&routines, &calls)?);
            }
            armoury = Some((strongest.1, gun.1, weakest.1, untrained.map(|(_, tag)| tag)));
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_items(items),
            )
        })?;
        let (tag, nth, _) = chosen.ok_or("no creature chosen")?;
        let (strongest, gun, weakest, untrained) = armoury.ok_or("no weapons chosen")?;
        let snapshot = probe.world.snapshot();
        let creature = crate::scenes::tagged(probe.world.level(), &snapshot, &tag)
            .get(nth as usize)
            .map(|&at| snapshot.entities[at].id)
            .ok_or("the chosen creature")?;
        let numbers = |probe: &Probe, name: &str| {
            let state = probe.world.script_state();
            sim::script::global(&state.numbers, name).copied().unwrap_or(i32::MIN)
        };
        let strings = |probe: &Probe, name: &str| {
            let state = probe.world.script_state();
            sim::script::global(&state.strings, name).cloned().unwrap_or_else(String::new)
        };
        let me_id = probe.me;
        let apart = |snapshot: &sim::Snapshot| -> Result<f32, String> {
            let at = |id| {
                snapshot
                    .get(id)
                    .map(|entity| entity.position)
                    .ok_or("an entity gone")
            };
            let (a, b) = (at(creature)?, at(me_id)?);
            Ok(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt())
        };

        // 1. Weapons, an attack.
        probe.queue("kjk_fight1", creature);
        probe.step(2);
        let melee = strings(&probe, "kjk_melee");
        report.check(
            melee.eq_ignore_ascii_case(&strongest),
            format!(
                "{tag} equips the most damaging melee weapon it is trained for: {melee} (not {weakest}, nor the untrained {})",
                untrained.as_deref().unwrap_or("(none in the module)")
            ),
        );
        let ranged = strings(&probe, "kjk_ranged");
        report.check(
            ranged.eq_ignore_ascii_case(&gun),
            format!("then the most damaging ranged one: {ranged}"),
        );

        // 2. Its last attack, fleeing (the player stood beside it).
        let at = probe
            .world
            .snapshot()
            .get(creature)
            .ok_or("the creature")?
            .position;
        probe.place(at[0] + BESIDE, at[1])?;
        probe.queue("kjk_fight2", creature);
        let before = probe.step(1);
        let action = numbers(&probe, "kjk_last_action");
        let feat = numbers(&probe, "kjk_last_feat");
        report.check(
            action == attack_object && feat == -1,
            format!("its last attack action {action} (ACTION_ATTACKOBJECT {attack_object}), no combat feat ({feat})"),
        );
        report.check(
            numbers(&probe, "kjk_spell_target") == 0,
            "it has used no power: no spell target".into(),
        );
        let following: i32 = strings(&probe, "kjk_following").parse().unwrap_or(-1);
        report.check(
            following == invalid,
            format!("no party member: ActionFollowLeader queues nothing ({following})"),
        );
        let queued = probe
            .world
            .script_state()
            .actions
            .get(&creature.0)
            .and_then(|queue| queue.first())
            .map(|queued| queued.action.clone());
        let player_at = before.get(probe.me).ok_or("the player")?.position;
        let flee_to = match queued {
            Some(sim::script::actions::Action::Move {
                to: sim::script::actions::Target::Point(at, _),
                run: true,
                ..
            }) => Some(((at[0] - player_at[0]).powi(2) + (at[1] - player_at[1]).powi(2)).sqrt()),
            _ => None,
        };
        report.check(
            flee_to.is_some_and(|far| (far - FLEE).abs() < 0.05 || (far - FLEE / 2.0).abs() < 0.05),
            format!("it runs for a place {flee_to:?} m from the player ({FLEE} asked, or half)"),
        );
        let near = apart(&before)?;
        let after = probe.step(400);
        let far = apart(&after)?;
        report.check(
            far > near + 1.0,
            format!("fleeing, it went from {near:.1} m to {far:.1} m off"),
        );

        // 3. Damage dealt it.
        probe.queue("kjk_fight3", creature);
        probe.step(3);
        probe.queue("kjk_fight4", creature);
        probe.step(1);
        let dealt = numbers(&probe, "kjk_dealt");
        report.check(
            dealt == DAMAGE,
            format!("GetTotalDamageDealt after {DAMAGE} damage: {dealt}"),
        );
        let faults = probe.faults();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        Ok(json!({ "module": module, "creature": tag, "melee": melee, "ranged": ranged }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
