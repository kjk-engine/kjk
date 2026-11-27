//! `kjk sim probes --only "creation"`: quick creation on the games' own
//! data. Each class a game's creation offers is made of a player portrait
//! in its first module (K1's Endar Spire, TSL's Ebon Hawk), as `kjk play
//! --made` makes it, and its sheet checked against the tables: the body its
//! class takes of the portrait, level 1, the hit die and CON modifier, the
//! skill points spent (at level 1 in K1 half of the class base plus the full
//! INT modifier, in TSL whole, times 4), the feats its first level grants, no powers, and the
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

/// The `portraits.2da` body column that a class takes. This copies the rule
/// in `session::portraits` on purpose, so this probe checks that rule
/// against its own answer instead of reusing it.
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
        let name: &'static str =
            Box::leak(format!("{}: level-up knows every class", game.key()).into_boxed_str());
        checks.push((name, every_class(&discovery, game)));
        let name: &'static str =
            Box::leak(format!("{}: the highest level and the experience for each level", game.key()).into_boxed_str());
        checks.push((name, level_cap(&discovery, game)));
        if game == Game::Kotor1 {
            checks.push((
                "k1: class tables past level 20 grow like TSL's",
                k1_levels_past_20(&discovery),
            ));
        }
        if game == Game::Kotor2 {
            checks.push((
                "tsl: level-up power chart lists only powers the class may pick",
                tsl_power_chart(&discovery),
            ));
        }
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

/// The screens' class views (`character::chargen`, which level-up reads by
/// the character's class row) hold every `classes.2da` row with its own skill
/// points and class skills, the sim's: issue 106, Bastila's Jedi class
/// (past the classes K1's creation offers) levelled up with no class's
/// points and every skill cross-class.
fn every_class(discovery: &vfs::Discovery, game: Game) -> Result<String, String> {
    let mut assets = ui::assets::GuiAssets::open(discovery, game)?;
    let view = crate::character::chargen(&mut assets);
    let source = kjk_game::viewer::ResolverSource(assets.resolver());
    let sim = session::classes::classes(&source);
    let rows = assets.rows("classes");
    let wrong: Vec<usize> = (0..rows)
        .filter(|&row| {
            let shown = view.classes.get(row);
            let own = sim.classes.get(row);
            match (shown, own) {
                (Some(shown), Some(own)) => {
                    shown.skill_points != own.skill_points
                        || shown.class_skills != own.class_skills
                }
                _ => true,
            }
        })
        .collect();
    check(
        rows > 0 && view.classes.len() == rows && wrong.is_empty(),
        format!(
            "{} views of {rows} rows, rows differing from the sim's {wrong:?}",
            view.classes.len()
        ),
    )
}

/// The install's classes as levelling reads them.
fn install_classes(discovery: &vfs::Discovery, game: Game) -> Result<sim::levels::Classes, String> {
    let assets = ui::assets::GuiAssets::open(discovery, game)?;
    let source = kjk_game::viewer::ResolverSource(assets.resolver());
    Ok(session::classes::classes(&source))
}

/// The highest level a character can reach is `levels::MOST` in both games,
/// and level n needs `levels::XP_STEP` × n × (n − 1) / 2 XP: K1's
/// `exptable.2da` for levels 1 to 20, the rule for 21 to 50, TSL's table for
/// all 50. Both tables end with a row of `0xFFFFFFFF`, which both games read
/// as a level that cannot be reached. Read as the decimal digits before the
/// `x`, it was 0 XP, and any character could take the level after the
/// table's last (Canderous showed "Soldier 21" at 250000 XP).
fn level_cap(discovery: &vfs::Discovery, game: Game) -> Result<String, String> {
    let cap = rules::progression::levels::MOST;
    let step = rules::progression::levels::XP_STEP;
    let classes = install_classes(discovery, game)?;
    let reached = classes.potential_level(1, i32::MAX);
    let wanted: Vec<i32> = (0..i32::from(cap))
        .map(|below| step * below * (below + 1) / 2)
        .collect();
    let wrong: Vec<usize> = (0..wanted.len())
        .filter(|&at| classes.levels.get(at) != wanted.get(at))
        .map(|at| at + 1)
        .collect();
    check(
        reached == cap && !classes.may_level(cap, i32::MAX) && wrong.is_empty(),
        format!(
            "{} levels, level {reached} reached with the most XP, level 21 at {:?} and \
             level {cap} at {:?} XP, levels off the rule {wrong:?}",
            classes.levels.len(),
            classes.levels.get(20),
            classes.levels.last()
        ),
    )
}

/// K1's classes past level 20 (`data/levels_k1.txt`): each class's attack
/// bonus, saves, Defense bonus, feats and powers picked have a row for every
/// class level to `levels::MOST`. Where a K1 class's rows for levels 1 to 20
/// are TSL's for the same class, its rows for 21 to 50 are TSL's too (when
/// TSL is installed). A level past the last offers no feats or powers and
/// does not panic.
fn k1_levels_past_20(discovery: &vfs::Discovery) -> Result<String, String> {
    const TWENTY: usize = 20;
    let most = usize::from(rules::progression::levels::MOST);
    let k1 = install_classes(discovery, Game::Kotor1)?;
    let tsl = discovery
        .get(Game::Kotor2)
        .map(|_| install_classes(discovery, Game::Kotor2))
        .transpose()?;
    let mut short = Vec::new();
    let mut unlike = Vec::new();
    let mut alike = 0;
    for (row, class) in k1.classes.iter().enumerate() {
        let saves = |saves: &[[i8; 3]], at: usize| -> Vec<i16> {
            saves.iter().map(|save| i16::from(save[at])).collect()
        };
        let columns = |class: &sim::levels::Class| -> [(&'static str, Vec<i16>); 7] {
            [
                ("attack", class.bab.iter().map(|&v| i16::from(v)).collect()),
                ("fortitude", saves(&class.saves, 0)),
                ("reflex", saves(&class.saves, 1)),
                ("will", saves(&class.saves, 2)),
                ("defence", class.defence.iter().map(|&v| i16::from(v)).collect()),
                ("feats", class.feat_picks.iter().map(|&v| i16::from(v)).collect()),
                ("powers", class.power_picks.iter().map(|&v| i16::from(v)).collect()),
            ]
        };
        let theirs = tsl.as_ref().and_then(|tsl| tsl.classes.get(row)).map(columns);
        for (at, (name, mine)) in columns(class).into_iter().enumerate() {
            // A class with no powers has no column for them.
            if mine.is_empty() {
                continue;
            }
            if mine.len() < most {
                short.push(format!("class {row} {name}: {} rows", mine.len()));
            }
            let Some((_, theirs)) = theirs.as_ref().map(|theirs| &theirs[at]) else {
                continue;
            };
            if theirs.len() >= most && mine.get(..TWENTY) == theirs.get(..TWENTY) {
                alike += 1;
                if mine.get(TWENTY..most) != theirs.get(TWENTY..most) {
                    unlike.push(format!("class {row} {name}"));
                }
            }
        }
        let sheet = sim::combat::Sheet {
            level: u8::MAX - 1,
            ..sim::combat::Sheet::commoner()
        };
        let past = k1.offer(&sheet, &vec![(row as u8, u8::MAX - 1)], row as u8);
        if past.is_some_and(|offer| offer.feats > 0 || offer.powers > 0) {
            short.push(format!("class {row}: picks past the tables {past:?}"));
        }
    }
    check(
        short.is_empty() && unlike.is_empty() && (tsl.is_none() || alike > 0),
        format!(
            "{} classes; short {short:?}; {alike} columns alike with TSL to level 20, \
             unlike past it {unlike:?}{}",
            k1.classes.len(),
            if tsl.is_none() { " (TSL not installed)" } else { "" }
        ),
    )
}

/// TSL's level-up power chart for a Jedi Consular (`classes.2da` row 4) who
/// knows Force Push (23), Force Sight (176), Breath Control (270), the
/// unnamed meditation row (202) and the Force Channel form (265). Only Force
/// Push is a power the class may pick, so the chart has a row for it and none
/// for the other four, as in TSL's level-up screen. A player saw a nameless
/// row with an empty description there (Discord, 2026-11-12).
fn tsl_power_chart(discovery: &vfs::Discovery) -> Result<String, String> {
    let assets = ui::assets::GuiAssets::open(discovery, Game::Kotor2)?;
    let source = kjk_game::viewer::ResolverSource(assets.resolver());
    let classes = session::classes::classes(&source);
    let consular = classes.classes.get(4).ok_or("classes.2da row 4: not read")?;
    let outside = [176, 202, 265, 270];
    let known = [FORCE_PUSH, 176, 202, 265, 270];
    let chart = classes.power_chart(consular, &known);
    let shown: Vec<u16> = chart.iter().flatten().flatten().copied().collect();
    let wrong: Vec<u16> = outside
        .into_iter()
        .filter(|power| shown.contains(power))
        .collect();
    check(
        shown.contains(&FORCE_PUSH) && wrong.is_empty(),
        format!("{} rows, powers outside the class shown {wrong:?}", chart.len()),
    )
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
    let k1_first = (base / 2 + int).max(1) * 4;
    let points = if whole {
        (base + int).max(1) * 4
    } else {
        k1_first
    };
    // The recommended skills spend what the caps let them; TSL's more than
    // K1's first-level rule when TSL's base is whole.
    let spent_right =
        |spent: i64| spent <= points && (!whole || points == k1_first || spent > k1_first);
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
    let powers = (0..sim::combat::sheet::Powers::ROWS)
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
