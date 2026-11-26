//! The Debug Mode map's terminal (render side): the menu its screen shows
//! (`ui::screens::computer`, KOTOR 2's computer layout) and what its rows
//! do.
//!
//! The sim opens the screen for the player whose body used the terminal
//! (`Screen::DebugTerminal`, `sim::world::arena`). The menu is data: each
//! `Page` is a message and a list of `Row`s built from the chosen party
//! member's sheet as the presented snapshot has it (`page`). A row goes to
//! another page, chooses something the menu keeps (the character, the
//! class and level a rebuild takes), or sends one of the arena's orders
//! (`ArenaOrder::Rebuild` to `ArenaOrder::Restore`) as the arena's keys
//! send theirs. An order sets a value from the number the row shows. The
//! screen's numbers are the chosen character's Computer Use and Repair and
//! the spikes and parts it carries, as on any computer.
//!
//! The names of the attributes, skills, classes, credits and alignment come
//! from the game's talk table and tables. kjk's own words are
//! `ui::strings::terminal`.

use bevy::prelude::*;
use sim::SimId;
use sim::action::{Action, ArenaOrder, ItemKind};
use sim::combat::Sheet;
use ui::draw::Textures;
use ui::intent::Intent;
use ui::skin::Source;
use ui::strings::terminal as words;
use ui::vm::ComputerView;

use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::{Ui, UiIntents};

/// The screen's name among the UI's.
pub const SCREEN: &str = "debug_terminal";

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

/// A page of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Main,
    Character,
    Class,
    Attributes,
    /// One attribute, by `Ability` order.
    Attribute(u8),
    Skills,
    /// One skill, by `skills.2da` row.
    Skill(u8),
    Alignment,
    Credits,
    Items,
    Restore,
}

impl Page {
    /// The page its Back row returns to.
    fn back(self) -> Page {
        match self {
            Page::Attribute(_) => Page::Attributes,
            Page::Skill(_) => Page::Skills,
            _ => Page::Main,
        }
    }

    /// Every page, for the menu's probes.
    #[cfg(feature = "probes")]
    fn all() -> Vec<Page> {
        let fixed = [
            Page::Main,
            Page::Character,
            Page::Class,
            Page::Attributes,
            Page::Skills,
            Page::Alignment,
            Page::Credits,
            Page::Items,
            Page::Restore,
        ];
        let attributes = (0..6).map(Page::Attribute);
        let skills = (0..8).map(Page::Skill);
        fixed.into_iter().chain(attributes).chain(skills).collect()
    }
}

/// What a row does.
#[derive(Debug, Clone, PartialEq)]
pub enum Does {
    Go(Page),
    Back,
    /// The party member the other pages change.
    Choose(SimId),
    /// The class a rebuild takes, moved by this many in the list.
    Class(i32),
    /// The level a rebuild takes, moved by this many.
    Level(i32),
    /// An order to the sim, and what the page then says.
    Order(ArenaOrder, Option<Vec<u8>>),
}

/// A row: its words and what it does.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub text: Vec<u8>,
    pub does: Does,
}

/// What the menu keeps between frames: the page shown, the party member
/// chosen (none: the player's own body), the class and level a rebuild
/// takes (an index in `Facts::classes`, and a level), and what the page
/// says after an order.
#[derive(Resource, Debug, Clone, Default)]
pub struct Menu {
    page: Page,
    who: Option<SimId>,
    class: usize,
    level: Option<u8>,
    told: Option<Vec<u8>>,
}

/// The game's own words the menu uses.
#[derive(Debug, Clone, Default)]
pub struct Words {
    /// Strength to Charisma.
    pub abilities: [Vec<u8>; 6],
    /// By `skills.2da` row.
    pub skills: [Vec<u8>; 8],
    pub attributes: Vec<u8>,
    pub skills_title: Vec<u8>,
    pub alignment: Vec<u8>,
    pub credits: Vec<u8>,
    pub class: Vec<u8>,
    pub level: Vec<u8>,
    pub light: Vec<u8>,
    pub dark: Vec<u8>,
    pub back: Vec<u8>,
}

/// How far the alignment rows move it, and the credits the credit rows add.
const ALIGNMENT_STEP: u8 = rules::world::arena::terminal::ALIGNMENT_STEP;
const CREDITS_SMALL: i32 = rules::world::arena::terminal::CREDITS_SMALL;
const CREDITS_LARGE: i32 = rules::world::arena::terminal::CREDITS_LARGE;

/// The talk table's lines for `Words` (the same strrefs in both games'
/// talk tables): the attributes, Strength to Charisma, from `ABILITIES`.
const ABILITIES: u32 = 719;
const ATTRIBUTES: u32 = 209;
const SKILLS: u32 = 233;
const ALIGNMENT: u32 = 921;
const CREDITS: u32 = 545;
const CLASS: u32 = 38692;
const LEVEL: u32 = 32154;
const LIGHT_SIDE: u32 = 727;
const DARK_SIDE: u32 = 728;
const BACK: u32 = 219;

impl Words {
    fn read(assets: &mut ui::assets::GuiAssets) -> Words {
        let text = |strref| {
            assets
                .text(Source::Campaign, strref)
                .unwrap_or_else(Vec::new)
        };
        let abilities = std::array::from_fn(|at| text(ABILITIES + at as u32));
        let (attributes, skills_title, alignment, credits) = (
            text(ATTRIBUTES),
            text(SKILLS),
            text(ALIGNMENT),
            text(CREDITS),
        );
        let (class, level, light, dark, back) = (
            text(CLASS),
            text(LEVEL),
            text(LIGHT_SIDE),
            text(DARK_SIDE),
            text(BACK),
        );
        let skills = std::array::from_fn(|row| {
            assets
                .cell_text("skills", row, "name")
                .unwrap_or_else(Vec::new)
        });
        Words {
            abilities,
            skills,
            attributes,
            skills_title,
            alignment,
            credits,
            class,
            level,
            light,
            dark,
            back,
        }
    }
}

/// What the pages read: the party (id and name, the player's own first),
/// the chosen member and its sheet, the party's credits, the classes a
/// rebuild may take (row and name), the game's level cap, the map's
/// starting character and whether the member may become it, and the words.
#[derive(Debug, Clone)]
pub struct Facts {
    pub party: Vec<(SimId, Vec<u8>)>,
    pub who: SimId,
    pub sheet: Sheet,
    pub credits: i32,
    pub classes: Vec<(u8, Vec<u8>)>,
    pub cap: u8,
    pub start: Sheet,
    /// Whether the chosen member is a player's own character, whom Restore
    /// makes the starting character again. A companion is not.
    pub restorable: bool,
    pub words: Words,
}

/// A row of `text` doing `does`.
fn row(text: impl Into<Vec<u8>>, does: Does) -> Row {
    Row {
        text: text.into(),
        does,
    }
}

/// The class and level a rebuild takes: the menu's, else the chosen
/// member's own level.
fn rebuild_of(menu: &Menu, facts: &Facts) -> (Option<(u8, Vec<u8>)>, u8) {
    let class = facts.classes.get(menu.class).cloned();
    let level = menu
        .level
        .unwrap_or(facts.sheet.level)
        .clamp(1, facts.cap.max(1));
    (class, level)
}

/// Page `menu.page`: its message and rows (a Back row last, except on the
/// first page, which the Leave row closes).
pub fn page(menu: &Menu, facts: &Facts) -> (Vec<u8>, Vec<Row>) {
    use sim::arena::edits::limits;
    let w = &facts.words;
    let who = facts.who;
    let sheet = &facts.sheet;
    let name = |id: SimId| {
        facts
            .party
            .iter()
            .find(|(of, _)| *of == id)
            .map_or_else(Vec::new, |(_, name)| name.clone())
    };
    let order = |order: ArenaOrder| Does::Order(order, None);
    let (message, mut rows) = match menu.page {
        Page::Main => {
            let message = [
                words::TITLE.as_bytes(),
                b"\n\n",
                &words::character(&name(who)),
            ]
            .concat();
            let rows = vec![
                row(words::CHARACTER, Does::Go(Page::Character)),
                row(words::CLASS_AND_LEVEL, Does::Go(Page::Class)),
                row(w.attributes.clone(), Does::Go(Page::Attributes)),
                row(w.skills_title.clone(), Does::Go(Page::Skills)),
                row(w.alignment.clone(), Does::Go(Page::Alignment)),
                row(w.credits.clone(), Does::Go(Page::Credits)),
                row(words::ITEMS, Does::Go(Page::Items)),
            ];
            // Only a player's own character has a starting state to go
            // back to; a companion's is not kept.
            let restore = facts
                .restorable
                .then(|| row(words::RESTORE, Does::Go(Page::Restore)));
            return (message, rows.into_iter().chain(restore).collect());
        }
        Page::Character => {
            let rows = (facts.party.iter())
                .map(|(id, name)| row(name.clone(), Does::Choose(*id)))
                .collect();
            (words::CHOOSE_CHARACTER.as_bytes().to_vec(), rows)
        }
        Page::Class => {
            let (class, level) = rebuild_of(menu, facts);
            let class_name = class.as_ref().map_or(&[][..], |(_, name)| name.as_slice());
            let message = [
                w.class.as_slice(),
                b": ",
                class_name,
                b"\n",
                &words::value(&w.level, i32::from(level), true),
                b"\n\n",
                words::REBUILD_HINT.as_bytes(),
            ]
            .concat();
            let mut rows = vec![
                row(words::NEXT_CLASS, Does::Class(1)),
                row(words::PREVIOUS_CLASS, Does::Class(-1)),
            ];
            rows.extend([1, -1, 5, -5].map(|by| row(words::step(&w.level, by), Does::Level(by))));
            if let Some((class, _)) = class {
                rows.push(row(
                    words::REBUILD,
                    order(ArenaOrder::Rebuild { who, class, level }),
                ));
            }
            (message, rows)
        }
        Page::Attributes => {
            let rows = (0..6u8)
                .map(|at| {
                    let score = sheet.abilities[usize::from(at)];
                    let text = words::value(&w.abilities[usize::from(at)], score.into(), false);
                    row(text, Does::Go(Page::Attribute(at)))
                })
                .collect();
            (w.attributes.clone(), rows)
        }
        Page::Attribute(at) => {
            let score = sheet.abilities[usize::from(at)];
            let set = |score: u8| {
                order(ArenaOrder::Ability {
                    who,
                    ability: at,
                    score,
                })
            };
            let message = words::value(&w.abilities[usize::from(at)], score.into(), false);
            let rows = vec![
                row(words::INCREASE, set(score.saturating_add(1))),
                row(words::DECREASE, set(score.saturating_sub(1))),
            ];
            (message, rows)
        }
        Page::Skills => {
            let rows = (0..8u8)
                .map(|at| {
                    let ranks = sheet.skills[usize::from(at)];
                    let text = words::value(&w.skills[usize::from(at)], ranks.into(), false);
                    row(text, Does::Go(Page::Skill(at)))
                })
                .collect();
            (w.skills_title.clone(), rows)
        }
        Page::Skill(at) => {
            let ranks = sheet.skills[usize::from(at)];
            let set = |ranks: u8| {
                order(ArenaOrder::Skill {
                    who,
                    skill: at,
                    ranks,
                })
            };
            let message = words::value(&w.skills[usize::from(at)], ranks.into(), false);
            let rows = vec![
                row(words::INCREASE, set(ranks.saturating_add(1))),
                row(words::DECREASE, set(ranks.saturating_sub(1))),
            ];
            (message, rows)
        }
        Page::Alignment => {
            let now = sheet.good_evil;
            let set = |good_evil: u8| order(ArenaOrder::Alignment { who, good_evil });
            let step = ALIGNMENT_STEP;
            let (light, neutral) = (limits::ALIGNMENT_MOST, rules::combat::sheet::NEUTRAL);
            let named = |name: &[u8], value: u8| [name, format!(" ({value})").as_bytes()].concat();
            let rows = vec![
                row(
                    words::step(&w.light, step.into()),
                    set(now.saturating_add(step)),
                ),
                row(
                    words::step(&w.dark, step.into()),
                    set(now.saturating_sub(step)),
                ),
                row(named(&w.light, light), set(light)),
                row(named(words::NEUTRAL.as_bytes(), neutral), set(neutral)),
                row(named(&w.dark, 0), set(0)),
            ];
            (words::value(&w.alignment, now.into(), true), rows)
        }
        Page::Credits => {
            let credits = facts.credits;
            let add = |amount: i32| {
                let text = words::add_credits(&w.credits, amount);
                row(
                    text,
                    order(ArenaOrder::Credits(credits.saturating_add(amount))),
                )
            };
            let rows = vec![
                add(CREDITS_SMALL),
                add(CREDITS_LARGE),
                row(words::REMOVE_CREDITS, order(ArenaOrder::Credits(0))),
            ];
            (words::value(&w.credits, credits, true), rows)
        }
        Page::Items => {
            let rows = ItemKind::ALL
                .into_iter()
                .zip(words::ITEM_KINDS)
                .map(|(kind, name)| {
                    let told = Some(words::added(name));
                    row(name, Does::Order(ArenaOrder::Items(kind), told))
                })
                .collect();
            (words::ITEMS_HINT.as_bytes().to_vec(), rows)
        }
        Page::Restore => {
            let restore = ArenaOrder::Restore {
                who,
                sheet: Box::new(facts.start),
            };
            let rows = (facts
                .restorable
                .then(|| row(words::RESTORE, order(restore))))
            .into_iter()
            .collect();
            (words::RESTORE_HINT.as_bytes().to_vec(), rows)
        }
    };
    rows.push(row(w.back.clone(), Does::Back));
    let message = match &menu.told {
        Some(told) => [message.as_slice(), b"\n\n", told].concat(),
        None => message,
    };
    (message, rows)
}

/// `menu` after row `does` was picked, and the order it sends, if any.
pub fn picked(menu: &Menu, facts: &Facts, does: Does) -> (Menu, Option<ArenaOrder>) {
    let mut menu = Menu {
        told: None,
        ..menu.clone()
    };
    let mut sent = None;
    match does {
        Does::Go(page) => menu.page = page,
        Does::Back => menu.page = menu.page.back(),
        Does::Choose(id) => {
            menu.who = Some(id);
            menu.level = None;
            menu.page = Page::Main;
        }
        Does::Class(by) => {
            let count = facts.classes.len().max(1) as i32;
            menu.class = (menu.class as i32 + by).rem_euclid(count) as usize;
        }
        Does::Level(by) => {
            let (_, level) = rebuild_of(&menu, facts);
            let moved = (i32::from(level) + by).clamp(1, i32::from(facts.cap.max(1)));
            menu.level = Some(moved as u8);
        }
        Does::Order(order, told) => {
            menu.told = told;
            sent = Some(order);
        }
    }
    (menu, sent)
}

/// What the pages read of the presented snapshot, for this seat: none
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
    let members = crate::cast::party_of(snapshot, seat);
    // The seat's body first, then its own character and the companions
    // that follow it (another seat's character is not among them).
    let party: Vec<(SimId, Vec<u8>)> = members
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
    let sheet = crate::cast::worn_sheet(snapshot, entity, level);
    let classes = (0..assets.rows("classes"))
        .filter_map(|row| {
            let playable = assets.cell("classes", row, "playerclass").as_deref() == Some("1");
            let name = assets
                .cell_text("classes", row, "name")
                .filter(|_| playable)?;
            Some((u8::try_from(row).ok()?, name))
        })
        .collect();
    Some(Facts {
        party,
        who,
        sheet,
        credits: snapshot.goods.gold,
        classes,
        cap: library.classes().cap(),
        start: crate::arena::sheet_for(level),
        restorable: matches!(entity.kind, sim::Kind::Avatar(_)),
        words: Words::read(assets),
    })
}

/// The screen's view while it is open: the page's message and rows, and
/// the chosen member's skills. The menu starts on its first page, for the
/// player's own body, each time the screen opens.
fn show(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    mut ui: ResMut<Ui>,
    mut menu: ResMut<Menu>,
) {
    let ui = &mut *ui;
    if !ui.stack.has(SCREEN) {
        ui.vm.debug_terminal = None;
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
    let (message, rows) = page(&menu, &facts);
    let user = snapshot.get(facts.who);
    ui.vm.debug_terminal = Some(ComputerView {
        message,
        replies: rows.into_iter().map(|row| row.text).collect(),
        leave: Some(true),
        leave_key: b"Esc".to_vec(),
        ..crate::terminal::skills_of(assets, snapshot, user, &swap)
    });
}

/// The rows picked: the menu moved, and the orders sent to the sim.
fn pick(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    mut ui: ResMut<Ui>,
    mut intents: ResMut<UiIntents>,
    mut menu: ResMut<Menu>,
    mut outgoing: Option<ResMut<crate::loot::Outgoing>>,
) {
    let picks: Vec<u8> = intents
        .0
        .iter()
        .filter_map(|intent| match intent {
            Intent::DebugTerminal(nth) => Some(*nth),
            _ => None,
        })
        .collect();
    if picks.is_empty() {
        return;
    }
    intents
        .0
        .retain(|intent| !matches!(intent, Intent::DebugTerminal(_)));
    let (Some(presented), Some(own), Some(swap), Some(assets)) =
        (presented, own, swap, ui.assets.as_mut())
    else {
        return;
    };
    let snapshot = presented.0.current();
    for nth in picks {
        let Some(facts) = facts(assets, snapshot, own.0, &swap.level, menu.who) else {
            return;
        };
        let (_, rows) = page(&menu, &facts);
        let Some(row) = rows.into_iter().nth(usize::from(nth)) else {
            continue;
        };
        let (next, order) = picked(&menu, &facts, row.does);
        *menu = next;
        if let (Some(order), Some(outgoing)) = (order, outgoing.as_mut()) {
            outgoing.0.push(Action::Arena(order));
        }
    }
}

/// The menu's probes (`kjk sim probes --only "debug terminal menu"`), on
/// a party of the player and one companion with Strength 16 at level 20,
/// two classes and placeholder words: every page but the first ends in a
/// Back row; every row leads to a page that exists, a choice or an order;
/// a companion is offered no Restore; Increase on Strength 16 sets 17; the class page rebuilds the class and
/// level chosen on it.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let mut sheet = Sheet::dev_jedi();
    sheet.abilities[0] = 16;
    sheet.level = 20;
    let facts = Facts {
        party: vec![(SimId(1), b"Revan".to_vec()), (SimId(2), b"Carth".to_vec())],
        who: SimId(1),
        sheet,
        credits: 500,
        classes: vec![(0, b"Soldier".to_vec()), (3, b"Jedi Guardian".to_vec())],
        cap: 20,
        start: Sheet::dev_jedi(),
        restorable: true,
        words: Words {
            back: b"Back".to_vec(),
            ..Words::default()
        },
    };
    let on = |page| Menu {
        page,
        ..Menu::default()
    };
    let pages = Page::all();
    let backless: Vec<Page> = (pages.iter().copied())
        .filter(|at| {
            let (_, rows) = page(&on(*at), &facts);
            rows.last().is_some_and(|row| row.does == Does::Back) == (*at == Page::Main)
        })
        .collect();
    let astray: Vec<(Page, Page)> = (pages.iter())
        .flat_map(|at| {
            page(&on(*at), &facts)
                .1
                .into_iter()
                .map(move |row| (*at, row.does))
        })
        .filter_map(|(at, does)| match does {
            Does::Go(to) if !pages.contains(&to) => Some((at, to)),
            _ => None,
        })
        .collect();
    let strength = on(Page::Attribute(0));
    let (_, rows) = page(&strength, &facts);
    let raised = rows
        .first()
        .and_then(|row| picked(&strength, &facts, row.does.clone()).1);
    let (menu, _) = picked(&on(Page::Class), &facts, Does::Class(1));
    let (menu, _) = picked(&menu, &facts, Does::Level(-5));
    let rebuild = page(&menu, &facts)
        .1
        .into_iter()
        .find_map(|row| match row.does {
            Does::Order(order, _) => Some(order),
            _ => None,
        });
    let restores = |restorable: bool| {
        let facts = Facts {
            restorable,
            ..facts.clone()
        };
        [Page::Main, Page::Restore].into_iter().any(|at| {
            let rows = page(&on(at), &facts).1;
            rows.iter().any(|row| {
                matches!(
                    row.does,
                    Does::Go(Page::Restore) | Does::Order(ArenaOrder::Restore { .. }, _)
                )
            })
        })
    };
    let (player_restores, companion_restores) = (restores(true), restores(false));
    let wanted = |order| Some(order);
    vec![
        (
            "debug_terminal_back_rows",
            check(
                backless.is_empty(),
                format!("pages wrong about their Back row: {backless:?}"),
            ),
        ),
        (
            "debug_terminal_rows_lead_somewhere",
            check(astray.is_empty(), format!("rows to no page: {astray:?}")),
        ),
        (
            "debug_terminal_no_companion_restore",
            check(
                player_restores && !companion_restores,
                format!(
                    "a Restore row for the player: {player_restores}; for a companion: {companion_restores}"
                ),
            ),
        ),
        (
            "debug_terminal_increase",
            check(
                raised
                    == wanted(ArenaOrder::Ability {
                        who: SimId(1),
                        ability: 0,
                        score: 17,
                    }),
                format!("Increase on Strength 16: {raised:?}"),
            ),
        ),
        (
            "debug_terminal_rebuild",
            check(
                rebuild
                    == wanted(ArenaOrder::Rebuild {
                        who: SimId(1),
                        class: 3,
                        level: 15,
                    }),
                format!("the next class, five levels down from 20: {rebuild:?}"),
            ),
        ),
    ]
}
