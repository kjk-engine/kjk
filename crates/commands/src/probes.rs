//! The table's checks (`kjk sim probes --only commands`, and the approved
//! test `commands`).

use sim::SimId;
use sim::action::{Action, ArenaOrder};

use crate::{Effect, Facts, Place, complete, help, run, table, words};

type Checks = Vec<(&'static str, Result<String, String>)>;

/// The facts of a player on the Debug Mode map with one companion, where
/// three creature templates and two classes are known.
fn arena() -> Facts {
    Facts {
        loaded: true,
        arena: true,
        module: Some("debug".into()),
        position: Some([1.0, 2.0, 0.0]),
        me: Some(SimId(7)),
        party: vec![SimId(7), SimId(9)],
        creatures: Some(vec![
            "g_sithtroop01".into(),
            "g_sithtroop05".into(),
            "g_darkjedi01".into(),
        ]),
        classes: Some(vec![
            (3, "JediGuardian".into()),
            (9, "JediWeaponMaster".into()),
        ]),
        version: "test".into(),
    }
}

fn line(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

/// The orders a line gives on the Debug Mode map, or its refusal.
fn orders(text: &str, facts: &Facts) -> Result<Vec<Effect>, String> {
    run(&line(text), facts)
        .map(|effects| {
            (effects.into_iter())
                .filter(|effect| !matches!(effect, Effect::Print(_)))
                .collect()
        })
        .map_err(|refusal| refusal.0)
}

/// Each case's line and what it should give; a failure names every case
/// that gave something else.
fn cases<K: std::fmt::Debug + Copy, T: std::fmt::Debug + PartialEq>(
    all: &[(K, T)],
    got: impl Fn(K) -> T,
) -> Result<String, String> {
    let wrong: Vec<String> = (all.iter())
        .filter_map(|(text, want)| {
            let had = got(*text);
            (had != *want).then(|| format!("{text:?} gave {had:?}, not {want:?}"))
        })
        .collect();
    match wrong.is_empty() {
        true => Ok(format!("{} cases", all.len())),
        false => Err(wrong.join("; ")),
    }
}

pub fn probes() -> Checks {
    vec![
        ("every command is well formed", well_formed()),
        ("the word splitter", splitter()),
        ("arguments accepted and refused", arguments()),
        ("arena words give the same orders", arena_words()),
        ("original names resolve", original_names()),
        ("completion", completion()),
        ("did you mean", did_you_mean()),
        ("where a command may run", places()),
        ("help", help_lines()),
        ("random lines never panic", random_lines()),
    ]
}

/// Every command has a help sentence, an example that passes the
/// launcher's check, a lower-case name, and no name or alias that another
/// command also uses. Optional arguments come last.
fn well_formed() -> Result<String, String> {
    let mut problems = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for spec in table() {
        let names = std::iter::once(spec.name).chain(spec.aliases.iter().map(|alias| alias.name));
        for name in names {
            if seen.contains(&name) {
                problems.push(format!("{name} is used twice"));
            }
            seen.push(name);
        }
        if spec.name.is_empty() || spec.name != spec.name.to_ascii_lowercase() {
            problems.push(format!("{} is not in lower case", spec.name));
        }
        if !spec.help.ends_with('.') || spec.help.len() < 10 {
            problems.push(format!("{} has no help sentence", spec.name));
        }
        if let Err(why) = crate::check(spec.example) {
            problems.push(format!("{}'s example: {why}", spec.name));
        }
        let first_optional = spec.args.iter().position(|arg| arg.optional);
        if first_optional.is_some_and(|at| spec.args[at..].iter().any(|arg| !arg.optional)) {
            problems.push(format!(
                "{} has an optional argument before a required one",
                spec.name
            ));
        }
    }
    match problems.is_empty() {
        true => Ok(format!("{} commands, {} names", table().len(), seen.len())),
        false => Err(problems.join("; ")),
    }
}

/// Quotes group words, `;` and new lines end commands, empty commands go,
/// and a 10,000 character line splits.
fn splitter() -> Result<String, String> {
    let owned = |commands: &[&[&str]]| -> Vec<Vec<String>> {
        (commands.iter())
            .map(|words| words.iter().map(|word| word.to_string()).collect())
            .collect()
    };
    let all: [(&str, Vec<Vec<String>>); 6] = [
        ("spawn g_sithtroop01", owned(&[&["spawn", "g_sithtroop01"]])),
        ("god; ;\n\nclearfoes", owned(&[&["god"], &["clearfoes"]])),
        ("find \"dark jedi\"", owned(&[&["find", "dark jedi"]])),
        ("find \"a;b\" ; god", owned(&[&["find", "a;b"], &["god"]])),
        ("find \"\"", owned(&[&["find", ""]])),
        ("  \t ; ", Vec::new()),
    ];
    cases(&all, words::split)?;
    let long = "x".repeat(10_000);
    match words::split(&format!("find {long}")).as_slice() {
        [command] if command.get(1).is_some_and(|word| word.len() == 10_000) => {
            Ok(format!("{} cases and a 10,000 character word", all.len()))
        }
        other => Err(format!("a long line split into {} commands", other.len())),
    }
}

/// Each argument kind accepts what it should and refuses the rest with
/// its message.
fn arguments() -> Result<String, String> {
    let facts = arena();
    let refused = |text: &str| run(&line(text), &facts).err().map(|refusal| refusal.0);
    cases(
        &[
            ("down abc", Some("\"abc\" is not a number. Usage: down DEATH".into())),
            ("down 20", Some("DEATH must be between 1 and 19.".into())),
            ("down 4", None),
            ("down", Some("DEATH is missing. Usage: down DEATH".into())),
            ("down 4 5", Some("Too many words. Usage: down DEATH".into())),
            ("spawn g_sithtroop99", Some("No creature has the template \"g_sithtroop99\".".into())),
            ("spawn G_SithTroop01", None),
            ("attribute STRENGTH 18", None),
            ("attribute 5 18", None),
            ("attribute luck 18", Some("\"luck\" is not an attribute. Usage: attribute NAME SCORE [WHO]".into())),
            ("skill treatinjury 4 2", None),
            ("skill security 4 3", Some("Party member 3 does not exist.".into())),
            ("skill security 4 them", Some("\"them\" is not a party member. Use me, or a party member's number from 1. Usage: skill NAME RANKS [WHO]".into())),
            ("level jediguardian 20", None),
            ("level 4 20", Some("No class has the number 4.".into())),
            ("level soldier 20", Some("\"soldier\" is not a class. Use its number or its label. Usage: level CLASS LEVEL [SECOND] [SECONDLEVELS]".into())),
            ("level 3 20 9", Some("SECONDLEVELS is missing. Usage: level CLASS LEVEL [SECOND] [SECONDLEVELS]".into())),
        ],
        refused,
    )
}

/// The `arena` words of earlier scripts give the orders the game's own
/// matching of them gave.
fn arena_words() -> Result<String, String> {
    let facts = arena();
    let me = SimId(7);
    let arena = |order: ArenaOrder| Ok(vec![Effect::Order(Action::Arena(order))]);
    cases(
        &[
            (
                "arena spawn g_sithtroop01",
                arena(ArenaOrder::Spawn("g_sithtroop01".into())),
            ),
            ("arena clear", arena(ArenaOrder::Clear)),
            ("arena god", arena(ArenaOrder::God)),
            ("arena down 3", arena(ArenaOrder::Down(3))),
            (
                "arena level 3 20",
                arena(ArenaOrder::Rebuild {
                    who: me,
                    class: 3,
                    level: 20,
                    second: None,
                }),
            ),
            (
                "arena level 3 20 9 5",
                arena(ArenaOrder::Rebuild {
                    who: me,
                    class: 3,
                    level: 20,
                    second: Some((9, 5)),
                }),
            ),
            (
                "arena ability strength 18",
                arena(ArenaOrder::Ability {
                    who: me,
                    ability: 0,
                    score: 18,
                }),
            ),
            (
                "arena ability 2 14",
                arena(ArenaOrder::Ability {
                    who: me,
                    ability: 2,
                    score: 14,
                }),
            ),
            (
                "arena skill security 4",
                arena(ArenaOrder::Skill {
                    who: me,
                    skill: 6,
                    ranks: 4,
                }),
            ),
            (
                "arena alignment 50",
                arena(ArenaOrder::Alignment {
                    who: me,
                    good_evil: 50,
                }),
            ),
            ("arena restore", Ok(vec![Effect::Restore { who: me }])),
            (
                "alignment 0 2",
                arena(ArenaOrder::Alignment {
                    who: SimId(9),
                    good_evil: 0,
                }),
            ),
        ],
        |text| orders(text, &facts),
    )
}

/// The original games' names of the commands this table has resolve to
/// them.
fn original_names() -> Result<String, String> {
    let facts = arena();
    let me = SimId(7);
    let arena = |order: ArenaOrder| Ok(vec![Effect::Order(Action::Arena(order))]);
    let abilities = [
        "setstrength",
        "setdexterity",
        "setconstitution",
        "setintelligence",
        "setwisdom",
        "setcharisma",
    ];
    let skills = [
        "setcomputeruse",
        "setdemolitions",
        "setstealth",
        "setawareness",
        "setpersuade",
        "setrepair",
        "setsecurity",
        "settreatinjury",
    ];
    let mut all: Vec<(String, Result<Vec<Effect>, String>)> =
        vec![("invulnerability".into(), arena(ArenaOrder::God))];
    for (ability, name) in abilities.iter().enumerate() {
        let ability = u8::try_from(ability).unwrap_or(0);
        all.push((
            format!("{name} 12"),
            arena(ArenaOrder::Ability {
                who: me,
                ability,
                score: 12,
            }),
        ));
    }
    for (skill, name) in skills.iter().enumerate() {
        let skill = u8::try_from(skill).unwrap_or(0);
        all.push((
            format!("{name} 3"),
            arena(ArenaOrder::Skill {
                who: me,
                skill,
                ranks: 3,
            }),
        ));
    }
    let all: Vec<(&str, Result<Vec<Effect>, String>)> = all
        .iter()
        .map(|(text, want)| (text.as_str(), want.clone()))
        .collect();
    cases(&all, |text| orders(text, &facts))
}

/// Completion returns the expected candidates for a fixed list of names.
fn completion() -> Result<String, String> {
    let facts = arena();
    let names =
        |list: &[&str]| -> Vec<String> { list.iter().map(|name| name.to_string()).collect() };
    cases(
        &[
            ("spa", names(&["spawn"])),
            ("spawn g_s", names(&["g_sithtroop01", "g_sithtroop05"])),
            ("spawn jedi", names(&["g_darkjedi01"])),
            ("attribute w", names(&["wisdom"])),
            ("skill security 4 ", names(&["me", "1", "2"])),
            ("level jedi", names(&["JediGuardian", "JediWeaponMaster"])),
            ("arena sp", names(&["spawn"])),
            ("help cl", names(&["clear", "clearfoes", "clip"])),
            ("god ", Vec::new()),
        ],
        |text| complete::candidates(text, &facts),
    )
}

fn did_you_mean() -> Result<String, String> {
    cases(
        &[
            ("spwan", Some("spawn")),
            ("attribut", Some("attribute")),
            ("setstrenght", Some("setstrength")),
            ("xyzzyplugh", None),
        ],
        complete::closest,
    )
}

/// A command refuses where it may not run, and says why.
fn places() -> Result<String, String> {
    let menu = Facts::default();
    let story = Facts {
        arena: false,
        ..arena()
    };
    let refused =
        |text: &str, facts: &Facts| run(&line(text), facts).err().map(|refusal| refusal.0);
    let script_only = table()
        .iter()
        .filter(|spec| spec.place == Place::ScriptOnly)
        .count();
    cases(
        &[
            (("god", &menu), Some("Load a game before using this command.".to_string())),
            (
                ("god", &story),
                Some("This command works only on the Debug Mode map and the other combat test levels.".into()),
            ),
            (("whereami", &menu), Some("Load a game before using this command.".into())),
            (("whereami", &story), None),
            (("version", &menu), None),
            (("wait world", &story), Some("This command works only in a command script (--cmds).".into())),
        ],
        |(text, facts)| refused(text, facts),
    )
    .map(|done| format!("{done}, {script_only} script verbs"))
}

/// `help` lists only what can run, `help all` says why the rest cannot,
/// and `help NAME` describes one command.
fn help_lines() -> Result<String, String> {
    let menu = Facts::default();
    let listed = help::list(&menu, false);
    let all = help::list(&menu, true);
    let one = help::describe(crate::find("attribute").ok_or("no attribute command")?);
    let mut problems = Vec::new();
    if listed.iter().any(|line| line.contains("spawn TEMPLATE")) {
        problems.push("help at the menu lists spawn".to_string());
    }
    if !listed.iter().any(|line| line.starts_with("  help [NAME]")) {
        problems.push("help does not list itself".into());
    }
    if !all
        .iter()
        .any(|line| line.contains("Load a game before using this command."))
    {
        problems.push("help all does not say why spawn cannot run".into());
    }
    if !one
        .iter()
        .any(|line| line.starts_with("Also accepted:") && line.contains("setstrength"))
    {
        problems.push(format!("help attribute: {one:?}"));
    }
    match problems.is_empty() {
        true => Ok(format!(
            "{} lines at the menu, {} with all",
            listed.len(),
            all.len()
        )),
        false => Err(problems.join("; ")),
    }
}

/// 10,000 lines of seeded random characters, printable or not, run on the
/// Debug Mode map without a panic.
fn random_lines() -> Result<String, String> {
    let facts = arena();
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let pieces: Vec<&str> = (crate::complete::names())
        .chain([
            "\"",
            ";",
            " ",
            "\n",
            "me",
            "1",
            "-5",
            "99999999999999999999",
            "strength",
        ])
        .collect();
    let mut ran = 0;
    for _ in 0..10_000 {
        let mut text = String::new();
        for _ in 0..(next() % 8) {
            match next() % 3 {
                0 => text.push(char::from_u32((next() % 0x2_0000) as u32).unwrap_or('?')),
                _ => {
                    let piece = pieces
                        .get((next() as usize) % pieces.len())
                        .copied()
                        .unwrap_or("");
                    text.push_str(piece);
                    text.push(' ');
                }
            }
        }
        let outcome = std::panic::catch_unwind(|| {
            crate::run_line(&text, &facts);
            let _ = crate::check(&text);
            complete::candidates(&text, &facts);
        });
        if outcome.is_err() {
            return Err(format!("{text:?} panicked"));
        }
        ran += 1;
    }
    Ok(format!("{ran} lines"))
}
