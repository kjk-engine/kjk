//! A save's characters: its players, the companions in the party, and
//! those the party table holds away from it.

use sim::combat::sheet::Sheet;
use sim::{Kind, Level, Save, Seat, SimId};

/// Who a character is in a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    /// A seat's player.
    Player(Seat),
    /// A companion in the party, its body in the module.
    Companion { npc: u8, id: SimId },
    /// A companion the party table holds, not in the party.
    Away { npc: u8 },
}

/// A character as a save holds it: its sheet and its hit and Force points
/// (hundredths, as `combat::Fighter` keeps them).
#[derive(Debug, Clone, PartialEq)]
pub struct Character {
    pub who: Who,
    /// Its template: a companion's; empty for a player.
    pub template: String,
    pub sheet: Sheet,
    pub hp: i32,
    pub fp: i32,
}

/// The characters of `save`: its players by seat, the party's members in
/// the order they joined, then the companions away from it by number. A
/// member whose sheet neither its body nor `level` gives is left out
/// (`unsheeted`).
pub fn characters(save: &Save, level: Option<&Level>) -> Vec<Character> {
    let mut players: Vec<Character> = save
        .snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let Kind::Avatar(seat) = entity.kind else {
                return None;
            };
            let sheet = entity.sheet?;
            let (hp, fp) = points(entity.fighter.as_ref(), &sheet);
            Some(Character {
                who: Who::Player(seat),
                template: String::new(),
                sheet,
                hp,
                fp,
            })
        })
        .collect();
    players.sort_by_key(|character| match character.who {
        Who::Player(seat) => seat.0,
        _ => u8::MAX,
    });
    let party = &save.script.party;
    let members = party.members.iter().filter_map(|&npc| {
        let id = SimId(party.available.get(&npc)?.object?);
        let entity = save.snapshot.get(id)?;
        let sheet = entity.sheet.or_else(|| module_sheet(entity.kind, level))?;
        let (hp, fp) = points(entity.fighter.as_ref(), &sheet);
        Some(Character {
            who: Who::Companion { npc, id },
            template: party.available[&npc].template.clone(),
            sheet,
            hp,
            fp,
        })
    });
    let away = party
        .available
        .iter()
        .filter(|(npc, _)| !party.members.contains(npc))
        .map(|(&npc, slot)| {
            let (sheet, hp, fp) = match &slot.kept {
                Some(kept) => (kept.sheet, kept.hp, kept.fp),
                // As the world makes it: a template without a sheet of
                // its own stands as a commoner (`sim::world::sheet_of`).
                None => {
                    let sheet = slot
                        .template_facts()
                        .and_then(|facts| facts.sheet)
                        .unwrap_or_else(Sheet::commoner);
                    let (hp, fp) = points(None, &sheet);
                    (sheet, hp, fp)
                }
            };
            Character {
                who: Who::Away { npc },
                template: slot.template.clone(),
                sheet,
                hp,
                fp,
            }
        });
    players.extend(members);
    players.extend(away);
    players
}

/// The members of `save`'s party whose sheet neither their body nor
/// `level` gives: loaded and saved once in kjk, they have one.
pub fn unsheeted(save: &Save, level: Option<&Level>) -> Vec<u8> {
    let listed = characters(save, level);
    save.script
        .party
        .members
        .iter()
        .copied()
        .filter(|npc| {
            !listed
                .iter()
                .any(|c| matches!(c.who, Who::Companion { npc: n, .. } if n == *npc))
        })
        .collect()
}

/// A placed creature's sheet as its module's facts give it.
fn module_sheet(kind: Kind, level: Option<&Level>) -> Option<Sheet> {
    let Kind::Object(_, index) = kind else {
        return None;
    };
    level?.module()?.object(index as usize)?.sheet
}

/// Hit and Force points (hundredths): the fighter's, else full.
fn points(fighter: Option<&sim::combat::Fighter>, sheet: &Sheet) -> (i32, i32) {
    match fighter {
        Some(fighter) => (fighter.hp, fighter.fp),
        None => (i32::from(sheet.max_hp) * 100, i32::from(sheet.max_fp) * 100),
    }
}
