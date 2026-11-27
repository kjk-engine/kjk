//! Conversations (DLG files) as the level editor changes them: a project's
//! own, or its copy of one of the game's, edited on the file's GFF so every
//! field the editor does not touch stays as it was.
//!
//! A conversation holds two lists. Entries are what the other side says
//! (the conversation's owner, or the speaker a line names); replies are
//! what the player answers. Each node links to nodes of the other list, and
//! the starting list links to the entries a conversation may open with. A
//! link's condition script (`Active`) decides whether it is taken (an
//! entry) or offered (a reply); a node's action script (`Script`) runs as
//! it shows. A link marked as a child (`IsChild`) points back to a node
//! that lives elsewhere in the tree.
//!
//! A line the editor writes keeps its text in the conversation itself (the
//! text of its `Text`, with no talk-table line), which kjk shows as it
//! shows a game line's. A line of the game keeps its talk-table line until
//! its text is changed.

use fmt_odyssey::dlg::Dlg;
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue, LocString};

/// Which list a node is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    /// What the other side says.
    Entry,
    /// What the player answers.
    Reply,
}

impl Side {
    fn list(self) -> &'static str {
        match self {
            Side::Entry => "EntryList",
            Side::Reply => "ReplyList",
        }
    }

    /// The list a node of this side links into, by its label in the node.
    fn links(self) -> &'static str {
        match self {
            Side::Entry => "RepliesList",
            Side::Reply => "EntriesList",
        }
    }

    /// The side a node of this side links to.
    pub fn other(self) -> Side {
        match self {
            Side::Entry => Side::Reply,
            Side::Reply => Side::Entry,
        }
    }
}

/// A link: one of the starting list's, or one of a node's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LinkAt {
    /// The starting list's `n`th link (to an entry).
    Start(usize),
    /// The `n`th link of the node `(side, index)`.
    From(Side, usize, usize),
}

/// One change to a conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// A new node: a new start (an entry) when `under` is `None`, else a
    /// node of the other side linked under `under`'s node.
    Add {
        under: Option<(Side, usize)>,
    },
    /// A node's text, its own (no talk-table line).
    Text(Side, usize, String),
    /// An entry's speaker (an object's tag; empty for the conversation's
    /// owner) or listener.
    Speaker(Side, usize, String),
    Listener(Side, usize, String),
    /// A node's action script, run as it shows; empty for none.
    Script(Side, usize, String),
    /// A link's condition script; empty for none.
    Condition(LinkAt, String),
    /// A node's journal entry: the quest's tag and the entry's number.
    Quest(Side, usize, String, u32),
    /// A link to a node shown elsewhere (`IsChild`), added under the node
    /// `from` (or the start for `None`), to the other side's `to`.
    LinkTo {
        from: Option<(Side, usize)>,
        to: usize,
    },
    /// A link taken away; the node it led to stays.
    Unlink(LinkAt),
    /// A link moved up (`-1`) or down (`1`) among its node's links: the
    /// order a node's replies are offered in, and an entry's links are
    /// tried in.
    Shift(LinkAt, i32),
    /// A node taken away, with every link to it.
    Delete(Side, usize),
    /// The scripts run as the conversation ends, and as it is cut short.
    End(String),
    Abort(String),
    /// How the games show the conversation: 0 as speech between characters,
    /// 1 on a computer's screen, 2 on a Rakatan computer's.
    Screen(u8),
}

/// The longest resref, a script's or a conversation's name.
const RESREF_LEN: usize = crate::git::RESREF_LEN;

fn field(label: &str, value: GffValue) -> GffField {
    GffField {
        label: label.as_bytes().to_vec(),
        value,
    }
}

fn set(item: &mut GffStruct, label: &str, value: GffValue) {
    match item
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
    {
        Some(found) => found.value = value,
        None => item.fields.push(field(label, value)),
    }
}

fn list_mut<'a>(item: &'a mut GffStruct, label: &str) -> &'a mut Vec<GffStruct> {
    if !matches!(item.get(label), Some(GffValue::List(_))) {
        set(item, label, GffValue::List(Vec::new()));
    }
    let Some(GffValue::List(list)) = item
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
        .map(|field| &mut field.value)
    else {
        unreachable!("set just above");
    };
    list
}

fn resref(name: &str) -> Result<GffValue, String> {
    if name.len() > RESREF_LEN {
        return Err(format!("{name:?} is longer than a script's name"));
    }
    Ok(GffValue::ResRef(name.to_ascii_lowercase().into_bytes()))
}

/// A conversation with nothing in it: no lines, skippable, cinematic.
pub fn empty() -> Gff {
    Gff {
        file_type: *b"DLG ",
        root: GffStruct {
            id: u32::MAX,
            fields: vec![
                field("DelayEntry", GffValue::Dword(0)),
                field("DelayReply", GffValue::Dword(0)),
                field("NumWords", GffValue::Dword(0)),
                field("EndConversation", GffValue::ResRef(Vec::new())),
                field("EndConverAbort", GffValue::ResRef(Vec::new())),
                field("Skippable", GffValue::Byte(1)),
                field("ConversationType", GffValue::Int(0)),
                field("ComputerType", GffValue::Byte(0)),
                field("CameraModel", GffValue::ResRef(Vec::new())),
                field("AnimatedCut", GffValue::Byte(0)),
                field("OldHitCheck", GffValue::Byte(0)),
                field("UnequipItems", GffValue::Byte(0)),
                field("UnequipHItem", GffValue::Byte(0)),
                field("AmbientTrack", GffValue::ResRef(Vec::new())),
                field("EntryList", GffValue::List(Vec::new())),
                field("ReplyList", GffValue::List(Vec::new())),
                field("StartingList", GffValue::List(Vec::new())),
                field("StuntList", GffValue::List(Vec::new())),
            ],
        },
    }
}

/// A new node of `side`, saying nothing yet.
fn node(side: Side, id: u32) -> GffStruct {
    GffStruct {
        id,
        fields: vec![
            field(
                "Text",
                GffValue::LocString(LocString {
                    strref: u32::MAX,
                    strings: Vec::new(),
                }),
            ),
            field("Speaker", GffValue::String(Vec::new())),
            field("Listener", GffValue::String(Vec::new())),
            field("Script", GffValue::ResRef(Vec::new())),
            field("VO_ResRef", GffValue::ResRef(Vec::new())),
            field("Sound", GffValue::ResRef(Vec::new())),
            field("Comment", GffValue::String(Vec::new())),
            field("Delay", GffValue::Dword(u32::MAX)),
            field("WaitFlags", GffValue::Dword(0)),
            field("CameraAngle", GffValue::Dword(0)),
            field("FadeType", GffValue::Byte(0)),
            field("Quest", GffValue::String(Vec::new())),
            field("PlotIndex", GffValue::Int(-1)),
            field("PlotXPPercentage", GffValue::Float(1f32.to_bits())),
            field("AnimList", GffValue::List(Vec::new())),
            field(side.links(), GffValue::List(Vec::new())),
        ],
    }
}

/// A link to node `index`.
fn link(index: usize, child: bool) -> GffStruct {
    GffStruct {
        id: 0,
        fields: vec![
            field("Index", GffValue::Dword(index as u32)),
            field("Active", GffValue::ResRef(Vec::new())),
            field("IsChild", GffValue::Byte(u8::from(child))),
        ],
    }
}

/// The node `(side, index)` of the conversation.
fn node_mut(gff: &mut Gff, side: Side, index: usize) -> Result<&mut GffStruct, String> {
    let count = gff.root.list(side.list()).len();
    list_mut(&mut gff.root, side.list())
        .get_mut(index)
        .ok_or(format!(
            "the conversation has no {} {index} (it has {count})",
            side_name(side)
        ))
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Entry => "line",
        Side::Reply => "reply",
    }
}

/// The links a `LinkAt` is among, and its place there.
fn links_mut(gff: &mut Gff, at: LinkAt) -> Result<(&mut Vec<GffStruct>, usize), String> {
    match at {
        LinkAt::Start(n) => Ok((list_mut(&mut gff.root, "StartingList"), n)),
        LinkAt::From(side, index, n) => {
            let node = node_mut(gff, side, index)?;
            Ok((list_mut(node, side.links()), n))
        }
    }
}

/// Numbers each struct of a list by its place, as the games' files do.
fn renumber(list: &mut [GffStruct]) {
    for (at, item) in list.iter_mut().enumerate() {
        item.id = at as u32;
    }
}

/// Makes `edit` in the conversation `gff`. Answers the node an `Add`
/// made, else none.
pub fn apply(gff: &mut Gff, edit: &Edit) -> Result<Option<(Side, usize)>, String> {
    match edit {
        Edit::Add { under } => {
            let side = under.map_or(Side::Entry, |(side, _)| side.other());
            if let Some((parent, index)) = under {
                node_mut(gff, *parent, *index)?;
            }
            let list = list_mut(&mut gff.root, side.list());
            let new = list.len();
            list.push(node(side, new as u32));
            let links = match under {
                Some((parent, index)) => {
                    let node = node_mut(gff, *parent, *index)?;
                    list_mut(node, parent.links())
                }
                None => list_mut(&mut gff.root, "StartingList"),
            };
            links.push(link(new, false));
            renumber(links);
            return Ok(Some((side, new)));
        }
        Edit::Text(side, index, text) => {
            let node = node_mut(gff, *side, *index)?;
            set(
                node,
                "Text",
                GffValue::LocString(LocString {
                    strref: u32::MAX,
                    strings: if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![(0, crate::game_bytes(text))]
                    },
                }),
            );
        }
        Edit::Speaker(side, index, tag) | Edit::Listener(side, index, tag) => {
            if tag.len() > crate::git::TAG_LEN {
                return Err(format!(
                    "a tag has at most {} characters",
                    crate::git::TAG_LEN
                ));
            }
            let label = if matches!(edit, Edit::Speaker(..)) {
                "Speaker"
            } else {
                "Listener"
            };
            let node = node_mut(gff, *side, *index)?;
            set(node, label, GffValue::String(tag.as_bytes().to_vec()));
        }
        Edit::Script(side, index, script) => {
            let value = resref(script)?;
            set(node_mut(gff, *side, *index)?, "Script", value);
        }
        Edit::Condition(at, script) => {
            let value = resref(script)?;
            let (links, n) = links_mut(gff, *at)?;
            let link = links.get_mut(n).ok_or(format!("there is no link {n}"))?;
            set(link, "Active", value);
        }
        Edit::Quest(side, index, quest, entry) => {
            let node = node_mut(gff, *side, *index)?;
            set(node, "Quest", GffValue::String(quest.as_bytes().to_vec()));
            set(node, "QuestEntry", GffValue::Dword(*entry));
        }
        Edit::LinkTo { from, to } => {
            let side = from.map_or(Side::Entry, |(side, _)| side.other());
            let count = gff.root.list(side.list()).len();
            if *to >= count {
                return Err(format!("there is no {} {to}", side_name(side)));
            }
            let links = match from {
                Some((parent, index)) => {
                    let node = node_mut(gff, *parent, *index)?;
                    list_mut(node, parent.links())
                }
                None => list_mut(&mut gff.root, "StartingList"),
            };
            links.push(link(*to, from.is_some()));
            renumber(links);
        }
        Edit::Unlink(at) => {
            let (links, n) = links_mut(gff, *at)?;
            if n >= links.len() {
                return Err(format!("there is no link {n}"));
            }
            links.remove(n);
            renumber(links);
        }
        Edit::Shift(at, by) => {
            let (links, n) = links_mut(gff, *at)?;
            let to = n as i64 + i64::from(*by);
            if n >= links.len() || to < 0 || to as usize >= links.len() {
                return Err("the link cannot move that way".into());
            }
            links.swap(n, to as usize);
            renumber(links);
        }
        Edit::Delete(side, index) => {
            let count = gff.root.list(side.list()).len();
            if *index >= count {
                return Err(format!("there is no {} {index}", side_name(*side)));
            }
            let list = list_mut(&mut gff.root, side.list());
            list.remove(*index);
            renumber(list);
            // Every link to the side's nodes: those to it go, those past it
            // move down one.
            let fix = |links: &mut Vec<GffStruct>| {
                links.retain(|link| link.int("Index") != Some(*index as i64));
                for link in links.iter_mut() {
                    if let Some(at) = link.int("Index")
                        && at > *index as i64
                    {
                        set(link, "Index", GffValue::Dword((at - 1) as u32));
                    }
                }
                renumber(links);
            };
            if *side == Side::Entry {
                fix(list_mut(&mut gff.root, "StartingList"));
            }
            let parents = list_mut(&mut gff.root, side.other().list());
            for parent in parents.iter_mut() {
                fix(list_mut(parent, side.other().links()));
            }
        }
        Edit::End(script) => {
            let value = resref(script)?;
            set(&mut gff.root, "EndConversation", value);
        }
        Edit::Abort(script) => {
            let value = resref(script)?;
            set(&mut gff.root, "EndConverAbort", value);
        }
        Edit::Screen(kind) => {
            if *kind > 2 {
                return Err("a conversation is shown as talk, computer or rakatan".into());
            }
            set(
                &mut gff.root,
                "ConversationType",
                GffValue::Int(i32::from(*kind > 0)),
            );
            set(
                &mut gff.root,
                "ComputerType",
                GffValue::Byte(u8::from(*kind == 2)),
            );
        }
    }
    Ok(None)
}

/// The conversation as the walk reads it (`fmt_odyssey::dlg`).
pub fn read(gff: &Gff) -> Dlg {
    Dlg::read(&gff.root)
}

/// A node's text: its own, else its talk-table line through `talk`, else
/// nothing.
pub fn text_of(node: &fmt_odyssey::dlg::Node, talk: &dyn Fn(u32) -> Option<String>) -> String {
    match node.text.strings.first() {
        Some((_, own)) if !own.is_empty() => crate::game_text(own),
        _ if node.text.strref != u32::MAX => talk(node.text.strref).unwrap_or_else(String::new),
        _ => String::new(),
    }
}

/// Whether a node's text is a talk-table line, not its own.
pub fn from_talk_table(node: &fmt_odyssey::dlg::Node) -> bool {
    node.text.strings.iter().all(|(_, own)| own.is_empty()) && node.text.strref != u32::MAX
}

/// What is wrong with a conversation that would stop it playing as it
/// reads: a link to a node that is not there.
pub fn problems(gff: &Gff) -> Vec<String> {
    let dlg = read(gff);
    let mut found = Vec::new();
    for (at, link) in dlg.starts.iter().enumerate() {
        if link.index as usize >= dlg.entries.len() {
            found.push(format!(
                "start {at} leads to line {}, which is not there",
                link.index
            ));
        }
    }
    for (side, nodes, others) in [
        (Side::Entry, &dlg.entries, dlg.replies.len()),
        (Side::Reply, &dlg.replies, dlg.entries.len()),
    ] {
        for (at, node) in nodes.iter().enumerate() {
            for link in &node.links {
                if link.index as usize >= others {
                    found.push(format!(
                        "{} {at} leads to {} {}, which is not there",
                        side_name(side),
                        side_name(side.other()),
                        link.index
                    ));
                }
            }
        }
    }
    found
}
