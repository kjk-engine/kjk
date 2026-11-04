//! An editing session: the save as opened stays untouched, and its list of
//! changes is replayed onto a fresh copy after every change or undo
//! through `edit::Editor` (kjk's own rules). A change `edit` refuses is
//! never listed, so undo is a pop and a replay, and never half-applies.

use std::sync::LazyLock;

use edit::Who;
use sim::inventory::slots;

use crate::model::catalog::Catalog;

/// One change, as the editor offers it (each an `edit::Editor` call).
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Credits(i32),
    PartyXp(i32),
    Xp(Who, i32),
    Quest(String, i32),
    Number(String, i32),
    Boolean(String, bool),
    Text(String, String),
    AddItem {
        holder: u32,
        template: String,
        stack: u16,
    },
    /// A new item of `template` given to `who` and put on in `slot`: one
    /// change, one undo.
    GiveEquipped {
        who: Who,
        template: String,
        slot: u8,
    },
    RemoveItem(u32),
    Stack(u32, u16),
    Equip(Who, u32, u8),
    Unequip(Who, u8),
    GrantFeat(Who, u16),
    RemoveFeat(Who, u16),
    GrantPower(Who, u16),
    RemovePower(Who, u16),
    Ability(Who, usize, u8),
    Skill(Who, usize, u8),
    Alignment(Who, u8),
    Hp(Who, i32),
    Fp(Who, i32),
    /// Companion `npc` made available as a template's creature.
    Available(u8, String),
    Join(u8),
    Leave(u8),
    Unavailable(u8),
}

/// A change made: what it was, in words, and the rule it bends, if any
/// (warned of, never blocked).
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub change: Change,
    pub words: String,
    pub warning: Option<String>,
}

/// What changes are made with: the install's items and classes, and its
/// names; with no install, no items (an item is refused by name) and no
/// names (resrefs shown).
pub struct Ground<'a> {
    pub items: &'a sim::inventory::Catalog,
    pub catalog: Option<&'a Catalog>,
}

static NO_ITEMS: LazyLock<sim::inventory::Catalog> =
    LazyLock::new(|| sim::inventory::Catalog::new([]));

impl<'a> Ground<'a> {
    /// The ground an install gives (none: no items, no names).
    pub fn of(catalog: Option<&'a Catalog>) -> Ground<'a> {
        Ground {
            items: catalog.map_or(&*NO_ITEMS, |catalog| &catalog.data.catalog),
            catalog,
        }
    }

    /// Items alone, unnamed: for checks without an install.
    pub fn items(items: &'a sim::inventory::Catalog) -> Ground<'a> {
        Ground {
            items,
            catalog: None,
        }
    }
}

/// The equipment slots a person sees, in order, by name.
pub const SLOTS: [(u8, &str); 11] = [
    (slots::HEAD, "head"),
    (slots::BODY, "body"),
    (slots::HANDS, "hands"),
    (slots::RIGHT_WEAPON, "right hand"),
    (slots::LEFT_WEAPON, "left hand"),
    (slots::RIGHT_WEAPON_2, "right hand (2nd set)"),
    (slots::LEFT_WEAPON_2, "left hand (2nd set)"),
    (slots::LEFT_ARM, "left arm"),
    (slots::RIGHT_ARM, "right arm"),
    (slots::IMPLANT, "implant"),
    (slots::BELT, "belt"),
];

/// A slot's name.
pub fn slot_name(slot: u8) -> String {
    SLOTS
        .iter()
        .find(|(of, _)| *of == slot)
        .map_or(format!("slot {slot}"), |(_, name)| (*name).to_string())
}

/// A save open for editing.
pub struct Session {
    /// The save as opened: never changed.
    pub opened: edit::Opened,
    /// The opened save with every change made.
    pub edited: sim::Save,
    pub changes: Vec<Applied>,
    /// The save's level as the install plans it, when there is one: what
    /// a member the module placed is edited from (`Editor::with_level`).
    pub level: Option<sim::Level>,
    /// The world file as opened (blake3, hex): kjk saving over it since
    /// shows as another (`write::save`). None for a save not from a file.
    pub stamp: Option<String>,
    /// The save format it was written in.
    pub format: u32,
    /// Where this session backed the save up, once it has.
    pub backup: Option<std::path::PathBuf>,
    /// The saves' folder it was opened from: the only one it is written to.
    pub root: Option<std::path::PathBuf>,
}

impl Session {
    pub fn new(opened: edit::Opened) -> Session {
        Session {
            edited: opened.save.clone(),
            opened,
            changes: Vec::new(),
            level: None,
            stamp: None,
            format: sim::SAVE_FORMAT,
            backup: None,
            root: None,
        }
    }

    /// Save `id` of the saves' folder `root` opened, its file stamped; why
    /// not, as `edit::open` tells it.
    pub fn open(root: &std::path::Path, id: &str) -> Result<Session, edit::Refusal> {
        let opened = edit::open(root, id)?;
        let mut session = Session::new(opened);
        session.root = Some(root.to_path_buf());
        let world = crate::model::write::world(root, id);
        session.stamp = world.as_deref().map(crate::model::write::stamp);
        session.format = world
            .and_then(|bytes| Some(u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?)))
            .unwrap_or(sim::SAVE_FORMAT);
        Ok(session)
    }

    /// The session with the save's level, for members the module placed.
    pub fn with_level(mut self, level: Option<sim::Level>) -> Session {
        self.level = level;
        self
    }

    /// `change` made after the others, or why `edit` refuses it (then
    /// nothing changes).
    pub fn apply(&mut self, change: Change, ground: &Ground) -> Result<(), String> {
        let (save, warning) = self.replayed(Some(&change), ground)?;
        let words = self.words(&change, &save, ground);
        self.edited = save;
        self.changes.push(Applied {
            change,
            words,
            warning,
        });
        Ok(())
    }

    /// The last change taken back: the others made again on the opened
    /// save, on the ground they were made on.
    pub fn undo(&mut self, ground: &Ground) -> Result<(), String> {
        let Some(last) = self.changes.pop() else {
            return Ok(());
        };
        match self.replayed(None, ground) {
            Ok((save, _)) => {
                self.edited = save;
                Ok(())
            }
            Err(why) => {
                self.changes.push(last);
                Err(why)
            }
        }
    }

    /// The opened save with the listed changes made again, then `then`, and
    /// the warning `then` earns; why not, naming the change refused.
    fn replayed(
        &self,
        then: Option<&Change>,
        ground: &Ground,
    ) -> Result<(sim::Save, Option<String>), String> {
        let mut save = self.opened.save.clone();
        let mut warning = None;
        {
            let mut editor = edit::Editor::new(&mut save, ground.items);
            if let Some(level) = &self.level {
                editor = editor.with_level(level);
            }
            for applied in &self.changes {
                make(&mut editor, &applied.change)
                    .map_err(|why| format!("replaying \"{}\": {why}", applied.words))?;
            }
            if let Some(change) = then {
                make(&mut editor, change)?;
                warning = self.warning(&editor, change, ground);
            }
        }
        Ok((save, warning))
    }

    /// The characters of the edited save.
    pub fn characters(&self) -> Vec<edit::Character> {
        edit::characters(&self.edited, self.level.as_ref())
    }

    /// The warning `change` earns once `editor` made it, if any.
    fn warning(&self, editor: &edit::Editor, change: &Change, ground: &Ground) -> Option<String> {
        let worn = |who: &Who, item: u32| editor.equip_warning(who, item).ok().flatten();
        match change {
            Change::Equip(who, item, _) => return worn(who, *item),
            Change::GiveEquipped { who, slot, .. } => {
                let holder = edit::holder_of(editor.save(), who)?;
                let (item, _) = editor
                    .save()
                    .script
                    .items
                    .held_by(holder)
                    .find(|(_, item)| item.slot == Some(*slot))?;
                return worn(who, item);
            }
            _ => {}
        }
        let classes = &ground.catalog?.data.classes;
        // The listed changes made, as `edited` holds them.
        let character = |who: &Who| {
            edit::characters(&self.edited, self.level.as_ref())
                .into_iter()
                .find(|character| character.who == *who)
        };
        match change {
            Change::GrantFeat(who, feat) => edit::feat_warning(classes, &character(who)?, *feat),
            Change::GrantPower(who, row) => edit::power_warning(classes, &character(who)?, *row),
            _ => None,
        }
    }

    /// `change` in words, after it is made on `save`.
    fn words(&self, change: &Change, save: &sim::Save, ground: &Ground) -> String {
        let item = |id: &u32| {
            let template = save
                .script
                .items
                .get(id)
                .or_else(|| self.edited.script.items.get(id))
                .map_or(format!("item {id}"), |item| item.template.clone());
            named_item(&template, ground)
        };
        let row = |list: fn(&Catalog) -> &[crate::model::catalog::Entry], key: u16, what: &str| {
            ground
                .catalog
                .and_then(|catalog| {
                    list(catalog)
                        .iter()
                        .find(|entry| entry.key == u32::from(key))
                })
                .map_or(format!("{what} {key}"), |entry| entry.label().to_string())
        };
        let feat = |key| row(|catalog| &catalog.feats, key, "feat");
        let power = |key| row(|catalog| &catalog.powers, key, "power");
        match change {
            Change::Credits(credits) => format!("credits set to {credits}"),
            Change::PartyXp(xp) => format!("party XP set to {xp}"),
            Change::Xp(who, xp) => format!("{}: XP set to {xp}", who_words(who)),
            Change::Quest(tag, entry) => format!("quest {tag}: entry {entry}"),
            Change::Number(name, value) => format!("{name} = {value}"),
            Change::Boolean(name, value) => format!("{name} = {value}"),
            Change::Text(name, value) => format!("{name} = {value:?}"),
            Change::AddItem {
                holder,
                template,
                stack,
            } => format!(
                "{}: + {} ×{stack}",
                holder_words(*holder),
                named_item(template, ground)
            ),
            Change::GiveEquipped {
                who,
                template,
                slot,
            } => format!(
                "{}: + {} (equipped, {})",
                who_words(who),
                named_item(template, ground),
                slot_name(*slot)
            ),
            Change::RemoveItem(id) => format!("− {}", item(id)),
            Change::Stack(id, stack) => format!("{}: stack of {stack}", item(id)),
            Change::Equip(who, id, slot) => format!(
                "{}: equips {} ({})",
                who_words(who),
                item(id),
                slot_name(*slot)
            ),
            Change::Unequip(who, slot) => {
                format!("{}: {} emptied", who_words(who), slot_name(*slot))
            }
            Change::GrantFeat(who, key) => format!("{}: + feat {}", who_words(who), feat(*key)),
            Change::RemoveFeat(who, key) => format!("{}: − feat {}", who_words(who), feat(*key)),
            Change::GrantPower(who, key) => format!("{}: + power {}", who_words(who), power(*key)),
            Change::RemovePower(who, key) => format!("{}: − power {}", who_words(who), power(*key)),
            Change::Ability(who, ability, score) => format!(
                "{}: {} set to {score}",
                who_words(who),
                ABILITIES.get(*ability).copied().unwrap_or("ability")
            ),
            Change::Skill(who, skill, ranks) => format!(
                "{}: {} set to {ranks}",
                who_words(who),
                row(|catalog| &catalog.skills, *skill as u16, "skill")
            ),
            Change::Alignment(who, value) => {
                format!("{}: alignment set to {value}", who_words(who))
            }
            Change::Hp(who, hp) => format!("{}: HP set to {}", who_words(who), hp / 100),
            Change::Fp(who, fp) => format!("{}: FP set to {}", who_words(who), fp / 100),
            Change::Available(npc, template) => {
                format!(
                    "companion {npc} made available as {template} (applies when you load the save)"
                )
            }
            Change::Join(npc) => {
                format!("companion {npc} joins the party (applies when you load the save)")
            }
            Change::Leave(npc) => {
                format!("companion {npc} leaves the party (applies when you load the save)")
            }
            Change::Unavailable(npc) => {
                format!("companion {npc} is no longer available (applies when you load the save)")
            }
        }
    }
}

/// The abilities in `Ability` order.
pub const ABILITIES: [&str; 6] = [
    "Strength",
    "Dexterity",
    "Constitution",
    "Intelligence",
    "Wisdom",
    "Charisma",
];

/// An item template by its game name, else as it is.
fn named_item(template: &str, ground: &Ground) -> String {
    ground
        .catalog
        .and_then(|catalog| catalog.item(template))
        .map_or(template.to_string(), |entry| entry.label().to_string())
}

/// Who a character is, in words.
pub fn who_words(who: &Who) -> String {
    match who {
        Who::Player(seat) => format!("player {}", seat.0 + 1),
        Who::Companion { npc, .. } | Who::Away { npc } => format!("companion {npc}"),
    }
}

/// Whose an item is, in words.
fn holder_words(holder: u32) -> String {
    if holder == sim::inventory::PARTY {
        "party".into()
    } else {
        format!("holder {holder:#x}")
    }
}

/// `change` made by `editor`.
fn make(editor: &mut edit::Editor, change: &Change) -> Result<(), String> {
    match change.clone() {
        Change::Credits(credits) => editor.set_credits(credits),
        Change::PartyXp(xp) => editor.set_party_xp(xp),
        Change::Xp(who, xp) => editor.set_xp(&who, xp),
        Change::Quest(tag, entry) => editor.set_quest(&tag, entry),
        Change::Number(name, value) => editor.set_number(&name, value),
        Change::Boolean(name, value) => editor.set_boolean(&name, value),
        Change::Text(name, value) => editor.set_string(&name, &value),
        Change::AddItem {
            holder,
            template,
            stack,
        } => editor.add_item(holder, &template, stack).map(|_| ()),
        Change::GiveEquipped {
            who,
            template,
            slot,
        } => {
            // Worn things come from what the party carries, as a player's
            // own equip takes them.
            let item = editor.add_item(sim::inventory::PARTY, &template, 1)?;
            editor.equip(&who, item, slot)
        }
        Change::RemoveItem(id) => editor.remove_item(id),
        Change::Stack(id, stack) => editor.set_stack(id, stack),
        Change::Equip(who, id, slot) => editor.equip(&who, id, slot),
        Change::Unequip(who, slot) => editor.unequip(&who, slot),
        Change::GrantFeat(who, feat) => editor.grant_feat(&who, feat),
        Change::RemoveFeat(who, feat) => editor.remove_feat(&who, feat),
        Change::GrantPower(who, row) => editor.grant_power(&who, row),
        Change::RemovePower(who, row) => editor.remove_power(&who, row),
        Change::Ability(who, ability, score) => editor.set_ability(&who, ability, score),
        Change::Skill(who, skill, ranks) => editor.set_skill(&who, skill, ranks),
        Change::Alignment(who, value) => editor.set_alignment(&who, value),
        Change::Hp(who, hp) => editor.set_hp(&who, hp),
        Change::Fp(who, fp) => editor.set_fp(&who, fp),
        Change::Available(npc, template) => editor.make_available(npc, &template),
        Change::Join(npc) => editor.join(npc),
        Change::Leave(npc) => editor.leave(npc),
        Change::Unavailable(npc) => editor.remove_available(npc),
    }
}
