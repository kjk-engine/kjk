//! What scripts keep between runs (`State`), and its encoding in a save.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use script_ncs::vm::{Deferred, Handle, ObjectId, Value};

use super::actions::{Action, Goal, Intent, Queued, Target, Use};
use super::dialogue::Talk;
use super::effects::{Applied, Effect, Strike};
use super::party::Party;
use super::{Cast, SpellEvent, key};
use crate::inventory::Item;
use crate::remains::Corpse;
use crate::wire::{DecodeError, Reader, Writer};

/// Code a script deferred, waiting to run.
#[derive(Debug, Clone, PartialEq)]
pub struct Delayed {
    /// Sim ms it runs at or after.
    pub due_ms: u64,
    /// The order it was deferred in: ties run first-deferred first.
    pub order: u64,
    /// Whom it runs for (`OBJECT_SELF`).
    pub caller: ObjectId,
    /// The script whose code it is.
    pub script: String,
    pub deferred: Deferred,
    /// The cast whose impact deferred it: its code runs as that impact
    /// (`GetSpellId`, the effects' spell), as KOTOR's does.
    pub spell: Option<Cast>,
    /// The conversation whose script deferred it (`dialogue::running`): a
    /// pause it queues holds that one.
    pub talk: Option<u32>,
}

/// A script to run from its start, for an event: its caller, the object
/// entering or leaving (`GetEnteringObject`, `GetExitingObject`), and the
/// event's number (`GetUserDefinedEventNumber`). Who opened, damaged or
/// killed an object is kept on it (`State::last`); so is, as a notice event
/// runs, what it perceived (`entering`) and how (`perception`).
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub script: String,
    pub caller: ObjectId,
    pub entering: Option<ObjectId>,
    pub number: i32,
    /// A notice event's `perception` kind; 0 for any other event.
    pub perception: u8,
    /// Whether the caller's own AI raised it (a user-defined event its AI
    /// run signalled to itself, `Event::ai`): its run is that AI's too.
    pub ai: bool,
    /// The companion a guest drives whose own act raised it (a door it
    /// opened or failed to open, a lock it picked, a thing it used): its
    /// run sees that companion lead the party (`Event::leads`).
    pub leads: Option<ObjectId>,
    /// The body whose use raised it (a dialogue hook, an `OnUsed`): a
    /// conversation its run starts is that body's (R1, `Event::asker`).
    pub asker: Option<ObjectId>,
    /// Its `GetRunScriptVar`: the party selection screen's script runs
    /// with `party::ACCEPTED` once a player accepted the party (TSL's
    /// `check_party_gui` keeps the party aboard the Ebon Hawk on 0, the
    /// screen cancelled); 0 for any other.
    pub script_var: i32,
}

impl Pending {
    pub fn new(script: &str, caller: ObjectId) -> Pending {
        Pending {
            script: script.to_string(),
            caller,
            entering: None,
            number: 0,
            perception: 0,
            ai: false,
            leads: None,
            asker: None,
            script_var: 0,
        }
    }
}

/// The most location, effect or spell event values kept from one step to
/// the next before those no deferred code holds go (`State::compact`).
const VALUES_KEPT: usize = 1024;

/// A spell event value (`EventSpellCastAt`) is a `Value::Event` with this
/// bit and its number in `State::spell_events`; a user-defined event's is
/// its number.
pub(super) const SPELL_EVENT: u32 = 0x4000_0000;

/// A global's value by name, its case aside (KOTOR's global names are
/// one whatever their case: Ebon Hawk's capture sets `Lev_Escape`, the
/// Leviathan's cell block reads `lev_escape`).
pub fn global<'m, V>(globals: &'m BTreeMap<String, V>, name: &str) -> Option<&'m V> {
    globals.get(name).or_else(|| {
        globals
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}

/// A global set by name, its case aside: under the name first written.
pub fn set_global<V>(globals: &mut BTreeMap<String, V>, name: &str, value: V) {
    let key = if globals.contains_key(name) {
        name.to_string()
    } else {
        globals
            .keys()
            .find(|key| key.eq_ignore_ascii_case(name))
            .cloned()
            .unwrap_or_else(|| name.to_string())
    };
    globals.insert(key, value);
}

/// What scripts keep between runs, on the authority; what a save carries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// Globals by name (`SetGlobalNumber`, `Boolean`, `String`).
    pub numbers: BTreeMap<String, i32>,
    pub booleans: BTreeMap<String, bool>,
    pub strings: BTreeMap<String, String>,
    /// Location globals by name (`SetGlobalLocation`): a place and facing.
    pub places: BTreeMap<String, ([f32; 3], f32)>,
    /// The modules whose area the party may not leave for the Ebon Hawk
    /// (`SetAreaUnescapable`).
    pub unescapable: BTreeSet<String>,
    /// What scripts left waiting in each module the party left, by
    /// lowercase name (`Left`).
    pub left: BTreeMap<String, Left>,
    /// Objects' locals: (object, 0 for booleans or 1 for numbers, index).
    pub locals: BTreeMap<(u32, u8, i32), i32>,
    pub pending: Vec<Pending>,
    pub delayed: Vec<Delayed>,
    /// The next `Delayed::order`.
    pub next: u64,
    /// Each object's action queue, front first.
    pub actions: BTreeMap<u32, Vec<Queued>>,
    /// Location values (position in metres, facing in degrees): a
    /// `Value::Location(Handle(n))` is `locations[n - 1]`.
    pub locations: Vec<([f32; 3], f32)>,
    /// Who last opened, used, damaged or killed an object
    /// (`engine::last`), for `GetLast…`.
    pub last: BTreeMap<(u32, u8), u32>,
    /// Objects `SetCommandable(FALSE)` made deaf to actions.
    pub uncommandable: BTreeSet<u32>,
    /// Creatures `SetOrientOnClick(FALSE)` keeps from turning in their
    /// conversations (TSL).
    pub unoriented: BTreeSet<u32>,
    /// Who stands in each trigger (by the trigger's id), for its enter and
    /// exit events.
    pub occupants: BTreeMap<u32, Vec<u32>>,
    /// The modules whose creatures' spawn scripts have run (once each).
    pub spawned: BTreeSet<String>,
    /// Whom each creature sees and hears now (by the creature's id), for
    /// its notice events and `GetObjectSeen`, `GetObjectHeard`.
    pub perceived: BTreeMap<u32, Perceived>,
    /// When each character in combat ends its round next (sim ms), for
    /// its end-of-round event.
    pub rounds: BTreeMap<u32, u64>,
    /// Effect values (`script::effects`): a `Value::Effect(Handle(n))` is
    /// `effects[n - 1]`.
    pub effects: Vec<Effect>,
    /// Spell event values: a `Value::Event(Handle(SPELL_EVENT | n))` is
    /// `spell_events[n - 1]`.
    pub spell_events: Vec<SpellEvent>,
    /// Effects applied, for the next step's damage pipeline, and lasting
    /// effects to take off (an object, one id or all of them, and how many
    /// applied effects came before: they are carried out in the scripts'
    /// order); the next lasting effect's id.
    pub applied: Vec<Applied>,
    pub removed: Vec<(u32, Option<u32>, u32)>,
    /// Scripts' bashes struck this step (`Action::Bash`: the door, the
    /// striker), for the next step's blows on objects (`world::objects`).
    pub bashes: Vec<(u32, u32)>,
    pub next_effect: u32,
    /// Who listens to shouts (`SetListening`), and the patterns each
    /// hears with their numbers (`SetListenPattern`).
    pub listening: BTreeSet<u32>,
    pub patterns: BTreeMap<u32, Vec<(String, i32)>>,
    /// The items in play by object id (`inventory::Item`, I19), the next
    /// one's number (`inventory::FIRST_ITEM` + it), and the party's gold.
    pub items: crate::inventory::Items,
    pub next_item: u32,
    pub gold: i32,
    /// The gold of creatures outside the party, by id
    /// (`GiveGoldToCreature`): left with their remains (`world::remains`).
    pub purses: BTreeMap<u32, i32>,
    /// The party table (`script::party`).
    pub party: Party,
    /// Conversations under way, barks, custom tokens (I18).
    pub talk: Talk,
    /// Objects whose heartbeat `RemoveHeartbeat` took away.
    pub heartless: BTreeSet<u32>,
    /// The trades under way (`OpenStore`, `script::stores`), by player id.
    pub trades: BTreeMap<u32, crate::inventory::Trade>,
    /// The entry each quest has reached, by lowercase tag, the latest
    /// updated last (`script::journal`).
    pub journal: Vec<(String, i32)>,
    /// XP by holder: the party's pool under `inventory::PARTY`, a creature
    /// outside the party by its id (`script::xp`).
    pub xp: BTreeMap<u32, i32>,
    /// The classes and levels each character took here, by id
    /// (`script::levelling`), and the levels asked for, not yet taken.
    pub careers: BTreeMap<u32, crate::levels::Career>,
    pub levelling: Vec<(u32, crate::levels::Choices)>,
    /// Blows scripts decided, waiting for their hit (`effects::Strike`).
    pub strikes: Vec<Strike>,
    /// When each attacker's last round's swing began (sim ms): its next
    /// waits a round on (`combat::rounds`).
    pub swung: BTreeMap<u32, u64>,
    /// The facts of items in play whose templates the module's catalog
    /// lacks: made where their template was a module's own (Gadon's papers),
    /// carried on (`SimWorld::install`). KOTOR's item keeps its own data.
    pub known: BTreeMap<String, crate::inventory::ItemFacts>,
    /// Creatures kept after death (`SetIsDestroyable(FALSE)`); the rest
    /// are taken away a while after it (`world::remains`).
    pub kept: BTreeSet<u32>,
    /// The galaxy map's planets (`planetary.2da` rows).
    pub galaxy: Galaxy,
    /// The dead outside the party the engine takes away, by id: when, and
    /// the remains they leave (`world::remains`).
    pub corpses: BTreeMap<u32, Corpse>,
    /// What the last step's queues ask of the next step's minds, by object
    /// id (`script::actions`): where scripted moves route each one, and
    /// whom scripted attacks have it fight. A save carries them, so a
    /// loaded world's first step acts as the saved one's would have.
    pub goals: BTreeMap<u32, Goal>,
    pub intents: BTreeMap<u32, Intent>,
    /// What the step's events are told from (`world::scripts`). A save
    /// carries it, so a loaded world's first step raises the events the
    /// saved one's would have.
    pub watch: Watch,
    /// What scripts changed of how factions stand (`AdjustReputation`),
    /// the game's from then on, in every module.
    pub reputation: crate::module::Adjustments,
    /// What each trap in play keeps beyond its facts, by the trap's id
    /// (`world::traps`).
    pub traps: BTreeMap<u32, TrapState>,
    /// The skills each seat is using on a trap (`world::traps`), by seat.
    pub skill_uses: BTreeMap<u8, SkillUse>,
    /// TSL's mines set on doors, by the door's id (`world::traps`).
    pub door_mines: BTreeMap<u32, DoorMine>,
    /// When the creatures last looked for traps (sim ms, `world::traps`).
    pub last_detect_ms: u64,
    /// When each body next perceives (sim ms; its AI's cadence), and the
    /// rolls it holds (`stealth::Rolls`), by id (`world::scripts`).
    pub perception: BTreeMap<u32, (u64, crate::stealth::Rolls)>,
    /// Each area's stealth XP, by its resref (`script::traps`).
    pub stealth_xp: BTreeMap<String, StealthXp>,
    /// When each creature that newly saw the party costs the area's stealth
    /// XP its loss (sim ms), by the creature's id (`world::scripts`).
    pub stealth_loss: BTreeMap<u32, u64>,
    /// The pazaak matches scripts started (`PlayPazaak`, `script::pazaak`),
    /// one a player body, in the order they began; and whether the player
    /// of the last to end won it (`GetLastPazaakResult`).
    pub pazaak: Vec<crate::pazaak::Match>,
    pub pazaak_won: bool,
    /// The party's components and chemicals (TSL's item creation,
    /// `inventory::workshop`).
    pub components: i32,
    pub chemicals: i32,
    /// The components and chemicals of creatures outside the party, by id
    /// (TSL's, `script::workbench`): what was given them, never in their
    /// remains, added to the party's whenever they join it.
    pub hoards: BTreeMap<u32, (i32, i32)>,
    /// The objects TSL's `ForceHeartbeat` ran: the moment (sim ms) their
    /// heartbeat's period counts from since, by id (`world::scripts`).
    pub beats: BTreeMap<u32, u64>,
    /// Whether each area's `DisableTransit` a script set, by module name
    /// (`SetDisableTransit`); the rest keep their ARE's.
    pub transit: BTreeMap<String, bool>,
    /// TSL's movies queued (`QueueMovie`: resref, skippable), which the
    /// next `PlayMovieQueue` plays: KOTOR's client application keeps them,
    /// so they outlast a module change, and no save carries them.
    pub movies: Vec<(String, bool)>,
    /// The `PlayMovieQueue` runs so far, each run's movies one group to a
    /// skip (`notice::Screen::Movie`); not saved.
    pub movie_runs: u16,
    /// Until when (sim ms) a click uses no placeable and starts no
    /// conversation (`NoClicksFor`: KOTOR's client keeps it; not saved).
    pub no_clicks_until: u64,
    /// TSL's `SetKeepStealthInDialog`: a conversation starting leaves its
    /// parties in stealth (KOTOR's client keeps it; not saved).
    pub keep_stealth: bool,
    /// The map page's return button as `SetReturnStrref` last set it:
    /// shown, label, confirm box. KOTOR's party table does not save it
    /// (`k_sup_guiopen` sets it anew each time an in-game screen opens,
    /// before anything reads it); a save carries it here because the
    /// snapshot does (`Snapshot::screens`), so a loaded world is the one
    /// saved.
    pub return_button: (bool, u32, u32),
    /// KOTOR's encounters as they run, by the encounter's id
    /// (`crate::spawner`): those of the module here (a module left keeps
    /// its own in `left`).
    pub encounters: BTreeMap<u32, crate::spawner::Spawn>,
    /// The creatures encounters made, by the creature's id, each with its
    /// encounter and whether its death or going was counted. Not saved, as
    /// the games' saves keep no such link (QUIRKS.md Q-0261).
    pub encounter_creatures: BTreeMap<u32, crate::spawner::Link>,
    /// The items lying on an area's floor (`CreateItemOnFloor`), by the
    /// item's id: the module whose area it lies in, and where. Each is
    /// held by the area (`AREA`) while it lies there (`State::lying_in`).
    pub floor: BTreeMap<u32, (String, crate::inventory::Lying)>,
}

/// What a skill's use is: on a trap (`world::traps`), or Security on a
/// lock (`world::locks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Disarm,
    Recover,
    Lay,
    Unlock,
    /// TSL's mine set on a door (`world::traps`).
    MineDoor,
}

impl UseKind {
    const ALL: [UseKind; 5] = [
        UseKind::Disarm,
        UseKind::Recover,
        UseKind::Lay,
        UseKind::Unlock,
        UseKind::MineDoor,
    ];
}

/// A mine set on a door (TSL, `world::traps`): who set it, the mine laid
/// at the door (its id), the kit's blast strength, when it blasts the lock
/// (sim ms), and when the door then opens, once it blasted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoorMine {
    pub setter: u32,
    pub mine: u32,
    pub strength: u8,
    pub blast_at: u64,
    pub open_at: Option<u64>,
}

/// A skill's use under way: what, by whom, on what (a trap's or a lock's
/// id; a kit's item id to lay), with what (a security spike's item id),
/// from where the user stood (metres), done when.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkillUse {
    pub kind: UseKind,
    pub by: u32,
    pub object: u32,
    pub tool: Option<u32>,
    pub from: [f32; 3],
    pub until: u64,
}

/// A trap's state beyond its facts: the creatures that detected it (for
/// `GetTrapDetectedBy`), and a laid mine's creator, the creator's faction
/// then and its DCs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrapState {
    pub detected_by: Vec<u32>,
    pub creator: Option<u32>,
    pub faction: Option<u8>,
    pub laid: Option<crate::traps::Laid>,
    /// A script disabled it (`SetTrapDisabled`).
    pub disabled: bool,
    /// The Demolitions its creator had as it set it (TSL's
    /// `OwnerDemolitionsSkill`: the total rank then, a signed byte; 0 for
    /// one no creature set): what its blast reads.
    pub owner_demolitions: i8,
}

/// An area's stealth XP (its ARE's, as scripts change it): the most, what
/// is left to award, what a creature seeing the party costs, and whether
/// it is still to be awarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StealthXp {
    pub max: i32,
    pub current: i32,
    pub loss: i32,
    pub enabled: bool,
}

/// The world as the scripts' step last saw it, to tell its events from
/// (`world::scripts`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Watch {
    /// Each character's life as last seen, by id: damage and death.
    pub seen: BTreeMap<u32, Watched>,
    /// When each (victim, attacker)'s attacked and spell-cast-at scripts
    /// last ran (C15).
    pub attacked: BTreeMap<(u32, u32), u64>,
    pub spelled: BTreeMap<(u32, u32), u64>,
    /// The party's items as last seen: what acquire events are told from.
    pub held: Option<BTreeSet<u32>>,
}

/// A character's HP and life as last seen, and when its damaged script
/// last ran.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Watched {
    pub hp: i32,
    pub alive: bool,
    pub dead: bool,
    /// When the last hit it took landed (`Fighter::last_attacker`).
    pub hit_at: u64,
    pub damaged_at: Option<u64>,
}

/// The galaxy map: the planets scripts made available (shown) and
/// selectable (travelled to), and the one last picked (`GetSelectedPlanet`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Galaxy {
    pub available: BTreeSet<u8>,
    pub selectable: BTreeSet<u8>,
    pub selected: Option<u8>,
}

impl Galaxy {
    /// A player may travel to `planet`: shown and selectable.
    pub fn travels_to(&self, planet: u8) -> bool {
        self.available.contains(&planet) && self.selectable.contains(&planet)
    }

    /// The map `ShowGalaxyMap` opens on `planet`, as the screen shows it.
    pub fn shown(&self, planet: u8) -> crate::notice::Screen {
        let bits = |planets: &BTreeSet<u8>| {
            planets
                .iter()
                .filter_map(|planet| 1u32.checked_shl(u32::from(*planet)))
                .fold(0, |bits, bit| bits | bit)
        };
        crate::notice::Screen::GalaxyMap {
            planet,
            available: bits(&self.available),
            selectable: bits(&self.selectable),
        }
    }
}

/// Whom a creature sees and hears, by id, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Perceived {
    pub seen: Vec<u32>,
    pub heard: Vec<u32>,
}

impl State {
    pub(super) fn defer(
        &mut self,
        due_ms: u64,
        caller: ObjectId,
        script: &str,
        deferred: Deferred,
        (spell, talk): (Option<Cast>, Option<u32>),
    ) {
        self.delayed.push(Delayed {
            due_ms,
            order: self.next,
            caller,
            script: script.to_string(),
            deferred,
            spell,
            talk,
        });
        self.next += 1;
    }

    /// Whether code `id` deferred (`DelayCommand`, its `OBJECT_SELF`) is
    /// due after `now_ms` and within `within_ms` of it: a scene of its
    /// scripts still to play its next part.
    pub fn due_soon(&self, id: u32, now_ms: u64, within_ms: u64) -> bool {
        self.delayed
            .iter()
            .any(|d| d.caller.0 == id && d.due_ms > now_ms && d.due_ms <= now_ms + within_ms)
    }

    /// Whether `id`'s scripts have something still under way for it: a
    /// conversation asked for with it or queued for it to start
    /// (`ActionStartConversation`), code queued for it (`ActionDoCommand`),
    /// or code it deferred due within `within_ms` (`due_soon`); not its
    /// walks and animations, which an idle creature's own scripts keep
    /// queued. What a use set going has played out once none is.
    pub fn busy(&self, id: u32, now_ms: u64, within_ms: u64) -> bool {
        let queued = self.actions.get(&id).is_some_and(|queue| {
            queue.iter().any(|queued| {
                matches!(queued.action, Action::Converse { .. } | Action::Code { .. })
            })
        });
        queued
            || self.talk.asked.iter().any(|start| start.owner.0 == id)
            || self.due_soon(id, now_ms, within_ms)
    }

    /// The deferred code due at `now_ms` that was deferred first.
    pub(super) fn next_due(&mut self, now_ms: u64) -> Option<Delayed> {
        let at = self
            .delayed
            .iter()
            .enumerate()
            .filter(|(_, d)| d.due_ms <= now_ms)
            .min_by_key(|(_, d)| (d.due_ms, d.order))
            .map(|(at, _)| at)?;
        Some(self.delayed.remove(at))
    }

    /// Appends `action` to `object`'s queue (a script's `Action…`; a
    /// probe's).
    pub fn queue(&mut self, object: ObjectId, action: Action) {
        self.actions.entry(object.0).or_default().push(Queued {
            action,
            started_ms: None,
        });
    }

    /// A location value for `at` (metres) facing `yaw`: the same handle
    /// for the same place.
    pub(super) fn location(&mut self, at: [f32; 3], yaw: f32) -> Handle {
        let same = |(p, f): &([f32; 3], f32)| {
            p.map(f32::to_bits) == at.map(f32::to_bits) && f.to_bits() == yaw.to_bits()
        };
        if let Some(n) = self.locations.iter().position(same) {
            return Handle(n as u32 + 1);
        }
        self.locations.push((at, yaw));
        Handle(self.locations.len() as u32)
    }

    pub(super) fn location_at(&self, handle: Handle) -> Option<([f32; 3], f32)> {
        let n = usize::try_from(handle.0).ok()?.checked_sub(1)?;
        self.locations.get(n).copied()
    }

    /// Every deferred stack (waiting code and queued code).
    fn stacks(&mut self) -> impl Iterator<Item = &mut Deferred> {
        fn code(queued: &mut Queued) -> Option<&mut Deferred> {
            match &mut queued.action {
                Action::Code { deferred, .. } => Some(deferred),
                _ => None,
            }
        }
        // The modules left keep theirs too.
        let left = self.left.values_mut().flat_map(|left| {
            left.delayed
                .iter_mut()
                .map(|delayed| &mut delayed.deferred)
                .chain(left.actions.values_mut().flatten().filter_map(code))
        });
        self.delayed
            .iter_mut()
            .map(|delayed| &mut delayed.deferred)
            .chain(self.actions.values_mut().flatten().filter_map(code))
            .chain(left)
    }

    /// Drops the location, effect and spell event values no deferred code
    /// holds, of each kind past `VALUES_KEPT`, renumbering those it holds.
    /// Only while no script runs: a running one's stack holds values too.
    pub(super) fn compact(&mut self) {
        if self.locations.len() > VALUES_KEPT {
            let old = std::mem::take(&mut self.locations);
            self.locations = compact(old, self.stacks(), 0, |cell| match cell {
                Value::Location(handle) => Some(handle),
                _ => None,
            });
        }
        if self.effects.len() > VALUES_KEPT {
            let old = std::mem::take(&mut self.effects);
            self.effects = compact(old, self.stacks(), 0, |cell| match cell {
                Value::Effect(handle) => Some(handle),
                _ => None,
            });
        }
        if self.spell_events.len() > VALUES_KEPT {
            let old = std::mem::take(&mut self.spell_events);
            self.spell_events = compact(old, self.stacks(), SPELL_EVENT, |cell| match cell {
                Value::Event(handle) if handle.0 & SPELL_EVENT != 0 => Some(handle),
                _ => None,
            });
        }
    }

    /// An effect value for `effect`.
    pub(super) fn effect(&mut self, effect: Effect) -> Handle {
        self.effects.push(effect);
        Handle(self.effects.len() as u32)
    }

    pub(super) fn effect_at(&self, handle: Handle) -> Option<&Effect> {
        let n = usize::try_from(handle.0).ok()?.checked_sub(1)?;
        self.effects.get(n)
    }

    /// A spell event value for `event`.
    pub(super) fn spell_event(&mut self, event: SpellEvent) -> Handle {
        self.spell_events.push(event);
        Handle(SPELL_EVENT | self.spell_events.len() as u32)
    }

    pub(super) fn spell_event_at(&self, handle: Handle) -> Option<SpellEvent> {
        if handle.0 & SPELL_EVENT == 0 {
            return None;
        }
        let n = usize::try_from(handle.0 & !SPELL_EVENT)
            .ok()?
            .checked_sub(1)?;
        self.spell_events.get(n).copied()
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.numbers.len() as u32);
        for (name, value) in &self.numbers {
            out.string(name);
            out.u32(*value as u32);
        }
        out.u32(self.booleans.len() as u32);
        for (name, value) in &self.booleans {
            out.string(name);
            out.u8(u8::from(*value));
        }
        out.u32(self.strings.len() as u32);
        for (name, value) in &self.strings {
            out.string(name);
            out.string(value);
        }
        out.u32(self.places.len() as u32);
        for (name, (at, yaw)) in &self.places {
            out.string(name);
            at.iter().for_each(|axis| out.f32(*axis));
            out.f32(*yaw);
        }
        out.u32(self.unescapable.len() as u32);
        for module in &self.unescapable {
            out.string(module);
        }
        out.u32(self.left.len() as u32);
        for (module, left) in &self.left {
            out.string(module);
            left.encode(out);
        }
        out.u32(self.locals.len() as u32);
        for ((object, kind, index), value) in &self.locals {
            out.u32(*object);
            out.u8(*kind);
            out.u32(*index as u32);
            out.u32(*value as u32);
        }
        encode_pendings(out, &self.pending);
        encode_delayeds(out, &self.delayed);
        out.u64(self.next);
        encode_queues(out, &self.actions);
        out.u32(self.locations.len() as u32);
        for (at, yaw) in &self.locations {
            for v in at {
                out.f32(*v);
            }
            out.f32(*yaw);
        }
        out.u32(self.last.len() as u32);
        for ((object, kind), by) in &self.last {
            out.u32(*object);
            out.u8(*kind);
            out.u32(*by);
        }
        out.u32(self.unoriented.len() as u32);
        for object in &self.unoriented {
            out.u32(*object);
        }
        out.u32(self.uncommandable.len() as u32);
        for object in &self.uncommandable {
            out.u32(*object);
        }
        out.u32(self.occupants.len() as u32);
        for (trigger, inside) in &self.occupants {
            out.u32(*trigger);
            encode_ids(out, inside);
        }
        out.u32(self.spawned.len() as u32);
        for module in &self.spawned {
            out.string(module);
        }
        out.u32(self.perceived.len() as u32);
        for (creature, perceived) in &self.perceived {
            out.u32(*creature);
            for ids in [&perceived.seen, &perceived.heard] {
                encode_ids(out, ids);
            }
        }
        out.u32(self.rounds.len() as u32);
        for (id, at) in &self.rounds {
            out.u32(*id);
            out.u64(*at);
        }
        out.u32(self.effects.len() as u32);
        for effect in &self.effects {
            effect.encode(out);
        }
        out.u32(self.spell_events.len() as u32);
        for (caster, spell, harmful) in &self.spell_events {
            out.u32(caster.0);
            out.u32(*spell as u32);
            out.u8(u8::from(*harmful));
        }
        out.u32(self.applied.len() as u32);
        for applied in &self.applied {
            applied.encode(out);
        }
        out.u32(self.removed.len() as u32);
        for (object, id, after) in &self.removed {
            out.u32(*object);
            out.u32(id.unwrap_or(u32::MAX));
            out.u32(*after);
        }
        out.u32(self.bashes.len() as u32);
        for (door, by) in &self.bashes {
            out.u32(*door);
            out.u32(*by);
        }
        out.u32(self.next_effect);
        out.u32(self.listening.len() as u32);
        for id in &self.listening {
            out.u32(*id);
        }
        out.u32(self.patterns.len() as u32);
        for (id, patterns) in &self.patterns {
            out.u32(*id);
            out.u32(patterns.len() as u32);
            for (pattern, number) in patterns {
                out.string(pattern);
                out.u32(*number as u32);
            }
        }
        out.u32(self.items.len() as u32);
        for (id, item) in &self.items {
            out.u32(*id);
            item.encode(out);
        }
        out.u32(self.next_item);
        out.u32(self.gold as u32);
        self.party.encode(out);
        self.talk.encode(out);
        out.u32(self.heartless.len() as u32);
        for id in &self.heartless {
            out.u32(*id);
        }
        out.u32(self.trades.len() as u32);
        for (player, trade) in &self.trades {
            out.u32(*player);
            trade.encode(out);
        }
        super::journal::encode_journal(out, &self.journal);
        out.u32(self.xp.len() as u32);
        for (holder, xp) in &self.xp {
            out.u32(*holder);
            out.u32(*xp as u32);
        }
        out.u32(self.careers.len() as u32);
        for (id, career) in &self.careers {
            out.u32(*id);
            out.u32(career.len() as u32);
            for (class, levels) in career {
                out.u8(*class);
                out.u8(*levels);
            }
        }
        out.u32(self.levelling.len() as u32);
        for (id, choices) in &self.levelling {
            out.u32(*id);
            choices.encode(out);
        }
        out.u32(self.strikes.len() as u32);
        for strike in &self.strikes {
            strike.encode(out);
        }
        out.u32(self.swung.len() as u32);
        for (id, ms) in &self.swung {
            out.u32(*id);
            out.u64(*ms);
        }
        out.u32(self.known.len() as u32);
        for facts in self.known.values() {
            crate::pack::encode_item(facts, out);
        }
        encode_ids(out, &self.kept.iter().copied().collect::<Vec<_>>());
        for planets in [&self.galaxy.available, &self.galaxy.selectable] {
            encode_ids(
                out,
                &planets.iter().map(|p| u32::from(*p)).collect::<Vec<_>>(),
            );
        }
        out.u8(self.galaxy.selected.unwrap_or(u8::MAX));
        encode_corpses(out, &self.corpses);
        out.u32(self.purses.len() as u32);
        for (id, gold) in &self.purses {
            out.u32(*id);
            out.u32(*gold as u32);
        }
        out.u32(self.goals.len() as u32);
        for (id, goal) in &self.goals {
            out.u32(*id);
            for v in goal.point {
                out.f32(v);
            }
            out.f32(goal.range);
            out.u8(u8::from(goal.run));
        }
        out.u32(self.intents.len() as u32);
        for (id, intent) in &self.intents {
            out.u32(*id);
            out.u32(intent.target.0);
            out.u8(u8::from(intent.passive));
            encode_use(out, intent.talent);
        }
        self.watch.encode(out);
        out.u32(self.reputation.len() as u32);
        for (&(a, b), by) in &self.reputation {
            out.u8(a);
            out.u8(b);
            out.u16(*by as u16);
        }
        out.u32(self.traps.len() as u32);
        for (id, trap) in &self.traps {
            out.u32(*id);
            encode_ids(out, &trap.detected_by);
            out.u32(trap.creator.unwrap_or(u32::MAX));
            out.u8(trap.faction.unwrap_or(u8::MAX));
            match trap.laid {
                Some(laid) => out.bytes(&[1, laid.detect_dc, laid.disarm_dc]),
                None => out.bytes(&[0, 0, 0]),
            }
            out.u8(u8::from(trap.disabled));
            out.i8(trap.owner_demolitions);
        }
        out.u64(self.last_detect_ms);
        out.u32(self.perception.len() as u32);
        for (id, (next, rolls)) in &self.perception {
            out.u32(*id);
            out.u64(*next);
            rolls.encode(out);
        }
        out.u32(self.stealth_xp.len() as u32);
        for (area, xp) in &self.stealth_xp {
            out.string(area);
            for value in [xp.max, xp.current, xp.loss] {
                out.u32(value as u32);
            }
            out.u8(u8::from(xp.enabled));
        }
        out.u32(self.stealth_loss.len() as u32);
        for (id, at) in &self.stealth_loss {
            out.u32(*id);
            out.u64(*at);
        }
        out.u32(self.skill_uses.len() as u32);
        for (seat, used) in &self.skill_uses {
            out.u8(*seat);
            out.u8(UseKind::ALL
                .iter()
                .position(|kind| *kind == used.kind)
                .unwrap_or(0) as u8);
            out.u32(used.by);
            out.u32(used.object);
            out.u32(used.tool.unwrap_or(u32::MAX));
            for v in used.from {
                out.f32(v);
            }
            out.u64(used.until);
        }
        out.u32(self.door_mines.len() as u32);
        for (door, mine) in &self.door_mines {
            for id in [*door, mine.setter, mine.mine] {
                out.u32(id);
            }
            out.u8(mine.strength);
            out.u64(mine.blast_at);
            out.u64(mine.open_at.unwrap_or(u64::MAX));
        }
        crate::pazaak::encode_tables(&self.pazaak, out);
        out.u8(u8::from(self.pazaak_won));
        out.u32(self.components as u32);
        out.u32(self.chemicals as u32);
        out.u32(self.hoards.len() as u32);
        for (id, (components, chemicals)) in &self.hoards {
            out.u32(*id);
            out.u32(*components as u32);
            out.u32(*chemicals as u32);
        }
        out.u32(self.beats.len() as u32);
        for (id, at) in &self.beats {
            out.u32(*id);
            out.u64(*at);
        }
        out.u32(self.transit.len() as u32);
        for (module, disabled) in &self.transit {
            out.string(module);
            out.u8(u8::from(*disabled));
        }
        let (show, label, query) = self.return_button;
        out.u8(u8::from(show));
        out.u32(label);
        out.u32(query);
        crate::spawner::encode_spawns(out, &self.encounters);
        out.u32(self.floor.len() as u32);
        for (id, (module, lying)) in &self.floor {
            out.u32(*id);
            out.string(module);
            lying.encode(out);
        }
    }

    /// An item template's facts: the module's catalog's, else those kept
    /// from the module its item came from (`known`).
    pub(crate) fn item_facts<'a>(
        &'a self,
        catalog: &'a crate::inventory::Catalog,
        template: &str,
    ) -> Option<&'a crate::inventory::ItemFacts> {
        catalog.get(template).or_else(|| self.known.get(template))
    }

    /// `known` for a module whose catalog is `next`, leaving one whose
    /// catalog was `last`: the facts of every item in play `next` lacks.
    pub(crate) fn carry_known(
        &mut self,
        last: &crate::inventory::Catalog,
        next: &crate::inventory::Catalog,
    ) {
        let known = self
            .items
            .values()
            .filter(|item| next.get(&item.template).is_none())
            .filter_map(|item| self.item_facts(last, &item.template).cloned())
            .map(|facts| (facts.template.clone(), facts))
            .collect();
        self.known = known;
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<State, DecodeError> {
        let mut state = State::default();
        for _ in 0..input.u32()? {
            state.numbers.insert(input.string()?, input.u32()? as i32);
        }
        for _ in 0..input.u32()? {
            state.booleans.insert(input.string()?, input.u8()? != 0);
        }
        for _ in 0..input.u32()? {
            state.strings.insert(input.string()?, input.string()?);
        }
        for _ in 0..input.u32()? {
            let name = input.string()?;
            let at = [input.f32()?, input.f32()?, input.f32()?];
            state.places.insert(name, (at, input.f32()?));
        }
        for _ in 0..input.u32()? {
            state.unescapable.insert(input.string()?);
        }
        for _ in 0..input.u32()? {
            let module = input.string()?;
            state.left.insert(module, Left::decode(input)?);
        }
        for _ in 0..input.u32()? {
            let key = (input.u32()?, input.u8()?, input.u32()? as i32);
            state.locals.insert(key, input.u32()? as i32);
        }
        state.pending = decode_pendings(input)?;
        state.delayed = decode_delayeds(input)?;
        state.next = input.u64()?;
        state.actions = decode_queues(input)?;
        for _ in 0..input.u32()? {
            let at = [input.f32()?, input.f32()?, input.f32()?];
            state.locations.push((at, input.f32()?));
        }
        for _ in 0..input.u32()? {
            let key = (input.u32()?, input.u8()?);
            state.last.insert(key, input.u32()?);
        }
        for _ in 0..input.u32()? {
            state.unoriented.insert(input.u32()?);
        }
        for _ in 0..input.u32()? {
            state.uncommandable.insert(input.u32()?);
        }
        for _ in 0..input.u32()? {
            let trigger = input.u32()?;
            state.occupants.insert(trigger, decode_ids(input)?);
        }
        for _ in 0..input.u32()? {
            state.spawned.insert(input.string()?);
        }
        for _ in 0..input.u32()? {
            let creature = input.u32()?;
            let seen = decode_ids(input)?;
            let heard = decode_ids(input)?;
            state.perceived.insert(creature, Perceived { seen, heard });
        }
        for _ in 0..input.u32()? {
            state.rounds.insert(input.u32()?, input.u64()?);
        }
        for _ in 0..input.u32()? {
            state.effects.push(Effect::decode(input)?);
        }
        for _ in 0..input.u32()? {
            let caster = ObjectId(input.u32()?);
            let spell = input.u32()? as i32;
            state.spell_events.push((caster, spell, input.u8()? != 0));
        }
        for _ in 0..input.u32()? {
            state.applied.push(Applied::decode(input)?);
        }
        for _ in 0..input.u32()? {
            let object = input.u32()?;
            let id = Some(input.u32()?).filter(|id| *id != u32::MAX);
            state.removed.push((object, id, input.u32()?));
        }
        for _ in 0..input.u32()? {
            state.bashes.push((input.u32()?, input.u32()?));
        }
        state.next_effect = input.u32()?;
        for _ in 0..input.u32()? {
            state.listening.insert(input.u32()?);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let patterns = (0..input.u32()?)
                .map(|_| Ok((input.string()?, input.u32()? as i32)))
                .collect::<Result<_, DecodeError>>()?;
            state.patterns.insert(id, patterns);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            state.items.insert(id, Item::decode(input)?);
        }
        state.next_item = input.u32()?;
        state.gold = input.u32()? as i32;
        state.party = Party::decode(input)?;
        state.talk = Talk::decode(input)?;
        for _ in 0..input.u32()? {
            state.heartless.insert(input.u32()?);
        }
        for _ in 0..input.u32()? {
            let player = input.u32()?;
            state
                .trades
                .insert(player, crate::inventory::Trade::decode(input)?);
        }
        state.journal = super::journal::decode_journal(input)?;
        for _ in 0..input.u32()? {
            state.xp.insert(input.u32()?, input.u32()? as i32);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let career = (0..input.u32()?)
                .map(|_| Ok((input.u8()?, input.u8()?)))
                .collect::<Result<_, DecodeError>>()?;
            state.careers.insert(id, career);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let choices = crate::levels::Choices::decode(input)?;
            state.levelling.push((id, choices));
        }
        for _ in 0..input.u32()? {
            state.strikes.push(Strike::decode(input)?);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            state.swung.insert(id, input.u64()?);
        }
        for _ in 0..input.u32()? {
            let facts = crate::pack::decode_item(input)?;
            state.known.insert(facts.template.clone(), facts);
        }
        state.kept = decode_ids(input)?.into_iter().collect();
        let mut planets = || -> Result<BTreeSet<u8>, DecodeError> {
            Ok(decode_ids(input)?
                .into_iter()
                .filter_map(|p| u8::try_from(p).ok())
                .collect())
        };
        state.galaxy.available = planets()?;
        state.galaxy.selectable = planets()?;
        state.galaxy.selected = Some(input.u8()?).filter(|p| *p != u8::MAX);
        state.corpses = decode_corpses(input)?;
        for _ in 0..input.u32()? {
            state.purses.insert(input.u32()?, input.u32()? as i32);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let goal = Goal {
                point: [input.f32()?, input.f32()?, input.f32()?],
                range: input.f32()?,
                run: input.u8()? != 0,
            };
            state.goals.insert(id, goal);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let intent = Intent {
                target: ObjectId(input.u32()?),
                passive: input.u8()? != 0,
                talent: decode_use(input)?,
            };
            state.intents.insert(id, intent);
        }
        state.watch = Watch::decode(input)?;
        for _ in 0..input.u32()? {
            let pair = (input.u8()?, input.u8()?);
            state.reputation.insert(pair, input.u16()? as i16);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let detected_by = decode_ids(input)?;
            let creator = Some(input.u32()?).filter(|id| *id != u32::MAX);
            let faction = Some(input.u8()?).filter(|faction| *faction != u8::MAX);
            let [laid, detect_dc, disarm_dc] = input.array()?;
            let trap = TrapState {
                detected_by,
                creator,
                faction,
                laid: (laid != 0).then_some(crate::traps::Laid {
                    detect_dc,
                    disarm_dc,
                }),
                disabled: input.u8()? != 0,
                owner_demolitions: input.i8()?,
            };
            state.traps.insert(id, trap);
        }
        state.last_detect_ms = input.u64()?;
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let next = input.u64()?;
            state
                .perception
                .insert(id, (next, crate::stealth::Rolls::decode(input)?));
        }
        for _ in 0..input.u32()? {
            let area = input.string()?;
            let xp = StealthXp {
                max: input.u32()? as i32,
                current: input.u32()? as i32,
                loss: input.u32()? as i32,
                enabled: input.u8()? != 0,
            };
            state.stealth_xp.insert(area, xp);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            state.stealth_loss.insert(id, input.u64()?);
        }
        for _ in 0..input.u32()? {
            let seat = input.u8()?;
            let at = input.at();
            let kind = *UseKind::ALL
                .get(usize::from(input.u8()?))
                .ok_or_else(|| input.invalid("skill use", at))?;
            let used = SkillUse {
                kind,
                by: input.u32()?,
                object: input.u32()?,
                tool: optional_id(input)?.map(|id| id.0),
                from: [input.f32()?, input.f32()?, input.f32()?],
                until: input.u64()?,
            };
            state.skill_uses.insert(seat, used);
        }
        for _ in 0..input.u32()? {
            let door = input.u32()?;
            let mine = DoorMine {
                setter: input.u32()?,
                mine: input.u32()?,
                strength: input.u8()?,
                blast_at: input.u64()?,
                open_at: Some(input.u64()?).filter(|at| *at != u64::MAX),
            };
            state.door_mines.insert(door, mine);
        }
        state.pazaak = crate::pazaak::decode_tables(input)?;
        state.pazaak_won = input.u8()? != 0;
        state.components = input.u32()? as i32;
        state.chemicals = input.u32()? as i32;
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let hoard = (input.u32()? as i32, input.u32()? as i32);
            state.hoards.insert(id, hoard);
        }
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            state.beats.insert(id, input.u64()?);
        }
        for _ in 0..input.u32()? {
            let module = input.string()?;
            state.transit.insert(module, input.u8()? != 0);
        }
        state.return_button = (input.u8()? != 0, input.u32()?, input.u32()?);
        state.encounters = crate::spawner::decode_spawns(input)?;
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let module = input.string()?;
            let lying = crate::inventory::Lying::decode(input)?;
            state.floor.insert(id, (module, lying));
        }
        Ok(state)
    }
}

impl Watch {
    fn encode(&self, out: &mut Writer) {
        out.u32(self.seen.len() as u32);
        for (id, watched) in &self.seen {
            out.u32(*id);
            out.u32(watched.hp as u32);
            out.u8(u8::from(watched.alive) | u8::from(watched.dead) << 1);
            out.u64(watched.hit_at);
            out.u64(watched.damaged_at.unwrap_or(u64::MAX));
        }
        for last in [&self.attacked, &self.spelled] {
            out.u32(last.len() as u32);
            for ((victim, by), at) in last {
                out.u32(*victim);
                out.u32(*by);
                out.u64(*at);
            }
        }
        match &self.held {
            Some(held) => {
                out.u8(1);
                encode_ids(out, &held.iter().copied().collect::<Vec<_>>());
            }
            None => out.u8(0),
        }
    }

    fn decode(input: &mut Reader) -> Result<Watch, DecodeError> {
        let mut watch = Watch::default();
        for _ in 0..input.u32()? {
            let id = input.u32()?;
            let hp = input.u32()? as i32;
            let life = input.u8()?;
            let watched = Watched {
                hp,
                alive: life & 1 != 0,
                dead: life & 2 != 0,
                hit_at: input.u64()?,
                damaged_at: Some(input.u64()?).filter(|at| *at != u64::MAX),
            };
            watch.seen.insert(id, watched);
        }
        for last in [&mut watch.attacked, &mut watch.spelled] {
            for _ in 0..input.u32()? {
                let key = (input.u32()?, input.u32()?);
                last.insert(key, input.u64()?);
            }
        }
        if input.u8()? != 0 {
            watch.held = Some(decode_ids(input)?.into_iter().collect());
        }
        Ok(watch)
    }
}

fn encode_ids(out: &mut Writer, ids: &[u32]) {
    out.u32(ids.len() as u32);
    for id in ids {
        out.u32(*id);
    }
}

fn decode_ids(input: &mut Reader) -> Result<Vec<u32>, DecodeError> {
    (0..input.u32()?).map(|_| input.u32()).collect()
}

/// Keeps only the values of `old` a deferred stack holds (`pick` gives a
/// cell's handle into them: `bits` and the value's number from 1),
/// renumbered there.
fn compact<'a, T: Clone>(
    old: Vec<T>,
    stacks: impl Iterator<Item = &'a mut Deferred>,
    bits: u32,
    pick: fn(&mut Value) -> Option<&mut Handle>,
) -> Vec<T> {
    let mut kept: BTreeMap<u32, u32> = BTreeMap::new();
    let mut fresh = Vec::new();
    for deferred in stacks {
        for cell in deferred.globals.iter_mut().chain(&mut deferred.locals) {
            if let Some(handle) = pick(cell)
                && let Some(value) = usize::try_from(handle.0 & !bits)
                    .ok()
                    .and_then(|n| old.get(n.checked_sub(1)?))
            {
                let n = *kept.entry(handle.0).or_insert_with(|| {
                    fresh.push(value.clone());
                    fresh.len() as u32
                });
                *handle = Handle(bits | n);
            }
        }
    }
    fresh
}

/// A conversation's id, `u32::MAX` for none.
fn optional_talk(input: &mut Reader) -> Result<Option<u32>, DecodeError> {
    Ok(Some(input.u32()?).filter(|id| *id != u32::MAX))
}

fn optional_id(input: &mut Reader) -> Result<Option<ObjectId>, DecodeError> {
    Ok(Some(input.u32()?)
        .filter(|id| *id != u32::MAX)
        .map(ObjectId))
}

fn encode_cast(out: &mut Writer, cast: Option<&Cast>) {
    let Some(cast) = cast else {
        out.u8(0);
        return;
    };
    out.u8(1);
    out.u32(cast.caster.0);
    out.u16(cast.row);
    out.u32(cast.target.0);
    for axis in cast.at {
        out.f32(axis);
    }
    out.u32(cast.caught.len() as u32);
    for id in &cast.caught {
        out.u32(id.0);
    }
}

fn decode_cast(input: &mut Reader) -> Result<Option<Cast>, DecodeError> {
    if input.u8()? == 0 {
        return Ok(None);
    }
    let caster = ObjectId(input.u32()?);
    let row = input.u16()?;
    let target = ObjectId(input.u32()?);
    let at = [input.f32()?, input.f32()?, input.f32()?];
    let caught = (0..input.u32()?)
        .map(|_| input.u32().map(ObjectId))
        .collect::<Result<_, _>>()?;
    Ok(Some(Cast {
        caster,
        row,
        target,
        at,
        caught,
    }))
}

fn encode_deferred(out: &mut Writer, deferred: &Deferred) {
    out.u32(deferred.resume);
    for cells in [&deferred.globals, &deferred.locals] {
        out.u32(cells.len() as u32);
        for cell in cells {
            encode_value(out, cell);
        }
    }
}

fn decode_deferred(input: &mut Reader) -> Result<Deferred, DecodeError> {
    let resume = input.u32()?;
    let mut cells = || -> Result<Vec<Value>, DecodeError> {
        (0..input.u32()?).map(|_| decode_value(input)).collect()
    };
    let (globals, locals) = (cells()?, cells()?);
    Ok(Deferred {
        resume,
        globals,
        locals,
    })
}

fn encode_target(out: &mut Writer, target: &Target) {
    match target {
        Target::Object(id) => {
            out.u8(0);
            out.u32(id.0);
        }
        Target::Point(at, yaw) => {
            out.u8(1);
            for v in at {
                out.f32(*v);
            }
            out.f32(*yaw);
        }
    }
}

fn decode_target(input: &mut Reader) -> Result<Target, DecodeError> {
    let at = input.at();
    Ok(match input.u8()? {
        0 => Target::Object(ObjectId(input.u32()?)),
        1 => Target::Point([input.f32()?, input.f32()?, input.f32()?], input.f32()?),
        _ => return Err(input.invalid("action target", at)),
    })
}

/// A talent: its kind (0 a power, 1 a feat, 255 none) and row.
fn encode_use(out: &mut Writer, talent: Option<Use>) {
    let (kind, row) = match talent {
        Some(Use::Power(row)) => (0, row),
        Some(Use::Feat(row)) => (1, row),
        None => (u8::MAX, 0),
    };
    out.u8(kind);
    out.u16(row);
}

fn decode_use(input: &mut Reader) -> Result<Option<Use>, DecodeError> {
    let kind = input.u8()?;
    let row = input.u16()?;
    Ok(match kind {
        0 => Some(Use::Power(row)),
        u8::MAX => None,
        _ => Some(Use::Feat(row)),
    })
}

fn encode_action(out: &mut Writer, action: &Action) {
    match action {
        Action::Code { script, deferred } => {
            out.u8(0);
            out.string(script);
            encode_deferred(out, deferred);
        }
        Action::Wait { ms } => {
            out.u8(1);
            out.u32(*ms);
        }
        Action::Animate { animation, ms } => {
            out.u8(9);
            out.u16(*animation);
            out.u32(*ms);
        }
        Action::Move {
            to,
            run,
            range,
            force_ms,
        } => {
            out.u8(2);
            encode_target(out, to);
            out.u8(u8::from(*run));
            out.f32(*range);
            out.u32(force_ms.unwrap_or(u32::MAX));
        }
        Action::Jump { to } => {
            out.u8(3);
            encode_target(out, to);
        }
        Action::Door { door, open } => {
            out.u8(4);
            out.u32(door.0);
            out.u8(u8::from(*open));
        }
        Action::Lock { object, locked } => {
            out.u8(12);
            out.u32(object.0);
            out.u8(u8::from(*locked));
        }
        Action::UseObject { object } => {
            out.u8(13);
            out.u32(object.0);
        }
        Action::Bash { door } => {
            out.u8(14);
            out.u32(door.0);
        }
        Action::SwitchWeapons => out.u8(15),
        Action::Instant { routine } => {
            out.u8(5);
            out.string(routine);
        }
        Action::Attack { target, passive } => {
            out.u8(6);
            out.u32(target.0);
            out.u8(u8::from(*passive));
        }
        Action::RandomWalk { from, to } => {
            out.u8(8);
            for point in [from, to] {
                match point {
                    Some(point) => {
                        out.u8(1);
                        for v in point {
                            out.f32(*v);
                        }
                    }
                    None => out.u8(0),
                }
            }
        }
        Action::Converse {
            with,
            dialog,
            asker,
        } => {
            out.u8(10);
            out.u32(with.0);
            out.string(dialog);
            out.u32(asker.map_or(u32::MAX, |id| id.0));
        }
        Action::Hold { talk } => {
            out.u8(11);
            out.u32(talk.unwrap_or(u32::MAX));
        }
        Action::Pause { talk } => {
            out.u8(PAUSE);
            out.u32(talk.unwrap_or(u32::MAX));
        }
        Action::Talent { talent, target } => {
            out.u8(7);
            encode_use(out, Some(*talent));
            out.u32(target.0);
        }
        Action::Cast {
            row,
            target,
            fake,
            cheat,
        } => {
            out.u8(45);
            out.u16(*row);
            encode_target(out, target);
            out.u8(u8::from(*fake) | u8::from(*cheat) << 1);
        }
        Action::Surrender => out.u8(46),
        Action::Bark { strref } => {
            out.u8(BARK);
            out.u32(*strref);
        }
        Action::FollowOwner { range } => {
            out.u8(FOLLOW_OWNER);
            out.f32(*range);
        }
    }
}

/// A queued bark's and a puppet's follow's codes in a save (I35's own
/// numbers, apart from the rest).
const BARK: u8 = 43;
const FOLLOW_OWNER: u8 = 57;
/// A queued pause's code in a save.
const PAUSE: u8 = 58;

fn decode_action(input: &mut Reader) -> Result<Action, DecodeError> {
    let at = input.at();
    Ok(match input.u8()? {
        0 => Action::Code {
            script: input.string()?,
            deferred: decode_deferred(input)?,
        },
        1 => Action::Wait { ms: input.u32()? },
        9 => Action::Animate {
            animation: input.u16()?,
            ms: input.u32()?,
        },
        2 => Action::Move {
            to: decode_target(input)?,
            run: input.u8()? != 0,
            range: input.f32()?,
            force_ms: Some(input.u32()?).filter(|ms| *ms != u32::MAX),
        },
        3 => Action::Jump {
            to: decode_target(input)?,
        },
        4 => Action::Door {
            door: ObjectId(input.u32()?),
            open: input.u8()? != 0,
        },
        5 => Action::Instant {
            routine: input.string()?,
        },
        13 => Action::UseObject {
            object: ObjectId(input.u32()?),
        },
        14 => Action::Bash {
            door: ObjectId(input.u32()?),
        },
        15 => Action::SwitchWeapons,
        12 => Action::Lock {
            object: ObjectId(input.u32()?),
            locked: input.u8()? != 0,
        },
        6 => Action::Attack {
            target: ObjectId(input.u32()?),
            passive: input.u8()? != 0,
        },
        8 => {
            let mut point = || -> Result<Option<[f32; 3]>, DecodeError> {
                Ok(match input.u8()? {
                    0 => None,
                    _ => Some([input.f32()?, input.f32()?, input.f32()?]),
                })
            };
            Action::RandomWalk {
                from: point()?,
                to: point()?,
            }
        }
        10 => Action::Converse {
            with: ObjectId(input.u32()?),
            dialog: input.string()?,
            asker: optional_id(input)?,
        },
        11 => Action::Hold {
            talk: optional_talk(input)?,
        },
        PAUSE => Action::Pause {
            talk: optional_talk(input)?,
        },
        7 => {
            let at = input.at();
            Action::Talent {
                talent: decode_use(input)?.ok_or_else(|| input.invalid("talent", at))?,
                target: ObjectId(input.u32()?),
            }
        }
        45 => {
            let (row, target) = (input.u16()?, decode_target(input)?);
            let flags = input.u8()?;
            Action::Cast {
                row,
                target,
                fake: flags & 1 != 0,
                cheat: flags & 2 != 0,
            }
        }
        46 => Action::Surrender,
        BARK => Action::Bark {
            strref: input.u32()?,
        },
        FOLLOW_OWNER => Action::FollowOwner {
            range: input.f32()?,
        },
        _ => return Err(input.invalid("action", at)),
    })
}

/// A stack cell in a save (vectors and actions are never cells).
fn encode_value(out: &mut Writer, value: &Value) {
    match value {
        Value::Int(v) => {
            out.u8(0);
            out.u32(*v as u32);
        }
        Value::Float(v) => {
            out.u8(1);
            out.f32(*v);
        }
        Value::String(v) => {
            out.u8(2);
            out.string(v);
        }
        Value::Object(v) => {
            out.u8(3);
            out.u32(v.0);
        }
        Value::Effect(h) | Value::Event(h) | Value::Location(h) | Value::Talent(h) => {
            out.u8(match value {
                Value::Effect(_) => 4,
                Value::Event(_) => 5,
                Value::Location(_) => 6,
                _ => 7,
            });
            out.u32(h.0);
        }
        Value::Vector(_) | Value::Action(_) => out.u8(255),
    }
}

fn decode_value(input: &mut Reader) -> Result<Value, DecodeError> {
    let at = input.at();
    Ok(match input.u8()? {
        0 => Value::Int(input.u32()? as i32),
        1 => Value::Float(input.f32()?),
        2 => Value::String(Arc::from(input.string()?)),
        3 => Value::Object(ObjectId(input.u32()?)),
        4 => Value::Effect(Handle(input.u32()?)),
        5 => Value::Event(Handle(input.u32()?)),
        6 => Value::Location(Handle(input.u32()?)),
        7 => Value::Talent(Handle(input.u32()?)),
        _ => return Err(input.invalid("script cell", at)),
    })
}

fn encode_pendings(out: &mut Writer, pendings: &[Pending]) {
    out.u32(pendings.len() as u32);
    for pending in pendings {
        out.string(&pending.script);
        out.u32(pending.caller.0);
        out.u32(pending.entering.map_or(u32::MAX, |id| id.0));
        out.u32(pending.number as u32);
        out.u8(pending.perception);
        out.u8(u8::from(pending.ai));
        out.u32(pending.leads.map_or(u32::MAX, |id| id.0));
        out.u32(pending.asker.map_or(u32::MAX, |id| id.0));
        out.u32(pending.script_var as u32);
    }
}

fn decode_pendings(input: &mut Reader) -> Result<Vec<Pending>, DecodeError> {
    (0..input.u32()?)
        .map(|_| {
            let script = input.string()?;
            let caller = ObjectId(input.u32()?);
            let entering = optional_id(input)?;
            Ok(Pending {
                script,
                caller,
                entering,
                number: input.u32()? as i32,
                perception: input.u8()?,
                ai: input.u8()? != 0,
                leads: optional_id(input)?,
                asker: optional_id(input)?,
                script_var: input.u32()? as i32,
            })
        })
        .collect()
}

fn encode_delayeds(out: &mut Writer, delayeds: &[Delayed]) {
    out.u32(delayeds.len() as u32);
    for delayed in delayeds {
        out.u64(delayed.due_ms);
        out.u64(delayed.order);
        out.u32(delayed.caller.0);
        out.string(&delayed.script);
        encode_deferred(out, &delayed.deferred);
        encode_cast(out, delayed.spell.as_ref());
        out.u32(delayed.talk.unwrap_or(u32::MAX));
    }
}

fn decode_delayeds(input: &mut Reader) -> Result<Vec<Delayed>, DecodeError> {
    (0..input.u32()?)
        .map(|_| {
            let due_ms = input.u64()?;
            let order = input.u64()?;
            let caller = ObjectId(input.u32()?);
            let script = input.string()?;
            Ok(Delayed {
                due_ms,
                order,
                caller,
                script,
                deferred: decode_deferred(input)?,
                spell: decode_cast(input)?,
                talk: optional_talk(input)?,
            })
        })
        .collect()
}

fn encode_queues(out: &mut Writer, queues: &BTreeMap<u32, Vec<Queued>>) {
    out.u32(queues.len() as u32);
    for (object, queue) in queues {
        out.u32(*object);
        out.u32(queue.len() as u32);
        for queued in queue {
            out.u64(queued.started_ms.unwrap_or(u64::MAX));
            encode_action(out, &queued.action);
        }
    }
}

fn decode_queues(input: &mut Reader) -> Result<BTreeMap<u32, Vec<Queued>>, DecodeError> {
    let mut queues = BTreeMap::new();
    for _ in 0..input.u32()? {
        let object = input.u32()?;
        let mut queue = Vec::new();
        for _ in 0..input.u32()? {
            let started_ms = Some(input.u64()?).filter(|ms| *ms != u64::MAX);
            queue.push(Queued {
                action: decode_action(input)?,
                started_ms,
            });
        }
        queues.insert(object, queue);
    }
    Ok(queues)
}

/// What scripts left waiting in a module the party left (KOTOR keeps it
/// with its module, which stands still while away): its objects' and its
/// own events, deferred code, action queues and locks (`SetCommandable`),
/// its dead the engine was to take away, its encounters' state, and when
/// it was left (sim ms).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Left {
    pub at_ms: u64,
    pub pending: Vec<Pending>,
    pub delayed: Vec<Delayed>,
    pub actions: BTreeMap<u32, Vec<Queued>>,
    pub uncommandable: BTreeSet<u32>,
    pub corpses: BTreeMap<u32, Corpse>,
    pub encounters: BTreeMap<u32, crate::spawner::Spawn>,
}

impl Left {
    fn encode(&self, out: &mut Writer) {
        out.u64(self.at_ms);
        encode_pendings(out, &self.pending);
        encode_delayeds(out, &self.delayed);
        encode_queues(out, &self.actions);
        encode_ids(
            out,
            &self.uncommandable.iter().copied().collect::<Vec<u32>>(),
        );
        encode_corpses(out, &self.corpses);
        crate::spawner::encode_spawns(out, &self.encounters);
    }

    fn decode(input: &mut Reader) -> Result<Left, DecodeError> {
        Ok(Left {
            at_ms: input.u64()?,
            pending: decode_pendings(input)?,
            delayed: decode_delayeds(input)?,
            actions: decode_queues(input)?,
            uncommandable: decode_ids(input)?.into_iter().collect(),
            corpses: decode_corpses(input)?,
            encounters: crate::spawner::decode_spawns(input)?,
        })
    }
}

fn encode_corpses(out: &mut Writer, corpses: &BTreeMap<u32, Corpse>) {
    out.u32(corpses.len() as u32);
    for (id, corpse) in corpses {
        out.u32(*id);
        corpse.encode(out);
    }
}

fn decode_corpses(input: &mut Reader) -> Result<BTreeMap<u32, Corpse>, DecodeError> {
    (0..input.u32()?)
        .map(|_| Ok((input.u32()?, Corpse::decode(input)?)))
        .collect()
}

impl State {
    /// The gold `holder` has: the party's (`inventory::PARTY`), else a
    /// creature's own.
    pub fn gold_of(&self, holder: u32) -> i32 {
        if holder == crate::inventory::PARTY {
            self.gold
        } else {
            self.purses.get(&holder).copied().unwrap_or(0)
        }
    }

    /// `amount` more gold for `holder` (less for a negative one, never
    /// under none); how much it moved.
    pub(crate) fn add_gold(&mut self, holder: u32, amount: i32) -> i32 {
        let gold = if holder == crate::inventory::PARTY {
            &mut self.gold
        } else {
            self.purses.entry(holder).or_insert(0)
        };
        let was = *gold;
        *gold = gold.saturating_add(amount).max(0);
        *gold - was
    }

    /// Module `module` left at `now`: what waits for its objects (`ids`)
    /// and for the module and its area themselves goes with it (`left`).
    pub fn leave_module(&mut self, module: &str, ids: &BTreeSet<u32>, now: u64) {
        let its = |id: ObjectId| ids.contains(&id.0) || id == super::MODULE || id == super::AREA;
        let mut left = Left {
            at_ms: now,
            ..Left::default()
        };
        let (pending, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| its(p.caller));
        (left.pending, self.pending) = (pending, kept);
        let (delayed, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.delayed)
            .into_iter()
            .partition(|d| its(d.caller));
        (left.delayed, self.delayed) = (delayed, kept);
        let ours: Vec<u32> = self
            .actions
            .keys()
            .copied()
            .filter(|id| ids.contains(id))
            .collect();
        for id in ours {
            if let Some(queue) = self.actions.remove(&id) {
                left.actions.insert(id, queue);
            }
        }
        let locked: Vec<u32> = self
            .uncommandable
            .iter()
            .copied()
            .filter(|id| ids.contains(id))
            .collect();
        for id in locked {
            self.uncommandable.remove(&id);
            left.uncommandable.insert(id);
        }
        let dead: Vec<u32> = self
            .corpses
            .keys()
            .copied()
            .filter(|id| ids.contains(id))
            .collect();
        for id in dead {
            if let Some(corpse) = self.corpses.remove(&id) {
                left.corpses.insert(id, corpse);
            }
        }
        let ours: Vec<u32> = self
            .encounters
            .keys()
            .copied()
            .filter(|id| ids.contains(id))
            .collect();
        for id in ours {
            if let Some(spawn) = self.encounters.remove(&id) {
                left.encounters.insert(id, spawn);
            }
        }
        let waiting = !(left.pending.is_empty()
            && left.delayed.is_empty()
            && left.actions.is_empty()
            && left.uncommandable.is_empty()
            && left.corpses.is_empty()
            && left.encounters.is_empty());
        if waiting {
            self.left.insert(key(module), left);
        }
    }

    /// Module `module` entered again at `now`: what it left waiting goes on
    /// where it stopped, its times moved on by the time away.
    pub fn return_to(&mut self, module: &str, now: u64) {
        let Some(left) = self.left.remove(&key(module)) else {
            return;
        };
        let away = now.saturating_sub(left.at_ms);
        self.pending.extend(left.pending);
        self.delayed
            .extend(left.delayed.into_iter().map(|mut delayed| {
                delayed.due_ms = delayed.due_ms.saturating_add(away);
                delayed
            }));
        for (id, mut queue) in left.actions {
            for queued in &mut queue {
                queued.started_ms = queued.started_ms.map(|ms| ms.saturating_add(away));
            }
            self.actions.insert(id, queue);
        }
        self.uncommandable.extend(left.uncommandable);
        self.corpses
            .extend(left.corpses.into_iter().map(|(id, mut corpse)| {
                corpse.due_ms = corpse.due_ms.saturating_add(away);
                (id, corpse)
            }));
        self.encounters
            .extend(left.encounters.into_iter().map(|(id, mut spawn)| {
                for at in [&mut spawn.last_spawn_ms, &mut spawn.heartbeat_ms] {
                    *at = at.map(|at| at.saturating_add(away));
                }
                (id, spawn)
            }));
    }
}
