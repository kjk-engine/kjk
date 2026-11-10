//! The fighters a balance fight sets on each other: a creature of the
//! game's own template (as the arena calls one in), or a player character
//! of a Jedi class at a level, levelled as the game levels one.
//!
//! A player character is made as quick creation makes one (its first
//! class's recommended scores from `classes.2da`, `persona::quick`; its
//! first level taken from nothing as `creation::sheet` takes it, knowing no
//! powers, TSL's player feats granted), then levelled one level at a time
//! by the game's own picks (`Classes::planned`, KOTOR's automatic level up)
//! with the chosen style's feat line put first, as a player building for
//! it would (Flurry for fast, Critical Strike for medium, Power Attack for
//! strong, Two-Weapon Fighting for the pair and the staff). Its levels are
//! all in the Jedi class unless a base class is given for the first ones
//! (`:soldier6`, K1's road to the Jedi classes). It holds its game's
//! lightsaber items (`g_w_lghtsbr01`, both hands for the pair;
//! `g_w_dblsbr001`) as their templates make them, and wears nothing.

use sim::combat::Sheet;
use sim::combat::saber::Style;
use sim::combat::sheet::{factions, feats};
use sim::levels::{Career, Plan};

/// Who fights: a creature template, or a player character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Spec {
    Template(String),
    Character(Build),
}

/// A player character's build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Build {
    /// The class its levels are in (`classes.2da` row) and the total level.
    pub class: u8,
    pub level: u8,
    /// Its saber style, or the item template of the weapon it holds instead.
    pub style: Style,
    pub weapon: Option<String>,
    /// A base class (row) its first levels are in, and how many.
    pub before: Option<(u8, u8)>,
}

const CLASSES: [(&str, u8); 6] = [
    ("soldier", 0),
    ("scout", 1),
    ("scoundrel", 2),
    ("guardian", 3),
    ("consular", 4),
    ("sentinel", 5),
];
const BASES: [(&str, u8); 3] = [("soldier", 0), ("scout", 1), ("scoundrel", 2)];
const STYLES: [Style; 5] = [
    Style::Fast,
    Style::Medium,
    Style::Strong,
    Style::Dual,
    Style::Staff,
];

/// The saber items a style's hands hold, by template.
pub(super) fn saber_items(style: Style) -> [Option<&'static str>; 2] {
    match style {
        Style::Dual => [Some(SABER), Some(SABER)],
        Style::Staff => [Some(STAFF), None],
        _ => [Some(SABER), None],
    }
}
const SABER: &str = "g_w_lghtsbr01";
const STAFF: &str = "g_w_dblsbr001";

fn named<T: Copy>(list: &[(&str, T)], name: &str) -> Option<T> {
    list.iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
}

fn name_of<T: Copy + PartialEq>(list: &[(&'static str, T)], value: T) -> &'static str {
    list.iter()
        .find(|(_, known)| *known == value)
        .map_or("?", |(name, _)| *name)
}

impl Spec {
    /// `[jedi:]<class>:<level>:<fast|medium|strong|dual|staff|weapon item>[:<base><levels>]`
    /// (a class of both games' six: guardian, consular, sentinel, soldier,
    /// scout, scoundrel), else a template's resref.
    pub fn parse(text: &str) -> Result<Spec, String> {
        let usage = || {
            format!(
                "{text}: a template resref, or [jedi:]<guardian|consular|sentinel|soldier|scout|scoundrel>:<level>:<fast|medium|strong|dual|staff|a weapon item's template>[:soldier<n>|scout<n>|scoundrel<n>]"
            )
        };
        if text.is_empty() {
            return Err(usage());
        }
        if !text.contains(':') {
            return Ok(Spec::Template(text.to_ascii_lowercase()));
        }
        let rest = text.strip_prefix("jedi:").unwrap_or(text);
        let parts: Vec<&str> = rest.split(':').collect();
        let (class, level, style) = match parts[..] {
            [class, level, style] | [class, level, style, _] => (class, level, style),
            _ => return Err(usage()),
        };
        let class = named(&CLASSES, class).ok_or_else(usage)?;
        let level = level
            .parse::<u8>()
            .ok()
            .filter(|level| (1..=30).contains(level))
            .ok_or_else(usage)?;
        let saber = STYLES
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(style));
        let weapon = saber.is_none().then(|| style.to_ascii_lowercase());
        let before = match parts.get(3) {
            None => None,
            Some(base) => {
                let split = base
                    .find(|c: char| c.is_ascii_digit())
                    .ok_or_else(usage)?;
                let (name, count) = base.split_at(split);
                let row = named(&BASES, name).ok_or_else(usage)?;
                let count = count
                    .parse::<u8>()
                    .ok()
                    .filter(|count| *count < level)
                    .ok_or_else(usage)?;
                Some((row, count))
            }
        };
        Ok(Spec::Character(Build {
            class,
            level,
            style: saber.unwrap_or(Style::Medium),
            weapon,
            before,
        }))
    }

    /// Its name in the tables.
    pub fn name(&self) -> String {
        match self {
            Spec::Template(template) => template.clone(),
            Spec::Character(build) => {
                let base = build.before.map_or(String::new(), |(row, levels)| {
                    format!(":{}{levels}", name_of(&BASES, row))
                });
                format!(
                    "{}:{}:{}{base}",
                    name_of(&CLASSES, build.class),
                    build.level,
                    build.weapon.as_deref().unwrap_or(build.style.name())
                )
            }
        }
    }

    pub fn template(&self) -> Option<&str> {
        match self {
            Spec::Template(template) => Some(template),
            Spec::Character(_) => None,
        }
    }
}

/// The feat line a style's build picks first.
fn style_feats(style: Style) -> &'static [u16] {
    match style {
        Style::Fast => &feats::FLURRY,
        Style::Medium => &feats::CRITICAL_STRIKE,
        Style::Strong => &feats::POWER_ATTACK,
        Style::Dual | Style::Staff => &feats::TWO_WEAPON_FIGHTING,
    }
}

/// `build`'s sheet (see the module documentation): `scores` its first
/// class's recommended scores, `library` the game's classes and items.
pub(super) fn sheet(
    build: &Build,
    library: &sim::script::Library,
    scores: [u8; 6],
) -> Result<Sheet, String> {
    let classes = library.classes();
    let plan = Plan {
        feats: match build.weapon {
            Some(_) => Vec::new(),
            None => style_feats(build.style).to_vec(),
        },
        ..Plan::default()
    };
    let mut sheet = sim::creation::unmade(scores, factions::PLAYER);
    let mut career = Career::new();
    let mut banked = 0u8;
    for level in 1..=build.level {
        let class = match build.before {
            Some((base, levels)) if level <= levels => base,
            _ => build.class,
        };
        let mut choices = classes
            .planned(&sheet, &career, class, &plan, banked)
            .ok_or(format!("level {level} of class {class}: not offered"))?;
        if level == 1 {
            // Neither game's creation picks powers.
            choices.powers.clear();
        }
        (sheet, career, banked) = classes
            .take(&sheet, &career, &choices, banked)
            .ok_or(format!("level {level} of class {class}: refused"))?;
        if level == 1 {
            let granted = &classes.classes[usize::from(class)].pc_granted;
            for &feat in granted {
                if !sheet.feats.has(feat) {
                    sheet.feats.insert(feat);
                    sim::combat::passive::learn(&mut sheet, feat, 0);
                }
            }
        }
    }
    let items = match &build.weapon {
        Some(item) => [Some(item.as_str()), None],
        None => saber_items(build.style),
    };
    for (hand, item) in items.into_iter().enumerate() {
        sheet.weapons[hand] = match item {
            Some(template) => Some(
                library
                    .items()
                    .get(template)
                    .and_then(|item| item.weapon)
                    .ok_or(format!("{template}: no such weapon item"))?,
            ),
            None => None,
        };
    }
    Ok(sheet)
}

/// The recommended scores (strength to charisma) `classes.2da` gives
/// `class`, as quick creation reads them.
pub(super) fn scores(table: &fmt_odyssey::twoda::TwoDa, class: u8) -> [u8; 6] {
    ["str", "dex", "con", "int", "wis", "cha"].map(|column| {
        table
            .column(column)
            .and_then(|at| table.named(usize::from(class), at))
            .and_then(|score| score.parse().ok())
            .unwrap_or(sim::creation::LOWEST)
    })
}

/// The class whose scores a build starts with.
pub(super) fn first_class(build: &Build) -> u8 {
    build.before.map_or(build.class, |(base, _)| base)
}
