//! The co-op lobby's state on the wire (the `Lobby` message). The host sends
//! the whole state on every change, so a guest never merges anything.
//!
//! ```text
//! lobby     = flags u8 (1: a save is chosen), [summary],
//!             saber rules text, collision u8, show replies u8 (0 off, 1 on),
//!             count u8, companion*, count u8, npc u8*,
//!             slot*   (exactly SLOTS of them)
//! summary   = save bytes, area bytes, game u8 (0 KOTOR, 1 KOTOR 2),
//!             saved varint, hero bytes, class bytes, level u8,
//!             portrait text
//! companion = npc u8, name bytes, portrait text, class bytes, level u8,
//!             choice u8 (0 open, 1 not available, 2 required)
//! slot      = 0 (empty), or 1, name text, playing u8 (0 the main
//!             character, 1 a companion then its npc u8, 2 a droid),
//!             ready u8, delay u16
//! bytes     = varint(len) then the bytes: game text in its code page
//! ```

use ::session::lobby::{Collision, Companion, Game, Lobby, Playing, SLOTS, Slot, Summary};
use sim::script::party_choice::Choice;

use crate::wire::{MAX_TEXT, Reader, WireError, put_text, put_u16, put_varint};

/// The most companions a lobby lists (KOTOR 2 has 12).
const MAX_COMPANIONS: usize = 16;

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    let bytes = &bytes[..bytes.len().min(MAX_TEXT)];
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn bytes(input: &mut Reader<'_>, what: &'static str) -> Result<Vec<u8>, WireError> {
    let len = input.count(what, MAX_TEXT)?;
    Ok(input.take(len, what)?.to_vec())
}

fn invalid(what: &'static str) -> WireError {
    WireError::Invalid { what }
}

const CHOICES: [Choice; 3] = [Choice::Open, Choice::Unavailable, Choice::Required];
const GAMES: [Game; 2] = [Game::K1, Game::Tsl];

fn code<T: PartialEq>(all: &[T], value: &T) -> u8 {
    all.iter().position(|one| one == value).unwrap_or(0) as u8
}

fn of_code<T: Copy>(all: &[T], code: u8, what: &'static str) -> Result<T, WireError> {
    all.get(usize::from(code)).copied().ok_or(invalid(what))
}

/// Appends `lobby`.
pub fn put_lobby(out: &mut Vec<u8>, lobby: &Lobby) {
    out.push(u8::from(lobby.summary.is_some()));
    if let Some(summary) = &lobby.summary {
        put_bytes(out, &summary.save);
        put_bytes(out, &summary.area);
        out.push(code(&GAMES, &summary.game));
        put_varint(out, summary.saved);
        put_bytes(out, &summary.hero);
        put_bytes(out, &summary.hero_class);
        out.push(summary.hero_level);
        put_text(out, &summary.hero_portrait);
    }
    put_text(out, &lobby.saber_rules);
    out.push(lobby.collision.code());
    out.push(u8::from(lobby.show_replies));
    out.push(lobby.cheats.code());
    let companions = &lobby.companions[..lobby.companions.len().min(MAX_COMPANIONS)];
    out.push(companions.len() as u8);
    for companion in companions {
        out.push(companion.npc);
        put_bytes(out, &companion.name);
        put_text(out, &companion.portrait);
        put_bytes(out, &companion.class);
        out.push(companion.level);
        out.push(code(&CHOICES, &companion.choice));
    }
    let with_party = &lobby.with_party[..lobby.with_party.len().min(MAX_COMPANIONS)];
    out.push(with_party.len() as u8);
    out.extend_from_slice(with_party);
    for slot in &lobby.slots {
        let Some(slot) = slot else {
            out.push(0);
            continue;
        };
        out.push(1);
        put_text(out, &slot.name);
        match slot.playing {
            Playing::Hero => out.push(0),
            Playing::Companion(npc) => out.extend_from_slice(&[1, npc]),
            Playing::Droid => out.push(2),
        }
        out.push(u8::from(slot.ready));
        put_u16(out, slot.delay_ms);
    }
}

fn flag(input: &mut Reader<'_>, what: &'static str) -> Result<bool, WireError> {
    match input.u8(what)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid(what)),
    }
}

/// Reads a lobby written by `put_lobby`.
pub fn read_lobby(input: &mut Reader<'_>) -> Result<Lobby, WireError> {
    let summary = if flag(input, "lobby flags")? {
        Some(Summary {
            save: bytes(input, "save name")?,
            area: bytes(input, "area name")?,
            game: of_code(&GAMES, input.u8("game")?, "game")?,
            saved: input.varint("saved")?,
            hero: bytes(input, "hero name")?,
            hero_class: bytes(input, "hero class")?,
            hero_level: input.u8("hero level")?,
            hero_portrait: input.text("hero portrait")?,
        })
    } else {
        None
    };
    let saber_rules = input.text("saber rules")?;
    let collision = Collision::from_code(input.u8("collision")?).ok_or(invalid("collision"))?;
    let show_replies = flag(input, "show replies")?;
    let cheats = sim::cheats::Rule::from_code(input.u8("cheats")?).ok_or(invalid("cheats"))?;
    let count = input.count("companions", MAX_COMPANIONS)?;
    let companions = (0..count)
        .map(|_| {
            Ok(Companion {
                npc: input.u8("npc")?,
                name: bytes(input, "companion name")?,
                portrait: input.text("portrait")?,
                class: bytes(input, "companion class")?,
                level: input.u8("level")?,
                choice: of_code(&CHOICES, input.u8("choice")?, "choice")?,
            })
        })
        .collect::<Result<Vec<_>, WireError>>()?;
    let count = input.count("with party", MAX_COMPANIONS)?;
    let with_party = input.take(count, "with party")?.to_vec();
    let mut slots: [Option<Slot>; SLOTS] = Default::default();
    for slot in &mut slots {
        if !flag(input, "slot")? {
            continue;
        }
        let name = input.text("player name")?;
        let playing = match input.u8("playing")? {
            0 => Playing::Hero,
            1 => Playing::Companion(input.u8("npc")?),
            2 => Playing::Droid,
            _ => return Err(invalid("playing")),
        };
        *slot = Some(Slot {
            name,
            playing,
            ready: flag(input, "ready")?,
            delay_ms: input.u16("delay")?,
        });
    }
    Ok(Lobby {
        summary,
        saber_rules,
        collision,
        show_replies,
        cheats,
        companions,
        with_party,
        slots,
    })
}
