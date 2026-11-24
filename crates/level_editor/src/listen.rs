//! What the editor plays for its user to hear before they play the level:
//! the area's music and ambient sound, and a sound object's sounds. One
//! thing plays at a time. Each file plays to its end as a bed of the game's
//! mixer (`kjk_game::sound::Beds`), heard alike in both ears, at the level
//! the game plays it at (`kjk_game::ambience::preview`).

use std::time::{Duration, Instant};

use bevy::prelude::*;
use kjk_game::ambience::{Hear, Preview};
use kjk_game::sound::{Beds, Sounding};

use crate::state::EditorState;

/// What the editor plays now, if anything.
#[derive(Default)]
pub(crate) struct Listening {
    playing: Option<Playing>,
    /// Counts the files begun, so each one's bed has a name of its own and
    /// a file heard again begins anew.
    begun: u64,
}

struct Playing {
    /// What the user asked to hear, as the panels name it (`Listening::is`).
    what: String,
    /// How the editor names it to the user.
    shown: String,
    preview: Preview,
    /// The file playing, or the one to play after the pause.
    at: usize,
    /// Its bed's name.
    name: String,
    /// The mixer has played it.
    heard: bool,
    /// The pause before the next file ends then.
    waits_until: Option<Instant>,
}

impl Listening {
    /// Whether `what` is playing.
    pub(crate) fn is(&self, what: &str) -> bool {
        self.playing
            .as_ref()
            .is_some_and(|playing| playing.what == what)
    }

    /// What is playing, as the panels name it.
    pub(crate) fn what(&self) -> Option<&str> {
        self.playing.as_ref().map(|playing| playing.what.as_str())
    }

    /// What is playing, as the editor names it to the user.
    pub(crate) fn shown(&self) -> Option<&str> {
        self.playing.as_ref().map(|playing| playing.shown.as_str())
    }

    pub(crate) fn stop(&mut self) {
        self.playing = None;
    }

    /// Plays `heard` in place of anything playing, read from `resolver`.
    /// Returns the files that do not read, or why nothing plays (a phrase
    /// that follows the name of what was asked for).
    pub(crate) fn play(
        &mut self,
        (what, shown): (String, &str),
        heard: &Hear,
        resolver: &vfs::resolver::Resolver,
    ) -> Result<Vec<String>, String> {
        self.playing = None;
        let preview = kjk_game::ambience::preview(resolver, heard);
        if preview.files.is_empty() {
            return Err(match preview.not_read.is_empty() {
                true => "has no sound to play".to_string(),
                false => format!(
                    "has no sound file that reads: {}",
                    preview.not_read.join(", ")
                ),
            });
        }
        let not_read = preview.not_read.clone();
        self.begun += 1;
        self.playing = Some(Playing {
            what,
            shown: shown.to_string(),
            preview,
            at: 0,
            name: format!("editor preview {}", self.begun),
            heard: false,
            waits_until: None,
        });
        Ok(not_read)
    }

    /// The seconds left before the next file, while the preview waits
    /// between two.
    pub(crate) fn waits(&self) -> Option<u64> {
        let until = self.playing.as_ref()?.waits_until?;
        Some(until.saturating_duration_since(Instant::now()).as_secs() + 1)
    }

    /// The file playing now, if one is.
    pub(crate) fn file(&self) -> Option<&str> {
        let playing = self.playing.as_ref()?;
        if playing.waits_until.is_some() {
            return None;
        }
        Some(playing.preview.files.get(playing.at)?.0.as_str())
    }
}

/// Keeps the file playing held in the mixer. When it has played to its
/// end, the next one starts after the pause, and after the last the
/// preview starts again or stops.
pub(crate) fn hold(
    mut state: ResMut<EditorState>,
    beds: Option<ResMut<Beds>>,
    sounding: Option<Res<Sounding>>,
) {
    let (Some(mut beds), Some(sounding)) = (beds, sounding) else {
        return;
    };
    let listening = &mut state.listening;
    let Some(playing) = &mut listening.playing else {
        return;
    };
    let now = Instant::now();
    if let Some(until) = playing.waits_until {
        if now < until {
            return;
        }
        playing.waits_until = None;
        listening.begun += 1;
        playing.name = format!("editor preview {}", listening.begun);
        playing.heard = false;
    }
    let sounds = sounding.0.contains(&playing.name);
    if sounds {
        playing.heard = true;
    } else if playing.heard {
        // Played to its end: the next file, after the pause.
        let next = playing.at + 1;
        if next < playing.preview.files.len() {
            playing.at = next;
        } else if playing.preview.again {
            playing.at = 0;
        } else {
            listening.playing = None;
            return;
        }
        playing.waits_until =
            Some(now + Duration::from_millis(u64::from(playing.preview.pause_ms)));
        return;
    }
    let (_, stream) = &playing.preview.files[playing.at];
    beds.0
        .push((playing.name.clone(), stream.clone(), playing.preview.bed));
}
