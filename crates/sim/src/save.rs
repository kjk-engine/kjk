//! A save is a snapshot plus the script VM's state, and the tick rate the
//! world ran at. Loading one gives a world that continues exactly as the
//! saved one would have.
//!
//! Not a KOTOR save: original save games are out of scope (`ROADMAP.md`).

use crate::snapshot::{Kind, Snapshot};
use crate::wire::{DecodeError, Reader, Stored, Writer};
use crate::world::{Level, LevelId, SimWorld};

const MAGIC: &[u8; 4] = b"KJKV";
/// 2: the level it was saved in (I14). 3: the scripts' action queues,
/// trigger occupants and perception (I17). 4: combat rounds, effects (C14).
/// 5: listeners (shouts), random walks. 6: lasting effects. 7: visual
/// effects carried, effects at a place. 9: items, inventories and gold
/// (I19). 10: the party table (I19). 11: conversations, barks, custom tokens
/// (I18). 12: heartbeats removed (`RemoveHeartbeat`). 13: trades under way
/// (stores, I19). 14: the journal, XP and levels (I19). 15: the party
/// selection screen a script showed, and blows scripts decided waiting for
/// their hit (`CutsceneAttack`). 16: item facts carried from the module
/// whose own template an item was (`script::State::known`). 17: the galaxy
/// map's planets. 18: the dead become remains (`script::State::remains`).
/// 19: the scripts' moves and attacks for the next step's minds
/// (`script::State::goals`, `intents`). 20: a start asked for carries who
/// asked; conversations their markers; answers heard by seat (I30 P6–P9).
/// 21: the party table the companion the first seat is switched to
/// (`SwitchPlayerCharacter`, `script::party::Party::switched`). 22: what
/// the scripts' step watches the world by (`script::State::watch`), spell
/// event values (`spell_events`). 23: how scripts changed the factions'
/// standing (`script::State::reputation`). 24: location globals
/// (`script::State::places`), areas made unescapable. 25: conversations the
/// placed camera a script cut to (`SetDialogPlaceableCamera`). 26: what the
/// party table keeps of a companion (`script::party::Kept`). 27: a shield's
/// damage types, effects of script shields, temporary hit points and a
/// resurrection (`script::effects::Effect`); what scripts left waiting in
/// each module the party left (`script::State::left`). 28: a poison effect
/// carries its `poison.2da` course (`combat::items::Course`). 29: main's
/// cameras with the second engine-smells batch. 30: the pazaak match a
/// script started and the last one's result (`script::State::pazaak`,
/// `pazaak_won`), items' upgrades fitted (`inventory::Item::upgrades`), the
/// party table's kept sheets' weapons' build (`Weapon::build`). 31: items
/// carried from another module keep their skill bonuses, trap and stealth
/// unit (`inventory::ItemFacts`); the traps' state (`script::State::traps`).
/// 32: items carry every property in force and TSL's upgrades
/// (`inventory::ItemFacts::properties`, `Item::fitted`); sheets what worn
/// items give (`Sheet::gear`). 33: the dead waiting to be taken away and
/// leave their remains in place of remains in their bodies
/// (`script::State::corpses`, also in each module left), creatures' own
/// gold (`script::State::purses`). 34: Security as a skill's use, with the
/// spike it is tried with (`script::SkillUse::tool`). 35: a scene's blows
/// carry their result, a miss too, and bolts whether a script fired them
/// (`script::effects::Strike::landed`, `combat::Missile::scripted`). 36: a
/// scene's blows carry the animation the script named, and whether it has
/// begun (`script::effects::Strike::animation`, `began`). 37: an event
/// waiting carries whether its caller's own AI raised it
/// (`script::Pending::ai`). 38: the pazaak match says whose it is
/// (`pazaak::Match::player`). 39: an event waiting carries the companion a
/// guest drives whose act raised it (`script::Pending::leads`). 40: a
/// conversation's listener, joined bodies, party, lock, request, wait and
/// the match its line dealt (private conversations); an event waiting and a queued
/// `ActionStartConversation` carry the body whose use raised them
/// (`script::Pending::asker`); the pazaak matches, each player's, with a
/// match's pause, its player's seat dropped, its hurt and who watches it
/// (`script::State::pazaak`); a held action and deferred code carry the
/// conversation they wait on (`Action::Hold::talk`, `Delayed::talk`). 41:
/// a conversation's camera angle, its default-angle table's place, step and
/// direction, its nodes' angles, the line's listener, the previous line's
/// pair and its turn and head-follow locks (scene mode). 42: TSL's
/// influence, puppets, `DisableHealthRegen`, `SetOrientOnClick` and mines
/// set on doors (`script::party`, `script::State::door_mines`). 43: the
/// workbench's components and chemicals (`script::State::components`,
/// `chemicals`). 44: TSL's companions' base alignments
/// (`script::party::Party::bases`). 45: TSL's forfeit conditions and the
/// last broken (`Party::forfeit`, `violation`). 46: a queued
/// `ActionSwitchWeapons` (`script::actions::Action::SwitchWeapons`) and
/// items in the second weapon set (`inventory::slots::RIGHT_WEAPON_2`,
/// `LEFT_WEAPON_2`).
/// 47: the components and chemicals of creatures outside the party
/// (`script::State::hoards`).
/// 48: a queued cast and surrender (`script::actions::Action::Cast`,
/// `Surrender`), a KOTOR state effect waiting
/// (`script::effects::Effect::State`).
/// 49: a trap keeps the Demolitions its setter had
/// (`script::TrapState::owner_demolitions`); the heartbeats TSL's
/// `ForceHeartbeat` ran (`script::State::beats`); the party table's kept
/// sheets TSL's bonus Force points, base saves and Form.
/// 50: the map jammed and the tutorial windows shown (`Party::no_map`,
/// `tutorials`), the areas' transit scripts set and the map page's return
/// button (`script::State::transit`, `return_button`), a queued bark
/// (`Action::Bark`).
/// 51: KOTOR's encounters as they run (`script::State::encounters`, also
/// in each module left).
/// 52: a queued puppet's follow of its owner (`Action::FollowOwner`).
/// 53: the items lying on an area's floor (`script::State::floor`).
/// 54: a scene's gunner's blow carries when its clip fires its bolts
/// (`script::effects::Strike::shots`).
/// 55: a script's push waiting (`script::effects::Effect::Pushed`).
/// 56: an item's flag that no one may put it on (`Item::non_equippable`,
/// `SetItemNonEquippable`).
/// 57: a blow carries its target's answer and its hit's moment
/// (`script::effects::Strike::answer`, `hit_ms`), and each attacker its
/// last round's swing (`script::State::swung`).
/// 58: a gunner's blow carries the hand each of its clip's bolts leaves
/// (`Discharge`'s switchmask).
/// 59: the party table keeps a companion's template made available by template
/// (`party::Npc::template_data`, `sim::pack`'s creature encoding), so a module
/// without it makes the companion all the same.
pub const VERSION: u32 = 59 + crate::pending::SAVE;
/// The version this build writes, for a save's meta to name.
pub const FORMAT: u32 = VERSION;

/// The oldest version read: v0.1.0's. Each change since keeps its old read
/// (`crate::since::save`, `bumps/README.md`).
pub const OLDEST: u32 = crate::wire::later(59, crate::pending::oldest::SAVE);

/// The companions' templates' version in a save from before it recorded
/// theirs (`since::save::SAVE_MIGRATIONS`): v0.1.0's, whatever the oldest
/// read is now.
const PACK_BEFORE_RECORDED: u32 = 20;

/// The scripts' state (`script::State`): globals, objects' locals, events
/// waiting and deferred code. `SimWorld::script_state` gives it, and
/// `set_script_state` puts a loaded one back.
pub type ScriptVmState = crate::script::State;

#[derive(Debug, Clone, PartialEq)]
pub struct Save {
    /// Ticks per second the world ran at. A tick's milliseconds depend on
    /// it, so a host at another rate refuses the save.
    pub hz: u32,
    /// The level the snapshot is in: loading it needs that level (a
    /// module's is planned from `snapshot.place`), and refuses another.
    pub level: LevelId,
    pub snapshot: Snapshot,
    pub script: ScriptVmState,
}

impl Save {
    /// The world going on from this save in `level` (the level it was
    /// made in, else refused), for a session that holds `seats`: the save's
    /// own count does not fix it (I31: guests take its companions), though
    /// every player of its own keeps a seat. Numbered from `tick` when given,
    /// as a running session's ticks only rise (the reload after the party
    /// fell, I30 P13). The menu's load and the reload both make it here.
    /// No container is held open in it (`containers::let_go`).
    pub fn world(&self, level: &Level, seats: u8, tick: Option<u64>) -> Result<SimWorld, String> {
        self.level.check(level).map_err(|error| {
            format!("{error}: the game's data is not what the save was made with")
        })?;
        let mut snapshot = self.snapshot.clone();
        snapshot.seats_held = self.seats_for(seats);
        // The world makes its droids again (`crate::remote`).
        snapshot.remotes.clear();
        crate::containers::let_go(&mut snapshot, level);
        if let Some(tick) = tick {
            snapshot.tick = tick;
        }
        // The companions' kept templates the level lacks, as an install adds
        // them (`install::with_party_templates`).
        let level = crate::install::with_party_templates(level.clone(), &self.script.party);
        let mut world =
            SimWorld::from_snapshot(&snapshot, level).map_err(|error| error.to_string())?;
        world.set_script_state(self.script.clone());
        Ok(world)
    }

    /// The seats a session asking for `seats` holds going on from this
    /// save: as many, or more when the save has players of its own past
    /// them (each keeps its seat).
    pub fn seats_for(&self, seats: u8) -> u8 {
        let players = self
            .snapshot
            .entities
            .iter()
            .filter_map(|entity| match entity.kind {
                Kind::Avatar(seat) => Some(seat.0.saturating_add(1)),
                _ => None,
            })
            .max()
            .unwrap_or(1);
        seats.max(players)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Writer::default();
        out.bytes(MAGIC);
        out.u32(VERSION);
        out.u32(self.hz);
        out.u32(crate::pack::VERSION);
        self.level.write(&mut out);
        // A save holds no droid (`crate::remote`).
        let snapshot = if self.snapshot.remotes.is_empty() {
            self.snapshot.encode()
        } else {
            let mut grounded = self.snapshot.clone();
            grounded.remotes.clear();
            grounded.encode()
        };
        out.section(&snapshot);
        let mut script = Writer::default();
        self.script.encode(&mut script);
        out.section(&script.0);
        out.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut input = Reader::new("save", bytes);
        input.header_since(MAGIC, Stored::Save, OLDEST, VERSION)?;
        let hz = input.u32()?;
        // The companions' templates' version (`party::Npc::template_data`).
        let pack = if input.since(crate::since::save::SAVE_MIGRATIONS) {
            input.u32()?
        } else {
            PACK_BEFORE_RECORDED
        };
        let level = LevelId::read(&mut input)?;
        let mut versions = input.versions();
        versions.pack = pack;
        let snapshot = Snapshot::decode_stored(input.section()?, &mut versions)?;
        // Saves from before I16 have an empty section.
        let section = input.section()?;
        let script = if section.is_empty() {
            ScriptVmState::default()
        } else {
            let mut script = Reader::new("save's scripts", section).with_versions(versions);
            let state = ScriptVmState::decode(&mut script)?;
            script.finish()?;
            state
        };
        input.finish()?;
        let mut save = Save {
            hz,
            level,
            snapshot,
            script,
        };
        crate::migrate::save(&mut save, versions)?;
        Ok(save)
    }
}
