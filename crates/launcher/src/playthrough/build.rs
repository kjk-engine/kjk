//! A build (`--build FILE`): the character a run's new game makes through
//! character creation, and the plan its level-ups follow. A line a key and
//! its words, `#` starting a comment:
//!
//! - `class NAME`: `soldier`, `scout`, `scoundrel`, `guardian`, `consular`
//!   or `sentinel` (`classes.2da` rows 0 to 5);
//! - `sex male` or `female`, `portrait RESREF` (a player's, `portraits.2da`)
//!   and `name WORDS`;
//! - `abilities STR DEX CON INT WIS CHA`: the scores bought (creation's
//!   point-buy checks them);
//! - `skills NAME=RANK ...` and `feats LABEL ...`: the first level's ranks
//!   and feats (`feat.2da` labels; the game's picks for those left out);
//! - `skill-order NAME ...`, `feat-order LABEL ...`, `power-order LABEL ...`
//!   (`spells.2da` labels): each level-up's picks in that order, before the
//!   class's own (`sim::levels::Plan`);
//! - `bank`: the points `skill-order` cannot spend are kept for a later
//!   level instead of going in the class's order (`Plan::bank`);
//! - `raises ABILITY ...` (`str` to `cha`): the ability each fourth level
//!   raises, in turn;
//! - `wear LEVEL SLOT TEMPLATE`: from that level of the character on, the
//!   item it wears in a slot (`head`, `body`, `hands`, `right`, `left`,
//!   `left-arm`, `right-arm`, `implant`, `belt`); the highest level reached
//!   wins a slot. Read by a remade run only;
//! - `wear-on TAG LEVEL SLOT TEMPLATE`: the same for the companion of that
//!   tag, by the main character's level, put on when the companion is
//!   first in the party during a remade run.
//!
//! The creation is made only on a run that starts a new game (a module, not
//! a checkpoint), or on one that makes its checkpoint's character again
//! (`--remake`: the balance bench's reference character, with the `wear`
//! lines its level has reached); the plan picks every level the run takes.

use fmt_odyssey::twoda::TwoDa;
use sim::levels::Plan;
use ui::intent::CharacterDesc;

/// The classes a build names, by `classes.2da` row.
const CLASSES: [&str; 6] = [
    "soldier",
    "scout",
    "scoundrel",
    "guardian",
    "consular",
    "sentinel",
];

/// Ability names, strength to charisma (`sim::combat::sheet::Ability`).
const ABILITIES: [&str; 6] = ["str", "dex", "con", "int", "wis", "cha"];

/// A build read: what creation makes, and the level-ups' plan.
#[derive(Debug, Clone)]
pub struct Build {
    pub made: CharacterDesc,
    pub plan: Plan,
    /// `wear` lines: the level each starts at, its slot and its template.
    pub gear: Vec<(u8, u8, String)>,
    /// `wear-on` lines: a companion's tag, then as `gear`.
    pub companions: Vec<(String, (u8, u8, String))>,
}

/// The slots a `wear` line names (`sim::inventory::slots`).
const SLOTS: [(&str, u8); 9] = {
    use sim::inventory::slots;
    [
        ("head", slots::HEAD),
        ("body", slots::BODY),
        ("hands", slots::HANDS),
        ("right", slots::RIGHT_WEAPON),
        ("left", slots::LEFT_WEAPON),
        ("left-arm", slots::LEFT_ARM),
        ("right-arm", slots::RIGHT_ARM),
        ("implant", slots::IMPLANT),
        ("belt", slots::BELT),
    ]
};

impl Build {
    /// What a character of `level` wears by the `wear` lines: each slot's
    /// item of the highest level reached.
    pub fn worn(&self, level: u8) -> Vec<(&str, u8)> {
        Self::reached(self.gear.iter(), level)
    }

    /// What the companion of `tag` wears by the `wear-on` lines when the
    /// main character is of `level`.
    pub fn worn_by(&self, tag: &str, level: u8) -> Vec<(&str, u8)> {
        let lines = self
            .companions
            .iter()
            .filter(|(of, _)| of.eq_ignore_ascii_case(tag))
            .map(|(_, line)| line);
        Self::reached(lines, level)
    }

    fn reached<'a>(
        lines: impl Iterator<Item = &'a (u8, u8, String)>,
        level: u8,
    ) -> Vec<(&'a str, u8)> {
        let mut worn: Vec<(u8, &str, u8)> = Vec::new();
        for (from, slot, template) in lines {
            if *from > level {
                continue;
            }
            match worn.iter_mut().find(|(_, _, taken)| taken == slot) {
                Some(held) if held.0 <= *from => *held = (*from, template, *slot),
                Some(_) => {}
                None => worn.push((*from, template, *slot)),
            }
        }
        worn.into_iter().map(|(_, template, slot)| (template, slot)).collect()
    }
}

/// The row of `label` in `table` (its `label` column), any case.
fn row(table: &TwoDa, label: &str, of: &str) -> Result<u16, String> {
    (0..table.row_labels.len())
        .find(|&row| {
            table
                .text(row, "label")
                .is_some_and(|named| named.eq_ignore_ascii_case(label))
        })
        .map(|row| row as u16)
        .ok_or(format!("{label}: not a label of {of}.2da"))
}

fn skill(name: &str) -> Result<u8, String> {
    crate::by_hand::parse_skill(&format!("{name}=0")).map(|(row, _)| row as u8)
}

fn ability(name: &str) -> Result<u8, String> {
    ABILITIES
        .iter()
        .position(|of| of.eq_ignore_ascii_case(name))
        .map(|at| at as u8)
        .ok_or(format!("{name}: one of {}", ABILITIES.join(", ")))
}

/// `text`'s build, its feats and powers named by `feat.2da` and
/// `spells.2da`.
pub fn parse(text: &str, feats: &TwoDa, spells: &TwoDa) -> Result<Build, String> {
    let mut made = CharacterDesc {
        class: 0,
        female: false,
        portrait: String::new(),
        abilities: [0; 6],
        skills: [0; 8],
        name: String::new(),
        feats: Vec::new(),
    };
    let mut plan = Plan::default();
    let mut gear = Vec::new();
    let mut companions = Vec::new();
    let (mut classed, mut scored) = (false, false);
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        if key.is_empty() {
            continue;
        }
        let words: Vec<&str> = rest.split_whitespace().collect();
        let rows = |table: &TwoDa, of: &str| -> Result<Vec<u16>, String> {
            words.iter().map(|word| row(table, word, of)).collect()
        };
        let small = |of: fn(&str) -> Result<u8, String>| -> Result<Vec<u8>, String> {
            words.iter().map(|word| of(word)).collect()
        };
        match key {
            "class" => {
                let named = rest.trim();
                made.class = CLASSES
                    .iter()
                    .position(|class| class.eq_ignore_ascii_case(named))
                    .ok_or(format!("class {named}: one of {}", CLASSES.join(", ")))?
                    as u8;
                classed = true;
            }
            "sex" => {
                made.female = match rest.trim() {
                    "male" => false,
                    "female" => true,
                    other => return Err(format!("sex {other}: male or female")),
                }
            }
            "portrait" => made.portrait = rest.trim().to_ascii_lowercase(),
            "name" => made.name = rest.trim().to_string(),
            "abilities" => {
                let scores: Vec<u8> = words
                    .iter()
                    .map(|word| word.parse().map_err(|_| format!("abilities: {word}")))
                    .collect::<Result<_, _>>()?;
                made.abilities = scores
                    .try_into()
                    .map_err(|_| "abilities: six scores, STR to CHA".to_string())?;
                scored = true;
            }
            "skills" => {
                for word in &words {
                    let (row, rank) = crate::by_hand::parse_skill(word)?;
                    made.skills[row] = rank;
                }
            }
            "feats" => made.feats = rows(feats, "feat")?,
            "skill-order" => plan.skills = small(skill)?,
            "feat-order" => plan.feats = rows(feats, "feat")?,
            "power-order" => plan.powers = rows(spells, "spells")?,
            "raises" => plan.abilities = small(ability)?,
            "bank" => plan.bank = true,
            "wear" | "wear-on" => {
                let (tag, line) = match (key, &words[..]) {
                    ("wear", line) => (None, line),
                    (_, [tag, line @ ..]) => (Some(*tag), line),
                    _ => (None, &words[..]),
                };
                let [level, slot, template] = line[..] else {
                    return Err(format!("{key} {rest}: a level, a slot and a template"));
                };
                let level = level.parse().map_err(|_| format!("{key}: level {level}"))?;
                let slot = SLOTS
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(slot))
                    .map(|(_, slot)| *slot)
                    .ok_or(format!("{key}: slot {slot}"))?;
                let line = (level, slot, template.to_ascii_lowercase());
                match tag {
                    Some(tag) => companions.push((tag.to_string(), line)),
                    None => gear.push(line),
                }
            }
            other => return Err(format!("{other}: not a build key")),
        }
    }
    if !classed || !scored || made.portrait.is_empty() {
        return Err("a build names its class, abilities and portrait".into());
    }
    Ok(Build {
        made,
        plan,
        gear,
        companions,
    })
}
