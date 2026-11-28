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
         Generated from `crates/commands` by `cargo xtask console` (do not edit\n\
         by hand). `kjk commands` prints this list; `kjk commands --json`\n\
         prints it as JSON.\n\
         \n\
         ## Syntax\n\
         \n\
         - Commands are case-insensitive and separated by `;` or newlines.\n\
         - Use double quotes (`\"...\"`) to include spaces in an argument.\n\
         - Arguments in `[BRACKETS]` are optional.\n\
         - `WHO` is `me` or a party slot number starting at `1` (defaults to your character).\n\
         - Headless `--cmds` scripts accept every command below (`docs/RUN.md`).\n\
         \n\
         ## Cheats & Permissions\n\
         \n\
         Commands marked **Cheat.** always work on Debug Mode and combat test\n\
         maps. In normal play they require cheats to be enabled:\n\
         \n\
         - **Solo:** enabled when Gameplay -> Console is on, or when running\n\
           with `--cmds` or `KJK_CONSOLE=1`.\n\
         - **Co-op:** set by the host in the lobby (`Cheats`: `Off`,\n\
           `Host only` [default], or `All players`). `warp` and `load` are\n\
           always host-only. Players can only edit their own character and\n\
           unclaimed companions.\n\
         - Using any cheat outside test maps permanently marks the game and\n\
           its saves **Cheats used**.\n\
         \n\
         ## Menus & Mods\n\
         \n\
         The console also opens at the main menu and in the co-op lobby\n\
         (main-menu commands cannot run inside a lobby). Mods can add custom\n\
         cheat commands via `kjkconsole.2da` (`docs/MODS.md`); list them with\n\
         `mods commands`.\n",
    );
    for group in Group::ALL {
        let _ = write!(page, "\n## {}\n", group.title());
        for spec in table().iter().filter(|spec| spec.group == group) {
            let _ = write!(page, "\n### `{}`\n\n{}\n", help::usage(spec), spec.help);
            let aliases = help::aliases(spec);
            if !aliases.is_empty() {
                let quoted: Vec<String> =
                    aliases.iter().map(|alias| format!("`{alias}`")).collect();
                let _ = write!(page, "\nAliases: {}.\n", quoted.join(", "));
            }
            let _ = write!(page, "\n{}", help::place(spec));
            if spec.kind == Kind::Cheat {
                page.push_str(" Cheat.");
            }
            let _ = writeln!(page, " Example: `{}`", spec.example);
        }
    }
    page
}
