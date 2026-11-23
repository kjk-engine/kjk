//! A Bink movie played as it is decoded. A thread of its own decodes the
//! frames in order, a few ahead of the one shown, and turns each into RGBA.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use bink::{Bink, Sound, Video};

/// How many decoded frames may wait for the screen.
const AHEAD: usize = 3;

/// A movie being decoded for the screen.
pub struct Player {
    pub width: u32,
    pub height: u32,
    rate: (u32, u32),
    frames: usize,
    /// Locked only so that a player can be shared, as a resource is.
    decoded: Mutex<Receiver<Result<(usize, Vec<u8>), String>>>,
    /// A frame decoded but not yet due.
    waiting: Option<(usize, Vec<u8>)>,
}

impl Player {
    /// Starts decoding `bink`'s pictures. Its sound is decoded separately,
    /// with `sound`.
    pub fn start(bink: Bink) -> Player {
        let (send, decoded) = mpsc::sync_channel(AHEAD);
        let (width, height, rate, frames) = (bink.width, bink.height, bink.rate, bink.frames());
        std::thread::spawn(move || {
            let mut video = Video::new(&bink);
            for frame in 0..bink.frames() {
                let picture = bink
                    .packets(frame)
                    .and_then(|packets| video.decode(packets.video))
                    .map(|()| {
                        let mut rgba = Vec::new();
                        video.rgba(&mut rgba);
                        (frame, rgba)
                    })
                    .map_err(|why| format!("frame {frame}: {why}"));
                let failed = picture.is_err();
                // The screen has stopped the movie.
                if send.send(picture).is_err() || failed {
                    return;
                }
            }
        });
        Player {
            width,
            height,
            rate,
            frames,
            decoded: Mutex::new(decoded),
            waiting: None,
        }
    }

    /// How long the movie runs, in seconds.
    pub fn seconds(&self) -> f64 {
        self.frames as f64 * f64::from(self.rate.1) / f64::from(self.rate.0)
    }

    /// How many frames the movie has.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The frame shown `seconds` into the movie.
    pub fn frame_at(&self, seconds: f64) -> usize {
        let frame = (seconds * f64::from(self.rate.0) / f64::from(self.rate.1)).floor();
        (frame.max(0.0) as usize).min(self.frames.saturating_sub(1))
    }

    /// The newest picture decoded at or before frame `due`, as RGBA, if one
    /// came since the last call. `wait` waits for frame `due` itself, which a
    /// recording does so that its frames do not depend on the decoder's
    /// speed. An error is a frame that did not decode.
    pub fn picture(&mut self, due: usize, wait: bool) -> Result<Option<(usize, Vec<u8>)>, String> {
        let decoded = self.decoded.get_mut().expect("the decoder's channel");
        let mut newest = None;
        loop {
            if self.waiting.is_none() {
                let next = if wait && newest.as_ref().is_none_or(|(frame, _)| *frame < due) {
                    decoded.recv().ok()
                } else {
                    match decoded.try_recv() {
                        Ok(next) => Some(next),
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
                    }
                };
                self.waiting = next.transpose()?;
            }
            match self.waiting.take() {
                Some(next) if next.0 <= due => newest = Some(next),
                later => {
                    self.waiting = later;
                    return Ok(newest);
                }
            }
        }
    }
}

/// `bink`'s first audio track as a WAV file, if it has one.
pub fn sound(bink: &Bink) -> Result<Option<Vec<u8>>, String> {
    if bink.tracks.is_empty() {
        return Ok(None);
    }
    Ok(Some(Sound::of(bink, 0)?.wav()))
}
