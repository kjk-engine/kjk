//! `kjk commands`: the console's table of commands (`crates/commands`),
//! and the check of a `--cmds` script's lines against it before the run
//! starts.

use std::process::ExitCode;

/// Prints every command: its usage, the other names it is accepted by and
/// its help, or the same as JSON.
pub fn run(json: bool) -> ExitCode {
    let table = commands::table();
    if json {
        let specs: Vec<serde_json::Value> = (table.iter())
            .map(|spec| {
                serde_json::json!({
                    "name": spec.name,
                    "usage": commands::help::usage(spec),
                    "aliases": commands::help::aliases(spec),
                    "help": spec.help,
                    "example": spec.example,
                    "group": spec.group.title(),
                    "where": commands::help::place(spec),
                    "changes_the_game": spec.kind == commands::Kind::Cheat,
                })
            })
            .collect();
        println!("{}", serde_json::Value::from(specs));
        return ExitCode::SUCCESS;
    }
    for group in commands::Group::ALL {
        println!("{}:", group.title());
        for spec in table.iter().filter(|spec| spec.group == group) {
            println!("  {}", commands::help::usage(spec));
            let aliases = commands::help::aliases(spec);
            if !aliases.is_empty() {
                println!("    Aliases: {}", aliases.join(", "));
            }
            println!("    {}", spec.help);
        }
    }
    ExitCode::SUCCESS
}

/// Every line of a parsed script that its parser handed to the table
/// (`console::Command::Line`), checked against the table: an unknown
/// command or a wrong argument is the error. A line that the built-in
/// commands refuse may name a command that a mod adds, so the mods' tables
/// of both games are read then, once, and the line is checked again.
pub fn check_script(script: &[console::Command]) -> Result<(), String> {
    let mut tables: Option<Vec<commands::mods::Table>> = None;
    for command in script {
        let console::Command::Line(line) = command else {
            continue;
        };
        if commands::check(line).is_ok() {
            continue;
        }
        let tables = tables.get_or_insert_with(mod_tables);
        let tables: Vec<&commands::mods::Table> = tables.iter().collect();
        commands::check_with(line, &tables).map_err(|why| format!("{line:?}: {why}"))?;
    }
    Ok(())
}

/// The mod commands of each KOTOR game installed (`kjkconsole.2da`, read
/// from the game's files as they stand outside any module). Their help
/// texts are not read.
fn mod_tables() -> Vec<commands::mods::Table> {
    let discovery = crate::discover();
    [vfs::Game::Kotor1, vfs::Game::Kotor2]
        .into_iter()
        .filter_map(|game| {
            let install = discovery.get(game)?;
            let index = vfs::DirIndex::of_install(install).ok()?;
            let resolver = vfs::resolver::Resolver::open(
                game,
                &index,
                None,
                vfs::resolver::TextureQuality::High,
            )
            .ok()?;
            let bytes = (resolver.find(commands::mods::TABLE, fmt_odyssey::ResType::TWO_DA))
                .map(|found| found.location.read().map_err(|error| error.to_string()));
            let parameters = game == vfs::Game::Kotor2;
            Some(commands::mods::read(bytes, parameters, &mut |_| None))
        })
        .collect()
}
