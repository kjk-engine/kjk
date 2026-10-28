//! An animation's keyframed sounds (`animevents.cfg`), and which a part
//! passes from one client frame to the next, as JA's client plays them
//! (`docs/SOUND.md`, "Keyframed").
//!
//! The file names, per part (`UPPEREVENTS`: the torso, `LOWEREVENTS`: the
//! legs), an animation, a frame in it, and a sound: one name, or a name
//! with `%d` and a range of up to four numbers to pick from, played that
//! often in a hundred (0: always). Saber whooshes (`saberhup…`) become
//! swings of a weight (the first number: below 4 fast, below 7 medium,
//! else strong) and `saberspin…` names spins. Names starting with `*` (a
//! character's own voice) play nothing, as in the game. Footsteps,
//! effects and the other kinds are left out.

use fmt_ja::animation_cfg::Sequence;

use crate::fx::Rng;

/// A sound's slot on its character (the game's channels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Auto,
    Weapon,
    Body,
    Voice,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Play {
    /// `name`, or with `%d` one of `range`'s numbers in it.
    Sound {
        name: String,
        range: Option<(i32, i32)>,
    },
    /// A saber whoosh: 0 fast, 1 medium, 2 strong.
    Swing(u8),
    /// A saber spin (`saberspin…`): the name, `%d` for one of 1 to 3.
    Spin(String),
}

/// One keyframed sound.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyed {
    /// The skeleton's frame (the animation's first plus the file's).
    pub frame: i32,
    pub channel: Channel,
    pub play: Play,
    /// In a hundred; 0 always.
    pub chance: i32,
    /// The kind as written (`AEV_SOUND`, `AEV_SOUNDCHAN`): a later line of
    /// the same kind on the same frame takes its place.
    kind: String,
}

/// Every keyframed sound, by part.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Events {
    pub torso: Vec<Keyed>,
    pub legs: Vec<Keyed>,
}

/// The most numbers a sound's range picks from.
const MOST_PICKS: i32 = 4;
const SWING: &str = "sound/weapons/saber/saberhup";
const SPIN: &str = "sound/weapons/saber/saberspin";

/// Reads `text`; `sequence` finds an animation by name (upper case).
pub fn parse<'s>(text: &str, sequence: impl Fn(&str) -> Option<&'s Sequence>) -> Events {
    let mut events = Events::default();
    let mut torso = None;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("");
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [] | ["{"] | ["}"] => continue,
            [section] if section.eq_ignore_ascii_case("UPPEREVENTS") => torso = Some(true),
            [section] if section.eq_ignore_ascii_case("LOWEREVENTS") => torso = Some(false),
            [anim, kind, frame, rest @ ..] => {
                let (Some(torso), Some(sequence)) = (torso, sequence(&anim.to_ascii_uppercase()))
                else {
                    continue;
                };
                let (Ok(offset), true) = (frame.parse::<i32>(), sequence.frame_count > 0) else {
                    continue;
                };
                let Some(keyed) = keyed(kind, sequence.first_frame + offset, rest) else {
                    continue;
                };
                let list = if torso {
                    &mut events.torso
                } else {
                    &mut events.legs
                };
                match list
                    .iter_mut()
                    .find(|k| k.frame == keyed.frame && k.kind == keyed.kind)
                {
                    Some(same) => *same = keyed,
                    None => list.push(keyed),
                }
            }
            _ => continue,
        }
    }
    events
}

fn keyed(kind: &str, frame: i32, rest: &[&str]) -> Option<Keyed> {
    let (channel, rest) = match kind.to_ascii_uppercase().as_str() {
        "AEV_SOUND" => (Channel::Auto, rest),
        "AEV_SOUNDCHAN" => {
            let (name, rest) = rest.split_first()?;
            let channel = match name.to_ascii_uppercase().as_str() {
                "CHAN_WEAPON" => Channel::Weapon,
                "CHAN_BODY" => Channel::Body,
                "CHAN_VOICE" | "CHAN_VOICE_ATTEN" | "CHAN_VOICE_GLOBAL" => Channel::Voice,
                _ => Channel::Auto,
            };
            (channel, rest)
        }
        _ => return None,
    };
    let [name, low, high, chance, ..] = rest else {
        return None;
    };
    let name = name.to_ascii_lowercase();
    if name.starts_with('*') {
        return None;
    }
    let (low, high) = (low.parse::<i32>().ok()?, high.parse::<i32>().ok()?);
    let play = if name.starts_with(SWING) {
        Play::Swing(match low {
            ..4 => 0,
            4..7 => 1,
            _ => 2,
        })
    } else if let Some(after) = name.strip_prefix(SPIN) {
        Play::Spin(match after.chars().next() {
            Some('o') => format!("{SPIN}off.wav"),
            Some(n @ '1'..='3') => format!("{SPIN}{n}.wav"),
            Some('%') => format!("{SPIN}%d.wav"),
            _ => format!("{SPIN}.wav"),
        })
    } else {
        Play::Sound {
            range: (low != 0 && high != 0).then(|| (low, high.min(low + MOST_PICKS - 1))),
            name,
        }
    };
    // Whooshes and spins play in no slot of their own.
    let channel = match play {
        Play::Sound { .. } => channel,
        Play::Swing(_) | Play::Spin(_) => Channel::Auto,
    };
    Some(Keyed {
        frame,
        channel,
        play,
        chance: chance.parse().ok()?,
        kind: kind.to_ascii_uppercase(),
    })
}

/// What a part played over a step: its whole frame before and now, and
/// whether it stayed in one animation (that animation, then).
#[derive(Debug, Clone, Copy)]
pub struct Passage<'s> {
    pub before: i32,
    pub now: i32,
    pub same: Option<&'s Sequence>,
}

impl Passage<'_> {
    /// Whether the part passed the keyed `frame` over the step: it landed
    /// on it, or, staying in one animation and moving more than a frame,
    /// crossed it running forward or backward, or wrapped round a loop
    /// past it. A step that never came within three frames of it does not
    /// count.
    pub fn passes(&self, frame: i32) -> bool {
        let (before, now) = (self.before, self.now);
        if frame == now {
            return true;
        }
        let Some(sequence) = self.same else {
            return false;
        };
        let far = |at: i32| (at - frame).abs() > 3;
        if (before - now).abs() <= 1 || (far(before) && far(now)) {
            return false;
        }
        let first = sequence.first_frame;
        let looped =
            sequence.loop_frame != -1 && (first..first + sequence.frame_count).contains(&frame);
        if sequence.frames_per_second < 0 {
            (before > frame && now < frame) || (looped && before > frame && now > before)
        } else {
            (before < frame && now > frame) || (looped && before < frame && now < before)
        }
    }
}

impl Keyed {
    /// What it plays this time, if anything: it plays `chance` times in a
    /// hundred (always at 0), and a range, a swing or a spin picks one of
    /// its numbers.
    pub fn sound(&self, rng: &mut Rng) -> Option<String> {
        if self.chance != 0 && self.chance <= rng.roll_between(0, 99) {
            return None;
        }
        Some(match &self.play {
            Play::Sound { name, range: None } => name.clone(),
            Play::Sound {
                name,
                range: Some((low, high)),
            } => numbered(name, rng.roll_between(*low, *high)),
            Play::Swing(weight) => {
                let low = i32::from(*weight) * 3 + 1;
                format!("{SWING}{}.wav", rng.roll_between(low, low + 2))
            }
            Play::Spin(name) => numbered(name, rng.roll_between(1, 3)),
        })
    }

    /// Whether the sound stays where it began rather than following the
    /// character (the saber whooshes).
    pub fn stays(&self) -> bool {
        matches!(self.play, Play::Swing(_))
    }

    /// Every name it may play.
    pub fn names(&self) -> Vec<String> {
        match &self.play {
            Play::Sound { name, range: None } => vec![name.clone()],
            Play::Sound {
                name,
                range: Some((low, high)),
            } => (*low..=*high).map(|n| numbered(name, n)).collect(),
            Play::Swing(weight) => {
                let low = i32::from(*weight) * 3 + 1;
                (low..=low + 2).map(|n| format!("{SWING}{n}.wav")).collect()
            }
            Play::Spin(name) => (1..=3).map(|n| numbered(name, n)).collect(),
        }
    }
}

fn numbered(name: &str, n: i32) -> String {
    name.replacen("%d", &n.to_string(), 1)
        .replacen("%i", &n.to_string(), 1)
}
