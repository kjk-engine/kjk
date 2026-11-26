//! The Debug Mode character page (render side): the rows the page shows
//! (`ui::screens::debug_character`, an options list on the layout of kjk's
//! Interface options) and what stepping them does.
//!
//! The sim opens the page for the player whose body used the Debug Mode
//! map's computer (`Screen::DebugTerminal`, `sim::world::arena`). Its rows
//! are the party member edited, the class, the level, the six attributes,
//! the eight skills and the alignment (`Line`), each with its value as the
//! presented snapshot has it. Stepping the party member chooses another
//! one. Stepping any other row sends one of the arena's orders
//! (`ArenaOrder::Rebuild` to `ArenaOrder::Alignment`) as the arena's keys
//! send theirs, at once: a class or level stepped rebuilds the character
//! there and then. Each order sets the value the row shows, one step on.
//! The page shows a stepped value until the snapshot has it
//! (`Menu::pending`), so a held key steps on from it and never sends the
//! same value twice. Restore sends `ArenaOrder::Restore`, for a player's
//! own character only.
//!
//! The names of the attributes, skills, classes, level and alignment come
//! from the game's talk table and tables. kjk's own words are
//! `ui::strings::terminal`.

use bevy::prelude::*;
use sim::SimId;
use sim::action::{Action, ArenaOrder};
use sim::arena::edits::limits;
use sim::combat::Sheet;
use ui::draw::Textures;
use ui::intent::Intent;
use ui::skin::Source;
use ui::strings::terminal as words;
use ui::vm::{DebugCharacterView, OptionRowView};

use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::{Ui, UiIntents};

/// The page's name among the UI's.
pub const SCREEN: &str = "debug_character";

/// How many frames a stepped value is shown while the snapshot does not
/// have it yet, before the snapshot's value shows again (an order the sim
/// held to another value).
const PENDING_FRAMES: u8 = 30;

pub struct DebugTerminalPlugin;

impl Plugin for DebugTerminalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menu>().add_systems(
            Update,
            (
                show.before(crate::ui_view::Stepped),
                pick.after(crate::ui_view::Stepped)
                    .run_if(crate::photo_mode::off),
            ),
        );
    }
}

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Member,
    Class,
    Level,
    /// An attribute, by `Ability` order.
    Ability(u8),
    /// A skill, by `skills.2da` row.
    Skill(u8),
    Alignment,
}

/// The page's rows, top to bottom.
pub fn lines() -> Vec<Line> {
    [Line::Member, Line::Class, Line::Level]
        .into_iter()
        .chain((0..6).map(Line::Ability))
        .chain((0..8).map(Line::Skill))
        .chain([Line::Alignment])
        .collect()
}

/// What the page keeps between frames: the party member chosen (none: the
/// player's own body), and the values stepped that the snapshot does not
/// show yet, each with the frames it is still shown for.
#[derive(Resource, Debug, Clone, Default)]
pub struct Menu {
    who: Option<SimId>,
    pending: Vec<(Line, i32, u8)>,
}

/// The game's own words the page uses.
#[derive(Debug, Clone, Default)]
pub struct Words {
    /// Strength to Charisma.
    pub abilities: [Vec<u8>; 6],
    /// By `skills.2da` row.
    pub skills: [Vec<u8>; 8],
    pub class: Vec<u8>,
    pub level: Vec<u8>,
    pub alignment: Vec<u8>,
}

/// The talk table's lines for `Words` (the same strrefs in both games'
/// talk tables): the attributes, Strength to Charisma, from `ABILITIES`.
const ABILITIES: u32 = 719;
const ALIGNMENT: u32 = 921;
const CLASS: u32 = 38692;
const LEVEL: u32 = 32154;

impl Words {
    fn read(assets: &mut ui::assets::GuiAssets) -> Words {
        let text = |strref| {
            assets
                .text(Source::Campaign, strref)
                .unwrap_or_else(Vec::new)
        };
        let abilities = std::array::from_fn(|at| text(ABILITIES + at as u32));
        let (class, level, alignment) = (text(CLASS), text(LEVEL), text(ALIGNMENT));
        let skills = std::array::from_fn(|row| {
            assets
                .cell_text("skills", row, "name")
                .unwrap_or_else(Vec::new)
        });
        Words {
            abilities,
            skills,
            class,
            level,
            alignment,
        }
    }
}

/// What the page reads: the party (id and name, the seat's body first),
/// the chosen member, its sheet and its class (its career's first; none
/// for a character the game made without one), the classes a rebuild may
/// take (row and name), the game's level cap, the map's starting character
/// and whether the member may become it, and the words.
#[derive(Debug, Clone)]
pub struct Facts {
    pub party: Vec<(SimId, Vec<u8>)>,
    pub who: SimId,
    pub sheet: Sheet,
    pub class: Option<u8>,
    pub classes: Vec<(u8, Vec<u8>)>,
    pub cap: u8,
    pub start: Sheet,
    /// Whether the chosen member is a player's own character, whom Restore
    /// makes the starting character again. A companion is not.
    pub restorable: bool,
    pub words: Words,
}

impl Facts {
    /// Row `line`'s value as the snapshot has it: the party member's and
    /// the class's place in their lists (-1 for no class), else the number.
    fn value(&self, line: Line) -> i32 {
        let sheet = &self.sheet;
        match line {
            Line::Member => (self.party.iter())
                .position(|(id, _)| *id == self.who)
                .map_or(0, |at| at as i32),
            Line::Class => (self.classes.iter())
                .position(|(row, _)| Some(*row) == self.class)
                .map_or(-1, |at| at as i32),
            Line::Level => sheet.level.into(),
            Line::Ability(at) => sheet.abilities[usize::from(at)].into(),
            Line::Skill(at) => sheet.skills[usize::from(at)].into(),
            Line::Alignment => sheet.good_evil.into(),
        }
    }

    /// The least and the most value row `line` may have.
    fn range(&self, line: Line) -> (i32, i32) {
        match line {
            Line::Member => (0, self.party.len() as i32 - 1),
            Line::Class => (0, self.classes.len() as i32 - 1),
            Line::Level => (1, i32::from(self.cap.max(1))),
            Line::Ability(_) => (limits::ABILITY_LEAST.into(), limits::ABILITY_MOST.into()),
            Line::Skill(_) => (0, limits::SKILL_MOST.into()),
            Line::Alignment => (0, limits::ALIGNMENT_MOST.into()),
        }
    }
}

impl Menu {
    /// Row `line`'s value as the page shows it: the one stepped to, else
    /// the snapshot's.
    fn shown(&self, facts: &Facts, line: Line) -> i32 {
        (self.pending.iter())
            .find(|(of, ..)| *of == line)
            .map_or_else(|| facts.value(line), |(_, value, _)| *value)
    }

    /// The values the snapshot now has, or that waited long enough, no
    /// longer pending.
    fn settle(&mut self, facts: &Facts) {
        self.pending.retain_mut(|(line, value, frames)| {
            *frames = frames.saturating_sub(1);
            *frames > 0 && facts.value(*line) != *value
        });
    }

    /// `line` shown at `value` until the snapshot has it.
    fn hold(&mut self, line: Line, value: i32) {
        self.pending.retain(|(of, ..)| *of != line);
        self.pending.push((line, value, PENDING_FRAMES));
    }
}

/// The page as the screen shows it.
pub fn view(menu: &Menu, facts: &Facts) -> DebugCharacterView {
    let w = &facts.words;
    let rows = lines()
        .into_iter()
        .map(|line| {
            let value = menu.shown(facts, line);
            let number = || value.to_string().into_bytes();
            let (least, most) = facts.range(line);
            let range = || ui::strings::bytes(&words::range(least as u8, most as u8));
            let (name, value, help) = match line {
                Line::Member => (
                    ui::strings::bytes(words::MEMBER),
                    facts
                        .party
                        .get(value as usize)
                        .map_or_else(Vec::new, |(_, name)| name.clone()),
                    ui::strings::bytes(words::MEMBER_HELP),
                ),
                Line::Class => (
                    w.class.clone(),
                    usize::try_from(value)
                        .ok()
                        .and_then(|at| facts.classes.get(at))
                        .map_or_else(
                            || ui::strings::bytes(words::NO_CLASS),
                            |(_, name)| name.clone(),
                        ),
                    ui::strings::bytes(words::CLASS_HELP),
                ),
                Line::Level => (
                    w.level.clone(),
                    number(),
                    ui::strings::bytes(words::LEVEL_HELP),
                ),
                Line::Ability(at) => (w.abilities[usize::from(at)].clone(), number(), range()),
                Line::Skill(at) => (w.skills[usize::from(at)].clone(), number(), range()),
                Line::Alignment => (
                    w.alignment.clone(),
                    number(),
                    ui::strings::bytes(words::ALIGNMENT_HELP),
                ),
            };
            OptionRowView { name, value, help }
        })
        .collect();
    DebugCharacterView {
        title: ui::strings::bytes(words::TITLE),
        rows,
        restore: facts.restorable,
    }
}

/// Row `line` stepped down (`back`) or up: `menu` after it, and the order
/// it sends, if any. The party member and the class wrap around their
/// lists; the numbers stop at their limits.
pub fn stepped(menu: &Menu, facts: &Facts, line: Line, back: bool) -> (Menu, Option<ArenaOrder>) {
    let mut menu = menu.clone();
    let who = facts.who;
    let by = if back { -1 } else { 1 };
    let now = menu.shown(facts, line);
    let (least, most) = facts.range(line);
    if most < least {
        return (menu, None);
    }
    let next = match line {
        // No class yet: on to the first, back to the last.
        Line::Class if now < 0 => {
            if back {
                most
            } else {
                least
            }
        }
        Line::Member | Line::Class => (now + by - least).rem_euclid(most - least + 1) + least,
        _ => (now + by).clamp(least, most),
    };
    if next == now {
        return (menu, None);
    }
    let level = |menu: &Menu| menu.shown(facts, Line::Level).clamp(1, 255) as u8;
    let class_row = |at: i32| {
        usize::try_from(at)
            .ok()
            .and_then(|at| facts.classes.get(at))
            .map(|(row, _)| *row)
    };
    let order = match line {
        Line::Member => {
            menu.who = facts.party.get(next as usize).map(|(id, _)| *id);
            menu.pending.clear();
            return (menu, None);
        }
        Line::Class => ArenaOrder::Rebuild {
            who,
            class: class_row(next).unwrap_or(0),
            level: level(&menu),
        },
        Line::Level => ArenaOrder::Rebuild {
            who,
            class: class_row(menu.shown(facts, Line::Class).max(0)).unwrap_or(0),
            level: next as u8,
        },
        Line::Ability(ability) => ArenaOrder::Ability {
            who,
            ability,
            score: next as u8,
        },
        Line::Skill(skill) => ArenaOrder::Skill {
            who,
            skill,
            ranks: next as u8,
        },
        Line::Alignment => ArenaOrder::Alignment {
            who,
            good_evil: next as u8,
        },
    };
    // A rebuild makes the attributes and skills anew: they show the
    // snapshot's again. The class it keeps shows as the one chosen.
    if matches!(line, Line::Class | Line::Level) {
        menu.pending
            .retain(|(of, ..)| matches!(of, Line::Class | Line::Level));
        if line == Line::Level && menu.shown(facts, Line::Class) < 0 {
            menu.hold(Line::Class, 0);
        }
    }
    menu.hold(line, next);
    (menu, Some(order))
}

/// What the page reads of the presented snapshot, for this seat: none
/// when the snapshot has no body for it or the level no library.
fn facts(
    assets: &mut ui::assets::GuiAssets,
    snapshot: &sim::Snapshot,
    seat: sim::Seat,
    level: &sim::Level,
    chosen: Option<SimId>,
) -> Option<Facts> {
    let library = level.scripts()?;
    let objects = sim::Instances::in_snapshot(level, snapshot);
    // The seat's body first, then its own character and the companions
    // that follow it (another seat's character is not among them).
    let party: Vec<(SimId, Vec<u8>)> = crate::cast::party_of(snapshot, seat)
        .iter()
        .map(|member| {
            let id = script_ncs::vm::ObjectId(member.id.0);
            let name = crate::dialogue_view::name_of(snapshot, &objects, id);
            let name = match name.is_empty() {
                true => words::UNNAMED.as_bytes().to_vec(),
                false => name,
            };
            (member.id, name)
        })
        .collect();
    let body = snapshot.body(seat)?.id;
    let who = chosen
        .filter(|id| party.iter().any(|(of, _)| of == id))
        .unwrap_or(body);
    let entity = snapshot.get(who)?;
    let classes = (0..assets.rows("classes"))
        .filter_map(|row| {
            let playable = assets.cell("classes", row, "playerclass").as_deref() == Some("1");
            let name = assets
                .cell_text("classes", row, "name")
                .filter(|_| playable)?;
            Some((u8::try_from(row).ok()?, name))
        })
        .collect();
    let class = (snapshot.progress.careers.get(&who.0))
        .and_then(|career| career.first())
        .map(|(class, _)| *class);
    Some(Facts {
        party,
        who,
        sheet: crate::cast::worn_sheet(snapshot, entity, level),
        class,
        classes,
        cap: library.classes().cap(),
        start: crate::arena::sheet_for(level),
        restorable: matches!(entity.kind, sim::Kind::Avatar(_)),
        words: Words::read(assets),
    })
}

/// The page's view while it is open. The page starts on the player's own
/// body each time it opens.
fn show(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    mut ui: ResMut<Ui>,
    mut menu: ResMut<Menu>,
) {
    let ui = &mut *ui;
    if !ui.stack.has(SCREEN) {
        ui.vm.debug_character = None;
        *menu = Menu::default();
        return;
    }
    let (Some(presented), Some(own), Some(swap), Some(assets)) =
        (presented, own, swap, ui.assets.as_mut())
    else {
        return;
    };
    let snapshot = presented.0.current();
    let Some(facts) = facts(assets, snapshot, own.0, &swap.level, menu.who) else {
        return;
    };
    menu.settle(&facts);
    ui.vm.debug_character = Some(view(&menu, &facts));
}

/// The rows stepped and Restore pressed: the page moved, and the orders
/// sent to the sim.
fn pick(
    presented: Option<Res<PresentedSnapshot>>,
    (own, swap): (Option<Res<OwnSeat>>, Option<Res<SceneSwap>>),
    mut ui: ResMut<Ui>,
    mut intents: ResMut<UiIntents>,
    mut menu: ResMut<Menu>,
    mut outgoing: Option<ResMut<crate::loot::Outgoing>>,
) {
    let asked: Vec<Intent> = intents
        .0
        .iter()
        .filter(|intent| matches!(intent, Intent::DebugStep { .. } | Intent::DebugRestore))
        .cloned()
        .collect();
    if asked.is_empty() {
        return;
    }
    intents
        .0
        .retain(|intent| !matches!(intent, Intent::DebugStep { .. } | Intent::DebugRestore));
    let (Some(presented), Some(own), Some(swap), Some(assets)) =
        (presented, own, swap, ui.assets.as_mut())
    else {
        return;
    };
    let snapshot = presented.0.current();
    let rows = lines();
    for intent in asked {
        let Some(facts) = facts(assets, snapshot, own.0, &swap.level, menu.who) else {
            return;
        };
        let order = match intent {
            Intent::DebugStep { row, back } => {
                let Some(line) = rows.get(usize::from(row)) else {
                    continue;
                };
                let (next, order) = stepped(&menu, &facts, *line, back);
                *menu = next;
                order
            }
            _ if facts.restorable => {
                menu.pending.clear();
                Some(ArenaOrder::Restore {
                    who: facts.who,
                    sheet: Box::new(facts.start),
                })
            }
            _ => None,
        };
        if let (Some(order), Some(outgoing)) = (order, outgoing.as_mut()) {
            outgoing.0.push(Action::Arena(order));
        }
    }
}

/// The page's probes (`kjk sim probes --only "debug terminal menu"`), on a
/// party of the player and one companion, Strength 16 at level 20, two
/// classes and placeholder words: every row has words to describe it;
/// Strength stepped up twice before the snapshot changes sets 17, then 18;
/// a step stops at the limit; the class stepped up from none rebuilds at
/// the first class and the same level, and the level stepped down then
/// rebuilds in that class one level lower; the party member stepped up
/// chooses the companion and sends nothing; Restore shows for the player
/// and not for a companion.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let mut sheet = Sheet::dev_jedi();
    sheet.abilities[0] = 16;
    sheet.abilities[1] = limits::ABILITY_MOST;
    sheet.level = 20;
    let facts = Facts {
        party: vec![(SimId(1), b"Revan".to_vec()), (SimId(2), b"Carth".to_vec())],
        who: SimId(1),
        sheet,
        class: None,
        classes: vec![(0, b"Soldier".to_vec()), (3, b"Jedi Guardian".to_vec())],
        cap: 20,
        start: Sheet::dev_jedi(),
        restorable: true,
        words: Words::default(),
    };
    let menu = Menu::default();
    let shown = view(&menu, &facts);
    let described =
        shown.rows.len() == lines().len() && shown.rows.iter().all(|row| !row.help.is_empty());
    let (once, first) = stepped(&menu, &facts, Line::Ability(0), false);
    let (_, second) = stepped(&once, &facts, Line::Ability(0), false);
    let strength = |score| {
        Some(ArenaOrder::Ability {
            who: SimId(1),
            ability: 0,
            score,
        })
    };
    let (_, past) = stepped(&menu, &facts, Line::Ability(1), false);
    let (classed, class) = stepped(&menu, &facts, Line::Class, false);
    let (_, lower) = stepped(&classed, &facts, Line::Level, true);
    let rebuild = |class, level| {
        Some(ArenaOrder::Rebuild {
            who: SimId(1),
            class,
            level,
        })
    };
    let (chosen, member) = stepped(&menu, &facts, Line::Member, false);
    let companion = Facts {
        restorable: false,
        ..facts.clone()
    };
    let restores = (view(&menu, &facts).restore, view(&menu, &companion).restore);
    vec![
        (
            "debug_character_rows",
            check(
                described,
                format!("{} rows, each described: {described}", shown.rows.len()),
            ),
        ),
        (
            "debug_character_steps",
            check(
                first == strength(17) && second == strength(18) && past.is_none(),
                format!("Strength 16 up twice: {first:?}, {second:?}; past the most: {past:?}"),
            ),
        ),
        (
            "debug_character_rebuilds",
            check(
                class == rebuild(0, 20) && lower == rebuild(0, 19),
                format!("class up from none: {class:?}; then level down: {lower:?}"),
            ),
        ),
        (
            "debug_character_member",
            check(
                member.is_none() && chosen.who == Some(SimId(2)),
                format!("member up: {:?}, order {member:?}", chosen.who),
            ),
        ),
        (
            "debug_character_restore",
            check(
                restores == (true, false),
                format!("Restore for the player, for a companion: {restores:?}"),
            ),
        ),
    ]
}
