//! The Debug Mode character page (render side): the rows the page shows
//! (`ui::screens::debug_character`, an options list on the layout of kjk's
//! Interface options) and what stepping them does.
//!
//! The sim opens the page for the player whose body used the Debug Mode
//! map's computer (`Screen::DebugTerminal`, `sim::world::arena`). Its rows
//! are the party member edited, the class, the total level, the second
//! class (the specialization) and its levels, the six attributes, the eight
//! skills and the alignment, then the feats and Force powers known, which
//! only show (`Line`). Each has its value as the presented snapshot has
//! it. Stepping the party member chooses another one. Stepping any other
//! row sends one of the arena's orders (`ArenaOrder::Rebuild` to
//! `ArenaOrder::Alignment`) as the arena's keys send theirs, at once: a
//! class, level or specialization stepped rebuilds the character there and
//! then. Each order sets the value the row shows, one step on.
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
    /// The total level, both classes' levels together.
    Level,
    /// The second class, if any.
    Specialization,
    /// The levels in the second class, part of the total.
    SpecializationLevel,
    /// An attribute, by `Ability` order.
    Ability(u8),
    /// A skill, by `skills.2da` row.
    Skill(u8),
    Alignment,
    /// The feats known, shown and not stepped.
    Feats,
    /// The Force powers known, shown and not stepped.
    Powers,
}

/// The page's rows, top to bottom.
pub fn lines() -> Vec<Line> {
    [
        Line::Member,
        Line::Class,
        Line::Level,
        Line::Specialization,
        Line::SpecializationLevel,
    ]
    .into_iter()
    .chain((0..6).map(Line::Ability))
    .chain((0..8).map(Line::Skill))
    .chain([Line::Alignment, Line::Feats, Line::Powers])
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

/// A class's `classes.2da` row and its name.
pub type Named = (u8, Vec<u8>);

/// What the page reads: the party (id and name, the seat's body first),
/// the chosen member, its sheet, its first class and its second class with
/// the levels in it (from its career; no first class for a character the
/// game made without one), the classes a character may start as, the
/// second classes each of them may take (`Classes::second_classes`), the
/// game's level cap, the names of the feats and Force powers it knows, the
/// map's starting character and whether the member may become it, and the
/// words.
#[derive(Debug, Clone)]
pub struct Facts {
    pub party: Vec<(SimId, Vec<u8>)>,
    pub who: SimId,
    pub sheet: Sheet,
    pub class: Option<u8>,
    pub second: Option<(u8, u8)>,
    pub classes: Vec<Named>,
    pub seconds: Vec<(u8, Vec<Named>)>,
    pub cap: u8,
    pub feats: Vec<Vec<u8>>,
    pub powers: Vec<Vec<u8>>,
    pub start: Sheet,
    /// Whether the chosen member is a player's own character, whom Restore
    /// makes the starting character again. A companion is not.
    pub restorable: bool,
    pub words: Words,
}

impl Facts {
    /// The second classes the first class at `class` (its place in
    /// `classes`) may take.
    fn seconds_of(&self, class: i32) -> &[Named] {
        let row = usize::try_from(class)
            .ok()
            .and_then(|at| self.classes.get(at))
            .map(|(row, _)| *row);
        (self.seconds.iter())
            .find(|(of, _)| Some(*of) == row)
            .map_or(&[], |(_, seconds)| seconds.as_slice())
    }

    /// Row `line`'s value as the snapshot has it: the party member's, the
    /// class's and the second class's place in their lists (-1 for none),
    /// else the number.
    fn value(&self, line: Line) -> i32 {
        let sheet = &self.sheet;
        let class = (self.classes.iter())
            .position(|(row, _)| Some(*row) == self.class)
            .map_or(-1, |at| at as i32);
        match line {
            Line::Member => (self.party.iter())
                .position(|(id, _)| *id == self.who)
                .map_or(0, |at| at as i32),
            Line::Class => class,
            Line::Level => sheet.level.into(),
            Line::Specialization => (self.seconds_of(class).iter())
                .position(|(row, _)| Some(*row) == self.second.map(|(row, _)| row))
                .map_or(-1, |at| at as i32),
            Line::SpecializationLevel => self.second.map_or(0, |(_, levels)| levels.into()),
            Line::Ability(at) => sheet.abilities[usize::from(at)].into(),
            Line::Skill(at) => sheet.skills[usize::from(at)].into(),
            Line::Alignment => sheet.good_evil.into(),
            Line::Feats => self.feats.len() as i32,
            Line::Powers => self.powers.len() as i32,
        }
    }

    /// The least and the most value row `line` may have, as `menu` shows
    /// the other rows (a specialization's levels leave the first class at
    /// least one level of the total).
    fn range(&self, menu: &Menu, line: Line) -> (i32, i32) {
        match line {
            Line::Member => (0, self.party.len() as i32 - 1),
            Line::Class => (0, self.classes.len() as i32 - 1),
            Line::Level => (1, i32::from(self.cap.max(1))),
            Line::Specialization => {
                let class = menu.shown(self, Line::Class).max(0);
                (-1, self.seconds_of(class).len() as i32 - 1)
            }
            Line::SpecializationLevel if menu.shown(self, Line::Specialization) >= 0 => {
                (1, (menu.shown(self, Line::Level) - 1).max(1))
            }
            Line::Ability(_) => (limits::ABILITY_LEAST.into(), limits::ABILITY_MOST.into()),
            Line::Skill(_) => (0, limits::SKILL_MOST.into()),
            Line::Alignment => (0, limits::ALIGNMENT_MOST.into()),
            Line::SpecializationLevel | Line::Feats | Line::Powers => (0, 0),
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

/// The rows that a rebuild sets, which stay as the page shows them while
/// the rebuilt character comes; a rebuild makes the other numbers anew.
const REBUILT: [Line; 4] = [
    Line::Class,
    Line::Level,
    Line::Specialization,
    Line::SpecializationLevel,
];

/// Names joined for a description box, "None" for none.
fn listed(names: &[Vec<u8>]) -> Vec<u8> {
    if names.is_empty() {
        return ui::strings::bytes(words::NONE);
    }
    names.join(&b", "[..])
}

/// The page as the screen shows it.
pub fn view(menu: &Menu, facts: &Facts) -> DebugCharacterView {
    let w = &facts.words;
    let ours = ui::strings::bytes;
    let rows = lines()
        .into_iter()
        .map(|line| {
            let value = menu.shown(facts, line);
            let number = || value.to_string().into_bytes();
            let (least, most) = facts.range(menu, line);
            let range = || ours(&words::range(least as u8, most as u8));
            let class = menu.shown(facts, Line::Class);
            let named = |list: &[Named], none: &str| {
                usize::try_from(value)
                    .ok()
                    .and_then(|at| list.get(at))
                    .map_or_else(|| ours(none), |(_, name)| name.clone())
            };
            let (name, value, help) = match line {
                Line::Member => (
                    ours(words::MEMBER),
                    facts
                        .party
                        .get(value as usize)
                        .map_or_else(Vec::new, |(_, name)| name.clone()),
                    ours(words::MEMBER_HELP),
                ),
                Line::Class => (
                    w.class.clone(),
                    named(&facts.classes, words::NO_CLASS),
                    ours(words::CLASS_HELP),
                ),
                Line::Level => (w.level.clone(), number(), ours(words::LEVEL_HELP)),
                Line::Specialization => (
                    ours(words::SPECIALIZATION),
                    named(facts.seconds_of(class), words::NONE),
                    ours(words::SPECIALIZATION_HELP),
                ),
                Line::SpecializationLevel => (
                    ours(words::SPECIALIZATION_LEVEL),
                    number(),
                    ours(words::SPECIALIZATION_LEVEL_HELP),
                ),
                Line::Ability(at) => (w.abilities[usize::from(at)].clone(), number(), range()),
                Line::Skill(at) => (w.skills[usize::from(at)].clone(), number(), range()),
                Line::Alignment => (w.alignment.clone(), number(), ours(words::ALIGNMENT_HELP)),
                Line::Feats => (ours(words::FEATS), number(), listed(&facts.feats)),
                Line::Powers => (ours(words::POWERS), number(), listed(&facts.powers)),
            };
            OptionRowView {
                name,
                value,
                help,
                fixed: matches!(line, Line::Feats | Line::Powers),
            }
        })
        .collect();
    DebugCharacterView {
        title: ours(words::TITLE),
        rows,
        restore: facts.restorable,
    }
}

/// Row `line` stepped down (`back`) or up: `menu` after it, and the order
/// it sends, if any. The party member, the class and the specialization
/// wrap around their lists (the specialization through None); the numbers
/// stop at their limits.
pub fn stepped(menu: &Menu, facts: &Facts, line: Line, back: bool) -> (Menu, Option<ArenaOrder>) {
    let mut menu = menu.clone();
    let who = facts.who;
    let by = if back { -1 } else { 1 };
    let now = menu.shown(facts, line);
    let (least, most) = facts.range(&menu, line);
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
        Line::Member | Line::Class | Line::Specialization => {
            (now + by - least).rem_euclid(most - least + 1) + least
        }
        _ => (now + by).clamp(least, most),
    };
    if next == now {
        return (menu, None);
    }
    // The class, levels and specialization the rebuild takes: the row's
    // new value, the others as shown.
    let mut built = REBUILT.map(|of| menu.shown(facts, of));
    let at = |of: Line| REBUILT.iter().position(|row| *row == of).unwrap_or(0);
    let order = match line {
        Line::Member => {
            menu.who = facts.party.get(next as usize).map(|(id, _)| *id);
            menu.pending.clear();
            return (menu, None);
        }
        Line::Class | Line::Level | Line::Specialization | Line::SpecializationLevel => {
            built[at(line)] = next;
            let [class, level, special, levels] = &mut built;
            *class = (*class).max(0);
            if line == Line::Class {
                *special = -1;
            }
            // A specialization needs a level of the first class besides.
            if *level < 2 {
                *special = -1;
            }
            *levels = match *special {
                -1 => 0,
                _ if *levels < 1 => (*level / 2).max(1),
                _ => (*levels).clamp(1, *level - 1),
            };
            let row = |list: &[Named], at: i32| {
                usize::try_from(at)
                    .ok()
                    .and_then(|at| list.get(at))
                    .map(|(row, _)| *row)
            };
            let second =
                row(facts.seconds_of(*class), *special).map(|other| (other, *levels as u8));
            let class_row = row(&facts.classes, *class);
            menu.pending.retain(|(of, ..)| REBUILT.contains(of));
            for (of, value) in REBUILT.into_iter().zip(built) {
                menu.hold(of, value);
            }
            let Some(class) = class_row else {
                return (menu, None);
            };
            ArenaOrder::Rebuild {
                who,
                class,
                level: built[at(Line::Level)] as u8,
                second,
            }
        }
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
        Line::Feats | Line::Powers => return (menu, None),
    };
    if !REBUILT.contains(&line) {
        menu.hold(line, next);
    }
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
    let rules = library.classes();
    let name = |assets: &mut ui::assets::GuiAssets, row: u8| {
        assets.cell_text("classes", usize::from(row), "name")
    };
    let classes: Vec<Named> = (0..assets.rows("classes"))
        .filter_map(|row| {
            let row = u8::try_from(row).ok()?;
            let playable = assets
                .cell("classes", usize::from(row), "playerclass")
                .as_deref()
                == Some("1");
            Some((
                row,
                name(assets, row).filter(|_| playable && rules.may_start(row))?,
            ))
        })
        .collect();
    let seconds = (classes.iter())
        .map(|(first, _)| {
            let named = (rules.second_classes(*first).into_iter())
                .filter_map(|row| Some((row, name(assets, row)?)))
                .collect();
            (*first, named)
        })
        .collect();
    let career = snapshot.progress.careers.get(&who.0);
    let class = career
        .and_then(|career| career.first())
        .map(|(class, _)| *class);
    let second = career.and_then(|career| career.get(1)).copied();
    let sheet = crate::cast::worn_sheet(snapshot, entity, level);
    let names = |assets: &mut ui::assets::GuiAssets, table: &str, rows: Vec<u16>| -> Vec<Vec<u8>> {
        (rows.into_iter())
            .filter_map(|row| assets.cell_text(table, usize::from(row), "name"))
            .filter(|name| !name.is_empty())
            .collect()
    };
    let feats = names(assets, "feat", sheet.feats.rows().collect());
    let powers = names(assets, "spells", sheet.powers.rows().collect());
    Some(Facts {
        party,
        who,
        sheet,
        class,
        second,
        classes,
        seconds,
        cap: rules.cap(),
        feats,
        powers,
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
/// rebuilds in that class one level lower; the specialization stepped up
/// on a Jedi Guardian takes the first prestige class with half the levels,
/// its levels stepped down give one back to the first class, and stepped
/// on to None drops it; a soldier shows None and steps on to the Jedi
/// Guardian with half its levels; the party member stepped up chooses the
/// companion and sends nothing; Restore shows for the player and not for
/// a companion.
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
        second: None,
        classes: vec![(0, b"Soldier".to_vec()), (3, b"Jedi Guardian".to_vec())],
        seconds: vec![
            (0, vec![(3, b"Jedi Guardian".to_vec())]),
            (
                3,
                vec![(11, b"Weapon Master".to_vec()), (14, b"Marauder".to_vec())],
            ),
        ],
        cap: 20,
        feats: vec![b"Flurry".to_vec()],
        powers: Vec::new(),
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
    let rebuild = |class, level, second| {
        Some(ArenaOrder::Rebuild {
            who: SimId(1),
            class,
            level,
            second,
        })
    };
    // A level-20 Jedi Guardian: a prestige class on, then fewer levels in
    // it, then none.
    let guardian = Facts {
        class: Some(3),
        ..facts.clone()
    };
    let (special, prestige) = stepped(&menu, &guardian, Line::Specialization, false);
    let (fewer_menu, fewer) = stepped(&special, &guardian, Line::SpecializationLevel, true);
    let (_, dropped) = stepped(&fewer_menu, &guardian, Line::Specialization, true);
    let soldier = Facts {
        class: Some(0),
        ..facts.clone()
    };
    let offered = view(&menu, &soldier).rows[3].value.clone();
    let (_, soldier_jedi) = stepped(&menu, &soldier, Line::Specialization, false);
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
                class == rebuild(0, 20, None) && lower == rebuild(0, 19, None),
                format!("class up from none: {class:?}; then level down: {lower:?}"),
            ),
        ),
        (
            "debug_character_specialization",
            check(
                prestige == rebuild(3, 20, Some((11, 10)))
                    && fewer == rebuild(3, 20, Some((11, 9)))
                    && dropped == rebuild(3, 20, None)
                    && offered == b"None"
                    && soldier_jedi == rebuild(0, 20, Some((3, 10))),
                format!(
                    "prestige on: {prestige:?}; a level less: {fewer:?}; back to None: {dropped:?}; a soldier shows {:?}, then takes {soldier_jedi:?}",
                    String::from_utf8_lossy(&offered)
                ),
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
