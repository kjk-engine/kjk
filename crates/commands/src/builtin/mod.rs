//! The built-in commands, one file for each of `help`'s groups.

mod character;
mod cheats;
mod general;
mod script;
mod session;
mod world;

use std::sync::LazyLock;

use crate::{Effect, Facts, Parsed, Refusal, Spec};

/// Every built-in command, group by group.
pub static TABLE: LazyLock<Vec<&'static Spec>> = LazyLock::new(|| {
    (general::SPECS.iter())
        .chain(character::SPECS)
        .chain(cheats::SPECS)
        .chain(world::SPECS)
        .chain(session::SPECS)
        .chain(script::SPECS)
        .collect()
});

/// What a command's `run` returns.
type Ran = Result<Vec<Effect>, Refusal>;

/// One line printed to the console's log.
fn print(text: impl Into<String>) -> Ran {
    Ok(vec![Effect::Print(text.into())])
}

/// An order to the simulation, and the line that says it was sent.
fn order(action: sim::action::Action, said: impl Into<String>) -> Ran {
    Ok(vec![Effect::Order(action), Effect::Print(said.into())])
}

/// The `run` of a command the script queue runs itself.
fn script_only(_: &Parsed, _: &Facts) -> Ran {
    Err(Refusal::new(
        "This command works only in a command script (--cmds).",
    ))
}
