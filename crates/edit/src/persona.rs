//! A player's head preset changed: another player portrait of the same
//! gender, worn with the body its first class takes. The save keeps the
//! head in the seat's persona (`sim::persona`), which the 3D model, the
//! portrait and the scripts all read. A disguise is an effect over it, so
//! a disguised player shows the disguise until it ends.

use sim::Seat;
use sim::persona::Persona;

use crate::{Editor, Who};

impl Editor<'_> {
    /// Player `who`'s head and body made those of player portrait `row`
    /// (`portraits.2da`), the body of the build its first class takes. A
    /// portrait of the other gender is refused, as is a companion: its look
    /// comes from its own creature template.
    pub fn set_portrait(&mut self, who: &Who, row: u16) -> Result<(), String> {
        let Who::Player(seat) = *who else {
            return Err("only a player's head can be changed: a companion's look comes from its own template".into());
        };
        let portraits = self
            .portraits
            .ok_or("changing a head needs the game's portraits (an install)")?;
        let persona = self.persona(seat)?;
        let portrait = portraits
            .at(row)
            .ok_or(format!("portrait {row} is not a player portrait"))?;
        if portrait.female != persona.female() {
            return Err(format!(
                "portrait {row} is of the other gender: scripts read the player's gender, so it is kept"
            ));
        }
        let class = self.first_class(who);
        let worn = portraits.worn(&persona, row, class).ok_or(match class {
            Some(class) => format!("portrait {row} has no body for class {class}"),
            None => format!(
                "the save does not record the player's class, and portrait {row} has no body like the one worn"
            ),
        })?;
        self.put_persona(worn);
        Ok(())
    }

    /// Player `seat`'s body made the one its portrait gives `class`, when
    /// the game's portraits are at hand and its portrait is one of them.
    pub(crate) fn rebody(&mut self, seat: Seat, class: u8) {
        let Some(portraits) = self.portraits else {
            return;
        };
        let Ok(persona) = self.persona(seat) else {
            return;
        };
        if let Some(worn) = portraits.worn(&persona, persona.portrait, Some(class)) {
            self.put_persona(worn);
        }
    }

    /// `who`'s first class, when the save records it.
    fn first_class(&self, who: &Who) -> Option<u8> {
        let keeping = self.keeping(who).ok()?;
        let career = self.career_of(who, &keeping).ok()?;
        career.first().map(|(class, _)| *class)
    }

    fn persona(&self, seat: Seat) -> Result<Persona, String> {
        sim::persona::of(&self.save.snapshot.personas, seat)
            .cloned()
            .ok_or(format!("player {} has no head in the save", seat.0 + 1))
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
