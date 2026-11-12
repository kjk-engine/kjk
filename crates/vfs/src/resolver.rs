//! Which file a KOTOR resource name resolves to: the install's sources as an
//! ordered stack of layers, lowest first, where the highest layer holding a
//! name wins.
//!
//! The order is the executables' (`CExoResMan::GetKeyEntry`, read from
//! TSL's Mac executable: journal 2026-11-05-override-precedence). A lookup
//! searches the sources by kind (`Rank`), highest first, and among sources
//! of one kind the one added last:
//!
//! 1. `chitin.key` BIFs
//! 2. ERFs of priority 2: `texturepacks/swpc_tex_gui.erf`, the texture
//!    pack for the chosen quality, `lips/global.mod`,
//!    `lips/localization.mod`, then a module's `lips/<module>_loc.mod` and
//!    its `.mod` (or, without one, its `_dlg.erf`)
//! 3. RIMs: the global ones in `rims/`, in the order below, then a
//!    module's `_s.rim` and its `.rim` (neither when it has a `.mod`)
//! 4. ERFs of priority 1: `patch.erf`
//! 5. `override/`: its top level, then (TSL only) its subfolders one level
//!    down, in name order; the first file of a name wins. Over it, each
//!    Steam Workshop item's `override/`, its top level alone
//!
//! So `override/` beats everything a module brings, which is how the games
//! take a mod's DLG, NCS or UTC for a module's own.
//!
//! Stream folders (`streammusic/`, `streamsounds/`, `streamwaves/` or
//! `streamvoice/`, `movies/`) are not in the stack. The games' executables
//! build paths into each of them by name, so each is its own namespace,
//! searched recursively, and asked for by folder.
//!
//! The Workshop items TSL loads (`DirIndex::workshop`) rank in their load
//! order, the last one winning, as TSL's executable has them
//! (`LoadSteamMods`, `GetOverridePath`): their `override/` over the game's;
//! every copy of a module's `.mod`, lips and `_dlg.erf`, the game folder's
//! lowest; its `.rim` the last item's, but its `_s.rim` the first item's
//! (`CExoResMan::AsyncLoad` searches those from the first); and per file,
//! `dialog.tlk`, movies and the stream folders' the last item's that has
//! it, the game folder's when none has.
//!
//! The install's sources are the same for every module. `Resolver::base`
//! opens them once; `with_module` ranks one module's archives among a
//! base's without reading the base's archives again (its layers are shared).
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
use fmt_odyssey::texture::Texture;
use fmt_odyssey::tlk::Tlk;
use fmt_odyssey::twoda::TwoDa;
use fmt_odyssey::txi::Txi;

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

/// The types a stream folder's sound is: a WAV, or an MP3 (`.mp3`).
const SOUNDS: [ResType; 2] = [ResType::WAV, ResType::BMU];

/// The characters a resref holds (the archives' name fields).
const RESREF_LEN: usize = 16;

/// The keys `name` is looked up by: itself, lowercase, then, when it is
/// longer than a resref, its first 16 characters.
fn resrefs(name: &str) -> impl Iterator<Item = String> {
    let name = name.to_ascii_lowercase();
    let cut = (name.len() > RESREF_LEN)
        .then(|| name.get(..RESREF_LEN).map(str::to_string))
        .flatten();
    std::iter::once(name).chain(cut)
}

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

/// Whether `rest`, a lowercase path inside `override/`, is a file the games
/// read as a resource: within the depth `game` reads (TSL one subfolder
/// down, K1 the top level), and named as a resource type other than TXT.
/// A TXT there is a document (a mod's readme, Aspyr's controller settings,
/// whose line endings differ by platform); kjk reads none.
/// `fingerprint::covered` covers these alone, so co-op compares exactly
/// what the resolver serves from `override/`.
pub fn read_from_override(game: Game, rest: &str) -> bool {
    let within = match game {
        Game::Kotor2 => rest.matches('/').count() <= 1,
        _ => !rest.contains('/'),
    };
    within && resource_file(rest.rsplit('/').next().unwrap_or(rest))
}

/// Whether `rest`, a lowercase path inside a Workshop item's `override/`,
/// is a file the game reads: its top level alone (only a folder named
/// `OVERRIDE:` reads subfolders, `AddDirectoryContents`), as
/// `read_from_override` takes a file.
pub fn read_from_item_override(rest: &str) -> bool {
    !rest.contains('/') && resource_file(rest)
}

/// Whether a lowercase file name in an override folder is a resource the
/// games read: not a file browser's, and of a resource type other than TXT.
fn resource_file(name: &str) -> bool {
    !crate::system_file(name)
        && name
            .rsplit_once('.')
            .and_then(|(_, extension)| ResType::from_extension(extension))
            .is_some_and(|restype| restype != ResType::TXT)
}

/// A layer's name without its Workshop item's label: `modules/m01.mod` for
/// `workshop/485537937/modules/m01.mod`.
pub fn in_folder(name: &str) -> &str {
    name.strip_prefix("workshop/")
        .and_then(|rest| rest.split_once('/'))
        .map_or(name, |(_, rest)| rest)
}

/// How deep a folder layer reads (`Resolver::folder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    Top,
    /// The top level, then its subfolders' files (not theirs).
    OneLevel,
    All,
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
    pub rank: Rank,
    pub resources: BTreeMap<ResKey, Location>,
}

/// The kinds of source a lookup searches, lowest first: a higher rank
/// wins whatever was added when (`CExoResMan::GetKeyEntry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// `chitin.key`'s BIFs.
    Key,
    /// Texture packs, lips, a module's `.mod` and `_dlg.erf`.
    Erf2,
    /// The global RIMs, a module's `.rim` and `_s.rim`.
    Rim,
    /// `patch.erf`.
    Erf1,
    /// `override/`.
    Directory,
    /// The rulebook's tables, made from what everything below resolves.
    Rulebook,
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
    /// The file it is read from.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

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

impl Resolved<'_> {
    /// Whether the resource is a mod's. A mod's file is read from an
    /// override folder (the game's own or a Workshop item's), and it is
    /// not one of the files Aspyr's KOTOR 2 ships there itself
    /// (`STOCK_OVERRIDE`, and for its layouts their bytes too,
    /// `STOCK_LAYOUTS`). The screens place and colour a mod's layouts
    /// and art as KOTOR 2 does, and the game's own as kjk does.
    pub fn is_mod(&self) -> bool {
        if self.layer.rank != Rank::Directory {
            return false;
        }
        let name = self
            .location
            .path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        !stock_override(&name, self.location.size) || !stock_layout_bytes(&name, self.location)
    }
}

/// Aspyr's own layouts in its override folder, with the FNV-1a digest
/// (64 bits) of their bytes. A mod that only changes numbers in a layout
/// keeps its size (the Widescreen UI Fix's `equip_p` does), so a layout
/// of a stock name and size is Aspyr's only when its bytes are too.
const STOCK_LAYOUTS: &[(&str, u64)] = &[
    ("custpnl_p.gui", 0xae82_f2a8_fe2c_0339),
    ("equip_p.gui", 0x9d93_6c48_28d7_5cb5),
    ("mainmenu8x6_p.gui", 0x108a_5e6d_fceb_a244),
    ("optgraphics_p.gui", 0xab7a_be93_fdbd_559b),
    ("saveload_p.gui", 0x6540_bd44_9dfe_d654),
];

/// Whether a stock file named `name` holds Aspyr's bytes: always for a
/// file that is not one of Aspyr's layouts (`STOCK_LAYOUTS`) or cannot be
/// read, else when its digest is the stock one.
fn stock_layout_bytes(name: &str, location: &Location) -> bool {
    let name = name.to_ascii_lowercase();
    let Some(&(_, digest)) = STOCK_LAYOUTS.iter().find(|(stock, _)| *stock == name) else {
        return true;
    };
    location
        .read()
        .map_or(true, |bytes| fnv1a(&bytes) == digest)
}

/// The FNV-1a digest (64 bits) of `bytes`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The files Aspyr's KOTOR 2 ships in its `override/` folder, by lower-case
/// name and byte size. They are the game's own files (its pad art, larger
/// font sheets, a few layouts and models), though they sit where mods go,
/// so they are not taken for a mod's. A file of one of these names but of
/// another size is a mod's copy.
const STOCK_OVERRIDE: &[(&[&str], u64)] = &[
    (
        &[
            "cntrl_ps3_eng.tga",
            "cntrl_ps3_fre.tga",
            "cntrl_ps3_ger.tga",
            "cntrl_ps3_ita.tga",
            "cntrl_ps3_spa.tga",
            "cntrl_xb360_eng.tga",
            "cntrl_xb360_fre.tga",
            "cntrl_xb360_ger.tga",
            "cntrl_xb360_ita.tga",
            "cntrl_xb360_spa.tga",
        ],
        6_566_444,
    ),
    (
        &[
            "cus_button_a.tga",
            "cus_button_aps.tga",
            "cus_button_b.tga",
            "cus_button_bps.tga",
            "cus_button_x.tga",
            "cus_button_xps.tga",
            "cus_button_y.tga",
            "cus_button_yps.tga",
        ],
        35_388,
    ),
    (&["cus_gpad_bg.tga"], 1_432_844),
    (
        &[
            "cus_gpad_fper.tga",
            "cus_gpad_fper2.tga",
            "cus_gpad_gen.tga",
            "cus_gpad_gen2.tga",
            "cus_gpad_hand.tga",
            "cus_gpad_hand2.tga",
            "cus_gpad_help.tga",
            "cus_gpad_help2.tga",
            "cus_gpad_map.tga",
            "cus_gpad_map2.tga",
            "cus_gpad_save.tga",
            "cus_gpad_save2.tga",
            "cus_gpad_solo.tga",
            "cus_gpad_solo2.tga",
            "cus_gpad_solox.tga",
            "cus_gpad_solox2.tga",
            "cus_gpad_ste.tga",
            "cus_gpad_ste2.tga",
            "cus_gpad_ste3.tga",
        ],
        21_848,
    ),
    (&["custom.txt"], 4_884),
    (&["custpnl_p.gui"], 16_367),
    (&["d2xfnt_d16x16b.tga"], 4_194_348),
    (&["d2xfont16x16b.tga", "d2xfont16x16b_ps.tga"], 1_048_620),
    (&["d3xfnt_d16x16b.tga"], 16_777_260),
    (&["d3xfont16x16b.tga", "d3xfont16x16b_ps.tga"], 4_194_348),
    (&["diafnt16x16b_ps.tga", "dialogfont16x16b.tga"], 262_188),
    (&["equip_p.gui"], 37_553),
    (&["fx_step_splash.mdl"], 3_677),
    (&["gamepad.txt"], 501),
    (&["gui_scroll.wav"], 1_964),
    (&["handmaiden.dlg"], 1_372_903),
    (
        &["lbl_miscroll_open_f.tga", "lbl_miscroll_open_f2.tga"],
        31_196,
    ),
    (&["mainmenu8x6_p.gui"], 14_367),
    (&["optgraphics_p.gui"], 14_985),
    (&["saveload_p.gui"], 15_538),
    (&["v_damagetat.mdl"], 3_751),
    (&["w_rockpoisn.mdl"], 26_999),
];

/// Whether a loose file named `name` (any case) of `size` bytes is one of
/// the files Aspyr's KOTOR 2 ships in its override folder
/// (`STOCK_OVERRIDE`).
pub fn stock_override(name: &str, size: u64) -> bool {
    let name = name.to_ascii_lowercase();
    STOCK_OVERRIDE
        .iter()
        .any(|(names, bytes)| *bytes == size && names.contains(&name.as_str()))
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
                path: index
                    .copies("dialog.tlk")
                    .last()
                    .map(|copy| copy.path.to_path_buf()),
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
        resolver.push(Layer {
            name: "chitin.key".into(),
            rank: Rank::Key,
            resources,
        });

        for rim in GLOBAL_RIMS {
            resolver.archive(index, &format!("rims/{rim}"), Rank::Rim);
        }
        resolver.archive(index, "texturepacks/swpc_tex_gui.erf", Rank::Erf2);
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
            let mut copies = std::iter::once((String::new(), index))
                .chain(
                    index
                        .workshop()
                        .iter()
                        .map(|item| (format!("{}/", item.label()), &item.files)),
                )
                .filter_map(|(label, files)| resolver.folder(files, &label, folder, Depth::All));
            // Per file, the last item's copy, the game folder's lowest; a
            // sound is a `.wav` or an `.mp3` of its name, either replacing
            // the other below it.
            if let Some(mut layer) = copies.next() {
                for item in copies.collect::<Vec<_>>() {
                    for (resref, restype) in item.resources.keys() {
                        if SOUNDS.contains(restype) {
                            for sound in SOUNDS {
                                layer.resources.remove(&(resref.clone(), sound));
                            }
                        }
                    }
                    layer.resources.extend(item.resources);
                }
                layer.name = folder.to_string();
                resolver.streams.push(Arc::new(layer));
            }
        }
        resolver.archive(
            index,
            &format!("texturepacks/{}", quality.pack()),
            Rank::Erf2,
        );
        for lips in GLOBAL_LIPS {
            resolver.archive(index, &format!("lips/{lips}"), Rank::Erf2);
        }
        resolver.archive(index, "patch.erf", Rank::Erf1);
        // TSL reads `override/`'s subfolders one level down; K1 its top
        // level alone (`AddDirectoryContents`).
        let depth = if game == Game::Kotor2 {
            Depth::OneLevel
        } else {
            Depth::Top
        };
        if let Some(layer) = resolver.folder(index, "", "override", depth) {
            resolver.push(layer);
        }
        for item in index.workshop() {
            let label = format!("{}/", item.label());
            if let Some(layer) = resolver.folder(&item.files, &label, "override", Depth::Top) {
                resolver.push(layer);
            }
        }
        resolver.stack_rulebook();
        Ok(resolver)
    }

    /// This resolver with `module`'s archives ranked among its layers (below
    /// `override/` and `patch.erf`); its own layers are shared, not read
    /// again. Meant for a `base`: on another module's resolver, both
    /// modules' archives are in the stack.
    pub fn with_module(&self, index: &DirIndex, module: &str) -> Result<Resolver, ResolveError> {
        let module = module.to_ascii_lowercase();
        let rims = index.copies(&format!("modules/{module}.rim"));
        let mods = index.copies(&format!("modules/{module}.mod"));
        if rims.is_empty() && mods.is_empty() {
            return Err(ResolveError::NoSuchModule(module));
        }
        let mut resolver = self.clone();
        // The game folder's lips, then each item's, its module's and its
        // `localization.mod` (`CSWSModule::AddModuleResources`).
        let loc = format!("lips/{module}_loc.mod");
        resolver.archive(index, &loc, Rank::Erf2);
        for item in index.workshop() {
            for lips in [loc.as_str(), "lips/localization.mod"] {
                if let Some(path) = item.files.get(lips) {
                    let name = format!("{}/{lips}", item.label());
                    resolver.open_archive(name, path, Rank::Erf2);
                }
            }
        }
        // A `.mod` is the whole module: the games then read none of its
        // RIMs or its `_dlg.erf` (`CExoResMan::AsyncLoad`).
        if !mods.is_empty() {
            for copy in mods {
                resolver.open_archive(copy.name, copy.path, Rank::Erf2);
            }
        } else {
            for copy in index.copies(&format!("modules/{module}_dlg.erf")) {
                resolver.open_archive(copy.name, copy.path, Rank::Erf2);
            }
            // `AsyncLoad` searches the items from the first, then the game
            // folder.
            let s_rims = index.copies(&format!("modules/{module}_s.rim"));
            let s_rim = s_rims.iter().find(|copy| copy.item.is_some());
            if let Some(copy) = s_rim.or(s_rims.first()) {
                resolver.open_archive(copy.name.clone(), copy.path, Rank::Rim);
            }
            // `CServerExoAppInternal::LoadModule` searches from the last item.
            if let Some(copy) = rims.into_iter().last() {
                resolver.open_archive(copy.name, copy.path, Rank::Rim);
            }
        }
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
            self.push(Layer {
                name: RULEBOOK.into(),
                rank: Rank::Rulebook,
                resources,
            });
        }
    }

    fn problem(&mut self, source: &str, problem: String) {
        self.problems.push(LayerProblem {
            source: source.to_string(),
            problem,
        });
    }

    /// `layer` above every layer of its rank or lower, below every higher
    /// one: of one kind, the source added last wins.
    fn push(&mut self, layer: Layer) {
        let at = self
            .layers
            .iter()
            .position(|other| other.rank > layer.rank)
            .unwrap_or(self.layers.len());
        self.layers.insert(at, Arc::new(layer));
    }

    /// Adds the game folder's archive at `relative` as a layer if the file
    /// exists; a missing one is no layer.
    fn archive(&mut self, index: &DirIndex, relative: &str, rank: Rank) {
        if let Some(path) = index.get(relative) {
            self.open_archive(relative.to_string(), path, rank);
        }
    }

    /// Adds the archive at `path` as the layer `relative`.
    fn open_archive(&mut self, relative: String, path: &Path, rank: Rank) {
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
                self.push(Layer {
                    name: relative,
                    rank,
                    resources,
                });
            }
            Err(error) => self.problem(&relative, error),
        }
    }

    /// A folder of loose files as a layer, if it has any, as deep as
    /// `depth` reads, named `label` and `relative` (`label` names a
    /// Workshop item, `workshop/<id>/`, or is empty). Names are
    /// `resref.ext`; of two files of one name the first is kept: in path
    /// order, and for `Depth::OneLevel` the top level's before its
    /// subfolders'.
    fn folder(
        &mut self,
        index: &DirIndex,
        label: &str,
        relative: &str,
        depth: Depth,
    ) -> Option<Layer> {
        let prefix = format!("{relative}/");
        let shown = format!("{label}{prefix}");
        let mut files: Vec<(&str, &Path)> = index
            .iter()
            .filter_map(|(key, path)| Some((key.strip_prefix(&prefix)?, path)))
            .filter(|(rest, _)| match depth {
                Depth::Top => !rest.contains('/'),
                Depth::OneLevel => rest.matches('/').count() <= 1,
                Depth::All => true,
            })
            .collect();
        if depth == Depth::OneLevel {
            files.sort_by_key(|(rest, _)| rest.contains('/'));
        }
        if files.is_empty() {
            return None;
        }
        let mut resources = BTreeMap::new();
        for (rest, path) in files {
            let name = rest.rsplit('/').next().unwrap_or(rest);
            if crate::system_file(name) {
                continue;
            }
            let Some((resref, extension)) = name.rsplit_once('.') else {
                self.problem(&format!("{shown}{rest}"), "no extension".into());
                continue;
            };
            let Some(restype) = ResType::from_extension(extension) else {
                self.problem(
                    &format!("{shown}{rest}"),
                    format!("unknown type .{extension}"),
                );
                continue;
            };
            if relative == "override" && !read_from_override(self.game, rest) {
                continue;
            }
            let size = std::fs::metadata(path).map_or(0, |meta| meta.len());
            let key = (resref.to_string(), restype);
            if let Some(kept) = resources.get(&key) {
                let kept: &Location = kept;
                self.problem(
                    &format!("{shown}{rest}"),
                    format!("same name as {}, which is kept", kept.path.display()),
                );
                continue;
            }
            resources.insert(key, Location::on_disk(path.to_path_buf(), 0, size));
        }
        Some(Layer {
            name: format!("{label}{relative}"),
            rank: Rank::Directory,
            resources,
        })
    }

    /// Every module of the install: each `.rim` or `.mod` under `modules/`
    /// by its name (`end_m01aa`), the `_s.rim` halves folded in; the
    /// Workshop items' with the game folder's.
    pub fn modules(index: &DirIndex) -> Vec<String> {
        let mut names: Vec<String> = std::iter::once(index)
            .chain(index.workshop().iter().map(|item| &item.files))
            .flat_map(DirIndex::iter)
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

    /// The module whose own archives hold each resref of `restype` that
    /// they hold, the first by name where several do: where a template the
    /// shared data lacks is read once its module is left (an item carried
    /// out of the module that made it). Meant for a `base`.
    pub fn module_owners(
        &self,
        index: &DirIndex,
        restype: ResType,
    ) -> std::collections::BTreeMap<String, String> {
        let mut owners = std::collections::BTreeMap::new();
        for module in Resolver::modules(index) {
            let Ok(resolver) = self.with_module(index, &module) else {
                continue;
            };
            for name in resolver.module_names(restype) {
                owners.entry(name).or_insert_with(|| module.clone());
            }
        }
        owners
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

    /// A name in one stream folder, e.g. `("streammusic", "mus_main", wav)`;
    /// a WAV asked for may be an `.mp3` (TSL streams either).
    pub fn stream(&self, folder: &str, resref: &str, restype: ResType) -> Option<&Location> {
        let resources = &self
            .streams
            .iter()
            .find(|layer| layer.name.eq_ignore_ascii_case(folder))?
            .resources;
        let resref = resref.to_ascii_lowercase();
        resources.get(&(resref.clone(), restype)).or_else(|| {
            (restype == ResType::WAV)
                .then(|| resources.get(&(resref, ResType::BMU)))
                .flatten()
        })
    }

    /// A sound by resref as the games look one up: a WAV of the layers,
    /// else of any stream folder (music, sounds, voices), there a WAV or an
    /// MP3 (`stream`; a mod's new tracks are MP3s, TSLRCM's
    /// `mus_a_arriv.mp3`).
    pub fn sound(&self, resref: &str) -> Option<&Location> {
        let wav = ResType::WAV;
        self.find(resref, wav)
            .map(|found| found.location)
            .or_else(|| {
                self.streams
                    .iter()
                    .find_map(|layer| self.stream(&layer.name, resref, wav))
            })
    }

    /// A voiced line's WAV: K1's `streamwaves`, TSL's `streamvoice`.
    pub fn voice(&self, resref: &str) -> Option<&Location> {
        let wav = ResType::WAV;
        ["streamwaves", "streamvoice"]
            .into_iter()
            .find_map(|folder| self.stream(folder, resref, wav))
    }

    /// The resource of a name and type after precedence. A name longer than
    /// a resref (a model's texture name holds 32 characters) that nothing
    /// holds whole is looked up cut to a resref's 16, as the games turn
    /// every name into one.
    pub fn find(&self, resref: &str, restype: ResType) -> Option<Resolved<'_>> {
        resrefs(resref).find_map(|resref| self.find_exact(&resref, restype))
    }

    fn find_exact(&self, resref: &str, restype: ResType) -> Option<Resolved<'_>> {
        let key = (resref.to_string(), restype);
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

    /// The first of `restypes` the highest layer holding any of them has,
    /// with its type: a texture is a TPC or a TGA, and a TGA in `override/`
    /// replaces the texture pack's TPC, as the games load mods. A long name
    /// as `find` looks it up.
    pub fn find_any(&self, resref: &str, restypes: &[ResType]) -> Option<(ResType, Resolved<'_>)> {
        resrefs(resref).find_map(|resref| {
            let restype = self.layers.iter().rev().find_map(|layer| {
                restypes
                    .iter()
                    .copied()
                    .find(|&restype| layer.resources.contains_key(&(resref.clone(), restype)))
            })?;
            Some((restype, self.find_exact(&resref, restype)?))
        })
    }

    /// The texture the games would pick for a name, parsed: the highest
    /// layer's TPC or TGA (`find_any`). `None` when there is neither, the
    /// reason when the one there does not read. A TGA whose TXI says `cube`
    /// holds a cube map's six faces, stacked. One whose TXI makes it a
    /// flip-book (`proceduretype cycle`) holds a sheet of its frames
    /// (`Texture::parse_tga_frames`); a sheet that does not divide into
    /// them is read as the one image it is.
    pub fn read_texture(&self, name: &str) -> Option<Result<Texture, String>> {
        let (restype, found) = self.find_any(name, &[ResType::TPC, ResType::TGA])?;
        let bytes = found.location.read().map_err(|error| error.to_string());
        let parsed = bytes.and_then(|bytes| {
            let parsed = if restype == ResType::TPC {
                Texture::parse_tpc(&bytes)
            } else {
                let txi = self.read(name, ResType::TXI).map(|txi| Txi::parse(&txi));
                let cube = (txi.as_ref())
                    .is_some_and(|txi| txi.number("cube").is_some_and(|on| on != 0.0));
                match txi.as_ref().and_then(Txi::cycle_frames).filter(|_| !cube) {
                    Some((columns, rows)) => {
                        Texture::parse_tga_frames(&bytes, columns as usize, rows as usize)
                            .or_else(|_| Texture::parse_tga(&bytes))
                    }
                    None => Texture::parse_tga_layers(&bytes, if cube { 6 } else { 1 }),
                }
            };
            parsed.map_err(|error| error.to_string())
        });
        Some(parsed)
    }

    /// `read_texture`, `None` as well when it does not read.
    pub fn texture(&self, name: &str) -> Option<Texture> {
        self.read_texture(name)?.ok()
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
            let name = in_folder(&layer.name);
            name.starts_with("modules/") || name.starts_with("lips/")
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
