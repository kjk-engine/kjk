//! Logic blocks: an object's behaviour on one event picked from a list,
//! not written ("open the door tagged X", then "give the player item Y").
//! The blocks of an event are written as a script of the project
//! (`kjb_N.nss`) whose header lists them (`//@block KEY "VALUE" ...`), so
//! the script holds them and nothing else does. It is compiled as any of
//! the project's scripts (`scripts`), and the blocks run in their order.
//! Editing the script by hand drops the header, and the blocks with it.

/// What a block's value is, for the field that takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    /// An object's tag.
    Tag,
    /// A template's or a resource's name (a resref).
    Resref,
    /// A whole number.
    Number,
    /// Text the player reads.
    Text,
    /// A global number of `globalcat.2da`.
    GlobalNumber,
    /// A global boolean of `globalcat.2da`.
    GlobalBoolean,
    /// True or false.
    Truth,
}

/// One kind of block: its key in the header, its name, its values and the
/// code it writes.
pub struct Kind {
    pub key: &'static str,
    pub title: &'static str,
    pub params: &'static [(&'static str, Param)],
    /// The statements it writes, from its values (strings already escaped).
    code: fn(&[String]) -> String,
}

/// The local boolean of the object running the script that "Only once"
/// keeps: past those the games' own scripts use.
pub const ONCE_BOOLEAN: i32 = 90;

/// The kinds of block, in the order the editor offers them.
pub const KINDS: &[Kind] = &[
    Kind {
        key: "once",
        title: "Only the first time",
        params: &[],
        code: |_| {
            format!(
                "if (GetLocalBoolean(OBJECT_SELF, {ONCE_BOOLEAN})) return;\n\
                 SetLocalBoolean(OBJECT_SELF, {ONCE_BOOLEAN}, TRUE);"
            )
        },
    },
    Kind {
        key: "if_number",
        title: "Only if a global number is",
        params: &[("global", Param::GlobalNumber), ("value", Param::Number)],
        code: |v| format!("if (GetGlobalNumber(\"{}\") != {}) return;", v[0], v[1]),
    },
    Kind {
        key: "if_boolean",
        title: "Only if a global boolean is",
        params: &[("global", Param::GlobalBoolean), ("value", Param::Truth)],
        code: |v| {
            format!(
                "if (GetGlobalBoolean(\"{}\") != {}) return;",
                v[0],
                truth(&v[1])
            )
        },
    },
    Kind {
        key: "if_has_item",
        title: "Only if the party carries an item",
        params: &[("item's tag", Param::Tag)],
        code: |v| {
            format!(
                "if (!GetIsObjectValid(GetItemPossessedBy(GetFirstPC(), \"{}\"))) return;",
                v[0]
            )
        },
    },
    Kind {
        key: "open_door",
        title: "Open a door",
        params: &[("door", Param::Tag)],
        code: |v| {
            format!(
                "AssignCommand(GetObjectByTag(\"{0}\"), ActionOpenDoor(GetObjectByTag(\"{0}\")));",
                v[0]
            )
        },
    },
    Kind {
        key: "close_door",
        title: "Close a door",
        params: &[("door", Param::Tag)],
        code: |v| {
            format!(
                "AssignCommand(GetObjectByTag(\"{0}\"), ActionCloseDoor(GetObjectByTag(\"{0}\")));",
                v[0]
            )
        },
    },
    Kind {
        key: "unlock",
        title: "Unlock a door or container",
        params: &[("object", Param::Tag)],
        code: |v| format!("SetLocked(GetObjectByTag(\"{}\"), FALSE);", v[0]),
    },
    Kind {
        key: "lock",
        title: "Lock a door or container",
        params: &[("object", Param::Tag)],
        code: |v| format!("SetLocked(GetObjectByTag(\"{}\"), TRUE);", v[0]),
    },
    Kind {
        key: "talk",
        title: "Start a conversation",
        params: &[
            ("conversation", Param::Resref),
            ("speaker (empty: this object)", Param::Tag),
        ],
        code: |v| {
            let speaker = if v[1].is_empty() {
                "OBJECT_SELF".to_string()
            } else {
                format!("GetObjectByTag(\"{}\")", v[1])
            };
            format!(
                "AssignCommand({speaker}, ActionStartConversation(GetFirstPC(), \"{}\"));",
                v[0]
            )
        },
    },
    Kind {
        key: "message",
        title: "Tell the player",
        params: &[("message", Param::Text)],
        code: |v| format!("SendMessageToPC(GetFirstPC(), \"{}\");", v[0]),
    },
    Kind {
        key: "set_number",
        title: "Set a global number",
        params: &[("global", Param::GlobalNumber), ("value", Param::Number)],
        code: |v| format!("SetGlobalNumber(\"{}\", {});", v[0], v[1]),
    },
    Kind {
        key: "set_boolean",
        title: "Set a global boolean",
        params: &[("global", Param::GlobalBoolean), ("value", Param::Truth)],
        code: |v| format!("SetGlobalBoolean(\"{}\", {});", v[0], truth(&v[1])),
    },
    Kind {
        key: "give_item",
        title: "Give the player an item",
        params: &[
            ("item template", Param::Resref),
            ("how many", Param::Number),
        ],
        code: |v| format!("CreateItemOnObject(\"{}\", GetFirstPC(), {});", v[0], v[1]),
    },
    Kind {
        key: "give_credits",
        title: "Give the player credits",
        params: &[("credits", Param::Number)],
        code: |v| format!("GiveGoldToCreature(GetFirstPC(), {});", v[0]),
    },
    Kind {
        key: "give_xp",
        title: "Give the player experience",
        params: &[("experience", Param::Number)],
        code: |v| format!("GiveXPToCreature(GetFirstPC(), {});", v[0]),
    },
    Kind {
        key: "journal",
        title: "Add a journal entry",
        params: &[("quest tag", Param::Tag), ("entry", Param::Number)],
        code: |v| format!("AddJournalQuestEntry(\"{}\", {});", v[0], v[1]),
    },
    Kind {
        key: "hostile",
        title: "Turn a creature hostile",
        params: &[("creature", Param::Tag)],
        code: |v| {
            format!(
                "ChangeToStandardFaction(GetObjectByTag(\"{}\"), STANDARD_FACTION_HOSTILE_1);",
                v[0]
            )
        },
    },
    Kind {
        key: "destroy",
        title: "Remove an object",
        params: &[("object", Param::Tag)],
        code: |v| format!("DestroyObject(GetObjectByTag(\"{}\"));", v[0]),
    },
    Kind {
        key: "travel",
        title: "Take the party to a module",
        params: &[
            ("module", Param::Resref),
            ("arrive at (tag, empty: its entry)", Param::Tag),
        ],
        code: |v| format!("StartNewModule(\"{}\", \"{}\");", v[0], v[1]),
    },
];

fn truth(value: &str) -> &'static str {
    if matches!(value.trim(), "1" | "true" | "TRUE" | "yes") {
        "TRUE"
    } else {
        "FALSE"
    }
}

/// The kind of block of `key`.
pub fn kind(key: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|kind| kind.key == key)
}

/// One block: its kind's key and its values, one for each of the kind's
/// params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub key: String,
    pub values: Vec<String>,
}

impl Block {
    /// A new block of `kind`, its values the kind's defaults.
    pub fn new(kind: &Kind) -> Block {
        Block {
            key: kind.key.to_string(),
            values: kind
                .params
                .iter()
                .map(|(_, param)| match param {
                    Param::Number => "1".to_string(),
                    Param::Truth => "1".to_string(),
                    _ => String::new(),
                })
                .collect(),
        }
    }
}

/// The first line of a script made from blocks.
const MADE: &str = "// Made by kjk's level editor from logic blocks (docs/LEVEL-EDIT.md).";
/// How a block's line starts.
const LINE: &str = "//@block ";

/// Whether `source` is a script made from blocks.
pub fn is_blocks(source: &str) -> bool {
    source.starts_with(MADE)
}

/// The blocks a script made from blocks lists; `None` for any other
/// script.
pub fn read(source: &str) -> Option<Vec<Block>> {
    if !is_blocks(source) {
        return None;
    }
    let mut blocks = Vec::new();
    for line in source.lines() {
        let Some(rest) = line.strip_prefix(LINE) else {
            continue;
        };
        let (key, mut rest) = rest.split_once(' ').unwrap_or((rest, ""));
        let mut values = Vec::new();
        // Each value quoted, as `escaped` writes it.
        while let Some(start) = rest.find('"') {
            let mut value = String::new();
            let mut chars = rest[start + 1..].char_indices();
            let mut end = None;
            while let Some((at, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, escaped)) = chars.next() {
                            value.push(match escaped {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                other => other,
                            });
                        }
                    }
                    '"' => {
                        end = Some(start + 1 + at + 1);
                        break;
                    }
                    other => value.push(other),
                }
            }
            values.push(value);
            match end {
                Some(end) => rest = &rest[end..],
                None => break,
            }
        }
        blocks.push(Block {
            key: key.to_string(),
            values,
        });
    }
    Some(blocks)
}

/// `value` as an NWScript string's contents: `"` and `\` escaped, and a
/// line break, a carriage return and a tab written as `\n`, `\r` and
/// `\t`, which the compiler reads as those. A value is written on one line
/// of the header and into a string of the code. A line break left in it
/// ended the header's line, so the rest of the value stood in the script as
/// code, and ended the string, which the compiler refuses.
fn escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// What is wrong with `blocks`: a kind not known, a value not given, a
/// number that is no number. The script is written all the same (a bad
/// number as 0), so a block being filled in is kept.
pub fn problems(blocks: &[Block]) -> Vec<String> {
    let mut found = Vec::new();
    for (at, block) in blocks.iter().enumerate() {
        let Some(kind) = kind(&block.key) else {
            found.push(format!(
                "block {}: {:?} is not a kind of block",
                at + 1,
                block.key
            ));
            continue;
        };
        for (n, (label, param)) in kind.params.iter().enumerate() {
            let value = block.values.get(n).map_or("", |value| value.trim());
            let empty_ok = matches!(param, Param::Tag) && label.contains("empty");
            if value.is_empty() && !empty_ok {
                found.push(format!("{}: the {label} is not given", kind.title));
            } else if matches!(param, Param::Number) && value.parse::<i32>().is_err() {
                found.push(format!("{}: {value:?} is not a whole number", kind.title));
            }
        }
    }
    found
}

/// The script `blocks` make: the header listing them, then `main` running
/// each in order (`problems` says what is wrong with them).
pub fn write(blocks: &[Block]) -> String {
    let mut header = format!(
        "{MADE}\n// Change the blocks in the editor; editing this file by hand ends them.\n"
    );
    let mut body = String::new();
    for block in blocks {
        header.push_str(LINE);
        header.push_str(&block.key);
        for value in &block.values {
            header.push_str(&format!(" \"{}\"", escaped(value)));
        }
        header.push('\n');
        let Some(kind) = kind(&block.key) else {
            body.push_str(&format!("    // {:?} is not a kind of block\n", block.key));
            continue;
        };
        let values: Vec<String> = kind
            .params
            .iter()
            .enumerate()
            .map(|(n, (_, param))| {
                let value = block.values.get(n).map_or("", |value| value.trim());
                match param {
                    Param::Number if value.parse::<i32>().is_err() => "0".to_string(),
                    _ => escaped(value),
                }
            })
            .collect();
        body.push_str(&format!("    // {}\n", kind.title));
        for line in (kind.code)(&values).lines() {
            body.push_str("    ");
            body.push_str(line);
            body.push('\n');
        }
    }
    format!("{header}\nvoid main()\n{{\n{body}}}\n")
}

/// A name for a new script of blocks: `kjb_N` for the smallest N from 1
/// that `taken` refuses.
pub fn fresh_name(taken: impl Fn(&str) -> bool) -> String {
    (1..)
        .map(|n| format!("kjb_{n}"))
        .find(|name| !taken(name))
        .expect("some number is free")
}
