//! DLG: a conversation tree, read from its GFF.
//!
//! Entries are what others say, replies what the player answers; each node
//! links to nodes of the other list. `StartingList` links into the entries.
//! A link's `Active` script is its condition; a node's `Script` runs when
//! it is shown. TSL adds a second condition (`Active2`, combined by
//! `Logic`, each negated by `Not`/`Not2`), a second action (`Script2`) and
//! parameters for each script.

use crate::FormatError;
use crate::gff::{Gff, GffStruct, GffValue, LocString};

/// A conversation as the walk needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Dlg {
    pub entries: Vec<Node>,
    pub replies: Vec<Node>,
    pub starts: Vec<Link>,
    pub kind: Kind,
    /// `ComputerType`: 0 the standard screen, 1 the Rakatan one.
    pub computer: u8,
    pub skippable: bool,
    /// `EndConversation`: run when it ends; `EndConverAbort`: when it is
    /// cut short.
    pub on_end: String,
    pub on_abort: String,
    pub camera_model: String,
    pub animated_cut: bool,
    /// `OldHitCheck`: selects an older variant of the dialog camera
    /// [verify].
    pub old_hit_check: bool,
    /// `UnequipItems` and `UnequipHItem`: the speaker's and listener's item
    /// models are hidden for the conversation, or only the held ones.
    pub unequip_items: bool,
    pub unequip_held: bool,
    /// `StuntList`: (participant tag, stunt model).
    pub stunts: Vec<(String, String)>,
    /// TSL's `AlienRaceOwner`: the `alienvo.2da` row whose speech its
    /// owner's lines play (`alien_race`); 0 for none.
    pub alien_race_owner: i32,
}

impl Dlg {
    /// Its lines stage their participants on stunt models' clips: an
    /// animated cutscene, or any conversation that lists stunt participants
    /// (the Taris wake-up's `AnimatedCut` is 0 but it stunts its player in
    /// bed). [verify] reone plays stunts only for `AnimatedCut`.
    pub fn staged(&self) -> bool {
        self.animated_cut || !self.stunts.is_empty()
    }

    /// The `alienvo.2da` row whose speech `node` plays, as TSL's loader
    /// picks it: its own `AlienRaceNode`, else for a line its owner says
    /// (no `Speaker`) the conversation's `AlienRaceOwner`; none for 0.
    pub fn alien_race(&self, node: &Node) -> Option<u32> {
        let race = if node.alien_race <= 0 && node.speaker.is_empty() {
            self.alien_race_owner
        } else {
            node.alien_race
        };
        u32::try_from(race).ok().filter(|&race| race > 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cinematic,
    Computer,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub text: LocString,
    /// The speaker's tag (empty: the conversation's owner).
    pub speaker: String,
    pub listener: String,
    pub script: Call,
    /// TSL's `Script2`.
    pub script2: Call,
    pub vo: String,
    pub sound: String,
    /// Seconds the node stays up, if set (`Delay`, `0xffff_ffff` for none).
    pub delay: Option<u32>,
    pub wait_flags: u32,
    pub camera_angle: u32,
    pub camera_id: i32,
    pub camera_animation: u16,
    pub camera_fov: f32,
    /// `CamHeightOffset` and `TarHeightOffset`: metres the camera and the
    /// point it looks at are raised (0 when absent).
    pub cam_height: f32,
    pub target_height: f32,
    /// `CamVidEffect`: the `videoeffects.2da` row the line's camera shows
    /// (-1 when absent; negative values keep or clear one).
    pub cam_vid_effect: i32,
    pub fade: NodeFade,
    pub quest: String,
    pub quest_entry: u32,
    pub plot_index: i32,
    pub plot_xp: f32,
    /// `AnimList`: (participant tag, `dialoganimations.2da` row + 10000).
    pub anims: Vec<(String, u16)>,
    /// TSL's `NodeUnskippable`: a skip is refused while it shows.
    pub unskippable: bool,
    /// TSL's `AlienRaceNode` (an `alienvo.2da` row, 0 for none) and
    /// `Emotion` (an `emotion.2da` row): the alien speech the line plays
    /// (`Dlg::alien_race`).
    pub alien_race: i32,
    pub emotion: i32,
    /// To the other list: an entry's replies, a reply's entries.
    pub links: Vec<Link>,
}

/// A node's own fade as it plays: `FadeType` (1 covers at once, 2 uncovers
/// at once, 3 fades in, 4 fades out; 0 none), after `FadeDelay` seconds
/// over `FadeLength` (the instant kinds have none), to or from `FadeColor`
/// (red, green, blue, 0 to 1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NodeFade {
    pub kind: u8,
    pub delay: f32,
    pub length: f32,
    pub colour: [f32; 3],
}

impl NodeFade {
    pub const COVER: u8 = 1;
    pub const UNCOVER: u8 = 2;
    pub const IN: u8 = 3;
    pub const OUT: u8 = 4;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub index: u32,
    pub active: Call,
    /// TSL's second condition and how the two combine (`Logic`: 0 and,
    /// 1 or).
    pub active2: Call,
    pub logic: i32,
    pub not: bool,
    pub not2: bool,
    /// A link back to a node shown elsewhere in the tree.
    pub is_child: bool,
}

/// A script and the parameters TSL passes it (`GetScriptParameter`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Call {
    pub script: String,
    pub ints: [i32; 5],
    pub string: Vec<u8>,
}

impl Call {
    fn read(node: &GffStruct, script: &str, ints: [&str; 5], string: &str) -> Call {
        Call {
            script: node.resref(script).unwrap_or_else(String::new),
            ints: ints.map(|label| node.int(label).unwrap_or(0) as i32),
            string: node
                .bytes(string)
                .map(<[u8]>::to_vec)
                .unwrap_or_else(Vec::new),
        }
    }
}

const PARAMS: [&str; 5] = ["Param1", "Param2", "Param3", "Param4", "Param5"];
const PARAMS_B: [&str; 5] = ["Param1b", "Param2b", "Param3b", "Param4b", "Param5b"];
const ACTION_PARAMS: [&str; 5] = [
    "ActionParam1",
    "ActionParam2",
    "ActionParam3",
    "ActionParam4",
    "ActionParam5",
];
const ACTION_PARAMS_B: [&str; 5] = [
    "ActionParam1b",
    "ActionParam2b",
    "ActionParam3b",
    "ActionParam4b",
    "ActionParam5b",
];

impl Link {
    fn read(link: &GffStruct) -> Link {
        Link {
            index: link.int("Index").unwrap_or(0) as u32,
            active: Call::read(link, "Active", PARAMS, "ParamStrA"),
            active2: Call::read(link, "Active2", PARAMS_B, "ParamStrB"),
            logic: link.int("Logic").unwrap_or(0) as i32,
            not: link.int("Not").unwrap_or(0) != 0,
            not2: link.int("Not2").unwrap_or(0) != 0,
            is_child: link.int("IsChild").unwrap_or(0) != 0,
        }
    }
}

impl Node {
    fn read(node: &GffStruct, links: &str) -> Node {
        let delay = node.int("Delay").map(|v| v as u32);
        Node {
            text: node.locstring("Text").cloned().unwrap_or(LocString {
                strref: u32::MAX,
                strings: Vec::new(),
            }),
            speaker: node.resref("Speaker").unwrap_or_else(String::new),
            listener: node.resref("Listener").unwrap_or_else(String::new),
            script: Call::read(node, "Script", ACTION_PARAMS, "ActionParamStrA"),
            script2: Call::read(node, "Script2", ACTION_PARAMS_B, "ActionParamStrB"),
            vo: node.resref("VO_ResRef").unwrap_or_else(String::new),
            sound: node.resref("Sound").unwrap_or_else(String::new),
            delay: delay.filter(|&v| v != u32::MAX),
            wait_flags: node.int("WaitFlags").unwrap_or(0) as u32,
            camera_angle: node.int("CameraAngle").unwrap_or(0) as u32,
            camera_id: node.int("CameraID").unwrap_or(0) as i32,
            camera_animation: node.int("CameraAnimation").unwrap_or(0) as u16,
            camera_fov: node.float("CamFieldOfView").unwrap_or(0.0),
            cam_height: node.float("CamHeightOffset").unwrap_or(0.0),
            target_height: node.float("TarHeightOffset").unwrap_or(0.0),
            cam_vid_effect: node.int("CamVidEffect").map_or(-1, |value| value as i32),
            fade: NodeFade {
                kind: node.int("FadeType").unwrap_or(0) as u8,
                delay: node.float("FadeDelay").unwrap_or(0.0),
                length: node.float("FadeLength").unwrap_or(0.0),
                colour: node.vector("FadeColor").unwrap_or([0.0; 3]),
            },
            quest: node.resref("Quest").unwrap_or_else(String::new),
            quest_entry: node.int("QuestEntry").unwrap_or(0) as u32,
            plot_index: node.int("PlotIndex").unwrap_or(-1) as i32,
            plot_xp: node.float("PlotXPPercentage").unwrap_or(0.0),
            anims: node
                .list("AnimList")
                .iter()
                .map(|anim| {
                    (
                        anim.resref("Participant").unwrap_or_else(String::new),
                        anim.int("Animation").unwrap_or(0) as u16,
                    )
                })
                .collect(),
            unskippable: node.int("NodeUnskippable").unwrap_or(0) != 0,
            alien_race: node.int("AlienRaceNode").unwrap_or(0) as i32,
            emotion: node.int("Emotion").unwrap_or(0) as i32,
            links: node.list(links).iter().map(Link::read).collect(),
        }
    }

    /// Whether it says nothing: no text of its own and no strref.
    pub fn is_silent(&self) -> bool {
        self.text.strref == u32::MAX && self.text.strings.iter().all(|(_, s)| s.is_empty())
    }
}

/// The fields of an entry or a reply `Node::read` reads, and those it
/// passes over knowingly because neither game's executable acts on them:
/// `Comment` and `SoundExists`, the toolset's note and bookkeeping; TSL's
/// `FacialAnim` (read into the node, never read back),
/// `PostProcNode`/`PostProcOwner` (named by no code), and the toolset's
/// `NodeID`, `Changed`, `RecordVO`, `RecordNoVOOverri` and `VOTextChanged`
/// (named nowhere in the executable).
pub const NODE_FIELDS: &[&str] = &[
    "Text",
    "Speaker",
    "Listener",
    "Script",
    "Script2",
    "ActionParam1",
    "ActionParam2",
    "ActionParam3",
    "ActionParam4",
    "ActionParam5",
    "ActionParam1b",
    "ActionParam2b",
    "ActionParam3b",
    "ActionParam4b",
    "ActionParam5b",
    "ActionParamStrA",
    "ActionParamStrB",
    "VO_ResRef",
    "Sound",
    "Delay",
    "WaitFlags",
    "CameraAngle",
    "CameraID",
    "CameraAnimation",
    "CamFieldOfView",
    "CamHeightOffset",
    "TarHeightOffset",
    "CamVidEffect",
    "FadeType",
    "FadeDelay",
    "FadeLength",
    "FadeColor",
    "Quest",
    "QuestEntry",
    "PlotIndex",
    "PlotXPPercentage",
    "AnimList",
    "RepliesList",
    "EntriesList",
    "NodeUnskippable",
    "AlienRaceNode",
    "Emotion",
    "Comment",
    "SoundExists",
    "FacialAnim",
    "PostProcNode",
    "PostProcOwner",
    "NodeID",
    "Changed",
    "RecordVO",
    "RecordNoVOOverri",
    "VOTextChanged",
];

/// The fields of a DLG's entries and replies `Node::read` does not read
/// (`NODE_FIELDS`) that are set: a number not 0 or −1, a string, list or
/// struct not empty. Each as (an entry, its index, the label), in order.
pub fn unread_fields(root: &GffStruct) -> Vec<(bool, u32, String)> {
    let set = |value: &GffValue| match value {
        GffValue::String(s) | GffValue::ResRef(s) | GffValue::Void(s) => !s.is_empty(),
        GffValue::List(items) => !items.is_empty(),
        GffValue::Struct(child) => !child.fields.is_empty(),
        GffValue::LocString(loc) => loc.strref != u32::MAX || !loc.strings.is_empty(),
        GffValue::Float(bits) => f32::from_bits(*bits) != 0.0,
        GffValue::Double(bits) => f64::from_bits(*bits) != 0.0,
        GffValue::Orientation(_) | GffValue::Vector(_) => true,
        other => other
            .int()
            .is_some_and(|v| v != 0 && v != -1 && v != i64::from(u32::MAX)),
    };
    let mut out = Vec::new();
    for (entry, list) in [(true, "EntryList"), (false, "ReplyList")] {
        for (index, node) in root.list(list).iter().enumerate() {
            for field in &node.fields {
                let label = String::from_utf8_lossy(&field.label);
                if !NODE_FIELDS.contains(&label.as_ref()) && set(&field.value) {
                    out.push((entry, index as u32, label.into_owned()));
                }
            }
        }
    }
    out
}

impl Dlg {
    pub fn parse(bytes: &[u8]) -> Result<Dlg, FormatError> {
        let gff = Gff::parse(bytes)?;
        if &gff.file_type != b"DLG " {
            return Err(FormatError::Unsupported {
                format: "DLG",
                what: format!(
                    "a GFF of type {:?}",
                    String::from_utf8_lossy(&gff.file_type)
                ),
            });
        }
        Ok(Dlg::read(&gff.root))
    }

    pub fn read(root: &GffStruct) -> Dlg {
        Dlg {
            entries: root
                .list("EntryList")
                .iter()
                .map(|node| Node::read(node, "RepliesList"))
                .collect(),
            replies: root
                .list("ReplyList")
                .iter()
                .map(|node| Node::read(node, "EntriesList"))
                .collect(),
            starts: root.list("StartingList").iter().map(Link::read).collect(),
            kind: match root.int("ConversationType") {
                Some(1) => Kind::Computer,
                _ => Kind::Cinematic,
            },
            computer: root.int("ComputerType").unwrap_or(0) as u8,
            skippable: root.int("Skippable").unwrap_or(0) != 0,
            on_end: root.resref("EndConversation").unwrap_or_else(String::new),
            on_abort: root.resref("EndConverAbort").unwrap_or_else(String::new),
            camera_model: root.resref("CameraModel").unwrap_or_else(String::new),
            animated_cut: root.int("AnimatedCut").unwrap_or(0) != 0,
            old_hit_check: root.int("OldHitCheck").unwrap_or(0) != 0,
            unequip_items: root.int("UnequipItems").unwrap_or(0) != 0,
            unequip_held: root.int("UnequipHItem").unwrap_or(0) != 0,
            stunts: root
                .list("StuntList")
                .iter()
                .map(|stunt| {
                    (
                        stunt.resref("Participant").unwrap_or_else(String::new),
                        stunt.resref("StuntModel").unwrap_or_else(String::new),
                    )
                })
                .collect(),
            alien_race_owner: root.int("AlienRaceOwner").unwrap_or(0) as i32,
        }
    }

    /// Every string parameter its calls pass (TSL's `ParamStrA`,
    /// `ActionParamStrA`, and their B), lowercase, without repeats: a
    /// script's own words, or a script or DLG the script goes on to name
    /// (`a_pazaak`'s end script, the den's `a_pazgeredi`).
    pub fn strings(&self) -> Vec<String> {
        let links = self
            .starts
            .iter()
            .chain(
                self.entries
                    .iter()
                    .chain(&self.replies)
                    .flat_map(|n| &n.links),
            )
            .flat_map(|link| [&link.active, &link.active2]);
        let nodes = self
            .entries
            .iter()
            .chain(&self.replies)
            .flat_map(|node| [&node.script, &node.script2]);
        let mut strings: Vec<String> = links
            .chain(nodes)
            .filter(|call| !call.string.is_empty())
            .map(|call| String::from_utf8_lossy(&call.string).to_ascii_lowercase())
            .collect();
        strings.sort_unstable();
        strings.dedup();
        strings
    }

    /// Every script the conversation names: conditions, actions, its end
    /// and abort, without repeats.
    pub fn scripts(&self) -> Vec<&str> {
        let links = self
            .starts
            .iter()
            .chain(
                self.entries
                    .iter()
                    .chain(&self.replies)
                    .flat_map(|n| &n.links),
            )
            .flat_map(|link| [&link.active, &link.active2]);
        let nodes = self
            .entries
            .iter()
            .chain(&self.replies)
            .flat_map(|node| [&node.script, &node.script2]);
        let mut names: Vec<&str> = links
            .chain(nodes)
            .map(|call| call.script.as_str())
            .chain([self.on_end.as_str(), self.on_abort.as_str()])
            .filter(|name| !name.is_empty())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}
