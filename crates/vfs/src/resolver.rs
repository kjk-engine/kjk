//! Which file a KOTOR resource name resolves to: the install's sources as an
//! ordered stack of layers, lowest first, where the highest layer holding a
//! name wins.
//!
//! The order follows the two references where they agree and is recorded in
//! the I01 journal where they differ:
//!
//! 1. `chitin.key` BIFs
//! 2. global RIMs in `rims/`, in the order below
//! 3. `texturepacks/swpc_tex_gui.erf`
//! 4. the texture pack for the chosen quality
//! 5. `lips/global.mod`, `lips/localization.mod`
//! 6. `patch.erf`
//! 7. `override/`, top level only
//! 8. the module: `.rim`, `_s.rim`, `.mod`, `lips/<module>_loc.mod`,
//!    `_dlg.erf`, later ones winning
//!
//! Stream folders (`streammusic/`, `streamsounds/`, `streamwaves/` or
//! `streamvoice/`, `movies/`) are not in the stack. The games' executables
//! build paths into each of them by name, so each is its own namespace,
//! searched recursively, and asked for by folder.
//!
//! Layers 1 to 7 are the same for every module. `Resolver::base` opens them
//! once; `with_module` stacks one module's archives on a base without
//! reading the base's archives again (its layers are shared).
//!
//! Over them all, the rulebook's layer (`RULEBOOK`): each table the
//! rulebook changes (`rules::TABLE_CHANGES`, `data/rules/tables/`), as the
//! layers below resolve it, with its cells changed, so every reader of a
//! 2DA reads the game's tables as the rulebook has them.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use fmt_odyssey::ResType;
use fmt_odyssey::tlk::Tlk;
use fmt_odyssey::twoda::TwoDa;

use crate::archive::Archive;
use crate::chitin::Chitin;
use crate::{DirIndex, Game};

const GLOBAL_RIMS: &[&str] = &[
    "mainmenu.rim",
    "mainmenudx.rim",
    "legal.rim",
    "legaldx.rim",
    "global.rim",
    "subglobaldx.rim",
    "miniglobaldx.rim",
    "globaldx.rim",
    "chargen.rim",
    "chargendx.rim",
];

const GLOBAL_LIPS: &[&str] = &["global.mod", "localization.mod"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureQuality {
    High,
    Medium,
    Low,
}

impl TextureQuality {
    fn pack(self) -> &'static str {
        match self {
            TextureQuality::High => "swpc_tex_tpa.erf",
            TextureQuality::Medium => "swpc_tex_tpb.erf",
            TextureQuality::Low => "swpc_tex_tpc.erf",
        }
    }
}

/// The rulebook's layer: the tables it changes (`stack_rulebook`).
pub const RULEBOOK: &str = "rulebook";

/// Where a resource's bytes are: a whole loose file, or a span of an
/// archive; or, for the rulebook's layer, held (`path` then names it).
#[derive(Debug, Clone)]
pub struct Location {
    pub path: PathBuf,
    pub offset: u64,
    pub size: u64,
    pub held: Option<Arc<[u8]>>,
}

impl Location {
    /// A span of a file on disk.
    pub fn on_disk(path: PathBuf, offset: u64, size: u64) -> Location {
        Location {
            path,
            offset,
            size,
            held: None,
        }
    }

    /// Fails without reading when the span runs past the file's end.
    pub fn read(&self) -> io::Result<Vec<u8>> {
        if let Some(held) = &self.held {
            return Ok(held.to_vec());
        }
        let mut file = File::open(&self.path)?;
        let len = file.metadata()?.len();
        if self.offset.saturating_add(self.size) > len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "{}+{} runs past the {len}-byte file",
                    self.offset, self.size
                ),
            ));
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = vec![0; self.size as usize];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

pub type ResKey = (String, ResType);

#[derive(Debug)]
pub struct Layer {
    /// Relative path of the source, e.g. `chitin.key` or `modules/end_m01aa.rim`.
    pub name: String,
    pub resources: BTreeMap<ResKey, Location>,
}

/// A source that exists but could not be read, or a loose file whose
/// extension is not a resource type. The layer is kept without it.
#[derive(Debug, Clone)]
pub struct LayerProblem {
    pub source: String,
    pub problem: String,
}

/// Cloning one is cheap: the layers are shared.
#[derive(Debug, Clone)]
pub struct Resolver {
    pub game: Game,
    pub layers: Vec<Arc<Layer>>,
    /// The stream folders, each its own namespace; never searched by `find`.
    pub streams: Vec<Arc<Layer>>,
    pub problems: Vec<LayerProblem>,
    /// The install's talk table (`dialog.tlk`), read when first asked for
    /// and shared by every resolver stacked on the same base.
    pub talk: TalkTable,
}

/// `dialog.tlk`: where it is, and once read, its strings (`None` when it
/// is missing or does not read).
#[derive(Debug, Clone, Default)]
pub struct TalkTable {
    path: Option<PathBuf>,
    read: Arc<OnceLock<Option<Arc<Tlk>>>>,
}

impl TalkTable {
    pub fn get(&self) -> Option<Arc<Tlk>> {
        self.read
            .get_or_init(|| {
                let bytes = std::fs::read(self.path.as_ref()?).ok()?;
                Tlk::parse(&bytes).ok().map(Arc::new)
            })
            .clone()
    }
}

#[derive(Debug)]
pub enum ResolveError {
    NotOdyssey(Game),
    Chitin(String),
    NoSuchModule(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotOdyssey(game) => write!(f, "{} is not a KOTOR game", game.key()),
            ResolveError::Chitin(error) => write!(f, "chitin.key: {error}"),
            ResolveError::NoSuchModule(module) => {
                write!(f, "no module {module}: no modules/{module}.rim or .mod")
            }
        }
    }
}

/// One resource after precedence: the winning layer and the layers it hides.
#[derive(Debug)]
pub struct Resolved<'a> {
    pub layer: &'a Layer,
    pub location: &'a Location,
    /// Lower layers holding the same name, highest first.
    pub shadowed: Vec<(&'a Layer, &'a Location)>,
}

impl Resolver {
    /// The install's layers, with `module`'s archives on top when given.
    pub fn open(
        game: Game,
        index: &DirIndex,
        module: Option<&str>,
        quality: TextureQuality,
    ) -> Result<Resolver, ResolveError> {
        let base = Resolver::base(game, index, quality)?;
        match module {
            Some(module) => base.with_module(index, module),
            None => Ok(base),
        }
    }

    /// The layers every module shares (1 to 7 above) and the stream
    /// folders.
    pub fn base(
        game: Game,
        index: &DirIndex,
        quality: TextureQuality,
    ) -> Result<Resolver, ResolveError> {
        if game == Game::JediAcademy {
            return Err(ResolveError::NotOdyssey(game));
        }
        let mut resolver = Resolver {
            game,
            layers: Vec::new(),
            streams: Vec::new(),
            problems: Vec::new(),
            talk: TalkTable {
                path: index.get("dialog.tlk").map(Path::to_path_buf),
                read: Arc::default(),
            },
        };

        let chitin =
            Chitin::load(index).map_err(|error| ResolveError::Chitin(error.to_string()))?;
        let mut resources = BTreeMap::new();
        for entry in &chitin.entries {
            let path = chitin.bifs[entry.bif]
                .path
                .clone()
                .expect("resolved entries have a BIF on disk");
            resources
                .entry((entry.resref.clone(), entry.restype))
                .or_insert(Location::on_disk(
                    path,
                    u64::from(entry.offset),
                    u64::from(entry.size),
                ));
        }
        for problem in &chitin.problems {
            resolver.problem("chitin.key", format!("{problem:?}"));
        }
        resolver.layers.push(Arc::new(Layer {
            name: "chitin.key".into(),
            resources,
        }));

        for rim in GLOBAL_RIMS {
            resolver.archive(index, &format!("rims/{rim}"));
        }
        resolver.archive(index, "texturepacks/swpc_tex_gui.erf");
        // Voice is `streamwaves` in K1's install and `streamvoice` in TSL's
        // (reone and xoreos name it by game); each install has only its
        // own, so whichever is there is mounted.
        let streams = [
            "streammusic",
            "streamsounds",
            "streamwaves",
            "streamvoice",
            "movies",
        ];
        for folder in streams {
            if let Some(layer) = resolver.folder(index, folder, true) {
                resolver.streams.push(Arc::new(layer));
            }
        }
        resolver.archive(index, &format!("texturepacks/{}", quality.pack()));
        for lips in GLOBAL_LIPS {
            resolver.archive(index, &format!("lips/{lips}"));
        }
        resolver.archive(index, "patch.erf");
        if let Some(layer) = resolver.folder(index, "override", false) {
            resolver.layers.push(Arc::new(layer));
        }
        resolver.stack_rulebook();
        Ok(resolver)
    }

    /// This resolver with `module`'s archives on top; its own layers are
    /// shared, not read again. Meant for a `base`: stacked on another
    /// module's resolver, both modules' archives are in the stack.
    pub fn with_module(&self, index: &DirIndex, module: &str) -> Result<Resolver, ResolveError> {
        let module = module.to_ascii_lowercase();
        let rim = format!("modules/{module}.rim");
        let mod_file = format!("modules/{module}.mod");
        if index.get(&rim).is_none() && index.get(&mod_file).is_none() {
            return Err(ResolveError::NoSuchModule(module));
        }
        let mut resolver = self.clone();
        resolver.archive(index, &rim);
        resolver.archive(index, &format!("modules/{module}_s.rim"));
        resolver.archive(index, &mod_file);
        resolver.archive(index, &format!("lips/{module}_loc.mod"));
        resolver.archive(index, &format!("modules/{module}_dlg.erf"));
        resolver.stack_rulebook();
        Ok(resolver)
    }

    /// The rulebook's layer on top, made again from what the layers below
    /// now resolve (a module's archives may hold a table of their own).
    /// A change naming a row or a column the table lacks is a problem.
    fn stack_rulebook(&mut self) {
        self.layers.retain(|layer| layer.name != RULEBOOK);
        self.problems.retain(|problem| problem.source != RULEBOOK);
        let game = self.game.key();
        let mut resources = BTreeMap::new();
        for table in rules::changed_tables(game) {
            let Some(mut parsed) = self.two_da(table) else {
                self.problem(
                    RULEBOOK,
                    format!("{table}.2da: changed, but not in the install"),
                );
                continue;
            };
            let changes = rules::TABLE_CHANGES
                .iter()
                .filter(|change| change.game == game && change.table == table);
            for change in changes {
                if let Err(problem) = change_cell(&mut parsed, change) {
                    self.problem(RULEBOOK, format!("{table}.2da: {problem}"));
                }
            }
            let bytes: Arc<[u8]> = parsed.write().into();
            let location = Location {
                path: PathBuf::from(format!("{RULEBOOK}/{table}.2da")),
                offset: 0,
                size: bytes.len() as u64,
                held: Some(bytes),
            };
            resources.insert((table.to_string(), ResType::TWO_DA), location);
        }
        if !resources.is_empty() {
            self.layers.push(Arc::new(Layer {
                name: RULEBOOK.into(),
                resources,
            }));
        }
    }

    fn problem(&mut self, source: &str, problem: String) {
        self.problems.push(LayerProblem {
            source: source.to_string(),
            problem,
        });
    }

    /// Adds an archive layer if the file exists; a missing one is no layer.
    fn archive(&mut self, index: &DirIndex, relative: &str) {
        let Some(path) = index.get(relative) else {
            return;
        };
        match Archive::open(path) {
            Ok(archive) => {
                let mut resources = BTreeMap::new();
                for entry in archive.entries {
                    resources
                        .entry((entry.resref, entry.restype))
                        .or_insert(Location::on_disk(
                            path.to_path_buf(),
                            u64::from(entry.offset),
                            u64::from(entry.size),
                        ));
                }
                self.layers.push(Arc::new(Layer {
                    name: relative.to_string(),
                    resources,
                }));
            }
            Err(error) => self.problem(relative, error),
        }
    }

    /// A folder of loose files as a layer, if it has any. Names are
    /// `resref.ext`.
    fn folder(&mut self, index: &DirIndex, relative: &str, recursive: bool) -> Option<Layer> {
        let prefix = format!("{relative}/");
        let files: Vec<(&str, &Path)> = index
            .iter()
            .filter_map(|(key, path)| Some((key.strip_prefix(&prefix)?, path)))
            .filter(|(rest, _)| recursive || !rest.contains('/'))
            .collect();
        if files.is_empty() {
            return None;
        }
        let mut resources = BTreeMap::new();
        for (rest, path) in files {
            let name = rest.rsplit('/').next().unwrap_or(rest);
            let Some((resref, extension)) = name.rsplit_once('.') else {
                self.problem(&format!("{prefix}{rest}"), "no extension".into());
                continue;
            };
            let Some(restype) = ResType::from_extension(extension) else {
                self.problem(
                    &format!("{prefix}{rest}"),
                    format!("unknown type .{extension}"),
                );
                continue;
            };
            let size = std::fs::metadata(path).map_or(0, |meta| meta.len());
            let key = (resref.to_string(), restype);
            if let Some(kept) = resources.get(&key) {
                let kept: &Location = kept;
                self.problem(
                    &format!("{prefix}{rest}"),
                    format!(
                        "same name as {}; the first in path order is kept",
                        kept.path.display()
                    ),
                );
                continue;
            }
            resources.insert(key, Location::on_disk(path.to_path_buf(), 0, size));
        }
        Some(Layer {
            name: relative.to_string(),
            resources,
        })
    }

    /// Every module of the install: each `.rim` or `.mod` under `modules/`
    /// by its name (`end_m01aa`), the `_s.rim` halves folded in.
    pub fn modules(index: &DirIndex) -> Vec<String> {
        let mut names: Vec<String> = index
            .iter()
            .filter_map(|(key, _)| {
                let name = key.strip_prefix("modules/")?;
                let stem = name
                    .strip_suffix(".rim")
                    .or_else(|| name.strip_suffix(".mod"))?;
                (!stem.ends_with("_s")).then(|| stem.to_string())
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Every layer once: this (shared) resolver's, then each module's own
    /// archives (`modules`). For walking every copy of every resource.
    pub fn every_layer(&self, index: &DirIndex) -> Vec<Arc<Layer>> {
        let mut layers = self.layers.clone();
        let mut seen: std::collections::BTreeSet<String> =
            layers.iter().map(|layer| layer.name.clone()).collect();
        for module in Resolver::modules(index) {
            if let Ok(resolver) = self.with_module(index, &module) {
                for layer in resolver.layers {
                    if seen.insert(layer.name.clone()) {
                        layers.push(layer);
                    }
                }
            }
        }
        layers
    }

    /// A name in one stream folder, e.g. `("streammusic", "mus_main", wav)`.
    pub fn stream(&self, folder: &str, resref: &str, restype: ResType) -> Option<&Location> {
        self.streams
            .iter()
            .find(|layer| layer.name.eq_ignore_ascii_case(folder))?
            .resources
            .get(&(resref.to_ascii_lowercase(), restype))
    }

    /// A voiced line's WAV: K1's `streamwaves`, TSL's `streamvoice`.
    pub fn voice(&self, resref: &str) -> Option<&Location> {
        let wav = ResType::WAV;
        ["streamwaves", "streamvoice"]
            .into_iter()
            .find_map(|folder| self.stream(folder, resref, wav))
    }

    pub fn find(&self, resref: &str, restype: ResType) -> Option<Resolved<'_>> {
        let key = (resref.to_ascii_lowercase(), restype);
        let mut holders = self
            .layers
            .iter()
            .rev()
            .filter_map(|layer| Some((&**layer, layer.resources.get(&key)?)));
        let (layer, location) = holders.next()?;
        Some(Resolved {
            layer,
            location,
            shadowed: holders.collect(),
        })
    }

    /// The resource's bytes, after precedence; `None` when it is missing or
    /// does not read.
    pub fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
        self.find(resref, restype)?.location.read().ok()
    }

    /// The 2DA `name`, read and parsed; `None` when it is missing or does
    /// not parse.
    pub fn two_da(&self, name: &str) -> Option<TwoDa> {
        TwoDa::parse(&self.read(name, ResType::TWO_DA)?).ok()
    }

    /// Every resref of `restype` in any layer, lowercase, once each.
    pub fn names(&self, restype: ResType) -> Vec<String> {
        self.names_in(restype, |_| true)
    }

    /// Every resref of `restype` in a module's own archives (`with_module`'s
    /// layers), lowercase, once each.
    pub fn module_names(&self, restype: ResType) -> Vec<String> {
        self.names_in(restype, |layer| {
            layer.name.starts_with("modules/") || layer.name.starts_with("lips/")
        })
    }

    fn names_in(&self, restype: ResType, keep: impl Fn(&Layer) -> bool) -> Vec<String> {
        let mut names: Vec<String> = self
            .layers
            .iter()
            .filter(|layer| keep(layer))
            .flat_map(|layer| layer.resources.keys())
            .filter(|(_, kind)| *kind == restype)
            .map(|(name, _)| name.to_ascii_lowercase())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Every name in every layer, resolved.
    pub fn all(&self) -> BTreeMap<&ResKey, Resolved<'_>> {
        let mut all = BTreeMap::new();
        for layer in self.layers.iter().rev().map(|layer| &**layer) {
            for (key, location) in &layer.resources {
                all.entry(key)
                    .and_modify(|resolved: &mut Resolved| resolved.shadowed.push((layer, location)))
                    .or_insert(Resolved {
                        layer,
                        location,
                        shadowed: Vec::new(),
                    });
            }
        }
        all
    }
}

/// `change` made in `table`: its row by its `label` cell (ignoring case),
/// else by number; its column by name.
pub fn change_cell(table: &mut TwoDa, change: &rules::TableChange) -> Result<(), String> {
    let row = table
        .find("label", change.row)
        .or_else(|| {
            change
                .row
                .parse::<usize>()
                .ok()
                .filter(|&row| row < table.cells.len())
        })
        .ok_or_else(|| format!("no row `{}`", change.row))?;
    let column = table
        .column(change.column)
        .ok_or_else(|| format!("no column `{}`", change.column))?;
    table.cells[row][column] = change.value.as_bytes().to_vec();
    Ok(())
}
