//! The screens the games' scripts open for this player (the GUI natives,
//! `sim::notice::Screen`, from the host), opened over whatever is open:
//!
//! - not a store (`OpenStore`): `merchant` opens it when the trade is in
//!   the snapshot; nor pazaak (`PlayPazaak`): `pazaak_view` follows the
//!   snapshot's match;
//! - the upgrade bench (`ShowUpgradeScreen`), at its choice of kind; TSL's
//!   with item creation (`workbench_view`) there, or at the workbench when
//!   upgrading is disabled; TSL's lab station (`ShowChemicalUpgradeScreen`)
//!   at its own screen;
//! - level-up (`ShowLevelUpGUI`): the character's next level in its class
//!   (a companion's from its template, else the class made in creation,
//!   else the first);
//! - party selection (`ShowPartySelectionGUI`, or the table the host
//!   tells when the party screen opens in play, asked with
//!   `Action::ShowParty`): each available companion's portrait and name
//!   from its template; the message box (its strref), the fall;
//! - a movie (`PlayMovie`, TSL's `PlayMovieQueue`): it plays over
//!   everything (`movie_view`);
//! - a tutorial window (`ShowTutorialWindow`: its row's pages, unless this
//!   player's "Tutorial Popups" option is off) and TSL's datapad
//!   (`DisplayDatapad`: its item's words), a message box of pages;
//! - the galaxy map (`ShowGalaxyMap`): the planets it shows from
//!   `planetary.2da`, the host's to travel from (I30 P6);
//! - the credits (`StartCreditSequence`, `credits.2da`);
//! - the game over (`EndGame` without its death screen): the main menu, or
//!   the credits' end when they roll.
//!
//! The credits and the game over wait for the movies before them.
//!
//! Level-up waits a frame for the character's view (`character`).

use bevy::prelude::*;
use sim::notice::{NPCS, Roster, Screen};
use ui::assets::GuiAssets;
use ui::vm::{
    CharacterView, CompanionView, GalaxyView, LevelUpView, PartySelectionView, PicksView,
    PlanetView,
};

use crate::character::Made;
use crate::ui_view::{Ui, UiIntents};

/// Screens the host opened for this player, not yet open.
#[derive(Resource, Default)]
pub struct Shown(pub Vec<Screen>);

pub struct ScriptScreensPlugin;

impl Plugin for ScriptScreensPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shown>()
            .init_resource::<crate::movie_view::Movies>()
            .add_systems(
                Update,
                (
                    crate::ui_view::fall.before(crate::character::Viewed),
                    open.after(crate::character::Viewed)
                        .before(crate::ui_view::Stepped),
                ),
            );
    }
}

/// A screen as the console's `ui script` names it (as a host would send
/// it): `upgrade`, `levelup`, `party` (no companions: `ui open party` in
/// play shows the host's table), `death`, `credits`, `end`, `box <strref>`,
/// `movie <name>`, `tutorial <row>`, `datapad <item template>`,
/// `galaxy <planet> [<available> <selectable>]` (rows by
/// commas: `galaxy 3 1,3,4 3,4`; else the planet alone, selectable).
pub fn parse(words: &[&str]) -> Option<Screen> {
    let rows = |listed: &str| -> Option<u32> {
        listed.split(',').try_fold(0u32, |bits, row| {
            Some(bits | 1u32.checked_shl(row.trim().parse().ok()?)?)
        })
    };
    Some(match words {
        ["upgrade"] => Screen::Upgrade { item: None },
        ["levelup"] => Screen::LevelUp,
        ["party"] => Screen::PartySelection(Roster::default()),
        ["death"] => Screen::Death,
        ["end"] => Screen::End,
        ["credits"] => Screen::Credits,
        ["box", strref] => Screen::MessageBox {
            strref: strref.parse().ok()?,
        },
        ["movie", name] => Screen::movie(name),
        ["tutorial", row] => Screen::Tutorial {
            row: row.parse().ok()?,
        },
        ["datapad", template] => Screen::Datapad {
            template: template.to_string(),
        },
        ["galaxy", planet, offered @ ..] => {
            let planet: u8 = planet.parse().ok()?;
            let (available, selectable) = match offered {
                [] => {
                    let alone = 1u32.checked_shl(u32::from(planet))?;
                    (alone, alone)
                }
                [available, selectable] => (rows(available)?, rows(selectable)?),
                _ => return None,
            };
            Screen::GalaxyMap {
                planet,
                available,
                selectable,
            }
        }
        _ => return None,
    })
}

/// The screens a bench a script opened shows, while one is open.
const BENCHES: [&str; 3] = ["workbench", "chemical", "upgrade_select"];

/// The screen's name among the UI's.
fn name(screen: Screen) -> String {
    match screen {
        Screen::Store { .. } => "store".into(),
        Screen::Upgrade { .. } => "upgrade_select".into(),
        Screen::Workbench(bench) if bench.lab => "chemical".into(),
        Screen::Workbench(bench) if bench.upgrade => "upgrade_select".into(),
        Screen::Workbench(_) => "workbench".into(),
        Screen::LevelUp => "levelup".into(),
        Screen::PartySelection(_) => "party".into(),
        Screen::Death => "death".into(),
        // The game over opens none of its own (`open`).
        Screen::End | Screen::Credits => "credits".into(),
        Screen::MessageBox { strref } => format!("message_box:{strref}"),
        Screen::Movie { .. } => "movie".into(),
        Screen::GalaxyMap { .. } => "galaxymap".into(),
        Screen::Pazaak => "pazaak".into(),
        Screen::Tutorial { .. } | Screen::Datapad { .. } => "pages".into(),
    }
}

/// A tutorial window's pages: its `tutorial.2da` row's `Message_PC0` to
/// `Message_PC2` (K1's `Message0` to `2`) that have words.
fn tutorial(assets: &mut GuiAssets, row: u8) -> Vec<Vec<u8>> {
    let row = usize::from(row);
    (0..3)
        .filter_map(|n| {
            assets
                .cell_text("tutorial", row, &format!("message_pc{n}"))
                .or_else(|| assets.cell_text("tutorial", row, &format!("message{n}")))
        })
        .filter(|words| !words.is_empty())
        .collect()
}

/// The character's next level in its class (a companion's, from its
/// template; else the class made in creation; else the first), from its
/// ranks now.
pub(crate) fn level_up(me: &CharacterView, made: &Made) -> LevelUpView {
    let mut skills = [0; 8];
    for (rank, skill) in skills.iter_mut().zip(&me.skills) {
        *rank = skill.count.map_or(0, |count| count.min(255) as u8);
    }
    let (class, picks) = me.next_level.clone().unwrap_or_else(|| {
        let class = me.class_row.or(made.0.as_ref().map(|made| made.class));
        (class.unwrap_or(0), PicksView::default())
    });
    LevelUpView {
        who: me.id,
        class,
        level: me.level.saturating_add(1),
        skills,
        picks,
    }
}

const PLANETARY: &str = "planetary";

/// The galaxy map's planets (`planetary.2da` rows): those `available`, each
/// `selectable` or not (bit n: row n), opened on `current`.
fn galaxy(
    assets: &mut GuiAssets,
    current: u8,
    (available, selectable): (u32, u32),
    host: bool,
) -> GalaxyView {
    let planets = (0..assets.rows(PLANETARY))
        .filter(|row| Screen::has_planet(available, *row))
        .filter_map(|row| {
            Some(PlanetView {
                row: u8::try_from(row).ok()?,
                tag: assets.cell_as_written(PLANETARY, row, "guitag")?,
                name: assets
                    .cell_text(PLANETARY, row, "name")
                    .unwrap_or(Vec::new()),
                description: assets
                    .cell_text(PLANETARY, row, "description")
                    .unwrap_or(Vec::new()),
                model: assets.cell(PLANETARY, row, "model"),
                selectable: Screen::has_planet(selectable, row),
            })
        })
        .collect();
    GalaxyView {
        planets,
        current,
        host,
    }
}

/// The party selection screen's table: each available companion's
/// portrait and name, by its template; its figure in what it wears now
/// (the table's), else what its template starts with.
fn party(assets: &mut GuiAssets, roster: &Roster) -> PartySelectionView {
    let numbers = |bits: u16| -> Vec<u8> {
        (0..NPCS)
            .filter(|npc| Roster::has(bits, *npc))
            .map(|npc| npc as u8)
            .collect()
    };
    let companions = (0..NPCS)
        .map(|npc| {
            let template = roster.template(npc)?;
            Some(CompanionView {
                portrait: assets.portrait_of(template),
                appearance: assets.appearance_of(template),
                armour: match roster.worn(npc) {
                    Some(worn) => worn.map(str::to_string),
                    None => assets.armour_of(template),
                },
                name: assets
                    .template_name(template, fmt_odyssey::ResType::UTC)
                    .unwrap_or_else(|| template.as_bytes().to_vec()),
                selectable: !Roster::has(roster.unselectable, npc),
            })
        })
        .collect();
    // The campaign's own layout's companion buttons (KOTOR 1's nine).
    let slots = assets
        .layout("partyselection")
        .or_else(|_| assets.layout(&ui::skin::moved("partyselection")))
        .map_or(NPCS, |gui| {
            gui.controls
                .iter()
                .filter(|control| control.tag.starts_with("BTN_NPC"))
                .count()
        });
    PartySelectionView {
        slots,
        companions,
        members: numbers(roster.members),
        forced: numbers(roster.forced),
        open: roster.open,
    }
}

/// The credits: each `credits.2da` row's block of names.
fn credits(assets: &mut GuiAssets) -> Vec<Vec<u8>> {
    (0..assets.rows("credits"))
        .filter_map(|row| assets.cell_text("credits", row, "name"))
        .collect()
}

fn open(
    mut shown: ResMut<Shown>,
    made: Res<Made>,
    mut ui: ResMut<Ui>,
    mut movies: ResMut<crate::movie_view::Movies>,
    (showing, own, mut intents): (
        Option<Res<crate::movie_view::Showing>>,
        Option<Res<crate::sim_host::OwnSeat>>,
        ResMut<UiIntents>,
    ),
    (mut bench, settings): (
        ResMut<crate::workbench_view::OpenBench>,
        Option<Res<crate::settings::Settings>>,
    ),
    (mut party_was_open, mut in_game_was): (Local<bool>, Local<bool>),
) {
    let ui = &mut *ui;
    let party_shown = shown
        .0
        .iter()
        .any(|screen| matches!(screen, Screen::PartySelection(_)));
    let mut waiting = Vec::new();
    let showing = showing.is_some_and(|showing| showing.0);
    for screen in std::mem::take(&mut shown.0) {
        match screen {
            Screen::Movie { .. } => {
                movies.0.extend(crate::movie_view::Queued::of(&screen));
                continue;
            }
            // This player's "Tutorial Popups" option off: none comes up.
            Screen::Tutorial { .. } if settings.as_ref().is_some_and(|s| !s.tutorials()) => {
                continue;
            }
            Screen::Tutorial { row } => {
                let Some(assets) = ui.assets.as_mut() else {
                    waiting.push(screen);
                    continue;
                };
                ui.vm.pages = tutorial(assets, row);
                ui.stack.close("pages");
            }
            Screen::Datapad { ref template } => {
                let Some(assets) = ui.assets.as_mut() else {
                    waiting.push(screen);
                    continue;
                };
                let words = assets.item(template).map(|item| item.description);
                ui.vm.pages = words.into_iter().collect();
                ui.stack.close("pages");
            }
            // After the movies before them (Taris's fall, then the menu).
            Screen::Credits | Screen::End
                if showing || !movies.0.is_empty() || ui.assets.is_none() =>
            {
                waiting.push(screen);
                continue;
            }
            Screen::Credits => {
                if let Some(assets) = ui.assets.as_mut() {
                    ui.vm.credits = credits(assets);
                }
            }
            // Credits rolling, or waiting to, take the player there.
            Screen::End => {
                if !waiting.contains(&Screen::Credits) && !ui.stack.has("credits") {
                    info!(target: "ui", "the game is over");
                    intents.0.push(ui::intent::Intent::Session(
                        ui::intent::SessionIntent::ToMenu,
                    ));
                }
                continue;
            }
            Screen::GalaxyMap {
                planet,
                available,
                selectable,
            } => {
                let Some(assets) = ui.assets.as_mut() else {
                    waiting.push(screen);
                    continue;
                };
                // The host is seat 0 (I30 P4); alone, the player is.
                let host = own.as_ref().is_none_or(|own| own.0 == sim::Seat(0));
                ui.vm.galaxy = Some(galaxy(assets, planet, (available, selectable), host));
                // A map shown again opens anew.
                ui.stack.close("galaxymap");
            }
            Screen::PartySelection(ref roster) => {
                let Some(assets) = ui.assets.as_mut() else {
                    waiting.push(screen);
                    continue;
                };
                ui.vm.party_selection = Some(party(assets, roster));
                // The table told for the screen a player opened: it is
                // open already.
                if !roster.open && ui.stack.has("party") {
                    continue;
                }
                ui.stack.close("party");
            }
            Screen::Store { .. } | Screen::Pazaak => continue,
            // Told again after a breakdown or an item made: the bench open
            // moves its stock.
            Screen::Workbench(ref told) => {
                let Some(assets) = ui.assets.as_ref() else {
                    waiting.push(screen);
                    continue;
                };
                let again = bench.0.is_some() && BENCHES.iter().any(|open| ui.stack.has(open));
                bench.told(assets, told, again);
                if again {
                    continue;
                }
            }
            Screen::LevelUp => {
                let Some(me) = &ui.vm.character else {
                    waiting.push(screen);
                    continue;
                };
                ui.vm.levelup = Some(level_up(me, &made));
            }
            _ => {}
        }
        // A bench for one item opens on its slots.
        let name = match screen {
            Screen::Upgrade { item: Some(item) }
            | Screen::Workbench(sim::notice::Bench {
                item: Some(item),
                upgrade: true,
                ..
            }) if ui
                .vm
                .upgrades
                .iter()
                .any(|upgradable| upgradable.id == item) =>
            {
                format!("upgrade:{item}")
            }
            _ => name(screen),
        };
        info!(target: "ui", screen = %name, "a script opens");
        if !ui.stack.open(&name, &ui.vm) {
            warn!(target: "ui", screen = %name, "no such screen");
        }
    }
    shown.0 = waiting;
    if !BENCHES.iter().any(|open| ui.stack.has(open)) {
        bench.0 = None;
        ui.vm.workbench = None;
    }
    // The party screen a player opened in play asks the host for the
    // table; closed, the table told is let go.
    let party_open = ui.stack.has("party");
    if party_open && !*party_was_open && !party_shown {
        intents
            .0
            .push(ui::intent::Intent::Game(ui::intent::GameAction::ShowParty));
    }
    if !party_open {
        ui.vm.party_selection = None;
    }
    *party_was_open = party_open;
    // An in-game screen opening runs the game's screen script on the host,
    // as KOTOR runs it each time its in-game screens come up.
    let in_game = IN_GAME.iter().any(|screen| ui.stack.has(screen));
    if in_game && !*in_game_was {
        intents.0.push(ui::intent::Intent::Game(
            ui::intent::GameAction::ScreenOpened,
        ));
    }
    *in_game_was = in_game;
}

/// The in-game screens (KOTOR's in-game GUI): the menu's pages and the
/// party screen.
const IN_GAME: [&str; 9] = [
    "equip",
    "inventory",
    "character",
    "abilities",
    "messages",
    "map",
    "journal",
    "options_in_game",
    "party",
];
