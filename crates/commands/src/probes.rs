//! The table's checks (`kjk sim probes --only commands`, and the approved
//! test `commands`).

use sim::SimId;
use sim::action::{Action, ArenaOrder, CheatOrder, Mark, ScriptParams};
use sim::cheats::Rule;

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
        ..Facts::default()
    }
}

/// The facts of the host in a story module under the rule Host only: the
/// party has 100 credits and 1,000 experience points, its next level needs
/// 2,500, and the crosshair is on object 12.
fn story() -> Facts {
    Facts {
        arena: false,
        module: Some("tar_m02aa".into()),
        items: Some(vec!["g_w_lghtsbr01".into(), "g_i_medeqpmnt01".into()]),
        scripts: Some(vec!["k_test".into(), "k_mod_heal".into()]),
        target: Some(SimId(12)),
        xp: Some(1000),
        next_level_xp: Some(2500),
        cheats: Rule::Host,
        host: true,
        ..arena()
    }
}

/// The facts of a player at the main menu.
fn at_menu() -> Facts {
    Facts {
        menu: true,
        version: "test".into(),
        ..Facts::default()
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
        ("cheat commands give their orders", cheat_orders()),
        ("original names resolve", original_names()),
        ("completion", completion()),
        ("did you mean", did_you_mean()),
        ("where a command may run", places()),
        ("session commands", session()),
        ("help", help_lines()),
        ("random lines never panic", random_lines()),
        ("mod commands read", mod_rows()),
        ("mod commands run", mod_runs()),
        ("mod commands in help and completion", mod_help()),
        ("damaged mod tables never panic", mod_damaged()),
    ]
}

/// What a check saw, passed when `ok`.
fn check(ok: bool, what: String) -> Result<String, String> {
    if ok { Ok(what) } else { Err(what) }
}

/// A `kjkconsole.2da` of `rows`, each a label, a script, the arguments and
/// a help text, as a mod would write it.
fn mod_table(rows: &[[&str; 4]]) -> Vec<u8> {
    let cell = |text: &str| text.as_bytes().to_vec();
    fmt_odyssey::twoda::TwoDa {
        columns: ["label", "script", "args", "help", "helptext"]
            .map(cell)
            .into(),
        row_labels: (0..rows.len()).map(|row| cell(&row.to_string())).collect(),
        cells: (rows.iter())
            .map(|[label, script, args, help]| {
                vec![
                    cell(label),
                    cell(script),
                    cell(args),
                    cell("****"),
                    cell(help),
                ]
            })
            .collect(),
        column_separator: b'\t',
        row_separator: b'\t',
    }
    .write()
}

/// The probes' table: one good row, one good row with no arguments, then
/// eight rows that cannot work.
const MOD_ROWS: [[&str; 4]; 10] = [
    [
        "mod_heal",
        "k_mod_heal",
        "int,int,text",
        "Heals by an amount.",
    ],
    ["Mod_Plain", "k_test", "****", "****"],
    ["", "k_test", "****", "No label."],
    ["mod bad", "k_test", "****", "A space in its label."],
    ["mod_kind", "k_test", "number", "An unknown type."],
    [
        "mod_six",
        "k_test",
        "int,int,int,int,int,int",
        "Six numbers.",
    ],
    ["mod_texts", "k_test", "item,creature", "Two texts."],
    ["heal", "k_test", "****", "A built-in name."],
    ["mod_heal", "k_test", "****", "A second mod_heal."],
    ["mod_noscript", "****", "****", "No script."],
];

/// The probes' table, read for KOTOR 2.
fn mod_facts() -> Facts {
    let bytes = mod_table(&MOD_ROWS);
    let table = crate::mods::read(Some(Ok(bytes)), true, &mut |_| None);
    Facts {
        mods: std::sync::Arc::new(table),
        ..story()
    }
}

/// The good rows become commands, and every other row is skipped with its
/// reason. In KOTOR 1 a row with parameters is skipped too.
fn mod_rows() -> Result<String, String> {
    let facts = mod_facts();
    let names: Vec<&str> = (facts.mods.commands.iter())
        .map(|command| command.name.as_str())
        .collect();
    let skipped: Vec<usize> = facts.mods.skipped.iter().map(|row| row.row).collect();
    let usage = facts.mods.find("MOD_HEAL").map(crate::mods::usage);
    let k1 = crate::mods::read(Some(Ok(mod_table(&MOD_ROWS[..2]))), false, &mut |_| None);
    let k1_names: Vec<&str> = (k1.commands.iter())
        .map(|command| command.name.as_str())
        .collect();
    let listed = facts.mods.listed();
    check(
        names == ["mod_heal", "mod_plain"]
            && skipped == [2, 3, 4, 5, 6, 7, 8, 9]
            && usage.as_deref() == Some("mod_heal NUMBER1 NUMBER2 TEXT")
            && k1_names == ["mod_plain"]
            && listed.len() == 10,
        format!(
            "commands {names:?}, skipped rows {skipped:?}, usage {usage:?}, KOTOR 1 {k1_names:?}, {} listed lines: {listed:?}",
            listed.len()
        ),
    )
}

/// A mod command gives the order `Script` with the player's character as
/// the caller and its arguments as the script's parameters. A wrong
/// argument, a missing script and the cheat rule Off refuse it.
fn mod_runs() -> Result<String, String> {
    let facts = mod_facts();
    let script = |resref: &str, numbers, text: &str| {
        Ok(vec![Effect::Order(Action::Cheat(CheatOrder::Script {
            resref: resref.into(),
            who: Some(SimId(7)),
            params: ScriptParams {
                numbers,
                text: text.into(),
            },
        }))])
    };
    let off = Facts {
        cheats: Rule::Off,
        ..mod_facts()
    };
    let gone = Facts {
        scripts: Some(Vec::new()),
        ..mod_facts()
    };
    let bad_script = crate::mods::read(
        Some(Ok(mod_table(&[[
            "mod_badscript",
            "bad/script",
            "****",
            "Bad script.",
        ]]))),
        true,
        &mut |_| None,
    );
    let runs = cases(
        &[
            ("mod_heal 10 -2 all of them", script("k_mod_heal", [10, -2, 0, 0, 0], "all of them")),
            ("MOD_PLAIN", script("k_test", [0; 5], "")),
            (
                "mod_heal ten 2 x",
                Err("\"ten\" is not a number. Usage: mod_heal NUMBER1 NUMBER2 TEXT".into()),
            ),
            (
                "mod_heal 1",
                Err("Missing NUMBER2. Usage: mod_heal NUMBER1 NUMBER2 TEXT".into()),
            ),
            ("mod_plain extra", Err("Too many arguments. Usage: mod_plain".into())),
            ("mod_bad", Err("Unknown command \"mod_bad\". Type \"help\" to list commands.".into())),
            (
                "mod_six 1",
                Err("Skipped kjkconsole.2da row 5 (mod_six): Too many \"int\" arguments (6; max 5).".into()),
            ),
        ],
        |text| orders(text, &facts),
    )?;
    let refused = [
        orders("mod_plain", &off),
        orders("mod_plain", &gone),
        orders(
            "mod_plain",
            &Facts {
                mods: mod_facts().mods,
                ..at_menu()
            },
        ),
    ];
    check(
        bad_script.commands.is_empty()
            && bad_script.skipped.len() == 1
            && refused
                == [
                    Err("Cheats are disabled in this game.".into()),
                    Err("Script \"k_test\" not found.".into()),
                    Err("Load a game first.".into()),
                ],
        format!(
            "{runs}; bad_script {:?}; refused {refused:?}",
            bad_script.skipped
        ),
    )
}

/// `help` lists the mod commands in a group of their own, `help NAME` and
/// `mods commands` describe them, completion offers their names, and the
/// launcher's check accepts their lines.
fn mod_help() -> Result<String, String> {
    let facts = mod_facts();
    let printed = |text: &str| -> Vec<String> {
        (run(&line(text), &facts)
            .unwrap_or_else(|_| Vec::new())
            .into_iter())
        .filter_map(|effect| match effect {
            Effect::Print(text) => Some(text),
            _ => None,
        })
        .collect()
    };
    let help = printed("help");
    let listed = help.iter().any(|line| line == "Mod commands:")
        && help
            .iter()
            .any(|line| line == "  mod_heal NUMBER1 NUMBER2 TEXT - Heals by an amount.");
    let described = printed("help mod_heal");
    let alias_help = printed("help setstrength");
    let two_word_help = printed("help arena spawn");
    let mods = printed("mods commands");
    let completed = complete::candidates("mod_", &facts);
    let argument = complete::candidates("runscript k_", &facts);
    let help_mod = complete::candidates("help mod_", &facts);
    let checked = [
        crate::check_with("mod_heal 1 2 x; heal", &[&facts.mods]),
        crate::check_with("mod_heal x", &[&facts.mods]),
        crate::check("mod_heal 1 2 x"),
        crate::check("level JediGuardian 10"),
    ];
    check(
        listed
            && described.first().map(String::as_str)
                == Some("Usage: mod_heal NUMBER1 NUMBER2 TEXT")
            && alias_help.first().map(String::as_str) == Some("Usage: attribute NAME SCORE [WHO]")
            && two_word_help.first().map(String::as_str) == Some("Usage: spawn TEMPLATE")
            && mods.len() == 10
            && completed == ["mod_heal", "mod_plain"]
            && argument == ["k_test", "k_mod_heal"]
            && help_mod == ["mod_heal", "mod_plain"]
            && checked[0].is_ok()
            && checked[1].is_err()
            && checked[2].is_err()
            && checked[3].is_ok(),
        format!(
            "help {help:?}; help mod_heal {described:?}; mods commands {} lines; completed {completed:?} and {argument:?}; help_mod {help_mod:?}; checked {checked:?}",
            mods.len()
        ),
    )
}

/// A table that is not a 2DA, cut short, or full of random bytes gives no
/// command and never panics. A file that cannot be read says why.
fn mod_damaged() -> Result<String, String> {
    let whole = mod_table(&MOD_ROWS);
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut tables: Vec<Vec<u8>> = (0..whole.len()).map(|end| whole[..end].to_vec()).collect();
    for _ in 0..2_000 {
        let mut bytes = whole.clone();
        for _ in 0..(1 + next() % 6) {
            let at = (next() as usize) % bytes.len();
            bytes[at] = next() as u8;
        }
        tables.push(bytes);
    }
    tables.push(b"not a table".to_vec());
    let outcome = std::panic::catch_unwind(|| {
        for bytes in &tables {
            let table = crate::mods::read(Some(Ok(bytes.clone())), true, &mut |_| None);
            let facts = Facts {
                mods: std::sync::Arc::new(table),
                ..story()
            };
            for command in &facts.mods.commands {
                let _ = run(&line(&format!("{} 1 2 three", command.name)), &facts);
            }
            let _ = facts.mods.listed();
        }
    });
    let unread = crate::mods::read(Some(Err("Permission denied".into())), true, &mut |_| None);
    let short = crate::mods::read(Some(Ok(b"not a table".to_vec())), true, &mut |_| None);
    check(
        outcome.is_ok()
            && unread.problem.as_deref() == Some("Permission denied")
            && short.commands.is_empty()
            && short.problem.is_some(),
        format!(
            "{} damaged tables read; unread {:?}; not a table {:?}",
            tables.len(),
            unread.problem,
            short.problem
        ),
    )
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
            ("down", Some("Missing DEATH. Usage: down DEATH".into())),
            ("down 4 5", Some("Too many arguments. Usage: down DEATH".into())),
            ("spawn g_sithtroop99", Some("Unknown creature template \"g_sithtroop99\".".into())),
            ("spawn G_SithTroop01", None),
            ("attribute STRENGTH 18", None),
            ("attribute 5 18", None),
            ("attribute luck 18", Some("Invalid attribute \"luck\". Usage: attribute NAME SCORE [WHO]".into())),
            ("skill treatinjury 4 2", None),
            ("skill security 4 3", Some("No party member #3.".into())),
            ("skill security 4 them", Some("Invalid party member \"them\". Use \"me\" or a slot number starting at 1. Usage: skill NAME RANKS [WHO]".into())),
            ("level jediguardian 20", None),
            ("level 4 20", Some("Unknown class ID 4.".into())),
            ("level soldier 20", Some("Unknown class \"soldier\". Use a class name or ID from classes.2da. Usage: level CLASS LEVEL [SECOND] [SECONDLEVELS]".into())),
            ("level 3 20 9", Some("Missing SECONDLEVELS. Usage: level CLASS LEVEL [SECOND] [SECONDLEVELS]".into())),
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

/// Each command that changes the party or the story gives its order.
/// `givecredits` and `addexp` send the amount to add.
fn cheat_orders() -> Result<String, String> {
    let facts = story();
    let cheat = |order: CheatOrder| Ok(vec![Effect::Order(Action::Cheat(order))]);
    let item = |template: &str, count| {
        cheat(CheatOrder::Item {
            template: template.into(),
            count,
        })
    };
    let script = |who, numbers, text: &str| {
        cheat(CheatOrder::Script {
            resref: "k_test".into(),
            who,
            params: ScriptParams {
                numbers,
                text: text.into(),
            },
        })
    };
    cases(
        &[
            ("giveitem g_w_lghtsbr01 2", item("g_w_lghtsbr01", 2)),
            ("giveitem G_I_MEDEQPMNT01", item("g_i_medeqpmnt01", 1)),
            (
                "giveitem nothing",
                Err("Unknown item template \"nothing\".".into()),
            ),
            ("givecredits 500", cheat(CheatOrder::Credits(500))),
            ("addexp 100", cheat(CheatOrder::Xp(100))),
            ("addxp 100", cheat(CheatOrder::Xp(100))),
            ("addlevel", cheat(CheatOrder::Xp(1500))),
            ("heal", cheat(CheatOrder::Heal { who: None })),
            (
                "heal 2",
                cheat(CheatOrder::Heal {
                    who: Some(SimId(9)),
                }),
            ),
            (
                "kill",
                cheat(CheatOrder::Kill {
                    who: Mark::Id(SimId(12)),
                }),
            ),
            (
                "kill tar_guard",
                cheat(CheatOrder::Kill {
                    who: Mark::Tag("tar_guard".into()),
                }),
            ),
            (
                "setglobal K_TEST -3",
                cheat(CheatOrder::Global {
                    name: "K_TEST".into(),
                    value: -3,
                }),
            ),
            ("runscript K_TEST", script(None, [0; 5], "")),
            ("runscript k_test me", script(Some(SimId(7)), [0; 5], "")),
            (
                "runscript k_test 2 3 -4 hello there 5",
                script(Some(SimId(9)), [3, -4, 0, 0, 0], "hello there 5"),
            ),
            (
                "runscript k_test me 1 2 3 4 5 6",
                Err("Too many integer parameters (max 5).".into()),
            ),
            (
                "runscript k_nothing",
                Err("Script \"k_nothing\" not found.".into()),
            ),
            (
                "spawn g_sithtroop01",
                Ok(vec![Effect::Order(Action::Arena(ArenaOrder::Spawn(
                    "g_sithtroop01".into(),
                )))]),
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
            ("ARENA sp", names(&["spawn"])),
            ("heal; spa", names(&["spawn"])),
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
    let menu = at_menu();
    let off = Facts {
        cheats: Rule::Off,
        ..story()
    };
    let guest = Facts {
        host: false,
        ..story()
    };
    let all = Facts {
        cheats: Rule::All,
        ..guest.clone()
    };
    let refused =
        |text: &str, facts: &Facts| run(&line(text), facts).err().map(|refusal| refusal.0);
    let script_only = table()
        .iter()
        .filter(|spec| spec.place == Place::ScriptOnly)
        .count();
    cases(
        &[
            (("god", &menu), Some("Load a game first.".to_string())),
            (
                ("god", &off),
                Some("Cheats are disabled in this game.".into()),
            ),
            (
                ("god", &guest),
                Some("Only the host can use cheats in this game.".into()),
            ),
            (("god", &all), None),
            (("god", &arena()), None),
            (
                ("clearfoes", &all),
                Some("Only available on Debug Mode and combat test maps.".into()),
            ),
            (("help", &off), None),
            (("whereami", &menu), Some("Load a game first.".into())),
            (("whereami", &off), None),
            (("version", &menu), None),
            (
                ("wait world", &off),
                Some("Only available in --cmds scripts.".into()),
            ),
        ],
        |(text, facts)| refused(text, facts),
    )
    .map(|done| format!("{done}, {script_only} script verbs"))
}

/// The session's commands: at the main menu they start, load, host or
/// join a game. In a game `warp` and `load` are the host's alone under
/// every rule, and the menu's own commands are refused. The lobby and an
/// area still loading refuse them with the reason.
fn session() -> Result<String, String> {
    use crate::Start;
    let menu = at_menu();
    let lobby = Facts {
        lobby: true,
        ..Facts::default()
    };
    let loading = Facts::default();
    let guest = Facts {
        host: false,
        cheats: Rule::All,
        ..story()
    };
    let off = Facts {
        cheats: Rule::Off,
        ..story()
    };
    let started = |start: Start| Ok(vec![Effect::Start(start)]);
    let warp = |arrival: Option<&str>| {
        started(Start::Warp {
            module: "tar_m02aa".into(),
            arrival: arrival.map(str::to_string),
        })
    };
    let refused = |text: &str| Err(text.to_string());
    let host_only = refused("Only the host can use this command.");
    let leave = refused("Leave the lobby to use this command.");
    cases(
        &[
            (("warp TAR_M02AA", &menu), warp(None)),
            (
                ("warp tar_m02aa from_m02ab", &story()),
                warp(Some("from_m02ab")),
            ),
            (("warp tar_m02aa", &guest), host_only.clone()),
            (
                ("warp tar_m02aa", &off),
                refused("Cheats are disabled in this game."),
            ),
            (("warp tar_m02aa", &lobby), leave.clone()),
            (
                ("warp tar_m02aa", &loading),
                refused("Wait for the area to finish loading."),
            ),
            (("sandbox", &menu), started(Start::Sandbox)),
            (
                ("sandbox", &story()),
                refused("Only available from the main menu."),
            ),
            (("sandbox", &lobby), leave.clone()),
            (("load 0003", &menu), started(Start::Load("0003".into()))),
            (("load 0003", &story()), started(Start::Load("0003".into()))),
            (("load 0003", &guest), host_only),
            (("load 0003", &lobby), leave),
            (("host", &menu), started(Start::Host(None))),
            (
                ("host 0003", &menu),
                started(Start::Host(Some("0003".into()))),
            ),
            (
                ("join 10.0.0.2:29170 Mia", &menu),
                started(Start::Join {
                    address: "10.0.0.2:29170".into(),
                    name: Some("Mia".into()),
                }),
            ),
            (
                ("join 10.0.0.2", &guest),
                refused("Only available from the main menu."),
            ),
            (("saves", &lobby), Ok(vec![Effect::Saves])),
            (("mods", &guest), Ok(vec![Effect::Mods])),
        ],
        |(text, facts)| orders(text, facts),
    )
}

/// `help` lists only what can run, `help all` says why the rest cannot,
/// and `help NAME` describes one command.
fn help_lines() -> Result<String, String> {
    let menu = at_menu();
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
    if !all.iter().any(|line| line.contains("Load a game first.")) {
        problems.push("help all does not say why spawn cannot run".into());
    }
    if !one
        .iter()
        .any(|line| line.starts_with("Aliases:") && line.contains("setstrength"))
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
