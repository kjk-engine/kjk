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
    /// current points, held within its new most. A player whose first class
    /// changes takes the body of the new class's build (`rebody`).
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
        let first_changed = career.first().map(|(class, _)| *class) == Some(from);
        let (sheet, career) = classes
            .swapped(&keeping.sheet, &career, from, to)
            .ok_or(format!("class {from} or class {to} is not in the game"))?;
        self.save.script.careers.insert(keeping.holder, career);
        if first_changed && let Who::Player(seat) = *who {
            self.rebody(seat, to);
        }
        let (hp, fp) = self.points_of(&keeping);
        let points = crate::keeping::Points {
            hp: hp.min(i32::from(sheet.max_hp) * 100),
            fp: fp.min(i32::from(sheet.pool()) * 100),
            set: false,
        };
        self.keep(&keeping, sheet, Some(points), false)
    }

    /// `who` taken up to `levels` levels in class `class`, one level at a
    /// time with the game's own picks for each (`levels::Classes::auto`),
    /// as TSL levels a companion on its own. The party's XP is raised to
    /// what the character's new total level needs, if it is below. Levels
    /// are never taken away: a lower number is refused.
    pub fn set_class_levels(&mut self, who: &Who, class: u8, levels: u8) -> Result<(), String> {
        let classes = self
            .classes
            .ok_or("changing levels needs the game's class tables (an install)")?;
        let keeping = self.keeping(who)?;
        let mut career = self.career_of(who, &keeping)?;
        let had = career
            .iter()
            .find(|(of, _)| *of == class)
            .map(|(_, levels)| *levels)
            .ok_or(format!("the character has no class {class}"))?;
        if levels < had {
            return Err(format!(
                "{levels} levels: a level taken cannot be taken away (it has {had})"
            ));
        }
        let mut sheet = keeping.sheet;
        let unspent = &mut self.save.script.unspent;
        let mut banked = unspent.get(&keeping.holder).copied().unwrap_or(0);
        for _ in had..levels {
            let choices = classes.auto(&sheet, &career, class, banked).ok_or(format!(
                "class {class} offers no level past {}",
                sheet.level
            ))?;
            let (after, grown, left) = classes
                .taken(&sheet, &career, &choices, banked)
                .map_err(|why| format!("level {} refused: {why:?}", sheet.level + 1))?;
            (sheet, career, banked) = (after, grown, left);
        }
        if banked > 0 {
            unspent.insert(keeping.holder, banked);
        } else {
            unspent.remove(&keeping.holder);
        }
        self.save.script.careers.insert(keeping.holder, career);
        let needed = classes.next(sheet.level.saturating_sub(1)).unwrap_or(0);
        let party = sim::inventory::PARTY;
        if self.save.script.xp(party) < needed {
            self.set_party_xp(needed)?;
        }
        let (hp, fp) = self.points_of(&keeping);
        let gained = |now: u16, was: u16| (i32::from(now) - i32::from(was)) * 100;
        let points = crate::keeping::Points {
            hp: hp + gained(sheet.max_hp, keeping.sheet.max_hp),
            fp: fp + gained(sheet.pool(), keeping.sheet.pool()),
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
    pub(crate) fn career_of(&self, who: &Who, keeping: &Keeping) -> Result<Career, String> {
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
