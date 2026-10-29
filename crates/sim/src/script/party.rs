//! The party table (I19, `docs/WORLD-STATE.md`): KOTOR's companions by
//! `NPC_*` number, who of them is in the party, the leader, and the AI and
//! control settings scripts read. Scripts change it (dialogue's action
//! scripts mostly, I18); a creature that joins follows and fights for the
//! first seat (`Change::Join`), one that leaves fights for itself again.

use std::collections::{BTreeMap, BTreeSet};

use crate::combat::Sheet;
use crate::combat::sheet::Delivery;
use crate::wire::{DecodeError, Reader, Writer};

/// The most followers beside the leader.
pub const FOLLOWERS: usize = rules::progression::party::FOLLOWERS;

/// A companion the party table knows: the template it is made from, the
/// creature it is in this module, if one, what the table kept of it, and,
/// made available by template, that template as the module naming it read
/// it (`sim::pack`'s creature encoding): KOTOR's table keeps the creature,
/// so a module without the template makes it all the same (the dark side's
/// Bastila, of the Unknown World summit's own `p_bastilla00N`, in the Star
/// Forge's arrival scene).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Npc {
    pub template: String,
    pub object: Option<u32>,
    pub kept: Option<Kept>,
    pub template_data: Option<std::sync::Arc<[u8]>>,
}

/// What the party table keeps of a companion away from the party
/// (`SaveNPCState`, a member leaving it): the creature it was, whose items,
/// career and XP the next one made of it takes (`script::make_npc`), its
/// sheet as it stood (levels, what it wore) and its hit and Force points
/// (hundredths). KOTOR's table keeps the whole creature; without it a
/// companion sent away came back as its template, its gear and levels lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kept {
    pub from: u32,
    pub sheet: Sheet,
    pub hp: i32,
    pub fp: i32,
}

/// The party selection screen a script showed (`ShowPartySelectionGUI`),
/// until a player closes it: the script run then (KOTOR's exit script)
/// and the companions it puts in the party whatever the player picks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub exit: String,
    pub forced: Vec<u8>,
}

/// The screen's exit script's `GetRunScriptVar` once a player accepted
/// the party; 0 is the screen cancelled (TSL's `nAllowCancel`): the Ebon
/// Hawk's ramp (`check_party_gui`) leaves on the one, stays on the other.
pub const ACCEPTED: i32 = 1;

/// A puppet the party table knows (TSL's `AddAvailablePUP*`): the template
/// it is made from, its creature in this module if one, the companion it
/// belongs to (`AssignPUP`), and whether it is in the party
/// (`AddPartyPuppet`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Puppet {
    pub template: String,
    pub object: Option<u32>,
    pub owner: Option<u8>,
    pub joined: bool,
}

/// TSL's puppets: `PUP_*` 0 to 2, and the most in the party at once.
pub const PUPPETS: u8 = 3;
pub const PUPPETS_JOINED: usize = rules::progression::party::PUPPETS_JOINED;

/// TSL's influence, as its exe keeps it (`CSWPartyTable`): one for each
/// of `INFLUENCE_SLOTS` companions, unset (-1 to `GetInfluence`) until a
/// script sets it, `START_INFLUENCE` where `ModifyInfluence` finds it
/// unset, never past `MAX_INFLUENCE`.
pub const INFLUENCE_SLOTS: u8 = 12;
pub const START_INFLUENCE: u8 = rules::progression::influence::START;
pub const MAX_INFLUENCE: u8 = rules::progression::influence::MOST;

/// TSL's forfeit conditions (`SetForfeitConditions`, nwscript's
/// `FORFEIT_*`): the rules of a duel the party may break, which its
/// executable checks as a party member equips (`equipping`), uses an item
/// (`using`), or its player orders a Force power (one it knows and can pay
/// for, `world::powers`). A broken one does not stop the act:
/// it is kept for `GetLastForfeitViolation`, and the area is told by a
/// user-defined event numbered `EVENT` (its script ends the duel).
pub mod forfeit {
    use super::Delivery;
    use crate::inventory::slots;

    pub const FORCE_POWERS: u32 = 1;
    pub const ITEMS: u32 = 2;
    pub const WEAPONS: u32 = 4;
    pub const DXUN_SWORD_ONLY: u32 = 8;
    pub const ARMOR: u32 = 16;
    pub const RANGED: u32 = 32;
    pub const LIGHTSABER: u32 = 64;
    pub const ITEM_BUT_SHIELD: u32 = 128;
    /// The user-defined event the area is told.
    pub const EVENT: i32 = 4001;
    /// The one sword Dxun's duel allows, by tag.
    const DXUN_SWORD: &str = "pl_sword";

    /// The condition of `rules` an item tagged `tag` (its weapon's
    /// `delivery`, if a weapon) breaks equipped into `slot`: armour into
    /// the body; into either hand a weapon at all, then any but Dxun's
    /// sword, a ranged weapon, a lightsaber, the first the executable
    /// tests. None for any other slot (the second weapon set's too).
    pub fn equipping(rules: u32, slot: u8, tag: &str, delivery: Option<Delivery>) -> Option<u32> {
        let broken = |rule: u32, breaks: bool| (rules & rule != 0 && breaks).then_some(rule);
        match slot {
            slots::BODY => broken(ARMOR, true),
            slots::RIGHT_WEAPON | slots::LEFT_WEAPON => broken(WEAPONS, true)
                .or_else(|| broken(DXUN_SWORD_ONLY, !tag.eq_ignore_ascii_case(DXUN_SWORD)))
                .or_else(|| broken(RANGED, delivery.is_some_and(Delivery::ranged)))
                .or_else(|| broken(LIGHTSABER, matches!(delivery, Some(Delivery::Saber(_))))),
            _ => None,
        }
    }

    /// The condition of `rules` using an item breaks: any item at all,
    /// else any but an energy shield (its base item's `itemtype` 20, the
    /// forearm bands: `ItemFacts::shield`), the first the executable tests.
    pub fn using(rules: u32, shield: bool) -> Option<u32> {
        [(ITEMS, true), (ITEM_BUT_SHIELD, !shield)]
            .into_iter()
            .find(|&(rule, breaks)| rules & rule != 0 && breaks)
            .map(|(rule, _)| rule)
    }
}

/// What scripts keep about the party (a save carries it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Party {
    /// Available companions by `NPC_*` number.
    pub available: BTreeMap<u8, Npc>,
    /// Those in the party, in the order they joined.
    pub members: Vec<u8>,
    /// The companion leading (`SetPartyLeader`), `None` for the player.
    pub leader: Option<u8>,
    /// The companion the first seat plays in place of its own character
    /// (`SwitchPlayerCharacter`), `None` for its own (`NPC_PLAYER`): the
    /// scripts' PC, moved by that seat (`world::switch`).
    pub switched: Option<u8>,
    /// `NPC_AISTYLE_*` by creature, and the party's `PARTY_AISTYLE_*`.
    pub ai_styles: BTreeMap<u32, i32>,
    pub party_ai: i32,
    pub solo: bool,
    /// The player's control restricted (cutscenes).
    pub restrict: bool,
    /// Companions the player may not pick (`SetNPCSelectability`).
    pub unselectable: BTreeSet<u8>,
    /// The selection screen open, if a script showed one.
    pub selection: Option<Selection>,
    /// TSL's `DisableHealthRegen`: the party's natural healing off.
    pub no_regen: bool,
    /// TSL's influence by companion, those scripts set.
    pub influence: BTreeMap<u8, u8>,
    /// TSL's puppets by `PUP_*` number.
    pub puppets: BTreeMap<u8, Puppet>,
    /// TSL's base alignment by companion (its executable's
    /// `BaseCNPCAlignment`), those an alignment shift on it as a party
    /// member moved; the rest's is the alignment it was made with
    /// (`script::influence`).
    pub bases: BTreeMap<u8, u8>,
    /// TSL's forfeit conditions in force (`forfeit`), and the last one a
    /// party member broke (0 for none).
    pub forfeit: u32,
    pub violation: u32,
    /// The map jammed (`DisableMap`: TSL's `PT_DISABLEMAP`), until a
    /// script frees it.
    pub no_map: bool,
    /// The `tutorial.2da` rows whose window has shown, a bit each
    /// (`ShowTutorialWindow`: the executable's `PT_TUT_WND_SHOWN`, 33
    /// bytes), cleared by a new game.
    pub tutorials: Tutorials,
}

/// The tutorial windows shown (`Party::tutorials`): a bit for each of 264
/// `tutorial.2da` rows in 33 bytes, as KOTOR keeps them (a row is a byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tutorials(pub [u8; 33]);

impl Default for Tutorials {
    fn default() -> Tutorials {
        Tutorials([0; 33])
    }
}

impl Tutorials {
    /// Whether row `row`'s window has shown.
    pub fn shown(&self, row: u8) -> bool {
        self.0[usize::from(row / 8)] & 1 << (row % 8) != 0
    }

    /// Row `row`'s window shown.
    pub fn show(&mut self, row: u8) {
        self.0[usize::from(row / 8)] |= 1 << (row % 8);
    }
}

impl Party {
    /// Whether `npc` is in the party.
    pub fn has(&self, npc: u8) -> bool {
        self.members.contains(&npc)
    }

    /// The creature the first seat is switched to (`switched`), if it has
    /// one.
    pub fn switched_body(&self) -> Option<u32> {
        self.available.get(&self.switched?)?.object
    }

    /// The creatures of the party's companions, in joining order.
    pub fn creatures(&self) -> impl Iterator<Item = u32> + '_ {
        self.members
            .iter()
            .filter_map(|npc| self.available.get(npc)?.object)
    }

    /// `npc` joins, on `object`, if it is available, not in yet and there
    /// is room; whether it did.
    pub fn join(&mut self, npc: u8, object: u32) -> bool {
        let Some(slot) = self.available.get_mut(&npc) else {
            return false;
        };
        if self.members.contains(&npc) || self.members.len() >= FOLLOWERS {
            return false;
        }
        slot.object = Some(object);
        self.members.push(npc);
        true
    }

    /// The table as a party selection screen shows it: `forced` the
    /// companions its script puts in, `open` whether a script showed it;
    /// what each companion wears on its body among `items`, held by its
    /// creature (in the party) or the one the table kept it from.
    pub fn roster(
        &self,
        forced: &[u8],
        open: bool,
        items: &crate::inventory::Items,
    ) -> crate::notice::Roster {
        use crate::notice::Roster;
        let mut roster = Roster {
            unselectable: Roster::bits(self.unselectable.iter().copied()),
            members: Roster::bits(self.members.iter().copied()),
            forced: Roster::bits(forced.iter().copied()),
            open,
            ..Roster::default()
        };
        for (npc, slot) in &self.available {
            roster.available.insert(*npc, slot.template.clone());
            if let Some(creature) = slot.object.or(slot.kept.map(|kept| kept.from)) {
                let armour = items
                    .held_by(creature)
                    .find(|(_, item)| item.slot == Some(crate::inventory::slots::BODY))
                    .map(|(_, item)| item.template.clone());
                roster.worn.insert(*npc, armour);
            }
        }
        roster
    }

    /// Who a choice from the selection screen puts in the party: the
    /// forced first, then those `picks` names that are available and
    /// selectable, each once, `FOLLOWERS` at most.
    pub fn chosen(&self, selection: &Selection, picks: &[u8]) -> Vec<u8> {
        let mut party = Vec::new();
        let picked = picks.iter().filter(|npc| !self.unselectable.contains(npc));
        for &npc in selection.forced.iter().chain(picked) {
            if self.available.contains_key(&npc) && !party.contains(&npc) && party.len() < FOLLOWERS
            {
                party.push(npc);
            }
        }
        party
    }

    /// TSL's influence of companion `npc` as `GetInfluence` reads it,
    /// available or not: -1 while unset, 0 past the table.
    pub fn influence(&self, npc: u8) -> i32 {
        match self.influence.get(&npc) {
            _ if npc >= INFLUENCE_SLOTS => 0,
            Some(&set) => i32::from(set),
            None => -1,
        }
    }

    /// The puppets in the party, with their creatures.
    pub fn puppet_creatures(&self) -> impl Iterator<Item = (u8, u32)> + '_ {
        self.puppets
            .iter()
            .filter(|(_, puppet)| puppet.joined)
            .filter_map(|(pup, puppet)| Some((*pup, puppet.object?)))
    }

    /// The puppet whose creature is `id`: one in the party only, if
    /// `joined` (the executable's puppet flag, `GetIsPuppet`).
    pub fn puppet_of(&self, id: u32, joined: bool) -> Option<&Puppet> {
        self.puppets
            .values()
            .find(|puppet| (puppet.joined || !joined) && puppet.object == Some(id))
    }

    /// The creature of the companion in the party puppet creature `id` was
    /// given to (`GetPUPOwner`).
    pub fn puppet_owner(&self, id: u32) -> Option<u32> {
        self.puppet_of(id, false)
            .and_then(|puppet| puppet.owner)
            .filter(|npc| self.has(*npc))
            .and_then(|npc| self.available.get(&npc)?.object)
    }

    /// `npc` leaves; its creature, if it was in.
    pub fn leave(&mut self, npc: u8) -> Option<u32> {
        let at = self.members.iter().position(|member| *member == npc)?;
        self.members.remove(at);
        if self.leader == Some(npc) {
            self.leader = None;
        }
        self.available.get(&npc)?.object
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.available.len() as u32);
        for (npc, slot) in &self.available {
            out.u8(*npc);
            out.string(&slot.template);
            out.u32(slot.object.unwrap_or(u32::MAX));
            match &slot.kept {
                Some(kept) => {
                    out.u8(1);
                    out.u32(kept.from);
                    kept.sheet.encode(out);
                    out.u32(kept.hp as u32);
                    out.u32(kept.fp as u32);
                }
                None => out.u8(0),
            }
            out.section(slot.template_data.as_deref().unwrap_or(&[]));
        }
        out.section(&self.members);
        out.u8(self.leader.unwrap_or(u8::MAX));
        out.u8(self.switched.unwrap_or(u8::MAX));
        out.u32(self.ai_styles.len() as u32);
        for (object, style) in &self.ai_styles {
            out.u32(*object);
            out.u32(*style as u32);
        }
        out.u32(self.party_ai as u32);
        out.u8(u8::from(self.solo)
            | u8::from(self.restrict) << 1
            | u8::from(self.no_regen) << 2
            | u8::from(self.no_map) << 3);
        out.section(&self.unselectable.iter().copied().collect::<Vec<u8>>());
        out.u8(u8::from(self.selection.is_some()));
        if let Some(selection) = &self.selection {
            out.string(&selection.exit);
            out.section(&selection.forced);
        }
        out.u32(self.influence.len() as u32);
        for (npc, influence) in &self.influence {
            out.bytes(&[*npc, *influence]);
        }
        out.u32(self.puppets.len() as u32);
        for (pup, puppet) in &self.puppets {
            out.u8(*pup);
            out.string(&puppet.template);
            out.u32(puppet.object.unwrap_or(u32::MAX));
            out.u8(puppet.owner.unwrap_or(u8::MAX));
            out.u8(u8::from(puppet.joined));
        }
        out.u32(self.bases.len() as u32);
        for (npc, base) in &self.bases {
            out.bytes(&[*npc, *base]);
        }
        out.u32(self.forfeit);
        out.u32(self.violation);
        out.bytes(&self.tutorials.0);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Party, DecodeError> {
        let mut party = Party::default();
        for _ in 0..input.u32()? {
            let npc = input.u8()?;
            let template = input.string()?;
            let object = Some(input.u32()?).filter(|id| *id != u32::MAX);
            let kept = match input.u8()? {
                0 => None,
                _ => Some(Kept {
                    from: input.u32()?,
                    sheet: Sheet::decode(input)?,
                    hp: input.u32()? as i32,
                    fp: input.u32()? as i32,
                }),
            };
            let data = input.section()?;
            party.available.insert(
                npc,
                Npc {
                    template,
                    object,
                    kept,
                    template_data: (!data.is_empty()).then(|| data.into()),
                },
            );
        }
        party.members = input.section()?.to_vec();
        party.leader = Some(input.u8()?).filter(|npc| *npc != u8::MAX);
        party.switched = Some(input.u8()?).filter(|npc| *npc != u8::MAX);
        for _ in 0..input.u32()? {
            let object = input.u32()?;
            party.ai_styles.insert(object, input.u32()? as i32);
        }
        party.party_ai = input.u32()? as i32;
        let flags = input.u8()?;
        (party.solo, party.restrict, party.no_regen, party.no_map) = (
            flags & 1 != 0,
            flags & 2 != 0,
            flags & 4 != 0,
            flags & 8 != 0,
        );
        party.unselectable = input.section()?.iter().copied().collect();
        if input.u8()? != 0 {
            party.selection = Some(Selection {
                exit: input.string()?,
                forced: input.section()?.to_vec(),
            });
        }
        for _ in 0..input.u32()? {
            let [npc, influence] = input.array()?;
            party.influence.insert(npc, influence);
        }
        for _ in 0..input.u32()? {
            let pup = input.u8()?;
            let puppet = Puppet {
                template: input.string()?,
                object: Some(input.u32()?).filter(|id| *id != u32::MAX),
                owner: Some(input.u8()?).filter(|npc| *npc != u8::MAX),
                joined: input.u8()? != 0,
            };
            party.puppets.insert(pup, puppet);
        }
        for _ in 0..input.u32()? {
            let [npc, base] = input.array()?;
            party.bases.insert(npc, base);
        }
        party.forfeit = input.u32()?;
        party.violation = input.u32()?;
        party.tutorials = Tutorials(input.array()?);
        Ok(party)
    }
}
