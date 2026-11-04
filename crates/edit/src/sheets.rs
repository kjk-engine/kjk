//! A character's own numbers changed: feats and powers by kjk's learning
//! rule (`combat::passive`), abilities (a Constitution moving the hit
//! points as effects move them), skills, alignment, hit and Force points,
//! XP. Each is written where the save keeps the character: its body's
//! sheet and fighter, or the party table's for a companion away.

use sim::combat::Fighter;
use sim::combat::effects::constitution_moved;
use sim::combat::passive;
use sim::combat::sheet::{Rows, Sheet};

use crate::characters::points;
use crate::{Editor, Who};

/// A sheet and its hit and Force points (hundredths), as an edit changes
/// them.
struct Body {
    sheet: Sheet,
    hp: i32,
    fp: i32,
}

/// The most alignment (`Sheet::good_evil`): all light.
const LIGHTEST: u8 = 100;

impl Editor<'_> {
    /// `feat` known by `who`, with what learning it gives (Toughness's HP
    /// for every level, Force Sensitive's FP), as TSL's `GrantFeat` gives
    /// it: whatever its prerequisites (`feat_warning` tells them).
    pub fn grant_feat(&mut self, who: &Who, feat: u16) -> Result<(), String> {
        within::<4>("feat", feat)?;
        self.change(who, |body| {
            if body.sheet.feats.has(feat) {
                return Err(format!("feat {feat} is known already"));
            }
            body.sheet.feats.insert(feat);
            let levels = body.sheet.level;
            passive::learn(&mut body.sheet, feat, levels);
            Ok(())
        })
    }

    /// `feat` forgotten by `who`, what learning it gave taken back.
    pub fn remove_feat(&mut self, who: &Who, feat: u16) -> Result<(), String> {
        within::<4>("feat", feat)?;
        self.change(who, |body| {
            if !body.sheet.feats.has(feat) {
                return Err(format!("feat {feat} is not known"));
            }
            body.sheet.feats.remove(feat);
            let levels = body.sheet.level;
            passive::unlearn(&mut body.sheet, feat, levels);
            Ok(())
        })
    }

    /// Power `row` (`spells.2da`) known by `who`, as a level's pick adds
    /// it: whatever its prerequisites (`power_warning` tells them).
    pub fn grant_power(&mut self, who: &Who, row: u16) -> Result<(), String> {
        within::<5>("power", row)?;
        self.change(who, |body| {
            if body.sheet.powers.has(row) {
                return Err(format!("power {row} is known already"));
            }
            body.sheet.powers.insert(row);
            Ok(())
        })
    }

    /// Power `row` forgotten by `who`.
    pub fn remove_power(&mut self, who: &Who, row: u16) -> Result<(), String> {
        within::<5>("power", row)?;
        self.change(who, |body| {
            if !body.sheet.powers.has(row) {
                return Err(format!("power {row} is not known"));
            }
            body.sheet.powers.remove(row);
            Ok(())
        })
    }

    /// `who`'s `ability` (`Ability` order) at `score`, from 1; a
    /// Constitution moved moves the most hit points as effects move them
    /// (`effects::constitution_moved`: the modifier's change a level).
    pub fn set_ability(&mut self, who: &Who, ability: usize, score: u8) -> Result<(), String> {
        if score == 0 {
            return Err("an ability score is at least 1".into());
        }
        self.change(who, |body| {
            let was = body.sheet;
            let into = body.sheet.abilities.get_mut(ability);
            *into.ok_or(format!("no ability {ability}: there are 6"))? = score;
            constitution_moved(&was, &mut body.sheet);
            Ok(())
        })
    }

    /// `who`'s ranks in skill `row` (`skills.2da`).
    pub fn set_skill(&mut self, who: &Who, row: usize, ranks: u8) -> Result<(), String> {
        self.change(who, |body| {
            let count = body.sheet.skills.len();
            *body
                .sheet
                .skills
                .get_mut(row)
                .ok_or(format!("no skill {row}: there are {count}"))? = ranks;
            Ok(())
        })
    }

    /// `who`'s alignment, 0 (all dark) to 100 (all light).
    pub fn set_alignment(&mut self, who: &Who, good_evil: u8) -> Result<(), String> {
        if good_evil > LIGHTEST {
            return Err(format!("alignment {good_evil}: it runs 0 to {LIGHTEST}"));
        }
        self.change(who, |body| {
            body.sheet.good_evil = good_evil;
            Ok(())
        })
    }

    /// `who`'s hit points (hundredths), at least one hundredth (an edit
    /// kills no one) and at most its most.
    pub fn set_hp(&mut self, who: &Who, hp: i32) -> Result<(), String> {
        self.change(who, |body| {
            body.hp = hp.max(1);
            Ok(())
        })
    }

    /// `who`'s Force points (hundredths; stamina for one without the
    /// Force), 0 to its bar's most.
    pub fn set_fp(&mut self, who: &Who, fp: i32) -> Result<(), String> {
        self.change(who, |body| {
            body.fp = fp.max(0);
            Ok(())
        })
    }

    /// `who`'s own XP (the party's is `set_party_xp`).
    pub fn set_xp(&mut self, who: &Who, xp: i32) -> Result<(), String> {
        let (holder, _, _) = self.dressed(who)?;
        self.save.script.set_xp(holder, xp);
        Ok(())
    }

    /// `edit` made to `who`'s sheet and points, the hit points then within
    /// the sheet's most (as a Constitution lost takes those over it,
    /// `effects::worn`), written where the save keeps them; nothing changes
    /// when it is refused.
    fn change(
        &mut self,
        who: &Who,
        edit: impl FnOnce(&mut Body) -> Result<(), String>,
    ) -> Result<(), String> {
        let (holder, sheet, _) = self.dressed(who)?;
        let (hp, fp) = match *who {
            Who::Away { npc } => {
                let kept = self.kept_mut(npc)?;
                (kept.hp, kept.fp)
            }
            Who::Player(_) | Who::Companion { .. } => {
                points(self.body_mut(holder)?.fighter.as_ref(), &sheet)
            }
        };
        let mut body = Body { sheet, hp, fp };
        edit(&mut body)?;
        let pointed = (body.hp, body.fp) != (hp, fp);
        let set_fp = body.fp != fp;
        let Body { sheet, hp, fp } = body;
        let hp = hp.min(i32::from(sheet.max_hp) * 100);
        // The bar is held to its most only when set: a sheet whose bar
        // turns from stamina to Force (Force Sensitive) keeps its points,
        // as play keeps them until they are spent.
        let fp = if set_fp {
            fp.min(i32::from(sheet.pool()) * 100)
        } else {
            fp
        };
        match *who {
            Who::Away { npc } => {
                let kept = self.kept_mut(npc)?;
                (kept.sheet, kept.hp, kept.fp) = (sheet, hp, fp);
            }
            Who::Player(_) | Who::Companion { .. } => {
                let entity = self.body_mut(holder)?;
                entity.sheet = Some(sheet);
                // A body without a fighter stands at full whatever its
                // sheet: one is made only when an edit sets its points.
                if entity.fighter.is_some() || (pointed && points(None, &sheet) != (hp, fp)) {
                    let fighter = entity.fighter.get_or_insert_with(|| Fighter::of(&sheet));
                    (fighter.hp, fighter.fp) = (hp, fp);
                }
            }
        }
        Ok(())
    }
}

/// `row` refused past the end of a `WORDS`-word set of `what`s.
fn within<const WORDS: usize>(what: &str, row: u16) -> Result<(), String> {
    if row < Rows::<WORDS>::ROWS {
        Ok(())
    } else {
        Err(format!(
            "{what} {row}: past the last ({})",
            Rows::<WORDS>::ROWS - 1
        ))
    }
}
