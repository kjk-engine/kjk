//! `kjk combat feats <game> <module>`: passive feats on the game's own
//! characters and items (`sim::combat::passive`), read through a module's
//! plan (its library: the party's templates, the item catalog, the
//! classes):
//!
//! 1. each party template's most HP is its hit points, CON's for each
//!    level and Toughness's for each level (the templates hold it already);
//! 2. its saves, defence, skills and to hit in each hand as the feats
//!    leave them, a pair of the game's blaster pistols in its hands;
//! 3. which of the game's armour, robes, implants and weapons it may equip;
//! 4. a level taken: the module's creature `trask` (if there) by the
//!    game's picks, a Soldier picking Toughness at level 2.

use std::process::ExitCode;

use serde_json::json;
use sim::combat::passive;
use sim::combat::sheet::{Ability, Sheet, feats};
use sim::levels::Choices;
use vfs::Discovery;

use crate::runtime::Report;

/// K1's party templates.
const PARTY: [&str; 9] = [
    "p_bastilla",
    "p_cand",
    "p_carth",
    "p_hk47",
    "p_jolee",
    "p_juhani",
    "p_mission",
    "p_t3m4",
    "p_zaalbar",
];

/// Base items whose first template in the catalog is tried on each: light,
/// medium and heavy armour, a Jedi robe, implants 1 to 3, a lightsaber, a
/// blaster rifle, a heavy repeating blaster.
const TRIED: [(u16, &str); 10] = [
    (38, "light armour"),
    (40, "medium armour"),
    (42, "heavy armour"),
    (35, "Jedi robe"),
    (48, "implant 1"),
    (49, "implant 2"),
    (50, "implant 3"),
    (8, "lightsaber"),
    (77, "blaster rifle"),
    (24, "heavy repeater"),
];
const PISTOL: u16 = 12;

pub fn feats(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let installer = crate::scenes::installer(discovery, game)?;
        let plan = installer.prepare_one(module, None)?.plan;
        let library = plan.level.scripts().ok_or("no script library")?;
        let catalog = library.items();
        let first = |base: u16| catalog.iter().find(|facts| facts.base_item == base);
        let pistol = first(PISTOL)
            .and_then(|facts| facts.weapon)
            .ok_or("no blaster pistol")?;
        let mut rows = Vec::new();
        for template in PARTY {
            let Some(data) = library.creature(template) else {
                continue;
            };
            let (Some(sheet), Some((_, base))) = (data.sheet, data.hit_points) else {
                continue;
            };
            let level = i32::from(sheet.level);
            let tough = i32::from(sheet.feats.count(&feats::TOUGHNESS));
            let made = base + sheet.modifier(Ability::Constitution) * level + tough * level;
            report.check(
                made == i32::from(sheet.max_hp),
                format!(
                    "{template}: level {level}, HP {} = hit points {base} + CON {} a level + \
                     Toughness {tough} a level",
                    sheet.max_hp,
                    sheet.modifier(Ability::Constitution)
                ),
            );
            let armed = Sheet {
                weapons: [Some(pistol), Some(pistol)],
                ..sheet
            };
            let aim = |sheet: &Sheet, hand: u8| {
                sim::combat::damage::attack_bonus(sheet, &pistol)
                    + passive::hand_attack(sheet, hand)
            };
            let single = Sheet {
                weapons: [Some(pistol), None],
                ..sheet
            };
            let may: Vec<&str> = TRIED
                .iter()
                .filter(|(base, _)| {
                    first(*base).is_some_and(|facts| passive::may_equip(&sheet.feats, &facts.needs))
                })
                .map(|(_, name)| *name)
                .collect();
            let saves = [0, 1, 2].map(|index| sheet.save(index));
            let skills: Vec<i32> = (0..8).map(|skill| sheet.skill(skill)).collect();
            let line = json!({
                "template": template,
                "level": sheet.level,
                "max_hp": sheet.max_hp,
                "saves": saves,
                "skills": skills,
                "ranks": sheet.skills,
                "defence_one_pistol": single.defence(),
                "to_hit_one_pistol": aim(&single, 0),
                "to_hit_two_pistols": [aim(&armed, 0), aim(&armed, 1)],
                "two_weapon_tier": sheet.feats.count(&feats::TWO_WEAPON_FIGHTING),
                "dueling_tier": sheet.feats.count(&feats::DUELING),
                "may_equip": may,
            });
            outln!("  {line}");
            rows.push(line);
        }
        report.check(
            !rows.is_empty(),
            format!("{} party templates read", rows.len()),
        );

        let classes = library.classes();
        let data = plan.level.module().ok_or("no module")?;
        if let Some(trask) = data.objects().find(|object| object.tag == "end_trask")
            && let Some(sheet) = trask.sheet
        {
            let career = trask.classes.clone();
            let class = career.first().map_or(0, |(class, _)| *class);
            let taken = classes
                .auto(&sheet, &career, class, 0)
                .and_then(|choices| classes.take(&sheet, &career, &choices, 0))
                .map(|(after, ..)| after);
            let die = classes.classes.get(usize::from(class)).map_or(0, |c| c.hit_die);
            let gain = i32::from(die)
                + sheet.modifier(Ability::Constitution)
                + passive::hp_per_level(&sheet.feats);
            report.check(
                taken.is_some_and(|after| {
                    i32::from(after.max_hp) == i32::from(sheet.max_hp) + gain
                }),
                format!(
                    "Trask (Toughness {}) takes level {}: HP {} → {:?} (d{die} + CON + Toughness)",
                    sheet.feats.has(feats::TOUGHNESS[0]),
                    sheet.level + 1,
                    sheet.max_hp,
                    taken.map(|after| after.max_hp)
                ),
            );
        }
        let soldier = Sheet {
            level: 1,
            max_hp: 12,
            ..Sheet::dev_soldier()
        };
        let career = vec![(0, 1)];
        let pick = Choices {
            feats: vec![feats::TOUGHNESS[0]],
            ..classes.auto(&soldier, &career, 0, 0).ok_or("no level 2")?
        };
        let picked = classes.take(&soldier, &career, &pick, 0).map(|(after, ..)| after);
        let plain = classes
            .auto(&soldier, &career, 0, 0)
            .and_then(|choices| classes.take(&soldier, &career, &choices, 0))
            .map(|(after, ..)| after);
        report.check(
            picked.zip(plain).is_some_and(|(picked, plain)| {
                picked.max_hp == plain.max_hp + 2 && picked.feats.has(feats::TOUGHNESS[0])
            }),
            format!(
                "a level 1 Soldier (12 HP) taking level 2: {:?} HP with the game's picks, {:?} \
                 picking Toughness (this level's and level 1's)",
                plain.map(|sheet| sheet.max_hp),
                picked.map(|sheet| sheet.max_hp)
            ),
        );
        Ok(json!({ "module": module, "party": rows }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
