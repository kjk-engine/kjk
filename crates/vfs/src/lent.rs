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
//!   lacks (Battle Meditation, Force Scream, Revitalize and the rest).
//! - KOTOR 2's models, textures, icons and sounds those rows name, where
//!   KOTOR 1 has none of the name, in a layer under every other.
//!
//! Nothing else of KOTOR 2 is served. KOTOR 1 keeps its own rows, scripts,
//! tables and rules, and no check of whether the install is KOTOR 2's (its
//! `itemcreate.2da`) sees anything of KOTOR 2's.
//!
//! When KOTOR 1's `spells.2da` has rows past the ones it shares with KOTOR
//! 2's (a mod's powers), nothing is lent: KOTOR 2's rows would take those
//! rows' numbers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use fmt_odyssey::ResType;
use fmt_odyssey::ncs::{Const, Ncs, Op};
use fmt_odyssey::tlk::LENT_FLAG;
use fmt_odyssey::twoda::TwoDa;
use fmt_odyssey::txi::Txi;
use script_ncs::Catalog;

use crate::resolver::{Layer, Location, Rank, ResKey, Resolver};

/// The layer of the tables and the script KOTOR 1's data gets.
pub const LENT: &str = "kotor2-powers";
/// The layer of KOTOR 2's models, textures, icons and sounds the lent rows
/// name, under every other layer.
pub const LENT_FILES: &str = "kotor2-powers/files";

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
    /// How many rows of KOTOR 1's `spells.2da` it was made for: the rows
    /// KOTOR 1 and KOTOR 2 share.
    pub shared: usize,
    /// The lent `spells.2da` rows by number, their strrefs flagged and
    /// their impact script renamed.
    pub spells: BTreeMap<usize, Row>,
    /// The lent `visualeffects.2da` rows (by their labels, as the games
    /// look them up).
    effects: Vec<Row>,
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

/// A lent script's name in KOTOR 1's data (`SCRIPT_PREFIX`); none when it
/// would be longer than a resref.
fn lent_name(script: &str) -> Option<String> {
    let name = format!(
        "{SCRIPT_PREFIX}{}",
        script.strip_prefix("k_").unwrap_or(script)
    );
    (name.len() <= 16).then_some(name)
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

impl Lent {
    /// What `k1` (KOTOR 1's resolver) gets of `tsl`'s data. Refused, with
    /// the reason, when KOTOR 1's `spells.2da` has rows past the ones it
    /// shares with KOTOR 2's, or when either game's tables or routines do
    /// not read.
    pub fn new(k1: &Resolver, tsl: &Resolver) -> Result<Lent, String> {
        let k1_spells = k1.two_da("spells").ok_or("KOTOR 1 has no spells.2da")?;
        let tsl_spells = tsl.two_da("spells").ok_or("KOTOR 2 has no spells.2da")?;
        let label = |table: &TwoDa, row: usize| table.text(row, "label");
        let shared = (0..k1_spells.cells.len().min(tsl_spells.cells.len()))
            .take_while(|&row| label(&k1_spells, row) == label(&tsl_spells, row))
            .count();
        if k1_spells.cells.len() > shared {
            return Err(format!(
                "KOTOR 1's spells.2da has {} rows, and row {shared} on is not KOTOR 2's \
                 (a mod adds powers there), so KOTOR 2's powers are not added",
                k1_spells.cells.len()
            ));
        }
        let mut spells: BTreeMap<usize, Row> = (shared..tsl_spells.cells.len())
            .map(|row| (row, row_of(&tsl_spells, row)))
            .filter(|(_, row)| offered(row))
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
        for row in spells.values_mut() {
            let Some(script) = cell(row, "impactscript").map(str::to_ascii_lowercase) else {
                continue;
            };
            let name = lent_name(&script).ok_or(format!("{script}: name too long"))?;
            if !scripts.contains_key(&name) {
                if k1.find(&name, ResType::NCS).is_some() {
                    return Err(format!("KOTOR 1 already has a script called {name}"));
                }
                let bytes = tsl
                    .read(&script, ResType::NCS)
                    .ok_or(format!("KOTOR 2 has no {script}.ncs"))?;
                let mut ncs = Ncs::parse(&bytes).map_err(|error| format!("{script}: {error}"))?;
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

        // The names and descriptions, read from KOTOR 2's talk table.
        let mut strrefs = Vec::new();
        for row in spells.values_mut() {
            for column in ["name", "spelldesc"] {
                if let Some(strref) = cell(row, column).and_then(|text| text.parse::<u32>().ok()) {
                    strrefs.push(strref);
                    row.1
                        .insert(column.into(), (strref | LENT_FLAG).to_string().into_bytes());
                }
            }
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

        let files = Arc::new(files(k1, tsl, &spells, &effects));
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"kotor2-powers 1\n");
        for (number, (_, cells)) in &spells {
            hasher.update(format!("row {number}\n").as_bytes());
            for (column, value) in cells {
                hasher.update(column.as_bytes());
                hasher.update(b"=");
                hasher.update(value);
                hasher.update(b"\n");
            }
        }
        for (label, cells) in &effects {
            hasher.update(label);
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
            effects,
            routines,
            scripts,
            files,
            strrefs,
            digest: *hasher.finalize().as_bytes(),
        })
    }

    /// The rows lent, by number.
    pub fn rows(&self) -> Vec<u16> {
        self.spells
            .keys()
            .filter_map(|&row| u16::try_from(row).ok())
            .collect()
    }

    /// The layers it adds over `k1`'s (the resolver as it stands without
    /// them): the tables and scripts on top, the files at the bottom. A
    /// problem instead when `k1`'s `spells.2da` is not the one it was made
    /// for (a module's own table with other rows).
    pub(crate) fn layers(&self, k1: &Resolver) -> Result<(Layer, Arc<Layer>), String> {
        let mut spells = k1.two_da("spells").ok_or("no spells.2da")?;
        if spells.cells.len() != self.shared {
            return Err(format!(
                "spells.2da has {} rows here, not the {} KOTOR 2's powers were lent to",
                spells.cells.len(),
                self.shared
            ));
        }
        let last = self.spells.keys().last().copied().unwrap_or(self.shared);
        for number in self.shared..=last {
            let row = self.spells.get(&number);
            add_row(&mut spells, number.to_string().into_bytes(), row);
        }
        let mut resources = BTreeMap::new();
        let mut hold = |name: &str, restype: ResType, bytes: Vec<u8>| {
            let bytes: Arc<[u8]> = bytes.into();
            resources.insert((name.to_string(), restype), held(name, restype, bytes));
        };
        hold("spells", ResType::TWO_DA, spells.write());
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
        let layer = Layer {
            name: LENT.into(),
            rank: Rank::Lent,
            resources,
        };
        Ok((layer, self.files.clone()))
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

/// KOTOR 2's files the lent rows name that KOTOR 1 lacks: the icons, the
/// models (with their supermodels, the models they attach and their
/// emitters' chunks), the textures those draw with (and the ones their TXIs
/// name), and the sounds.
fn files(k1: &Resolver, tsl: &Resolver, spells: &BTreeMap<usize, Row>, effects: &[Row]) -> Layer {
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
