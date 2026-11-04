//! An edited save checked before it is written (spec: two checks): it
//! reads back as it is, always; and with an install, it loads in its
//! module and steps, as kjk loads it (`Save::world`: re-stamped and
//! reconciled), its queued party changes run. A panic in either is told as
//! an error, never unwound into the editor.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use sim::{Level, Save, SimWorld, StepReason, TickRate};

use crate::InstallSource;

/// What an edited save passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checked {
    /// It reads back as it is (no install to load it in).
    RoundTrip,
    /// That, and it loaded and stepped `ticks` ticks in its module.
    Stepped { ticks: u64 },
}

/// Whether `save` reads back as it is.
pub fn round_trip(save: &Save) -> Result<Checked, String> {
    caught("reading it back", || {
        let again = Save::decode(&save.encode())
            .map_err(|error| format!("it does not read back: {error}"))?;
        if again == *save {
            Ok(Checked::RoundTrip)
        } else {
            Err("it reads back changed".into())
        }
    })
}

/// Whether `save` reads back, then loads in its module as `discovery`'s
/// install plans it and steps `ticks` ticks; why not, naming what failed.
pub fn load_and_step(
    save: &Save,
    discovery: &vfs::Discovery,
    ticks: u64,
) -> Result<Checked, String> {
    round_trip(save)?;
    let world = stepped(save, discovery, ticks)?;
    match crate::party::unmade(save, &world.script_state().party) {
        Some(change) => Err(format!(
            "the queued change \"{change}\" did not happen as the save loaded"
        )),
        None => Ok(Checked::Stepped { ticks }),
    }
}

/// The level `save` is in, planned as `discovery`'s install has it (for
/// `Editor::with_level`, and the load `load_and_step` makes).
pub fn level_of(save: &Save, discovery: &vfs::Discovery) -> Result<Level, String> {
    caught("planning its level", || {
        let place = save
            .snapshot
            .place
            .as_ref()
            .ok_or("the save is in no module of a game: nothing to load it in")?;
        let game = vfs::Game::from_key(&place.game)
            .filter(|game| *game != vfs::Game::JediAcademy)
            .ok_or(format!("{}: not a KOTOR game", place.game))?;
        let seats = (save.snapshot.seats(), place.players);
        planned(discovery, game, &place.module, seats)
    })
}

/// `save` loaded in its module and stepped `ticks` ticks: the world then.
pub(crate) fn stepped(
    save: &Save,
    discovery: &vfs::Discovery,
    ticks: u64,
) -> Result<SimWorld, String> {
    let level = level_of(save, discovery)?;
    caught("loading and stepping it", || {
        let rate = TickRate::new(save.hz).ok_or(format!("the save's {} Hz", save.hz))?;
        let mut world = save.world(&level, 1, None)?;
        let (seats, from) = (save.seats_for(1), world.snapshot().tick);
        for tick in from + 1..=from + ticks {
            let input = sim::determinism::pilot(1, tick, seats);
            sim::step(
                &mut world,
                tick,
                &input,
                rate.msec(tick),
                StepReason::Authority,
            );
        }
        Ok(world)
    })
}

/// `module` of `game` planned as `discovery`'s install has it, for
/// (`seats`, `players`).
pub(crate) fn planned(
    discovery: &vfs::Discovery,
    game: vfs::Game,
    module: &str,
    (seats, players): (u8, u8),
) -> Result<Level, String> {
    let source = InstallSource::with_module(discovery, game, Some(module))?;
    // Animation lengths stood in, as the launcher's own module checks plan
    // (`launcher::probe`): JA's skeleton is the game's to read.
    let anims = Arc::new(sim::motion::AnimTable::stand_in());
    let request = session::Request::new(game.key(), module, None, seats).with_players(players);
    session::plan(&source, &request, anims)
        .map(|plan| plan.level)
        .map_err(|error| format!("{module}: {error}"))
}

/// `check` run, a panic in it an error saying what it was doing.
pub(crate) fn caught<T>(
    what: &str,
    check: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(check)).unwrap_or_else(|panic| {
        Err(format!(
            "{what} panicked: {}",
            saves::corpus::panic_words(&*panic)
        ))
    })
}
