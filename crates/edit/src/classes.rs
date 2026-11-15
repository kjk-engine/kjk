//! A character's classes and a companion's influence changed.
//!
//! Changing a class trades every level of one class for the same number of
//! levels of another (`levels::Classes::swapped`). Adding a class is what a
//! script's `AddMultiClass` does: the class joins the career with no levels
//! yet, and the character's next level-up in kjk can be of that class.

use sim::levels::Career;
use sim::script::party::{INFLUENCE_SLOTS, MAX_INFLUENCE};

use crate::keeping::Keeping;
use crate::{Editor, Who};

impl Editor<'_> {
    /// `who`'s levels in class `from` (`classes.2da` row) made levels in
    /// class `to`, with the sheet regrown as `levels::Classes::swapped`
    /// regrows it. A character with no hit or Force points set keeps its
    /// current points, held within its new most.
    pub fn change_class(&mut self, who: &Who, from: u8, to: u8) -> Result<(), String> {
        let classes = self
            .classes
            .ok_or("changing a class needs the game's class tables (an install)")?;
        let keeping = self.keeping(who)?;
        let career = self.career_of(who, &keeping)?;
        if !career.iter().any(|(class, _)| *class == from) {
            return Err(format!("the character has no levels in class {from}"));
        }
        if career.iter().any(|(class, _)| *class == to) {
            return Err(format!("the character already has class {to}"));
        }
        let (sheet, career) = classes
            .swapped(&keeping.sheet, &career, from, to)
            .ok_or(format!("class {from} or class {to} is not in the game"))?;
        self.save.script.careers.insert(keeping.holder, career);
        let (hp, fp) = self.points_of(&keeping);
        let points = crate::keeping::Points {
            hp: hp.min(i32::from(sheet.max_hp) * 100),
            fp: fp.min(i32::from(sheet.pool()) * 100),
            set: false,
        };
        self.keep(&keeping, sheet, Some(points), false)
    }

    /// Class `class` added to `who`'s career with no levels in it, as a
    /// script's `AddMultiClass` adds it. The character takes levels in it
    /// at its next level-up in kjk.
    pub fn add_class(&mut self, who: &Who, class: u8) -> Result<(), String> {
        if let Some(classes) = self.classes
            && usize::from(class) >= classes.classes.len()
        {
            return Err(format!("class {class} is not in the game"));
        }
        let keeping = self.keeping(who)?;
        let mut career = self.career_of(who, &keeping)?;
        if career.iter().any(|(had, _)| *had == class) {
            return Err(format!("the character already has class {class}"));
        }
        if career.len() >= 2 {
            return Err("a character has two classes at most".into());
        }
        career.push((class, 0));
        self.save.script.careers.insert(keeping.holder, career);
        Ok(())
    }

    /// Companion `npc`'s influence (TSL only), 0 to the most the game
    /// allows.
    pub fn set_influence(&mut self, npc: u8, influence: u8) -> Result<(), String> {
        if npc >= INFLUENCE_SLOTS {
            return Err(format!(
                "companion {npc} has no influence: only companions 0 to {} do",
                INFLUENCE_SLOTS - 1
            ));
        }
        if influence > MAX_INFLUENCE {
            return Err(format!(
                "influence {influence}: it runs 0 to {MAX_INFLUENCE}"
            ));
        }
        self.save.script.party.influence.insert(npc, influence);
        Ok(())
    }

    /// `who`'s career (`characters::career`), refused when the save does
    /// not record it.
    fn career_of(&self, who: &Who, keeping: &Keeping) -> Result<Career, String> {
        let npc = match *who {
            Who::Companion { npc, .. } | Who::Away { npc } => Some(npc),
            Who::Player(_) => None,
        };
        let career = crate::characters::career(self.save, keeping.holder, npc, keeping.sheet.level);
        if career.is_empty() {
            return Err("the save does not record this character's class".into());
        }
        Ok(career)
    }
}
