//! Scripted player input (`--moves`): what the keys and the mouse do over
//! time, as a list of segments, each held for a while:
//!
//! ```text
//! 500ms; 2s w; 1s w turn 90; 0.4s w jump; 1s w crouch; 2s s walk; 1s yaw 0 d
//! ```
//!
//! A segment is a duration, then any of `w a s d` (the stick), `jump`,
//! `crouch`, `walk`, `use` (open a door; held, revive a downed party
//! member ahead: `USE`), `attack` (hold the trigger: fire,
//! or swing), `alt` (the weapon's other mode), `style` (the next saber
//! style), `grip`, `lightning`, `drain`, `grenade` (held; a grenade is
//! thrown on its release), `medpac`, `shield`, `stim` (an item used on
//! the press), `stealth` (stealth mode on or off, on the press), `mine`
//! (lay the best mine carried, on the press), `force ROW` (the power
//! button with `spells.2da` row ROW selected, or `force pull`), `hand N`
//! (take out hand N's weapon, 0 or 1), `yaw DEG` (look that way from the
//! segment's start; degrees from +X, 90 is +Y), `turn DEG` (turn that far
//! over the segment, as a mouse would), `pitch DEG` (positive looks down),
//! `reply N` (answer a conversation with the Nth reply offered, from 1),
//! `skip` (end the conversation's line). The view, the hand and the last
//! answer carry over from segment to segment. A pure
//! function of the time, so the sim can be stepped to any tick and a run
//! repeats exactly.

use crate::combat::force::PULL;
use crate::script::dialogue::{Pick, answer};
use crate::{Command, buttons, math};

/// What `use` holds: the use bit (open what it faces) and the use held
/// that revives a downed party member ahead (`world::falls`).
pub const USE: u16 = buttons::USE | buttons::USE_HELD;

/// The words that hold a button for the segment.
const BUTTONS: [(&str, u16); 13] = [
    ("use", USE),
    ("attack", buttons::ATTACK),
    ("alt", buttons::ALT_ATTACK),
    ("style", buttons::SABER_STYLE),
    ("grip", buttons::FORCE_GRIP),
    ("lightning", buttons::FORCE_LIGHTNING),
    ("drain", buttons::FORCE_DRAIN),
    ("grenade", buttons::GRENADE),
    ("medpac", buttons::MEDPAC),
    ("shield", buttons::SHIELD),
    ("stim", buttons::STIM),
    ("stealth", buttons::STEALTH),
    ("mine", buttons::LAY_MINE),
];

#[derive(Debug, Clone, PartialEq)]
struct Segment {
    ms: u32,
    forward: i8,
    right: i8,
    up: i8,
    walk: bool,
    /// Buttons held (walking's among them).
    buttons: u16,
    /// The hand out, and the power selected.
    weapon: u8,
    force: u8,
    /// The answer byte held (`script::dialogue::answer`).
    reply: u8,
    /// View at the start (pitch, yaw) and turned by the end.
    start: [f32; 2],
    turn: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Script {
    segments: Vec<Segment>,
    /// The view after the last segment.
    end: [f32; 2],
}

/// Named scripts: the course walk and the measurement runs.
pub fn named(name: &str) -> Option<&'static str> {
    Some(match name {
        // The I13 course (`course::course`), start to end: stairs, a
        // drop, a ramp and a higher drop, a crouch through the tunnel, a
        // roll, two jumps over the gap, the 18-unit step, a look about,
        // strafing and running backward.
        "course" => {
            "600ms yaw 90; 3800ms w; 400ms; 2600ms w crouch; 400ms w; 300ms w crouch;              700ms w; 200ms w; 100ms w jump; 900ms w; 100ms w jump; 2300ms w; 600ms;              1200ms turn 180; 1500ms w; 800ms a; 800ms d; 1200ms s; 600ms s walk; 800ms"
        }
        // I14, from `--stage end_m01aa --arrive wp_player01`: east into
        // the door to the Starboard Section (end_m01ab), turn about there
        // and walk back through its door (our link, data/links), then on
        // down the Command Module's corridor.
        "lifecycle-walk" => {
            "500ms yaw 0; 1500ms w; 500ms; 700ms turn 180; 1500ms w; 500ms; 1500ms w; 500ms"
        }
        // I14, from `--stage end_m01aa` (its entry, the bunk room): east to
        // the room's door, use it (it opens), through into the corridor,
        // on to its far door, use it (locked: Trask's to open).
        "lifecycle-doors" => {
            "300ms yaw 0; 800ms w; 500ms use; 400ms; 1500ms w; 300ms; 600ms w; 500ms use; 500ms"
        }
        _ => return None,
    })
}

fn number(word: Option<&str>, what: &str) -> Result<f32, String> {
    word.and_then(|w| w.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .ok_or(format!("{what} takes a number"))
}

fn duration(word: &str) -> Result<u32, String> {
    let (value, scale) = if let Some(ms) = word.strip_suffix("ms") {
        (ms, 1.0)
    } else if let Some(s) = word.strip_suffix('s') {
        (s, 1000.0)
    } else {
        return Err(format!("{word}: a duration such as 500ms or 2s"));
    };
    value
        .parse::<f32>()
        .ok()
        .filter(|v| *v >= 0.0 && v.is_finite())
        .map(|v| (v * scale).round() as u32)
        .ok_or(format!("{word}: a duration such as 500ms or 2s"))
}

impl Script {
    /// Parses segments separated by `;` or `,`; a named script by its name.
    pub fn parse(text: &str) -> Result<Script, String> {
        let text = named(text.trim()).unwrap_or(text);
        let mut segments = Vec::new();
        let mut view = [0.0f32, 90.0];
        let mut weapon = 0u8;
        let mut reply = 0u8;
        for part in text
            .split([';', ','])
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let mut words = part.split_whitespace();
            let ms = duration(words.next().unwrap_or(""))?;
            let (mut forward, mut right, mut up) = (0i8, 0i8, 0i8);
            let (mut walk, mut held, mut force, mut turn) = (false, 0u16, 0u8, 0.0);
            while let Some(word) = words.next() {
                match word {
                    "w" => forward = 127,
                    "s" => forward = -127,
                    "a" => right = -127,
                    "d" => right = 127,
                    "jump" => up = 127,
                    "crouch" => up = -127,
                    "walk" => {
                        walk = true;
                        held |= buttons::WALKING;
                    }
                    "force" => {
                        held |= buttons::FORCE_POWER;
                        force = match words.next() {
                            Some("pull") => PULL,
                            row => number(row, "force")? as u8,
                        };
                    }
                    "hand" => weapon = number(words.next(), "hand")?.clamp(0.0, 1.0) as u8,
                    "reply" => {
                        let nth = number(words.next(), "reply")?.clamp(1.0, 30.0) as u8;
                        reply = answer(reply, Pick::Reply(nth - 1));
                    }
                    "skip" => reply = answer(reply, Pick::Skip),

                    "yaw" => view[1] = number(words.next(), "yaw")?,
                    "pitch" => view[0] = number(words.next(), "pitch")?,
                    "turn" => turn = number(words.next(), "turn")?,
                    other => {
                        held |= BUTTONS
                            .iter()
                            .find(|(name, _)| *name == other)
                            .map(|(_, bit)| *bit)
                            .ok_or(format!("{part}: {other}: not a key or setting"))?;
                    }
                }
            }
            segments.push(Segment {
                ms,
                forward,
                right,
                up,
                walk,
                buttons: held,
                weapon,
                force,
                reply,
                start: view,
                turn,
            });
            view[1] += turn;
        }
        Ok(Script {
            segments,
            end: view,
        })
    }

    /// How long it lasts, milliseconds.
    pub fn length_ms(&self) -> u64 {
        self.segments.iter().map(|s| u64::from(s.ms)).sum()
    }

    /// The command at `ms` into the script; after its end, standing still
    /// looking where it last looked.
    pub fn command(&self, ms: u64) -> Command {
        let mut from = 0u64;
        for segment in &self.segments {
            let to = from + u64::from(segment.ms);
            if ms < to {
                let done = (ms - from) as f32 / segment.ms.max(1) as f32;
                let yaw = segment.start[1] + segment.turn * done;
                // JA's client sends 64 instead of 127 when walking.
                let slow = |axis: i8| {
                    if segment.walk {
                        axis.signum() * 64
                    } else {
                        axis
                    }
                };
                let (forward, right) = (slow(segment.forward), slow(segment.right));
                return Command {
                    angles: view_angles(segment.start[0], yaw),
                    forward,
                    right,
                    up: segment.up,
                    buttons: segment.buttons,
                    weapon: segment.weapon,
                    force: segment.force,
                    reply: segment.reply,
                    // The client's camera sets it (`Command::aim`).
                    aim: [0; 3],
                };
            }
            from = to;
        }
        Command {
            angles: view_angles(self.end[0], self.end[1]),
            ..Command::default()
        }
    }
}

/// A view as a command's angles (JA shorts).
pub fn view_angles(pitch: f32, yaw: f32) -> [i16; 3] {
    [
        math::degrees_to_short(pitch),
        math::degrees_to_short(yaw),
        0,
    ]
}
