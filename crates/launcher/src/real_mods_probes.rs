//! Real Force power mods (`kjk sim probes --only "real mods"`), on game
//! folders a patcher has patched. The mods' files cannot be in the
//! repository, so each folder is named by a variable:
//!
//! - `KJK_MODDED_KOTOR2`: a KOTOR 2 game folder with High-Level Force
//!   Powers 2.0 and its update 2.1, Powers of the Triumvirate, or both, and
//!   the Triumvirate's Nihilus Has Hunger option or not.
//! - `KJK_MODDED_KOTOR1`: a KOTOR 1 game folder with K2 Force Powers for K1,
//!   then Bastila Has TSL Battle Meditation. KOTOR 2's own install, found as
//!   usual, lends its powers and feats to it.
//!
//! A group whose folder is not named has no checks at all, and says so on
//! the standard error. A check about a mod the folder lacks is left out the
//! same way: a skipped check is never shown as passed.
//!
//! The checks find the mods' rows by their labels, so a folder HoloPatcher
//! patched in another order is checked the same way.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fmt_odyssey::ResType;
use rules::rows::Table;
use sim::probe::{Checks, check};
use vfs::resolver::Resolver;
use vfs::{Discovery, Game};

use crate::modules::ResolverSource;
use crate::scenes::Stage;

/// The variables naming the patched game folders.
pub const MODDED_KOTOR1: &str = "KJK_MODDED_KOTOR1";
pub const MODDED_KOTOR2: &str = "KJK_MODDED_KOTOR2";

/// The label each mod's powers start with in KOTOR 2.
const HIGH_LEVEL: &str = "ST_FORCE_";
const TRIUMVIRATE: &str = "OTGM_FP_";

/// A KOTOR 2 module and the creatures a power is cast at there.
const KOTOR2_STAGE: (&str, &str) = ("202tel", "guard");

/// The usual installs with `game`'s replaced by the folder `var` names.
fn modded(var: &str, game: Game) -> Option<Result<Discovery, String>> {
    let path = std::env::var(var).ok().filter(|path| !path.trim().is_empty())?;
    let mut discovery = kjk_game::discover();
    discovery.installs.retain(|install| install.game != game);
    Some(vfs::install_at(game, &PathBuf::from(path.trim())).map(|install| {
        discovery.installs.push(install);
        discovery
    }))
}

/// The game data `discovery` opens for `game`, as play opens it.
fn opened(discovery: &Discovery, game: Game) -> Result<Resolver, String> {
    let install = kjk_game::installs::open(discovery, game)?;
    Ok(install.base()?.clone())
}

pub fn probes() -> Checks {
    let mut checks = Checks::new();
    for (var, game, cases) in [
        (MODDED_KOTOR2, Game::Kotor2, &KOTOR2_CASES[..]),
        (MODDED_KOTOR1, Game::Kotor1, &KOTOR1_CASES[..]),
    ] {
        match modded(var, game) {
            None => eprintln!("real mods: {var} names no game folder, so its {} checks are skipped", cases.len()),
            Some(Err(why)) => checks.push((cases[0].0, Err(format!("{var}: {why}")))),
            Some(Ok(discovery)) => {
                let data = opened(&discovery, game).map(|resolver| Modded { discovery, resolver });
                let present = data.as_ref().map_or_else(|_| vec![Mod::Any], |data| installed(data, game));
                for (name, of, case) in cases {
                    if !present.contains(of) {
                        eprintln!("real mods: {} is not installed in {var}'s folder, so \"{name}\" is skipped", of.name());
                        continue;
                    }
                    let result = match &data {
                        Ok(data) => case(data),
                        Err(why) => Err(why.clone()),
                    };
                    checks.push((name, result));
                }
            }
        }
    }
    checks
}

/// A patched game folder: the installs with it, and its data as play
/// opens it.
pub(crate) struct Modded {
    discovery: Discovery,
    resolver: Resolver,
}

type Case = fn(&Modded) -> Result<String, String>;

/// The mods a check is about. A check runs only on a folder that has its
/// mod; the others are skipped and said so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mod {
    /// Any of the folder's mods: the check covers the mods it finds.
    Any,
    HighLevel,
    Triumvirate,
    /// Powers of the Triumvirate's Nihilus Has Hunger option, which puts
    /// `852NIH.mod` in the Modules folder.
    Nihilus,
}

impl Mod {
    fn name(self) -> &'static str {
        match self {
            Mod::Any => "no Force power mod",
            Mod::HighLevel => "High-Level Force Powers",
            Mod::Triumvirate => "Powers of the Triumvirate",
            Mod::Nihilus => "Powers of the Triumvirate's Nihilus Has Hunger option",
        }
    }
}

/// The mods `data`'s folder of `game` has: a KOTOR 2 power mod by its
/// rows' labels, the Nihilus option by its module file, and `Any` when the
/// folder has any of the mods the checks know.
fn installed(data: &Modded, game: Game) -> Vec<Mod> {
    let has = |prefix| rows_labelled(&data.resolver, prefix).is_ok_and(|rows| !rows.is_empty());
    let mut mods = Vec::new();
    if game == Game::Kotor1 {
        if kotor1_mod_rows(data).is_ok_and(|rows| !rows.is_empty()) {
            mods.push(Mod::Any);
        }
        return mods;
    }
    if has(HIGH_LEVEL) {
        mods.push(Mod::HighLevel);
    }
    if has(TRIUMVIRATE) {
        mods.push(Mod::Triumvirate);
        let nihilus = kjk_game::installs::open(&data.discovery, game)
            .is_ok_and(|install| install.index.get("modules/852nih.mod").is_some());
        if nihilus {
            mods.push(Mod::Nihilus);
        }
    }
    if !mods.is_empty() {
        mods.push(Mod::Any);
    }
    mods
}

const KOTOR2_CASES: [(&str, Mod, Case); 9] = [
    ("kotor2 mods' rows, names and icons", Mod::Any, kotor2_rows),
    ("kotor2 mods' powers at level-up", Mod::Any, kotor2_level_up),
    ("high-level force powers cast", Mod::HighLevel, high_level_casts),
    ("kotor2 mods' powers for creatures", Mod::Any, kotor2_minds),
    ("high-level force powers' clone, poison and shield", Mod::HighLevel, high_level_pieces),
    ("triumvirate's placeables", Mod::Triumvirate, triumvirate_pieces),
    ("nihilus's hunger", Mod::Nihilus, nihilus_hunger),
    ("triumvirate's scripts", Mod::Triumvirate, triumvirate_scripts),
    ("triumvirate casts", Mod::Triumvirate, triumvirate_casts),
];

const KOTOR1_CASES: [(&str, Mod, Case); 5] = [
    ("kotor1 mods' rows by label", Mod::Any, kotor1_rows),
    ("kotor1 mods' powers at level-up", Mod::Any, kotor1_level_up),
    ("powers shown twice", Mod::Any, shown_twice),
    ("bastila's templates", Mod::Any, bastila_templates),
    ("kotor1 mods' scripts", Mod::Any, kotor1_scripts),
];

/// The served `spells.2da` rows whose labels start with `prefix`, by
/// number.
fn rows_labelled(resolver: &Resolver, prefix: &str) -> Result<Vec<(u16, String)>, String> {
    let spells = resolver.two_da("spells").ok_or("no spells.2da")?;
    Ok((0..spells.cells.len())
        .filter_map(|row| {
            let label = spells.text(row, "label")?;
            (label.starts_with(prefix)).then(|| (u16::try_from(row).ok(), label))
        })
        .filter_map(|(row, label)| Some((row?, label)))
        .collect())
}

/// For each of `rows`, what is missing of its name, description and icon.
fn shown(resolver: &Resolver, rows: &[(u16, String)]) -> Result<Vec<String>, String> {
    let spells = resolver.two_da("spells").ok_or("no spells.2da")?;
    let talk = resolver.talk.get().ok_or("no talk table")?;
    let mut missing = Vec::new();
    for (row, label) in rows {
        let row = usize::from(*row);
        for column in ["name", "spelldesc"] {
            let text = spells
                .int(row, column)
                .and_then(|strref| talk.text(strref as u32))
                .filter(|text| !text.is_empty());
            if text.is_none() {
                missing.push(format!("{label} {column}"));
            }
        }
        let icon = spells.resref(row, "iconresref");
        if icon.as_ref().is_none_or(|icon| resolver.texture(icon).is_none()) {
            missing.push(format!("{label} icon {icon:?}"));
        }
    }
    Ok(missing)
}

/// Every row of both KOTOR 2 mods is served at its own number (the map is
/// the identity, since both only add rows after KOTOR 2's own), with its
/// name, description and icon.
fn kotor2_rows(data: &Modded) -> Result<String, String> {
    let high = rows_labelled(&data.resolver, HIGH_LEVEL)?;
    let triumvirate = rows_labelled(&data.resolver, TRIUMVIRATE)?;
    let all: Vec<(u16, String)> = high.iter().chain(&triumvirate).cloned().collect();
    let missing = shown(&data.resolver, &all)?;
    // Each mod the folder has brings all its rows: High-Level Force Powers
    // 22 (19 of 2.0 and 3 of 2.1), Powers of the Triumvirate 3.
    let whole = |rows: &[(u16, String)], count: usize| rows.is_empty() || rows.len() == count;
    check(
        whole(&high, 22) && whole(&triumvirate, 3) && missing.is_empty() && data.resolver.rows.is_identity(),
        format!(
            "{} High-Level Force Powers rows and {} Triumvirate rows ({:?}); missing {missing:?}; \
             map the identity {}",
            high.len(),
            triumvirate.len(),
            all.iter().map(|(row, _)| *row).collect::<Vec<_>>(),
            data.resolver.rows.is_identity()
        ),
    )
}

/// The classes' level-up tables offer each of `rows` at the level its row's
/// class columns give: which rows no class offers.
fn offered(resolver: &Resolver, rows: &[(u16, String)]) -> (Vec<String>, BTreeMap<u16, Vec<u8>>) {
    let classes = session::classes::classes(&ResolverSource(resolver));
    let mut levels: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
    for class in &classes.classes {
        for &(level, row) in &class.powers {
            levels.entry(row).or_default().push(level);
        }
    }
    let never = rows
        .iter()
        .filter(|(row, _)| !levels.contains_key(row))
        .map(|(_, label)| label.clone())
        .collect();
    (never, levels)
}

/// The lowest level any class offers power `row` at.
fn offered_at(resolver: &Resolver, row: u16) -> Option<u8> {
    let (_, levels) = offered(resolver, &[]);
    levels.get(&row)?.iter().min().copied()
}

/// Each KOTOR 2 mod power is offered at level-up by at least one class.
fn kotor2_level_up(data: &Modded) -> Result<String, String> {
    let mut rows = rows_labelled(&data.resolver, HIGH_LEVEL)?;
    rows.extend(rows_labelled(&data.resolver, TRIUMVIRATE)?);
    let (never, levels) = offered(&data.resolver, &rows);
    let shown: Vec<String> = rows
        .iter()
        .map(|(row, label)| format!("{label} at {:?}", levels.get(row).cloned().unwrap_or_else(Vec::new)))
        .collect();
    check(never.is_empty(), format!("offered by no class: {never:?}; levels {shown:?}"))
}

/// The stage `KOTOR2_STAGE` on the patched folder, and the creature cast
/// at.
fn kotor2_stage(data: &Modded) -> Result<(Stage, usize), String> {
    let (module, tag) = KOTOR2_STAGE;
    let stage = Stage::new(&data.discovery, "tsl", module, &[tag.to_string()])?;
    let target = crate::scenes::tagged(&stage.level, &stage.start, tag)
        .into_iter()
        .next()
        .ok_or(format!("no {tag} in {module}"))?;
    Ok((stage, target))
}

/// Each High-Level Force Powers power cast alone at a guard on Telos by a
/// Jedi who knows only it, through the mod's own compiled script, against
/// the same scene without it. The routines the scripts called that kjk does
/// not carry out are listed.
fn high_level_casts(data: &Modded) -> Result<String, String> {
    let (stage, target) = kotor2_stage(data)?;
    let library = stage.level.scripts().ok_or("no scripts")?;
    let rows = rows_labelled(&data.resolver, HIGH_LEVEL)?;
    let before = sim::script::unbound::warned();
    let mut lines = Vec::new();
    let mut failed = Vec::new();
    for (row, label) in &rows {
        let script = library.impact(*row).unwrap_or("").to_string();
        // The Jedi is of the lowest level a class offers the power at.
        let level = offered_at(&data.resolver, *row);
        let scene = crate::force_verbs::lent_scene_at(&stage, target, *row, level);
        // Static Field acts on droids alone, and the guard is a human: it
        // must be paid for, and nothing more is judged.
        let judged = !label.contains("STATICFIELD");
        // The Jedi regains a hundredth of a Force point or so during the
        // cast, so a payment within one point of the cost is the cost.
        let works = scene.as_ref().is_some_and(|(seen, paid, cost, replayed)| {
            (!seen.is_empty() || !judged) && *paid + 100 >= *cost && *replayed
        });
        if !works {
            failed.push(label.clone());
        }
        lines.push(format!("{label} ({row}, {script}): {scene:?}"));
    }
    let unbound: Vec<String> = sim::script::unbound::warned()
        .into_iter()
        .filter(|(name, _)| !before.contains_key(name))
        .map(|(_, warning)| warning)
        .collect();
    check(
        failed.is_empty(),
        format!("not working {failed:?}; routines not carried out {unbound:?}; {}", lines.join("; ")),
    )
}

/// The creatures' minds read each KOTOR 2 mod power from its row as the
/// games do: a power with a `forcehostile` or `forcefriendly` cell is on
/// that menu, and its `category` is the talent category
/// `GetCreatureTalentBest` looks it up by. A row the mod left without a
/// menu cell is listed: no creature's mind picks it, in kjk or in the games.
/// A power whose impact script is missing is on no menu.
fn kotor2_minds(data: &Modded) -> Result<String, String> {
    let (stage, _) = kotor2_stage(data)?;
    let library = stage.level.scripts().ok_or("no scripts")?;
    let spells = data.resolver.two_da("spells").ok_or("no spells.2da")?;
    let mut rows = rows_labelled(&data.resolver, HIGH_LEVEL)?;
    rows.extend(rows_labelled(&data.resolver, TRIUMVIRATE)?);
    let mut wrong = Vec::new();
    let mut no_menu = Vec::new();
    for (row, label) in &rows {
        let at = usize::from(*row);
        let menu_cell = ["forcehostile", "forcefriendly"].iter().any(|column| spells.int(at, column).is_some());
        let category = spells.int(at, "category").unwrap_or(0);
        let menu = library.offered(*row) != sim::script::Menu::None;
        // kjk offers a power only its impact script carries out when the
        // script is there (`force::scripted`).
        let script = spells.resref(at, "impactscript").is_some_and(|name| data.resolver.find(&name, ResType::NCS).is_some());
        if menu != (menu_cell && script) || i64::from(library.spell(*row).category) != category {
            wrong.push(label.clone());
        }
        if !menu_cell {
            no_menu.push(label.clone());
        }
    }
    check(
        wrong.is_empty(),
        format!("{} rows; read otherwise than the table says {wrong:?}; the mod gave no menu to {no_menu:?}", rows.len()),
    )
}

/// High-Level Force Powers' clone template reads with its three AI
/// scripts, and its poison and Force shield rows are read.
fn high_level_pieces(data: &Modded) -> Result<String, String> {
    let source = ResolverSource(&data.resolver);
    let clone = session::creature_facts(&source, "st_clone").ok_or("st_clone.utc does not read")?;
    let missing: Vec<String> = clone
        .scripts
        .iter()
        .map(|(_, script)| script.clone())
        .filter(|script| !script.is_empty() && data.resolver.find(script, ResType::NCS).is_none())
        .collect();
    let named: Vec<String> = clone.scripts.iter().map(|(_, s)| s.clone()).filter(|s| s.starts_with("st_ai")).collect();
    let talents = session::talents(&source);
    let row = |table: &str, label: &str| -> Option<usize> { data.resolver.two_da(table)?.find("label", label) };
    let poison = row("poison", "ST_FORCE_POWER_CONTAGION");
    let shield = row("forceshields", "ST_FORCE_POWER_PROTECT");
    let read = poison.is_some_and(|row| talents.poisons.get(row).is_some_and(|course| course.points == 20))
        && shield.is_some_and(|row| talents.shields.get(row).is_some_and(|shield| shield.points == 140));
    check(
        clone.sheet.is_some() && named.len() == 3 && missing.is_empty() && read,
        format!(
            "st_clone has a sheet {}, AI scripts {named:?}, missing {missing:?}; poison row {poison:?} and \
             shield row {shield:?} read {read}",
            clone.sheet.is_some()
        ),
    )
}

/// Powers of the Triumvirate's placeables read with their heartbeat
/// scripts named.
fn triumvirate_pieces(data: &Modded) -> Result<String, String> {
    let mut lines = Vec::new();
    let mut good = true;
    for name in ["plc_deathaura", "plc_betrayal"] {
        let gff = data
            .resolver
            .read(name, ResType::from_extension("utp").ok_or("no utp type")?)
            .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok());
        let heartbeat = gff.as_ref().and_then(|gff| gff.root.resref("OnHeartbeat"));
        good &= heartbeat.is_some();
        lines.push(format!("{name} heartbeat {heartbeat:?}"));
    }
    check(good, lines.join("; "))
}

/// Powers of the Triumvirate's Nihilus Has Hunger option: Nihilus's module
/// opens from the option's `852NIH.mod` in place of the game's `.rim`
/// files, with the option's conversation.
fn nihilus_hunger(data: &Modded) -> Result<String, String> {
    let install = kjk_game::installs::open(&data.discovery, Game::Kotor2)?;
    let module = data.resolver.with_module(&install.index, "852nih").map_err(|e| e.to_string())?;
    let dialogue = module.find("852nih", ResType::from_extension("dlg").ok_or("no dlg type")?);
    let from = dialogue.map(|found| found.layer.name.clone());
    let from_mod = from.as_ref().is_some_and(|layer| layer.to_ascii_lowercase().ends_with("852nih.mod"));
    check(from_mod, format!("852nih.dlg from {from:?}"))
}

/// Which scripts of `rows`' impact scripts the folder has: a mod that ships
/// sources only gets its scripts from the patcher's compiler.
fn scripts_present(resolver: &Resolver, rows: &[(u16, String)]) -> Result<(Vec<String>, Vec<String>), String> {
    let spells = resolver.two_da("spells").ok_or("no spells.2da")?;
    let (mut present, mut absent) = (Vec::new(), Vec::new());
    for (row, label) in rows {
        let script = spells.resref(usize::from(*row), "impactscript").unwrap_or_else(String::new);
        match resolver.find(&script, ResType::NCS) {
            Some(_) => present.push(format!("{label}: {script}")),
            None => absent.push(format!("{label}: {script}")),
        }
    }
    Ok((present, absent))
}

/// Powers of the Triumvirate's impact scripts are in the folder. The mod
/// ships them as sources, so a folder patched without a compiler has none.
fn triumvirate_scripts(data: &Modded) -> Result<String, String> {
    let rows = rows_labelled(&data.resolver, TRIUMVIRATE)?;
    let (present, absent) = scripts_present(&data.resolver, &rows)?;
    check(absent.is_empty(), format!("compiled {present:?}; not compiled {absent:?}"))
}

/// The labels K2 Force Powers for K1 and Bastila Has TSL Battle Meditation
/// give their rows.
const KOTOR1_MOD_LABELS: [&str; 12] = [
    "FORCE_POWER_MASTER_HEAL",
    "FORCE_POWER_FORCE_BARRIER",
    "FORCE_POWER_IMPROVED_FORCE_BARRIER",
    "FORCE_POWER_DRAIN_FORCE",
    "FORCE_POWER_IMPROVED_DRAIN_FORCE",
    "FORCE_POWER_MASTER_DRAIN_FORCE",
    "FORCE_POWER_FORCE_SCREAM",
    "FORCE_POWER_IMPROVED_FORCE_SCREAM",
    "FORCE_POWER_MASTER_FORCE_SCREAM",
    "FORCE_POWER_REVITALIZE",
    "FORCE_POWER_IMPROVED_REVITALIZE",
    "FORCE_POWER_BATTLE_MEDITATION_PC",
];

/// The install rows past KOTOR 1's own 132 that carry the mods' labels,
/// with their labels.
fn kotor1_mod_rows(data: &Modded) -> Result<Vec<(u16, String)>, String> {
    let lent = data.resolver.lent.as_ref();
    let spells = data.resolver.two_da("spells").ok_or("no spells.2da")?;
    let map = data.resolver.rows.get(Table::Spells);
    let install = map.by_canonical();
    Ok((0..spells.cells.len())
        .filter_map(|number| {
            let label = spells.text(number, "label")?;
            let row = install.get(number).copied().flatten()?;
            let lent_row = lent.is_some_and(|lent| lent.spell_numbers.contains_key(&usize::from(row)));
            (usize::from(row) >= usize::from(Table::Spells.kotor1_rows())
                && !lent_row
                && KOTOR1_MOD_LABELS.contains(&label.as_str()))
            .then(|| (u16::try_from(number).ok(), label))
        })
        .filter_map(|(number, label)| Some((number?, label)))
        .collect())
}

/// The mods' rows each get a canonical number: a row of a label the engine
/// names gets that name's number, and the second row labelled Improved
/// Force Barrier (the mod's Master Force Barrier) is moved past KOTOR 2's
/// own rows. Each row is served with its name, description and icon.
fn kotor1_rows(data: &Modded) -> Result<String, String> {
    let rows = kotor1_mod_rows(data)?;
    let named = |label: &str| sim::combat::spells::NAMED.iter().find(|(name, _)| *name == label).map(|(_, n)| *n);
    let wrong: Vec<String> = rows
        .iter()
        .filter(|(row, label)| match named(label) {
            Some(number) => *row != number && *row < Table::Spells.kotor2_rows(),
            None => *row < Table::Spells.kotor2_rows(),
        })
        .map(|(row, label)| format!("{label} at {row}"))
        .collect();
    let missing = shown(&data.resolver, &rows)?;
    check(
        rows.len() == 13 && wrong.is_empty() && missing.is_empty(),
        format!("{} rows {rows:?}; at a wrong number {wrong:?}; missing {missing:?}", rows.len()),
    )
}

/// Each KOTOR 1 mod power is offered at level-up by KOTOR 1's Jedi classes.
fn kotor1_level_up(data: &Modded) -> Result<String, String> {
    let rows = kotor1_mod_rows(data)?;
    let (never, levels) = offered(&data.resolver, &rows);
    let shown: Vec<String> = rows
        .iter()
        .map(|(row, label)| format!("{label} at {:?}", levels.get(row).cloned().unwrap_or_else(Vec::new)))
        .collect();
    check(never.is_empty(), format!("offered by no class: {never:?}; levels {shown:?}"))
}

/// The names a player sees at level-up on KOTOR 1 with the mods and KOTOR
/// 2's loan, and which name shows on two rows. This check reports; it fails
/// only when a name shows on more than two rows.
fn shown_twice(data: &Modded) -> Result<String, String> {
    let spells = data.resolver.two_da("spells").ok_or("no spells.2da")?;
    let talk = data.resolver.talk.get().ok_or("no talk table")?;
    let (_, levels) = offered(&data.resolver, &[]);
    let mut names: BTreeMap<String, Vec<u16>> = BTreeMap::new();
    for &row in levels.keys() {
        let name = spells
            .int(usize::from(row), "name")
            .and_then(|strref| talk.text(strref as u32))
            .map(|text| String::from_utf8_lossy(text).into_owned())
            .unwrap_or_else(String::new);
        names.entry(name).or_default().push(row);
    }
    let twice: Vec<String> = names
        .iter()
        .filter(|(_, rows)| rows.len() > 1)
        .map(|(name, rows)| format!("{name} {rows:?}"))
        .collect();
    let thrice = names.values().any(|rows| rows.len() > 2);
    check(!thrice, format!("{} powers offered; names on two rows: {twice:?}", levels.len()))
}

/// Bastila's templates name the mod's Battle Meditation by its install row,
/// and her sheet knows it by its canonical number. The six templates the mod
/// puts in the Unknown World's module (`unk_m44ac.mod`) are read from that
/// `.mod`.
fn bastila_templates(data: &Modded) -> Result<String, String> {
    let install = kjk_game::installs::open(&data.discovery, Game::Kotor1)?;
    let row = rows_labelled(&data.resolver, "FORCE_POWER_BATTLE_MEDITATION_PC")?
        .first()
        .map(|(row, _)| *row)
        .ok_or("no Battle Meditation row")?;
    let module = data.resolver.with_module(&install.index, "unk_m44ac").map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    let mut good = row == sim::combat::spells::FORCE_POWER_BATTLE_MEDITATION_PC;
    let templates = std::iter::once(("p_bastilla".to_string(), &data.resolver))
        .chain((1..=6).map(|n| (format!("p_bastilla{n:03}"), &module)));
    for (name, resolver) in templates {
        let from = resolver
            .find(&name, ResType::UTC)
            .map(|found| found.layer.name.clone())
            .unwrap_or_else(String::new);
        let knows = session::creature_facts(&ResolverSource(resolver), &name)
            .and_then(|facts| facts.sheet)
            .is_some_and(|sheet| sheet.powers.has(row));
        good &= knows && (name == "p_bastilla" || from.to_ascii_lowercase().ends_with("unk_m44ac.mod"));
        lines.push(format!("{name} from {from} knows {row} {knows}"));
    }
    check(good, format!("Battle Meditation at {row}; {}", lines.join("; ")))
}

/// The KOTOR 1 mods' impact scripts are in the folder. Both mods ship them
/// as sources, so a folder patched without a compiler has none.
fn kotor1_scripts(data: &Modded) -> Result<String, String> {
    let rows = kotor1_mod_rows(data)?;
    let (present, absent) = scripts_present(&data.resolver, &rows)?;
    check(absent.is_empty(), format!("compiled {present:?}; not compiled {absent:?}"))
}

/// What one cast of a Triumvirate power did over a scene, read from its
/// snapshots by the creatures' ids (`triumvirate_scene`): each change as it
/// came, with the moment (ms after the scene's start).
#[derive(Debug, Default)]
struct Watched {
    /// The caster's hit points before the cast, and its most later.
    caster_hp: (i32, i32),
    /// The moments the target lost hit points, and its lowest.
    target_hurt: (Vec<u64>, i32),
    /// The creatures, by id, the caster's faction effect turned, with the
    /// first and last moment it ran on them.
    turned: BTreeMap<u32, (u64, u64)>,
    /// The first and last moment a placeable of the power's tag stood.
    placeable: Option<(u64, u64)>,
    /// The kinds of the power's lasting effects on the caster, and the
    /// effect icons the caster showed.
    caster_effects: std::collections::BTreeSet<String>,
    caster_icons: std::collections::BTreeSet<u16>,
    /// The visual effects played on the caster.
    caster_visuals: std::collections::BTreeSet<u16>,
    /// The kinds of lasting effects on the target, with the first and last
    /// moment each ran.
    target_effects: BTreeMap<String, (u64, u64)>,
    /// The mod's script runs the VM stopped at an argument of the wrong
    /// type.
    aborted: Vec<String>,
}

/// The level a Triumvirate power is cast at: the Sith prestige classes
/// pick each at level 24.
const TRIUMVIRATE_LEVEL: u8 = 24;

/// Where the Triumvirate's powers are cast: Telos's residential module,
/// whose room holds two TSF officers close together, both turned hostile,
/// and the one aimed at. Betrayal needs two enemies within 15 m.
const TRIUMVIRATE_STAGE: (&str, [&str; 2]) = ("203tel", ["203_tsf1", "203_tsf2"]);

/// A dark-sided Jedi of level 24 who knows only `row`, at half its hit
/// points (Pain heals what is missing, and Hunger's aura drains only for a
/// caster who misses hit points or Force points), with the Force points
/// the power costs, casts `row` at `stage`'s target, and the scene runs for
/// `ms`. `placeable` is the tag of the placeable the power makes.
fn triumvirate_scene(stage: &Stage, target: usize, row: u16, ms: u64, placeable: &str) -> Result<Watched, String> {
    use sim::combat::sheet::{Rows, flags};
    let mut caster = sim::combat::Sheet::dev_jedi();
    caster.level = TRIUMVIRATE_LEVEL;
    caster.powers = Rows::default().with(row);
    caster.flags |= flags::IMMORTAL | flags::TSL;
    caster.good_evil = 0;
    let mut start = stage.scene(target, 160.0, caster).ok_or("no scene")?;
    let me = crate::scenes::seat0(&start);
    let cost = stage
        .level
        .module()
        .and_then(|module| module.force_costs.get(usize::from(row)).copied())
        .map_or(0, |cost| i32::from(cost) * 100);
    if let Some(mine) = start.entities[me].fighter.as_mut() {
        mine.hp /= 2;
        mine.fp = mine.fp.max(cost);
    }
    let (my_id, target_id) = (start.entities[me].id, start.entities[target].id);
    let hp = |entity: Option<&sim::EntityState>| entity.and_then(|entity| entity.fighter).map_or(0, |f| f.hp / 100);
    let held = sim::buttons::FORCE_POWER;
    let run = stage.run(&start, target, ms, &mut |ms| (if ms < 1000 { held } else { 0 }, row, 0));
    let mut watched = Watched {
        caster_hp: (hp(by_id(&start, my_id)), hp(by_id(&start, my_id))),
        target_hurt: (Vec::new(), hp(by_id(&start, target_id))),
        aborted: run.aborted.iter().filter(|line| line.starts_with("m_")).cloned().collect(),
        ..Watched::default()
    };
    let mut last_target = hp(by_id(&start, target_id));
    for snapshot in &run.snapshots {
        let at = snapshot.time_ms - start.time_ms;
        let now = snapshot.time_ms;
        let mine = by_id(snapshot, my_id);
        watched.caster_hp.1 = watched.caster_hp.1.max(hp(mine));
        let theirs = hp(by_id(snapshot, target_id));
        if theirs < last_target {
            watched.target_hurt.0.push(at);
        }
        last_target = theirs;
        watched.target_hurt.1 = watched.target_hurt.1.min(theirs);
        if let Some(fighter) = mine.and_then(|entity| entity.fighter) {
            for lasting in fighter.effects.running(now).filter(|lasting| lasting.spell == row) {
                watched.caster_effects.insert(format!("{:?}", lasting.modifies));
            }
            watched.caster_icons.extend(fighter.effects.icons(now));
            watched.caster_visuals.extend(fighter.effects.visuals(now).map(|(visual, ..)| visual));
        }
        let objects = sim::Instances::in_snapshot(&stage.level, snapshot);
        let standing = snapshot.entities.iter().any(|entity| {
            matches!(entity.kind, sim::Kind::Object(sim::module::ObjectKind::Placeable, _))
                && crate::cast::is_tagged(entity, &objects, placeable)
        });
        if standing {
            let first = watched.placeable.map_or(at, |(first, _)| first);
            watched.placeable = Some((first, at));
        }
        let span = |spans: &mut (u64, u64)| spans.1 = at;
        for entity in snapshot.entities.iter().filter(|entity| entity.id != my_id) {
            let Some(fighter) = entity.fighter else {
                continue;
            };
            if entity.id == target_id && fighter.force.stunned_until > now {
                let held = format!("held {:?}", fighter.force.hold);
                span(watched.target_effects.entry(held).or_insert((at, at)));
            }
            for lasting in fighter.effects.running(now) {
                if lasting.modifies == sim::combat::effects::Modifies::Faction && lasting.by == my_id.0 {
                    span(watched.turned.entry(entity.id.0).or_insert((at, at)));
                }
                if entity.id == target_id {
                    span(watched.target_effects.entry(format!("{:?}", lasting.modifies)).or_insert((at, at)));
                }
            }
        }
    }
    Ok(watched)
}

/// The entity of `id` in `snapshot`, if it is there.
fn by_id(snapshot: &sim::Snapshot, id: sim::SimId) -> Option<&sim::EntityState> {
    snapshot.entities.iter().find(|entity| entity.id == id)
}

/// Each Powers of the Triumvirate power cast through its compiled script
/// by a level 24 dark-sided Jedi at a TSF officer on Telos, with another
/// hostile officer beside it, and what the mod's sources say it does.
/// Hunger makes its aura placeable, slows the caster with Hunger's icon,
/// and the aura's heartbeat hurts the officers near the caster each round
/// and heals the caster. Pain heals the caster and gives it a Will bonus
/// with Pain's icon. Betrayal turns an officer to the caster's side and
/// makes its placeable. A folder whose scripts are not compiled, or whose
/// scripts the VM stops at a wrong argument, fails here.
fn triumvirate_casts(data: &Modded) -> Result<String, String> {
    let rows = rows_labelled(&data.resolver, TRIUMVIRATE)?;
    let (_, absent) = scripts_present(&data.resolver, &rows)?;
    if !absent.is_empty() {
        return Err(format!("not compiled, so nothing is cast: {absent:?}"));
    }
    let (module, hostile) = TRIUMVIRATE_STAGE;
    let hostile: Vec<String> = hostile.iter().map(|tag| tag.to_string()).collect();
    let stage = Stage::new(&data.discovery, "tsl", module, &hostile)?;
    let target = crate::scenes::tagged(&stage.level, &stage.start, &hostile[0])
        .into_iter()
        .next()
        .ok_or(format!("no {} in {module}", hostile[0]))?;
    let library = stage.level.scripts().ok_or("no scripts")?;
    let heartbeats = ["m_htb_deathaura", "m_htb_betrayal"];
    let gathered: Vec<&str> = heartbeats.into_iter().filter(|name| library.program(name).is_some()).collect();
    let row = |label: &str| rows.iter().find(|(_, name)| name == label).map(|(row, _)| *row);
    let before = sim::script::unbound::warned();
    let (hunger, pain, betrayal) = (
        row("OTGM_FP_HUNGER").ok_or("no Hunger")?,
        row("OTGM_FP_PAIN").ok_or("no Pain")?,
        row("OTGM_FP_BETRAYAL").ok_or("no Betrayal")?,
    );
    let hungered = triumvirate_scene(&stage, target, hunger, 70_000, "plc_deathaura")?;
    let pained = triumvirate_scene(&stage, target, pain, 3_000, "plc_deathaura")?;
    let betrayed = triumvirate_scene(&stage, target, betrayal, 45_000, "plc_betrayal")?;
    let unbound: Vec<String> = sim::script::unbound::warned()
        .into_iter()
        .filter(|(name, _)| !before.contains_key(name))
        .map(|(_, warning)| warning)
        .collect();
    let works = [
        hungered.placeable.is_some_and(|(_, last)| last < 66_000)
            && hungered.caster_effects.contains("Speed")
            && hungered.caster_effects.contains("Icon")
            && !hungered.target_hurt.0.is_empty()
            && hungered.caster_hp.1 > hungered.caster_hp.0
            && hungered.aborted.is_empty(),
        pained.caster_hp.1 > pained.caster_hp.0
            && pained.caster_effects.contains("Save")
            && pained.caster_effects.contains("Icon")
            && pained.aborted.is_empty(),
        // The officer turned is turned back by `m_betrayal_fin` (about
        // 30 s), which holds it in fear, and the placeable's heartbeat then
        // takes the placeable away.
        betrayed.turned.values().any(|(_, last)| *last < 31_000)
            && betrayed.target_effects.contains_key("held Fear")
            && betrayed.placeable.is_some_and(|(_, last)| last < 44_000)
            && betrayed.aborted.is_empty(),
    ];
    check(
        works.iter().all(|works| *works) && gathered.len() == heartbeats.len(),
        format!(
            "Hunger {hungered:?}; Pain {pained:?}; Betrayal {betrayed:?}; works {works:?}; heartbeats in the \
             level's scripts {gathered:?}; routines not carried out {unbound:?}"
        ),
    )
}
