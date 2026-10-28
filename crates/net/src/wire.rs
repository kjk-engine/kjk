//! The bytes on the wire: varints, the connection-less handshake packets,
//! the session packet header and its frames.
//!
//! Every decoder takes `&[u8]` and returns a `Result`; lengths are checked
//! before they are used, a count read from the wire never reserves memory
//! before the bytes it promises are known to be there, and every limit
//! below is enforced on decode. So no received byte can panic the process
//! or make it allocate more than the datagram it came in (`MAX_DATAGRAM`).
//!
//! Two classes of datagram share a port, told apart by their first bytes
//! (`classify`):
//! - **handshake** packets start with the magic `KJKN`, a `u16` protocol
//!   version and a type byte. The prefix up to the client's nonce (magic,
//!   version, type, nonce: 15 bytes) is frozen for every protocol version,
//!   so a host can answer a client of another version with a typed
//!   refusal it can match to its attempt. `Hello` and `Connect` must be
//!   exactly `MAX_DATAGRAM` long (padded with zeros), so no reply is ever
//!   larger than the request that caused it;
//! - **session** packets start with the kind byte: the high nibble is the
//!   protocol generation, the low nibble the packet type: `PACKET_DATA`
//!   (frames and acks) or `PACKET_FIRST` (frames from a peer that has
//!   received nothing yet, so its ack fields are zero and mean nothing).
//!   Generation 4 would read as the magic's `K` (0x4B) and is never used.
//!
//! A session packet is the 19-byte `Header` followed by frames, each a
//! type byte, a varint length and that many bytes of payload, until the
//! end of the datagram. A frame of a type this build does not know is
//! skipped by its length, so a newer peer can add frames.
//!
//! All integers are little-endian.

use crate::MAX_DATAGRAM;
use std::fmt;

/// The first four bytes of every handshake packet.
pub const MAGIC: [u8; 4] = *b"KJKN";
/// The handshake's protocol version; a host refuses any other
/// (`Refusal::Protocol`). 3: commands carry the crosshair line
/// (`Command::aim`), their mask a varint.
pub const PROTOCOL_VERSION: u16 = 3;
/// The protocol generation in every session packet's kind byte (high
/// nibble). Never 4: see the module docs.
pub const GENERATION: u8 = 1;
/// Session packet type (kind byte's low nibble): frames, with acks.
pub const PACKET_DATA: u8 = 1;
/// Session packet type: frames, from a peer that has received nothing yet
/// (its ack fields are zero and mean nothing).
pub const PACKET_FIRST: u8 = 2;
/// A session packet header: kind 1, session 8, seq 2, ack 2, ack bits 4,
/// ack delay 2.
pub const HEADER_LEN: usize = 19;
/// The frozen handshake prefix: magic 4, version 2, type 1, nonce 8.
pub const HANDSHAKE_PREFIX_LEN: usize = 15;
/// A LEB128 `u64` takes at most ten bytes.
pub const MAX_VARINT_LEN: usize = 10;
/// The longest text on the wire (names, builds, games, groups, reasons),
/// in bytes of UTF-8.
pub const MAX_TEXT: usize = 128;
/// The most games a `Credentials` names.
pub const MAX_GAMES: usize = 4;
/// The most groups per game in a `Credentials`, and in a
/// `Refusal::GameData`.
pub const MAX_GROUPS: usize = 8;
/// Bytes of a handshake cookie (a truncated keyed BLAKE3).
pub const COOKIE_LEN: usize = 16;
/// Bytes of a join password's proof (a truncated keyed BLAKE3).
pub const PROOF_LEN: usize = 16;
/// A reliable message is cut into slices of this many bytes (the last
/// one may be shorter), so a slice and a packet's header always fit one
/// datagram.
pub const SLICE: usize = 1100;
/// The largest reliable message.
pub const MAX_MESSAGE: usize = 4 * 1024 * 1024;
/// The most slices of one message: `MAX_MESSAGE / SLICE`, rounded up.
pub const MAX_PARTS: u32 = MAX_MESSAGE.div_ceil(SLICE) as u32;

/// Frame types of a session packet. The link reads only `RELIABLE`; the
/// others are the session's, carried as opaque latest-wins payloads.
pub mod frame {
    /// A slice of a reliable message (`Slice`).
    pub const RELIABLE: u8 = 1;
    /// Client to host: commands with their ticks (slice 3).
    pub const COMMANDS: u8 = 2;
    /// Host to client: a snapshot delta or one fragment of it (slice 2).
    pub const SNAPSHOT: u8 = 3;
    /// Host to client: the input slack report that steers the clock
    /// (slice 3).
    pub const CLOCK: u8 = 4;
    /// Either side: the session ends, for this reason (a `Refusal`); sent
    /// in three packets of its own, as nothing answers it.
    pub const DISCONNECT: u8 = 5;
}

/// Why bytes could not be decoded (or encoded). Received bytes that fail
/// are dropped; nothing here is fatal to the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// Fewer bytes than the field `what` needs.
    Truncated { what: &'static str },
    /// A datagram longer than `MAX_DATAGRAM`.
    Oversized { len: usize },
    /// A handshake packet that must be padded to `MAX_DATAGRAM` is not.
    Unpadded { len: usize },
    /// A varint longer than ten bytes, over 64 bits, or not in its
    /// shortest form (one value has one encoding).
    Varint,
    /// Not a handshake packet (no `KJKN`).
    Magic,
    /// A handshake packet of another protocol version (its frozen prefix
    /// is still readable, `peek_prefix`).
    Version { got: u16 },
    /// An unknown handshake packet type, session packet type or refusal.
    Kind { what: &'static str, got: u8 },
    /// A session packet of another protocol generation.
    Generation { got: u8 },
    /// A count or length over its limit.
    Limit {
        what: &'static str,
        len: usize,
        max: usize,
    },
    /// Text that is not UTF-8.
    Text,
    /// A value that breaks a rule of its field.
    Invalid { what: &'static str },
    /// Bytes left over after a packet that has no room for them.
    Trailing { len: usize },
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Truncated { what } => write!(f, "truncated at {what}"),
            WireError::Oversized { len } => {
                write!(f, "{len} bytes, over the {MAX_DATAGRAM}-byte datagram")
            }
            WireError::Unpadded { len } => {
                write!(f, "{len} bytes, not padded to {MAX_DATAGRAM}")
            }
            WireError::Varint => write!(f, "malformed varint"),
            WireError::Magic => write!(f, "not a handshake packet"),
            WireError::Version { got } => {
                write!(
                    f,
                    "protocol version {got}, this build speaks {PROTOCOL_VERSION}"
                )
            }
            WireError::Kind { what, got } => write!(f, "unknown {what} {got}"),
            WireError::Generation { got } => {
                write!(
                    f,
                    "protocol generation {got}, this build speaks {GENERATION}"
                )
            }
            WireError::Limit { what, len, max } => write!(f, "{what}: {len}, over {max}"),
            WireError::Text => write!(f, "text is not UTF-8"),
            WireError::Invalid { what } => write!(f, "invalid {what}"),
            WireError::Trailing { len } => write!(f, "{len} trailing bytes"),
        }
    }
}

impl std::error::Error for WireError {}

// ---------------------------------------------------------------------------
// Reading and writing primitives.

/// A cursor over received bytes. Every read checks the length first.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    /// Bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    /// The next `n` bytes.
    pub fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], WireError> {
        if self.remaining() < n {
            return Err(WireError::Truncated { what });
        }
        let taken = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(taken)
    }

    /// Everything not read yet.
    pub fn rest(&mut self) -> &'a [u8] {
        let rest = &self.bytes[self.at..];
        self.at = self.bytes.len();
        rest
    }

    /// The next `N` bytes as an array.
    pub fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N], WireError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N, what)?);
        Ok(out)
    }

    /// `what` names the field in an error, here and below.
    pub fn u8(&mut self, what: &'static str) -> Result<u8, WireError> {
        Ok(self.take(1, what)?[0])
    }

    pub fn u16(&mut self, what: &'static str) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(self.array(what)?))
    }

    pub fn u32(&mut self, what: &'static str) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(self.array(what)?))
    }

    pub fn u64(&mut self, what: &'static str) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(self.array(what)?))
    }

    /// An unsigned LEB128 varint, in its shortest form (`read_varint`).
    pub fn varint(&mut self, what: &'static str) -> Result<u64, WireError> {
        let (value, len) = read_varint(&self.bytes[self.at..]).map_err(|error| match error {
            VarintError::Truncated => WireError::Truncated { what },
            VarintError::Invalid => WireError::Varint,
        })?;
        self.at += len;
        Ok(value)
    }

    /// A zigzag-coded signed varint.
    pub fn zigzag(&mut self, what: &'static str) -> Result<i64, WireError> {
        Ok(unzigzag(self.varint(what)?))
    }

    /// A varint no larger than `max`, as a `usize`.
    pub fn count(&mut self, what: &'static str, max: usize) -> Result<usize, WireError> {
        let value = self.varint(what)?;
        match usize::try_from(value) {
            Ok(len) if len <= max => Ok(len),
            _ => Err(WireError::Limit {
                what,
                len: usize::try_from(value).unwrap_or(usize::MAX),
                max,
            }),
        }
    }

    /// UTF-8 text with a varint length of at most `MAX_TEXT` bytes.
    pub fn text(&mut self, what: &'static str) -> Result<String, WireError> {
        let len = self.count(what, MAX_TEXT)?;
        let bytes = self.take(len, what)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| WireError::Text)
    }

    /// Fails if anything is left.
    pub fn finish(&self) -> Result<(), WireError> {
        match self.remaining() {
            0 => Ok(()),
            len => Err(WireError::Trailing { len }),
        }
    }
}

/// Appends a little-endian integer, here and below.
pub fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Why bytes are not a varint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarintError {
    /// They end inside it.
    Truncated,
    /// Longer than a u64 holds, or padded (not its shortest form).
    Invalid,
}

/// The unsigned LEB128 varint `bytes` start with, in its shortest form,
/// and how many bytes it takes.
pub fn read_varint(bytes: &[u8]) -> Result<(u64, usize), VarintError> {
    let mut value = 0u64;
    for index in 0..MAX_VARINT_LEN {
        let byte = *bytes.get(index).ok_or(VarintError::Truncated)?;
        let bits = u64::from(byte & 0x7f);
        // The tenth byte holds only the top bit of a u64.
        if index == MAX_VARINT_LEN - 1 && bits > 1 {
            return Err(VarintError::Invalid);
        }
        value |= bits << (7 * index);
        if byte & 0x80 == 0 {
            // A zero last byte after the first is a padded encoding.
            if index > 0 && byte == 0 {
                return Err(VarintError::Invalid);
            }
            return Ok((value, index + 1));
        }
    }
    Err(VarintError::Invalid)
}

/// Appends `value` as an unsigned LEB128 varint (shortest form).
pub fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Bytes `put_varint` writes for `value`.
pub fn varint_len(value: u64) -> usize {
    let bits = 64 - (value | 1).leading_zeros() as usize;
    bits.div_ceil(7)
}

/// Zigzag: small magnitudes of either sign take few bytes.
pub fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// The inverse of `zigzag`.
pub fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

/// Appends a signed value as a zigzag varint.
pub fn put_zigzag(out: &mut Vec<u8>, value: i64) {
    put_varint(out, zigzag(value));
}

/// Appends text with its varint length, cut to at most `MAX_TEXT` bytes
/// at a character boundary (so what is sent always decodes).
pub fn put_text(out: &mut Vec<u8>, text: &str) {
    let text = clip(text);
    put_varint(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// `text` cut to at most `MAX_TEXT` bytes at a character boundary.
pub fn clip(text: &str) -> &str {
    if text.len() <= MAX_TEXT {
        return text;
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

// ---------------------------------------------------------------------------
// Classification.

/// What a received datagram is, from its first bytes alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Starts with `KJKN`: `Handshake::decode`.
    Handshake,
    /// Long enough for a header and of this generation: `Header::decode`.
    Session,
    /// Anything else: dropped silently.
    Other,
}

/// The class of a received datagram.
pub fn classify(bytes: &[u8]) -> Class {
    if bytes.len() > MAX_DATAGRAM {
        Class::Other
    } else if bytes.starts_with(&MAGIC) {
        Class::Handshake
    } else if bytes.len() >= HEADER_LEN && bytes[0] >> 4 == GENERATION {
        Class::Session
    } else {
        Class::Other
    }
}

// ---------------------------------------------------------------------------
// Session packets.

/// What a packet says about the peer's packets it has received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ack {
    /// The newest packet number received from the peer.
    pub seq: u16,
    /// Bit `n` set: packet `seq - 1 - n` was received too.
    pub bits: u32,
    /// How long ago `seq` arrived when this packet left, in 0.1 ms
    /// (saturating), so the peer measures RTT without shared clocks.
    pub delay: u16,
}

/// The header every session packet starts with (19 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// The id the host chose at accept; a packet with another is dropped
    /// silently.
    pub session: u64,
    /// This packet's number, wrapping (compared over half the range).
    pub seq: u16,
    /// The acks, once this side has received anything. Without, the
    /// packet type is `PACKET_FIRST` and the ack fields are zero: an ack
    /// field of 0 must never acknowledge the peer's packet 0 by accident.
    pub ack: Option<Ack>,
}

impl Header {
    /// Appends the header.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let (kind, ack) = match self.ack {
            Some(ack) => (PACKET_DATA, ack),
            None => (
                PACKET_FIRST,
                Ack {
                    seq: 0,
                    bits: 0,
                    delay: 0,
                },
            ),
        };
        out.push(GENERATION << 4 | kind);
        put_u64(out, self.session);
        put_u16(out, self.seq);
        put_u16(out, ack.seq);
        put_u32(out, ack.bits);
        put_u16(out, ack.delay);
    }

    /// The header and the frame bytes after it.
    pub fn decode(bytes: &[u8]) -> Result<(Header, &[u8]), WireError> {
        if bytes.len() > MAX_DATAGRAM {
            return Err(WireError::Oversized { len: bytes.len() });
        }
        let mut input = Reader::new(bytes);
        let kind = input.u8("kind")?;
        if kind >> 4 != GENERATION {
            return Err(WireError::Generation { got: kind >> 4 });
        }
        let session = input.u64("session")?;
        let seq = input.u16("seq")?;
        let ack = Ack {
            seq: input.u16("ack")?,
            bits: input.u32("ack bits")?,
            delay: input.u16("ack delay")?,
        };
        let ack = match kind & 0x0f {
            PACKET_DATA => Some(ack),
            PACKET_FIRST
                if ack
                    == (Ack {
                        seq: 0,
                        bits: 0,
                        delay: 0,
                    }) =>
            {
                None
            }
            PACKET_FIRST => return Err(WireError::Invalid { what: "ack fields" }),
            got => {
                return Err(WireError::Kind {
                    what: "packet type",
                    got,
                });
            }
        };
        Ok((Header { session, seq, ack }, input.rest()))
    }
}

/// One frame of a session packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    /// A `frame` constant, or a type from a newer build.
    pub kind: u8,
    pub payload: &'a [u8],
}

/// Bytes a frame with `payload_len` bytes of payload takes.
pub fn frame_len(payload_len: usize) -> usize {
    1 + varint_len(payload_len as u64) + payload_len
}

/// Appends one frame.
pub fn put_frame(out: &mut Vec<u8>, kind: u8, payload: &[u8]) {
    out.push(kind);
    put_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

/// The frames after a header, in order. Yields an error (once, then
/// stops) at a frame whose length runs past the end.
#[derive(Debug, Clone)]
pub struct Frames<'a> {
    input: Reader<'a>,
    failed: bool,
}

impl<'a> Frames<'a> {
    /// The frames in `bytes` (a packet after its header).
    pub fn new(bytes: &'a [u8]) -> Frames<'a> {
        Frames {
            input: Reader::new(bytes),
            failed: false,
        }
    }

    fn next_frame(&mut self) -> Result<Frame<'a>, WireError> {
        let kind = self.input.u8("frame type")?;
        let len = self.input.count("frame length", MAX_DATAGRAM)?;
        let payload = self.input.take(len, "frame payload")?;
        Ok(Frame { kind, payload })
    }
}

impl<'a> Iterator for Frames<'a> {
    type Item = Result<Frame<'a>, WireError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.input.remaining() == 0 {
            return None;
        }
        let frame = self.next_frame();
        self.failed = frame.is_err();
        Some(frame)
    }
}

/// A reliable frame's payload: slice `part` of `count` of message `id`.
/// Every slice but the last carries exactly `SLICE` bytes; the last
/// carries 1 to `SLICE` (0 only for an empty message of one part), and
/// the message is at most `MAX_MESSAGE` long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slice<'a> {
    pub id: u16,
    pub part: u32,
    pub count: u32,
    pub data: &'a [u8],
}

impl Slice<'_> {
    /// Bytes of the payload before `data`.
    pub fn head_len(part: u32, count: u32) -> usize {
        2 + varint_len(u64::from(part)) + varint_len(u64::from(count))
    }

    /// Bytes this slice takes as a frame.
    pub fn frame_len(&self) -> usize {
        frame_len(Slice::head_len(self.part, self.count) + self.data.len())
    }

    /// Appends this slice as a `frame::RELIABLE` frame.
    pub fn put_frame(&self, out: &mut Vec<u8>) {
        out.push(frame::RELIABLE);
        let len = Slice::head_len(self.part, self.count) + self.data.len();
        put_varint(out, len as u64);
        self.encode(out);
    }

    /// Appends the payload (without the frame's type and length).
    pub fn encode(&self, out: &mut Vec<u8>) {
        put_u16(out, self.id);
        put_varint(out, u64::from(self.part));
        put_varint(out, u64::from(self.count));
        out.extend_from_slice(self.data);
    }

    /// Reads a reliable frame's payload, checking the slice rules above.
    pub fn decode(payload: &[u8]) -> Result<Slice<'_>, WireError> {
        let mut input = Reader::new(payload);
        let id = input.u16("message id")?;
        let part = input.count("slice part", MAX_PARTS as usize)? as u32;
        let count = input.count("slice count", MAX_PARTS as usize)? as u32;
        let data = input.rest();
        if count == 0 || part >= count {
            return Err(WireError::Invalid { what: "slice part" });
        }
        let last = part + 1 == count;
        let fits = if last {
            let total = (count as usize - 1) * SLICE + data.len();
            data.len() <= SLICE && (count == 1 || !data.is_empty()) && total <= MAX_MESSAGE
        } else {
            data.len() == SLICE
        };
        if !fits {
            return Err(WireError::Invalid {
                what: "slice length",
            });
        }
        Ok(Slice {
            id,
            part,
            count,
            data,
        })
    }
}

// ---------------------------------------------------------------------------
// Handshake packets.

/// Handshake packet types (the byte after the version).
mod kind {
    pub const HELLO: u8 = 1;
    pub const CHALLENGE: u8 = 2;
    pub const CONNECT: u8 = 3;
    pub const ACCEPT: u8 = 4;
    pub const DENY: u8 = 5;
}

/// A game's data as the host must match it: `vfs::fingerprint`'s digest
/// and its per-group digests (`data`, `modules`, `override`, …), carried
/// as opaque names and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFingerprint {
    pub game: String,
    pub digest: [u8; 32],
    pub groups: Vec<(String, [u8; 32])>,
}

/// What a client says about itself in `Connect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// The build (`KJK_GIT_SHA`).
    pub build: String,
    /// The games the session needs, with their fingerprints.
    pub games: Vec<GameFingerprint>,
    /// The player's name.
    pub name: String,
    /// The token of a seat to take back after a drop.
    pub reconnect: Option<u64>,
}

impl Credentials {
    /// Appends the credentials; fails over the limits (`MAX_GAMES`,
    /// `MAX_GROUPS`, `MAX_TEXT`), which decode would refuse.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        let long = [self.build.as_str(), self.name.as_str()]
            .into_iter()
            .chain(self.games.iter().map(|game| game.game.as_str()))
            .chain(
                self.games
                    .iter()
                    .flat_map(|game| game.groups.iter().map(|(name, _)| name.as_str())),
            )
            .find(|text| text.len() > MAX_TEXT);
        if let Some(text) = long {
            return Err(WireError::Limit {
                what: "credentials text",
                len: text.len(),
                max: MAX_TEXT,
            });
        }
        limit("games", self.games.len(), MAX_GAMES)?;
        for game in &self.games {
            limit("groups", game.groups.len(), MAX_GROUPS)?;
        }
        put_text(out, &self.build);
        put_text(out, &self.name);
        match self.reconnect {
            Some(token) => {
                out.push(1);
                put_u64(out, token);
            }
            None => out.push(0),
        }
        put_varint(out, self.games.len() as u64);
        for game in &self.games {
            put_text(out, &game.game);
            out.extend_from_slice(&game.digest);
            put_varint(out, game.groups.len() as u64);
            for (name, digest) in &game.groups {
                put_text(out, name);
                out.extend_from_slice(digest);
            }
        }
        Ok(())
    }

    /// Reads credentials, refusing anything over the limits.
    pub fn decode(input: &mut Reader<'_>) -> Result<Credentials, WireError> {
        let build = input.text("build")?;
        let name = input.text("name")?;
        let reconnect = match input.u8("reconnect flag")? {
            0 => None,
            1 => Some(input.u64("reconnect token")?),
            _ => {
                return Err(WireError::Invalid {
                    what: "reconnect flag",
                });
            }
        };
        let count = input.count("games", MAX_GAMES)?;
        let mut games = Vec::with_capacity(count);
        for _ in 0..count {
            let game = input.text("game")?;
            let digest = input.array("game digest")?;
            let group_count = input.count("groups", MAX_GROUPS)?;
            let mut groups = Vec::with_capacity(group_count);
            for _ in 0..group_count {
                groups.push((input.text("group")?, input.array("group digest")?));
            }
            games.push(GameFingerprint {
                game,
                digest,
                groups,
            });
        }
        Ok(Credentials {
            build,
            games,
            name,
            reconnect,
        })
    }

    /// The host's check of a client's build and data against its own:
    /// the same build, and for every game the host plays, the same
    /// digest. A difference names the groups that differ (either side's
    /// group missing from the other counts), sorted, so the refusal says
    /// "your override folder differs", not a hash. A `-dirty` build is
    /// accepted only against the same dirty build, which string equality
    /// already means; warning about it is the caller's.
    pub fn check(host: &Credentials, here: &Credentials) -> Result<(), Refusal> {
        if host.build != here.build {
            return Err(Refusal::Build {
                host: host.build.clone(),
                here: here.build.clone(),
            });
        }
        for game in &host.games {
            let theirs = here.games.iter().find(|other| other.game == game.game);
            let differ = match theirs {
                Some(theirs) if theirs.digest == game.digest => continue,
                Some(theirs) => {
                    let mut names: Vec<String> = game
                        .groups
                        .iter()
                        .filter(|group| !theirs.groups.contains(group))
                        .chain(
                            theirs
                                .groups
                                .iter()
                                .filter(|group| !game.groups.contains(group)),
                        )
                        .map(|(name, _)| name.clone())
                        .collect();
                    names.sort();
                    names.dedup();
                    names
                }
                None => game.groups.iter().map(|(name, _)| name.clone()).collect(),
            };
            return Err(Refusal::GameData {
                game: game.game.clone(),
                groups: differ,
            });
        }
        Ok(())
    }
}

fn limit(what: &'static str, len: usize, max: usize) -> Result<(), WireError> {
    if len > max {
        Err(WireError::Limit { what, len, max })
    } else {
        Ok(())
    }
}

/// Why a connection is refused or ended, typed so both sides can say it
/// in words (`Display`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Another protocol version.
    Protocol { host: u16, here: u16 },
    /// Different commits. A `-dirty` build is accepted only against the
    /// same dirty SHA.
    Build { host: String, here: String },
    /// Named groups of a game's data differ (e.g. `override`).
    GameData { game: String, groups: Vec<String> },
    /// Every seat is taken.
    Full { seats: u8 },
    /// After accept, from the client: the host's level is not the one this
    /// machine builds.
    Level(String),
    /// After accept, from the client: it cannot plan the host's level.
    Plan(String),
    /// No answer in time (made by the side that waited; never sent).
    Timeout,
    /// The host removed this player.
    Kicked(String),
    /// The host ended the session.
    HostQuit,
    /// The player left the session.
    Left,
    /// The host has a join password, and the client gave none (`given`
    /// false) or another.
    Password { given: bool },
    /// The seat the reconnect token names is still played by a connection
    /// the host hears from (`session::TAKEOVER_QUIET_US`): not taken over.
    InUse,
}

impl Refusal {
    /// Appends the refusal. Texts are cut to `MAX_TEXT` bytes and groups
    /// to `MAX_GROUPS`, so an encoded `Deny` always fits a datagram (the
    /// longest is 1187 bytes).
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Refusal::Protocol { host, here } => {
                out.push(1);
                put_u16(out, *host);
                put_u16(out, *here);
            }
            Refusal::Build { host, here } => {
                out.push(2);
                put_text(out, host);
                put_text(out, here);
            }
            Refusal::GameData { game, groups } => {
                out.push(3);
                put_text(out, game);
                let groups = &groups[..groups.len().min(MAX_GROUPS)];
                put_varint(out, groups.len() as u64);
                for group in groups {
                    put_text(out, group);
                }
            }
            Refusal::Full { seats } => {
                out.push(4);
                out.push(*seats);
            }
            Refusal::Level(text) => {
                out.push(5);
                put_text(out, text);
            }
            Refusal::Plan(text) => {
                out.push(6);
                put_text(out, text);
            }
            Refusal::Timeout => out.push(7),
            Refusal::Kicked(text) => {
                out.push(8);
                put_text(out, text);
            }
            Refusal::HostQuit => out.push(9),
            Refusal::Left => out.push(10),
            Refusal::Password { given } => {
                out.push(11);
                out.push(u8::from(*given));
            }
            Refusal::InUse => out.push(12),
        }
    }

    /// Reads a refusal, refusing texts and groups over the limits.
    pub fn decode(input: &mut Reader<'_>) -> Result<Refusal, WireError> {
        Ok(match input.u8("refusal")? {
            1 => Refusal::Protocol {
                host: input.u16("host version")?,
                here: input.u16("client version")?,
            },
            2 => Refusal::Build {
                host: input.text("host build")?,
                here: input.text("client build")?,
            },
            3 => {
                let game = input.text("game")?;
                let count = input.count("groups", MAX_GROUPS)?;
                let mut groups = Vec::with_capacity(count);
                for _ in 0..count {
                    groups.push(input.text("group")?);
                }
                Refusal::GameData { game, groups }
            }
            4 => Refusal::Full {
                seats: input.u8("seats")?,
            },
            5 => Refusal::Level(input.text("level")?),
            6 => Refusal::Plan(input.text("plan")?),
            7 => Refusal::Timeout,
            8 => Refusal::Kicked(input.text("reason")?),
            9 => Refusal::HostQuit,
            10 => Refusal::Left,
            11 => Refusal::Password {
                given: match input.u8("password given")? {
                    0 => false,
                    1 => true,
                    _ => {
                        return Err(WireError::Invalid {
                            what: "password given",
                        });
                    }
                },
            },
            12 => Refusal::InUse,
            got => {
                return Err(WireError::Kind {
                    what: "refusal",
                    got,
                });
            }
        })
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Protocol { host, here } => write!(
                f,
                "the host speaks network protocol {host}, this build speaks {here}"
            ),
            Refusal::Build { host, here } => {
                write!(f, "the host runs build {host}, this is build {here}")
            }
            Refusal::GameData { game, groups } => {
                // A group is a folder, or a file at the data's root.
                let named: Vec<String> = groups
                    .iter()
                    .map(|group| {
                        if group.contains('.') {
                            group.clone()
                        } else {
                            format!("{group} folder")
                        }
                    })
                    .collect();
                let Some((last, rest)) = named.split_last() else {
                    return write!(f, "your {game} data differs from the host's");
                };
                let list = if rest.is_empty() {
                    last.clone()
                } else {
                    format!("{} and {last}", rest.join(", "))
                };
                let verb = if rest.is_empty() { "differs" } else { "differ" };
                write!(f, "your {game} {list} {verb} from the host's")
            }
            Refusal::Full { seats } => write!(f, "the host's {seats} seats are all taken"),
            Refusal::Level(text) => {
                write!(f, "this machine builds another world than the host: {text}")
            }
            Refusal::Plan(text) => write!(f, "this machine cannot build the host's level: {text}"),
            Refusal::Timeout => write!(f, "no answer from the other side in time"),
            Refusal::Kicked(text) => write!(f, "removed by the host: {text}"),
            Refusal::HostQuit => write!(f, "the host ended the session"),
            Refusal::Left => write!(f, "the player left"),
            Refusal::Password { given: true } => write!(f, "wrong password"),
            Refusal::Password { given: false } => {
                write!(f, "the host asks for a password, and none was given")
            }
            Refusal::InUse => write!(
                f,
                "this player's seat is still played by a connection the host hears from"
            ),
        }
    }
}

/// A connection-less packet (section 9 of the design).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handshake {
    /// Client to host, padded: "I would like to join".
    Hello { nonce: u64 },
    /// Host to client: a cookie that proves the client receives at its
    /// address, valid until `expiry` (host time, microseconds).
    Challenge {
        nonce: u64,
        expiry: u64,
        cookie: [u8; COOKIE_LEN],
    },
    /// Client to host, padded: the cookie back, the proof that it knows
    /// the host's join password (over the cookie: the password itself is
    /// never sent), and who it is.
    Connect {
        nonce: u64,
        expiry: u64,
        cookie: [u8; COOKIE_LEN],
        proof: Option<[u8; PROOF_LEN]>,
        credentials: Credentials,
    },
    /// Host to client: in, with this session id.
    Accept { nonce: u64, session: u64 },
    /// Host to client: not in, and why.
    Deny { nonce: u64, refusal: Refusal },
}

impl Handshake {
    /// The client's nonce, which every handshake packet carries.
    pub fn nonce(&self) -> u64 {
        match self {
            Handshake::Hello { nonce }
            | Handshake::Challenge { nonce, .. }
            | Handshake::Connect { nonce, .. }
            | Handshake::Accept { nonce, .. }
            | Handshake::Deny { nonce, .. } => *nonce,
        }
    }

    /// Whether this packet is padded to `MAX_DATAGRAM` (the requests).
    pub fn padded(&self) -> bool {
        matches!(self, Handshake::Hello { .. } | Handshake::Connect { .. })
    }

    /// The packet's bytes, `Hello` and `Connect` padded to
    /// `MAX_DATAGRAM`. Fails only when credentials are over their limits
    /// or do not fit one datagram.
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        self.encode_as(PROTOCOL_VERSION)
    }

    /// `encode` with another protocol version in the prefix (probes play
    /// an old client with it).
    pub fn encode_as(&self, version: u16) -> Result<Vec<u8>, WireError> {
        let mut out = Vec::with_capacity(MAX_DATAGRAM);
        out.extend_from_slice(&MAGIC);
        put_u16(&mut out, version);
        match self {
            Handshake::Hello { nonce } => {
                out.push(kind::HELLO);
                put_u64(&mut out, *nonce);
            }
            Handshake::Challenge {
                nonce,
                expiry,
                cookie,
            } => {
                out.push(kind::CHALLENGE);
                put_u64(&mut out, *nonce);
                put_u64(&mut out, *expiry);
                out.extend_from_slice(cookie);
            }
            Handshake::Connect {
                nonce,
                expiry,
                cookie,
                proof,
                credentials,
            } => {
                out.push(kind::CONNECT);
                put_u64(&mut out, *nonce);
                put_u64(&mut out, *expiry);
                out.extend_from_slice(cookie);
                match proof {
                    Some(proof) => {
                        out.push(1);
                        out.extend_from_slice(proof);
                    }
                    None => out.push(0),
                }
                credentials.encode(&mut out)?;
            }
            Handshake::Accept { nonce, session } => {
                out.push(kind::ACCEPT);
                put_u64(&mut out, *nonce);
                put_u64(&mut out, *session);
            }
            Handshake::Deny { nonce, refusal } => {
                out.push(kind::DENY);
                put_u64(&mut out, *nonce);
                refusal.encode(&mut out);
            }
        }
        if out.len() > MAX_DATAGRAM {
            return Err(WireError::Oversized { len: out.len() });
        }
        if self.padded() {
            out.resize(MAX_DATAGRAM, 0);
        }
        Ok(out)
    }

    /// Decodes a handshake packet of this protocol version. A packet of
    /// another version is `WireError::Version` (read its prefix with
    /// `peek_prefix`); requests must be exactly `MAX_DATAGRAM` long, and
    /// their padding is not read.
    pub fn decode(bytes: &[u8]) -> Result<Handshake, WireError> {
        let prefix = peek_prefix(bytes)?;
        if prefix.version != PROTOCOL_VERSION {
            return Err(WireError::Version {
                got: prefix.version,
            });
        }
        let nonce = prefix.nonce;
        let mut input = Reader::new(&bytes[HANDSHAKE_PREFIX_LEN..]);
        let padded = matches!(prefix.kind, kind::HELLO | kind::CONNECT);
        if padded && bytes.len() != MAX_DATAGRAM {
            return Err(WireError::Unpadded { len: bytes.len() });
        }
        let packet = match prefix.kind {
            kind::HELLO => Handshake::Hello { nonce },
            kind::CHALLENGE => Handshake::Challenge {
                nonce,
                expiry: input.u64("expiry")?,
                cookie: input.array("cookie")?,
            },
            kind::CONNECT => Handshake::Connect {
                nonce,
                expiry: input.u64("expiry")?,
                cookie: input.array("cookie")?,
                proof: match input.u8("proof flag")? {
                    0 => None,
                    1 => Some(input.array("proof")?),
                    _ => return Err(WireError::Invalid { what: "proof flag" }),
                },
                credentials: Credentials::decode(&mut input)?,
            },
            kind::ACCEPT => Handshake::Accept {
                nonce,
                session: input.u64("session")?,
            },
            kind::DENY => Handshake::Deny {
                nonce,
                refusal: Refusal::decode(&mut input)?,
            },
            got => {
                return Err(WireError::Kind {
                    what: "handshake packet",
                    got,
                });
            }
        };
        if !padded {
            input.finish()?;
        }
        Ok(packet)
    }
}

/// The frozen first 15 bytes of a handshake packet of any version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefix {
    pub version: u16,
    pub kind: u8,
    pub nonce: u64,
}

impl Prefix {
    /// Whether the packet is a request (`Hello` or `Connect`), which a
    /// host of any version may answer with `Refusal::Protocol`.
    pub fn is_request(&self) -> bool {
        matches!(self.kind, kind::HELLO | kind::CONNECT)
    }

    /// Whether the packet is a `Hello` (rate limited).
    pub fn is_hello(&self) -> bool {
        self.kind == kind::HELLO
    }
}

/// Reads the frozen prefix (magic, version, type, nonce).
pub fn peek_prefix(bytes: &[u8]) -> Result<Prefix, WireError> {
    if bytes.len() > MAX_DATAGRAM {
        return Err(WireError::Oversized { len: bytes.len() });
    }
    let mut input = Reader::new(bytes);
    if input.array::<4>("magic")? != MAGIC {
        return Err(WireError::Magic);
    }
    Ok(Prefix {
        version: input.u16("version")?,
        kind: input.u8("handshake type")?,
        nonce: input.u64("nonce")?,
    })
}
