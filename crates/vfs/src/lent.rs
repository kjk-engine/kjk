//! KOTOR 2's Force powers lent to a KOTOR 1 install (`Resolver::lend`).
//!
//! A KOTOR 1 character may learn and cast the powers of KOTOR 2's
//! `spells.2da` that one of KOTOR 1's three Jedi classes can pick at some
//! level (`BASE_CLASS_COLUMNS`) and that KOTOR 1's table lacks. Each keeps
//! KOTOR 2's row number, Force point cost, levels, prerequisites, icon,
//! name and description. The powers are cast by KOTOR 2's own impact script,
//! so the resolver serves what that script needs beside KOTOR 1's data:
//!
//! - `spells.2da` with KOTOR 2's rows added after KOTOR 1's, under KOTOR 1's
//!   columns. A row's name and description are strrefs with
//!   `tlk::LENT_FLAG` set, which KOTOR 1's talk table reads from KOTOR 2's.
//!   A row's impact script is KOTOR 2's under another name
//!   (`SCRIPT_PREFIX`), so it never takes the place of KOTOR 1's own.
//! - The impact script, its calls of KOTOR 2's own routines numbered after
//!   KOTOR 1's (`nwscript.nss` gets those routines' declarations after its
//!   own). A routine both games declare alike keeps KOTOR 1's number, so
//!   each game's script calls the routine its own game declares.
//! - `visualeffects.2da` with the rows the script names that KOTOR 1's table
//!   lacks (Battle Meditation, Force Scream, Revitalize and the rest), and
//!   `effecticon.2da` with every row of KOTOR 2's table past KOTOR 1's own
//!   rows, each at KOTOR 2's number. The script picks many of its icons by
//!   a sum or a variable, not a number written in the call: Force Body
//!   shows row 76 plus its rank, and Force Scream keeps its row in a
//!   variable. So every row the script could show is lent, not only the
//!   numbers written into it.
//! - KOTOR 2's models, textures, icons and sounds those rows name, where
//!   KOTOR 1 has none of the name, in a layer under every other.
//!
//! KOTOR 2's feats are lent the same way (`lent_feat`). A KOTOR 1 character
//! may learn the feats of KOTOR 2's `feat.2da` past KOTOR 1's own rows that
//! one of KOTOR 1's classes can pick or is granted. Each keeps KOTOR 2's row
//! number, prerequisites, levels, icon, name and description, and the cells
//! of KOTOR 1's classes. KOTOR 2's Class Skill feats are the exception to
//! the class cells. KOTOR 1's classes have other class skills than KOTOR
//! 2's, so a KOTOR 1 class may pick a Class Skill feat exactly when its
//! skill is cross-class for that class in KOTOR 1's `skills.2da`
//! (`class_skill_lists`). KOTOR 1's table has no `_pc_granted` or companion
//! columns, and those cells are left out with the rest of KOTOR 2's columns.
//!
//! Nothing else of KOTOR 2 is served. KOTOR 1 keeps its own rows, scripts,
//! tables and rules, and no check of whether the install is KOTOR 2's (its
//! `itemcreate.2da`) sees anything of KOTOR 2's.
//!
//! A row is lent only when KOTOR 1's table has no row of its label. A mod
//! that brings KOTOR 2's Master Heal to KOTOR 1 keeps its own, and KOTOR 2's
//! is not lent a second time.
//!
//! Each lent row is placed in KOTOR 1's table at its KOTOR 2 number when
//! that number is past the table's own rows and past the row lent before
//! it. Otherwise it is placed right after the row lent before it, or after
//! the table's own rows for the first one. On a KOTOR 1 install without
//! mods every lent row is therefore at its KOTOR 2 number. On an install
//! whose mods added rows, the lent rows come after the mods' rows. The
//! rows-by-label map (`rules::rows`, `vfs::rows`) then gives each lent row
//! its KOTOR 2 number as its canonical number, wherever it is placed. A lent
//! row's cells that hold row numbers (`prerequisites`, `prereqfeat1`,
//! `successor` and the rest) name the install rows of those labels.
//!
//! KOTOR 2's own data may hold a mod's rows too, past its own 282. They
//! are lent like KOTOR 2's own when KOTOR 1's classes may pick them. A row
//! whose impact script KOTOR 2's data lacks (a mod that shipped its scripts
//! uncompiled) is not lent, and the rest of the loan stays whole. A script
//! whose name with `SCRIPT_PREFIX` is longer than a resref gets a shortened
//! name (`lent_name`).
//!
//! A table gets no rows only for a real failure, such as a table that does
//! not read, and the reason is kept (`Lent::refused`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use fmt_odyssey::ResType;
use fmt_odyssey::ncs::{Const, Ncs, Op};
use fmt_odyssey::tlk::LENT_FLAG;
use fmt_odyssey::twoda::TwoDa;
use fmt_odyssey::txi::Txi;
use script_ncs::Catalog;

use rules::rows::Table;

use crate::resolver::{Layer, Location, Rank, ResKey, Resolver};

/// The layer of the tables and the script KOTOR 1's data gets.
pub const LENT: &str = "kotor2-lent";
/// The layer of KOTOR 2's models, textures, icons and sounds the lent rows
/// name, under every other layer.
pub const LENT_FILES: &str = "kotor2-lent/files";

/// The feats of KOTOR 2's `feat.2da` left out by the owner's ruling,
/// although a KOTOR 1 class could have them: Droid Interface, which KOTOR
/// 2 grants its Expert Droid.
const FEATS_LEFT_OUT: [&str; 1] = ["DROID_INTERFACE"];

/// The label every Class Skill feat of KOTOR 2's `feat.2da` starts with.
/// The rest of the label is its skill's `skills.2da` label in capitals with
/// underscores (`CLASS_SKILL_COMPUTER_USE` is `ComputerUse`'s).
const CLASS_SKILL: &str = "CLASS_SKILL_";

/// A `<class>_list` cell: the class may pick the feat (`1`), or never
/// (`4`). Cells from 0 to 2 are picks and 3 a grant (`session::classes`).
const PICKED: &[u8] = b"1";
const NEVER: &[u8] = b"4";
const LAST_LISTED: i64 = 3;

/// The `spells.2da` columns of KOTOR 1's Jedi classes (Jedi Guardian,
/// Jedi Consular and Jedi Sentinel): the level each may pick a power from,
/// 255 never. KOTOR 2's prestige classes have columns of their own, which
/// KOTOR 1's table lacks.
pub const BASE_CLASS_COLUMNS: [&str; 3] = ["guardian", "consular", "sentinel"];

/// What a lent impact script is called in KOTOR 1's data: this before its
/// KOTOR 2 name, without the name's own `k_` (`k_sp1_generic` is
/// `k2_sp1_generic`).
pub const SCRIPT_PREFIX: &str = "k2_";

/// The `spells.2da` columns whose cells name a model.
const SPELL_MODELS: [&str; 7] = [
    "conjheadvisual",
    "conjhandvisual",
    "conjgrndvisual",
    "castheadvisual",
    "casthandvisual",
    "castgrndvisual",
    "projmodel",
];
/// The `spells.2da` columns whose cells name a sound.
const SPELL_SOUNDS: [&str; 5] = [
    "conjsoundvfx",
    "conjsoundmale",
    "conjsoundfemale",
    "castsound",
    "projsound",
];
/// The `visualeffects.2da` columns whose cells name a sound; the columns
/// whose names end in `_node` name models.
const EFFECT_SOUNDS: [&str; 3] = ["soundimpact", "soundduration", "soundcessastion"];
/// The keywords of a texture's TXI that name another texture.
const TXI_TEXTURES: [&str; 3] = ["envmaptexture", "bumpmaptexture", "bumpyshinytexture"];

/// A row as KOTOR 2 has it: its label and each cell by its column's name.
type Row = (Vec<u8>, BTreeMap<String, Vec<u8>>);

/// What KOTOR 1's data gets of KOTOR 2's (`Lent::new`).
#[derive(Debug)]
pub struct Lent {
    /// How many rows KOTOR 1's `spells.2da` had before the loan: its own
    /// rows and any mod's.
    pub shared: usize,
    /// The lent `spells.2da` rows by install row, their strrefs flagged and
    /// their impact script renamed. None when the powers were refused.
    pub spells: BTreeMap<usize, Row>,
    /// How many rows KOTOR 1's `feat.2da` had before the loan.
    pub feat_shared: usize,
    /// The lent `feat.2da` rows by install row, their strrefs flagged. None
    /// when the feats were refused.
    pub feats: BTreeMap<usize, Row>,
    /// Each lent `spells.2da` row's KOTOR 2 number, by install row.
    pub spell_numbers: BTreeMap<usize, u16>,
    /// Each lent `feat.2da` row's KOTOR 2 number, by install row.
    pub feat_numbers: BTreeMap<usize, u16>,
    /// Why a table got none of KOTOR 2's rows, one line a table.
    pub refused: Vec<String>,
    /// The lent `visualeffects.2da` rows (by their labels, as the games
    /// look them up).
    effects: Vec<Row>,
    /// The lent `effecticon.2da` rows by number (as the games look them
    /// up): KOTOR 2's rows past KOTOR 1's table, the icons the lent scripts
    /// show on a character (Force Barrier's, Force Body's, Force Scream's).
    icons: BTreeMap<usize, Row>,
    /// The declarations of KOTOR 2's routines added after KOTOR 1's.
    routines: String,
    /// The lent impact scripts' bytes by their names in KOTOR 1's data.
    scripts: BTreeMap<String, Arc<[u8]>>,
    /// KOTOR 2's files the rows name and KOTOR 1 lacks.
    files: Arc<Layer>,
    /// The KOTOR 2 strrefs of the rows' names and descriptions.
    pub strrefs: Vec<u32>,
    /// BLAKE3 of the rows, routines and scripts, and of the files' names:
    /// what two machines sharing a KOTOR 1 world must agree on.
    pub digest: [u8; 32],
}

/// Each row of `table` by its cells under their column names.
fn row_of(table: &TwoDa, row: usize) -> Row {
    let cells = table
        .columns
        .iter()
        .enumerate()
        .map(|(column, name)| {
            let cell = table.cells[row]
                .get(column)
                .cloned()
                .unwrap_or_else(Vec::new);
            (String::from_utf8_lossy(name).to_ascii_lowercase(), cell)
        })
        .collect();
    (table.row_labels[row].clone(), cells)
}

/// `row`'s cell under `column`, trimmed, when it is not blank.
fn cell<'a>(row: &'a Row, column: &str) -> Option<&'a str> {
    let text = std::str::from_utf8(row.1.get(column)?).ok()?.trim();
    (!text.is_empty() && text != "****").then_some(text)
}

/// Whether `resolver` reads script `resref` from the lent layer: one of
/// KOTOR 2's impact scripts, which speaks KOTOR 2's row numbers.
pub fn lent_script(resolver: &Resolver, resref: &str) -> bool {
    resolver
        .find(resref, ResType::NCS)
        .is_some_and(|found| found.layer.name == LENT)
}

/// A row's label, trimmed, as the tables compare labels.
fn label_of(table: &TwoDa, row: usize) -> Option<String> {
    let label = table.text(row, "label")?;
    let label = label.trim();
    (!label.is_empty() && label != "****").then(|| label.to_string())
}

/// A test of whether `table` has no row with a KOTOR 2 row's label. A mod
/// that brings a KOTOR 2 row to KOTOR 1 keeps its own row of that label.
fn unlabelled(table: &TwoDa) -> impl Fn(&Row) -> bool + use<> {
    let labels: BTreeSet<String> = (0..table.cells.len())
        .filter_map(|row| label_of(table, row))
        .collect();
    move |row| cell(row, "label").is_some_and(|label| !labels.contains(label))
}

/// The rows chosen for a loan (by KOTOR 2 number) placed in a table of
/// `rows` rows (the module comment's rule): the rows by install row, and
/// each install row's KOTOR 2 number.
fn place(
    rows: usize,
    chosen: BTreeMap<usize, Row>,
) -> (BTreeMap<usize, Row>, BTreeMap<usize, u16>) {
    let mut next = rows;
    let mut placed = BTreeMap::new();
    let mut numbers = BTreeMap::new();
    for (number, row) in chosen {
        let at = number.max(next);
        next = at + 1;
        placed.insert(at, row);
        numbers.insert(at, u16::try_from(number).unwrap_or(u16::MAX));
    }
    (placed, numbers)
}

/// Where the rows of one table a lent row's cells name are in KOTOR 1's
/// table: KOTOR 2's labels by KOTOR 2 number, KOTOR 1's labels by install
/// row, and each lent row's install row by its KOTOR 2 number.
struct Links {
    tsl: Vec<Option<String>>,
    k1: BTreeMap<String, usize>,
    lent: BTreeMap<u16, usize>,
}

impl Links {
    fn new(k1: &TwoDa, tsl: &TwoDa, numbers: &BTreeMap<usize, u16>) -> Links {
        let mut labels = BTreeMap::new();
        for row in 0..k1.cells.len() {
            if let Some(label) = label_of(k1, row) {
                labels.entry(label).or_insert(row);
            }
        }
        Links {
            tsl: (0..tsl.cells.len()).map(|row| label_of(tsl, row)).collect(),
            k1: labels,
            lent: numbers.iter().map(|(&at, &number)| (number, at)).collect(),
        }
    }

    /// The install row of KOTOR 2's row `number`: where it was lent to, or
    /// KOTOR 1's first row of its label. None when KOTOR 1 has neither.
    fn install_row(&self, number: usize) -> Option<usize> {
        let number16 = u16::try_from(number).ok()?;
        if let Some(&at) = self.lent.get(&number16) {
            return Some(at);
        }
        let label = self.tsl.get(number)?.as_ref()?;
        self.k1.get(label).copied()
    }
}

/// The columns of a lent `spells.2da` row whose cells hold rows of the
/// table named with them, several joined by `_`.
const SPELL_LINKS: [(&str, Table); 1] = [("prerequisites", Table::Spells)];
/// The columns of a lent `feat.2da` row whose cells hold rows.
const FEAT_LINKS: [(&str, Table); 9] = [
    ("prereqfeat1", Table::Feats),
    ("prereqfeat2", Table::Feats),
    ("successor", Table::Feats),
    ("orreqfeat0", Table::Feats),
    ("orreqfeat1", Table::Feats),
    ("orreqfeat2", Table::Feats),
    ("orreqfeat3", Table::Feats),
    ("orreqfeat4", Table::Feats),
    ("spellid", Table::Spells),
];

/// `row`'s cells under `columns`, each KOTOR 2 row number in them made the
/// install row of the same row (`Links::install_row`). A number KOTOR 1's
/// table has no row for is left out.
fn relink<'a>(
    row: &mut Row,
    columns: &[(&str, Table)],
    links: impl Fn(Table) -> Option<&'a Links>,
) {
    for &(column, table) in columns {
        let Some(text) = cell(row, column).map(str::to_string) else {
            continue;
        };
        let Some(links) = links(table) else {
            row.1.insert(column.into(), Vec::new());
            continue;
        };
        let numbers: Vec<String> = text
            .split('_')
            .filter_map(|number| links.install_row(number.trim().parse().ok()?))
            .map(|at| at.to_string())
            .collect();
        row.1.insert(column.into(), numbers.join("_").into_bytes());
    }
}

/// Whether KOTOR 1's Jedi classes may pick power `row` at some level: a
/// power the player picks (`usertype` 1), with a name, and a level under
/// one of `BASE_CLASS_COLUMNS` that is not 255.
fn offered(row: &Row) -> bool {
    let number = |column: &str| cell(row, column).and_then(|text| text.parse::<i64>().ok());
    number("usertype") == Some(1)
        && number("name").is_some_and(|name| name > 0)
        && BASE_CLASS_COLUMNS
            .iter()
            .any(|column| number(column).is_some_and(|level| (0..255).contains(&level)))
}

/// How long a resref may be.
const RESREF: usize = 16;

/// A lent script's name in KOTOR 1's data (`SCRIPT_PREFIX`). A name that
/// would be longer than a resref (a KOTOR 2 mod's `st_forcesapwill` is
/// `k2_st_forcesapwill`) keeps its first letters and ends in four letters of
/// the whole name's BLAKE3, so every machine names it alike. None for an
/// empty name.
fn lent_name(script: &str) -> Option<String> {
    let rest = script.strip_prefix("k_").unwrap_or(script);
    if rest.is_empty() {
        return None;
    }
    let name = format!("{SCRIPT_PREFIX}{rest}");
    if name.len() <= RESREF {
        return Some(name);
    }
    let hash = blake3::hash(name.as_bytes()).to_hex();
    let kept = RESREF - SCRIPT_PREFIX.len() - 4;
    Some(format!(
        "{SCRIPT_PREFIX}{}{}",
        &rest[..kept.min(rest.len())],
        &hash[..4]
    ))
}

/// A routine's name and types, which two games' declarations must share
/// for one number to serve both.
fn signature(routine: &script_ncs::Routine) -> String {
    let params: Vec<&str> = routine.params.iter().map(|p| p.ty.name()).collect();
    format!(
        "{} {}({})",
        routine.ret.name(),
        routine.name,
        params.join(",")
    )
}

/// The game's routine catalog.
fn catalog(resolver: &Resolver) -> Result<Catalog, String> {
    let bytes = resolver
        .read("nwscript", ResType::NSS)
        .ok_or("no nwscript.nss")?;
    let text: String = bytes.iter().map(|&byte| char::from(byte)).collect();
    Catalog::parse(&text).map_err(|error| error.to_string())
}

/// The powers' half of a loan (`Powers::new`): the `spells.2da` rows, the
/// visual effects their scripts name, the effect icons, KOTOR 2's routines
/// and the renamed impact scripts. Empty when the powers are refused.
#[derive(Debug, Default)]
struct Powers {
    shared: usize,
    spells: BTreeMap<usize, Row>,
    effects: Vec<Row>,
    icons: BTreeMap<usize, Row>,
    routines: String,
    scripts: BTreeMap<String, Arc<[u8]>>,
}

/// The `featstable` codes of KOTOR 1's classes (`classes.2da`), lowercase:
/// the prefixes of their `feat.2da` and `skills.2da` columns. KOTOR 1's
/// Minion shares the soldier's.
fn class_codes(k1: &Resolver) -> BTreeSet<String> {
    let Some(classes) = k1.two_da("classes") else {
        return BTreeSet::new();
    };
    (0..classes.cells.len())
        .filter_map(|row| classes.text(row, "featstable"))
        .map(|code| code.trim().to_ascii_lowercase())
        .filter(|code| !code.is_empty())
        .collect()
}

/// Whether KOTOR 2's feat `row` is lent to KOTOR 1: a feat in use (its
/// label not `XXX`), with a name and an icon (Complex Unarmed Animations,
/// a hidden flag, has none), not one of `FEATS_LEFT_OUT`, that one of
/// `codes` may pick or is granted (a `<class>_list` of 0 to 3). The Class
/// Skill feats are lent by `class_skill_lists` instead.
fn lent_feat(row: &Row, codes: &BTreeSet<String>) -> bool {
    let Some(label) = cell(row, "label") else {
        return false;
    };
    let number = |column: &str| cell(row, column).and_then(|text| text.parse::<i64>().ok());
    !label.starts_with("XXX")
        && !label.starts_with(CLASS_SKILL)
        && !FEATS_LEFT_OUT.contains(&label)
        && number("name").is_some_and(|name| name > 0)
        && cell(row, "icon").is_some()
        && codes.iter().any(|code| {
            number(&format!("{code}_list")).is_some_and(|list| (0..=LAST_LISTED).contains(&list))
        })
}

/// KOTOR 2's Class Skill feat `row` with the `<class>_list` cells of
/// `codes` set by KOTOR 1's `skills` table: picked by a class for which its
/// skill is cross-class, never by one for which it is a class skill. None
/// when it is no Class Skill feat, names no skill of KOTOR 1's table, or no
/// class of KOTOR 1 may pick it.
fn class_skill_lists(mut row: Row, codes: &BTreeSet<String>, skills: &TwoDa) -> Option<Row> {
    let skill_name = cell(&row, "label")?
        .strip_prefix(CLASS_SKILL)?
        .replace('_', "");
    let skill = (0..skills.cells.len()).find(|&skill| {
        skills
            .text(skill, "label")
            .is_some_and(|label| label.trim().eq_ignore_ascii_case(&skill_name))
    })?;
    let mut any = false;
    for code in codes {
        let own = skills.int(skill, &format!("{code}_class")) == Some(1);
        any |= !own;
        let list = if own { NEVER } else { PICKED };
        row.1.insert(format!("{code}_list"), list.to_vec());
    }
    any.then_some(row)
}

/// The rows of KOTOR 2's `feat.2da` lent to KOTOR 1 (`lent_feat`,
/// `class_skill_lists`), by KOTOR 2 number, and how many rows KOTOR 1's
/// table has. A row past KOTOR 1's own rows is lent when KOTOR 1's table
/// has no row of its label (`unlabelled`). Refused when a table does not
/// read or KOTOR 1's table is shorter than KOTOR 1's own.
fn feats(k1: &Resolver, tsl: &Resolver) -> Result<(usize, BTreeMap<usize, Row>), String> {
    let k1_feats = k1.two_da("feat").ok_or("KOTOR 1 has no feat.2da")?;
    let tsl_feats = tsl.two_da("feat").ok_or("KOTOR 2 has no feat.2da")?;
    let skills = k1.two_da("skills").ok_or("KOTOR 1 has no skills.2da")?;
    let shared = k1_feats.cells.len();
    let own = usize::from(Table::Feats.kotor1_rows());
    if shared < own {
        return Err(format!(
            "KOTOR 1's feat.2da has {shared} rows, fewer than KOTOR 1's own {own}"
        ));
    }
    let codes = class_codes(k1);
    let unlabelled = unlabelled(&k1_feats);
    let feats: BTreeMap<usize, Row> = (own..tsl_feats.cells.len())
        .filter_map(|number| {
            let row = row_of(&tsl_feats, number);
            if !unlabelled(&row) {
                return None;
            }
            if lent_feat(&row, &codes) {
                return Some((number, row));
            }
            class_skill_lists(row, &codes, &skills).map(|row| (number, row))
        })
        .collect();
    if feats.is_empty() {
        return Err("KOTOR 2's feat.2da has no feats KOTOR 1's classes may have".into());
    }
    Ok((shared, feats))
}

impl Lent {
    /// What `k1` (KOTOR 1's resolver) gets of `tsl`'s data: its powers
    /// (`Powers::new`) and its feats (`feats`), each refused alone with the
    /// reason kept in `refused`. Refused as a whole, with both reasons,
    /// when both are.
    pub fn new(k1: &Resolver, tsl: &Resolver) -> Result<Lent, String> {
        let mut refused = Vec::new();
        let powers = Powers::new(k1, tsl).unwrap_or_else(|why| {
            refused.push(why);
            Powers::default()
        });
        let (feat_shared, feats) = feats(k1, tsl).unwrap_or_else(|why| {
            refused.push(why);
            (0, BTreeMap::new())
        });
        if powers.spells.is_empty() && feats.is_empty() {
            return Err(refused.join("; "));
        }
        let Powers {
            shared,
            spells,
            effects,
            icons,
            routines,
            scripts,
        } = powers;

        // The rows placed in KOTOR 1's tables, and their cells that name
        // rows made to name KOTOR 1's rows.
        let (mut spells, spell_numbers) = place(shared, spells);
        let (mut feats, feat_numbers) = place(feat_shared, feats);
        let links = |table: Table, numbers: &BTreeMap<usize, u16>| {
            let (k1, tsl) = (k1.two_da(table.name())?, tsl.two_da(table.name())?);
            Some(Links::new(&k1, &tsl, numbers))
        };
        let (spell_links, feat_links) = (
            links(Table::Spells, &spell_numbers).filter(|_| !spells.is_empty()),
            links(Table::Feats, &feat_numbers).filter(|_| !feats.is_empty()),
        );
        let links = |table| match table {
            Table::Spells => spell_links.as_ref(),
            Table::Feats => feat_links.as_ref(),
        };
        for row in spells.values_mut() {
            relink(row, &SPELL_LINKS, links);
        }
        for row in feats.values_mut() {
            relink(row, &FEAT_LINKS, links);
        }

        // The names and descriptions, read from KOTOR 2's talk table.
        let mut strrefs = Vec::new();
        let named = spells
            .values_mut()
            .map(|row| (row, ["name", "spelldesc"]))
            .chain(feats.values_mut().map(|row| (row, ["name", "description"])));
        for (row, columns) in named {
            for column in columns {
                if let Some(strref) = cell(row, column).and_then(|text| text.parse::<u32>().ok()) {
                    strrefs.push(strref);
                    row.1
                        .insert(column.into(), (strref | LENT_FLAG).to_string().into_bytes());
                }
            }
        }

        let files = Arc::new(files(k1, tsl, (&spells, &effects, &icons), &feats));
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"kotor2-lent 3\n");
        for (table, rows, numbers) in [
            ("spells", &spells, &spell_numbers),
            ("feat", &feats, &feat_numbers),
        ] {
            for (number, (_, cells)) in rows {
                let kotor2 = numbers.get(number).copied().unwrap_or(u16::MAX);
                hasher.update(format!("{table} row {number} kotor2 {kotor2}\n").as_bytes());
                for (column, value) in cells {
                    hasher.update(column.as_bytes());
                    hasher.update(b"=");
                    hasher.update(value);
                    hasher.update(b"\n");
                }
            }
        }
        for (label, cells) in &effects {
            hasher.update(label);
            for value in cells.values() {
                hasher.update(value);
                hasher.update(b"\n");
            }
        }
        for (number, (_, cells)) in &icons {
            hasher.update(format!("icon {number}\n").as_bytes());
            for value in cells.values() {
                hasher.update(value);
                hasher.update(b"\n");
            }
        }
        hasher.update(routines.as_bytes());
        for (name, bytes) in &scripts {
            hasher.update(name.as_bytes());
            hasher.update(bytes);
        }
        for (name, restype) in files.resources.keys() {
            hasher.update(format!("{name}.{}\n", restype.0).as_bytes());
        }
        Ok(Lent {
            shared,
            spells,
            feat_shared,
            feats,
            spell_numbers,
            feat_numbers,
            refused,
            effects,
            icons,
            routines,
            scripts,
            files,
            strrefs,
            digest: *hasher.finalize().as_bytes(),
        })
    }

    /// The `spells.2da` rows lent, by number.
    pub fn rows(&self) -> Vec<u16> {
        Self::numbers(&self.spells)
    }

    /// The `feat.2da` rows lent, by number.
    pub fn feat_rows(&self) -> Vec<u16> {
        Self::numbers(&self.feats)
    }

    fn numbers(rows: &BTreeMap<usize, Row>) -> Vec<u16> {
        rows.keys()
            .filter_map(|&row| u16::try_from(row).ok())
            .collect()
    }
}

impl Powers {
    /// The powers' half of what `k1` gets of `tsl`'s data, its rows by
    /// KOTOR 2 number. A row past KOTOR 1's own rows is lent when KOTOR 1's
    /// Jedi may pick it (`offered`) and KOTOR 1's table has no row of its
    /// label (`unlabelled`). Refused, with the reason, when either game's
    /// tables or routines do not read, or KOTOR 1's table is shorter than
    /// KOTOR 1's own.
    fn new(k1: &Resolver, tsl: &Resolver) -> Result<Powers, String> {
        let k1_spells = k1.two_da("spells").ok_or("KOTOR 1 has no spells.2da")?;
        let tsl_spells = tsl.two_da("spells").ok_or("KOTOR 2 has no spells.2da")?;
        let shared = k1_spells.cells.len();
        let own = usize::from(Table::Spells.kotor1_rows());
        if shared < own {
            return Err(format!(
                "KOTOR 1's spells.2da has {shared} rows, fewer than KOTOR 1's own {own}"
            ));
        }
        let unlabelled = unlabelled(&k1_spells);
        let mut spells: BTreeMap<usize, Row> = (own..tsl_spells.cells.len())
            .map(|row| (row, row_of(&tsl_spells, row)))
            .filter(|(_, row)| offered(row) && unlabelled(row))
            .collect();
        if spells.is_empty() {
            return Err("KOTOR 2's spells.2da has no powers KOTOR 1 lacks".into());
        }

        // The routines: KOTOR 1's number for a routine declared alike,
        // else KOTOR 2's declaration added after KOTOR 1's.
        let (k1_catalog, tsl_catalog) = (catalog(k1)?, catalog(tsl)?);
        let known: BTreeMap<String, u16> = k1_catalog
            .routines
            .iter()
            .map(|routine| (signature(routine), routine.id))
            .collect();
        let mut routines = String::from("\n// KOTOR 2's routines its lent scripts call.\n");
        let mut next = u16::try_from(k1_catalog.routines.len()).map_err(|_| "too many routines")?;
        let numbers: Vec<u16> = tsl_catalog
            .routines
            .iter()
            .map(|routine| match known.get(&signature(routine)) {
                Some(&id) => id,
                None => {
                    routines.push_str(&format!("{routine};\n"));
                    next += 1;
                    next - 1
                }
            })
            .collect();

        // The impact scripts, renamed and renumbered, and the numbers they
        // hold (the visual effects they name among them).
        let mut scripts = BTreeMap::new();
        let mut constants = BTreeSet::new();
        // A row whose script KOTOR 2's data lacks, or whose script does not
        // read, is left out alone: a KOTOR 2 mod that shipped its scripts as
        // sources, uncompiled, keeps the rest of the loan whole.
        let mut left_out = Vec::new();
        for (&number, row) in spells.iter_mut() {
            let Some(script) = cell(row, "impactscript").map(str::to_ascii_lowercase) else {
                continue;
            };
            let name = lent_name(&script).ok_or(format!("{script}: no name"))?;
            if !scripts.contains_key(&name) {
                if k1.find(&name, ResType::NCS).is_some() {
                    return Err(format!("KOTOR 1 already has a script called {name}"));
                }
                let Some(Ok(mut ncs)) = tsl
                    .read(&script, ResType::NCS)
                    .map(|bytes| Ncs::parse(&bytes))
                else {
                    left_out.push(number);
                    continue;
                };
                for instruction in &mut ncs.code {
                    match &mut instruction.op {
                        Op::Action { routine, .. } => {
                            *routine = *numbers
                                .get(usize::from(*routine))
                                .ok_or(format!("{script}: routine {routine} is not KOTOR 2's"))?;
                        }
                        Op::Const(Const::Int(value)) => {
                            constants.insert(i64::from(*value));
                        }
                        _ => {}
                    }
                }
                scripts.insert(name.clone(), Arc::from(ncs.write()));
            }
            row.1.insert("impactscript".into(), name.into_bytes());
        }
        for number in left_out {
            spells.remove(&number);
        }
        if spells.is_empty() {
            return Err("none of KOTOR 2's powers KOTOR 1 lacks has a script that reads".into());
        }

        // The visual effects the scripts name that KOTOR 1 lacks.
        let labels = |table: &Option<TwoDa>| -> BTreeSet<i64> {
            table
                .iter()
                .flat_map(|table| &table.row_labels)
                .filter_map(|label| String::from_utf8_lossy(label).trim().parse().ok())
                .collect()
        };
        let (k1_effects, tsl_effects) = (k1.two_da("visualeffects"), tsl.two_da("visualeffects"));
        let k1_labels = labels(&k1_effects);
        let effects: Vec<Row> = tsl_effects
            .iter()
            .flat_map(|table| (0..table.cells.len()).map(move |row| row_of(table, row)))
            .filter(|(label, _)| {
                String::from_utf8_lossy(label)
                    .trim()
                    .parse::<i64>()
                    .is_ok_and(|label| constants.contains(&label) && !k1_labels.contains(&label))
            })
            .collect();

        // Every effect icon past KOTOR 1's table: the scripts work out many
        // of the rows they show (Force Body's 76 plus its rank), so the rows
        // written into them are not all the rows they show.
        let k1_icons = k1.two_da("effecticon").map_or(0, |table| table.cells.len());
        let icons: BTreeMap<usize, Row> = tsl
            .two_da("effecticon")
            .map(|table| {
                (k1_icons..table.cells.len())
                    .map(|number| (number, row_of(&table, number)))
                    .collect()
            })
            .unwrap_or_else(BTreeMap::new);

        Ok(Powers {
            shared,
            spells,
            effects,
            icons,
            routines,
            scripts,
        })
    }
}

impl Lent {
    /// The layers it adds over `k1`'s (the resolver as it stands without
    /// them): the tables and scripts on top, the files at the bottom. A
    /// problem instead when `k1`'s `spells.2da` or `feat.2da` is not the
    /// one it was made for (a module's own table with other rows).
    pub(crate) fn layers(&self, k1: &Resolver) -> Result<(Layer, Arc<Layer>), String> {
        let mut resources = BTreeMap::new();
        let mut hold = |name: &str, restype: ResType, bytes: Vec<u8>| {
            let bytes: Arc<[u8]> = bytes.into();
            resources.insert((name.to_string(), restype), held(name, restype, bytes));
        };
        for (table, shared, rows) in [
            ("spells", self.shared, &self.spells),
            ("feat", self.feat_shared, &self.feats),
        ] {
            let Some(&last) = rows.keys().last() else {
                continue;
            };
            let mut parsed = k1.two_da(table).ok_or(format!("no {table}.2da"))?;
            if parsed.cells.len() != shared {
                return Err(format!(
                    "{table}.2da has {} rows here, not the {shared} KOTOR 2's rows were lent to",
                    parsed.cells.len()
                ));
            }
            for number in shared..=last {
                add_row(
                    &mut parsed,
                    number.to_string().into_bytes(),
                    rows.get(&number),
                );
            }
            hold(table, ResType::TWO_DA, parsed.write());
        }
        if self.spells.is_empty() {
            return Ok((self.layer(resources), self.files.clone()));
        }
        if let Some(mut effects) = k1.two_da("visualeffects") {
            for (label, row) in &self.effects {
                add_row(
                    &mut effects,
                    label.clone(),
                    Some(&(label.clone(), row.clone())),
                );
            }
            hold("visualeffects", ResType::TWO_DA, effects.write());
        }
        if let (Some(mut table), Some(&last)) = (k1.two_da("effecticon"), self.icons.keys().last())
        {
            for number in table.cells.len()..=last {
                add_row(
                    &mut table,
                    number.to_string().into_bytes(),
                    self.icons.get(&number),
                );
            }
            hold("effecticon", ResType::TWO_DA, table.write());
        }
        let nwscript = k1.read("nwscript", ResType::NSS).ok_or("no nwscript.nss")?;
        hold(
            "nwscript",
            ResType::NSS,
            [nwscript, self.routines.as_bytes().to_vec()].concat(),
        );
        for (name, bytes) in &self.scripts {
            resources.insert(
                (name.clone(), ResType::NCS),
                held(name, ResType::NCS, bytes.clone()),
            );
        }
        Ok((self.layer(resources), self.files.clone()))
    }

    /// The lent tables' and scripts' layer of `resources`.
    fn layer(&self, resources: BTreeMap<ResKey, Location>) -> Layer {
        Layer {
            name: LENT.into(),
            rank: Rank::Lent,
            resources,
        }
    }
}

/// A resource whose bytes are held, named for the lent layer.
fn held(name: &str, restype: ResType, bytes: Arc<[u8]>) -> Location {
    Location {
        path: PathBuf::from(format!("{LENT}/{name}.{}", restype.0)),
        offset: 0,
        size: bytes.len() as u64,
        held: Some(bytes),
    }
}

/// `table` with a row labelled `label` added: `row`'s cells under the
/// table's columns of the same names, blank where it has none or there is
/// no row.
fn add_row(table: &mut TwoDa, label: Vec<u8>, row: Option<&Row>) {
    let cells = table
        .columns
        .iter()
        .map(|column| {
            let column = String::from_utf8_lossy(column).to_ascii_lowercase();
            row.and_then(|(_, cells)| cells.get(&column).cloned())
                .unwrap_or_else(Vec::new)
        })
        .collect();
    table.row_labels.push(label);
    table.cells.push(cells);
}

/// KOTOR 2's files the lent rows name that KOTOR 1 lacks: the powers' and
/// the feats' icons, the models (with their supermodels, the models they
/// attach and their emitters' chunks), the textures those draw with (and
/// the ones their TXIs name), and the sounds.
fn files(
    k1: &Resolver,
    tsl: &Resolver,
    (spells, effects, icons): (&BTreeMap<usize, Row>, &[Row], &BTreeMap<usize, Row>),
    feats: &BTreeMap<usize, Row>,
) -> Layer {
    let mut lending = Lending {
        k1,
        tsl,
        resources: BTreeMap::new(),
        seen: BTreeSet::new(),
    };
    for row in spells.values() {
        if let Some(icon) = cell(row, "iconresref") {
            lending.texture(icon);
        }
        for model in SPELL_MODELS.iter().filter_map(|column| cell(row, column)) {
            lending.model(model);
        }
        for sound in SPELL_SOUNDS.iter().filter_map(|column| cell(row, column)) {
            lending.sound(sound);
        }
    }
    for icon in icons.values().filter_map(|row| cell(row, "iconresref")) {
        lending.texture(icon);
    }
    for icon in feats.values().filter_map(|row| cell(row, "icon")) {
        lending.texture(icon);
    }
    for row in effects {
        for (column, _) in row.1.iter().filter(|(column, _)| column.ends_with("_node")) {
            if let Some(model) = cell(row, column) {
                lending.model(model);
            }
        }
        for sound in EFFECT_SOUNDS.iter().filter_map(|column| cell(row, column)) {
            lending.sound(sound);
        }
    }
    Layer {
        name: LENT_FILES.into(),
        rank: Rank::LentFiles,
        resources: lending.resources,
    }
}

/// The walk over what the lent rows name (`files`).
struct Lending<'a> {
    k1: &'a Resolver,
    tsl: &'a Resolver,
    resources: BTreeMap<ResKey, Location>,
    seen: BTreeSet<String>,
}

impl Lending<'_> {
    /// Whether `name` is new to the walk (each name is looked at once).
    fn first(&mut self, kind: &str, name: &str) -> Option<String> {
        let name = name.trim().to_ascii_lowercase();
        let fresh = !name.is_empty() && name != "null" && name != "****";
        (fresh && self.seen.insert(format!("{kind}:{name}"))).then_some(name)
    }

    /// KOTOR 2's copy of `name`'s `restype`, kept.
    fn keep(&mut self, name: &str, restype: ResType) -> bool {
        let Some(found) = self.tsl.find(name, restype) else {
            return false;
        };
        let location = found.location.clone();
        self.resources.insert((name.to_string(), restype), location);
        true
    }

    fn texture(&mut self, name: &str) {
        let Some(name) = self.first("texture", name) else {
            return;
        };
        if self
            .k1
            .find_any(&name, &[ResType::TPC, ResType::TGA])
            .is_some()
        {
            return;
        }
        let mut kept = false;
        for restype in [ResType::TPC, ResType::TGA, ResType::TXI] {
            kept |= self.keep(&name, restype);
        }
        if !kept {
            return;
        }
        let tsl = self.tsl;
        let txi = fmt_odyssey::texture::texture_txi(None, |restype| tsl.read(&name, restype));
        let named: Vec<String> = txi
            .iter()
            .flat_map(|txi: &Txi| TXI_TEXTURES.iter().filter_map(|key| txi.get(key)))
            .filter_map(|words| words.first().cloned())
            .collect();
        for texture in named {
            self.texture(&texture);
        }
    }

    fn model(&mut self, name: &str) {
        let Some(name) = self.first("model", name) else {
            return;
        };
        if self.k1.find(&name, ResType::MDL).is_some() || !self.keep(&name, ResType::MDL) {
            return;
        }
        self.keep(&name, ResType::MDX);
        let (Some(mdl), mdx) = (
            self.tsl.read(&name, ResType::MDL),
            self.tsl.read(&name, ResType::MDX).unwrap_or_else(Vec::new),
        ) else {
            return;
        };
        let Ok(model) = fmt_odyssey::mdl::Model::parse(&mdl, &mdx) else {
            return;
        };
        if let Some(supermodel) = &model.supermodel {
            self.model(supermodel);
        }
        let mut nodes = vec![&model.root];
        nodes.extend(model.animations.iter().map(|clip| &clip.root));
        while let Some(node) = nodes.pop() {
            if let Some(mesh) = &node.mesh {
                for texture in &mesh.textures {
                    self.texture(texture);
                }
            }
            if let Some(emitter) = &node.emitter {
                self.texture(&emitter.texture);
                self.model(&emitter.chunk);
            }
            if let Some(reference) = &node.reference {
                self.model(&reference.model);
            }
            nodes.extend(&node.children);
        }
    }

    fn sound(&mut self, name: &str) {
        let Some(name) = self.first("sound", name) else {
            return;
        };
        if self.k1.sound(&name).is_some() {
            return;
        }
        if let Some(location) = self.tsl.sound(&name) {
            self.resources
                .insert((name, ResType::WAV), location.clone());
        }
    }
}
