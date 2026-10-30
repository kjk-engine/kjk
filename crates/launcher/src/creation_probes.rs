//! `kjk sim probes --only "creation"`: quick creation on the games' own
//! data. Each class a game's creation offers is made of a player portrait
//! in its first module (K1's Endar Spire, TSL's Ebon Hawk), as `kjk play
//! --made` makes it, and its sheet checked against the tables: the body its
//! class takes of the portrait, level 1, the hit die and CON modifier, the
//! skill points spent (K1's half of the class's and the INT modifier, TSL's
//! whole, times 4), the feats its first level grants, no powers, and the
//! globals a new game sets (TSL's `G_PC_LEVEL`, `G_PC_Align_Val`), and
//! TSL's player feats and its powers open from level 0. A game
//! not installed is skipped.

use std::sync::Arc;

use sim::probe::{Checks, check};
use vfs::Game;

use crate::dialogue_verbs::{Run, play};
use crate::lifecycle::Installer;

/// Each game's creation: its first module, its classes (`classes.2da`
/// rows, `--made`'s names) and a man's and a woman's player portrait.
type Creations = (Game, &'static str, &'static [(u8, &'static str)], [&'static str; 2]);

const GAMES: [Creations; 2] = [
    (
        Game::Kotor1,
        "end_m01aa",
        &[(0, "soldier"), (1, "scout"), (2, "scoundrel")],
        ["po_pmhc1", "po_pfha2"],
    ),
    (
        Game::Kotor2,
        "001ebo",
        &[(3, "guardian"), (4, "consular"), (5, "sentinel")],
        ["po_pmha01", "po_pfha01"],
    ),
];

/// `spells.2da`'s Force Push.
const FORCE_PUSH: u16 = 23;

/// `portraits.2da`'s body column a class takes (`persona::build`).
fn body_column(class: u8) -> &'static str {
    match class {
        0 | 3 => "appearance_l",
        2 | 4 => "appearance_s",
        _ => "appearancenumber",
    }
}

pub fn probes() -> Checks {
    let discovery = crate::discover();
    let mut checks: Checks = Vec::new();
    for (game, module, classes, portraits) in GAMES {
        if discovery.get(game).is_none() {
            continue;
        }
        let anims = Arc::new(sim::motion::AnimTable::stand_in());
        let installer = match Installer::new(&discovery, game, anims) {
            Ok(installer) => installer,
            Err(why) => {
                checks.push(("creation: the install opens", Err(why)));
                continue;
            }
        };
        for (at, &(class, named)) in classes.iter().enumerate() {
            let portrait = portraits[at % 2];
            let made = made(&installer, module, (class, portrait));
            let name: &'static str = Box::leak(
                format!("{}: quick {named} of {portrait}", game.key()).into_boxed_str(),
            );
            checks.push((name, made));
        }
    }
    checks
}

/// Quick creation of `class` of `portrait` in `module`, checked against the
/// install's tables.
fn made(installer: &Installer, module: &str, (class, portrait): (u8, &str)) -> Result<String, String> {
    let run = Run {
        made: Some((class, portrait.to_string(), "Probe".into())),
        ..Run::default()
    };
    let mut played = play(installer, module, &run)?;
    let resolver = installer.base_resolver()?;
    let tables = |name: &str| resolver.two_da(name).ok_or(format!("{name}.2da: not read"));
    let (classes, portraits) = (tables("classes")?, tables("portraits")?);
    let row = usize::from(class);
    let cell = |table: &fmt_odyssey::twoda::TwoDa, row: usize, column: &str| {
        table.int(row, column).ok_or(format!("{column} of row {row}: not read"))
    };
    let portrait_row = (0..portraits.row_labels.len())
        .find(|&at| {
            portraits
                .text(at, "baseresref")
                .is_some_and(|texture| texture.eq_ignore_ascii_case(portrait))
        })
        .ok_or(format!("{portrait}: not in portraits.2da"))?;
    let body = cell(&portraits, portrait_row, body_column(class))?;
    let modifier = |score: i64| (score - 10).div_euclid(2);
    let con = modifier(cell(&classes, row, "con")?);
    let int = modifier(cell(&classes, row, "int")?);
    let base = cell(&classes, row, "skillpointbase")?;
    let whole = played.library.classes().whole_skill_points;
    let halved = ((base + int) / 2).max(1) * 4;
    let points = if whole {
        (base + int).max(1) * 4
    } else {
        halved
    };
    // The recommended skills spend what the caps let them; TSL's more than
    // K1's rule would give.
    let spent_right = |spent: i64| spent <= points && (!whole || points == halved || spent > halved);
    let snapshot = played.world.snapshot();
    let me = snapshot.own(sim::Seat(0)).ok_or("no player")?;
    let sheet = me.sheet.ok_or("no sheet")?;
    // The hit die, the CON modifier and what its feats add a level
    // (Toughness, picked by the game for some classes).
    let hp = cell(&classes, row, "hitdie")?
        + con
        + i64::from(sim::combat::passive::hp_per_level(&sheet.feats));
    let persona = sim::persona::of(&snapshot.personas, sim::Seat(0)).ok_or("no persona")?;
    let of_class = &played.library.classes().classes[row];
    let spent: i64 = sheet
        .skills
        .iter()
        .enumerate()
        .map(|(skill, ranks)| i64::from(*ranks) * if of_class.class_skills[skill] { 1 } else { 2 })
        .sum();
    let granted: Vec<u16> = of_class
        .granted
        .iter()
        .filter(|(level, _)| *level == 1)
        .map(|(_, feat)| *feat)
        .collect();
    // TSL's player feats granted besides (`_pc_granted`), and a power its
    // `spells.2da` column opens at level 0 (Force Push) among those its
    // levels may pick.
    let pc_granted = of_class.pc_granted.iter().all(|feat| sheet.feats.has(*feat))
        && (!whole || !of_class.pc_granted.is_empty());
    let push = !whole || of_class.powers.contains(&(0, FORCE_PUSH));
    let powers = (0..sim::combat::sheet::Rows::<5>::ROWS)
        .filter(|power| sheet.powers.has(*power))
        .count();
    let numbers = &played.world.script_state().numbers;
    let global = |name: &str| sim::script::global(numbers, name).copied();
    let declared = |name: &str| {
        played
            .world
            .level()
            .module()
            .is_some_and(|module| module.globals.contains_key(&name.to_ascii_lowercase()))
    };
    let told = [("G_PC_LEVEL", 1), ("G_PC_Align_Val", 50)]
        .iter()
        .all(|(name, value)| !declared(name) || global(name) == Some(*value));
    let facts = format!(
        "appearance {} (portrait's {body}), level {}, hp {} ({hp}), skills {:?} spent {spent} \
         of {points}, granted {granted:?} known {}, {powers} powers, G_PC_LEVEL {:?}, \
         G_PC_Align_Val {:?}, player feats {pc_granted}, Force Push {push}",
        persona.appearance,
        sheet.level,
        sheet.max_hp,
        sheet.skills,
        granted.iter().all(|feat| sheet.feats.has(*feat)),
        global("G_PC_LEVEL"),
        global("G_PC_Align_Val"),
    );
    check(
        i64::from(persona.appearance) == body
            && sheet.level == 1
            && i64::from(sheet.max_hp) == hp.max(1)
            && spent_right(spent)
            && granted.iter().all(|feat| sheet.feats.has(*feat))
            && powers == 0
            && told
            && pc_granted
            && push,
        facts,
    )
}
