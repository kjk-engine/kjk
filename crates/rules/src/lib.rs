//! The rulebook: every number the game's rules use, with what it does, in
//! one place (`data/rules/*.toml`; the format is `build.rs`'s). The sim
//! reads each as a constant (`rules::combat::damage::PACE`); the pages
//! (`docs/RULEBOOK.md` by `cargo xtask rulebook`, the wiki by `kjk rules`,
//! the game's own descriptions) read the same book, so a page can never
//! say another number than the game plays. Changing a value is changing
//! the game: edit the book, build, play.

pub mod audit;
pub mod page;
pub mod shown;

/// One entry of the book: a section heading the rules inside it, or a rule.
#[derive(Debug)]
pub struct Entry {
    /// Its place in the book: `chapter.section.rule`.
    pub path: &'static str,
    /// Its depth (a chapter is 0).
    pub depth: usize,
    pub title: &'static str,
    /// What it does, every value it names filled in.
    pub summary: &'static str,
    pub detail: &'static str,
    /// A rule's value as a page shows it (an array's each entry).
    pub values: &'static [&'static str],
    /// What it applies to (`spell:FORCE_POWER_FORCE_PUSH`, `kind:weapon`): the
    /// pages about those things quote it.
    pub applies: &'static [&'static str],
    pub kind: Kind,
}

#[derive(Debug)]
pub enum Kind {
    Section,
    Rule {
        /// The element's Rust type.
        ty: &'static str,
        array: bool,
        unit: &'static str,
        /// An array's entries' names.
        labels: &'static [&'static str],
        /// Where the number comes from.
        source: &'static str,
        /// Where the code applies it.
        see: &'static str,
        /// The constant the code reads.
        constant: &'static str,
    },
}

impl Entry {
    /// The rule's whole value as the games would say it (`3 seconds`,
    /// `-6 / -6 / -4 / -2`); empty for a section.
    pub fn value(&self) -> String {
        match self.kind {
            Kind::Rule { unit, .. } => self
                .values
                .iter()
                .map(|value| shown::said(value, unit))
                .collect::<Vec<_>>()
                .join(" / "),
            Kind::Section => String::new(),
        }
    }

    /// The rule's value as the book writes it, with its unit (`3000 ms`);
    /// empty for a section.
    pub fn written(&self) -> String {
        match self.kind {
            Kind::Rule { unit, .. } => shown::with_unit(&self.values.join(" / "), unit),
            Kind::Section => String::new(),
        }
    }

    pub fn is_rule(&self) -> bool {
        matches!(self.kind, Kind::Rule { .. })
    }

    /// The chapter it belongs to (its path's first word).
    pub fn chapter(&self) -> &'static str {
        self.path.split('.').next().unwrap_or(self.path)
    }
}

/// A change the rulebook makes to one cell of a game's own table
/// (`data/rules/tables/<game>/<table>.toml`): the game reads the table so
/// changed (`vfs::Resolver`).
#[derive(Debug)]
pub struct TableChange {
    /// `k1` or `tsl`.
    pub game: &'static str,
    /// The 2DA's name, lowercase (`baseitems`).
    pub table: &'static str,
    /// The row: its `label` cell (ignoring case), or its number.
    pub row: &'static str,
    pub column: &'static str,
    /// The cell's new text.
    pub value: &'static str,
    pub why: &'static str,
}

/// The tables of `game` (`k1`, `tsl`) the rulebook changes, each once.
pub fn changed_tables(game: &str) -> Vec<&'static str> {
    let mut tables: Vec<&'static str> = TABLE_CHANGES
        .iter()
        .filter(|change| change.game == game)
        .map(|change| change.table)
        .collect();
    tables.dedup();
    tables
}

/// The entry at `path`, if the book has one.
pub fn entry(path: &str) -> Option<&'static Entry> {
    BOOK.iter().find(|entry| entry.path == path)
}

/// The entries under `path` (itself first), in the book's order.
pub fn under(path: &str) -> impl Iterator<Item = &'static Entry> + '_ {
    BOOK.iter().filter(move |entry| {
        entry.path == path
            || entry
                .path
                .strip_prefix(path)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

/// The entries that apply to `tag` (`spell:FORCE_POWER_FORCE_PUSH`, matched
/// ignoring case), in the book's order.
pub fn applying(tag: &str) -> impl Iterator<Item = &'static Entry> + '_ {
    BOOK.iter().filter(move |entry| {
        entry
            .applies
            .iter()
            .any(|applies| applies.eq_ignore_ascii_case(tag))
    })
}

include!(concat!(env!("OUT_DIR"), "/book.rs"));
