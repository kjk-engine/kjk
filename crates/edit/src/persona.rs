//! A player's head preset is changed to another player portrait of the same
//! gender. The player wears that portrait's head and the body that their
//! first class takes. The save keeps the head in the seat's persona
//! (`sim::persona`), which the 3D model, the portrait and the scripts all
//! read. A disguise is an effect laid over the persona, so a disguised
//! player shows the disguise until it ends.

use session::portraits::Refusal;
use sim::Seat;
use sim::persona::Persona;

use crate::{Editor, Who};

impl Editor<'_> {
    /// Player `who` given the head and body of player portrait `row`
    /// (`portraits.2da`). The body is of the build that the player's first
    /// class takes. A portrait of the other gender is refused, because
    /// scripts read the player's gender for romances and some dialogue. A
    /// companion is refused, because its look comes from its own template.
    pub fn set_portrait(&mut self, who: &Who, row: u16) -> Result<(), String> {
        let Who::Player(seat) = *who else {
            return Err(
                "Only a player's head can be changed. A companion's look comes from its own template."
                    .into(),
            );
        };
        let portraits = self
            .portraits
            .ok_or("Changing a head needs the game's portraits, and no game install was given.")?;
        let persona = self.persona(seat)?;
        let class = self.first_class(who);
        let worn = portraits
            .worn(&persona, row, class)
            .map_err(|refusal| match refusal {
                Refusal::NotAPlayerPortrait => format!("Portrait {row} is not a player portrait."),
                Refusal::OtherGender => format!(
                    "Portrait {row} is of the other gender. Scripts read the player's gender, so it is kept."
                ),
                Refusal::NoBody => format!(
                    "Portrait {row} has no body for the build of the player's first class."
                ),
                Refusal::UnknownBuild => format!(
                    "The save does not record the player's class, and the body worn now is not one of the portrait's bodies, so the body for portrait {row} is unknown."
                ),
            })?;
        self.put_persona(worn);
        Ok(())
    }

    /// Why player `who`'s body may not be the one their first class takes,
    /// or none when it is. A change of first class leaves the body as it
    /// was when the game's portraits are missing, or when the player's
    /// portrait is not a player portrait.
    pub fn body_warning(&self, who: &Who) -> Option<String> {
        let Who::Player(seat) = *who else {
            return None;
        };
        let persona = self.persona(seat).ok()?;
        let class = self.first_class(who)?;
        let Some(portraits) = self.portraits else {
            return Some(
                "The game's portraits did not read, so the player's body was not changed to the new class's build."
                    .into(),
            );
        };
        match portraits.worn(&persona, persona.portrait, Some(class)) {
            Ok(dressed) if dressed == persona => None,
            Ok(_) => Some("The player's body is not the one their first class takes.".into()),
            Err(_) => Some(format!(
                "The player's portrait ({}) is not a player portrait, so the body was not changed to the new class's build.",
                persona.portrait
            )),
        }
    }

    /// Player `seat` given the body that their portrait gives `class`, when
    /// the game's portraits are at hand and their portrait is one of them.
    pub(crate) fn rebody(&mut self, seat: Seat, class: u8) {
        let Some(portraits) = self.portraits else {
            return;
        };
        let Ok(persona) = self.persona(seat) else {
            return;
        };
        if let Ok(dressed) = portraits.worn(&persona, persona.portrait, Some(class)) {
            self.put_persona(dressed);
        }
    }

    fn first_class(&self, who: &Who) -> Option<u8> {
        let keeping = self.keeping(who).ok()?;
        let career = self.career_of(who, &keeping).ok()?;
        career.first().map(|(class, _)| *class)
    }

    fn persona(&self, seat: Seat) -> Result<Persona, String> {
        sim::persona::of(&self.save.snapshot.personas, seat)
            .cloned()
            .ok_or(format!("Player {} has no head in the save.", seat.0 + 1))
    }

    fn put_persona(&mut self, persona: Persona) {
        if let Some(held) = self
            .save
            .snapshot
            .personas
            .iter_mut()
            .find(|held| held.seat == persona.seat)
        {
            *held = persona;
        }
    }
}
