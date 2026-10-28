//! `kjk combat worked`: the worked fights on the Endar Spire
//! (`docs/COMBAT.md`) recomputed through the
//! implemented pipeline. The creatures are the modules' own, read by
//! `session::sheets` (scores, levels, items and their properties, natural
//! defence); the player is the design's assumed one, which is the dev
//! soldier (a level-1 Soldier, STR 16, DEX 14, CON 14, 12 HP, defence 12,
//! a blaster pistol at AB +3 and a vibroblade at +4). Each claim, a hit's
//! mean damage at the design's cadence (`damage::assess` over seeded rolls,
//! crits and all), is checked against the design's number, allowing 5% for
//! the crits it leaves out, within 15% (it rounds; blades crit on 19 and
//! 20). Trask's
//! `EffectAssuredHit` is a script effect and waits for I16. Writes
//! `facts.json`.

use std::process::ExitCode;

use sim::combat::Sheet;
use sim::combat::sheet::{Weapon, damage_types, feats};
use vfs::Discovery;

use crate::damage_verbs::mean_hit;
use crate::scenes::Report;

/// The rolls each mean is taken over.
const ROLLS: u64 = 4000;

/// The sheets of the first creatures tagged `tags` in `module`'s plan.
fn creatures(discovery: &Discovery, module: &str, tags: &[&str]) -> Result<Vec<Sheet>, String> {
    let installer = crate::scenes::installer(discovery, "k1")?;
    let plan = installer.prepare_one(module, None)?.plan;
    let data = plan.level.module().ok_or("no module")?;
    tags.iter()
        .map(|tag| {
            data.objects()
                .find(|object| object.tag.eq_ignore_ascii_case(tag))
                .and_then(|object| object.sheet)
                .ok_or(format!("no creature tagged {tag} with a sheet in {module}"))
        })
        .collect()
}

/// The dev saber the design's fight 3 grants: 2d8 energy, proficient (no
/// focus or specialization).
fn dev_saber() -> Weapon {
    Weapon {
        focus: None,
        specialization: None,
        ..Weapon::plain(
            8,
            2,
            8,
            (2, 2),
            damage_types::ENERGY,
            [feats::PROFICIENCY_LIGHTSABER, 0, 0],
        )
    }
}

/// `kjk combat worked`.
pub fn worked(discovery: &Discovery) -> ExitCode {
    let mut report = Report::default();
    outln!("combat worked: I22 section 11's numbers through the pipeline ({ROLLS} rolls a mean)");
    let player = Sheet::dev_soldier();
    let pistol = player.weapons[0].expect("the dev soldier's pistol");
    let vibroblade = player.weapons[1].expect("the dev soldier's vibroblade");
    let mut saber_player = player;
    saber_player.weapons[0] = Some(dev_saber());
    let read = creatures(
        discovery,
        "end_m01aa",
        &["end_sith2", "end_sith3", "sithsoldier", "end_darkjedi01"],
    )
    .and_then(|aa| Ok((aa, creatures(discovery, "end_m01ab", &["end_sith01"])?)));
    let (sith2, sith3, soldier, apprentice, trooper) = match read {
        Ok((aa, ab)) => (aa[0], aa[1], aa[2], aa[3], ab[0]),
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let main = |sheet: &Sheet| sheet.main_weapon();
    let mut facts = Vec::new();
    // (what, attacker, weapon, target, cadence (ms), the design's number)
    let claims: [(&str, &Sheet, Weapon, &Sheet, u32, f64); 8] = [
        (
            "fight 1: a Sith's one-damage rifle bolt on the player (p 0.05)",
            &sith2,
            main(&sith2),
            &player,
            350,
            0.02,
        ),
        (
            "fight 1: the player's pistol bolt on end_sith2 (p 0.9)",
            &player,
            pistol,
            &sith2,
            800,
            3.4,
        ),
        (
            "fight 2: a Sith soldier's one-damage blade on the player (p 0.25, medium 0.6 s)",
            &soldier,
            main(&soldier),
            &player,
            600,
            0.2,
        ),
        (
            "fight 2: the player's vibroblade on a Sith soldier (p 0.7)",
            &player,
            vibroblade,
            &soldier,
            600,
            4.8,
        ),
        (
            "fight 3: the player's dev saber on the Sith Apprentice (p 0.7)",
            &saber_player,
            dev_saber(),
            &apprentice,
            600,
            6.7,
        ),
        (
            "fight 3: the Apprentice's saber on the player (p 0.7)",
            &apprentice,
            main(&apprentice),
            &player,
            600,
            5.6,
        ),
        (
            "co-op: the player's pistol bolt on a Sith trooper (p 0.4)",
            &player,
            pistol,
            &trooper,
            800,
            1.5,
        ),
        (
            "co-op: a trooper's one-damage rifle bolt on the player (p 0.4)",
            &trooper,
            main(&trooper),
            &player,
            350,
            0.19,
        ),
    ];
    for (what, attacker, weapon, target, cadence, design) in claims {
        let (mean, _) = mean_hit(attacker, weapon, target, cadence, ROLLS);
        // A 5% allowance for crits (the design leaves them out), then 15%
        // either way (it rounds, and the blades crit on 19 and 20).
        let ratio = mean / (design * 1.05);
        report.check(
            (0.85..=1.15).contains(&ratio),
            format!("{what}: {mean:.3} HP a hit (the design: {design})"),
        );
        facts.push(serde_json::json!({"claim": what, "mean": mean, "design": design}));
    }
    // What the design's conclusions rest on: hits to kill.
    let (bolt, _) = mean_hit(&player, pistol, &sith2, 800, ROLLS);
    let (swing, _) = mean_hit(&saber_player, dev_saber(), &apprentice, 600, ROLLS);
    let (trooper_bolt, _) = mean_hit(&player, pistol, &trooper, 800, ROLLS);
    let to_kill = |hp: u16, hit: f64| (f64::from(hp) / hit).ceil();
    report.check(
        to_kill(sith3.max_hp, bolt) == 1.0
            && to_kill(sith2.max_hp, bolt) <= 5.0
            && to_kill(apprentice.max_hp, swing) == 2.0
            && (15.0..=18.0).contains(&to_kill(trooper.max_hp, trooper_bolt)),
        format!(
            "end_sith3 to {} bolt, end_sith2 to {}, the Apprentice to {} swings, a trooper to {} bolts (the design: 1, 5, 2, 17)",
            to_kill(sith3.max_hp, bolt),
            to_kill(sith2.max_hp, bolt),
            to_kill(apprentice.max_hp, swing),
            to_kill(trooper.max_hp, trooper_bolt)
        ),
    );
    report.finish(serde_json::json!({"verb": "combat worked", "claims": facts}))
}
