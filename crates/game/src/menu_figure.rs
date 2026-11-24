//! Who stands on the main menu, by what was played last (`docs/UI.md`,
//! "The main menu").
//!
//! - A KOTOR 1 game: Malak (`mainmenu`), on KOTOR 2's screens in KOTOR 1's
//!   colours.
//! - A KOTOR 2 game: the Sith Lord of the story so far, in its own colours.
//!   KOTOR 2 keeps a number for it outside its saves, taken from the
//!   story's global `GBL_MAIN_SITH_LORD` each time it saves, loads or quits,
//!   and stands that lord's model there: 0 Sion (`mainmenu01`, which its
//!   prologue sets), 1 Traya, 2 Nihilus (which meeting Visas sets), 3
//!   Kreia, 4 the player's own character on an empty stage (`mainmenu05`);
//!   any other number Sion. Its scripts set only 0 and 2, so kjk carries
//!   the story on as players tell it: Traya once on Malachor V, and after a
//!   game won the player's own character standing there as the new Sith
//!   Lord after a dark-side end, Kreia after a light-side one.
//! - While Nihilus stands there, now and then the Twi'lek dancers KOTOR 2
//!   ships in its data and never shows (`mainmenu06`) dance around him.
//!
//! What was played last is the newer of the newest save (a player's own,
//! or a copy of a host's it received) and the note every machine keeps of
//! the module it last played and how its game was won (`saves::Played`):
//! a guest who never saves is kept up to date too. The story's number is
//! the newest save's of that game (its story, else its world: a host's
//! copies carry no story). With nothing played, KOTOR 1's.

use std::path::Path;

use bevy::prelude::World;
use saves::{Meta, Played, Story, Winner};
use vfs::Game;

/// KOTOR 1's main menu model: Malak.
const MALAK: &str = "mainmenu";
/// KOTOR 2's menu models by the lord's number (`GBL_MAIN_SITH_LORD`).
const LORDS: [&str; 5] = [
    "mainmenu01",
    "mainmenu02",
    "mainmenu03",
    "mainmenu04",
    "mainmenu05",
];
const LORD_GLOBAL: &str = "GBL_MAIN_SITH_LORD";
const SION: usize = 0;
const TRAYA: usize = 1;
const NIHILUS: usize = 2;
const KREIA: usize = 3;
/// The player's own character, on `LORDS[EXILE]`'s empty stage.
const EXILE: usize = 4;
/// Nihilus with the Twi'lek dancers.
const DANCERS: &str = "mainmenu06";
/// One showing of Nihilus in this many has the dancers.
const DANCERS_ODDS: u32 = 8;
/// Names a menu model to stand there whatever the saves say (a screen's
/// check, a screenshot).
const OVERRIDE: &str = "KJK_MENU_FIGURE";

/// The player's character as it stands on the menu: its `appearance.2da`
/// row, the armour it wears (none: its own clothes) and its alignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exile {
    pub appearance: u16,
    pub armour: Option<String>,
    pub good_evil: u8,
}

/// What the main menu shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figure {
    /// The menu model, in the campaign's data.
    pub model: String,
    /// Stands on the model's empty stage (`mainmenu05`).
    pub exile: Option<Exile>,
}

/// Where a KOTOR 2 game stands, as the menu reads it.
struct Progress {
    /// The module played last.
    module: String,
    /// The story's `GBL_MAIN_SITH_LORD`, where a save of it tells it.
    lord: Option<i32>,
    /// The player's own character as the game was won.
    won: Option<Winner>,
}

/// The game played last; none before any was.
pub fn newest_campaign() -> Option<Game> {
    let root = crate::save_view::folder()?;
    let listing = saves::list(&root);
    let saved = listing.saves.first().map(|(id, meta)| {
        let game = game_of(meta, saves::read_story(&root, id).as_ref());
        (meta.saved, Some(game))
    });
    let played = played(&root).map(|played| (played.saved, Game::from_key(&played.game)));
    [saved, played]
        .into_iter()
        .flatten()
        .max_by_key(|(when, _)| *when)
        .and_then(|(_, game)| game)
}

/// The note of what was last played here (`saves::read_played`). A note
/// that does not read is told in the log and moved aside, and the menu goes
/// on as if nothing was played.
fn played(root: &Path) -> Option<Played> {
    saves::read_played(root)
        .inspect_err(|why| bevy::log::warn!(target: "saves", %why, "the note of what was played does not read"))
        .ok()
        .flatten()
}

/// A save's game: its meta's, else its story's, else the install its module
/// is in.
fn game_of(meta: &Meta, story: Option<&Story>) -> Game {
    meta.game
        .as_deref()
        .or(story.map(|story| story.game.as_str()))
        .and_then(Game::from_key)
        .unwrap_or_else(|| crate::installs::game_of_module(&crate::discover(), &meta.module))
}

/// The menu of `campaign` as what was played of it last leaves it.
pub fn of(campaign: Game) -> Figure {
    if let Ok(model) = std::env::var(OVERRIDE)
        && !model.is_empty()
    {
        return Figure { model, exile: None };
    }
    match campaign {
        Game::Kotor2 => {
            let root = crate::save_view::folder();
            lord(root.as_deref().and_then(progress), dancers_roll())
        }
        _ => Figure {
            model: MALAK.into(),
            exile: None,
        },
    }
}

/// Where the KOTOR 2 game played last here stands: the newer of its newest
/// save and the note of what was played (a guest's game, a game won).
fn progress(root: &Path) -> Option<Progress> {
    let listing = saves::list(root);
    let saved = listing.saves.iter().find_map(|(id, meta)| {
        let story = saves::read_story(root, id);
        (game_of(meta, story.as_ref()) == Game::Kotor2).then_some((id, meta, story))
    });
    let lord = saved
        .as_ref()
        .and_then(|(id, _, story)| lord_number(root, id, story.as_ref()));
    let played = played(root).filter(|played| Game::from_key(&played.game) == Some(Game::Kotor2));
    match (played, saved) {
        (Some(played), saved)
            if saved
                .as_ref()
                .is_none_or(|(_, meta, _)| played.saved >= meta.saved) =>
        {
            Some(Progress {
                module: played.module,
                lord,
                won: played.won,
            })
        }
        (_, saved) => saved.map(|(_, meta, _)| Progress {
            module: meta.module.clone(),
            lord,
            won: None,
        }),
    }
}

/// The story's `GBL_MAIN_SITH_LORD` in save `id`: from its story, else
/// from its world.
fn lord_number(root: &Path, id: &str, story: Option<&Story>) -> Option<i32> {
    let find = |numbers: &std::collections::BTreeMap<String, i32>| {
        numbers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(LORD_GLOBAL))
            .map(|(_, number)| *number)
    };
    match story {
        Some(story) => find(&story.globals.numbers),
        None => match saves::read(root, id) {
            Ok((save, _)) => find(&save.script.numbers),
            Err(why) => {
                bevy::log::info!(target: "ui", %why, "the main menu reads no story from a save");
                None
            }
        },
    }
}

/// Whether this showing of Nihilus has the dancers: a roll off the
/// standard hasher's random keys (a clock's fraction is no roll: macOS's
/// counts whole microseconds).
fn dancers_roll() -> bool {
    use std::hash::BuildHasher;
    let roll = std::collections::hash_map::RandomState::new().hash_one(());
    roll.is_multiple_of(u64::from(DANCERS_ODDS))
}

/// KOTOR 2's menu for `progress`, the dancers with Nihilus when `dancers`.
fn lord(progress: Option<Progress>, dancers: bool) -> Figure {
    let (nth, winner) = match progress {
        None => (SION, None),
        Some(Progress {
            won: Some(winner), ..
        }) => {
            let dark = sim::combat::sheet::side(winner.alignment) == sim::combat::sheet::Side::Dark;
            (if dark { EXILE } else { KREIA }, Some(winner))
        }
        Some(progress) if on_malachor(&progress.module) => (TRAYA, None),
        Some(progress) => {
            let nth = progress
                .lord
                .and_then(|number| usize::try_from(number).ok())
                .filter(|nth| *nth < LORDS.len())
                .unwrap_or(SION);
            (nth, None)
        }
    };
    let exile = (nth == EXILE)
        .then_some(winner)
        .flatten()
        .map(|winner| Exile {
            appearance: winner.appearance,
            armour: winner.armour,
            good_evil: winner.alignment,
        });
    let model = match nth {
        NIHILUS if dancers => DANCERS,
        // The empty stage without a character to stand on it: Sion's.
        EXILE if exile.is_none() => LORDS[SION],
        nth => LORDS[nth],
    };
    Figure {
        model: model.into(),
        exile,
    }
}

/// Malachor V's modules (`901MAL` to `907MAL`).
fn on_malachor(module: &str) -> bool {
    module.len() == 6
        && module.starts_with('9')
        && module
            .get(3..)
            .is_some_and(|planet| planet.eq_ignore_ascii_case("mal"))
}

/// Keeps `played` as what was last played here.
fn keep(played: &Played) {
    let written = crate::save_view::folder()
        .ok_or_else(|| "no folder for saves here".to_string())
        .and_then(|root| saves::write_played(&root, played));
    if let Err(why) = written {
        bevy::log::warn!(target: "saves", %why, "what was played is not kept");
    }
}

/// Notes module `module` of game `game` (its key) as what is played here,
/// hosting or joined.
pub fn note_played(game: &str, module: &str) {
    keep(&Played {
        game: game.into(),
        module: module.into(),
        saved: saves::now(),
        won: None,
    });
}

/// Notes the game played here won, with this player's own character as it
/// ended (its seat's in the presented world), hosting or joined.
pub fn note_won(world: &mut World) {
    let Some(seat) = world
        .get_resource::<crate::sim_host::OwnSeat>()
        .map(|own| own.0)
    else {
        return;
    };
    let Some(presented) = world.get_resource::<crate::sim_host::PresentedSnapshot>() else {
        return;
    };
    let snapshot = presented.0.current();
    let Some(place) = snapshot.place.as_ref() else {
        return;
    };
    let won = sim::persona::dressed(snapshot, seat).map(|(appearance, armour)| Winner {
        appearance,
        armour,
        alignment: snapshot
            .own(seat)
            .and_then(|own| own.sheet.as_ref())
            .map_or(sim::combat::sheet::NEUTRAL, |sheet| sheet.good_evil),
    });
    bevy::log::info!(target: "saves", game = place.game, "the game won is kept for the main menu");
    keep(&Played {
        game: place.game.clone(),
        module: place.module.clone(),
        saved: saves::now(),
        won,
    });
}
