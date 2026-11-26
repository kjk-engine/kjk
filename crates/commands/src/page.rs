//! `docs/CONSOLE.md`, written from the table (`cargo xtask console`). The
//! approved test `console_page` fails when the page in the repository is
//! not this page.

use std::fmt::Write as _;

use crate::{Group, Kind, help, table};

/// The documentation page of every command.
pub fn markdown() -> String {
    let mut page = String::from(
        "# Console commands\n\
         \n\
         This page is written from the table of commands in `crates/commands` by\n\
         `cargo xtask console`. Do not edit it by hand. `kjk commands` prints the\n\
         same list, and `kjk commands --json` prints it as JSON.\n\
         \n\
         The `--cmds` scripts of headless runs accept every command on this page\n\
         (`docs/RUN.md`). Commands are separated by `;` or by new lines. Double\n\
         quotes group words into one word. A command's name may be typed in upper\n\
         or lower case. The launcher checks every line of a script before the run\n\
         starts, and a line with an unknown command or a wrong argument ends the\n\
         run with exit code 2.\n\
         \n\
         A command that changes the game sends an order to the simulation, as\n\
         the keys of the Debug Mode map do. These commands are marked \"Changes\n\
         the game\". On the Debug Mode map and the other combat test levels\n\
         they always work. Elsewhere they work only when the session's cheat\n\
         rule allows them. The rule is Off by default. Under the rule Host only,\n\
         only the host can use them. A run with a command script (`--cmds`), or\n\
         with the console setting turned on, plays under the rule Host only.\n\
         The first such command outside the combat test levels marks the game\n\
         as \"Cheats used\", and a save keeps the mark. A player\n\
         can change only their own character, the companion they lead, or a\n\
         companion that no other player controls.\n\
         \n\
         An argument in brackets may be left out. WHO is a party member: `me`, or\n\
         a party member's number, where 1 is the player's own character. Without\n\
         WHO, a command changes the player's own character.\n",
    );
    for group in Group::ALL {
        let _ = write!(page, "\n## {}\n", group.title());
        for spec in table().iter().filter(|spec| spec.group == group) {
            let _ = write!(page, "\n### `{}`\n\n{}\n", help::usage(spec), spec.help);
            let aliases = help::aliases(spec);
            if !aliases.is_empty() {
                let quoted: Vec<String> =
                    aliases.iter().map(|alias| format!("`{alias}`")).collect();
                let _ = write!(page, "\nAlso accepted: {}.\n", quoted.join(", "));
            }
            let _ = write!(page, "\n{}", help::place(spec));
            if spec.kind == Kind::Cheat {
                page.push_str(" Changes the game.");
            }
            let _ = writeln!(page, " Example: `{}`", spec.example);
        }
    }
    page
}
