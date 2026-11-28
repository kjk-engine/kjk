//! The commands that mods add (`kjkconsole.2da`, docs/MODS.md).
//!
//! A mod puts a table named `kjkconsole.2da` in its override folder. Each
//! row is one command that runs one of the mod's scripts. The game reads the
//! table through the resolver, so a table in a Steam Workshop item or in a
//! module's archive is found as any other 2DA is. A row that cannot work is
//! skipped, and the reason is kept (`Table::skipped`) for `mods commands`
//! and the log. A missing or damaged table means no mod commands. Nothing
//! in a table can stop the game.
//!
//! A mod command changes the game: it is a cheat, and it runs as the order
//! `CheatOrder::Script`. The script's caller is the player's character, or
//! the party member a `who` argument names. In KOTOR 2 the script reads the
//! `int` arguments with `GetScriptParameter(1)` to `GetScriptParameter(5)`,
//! and the `text`, `item` or `creature` argument with
//! `GetScriptStringParameter()`.

use sim::action::{Action, CheatOrder, ScriptParams};

use crate::{Arg, ArgKind, Effect, Facts, Kind, Place, Refusal};

/// The table's resref.
pub const TABLE: &str = "kjkconsole";

/// The most `int` arguments a command may have: KOTOR 2's
/// `GetScriptParameter` reads five.
pub const NUMBERS_MOST: usize = 5;

/// The longest name a command may have.
const NAME_LONGEST: usize = 32;

/// The longest resref a script may have.
const RESREF_LONGEST: usize = 16;

/// The names of the `int` arguments in a usage line, in order.
const NUMBER_NAMES: [&str; NUMBERS_MOST] = ["NUMBER1", "NUMBER2", "NUMBER3", "NUMBER4", "NUMBER5"];

/// One command a mod added.
#[derive(Debug, Clone)]
pub struct Command {
    /// The command's name, in lower case.
    pub name: String,
    /// The script it runs, by resref, in lower case.
    pub script: String,
    /// The arguments, in the table's order.
    pub args: Vec<Arg>,
    /// The help sentence, from `dialog.tlk` or the `helptext` column.
    pub help: String,
    /// The table's row that added it, from 0.
    pub row: usize,
}

/// A row of the table that was skipped, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The row, from 0.
    pub row: usize,
    /// The row's `label`, as written, or empty.
    pub label: String,
    /// Why the row was skipped, as one plain sentence.
    pub why: String,
}

/// The mod commands of the running game.
#[derive(Debug, Clone, Default)]
pub struct Table {
    /// Whether a `kjkconsole.2da` was found.
    pub found: bool,
    /// Why the table could not be read, when it could not.
    pub problem: Option<String>,
    /// The commands, in the table's order.
    pub commands: Vec<Command>,
    /// The rows that were skipped, in the table's order.
    pub skipped: Vec<Skipped>,
}

impl Table {
    /// The command named `name`, regardless of upper or lower case.
    pub fn find(&self, name: &str) -> Option<&Command> {
        let name = name.to_ascii_lowercase();
        self.commands.iter().find(|command| command.name == name)
    }

    /// The lines of `mods commands`: each command with its usage and
    /// script, then each skipped row and why.
    pub fn listed(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(problem) = &self.problem {
            lines.push(format!("Failed to read {TABLE}.2da: {problem}"));
            return lines;
        }
        if !self.found {
            lines.push(format!(
                "No mod commands found. Mods can add commands via {TABLE}.2da in Override."
            ));
            return lines;
        }
        if self.commands.is_empty() {
            lines.push(format!("{TABLE}.2da has no valid commands."));
        }
        for command in &self.commands {
            lines.push(format!(
                "{} -> script {} (row {})",
                usage(command),
                command.script,
                command.row
            ));
        }
        for skipped in &self.skipped {
            lines.push(skipped_line(skipped));
        }
        lines
    }
}

/// A skipped row as a line for `mods commands` and the log.
pub fn skipped_line(skipped: &Skipped) -> String {
    match skipped.label.is_empty() {
        true => format!("Skipped {TABLE}.2da row {}: {}", skipped.row, skipped.why),
        false => format!(
            "Skipped {TABLE}.2da row {} ({}): {}",
            skipped.row, skipped.label, skipped.why
        ),
    }
}

/// The usage line of a mod command.
pub fn usage(command: &Command) -> String {
    crate::help::usage_of(&command.name, &command.args)
}

/// The table read from `bytes`, the stored `kjkconsole.2da` (`None` when no
/// layer has one, an error when it does not read). `parameters` says
/// whether the game's scripts can read parameters: KOTOR 2's can and KOTOR
/// 1's cannot, so in KOTOR 1 a row may only have a `who` argument.
/// `talk` gives the text of a string reference in `dialog.tlk`, as the
/// game's bytes.
pub fn read(
    bytes: Option<Result<Vec<u8>, String>>,
    parameters: bool,
    talk: &mut dyn FnMut(u32) -> Option<Vec<u8>>,
) -> Table {
    let bytes = match bytes {
        None => return Table::default(),
        Some(Err(why)) => return problem(why),
        Some(Ok(bytes)) => bytes,
    };
    let twoda = match fmt_odyssey::twoda::TwoDa::parse(&bytes) {
        Ok(twoda) => twoda,
        Err(error) => return problem(error.to_string()),
    };
    let mut table = Table {
        found: true,
        ..Table::default()
    };
    for row in 0..twoda.cells.len() {
        let label = cell(&twoda, row, "label").unwrap_or_else(String::new);
        match command(&twoda, row, &label, parameters, &table, talk) {
            Ok(command) => table.commands.push(command),
            Err(why) => table.skipped.push(Skipped { row, label, why }),
        }
    }
    table
}

fn problem(why: String) -> Table {
    Table {
        found: true,
        problem: Some(why),
        ..Table::default()
    }
}

/// A cell as the game's text, trimmed of ASCII blanks, or `None` for `****`,
/// an empty cell or a missing column.
fn cell(twoda: &fmt_odyssey::twoda::TwoDa, row: usize, column: &str) -> Option<String> {
    let bytes = twoda.get(row, twoda.column(column)?)?;
    let text = fmt_odyssey::code_page(bytes.trim_ascii());
    (!text.is_empty()).then_some(text)
}

/// Row `row`'s command, or why the row is skipped.
fn command(
    twoda: &fmt_odyssey::twoda::TwoDa,
    row: usize,
    label: &str,
    parameters: bool,
    table: &Table,
    talk: &mut dyn FnMut(u32) -> Option<Vec<u8>>,
) -> Result<Command, String> {
    let name = label.to_ascii_lowercase();
    if name.is_empty() {
        return Err("Missing label.".into());
    }
    let good = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
    if name.len() > NAME_LONGEST || !name.chars().all(good) {
        return Err(format!(
            "Invalid label: use 1-{NAME_LONGEST} letters, digits, or underscores."
        ));
    }
    if taken(&name) {
        return Err(format!("Conflicts with built-in command \"{name}\"."));
    }
    if let Some(first) = table.find(&name) {
        return Err(format!(
            "Duplicate command \"{name}\" (already defined on row {}).",
            first.row
        ));
    }
    let script = cell(twoda, row, "script")
        .unwrap_or_else(String::new)
        .to_ascii_lowercase();
    if script.is_empty() || script.len() > RESREF_LONGEST || !script.chars().all(good) {
        return Err(format!(
            "Invalid script resref: use 1-{RESREF_LONGEST} letters, digits, or underscores."
        ));
    }
    let args = arguments(cell(twoda, row, "args").as_deref(), parameters)?;
    let help = help(twoda, row, talk).unwrap_or_else(|| format!("Run script {script}."));
    Ok(Command {
        name,
        script,
        args,
        help,
        row,
    })
}

/// Whether a built-in command is typed by `name`, or a two-word built-in
/// name starts with it (`arena`).
fn taken(name: &str) -> bool {
    crate::complete::names().any(|known| known == name || known.split(' ').next() == Some(name))
}

/// The row's help: the text of its `help` string reference, else its
/// `helptext`.
fn help(
    twoda: &fmt_odyssey::twoda::TwoDa,
    row: usize,
    talk: &mut dyn FnMut(u32) -> Option<Vec<u8>>,
) -> Option<String> {
    let spoken = cell(twoda, row, "help")
        .and_then(|strref| strref.parse::<u32>().ok())
        .and_then(talk)
        .map(|bytes| fmt_odyssey::code_page(bytes.trim_ascii()))
        .filter(|text| !text.is_empty());
    spoken.or_else(|| cell(twoda, row, "helptext"))
}

/// The arguments the `args` cell lists, or why they cannot work.
fn arguments(cell: Option<&str>, parameters: bool) -> Result<Vec<Arg>, String> {
    let Some(cell) = cell else {
        return Ok(Vec::new());
    };
    let kinds: Vec<String> = (cell.split(','))
        .map(|kind| kind.trim_ascii().to_ascii_lowercase())
        .collect();
    let mut args = Vec::with_capacity(kinds.len());
    let (mut numbers, mut strings, mut whos) = (0, 0, 0);
    for (at, kind) in kinds.iter().enumerate() {
        let last = at + 1 == kinds.len();
        let arg = match kind.as_str() {
            "int" => {
                numbers += 1;
                let name = NUMBER_NAMES.get(numbers - 1).copied().unwrap_or("NUMBER");
                Arg::new(
                    name,
                    ArgKind::Int {
                        min: i64::from(i32::MIN),
                        max: i64::from(i32::MAX),
                    },
                )
            }
            "text" if !last => {
                return Err("\"text\" must be the last argument.".into());
            }
            "text" => {
                strings += 1;
                Arg::new("TEXT", ArgKind::Text)
            }
            "item" => {
                strings += 1;
                Arg::new("ITEM", ArgKind::Item)
            }
            "creature" => {
                strings += 1;
                Arg::new("CREATURE", ArgKind::Creature)
            }
            "who" => {
                whos += 1;
                Arg::optional("WHO", ArgKind::Who)
            }
            other => {
                return Err(format!(
                    "Unknown argument type \"{other}\". Allowed: int, text, item, creature, who."
                ));
            }
        };
        args.push(arg);
    }
    if numbers > NUMBERS_MOST {
        return Err(format!(
            "Too many \"int\" arguments ({numbers}; max {NUMBERS_MOST})."
        ));
    }
    if strings > 1 {
        return Err("Only one string argument (text, item, or creature) is allowed.".into());
    }
    if whos > 1 {
        return Err("Only one \"who\" argument is allowed.".into());
    }
    if !parameters && numbers + strings > 0 {
        return Err("KOTOR 1 scripts don't support parameters; only \"who\" is allowed.".into());
    }
    // A `who` argument is optional, so only other optional arguments may
    // follow it.
    if let Some(at) = args.iter().position(|arg| arg.kind == ArgKind::Who)
        && at + 1 != args.len()
    {
        return Err("\"who\" must be the last argument.".into());
    }
    Ok(args)
}

/// The scripts' parameters from parsed arguments: the numbers in order, and
/// the text, item or creature argument.
fn params(command: &Command, parsed: &crate::Parsed) -> ScriptParams {
    let mut params = ScriptParams::default();
    let mut numbers = params.numbers.iter_mut();
    for (at, arg) in command.args.iter().enumerate() {
        match arg.kind {
            ArgKind::Int { .. } => {
                if let Some(number) = numbers.next() {
                    *number = i32::try_from(parsed.int(at)).unwrap_or(0);
                }
            }
            ArgKind::Text | ArgKind::Item | ArgKind::Creature => {
                params.text = parsed.word(at).unwrap_or("").to_string();
            }
            _ => {}
        }
    }
    params
}

/// A mod command run with `words`, the words after its name.
pub fn run(command: &Command, words: &[String], facts: &Facts) -> Result<Vec<Effect>, Refusal> {
    let refused = crate::place_refused(Place::Game, facts)
        .or_else(|| crate::kind_refused(Kind::Cheat, facts));
    if let Some(refusal) = refused {
        return Err(refusal);
    }
    let usage = || usage(command);
    let parsed = crate::parse(&command.args, words, facts, &usage)?;
    crate::script_known(&command.script, facts)?;
    // With no `who` argument given, the player's own character runs the
    // script (`Parsed::who` of an argument not given).
    let who_at = (command.args.iter())
        .position(|arg| arg.kind == ArgKind::Who)
        .unwrap_or(command.args.len());
    let who = parsed.who(who_at, facts)?;
    let order = CheatOrder::Script {
        resref: command.script.clone(),
        who: Some(who),
        params: params(command, &parsed),
    };
    Ok(vec![
        Effect::Order(Action::Cheat(order)),
        Effect::Print(format!("Running {}.", command.name)),
    ])
}
