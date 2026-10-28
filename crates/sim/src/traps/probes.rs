//! The probes of `traps` (`kjk sim probes`; the approved test
//! `traps_lifecycle`).

use super::{Laid, Outcome, TrapRow, blast_strength, blasts, door_kit};

fn check(ok: bool, what: String) -> Result<String, String> {
    if ok { Ok(what) } else { Err(what) }
}

/// `traps.2da` row 3 as K1 has it (TRAP_FRAGMENTATION_MINE_MINOR).
fn frag_minor() -> TrapRow {
    TrapRow {
        label: "TRAP_FRAGMENTATION_MINE_MINOR".into(),
        script: "k_trp_generic".into(),
        set_dc: 15,
        detect_mod: 20,
        disarm_mod: 20,
        name: 1281,
        kit: "g_i_trapkit004".into(),
        model: "v_mnfrag".into(),
        sound: "cb_gr_fragment".into(),
    }
}

/// KOTOR's numbers at fixed rolls (the spec's appendix).
fn rules() -> Vec<(&'static str, Result<String, String>)> {
    use super::{best_kit, detects, disarm, laid_row, laid_template, lay, recover, springs};
    let row = frag_minor();
    let laid = lay(3, 3, None, &row);
    let ranked = lay(3, 5, None, &row);
    let quirk = lay(-10, 0, Some(1), &row);
    let failed = lay(-1, 0, Some(10), &row);
    let best = best_kit([0, 4, 3, 9]);
    vec![
        // Awareness 4 + d10 6 + 10 = 20 meets DC 20; running loses 5.
        (
            "detect_meets_dc",
            check(detects(4, 6, false, 20), "4+6+10 vs 20".into()),
        ),
        (
            "detect_running",
            check(!detects(4, 6, true, 20), "4+6+10-5 vs 20".into()),
        ),
        // Out of combat it takes 20: 20 + 0 vs 20 is done.
        (
            "disarm_take_20",
            check(
                disarm(0, None, 20, false) == Outcome::Done,
                "20+0 vs 20".into(),
            ),
        ),
        // d20 3 + 2 = 5 vs 20: short by 15, more than 10: it springs.
        (
            "disarm_springs",
            check(
                disarm(2, Some(3), 20, false) == Outcome::Sprung,
                "3+2 vs 20".into(),
            ),
        ),
        // d20 9 + 2 = 11 vs 20: short by 9: just failed.
        (
            "disarm_fails",
            check(
                disarm(2, Some(9), 20, false) == Outcome::Failed,
                "9+2 vs 20".into(),
            ),
        ),
        // A DC past 35 never yields, whatever the roll.
        (
            "disarm_hopeless",
            check(
                disarm(40, None, 36, false) == Outcome::Failed,
                "20+40 vs 36".into(),
            ),
        ),
        // The party's own mine yields to it.
        (
            "disarm_own",
            check(disarm(-5, Some(1), 60, true) == Outcome::Done, "own".into()),
        ),
        // DC 0 counts as 1.
        (
            "disarm_dc_zero",
            check(
                disarm(-19, None, 0, false) == Outcome::Done,
                "20-19 vs 1".into(),
            ),
        ),
        // Recovering asks 10 more: 25 vs 30 is short by 5 (no spring), 24
        // vs 30 by 6 (it springs).
        (
            "recover_fails",
            check(
                recover(5, None, 20, false) == Outcome::Failed,
                "25 vs 30".into(),
            ),
        ),
        (
            "recover_springs",
            check(
                recover(4, None, 20, false) == Outcome::Sprung,
                "24 vs 30".into(),
            ),
        ),
        (
            "recover_done",
            check(
                recover(10, None, 20, false) == Outcome::Done,
                "30 vs 30".into(),
            ),
        ),
        // Laying out of combat: 20 + 3 = 23 vs 15; the mine's DCs are its
        // row's mods plus the sum.
        (
            "lay_dcs",
            check(
                laid == (
                    23,
                    Some(Laid {
                        detect_dc: 43,
                        disarm_dc: 43,
                    }),
                ),
                format!("{laid:?}"),
            ),
        ),
        // Five base ranks add 2.
        (
            "lay_five_ranks",
            check(ranked.0 == 25, format!("{ranked:?}")),
        ),
        // In combat a d20 1 - 10 = -9 vs 15, short by 24: KOTOR lays it all
        // the same (Q-0142); short by 10 or less lays nothing.
        ("lay_quirk", check(quirk.1.is_some(), format!("{quirk:?}"))),
        (
            "lay_fails",
            check(failed.1.is_none(), format!("{failed:?}")),
        ),
        // Springing: hostile (10) yes; 11 no; the same faction no; immune
        // no.
        (
            "springs_hostile",
            check(springs(10, false, false), "10".into()),
        ),
        (
            "springs_not_neutral",
            check(!springs(11, false, false), "11".into()),
        ),
        (
            "springs_not_own",
            check(!springs(0, true, false), "same faction".into()),
        ),
        (
            "springs_not_immune",
            check(!springs(0, false, true), "immune".into()),
        ),
        // The best kit: the deadliest tier, then the later row.
        ("best_kit", check(best == Some(4), format!("{best:?}"))),
        (
            "laid_template",
            check(
                laid_row(&laid_template(7)) == Some(7) && laid_row("g_t_trap004").is_none(),
                laid_template(7),
            ),
        ),
    ]
}

/// A mine laid in play is made from its row (`Level::template`): a
/// trigger with no outline of its own, the row's script, the row's trap;
/// a row the table lacks makes nothing.
fn facts() -> Vec<(&'static str, Result<String, String>)> {
    use crate::module::ObjectKind;
    let ground = crate::determinism::proving_ground();
    let Some(module) = ground.module() else {
        return vec![(
            "laid_template_facts",
            Err("the ground has no module".into()),
        )];
    };
    let mut data = (**module).clone();
    data.traps.resize(4, TrapRow::default());
    data.traps[3] = frag_minor();
    let level = ground.with_module(data);
    let made = level.template(ObjectKind::Trigger, &super::laid_template(3));
    let shaped = made.as_ref().is_some_and(|facts| {
        facts.kind == ObjectKind::Trigger
            && facts
                .trap
                .as_ref()
                .is_some_and(|trap| trap.row == 3 && trap.one_shot)
            && facts.geometry.is_empty()
            && facts.scripts == [("OnTrapTriggered".to_string(), "k_trp_generic".to_string())]
    });
    let unknown = level.template(ObjectKind::Trigger, &super::laid_template(9));
    vec![
        (
            "laid_template_facts",
            check(
                shaped,
                format!(
                    "{:?}",
                    made.map(|facts| (facts.trap.clone(), facts.scripts.clone()))
                ),
            ),
        ),
        (
            "laid_template_unknown_row",
            check(unknown.is_none(), "row 9 of 4".into()),
        ),
    ]
}

/// TSL's mines on doors, the rules: the exe's strength table, the blast's
/// sum, the kit a player sets.
fn door_mines() -> Vec<(&'static str, Result<String, String>)> {
    let strengths = [3, 4, 5, 12, 20, 26, 27].map(blast_strength);
    let sums = [blasts(0, 0, 1), blasts(7, 3, 26), blasts(-1, 0, 21)];
    let kits = [
        door_kit(0, 30, [3, 4, 5, 20]),
        door_kit(0, 1, [5, 3, 4]),
        door_kit(0, 40, [3, 4]),
        door_kit(0, 1, []),
    ];
    vec![
        (
            "door_mine_strength",
            check(
                strengths == [0, 3, 9, 0, 12, 12, 0],
                format!("{strengths:?}"),
            ),
        ),
        (
            "door_mine_blast",
            check(
                sums == [(20, true), (26, true), (20, false)],
                format!("{sums:?}"),
            ),
        ),
        (
            "door_mine_kit",
            check(
                kits == [Some(20), Some(3), Some(4), None],
                format!("{kits:?}"),
            ),
        ),
    ]
}

/// Every check (see the module documentation).
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let mut all = rules();
    all.extend(door_mines());
    all.extend(facts());
    all.extend(crate::world::traps::probes::springing());
    all.extend(crate::world::traps::probes::trap_routines());
    all.extend(crate::world::traps::probes::skill_uses());
    all.extend(crate::world::traps::probes::choices());
    all.extend(crate::world::traps::probes::laying());
    all.extend(crate::world::traps::probes::routines());
    all.extend(crate::world::traps::probes::races());
    all
}
