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
                println!("    Also accepted: {}", aliases.join(", "));
            }
            println!("    {}", spec.help);
        }
    }
    ExitCode::SUCCESS
}

/// Every line of a parsed script that its parser handed to the table
/// (`console::Command::Line`), checked against the table: an unknown
/// command or a wrong argument is the error.
pub fn check_script(script: &[console::Command]) -> Result<(), String> {
    for command in script {
        if let console::Command::Line(line) = command {
            commands::check(line).map_err(|why| format!("{line:?}: {why}"))?;
        }
    }
    Ok(())
}
