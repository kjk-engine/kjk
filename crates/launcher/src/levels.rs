//! The levels a recording or save names, built here from this machine's
//! installs: the verbs' replays and loads, and `kjk report replay` (which a
//! player's build has too, so this is not behind `probes`).

use sim::Level;
use std::sync::Arc;

use kjk_game::lifecycle::Installer;

/// The level a snapshot is in (its module planned here, entered at its
/// entry, for `seats` players, its encounters for the players it was
/// planned for), checked to be the one the snapshot names.
pub fn level_of_place(
    installer: &Installer,
    place: &sim::Place,
    seats: u8,
) -> Result<Level, String> {
    let level = planned(installer, place, seats)?;
    if level.digest() != place.digest {
        return Err(format!(
            "{}: the data here is not the data the snapshot was taken with",
            place.module
        ));
    }
    Ok(level)
}

/// The same, unchecked: whatever this install's data makes of it.
fn planned(installer: &Installer, place: &sim::Place, seats: u8) -> Result<Level, String> {
    let request = installer
        .request(&place.module, None, seats)
        .with_players(place.players);
    Ok(installer.prepare(&request, false)?.plan.level)
}

/// Installers by game, each made once (its base archives opened once),
/// with the JA skeleton's animation lengths loaded once for all of them.
#[derive(Default)]
pub(crate) struct Installers {
    anims: Option<Arc<sim::motion::AnimTable>>,
    made: Vec<Arc<Installer>>,
}

impl Installers {
    fn anims(&mut self, discovery: &vfs::Discovery) -> Arc<sim::motion::AnimTable> {
        self.anims
            .get_or_insert_with(|| Arc::new(kjk_game::stage::anims(discovery).0))
            .clone()
    }

    /// The installer for `game` (`k1` or `tsl`).
    pub(crate) fn get(
        &mut self,
        discovery: &vfs::Discovery,
        game: &str,
    ) -> Result<Arc<Installer>, String> {
        let game = vfs::Game::from_key(game)
            .filter(|game| *game != vfs::Game::JediAcademy)
            .ok_or(format!("{game}: not a KOTOR game"))?;
        if let Some(made) = self.made.iter().find(|made| made.game == game) {
            return Ok(made.clone());
        }
        let anims = self.anims(discovery);
        let made = Arc::new(Installer::new(discovery, game, anims)?);
        self.made.push(made.clone());
        Ok(made)
    }

    /// The level a save or recording names, built here: the placeholder
    /// arena, the movement course, or its module, planned for `seats`
    /// players; refused unless it is the very level named
    /// (`LevelId::check`).
    pub(crate) fn level_named(
        &mut self,
        discovery: &vfs::Discovery,
        id: &sim::LevelId,
        place: Option<&sim::Place>,
        seats: u8,
    ) -> Result<Level, String> {
        let level = match place {
            Some(place) => level_of_place(&*self.get(discovery, &place.game)?, place, seats)?,
            None => self.level(discovery, id, None, seats)?,
        };
        id.check(&level).map_err(|error| error.to_string())?;
        Ok(level)
    }

    /// The same, unchecked: the level this machine's data makes of it,
    /// which another edition or a mod makes differently.
    pub(crate) fn level(
        &mut self,
        discovery: &vfs::Discovery,
        id: &sim::LevelId,
        place: Option<&sim::Place>,
        seats: u8,
    ) -> Result<Level, String> {
        Ok(match place {
            None if id.name == "arena" => Level::arena(),
            None if id.name == "course" => Level::new(
                "course",
                Arc::new(kjk_game::course::course()),
                self.anims(discovery),
            ),
            None if id.name == "duel" => kjk_game::stage::duel_level(self.anims(discovery)),
            None if id.name == "ring" => kjk_game::stage::ring(&self.anims(discovery)),
            None => return Err(format!("{}: no such level without a module", id.name)),
            Some(place) => planned(&*self.get(discovery, &place.game)?, place, seats)?,
        })
    }
}
