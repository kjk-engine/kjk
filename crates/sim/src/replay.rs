//! Recording and replay: the starting snapshot, then every tick's input and
//! milliseconds, and the modules installed on the way. Playing a recording
//! steps a world from the start with `StepReason::Replay`, installing each
//! module after the tick it was installed after; hashes taken while
//! recording (and each install's announced state) say whether it still
//! comes out the same, and if not, near which tick it left.

use crate::input::TickInput;
use crate::install::Announcement;
use crate::save::ScriptVmState;
use crate::snapshot::{Snapshot, SnapshotHash};
use crate::wire::{DecodeError, Reader, Writer};
use crate::world::{Level, LevelId, SimWorld, StepReason, step};

const MAGIC: &[u8; 4] = b"KJKR";
/// 2: commands carry view angles (I13). 3: the level it starts in, module
/// installs (I14). 4: a tick's command count takes two bytes (every seat
/// fits). 5: announcements carry their seats. 6: commands carry the weapon
/// and power selected (I23). 8: the scripts' state at the start (I16).
/// 9: with perception (I17). 10: combat rounds, effects (C14). 11:
/// listeners (shouts), random walks. 12: lasting effects. 13: visual
/// effects carried, effects at a place. 14: conversations (I18). 15: ticks
/// carry the seats' actions (I21's containers). 16: heartbeats
/// removed (`RemoveHeartbeat`). 17: the scripts' state as a save's (`KJKV`
/// 21: the party table's switch, `SwitchPlayerCharacter`). 18: the
/// scripts' state as a save carries it (`KJKV` 22). 19: as `KJKV` 24 does
/// (the factions' standing, location globals). 20: and its conversations'
/// placed cameras (`KJKV` 25). 21: as `KJKV` 26 (the party table's kept
/// companions). 22: as `KJKV` 27. 23: as `KJKV` 28. 24: as `KJKV` 29 (main's
/// cameras with the second engine-smells batch). 25: as `KJKV` 30. 26: as
/// `KJKV` 31 (the traps' state). 27: as `KJKV` 32 (items' properties,
/// sheets' gear). 28: as `KJKV` 33 (the dead's remains, creatures' gold).
/// 29: as `KJKV` 34 (Security as a skill's use). 30: as `KJKV` 38 (the
/// pazaak match's player). 31: as `KJKV` 39 and 40 (the companion leading
/// in an event waiting; a conversation's listener, joined bodies, party,
/// lock, request, wait and the match its line dealt; the asker of an event
/// waiting and of a queued conversation; the host's answer to a request,
/// `Action::Request`; the answer byte's 30 places and Leave; the pazaak
/// matches, each player's, and their watchers; `Action::Watch`). 32: as
/// `KJKV` 41 (a conversation's camera angle, its nodes' angles, the line's
/// pair and its turn and head-follow locks). 33: commands carry the
/// crosshair line (`Command::aim`).
/// 34: the party table keeps a companion's template made available by template
/// (`party::Npc::template_data`).
/// 35: a take names the item's script id (`Action::Take::item`), not a slot.
const VERSION: u32 = 35 + crate::pending::REPLAY;

#[derive(Debug, Clone, PartialEq)]
pub struct RecordedTick {
    pub msec: u32,
    pub input: TickInput,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    /// The rate it was recorded at, for reports; each tick carries its own
    /// milliseconds.
    pub hz: u32,
    /// The level `start` is in: a replay refuses any other.
    pub level: LevelId,
    pub start: Snapshot,
    /// The scripts' state at `start` (what a save would carry with it).
    pub script: ScriptVmState,
    /// From `start.tick + 1` on.
    pub ticks: Vec<RecordedTick>,
    /// The hash after every `every` ticks: (tick, hash).
    pub checkpoints: Vec<(u64, SnapshotHash)>,
    /// The hash after the last tick.
    pub end: SnapshotHash,
    /// The modules installed, in order, each after its tick.
    pub installs: Vec<Announcement>,
}

/// Collects a recording while a host steps: the ticks kept as the
/// recording's file has them (a few bytes each, where a `TickInput` held
/// is hundreds), for a session of hours.
pub struct Recorder {
    recording: Recording,
    ticks: Writer,
    count: u64,
    every: u64,
}

impl Recorder {
    /// Starts from the snapshot the world is at in `level`, hashing every
    /// `every` ticks.
    pub fn new(hz: u32, start: Snapshot, level: LevelId, every: u64) -> Self {
        let end = start.hash();
        Recorder {
            recording: Recording {
                hz,
                level,
                start,
                script: ScriptVmState::default(),
                ticks: Vec::new(),
                checkpoints: Vec::new(),
                end,
                installs: Vec::new(),
            },
            ticks: Writer::default(),
            count: 0,
            every: every.max(1),
        }
    }

    /// The scripts' state the world started from (`SimWorld::script_state`),
    /// for a world whose scripts may run.
    pub fn with_script(mut self, script: ScriptVmState) -> Self {
        self.recording.script = script;
        self
    }

    /// A module installed after the last recorded tick.
    pub fn install(&mut self, announcement: Announcement) {
        self.recording.installs.push(announcement);
    }

    /// One stepped tick: what went in, and the snapshot that came out.
    pub fn record(&mut self, msec: u32, input: &TickInput, after: &Snapshot) {
        encode_tick(msec, input, &mut self.ticks);
        self.count += 1;
        if self.count.is_multiple_of(self.every) {
            self.recording.checkpoints.push((after.tick, after.hash()));
        }
    }

    /// Ends the recording at `last`, the latest snapshot.
    pub fn finish(mut self, last: &Snapshot) -> Recording {
        let mut ticks = Reader::new("recorded ticks", &self.ticks.0);
        self.recording.ticks = (0..self.count)
            .map(|_| decode_tick(&mut ticks).expect("the recorder's own encoding"))
            .collect();
        self.recording.end = last.hash();
        self.recording
    }

    /// The same as its file (`Recording::encode`), the ticks written as
    /// they were kept, none decoded.
    pub fn finish_encoded(mut self, last: &Snapshot) -> Vec<u8> {
        self.recording.end = last.hash();
        self.recording.encode_with(self.count, &self.ticks.0)
    }
}

/// Where a replay stopped matching its recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Divergence {
    /// The first checkpoint (or the end) that differed; the cause lies at
    /// or before it, after the previous checkpoint.
    pub tick: u64,
    pub recorded: SnapshotHash,
    pub replayed: SnapshotHash,
    /// Why, when it was not the state: a level or install that failed.
    pub reason: Option<String>,
}

impl Recording {
    /// Steps the recording from its start in `level`; the final snapshot,
    /// or where it diverged. A recording that installs modules needs
    /// `play_with`.
    pub fn play(&self, level: &Level) -> Result<Snapshot, Divergence> {
        self.play_with(level, &mut |announcement| {
            Err(format!("no level for {}", announcement.module))
        })
    }

    /// The same, building each installed module's level with `levels`.
    pub fn play_with(
        &self,
        level: &Level,
        levels: &mut dyn FnMut(&Announcement) -> Result<Level, String>,
    ) -> Result<Snapshot, Divergence> {
        self.play_watched(level, levels, &mut |_| {})
    }

    /// The same, showing `watch` every snapshot stepped.
    pub fn play_watched(
        &self,
        level: &Level,
        levels: &mut dyn FnMut(&Announcement) -> Result<Level, String>,
        watch: &mut dyn FnMut(&Snapshot),
    ) -> Result<Snapshot, Divergence> {
        let failed = |tick: u64, reason: String| Divergence {
            tick,
            recorded: SnapshotHash([0; 32]),
            replayed: SnapshotHash([0; 32]),
            reason: Some(reason),
        };
        self.level
            .check(level)
            .map_err(|error| failed(self.start.tick, error.to_string()))?;
        let mut world = SimWorld::from_snapshot(&self.start, level.clone())
            .map_err(|error| failed(self.start.tick, error.to_string()))?;
        world.set_script_state(self.script.clone());
        let mut checkpoints = self.checkpoints.iter().peekable();
        let mut installs = self.installs.iter().peekable();
        let mut last = self.start.clone();
        // The start's installs (made before any tick), then each tick's
        // step and the installs made after it.
        let first = self.start.tick;
        for number in first..=first + self.ticks.len() as u64 {
            if number > first {
                let tick = &self.ticks[(number - first - 1) as usize];
                last = step(
                    &mut world,
                    number,
                    &tick.input,
                    tick.msec,
                    StepReason::Replay,
                );
                watch(&last);
                if let Some((_, recorded)) = checkpoints.next_if(|(at, _)| *at == number) {
                    let replayed = last.hash();
                    if replayed != *recorded {
                        return Err(Divergence {
                            tick: number,
                            recorded: *recorded,
                            replayed,
                            reason: None,
                        });
                    }
                }
            }
            while let Some(announcement) = installs.next_if(|a| a.tick == number) {
                let next = levels(announcement).map_err(|reason| failed(number, reason))?;
                announcement
                    .check(&next)
                    .map_err(|mismatch| failed(number, mismatch.to_string()))?;
                last = world
                    .install(next, announcement.arrival())
                    .map_err(|error| failed(number, error.to_string()))?;
                if last.hash() != announcement.after {
                    return Err(Divergence {
                        tick: number,
                        recorded: announcement.after,
                        replayed: last.hash(),
                        reason: Some(format!("installing {}", announcement.module)),
                    });
                }
                // The world as the module's entry left it, as its host
                // sees it after the install (a recording ending there).
                last = world.snapshot();
            }
        }
        let replayed = last.hash();
        if replayed != self.end {
            return Err(Divergence {
                tick: last.tick,
                recorded: self.end,
                replayed,
                reason: None,
            });
        }
        Ok(last)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut ticks = Writer::default();
        for tick in &self.ticks {
            encode_tick(tick.msec, &tick.input, &mut ticks);
        }
        self.encode_with(self.ticks.len() as u64, &ticks.0)
    }

    /// The file, with `count` ticks already encoded (`ticks`) in place of
    /// the recording's own.
    fn encode_with(&self, count: u64, ticks: &[u8]) -> Vec<u8> {
        let mut out = Writer::default();
        out.bytes(MAGIC);
        out.u32(VERSION);
        out.u32(self.hz);
        self.level.write(&mut out);
        out.section(&self.start.encode());
        let mut script = Writer::default();
        self.script.encode(&mut script);
        out.section(&script.0);
        out.u64(count);
        out.bytes(ticks);
        out.u32(u32::try_from(self.checkpoints.len()).expect("fewer than 2^32 checkpoints"));
        for (tick, hash) in &self.checkpoints {
            out.u64(*tick);
            out.bytes(&hash.0);
        }
        out.bytes(&self.end.0);
        out.u32(u32::try_from(self.installs.len()).expect("fewer than 2^32 installs"));
        for announcement in &self.installs {
            announcement.write(&mut out);
        }
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut input = Reader::new("recording", bytes);
        input.header(MAGIC, VERSION)?;
        let hz = input.u32()?;
        let level = LevelId::read(&mut input)?;
        let start = Snapshot::decode(input.section()?)?;
        let mut section = Reader::new("recording's scripts", input.section()?);
        let script = ScriptVmState::decode(&mut section)?;
        section.finish()?;
        let count = input.u64()?;
        let mut ticks = Vec::new();
        for _ in 0..count {
            ticks.push(decode_tick(&mut input)?);
        }
        let mut checkpoints = Vec::new();
        for _ in 0..input.u32()? {
            checkpoints.push((input.u64()?, SnapshotHash(input.array()?)));
        }
        let end = SnapshotHash(input.array()?);
        let mut installs = Vec::new();
        for _ in 0..input.u32()? {
            installs.push(Announcement::read(&mut input)?);
        }
        input.finish()?;
        Ok(Recording {
            hz,
            level,
            start,
            script,
            ticks,
            checkpoints,
            end,
            installs,
        })
    }
}

fn encode_tick(msec: u32, input: &TickInput, out: &mut Writer) {
    out.u16(u16::try_from(msec).expect("a tick is under a minute"));
    input.encode(out);
}

fn decode_tick(input: &mut Reader) -> Result<RecordedTick, DecodeError> {
    let msec = u32::from(input.u16()?);
    Ok(RecordedTick {
        msec,
        input: TickInput::decode(input)?,
    })
}
