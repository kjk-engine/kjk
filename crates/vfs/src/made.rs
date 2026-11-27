//! Resources a run makes in memory, as the highest layer of every resolver
//! opened after them: rooms of a new map that kjk builds from another
//! game's level (a Jedi Academy room, `asset_ja`), with their textures. No
//! file of them is written. They are made again, from the player's own
//! install, each time a run needs them.
//!
//! A run puts them before it opens any resolver that should hold them.
//! Every resolver of their game opened after that (`Resolver::base`) has
//! them as a layer over every other, the project's folder too (`project`).
//!
//! Nothing here is ever dropped. Each program fills the store once, at its
//! start: the game with the rooms its mods' maps place, the level editor
//! with the rooms its projects cut. Any level loaded later in the run can
//! ask for them, so they are kept until the program closes. A room made
//! again takes the place of the one made before under its name (`put`). A
//! program that makes rooms again and again while it runs, under new
//! names, needs a way to drop the old ones, which it should bring with it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use fmt_odyssey::ResType;

use crate::Game;
use crate::resolver::{Layer, Location, Rank};

/// The layer's name in `kjk ls --shadowed`.
pub const LAYER: &str = "made";

/// By game key (`Game::key`), resref and type.
type Made = BTreeMap<(&'static str, String, ResType), Arc<[u8]>>;

static MADE: Mutex<Made> = Mutex::new(BTreeMap::new());

fn made() -> std::sync::MutexGuard<'static, Made> {
    MADE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Makes `bytes` the resource `resref` of type `restype` of `game`, in place
/// of any made before under that name.
pub fn put(game: Game, resref: &str, restype: ResType, bytes: Vec<u8>) {
    made().insert(
        (game.key(), resref.to_ascii_lowercase(), restype),
        bytes.into(),
    );
}

/// Whether a resource of that name and type was made for `game`.
pub fn holds(game: Game, resref: &str, restype: ResType) -> bool {
    made().contains_key(&(game.key(), resref.to_ascii_lowercase(), restype))
}

/// The resource `resref` of type `restype` made for `game`, if one was.
pub fn get(game: Game, resref: &str, restype: ResType) -> Option<Arc<[u8]>> {
    (made().get(&(game.key(), resref.to_ascii_lowercase(), restype))).cloned()
}

/// The resources made for `game` as a layer; none when there are none.
pub(crate) fn layer(game: Game) -> Option<Layer> {
    let resources: BTreeMap<_, _> = (made().iter())
        .filter(|((made_for, ..), _)| *made_for == game.key())
        .map(|((_, resref, restype), bytes)| {
            let location = Location {
                path: PathBuf::from(format!(
                    "{LAYER}/{resref}.{}",
                    restype.extension().unwrap_or("bin")
                )),
                offset: 0,
                size: bytes.len() as u64,
                held: Some(bytes.clone()),
            };
            ((resref.clone(), *restype), location)
        })
        .collect();
    (!resources.is_empty()).then(|| Layer {
        name: LAYER.into(),
        rank: Rank::Directory,
        resources,
    })
}
