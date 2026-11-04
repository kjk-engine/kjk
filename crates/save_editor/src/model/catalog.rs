//! What an install holds, by the game's own names and icons: every item
//! (override's marked as mods), feat, Force power, class, skill and quest,
//! and a search over them.

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;
use fmt_odyssey::texture::Texture;
use fmt_odyssey::twoda::TwoDa;
use vfs::Game;

/// One row of a list: an item, feat, power, class, skill or quest.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Its row (feats, powers, classes, skills) or its index in the list
    /// (items, quests).
    pub key: u32,
    /// Its resref (an item's template, a quest's tag); for rows, the
    /// table's label.
    pub resref: String,
    /// Its name in the game's words; empty when the game gives none (then
    /// the resref is shown, never a made-up name).
    pub name: String,
    /// Its icon's texture name.
    pub icon: Option<String>,
    /// Its kind in words (an item's base item: "Lightsaber", "Armor").
    pub kind: String,
    /// A short stat: a weapon's damage, an armour's defence.
    pub detail: String,
    /// It comes from the install's `override` folder (a mod).
    pub modded: bool,
    /// The slots it may be equipped in (`INVENTORY_SLOT_*` bits), items
    /// only.
    pub slots: u32,
    /// Name and resref, lowercase: what search matches.
    pub lower: String,
}

impl Entry {
    pub fn new(key: u32, resref: &str, name: &str) -> Entry {
        Entry {
            key,
            resref: resref.to_string(),
            name: name.to_string(),
            icon: None,
            kind: String::new(),
            detail: String::new(),
            modded: false,
            slots: 0,
            lower: format!("{name} {resref}").to_lowercase(),
        }
    }

    /// Whether the game names it.
    pub fn named(&self) -> bool {
        !self.name.is_empty()
    }

    /// What a row shows first: its name, else its resref.
    pub fn label(&self) -> &str {
        if self.named() {
            &self.name
        } else {
            &self.resref
        }
    }
}

/// A quest as the journal has it: its tag (the entry's resref) and name,
/// and its stages.
#[derive(Debug, Clone, PartialEq)]
pub struct Quest {
    pub entry: Entry,
    pub stages: Vec<Stage>,
}

/// One journal entry of a quest.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub id: u32,
    pub text: String,
    /// It ends the quest.
    pub end: bool,
}

/// An icon decoded: its size and its pixels (RGBA, row by row).
#[derive(Debug, Clone, PartialEq)]
pub struct Icon {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// A creature as its template names and pictures it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Creature {
    pub name: String,
    pub portrait: Option<String>,
}

/// Everything an install holds that the editor names.
pub struct Catalog {
    pub game: Game,
    pub items: Vec<Entry>,
    pub feats: Vec<Entry>,
    pub powers: Vec<Entry>,
    pub classes: Vec<Entry>,
    pub skills: Vec<Entry>,
    pub quests: Vec<Quest>,
    /// What edits need of it (`edit::Data`): its items as scripts know
    /// them, its classes, and its files.
    pub data: edit::Data,
}

/// Text in the game's code page (Windows-1252, read as Latin-1), trimmed.
fn text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| char::from(byte))
        .collect::<String>()
        .trim()
        .to_string()
}

/// A talk-table line, empty when there is none.
fn line(source: &dyn Source, strref: u32) -> String {
    source
        .talk()
        .and_then(|tlk| tlk.text(strref).map(text))
        .unwrap_or_else(String::new)
}

fn table(source: &dyn Source, name: &str) -> Option<TwoDa> {
    TwoDa::parse(&source.read(name, ResType::TWO_DA)?).ok()
}

/// A table cell as a strref (none: `u32::MAX`).
fn strref_cell(table: &TwoDa, row: usize, column: &str) -> u32 {
    table
        .int(row, column)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(u32::MAX)
}

/// Every row of `name` the game names, as entries: its name from `column`,
/// its icon from `icon` (if any), and the rows `keep` keeps.
fn rows(
    source: &dyn Source,
    name: &str,
    icon: Option<&str>,
    keep: impl Fn(&TwoDa, usize) -> bool,
) -> Vec<Entry> {
    let Some(table) = table(source, name) else {
        return Vec::new();
    };
    (0..table.row_labels.len())
        .filter(|&row| keep(&table, row))
        .filter_map(|row| {
            let name = line(source, strref_cell(&table, row, "name"));
            if name.is_empty() {
                return None;
            }
            let label = table.text(row, "label").unwrap_or_else(String::new);
            let mut entry = Entry::new(row as u32, &label.to_lowercase(), &name);
            entry.icon = icon.and_then(|column| table.resref(row, column));
            Some(entry)
        })
        .collect()
}

impl Catalog {
    /// `game`'s install as `discovery` found it, named; why not when it is
    /// not there or does not open.
    pub fn load(discovery: &vfs::Discovery, game: Game) -> Result<Catalog, String> {
        let data = edit::Data::load(discovery, game)?;
        let source: &dyn Source = &data.source;
        let items = items(&data);
        let feats = rows(source, "feat", Some("icon"), |_, _| true);
        let powers = rows(source, "spells", Some("iconresref"), |table, row| {
            table.int(row, "usertype") == Some(1)
        });
        let classes = rows(source, "classes", None, |_, _| true);
        let skills = rows(source, "skills", Some("icon"), |_, _| true);
        let quests = session::journal::quests(source)
            .0
            .into_iter()
            .enumerate()
            .map(|(index, (tag, quest))| Quest {
                entry: Entry::new(index as u32, &tag, &line(source, quest.name)),
                stages: quest
                    .entries
                    .iter()
                    .map(|entry| Stage {
                        id: entry.id,
                        text: line(source, entry.text),
                        end: entry.end,
                    })
                    .collect(),
            })
            .collect();
        Ok(Catalog {
            game,
            items,
            feats,
            powers,
            classes,
            skills,
            quests,
            data,
        })
    }

    /// The entries of `list` whose name or resref holds every word of
    /// `query`, any case, in list order.
    pub fn search<'a>(list: &'a [Entry], query: &str) -> impl Iterator<Item = &'a Entry> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        list.iter()
            .filter(move |entry| words.iter().all(|word| entry.lower.contains(word.as_str())))
    }

    /// The texture `name` decoded (TPC, else TGA), if the install has it.
    pub fn icon(&self, name: &str) -> Option<Icon> {
        let source: &dyn Source = &self.data.source;
        let texture = source
            .read(name, ResType::TPC)
            .and_then(|bytes| Texture::parse_tpc(&bytes).ok())
            .or_else(|| {
                source
                    .read(name, ResType::TGA)
                    .and_then(|bytes| Texture::parse_tga(&bytes).ok())
            })?;
        let rgba = texture.rgba(0, 0)?;
        Some(Icon {
            size: [texture.width(), texture.height()],
            rgba,
        })
    }

    /// A creature template's name (`FirstName` and `LastName`) and portrait
    /// (`PortraitId` in `portraits.2da`); empty when the install lacks it.
    pub fn creature(&self, template: &str) -> Creature {
        let utc = (self.data.source.read(template, ResType::UTC))
            .and_then(|bytes| Gff::parse(&bytes).ok());
        utc.map_or(Creature::default(), |utc| {
            creature_of(&self.data.source, &utc)
        })
    }

    /// The same of a template's bytes: a party table keeps them in the
    /// save, for a companion whose template only one module holds.
    pub fn creature_from(&self, bytes: &[u8]) -> Creature {
        Gff::parse(bytes).map_or(Creature::default(), |utc| {
            creature_of(&self.data.source, &utc)
        })
    }

    /// The item whose template is `resref`.
    pub fn item(&self, resref: &str) -> Option<&Entry> {
        self.items.iter().find(|entry| entry.resref == resref)
    }
}

/// Every item the install holds, named from its template, with its icon
/// (`i` + its base item's class + `_` + its model variation), kind and a
/// stat, and whether it is a mod.
fn items(data: &edit::Data) -> Vec<Entry> {
    let source: &dyn Source = &data.source;
    let base = table(source, "baseitems");
    data.catalog
        .iter()
        .enumerate()
        .map(|(index, facts)| {
            let uti = source
                .read(&facts.template, ResType::UTI)
                .and_then(|bytes| Gff::parse(&bytes).ok());
            let name = uti
                .as_ref()
                .and_then(|uti| uti.root.locstring("LocalizedName"))
                .map_or(String::new(), |loc| told(source, loc));
            let mut entry = Entry::new(index as u32, &facts.template, &name);
            let row = usize::from(facts.base_item);
            if let Some(base) = &base {
                let variation = uti
                    .as_ref()
                    .and_then(|uti| uti.root.int("ModelVariation"))
                    .unwrap_or(1);
                entry.icon = base
                    .resref(row, "itemclass")
                    .map(|class| format!("i{class}_{variation:03}"));
                entry.kind = line(source, strref_cell(base, row, "name"));
            }
            entry.detail = detail(facts);
            entry.slots = facts.slots;
            entry.modded = data
                .source
                .0
                .find(&facts.template, ResType::UTI)
                .is_some_and(|found| found.layer.name.eq_ignore_ascii_case("override"));
            entry
        })
        .collect()
}

/// A weapon's damage or an armour's defence, short.
fn detail(facts: &sim::inventory::ItemFacts) -> String {
    if let Some(weapon) = &facts.weapon {
        let bonus = match weapon.damage_bonus {
            0 => String::new(),
            more => format!("{more:+}"),
        };
        format!("{}d{}{bonus}", weapon.dice, weapon.die)
    } else if let Some(armour) = &facts.armour {
        format!("def {}", armour.defence)
    } else {
        String::new()
    }
}

/// A localized string as the game shows it: its talk-table line, else its
/// own first string; empty when it has neither.
fn told(source: &dyn Source, loc: &fmt_odyssey::gff::LocString) -> String {
    let told = line(source, loc.strref);
    match loc.strings.first() {
        Some((_, bytes)) if told.is_empty() => text(bytes),
        _ => told,
    }
}

/// A creature template's (`utc`) name, `FirstName` and `LastName`, and
/// portrait, `PortraitId` in `source`'s `portraits.2da`.
fn creature_of(source: &dyn Source, utc: &Gff) -> Creature {
    let part = |label: &str| {
        utc.root
            .locstring(label)
            .map_or(String::new(), |loc| told(source, loc))
    };
    let name = [part("FirstName"), part("LastName")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let portrait = utc
        .root
        .int("PortraitId")
        .and_then(|row| usize::try_from(row).ok())
        .and_then(|row| table(source, "portraits")?.resref(row, "baseresref"));
    Creature { name, portrait }
}
