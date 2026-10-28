//! The flight recorder on disk: the live host's ticks streamed into the
//! run's folder as `flight-<n>.kjkf` (`sim::flight`), a segment from each
//! module's entry, the current one and the one before kept. A thread of its
//! own writes them and flushes each chunk, so a frame never waits on the
//! disk and a crash or a kill loses at most the last second.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use sim::flight::FlightWriter;
use sim::{LevelId, ScriptVmState, Snapshot, TickInput};

/// What the writer's thread is told.
enum Message {
    /// A new segment at this path, starting with these bytes.
    Segment(PathBuf, Vec<u8>),
    /// More of the current segment.
    Append(Vec<u8>),
}

/// The live host's flight recorder.
pub(crate) struct Flight {
    tx: Option<Sender<Message>>,
    thread: Option<JoinHandle<()>>,
    writer: Option<FlightWriter>,
    folder: PathBuf,
    segment: u32,
    hz: u32,
}

/// Segment `n`'s file in `folder`.
pub fn segment_path(folder: &Path, n: u32) -> PathBuf {
    folder.join(format!("flight-{n}.kjkf"))
}

/// The writer's thread: each message to the disk, flushed.
fn write_all(rx: Receiver<Message>) {
    let mut file: Option<std::fs::File> = None;
    for message in rx {
        let written = match message {
            Message::Segment(path, bytes) => std::fs::File::create(&path).and_then(|mut new| {
                new.write_all(&bytes)?;
                new.flush()?;
                file = Some(new);
                Ok(())
            }),
            Message::Append(bytes) => match file.as_mut() {
                Some(file) => file.write_all(&bytes).and_then(|()| file.flush()),
                None => Ok(()),
            },
        };
        if let Err(error) = written {
            bevy::log::warn!(target: "flight", "the flight recorder stops: {error}");
            file = None;
        }
    }
}

impl Flight {
    /// The recorder writing into `folder`, its first segment starting from
    /// `start` in `level`.
    pub(crate) fn new(
        folder: &Path,
        hz: u32,
        start: &Snapshot,
        level: &LevelId,
        script: &ScriptVmState,
    ) -> Flight {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("kjk-flight".into())
            .spawn(move || write_all(rx))
            .inspect_err(|error| bevy::log::warn!(target: "flight", "no flight recorder: {error}"))
            .ok();
        let mut flight = Flight {
            tx: Some(tx),
            thread,
            writer: None,
            folder: folder.to_path_buf(),
            segment: 0,
            hz,
        };
        flight.start(start, level, script);
        flight
    }

    fn send(&self, message: Message) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(message);
        }
    }

    /// A new segment from `start` in `level` (a module entered), the one
    /// before the last removed.
    pub(crate) fn start(&mut self, start: &Snapshot, level: &LevelId, script: &ScriptVmState) {
        self.segment += 1;
        let (writer, bytes) = FlightWriter::new(self.hz, start, level, script, env!("KJK_GIT_SHA"));
        self.writer = Some(writer);
        self.send(Message::Segment(
            segment_path(&self.folder, self.segment),
            bytes,
        ));
        if let Some(old) = self.segment.checked_sub(2).filter(|old| *old > 0) {
            let _ = std::fs::remove_file(segment_path(&self.folder, old));
        }
    }

    /// One stepped tick.
    pub(crate) fn tick(&mut self, msec: u32, input: &TickInput, after: &Snapshot) {
        if let Some(bytes) = self
            .writer
            .as_mut()
            .and_then(|writer| writer.tick(msec, input, after))
        {
            self.send(Message::Append(bytes));
        }
    }

    /// The ticks not yet written, ending at `last` (the run ends).
    pub(crate) fn flush(&mut self, last: &Snapshot) {
        let bytes = self
            .writer
            .as_mut()
            .map(|writer| writer.flush(last))
            .unwrap_or_else(Vec::new);
        if !bytes.is_empty() {
            self.send(Message::Append(bytes));
        }
    }
}

impl Drop for Flight {
    /// Waits for what was sent to reach the disk.
    fn drop(&mut self) {
        self.tx = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
