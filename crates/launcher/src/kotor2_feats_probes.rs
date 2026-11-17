//! KOTOR 2's feats in KOTOR 1 (`vfs::lent`, `kjk sim probes --only "kotor2
//! feats"`), on both games' data where this machine has them: the rows
//! KOTOR 1 gets with their names and icons, what each KOTOR 1 class may
//! pick and is granted, the Class Skill feats against KOTOR 1's skills, a
//! KOTOR 1 table a mod extended getting the rows after the mod's row,
//! KOTOR 1's own rows unchanged, a
//! character who missed a class grant catching up at its next level, and
//! each new feat rule with and without its feat on the rows both games
//! share.

use fmt_odyssey::tlk::LENT_FLAG;
use fmt_odyssey::twoda::TwoDa;
use sim::combat::damage::{self, Blow, FriendlyFire, Relation, Source};
use sim::combat::fighter::Location;
use sim::combat::passive::{self, Game};
use sim::combat::sheet::{Blaster, Delivery, Feats, SaberShape, Sheet, Weapon, feats};
use sim::levels::{Choices, Classes};
use sim::probe::{Checks, check};
use vfs::resolver::Resolver;

use crate::kotor2_powers_probes::{Case, Data, run};
use crate::modules::ResolverSource;

/// The rows KOTOR 2's `feat.2da` lends KOTOR 1: Evasion, Targeting I to
/// VIII, Close Combat and Improved Close Combat, Regenerate Force Points,
/// the seven Class Skill feats, the Dual Strike line, the two Finesse
/// feats, Regenerate Vitality Points, Stealth Run, Unarmed Specialist I to
/// VIII and Precise Shot I to V.
const LENT: [u16; 39] = [
    125, 126, 127, 128, 129, 130, 131, 132, 133, 139, 140, 143, 183, 184, 185, 186, 187, 188, 189,
    190, 191, 192, 193, 194, 196, 197, 212, 213, 214, 215, 216, 217, 218, 219, 240, 241, 242, 243,
    244,
];

/// KOTOR 2's Class Skill feats by the KOTOR 1 skill each makes a class
/// skill (`levels::CLASS_SKILL_FEATS`, which has none for Persuade).
fn class_skill_feats() -> Vec<(usize, u16)> {
    sim::levels::CLASS_SKILL_FEATS
        .iter()
        .enumerate()
        .filter_map(|(skill, feat)| Some((skill, (*feat)?)))
        .collect()
}

const CASES: [(&str, Case); 18] = [
    ("rows", rows),
    ("names and icons", names),
    ("class picks and grants", class_lists),
    ("class skill feats", class_skills),
    ("feat chart", chart),
    ("KOTOR 1 stays KOTOR 1", unchanged),
    ("a mod's row 125", modded),
    ("missed grants catch up", catching_up),
    ("targeting", targeting),
    ("finesse", finesse),
    ("precise shot", precise_shot),
    ("close combat", close_combat),
    ("dual strike", dual_strike),
    ("dual strike in a scene", dual_strike_scene),
    ("unarmed specialist", unarmed_specialist),
    ("evasion", evasion),
    ("regenerate force points", regenerate_force_points),
    ("regenerate vitality points", regenerate_vitality_points),
];

pub fn probes() -> Checks {
    run(&CASES)
}

fn feat_table(resolver: &Resolver) -> Result<TwoDa, String> {
    resolver.two_da("feat").ok_or_else(|| "no feat.2da".into())
}

/// KOTOR 1 gets exactly `LENT`.
fn rows(data: &Data) -> Result<String, String> {
    let rows = data.lent.lent.as_ref().map(|lent| lent.feat_rows());
    let refused = data.lent.lent.as_ref().map(|lent| lent.refused.clone());
    check(
        rows.as_deref() == Some(&LENT[..]),
        format!("feat.2da rows {rows:?}; refused {refused:?}"),
    )
}

/// Each row's name and description read KOTOR 2's lines through flagged
/// strrefs, and its icon is a texture KOTOR 1's data serves.
fn names(data: &Data) -> Result<String, String> {
    let (Some(k1), Some(tsl)) = (data.lent.talk.get(), data.tsl.talk.get()) else {
        return Err("a talk table does not read".into());
    };
    let (lent, theirs) = (feat_table(&data.lent)?, feat_table(&data.tsl)?);
    let mut wrong = Vec::new();
    let mut shown = Vec::new();
    for row in LENT.map(usize::from) {
        for column in ["name", "description"] {
            let (here, there) = (lent.int(row, column), theirs.int(row, column));
            let (Some(here), Some(there)) = (here, there) else {
                wrong.push(format!("{row} {column}: no strref"));
                continue;
            };
            let (here, there) = (here as u32, there as u32);
            let text = k1.text(here);
            if here != (there | LENT_FLAG) || text.is_none() || text != tsl.text(there) {
                wrong.push(format!("{row} {column}"));
            } else if column == "name" {
                shown.push(String::from_utf8_lossy(text.unwrap_or(&[])).into_owned());
            }
        }
        let icon = lent.resref(row, "icon");
        if icon.as_ref().is_none_or(|icon| data.lent.texture(icon).is_none()) {
            wrong.push(format!("{row} icon {icon:?}"));
        }
    }
    check(wrong.is_empty(), format!("wrong: {wrong:?}; names: {shown:?}"))
}

/// KOTOR 1's classes with their `feat.2da` codes (`featstable`, lowercase),
/// by row.
fn codes(data: &Data) -> Result<Vec<(usize, String)>, String> {
    let classes = data.lent.two_da("classes").ok_or("no classes.2da")?;
    Ok((0..classes.cells.len())
        .filter_map(|row| Some((row, classes.text(row, "featstable")?.to_ascii_lowercase())))
        .collect())
}

/// Each KOTOR 1 class may pick and is granted the lent rows KOTOR 2's
/// columns of the same class give it: a pick for a `_list` of 0 to 2, a
/// grant at `_granted` for a `_list` of 3. The Class Skill feats are
/// `class_skills`' case.
fn class_lists(data: &Data) -> Result<String, String> {
    let classes = session::classes::classes(&ResolverSource(&data.lent));
    let tsl = feat_table(&data.tsl)?;
    let skill_feats: Vec<u16> = class_skill_feats().iter().map(|(_, feat)| *feat).collect();
    let mut wrong = Vec::new();
    let mut lines = Vec::new();
    for (row, code) in codes(data)? {
        let class = classes.classes.get(row).ok_or("a class is missing")?;
        let mut picks = Vec::new();
        let mut grants = Vec::new();
        for feat in LENT.into_iter().filter(|feat| !skill_feats.contains(feat)) {
            let list = tsl.int(usize::from(feat), &format!("{code}_list"));
            let granted = tsl.int(usize::from(feat), &format!("{code}_granted"));
            let picked = list.is_some_and(|list| (0..=2).contains(&list));
            let grant = granted.filter(|level| *level > 0 && list == Some(3));
            if picked != class.feats.contains(&feat) {
                wrong.push(format!("{code} {feat} pick"));
            }
            let held = class.granted.iter().find(|(_, of)| *of == feat);
            if grant != held.map(|(level, _)| i64::from(*level)) {
                wrong.push(format!("{code} {feat} grant"));
            }
            if picked {
                picks.push(feat);
            }
            if let Some(level) = grant {
                grants.push(format!("{feat}@{level}"));
            }
        }
        lines.push(format!("{code} picks {picks:?} grants {grants:?}"));
    }
    check(wrong.is_empty(), format!("wrong: {wrong:?}; {}", lines.join("; ")))
}

/// Each KOTOR 1 class may pick a Class Skill feat exactly when its skill is
/// cross-class for the class in KOTOR 1's `skills.2da`, and the feat then
/// makes the skill a class skill (`levels::class_skill`).
fn class_skills(data: &Data) -> Result<String, String> {
    let classes = session::classes::classes(&ResolverSource(&data.lent));
    let skills = data.lent.two_da("skills").ok_or("no skills.2da")?;
    let mut wrong = Vec::new();
    let mut lines = Vec::new();
    for (row, code) in codes(data)? {
        let class = classes.classes.get(row).ok_or("a class is missing")?;
        let mut offered = Vec::new();
        for (skill, feat) in class_skill_feats() {
            let cross = skills.int(skill, &format!("{code}_class")) != Some(1);
            let picks = class.feats.contains(&feat);
            let made = sim::levels::class_skill(class, &[feat], skill);
            if picks != cross || !made {
                wrong.push(format!("{code} {feat}"));
            }
            if picks {
                offered.push(feat);
            }
        }
        lines.push(format!("{code} {offered:?}"));
    }
    check(wrong.is_empty(), format!("wrong: {wrong:?}; {}", lines.join("; ")))
}

/// The level-up feat chart of each KOTOR 1 class holds every lent feat the
/// class may pick, and each lent grant once the character knows it, in
/// chains by KOTOR 2's prerequisites (Targeting I, II and III a row).
fn chart(data: &Data) -> Result<String, String> {
    let classes = session::classes::classes(&ResolverSource(&data.lent));
    let mut missing = Vec::new();
    let mut scout = Vec::new();
    for (row, code) in codes(data)? {
        let class = classes.classes.get(row).ok_or("a class is missing")?;
        let granted: Vec<u16> = class
            .granted
            .iter()
            .map(|(_, feat)| *feat)
            .filter(|feat| LENT.contains(feat))
            .collect();
        let chart = classes.chart(class, &granted);
        let shown: Vec<u16> = chart.iter().flatten().flatten().copied().collect();
        let wanted = class.feats.iter().chain(&granted).filter(|feat| LENT.contains(feat));
        missing.extend(wanted.filter(|feat| !shown.contains(feat)).map(|feat| format!("{code} {feat}")));
        if code == "sct" {
            scout = chart
                .into_iter()
                .filter(|chain| chain.iter().flatten().any(|feat| LENT.contains(feat)))
                .collect();
        }
    }
    let targeting = [Some(feats::TARGETING[0]), Some(feats::TARGETING[1]), Some(feats::TARGETING[2])];
    check(
        missing.is_empty() && scout.contains(&targeting),
        format!("missing: {missing:?}; the Scout's chains of lent feats: {scout:?}"),
    )
}

/// KOTOR 1's own 125 rows and its columns are as without the loan (no
/// `_pc_granted` or companion column comes with KOTOR 2's rows), it has no
/// `itemcreate.2da`, and its level rules stay KOTOR 1's.
fn unchanged(data: &Data) -> Result<String, String> {
    let (lent, plain) = (feat_table(&data.lent)?, feat_table(&data.plain)?);
    let own = plain.cells.len();
    let rows_kept = lent.cells[..own] == plain.cells[..] && lent.columns == plain.columns;
    let own_rows = own == usize::from(feats::K1_FEAT_ROWS);
    let not_tsl = data
        .lent
        .find("itemcreate", fmt_odyssey::ResType::TWO_DA)
        .is_none();
    let classes = session::classes::classes(&ResolverSource(&data.lent));
    let k1_rules = !classes.whole_skill_points && !classes.granted_powers;
    let lent_rows = classes.lent_feats == LENT;
    check(
        rows_kept && own_rows && not_tsl && k1_rules && lent_rows,
        format!(
            "{own} own rows kept {rows_kept}, no itemcreate.2da {not_tsl}, KOTOR 1's level rules \
             {k1_rules}, the classes list the lent rows {lent_rows}"
        ),
    )
}

/// A KOTOR 1 table with a row of a mod's own at 125 still gets KOTOR 2's
/// feats, placed after the mod's row, each at its KOTOR 2 number as its
/// canonical number. The resolver serves the mod's row past KOTOR 2's own
/// 245 rows (`rows by label`), and KOTOR 2's powers are lent too.
fn modded(data: &Data) -> Result<String, String> {
    let (lent, rows, after) =
        crate::kotor2_powers_probes::lent_with_mods_row(data, "feat", b"A_MODS_OWN_FEAT")?;
    let lent = lent?;
    let unmodded = data.lent.lent.as_ref().map(|lent| lent.feat_rows()).unwrap_or_default();
    let numbers: Vec<u16> = lent.feat_numbers.values().copied().collect();
    let placed_after = lent.feat_rows().first().is_some_and(|&row| usize::from(row) >= rows);
    let served = usize::from(rules::rows::Table::Feats.kotor2_rows()) + 1;
    check(
        !numbers.is_empty()
            && numbers == unmodded
            && !lent.rows().is_empty()
            && lent.refused.is_empty()
            && placed_after
            && after == Some(served),
        format!(
            "{} powers lent; feats' KOTOR 2 numbers {numbers:?} at install rows {:?}; refused {:?}; \
             {after:?} rows served, {rows} in the install",
            lent.rows().len(),
            lent.feat_rows(),
            lent.refused
        ),
    )
}

/// The lent feats a character of `class` at `level` knowing none of them
/// gains with its next level in that class, picking nothing.
fn gained(classes: &Classes, class: u8, level: u8) -> Result<Vec<u16>, String> {
    let mut sheet = Sheet::dev_soldier();
    sheet.level = level;
    sheet.feats = Feats::default();
    let career = if level == 0 {
        Vec::new()
    } else {
        vec![(class, level)]
    };
    let ability = (sheet.level + 1).is_multiple_of(rules::progression::levels::ABILITY_EVERY);
    let choices = Choices {
        class,
        ability: ability.then_some(0),
        ..Choices::default()
    };
    let (grown, ..) = classes
        .taken(&sheet, &career, &choices, 0)
        .map_err(|why| format!("{why:?}"))?;
    Ok(LENT.into_iter().filter(|feat| grown.feats.has(*feat)).collect())
}

/// A KOTOR 1 Scout made at level 1 is granted Targeting I and Close Combat.
/// A level 8 Scout who never got the lent grants (made before the loan, or
/// from a template) gets every grant it missed with its level 9: Evasion,
/// Targeting I to III, Close Combat and Precise Shot I and II. A level 5
/// Jedi Consular gets Unarmed Specialist I and II with its level 6. KOTOR
/// 2 grants only the level's own feats (Targeting III, Unarmed Specialist
/// II).
fn catching_up(data: &Data) -> Result<String, String> {
    const SCOUT: u8 = 1;
    const CONSULAR: u8 = 4;
    let k1 = session::classes::classes(&ResolverSource(&data.lent));
    let tsl = session::classes::classes(&ResolverSource(&data.tsl));
    let cases: [(&str, &Classes, u8, u8, &[u16]); 5] = [
        ("K1 Scout made", &k1, SCOUT, 0, &[126, 139]),
        ("K1 Scout 8 to 9", &k1, SCOUT, 8, &[125, 126, 127, 128, 139, 240, 241]),
        ("TSL Scout 8 to 9", &tsl, SCOUT, 8, &[128]),
        ("K1 Consular 5 to 6", &k1, CONSULAR, 5, &[212, 213]),
        ("TSL Consular 5 to 6", &tsl, CONSULAR, 5, &[213]),
    ];
    let mut good = true;
    let mut lines = Vec::new();
    for (name, classes, class, level, wanted) in cases {
        let got = gained(classes, class, level)?;
        good &= got == wanted;
        lines.push(format!("{name}: {got:?}"));
    }
    check(good, lines.join("; "))
}

/// The labels `rows` have in KOTOR 1's lent table, which must be KOTOR 2's
/// at the same rows: a rule reads the same rows in both games.
fn labels(data: &Data, rows: &[u16]) -> Result<String, String> {
    let (lent, tsl) = (feat_table(&data.lent)?, feat_table(&data.tsl)?);
    let mut labels = Vec::new();
    for &row in rows {
        let (here, there) = (
            lent.text(usize::from(row), "label"),
            tsl.text(usize::from(row), "label"),
        );
        match here {
            Some(label) if Some(&label) == there.as_ref() => labels.push(label),
            _ => return Err(format!("row {row}: {here:?} in KOTOR 1, {there:?} in KOTOR 2")),
        }
    }
    Ok(labels.join(" "))
}

/// A rule's case: `rows` labelled alike in both games, and what `measure`
/// gives knowing none of them, then each alone (a line's highest tier
/// counts), against `wanted`, what each adds.
fn tiers(
    data: &Data,
    rows: &[u16],
    wanted: &[i64],
    measure: &dyn Fn(&Feats) -> i64,
) -> Result<String, String> {
    let labels = labels(data, rows)?;
    let none = measure(&Feats::default());
    let added: Vec<i64> = rows
        .iter()
        .map(|&row| measure(&Feats::default().with(row)) - none)
        .collect();
    check(
        added == wanted,
        format!("{labels}: {none} without, {added:?} more with each"),
    )
}

/// A sheet knowing `known`, Strength 10 and Dexterity 18.
fn nimble(known: &Feats) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.abilities = [10, 18, 14, 10, 12, 10];
    sheet.feats = *known;
    sheet
}

fn weapon(delivery: Delivery) -> Weapon {
    Weapon {
        delivery,
        dice: 1,
        die: 6,
        ..Weapon::UNARMED
    }
}

const BLASTER: Delivery = Delivery::Blaster(Blaster::Pistol);

fn widened<T: Into<i64> + Copy>(values: &[T]) -> Vec<i64> {
    values.iter().map(|value| (*value).into()).collect()
}

/// Targeting I to VIII add their bonus to the attack bonus with a blaster.
fn targeting(data: &Data) -> Result<String, String> {
    tiers(
        data,
        &feats::TARGETING,
        &widened(&rules::feats::targeting::ATTACK),
        &|known| i64::from(damage::attack_bonus(&nimble(known), &weapon(BLASTER))),
    )
}

/// Finesse: Lightsabers with a lightsaber and Finesse: Melee Weapons with a
/// vibroblade hit by Dexterity's +4 instead of Strength's +0. Fists stay
/// on Strength.
fn finesse(data: &Data) -> Result<String, String> {
    let dex_over_str = i64::from(nimble(&Feats::default()).modifier(sim::combat::sheet::Ability::Dexterity));
    let cases = [
        (feats::FINESSE_LIGHTSABERS, Delivery::Saber(SaberShape::Single), dex_over_str),
        (feats::FINESSE_MELEE_WEAPONS, Delivery::Melee, dex_over_str),
        (feats::FINESSE_MELEE_WEAPONS, Delivery::Unarmed, 0),
    ];
    let results: Vec<Result<String, String>> = cases
        .into_iter()
        .map(|(feat, delivery, wanted)| {
            tiers(data, &[feat], &[wanted], &|known| {
                i64::from(damage::attack_bonus(&nimble(known), &weapon(delivery)))
            })
        })
        .collect();
    joined(results)
}

/// The cases' lines joined, failed if any failed.
fn joined(results: Vec<Result<String, String>>) -> Result<String, String> {
    let good = results.iter().all(Result::is_ok);
    let lines: Vec<String> = results
        .into_iter()
        .map(|result| result.unwrap_or_else(|why| format!("FAILED {why}")))
        .collect();
    check(good, lines.join("; "))
}

/// Precise Shot I to V add their damage to a blaster's least roll, and
/// take their penalty off the deflection bonus of whoever turns its bolts.
fn precise_shot(data: &Data) -> Result<String, String> {
    let penalties: Vec<i64> = rules::feats::precise_shot::DEFLECT_PENALTY
        .iter()
        .map(|penalty| -i64::from(*penalty))
        .collect();
    joined(vec![
        tiers(
            data,
            &feats::PRECISE_SHOT,
            &rules::feats::precise_shot::DAMAGE,
            &|known| damage::damage_range(&nimble(known), &weapon(BLASTER)).0,
        ),
        tiers(data, &feats::PRECISE_SHOT, &penalties, &|known| {
            let penalty = passive::deflect_penalty(known, &weapon(BLASTER));
            i64::from(passive::deflect_after(0, penalty))
        }),
    ])
}

/// Close Combat and Improved Close Combat add their bonus to hit with a
/// blaster at a target within short range, and nothing past it.
fn close_combat(data: &Data) -> Result<String, String> {
    let short = rules::feats::close_combat::SHORT_RANGE_M;
    let at = |distance_m: f32| {
        move |known: &Feats| i64::from(passive::close_combat(known, &weapon(BLASTER), distance_m))
    };
    joined(vec![
        tiers(
            data,
            &feats::CLOSE_COMBAT,
            &widened(&rules::feats::close_combat::ATTACK),
            &at(short / 2.0),
        ),
        tiers(data, &feats::CLOSE_COMBAT, &[0, 0], &at(short * 2.0)),
    ])
}

/// The damage `aim` more to hit makes of one blaster hit on a soldier
/// (`Blow::aim`), in hundredths, on one seed.
fn hit_with(aim: i32) -> i32 {
    let attacker = nimble(&Feats::default());
    let target = Sheet::dev_soldier();
    let blow = Blow {
        attacker: None,
        relation: Relation::Hostile,
        source: Source::Weapon {
            weapon: weapon(BLASTER),
            cadence_ms: 1000,
            share: 1000,
            area: false,
            second: None,
        },
        location: Location::None,
        direction: None,
        sneak: 0,
        aim,
    };
    let mut rng = sim::SimRng::new(1, 0);
    damage::assess(&blow, Some(&attacker), &target, FriendlyFire::Off, Game::K1, &mut rng).damage
}

/// Dual Strike's three tiers add their bonus to hit a target another of
/// the side attacks, and that bonus sizes a hit as KOTOR's better chance
/// to hit would.
fn dual_strike(data: &Data) -> Result<String, String> {
    let master = passive::dual_strike(&Feats::default().with(feats::DUAL_STRIKE[2]));
    let (plain, teamed) = (hit_with(0), hit_with(master));
    joined(vec![
        tiers(
            data,
            &feats::DUAL_STRIKE,
            &widened(&rules::feats::dual_strike::ATTACK),
            &|known| i64::from(passive::dual_strike(known)),
        ),
        check(
            teamed > plain,
            format!("a hit takes {plain} hundredths, {teamed} with Master Dual Strike's aim"),
        ),
    ])
}

/// Unarmed Specialist I to VIII add their dice to the most a fist deals.
fn unarmed_specialist(data: &Data) -> Result<String, String> {
    let die = i64::from(rules::feats::unarmed_specialist::DIE);
    let wanted: Vec<i64> = rules::feats::unarmed_specialist::DICE
        .iter()
        .map(|dice| i64::from(*dice) * die)
        .collect();
    tiers(data, &feats::UNARMED_SPECIALIST, &wanted, &|known| {
        damage::damage_range(&nimble(known), &Weapon::UNARMED).1
    })
}

/// A fragmentation grenade's blast on a target that makes its Reflex save:
/// half without Evasion and nothing with it, in both games.
fn evasion(data: &Data) -> Result<String, String> {
    let blast = |known: &Feats, game: Game| {
        let mut target = Sheet::dev_soldier();
        target.saves[1] = 100;
        target.feats = *known;
        let blow = Blow {
            attacker: None,
            relation: Relation::Hostile,
            source: Source::Blast {
                row: sim::combat::spells::ITEM_ABILITY_GRENADE_FRAGMENTATION,
            },
            location: Location::None,
            direction: None,
            sneak: 0,
            aim: 0,
        };
        let mut rng = sim::SimRng::new(1, 0);
        let assessed = damage::assess(&blow, None, &target, FriendlyFire::Off, game, &mut rng);
        (assessed.saved, assessed.damage)
    };
    let labels = labels(data, &[feats::EVASION])?;
    let evading = Feats::default().with(feats::EVASION);
    let mut good = true;
    let mut lines = Vec::new();
    for game in [Game::K1, Game::Tsl] {
        let (without, with) = (blast(&Feats::default(), game), blast(&evading, game));
        good &= without.0 && with.0 && without.1 > 0 && with.1 == 0;
        lines.push(format!("{game:?}: saved for {without:?} without, {with:?} with"));
    }
    check(good, format!("{labels}: {}", lines.join("; ")))
}

/// KOTOR 1: Regenerate Force Points shortens the time a point takes, by
/// KOTOR 2's share of the pool every `force.k1_regeneration.force_time_s`.
/// KOTOR 2: its own rule adds its rate to a second of regeneration.
fn regenerate_force_points(data: &Data) -> Result<String, String> {
    use sim::combat::force;
    let labels = labels(data, &[feats::REGENERATE_FORCE_POINTS])?;
    let with = Feats::default().with(feats::REGENERATE_FORCE_POINTS);
    const POOL: u16 = 60;
    let (k1_without, k1_with) = (
        force::regen_every(&Feats::default(), POOL),
        force::regen_every(&with, POOL),
    );
    let per_s = |every: u64| 1000.0 / every as f32 / f32::from(POOL);
    let added = per_s(k1_with) - per_s(k1_without);
    let wanted =
        force::REGENERATE_FEAT_RATE / rules::force::k1_regeneration::FORCE_TIME_S;
    let k1_good = k1_with < k1_without && (added - wanted).abs() < wanted / 20.0;
    let table = session::regeneration(&ResolverSource(&data.tsl))
        .ok_or("KOTOR 2 has no regeneration table")?
        .fp;
    let second = |known: &Feats| {
        let mut sheet = Sheet::dev_jedi();
        sheet.feats = *known;
        let mut state = force::ForceState::default();
        let mut fp = 0;
        force::regenerate_tsl(&mut state, &mut fp, &sheet, table, (false, 0, 0, 0), 1000);
        fp
    };
    let (tsl_without, tsl_with) = (second(&Feats::default()), second(&with));
    check(
        k1_good && tsl_with > tsl_without,
        format!(
            "{labels}: KOTOR 1 a point every {k1_without} ms, {k1_with} ms with it \
             ({added:.5} pools a second more, {wanted:.5} wanted); KOTOR 2 a second gains \
             {tsl_without} hundredths, {tsl_with} with it"
        ),
    )
}

/// KOTOR 1: a party member who knows Regenerate Vitality Points heals a
/// point every `force.k1_regeneration.healing_time_s` over
/// `force.tsl_regeneration.healing_feat_rate`, where KOTOR 1's own healing
/// gives one every 10,000 s. KOTOR 2's own rule is the `regen` group's.
fn regenerate_vitality_points(data: &Data) -> Result<String, String> {
    let labels = labels(data, &[feats::REGENERATE_VITALITY_POINTS])?;
    let rates = session::health_regeneration(&ResolverSource(&data.lent))
        .ok_or("KOTOR 1 has no regeneration table")?;
    let wanted = (1000.0 * rules::force::k1_regeneration::HEALING_TIME_S
        / rules::force::tsl_regeneration::HEALING_FEAT_RATE) as u64;
    let lines: Vec<(bool, u64, u64)> = [false, true]
        .into_iter()
        .map(|fighting| (fighting, rates.every_ms(50, fighting, false), rates.every_ms(50, fighting, true)))
        .collect();
    check(
        lines.iter().all(|(_, without, with)| *with == wanted && with < without),
        format!("{labels}: (fighting, ms a point without, with) {lines:?}; {wanted} ms wanted"),
    )
}

/// The damage seat 0's own blaster hits did to the Endar Spire's Sith
/// soldier in `ms` of holding the trigger, in hundredths: seat 0 a soldier
/// knowing `known`, and, when `teamed`, a second creature of the module
/// made a living party member whose target is the soldier.
fn shot_damage(stage: &crate::scenes::Stage, target: usize, known: &Feats, teamed: bool) -> Result<i32, String> {
    use sim::combat::{Life, sheet::factions};
    const MS: u64 = 2500;
    let mut shooter = Sheet::dev_soldier();
    for feat in known.rows() {
        shooter.feats.insert(feat);
    }
    let mut start = stage.scene(target, 160.0, shooter).ok_or("no scene")?;
    let me = crate::scenes::seat0(&start);
    let target_id = start.entities[target].id;
    let ally = (0..start.entities.len())
        .find(|&at| {
            at != target
                && at != me
                && matches!(start.entities[at].kind, sim::Kind::Object(sim::module::ObjectKind::Creature, _))
                && start.entities[at].fighter.is_some()
        })
        .ok_or("no second creature")?;
    if teamed {
        // A soldier of the party in the creature's place.
        let entity = &mut start.entities[ally];
        let mut sheet = Sheet::dev_soldier();
        sheet.faction = factions::PLAYER;
        entity.sheet = Some(sheet);
        if let Some(player) = entity.player.as_mut() {
            player.flags &= !sim::motion::motion_flags::DEAD;
        }
        if let Some(fighter) = entity.fighter.as_mut() {
            fighter.life = Life::Alive;
            fighter.hp = i32::from(sheet.max_hp) * 100;
            fighter.mind.leader = Some(sim::Seat(0));
            fighter.target = Some(target_id);
        }
    }
    let run = stage.run(&start, target, MS, &mut crate::scenes::hold(sim::buttons::ATTACK, MS));
    if !run.replayed {
        return Err("the scene did not replay".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0;
    for snapshot in &run.snapshots {
        let fighter = crate::scenes::fighter(snapshot, target);
        for event in fighter.events.iter().flatten() {
            if event.by == Some(start.entities[me].id) && seen.insert((event.sequence, event.tick)) {
                total += event.damage;
            }
        }
    }
    Ok(total)
}

/// In KOTOR 1's Endar Spire a soldier shooting a Sith soldier deals more
/// with Master Dual Strike while a second living member of its side attacks
/// the same soldier, and exactly the same with or without the feat when
/// nobody else does.
fn dual_strike_scene(_: &Data) -> Result<String, String> {
    let hostile = ["sithsoldier".to_string()];
    let stage = crate::scenes::Stage::new(&kjk_game::discover(), "k1", "end_m01aa", &hostile)?;
    let target = stage.creature("sithsoldier").ok_or("no sithsoldier")?;
    let master = Feats::default().with(feats::DUAL_STRIKE[2]);
    let none = Feats::default();
    let (teamed_without, teamed_with) = (
        shot_damage(&stage, target, &none, true)?,
        shot_damage(&stage, target, &master, true)?,
    );
    let (alone_without, alone_with) = (
        shot_damage(&stage, target, &none, false)?,
        shot_damage(&stage, target, &master, false)?,
    );
    check(
        teamed_with > teamed_without && teamed_without > 0 && alone_with == alone_without && alone_with > 0,
        format!(
            "with an ally attacking: {teamed_without} hundredths without the feat, {teamed_with} with it; \
             alone: {alone_without} without, {alone_with} with"
        ),
    )
}
