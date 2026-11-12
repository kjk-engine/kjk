//! A creature the scene cannot draw is named with the reason. A player's
//! report from Javyar's Cantina on Taris (`tar_m03ae`) showed Bib Surool,
//! the Twi'lek talent scout, missing. That player's game data was not the
//! stock install's. The log said only "not drawn", so the report could not
//! say which `appearance.2da` row or which model was at fault.

use super::{CreatureLoad, Kept, resolver};
use crate::probe_kit::{Checks, check, not_installed};

const MODULE: &str = "tar_m03ae";
const BIB: &str = "n_twilekm001";

/// With a K1 install: Bib Surool is drawn from his own template in the
/// cantina. A look of an `appearance.2da` row past the table's end is not
/// drawn, and the reason names that row.
pub fn probes() -> Checks {
    let game = vfs::Game::Kotor1;
    let discovery = crate::discover();
    if discovery.get(game).is_none() {
        let skipped = Ok(not_installed(game));
        return vec![
            ("bib_surool_drawn", skipped.clone()),
            ("undrawn_creature_named", skipped),
        ];
    }
    let opened = resolver(&discovery, game, Some(MODULE));
    let (drawn, named) = match &opened {
        Ok(resolver) => (bib_drawn(resolver, game), unknown_row_named(resolver, game)),
        Err(error) => (Err(error.clone()), Err(error.clone())),
    };
    vec![
        ("bib_surool_drawn", drawn),
        ("undrawn_creature_named", named),
    ]
}

fn bib_drawn(resolver: &vfs::resolver::Resolver, game: vfs::Game) -> Result<String, String> {
    let mut models = Vec::new();
    let view =
        CreatureLoad::default().place((resolver, game), (1, BIB), ([0.0; 3], 0.0), &mut models)?;
    let names: Vec<&str> = models
        .iter()
        .map(|placed| placed.ir.name.as_str())
        .collect();
    check(
        view.is_some()
            && names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("n_twilekm")),
        format!("{BIB} draws {names:?}"),
    )
}

fn unknown_row_named(
    resolver: &vfs::resolver::Resolver,
    game: vfs::Game,
) -> Result<String, String> {
    let rows = resolver
        .two_da("appearance")
        .ok_or("no appearance.2da")?
        .cells
        .len();
    let row = u32::try_from(rows).map_err(|error| error.to_string())?;
    let kept = Kept {
        appearance: row,
        armour: None,
        headgear: None,
        weapons: [None, None],
    };
    let mut load = CreatureLoad::default();
    let noted = load.noted();
    let mut models = Vec::new();
    let view = load.place_kept(
        (resolver, game),
        (1, BIB, &kept),
        ([0.0; 3], 0.0),
        &mut models,
    )?;
    let why = load.why_not_drawn(noted);
    check(
        view.is_none() && models.is_empty() && why.contains(&format!("NoAppearanceRow({row})")),
        format!("row {row} of {rows}: {why}"),
    )
}
