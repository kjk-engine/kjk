//! Encounter overrides (I28): our own data, one text file per module
//! (`data/encounters/<game>/<module>.encounter`, built in), that change a
//! module's creatures for real-time and co-op play — never written into
//! the game install. Applied to the module's facts when it is planned, for
//! the players driving a body when it is planned (I30 P21;
//! `ModuleData::players`), so every machine installs the same encounter
//! (the module digest covers it).
//!
//! One change per line, `#` to the end of a line is a comment; a line may
//! end with a condition on the players, `if players >= 2` (also `<=`, `==`):
//!
//! ```text
//! hp <tag> <whole HP> | x<factor>     every creature tagged: its maximum
//! move <tag> <x> <y> <z> [<yaw°>]     the first so tagged, metres
//! add <tag> <new tag> <x> <y> <z> [<yaw°>]  a copy of the first so tagged
//! add <tag> <new tag> by <dx> <dy> <dz>      the same, offset from it
//! remove <tag>                         every creature so tagged
//! faction <tag> hostile|friendly|neutral|player
//! flag <tag> +plot|-plot|+min1hp|-min1hp
//! per_player hp x<factor>              creatures hostile to the player:
//!                                      each player past the first adds
//!                                      the factor of their maximum
//! ```
//!
//! Tags are matched without case. An added creature goes at the end of the
//! module's placements (a placement's number is its entity in the level's
//! collision, so nothing before it may move); a removed one stays in its
//! place as a waypoint. A creature changed is no longer its template as it
//! is: its placement keeps its own facts.

use sim::combat::Sheet;
use sim::combat::sheet::{factions, flags};
use sim::module::{ModuleData, ObjectData, ObjectKind, Placement};

include!(concat!(env!("OUT_DIR"), "/encounters.rs"));

/// A change, and the players it applies for.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub line: usize,
    pub what: What,
    pub when: Option<(Compare, u8)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    AtLeast,
    AtMost,
    Exactly,
}

#[derive(Debug, Clone, PartialEq)]
pub enum What {
    Hp {
        tag: String,
        hp: Hp,
    },
    Move {
        tag: String,
        at: [f32; 3],
        yaw: Option<f32>,
    },
    Add {
        tag: String,
        new_tag: String,
        at: [f32; 3],
        yaw: Option<f32>,
        /// `at` is an offset from the copied creature.
        by: bool,
    },
    Remove {
        tag: String,
    },
    Faction {
        tag: String,
        faction: u8,
    },
    Flag {
        tag: String,
        flag: u8,
        on: bool,
    },
    PerPlayerHp {
        factor: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hp {
    Whole(u16),
    Times(f32),
}

/// The changes built in for `module` of `game`; none without a file.
pub fn built_in(game: &str, module: &str) -> Result<Vec<Change>, String> {
    let Some((_, _, text)) = FILES
        .iter()
        .find(|(g, m, _)| g.eq_ignore_ascii_case(game) && m.eq_ignore_ascii_case(module))
    else {
        return Ok(Vec::new());
    };
    parse(text).map_err(|error| format!("data/encounters/{game}/{module}.encounter: {error}"))
}

/// The share of their maximum each player past the first adds to the
/// hostile (`per_player hp`), for what scales by players later than the
/// plan (a minigame's gunners, M11): the first such line holding for any
/// count; 0 without one.
pub fn per_player_hp(changes: &[Change]) -> f32 {
    changes
        .iter()
        .find_map(|change| match change.what {
            What::PerPlayerHp { factor } if change.when.is_none() => Some(factor),
            _ => None,
        })
        .unwrap_or(0.0)
}

/// Every built-in file: (game, module).
pub fn all() -> impl Iterator<Item = (&'static str, &'static str)> {
    FILES.iter().map(|&(game, module, _)| (game, module))
}

fn number(word: &str, line: usize) -> Result<f32, String> {
    word.parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("line {line}: {word:?} is not a number"))
}

fn factor(word: &str, line: usize) -> Result<f32, String> {
    word.strip_prefix('x')
        .map(|rest| number(rest, line))
        .unwrap_or_else(|| Err(format!("line {line}: a factor is written x<number>")))
}

fn point(words: &[&str], line: usize) -> Result<([f32; 3], Option<f32>), String> {
    match words {
        [x, y, z] => Ok(([number(x, line)?, number(y, line)?, number(z, line)?], None)),
        [x, y, z, yaw] => Ok((
            [number(x, line)?, number(y, line)?, number(z, line)?],
            Some(number(yaw, line)?),
        )),
        _ => Err(format!("line {line}: a place is <x> <y> <z> [<yaw>]")),
    }
}

pub fn parse(text: &str) -> Result<Vec<Change>, String> {
    let mut changes = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.split('#').next().unwrap_or("");
        let mut words: Vec<&str> = content.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        let when = match words.iter().position(|word| *word == "if") {
            Some(at) => {
                let condition = words.split_off(at);
                match condition.as_slice() {
                    ["if", "players", op, count] => {
                        let compare = match *op {
                            ">=" => Compare::AtLeast,
                            "<=" => Compare::AtMost,
                            "==" => Compare::Exactly,
                            _ => return Err(format!("line {line}: compare with >=, <= or ==")),
                        };
                        let count = count
                            .parse::<u8>()
                            .map_err(|_| format!("line {line}: {count:?} is not a player count"))?;
                        Some((compare, count))
                    }
                    _ => return Err(format!("line {line}: a condition is `if players >= <n>`")),
                }
            }
            None => None,
        };
        let tag = |at: usize| words.get(at).map(|word| (*word).to_string());
        let what = match words.as_slice() {
            ["hp", tag, amount] => What::Hp {
                tag: (*tag).to_string(),
                hp: if amount.starts_with('x') {
                    Hp::Times(factor(amount, line)?)
                } else {
                    Hp::Whole(
                        amount
                            .parse()
                            .map_err(|_| format!("line {line}: {amount:?} is not whole HP"))?,
                    )
                },
            },
            ["move", _, rest @ ..] => {
                let (at, yaw) = point(rest, line)?;
                What::Move {
                    tag: tag(1).expect("matched"),
                    at,
                    yaw,
                }
            }
            ["add", _, new_tag, rest @ ..] => {
                let (by, rest) = match rest {
                    ["by", rest @ ..] => (true, rest),
                    _ => (false, rest),
                };
                let (at, yaw) = point(rest, line)?;
                What::Add {
                    tag: tag(1).expect("matched"),
                    new_tag: new_tag.to_ascii_lowercase(),
                    at,
                    yaw,
                    by,
                }
            }
            ["remove", tag] => What::Remove {
                tag: (*tag).to_string(),
            },
            ["faction", tag, side] => What::Faction {
                tag: (*tag).to_string(),
                faction: match *side {
                    "hostile" => factions::HOSTILE_1,
                    "friendly" => factions::FRIENDLY_1,
                    "neutral" => factions::NEUTRAL,
                    "player" => factions::PLAYER,
                    _ => {
                        return Err(format!(
                            "line {line}: a faction is hostile, friendly, neutral or player"
                        ));
                    }
                },
            },
            ["flag", tag, change] => {
                let (on, name) = match change.split_at_checked(1) {
                    Some(("+", name)) => (true, name),
                    Some(("-", name)) => (false, name),
                    _ => {
                        return Err(format!(
                            "line {line}: a flag is +plot, -plot, +min1hp or -min1hp"
                        ));
                    }
                };
                let flag = match name {
                    "plot" => flags::PLOT,
                    "min1hp" => flags::MIN_ONE_HP,
                    _ => return Err(format!("line {line}: a flag is plot or min1hp")),
                };
                What::Flag {
                    tag: (*tag).to_string(),
                    flag,
                    on,
                }
            }
            ["per_player", "hp", amount] => What::PerPlayerHp {
                factor: factor(amount, line)?,
            },
            _ => return Err(format!("line {line}: unknown change {:?}", words.join(" "))),
        };
        changes.push(Change { line, what, when });
    }
    Ok(changes)
}

/// What applying changed, for reports.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applied {
    /// Changes that applied for this many players, and those skipped.
    pub applied: usize,
    pub skipped: usize,
    /// Changes whose tag matched no creature.
    pub unmatched: Vec<usize>,
}

fn creature_tagged(object: &ObjectData, tag: &str) -> bool {
    object.kind == ObjectKind::Creature && object.tag.eq_ignore_ascii_case(tag)
}

/// The first of `module`'s creatures tagged `tag`, by its place.
fn first_tagged(module: &ModuleData, tag: &str) -> Option<usize> {
    module
        .objects()
        .position(|object| creature_tagged(&object, tag))
}

/// `change` on the sheet of each creature tagged `tag` that has one; whether
/// any had one.
fn sheets(module: &mut ModuleData, tag: &str, change: impl Fn(&mut Sheet)) -> bool {
    let picked = |object: &ObjectData| creature_tagged(object, tag) && object.sheet.is_some();
    let changed = module.change_each(picked, |_, object| {
        if let Some(sheet) = object.sheet.as_mut() {
            change(sheet);
        }
    });
    changed > 0
}

fn scale(hp: u16, factor: f32) -> u16 {
    (f32::from(hp) * factor)
        .round_ties_even()
        .clamp(1.0, f32::from(u16::MAX)) as u16
}

/// Applies `changes` to `module` for `players` (at least one), in their
/// order, and names the count on the module (`ModuleData::players`), which
/// its digest and `Place` carry.
pub fn apply(module: &mut ModuleData, changes: &[Change], players: u8) -> Applied {
    let players = players.max(1);
    module.players = players;
    let mut applied = Applied::default();
    for change in changes {
        let holds = change.when.is_none_or(|(compare, count)| match compare {
            Compare::AtLeast => players >= count,
            Compare::AtMost => players <= count,
            Compare::Exactly => players == count,
        });
        if !holds {
            applied.skipped += 1;
            continue;
        }
        applied.applied += 1;
        let matched = match &change.what {
            What::Hp { tag, hp } => {
                let hp = *hp;
                sheets(module, tag, |sheet| {
                    sheet.max_hp = match hp {
                        Hp::Whole(value) => value.max(1),
                        Hp::Times(factor) => scale(sheet.max_hp, factor),
                    };
                })
            }
            What::Move { tag, at, yaw } => {
                match first_tagged(module, tag).and_then(|index| module.object_mut(index)) {
                    Some(object) => {
                        object.position = *at;
                        if let Some(yaw) = yaw {
                            object.yaw = *yaw;
                        }
                        true
                    }
                    None => false,
                }
            }
            What::Add {
                tag,
                new_tag,
                at,
                yaw,
                by,
            } => match first_tagged(module, tag).and_then(|index| module.object(index)) {
                Some(object) => {
                    let mut copy = object.into_owned();
                    copy.tag.clone_from(new_tag);
                    copy.position = if *by {
                        std::array::from_fn(|axis| copy.position[axis] + at[axis])
                    } else {
                        *at
                    };
                    if let Some(yaw) = yaw {
                        copy.yaw = *yaw;
                    }
                    module.placements.push(Placement::Own(copy));
                    true
                }
                None => false,
            },
            What::Remove { tag } => {
                let removed = module.change_each(
                    |object| creature_tagged(object, tag),
                    |_, object| {
                        object.kind = ObjectKind::Waypoint;
                        object.sheet = None;
                    },
                );
                removed > 0
            }
            What::Faction { tag, faction } => {
                let faction = *faction;
                sheets(module, tag, |sheet| sheet.faction = faction)
            }
            What::Flag { tag, flag, on } => {
                let (flag, on) = (*flag, *on);
                sheets(module, tag, |sheet| {
                    if on {
                        sheet.flags |= flag;
                    } else {
                        sheet.flags &= !flag;
                    }
                })
            }
            What::PerPlayerHp { factor } => {
                let factor = 1.0 + factor * f32::from(players.saturating_sub(1));
                // Only those it changes become their own.
                let repute = module.repute.clone();
                let grows = |object: &ObjectData| {
                    object.sheet.is_some_and(|sheet| {
                        object.kind == ObjectKind::Creature
                            && repute.hostile(sheet.faction, factions::PLAYER)
                            && scale(sheet.max_hp, factor) != sheet.max_hp
                    })
                };
                module.change_each(grows, |_, object| {
                    if let Some(sheet) = object.sheet.as_mut() {
                        sheet.max_hp = scale(sheet.max_hp, factor);
                    }
                });
                true
            }
        };
        if !matched {
            applied.unmatched.push(change.line);
        }
    }
    applied
}

/// The count a plan is for reaches its module (I30 P21). On the proving
/// ground with a creature `probe_foe`, `hp probe_foe x2 if players >= 2`
/// is planned for none, one and two. For `kjk sim probes`.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let ground = sim::determinism::proving_ground();
    let Some(module) = ground.module() else {
        return vec![("encounter_count", Err("the ground has no module".into()))];
    };
    let mut base = (**module).clone();
    let at = base.entry.position;
    let foe = ObjectData::creature("probe_foe", "probe_foe", at, Sheet::dev_soldier());
    base.placements.push(Placement::Own(foe));
    let changes = match parse("hp probe_foe x2 if players >= 2") {
        Ok(changes) => changes,
        Err(why) => return vec![("encounter_count", Err(why))],
    };
    let planned = |players: u8| {
        let mut data = base.clone();
        apply(&mut data, &changes, players);
        data
    };
    let hp = |data: &ModuleData| {
        data.objects()
            .find(|object| creature_tagged(object, "probe_foe"))
            .and_then(|object| object.sheet)
            .map(|sheet| sheet.max_hp)
    };
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let (none, one, two) = (planned(0), planned(1), planned(2));
    vec![
        (
            "planned_for_one",
            check(
                one.players == 1 && hp(&one) == hp(&base) && one.digest() == base.digest(),
                format!("one player: {:?} HP, the module's own digest", hp(&one)),
            ),
        ),
        (
            "planned_for_two",
            check(
                two.players == 2
                    && hp(&two) == hp(&base).map(|hp| scale(hp, 2.0))
                    && two.digest() != one.digest(),
                format!("two players: {:?} HP, another digest", hp(&two)),
            ),
        ),
        (
            "none_is_one",
            check(
                none.players == 1 && none.digest() == one.digest(),
                format!("none counted: planned for {}", none.players),
            ),
        ),
    ]
}
