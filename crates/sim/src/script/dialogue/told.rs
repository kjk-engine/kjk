//! What everyone sees of the conversations (`Told`), and its encoding.

use super::*;

/// The conversations, barks and custom tokens as everyone sees them: what
/// a snapshot carries for views and for holding a talking player still.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Told {
    pub conversations: Vec<Conversation>,
    pub barks: Vec<Bark>,
    pub tokens: BTreeMap<i32, String>,
}

impl Told {
    /// The conversation `body` stands in (listener or joined), if any.
    pub fn holding(&self, body: ObjectId) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.holds(body))
    }

    /// The conversation holding every seat, if any (R1).
    pub fn party(&self) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.party)
    }

    /// The conversation under way `who` takes part in, if any, as the
    /// scripts' `Talk::with` has it (`host` the host's two bodies,
    /// `world::host_bodies`).
    pub fn with(&self, who: ObjectId, host: [ObjectId; 2]) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.takes_part(who, host))
    }

    /// Whether a conversation holds the world (S1): one holds every seat
    /// (`party`), or each of the seats' bodies (`seat_bodies`, at least
    /// one) stands in one.
    pub fn holds_world(&self, seat_bodies: &[ObjectId]) -> bool {
        holds_world(&self.conversations, seat_bodies)
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        encode_told(out, &self.conversations, &self.barks, &self.tokens);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Told, DecodeError> {
        let mut told = Told::default();
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let dialog = input.string()?;
            let (owner, pc, asker) = (
                ObjectId(input.u32()?),
                ObjectId(input.u32()?),
                ObjectId(input.u32()?),
            );
            let entry = input.u32()?;
            let speaker = ObjectId(input.u32()?);
            let replies: Vec<u32> = (0..input.u32()?)
                .map(|_| input.u32())
                .collect::<Result<_, _>>()?;
            let (shown_ms, until_ms) = (input.u64()?, input.u64()?);
            let flags = input.u8()?;
            // By seat, one each, below `MAX_SEATS`, at a reply offered: one
            // state, one encoding.
            let mut marks = BTreeMap::new();
            for _ in 0..input.u8()? {
                let at = input.at();
                let (seat, nth) = (input.u8()?, input.u8()?);
                let ordered = marks.last_key_value().is_none_or(|(last, _)| *last < seat);
                if !ordered || seat >= crate::MAX_SEATS || usize::from(nth) >= replies.len() {
                    return Err(input.invalid("reply mark", at));
                }
                marks.insert(seat, nth);
            }
            let camera = input.u32()? as i32;
            let listener = ObjectId(input.u32()?);
            let at = input.at();
            let count = input.u8()?;
            if count > crate::MAX_SEATS {
                return Err(input.invalid("joined", at));
            }
            let joined = (0..count)
                .map(|_| input.u32().map(ObjectId))
                .collect::<Result<_, _>>()?;
            let (asking, waits_for) = (ObjectId(input.u32()?), ObjectId(input.u32()?));
            let dealt = ObjectId(input.u32()?);
            let (angle, cursor, step) = (input.u8()?, input.i8()?, input.u8()?);
            let table = input.u8()?;
            let kept = |input: &mut Reader| {
                (0..input.u32()?)
                    .map(|_| Ok((input.u16()?, input.u8()?)))
                    .collect::<Result<Vec<_>, DecodeError>>()
            };
            let (angles, voices) = (kept(input)?, kept(input)?);
            let voice_cursor = input.i8()?;
            let listener_now = ObjectId(input.u32()?);
            let pair = (ObjectId(input.u32()?), ObjectId(input.u32()?));
            let (lock_turn, lock_head) = (bodies(input)?, bodies(input)?);
            told.conversations.push(Conversation {
                id,
                dialog,
                owner,
                pc,
                asker,
                entry,
                speaker,
                replies,
                shown_ms,
                until_ms,
                auto: flags & 1 != 0,
                paused: flags & 2 != 0,
                marks,
                camera,
                listener,
                joined,
                party: flags & 4 != 0,
                locked: flags & 8 != 0,
                asking,
                waits_for,
                dealt,
                angle,
                cursor,
                step,
                forward: table & 1 != 0,
                defaults_seen: table & 2 != 0,
                angles,
                voice_cursor,
                voices,
                listener_now,
                pair,
                lock_turn,
                lock_head,
            });
        }
        for _ in 0..input.u32()? {
            let who = ObjectId(input.u32()?);
            let at = input.at();
            let text = match input.u8()? {
                0 => Said::Entry {
                    dialog: input.string()?,
                    entry: input.u32()?,
                },
                1 => Said::StrRef(input.u32()?),
                _ => return Err(input.invalid("bark", at)),
            };
            told.barks.push(Bark {
                who,
                text,
                until_ms: input.u64()?,
            });
        }
        for _ in 0..input.u32()? {
            told.tokens.insert(input.u32()? as i32, input.string()?);
        }
        Ok(told)
    }
}

/// A list of bodies as `encode_told` writes it.
pub(super) fn bodies(input: &mut Reader) -> Result<Vec<ObjectId>, DecodeError> {
    (0..input.u32()?)
        .map(|_| input.u32().map(ObjectId))
        .collect()
}

pub(super) fn encode_told(
    out: &mut Writer,
    conversations: &[Conversation],
    barks: &[Bark],
    tokens: &BTreeMap<i32, String>,
) {
    out.u32(conversations.len() as u32);
    for c in conversations {
        out.u32(c.id);
        out.string(&c.dialog);
        for id in [c.owner, c.pc, c.asker] {
            out.u32(id.0);
        }
        out.u32(c.entry);
        out.u32(c.speaker.0);
        out.u32(c.replies.len() as u32);
        for reply in &c.replies {
            out.u32(*reply);
        }
        out.u64(c.shown_ms);
        out.u64(c.until_ms);
        out.u8(u8::from(c.auto)
            | u8::from(c.paused) << 1
            | u8::from(c.party) << 2
            | u8::from(c.locked) << 3);
        out.u8(c.marks.len() as u8);
        for (seat, nth) in &c.marks {
            out.u8(*seat);
            out.u8(*nth);
        }
        out.u32(c.camera as u32);
        out.u32(c.listener.0);
        out.u8(c.joined.len() as u8);
        for body in &c.joined {
            out.u32(body.0);
        }
        out.u32(c.asking.0);
        out.u32(c.waits_for.0);
        out.u32(c.dealt.0);
        out.u8(c.angle);
        out.i8(c.cursor);
        out.u8(c.step);
        out.u8(u8::from(c.forward) | u8::from(c.defaults_seen) << 1);
        for kept in [&c.angles, &c.voices] {
            out.u32(kept.len() as u32);
            for (node, value) in kept {
                out.u16(*node);
                out.u8(*value);
            }
        }
        out.i8(c.voice_cursor);
        out.u32(c.listener_now.0);
        out.u32(c.pair.0.0);
        out.u32(c.pair.1.0);
        for list in [&c.lock_turn, &c.lock_head] {
            out.u32(list.len() as u32);
            for body in list {
                out.u32(body.0);
            }
        }
    }
    out.u32(barks.len() as u32);
    for bark in barks {
        out.u32(bark.who.0);
        match &bark.text {
            Said::Entry { dialog, entry } => {
                out.u8(0);
                out.string(dialog);
                out.u32(*entry);
            }
            Said::StrRef(strref) => {
                out.u8(1);
                out.u32(*strref);
            }
        }
        out.u64(bark.until_ms);
    }
    out.u32(tokens.len() as u32);
    for (n, value) in tokens {
        out.u32(*n as u32);
        out.string(value);
    }
}
