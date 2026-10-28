//! Who each seat's player is: the character made in character creation
//! (or, without one, the default the host gives), as the game's own
//! player creature holds it. The scripts read it of a player
//! (`GetGender`, `GetName`, `GetAppearanceType`, `GetPortraitId`), the
//! view draws the player's body from it (`appearance.2da` row, dressed by
//! the body armour worn), and the screens show its portrait and name.
//!
//! It rides in the snapshot's tail (`Snapshot::personas`), so a guest
//! draws every seat as its host does and a save keeps it; it changes only
//! where the session starts or a seat is given one (`SimWorld::set_persona`).

use crate::input::Seat;
use crate::wire::{DecodeError, Reader, Writer};

/// `GENDER_MALE`, `GENDER_FEMALE` (`gender.2da` rows).
pub const MALE: u8 = 0;
pub const FEMALE: u8 = 1;
/// The last `gender.2da` row (`GENDER_NONE`).
const GENDERS: u8 = 4;

/// One seat's player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persona {
    pub seat: Seat,
    /// `appearance.2da` row: the body and head (a player portrait's body
    /// for its class's build, `portraits.2da`).
    pub appearance: u16,
    /// `gender.2da` row (`MALE`, `FEMALE`).
    pub gender: u8,
    /// `portraits.2da` row.
    pub portrait: u16,
    /// Its name as the game shows it (`GetName`); empty for none.
    pub name: String,
}

impl Persona {
    pub fn female(&self) -> bool {
        self.gender == FEMALE
    }

    fn encode(&self, out: &mut Writer) {
        out.u8(self.seat.0);
        out.u16(self.appearance);
        out.u8(self.gender);
        out.u16(self.portrait);
        out.string(&self.name);
    }
}

/// The personas by seat, one each, below `MAX_SEATS`: one state, one
/// encoding.
pub(crate) fn encode(personas: &[Persona], out: &mut Writer) {
    out.u8(u8::try_from(personas.len()).expect("fewer personas than seats"));
    for persona in personas {
        persona.encode(out);
    }
}

pub(crate) fn decode(input: &mut Reader) -> Result<Vec<Persona>, DecodeError> {
    let mut personas: Vec<Persona> = Vec::new();
    for _ in 0..input.u8()? {
        let at = input.at();
        let seat = Seat(input.u8()?);
        if seat.0 >= crate::MAX_SEATS || personas.last().is_some_and(|last| last.seat >= seat) {
            return Err(input.invalid("persona seat", at));
        }
        let appearance = input.u16()?;
        let at = input.at();
        let gender = input.u8()?;
        if gender > GENDERS {
            return Err(input.invalid("persona gender", at));
        }
        personas.push(Persona {
            seat,
            appearance,
            gender,
            portrait: input.u16()?,
            name: input.string()?,
        });
    }
    Ok(personas)
}

/// `personas` with `persona` in its seat's place, kept in seat order.
pub(crate) fn put(personas: &mut Vec<Persona>, persona: Persona) {
    match personas.binary_search_by_key(&persona.seat, |held| held.seat) {
        Ok(at) => personas[at] = persona,
        Err(at) => personas.insert(at, persona),
    }
}

/// Seat `seat`'s persona, if it has one.
pub fn of(personas: &[Persona], seat: Seat) -> Option<&Persona> {
    personas
        .binary_search_by_key(&seat, |held| held.seat)
        .ok()
        .map(|at| &personas[at])
}

/// What seat `seat`'s body is dressed as in `snapshot`: its persona's
/// appearance and the template of the body armour its own character wears
/// (none: its underclothes), if it has a persona and a character.
pub fn dressed(snapshot: &crate::Snapshot, seat: Seat) -> Option<(u16, Option<String>)> {
    let persona = of(&snapshot.personas, seat)?;
    let own = snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == crate::Kind::Avatar(seat))?
        .id;
    let armour = snapshot.worn(own.0, crate::inventory::slots::BODY);
    Some((persona.appearance, armour.map(str::to_string)))
}
