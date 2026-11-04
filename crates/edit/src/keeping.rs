//! Where a save keeps a character, for an edit to read and write it: a
//! player's or a member's body (its sheet, else its module's, and its
//! fighter); for a companion away, the party table's kept state, and its
//! creature's body too while one stands in the module (the Ebon Hawk's
//! idle companions): joining, that creature is taken as it stands.

use sim::combat::Fighter;
use sim::combat::sheet::Sheet;
use sim::inventory::slots;
use sim::script::party::Kept;
use sim::{EntityState, Kind, SimId};

use crate::characters::{module_sheet, points};
use crate::{Editor, Who};

/// Where `Editor::keeping` found a character.
pub(crate) struct Keeping {
    /// Who holds its items, XP and career.
    pub holder: u32,
    /// Its body in the module, if one stands.
    pub body: Option<SimId>,
    /// Its party number, when the table keeps it (a companion away).
    pub kept: Option<u8>,
    pub sheet: Sheet,
    pub player: bool,
}

/// Hit and Force points (hundredths) an edit leaves, and whether the edit
/// set them (a body without a fighter stands at full whatever its sheet:
/// one is made only for points set).
pub(crate) struct Points {
    pub hp: i32,
    pub fp: i32,
    pub set: bool,
}

impl Editor<'_> {
    /// Where the save keeps `who`.
    pub(crate) fn keeping(&self, who: &Who) -> Result<Keeping, String> {
        let at = |id: SimId, kept: Option<u8>, player: bool| -> Result<Keeping, String> {
            Ok(Keeping {
                holder: id.0,
                body: Some(id),
                kept,
                sheet: self.body_sheet(id)?,
                player,
            })
        };
        match *who {
            Who::Player(seat) => {
                let entity = self
                    .save
                    .snapshot
                    .entities
                    .iter()
                    .find(|e| e.kind == Kind::Avatar(seat));
                at(
                    entity.ok_or(format!("no player in seat {}", seat.0))?.id,
                    None,
                    true,
                )
            }
            Who::Companion { id, .. } => at(id, None, false),
            Who::Away { npc } => {
                let slot = self
                    .save
                    .script
                    .party
                    .available
                    .get(&npc)
                    .ok_or(format!("companion {npc} is not available"))?;
                let kept = slot.kept.is_some().then_some(npc);
                let standing = slot
                    .object
                    .map(SimId)
                    .filter(|id| self.save.snapshot.get(*id).is_some());
                if let Some(id) = standing {
                    return at(id, kept, false);
                }
                let table = slot.kept.ok_or(format!(
                    "companion {npc} has never been in the party: its gear is its template's until it joins"
                ))?;
                Ok(Keeping {
                    holder: table.from,
                    body: None,
                    kept,
                    sheet: table.sheet,
                    player: false,
                })
            }
        }
    }

    /// Body `id`'s sheet: its own, else its module's (with the level).
    fn body_sheet(&self, id: SimId) -> Result<Sheet, String> {
        let entity = self
            .save
            .snapshot
            .get(id)
            .ok_or(format!("no body {id:?} in the save"))?;
        entity
            .sheet
            .or_else(|| module_sheet(entity.kind, self.level))
            .ok_or(match self.level {
                None => format!(
                    "{id:?} holds no sheet of its own yet: edit it with its level (`Editor::with_level`) for its module's"
                ),
                Some(_) => format!("{id:?}: neither the save nor its module gives it a sheet"),
            })
    }

    /// The points `keeping` stands at: its body's fighter's (full without
    /// one), else the table's.
    pub(crate) fn points_of(&self, keeping: &Keeping) -> (i32, i32) {
        match (keeping.body, keeping.kept.and_then(|npc| self.kept(npc))) {
            (Some(id), _) => {
                let fighter = self
                    .save
                    .snapshot
                    .get(id)
                    .and_then(|entity| entity.fighter.as_ref());
                points(fighter, &keeping.sheet)
            }
            (None, Some(kept)) => (kept.hp, kept.fp),
            (None, None) => points(None, &keeping.sheet),
        }
    }

    /// `sheet` (and `points`, if any) written where `keeping` says, and
    /// the table's kept slots too when `dressed`.
    pub(crate) fn keep(
        &mut self,
        keeping: &Keeping,
        sheet: Sheet,
        points: Option<Points>,
        dressed: bool,
    ) -> Result<(), String> {
        if let Some(id) = keeping.body {
            let entity = self.body_mut(id.0)?;
            entity.sheet = Some(sheet);
            if let Some(Points { hp, fp, set }) = points
                && (entity.fighter.is_some() || (set && self::points(None, &sheet) != (hp, fp)))
            {
                let fighter = entity.fighter.get_or_insert_with(|| Fighter::of(&sheet));
                (fighter.hp, fighter.fp) = (hp, fp);
            }
        }
        if let Some(npc) = keeping.kept {
            let worn = dressed.then(|| self.worn_slots(keeping.holder));
            let kept = self.kept_mut(npc)?;
            kept.sheet = sheet;
            if let Some(Points { hp, fp, .. }) = points {
                (kept.hp, kept.fp) = (hp, fp);
            }
            if worn.is_some() {
                kept.worn = worn;
            }
        }
        Ok(())
    }

    /// The body `holder` stands as in the save.
    pub(crate) fn body_mut(&mut self, holder: u32) -> Result<&mut EntityState, String> {
        let id = SimId(holder);
        let body = self.save.snapshot.entities.iter_mut().find(|e| e.id == id);
        body.ok_or(format!("no body {holder:#x} in the save"))
    }

    /// The item `holder` wears in each slot (0: none), as the party table
    /// keeps them.
    fn worn_slots(&self, holder: u32) -> [u32; slots::COUNT as usize] {
        let mut worn = [0; slots::COUNT as usize];
        for (id, item) in self.save.script.items.held_by(holder) {
            if let Some(slot) = item.slot.filter(|slot| *slot < slots::COUNT) {
                worn[usize::from(slot)] = id;
            }
        }
        worn
    }

    pub(crate) fn kept(&self, npc: u8) -> Option<&Kept> {
        self.save.script.party.available.get(&npc)?.kept.as_ref()
    }

    fn kept_mut(&mut self, npc: u8) -> Result<&mut Kept, String> {
        self.save
            .script
            .party
            .available
            .get_mut(&npc)
            .and_then(|slot| slot.kept.as_mut())
            .ok_or(format!("companion {npc} is not kept by the party table"))
    }
}
