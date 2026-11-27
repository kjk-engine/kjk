//! The console's table of commands (`docs/CONSOLE.md`).
//!
//! Every command the console and the `--cmds` scripts of headless runs
//! accept is one `Spec` in `table`. A spec names the command, the other
//! names it is accepted by, its arguments, its help sentence, where it may
//! run and what it changes. Its `run` function is pure. It reads the parsed
//! arguments and the `Facts` of the moment, and returns what the command
//! asks for as `Effect` values, or a `Refusal` that tells the player why
//! not. The game carries out the effects. A command that changes the game
//! asks for an order to the simulation (`Effect::Order`), so nothing outside
//! the simulation writes to the game's state.
//!
//! The crate has no Bevy, so the launcher's check of `--cmds` lines,
//! `kjk commands` and the documentation page read the table without the
//! game.

mod builtin;
pub mod complete;
pub mod help;
pub mod page;
#[cfg(feature = "probes")]
pub mod probes;
pub mod words;

use std::fmt;

use sim::SimId;
use sim::action::Action;

/// One command of the table.
pub struct Spec {
    /// The command's name, in lower case.
    pub name: &'static str,
    /// The other names the command is accepted by.
    pub aliases: &'static [Alias],
    /// The arguments, in order. Optional ones come last.
    pub args: &'static [Arg],
    /// One plain sentence or two that say what the command does.
    pub help: &'static str,
    /// A line that uses the command.
    pub example: &'static str,
    /// The group `help` lists the command in.
    pub group: Group,
    pub place: Place,
    pub kind: Kind,
    /// What the command asks for. It cannot touch the game.
    pub run: fn(&Parsed, &Facts) -> Result<Vec<Effect>, Refusal>,
}

/// Another name a command is accepted by. A name may be two words
/// (`arena spawn`). `words` are put in front of the typed arguments, so
/// `setstrength 18` runs `attribute strength 18`.
pub struct Alias {
    pub name: &'static str,
    pub words: &'static [&'static str],
}

impl Alias {
    /// A plain other name.
    pub const fn plain(name: &'static str) -> Alias {
        Alias { name, words: &[] }
    }
}

/// One argument of a command.
pub struct Arg {
    /// The name shown in the usage line, in capitals.
    pub name: &'static str,
    pub kind: ArgKind,
    pub optional: bool,
}

impl Arg {
    pub const fn new(name: &'static str, kind: ArgKind) -> Arg {
        Arg {
            name,
            kind,
            optional: false,
        }
    }

    pub const fn optional(name: &'static str, kind: ArgKind) -> Arg {
        Arg {
            name,
            kind,
            optional: true,
        }
    }
}

/// What an argument may be. Each kind checks a word, converts it and
/// completes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// A whole number from `min` to `max`.
    Int { min: i64, max: i64 },
    /// One word.
    Word,
    /// The rest of the line.
    Text,
    /// A creature template (`.utc`), checked against the game's list when
    /// the game has given one.
    Creature,
    /// An item template (`.uti`), checked against the game's list when the
    /// game has given one.
    Item,
    /// A party member: `me`, or a party member's number from 1.
    Who,
    /// An attribute, by its name or its number from 0 (`ABILITIES`).
    Ability,
    /// A skill, by its name or its `skills.2da` row (`SKILLS`).
    Skill,
    /// A class, by its `classes.2da` row or its label.
    Class,
    /// A command's name.
    CommandName,
}

/// The attributes' names, in the order of the `Ability` order's numbers.
pub const ABILITIES: [&str; 6] = [
    "strength",
    "dexterity",
    "constitution",
    "intelligence",
    "wisdom",
    "charisma",
];

/// The skills' names, in the order of `skills.2da`'s rows. The second name
/// of a pair is also accepted.
pub const SKILLS: [(&str, &str); 8] = [
    ("computer", "computeruse"),
    ("demolitions", "demolitions"),
    ("stealth", "stealth"),
    ("awareness", "awareness"),
    ("persuade", "persuade"),
    ("repair", "repair"),
    ("security", "security"),
    ("treat", "treatinjury"),
];

/// Where a command may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// At the main menu and in a game.
    Anywhere,
    /// Only while a module is loaded.
    Game,
    /// Only at the main menu, outside the co-op lobby.
    Menu,
    /// Only on a level with arena rules: the Debug Mode map and the other
    /// combat test levels. The simulation carries out the arena's orders
    /// only there.
    Arena,
    /// Only in a `--cmds` script. The script queue runs these itself.
    ScriptOnly,
}

/// What a command changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Nothing in the game.
    Local,
    /// The session on this machine.
    Session,
    /// The game's state, through an order to the simulation.
    Cheat,
}

/// The groups `help` lists commands in, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    General,
    Character,
    World,
    Session,
    Script,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::General,
        Group::Character,
        Group::World,
        Group::Session,
        Group::Script,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::General => "General",
            Group::Character => "Character",
            Group::World => "World",
            Group::Session => "Session",
            Group::Script => "Command scripts",
        }
    }
}

/// A checked and converted argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Word(String),
    /// A table row: an attribute's, a skill's or a class's number.
    Row(u8),
    Who(Who),
}

/// A party member as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Me,
    /// The party member's number, from 1 (the player's own body).
    Party(u8),
}

/// A command's arguments, one for each of its spec's `args`, `None` for an
/// optional one not given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed(pub Vec<Option<Value>>);

impl Parsed {
    fn get(&self, at: usize) -> Option<&Value> {
        self.0.get(at).and_then(Option::as_ref)
    }

    /// The number at `at`, 0 when there is none.
    pub fn int(&self, at: usize) -> i64 {
        match self.get(at) {
            Some(Value::Int(n)) => *n,
            _ => 0,
        }
    }

    /// The number at `at` as a byte. The argument's limits keep it in range.
    pub fn byte(&self, at: usize) -> u8 {
        u8::try_from(self.int(at).clamp(0, i64::from(u8::MAX))).unwrap_or(u8::MAX)
    }

    pub fn word(&self, at: usize) -> Option<&str> {
        match self.get(at) {
            Some(Value::Word(word)) => Some(word),
            _ => None,
        }
    }

    pub fn row(&self, at: usize) -> Option<u8> {
        match self.get(at) {
            Some(Value::Row(row)) => Some(*row),
            _ => None,
        }
    }

    /// Whether the argument at `at` was given.
    pub fn given(&self, at: usize) -> bool {
        self.get(at).is_some()
    }

    /// The party member at `at`, the player's own body when none is given.
    pub fn who(&self, at: usize, facts: &Facts) -> Result<SimId, Refusal> {
        let who = match self.get(at) {
            Some(Value::Who(who)) => *who,
            _ => Who::Me,
        };
        match who {
            Who::Me => facts
                .me
                .ok_or_else(|| Refusal::new("You have no character in this area.")),
            Who::Party(n) => (usize::from(n).checked_sub(1))
                .and_then(|at| facts.party.get(at).copied())
                .ok_or_else(|| Refusal(format!("Party member {n} does not exist."))),
        }
    }
}

/// A read-only description of the moment, filled by the game before each
/// command. A list the game has not given (`None`) is not checked against.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// A module is loaded and its world is up.
    pub loaded: bool,
    /// The player is at the main menu: no game is loaded or loading, and
    /// the co-op lobby is not open.
    pub menu: bool,
    /// The co-op lobby is open.
    pub lobby: bool,
    /// The level has arena rules.
    pub arena: bool,
    /// The module's name.
    pub module: Option<String>,
    /// Where the player's body stands (game space, metres).
    pub position: Option<[f32; 3]>,
    /// The player's own body.
    pub me: Option<SimId>,
    /// The party, the player's body first.
    pub party: Vec<SimId>,
    /// Every creature template the game knows, in lower case.
    pub creatures: Option<Vec<String>>,
    /// The classes, as `classes.2da` row and label.
    pub classes: Option<Vec<(u8, String)>>,
    /// Every item template the game knows, in lower case.
    pub items: Option<Vec<String>>,
    /// What the player's crosshair is on.
    pub target: Option<SimId>,
    /// The party's credits and experience, when a world is up.
    pub credits: Option<i32>,
    pub xp: Option<i32>,
    /// The experience the party's next level needs, when it is known.
    pub next_level_xp: Option<i32>,
    /// Who may cheat in this session (`sim::cheats::Rule`).
    pub cheats: sim::cheats::Rule,
    /// This player is the host (the first seat). In solo play the player
    /// is the host.
    pub host: bool,
    /// kjk's version text.
    pub version: String,
}

/// What a command asks for, as data.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// An order to the simulation.
    Order(Action),
    /// The arena's `Restore` order for `who`, with the Debug Mode map's
    /// starting character, which only the game can make.
    Restore { who: SimId },
    /// A line in the console's log.
    Print(String),
    /// The console's log emptied.
    ClearLog,
    /// Words for the game's screens, as a script's `ui` words.
    Ui(Vec<String>),
    /// Words for the view's visual effects, as a script's `vfx` words.
    Vfx(Vec<String>),
    /// A game started, loaded or joined, or a module loaded in the game.
    Start(Start),
    /// The saves listed in the console's log.
    Saves,
    /// The mods the game reads listed in the console's log.
    Mods,
}

/// What `Effect::Start` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// At the main menu, a new solo game in `module` with a quick
    /// character. In a game, `module` loaded for everyone. Either way the
    /// game is marked as cheated (`sim::action::CheatOrder::Warped`).
    Warp {
        module: String,
        arrival: Option<String>,
    },
    /// The Debug Mode map, played alone.
    Sandbox,
    /// The save with this number loaded.
    Load(String),
    /// A co-op lobby for a new game, or for the save with this number.
    Host(Option<String>),
    /// The game hosted at this address joined, as `name` when one is given.
    Join {
        address: String,
        name: Option<String>,
    },
}

/// Why a command did not run, as one plain sentence for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl Refusal {
    pub fn new(text: &str) -> Refusal {
        Refusal(text.to_string())
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Every command, in the order `help` lists them within a group.
pub fn table() -> &'static [&'static Spec] {
    &builtin::TABLE
}

/// The command named `name` (by its name or a plain alias), regardless of
/// upper or lower case.
pub fn find(name: &str) -> Option<&'static Spec> {
    let name = name.to_ascii_lowercase();
    table().iter().copied().find(|spec| {
        spec.name == name
            || (spec.aliases.iter()).any(|alias| alias.name == name && alias.words.is_empty())
    })
}

/// The command a line's words name, and its arguments: the alias's own
/// words, then the rest of the line. A two-word name (`arena spawn`) is
/// tried before a one-word name.
pub fn resolve(words: &[String]) -> Result<(&'static Spec, Vec<String>), Refusal> {
    let Some(first) = words.first() else {
        return Err(Refusal::new("Type a command. Type help for a list."));
    };
    let first = first.to_ascii_lowercase();
    let two = words
        .get(1)
        .map(|second| format!("{first} {}", second.to_ascii_lowercase()));
    let named = |name: &str| {
        table().iter().copied().find_map(|spec| {
            if spec.name == name {
                return Some((spec, &[][..]));
            }
            (spec.aliases.iter())
                .find(|alias| alias.name == name)
                .map(|alias| (spec, alias.words))
        })
    };
    let (spec, preset, used) = match two.as_deref().and_then(named) {
        Some((spec, preset)) => (spec, preset, 2),
        None => match named(&first) {
            Some((spec, preset)) => (spec, preset, 1),
            None => return Err(unknown(&first)),
        },
    };
    let args = (preset.iter().map(|word| word.to_string()))
        .chain(words.iter().skip(used).cloned())
        .collect();
    Ok((spec, args))
}

/// The refusal for a name that is no command, with the closest name.
fn unknown(name: &str) -> Refusal {
    match complete::closest(name) {
        Some(close) => Refusal(format!(
            "Unknown command \"{name}\". Did you mean \"{close}\"?"
        )),
        None => Refusal(format!("Unknown command \"{name}\". Type help for a list.")),
    }
}

/// `args` checked and converted by `spec`'s arguments.
pub fn parse_args(spec: &Spec, args: &[String], facts: &Facts) -> Result<Parsed, Refusal> {
    let usage = || help::usage(spec);
    let mut values = Vec::with_capacity(spec.args.len());
    let mut rest = args.iter();
    for arg in spec.args {
        if arg.kind == ArgKind::Text {
            let text: Vec<&str> = rest.by_ref().map(String::as_str).collect();
            if text.is_empty() {
                if arg.optional {
                    values.push(None);
                    continue;
                }
                return Err(missing(arg, &usage()));
            }
            values.push(Some(Value::Word(text.join(" "))));
            continue;
        }
        let Some(word) = rest.next() else {
            if arg.optional {
                values.push(None);
                continue;
            }
            return Err(missing(arg, &usage()));
        };
        values.push(Some(value(arg, word, facts, &usage)?));
    }
    if rest.next().is_some() {
        return Err(Refusal(format!("Too many words. Usage: {}", usage())));
    }
    Ok(Parsed(values))
}

fn missing(arg: &Arg, usage: &str) -> Refusal {
    Refusal(format!("{} is missing. Usage: {usage}", arg.name))
}

/// One word checked and converted by its argument's kind.
fn value(
    arg: &Arg,
    word: &str,
    facts: &Facts,
    usage: &dyn Fn() -> String,
) -> Result<Value, Refusal> {
    let lower = word.to_ascii_lowercase();
    // A name's place in its list, or the place typed as a number.
    let listed = |count: usize, at: Option<usize>| {
        at.or_else(|| lower.parse::<usize>().ok().filter(|at| *at < count))
            .and_then(|at| u8::try_from(at).ok())
    };
    let row = |at: Option<u8>, what: &str| {
        at.map(Value::Row)
            .ok_or_else(|| Refusal(format!("\"{word}\" is not {what}. Usage: {}", usage())))
    };
    match arg.kind {
        ArgKind::Int { min, max } => {
            let Ok(n) = word.parse::<i64>() else {
                return Err(Refusal(format!(
                    "\"{word}\" is not a number. Usage: {}",
                    usage()
                )));
            };
            if !(min..=max).contains(&n) {
                return Err(Refusal(format!(
                    "{} must be between {min} and {max}.",
                    arg.name
                )));
            }
            Ok(Value::Int(n))
        }
        ArgKind::Word | ArgKind::Text => Ok(Value::Word(word.to_string())),
        ArgKind::CommandName => Ok(Value::Word(lower)),
        ArgKind::Creature => match &facts.creatures {
            Some(known) if !known.contains(&lower) => {
                Err(Refusal(format!("No creature has the template \"{word}\".")))
            }
            _ => Ok(Value::Word(lower)),
        },
        ArgKind::Item => match &facts.items {
            Some(known) if !known.contains(&lower) => {
                Err(Refusal(format!("No item has the template \"{word}\".")))
            }
            _ => Ok(Value::Word(lower)),
        },
        ArgKind::Who => match lower.as_str() {
            "me" => Ok(Value::Who(Who::Me)),
            _ => match lower.parse::<u8>() {
                Ok(n) if n >= 1 => Ok(Value::Who(Who::Party(n))),
                _ => Err(Refusal(format!(
                    "\"{word}\" is not a party member. Use me, or a party member's number from 1. Usage: {}",
                    usage()
                ))),
            },
        },
        ArgKind::Ability => row(
            listed(
                ABILITIES.len(),
                ABILITIES.iter().position(|name| *name == lower),
            ),
            "an attribute",
        ),
        ArgKind::Skill => row(
            listed(
                SKILLS.len(),
                (SKILLS.iter()).position(|(name, other)| *name == lower || *other == lower),
            ),
            "a skill",
        ),
        ArgKind::Class => {
            if let Ok(row) = lower.parse::<u8>() {
                return match &facts.classes {
                    Some(classes) if !classes.iter().any(|(at, _)| *at == row) => {
                        Err(Refusal(format!("No class has the number {row}.")))
                    }
                    _ => Ok(Value::Row(row)),
                };
            }
            let by_label = (facts.classes.iter().flatten())
                .find(|(_, label)| label.to_ascii_lowercase() == lower)
                .map(|(row, _)| Value::Row(*row));
            by_label.ok_or_else(|| {
                Refusal(format!(
                    "\"{word}\" is not a class. Use its number or its label. Usage: {}",
                    usage()
                ))
            })
        }
    }
}

/// Why `spec` cannot run now, if it cannot.
pub fn place_refusal(spec: &Spec, facts: &Facts) -> Option<Refusal> {
    match spec.place {
        Place::Anywhere => None,
        Place::ScriptOnly => Some(Refusal::new(
            "This command works only in a command script (--cmds).",
        )),
        Place::Menu if facts.lobby => Some(Refusal::new(
            "Leave the lobby before using this command.",
        )),
        Place::Menu if !facts.menu => Some(Refusal::new(
            "Quit to the main menu before using this command.",
        )),
        Place::Menu => None,
        Place::Game | Place::Arena if !facts.loaded && !facts.menu && !facts.lobby => {
            Some(Refusal::new("Wait for the area to finish loading."))
        }
        Place::Game | Place::Arena if !facts.loaded => {
            Some(Refusal::new("Load a game before using this command."))
        }
        Place::Arena if !facts.arena => Some(Refusal::new(
            "This command works only on the Debug Mode map and the other combat test levels.",
        )),
        Place::Game | Place::Arena => None,
    }
}

/// Why a command that changes the game cannot run under the session's
/// cheat rule, if it cannot. A level with arena rules takes every such
/// command. Outside a game the rule is not checked: there such a command
/// (`warp`) starts a game that the player hosts, or says why it cannot.
/// The simulation checks the rule again for every order.
pub fn cheat_refusal(spec: &Spec, facts: &Facts) -> Option<Refusal> {
    if spec.kind != Kind::Cheat || facts.arena || !facts.loaded {
        return None;
    }
    match facts.cheats {
        sim::cheats::Rule::Off => Some(Refusal::new("Cheats are off in this game.")),
        sim::cheats::Rule::Host if !facts.host => Some(Refusal::new(
            "Only the host can use this command in this game.",
        )),
        sim::cheats::Rule::Host | sim::cheats::Rule::All => None,
    }
}

/// One command's words run: looked up, its arguments checked, its place
/// and the cheat rule checked, then its `run`.
pub fn run(words: &[String], facts: &Facts) -> Result<Vec<Effect>, Refusal> {
    let (spec, args) = resolve(words)?;
    let parsed = parse_args(spec, &args, facts)?;
    if let Some(refusal) = place_refusal(spec, facts).or_else(|| cheat_refusal(spec, facts)) {
        return Err(refusal);
    }
    (spec.run)(&parsed, facts)
}

/// A line run: each of its commands (`words::split`) in turn, with its
/// text and what it asked for.
pub fn run_line(line: &str, facts: &Facts) -> Vec<(String, Result<Vec<Effect>, Refusal>)> {
    words::split(line)
        .into_iter()
        .map(|command| (command.join(" "), run(&command, facts)))
        .collect()
}

/// Whether a line's commands name commands of the table with the right
/// arguments, as far as that is known before the game runs: the launcher's
/// check of `--cmds` lines. Lists of names are not checked, nor where a
/// command may run.
pub fn check(line: &str) -> Result<(), String> {
    let facts = Facts::default();
    for command in words::split(line) {
        let (spec, args) = resolve(&command).map_err(|refusal| refusal.0)?;
        parse_args(spec, &args, &facts).map_err(|refusal| refusal.0)?;
    }
    Ok(())
}

/// Whether the command these words name needs a loaded game. The game
/// holds such a line while a module loads, so a line that a script gives
/// early runs once the world is up.
pub fn needs_game(words: &[String]) -> bool {
    resolve(words).is_ok_and(|(spec, _)| matches!(spec.place, Place::Game | Place::Arena))
}
