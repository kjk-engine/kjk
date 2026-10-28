//! `kjk script effects <game>`: the effects engine checked end to end
//! (`sim::script::effects`, `combat::effects::Lasting`). Two scripts made
//! here (`script_ncs::probes::Asm`; nothing of the game's) run as the
//! player in a module (the Endar Spire by default) with only them in its
//! library, and the player's fighter is read after the steps that carry
//! the effects out:
//!
//! 1. Strength +4 for 5 s, 5 damage at once, a 2 s stun, defence −2 for
//!    5 s: the slots hold them, the sheet reads STR + 4 and defence − 2,
//!    the HP fall by 5, the player is stunned; after 6 s the modifiers
//!    have run out.
//! 2. `ClearAllEffects`, then a permanent immunity to stun and a stun: the
//!    immunity holds the only slot and no stun lands.

use std::process::ExitCode;

use serde_json::json;
use sim::combat::effects::{self, Modifies};
use sim::combat::sheet::Ability;
use vfs::Discovery;

use crate::probe::{Call, Probe, float, int, me, program};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// `ApplyEffectToObject(duration, <effect>, OBJECT_SELF, seconds)`: the
/// arguments pushed last first, the effect made in between.
fn apply(duration: i32, effect: &[Call], seconds: f32) -> Vec<Call> {
    let mut calls = vec![float(seconds), me()];
    calls.extend_from_slice(effect);
    calls.push(int(duration));
    calls.push(Call::Routine("ApplyEffectToObject", 4));
    calls
}

const INSTANT: i32 = 0;
const TEMPORARY: i32 = 1;
const PERMANENT: i32 = 2;

/// `kjk script effects`.
pub fn effects(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<(), String> {
        let (game, _, base) = open(discovery, game)?;
        let catalog = catalog(&base)?;
        let first = [
            apply(
                TEMPORARY,
                &[int(4), int(0), Call::Routine("EffectAbilityIncrease", 2)],
                5.0,
            ),
            apply(
                INSTANT,
                &[int(0), int(8), int(5), Call::Routine("EffectDamage", 3)],
                0.0,
            ),
            apply(TEMPORARY, &[Call::Routine("EffectStunned", 0)], 2.0),
            apply(
                TEMPORARY,
                &[int(2), Call::Routine("EffectACDecrease", 1)],
                5.0,
            ),
        ];
        let second = [
            vec![Call::Routine("ClearAllEffects", 0)],
            apply(
                PERMANENT,
                &[int(12), Call::Routine("EffectImmunity", 1)],
                0.0,
            ),
            apply(TEMPORARY, &[Call::Routine("EffectStunned", 0)], 2.0),
        ];
        let mut scripts = std::collections::BTreeMap::new();
        scripts.insert("kjk_effects1".to_string(), program(&catalog, &first)?);
        scripts.insert("kjk_effects2".to_string(), program(&catalog, &second)?);
        let mut probe = Probe::new(discovery, game, module, |_| {
            Ok(sim::script::Library::new(
                catalog,
                scripts,
                sim::script::Talents::default(),
            ))
        })?;
        let me = probe.me;
        let player = |snapshot: &sim::Snapshot| snapshot.get(me).cloned().expect("the player");

        // The first script: queued, run at the end of a step, carried out
        // by the next step's pipeline.
        let before = player(&probe.world.snapshot());
        let hp_before = before.fighter.map_or(0, |f| f.hp);
        probe.queue("kjk_effects1", me);
        let after = probe.step(2);
        let now = after.time_ms;
        let me_now = player(&after);
        let fighter = me_now.fighter.ok_or("no fighter")?;
        let sheet = me_now.sheet.ok_or("no sheet")?;
        let running: Vec<_> = fighter.effects.running(now).collect();
        let read = effects::sheet_now(sheet, &fighter, now);
        report.check(
            running
                .iter()
                .any(|l| l.modifies == Modifies::Ability && l.amount == 4)
                && running
                    .iter()
                    .any(|l| l.modifies == Modifies::Defence && l.amount == -2),
            format!(
                "the slots hold STR +4 and defence −2 ({} running)",
                running.len()
            ),
        );
        let str_was = sheet.abilities[Ability::Strength as usize];
        report.check(
            read.abilities[Ability::Strength as usize] == str_was + 4
                && read.defence == sheet.defence - 2,
            format!(
                "the sheet reads STR {} (was {str_was}) and defence {} (was {})",
                read.abilities[Ability::Strength as usize],
                read.defence,
                sheet.defence
            ),
        );
        report.check(
            fighter.hp == hp_before - 500,
            format!(
                "5 damage at once: {:.2} → {:.2} HP",
                f64::from(hp_before) / 100.0,
                f64::from(fighter.hp) / 100.0
            ),
        );
        report.check(
            fighter.force.stunned_until > now,
            format!(
                "stunned for {} ms more",
                fighter.force.stunned_until.saturating_sub(now)
            ),
        );
        let later = probe.step(6 * u64::from(sim::TICK_HZ));
        let fighter = player(&later).fighter.ok_or("no fighter")?;
        report.check(
            fighter.effects.running(later.time_ms).count() == 0,
            "after 6 s the 5 s modifiers have run out".to_string(),
        );

        // The second: clear, immune to stun, a stun.
        probe.queue("kjk_effects2", me);
        let after = probe.step(2);
        let fighter = player(&after).fighter.ok_or("no fighter")?;
        let running: Vec<_> = fighter.effects.running(after.time_ms).collect();
        report.check(
            running.len() == 1 && running[0].modifies == Modifies::Immunity && running[0].of == 12,
            format!(
                "ClearAllEffects, then only the immunity to stun ({} running)",
                running.len()
            ),
        );
        report.check(
            fighter.force.stunned_until <= after.time_ms,
            "the stun after it does not land".to_string(),
        );
        let ours = probe.faults();
        report.check(
            ours.is_empty(),
            format!("the scripts ran without faults ({ours:?})"),
        );
        Ok(())
    };
    if let Err(error) = run(&mut report) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    report.finish(json!({ "module": module }))
}
