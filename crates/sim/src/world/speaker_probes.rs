//! The probes of who says a line (`kjk sim probes "speakers"`), on the
//! proving ground, no install, as both executables pick and resolve it
//! (`script::dialogue::Step::speaker_for`).
//!
//! An owner and four bodies around it, in the area's order: `far_first`
//! 3 m off then another `far_first` 2 m off (the search's first bar, its
//! plain 3 m, keeps the farther one: Q-0323), `near_late` 3 m off then
//! another `near_late` 1.5 m off (taken: 2.25 is within 3).
//! A start naming a speaker no one answers to is passed over before its
//! condition runs (it would tell the player "asked").
//!
//! A line's replies are chosen once it has played, after its scripts ran
//! (TSL's `CSWSDialog::SendDialogEntry`: the line's Script, Script2, then
//! `CSWSObject::SendDialogReplies` once `IsDialogDelay` is over): the
//! entry's script tells "line" before the first reply's condition tells
//! "asked" (as the conversation starts, kjk asks it once more first, to
//! know a one-liner), and no reply is offered while the line plays.
//!
//! A line with a `Delay` lasts it, its text's reading time not counted
//! (TSL's `CSWSDialog::SetDialogDelay` reads that only for a conversation
//! no player is in): a 48-character developer note with a 1 s delay lasts
//! 1 s; with none, its reading time.

use super::SimWorld;
use super::talk_end_probes::{CATALOG, by_entry, call, id_of, step, told, world_with};
use crate::combat::Sheet;
use crate::input::{Command, Seat, TickInput};
use crate::module::ObjectData;
use crate::probe::{Checks, check};
use crate::script::dialogue::Start;
use crate::script::dialogue_probes::talking;
use fmt_odyssey::dlg::Dlg;
use fmt_odyssey::ncs::{Const, Op, Ty};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;
use std::collections::BTreeMap;

/// The owner, then the bodies by tag, each `(dx, dy)` off the ground's
/// entry; the owner stands at `OWNER_AT`.
const OWNER_AT: (f32, f32) = (1.5, 0.0);
const BODIES: [(&str, (f32, f32)); 4] = [
    ("far_first", (1.5, 3.0)),
    ("far_first", (1.5, -2.0)),
    ("near_late", (4.5, 0.0)),
    ("near_late", (1.5, 1.5)),
];

/// A conditional that tells the player "asked" and holds.
fn asked() -> fmt_odyssey::ncs::Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Rsadd(Ty::Int))
        .to(Op::Jsr, "body")
        .op(Op::Retn)
        .label("body")
        .op(Op::Const(Const::String(b"asked".to_vec())))
        .op(call(1, 0))
        .op(call(0, 2))
        .op(Op::Const(Const::Int(1)))
        .op(Op::CpDownSp {
            offset: -8,
            size: 4,
        })
        .op(Op::MovSp(-4))
        .op(Op::Retn);
    asm.build()
}

/// `SendMessageToPC(GetFirstPC(), "line")`.
fn says_line() -> fmt_odyssey::ncs::Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::String(b"line".to_vec())))
        .op(call(1, 0))
        .op(call(0, 2))
        .op(Op::Retn);
    asm.build()
}

/// A DLG whose start links lead, in order, to entries said by `speakers`,
/// the first link's condition `probe_asked`.
fn naming(speakers: &[&str]) -> Dlg {
    let mut dlg = talking(1, "");
    let (entry, start) = (dlg.entries[0].clone(), dlg.starts[0].clone());
    dlg.entries = speakers
        .iter()
        .map(|speaker| {
            let mut entry = entry.clone();
            entry.speaker = speaker.to_string();
            entry
        })
        .collect();
    dlg.starts = (0..speakers.len() as u32)
        .map(|index| {
            let mut link = start.clone();
            link.index = index;
            if index == 0 {
                link.active.script = "probe_asked".into();
            }
            link
        })
        .collect();
    dlg
}

/// The minefield vision's unvoiced note, 48 characters, with `delay`.
const NOTE: &str = "{Mandalorians rush in the room ready for battle}";

fn noted(delay: Option<u32>) -> Dlg {
    let mut dlg = talking(2, "");
    let entry = &mut dlg.entries[0];
    entry.text.strings = vec![(0, NOTE.as_bytes().to_vec())];
    entry.delay = delay;
    dlg
}

fn library() -> crate::script::Library {
    let dialogues: BTreeMap<String, Dlg> = [
        ("probe_absent", naming(&["absent", ""])),
        ("probe_first_bar", naming(&["far_first"])),
        ("probe_nearest", naming(&["NEAR_LATE"])),
        ("probe_delayed", noted(Some(1))),
        ("probe_undelayed", noted(None)),
        ("probe_script_first", {
            let mut dlg = talking(2, "probe_line");
            dlg.entries[0].links[0].active.script = "probe_asked".into();
            dlg
        }),
    ]
    .map(|(name, dlg)| (name.to_string(), dlg))
    .into();
    crate::probe::library(
        CATALOG,
        [("probe_asked", asked()), ("probe_line", says_line())],
    )
    .with_dialogues(dialogues, BTreeMap::new())
}

/// The world with the owner and the bodies placed, `dialog` asked to
/// start by the owner with the player: the world, the placed ids (the
/// owner first) and the next tick.
fn started(dialog: &str) -> Result<(SimWorld, Vec<u32>, u64), String> {
    let at = |(dx, dy): (f32, f32)| by_entry(dx, dy);
    let placed = std::iter::once(("owner", OWNER_AT))
        .chain(BODIES)
        .map(|(tag, off)| ObjectData::creature(tag, tag, at(off), Sheet::commoner()))
        .collect();
    let (mut world, first) = world_with(placed, None, library())?;
    let ids: Vec<u32> = (first..first + 1 + BODIES.len() as u32)
        .map(|index| id_of(&mut world, index))
        .collect::<Result<_, _>>()?;
    let player = step(&mut world, 1, &TickInput::default())
        .body(Seat(0))
        .map(|entity| entity.id.0)
        .ok_or("no player")?;
    let mut state = world.script_state().clone();
    state.talk.asked.push(Start {
        owner: ObjectId(ids[0]),
        pc: ObjectId(player),
        asker: ObjectId(player),
        dialog: dialog.into(),
        bark: false,
        cue: None,
    });
    world.set_script_state(state);
    Ok((world, ids, 2))
}

/// A seat's still command (its first answer byte only starts its count).
fn still() -> TickInput {
    let mut still = TickInput::default();
    still.insert(Seat(0), Command::default());
    still
}

/// `dialog` started by the owner with the player: its first line's entry,
/// who says it (by tag, `owner` for the owner) and what the player was
/// told.
fn first_line(dialog: &str) -> Result<(u32, String, Vec<String>), String> {
    let (mut world, ids, from) = started(dialog)?;
    let still = still();
    let mut said = Vec::new();
    for tick in from..from + 4 {
        let snapshot = step(&mut world, tick, &still);
        told(&mut world, &mut said);
        if let Some(talk) = snapshot.talk.conversations.first() {
            let who = match ids.iter().position(|id| *id == talk.speaker.0) {
                Some(0) => "owner".to_string(),
                Some(n) => format!("{} {}", BODIES[n - 1].0, n - 1),
                None => format!("#{}", talk.speaker.0),
            };
            return Ok((talk.entry, who, said));
        }
    }
    Err(format!("{dialog} did not start"))
}

/// How long `dialog`'s first line lasts (ms).
fn first_line_ms(dialog: &str) -> Result<u64, String> {
    let (mut world, _, from) = started(dialog)?;
    let still = still();
    for tick in from..from + 4 {
        let snapshot = step(&mut world, tick, &still);
        if let Some(talk) = snapshot.talk.conversations.first() {
            return Ok(talk.until_ms - talk.shown_ms);
        }
    }
    Err(format!("{dialog} did not start"))
}

/// `probe_script_first` played until its replies are up: what the player
/// was told, in order, and whether any reply was offered while the line
/// played.
fn replies_after_line() -> Result<(Vec<String>, bool), String> {
    let (mut world, _, from) = started("probe_script_first")?;
    let still = still();
    let (mut said, mut early) = (Vec::new(), false);
    for tick in from..from + 1000 {
        let snapshot = step(&mut world, tick, &still);
        told(&mut world, &mut said);
        if let Some(talk) = snapshot.talk.conversations.first() {
            if talk.until_ms != u64::MAX {
                early |= !talk.replies.is_empty();
            } else if !talk.replies.is_empty() {
                return Ok((said, early));
            }
        }
    }
    Err(format!("no replies offered; told {said:?}"))
}

pub fn probes() -> Checks {
    // The first start's condition ran (told "asked") where its speaker is
    // here, never where it is not.
    let line = |dialog: &str, want: (u32, &str, bool)| match first_line(dialog) {
        Ok((entry, who, said)) => check(
            (entry, who.as_str(), !said.is_empty()) == want,
            format!("E{entry} said by {who}, the player told {said:?}"),
        ),
        Err(why) => Err(why),
    };
    vec![
        (
            "speaker_not_here_passed_over_before_its_condition",
            line("probe_absent", (1, "owner", false)),
        ),
        (
            "speaker_search_keeps_its_first_bar",
            line("probe_first_bar", (0, "far_first 0", true)),
        ),
        (
            "speaker_nearest_the_owner_any_case",
            line("probe_nearest", (0, "near_late 3", true)),
        ),
        (
            "replies_chosen_after_the_lines_scripts",
            replies_after_line().and_then(|(said, early)| {
                check(
                    said.ends_with(&["line".to_string(), "asked".to_string()]) && !early,
                    format!("told {said:?}, a reply offered while the line played: {early}"),
                )
            }),
        ),
        (
            "delayed_line_lasts_its_delay",
            first_line_ms("probe_delayed").and_then(|delayed| {
                let read = first_line_ms("probe_undelayed")?;
                check(
                    delayed == 1000 && read == 5400,
                    format!("delayed {delayed} ms, undelayed {read} ms"),
                )
            }),
        ),
    ]
}
