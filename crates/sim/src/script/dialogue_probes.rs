//! Conversations' probes: the formats they read (DLG, LIP, KOTOR's voice
//! WAVs) from bytes made here, and what they keep (answers, the scripts'
//! state and the snapshot's view) through their encodings. The approved
//! tests `dialogue_formats` and `dialogue_state` need every one to pass.

use std::collections::BTreeMap;

use fmt_odyssey::dlg::{Dlg, Kind};
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue, LocString};
use fmt_odyssey::lip::Lip;
use fmt_odyssey::wav::{Coding, Wav};
use script_ncs::probes::Check;
use script_ncs::vm::ObjectId;

use super::actions::{Action, Queued};
use super::dialogue::{
    Bark, Conversation, LEAVE, Pick, SKIP, Said, Start, Talk, Told, answer, pick_of,
};
use super::state::State;
use crate::wire::{Reader, Writer};

fn check(name: &'static str, ok: bool, detail: String) -> Check {
    Check { name, ok, detail }
}

fn field(label: &str, value: GffValue) -> GffField {
    GffField {
        label: label.as_bytes().to_vec(),
        value,
    }
}

fn node(fields: Vec<GffField>) -> GffStruct {
    GffStruct { id: 0, fields }
}

fn resref(text: &str) -> GffValue {
    GffValue::ResRef(text.as_bytes().to_vec())
}

fn float(value: f32) -> GffValue {
    GffValue::Float(value.to_bits())
}

fn said(text: &str) -> GffValue {
    GffValue::LocString(LocString {
        strref: u32::MAX,
        strings: if text.is_empty() {
            Vec::new()
        } else {
            vec![(0, text.as_bytes().to_vec())]
        },
    })
}

fn link(index: u32, active: &str, child: bool) -> GffStruct {
    node(vec![
        field("Index", GffValue::Dword(index)),
        field("Active", resref(active)),
        field("IsChild", GffValue::Byte(u8::from(child))),
    ])
}

/// A DLG of one entry offering a silent reply and a spoken one that leads
/// back to it, written as a GFF and read again, then through the store's
/// gathering (`pack::Gathered`).
fn dlg() -> Check {
    let entry = node(vec![
        field("Text", said("Hello there.")),
        field("Speaker", GffValue::String(b"Trask".to_vec())),
        field("Script", resref("e_act")),
        field("Sound", resref("snd1")),
        field("VO_ResRef", resref("vo1")),
        field("Delay", GffValue::Dword(u32::MAX)),
        field("CamHeightOffset", float(0.25)),
        field("TarHeightOffset", float(-0.1)),
        field("NodeUnskippable", GffValue::Byte(1)),
        field("AlienRaceNode", GffValue::Int(23)),
        field("Emotion", GffValue::Int(14)),
        field(
            "RepliesList",
            GffValue::List(vec![link(0, "c_ok", false), link(1, "", false)]),
        ),
    ]);
    let silent = node(vec![
        field("Text", said("")),
        field("EntriesList", GffValue::List(Vec::new())),
    ]);
    let spoken = node(vec![
        field("Text", said("Again.")),
        field("Delay", GffValue::Dword(2)),
        field("EntriesList", GffValue::List(vec![link(0, "", true)])),
    ]);
    let root = node(vec![
        field("EntryList", GffValue::List(vec![entry])),
        field("ReplyList", GffValue::List(vec![silent, spoken])),
        field(
            "StartingList",
            GffValue::List(vec![link(0, "c_start", false)]),
        ),
        field("EndConversation", resref("e_end")),
        field("Skippable", GffValue::Byte(1)),
        field("ConversationType", GffValue::Int(1)),
        field("OldHitCheck", GffValue::Byte(1)),
        field("AlienRaceOwner", GffValue::Int(24)),
    ]);
    let bytes = Gff {
        file_type: *b"DLG ",
        root,
    }
    .write();
    let Ok(read) = Dlg::parse(&bytes) else {
        return check("dlg", false, "did not read".into());
    };
    let entry = &read.entries[0];
    let gathered = crate::pack::Gathered {
        dialogues: BTreeMap::from([("probe".to_string(), read.clone())]),
        ..crate::pack::Gathered::default()
    };
    let packed =
        crate::pack::Gathered::decode(&gathered.encode()).is_ok_and(|back| back == gathered);
    let ok = packed
        && read.entries.len() == 1
        && read.replies.len() == 2
        && read.kind == Kind::Computer
        && read.skippable
        && read.on_end == "e_end"
        && entry.speaker == "trask"
        && (entry.sound.as_str(), entry.vo.as_str()) == ("snd1", "vo1")
        && entry.delay.is_none()
        && (entry.cam_height, entry.target_height) == (0.25, -0.1)
        && (read.replies[0].cam_height, read.replies[0].target_height) == (0.0, 0.0)
        && entry.unskippable
        && !read.replies[0].unskippable
        && (entry.alien_race, entry.emotion, read.alien_race_owner) == (23, 14, 24)
        && read.alien_race(entry) == Some(23)
        && read.alien_race(&read.replies[1]) == Some(24)
        && read.old_hit_check
        && !read.unequip_items
        && !read.unequip_held
        && read.replies[1].delay == Some(2)
        && read.replies[0].is_silent()
        && !read.replies[1].is_silent()
        && read.replies[1].links[0].is_child
        && read.starts[0].active.script == "c_start"
        && read.scripts() == ["c_ok", "c_start", "e_act", "e_end"];
    check(
        "dlg",
        ok,
        format!(
            "{} entries, {} replies, scripts {:?}, heights {:?}, packed {packed}",
            read.entries.len(),
            read.replies.len(),
            read.scripts(),
            (entry.cam_height, entry.target_height),
        ),
    )
}

/// A DLG for the co-op probes (`world::coop_talk`), made here: one entry,
/// "Hello there.", that runs `script` and offers `spoken` replies, the
/// first leading back to it and the rest ending it; with none spoken, one
/// silent reply that leads nowhere (a one-liner: a bark).
pub(crate) fn talking(spoken: usize, script: &str) -> Dlg {
    let replies: Vec<GffStruct> = if spoken == 0 {
        vec![node(vec![
            field("Text", said("")),
            field("EntriesList", GffValue::List(Vec::new())),
        ])]
    } else {
        (0..spoken)
            .map(|n| {
                let (text, back) = if n == 0 {
                    ("Again.", vec![link(0, "", true)])
                } else {
                    ("Goodbye.", Vec::new())
                };
                node(vec![
                    field("Text", said(text)),
                    field("EntriesList", GffValue::List(back)),
                ])
            })
            .collect()
    };
    let offered = (0..replies.len() as u32)
        .map(|n| link(n, "", false))
        .collect();
    let entry = node(vec![
        field("Text", said("Hello there.")),
        field("Script", resref(script)),
        field("RepliesList", GffValue::List(offered)),
    ]);
    let root = node(vec![
        field("EntryList", GffValue::List(vec![entry])),
        field("ReplyList", GffValue::List(replies)),
        field("StartingList", GffValue::List(vec![link(0, "", false)])),
        field("Skippable", GffValue::Byte(1)),
    ]);
    let bytes = Gff {
        file_type: *b"DLG ",
        root,
    }
    .write();
    Dlg::parse(&bytes).expect("the probes' DLG reads back")
}

/// Three mouth shapes over a second: held before the first and after the
/// last, blended between.
fn lip() -> Check {
    let mut bytes = b"LIP V1.0".to_vec();
    bytes.extend(1.0f32.to_le_bytes());
    bytes.extend(3u32.to_le_bytes());
    for (time, shape) in [(0.0f32, 0u8), (0.5, 4), (1.0, 11)] {
        bytes.extend(time.to_le_bytes());
        bytes.push(shape);
    }
    let Ok(lip) = Lip::parse(&bytes) else {
        return check("lip", false, "did not read".into());
    };
    let at = [-1.0, 0.25, 0.5, 2.0].map(|t| lip.shapes_at(t));
    let ok = at == [(0, 0, 0.0), (0, 4, 0.5), (4, 11, 0.0), (11, 11, 0.0)]
        && Lip::parse(b"LIP V2.0").is_err();
    check("lip", ok, format!("{at:?}"))
}

/// A RIFF header: `fmt ` (tag, channels, rate, bits) and a `data` chunk
/// of `len` bytes declared.
fn riff(tag: u16, channels: u16, rate: u32, bits: u16, len: u32) -> Vec<u8> {
    let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    out.extend(16u32.to_le_bytes());
    out.extend(tag.to_le_bytes());
    out.extend(channels.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes());
    out.extend((channels * bits / 8).to_le_bytes());
    out.extend(bits.to_le_bytes());
    out.extend(b"data");
    out.extend(len.to_le_bytes());
    out
}

/// A second of 16-bit mono PCM at 22 050 Hz.
fn wav_pcm() -> Check {
    let mut bytes = riff(1, 1, 22050, 16, 44100);
    bytes.resize(bytes.len() + 44100, 0);
    let wav = Wav::parse(&bytes);
    let ms = wav.as_ref().ok().and_then(Wav::duration_ms);
    check("wav_pcm", ms == Some(1000), format!("{ms:?} ms"))
}

/// KOTOR's voice file: a RIFF declaring 8-bit PCM with an empty `data`,
/// then an ID3 tag and ten MPEG 2 layer III frames (22 050 Hz, 32 kbps,
/// 576 samples each: 261 ms).
fn wav_mp3() -> Check {
    let mut bytes = riff(1, 1, 22050, 8, 0);
    bytes.extend(b"ID3\x03\0\0\0\0\0\0");
    for _ in 0..10 {
        let mut frame = vec![0xff, 0xf3, 0x40, 0xc0];
        frame.resize(72 * 32000 / 22050, 0);
        bytes.extend(frame);
    }
    let wav = Wav::parse(&bytes);
    let coding = wav.as_ref().ok().map(|wav| wav.coding);
    let ms = wav.as_ref().ok().and_then(Wav::duration_ms);
    check(
        "wav_mp3",
        coding == Some(Coding::Mp3) && ms == Some(261),
        format!("{coding:?}, {ms:?} ms"),
    )
}

/// Answer bytes: a place taken or pointed at, a leave and a skip read
/// back, the last places of each; a place past the last is the last; each
/// answer moves the count, which comes round after four.
fn answers() -> Check {
    let mut byte = 0;
    let mut counts = Vec::new();
    for _ in 0..5 {
        byte = answer(byte, Pick::Reply(1));
        counts.push(byte >> 6);
    }
    let back = |pick| pick_of(answer(0, pick)) == Some(pick);
    let ok = pick_of(0).is_none()
        && [
            Pick::Reply(1),
            Pick::Reply(29),
            Pick::Point(0),
            Pick::Point(29),
            Pick::Leave,
            Pick::Skip,
        ]
        .into_iter()
        .all(back)
        && pick_of(answer(0, Pick::Reply(40))) == Some(Pick::Reply(29))
        && answer(0, Pick::Leave) & SKIP == LEAVE
        && answer(0, Pick::Skip) & SKIP == SKIP
        && counts.windows(2).all(|pair| pair[0] != pair[1])
        && counts[4] == counts[0];
    check("answers", ok, format!("counts {counts:?}"))
}

/// A conversation with every field set (a party one, locked, its listener
/// `ObjectId(2)`, filmed from a default angle with locks on its turns).
pub fn conversation() -> Conversation {
    Conversation {
        id: 3,
        dialog: "end_trask01".into(),
        owner: ObjectId(7),
        pc: ObjectId(1),
        asker: ObjectId(2),
        entry: 55,
        speaker: ObjectId(7),
        replies: vec![74, 72],
        shown_ms: 1310,
        until_ms: u64::MAX,
        auto: false,
        paused: true,
        marks: BTreeMap::from([(1, 1), (2, 0)]),
        camera: 17,
        listener: ObjectId(2),
        joined: vec![ObjectId(4), ObjectId(5)],
        party: true,
        locked: true,
        asking: ObjectId(4),
        waits_for: ObjectId(6),
        dealt: ObjectId(5),
        angle: 2,
        cursor: 5,
        step: 3,
        forward: true,
        defaults_seen: true,
        angles: vec![(55, 2), (0x8000 | 74, 1)],
        voice_cursor: 11,
        voices: vec![(55, 3)],
        listener_now: ObjectId(2),
        pair: (ObjectId(7), ObjectId(4)),
        lock_turn: vec![ObjectId(7)],
        lock_head: vec![ObjectId(2), ObjectId(4)],
    }
}

fn told() -> Told {
    Told {
        conversations: vec![conversation()],
        barks: vec![
            Bark {
                who: ObjectId(9),
                text: Said::StrRef(42),
                until_ms: 5000,
            },
            Bark {
                who: ObjectId(7),
                text: Said::Entry {
                    dialog: "end_pop01".into(),
                    entry: 2,
                },
                until_ms: 6000,
            },
        ],
        tokens: BTreeMap::from([(0, "Revan".to_string())]),
    }
}

/// The scripts' state with a conversation, a start asked for, barks,
/// tokens, answers heard, a use's event waiting, both conversation actions
/// queued (one with its asker, the pause with its conversation) and code a
/// conversation's script deferred, saved and read again; the start, the
/// conversation queued and the code carry a trigger's cue (`Cue`).
fn state() -> Check {
    let told = told();
    let cue = crate::script::Cue {
        trigger: ObjectId(9),
        entering: ObjectId(1),
    };
    let mut state = State {
        talk: Talk {
            conversations: told.conversations,
            asked: vec![Start {
                owner: ObjectId(8),
                pc: ObjectId(1),
                asker: ObjectId(2),
                dialog: "end_door01".into(),
                bark: false,
                cue: Some(cue),
            }],
            barks: told.barks,
            tokens: told.tokens,
            next: 4,
            heard: BTreeMap::from([(0, answer(0, Pick::Reply(0))), (2, answer(0, Pick::Skip))]),
            ..Talk::default()
        },
        pending: vec![crate::script::Pending {
            asker: Some(ObjectId(2)),
            ..crate::script::Pending::new("k_def_dialogue01", ObjectId(8))
        }],
        ..State::default()
    };
    let queued = |action| Queued {
        action,
        started_ms: None,
    };
    state.actions.insert(
        7,
        vec![
            queued(Action::Converse {
                with: ObjectId(1),
                dialog: String::new(),
                asker: Some(ObjectId(2)),
                cue: Some(cue),
            }),
            queued(Action::Hold { talk: Some(3) }),
        ],
    );
    let code = script_ncs::vm::Deferred {
        resume: 16,
        globals: Vec::new(),
        locals: Vec::new(),
    };
    let carried = (None, Some(3), Some(cue));
    state.defer(500, ObjectId(1), "k_def_dialogue01", code, carried);
    let mut out = Writer::default();
    state.encode(&mut out);
    let read = State::decode(&mut Reader::new("state", &out.0));
    check(
        "state",
        read.is_ok_and(|read| read == state),
        format!("{} bytes", out.0.len()),
    )
}

/// What a snapshot carries of conversations, written and read again.
fn snapshot_view() -> Check {
    let told = told();
    let mut out = Writer::default();
    told.encode(&mut out);
    let mut input = Reader::new("told", &out.0);
    let read = Told::decode(&mut input);
    check(
        "snapshot_view",
        read.is_ok_and(|read| read == told) && input.finish().is_ok(),
        format!("{} bytes", out.0.len()),
    )
}

/// More bodies joined than there are seats is refused (`input.invalid`); as
/// many as there are seats reads back.
fn joined_limit() -> Check {
    let decoded = |joined: usize| {
        let mut told = Told {
            conversations: vec![conversation()],
            ..Told::default()
        };
        told.conversations[0].joined = (0..joined as u32).map(|n| ObjectId(10 + n)).collect();
        let mut out = Writer::default();
        told.encode(&mut out);
        Told::decode(&mut Reader::new("told", &out.0)).is_ok()
    };
    let seats = usize::from(crate::MAX_SEATS);
    check(
        "joined_limit",
        decoded(seats) && !decoded(seats + 1),
        format!("{seats} seats"),
    )
}

/// Whether a conversation holds the world (`Told::holds_world`): a party
/// one does; one seat whose body is a private one's listener does; of two
/// seats, one in a private one, does not; two seats each in a private one
/// of its own do.
fn holds_world() -> Check {
    let private = |listeners: &[u32]| Told {
        conversations: listeners
            .iter()
            .map(|&listener| Conversation {
                listener: ObjectId(listener),
                joined: Vec::new(),
                party: false,
                ..conversation()
            })
            .collect(),
        ..Told::default()
    };
    let cases = [
        told().holds_world(&[ObjectId(1), ObjectId(3)]),
        private(&[2]).holds_world(&[ObjectId(2)]),
        !private(&[2]).holds_world(&[ObjectId(2), ObjectId(3)]),
        !private(&[2]).holds_world(&[]),
        private(&[2, 3]).holds_world(&[ObjectId(2), ObjectId(3)]),
    ];
    check(
        "holds_world",
        cases.iter().all(|ok| *ok),
        format!(
            "party, one seat held, two seats one held, no seats, two seats both held: {cases:?}"
        ),
    )
}

/// The formats' probes (`dialogue_formats`).
pub fn formats() -> Vec<Check> {
    vec![dlg(), lip(), wav_pcm(), wav_mp3()]
}

/// What conversations keep (`dialogue_state`).
pub fn kept() -> Vec<Check> {
    vec![
        answers(),
        state(),
        snapshot_view(),
        joined_limit(),
        holds_world(),
    ]
}
