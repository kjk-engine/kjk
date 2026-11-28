//! Commands that change the area: spawn and clearfoes.

use sim::action::{Action, ArenaOrder};

use super::{Ran, order};
use crate::{Alias, Arg, ArgKind, Facts, Group, Kind, Parsed, Place, Spec};

pub static SPECS: &[Spec] = &[
    Spec {
        name: "spawn",
        aliases: &[Alias::plain("arena spawn")],
        args: &[Arg::new("TEMPLATE", ArgKind::Creature)],
        help: "Spawn a hostile creature (TEMPLATE) in front of you.",
        example: "spawn g_sithtroop01",
        group: Group::World,
        place: Place::Game,
        kind: Kind::Cheat,
        run: spawn,
    },
    Spec {
        name: "clearfoes",
        aliases: &[Alias::plain("arena clear")],
        args: &[],
        help: "Remove all non-party creatures from the map.",
        example: "clearfoes",
        group: Group::World,
        place: Place::Arena,
        kind: Kind::Cheat,
        run: clearfoes,
    },
];

fn spawn(parsed: &Parsed, _: &Facts) -> Ran {
    let template = parsed.word(0).unwrap_or("").to_string();
    let said = format!("Spawning {template}.");
    order(Action::Arena(ArenaOrder::Spawn(template)), said)
}

fn clearfoes(_: &Parsed, _: &Facts) -> Ran {
    order(
        Action::Arena(ArenaOrder::Clear),
        "Cleared all non-party creatures.",
    )
}
