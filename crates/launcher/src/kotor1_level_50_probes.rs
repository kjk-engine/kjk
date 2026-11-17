//! KOTOR 1 past level 20 (`data/levels_k1.txt`, `kjk sim probes --only
//! "kotor1 level 50"`), on both games' data where this machine has them:
//! KOTOR 1's class tables to level 50 with its own rows for levels 1 to 20
//! kept, each KOTOR 1 class levelled from 20 to 21 and from 20 to 30, the
//! lent feats and the lent power that only a character past level 20 can
//! have, and KOTOR 2's tables as KOTOR 2 has them.

use sim::combat::sheet::{Sheet, feats};
use sim::combat::spells::{
    FORCE_POWER_IMPROVED_REVITALIZE, FORCE_POWER_MASTER_REVITALIZE, FORCE_POWER_REVITALIZE,
};
use sim::levels::{Career, Choices, Class, Classes, Refusal, Unpickable};
use sim::probe::{Checks, check};
use vfs::resolver::Resolver;

use crate::kotor2_powers_probes::{Case, Data, run};
use crate::modules::ResolverSource;

const CASES: [(&str, Case); 5] = [
    ("tables to level 50", tables),
    ("level 20 to 21 and to 30", levels),
    ("master revitalize", master_revitalize),
    ("kotor 1 rows kept", own_rows),
    ("kotor 2 stays kotor 2", kotor2_unchanged),
];

pub fn probes() -> Checks {
    run(&CASES)
}

/// The last level of KOTOR 1's own class tables.
const TWENTY: u8 = 20;

/// What a KOTOR 1 character of a class has at a level past 20, after
/// taking every level from 20 in that class and picking nothing: its base
/// attack bonus, its saves (Fortitude, Reflex, Will) and class Defense
/// bonus from the class's tables, the feats and powers its levels from 21
/// on let it pick, and the lent feats those levels grant.
#[derive(Debug, PartialEq)]
struct Reached {
    bab: u8,
    saves: [i8; 3],
    defence: i8,
    feats: u8,
    powers: u8,
    granted: Vec<u16>,
}

/// A KOTOR 1 class (`classes.2da` row) with what each of its levels adds
/// (vitality points with Constitution 10, Force points, skill points with
/// Intelligence 10), and what a character has at levels 21 and 30.
struct Wanted {
    class: u8,
    name: &'static str,
    hp: u16,
    fp: u16,
    skill_points: u8,
    at_21: Reached,
    at_30: Reached,
}

/// The Scout's Targeting VI, VII and VIII, granted at Scout levels 21, 25
/// and 29.
const TARGETING_PAST_20: [u16; 3] = [
    feats::TARGETING[5],
    feats::TARGETING[6],
    feats::TARGETING[7],
];
/// The Jedi classes' Unarmed Specialist VI, VII and VIII, granted at class
/// levels 22, 26 and 30.
const UNARMED_PAST_20: [u16; 3] = [
    feats::UNARMED_SPECIALIST[5],
    feats::UNARMED_SPECIALIST[6],
    feats::UNARMED_SPECIALIST[7],
];

/// Each KOTOR 1 class at levels 21 and 30, the numbers read by hand from
/// KOTOR 1's tables and `data/levels_k1.txt`. A class's base attack bonus is
/// `cls_atk_1` (+1 a level) or `cls_atk_2` (15 at 21, 22 at 30). Its saves
/// are its `cls_st_*` table's. The Defense bonus of the Jedi, the Scoundrel
/// and the droids (`acbonus` `scd`, `jdc`, `jds`, `jdg`) is +6 at 21 and +10
/// at 30. The feats are each class's `featgain` cycle from level 21 on, and
/// the powers its `classpowergain` (the Consular picks 2 at levels 21, 25
/// and 29).
fn wanted() -> [Wanted; 9] {
    let at = |bab, saves, defence, (feats, powers), granted: &[u16]| Reached {
        bab,
        saves,
        defence,
        feats,
        powers,
        granted: granted.to_vec(),
    };
    let class = |class, name, (hp, fp, skill_points), at_21, at_30| Wanted {
        class,
        name,
        hp,
        fp,
        skill_points,
        at_21,
        at_30,
    };
    [
        class(0, "Soldier", (10, 0, 1), at(21, [12, 7, 7], 0, (0, 0), &[]), at(30, [17, 10, 10], 0, (5, 0), &[])),
        class(
            1,
            "Scout",
            (8, 0, 3),
            at(15, [12, 12, 12], 0, (1, 0), &TARGETING_PAST_20[..1]),
            at(22, [17, 17, 17], 0, (5, 0), &TARGETING_PAST_20),
        ),
        class(2, "Scoundrel", (6, 0, 4), at(15, [7, 12, 7], 6, (0, 0), &[]), at(22, [10, 17, 10], 10, (3, 0), &[])),
        class(
            3,
            "Jedi Guardian",
            (10, 4, 1),
            at(21, [12, 12, 9], 6, (1, 1), &[]),
            at(30, [17, 17, 13], 10, (5, 10), &UNARMED_PAST_20),
        ),
        class(
            4,
            "Jedi Consular",
            (6, 8, 1),
            at(15, [12, 9, 12], 6, (1, 2), &[]),
            at(22, [17, 13, 17], 10, (4, 13), &UNARMED_PAST_20),
        ),
        class(
            5,
            "Jedi Sentinel",
            (8, 6, 2),
            at(15, [12, 12, 9], 6, (1, 1), &[]),
            at(22, [17, 17, 13], 10, (4, 10), &UNARMED_PAST_20),
        ),
        class(6, "Combat Droid", (12, 0, 1), at(21, [12, 7, 7], 6, (1, 0), &[]), at(30, [17, 10, 10], 10, (4, 0), &[])),
        class(7, "Expert Droid", (8, 0, 2), at(15, [7, 12, 7], 6, (1, 0), &[]), at(22, [10, 17, 10], 10, (6, 0), &[])),
        class(8, "Minion", (10, 0, 1), at(21, [9, 9, 9], 0, (0, 0), &[]), at(30, [14, 14, 14], 0, (5, 0), &[])),
    ]
}

/// KOTOR 1's classes as the game reads them (KOTOR 2's powers and feats
/// lent), and KOTOR 2's.
fn both(data: &Data) -> (Classes, Classes) {
    let of = |resolver: &Resolver| session::classes::classes(&ResolverSource(resolver));
    (of(&data.lent), of(&data.tsl))
}

/// Every KOTOR 1 class has a row for each level to 50 of its attack bonus,
/// saves, Defense bonus and feats picked, and each Jedi class of its powers
/// picked. Level 50 is the last a character can reach.
fn tables(data: &Data) -> Result<String, String> {
    let (k1, _) = both(data);
    let most = usize::from(rules::progression::levels::MOST);
    let mut short = Vec::new();
    for (row, class) in k1.classes.iter().enumerate() {
        let counts = [
            ("attack", class.bab.len()),
            ("saves", class.saves.len()),
            ("defence", class.defence.len()),
            ("feats", class.feat_picks.len()),
        ];
        let jedi = !class.powers.is_empty();
        let powers = [("powers", class.power_picks.len())];
        let counted = counts.iter().chain(powers.iter().filter(|_| jedi));
        short.extend(
            counted
                .filter(|(_, count)| *count < most)
                .map(|(name, count)| format!("class {row} {name}: {count}")),
        );
    }
    let cap = rules::progression::levels::MOST;
    let capped = k1.levels.len() == most && !k1.may_level(cap, i32::MAX);
    check(
        k1.classes.len() == wanted().len() && short.is_empty() && capped,
        format!(
            "{} classes, {} levels, level {cap} the last {capped}; short {short:?}",
            k1.classes.len(),
            k1.levels.len()
        ),
    )
}

/// A level-20 character of `class` who has every feat the class grants up
/// to level 20, with each ability at 10 and the class's level-20 attack
/// bonus, saves and Defense bonus.
fn twentieth(class: &Class, of: u8) -> (Sheet, Career) {
    let row = usize::from(TWENTY - 1);
    let mut sheet = Sheet::commoner();
    sheet.level = TWENTY;
    sheet.abilities = [10; 6];
    sheet.bab = class.bab[row];
    sheet.saves = class.saves[row];
    sheet.defence = class.defence.get(row).copied().unwrap_or(0);
    sheet.feats = Default::default();
    for &(level, feat) in &class.granted {
        if level <= TWENTY {
            sheet.feats.insert(feat);
        }
    }
    (sheet, vec![(of, TWENTY)])
}

/// What levelling a character up makes (`level_to`): its sheet, the skill
/// points left over, the feats and powers its levels offered, and the feats
/// it was granted.
struct Levelled {
    sheet: Sheet,
    skill_points: u8,
    feats: u8,
    powers: u8,
    granted: Vec<u16>,
}

/// `sheet` and `career` levelled up to `to` in `class`, picking nothing and
/// raising Strength at each ability level.
fn level_to(classes: &Classes, class: u8, (sheet, career): (Sheet, Career), to: u8) -> Result<Levelled, String> {
    let before = sheet;
    let (mut sheet, mut career, mut banked) = (sheet, career, 0);
    let (mut feats, mut powers) = (0, 0);
    while sheet.level < to {
        let offer = classes
            .offer(&sheet, &career, class)
            .ok_or(format!("no level {} offered", sheet.level + 1))?;
        feats += offer.feats;
        powers += offer.powers;
        let choices = Choices {
            class,
            ability: offer.ability.then_some(0),
            ..Choices::default()
        };
        (sheet, career, banked) = classes
            .taken(&sheet, &career, &choices, banked)
            .map_err(|why| format!("level {} refused: {why:?}", sheet.level + 1))?;
    }
    let granted = sheet.feats.rows().filter(|feat| !before.feats.has(*feat)).collect();
    Ok(Levelled {
        sheet,
        skill_points: banked,
        feats,
        powers,
        granted,
    })
}

/// Each KOTOR 1 class levelled from 20 to 21 and from 20 to 30: the attack
/// bonus, saves, Defense bonus, feats and powers offered and lent feats
/// granted are `wanted`'s, and each level adds the class's hit die and
/// force die and its skill points.
fn levels(data: &Data) -> Result<String, String> {
    let (k1, _) = both(data);
    let mut wrong = Vec::new();
    let mut lines = Vec::new();
    for want in wanted() {
        let class = k1.classes.get(usize::from(want.class)).ok_or("a class is missing")?;
        for (to, reach) in [(21, &want.at_21), (30, &want.at_30)] {
            let start = twentieth(class, want.class);
            let (hp, fp) = (start.0.max_hp, start.0.max_fp);
            let got = level_to(&k1, want.class, start, to)?;
            let taken = u16::from(to - TWENTY);
            let reached = Reached {
                bab: got.sheet.bab,
                saves: got.sheet.saves,
                defence: got.sheet.defence,
                feats: got.feats,
                powers: got.powers,
                granted: got.granted,
            };
            let gained = (
                got.sheet.max_hp - hp,
                got.sheet.max_fp - fp,
                got.skill_points,
            );
            let should = (
                want.hp * taken,
                want.fp * taken,
                want.skill_points * (to - TWENTY),
            );
            if reached != *reach || gained != should {
                wrong.push(format!("{} to {to}: {reached:?} {gained:?}", want.name));
            }
            lines.push(format!(
                "{} {to}: attack {}, saves {:?}, defence {}, feats {}, powers {}, granted {:?}, \
                 +{} vitality, +{} Force, {} skill points",
                want.name,
                reached.bab,
                reached.saves,
                reached.defence,
                reached.feats,
                reached.powers,
                reached.granted,
                gained.0,
                gained.1,
                gained.2
            ));
        }
    }
    check(wrong.is_empty(), format!("wrong: {wrong:?}; {}", lines.join("; ")))
}

/// Each KOTOR 1 Jedi class may pick Master Revitalize (a power KOTOR 2
/// lends at level 21) with its level 21, knowing Revitalize and Improved
/// Revitalize, and is told it needs level 21 at its level 20.
fn master_revitalize(data: &Data) -> Result<String, String> {
    let (k1, _) = both(data);
    let mut lines = Vec::new();
    let mut good = true;
    for (row, class) in k1.classes.iter().enumerate().filter(|(_, class)| !class.powers.is_empty()) {
        let row = row as u8;
        let (mut sheet, career) = twentieth(class, row);
        sheet.powers.insert(FORCE_POWER_REVITALIZE);
        sheet.powers.insert(FORCE_POWER_IMPROVED_REVITALIZE);
        let choices = Choices {
            class: row,
            powers: vec![FORCE_POWER_MASTER_REVITALIZE],
            ..Choices::default()
        };
        let at_21 = k1
            .taken(&sheet, &career, &choices, 0)
            .map(|(grown, ..)| grown.powers.has(FORCE_POWER_MASTER_REVITALIZE));
        let mut nineteenth = sheet;
        nineteenth.level = TWENTY - 1;
        // Level 20 raises an ability.
        let raising = Choices {
            ability: Some(0),
            ..choices.clone()
        };
        let at_20 = k1
            .taken(&nineteenth, &vec![(row, TWENTY - 1)], &raising, 0)
            .map(|_| true);
        let refused = Refusal::Power(FORCE_POWER_MASTER_REVITALIZE, Unpickable::Level(21));
        good &= at_21 == Ok(true) && at_20 == Err(refused);
        lines.push(format!("class {row}: at 21 {at_21:?}, at 20 {at_20:?}"));
    }
    check(good && lines.len() == 3, lines.join("; "))
}

/// The rows of `column` of `table` in `resolver`, in order while they read
/// as numbers.
fn column(resolver: &Resolver, table: &str, column: &str) -> Vec<i64> {
    resolver.two_da(table).map_or_else(Vec::new, |table| {
        (0..table.cells.len())
            .map_while(|row| table.int(row, column))
            .collect()
    })
}

/// `values` as `i64`s.
fn widened<T: Copy + Into<i64>>(values: &[T]) -> Vec<i64> {
    values.iter().map(|&value| value.into()).collect()
}

/// A class's by-level columns as levelling reads them, beside the same
/// columns of the tables `classes.2da` names for it in `resolver`. A
/// level's feats are its `featgain` row's regular and bonus feats.
type Columns = Vec<(&'static str, Vec<i64>, Vec<i64>)>;
fn columns(resolver: &Resolver, class: &Class, row: usize) -> Result<Columns, String> {
    let classes = resolver.two_da("classes").ok_or("no classes.2da")?;
    let named = |column: &str| classes.text(row, column).unwrap_or_else(Default::default).to_ascii_lowercase();
    let saves = |at: usize| widened(&class.saves.iter().map(|saves| saves[at]).collect::<Vec<i8>>());
    let (attack, saving, gain) = (named("attackbonustable"), named("savingthrowtable"), named("featgain"));
    let bonus = column(resolver, "featgain", &format!("{gain}_bon"));
    let feats = column(resolver, "featgain", &format!("{gain}_reg"))
        .into_iter()
        .enumerate()
        .map(|(at, regular)| regular + bonus.get(at).copied().unwrap_or(0))
        .collect();
    Ok(vec![
        ("attack", widened(&class.bab), column(resolver, &attack, "bab")),
        ("fortitude", saves(0), column(resolver, &saving, "fortsave")),
        ("reflex", saves(1), column(resolver, &saving, "refsave")),
        ("will", saves(2), column(resolver, &saving, "willsave")),
        ("defence", widened(&class.defence), column(resolver, "acbonus", &named("armorclasscolumn"))),
        ("feats", widened(&class.feat_picks), feats),
        ("powers", widened(&class.power_picks), column(resolver, "classpowergain", &named("featstable"))),
    ])
}

/// KOTOR 1's rows for levels 1 to 20 are its own tables' rows, read
/// without KOTOR 2's loan.
fn own_rows(data: &Data) -> Result<String, String> {
    let (k1, _) = both(data);
    let twenty = usize::from(TWENTY);
    let mut wrong = Vec::new();
    for (row, class) in k1.classes.iter().enumerate() {
        for (name, read, table) in columns(&data.plain, class, row)? {
            let own = table.len().min(twenty);
            if read.get(..own) != table.get(..own) || own < twenty && !table.is_empty() {
                wrong.push(format!("class {row} {name}"));
            }
        }
    }
    check(
        wrong.is_empty(),
        format!("{} classes; rows 1 to 20 unlike KOTOR 1's tables {wrong:?}", k1.classes.len()),
    )
}

/// Every KOTOR 2 class's columns are KOTOR 2's tables' rows, no more and no
/// fewer, and its levels are its `exptable.2da`'s 50.
fn kotor2_unchanged(data: &Data) -> Result<String, String> {
    let (_, tsl) = both(data);
    let mut wrong = Vec::new();
    for (row, class) in tsl.classes.iter().enumerate() {
        for (name, read, table) in columns(&data.tsl, class, row)? {
            if read != table {
                wrong.push(format!("class {row} {name}: {} rows read, {} in the table", read.len(), table.len()));
            }
        }
    }
    // The table's last row, 0xFFFFFFFF, is a level that cannot be reached.
    let most = usize::from(rules::progression::levels::MOST);
    let xp = widened(&tsl.levels);
    let table = column(&data.tsl, "exptable", "xp");
    let levels_kept = xp.len() == most && table.get(..most) == Some(&xp[..]);
    check(
        wrong.is_empty() && levels_kept && !tsl.classes.is_empty(),
        format!(
            "{} classes; {} levels kept {levels_kept}; unlike the tables {wrong:?}",
            tsl.classes.len(),
            xp.len()
        ),
    )
}
