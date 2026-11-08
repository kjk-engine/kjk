//! Changes to a save, each through the scripts' state kjk keeps
//! (`save.script`). The snapshot's goods and progress are views of it,
//! gathered again on a step in a module with scripts
//! (`sim::world::scripts`); the credits, the XP and the journal are shown
//! there as they are set, so the view agrees before that step too, and the
//! items are gathered on it.

use sim::action::Action;
use sim::combat::passive;
use sim::inventory::Catalog;
use sim::inventory::properties::Wearer;
use sim::notice::Notice;
use sim::script::items::Limits;
use sim::script::set_global;
use sim::{Level, Save};

use crate::Who;

/// A save being changed, with the items an install holds, and the level
/// its module makes when one is at hand (the sheets of creatures the save
/// does not hold one for yet).
pub struct Editor<'a> {
    pub(crate) save: &'a mut Save,
    pub(crate) catalog: &'a Catalog,
    pub(crate) level: Option<&'a Level>,
}

impl<'a> Editor<'a> {
    pub fn new(save: &'a mut Save, catalog: &'a Catalog) -> Editor<'a> {
        Editor {
            save,
            catalog,
            level: None,
        }
    }

    /// The same, with the save's level (`edit::level_of`): a character
    /// whose body holds no sheet of its own yet is edited from its module's.
    pub fn with_level(self, level: &'a Level) -> Editor<'a> {
        Editor {
            level: Some(level),
            ..self
        }
    }

    /// The save as changed so far.
    pub fn save(&self) -> &Save {
        self.save
    }

    /// The party's credits.
    pub fn set_credits(&mut self, credits: i32) -> Result<(), String> {
        if credits < 0 {
            return Err(format!("{credits} credits: credits are never below 0"));
        }
        self.save.script.gold = credits;
        self.save.snapshot.goods.gold = credits;
        Ok(())
    }

    /// The party's XP. A character levels up in kjk's own level-up screen
    /// once its XP allows, every per-level rule applied there.
    pub fn set_party_xp(&mut self, xp: i32) -> Result<(), String> {
        let party = sim::inventory::PARTY;
        self.save.script.set_xp(party, xp);
        self.save.snapshot.progress.xp = self.save.script.xp(party);
        Ok(())
    }

    /// Quest `tag` at `entry`, lower than it stands or not.
    pub fn set_quest(&mut self, tag: &str, entry: i32) -> Result<(), String> {
        if sim::script::key(tag).is_empty() {
            return Err("a quest needs its tag".into());
        }
        self.save.script.set_quest(tag, entry, true);
        let journal = self.save.script.journal.clone();
        self.save.snapshot.progress.journal = journal;
        Ok(())
    }

    /// A number global, its name's case aside.
    pub fn set_number(&mut self, name: &str, value: i32) -> Result<(), String> {
        set_global(&mut self.save.script.numbers, named(name)?, value);
        Ok(())
    }

    /// A boolean global, its name's case aside.
    pub fn set_boolean(&mut self, name: &str, value: bool) -> Result<(), String> {
        set_global(&mut self.save.script.booleans, named(name)?, value);
        Ok(())
    }

    /// A string global, its name's case aside.
    pub fn set_string(&mut self, name: &str, value: &str) -> Result<(), String> {
        set_global(
            &mut self.save.script.strings,
            named(name)?,
            value.to_string(),
        );
        Ok(())
    }

    /// `stack` of `template` given to `holder` (`sim::inventory::PARTY` for
    /// the party's), joining a stack it has room on as scripts' items do;
    /// the item made or joined.
    pub fn add_item(&mut self, holder: u32, template: &str, stack: u16) -> Result<u32, String> {
        self.save
            .script
            .make_item(self.catalog, template, holder, stack, None)
            .map(|id| id.0)
            .ok_or(format!("no item {template:?} in the install"))
    }

    /// Item `id` taken away; refused while it is worn (take it off first).
    pub fn remove_item(&mut self, id: u32) -> Result<(), String> {
        let item = self.save.script.items.get(&id).ok_or(no_item(id))?;
        if item.slot.is_some() {
            return Err(format!("{} is worn: take it off first", item.template));
        }
        self.save.script.items.remove(&id);
        Ok(())
    }

    /// Item `id`'s stack, at least one and at most what its kind stacks to.
    pub fn set_stack(&mut self, id: u32, stack: u16) -> Result<(), String> {
        let item = self.save.script.items.get(&id).ok_or(no_item(id))?;
        let most = self
            .catalog
            .get(&item.template)
            .map_or(1, |facts| facts.stacking.max(1));
        if !(1..=most).contains(&stack) {
            return Err(format!("{} stacks 1 to {most}, not {stack}", item.template));
        }
        if let Some(mut item) = self.save.script.items.get_mut(&id) {
            item.stack = stack;
        }
        Ok(())
    }
}

impl Editor<'_> {
    /// Item `item`, one the party carries, put on `who` in `slot`, by the
    /// rules a player's own equip goes through but for the proficiencies
    /// and use limits, which `equip_warning` tells instead (spec: warn,
    /// don't block); what was there back with the party.
    pub fn equip(&mut self, who: &Who, item: u32, slot: u8) -> Result<(), String> {
        self.wear(who, Action::Equip { item, slot })
    }

    /// What `who` wears in `slot` back with the party.
    pub fn unequip(&mut self, who: &Who, slot: u8) -> Result<(), String> {
        self.wear(who, Action::Unequip(slot))
    }

    /// Why play would not let `who` use item `item` (a proficiency it
    /// lacks, naming the feats, or the item's use limits); `None` when it
    /// would.
    pub fn equip_warning(&self, who: &Who, item: u32) -> Result<Option<String>, String> {
        let template = &self
            .save
            .script
            .items
            .get(&item)
            .ok_or(no_item(item))?
            .template;
        let facts = self
            .catalog
            .get(template)
            .ok_or(format!("no item {template:?} in the install"))?;
        let keeping = self.keeping(who)?;
        let wearer = self.wearer(keeping.holder, &keeping.sheet, keeping.player);
        Ok(
            sim::script::State::equip_refusal(facts, &wearer).map(|notice| {
                let lacked: Vec<String> = facts
                    .needs
                    .iter()
                    .filter(|feat| !wearer.feats.has(**feat))
                    .map(u16::to_string)
                    .collect();
                match notice {
                    Notice::Line(passive::NOT_PROFICIENT) => {
                        format!("needs feat {} (a proficiency)", lacked.join(" or "))
                    }
                    other => refused(other),
                }
            }),
        )
    }

    /// `action` done on `who`, its sheet dressed anew where the save keeps
    /// it (`keeping`). A companion the table keeps with no creature is
    /// first slotted as the table keeps it, as the creature made of it
    /// will be (`State::dress_as_kept`).
    fn wear(&mut self, who: &Who, action: Action) -> Result<(), String> {
        let keeping = self.keeping(who)?;
        if keeping.body.is_none()
            && let Some(worn) = keeping
                .kept
                .and_then(|npc| self.kept(npc))
                .and_then(|kept| kept.worn)
        {
            self.save.script.dress_as_kept(keeping.holder, &worn);
        }
        let wearer = self.wearer(keeping.holder, &keeping.sheet, keeping.player);
        let sheet = self
            .save
            .script
            .wear_outside(
                self.catalog,
                (keeping.holder, &wearer),
                keeping.sheet,
                (action, Limits::Warned),
            )
            .map_err(refused)?;
        self.keep(&keeping, sheet, None, true)
    }

    /// `holder` as an item's use limits ask of it: its sheet's alignment,
    /// race, feats and scores, the classes its career gives, the game's
    /// rules.
    fn wearer(&self, holder: u32, sheet: &sim::combat::sheet::Sheet, player: bool) -> Wearer {
        let mut classes = [u8::MAX; 2];
        if let Some(career) = self.save.script.careers.get(&holder) {
            for (into, (class, _)) in classes.iter_mut().zip(career) {
                *into = *class;
            }
        }
        let game = self
            .save
            .snapshot
            .place
            .as_ref()
            .map_or("", |place| place.game.as_str());
        Wearer {
            good_evil: sheet.good_evil,
            classes,
            race: sheet.race,
            subrace: 0,
            feats: sheet.feats,
            gender: 0,
            abilities: sheet.abilities,
            player,
            tag: String::new(),
            tsl: passive::Game::named(game) == passive::Game::Tsl,
            // The editor warns of limits rather than holding to them, and a
            // locked slot is its player's limit only.
            locked: 0,
        }
    }
}

/// A refused equip in words.
fn refused(notice: Notice) -> String {
    match notice {
        Notice::Line(passive::NOT_PROFICIENT) => {
            "needs a feat the character lacks (a proficiency)".into()
        }
        Notice::Line(line) => format!("refused by the item's use limits (talk table line {line})"),
        Notice::CannotEquip => {
            "does not fit that slot or this character, or the party does not carry it".into()
        }
        other => format!("refused: {other:?}"),
    }
}

/// A global's name, refused when empty.
fn named(name: &str) -> Result<&str, String> {
    if name.trim().is_empty() {
        Err("a global needs its name".into())
    } else {
        Ok(name)
    }
}

pub(crate) fn no_item(id: u32) -> String {
    format!("no item {id:#x} in the save")
}
