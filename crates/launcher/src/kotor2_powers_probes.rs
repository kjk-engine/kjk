//! KOTOR 2's Force powers in KOTOR 1 (`vfs::lent`, `kjk sim probes --only
//! "kotor2 powers"`), on both games' data where this machine has them: the
//! rows KOTOR 1 gets, their costs, names and levels, a KOTOR 1 table a mod
//! extended left alone, KOTOR 1's own data and checks unchanged, and a
//! KOTOR 1 character casting Force Barrier, Battle Meditation and Force
//! Scream through KOTOR 2's own script in the Endar Spire.

use fmt_odyssey::ResType;
use fmt_odyssey::tlk::LENT_FLAG;
use sim::probe::{Checks, check};
use vfs::Game;
use vfs::resolver::{Layer, Rank, Resolver, TextureQuality};

use crate::modules::ResolverSource;

/// The rows KOTOR 2's `spells.2da` offers KOTOR 1's three Jedi classes
/// past KOTOR 1's own: Master Energy Resistance, Master Heal, the Force
/// Barrier, Battle Meditation, Force Body, Drain Force and Force Scream
/// lines, Force Deflection, Force Redirection, the Revitalize line and
/// Mind Trick.
const LENT: [u16; 23] = [
    133, 134, 135, 136, 137, 138, 139, 140, 150, 151, 152, 153, 154, 155, 159, 160, 161, 162, 163,
    173, 174, 175, 181,
];

/// The powers cast in the Endar Spire, each through KOTOR 2's script.
const CAST: [u16; 3] = [
    sim::combat::spells::FORCE_POWER_FORCE_BARRIER,
    sim::combat::spells::FORCE_POWER_BATTLE_MEDITATION_PC,
    sim::combat::spells::FORCE_POWER_FORCE_SCREAM,
];

/// Both games' resolvers: KOTOR 1's as the game opens it (lent to) and
/// without the loan, and KOTOR 2's.
struct Data {
    lent: Resolver,
    plain: Resolver,
    tsl: Resolver,
}

fn data() -> Option<Data> {
    let discovery = kjk_game::discover();
    let open = |game| kjk_game::installs::open(&discovery, game).ok();
    let (k1, tsl) = (open(Game::Kotor1)?, open(Game::Kotor2)?);
    let plain = Resolver::base(Game::Kotor1, &k1.index, TextureQuality::High).ok()?;
    Some(Data {
        lent: k1.base().ok()?.clone(),
        plain,
        tsl: tsl.base().ok()?.clone(),
    })
}

/// A cell of `table` in `resolver`.
fn int(resolver: &Resolver, table: &str, row: u16, column: &str) -> Option<i64> {
    resolver.two_da(table)?.int(usize::from(row), column)
}

/// The checks, by name.
type Case = fn(&Data) -> Result<String, String>;
const CASES: [(&str, Case); 9] = [
    ("rows and costs", rows),
    ("names", names),
    ("icons", icons),
    ("effect icons and feedback", effect_icons),
    ("rule notes", rule_notes),
    ("level-up", level_up),
    ("KOTOR 1 stays KOTOR 1", unchanged),
    ("a mod's rows", modded),
    ("casts", casts),
];

pub fn probes() -> Checks {
    let data = data();
    CASES
        .iter()
        .map(|(name, case)| {
            let result = match &data {
                Some(data) => case(data),
                None => Ok("no KOTOR 1 or KOTOR 2 install: skipped".into()),
            };
            (*name, result)
        })
        .collect()
}

/// KOTOR 1 gets exactly `LENT`, each at KOTOR 2's Force point cost.
fn rows(data: &Data) -> Result<String, String> {
    let rows = data.lent.lent.as_ref().map(|lent| lent.rows());
    let costs: Vec<(u16, Option<i64>, Option<i64>)> = LENT
        .iter()
        .map(|&row| {
            let cost = |resolver| int(resolver, "spells", row, "forcepoints");
            (row, cost(&data.lent), cost(&data.tsl))
        })
        .filter(|(_, here, there)| here != there || here.is_none() && *there != Some(0))
        .collect();
    check(
        rows.as_deref() == Some(&LENT[..]) && costs.is_empty(),
        format!("rows {rows:?}; costs that differ from KOTOR 2's: {costs:?}"),
    )
}

/// Each row's name and description read from KOTOR 2's talk table.
fn names(data: &Data) -> Result<String, String> {
    let (Some(k1), Some(tsl)) = (data.lent.talk.get(), data.tsl.talk.get()) else {
        return Err("a talk table does not read".into());
    };
    let mut wrong = Vec::new();
    let mut shown = Vec::new();
    for row in LENT {
        for column in ["name", "spelldesc"] {
            let strref = |resolver| int(resolver, "spells", row, column).map(|s| s as u32);
            let (Some(here), Some(there)) = (strref(&data.lent), strref(&data.tsl)) else {
                wrong.push(format!("{row} {column}: no strref"));
                continue;
            };
            let text = k1.text(here);
            if here != (there | LENT_FLAG) || text.is_none() || text != tsl.text(there) {
                wrong.push(format!("{row} {column}"));
            } else if column == "name" {
                shown.push(String::from_utf8_lossy(text.unwrap_or(&[])).into_owned());
            }
        }
    }
    check(wrong.is_empty(), format!("wrong: {wrong:?}; names: {shown:?}"))
}

/// Each row's icon is a texture KOTOR 1's data serves (KOTOR 2's own,
/// where KOTOR 1 has none of the name).
fn icons(data: &Data) -> Result<String, String> {
    let Some(spells) = data.lent.two_da("spells") else {
        return Err("no spells.2da".into());
    };
    let missing: Vec<(u16, Option<String>)> = LENT
        .iter()
        .map(|&row| (row, spells.resref(usize::from(row), "iconresref")))
        .filter(|(_, icon)| {
            icon.as_ref()
                .is_none_or(|icon| data.lent.texture(icon).is_none())
        })
        .collect();
    check(missing.is_empty(), format!("rows without an icon: {missing:?}"))
}

/// The effect icons KOTOR 2's script shows on a character past KOTOR 1's
/// `effecticon.2da` (Force Barrier's 64 to 66, Battle Meditation's 67 to 69
/// and its enemies' 107 to 109, Force Body's 76, Master Energy Resistance's
/// 104 and Breath Control's 111) are KOTOR 2's rows, at KOTOR 2's numbers,
/// their icons served wherever KOTOR 2 has them (Battle Meditation's
/// enemies' icon, `ip_resistenergy`, is in neither game). KOTOR 1's own rows are as before. The script's
/// feedback lines (`feedbacktext.2da` rows 0 to 2: resisted, immune,
/// saved) read the same in both games.
fn effect_icons(data: &Data) -> Result<String, String> {
    const SHOWN: [usize; 12] = [64, 65, 66, 67, 68, 69, 76, 104, 107, 108, 109, 111];
    let table = |resolver: &Resolver, name: &str| resolver.two_da(name).ok_or(format!("no {name}.2da"));
    let (lent, plain, tsl) = (
        table(&data.lent, "effecticon")?,
        table(&data.plain, "effecticon")?,
        table(&data.tsl, "effecticon")?,
    );
    let kept = lent.cells[..plain.cells.len()] == plain.cells[..];
    let wrong: Vec<usize> = SHOWN
        .into_iter()
        .filter(|&row| {
            let icon = lent.resref(row, "iconresref");
            let served = |resolver: &Resolver| {
                icon.as_ref()
                    .is_some_and(|icon| resolver.texture(icon).is_some())
            };
            icon != tsl.resref(row, "iconresref") || served(&data.tsl) && !served(&data.lent)
        })
        .collect();
    let (Some(k1_talk), Some(tsl_talk)) = (data.lent.talk.get(), data.tsl.talk.get()) else {
        return Err("a talk table does not read".into());
    };
    let (k1_lines, tsl_lines) = (table(&data.lent, "feedbacktext")?, table(&data.tsl, "feedbacktext")?);
    let line = |table: &fmt_odyssey::twoda::TwoDa, talk: &fmt_odyssey::tlk::Tlk, row: usize| {
        let strref = u32::try_from(table.int(row, "strref")?).ok()?;
        talk.text(strref).map(<[u8]>::to_vec)
    };
    let lines: Vec<Option<Vec<u8>>> = (0..3).map(|row| line(&k1_lines, &k1_talk, row)).collect();
    let same = (0..3).all(|row| lines[row].is_some() && lines[row] == line(&tsl_lines, &tsl_talk, row));
    check(
        kept && wrong.is_empty() && same,
        format!(
            "KOTOR 1's {} rows kept {kept}; rows not KOTOR 2's or without KOTOR 2's icon: {wrong:?}; \
             feedback lines alike {same}: {:?}",
            plain.cells.len(),
            lines.iter().flatten().map(|text| String::from_utf8_lossy(text).into_owned()).collect::<Vec<_>>()
        ),
    )
}

/// The rulebook's paragraphs a row's description goes on with (its
/// `applies`), the same in both games since they follow the row's label:
/// none of Affect Mind's and Dominate Mind's reaches Mind Trick or Force
/// Confusion, and each Force Scream row tells of the ear covering.
fn rule_notes(data: &Data) -> Result<String, String> {
    let Some(spells) = data.tsl.two_da("spells") else {
        return Err("no spells.2da".into());
    };
    let label = |row: u16| spells
        .text(usize::from(row), "label")
        .unwrap_or_else(String::new);
    let paths = |label: &str| -> Vec<&'static str> {
        rules::applying(&format!("spell:{label}"))
            .map(|entry| entry.path)
            .collect()
    };
    let confusion = sim::combat::spells::FORCE_POWER_CONFUSION;
    let wrong: Vec<(u16, &str)> = LENT
        .iter()
        .chain(&[confusion])
        .flat_map(|&row| paths(&label(row)).into_iter().map(move |path| (row, path)))
        .filter(|(_, path)| path.starts_with("force.mind_trick"))
        .collect();
    let screams = [
        sim::combat::spells::FORCE_POWER_FORCE_SCREAM,
        sim::combat::spells::FORCE_POWER_IMPROVED_FORCE_SCREAM,
        sim::combat::spells::FORCE_POWER_MASTER_FORCE_SCREAM,
    ];
    let ears = ["force.crowd.ears_ms", "force.crowd.ears_immune_ms"];
    let deaf: Vec<u16> = screams
        .into_iter()
        .filter(|&row| !ears.iter().all(|ear| paths(&label(row)).contains(ear)))
        .collect();
    check(
        wrong.is_empty() && deaf.is_empty(),
        format!("Affect Mind's paragraphs on {wrong:?}; Force Scream rows without the ear covering: {deaf:?}"),
    )
}

/// KOTOR 1's Jedi classes offer each row at KOTOR 2's level.
fn level_up(data: &Data) -> Result<String, String> {
    let classes = session::classes::classes(&ResolverSource(&data.lent));
    let jedi: Vec<&sim::levels::Class> = classes
        .classes
        .iter()
        .filter(|class| !class.powers.is_empty())
        .collect();
    let mut wrong = Vec::new();
    for class in &jedi {
        for row in LENT {
            let wanted = int(&data.tsl, "spells", row, "guardian");
            let offered = class.powers.iter().find(|(_, power)| *power == row);
            if offered.map(|(level, _)| i64::from(*level)) != wanted {
                wrong.push((row, offered.map(|(level, _)| *level), wanted));
            }
        }
    }
    check(
        jedi.len() == 3 && wrong.is_empty(),
        format!("{} Jedi classes; offered at another level: {wrong:?}", jedi.len()),
    )
}

/// KOTOR 1's own rows, routines, script and checks are as without the
/// loan: no `itemcreate.2da` (the check of whether an install is KOTOR
/// 2's), KOTOR 1's skill points and granted powers.
fn unchanged(data: &Data) -> Result<String, String> {
    let spells = |resolver: &Resolver| resolver.two_da("spells");
    let (Some(lent), Some(plain)) = (spells(&data.lent), spells(&data.plain)) else {
        return Err("no spells.2da".into());
    };
    let own_rows = plain.cells.len();
    let rows_kept = lent.cells[..own_rows] == plain.cells[..] && lent.columns == plain.columns;
    let script = |resolver: &Resolver| resolver.read("k_sp1_generic", ResType::NCS);
    let script_kept = script(&data.lent) == script(&data.plain);
    let nss = |resolver: &Resolver| resolver.read("nwscript", ResType::NSS);
    let routines_kept = nss(&data.lent)
        .zip(nss(&data.plain))
        .is_some_and(|(lent, plain)| lent.starts_with(&plain));
    let not_tsl = data.lent.find("itemcreate", ResType::TWO_DA).is_none();
    let classes = session::classes::classes(&ResolverSource(&data.lent));
    let k1_rules = !classes.whole_skill_points && !classes.granted_powers;
    check(
        rows_kept && script_kept && routines_kept && not_tsl && k1_rules,
        format!(
            "{own_rows} own rows kept {rows_kept}, k_sp1_generic kept {script_kept}, \
             routines kept {routines_kept}, no itemcreate.2da {not_tsl}, KOTOR 1's level rules {k1_rules}"
        ),
    )
}

/// A KOTOR 1 table with a row of its own at 132 (a mod's power) gets none
/// of KOTOR 2's rows, and the resolver is left as it was.
fn modded(data: &Data) -> Result<String, String> {
    let mut resolver = data.plain.clone();
    let Some(mut spells) = resolver.two_da("spells") else {
        return Err("no spells.2da".into());
    };
    let mut row = spells.cells[0].clone();
    if let Some(label) = spells.column("label") {
        row[label] = b"FORCE_POWER_A_MODS_OWN".to_vec();
    }
    spells.row_labels.push(spells.cells.len().to_string().into_bytes());
    spells.cells.push(row);
    let rows = spells.cells.len();
    let bytes: std::sync::Arc<[u8]> = spells.write().into();
    let location = vfs::resolver::Location {
        path: "override/spells.2da".into(),
        offset: 0,
        size: bytes.len() as u64,
        held: Some(bytes),
    };
    resolver.layers.push(std::sync::Arc::new(Layer {
        name: "override".into(),
        rank: Rank::Directory,
        resources: [(("spells".to_string(), ResType::TWO_DA), location)].into(),
    }));
    let refused = resolver.lend(&data.tsl);
    let after = resolver.two_da("spells").map(|table| table.cells.len());
    check(
        refused.is_err() && after == Some(rows) && resolver.lent.is_none(),
        format!("{refused:?}; {after:?} rows after, {rows} before"),
    )
}

/// A KOTOR 1 Jedi casts Force Barrier, Battle Meditation and Force Scream
/// at the Endar Spire's Sith soldier: each runs KOTOR 2's script, pays
/// KOTOR 2's cost and does something a scene without it does not. A
/// character who knows every row can choose none that KOTOR 1's table has
/// no name for (KOTOR 2's Fury and Force Sight, which it lacks, and its own
/// unused row 39).
fn casts(data: &Data) -> Result<String, String> {
    let hostile = ["sithsoldier".to_string()];
    let stage = crate::scenes::Stage::new(&kjk_game::discover(), "k1", "end_m01aa", &hostile)?;
    let target = stage.creature("sithsoldier").ok_or("no sithsoldier")?;
    let library = stage.level.scripts().ok_or("no scripts")?;
    let own = library.impact(sim::combat::spells::FORCE_POWER_FORCE_PUSH);
    let mut lines = vec![format!("KOTOR 1's Force Push runs {own:?}")];
    let mut good = own == Some("k_sp1_generic");
    for row in CAST {
        let script = library.impact(row);
        let scene = crate::force_verbs::lent_scene(&stage, target, row);
        let works = script == Some("k2_sp1_generic")
            && scene.as_ref().is_some_and(|(seen, paid, cost, replayed)| {
                !seen.is_empty() && *paid >= *cost && *cost > 0 && *replayed
            });
        good &= works;
        lines.push(format!("{row} runs {script:?}: {scene:?}"));
    }
    let spells = data.lent.two_da("spells").ok_or("no spells.2da")?;
    let every = (0..sim::combat::sheet::Powers::ROWS)
        .fold(sim::combat::sheet::Powers::default(), |known, row| known.with(row));
    let nameless: Vec<u16> = sim::combat::force::choices(&every, Some(library))
        .into_iter()
        .map(sim::combat::force::data_row)
        .filter(|&row| spells.int(usize::from(row), "name").is_none())
        .collect();
    good &= nameless.is_empty();
    lines.push(format!("choices without a name: {nameless:?}"));
    check(good, lines.join("; "))
}
