//! The session's bytes over a link: reliable messages, the commands frame,
//! the clock frame and snapshot payloads (whole or in fragments). Every
//! decoder takes `&[u8]`, checks lengths first and returns a `Result`.
//!
//! ```text
//! message   = type u8, then per type (reliable, in order):
//!   1 Welcome    seat u8, seats u8, hz varint, every u8, tick varint,
//!                epoch u16, reconnect token u64
//!   2 Level      0 name text, digest 32   (a stage: arena, course)
//!                1 announcement (KJKA)     (a module)
//!   3 Snapshot   payload                   (a keyframe, or a delta too large)
//!   4 Installing request, payload          (the pre-install snapshot)
//!   5 Installed  epoch u16, announcement
//!   6 Resume     tick varint
//!   7 Action     a player's action (`sim::action`)   (client to host)

//!   8 Loaded     epoch u16, tick varint, check 8   (client to host)
//!   9 Resync     (client to host: send me a keyframe)
//!  10 Notice     text: something that player's action came to, or a
//!                script told it
//!  11 Screen     a screen a script opens for that player
//!                (`sim::notice::Screen`)
//!  12 Sound      a sound a script plays (`sim::notice::Sound`)
//!  13 SaveCopy   copy varint, part varint, parts varint, bytes
//!                (host to client: a save, in parts, I30 P17)
//!  14 Hold       payload   (the held snapshot: a movie, a break; Resume
//!                ends it)
//! request   = game text, module text, arrival text, seats u8, players u8
//! payload   = since varint, every u8, varint(len) codec frame,
//!             seats varint, (seat u8, command)*    seats rising
//! fragment  = tick varint, index << 4 | count - 1, bytes     (SNAPSHOT)
//! commands  = epoch u16, confirmed varint (tick + 1, 0 none),
//!             newest varint, count u8, (command, lag)*  oldest first,
//!             each command against the one before, each view lag (ms)
//!             a zigzag varint of its change from the one before
//! clock     = least varint (tick + 1, 0 none), slack zigzag (µs),
//!             late varint, substituted varint, miss varint (tick + 1)
//! command   = mask u8 (pitch, yaw, roll, forward, right, up, buttons,
//!             the selection), then each field that differs from the one
//!             it is coded against: angles as zigzag varints of the
//!             wrapping difference, axes a byte each, buttons u16, the
//!             selection (weapon, force, reply) a byte each
//! ```
//!
//! A snapshot's seat commands are coded against the commands of the
//! frame's baseline (both sides keep them by tick), so an unchanged seat
//! costs its seat byte and a zero mask.

use sim::{Announcement, Command, LevelId, MAX_SEATS, Seat, TickInput};

use crate::codec::Check;
use crate::wire::{self, Reader, WireError, put_text, put_u16, put_u64, put_varint, put_zigzag};

/// The most commands one commands frame carries (256 ms at 125 Hz).
pub const MAX_COMMANDS: usize = 32;
/// The most fragments of one snapshot payload.
pub const MAX_FRAGMENTS: usize = 16;
/// Bytes of payload in one fragment: a fragment, its frame header and the
/// packet header fit a datagram with room to spare.
pub const FRAGMENT: usize = 1150;

const WELCOME: u8 = 1;
const LEVEL: u8 = 2;
const SNAPSHOT: u8 = 3;
const INSTALLING: u8 = 4;
const INSTALLED: u8 = 5;
const RESUME: u8 = 6;

const ACTION: u8 = 7;
const LOADED: u8 = 8;
const RESYNC: u8 = 9;
const NOTICE: u8 = 10;
const SCREEN: u8 = 11;
const SOUND: u8 = 12;
const SAVE_COPY: u8 = 13;
const HOLD: u8 = 14;

/// Bytes of a save copy in one `SaveCopy` message. A connection sends a
/// part only when nothing else of its reliable channel is in flight, so a
/// message after it (an install, a notice) waits for one part at most: a
/// quarter second at the default budget.
pub const SAVE_PART: usize = 64 * 1024;
/// The most parts of one save copy: 32 MiB.
pub const MAX_SAVE_PARTS: usize = 512;

/// One part of a save copy (I30 P17): copy `copy`'s part `part` of
/// `parts`; every part but the last carries `SAVE_PART` bytes. The bytes
/// are opaque here: what the host's game packed (its save and its meta).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavePart<'a> {
    pub copy: u32,
    pub part: usize,
    pub parts: usize,
    pub bytes: &'a [u8],
}

impl<'a> SavePart<'a> {
    /// `blob` as copy `copy`'s parts, in order.
    pub fn split(copy: u32, blob: &'a [u8]) -> impl Iterator<Item = SavePart<'a>> {
        let parts = blob.len().div_ceil(SAVE_PART);
        blob.chunks(SAVE_PART)
            .enumerate()
            .map(move |(part, bytes)| SavePart {
                copy,
                part,
                parts,
                bytes,
            })
    }
}

/// A save copy coming in part by part, in order (the reliable channel's).
/// A copy is kept whole or not at all.
#[derive(Debug, Default)]
pub struct CopyAssembly {
    /// The copy being joined, the part expected next and its count.
    next: Option<(u32, usize, usize)>,
    bytes: Vec<u8>,
}

impl CopyAssembly {
    /// Takes `part`, and returns the copy when it was the last. A part that
    /// does not follow the one before starts over: a first part (another
    /// copy after a cut) begins anew, any other is dropped. Either way,
    /// what was held is dropped.
    pub fn take(&mut self, part: &SavePart<'_>) -> Option<Vec<u8>> {
        if self.next != Some((part.copy, part.part, part.parts)) {
            self.next = None;
            self.bytes.clear();
            if part.part != 0 {
                return None;
            }
        }
        self.bytes.extend_from_slice(part.bytes);
        if part.part + 1 == part.parts {
            self.next = None;
            return Some(std::mem::take(&mut self.bytes));
        }
        self.next = Some((part.copy, part.part + 1, part.parts));
        None
    }

    /// Bytes held for a copy not yet whole.
    pub fn held(&self) -> usize {
        self.bytes.len()
    }
}

/// What the host tells a client right after accepting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Welcome {
    /// The seat this connection occupies.
    pub seat: Seat,
    /// How many seats the world has (each one's avatar is drawn).
    pub seats: u8,
    /// Ticks per second.
    pub hz: u32,
    /// Snapshots every this many ticks, to start with.
    pub every: u8,
    /// The host's newest tick.
    pub tick: u64,
    /// The install epoch the session is in.
    pub epoch: u16,
    /// What a new connection names (`Credentials::reconnect`) to take
    /// this seat back after this one times out.
    pub token: u64,
}

/// The level a client builds to join: a stage by its id, or the current
/// module install's announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelRequest {
    /// A level without a module (`arena`, `course`), by name and digest.
    Stage(LevelId),
    /// A module, as the host installed it.
    Module(Announcement),
}

/// What a host is installing: enough for a client to start planning the
/// same module at once (`session::Request` in the launcher's terms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    /// `k1`, `tsl`, or `kjk` for a built level.
    pub game: String,
    pub module: String,
    /// The door or waypoint arrived at; empty for the module's entry.
    pub arrival: String,
    /// The players installed with.
    pub seats: u8,
    /// The players the module is planned for (I30 P21).
    pub players: u8,
}

impl InstallRequest {
    /// The request an announcement answers.
    pub fn of(announcement: &Announcement) -> InstallRequest {
        InstallRequest {
            game: announcement.game.clone(),
            module: announcement.module.clone(),
            arrival: announcement.arrival.clone(),
            seats: announcement.seats,
            players: announcement.players,
        }
    }

    /// The arrival, as `SimWorld::install` takes it.
    pub fn arrival(&self) -> Option<&str> {
        (!self.arrival.is_empty()).then_some(self.arrival.as_str())
    }
}

/// A reliable message, borrowing its payloads from the bytes it was read
/// from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message<'a> {
    Welcome(Welcome),
    Level(LevelRequest),
    /// A snapshot payload sent reliably: a keyframe, or a delta too large
    /// for `MAX_FRAGMENTS` datagrams.
    Snapshot(&'a [u8]),
    /// The host stopped after this payload's tick to install a module.
    Installing {
        request: InstallRequest,
        payload: &'a [u8],
    },
    /// The host installed; `epoch` starts with the install's result.
    Installed {
        epoch: u16,
        announcement: Announcement,
    },
    /// The world steps again after `tick`.
    Resume {
        tick: u64,
    },
    /// The client holds `epoch`'s snapshot of `tick`, whose check is
    /// `check`, and can play.
    Loaded {
        epoch: u16,
        tick: u64,
        check: Check,
    },
    /// The client cannot rebuild frames: send a keyframe.
    Resync,
    /// Something the client's player asks of the world (applied at the
    /// next tick the host steps).
    Action(sim::action::Action),
    /// What became of a player's action, or a script's words, for that
    /// player.
    Notice(String),
    /// A screen a script opens for that player.
    Screen(sim::notice::Screen),
    /// A sound a script plays, for that player to hear.
    Sound(sim::notice::Sound),
    /// A part of a save the host made (I30 P17).
    SaveCopy(SavePart<'a>),
    /// The host holds its world after this payload's tick (a movie, a
    /// break): no ticks until `Resume`.
    Hold {
        payload: &'a [u8],
    },
}

impl Message<'_> {
    /// Appends the message.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Message::Welcome(welcome) => {
                out.push(WELCOME);
                out.push(welcome.seat.0);
                out.push(welcome.seats);
                put_varint(out, u64::from(welcome.hz));
                out.push(welcome.every);
                put_varint(out, welcome.tick);
                put_u16(out, welcome.epoch);
                put_u64(out, welcome.token);
            }
            Message::Level(LevelRequest::Stage(id)) => {
                out.extend_from_slice(&[LEVEL, 0]);
                put_text(out, &id.name);
                out.extend_from_slice(&id.digest);
            }
            Message::Level(LevelRequest::Module(announcement)) => {
                out.extend_from_slice(&[LEVEL, 1]);
                out.extend_from_slice(&announcement.encode());
            }
            Message::Snapshot(payload) => {
                out.push(SNAPSHOT);
                out.extend_from_slice(payload);
            }
            Message::Installing { request, payload } => {
                out.push(INSTALLING);
                put_text(out, &request.game);
                put_text(out, &request.module);
                put_text(out, &request.arrival);
                out.push(request.seats);
                out.push(request.players);
                out.extend_from_slice(payload);
            }
            Message::Installed {
                epoch,
                announcement,
            } => {
                out.push(INSTALLED);
                put_u16(out, *epoch);
                out.extend_from_slice(&announcement.encode());
            }
            Message::Resume { tick } => {
                out.push(RESUME);
                put_varint(out, *tick);
            }

            Message::Loaded { epoch, tick, check } => {
                out.push(LOADED);
                put_u16(out, *epoch);
                put_varint(out, *tick);
                out.extend_from_slice(&check.0);
            }
            Message::Resync => out.push(RESYNC),
            Message::Action(action) => {
                out.push(ACTION);
                out.extend_from_slice(&action.to_bytes());
            }
            Message::Notice(text) => {
                out.push(NOTICE);
                put_text(out, text);
            }
            Message::Screen(screen) => {
                out.push(SCREEN);
                out.extend_from_slice(&screen.to_bytes());
            }
            Message::Sound(sound) => {
                out.push(SOUND);
                out.extend_from_slice(&sound.to_bytes());
            }
            Message::SaveCopy(part) => {
                out.push(SAVE_COPY);
                put_varint(out, u64::from(part.copy));
                put_varint(out, part.part as u64);
                put_varint(out, part.parts as u64);
                out.extend_from_slice(part.bytes);
            }
            Message::Hold { payload } => {
                out.push(HOLD);
                out.extend_from_slice(payload);
            }
        }
    }

    /// The message's bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }

    /// Reads a message; the bytes must hold exactly one.
    pub fn decode(bytes: &[u8]) -> Result<Message<'_>, WireError> {
        let mut input = Reader::new(bytes);
        let message = match input.u8("message")? {
            WELCOME => Message::Welcome(Welcome {
                seat: Seat(input.u8("seat")?),
                seats: input.u8("seats")?,
                hz: u32::try_from(input.varint("hz")?)
                    .map_err(|_| WireError::Invalid { what: "hz" })?,
                every: input.u8("every")?,
                tick: input.varint("tick")?,
                epoch: input.u16("epoch")?,
                token: input.u64("token")?,
            }),
            LEVEL => match input.u8("level kind")? {
                0 => Message::Level(LevelRequest::Stage(LevelId {
                    name: input.text("level")?,
                    digest: input.array("digest")?,
                })),
                1 => Message::Level(LevelRequest::Module(announcement(input.rest())?)),
                got => {
                    return Err(WireError::Kind {
                        what: "level kind",
                        got,
                    });
                }
            },
            SNAPSHOT => Message::Snapshot(input.rest()),
            INSTALLING => Message::Installing {
                request: InstallRequest {
                    game: input.text("game")?,
                    module: input.text("module")?,
                    arrival: input.text("arrival")?,
                    seats: input.u8("seats")?,
                    players: input.u8("players")?,
                },
                payload: input.rest(),
            },
            INSTALLED => Message::Installed {
                epoch: input.u16("epoch")?,
                announcement: announcement(input.rest())?,
            },
            RESUME => Message::Resume {
                tick: input.varint("tick")?,
            },

            LOADED => Message::Loaded {
                epoch: input.u16("epoch")?,
                tick: input.varint("tick")?,
                check: Check(input.array("check")?),
            },
            RESYNC => Message::Resync,
            ACTION => Message::Action(
                sim::action::Action::from_bytes(input.rest())
                    .map_err(|_| WireError::Invalid { what: "action" })?,
            ),
            NOTICE => Message::Notice(input.text("notice")?),
            SCREEN => Message::Screen(
                sim::notice::Screen::from_bytes(input.rest())
                    .map_err(|_| WireError::Invalid { what: "screen" })?,
            ),
            SOUND => Message::Sound(
                sim::notice::Sound::from_bytes(input.rest())
                    .map_err(|_| WireError::Invalid { what: "sound" })?,
            ),
            SAVE_COPY => {
                let copy = u32::try_from(input.varint("copy")?)
                    .map_err(|_| WireError::Invalid { what: "copy" })?;
                let part = input.count("save part", MAX_SAVE_PARTS)?;
                let parts = input.count("save parts", MAX_SAVE_PARTS)?;
                let bytes = input.rest();
                let last = part + 1 == parts;
                if part >= parts
                    || bytes.is_empty()
                    || bytes.len() > SAVE_PART
                    || (!last && bytes.len() != SAVE_PART)
                {
                    return Err(WireError::Invalid { what: "save part" });
                }
                Message::SaveCopy(SavePart {
                    copy,
                    part,
                    parts,
                    bytes,
                })
            }
            HOLD => Message::Hold {
                payload: input.rest(),
            },
            got => {
                return Err(WireError::Kind {
                    what: "message",
                    got,
                });
            }
        };
        input.finish()?;
        Ok(message)
    }
}

fn announcement(bytes: &[u8]) -> Result<Announcement, WireError> {
    Announcement::decode(bytes).map_err(|_| WireError::Invalid {
        what: "announcement",
    })
}

// ---------------------------------------------------------------------------
// Commands.

const PITCH: u16 = 1;
const YAW: u16 = 1 << 1;
const ROLL: u16 = 1 << 2;
const FORWARD: u16 = 1 << 3;
const RIGHT: u16 = 1 << 4;
const UP: u16 = 1 << 5;
const BUTTONS: u16 = 1 << 6;
const SELECT: u16 = 1 << 7;
const AIM: u16 = 1 << 8;
const ALL: u16 = (AIM << 1) - 1;

/// Appends `command` coded against `before`: a varint mask of the fields
/// that changed (one byte while only the first seven do), then each.
/// Every field is named (no `..`), here and in `read_command`: a field
/// added to `Command` does not compile until both code it.
pub fn put_command(out: &mut Vec<u8>, before: &Command, command: &Command) {
    let Command {
        angles,
        forward,
        right,
        up,
        buttons,
        weapon,
        force,
        reply,
        aim,
    } = *command;
    let mut body = Vec::new();
    let mut mask = 0;
    for (axis, bit) in [PITCH, YAW, ROLL].into_iter().enumerate() {
        if angles[axis] != before.angles[axis] {
            mask |= bit;
            let change = angles[axis].wrapping_sub(before.angles[axis]);
            put_zigzag(&mut body, i64::from(change));
        }
    }
    for (value, was, bit) in [
        (forward, before.forward, FORWARD),
        (right, before.right, RIGHT),
        (up, before.up, UP),
    ] {
        if value != was {
            mask |= bit;
            body.push(value as u8);
        }
    }
    if buttons != before.buttons {
        mask |= BUTTONS;
        put_u16(&mut body, buttons);
    }
    if (weapon, force, reply) != selected(before) {
        mask |= SELECT;
        body.extend_from_slice(&[weapon, force, reply]);
    }
    if aim != before.aim {
        mask |= AIM;
        body.extend(aim.map(|step| step as u8));
    }
    put_varint(out, u64::from(mask));
    out.extend(body);
}

/// The selections a command carries, sent together.
fn selected(command: &Command) -> (u8, u8, u8) {
    (command.weapon, command.force, command.reply)
}

/// Reads a command coded against `before`. A field the mask names must
/// differ from `before`'s (one command has one encoding).
pub fn read_command(input: &mut Reader<'_>, before: &Command) -> Result<Command, WireError> {
    let mask = u16::try_from(input.varint("command mask")?)
        .ok()
        .filter(|mask| mask & !ALL == 0)
        .ok_or(WireError::Invalid {
            what: "command mask",
        })?;
    let mut angles = before.angles;
    for (axis, bit) in [PITCH, YAW, ROLL].into_iter().enumerate() {
        if mask & bit != 0 {
            let change = i16::try_from(input.zigzag("angle")?)
                .map_err(|_| WireError::Invalid { what: "angle" })?;
            if change == 0 {
                return Err(WireError::Invalid { what: "angle" });
            }
            angles[axis] = before.angles[axis].wrapping_add(change);
        }
    }
    let mut axes = [before.forward, before.right, before.up];
    for (field, bit) in axes.iter_mut().zip([FORWARD, RIGHT, UP]) {
        if mask & bit != 0 {
            let value = input.u8("axis")? as i8;
            if value == *field {
                return Err(WireError::Invalid { what: "axis" });
            }
            *field = value;
        }
    }
    let mut buttons = before.buttons;
    if mask & BUTTONS != 0 {
        buttons = input.u16("buttons")?;
        if buttons == before.buttons {
            return Err(WireError::Invalid { what: "buttons" });
        }
    }
    let (mut weapon, mut force, mut reply) = selected(before);
    if mask & SELECT != 0 {
        weapon = input.u8("weapon")?;
        force = input.u8("force")?;
        reply = input.u8("reply")?;
        if (weapon, force, reply) == selected(before) {
            return Err(WireError::Invalid { what: "selection" });
        }
    }
    let mut aim = before.aim;
    if mask & AIM != 0 {
        for step in &mut aim {
            *step = input.u8("aim")? as i8;
        }
        if aim == before.aim {
            return Err(WireError::Invalid { what: "aim" });
        }
    }
    let [forward, right, up] = axes;
    Ok(Command {
        angles,
        forward,
        right,
        up,
        buttons,
        weapon,
        force,
        reply,
        aim,
    })
}

/// A client's commands frame: its commands by tick, each with its view lag
/// (how far behind the tick its player saw the world when it was made,
/// ms: C18's view stamp, which the authority rewinds shots by), and what
/// the client confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Commands {
    /// The install epoch the commands are for.
    pub epoch: u16,
    /// The newest snapshot the client rebuilt and verified.
    pub confirmed: Option<u64>,
    /// The tick of the last command.
    pub newest: u64,
    count: usize,
    list: [Command; MAX_COMMANDS],
    lags: [u16; MAX_COMMANDS],
}

impl Commands {
    /// A frame with no commands yet.
    pub fn new(epoch: u16, confirmed: Option<u64>) -> Commands {
        Commands {
            epoch,
            confirmed,
            newest: 0,
            count: 0,
            list: [Command::default(); MAX_COMMANDS],
            lags: [0; MAX_COMMANDS],
        }
    }

    /// Adds the command of the tick after the last one (the first may be
    /// any tick) with its view lag (ms); false when full.
    pub fn push(&mut self, tick: u64, command: Command, lag: u16) -> bool {
        if self.count == MAX_COMMANDS
            || (self.count > 0 && Some(tick) != self.newest.checked_add(1))
        {
            return false;
        }
        self.list[self.count] = command;
        self.lags[self.count] = lag;
        self.count += 1;
        self.newest = tick;
        true
    }

    /// The commands, oldest first, with their ticks and view lags. No tick
    /// is past `newest`, so no tick overflows, whatever a frame said.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &Command, u16)> {
        let first = self
            .newest
            .saturating_sub((self.count as u64).saturating_sub(1));
        self.list[..self.count]
            .iter()
            .zip(&self.lags[..self.count])
            .enumerate()
            .map(move |(index, (command, lag))| (first + index as u64, command, *lag))
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Appends the frame's payload.
    pub fn encode(&self, out: &mut Vec<u8>) {
        put_u16(out, self.epoch);
        put_varint(out, self.confirmed.map_or(0, |tick| tick + 1));
        put_varint(out, self.newest);
        out.push(self.count as u8);
        let (mut before, mut lag) = (Command::default(), 0i64);
        for (command, &next) in self.list[..self.count].iter().zip(&self.lags) {
            put_command(out, &before, command);
            put_zigzag(out, i64::from(next) - lag);
            (before, lag) = (*command, i64::from(next));
        }
    }

    /// Reads a frame's payload.
    pub fn decode(payload: &[u8]) -> Result<Commands, WireError> {
        let mut input = Reader::new(payload);
        let epoch = input.u16("epoch")?;
        let confirmed = input.varint("confirmed")?.checked_sub(1);
        let newest = input.varint("newest")?;
        let count = usize::from(input.u8("count")?);
        if count > MAX_COMMANDS {
            return Err(WireError::Limit {
                what: "commands",
                len: count,
                max: MAX_COMMANDS,
            });
        }
        // No tick is u64::MAX: the tick after the newest is always one.
        if (count as u64).saturating_sub(1) > newest || newest == u64::MAX {
            return Err(WireError::Invalid {
                what: "command ticks",
            });
        }
        let mut commands = Commands::new(epoch, confirmed);
        let (mut before, mut lag) = (Command::default(), 0i64);
        for index in 0..count {
            let command = read_command(&mut input, &before)?;
            lag = lag
                .checked_add(input.zigzag("view lag")?)
                .filter(|lag| (0..=i64::from(u16::MAX)).contains(lag))
                .ok_or(WireError::Invalid { what: "view lag" })?;
            commands.list[index] = command;
            commands.lags[index] = lag as u16;
            before = command;
        }
        commands.count = count;
        commands.newest = newest;
        input.finish()?;
        Ok(commands)
    }
}

// ---------------------------------------------------------------------------
// The clock report.

/// The host's report of how a client's commands arrive (design section
/// 7): the command that came with the least slack since the last report
/// (a frame makes several commands at once, so the oldest of them is the
/// one to keep in time), and what went wrong since.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClockReport {
    /// A command's tick and how long before its step it arrived (µs;
    /// negative is late).
    pub least: Option<(u64, i64)>,
    /// Commands that came after their tick was stepped.
    pub late: u32,
    /// Ticks stepped with a substitute for the seat's command.
    pub substituted: u32,
    /// The newest tick missed (late or substituted).
    pub miss: Option<u64>,
}

impl ClockReport {
    /// A command's slack, kept if it is the least since the last report.
    pub fn take(&mut self, tick: u64, slack_us: i64) {
        if self.least.is_none_or(|(_, least)| slack_us < least) {
            self.least = Some((tick, slack_us));
        }
    }

    /// Appends the frame's payload.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self.least {
            Some((tick, slack)) => {
                put_varint(out, tick + 1);
                put_zigzag(out, slack);
            }
            None => put_varint(out, 0),
        }
        put_varint(out, u64::from(self.late));
        put_varint(out, u64::from(self.substituted));
        put_varint(out, self.miss.map_or(0, |tick| tick + 1));
    }

    /// Reads a frame's payload.
    pub fn decode(payload: &[u8]) -> Result<ClockReport, WireError> {
        let mut input = Reader::new(payload);
        let least = match input.varint("least")?.checked_sub(1) {
            Some(tick) => Some((tick, input.zigzag("slack")?)),
            None => None,
        };
        let count = |input: &mut Reader<'_>, what| {
            u32::try_from(input.varint(what)?).map_err(|_| WireError::Invalid { what })
        };
        let report = ClockReport {
            least,
            late: count(&mut input, "late")?,
            substituted: count(&mut input, "substituted")?,
            miss: input.varint("miss")?.checked_sub(1),
        };
        input.finish()?;
        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// Snapshot payloads and fragments.

/// A snapshot payload, read up to its seat commands (which need the
/// baseline's to decode: `read_seats`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Payload<'a> {
    /// Microseconds from the tick's step to the send.
    pub since_us: u64,
    /// The host's snapshot interval for this client, in ticks.
    pub every: u8,
    /// The codec frame (`codec::FrameHeader` and delta).
    pub frame: &'a [u8],
    seats: &'a [u8],
}

impl<'a> Payload<'a> {
    /// Reads the parts of a payload.
    pub fn read(bytes: &'a [u8]) -> Result<Payload<'a>, WireError> {
        let mut input = Reader::new(bytes);
        let since_us = input.varint("since")?;
        let every = input.u8("every")?;
        let len = input.count("frame", wire::MAX_MESSAGE)?;
        let frame = input.take(len, "frame")?;
        Ok(Payload {
            since_us,
            every,
            frame,
            seats: input.rest(),
        })
    }

    /// The seat commands, coded against `base` (the baseline's; empty for
    /// a keyframe).
    pub fn read_seats(&self, base: &TickInput) -> Result<TickInput, WireError> {
        let mut input = Reader::new(self.seats);
        let count = input.count("seats", usize::from(MAX_SEATS))?;
        let mut commands = TickInput::default();
        let mut last = None;
        for _ in 0..count {
            let seat = input.u8("seat")?;
            if last.is_some_and(|last| seat <= last) || seat >= MAX_SEATS {
                return Err(WireError::Invalid { what: "seat" });
            }
            last = Some(seat);
            let before = base.get(Seat(seat)).copied().unwrap_or(Command::default());
            commands.insert(Seat(seat), read_command(&mut input, &before)?);
        }
        input.finish()?;
        Ok(commands)
    }
}

/// Appends a payload's frame and seat commands (all but the send stamp,
/// which `put_payload_head` writes first).
pub fn put_payload_body(out: &mut Vec<u8>, frame: &[u8], commands: &TickInput, base: &TickInput) {
    put_varint(out, frame.len() as u64);
    out.extend_from_slice(frame);
    put_varint(out, commands.len() as u64);
    for (seat, command) in commands.iter() {
        out.push(seat.0);
        let before = base.get(seat).copied().unwrap_or(Command::default());
        put_command(out, &before, command);
    }
}

/// Appends a payload's send stamp and interval.
pub fn put_payload_head(out: &mut Vec<u8>, since_us: u64, every: u8) {
    put_varint(out, since_us);
    out.push(every);
}

/// One fragment of a snapshot payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fragment<'a> {
    pub tick: u64,
    pub index: usize,
    pub count: usize,
    pub bytes: &'a [u8],
}

impl<'a> Fragment<'a> {
    /// Appends the fragment as a frame payload.
    pub fn encode(&self, out: &mut Vec<u8>) {
        put_varint(out, self.tick);
        out.push(((self.index << 4) | (self.count - 1)) as u8);
        out.extend_from_slice(self.bytes);
    }

    /// Reads a fragment.
    pub fn decode(payload: &'a [u8]) -> Result<Fragment<'a>, WireError> {
        let mut input = Reader::new(payload);
        let tick = input.varint("tick")?;
        let byte = usize::from(input.u8("fragment")?);
        let (index, count) = (byte >> 4, (byte & 0x0f) + 1);
        if index >= count {
            return Err(WireError::Invalid { what: "fragment" });
        }
        let bytes = input.rest();
        if bytes.is_empty()
            || bytes.len() > FRAGMENT
            || (index + 1 < count && bytes.len() != FRAGMENT)
        {
            return Err(WireError::Invalid {
                what: "fragment length",
            });
        }
        Ok(Fragment {
            tick,
            index,
            count,
            bytes,
        })
    }
}
