//! Beds streamed: a sound kept as its coded bytes and decoded as it plays
//! (`Stream`), as kira plays a sound decoded whole (a static sound: the
//! same frames, resampled, faded and glided alike), so a bed sounds as a
//! `Clip` played whole would, without holding its frames.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use kira::info::Info;
use kira::sound::{Sound, SoundData};
use kira::{Decibels, Frame, Parameter, Tween, Value, interpolate_frame};

use crate::decode::{Coded, Decoder};

/// Pieces (packets, blocks) a device's bed is decoded ahead by: a second
/// or more.
const AHEAD: usize = 64;

/// A sound kept as its file's coded bytes and decoded as it plays: a bed's
/// (music, ambience, a movie's sound), minutes long, which decoded whole
/// would hold tens of megabytes of frames.
#[derive(Clone)]
pub struct Stream(Coded);

impl Stream {
    /// A WAV, MP3 or FLAC file (`Clip::decode`'s).
    pub fn file(bytes: &[u8]) -> Result<Stream, String> {
        Stream::checked(Coded::file(bytes))
    }

    /// A KOTOR WAV (`Clip::kotor`'s).
    pub fn kotor(bytes: &[u8]) -> Result<Stream, String> {
        Stream::checked(Coded::kotor(bytes)?)
    }

    /// Its first frames decoded, so one that does not read is told now,
    /// not when it is to play.
    fn checked(coded: Coded) -> Result<Stream, String> {
        Decoder::open(&coded)?.next()?.ok_or("no sound")?;
        Ok(Stream(coded))
    }

    /// The bytes it keeps.
    pub fn bytes(&self) -> usize {
        self.0.len()
    }
}

/// A stream from its start, again from its start at its end if it loops;
/// a piece that does not decode ends it (or that time round).
struct Reader {
    coded: Coded,
    decoder: Decoder,
    looping: bool,
    /// Nothing decoded since it began again: a loop of nothing ends.
    fresh: bool,
}

impl Reader {
    fn next(&mut self) -> Option<Vec<Frame>> {
        loop {
            match self.decoder.next() {
                Ok(Some(frames)) => {
                    self.fresh = false;
                    return Some(frames);
                }
                _ if self.looping && !self.fresh => {
                    self.decoder = Decoder::open(&self.coded).ok()?;
                    self.fresh = true;
                }
                _ => return None,
            }
        }
    }
}

/// Where a playing bed's frames come from.
///
/// The capture (`Output::Capture`) renders on the game's clock, driven by
/// `Mixer::advance` as fast as the game runs, not by a device: a decoder
/// on a thread of its own would race it, and whatever it fell behind by
/// would be heard as silence, differently every run. So the capture
/// decodes in the render call itself, each frame when it is wanted: what
/// it writes is a function of the clock alone, whole, and the very frames
/// a clip decoded whole would have played (`Clip` decodes with the same
/// `Decoder`). A device's renderer runs on its real-time thread, which
/// must not wait on a decoder: there a thread decodes `AHEAD` pieces
/// ahead, and should it ever fall behind, the bed waits silent for it
/// (as kira's own streaming sounds do).
enum Feed {
    Inline(Reader),
    Ahead(Receiver<Vec<Frame>>),
}

impl Feed {
    fn ahead(mut reader: Reader) -> Feed {
        let (send, frames) = mpsc::sync_channel(AHEAD);
        // Not started: the bed ends at once (its sender is gone).
        let _ = std::thread::Builder::new()
            .name("bed".into())
            .spawn(move || {
                while let Some(more) = reader.next() {
                    if send.send(more).is_err() {
                        break;
                    }
                }
            });
        Feed::Ahead(frames)
    }
}

enum Command {
    Volume(Decibels, Tween),
    Stop(Tween),
}

/// A bed to play (`Mixer::bed`): from its start, at `volume`, faded in
/// over `fade_in`, looping or played to its end.
pub(crate) struct BedData {
    pub stream: Stream,
    pub volume: Decibels,
    pub fade_in: Tween,
    pub looping: bool,
    /// Decoded in the render call (the capture) rather than ahead.
    pub inline: bool,
}

/// A bed playing: its volume glided, stopped with a fade, and whether it
/// stopped (faded out, or played to its end).
pub(crate) struct BedHandle {
    commands: Sender<Command>,
    stopped: Arc<AtomicBool>,
}

impl BedHandle {
    pub fn set_volume(&self, volume: Decibels, tween: Tween) {
        let _ = self.commands.send(Command::Volume(volume, tween));
    }

    pub fn stop(&self, tween: Tween) {
        let _ = self.commands.send(Command::Stop(tween));
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

impl SoundData for BedData {
    type Error = String;
    type Handle = BedHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, BedHandle), String> {
        let coded = self.stream.0;
        let decoder = Decoder::open(&coded)?;
        let rate = decoder.rate();
        let reader = Reader {
            coded,
            decoder,
            looping: self.looping,
            fresh: true,
        };
        let feed = if self.inline {
            Feed::Inline(reader)
        } else {
            Feed::ahead(reader)
        };
        let (commands, received) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        // Faded in from silence, as kira fades a sound in.
        let mut fade = Parameter::new(Value::Fixed(Decibels::SILENCE), Decibels::SILENCE);
        fade.set(Value::Fixed(Decibels::IDENTITY), self.fade_in);
        let sound = BedSound {
            feed,
            rate,
            buffered: VecDeque::new(),
            ended: false,
            primed: false,
            recent: [Frame::ZERO; 4],
            until_empty: 0,
            fraction: 0.0,
            volume: Parameter::new(Value::Fixed(self.volume), Decibels::IDENTITY),
            fade,
            stopping: false,
            commands: received,
            stopped: stopped.clone(),
        };
        Ok((Box::new(sound), BedHandle { commands, stopped }))
    }
}

/// A bed as kira's renderer plays it: a static sound's playing, its
/// frames taken from the feed rather than from memory.
struct BedSound {
    feed: Feed,
    /// The stream's frames a second.
    rate: u32,
    buffered: VecDeque<Frame>,
    /// The feed has no more.
    ended: bool,
    /// The first three frames are in `recent`.
    primed: bool,
    /// The last four frames taken, interpolated between, and how many more
    /// steps until they are all past the end.
    recent: [Frame; 4],
    until_empty: usize,
    /// How far between `recent[1]` and `recent[2]` the next frame is.
    fraction: f64,
    volume: Parameter<Decibels>,
    fade: Parameter<Decibels>,
    /// Fading out, to stop at the fade's end.
    stopping: bool,
    commands: Receiver<Command>,
    stopped: Arc<AtomicBool>,
}

impl BedSound {
    fn stop_now(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// At least `wanted` frames buffered, or every one left; `false` when
    /// the thread decoding ahead has not got them yet.
    fn fill(&mut self, wanted: usize) -> bool {
        while self.buffered.len() < wanted && !self.ended {
            let more = match &mut self.feed {
                Feed::Inline(reader) => reader.next(),
                Feed::Ahead(frames) => match frames.try_recv() {
                    Ok(more) => Some(more),
                    Err(TryRecvError::Empty) => return false,
                    Err(TryRecvError::Disconnected) => None,
                },
            };
            match more {
                Some(more) => self.buffered.extend(more),
                None => self.ended = true,
            }
        }
        true
    }

    /// The next frame into `recent` (silence past the end); four past the
    /// last, it has played to its end.
    fn step(&mut self) {
        let next = self.buffered.pop_front();
        self.until_empty = match next {
            Some(_) => 4,
            None => self.until_empty.saturating_sub(1),
        };
        self.recent.copy_within(1.., 0);
        self.recent[3] = next.unwrap_or(Frame::ZERO);
        if self.until_empty == 0 {
            self.stop_now();
        }
    }
}

impl Sound for BedSound {
    fn on_start_processing(&mut self) {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                Command::Volume(volume, tween) => self.volume.set(Value::Fixed(volume), tween),
                Command::Stop(tween) if !self.is_stopped() => {
                    self.stopping = true;
                    self.fade.set(Value::Fixed(Decibels::SILENCE), tween);
                }
                Command::Stop(_) => {}
            }
        }
    }

    fn process(&mut self, out: &mut [Frame], dt: f64, info: &Info) {
        let span = dt * out.len() as f64;
        self.volume.update(span, info);
        if self.fade.update(span, info) && self.stopping {
            self.stop_now();
        }
        // Every frame this block can step past, and the first three.
        let wanted = (f64::from(self.rate) * span).ceil() as usize + 4;
        if self.is_stopped() || !self.fill(wanted) {
            out.fill(Frame::ZERO);
            return;
        }
        if !self.primed {
            self.primed = true;
            for _ in 0..3 {
                self.step();
            }
        }
        let frames = out.len();
        for (i, frame) in out.iter_mut().enumerate() {
            let along = (i + 1) as f64 / frames as f64;
            let volume = self.volume.interpolated_value(along).as_amplitude();
            let fade = self.fade.interpolated_value(along).as_amplitude();
            let [previous, current, next, after] = self.recent;
            let resampled = interpolate_frame(previous, current, next, after, self.fraction as f32);
            self.fraction += f64::from(self.rate) * dt;
            while self.fraction >= 1.0 {
                self.fraction -= 1.0;
                self.step();
            }
            *frame = resampled * fade * volume;
        }
    }

    fn finished(&self) -> bool {
        self.is_stopped()
    }
}
