//! Changes to a save, each through the scripts' state kjk keeps
//! (`save.script`). The snapshot's goods and progress are views of it,
//! gathered again on a step in a module with scripts
//! (`sim::world::scripts`); the credits, the XP and the journal are shown
//! there as they are set, so the view agrees before that step too, and the
//! items are gathered on it.

use sim::Save;
use sim::inventory::Catalog;
use sim::script::set_global;

/// A save being changed, with the items an install holds.
pub struct Editor<'a> {
    save: &'a mut Save,
    catalog: &'a Catalog,
}

impl<'a> Editor<'a> {
    pub fn new(save: &'a mut Save, catalog: &'a Catalog) -> Editor<'a> {
        Editor { save, catalog }
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
        set_global(&mut self.save.script.strings, named(name)?, value.to_string());
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
        let most = self.catalog.get(&item.template).map_or(1, |facts| facts.stacking.max(1));
        if !(1..=most).contains(&stack) {
            return Err(format!("{} stacks 1 to {most}, not {stack}", item.template));
        }
        if let Some(mut item) = self.save.script.items.get_mut(&id) {
            item.stack = stack;
        }
        Ok(())
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

fn no_item(id: u32) -> String {
    format!("no item {id:#x} in the save")
}
