//! Effect files (`effects/**.efx`), in the text layout of JA's
//! GenericParser2: a group is a name and `{ … }`; inside it a line is a key
//! and the rest of the line its value, a key followed by `[` opens a list
//! of lines closed by `]`, and a key followed by `{` a subgroup. A key
//! alone on its line takes the next line as its value. Keys keep their
//! case; readers match them without it, as the engine does.

use crate::script::{self, Block, ScriptError};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Group {
    pub name: String,
    /// Key and value (the rest of the line, words joined by one space).
    pub pairs: Vec<(String, String)>,
    /// Key and the list's lines.
    pub lists: Vec<(String, Vec<String>)>,
    pub groups: Vec<Group>,
}

impl Group {
    /// The last value given for `key` (case-insensitive).
    pub fn pair(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

/// A whole file: the root group (top-level pairs such as `repeatDelay`,
/// and one subgroup per primitive).
pub fn parse(bytes: &[u8]) -> Result<Group, ScriptError> {
    Ok(group(script::parse(bytes)?))
}

fn group(block: Block) -> Group {
    let Block {
        name,
        lines,
        children,
    } = block;
    let mut out = Group {
        name: name.join(" "),
        groups: children.into_iter().map(group).collect(),
        ..Group::default()
    };
    let mut lines = lines.into_iter().peekable();
    while let Some(mut words) = lines.next() {
        if words.is_empty() {
            continue;
        }
        let key = words.remove(0);
        if words.is_empty() {
            // The value is on the next line: a list, or a plain value.
            match lines.peek() {
                Some(next) if next.first().is_some_and(|w| w == "[") => {
                    let opening = lines.next().expect("peeked");
                    let items = list(opening[1..].to_vec(), &mut lines);
                    out.lists.push((key, items));
                }
                Some(_) => {
                    let value = lines.next().expect("peeked").join(" ");
                    out.pairs.push((key, value));
                }
                None => out.pairs.push((key, String::new())),
            }
        } else if words[0] == "[" {
            // A list opened on its key's line.
            let items = list(words[1..].to_vec(), &mut lines);
            out.lists.push((key, items));
        } else {
            out.pairs.push((key, words.join(" ")));
        }
    }
    out
}

/// A list's lines from `first` (the words after its `[`) on, up to the
/// `]` that closes it.
fn list(first: Vec<String>, lines: &mut impl Iterator<Item = Vec<String>>) -> Vec<String> {
    let mut items = Vec::new();
    let mut line = first;
    loop {
        let end = line.iter().position(|w| w == "]");
        let taken = &line[..end.unwrap_or(line.len())];
        if !taken.is_empty() {
            items.push(taken.join(" "));
        }
        if end.is_some() {
            return items;
        }
        match lines.next() {
            Some(next) => line = next,
            None => return items,
        }
    }
}
