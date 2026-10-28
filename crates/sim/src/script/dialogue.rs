//! Conversations (I18): DLG trees walked on the authority, inside the
//! scripts' step (`docs/DIALOGUE.md`).
//!
//! A conversation starts from `ActionStartConversation` (when the action
//! reaches the front of its caller's queue), `BeginConversation` (the
//! games' OnDialogue handlers answer a player's use with it), or a used
//! object that speaks a DLG and has no dialogue hook. It shows the first
//! entry whose starting link's condition holds; each entry offers the
//! replies whose links' conditions hold (all of them, in order), then runs
//! its action script. A player picks a reply (its script runs, then the
//! first entry it links to whose condition holds is shown) or lets the
//! line play out: an entry whose only reply says nothing takes it when the
//! line ends; one with no replies ends the conversation. Every script runs
//! for the conversation's owner. The end runs `EndConversation`, an end
//! cut short `EndConverAbort`.

use std::collections::BTreeMap;
use std::sync::Arc;

use fmt_odyssey::dlg::{Call, Dlg, Link, Node, NodeFade};
use script_ncs::vm::{ObjectId, Value};

use super::actions::Action;
use super::engine::{Bound, Engine, int, last, object, text, truth};
use super::party::Party;
use super::{Event, Step, View, key};
use crate::module::ObjectKind;
use crate::wire::{DecodeError, Reader, Writer};
mod told;
pub use told::*;
mod answers;
mod interrupts;
mod lines;
mod pass;
mod start;
pub(super) use pass::*;

/// How long a line with no usable voice stays up for each character of its
/// text, in tenths of a millisecond.
const READ_TENTHS_MS_PER_CHAR: u64 = rules::dialogue::lines::READ_TENTHS_MS_PER_CHAR;
/// A voice under this (ms) is not a line's length: its text is.
const MIN_VOICE_MS: u64 = rules::dialogue::lines::MIN_VOICE_MS;
/// The least a line lasts (ms), except in an animated cut.
const LEAST_LINE_MS: u64 = rules::dialogue::lines::LEAST_LINE_MS;
/// `WaitFlags`: the line waits until its camera model's clip ends.
const WAIT_ANIMATION: u32 = 1;
/// `WaitFlags`: the line waits until its voice ends.
const WAIT_VOICE: u32 = 2;
/// `WaitFlags`: the line waits until each `AnimList` participant's clip
/// ends.
const WAIT_ANIMS: u32 = 4;
/// `WaitFlags`: the line waits until its own fade (`FadeType`) is done.
const WAIT_FADE: u32 = 8;
/// How far into its fade a node's fade starts (ms).
const FADE_STARTS_MS: u64 = rules::dialogue::lines::FADE_STARTS_MS;
/// How long a bark stays over its speaker without a voice (ms) [verify in
/// K1's exe].
const BARK_MS: u64 = rules::dialogue::lines::BARK_MS;
/// How long a line shows before a skip may end it (ms): a skip meant for
/// the line before, arriving late, does not end this one unseen.
pub const SKIP_AFTER_MS: u64 = rules::dialogue::lines::SKIP_AFTER_MS;
/// Whether a skip may end entry `entry` of `dlg`: the conversation is
/// `Skippable` and the entry not TSL's `NodeUnskippable`. As TSL's
/// executable has it, a skip asked on such a line is not heard: the line
/// plays out (whether the ask then carries to the next line [verify]).
fn skippable(dlg: &Dlg, entry: u32) -> bool {
    dlg.skippable
        && dlg
            .entries
            .get(entry as usize)
            .is_none_or(|node| !node.unskippable)
}

/// How long a line a seat's body says waits on that seat's locked
/// conversation (ms) before it plays without it (R4): a tenth of K1's
/// lines have a party member speaking, so the wait must never stall.
pub const INTERJECTION_WAIT_MS: u64 = rules::dialogue::coop::INTERJECTION_WAIT_MS;

/// What a player answers (`View::picks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The nth reply offered, from 0.
    Reply(u8),
    /// End the line now (a click), where the conversation allows it.
    Skip,
    /// Point at the nth reply offered (the one highlighted): a marker the
    /// other players see, never an answer (I30 P6).
    Point(u8),
    /// Leave the conversation while it is unlocked (R6): its listener ends
    /// it cut short, one joined steps out.
    Leave,
}

/// A seat's new answer byte this step (`View::picks`): what it asks, and
/// the body the seat drives (`INVALID` for a watching seat, I30 P3).
/// It answers the conversation its body stands in, else the one holding
/// every seat (R1, R3). A story reply (`story`) is a PC's to take (I30
/// P6): another marks it, or, where no PC stands, asks the host in (R5).
/// A watching seat only marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub by: ObjectId,
    pub pick: Pick,
}

/// A conversation asked for, started in the step's dialogue pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Start {
    pub owner: ObjectId,
    pub pc: ObjectId,
    /// The body whose use asked for it (I30 P8): a guest's companion when a
    /// guest clicked, the PC itself when its own player did; `INVALID`
    /// when a script asked.
    pub asker: ObjectId,
    pub dialog: String,
    /// Only bark its first line (`SpeakOneLinerConversation`).
    pub bark: bool,
}

/// One conversation under way.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    pub id: u32,
    pub dialog: String,
    /// Whom its scripts run for.
    pub owner: ObjectId,
    /// The player it is held with (`GetPCSpeaker`).
    pub pc: ObjectId,
    /// Who asked for what it does next: the body whose use started it
    /// (`Start::asker`), then the one whose pick took its last reply. A
    /// store or a pazaak match it opens for its player opens for this body
    /// instead (I30 P8).
    pub asker: ObjectId,
    /// The entry shown, and who says it.
    pub entry: u32,
    pub speaker: ObjectId,
    /// The replies offered (`ReplyList` indices), in order.
    pub replies: Vec<u32>,
    pub shown_ms: u64,
    /// When the line ends; `u64::MAX` once it has, waiting on a reply.
    pub until_ms: u64,
    /// The one reply says nothing: taken when the line ends.
    pub auto: bool,
    /// `ActionPauseConversation`: the line holds until resumed.
    pub paused: bool,
    /// The seats pointing at a reply, by seat: the reply's place among
    /// those offered, from 0 (I30 P6). Never taken; cleared when the line
    /// changes. At most `MAX_SEATS` entries.
    pub marks: BTreeMap<u8, u8>,
    /// The area's placed camera a script cut to (`SetDialogPlaceableCamera`),
    /// 0 for none: the line shows from it instead of its own. Cleared when
    /// the line changes [verify].
    pub camera: i32,
    /// The body standing in it (R2): the asker's when a player's use
    /// started it, else `pc`. The camera's "player", facing, holding and
    /// answering use it; the scripts' PC stays `pc`.
    pub listener: ObjectId,
    /// Other players' bodies joined to it (R3), in the order they came.
    pub joined: Vec<ObjectId>,
    /// It holds every seat (R1): the game started it, or the host accepted a
    /// request.
    pub party: bool,
    /// A story step happened in it, or it is a cutscene (R6): no Leave.
    pub locked: bool,
    /// The body asking the host in (R5), `INVALID` for none.
    pub asking: ObjectId,
    /// A party member its line waits on (R4), `INVALID` for none.
    pub waits_for: ObjectId,
    /// The player of the pazaak match its line dealt (`PlayPazaak`),
    /// `INVALID` for none: the others in it watch that match once it ends
    /// (R37).
    pub dealt: ObjectId,
    /// The line's camera angle (S6): the shown entry's, while the replies
    /// are offered the first one's, and the reply said's; a default (0)
    /// resolved from `DEFAULT_ANGLES`.
    pub angle: u8,
    /// The place in `DEFAULT_ANGLES` (−1 before the first entry, which
    /// sets it from the DLG's name, `angle_walk`), the step it moves by and
    /// whether it walks forward; whether a reply set has been offered (the
    /// first set's defaults take 2, as the first entry's does).
    pub cursor: i8,
    pub step: u8,
    pub forward: bool,
    pub defaults_seen: bool,
    /// The angle each default node shown was resolved to, kept for when it
    /// shows again (the games write it back into the node, [verify] for
    /// longer than the conversation): an entry by its index, a reply as
    /// `REPLY_ANGLE | index`.
    pub angles: Vec<(u16, u8)>,
    /// TSL's alien speech: the place in `VOICE_VARIANTS` (−1 before the
    /// first entry, which sets it as the angles' walk starts,
    /// `angle_walk`), moved by every entry shown; and the variant each
    /// entry with alien speech took the first time it showed, kept for when
    /// it shows again, by entry.
    pub voice_cursor: i8,
    pub voices: Vec<(u16, u8)>,
    /// The line's listener (S2; while the replies are up, the one who
    /// spoke, the player saying them), and the speaker and listener of the
    /// last entry or reply said: at a line's start, the previous line's
    /// (`Step::listener_for`). The replies offered leave the pair.
    pub listener_now: ObjectId,
    pub pair: (ObjectId, ObjectId),
    /// The bodies a line does not turn (`SetLockOrientationInDialog`), and
    /// those whose body turns fully, the head not following
    /// (`SetLockHeadFollowInDialog`). Both empty as it starts.
    pub lock_turn: Vec<ObjectId>,
    pub lock_head: Vec<ObjectId>,
}

/// A line over someone's head, outside any conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct Bark {
    pub who: ObjectId,
    pub text: Said,
    pub until_ms: u64,
}

impl Bark {
    /// When it was barked (ms): its end less how long it lasts.
    pub fn began_ms(&self, library: &super::Library) -> u64 {
        self.until_ms.saturating_sub(bark_ms(library, &self.text))
    }
}

/// Where a line's text comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    Entry { dialog: String, entry: u32 },
    StrRef(u32),
}

/// How long a bark of `text` stays up (ms): an entry's voice as long as it
/// plays, anything else `BARK_MS` (K1's talk table gives its lines' voices
/// no length: every entry's is 0).
fn bark_ms(library: &super::Library, text: &Said) -> u64 {
    let voice = match text {
        Said::Entry { dialog, entry } => library
            .dialogue(dialog)
            .and_then(|dlg| dlg.entries.get(*entry as usize))
            .and_then(|node| library.voice(node)),
        Said::StrRef(_) => None,
    };
    voice.map_or(BARK_MS, |(_, ms)| u64::from(ms))
}

/// `who` barks talk-table line `strref` (`BarkString`, an
/// `ActionBarkString` whose turn came): over its head for `BARK_MS`, and in
/// the transcript.
pub(super) fn bark(
    library: &super::Library,
    state: &mut super::State,
    report: &mut super::Report,
    now: u64,
    (who, strref): (ObjectId, u32),
) {
    let text = Said::StrRef(strref);
    state.talk.barks.push(Bark {
        who,
        until_ms: now + bark_ms(library, &text),
        text,
    });
    report.talk.push(Line::Bark {
        ms: now,
        who,
        text: Said::StrRef(strref),
        conversation: false,
    });
}

/// Whether one of `conversations` holds the world (S1): one holds every
/// seat (`party`), or each of the seats' bodies (`seat_bodies`, at least
/// one) stands in one.
fn holds_world(conversations: &[Conversation], seat_bodies: &[ObjectId]) -> bool {
    conversations.iter().any(|c| c.party)
        || !seat_bodies.is_empty()
            && seat_bodies
                .iter()
                .all(|body| conversations.iter().any(|c| c.holds(*body)))
}

/// How long the line `node` lasts (ms): its voice's length (`voice`: its
/// own or its alien speech's, `Library::entry_voice`), or with none
/// or one under `MIN_VOICE_MS` its text's (`READ_TENTHS_MS_PER_CHAR` a
/// character of the text as stored, its developer notes and tokens counted
/// [verify]), raised to its `Delay`, to its voice's length where its wait
/// flags hold it for the voice, to its fade's end where they hold it for its
/// fade (`fade_ms`), and to `LEAST_LINE_MS`. A line with no wait
/// flags in an animated cut lasts nothing: the cut's animation times it. As
/// the games' loader has it, a `Delay` counts as a wait flag and a voiced
/// line (`voiced`: naming a voice of its own or alien speech) with neither
/// waits for its voice.
fn line_ms(
    library: &super::Library,
    node: &Node,
    (voice, voiced): (Option<u32>, bool),
    animated_cut: bool,
) -> u64 {
    let voice = u64::from(voice.unwrap_or(0));
    let chars = shown_text(library, node).chars().count() as u64;
    let base = if voice >= MIN_VOICE_MS {
        voice
    } else {
        chars * READ_TENTHS_MS_PER_CHAR / 10
    };
    let flags = match node.delay {
        None if voiced && node.wait_flags == 0 => WAIT_VOICE,
        _ => node.wait_flags,
    };
    if animated_cut && flags == 0 && node.delay.is_none() {
        return 0;
    }
    let held = if flags & WAIT_VOICE != 0 { voice } else { 0 };
    let faded = if flags & WAIT_FADE != 0 {
        fade_ms(&node.fade)
    } else {
        0
    };
    let delay = u64::from(node.delay.unwrap_or(0)) * 1000;
    base.max(delay).max(held).max(faded).max(LEAST_LINE_MS)
}

/// How long a line waiting on its fade (`WAIT_FADE`) is held by it (ms):
/// until the fade's delay and length have run, from `FADE_STARTS_MS` in,
/// as the K1 and TSL executables hold it (a script's fade holds no line).
/// A node with no fade holds nothing; one that covers or uncovers at once
/// (1, 2) has no length, its delay alone holding it.
fn fade_ms(fade: &NodeFade) -> u64 {
    let length = match fade.kind {
        0 => return 0,
        NodeFade::COVER | NodeFade::UNCOVER => 0.0,
        _ => fade.length,
    };
    let ms = ((fade.delay + length).max(0.0) * 1000.0) as u64;
    ms.saturating_sub(FADE_STARTS_MS)
}

/// The text a line shows: its own string, else its talk-table line's.
fn shown_text(library: &super::Library, node: &Node) -> String {
    match node.text.strings.first() {
        Some((_, own)) if !own.is_empty() => fmt_odyssey::code_page(own),
        _ => library.text(node.text.strref).unwrap_or_else(String::new),
    }
}

/// Conversations and what they keep (`State::talk`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Talk {
    pub conversations: Vec<Conversation>,
    pub asked: Vec<Start>,
    pub barks: Vec<Bark>,
    /// `SetCustomToken`: `<CUSTOMn>` in lines.
    pub tokens: BTreeMap<i32, String>,
    pub next: u32,
    /// Each seat's last answer byte (by seat number): a new count is a new
    /// answer or mark (`answer`).
    pub heard: BTreeMap<u32, u8>,
    /// The player of the conversation whose script runs now
    /// (`GetPCSpeaker`; not saved: within a step).
    pub(super) running: Option<ObjectId>,
    /// The conversation whose script runs now (not saved: within a step).
    pub(super) current: Option<u32>,
    /// The asker of the conversation whose end script runs now (not saved:
    /// within a step).
    pub(super) ending: Option<ObjectId>,
    /// Bodies standing in a conversation or playing a match that took
    /// damage this step (`world::scripts`, R7), and those whose seat let
    /// them go, and why (`world::drivers`, R12): the step's pazaak pass
    /// pauses or ends their matches, then its dialogue pass interrupts their
    /// conversations (not saved: within a step).
    pub(crate) hurt: Vec<ObjectId>,
    pub(crate) released: Vec<(ObjectId, Release)>,
}

/// Why a seat let go of the body it drove (`world::drivers`), for the
/// conversation that body stands in (R12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// Its player dropped (no command), or plays another body now.
    Dropped,
    /// It died.
    Died,
    /// It left the party.
    Left,
}

/// The conversation whose script runs now: its own run (`Talk::current`),
/// or code it deferred (`Event::talk`).
pub(super) fn running(talk: &Talk, event: &super::Event) -> Option<u32> {
    talk.current.or(event.talk)
}

/// What happened, for the transcript (`Report::talk`).
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Entry {
        ms: u64,
        id: u32,
        dialog: String,
        entry: u32,
        speaker: ObjectId,
        voice: Option<String>,
    },
    Offered {
        ms: u64,
        id: u32,
        dialog: String,
        replies: Vec<u32>,
    },
    Picked {
        ms: u64,
        id: u32,
        dialog: String,
        reply: u32,
    },
    /// A seat pointed at a reply (I30 P6): shown to everyone, never taken.
    Marked {
        ms: u64,
        id: u32,
        dialog: String,
        seat: u8,
        reply: u32,
    },
    Bark {
        ms: u64,
        who: ObjectId,
        text: Said,
        /// A conversation of one line (`BeginConversation`,
        /// `ActionStartConversation`), not a bark asked for as one
        /// (`SpeakOneLinerConversation`, `ActionBarkString`).
        conversation: bool,
    },
    Ended {
        ms: u64,
        id: u32,
        dialog: String,
        aborted: bool,
    },
    /// A start asked for that could not be (no such DLG, or someone in it
    /// is already talking), or a leave (R6): the conversation is locked,
    /// or it is the host's.
    Refused {
        ms: u64,
        dialog: String,
        why: &'static str,
    },
    /// Conversation `id` was cut short by another's start (R7), or by the
    /// host joining another (R5), before it ends (`Ended`).
    CutShort { ms: u64, id: u32, why: &'static str },
    /// `by`, standing in conversation `id` where no PC does, picked a story
    /// reply: it asks the host in (R5).
    Asked { ms: u64, id: u32, by: ObjectId },
    /// A quest reached an entry (a node's `Quest`, or a script's
    /// `AddJournalQuestEntry`).
    Journal { ms: u64, quest: String, entry: i32 },
}

/// A conversation with nothing in it: no one, no angle, the default-angle
/// table before its first default (`cursor` −1), and nothing locked.
impl Default for Conversation {
    fn default() -> Conversation {
        Conversation {
            id: 0,
            dialog: String::new(),
            owner: ObjectId::INVALID,
            pc: ObjectId::INVALID,
            asker: ObjectId::INVALID,
            entry: 0,
            speaker: ObjectId::INVALID,
            replies: Vec::new(),
            shown_ms: 0,
            until_ms: 0,
            auto: false,
            paused: false,
            marks: BTreeMap::new(),
            camera: 0,
            listener: ObjectId::INVALID,
            joined: Vec::new(),
            party: false,
            locked: false,
            asking: ObjectId::INVALID,
            waits_for: ObjectId::INVALID,
            dealt: ObjectId::INVALID,
            angle: 0,
            cursor: -1,
            step: 0,
            forward: false,
            defaults_seen: false,
            angles: Vec::new(),
            voice_cursor: -1,
            voices: Vec::new(),
            listener_now: ObjectId::INVALID,
            pair: (ObjectId::INVALID, ObjectId::INVALID),
            lock_turn: Vec::new(),
            lock_head: Vec::new(),
        }
    }
}

/// The camera angles a default line (`CameraAngle` 0) takes in turn, the
/// games' own sequence (S6).
const DEFAULT_ANGLES: [u8; 19] = rules::dialogue::cameras::DEFAULT_ANGLES;

/// The variants (1 to 3, the number after an alien sound's name) TSL's
/// entries take in turn, walked as `DEFAULT_ANGLES` is.
const VOICE_VARIANTS: [u8; 19] = rules::dialogue::lines::VOICE_VARIANTS;

/// The angle the first entry takes where it has none of its own, and the
/// first reply set's defaults, whatever the sequence says: over the
/// shoulder.
const FIRST_DEFAULT_ANGLE: u8 = rules::dialogue::cameras::FIRST_DEFAULT_ANGLE;

/// A reply's place in `Conversation::angles` is this or its index, beside
/// the entries'.
pub const REPLY_ANGLE: u16 = 0x8000;

/// Where a conversation of DLG `name` starts in `DEFAULT_ANGLES` (and in
/// `VOICE_VARIANTS`), the step it moves by and whether forward, from the
/// games' hash of the name,
/// lowercased ([verify] the games' resref is): the sum over its characters c
/// (signed bytes), the i-th from 0, of
/// c·(1 + int(10^i)) in wrapping 32-bit arithmetic, a power past the range
/// taken as `i32::MIN` (as x86's conversion gives). The start is the hash
/// mod 19 and the step mod 5, plus 1, both of the hash read unsigned, as
/// the games divide; an odd hash walks forward.
fn angle_walk(name: &str) -> (i8, u8, bool) {
    let name = name.to_ascii_lowercase();
    let hash = name.bytes().zip(0..).fold(0i32, |hash, (c, i)| {
        let power = 10f64.powi(i);
        let power = if power < 2_147_483_648.0 {
            power as i32
        } else {
            i32::MIN
        };
        let c = i32::from(c as i8);
        hash.wrapping_add(c.wrapping_mul(power.wrapping_add(1)))
    });
    let unsigned = hash as u32;
    (
        (unsigned % 19) as i8,
        (unsigned % 5 + 1) as u8,
        unsigned & 1 == 1,
    )
}

/// The place `cursor` holds in a table of 19 (as `DEFAULT_ANGLES`), the
/// cursor then moved by `step` (`forward` or back): past the end back to
/// the start and before the start to the end (not round).
fn walk(cursor: &mut i8, step: u8, forward: bool) -> usize {
    const LAST: i8 = DEFAULT_ANGLES.len() as i8 - 1;
    let at = (*cursor).clamp(0, LAST);
    let step = step as i8;
    let moved = if forward { at + step } else { at - step };
    *cursor = if moved > LAST {
        0
    } else if moved < 0 {
        LAST
    } else {
        moved
    };
    at as usize
}

/// A node's own `CameraAngle` as a conversation keeps it.
fn own_angle(own: u32) -> u8 {
    u8::try_from(own).unwrap_or(u8::MAX)
}

/// Reply `reply`'s own `CameraAngle` in `dlg` (0 for none).
fn reply_own(dlg: &Dlg, reply: u32) -> u32 {
    dlg.replies
        .get(reply as usize)
        .map_or(0, |node| node.camera_angle)
}

impl Conversation {
    /// The next default angle (S6): the sequence's at its place, the place
    /// then moved by its step, past the end back to the start and before
    /// the start to the end (not round). The first call sets the walk from
    /// the DLG's name (`angle_walk`).
    fn next_default(&mut self) -> u8 {
        if self.cursor < 0 {
            (self.cursor, self.step, self.forward) = angle_walk(&self.dialog);
        }
        DEFAULT_ANGLES[walk(&mut self.cursor, self.step, self.forward)]
    }

    /// The alien speech's variant of entry `entry`, shown now (TSL's): every
    /// entry moves the walk through `VOICE_VARIANTS` once, starting where
    /// the angles' walk starts, with its step; one with alien speech (`alien`)
    /// takes the variant drawn the first time it shows, and keeps it.
    fn entry_variant(&mut self, entry: u32, alien: bool) -> Option<u8> {
        let (start, step, forward) = angle_walk(&self.dialog);
        if self.voice_cursor < 0 {
            self.voice_cursor = start;
        }
        let drawn = VOICE_VARIANTS[walk(&mut self.voice_cursor, step, forward)];
        if !alien {
            return None;
        }
        let kept = self.voice_variant(entry);
        kept.or_else(|| {
            self.voices.push((entry as u16, drawn));
            Some(drawn)
        })
    }

    /// The variant of its alien speech entry `entry` took (`entry_variant`),
    /// if it has shown.
    pub fn voice_variant(&self, entry: u32) -> Option<u8> {
        let kept = self
            .voices
            .iter()
            .find(|(node, _)| u32::from(*node) == entry);
        kept.map(|(_, variant)| *variant)
    }

    /// Node `key`'s angle: the one it was resolved to before, else its
    /// own (`CameraAngle`), else (0) `default`, kept for when it shows
    /// again.
    fn resolve(&mut self, key: u16, own: u32, default: u8) -> u8 {
        if let Some(angle) = self.kept(key) {
            return angle;
        }
        if own != 0 {
            return own_angle(own);
        }
        self.angles.push((key, default));
        default
    }

    /// The angle node `key` was resolved to, if it was.
    fn kept(&self, key: u16) -> Option<u8> {
        let kept = self.angles.iter().find(|(node, _)| *node == key);
        kept.map(|(_, angle)| *angle)
    }

    /// The angle of `node`, the entry `entry` shown now: every entry moves
    /// the sequence once, whatever its own angle, and the first takes
    /// `FIRST_DEFAULT_ANGLE`.
    fn entry_angle(&mut self, entry: u32, node: &Node) -> u8 {
        let first = self.cursor < 0;
        let drawn = self.next_default();
        let default = if first { FIRST_DEFAULT_ANGLE } else { drawn };
        self.resolve(entry as u16, node.camera_angle, default)
    }

    /// The replies `offered` (of `dlg`) given their angles: the set moves
    /// the sequence once (a silent one too [verify]) and its defaults all
    /// take one angle, the first set's `FIRST_DEFAULT_ANGLE`.
    fn offer_angles(&mut self, dlg: &Dlg, offered: &[u32]) {
        let drawn = self.next_default();
        let default = if self.defaults_seen {
            drawn
        } else {
            FIRST_DEFAULT_ANGLE
        };
        self.defaults_seen = true;
        for &reply in offered {
            self.resolve(REPLY_ANGLE | reply as u16, reply_own(dlg, reply), default);
        }
    }

    /// Reply `reply`'s angle, given as its set was offered.
    fn reply_angle(&self, dlg: &Dlg, reply: u32) -> u8 {
        self.kept(REPLY_ANGLE | reply as u16)
            .unwrap_or_else(|| own_angle(reply_own(dlg, reply)))
    }

    /// Whether `body` stands in it: its listener or one joined.
    pub fn holds(&self, body: ObjectId) -> bool {
        self.listener == body || self.joined.contains(&body)
    }

    /// Whether `id` owns it, stands in it or speaks in it.
    fn involves(&self, id: ObjectId) -> bool {
        self.owner == id || self.speaker == id || self.holds(id)
    }

    /// Whether `id` takes part in it (`involves`), the host's two bodies
    /// (`View::host`) one where `id` is its PC: leading through a companion,
    /// the host's character is in the conversation that companion stands
    /// in, as one player's always was.
    fn takes_part(&self, id: ObjectId, host: [ObjectId; 2]) -> bool {
        self.involves(id) || self.pc == id && host.contains(&self.listener)
    }

    /// Whether its line holds: paused (`ActionPauseConversation`), or
    /// waiting on a party member to say it (R4). It takes no answer and
    /// no mark meanwhile.
    pub fn on_hold(&self) -> bool {
        self.paused || self.waits_for != ObjectId::INVALID
    }

    /// `body` comes to stand in it (R3, R4). A PC answers its story
    /// replies itself (`pc`): no one asks the host in any more (R5).
    fn take_in(&mut self, body: ObjectId, pc: bool) {
        self.joined.push(body);
        if pc {
            self.asking = ObjectId::INVALID;
        }
    }

    /// `body`, the body of seat `seat` (if a seat's), steps out of it, if it
    /// joined, its request and that seat's mark going with it (R6, R7, R12,
    /// R35).
    pub(super) fn step_out(&mut self, body: ObjectId, seat: Option<u8>) {
        if !self.joined.contains(&body) {
            return;
        }
        self.joined.retain(|joined| *joined != body);
        if self.asking == body {
            self.asking = ObjectId::INVALID;
        }
        if let Some(seat) = seat {
            self.marks.remove(&seat);
        }
    }
}

impl Talk {
    /// The conversation `who`'s pause or resume (`ActionPauseConversation`,
    /// `ActionResumeConversation`) acts on: the one whose script called it
    /// (`talk`) where `who` takes part in it or is its PC (a guest's line
    /// pausing `GetPCSpeaker`'s); else the one `who` takes part in.
    pub(super) fn held_by(
        &self,
        who: ObjectId,
        talk: Option<u32>,
        host: [ObjectId; 2],
    ) -> Option<usize> {
        let conversations = &self.conversations;
        talk.and_then(|id| conversations.iter().position(|c| c.id == id))
            .filter(|at| {
                let c = &conversations[*at];
                c.pc == who || c.takes_part(who, host)
            })
            .or_else(|| conversations.iter().position(|c| c.takes_part(who, host)))
    }

    /// The conversation under way `id` takes part in, if any
    /// (`Conversation::takes_part`, `host` the host's two bodies).
    pub fn with(&self, id: ObjectId, host: [ObjectId; 2]) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.takes_part(id, host))
    }

    /// The conversation `body` stands in (listener or joined), if any.
    pub fn holding(&self, body: ObjectId) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.holds(body))
    }

    /// The conversation holding every seat, if any (R1).
    pub fn party(&self) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.party)
    }

    /// Whether a conversation under way holds the world (S1), the seats'
    /// bodies `seat_bodies`: what KOTOR's dialog state is to its barks.
    pub fn holds_world(&self, seat_bodies: &[ObjectId]) -> bool {
        holds_world(&self.conversations, seat_bodies)
    }

    /// `who` barks the talk table's line `strref` from `now_ms` (ms), for a
    /// talk-table bark's time; the same line it still barks starts again.
    pub(crate) fn bark_line(&mut self, who: ObjectId, strref: u32, now_ms: u64) {
        let text = Said::StrRef(strref);
        self.barks
            .retain(|bark| bark.who != who || bark.text != text);
        self.barks.push(Bark {
            who,
            text,
            until_ms: now_ms + BARK_MS,
        });
    }

    /// The conversations under way and asked for with PC `from` go on with
    /// `to` (`SwitchPlayerCharacter`): its seat answers them, and it is
    /// their `GetPCSpeaker`.
    pub(super) fn retarget(&mut self, from: ObjectId, to: ObjectId) {
        let pcs = self
            .conversations
            .iter_mut()
            .map(|c| &mut c.pc)
            .chain(self.asked.iter_mut().map(|start| &mut start.pc))
            .chain(self.running.as_mut());
        for pc in pcs.filter(|pc| **pc == from) {
            *pc = to;
        }
        let bodies = self
            .conversations
            .iter_mut()
            .flat_map(|c| c.joined.iter_mut().chain([&mut c.listener]));
        for body in bodies.filter(|body| **body == from) {
            *body = to;
        }
    }

    /// The body a store or a pazaak match opened for `pc` opens for (I30
    /// P8): whoever asked for what the running conversation (or the one
    /// ending) does next (`Conversation::asker`), when that is another body.
    pub fn asker_of_running(&self, pc: ObjectId) -> Option<ObjectId> {
        self.current
            .and_then(|id| self.index(id))
            .map(|at| self.conversations[at].asker)
            .or(self.ending)
            .filter(|asker| *asker != ObjectId::INVALID && *asker != pc)
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.conversations.iter().position(|c| c.id == id)
    }

    /// What everyone sees of it (`Snapshot::talk`).
    pub fn told(&self) -> Told {
        Told {
            conversations: self.conversations.clone(),
            barks: self.barks.clone(),
            tokens: self.tokens.clone(),
        }
    }

    pub(super) fn encode(&self, out: &mut Writer) {
        encode_told(out, &self.conversations, &self.barks, &self.tokens);
        out.u32(self.asked.len() as u32);
        for start in &self.asked {
            out.u32(start.owner.0);
            out.u32(start.pc.0);
            out.u32(start.asker.0);
            out.string(&start.dialog);
            out.u8(u8::from(start.bark));
        }
        out.u32(self.next);
        out.u32(self.heard.len() as u32);
        for (seat, answer) in &self.heard {
            out.u32(*seat);
            out.u8(*answer);
        }
    }

    pub(super) fn decode(input: &mut Reader) -> Result<Talk, DecodeError> {
        let told = Told::decode(input)?;
        let mut talk = Talk {
            conversations: told.conversations,
            barks: told.barks,
            tokens: told.tokens,
            ..Talk::default()
        };
        for _ in 0..input.u32()? {
            talk.asked.push(Start {
                owner: ObjectId(input.u32()?),
                pc: ObjectId(input.u32()?),
                asker: ObjectId(input.u32()?),
                dialog: input.string()?,
                bark: input.u8()? != 0,
            });
        }
        talk.next = input.u32()?;
        for _ in 0..input.u32()? {
            talk.heard.insert(input.u32()?, input.u8()?);
        }
        Ok(talk)
    }
}

/// A command's answer byte (`Command::reply`): the low six bits what it
/// asks, the high two a count of answers made. Of the six, 1 to `PLACES`
/// take the reply at that place among those offered from 1 (a place past
/// the last is the last), the next `PLACES` point at the reply at their
/// place less `PLACES`, `LEAVE` leaves the conversation and `SKIP` skips
/// the line. A client holds it until its next answer; the authority
/// answers when the count moves, so a repeated or late command answers
/// once.
pub const SKIP: u8 = 63;
pub const LEAVE: u8 = 62;
/// The places a reply is taken or pointed at.
const PLACES: u8 = 30;
const COUNT: u8 = 6;

/// The answer byte after `last` that gives `pick`.
pub fn answer(last: u8, pick: Pick) -> u8 {
    let count = (last >> COUNT).wrapping_add(1) & 3;
    let choice = match pick {
        Pick::Reply(nth) => nth.min(PLACES - 1) + 1,
        Pick::Point(nth) => PLACES + nth.min(PLACES - 1) + 1,
        Pick::Leave => LEAVE,
        Pick::Skip => SKIP,
    };
    count << COUNT | choice
}

/// What an answer byte asks.
pub fn pick_of(answer: u8) -> Option<Pick> {
    match answer & SKIP {
        0 => None,
        SKIP => Some(Pick::Skip),
        LEAVE => Some(Pick::Leave),
        n @ 1..=PLACES => Some(Pick::Reply(n - 1)),
        n if n <= 2 * PLACES => Some(Pick::Point(n - PLACES - 1)),
        _ => None,
    }
}

/// The routines that move the story (the survey's set S,
/// `context/artifacts/2026-10-13-coop-dialogue/SURVEY.md`): a script that
/// may call one is the story's (`story`). XP alone is not: K1's "XP for
/// asking" lines and tutorial answers are anyone's. Locals are not: they
/// keep a conversation's own place ("talked to").
pub const STORY_ROUTINES: &[&str] = &[
    // Globals.
    "SetGlobalNumber",
    "SetGlobalBoolean",
    "SetGlobalString",
    "SetGlobalLocation",
    "IncrementGlobalNumber",
    "DecrementGlobalNumber",
    // The journal.
    "AddJournalQuestEntry",
    "RemoveJournalQuestEntry",
    // Alignment and influence.
    "AdjustAlignment",
    "ModifyInfluence",
    // The party.
    "AddPartyMember",
    "RemovePartyMember",
    "AddAvailableNPCByTemplate",
    "AddAvailableNPCByObject",
    "RemoveAvailableNPC",
    "ShowPartySelectionGUI",
    "SetNPCSelectability",
    "SwitchPlayerCharacter",
    "SetPartyLeader",
    "RemoveNPCFromPartyToBase",
    "SpawnAvailableNPC",
    "AddAvailablePUPByObject",
    "AssignPUP",
    // Where the party goes.
    "StartNewModule",
    "EndGame",
    "StartCreditSequence",
    "SetPlanetAvailable",
    "SetPlanetSelectable",
    "ShowGalaxyMap",
    // Who is hostile.
    "ChangeToStandardFaction",
    "ChangeFaction",
    "ChangeFactionByFaction",
    "AdjustReputation",
    "SurrenderToEnemies",
    // Items and credits.
    "CreateItemOnObject",
    "CreateItemOnFloor",
    "GiveItem",
    "ActionGiveItem",
    "ActionTakeItem",
    "GiveGoldToCreature",
    "TakeGoldFromCreature",
    "SetItemStackSize",
    // The character.
    "AddMultiClass",
    "GrantFeat",
    "GrantSpell",
    "AddBonusForcePoints",
    "SetCurrentForm",
    "AdjustCreatureAttributes",
    "AdjustCreatureSkills",
    "ModifyFortitudeSavingThrowBase",
    "ModifyReflexSavingThrowBase",
    "ModifyWillSavingThrowBase",
];

/// The routines of a minigame anyone may sit down to: a script that may
/// call one is not the story's, whatever else it calls (K1's pazaak
/// launchers set their wager's global, `k_ptar_pazup`).
const OWN_GAME: &[&str] = &["PlayPazaak"];

/// Whether `script` may move the story: it may call one of
/// `STORY_ROUTINES`, and none of `OWN_GAME`.
fn moves(library: &super::Library, script: &str) -> bool {
    library.may_call(script, STORY_ROUTINES) && !library.may_call(script, OWN_GAME)
}

/// Whether `node` moves the story as it plays: a script of its own that
/// may (`moves`), its quest, or its plot's XP.
fn node_moves(library: &super::Library, node: &Node) -> bool {
    [&node.script, &node.script2]
        .iter()
        .any(|call| moves(library, &call.script))
        || !node.quest.is_empty()
        || node.plot_index >= 0 && node.plot_xp > 0.0
}

/// Whether reply `reply` of `dlg` is the story's: only a PC (the host's
/// character, I30 P6) takes it; another player marks it, or asks the host
/// in (R5). Picking it reaches, before the next replies are offered, a
/// script that may call one of `STORY_ROUTINES` (its node's or an entry's
/// it leads to, through replies taken by themselves, or the conversation's
/// end script where it ends), the journal or plot XP. The links'
/// conditions are not asked: a reply that may move the story is the
/// host's.
pub fn story(library: &super::Library, dlg: &Dlg, reply: u32) -> bool {
    // A node that moves it, or the last before an end script that does.
    let acts = |node: &&Node| {
        node_moves(library, node) || node.links.is_empty() && moves(library, &dlg.on_end)
    };
    let Some(node) = dlg.replies.get(reply as usize) else {
        return false;
    };
    // The entries it may lead to, and on through replies taken by
    // themselves: a few deep, as far as the games chain them.
    let mut replies = vec![node];
    for _ in 0..STORY_DEPTH {
        if replies.iter().any(acts) {
            return true;
        }
        let entries: Vec<&Node> = replies
            .iter()
            .flat_map(|node| &node.links)
            .filter_map(|link| dlg.entries.get(link.index as usize))
            .collect();
        if entries.iter().any(acts) {
            return true;
        }
        replies = entries
            .iter()
            .filter(|entry| {
                entry.links.len() == 1
                    && dlg
                        .replies
                        .get(entry.links[0].index as usize)
                        .is_some_and(Node::is_silent)
            })
            .filter_map(|entry| dlg.replies.get(entry.links[0].index as usize))
            .collect();
        if replies.is_empty() {
            return false;
        }
    }
    false
}

/// How many replies taken by themselves `story` follows.
const STORY_DEPTH: usize = rules::dialogue::coop::STORY_DEPTH;

/// A line shown that moves the story (R6): its quest, its plot's XP (a plot
/// index alone moves nothing), or a script of its own that may call one of
/// `STORY_ROUTINES`.
pub fn story_line(library: &super::Library, dlg: &Dlg, entry: u32) -> bool {
    dlg.entries
        .get(entry as usize)
        .is_some_and(|node| node_moves(library, node))
}

/// Its `EndConverAbort` may call one of `STORY_ROUTINES` (R6): leaving
/// would move the story itself.
pub fn story_abort(library: &super::Library, dlg: &Dlg) -> bool {
    moves(library, &dlg.on_abort)
}

/// Whether a conversation with PC `id` is the party's: `id` is one of the
/// scripts' PCs (`View::pcs`), or the companion leading the party (`lent`).
fn party_pc(pcs: &[ObjectId], lent: Option<ObjectId>, id: ObjectId) -> bool {
    pcs.contains(&id) || lent == Some(id)
}

/// Who a DLG names by `tag` (a line's `AnimList`, an animated cutscene's
/// stunts): `owner` the conversation's owner, `player` its player, any
/// other tag the first object `tagged` finds by it.
pub fn participant<T>(
    tag: &str,
    (owner, player): (T, T),
    tagged: impl FnOnce(&str) -> Option<T>,
) -> Option<T> {
    match tag.to_ascii_lowercase().as_str() {
        "owner" => Some(owner),
        "player" => Some(player),
        _ => tagged(tag),
    }
}

impl View<'_> {
    /// The DLG a script names (any case), else the one `id` speaks.
    fn dialog_or_own(&self, named: &str, id: ObjectId) -> Option<String> {
        Some(key(named))
            .filter(|name| !name.is_empty())
            .or_else(|| self.conversation(id).map(str::to_string))
    }

    /// The DLG an object speaks (its `Conversation`), if any.
    pub fn conversation(&self, id: ObjectId) -> Option<&str> {
        let object = self.facts(id)?;
        Some(object.conversation.as_str()).filter(|name| !name.is_empty())
    }

    /// The PC of a conversation that `user` starts by using something
    /// (I30 P8). It is `user` itself when that is a PC. Otherwise `user`
    /// is a companion a guest drives, and the PC is the first one: the
    /// host's character. The scripts see one PC.
    pub(crate) fn pc_for(&self, table: &Party, user: ObjectId) -> ObjectId {
        if self.is_player(table, user) {
            user
        } else {
            self.first_pc(table)
        }
    }

    /// The scripts' PCs, by seat: the seats' own characters (`party`), the
    /// first seat's the creature it is switched to while it is
    /// (`Party::switched`, `world::switch`), from the moment a script
    /// switches it.
    pub(crate) fn pcs(&self, table: &Party) -> Vec<ObjectId> {
        let mut pcs = self.party.to_vec();
        if let Some(body) = table.switched_body().map(ObjectId) {
            match pcs.first_mut() {
                Some(first) => *first = body,
                None => pcs.push(body),
            }
        }
        pcs
    }

    pub(crate) fn first_pc(&self, table: &Party) -> ObjectId {
        self.pcs(table)
            .first()
            .copied()
            .unwrap_or(ObjectId::INVALID)
    }

    pub(crate) fn is_player(&self, table: &Party, id: ObjectId) -> bool {
        self.pcs(table).contains(&id)
    }

    /// A conversation's PC and asker when a script names `named` as its
    /// PC during a run a use raised by `asker` (`Event::asker`): a guest's
    /// companion named stands for the host's character, the scripts' only
    /// PC (I30 §0, R10). The use's body is the asker when the PC is the
    /// one its use names (`pc_for`, I30 P8); a script's own start has
    /// none.
    pub(crate) fn speakers(
        &self,
        table: &Party,
        named: ObjectId,
        asker: Option<ObjectId>,
    ) -> (ObjectId, ObjectId) {
        let guest = self
            .seat_of(named)
            .is_some_and(|seat| seat != crate::world::LEADER_SEAT.0);
        let pc = if guest && !self.is_player(table, named) {
            self.first_pc(table)
        } else {
            named
        };
        let asker = asker
            .filter(|asker| self.pc_for(table, *asker) == pc)
            .unwrap_or(ObjectId::INVALID);
        (pc, asker)
    }

    /// The host's character (the scripts' first PC), and the body its seat
    /// acts through (`seats`: the companion leading, while one does).
    pub(crate) fn host(&self, table: &Party) -> [ObjectId; 2] {
        let pc = self.first_pc(table);
        let acting = self
            .seats
            .iter()
            .find(|(seat, _)| *seat == crate::world::LEADER_SEAT.0)
            .map_or(pc, |(_, body)| *body);
        [pc, acting]
    }

    /// More than one seat plays a body: only then does a hurt interrupt a
    /// conversation or pause a match (R7, R9), or a gather close a store
    /// (R8). One player alone plays as the originals do (R1).
    pub(crate) fn together(&self) -> bool {
        self.seats.len() > 1
    }

    /// The seat whose body `body` is (`seats`), if any.
    pub(crate) fn seat_of(&self, body: ObjectId) -> Option<u8> {
        self.seats
            .iter()
            .find(|(_, of)| *of == body)
            .map(|(seat, _)| *seat)
    }
}

/// What becomes of a player's own conversation when something happens to
/// its listener (`Step::interrupt`).
enum Interrupted {
    /// Cut short (`EndConverAbort`).
    Cut,
    /// Handed to the host (`Step::hand_to_host`).
    Handed,
}

/// Whether `dlg` is a cutscene (R1): animated, with a camera model, or not
/// to be skipped. A player's use of it starts a party conversation.
fn cutscene(dlg: &Dlg) -> bool {
    dlg.animated_cut || !dlg.camera_model.is_empty() || !dlg.skippable
}

impl Step<'_> {
    /// The companion leading the party in its player's hands
    /// (`world::lent`), whose conversations are the party's as a
    /// player's own character's are (`party_pc`).
    fn lent(&self) -> Option<ObjectId> {
        crate::world::lent(&self.state.party).map(|body| ObjectId(body.0))
    }

    /// Runs a DLG's script for `owner`, in conversation `id` with `pc`, with
    /// its parameters; its int.
    fn call(&mut self, call: &Call, id: u32, owner: ObjectId, pc: ObjectId) -> Option<i32> {
        if call.script.is_empty() {
            return None;
        }
        // Another conversation's, should this one run within its script,
        // goes on after it.
        let talk = &mut self.state.talk;
        let before = (talk.running.replace(pc), talk.current.replace(id));
        let event = Event {
            params: Some(Arc::new(call.clone())),
            ..Event::default()
        };
        #[cfg(feature = "probes")]
        let queues = self.state.actions.clone();
        let returned = self.run(&call.script, owner, event, None);
        (self.state.talk.running, self.state.talk.current) = before;
        // The objects whose queues the conversation's script changed are
        // the ones it directs (`kjk dialogue check`'s scene moves).
        #[cfg(feature = "probes")]
        for (who, queue) in &self.state.actions {
            if queues.get(who) != Some(queue) {
                self.report
                    .directed
                    .push((ObjectId(*who), self.view.now_ms));
            }
        }
        returned
    }

    /// Whether a link may be taken: its condition, and TSL's second one
    /// joined by `Logic`, each turned by its `Not`. A script that is
    /// missing or returns no int does not hold, `Not` or not (xoreos; reone
    /// holds it).
    fn holds(&mut self, link: &Link, id: u32, owner: ObjectId, pc: ObjectId) -> bool {
        let mut test = |call: &Call, not: bool| -> Option<bool> {
            (!call.script.is_empty()).then(|| {
                self.call(call, id, owner, pc)
                    .is_some_and(|returned| (returned != 0) != not)
            })
        };
        let first = test(&link.active, link.not);
        let second = test(&link.active2, link.not2);
        match (first, second) {
            (None, None) => true,
            (Some(one), None) | (None, Some(one)) => one,
            (Some(a), Some(b)) if link.logic == 1 => a || b,
            (Some(a), Some(b)) => a && b,
        }
    }

    fn line(&mut self, line: Line) {
        self.report.talk.push(line);
    }

    /// A party conversation gathers every seat (R8): while more than one
    /// seat plays a body (`together`), each trade ends as its Leave ends
    /// it, but one whose player stands in a player's own conversation, its
    /// own or one it joined (not gathered, R5). One player shops on, as the
    /// originals do.
    fn close_stores(&mut self) {
        if !self.view.together() {
            return;
        }
        let talk = &self.state.talk;
        let gathered: Vec<u32> = self
            .state
            .trades
            .keys()
            .copied()
            .filter(|player| talk.holding(ObjectId(*player)).is_none_or(|c| c.party))
            .collect();
        for player in gathered {
            self.state.end_trade(player);
        }
    }

    /// Ends conversation `id`: `EndConversation`, or `EndConverAbort` when
    /// cut short. Ended normally, those in it may watch the match it dealt
    /// (R37; R40: not cut short, by its listener's Leave among others).
    pub(super) fn end(&mut self, id: u32, aborted: bool) {
        let Some(at) = self.state.talk.index(id) else {
            return;
        };
        let c = self.state.talk.conversations.remove(at);
        // Its end script's store opens for whoever asked, as its nodes' do.
        self.state.talk.ending = Some(c.asker);
        self.finish(id, (c.owner, c.pc), &c.dialog, (aborted, Heard::Area));
        self.state.talk.ending = None;
        if !aborted {
            super::pazaak::watch_after_talk(self.view, self.state, &c);
        }
    }

    fn finish(
        &mut self,
        id: u32,
        (owner, pc): (ObjectId, ObjectId),
        dialog: &str,
        (aborted, heard): (bool, Heard),
    ) {
        let now = self.view.now_ms;
        self.line(Line::Ended {
            ms: now,
            id,
            dialog: dialog.to_string(),
            aborted,
        });
        // What its nodes left covering is lifted, before its end script
        // (whose own fades then win).
        if let Some(fade) = self.out.fade(self.view).filter(|fade| fade.conversation) {
            self.out.fade = Some(fade.lifted(now));
        }
        let library = self.library;
        let Some(dlg) = library.dialogue(dialog) else {
            return;
        };
        let script = if aborted { &dlg.on_abort } else { &dlg.on_end };
        let call = Call {
            script: script.clone(),
            ..Call::default()
        };
        self.call(&call, id, owner, pc);
        if !aborted {
            self.heard_end(owner, heard);
        }
    }

    /// Those who hear a conversation end (`Heard`) run their end-of-
    /// conversation scripts at once. K1's exe runs, after EndConversation,
    /// every creature's and placeable's end script in the owner's area: a
    /// creature's `ScriptEndDialogu`, a placeable's `OnEndDialogue`. Ended
    /// normally, every living creature and unbroken placeable of the area,
    /// the owner among them, in the area's order (its instances, then those
    /// made since); a one-liner, its owner only; one cut short, none. The
    /// dead hear nothing, as they hear no heartbeat [verify: that K1's exe
    /// skips the dead]. The Endar Spire's reinforcements, made hostile in
    /// the conversation, attack as it ends (`k_def_endconv` →
    /// `k_ai_master`).
    fn heard_end(&mut self, owner: ObjectId, heard: Heard) {
        let mut hearers: Vec<(bool, u32, ObjectId)> = match heard {
            Heard::Owner => vec![(false, 0, owner)],
            Heard::Area => self
                .view
                .things
                .iter()
                .filter(|thing| {
                    matches!(
                        thing.kind,
                        Some(ObjectKind::Creature | ObjectKind::Placeable)
                    ) && !thing.dead()
                })
                .map(|thing| (thing.index.is_none(), thing.index.unwrap_or(0), thing.id))
                .collect(),
        };
        hearers.sort_by_key(|&(made, index, id)| (made, index, id.0));
        for (_, _, who) in hearers {
            let Some(script) = self.view.hook(who, super::Hook::EndDialogue) else {
                continue;
            };
            let ai = super::engine::own_ai(self.view, who, &script);
            let event = Event {
                ai: ai.then_some(who),
                ..Event::default()
            };
            self.run(&script, who, event, None);
        }
    }
}
