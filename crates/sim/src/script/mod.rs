//! The game's scripts in the sim (I16): what a level's scripts are
//! (`Library`), what they keep between runs (`State`: globals, objects'
//! locals, events waiting, deferred code), and the engine routines they
//! call (`Engine`, over a `View` of the world).
//!
//! Scripts run on the authority only, inside the step (`world::scripts`),
//! never in prediction: what they change reaches clients through
//! snapshots. Their randomness is the authority stream; nothing else they
//! read changes outside the step, so a replay runs them the same way. Their
//! state is not in snapshots; a save carries it (`Save::script`).
//!
//! When scripts run (until I17 brings objects' events and action queues):
//! - a module's install queues its `Mod_OnModLoad` (the module calling),
//!   then for each player `Mod_OnClientEntr` (the module) and the area's
//!   `OnEnter` (the area), the player the entering object;
//! - `DelayCommand` runs its code when due; `AssignCommand` and
//!   `ActionDoCommand` run theirs after the running script, the same tick
//!   (stand-ins for action queues); `ExecuteScript` runs at once, nested.
//!
//! A routine without a binding here is a gap: it returns its type's empty
//! value and is counted (`Report`).

pub mod actions;
pub mod dialogue;
#[cfg(feature = "probes")]
pub mod dialogue_probes;
pub mod effects;
mod engine;
mod force;
#[cfg(feature = "probes")]
pub(crate) use force::random_destination;
pub mod hooks;
pub mod influence;
pub mod items;
pub mod journal;
pub mod levelling;
mod locks;
mod minigame;
mod pazaak;
pub(crate) use minigame::commit as commit_race;
#[cfg(feature = "probes")]
pub(crate) use pazaak::deal as deal_pazaak;
pub(crate) use pazaak::{WatchRule, Watcher};
pub mod party;
pub mod presentation;
mod state;
mod stores;
mod traps;
pub mod workbench;
pub mod xp;

pub use actions::DOOR_REACH;
#[cfg(feature = "probes")]
pub(crate) use engine::MOVE_TO_LOCATION_RANGE;
pub use engine::last;
pub use engine::{CONVERSATION_RANGE, bound};
pub use hooks::{Holder, Hook};
pub use presentation::ENGINE_SCRIPTS;

use std::collections::BTreeMap;
use std::sync::Arc;

use fmt_odyssey::dlg::Dlg;
use fmt_odyssey::ncs::Ncs;
use fmt_odyssey::tlk::Tlk;
use script_ncs::catalog::Catalog;
use script_ncs::vm::{self, Deferred, ObjectId, Program, Routines};

pub use state::{
    Delayed, DoorMine, Left, Pending, Perceived, SkillUse, State, StealthXp, TrapState, UseKind,
    Watch, Watched, global, set_global,
};

/// `GetNearestCreature`'s criteria (`CREATURE_TYPE_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Criterion {
    Race,
    PlayerChar,
    Class,
    Reputation,
    Alive,
    HasEffect,
    LacksEffect,
    Perception,
}

const CRITERIA: [(&str, Criterion); 8] = [
    ("CREATURE_TYPE_RACIAL_TYPE", Criterion::Race),
    ("CREATURE_TYPE_PLAYER_CHAR", Criterion::PlayerChar),
    ("CREATURE_TYPE_CLASS", Criterion::Class),
    ("CREATURE_TYPE_REPUTATION", Criterion::Reputation),
    ("CREATURE_TYPE_IS_ALIVE", Criterion::Alive),
    ("CREATURE_TYPE_HAS_SPELL_EFFECT", Criterion::HasEffect),
    (
        "CREATURE_TYPE_DOES_NOT_HAVE_SPELL_EFFECT",
        Criterion::LacksEffect,
    ),
    ("CREATURE_TYPE_PERCEPTION", Criterion::Perception),
];

/// The perception criterion's values: whether the target must see and
/// hear the creature (`None`: either).
const PERCEPTIONS: [(&str, Option<bool>, Option<bool>); 8] = [
    ("PERCEPTION_SEEN_AND_HEARD", Some(true), Some(true)),
    (
        "PERCEPTION_NOT_SEEN_AND_NOT_HEARD",
        Some(false),
        Some(false),
    ),
    ("PERCEPTION_HEARD_AND_NOT_SEEN", Some(false), Some(true)),
    ("PERCEPTION_SEEN_AND_NOT_HEARD", Some(true), Some(false)),
    ("PERCEPTION_NOT_HEARD", None, Some(false)),
    ("PERCEPTION_HEARD", None, Some(true)),
    ("PERCEPTION_NOT_SEEN", Some(false), None),
    ("PERCEPTION_SEEN", Some(true), None),
];

/// How a notice event's object was perceived (`Pending::perception`).
pub mod perception {
    pub const SEEN: u8 = 1;
    pub const VANISHED: u8 = 2;
    pub const HEARD: u8 = 3;
    pub const INAUDIBLE: u8 = 4;
}

use crate::combat::sheet::Rows;
use crate::module::{ModuleData, ObjectData, ObjectKind};
use crate::rng::SimRng;
use actions::{ACTIONS_PER_STEP, Action, Goal, Intent, ROUND_MS, Target, Use, WANDER_M, WANDER_MS};
use engine::{Bound, Engine};

/// A resref, tag, quest tag, module or global name as the script layer
/// keys it: lowercase, as the games' data is one whatever its case (the
/// resource system's names, `GetObjectByTag`, `Lev_Escape` and
/// `lev_escape`). Names are keyed by this once, where they come in.
pub fn key(name: &str) -> String {
    name.to_ascii_lowercase()
}

/// An object answering to a tag (`tagged`): its id, kind (`None` for a
/// player, or an item lying on the floor) and where it stands (metres).
pub(super) struct Tagged {
    pub id: ObjectId,
    pub kind: Option<ObjectKind>,
    pub at: [f32; 3],
}

/// Every object tagged `tag` (any case, `key`): the world's, then those
/// made this step, as KOTOR's `CreateObject` makes one at once (Ahto's
/// module load makes the arbiter its area's enter script then sends to the
/// party; a DLG's speaker made by the node before).
pub(super) fn tagged<'a>(
    view: &'a View,
    made: &'a [Made],
    tag: &str,
) -> impl Iterator<Item = Tagged> + 'a {
    let tag = key(tag);
    let placed = view.things.iter().map(|thing| {
        let tagged = Tagged {
            id: thing.id,
            kind: thing.kind,
            at: thing.position,
        };
        (thing.tag(), tagged)
    });
    let made = made.iter().map(|made| {
        let tagged = Tagged {
            id: made.id,
            kind: Some(made.facts.kind),
            at: made.at,
        };
        (made.facts.tag.as_str(), tagged)
    });
    placed
        .chain(made)
        .filter(move |(of, _)| *of == tag)
        .map(|(_, tagged)| tagged)
}

/// The `nth` (from 1) nearest of `found` (distance, object), ties by id;
/// `INVALID` for none.
pub(super) fn nth_nearest(found: Vec<(f32, ObjectId)>, nth: i32) -> ObjectId {
    usize::try_from(nth - 1)
        .ok()
        .and_then(|at| nearest_first(found).nth(at))
        .unwrap_or(ObjectId::INVALID)
}

/// `found` (distance, object) nearest first, ties by id.
pub(super) fn nearest_first(mut found: Vec<(f32, ObjectId)>) -> impl Iterator<Item = ObjectId> {
    found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    found.into_iter().map(|(_, id)| id)
}

/// A whole HP or FP in the fighters' points (`combat::Fighter` keeps
/// hundredths, C3).
pub(crate) const HUNDREDTHS: i32 = 100;

/// Twice a weapon's average damage, to stay in whole numbers: its dice,
/// its bonus dice and its flat bonus.
pub(super) fn twice_average(weapon: &crate::combat::Weapon) -> i32 {
    let dice = |count: u8, sides: u8| i32::from(count) * (i32::from(sides) + 1);
    dice(weapon.dice, weapon.die)
        + dice(weapon.bonus_dice, weapon.bonus_die)
        + 2 * i32::from(weapon.damage_bonus)
}

/// The module and its area as scripts see them (`GetModule`, `GetArea`).
pub const MODULE: ObjectId = ObjectId(0x7e00_0001);
pub const AREA: ObjectId = ObjectId(0x7e00_0002);

/// The script the galaxy map's travel runs, for the traveller: both games'
/// exes name the same one (K1's `KOTOR_Exe`, TSL's `KOTOR2sub`), and both
/// installs ship it.
pub const TRAVEL_SCRIPT: &str = "k_sup_galaxymap";
/// How deep `ExecuteScript` nests.
const MOST_NESTED: u8 = 8;
/// The most scripts and deferred blocks one step runs; the rest wait.
const RUNS_PER_STEP: usize = 256;
/// How long a move is tried before no way there fails it (ms), and the
/// window that is checked in (the move's first steps after it; later a
/// way shut is walked into, as before): the grace lets a door a script
/// opens in the same step stand open first.
const MOVE_GRACE_MS: u64 = rules::scripts::moves::GRACE_MS;
const MOVE_CHECK_MS: u64 = rules::scripts::moves::CHECK_MS;

/// How near (metres) a melee attacker's target stands to be in its reach:
/// an assured blow lands, a scripted attack keeps it in combat.
const MELEE_REACH_M: f32 = rules::scripts::fights::MELEE_REACH;
/// Trace lines a report keeps: its first, and its last (a blocker's
/// lines are the last; heartbeats fill thousands a minute).
const TRACE_LINES: usize = 4000;
const TRACE_TAIL: usize = 100_000;

/// A Force power's or feat's talent facts (`spells.2da`, `feat.2da`): its
/// category code (the AI's codes are masks of it, `GetCreatureTalent…`),
/// its challenge rating (`maxcr`), whether a spell is used on a foe
/// (`hostilesetting`: a grenade, a droid's weapon) and how far it reaches
/// (`range`'s `ranges.2da` `primaryrange`, decimetres).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Talent {
    pub category: u16,
    pub maxcr: u8,
    pub hostile: bool,
    pub reach_dm: u16,
    /// A power's conjure and cast time (`spells.2da` `conjtime` +
    /// `casttime`, ms): how long a script's cast takes (`Action::Cast`).
    pub cast_ms: u16,
}

/// A `forceshields.2da` row (`EffectForceShield`: the energy shields' own
/// numbers): the damage types it takes (`damageflags`, `DAMAGE_TYPE_*`
/// bits) and how much of them in all (`amount`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShieldRow {
    pub types: u16,
    pub points: u16,
}

/// Every power's and feat's talent facts, by row, each power's impact
/// script (`spells.2da` `impactscript`, lowercase; empty for none), each
/// skill's key ability (`skills.2da` `keyability`, a
/// `combat::sheet::Ability`; K1's Security is Wisdom's, TSL's
/// Intelligence's), the shields by `forceshields.2da` row and the poisons'
/// courses by `poison.2da` row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Talents {
    pub spells: Vec<Talent>,
    pub feats: Vec<Talent>,
    pub impacts: Vec<String>,
    pub skill_abilities: Vec<u8>,
    pub shields: Vec<ShieldRow>,
    pub poisons: Vec<crate::combat::items::Course>,
}

/// The clip a camera model plays for a node's `CameraAnimation` (and a
/// stunt model for an `AnimList` entry): `cut001w` for 1200, counting up.
pub fn camera_clip_name(animation: u16) -> Option<String> {
    let n = animation.checked_sub(FIRST_CUT_CLIP)? + 1;
    Some(format!("cut{n:03}w"))
}

/// The first `CameraAnimation` / `AnimList` value naming a clip
/// (`camera_clip_name`), and the first naming a `dialoganimations.2da` row
/// (`Library::dialog_animation`).
const FIRST_CUT_CLIP: u16 = 1200;
const FIRST_DIALOG_ANIMATION: u16 = 10000;
/// Where TSL's scripts number `animations.2da`'s rows from
/// (`Library::script_animation`).
const SCRIPT_ROWS: i32 = 10_000;

/// Whether an `AnimList` entry names a stunt model's clip
/// (`camera_clip_name`) rather than a `dialoganimations.2da` row.
pub fn names_clip(animation: u16) -> bool {
    (FIRST_CUT_CLIP..FIRST_DIALOG_ANIMATION).contains(&animation)
}

/// The walk and run rates (m/s) of two `creaturespeed.2da` rows, by their
/// `2DAName`: `PLAYER`, a player's body's, and `DEFAULT`, row 7's
/// (`walk::rates`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveRates {
    pub player: Option<(f32, f32)>,
    pub default: Option<(f32, f32)>,
}

/// TSL's natural healing (`regeneration.2da`, `CSWSCreature`'s regeneration
/// in its executable): how many seconds a hit point takes, and the rate's
/// parts (out of combat and in it; per Constitution modifier point; per
/// rank in Treat Injury, Repair for a droid); and its Force points' (`fp`).
/// K1's table has no such rows (`HealthRegeneration`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Regeneration {
    pub time_per_hp: f32,
    pub out_of_combat: f32,
    pub in_combat: f32,
    pub con_bonus: f32,
    pub skill_bonus: f32,
    pub fp: FpRegeneration,
}

/// K1's natural healing (its `regeneration.2da`, rows `InCombat` and
/// `OutOfCombat`, column `healthregen`; the creature's update in its
/// executable): a living party member heals that percent of its most hit
/// points a second, at least 0.0001 a second, given in whole points. K1
/// ships 0 for both, so 1 point every 10,000 s. (Its `forceregen` column,
/// 1 % a second out of combat, is not read: C13 keeps JA's refill.)
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HealthRegeneration {
    pub in_combat: f32,
    pub out_of_combat: f32,
}

impl HealthRegeneration {
    /// The fewest hit points a second it heals (K1's executable's floor).
    pub const LEAST: f32 = rules::world::healing::LEAST_HP_PER_S;

    /// The milliseconds between whole points for a creature of `max_hp`,
    /// fighting or not.
    pub fn every_ms(&self, max_hp: u16, fighting: bool) -> u64 {
        let percent = if fighting {
            self.in_combat
        } else {
            self.out_of_combat
        };
        let rate = (percent / 100.0 * f32::from(max_hp)).max(Self::LEAST);
        ((1000.0 / rate) as u64).max(1)
    }
}

/// TSL's Force point regeneration (`regeneration.2da`'s `fpregentime`,
/// `outofcombatfpbase`, `incombatfpbase`, `wismodbonus`;
/// `CSWSCreature::HandleFPRegeneration`): the seconds a whole pool takes at
/// a rate of 1, and the rate's parts. A time of 0: none (`world::powers`
/// keeps JA's refill, C13).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FpRegeneration {
    pub time: f32,
    pub out_of_combat: f32,
    pub in_combat: f32,
    pub wis_bonus: f32,
}

/// What the presentation routines read of the 2DAs (`script::presentation`):
/// whether each `tutorial.2da` row has words for its window (`Message_PC0`
/// set: KOTOR shows no window for the rest); `feedbacktext.2da`'s strrefs;
/// each `appearance.2da` row's `normalhead` (a `heads.2da` row; -1 blank),
/// the head `DuplicateHeadAppearance` gives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presentation {
    pub tutorials: Vec<bool>,
    pub feedback: Vec<u32>,
    pub heads: Vec<i16>,
}

/// The key of `weapondischarge.2da`'s rows every body shares
/// (`Library::discharge`), beside those of one `appearance.2da` row.
pub const ANY_BODY: u16 = u16::MAX;

/// The alien speech `node` of `dlg` plays in TSL, its sound's name before
/// the variant's number: in `sounds` (by `alienvo.2da` row, then
/// `emotion.2da` row, `Library::with_alien_voices`), its race's
/// (`Dlg::alien_race`) for its `Emotion`, where the race says something
/// for it.
pub fn alien_sound<'a>(
    sounds: &'a [Vec<String>],
    dlg: &Dlg,
    node: &fmt_odyssey::dlg::Node,
) -> Option<&'a str> {
    let row = sounds.get(dlg.alien_race(node)? as usize)?;
    let sound = row.get(usize::try_from(node.emotion).ok()?)?;
    (!sound.is_empty()).then_some(sound.as_str())
}

/// A level's compiled scripts and the routines they are compiled against.
pub struct Library {
    routines: Routines,
    programs: BTreeMap<String, Program>,
    bound: Vec<Option<Bound>>,
    /// `OBJECT_TYPE_*` by kind, from the catalog.
    object_types: BTreeMap<ObjectKind, i32>,
    /// `CREATURE_TYPE_*` and `PERCEPTION_*` by value, from the catalog.
    criteria: BTreeMap<i32, Criterion>,
    perceptions: BTreeMap<i32, (Option<bool>, Option<bool>)>,
    talents: Talents,
    /// The level's conversations, by lowercase resref.
    dialogues: BTreeMap<String, Dlg>,
    /// How long each voiced line of them plays (ms), by sound resref.
    voices: BTreeMap<String, u32>,
    /// TSL's alien speech (`alienvo.2da` by `emotion.2da`): the lowercase
    /// sound each race says for each emotion, numbered 1 to 3 after it
    /// (`entry_voice`); empty for none.
    alien_voices: Vec<Vec<String>>,
    /// How long the clips of their camera models play (ms), by lowercase
    /// model and clip name.
    camera_clips: BTreeMap<(String, String), u32>,
    /// How long the clip an `ANIMATION_…` constant names plays on a body
    /// (ms), by its `appearance.2da` row and the constant: the clips of the
    /// module's creature models (`clip_ms`).
    clips: BTreeMap<(u16, u16), u32>,
    /// When the bolts of each gun's attack clip leave the gun
    /// (`weapondischarge.2da`), by the `appearance.2da` row whose own row it
    /// is (`ANY_BODY` for the rows every body shares) and the attack's
    /// animation (`discharge`).
    discharges: BTreeMap<(u16, u16), effects::Discharge>,
    /// What a round between creatures swings and answers with
    /// (`combat::rounds`).
    rounds: crate::combat::rounds::Table,
    /// The `ANIMATION_…` constant of each `dialoganimations.2da` row an
    /// `AnimList` names (as row + 10000), where it has one.
    dialog_animations: Vec<Option<u16>>,
    /// TSL's `animations.2da` rows as animations (`with_animation_rows`).
    animation_rows: Vec<u16>,
    /// Each `appearance.2da` row's `head_arc_h` (degrees): a player's body
    /// reads its head arc here by its persona's appearance (a creature's
    /// rides its template, `module::Body`).
    head_arcs: Vec<f32>,
    /// `creaturespeed.2da`'s walk and run rates (m/s) of its `PLAYER` row
    /// (a player's body, which has no template) and its `DEFAULT` row (a
    /// creature whose own row reads as nothing), where it has them.
    move_rates: MoveRates,
    regeneration: Option<Regeneration>,
    health_regeneration: Option<HealthRegeneration>,
    /// TSL's `influence.2da`: each companion tag's base influence
    /// (`script::influence`); none in K1.
    influence: Vec<(String, u8)>,
    /// The tables the presentation routines read (`Presentation`).
    presentation: Presentation,
    /// The item templates scripts may make (I19), and `OBJECT_TYPE_ITEM`.
    items: Arc<crate::inventory::Catalog>,
    item_type: i32,
    /// The install's talk table (`GetStringByStrRef`).
    talk: Option<Arc<Tlk>>,
    /// The creature templates scripts and the party may make, by lowercase
    /// resref (I19: companions, `CreateObject`).
    creatures: Arc<BTreeMap<String, Arc<ObjectData>>>,
    /// The placeable templates scripts may make (`CreateObject`), by
    /// lowercase resref.
    placeables: Arc<BTreeMap<String, Arc<ObjectData>>>,
    /// The journal's quests (`global.jrl`, I19).
    quests: Arc<journal::Quests>,
    /// The XP tables (`plot.2da`, `xptable.2da`, I19).
    experience: Arc<xp::Experience>,
    /// The classes as levels grow them (I19).
    classes: Arc<crate::levels::Classes>,
    /// TSL's item creation tables (`inventory::workshop`).
    workshop: Arc<crate::inventory::workshop::Workshop>,
    /// The pazaak decks and cards (`crate::pazaak::Decks`).
    pazaak: Arc<crate::pazaak::Decks>,
    digest: [u8; 32],
}

impl Library {
    /// `scripts` by lowercase resref.
    pub fn new(catalog: Catalog, scripts: BTreeMap<String, Ncs>, talents: Talents) -> Library {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"KJKS-library");
        for table in [&talents.spells, &talents.feats] {
            hasher.update(&(table.len() as u32).to_le_bytes());
            for talent in table {
                hasher.update(&talent.category.to_le_bytes());
                hasher.update(&[talent.maxcr]);
                // Only a hostile one adds a byte, so a table without keeps
                // its digest.
                if talent.hostile {
                    hasher.update(b"hostile");
                }
                if talent.reach_dm != 0 {
                    hasher.update(b"reach");
                    hasher.update(&talent.reach_dm.to_le_bytes());
                }
                if talent.cast_ms != 0 {
                    hasher.update(b"cast");
                    hasher.update(&talent.cast_ms.to_le_bytes());
                }
            }
        }
        for impact in &talents.impacts {
            hasher.update(impact.as_bytes());
            hasher.update(&[0]);
        }
        // Only a library with skills names them, so one without keeps its
        // digest.
        if !talents.skill_abilities.is_empty() {
            hasher.update(b"skills");
            hasher.update(&talents.skill_abilities);
        }
        for routine in &catalog.routines {
            hasher.update(routine.to_string().as_bytes());
        }
        for (name, ncs) in &scripts {
            hasher.update(name.as_bytes());
            hasher.update(&ncs.write());
        }
        let bound = catalog
            .routines
            .iter()
            .map(|routine| Bound::named(&routine.name))
            .collect();
        let object_types = [
            (ObjectKind::Creature, "OBJECT_TYPE_CREATURE"),
            (ObjectKind::Door, "OBJECT_TYPE_DOOR"),
            (ObjectKind::Placeable, "OBJECT_TYPE_PLACEABLE"),
            (ObjectKind::Trigger, "OBJECT_TYPE_TRIGGER"),
            (ObjectKind::Waypoint, "OBJECT_TYPE_WAYPOINT"),
            (ObjectKind::Sound, "OBJECT_TYPE_SOUND"),
            (ObjectKind::Encounter, "OBJECT_TYPE_ENCOUNTER"),
            (ObjectKind::Store, "OBJECT_TYPE_STORE"),
        ]
        .into_iter()
        .filter_map(|(kind, name)| Some((kind, int_constant(&catalog, name)?)))
        .collect();
        let criteria = CRITERIA
            .into_iter()
            .filter_map(|(name, criterion)| Some((int_constant(&catalog, name)?, criterion)))
            .collect();
        let perceptions = PERCEPTIONS
            .into_iter()
            .filter_map(|(name, seen, heard)| Some((int_constant(&catalog, name)?, (seen, heard))))
            .collect();
        let item_type = int_constant(&catalog, "OBJECT_TYPE_ITEM").unwrap_or(2);
        Library {
            routines: Routines::new(Arc::new(catalog)),
            programs: scripts
                .into_iter()
                .map(|(name, ncs)| (name, Program::new(ncs)))
                .collect(),
            bound,
            object_types,
            criteria,
            perceptions,
            talents,
            dialogues: BTreeMap::new(),
            voices: BTreeMap::new(),
            alien_voices: Vec::new(),
            camera_clips: BTreeMap::new(),
            clips: BTreeMap::new(),
            discharges: BTreeMap::new(),
            rounds: crate::combat::rounds::Table::default(),
            dialog_animations: Vec::new(),
            animation_rows: Vec::new(),
            head_arcs: Vec::new(),
            move_rates: MoveRates::default(),
            regeneration: None,
            health_regeneration: None,
            influence: Vec::new(),
            presentation: Presentation::default(),
            items: Arc::default(),
            creatures: Arc::default(),
            placeables: Arc::default(),
            quests: Arc::default(),
            experience: Arc::default(),
            classes: Arc::default(),
            workshop: Arc::default(),
            pazaak: Arc::default(),
            item_type,
            talk: None,
            digest: *hasher.finalize().as_bytes(),
        }
    }

    /// The conversations (by lowercase resref) and their voiced lines'
    /// lengths (ms by sound resref), in the digest (each conversation as it
    /// runs: what the DLG holds that it does not read changes nothing).
    pub fn with_dialogues(
        mut self,
        dialogues: BTreeMap<String, Dlg>,
        voices: BTreeMap<String, u32>,
    ) -> Library {
        if dialogues.is_empty() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        for (name, dlg) in dialogues {
            let mut out = crate::wire::Writer::default();
            crate::pack::encode_dlg(&dlg, &mut out);
            hasher.update(name.as_bytes());
            hasher.update(&out.0);
            self.dialogues.insert(name, dlg);
        }
        for (name, ms) in &voices {
            hasher.update(name.as_bytes());
            hasher.update(&ms.to_le_bytes());
        }
        self.voices = voices;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// TSL's alien speech, by `alienvo.2da` row and `emotion.2da` row
    /// (`alien_voices`), in the digest when there is any.
    pub fn with_alien_voices(mut self, rows: Vec<Vec<String>>) -> Library {
        if rows.iter().flatten().all(String::is_empty) {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"alien voices");
        for row in &rows {
            hasher.update(&(row.len() as u32).to_le_bytes());
            for sound in row {
                hasher.update(sound.as_bytes());
                hasher.update(&[0]);
            }
        }
        self.alien_voices = rows;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// The alien speech `node` of `dlg` plays in TSL (`alien_sound`).
    pub fn alien_voice(&self, dlg: &Dlg, node: &fmt_odyssey::dlg::Node) -> Option<&str> {
        alien_sound(&self.alien_voices, dlg, node)
    }

    /// The voice entry `node` of `dlg` plays and how long (ms): its alien
    /// speech (`alien_voice`) as variant `variant` (1 to 3) where it has
    /// one, which TSL plays in place of its own; else its own (`voice`).
    pub fn entry_voice<'a>(
        &'a self,
        dlg: &Dlg,
        node: &'a fmt_odyssey::dlg::Node,
        variant: Option<u8>,
    ) -> Option<(std::borrow::Cow<'a, str>, u32)> {
        let Some(alien) = self.alien_voice(dlg, node) else {
            return self.voice(node).map(|(name, ms)| (name.into(), ms));
        };
        let name = format!("{alien}{}", variant?);
        let ms = *self.voices.get(&name)?;
        Some((name.into(), ms))
    }

    /// The clips of the conversations' camera models and how long each
    /// plays (ms), by lowercase model and clip name, in the digest when
    /// there are any.
    pub fn with_camera_clips(mut self, clips: BTreeMap<(String, String), u32>) -> Library {
        if clips.is_empty() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"camera clips");
        for ((model, clip), ms) in &clips {
            for name in [model, clip] {
                hasher.update(name.as_bytes());
                hasher.update(&[0]);
            }
            hasher.update(&ms.to_le_bytes());
        }
        self.camera_clips = clips;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// How long the clips the `ANIMATION_…` constants name play on the
    /// module's bodies (ms), by `appearance.2da` row and constant, in the
    /// digest when there are any.
    pub fn with_clips(mut self, clips: BTreeMap<(u16, u16), u32>) -> Library {
        if clips.is_empty() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"clips");
        for ((appearance, animation), ms) in &clips {
            hasher.update(&appearance.to_le_bytes());
            hasher.update(&animation.to_le_bytes());
            hasher.update(&ms.to_le_bytes());
        }
        self.clips = clips;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// How long (ms) the clip `animation` (an `ANIMATION_…` constant) names
    /// plays once on a body of `appearance.2da` row `appearance`, if its
    /// model has it. The sim knows a body by its row, not its model: the
    /// row's unarmoured model was measured (`session::library`).
    pub fn clip_ms(&self, appearance: u16, animation: u16) -> Option<u32> {
        self.clips.get(&(appearance, animation)).copied()
    }

    /// When the bolts of the guns' attack clips leave the gun
    /// (`weapondischarge.2da`), by `appearance.2da` row (`ANY_BODY` for
    /// every body's) and attack animation (`discharge`), in the digest when
    /// there are any.
    pub fn with_discharges(
        mut self,
        discharges: BTreeMap<(u16, u16), effects::Discharge>,
    ) -> Library {
        if discharges.is_empty() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"discharges");
        for ((appearance, animation), shots) in &discharges {
            hasher.update(&appearance.to_le_bytes());
            hasher.update(&animation.to_le_bytes());
            let mut out = crate::wire::Writer::default();
            shots.encode(&mut out);
            hasher.update(&out.0);
        }
        self.discharges = discharges;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// The game's round tables (`combat::rounds::Table`), in the digest
    /// when there are any.
    pub fn with_rounds(mut self, rounds: crate::combat::rounds::Table) -> Library {
        if rounds == crate::combat::rounds::Table::default() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        rounds.digest(&mut hasher);
        self.rounds = rounds;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// What a round between creatures swings and answers with.
    pub fn rounds(&self) -> &crate::combat::rounds::Table {
        &self.rounds
    }

    /// The animation each `animations.2da` row plays as (TSL's: the rows
    /// its scripts play by number, `script_animation`), numbered as
    /// `asset_odyssey::creature::row_animation` numbers it, in the digest
    /// when there are any.
    pub fn with_animation_rows(mut self, rows: Vec<u16>) -> Library {
        if rows.is_empty() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"animation rows");
        for row in &rows {
            hasher.update(&row.to_le_bytes());
        }
        self.animation_rows = rows;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// When the bolts of attack `animation` (an `animations.2da` row) leave
    /// the gun of a body of
    /// `appearance.2da` row `appearance`: its own row's (a droid's, HK-47's),
    /// else every body's; none for a clip the table has no shots for.
    pub fn discharge(&self, appearance: Option<u16>, animation: u16) -> Option<effects::Discharge> {
        appearance
            .and_then(|row| self.discharges.get(&(row, animation)))
            .or_else(|| self.discharges.get(&(ANY_BODY, animation)))
            .copied()
            .filter(|shots| !shots.times().is_empty())
    }

    /// What a script's `ActionPlayAnimation` or `PlayAnimation` value plays
    /// on a creature, as both executables turn it: a value their table of
    /// constants does not reach (K1's from 10 000, TSL's 10 000 itself, a
    /// negative one) plays that table's default, `ANIMATION_LOOPING_PAUSE`;
    /// TSL's from 10 001 are `animations.2da` rows, 10 000 and the row
    /// (`with_animation_rows`; a row past the table, nothing).
    pub fn script_animation(&self, value: i32, tsl: bool) -> u16 {
        match value {
            v if v < 0 || v == SCRIPT_ROWS || (!tsl && v > SCRIPT_ROWS) => 0,
            v if v > SCRIPT_ROWS => usize::try_from(v - SCRIPT_ROWS)
                .ok()
                .and_then(|row| self.animation_rows.get(row).copied())
                .unwrap_or(u16::MAX),
            v => u16::try_from(v).unwrap_or(u16::MAX),
        }
    }

    /// The `ANIMATION_…` constants of `dialoganimations.2da`'s rows (none
    /// where a row names no animation), in the digest when there are any.
    pub fn with_dialog_animations(mut self, rows: Vec<Option<u16>>) -> Library {
        if rows.iter().all(Option::is_none) {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"dialog animations");
        for row in &rows {
            hasher.update(&row.map_or(u32::MAX, u32::from).to_le_bytes());
        }
        self.dialog_animations = rows;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// The `head_arc_h` of each `appearance.2da` row (degrees, 0 where
    /// blank), in the digest when any is not 0.
    pub fn with_head_arcs(mut self, arcs: Vec<f32>) -> Library {
        if arcs.iter().all(|arc| *arc == 0.0) {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"head arcs");
        for arc in &arcs {
            hasher.update(&arc.to_le_bytes());
        }
        self.head_arcs = arcs;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// `creaturespeed.2da`'s `PLAYER` and `DEFAULT` rows (`MoveRates`), in
    /// the digest when it has either.
    pub fn with_move_rates(mut self, rates: MoveRates) -> Library {
        if rates == MoveRates::default() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"move rates");
        for (walk, run) in [rates.player, rates.default].into_iter().flatten() {
            hasher.update(&walk.to_le_bytes());
            hasher.update(&run.to_le_bytes());
        }
        self.move_rates = rates;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// `creaturespeed.2da`'s `PLAYER` and `DEFAULT` rows, where read.
    pub fn move_rates(&self) -> MoveRates {
        self.move_rates
    }

    /// TSL's natural healing (`Regeneration`), in the digest when read.
    pub fn with_regeneration(mut self, regeneration: Option<Regeneration>) -> Library {
        let Some(r) = regeneration else {
            return self;
        };
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"regeneration");
        for v in [
            r.time_per_hp,
            r.out_of_combat,
            r.in_combat,
            r.con_bonus,
            r.skill_bonus,
        ] {
            hasher.update(&v.to_le_bytes());
        }
        // Only a table with the Force's rows names them, so one without
        // keeps its digest.
        if r.fp != FpRegeneration::default() {
            hasher.update(b"fp");
            for v in [
                r.fp.time,
                r.fp.out_of_combat,
                r.fp.in_combat,
                r.fp.wis_bonus,
            ] {
                hasher.update(&v.to_le_bytes());
            }
        }
        self.regeneration = Some(r);
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// TSL's natural healing, where the game has it.
    pub fn regeneration(&self) -> Option<Regeneration> {
        self.regeneration
    }

    /// K1's natural healing (`HealthRegeneration`), in the digest when read.
    pub fn with_health_regeneration(mut self, rates: Option<HealthRegeneration>) -> Library {
        let Some(rates) = rates else {
            return self;
        };
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"health regeneration");
        for v in [rates.in_combat, rates.out_of_combat] {
            hasher.update(&v.to_le_bytes());
        }
        self.health_regeneration = Some(rates);
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// K1's natural healing, where the game has it.
    pub fn health_regeneration(&self) -> Option<HealthRegeneration> {
        self.health_regeneration
    }

    /// TSL's `influence.2da` (lowercase tag, base influence), in the digest
    /// when it has rows.
    pub fn with_influence(mut self, rows: Vec<(String, u8)>) -> Library {
        if rows.is_empty() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"influence");
        for (tag, base) in &rows {
            hasher.update(tag.as_bytes());
            hasher.update(&[0, *base]);
        }
        self.influence = rows;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// The tables the presentation routines read (`Presentation`), in the
    /// digest when read.
    pub fn with_presentation(mut self, tables: Presentation) -> Library {
        if tables == Presentation::default() {
            return self;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"presentation");
        for (name, values) in [
            (
                "tutorials",
                tables.tutorials.iter().map(|&v| i32::from(v)).collect(),
            ),
            (
                "feedback",
                tables.feedback.iter().map(|&v| v as i32).collect(),
            ),
            (
                "heads",
                tables
                    .heads
                    .iter()
                    .map(|&v| i32::from(v))
                    .collect::<Vec<_>>(),
            ),
        ] {
            hasher.update(name.as_bytes());
            hasher.update(&(values.len() as u32).to_le_bytes());
            for value in values {
                hasher.update(&value.to_le_bytes());
            }
        }
        self.presentation = tables;
        self.digest = *hasher.finalize().as_bytes();
        self
    }

    /// The tables the presentation routines read.
    pub fn presentation(&self) -> &Presentation {
        &self.presentation
    }

    /// Whether the game keeps influence (TSL).
    pub fn influences(&self) -> bool {
        !self.influence.is_empty()
    }

    /// The base influence of the companion tagged `tag` (any case), if
    /// `influence.2da` lists it.
    pub fn base_influence(&self, tag: &str) -> Option<u8> {
        self.influence
            .iter()
            .find(|(of, _)| of.eq_ignore_ascii_case(tag))
            .map(|(_, base)| *base)
    }

    /// How far (degrees) the head of a body of `appearance.2da` row
    /// `appearance` turns from it; 0 for a row it doesn't know.
    pub fn head_arc(&self, appearance: u16) -> f32 {
        self.head_arcs
            .get(usize::from(appearance))
            .copied()
            .unwrap_or(0.0)
    }

    /// The `ANIMATION_…` constant an `AnimList` entry (a `dialoganimations.2da`
    /// row + 10000) plays, if the row names one.
    pub fn dialog_animation(&self, value: u16) -> Option<u16> {
        let row = usize::from(value.checked_sub(FIRST_DIALOG_ANIMATION)?);
        self.dialog_animations.get(row).copied().flatten()
    }

    /// How long a conversation's camera model plays a node's
    /// `CameraAnimation` (ms), if the model has that clip.
    pub fn camera_clip(&self, model: &str, animation: u16) -> Option<u32> {
        let clip = camera_clip_name(animation)?;
        self.camera_clips.get(&(key(model), clip)).copied()
    }

    /// The sound a node plays and how long (ms): its `Sound`, else its
    /// `VO_ResRef`, whichever the install has (reone's preference).
    pub fn voice<'a>(&self, node: &'a fmt_odyssey::dlg::Node) -> Option<(&'a str, u32)> {
        [node.sound.as_str(), node.vo.as_str()]
            .into_iter()
            .find_map(|name| Some((name, *self.voices.get(name)?)))
    }

    /// This library with the item templates scripts may make.
    pub fn with_items(self, items: crate::inventory::Catalog) -> Library {
        let mut out = crate::wire::Writer::default();
        out.bytes(&self.digest);
        items.digest_into(&mut out);
        Library {
            items: Arc::new(items),
            digest: *blake3::hash(&out.0).as_bytes(),
            ..self
        }
    }

    /// This library with the install's talk table, in the digest.
    pub fn with_talk(self, talk: Arc<Tlk>) -> Library {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.digest);
        hasher.update(b"talk");
        for entry in &talk.entries {
            hasher.update(&(entry.text.len() as u32).to_le_bytes());
            hasher.update(&entry.text);
        }
        Library {
            talk: Some(talk),
            digest: *hasher.finalize().as_bytes(),
            ..self
        }
    }

    /// A talk-table line, in the game's code page (one character a byte).
    pub fn text(&self, strref: u32) -> Option<String> {
        let bytes = self.talk.as_ref()?.text(strref)?;
        Some(bytes.iter().map(|&byte| char::from(byte)).collect())
    }

    /// The item templates scripts may make.
    pub fn items(&self) -> &crate::inventory::Catalog {
        &self.items
    }

    /// This library with the creature templates the party and scripts may
    /// make.
    pub fn with_placeables(self, placeables: BTreeMap<String, Arc<ObjectData>>) -> Library {
        let mut out = crate::wire::Writer::default();
        out.bytes(&self.digest);
        out.u32(placeables.len() as u32);
        for (name, facts) in &placeables {
            out.string(name);
            facts.digest_into(&mut out);
        }
        Library {
            placeables: Arc::new(placeables),
            digest: *blake3::hash(&out.0).as_bytes(),
            ..self
        }
    }

    pub fn with_creatures(self, creatures: BTreeMap<String, Arc<ObjectData>>) -> Library {
        let mut out = crate::wire::Writer::default();
        out.bytes(&self.digest);
        out.u32(creatures.len() as u32);
        for (name, facts) in &creatures {
            out.string(name);
            facts.digest_into(&mut out);
        }
        Library {
            creatures: Arc::new(creatures),
            digest: *blake3::hash(&out.0).as_bytes(),
            ..self
        }
    }

    /// This library with the journal's quests, in the digest.
    pub fn with_quests(self, quests: journal::Quests) -> Library {
        let mut out = crate::wire::Writer::default();
        out.bytes(&self.digest);
        quests.digest_into(&mut out);
        Library {
            quests: Arc::new(quests),
            digest: *blake3::hash(&out.0).as_bytes(),
            ..self
        }
    }

    /// The journal's quests.
    pub fn quests(&self) -> &journal::Quests {
        &self.quests
    }

    /// This library with the pazaak decks and cards, in the digest when it
    /// has any.
    pub fn with_pazaak(self, decks: crate::pazaak::Decks) -> Library {
        if decks == crate::pazaak::Decks::default() {
            return self;
        }
        let mut out = crate::wire::Writer::default();
        out.bytes(&self.digest);
        out.string("pazaak");
        decks.digest_into(&mut out);
        Library {
            pazaak: Arc::new(decks),
            digest: *blake3::hash(&out.0).as_bytes(),
            ..self
        }
    }

    /// The pazaak decks and cards.
    pub fn pazaak(&self) -> &crate::pazaak::Decks {
        &self.pazaak
    }

    /// This library with the XP tables, in the digest.
    pub fn with_experience(self, experience: xp::Experience) -> Library {
        let mut out = crate::wire::Writer::default();
        out.bytes(&self.digest);
        experience.digest_into(&mut out);
        Library {
            experience: Arc::new(experience),
            digest: *blake3::hash(&out.0).as_bytes(),
            ..self
        }
    }

    /// The XP tables.
    pub fn experience(&self) -> &xp::Experience {
        &self.experience
    }

    /// This library with the classes as levels grow them, in the digest.
    pub fn with_classes(self, classes: crate::levels::Classes) -> Library {
        let mut out = crate::wire::Writer::default();
        out.bytes(&self.digest);
        classes.digest_into(&mut out);
        Library {
            classes: Arc::new(classes),
            digest: *blake3::hash(&out.0).as_bytes(),
            ..self
        }
    }

    /// The classes as levels grow them.
    pub fn classes(&self) -> &crate::levels::Classes {
        &self.classes
    }

    /// This library with TSL's item creation tables, in the digest (a
    /// library without keeps its digest).
    pub fn with_workshop(self, workshop: crate::inventory::workshop::Workshop) -> Library {
        if workshop == crate::inventory::workshop::Workshop::default() {
            return self;
        }
        let mut out = crate::wire::Writer::default();
        out.bytes(&self.digest);
        workshop.digest_into(&mut out);
        Library {
            workshop: Arc::new(workshop),
            digest: *blake3::hash(&out.0).as_bytes(),
            ..self
        }
    }

    /// TSL's item creation tables (empty for K1).
    pub fn workshop(&self) -> &crate::inventory::workshop::Workshop {
        &self.workshop
    }

    /// Every creature template, by resref.
    pub fn creatures(&self) -> &BTreeMap<String, Arc<ObjectData>> {
        &self.creatures
    }

    /// A creature template's facts, if the library has it.
    pub fn creature(&self, template: &str) -> Option<&Arc<ObjectData>> {
        self.creatures.get(&key(template))
    }

    pub fn placeable(&self, template: &str) -> Option<&Arc<ObjectData>> {
        self.placeables.get(&key(template))
    }

    /// How many creature templates it has.
    pub fn creature_count(&self) -> usize {
        self.creatures.len()
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub fn dialogue(&self, name: &str) -> Option<&Dlg> {
        self.dialogues.get(&key(name))
    }

    pub fn dialogues(&self) -> impl Iterator<Item = (&str, &Dlg)> {
        self.dialogues
            .iter()
            .map(|(name, dlg)| (name.as_str(), dlg))
    }

    /// Skill `row`'s key ability, if the library knows it.
    pub fn skill_ability(&self, row: usize) -> Option<crate::combat::sheet::Ability> {
        crate::combat::sheet::Ability::ALL
            .get(usize::from(*self.talents.skill_abilities.get(row)?))
            .copied()
    }

    /// The impact script of power `row`, if it has one in the library.
    /// `forceshields.2da` row `row`.
    pub fn shield(&self, row: usize) -> Option<ShieldRow> {
        self.talents.shields.get(row).copied()
    }

    /// `poison.2da` row `row`.
    pub fn poison(&self, row: usize) -> Option<crate::combat::items::Course> {
        self.talents.poisons.get(row).copied()
    }

    /// `spells.2da` row `row`'s talent facts (none for a row past the
    /// table).
    pub fn spell(&self, row: u16) -> Talent {
        self.talents
            .spells
            .get(usize::from(row))
            .copied()
            .unwrap_or(Talent::default())
    }

    pub fn impact(&self, row: u16) -> Option<&str> {
        let name = self.talents.impacts.get(usize::from(row))?;
        self.programs
            .contains_key(&key(name))
            .then_some(name.as_str())
    }

    pub fn program(&self, name: &str) -> Option<&Program> {
        self.programs.get(&key(name))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.programs.keys().map(String::as_str)
    }

    /// The globals its scripts write by a constant name (`SetGlobalNumber`,
    /// `SetGlobalBoolean`, `SetGlobalString`: the name the last constant
    /// pushed before the call, as arguments go right to left), each of its
    /// kind, lowercase: what a test's own scripts keep, for its module's
    /// catalogue to declare (`Level::declaring`).
    pub fn globals_written(&self) -> Vec<(String, crate::module::Global)> {
        use crate::module::Global;
        use fmt_odyssey::ncs::{Const, Op};
        let catalog = self.routines.catalog();
        let kind = |routine: u16| match catalog.routine_by_id(routine)?.name.as_str() {
            "SetGlobalNumber" => Some(Global::Number),
            "SetGlobalBoolean" => Some(Global::Boolean),
            "SetGlobalString" => Some(Global::String),
            _ => None,
        };
        let mut written = Vec::new();
        for program in self.programs.values() {
            let mut last = None;
            for instruction in &program.ncs.code {
                match &instruction.op {
                    Op::Const(Const::String(name)) => last = Some(name),
                    Op::Action { routine, .. } => {
                        if let (Some(kind), Some(name)) = (kind(*routine), last) {
                            let name = key(&String::from_utf8_lossy(name));
                            written.push((name, kind));
                        }
                        last = None;
                    }
                    _ => {}
                }
            }
        }
        written
    }

    /// Whether script `name` may call one of `routines` (by name): any of
    /// its code does, or a script it runs by a constant name
    /// (`ExecuteScript`), a few deep. A missing script calls nothing.
    pub fn may_call(&self, name: &str, routines: &[&str]) -> bool {
        self.calls_within(name, routines, 3)
    }

    fn calls_within(&self, name: &str, routines: &[&str], depth: usize) -> bool {
        use fmt_odyssey::ncs::{Const, Op};
        let Some(program) = self.program(name) else {
            return false;
        };
        let catalog = self.routines.catalog();
        let mut last = None;
        for instruction in &program.ncs.code {
            match &instruction.op {
                Op::Const(Const::String(text)) => last = Some(text),
                Op::Action { routine, .. } => {
                    let called = catalog.routine_by_id(*routine).map(|r| r.name.as_str());
                    if called.is_some_and(|called| routines.contains(&called)) {
                        return true;
                    }
                    if called == Some("ExecuteScript")
                        && depth > 0
                        && let Some(script) = last
                        && self.calls_within(&String::from_utf8_lossy(script), routines, depth - 1)
                    {
                        return true;
                    }
                    last = None;
                }
                _ => {}
            }
        }
        false
    }

    pub fn routines(&self) -> &Routines {
        &self.routines
    }

    /// A catalog constant's value (`ACTION_INVALID`, ...), if an int.
    pub fn constant(&self, name: &str) -> Option<i32> {
        int_constant(self.routines.catalog(), name)
    }

    /// How many routines have a binding here.
    pub fn bound(&self) -> usize {
        self.bound.iter().flatten().count()
    }
}

fn int_constant(catalog: &Catalog, name: &str) -> Option<i32> {
    match catalog.constant_named(name)?.value {
        script_ncs::catalog::Value::Int(value) => Some(value),
        _ => None,
    }
}

/// One thing a script may name.
#[derive(Debug, Clone)]
pub struct Thing {
    pub id: ObjectId,
    /// `None` for a player.
    pub kind: Option<ObjectKind>,
    /// Its number among the area's objects (its instance), if it is one.
    pub index: Option<u32>,
    /// Its module's facts, shared, whose tag it answers to (`tag`); `None`
    /// for a player.
    pub facts: Option<Arc<crate::module::ObjectData>>,
    /// Metres, game space.
    pub position: [f32; 3],
    /// Degrees from +X.
    pub yaw: f32,
    /// Whole hit points now and at most, for a character.
    pub hp: Option<(i32, i32)>,
    /// A door's state (0 closed).
    pub open: Option<u8>,
    /// Whether a door or placeable is locked
    /// (`snapshot::ObjectState::locked`).
    pub locked: Option<bool>,
    /// A sound object's state and volume as scripts set them
    /// (`snapshot::ObjectState`).
    pub sound: Option<(u8, u8)>,
    /// A trap's bits (`snapshot::trap`); 0 for the rest.
    pub trap: u8,
    /// A character's facts beyond its hit points.
    pub character: Option<Character>,
    /// Who a player is (`persona`): a seat's own character's, never a
    /// companion's it drives.
    pub persona: Option<crate::persona::Persona>,
}

impl Thing {
    /// Its tag, lowercase; empty for none.
    pub fn tag(&self) -> &str {
        self.facts.as_ref().map_or("", |facts| facts.tag.as_str())
    }

    /// Whether it has hit points and none left: a dead character, a broken
    /// placeable. Such a thing hears no heartbeat nor a conversation's end,
    /// and a dead character's queue does not move.
    pub fn dead(&self) -> bool {
        self.hp.is_some_and(|(hp, _)| hp <= 0)
    }

    /// Whether blows may break it open (`module::bashable`).
    pub fn bashable(&self) -> bool {
        let locked = self.locked == Some(true);
        (self.facts.as_deref()).is_some_and(|facts| crate::module::bashable(facts, locked, self.hp))
    }
}

impl Character {
    /// The weapon it has out (`Sheet::held`).
    pub fn held(&self) -> crate::combat::Weapon {
        self.sheet.held(self.hand)
    }

    /// Whether the weapon it has out is a gun (`Sheet::gunner`): what a
    /// scene's blow goes out as, a shot or a melee strike.
    pub fn gunner(&self) -> bool {
        self.sheet.gunner(self.hand)
    }
}

/// How many stalls a walk takes before its mover is stuck (`Character::stuck`).
pub const STUCK_STALLS: u8 = rules::scripts::moves::STUCK_STALLS;

/// What scripts read of a character besides its hit points.
#[derive(Debug, Clone, Copy)]
pub struct Character {
    /// Its standard faction now (`repute.2da` row).
    pub faction: u8,
    /// Who poisoned it, while it is (`combat::items::Poisoned`).
    pub poisoned: Option<ObjectId>,
    /// `racialtypes.2da` row.
    pub race: u8,
    /// Total character level (`GetHitDice`).
    pub level: u8,
    /// A player or a companion.
    pub party: bool,
    /// The seat whose body it is: a player's own seat, or the seat driving
    /// a companion (`Mind::driver`); `None` for a character nobody plays.
    pub driver: Option<crate::input::Seat>,
    /// Whom it fights.
    pub target: Option<ObjectId>,
    /// In combat (`GetIsInCombat`: 8 s after its last hit or attack, or in
    /// TSL's fake combat, `fake_combat`).
    pub in_combat: bool,
    pub fake_combat: bool,
    /// Its walk has stalled again and again (`STUCK_STALLS`): it can come
    /// no nearer where it is going.
    pub stuck: bool,
    /// Its walk carries it on now: not stalled, its body moving.
    pub walking: bool,
    /// Off the floor: thrown, falling, jumping.
    pub airborne: bool,
    /// The last to hurt it, and when (sim ms).
    pub struck: Option<(ObjectId, u64)>,
    /// A party member down (I30 P11): out of the fight but still the
    /// party's, where it fell.
    pub downed: bool,
    /// Stunned, held or confused (`GetIsDebilitated`).
    pub debilitated: bool,
    /// Hundredths of its FP (the talents it can pay for).
    pub fp: i32,
    /// `feat.2da` rows known.
    pub feats: Rows<4>,
    /// Its classes (`classes.2da` rows) and levels, the first two; 0
    /// levels for none.
    pub classes: [(u8, u8); 2],
    /// `Gender`, `SubraceIndex` (0 for a player until the party table).
    pub gender: u8,
    pub subrace: u8,
    /// What it is now (its sheet as its items and effects change it,
    /// `effects::sheet_now`), and the absorb tier it holds (Resist Force,
    /// Force Immunity).
    pub sheet: crate::combat::Sheet,
    /// Its sheet as its items make it, without its effects.
    pub worn: crate::combat::Sheet,
    /// Which hand's weapon it has out (`Fighter::hand`).
    pub hand: u8,
    pub absorbing: u8,
    /// The lasting effects on it (`combat::effects::Lasting`).
    pub lasting: [Option<crate::combat::effects::Lasting>; crate::combat::effects::LASTING_SLOTS],
    /// `spells.2da` rows known.
    pub powers: Rows<5>,
    /// How it moves now (`stealth::Pace`): standing still, running.
    pub still: bool,
    pub running: bool,
    /// Its command holds an attack (the attack or alternate attack
    /// button): what breaks off a skill's use.
    pub attacking: bool,
    /// In stealth mode (`IsStealthed`).
    pub stealth: bool,
    /// What scripts made of its body for good (`combat::Look`).
    pub look: crate::combat::Look,
}

/// The world as scripts read it this step: things in id order, the
/// players by seat.
pub struct View<'a> {
    pub things: &'a [Thing],
    pub party: &'a [ObjectId],
    /// Each seat's body, the one it acts through (`world::acting_body`:
    /// its avatar, the companion it drives, or for the first seat the
    /// companion leading), in seat order; a watching seat has none.
    pub seats: &'a [(u8, ObjectId)],
    pub module: &'a ModuleData,
    /// Every object of the area, by number (`Thing::index`).
    pub instances: &'a crate::instance::Instances,
    /// The id the next entity made gets (objects scripts make take the
    /// next ones in order).
    pub next_id: u32,
    pub now_ms: u64,
    /// Whom objects fight and what talent they use, from the last step's
    /// scripts (`GetAttemptedSpellTarget`).
    pub intents: &'a BTreeMap<u32, Intent>,
    /// Each seat's new answer this step, by seat: what it asks and the body
    /// it drives (`dialogue::Answer`).
    pub picks: &'a BTreeMap<u8, dialogue::Answer>,
    /// The host's answers to guests asking it into a conversation this step
    /// (`Action::Request`): the seat, the conversation and whether it
    /// joins it.
    pub requests: &'a [(u8, u32, bool)],
    /// The party is in combat (`world::InCombat`, what an exit's ready
    /// check reads).
    pub in_combat: bool,
    /// The party members a fight is near (`world::party::embattled`: a
    /// hostile one keeping the party in combat within its reach), by id.
    pub embattled: &'a [ObjectId],
    /// Players (by id) closing the party selection screen this step, with
    /// the companions they picked (`Action::ChooseParty`).
    pub chosen: &'a [(ObjectId, Option<[u8; 2]>)],
    /// Players (by id) travelling from the galaxy map this step, with the
    /// planet picked (`Action::Travel`).
    pub travels: &'a [(ObjectId, u8)],
    /// The moves at the pazaak tables this step (`Action::Pazaak`), each
    /// with the body whose match it is.
    pub pazaak: &'a [(ObjectId, crate::pazaak::Move)],
    /// The asks to watch a match this step (`Action::Watch`, or a use of a
    /// body at a table), in seat order: the asking seat's body, and the
    /// body whose match it is (`u32::MAX`: stop watching).
    pub watches: &'a [(ObjectId, u32)],
    /// The area's sound as scripts set it so far.
    pub area_sound: crate::snapshot::AreaSound,
    /// The screen's fade as scripts set it so far.
    pub fade: Option<crate::snapshot::Fade>,
    /// Where the engine's fade in at the module's beginning stands.
    pub world_fade_in: crate::snapshot::WorldFadeIn,
    /// Whether a way leads a character standing at the first place to the
    /// second (metres), through the doors as they stand (`walk::route`).
    pub leads: &'a dyn Fn([f32; 3], [f32; 3]) -> bool,
    /// Whether sight runs clear from the first place to the second
    /// (metres), through the doors as they stand.
    pub sees: &'a dyn Fn([f32; 3], [f32; 3]) -> bool,
    /// The walkable floor's height under a place (metres), if one is near.
    pub floor: &'a dyn Fn([f32; 3]) -> Option<f32>,
    /// Whether a body standing at the first place (soles, metres) walks
    /// straight to the second (`walk::straight`; TSL's `TestDirectLine`).
    pub walks: &'a dyn Fn([f32; 3], [f32; 3]) -> bool,
    /// The minigame in play and its rules, in a minigame's module.
    pub minigame: Option<(&'a crate::minigame::Course, &'a crate::minigame::Rules)>,
    /// Whether a conversation holds the world this step (S1,
    /// `world::scene::Held`).
    pub held: bool,
}

/// A power cast this step (`world::powers`), for its impact script (I22
/// C10: JA delivers, KOTOR decides): the caster, the `spells.2da` row, the
/// one aimed at (`INVALID` for none), where (metres), and whom JA's
/// delivery caught (the impact's `GetFirst/NextObjectInShape`).
#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    pub caster: ObjectId,
    pub row: u16,
    pub target: ObjectId,
    pub at: [f32; 3],
    pub caught: Vec<ObjectId>,
}

/// An event `EventSpellCastAt` made, for `SignalEvent`: the caster, the
/// spell row, whether harmful.
pub type SpellEvent = (ObjectId, i32, bool);

/// The objects a `GetFirst…`/`GetNext…` pair walks, by (caller, kind),
/// and how far along: for one step's scripts.
pub(super) type Iterators = BTreeMap<(u32, u8), (Vec<ObjectId>, usize)>;

impl View<'_> {
    pub fn thing(&self, id: ObjectId) -> Option<&Thing> {
        self.things
            .binary_search_by_key(&id, |thing| thing.id)
            .ok()
            .map(|at| &self.things[at])
    }

    /// Where minigame thing `id` is (metres), if it is one
    /// (`crate::minigame::thing`).
    pub fn minigame_place(&self, id: ObjectId) -> Option<[f32; 3]> {
        let (course, rules) = self.minigame?;
        let run = course.run(crate::minigame::run(id.0)?)?;
        run.place(rules, crate::minigame::thing(id.0)?)
    }

    /// Object `index`'s facts (its instance's).
    pub fn object(&self, index: u32) -> Option<&ObjectData> {
        self.instances.get(index)
    }

    /// The facts of thing `id`, if it is an object of the area.
    pub fn facts(&self, id: ObjectId) -> Option<&ObjectData> {
        self.object(self.thing(id)?.index?)
    }

    /// The bodies the seats act through (`seats`), for whether a
    /// conversation holds the world (`Talk::holds_world`).
    pub fn seat_bodies(&self) -> Vec<ObjectId> {
        self.seats.iter().map(|(_, body)| *body).collect()
    }

    /// The `appearance.2da` row of `id`'s body: the one a script changed it
    /// to (`ChangeObjectAppearance`), else a creature's template's, a
    /// player's persona's.
    pub fn appearance(&self, id: ObjectId) -> Option<u16> {
        let changed = (self.thing(id))
            .and_then(|thing| thing.character.as_ref())
            .and_then(|c| c.look.appearance);
        if changed.is_some() {
            return changed;
        }
        match self.facts(id).and_then(|facts| facts.body) {
            Some(body) => Some(body.appearance),
            None => Some(self.thing(id)?.persona.as_ref()?.appearance),
        }
    }

    /// The script `id` runs for `hook`, if it names one: the module's, the
    /// area's, or a module object's.
    pub fn hook(&self, id: ObjectId, hook: Hook) -> Option<String> {
        let (holder, scripts): (_, &[(String, String)]) = match id {
            MODULE => (Holder::Module, &self.module.module_scripts),
            AREA => (Holder::Area, &self.module.area_scripts),
            _ => {
                let facts = self.facts(id)?;
                (Holder::Object(facts.kind), &facts.scripts)
            }
        };
        hook.in_scripts(holder, scripts).map(str::to_string)
    }

    /// Where `target` is: metres and facing.
    fn place(&self, target: Target) -> Option<([f32; 3], f32)> {
        match target {
            Target::Object(id) => self.thing(id).map(|thing| (thing.position, thing.yaw)),
            Target::Point(at, yaw) => Some((at, yaw)),
        }
    }
}

/// What a step's scripts change in the world, for `world::scripts` to
/// apply after them, and where moving objects route next step.
#[derive(Debug, Clone, Default)]
pub struct Out {
    pub changes: Vec<Change>,
    /// What scripts tell players: one character's, or (none) every one.
    pub notices: Vec<(Option<ObjectId>, crate::notice::Notice)>,
    /// By object id (units).
    pub goals: BTreeMap<u32, Goal>,
    /// Whom objects fight for their scripts, by object id.
    pub intents: BTreeMap<u32, Intent>,
    /// The area's sound as this step's scripts left it, if they touched
    /// it.
    pub area_sound: Option<crate::snapshot::AreaSound>,
    /// The screen's fade this step's scripts last set, if they set one.
    pub fade: Option<crate::snapshot::Fade>,
    /// The engine's fade in at the module's beginning, if they moved it.
    pub world_fade_in: Option<crate::snapshot::WorldFadeIn>,
    /// The video effect as this step's scripts last left it, if they
    /// enabled (a `videoeffects.2da` row) or disabled (`None`) one.
    pub video_effect: Option<Option<u16>>,
    /// The module a script started, if one did (the last, if several).
    pub new_module: Option<NewModule>,
    /// What each character whose equipment changed this step wore before
    /// (slot, template), until `items::dress_changed` dresses it anew.
    pub worn_before: BTreeMap<u32, Vec<items::Worn>>,
    /// Objects scripts made this step, in id order from `View::next_id`.
    pub made: Vec<Made>,
    /// The minigame as this step's scripts left it, once one changed it
    /// (`script::minigame`): what they read from then on.
    pub minigame: Option<crate::minigame::Course>,
    /// A script ended the game lost (`EndGame` with its death screen):
    /// the party's loss (`Snapshot::lost`).
    pub lost: bool,
    /// What this step's scripts told the party's status summary
    /// (`Progress::statuses`).
    pub statuses: Vec<crate::snapshot::StatusKind>,
    /// The feedback this step's scripts floated over objects (over whom,
    /// strref: `Progress::feedback`).
    pub feedback: Vec<(ObjectId, u32)>,
}

impl Out {
    /// The screen's fade as this step's scripts leave it so far.
    pub(super) fn fade(&self, view: &View) -> Option<crate::snapshot::Fade> {
        self.fade.or(view.fade)
    }

    /// Sets a fade from now, from as covered as the screen is (`Fade::set`).
    pub(super) fn set_fade(
        &mut self,
        view: &View,
        (wait, length): (f32, f32),
        colour: [f32; 3],
        out: bool,
    ) -> &mut crate::snapshot::Fade {
        let now = view.now_ms;
        let from = self.fade(view).map_or(0.0, |fade| fade.alpha(now));
        self.fade.insert(crate::snapshot::Fade::set(
            now, wait, length, colour, out, from,
        ))
    }

    /// The module's fade in as this step's scripts leave it so far.
    pub(super) fn world_fade_in(&self, view: &View) -> crate::snapshot::WorldFadeIn {
        self.world_fade_in.unwrap_or(view.world_fade_in)
    }

    /// A conversation starts: the engine lifts a covering fade not locked,
    /// and the module's fade in held for it (`snapshot::WorldFadeIn`).
    pub(super) fn lift_fade(&mut self, view: &View) {
        use crate::snapshot::WorldFadeIn;
        self.fade = self.fade(view).map(|fade| fade.lifted(view.now_ms));
        if self.world_fade_in(view) == WorldFadeIn::AtDialog {
            self.world_fade_in = Some(WorldFadeIn::Done);
        }
    }
}

/// An object a script made (`CreateObject`, `SpawnAvailableNPC`): its id,
/// its template's facts, where (metres) facing `yaw` (degrees).
#[derive(Debug, Clone, PartialEq)]
pub struct Made {
    pub id: ObjectId,
    pub facts: Arc<ObjectData>,
    pub at: [f32; 3],
    pub yaw: f32,
    /// A companion the party table kept (`make_npc`): made as it was.
    pub kept: Option<party::Kept>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Change {
    /// Put `who` at `to` (metres), facing `yaw`.
    Jump {
        who: ObjectId,
        to: [f32; 3],
        yaw: f32,
    },
    /// `by` opens or closes a door.
    Door {
        door: ObjectId,
        open: bool,
        by: ObjectId,
    },
    /// Lock or unlock a door or placeable (`SetLocked`, the lock actions).
    Lock { door: ObjectId, locked: bool },
    /// Show or hide a waypoint's note on the map (`SetMapPinEnabled`).
    MapPin { pin: ObjectId, enabled: bool },
    /// Explore the area's map (`ExploreAreaForPlayer`, `RevealMap`).
    Reveal(crate::fog::Reveal),
    /// Set a trap's bits (`snapshot::trap`).
    Trap { object: ObjectId, bits: u8 },
    /// Turn `who` to `yaw` (degrees).
    Face { who: ObjectId, yaw: f32 },
    /// `who` takes the first seat's view as it was (`view`, degrees) and
    /// its commands from `angles` on: the body a new party leader leads
    /// with, so the camera keeps its way (`world::scripts`).
    TakeView {
        who: ObjectId,
        angles: [i16; 3],
        view: [f32; 3],
    },
    /// `who` plays an `ANIMATION_…` constant for `ms` (`u32::MAX`: until
    /// stopped), from now.
    Animate { who: ObjectId, play: (u16, u32) },
    /// `who` struck by a scene's shot that landed without its bolt, its
    /// answer row `row` (`effects::blows::struck`).
    Struck { who: ObjectId, row: u16 },
    /// `who`'s actions cleared (`ClearAllActions`): what it plays stops,
    /// and so does its chase of a foe out of sight, the mind's stand-in
    /// for KOTOR's attack still in the queue.
    Clear { who: ObjectId },
    /// A move of `who`'s begun (`Action::Move`, `anew`): its walk is a new
    /// one, its bumps and stalls counted afresh, as KOTOR's walker paths
    /// each move anew; or a forced move's walk taken up again: its bumps
    /// counted afresh. An animation looping on until something else plays
    /// (`u32::MAX`) ends.
    Walk { who: ObjectId, anew: bool },
    /// A sound object started, stopped (`snapshot::sound::*`) or its
    /// volume set (1 + 0..=127).
    Sound {
        object: ObjectId,
        state: Option<u8>,
        volume: Option<u8>,
        pitch: Option<u16>,
        /// Started (`SoundObjectPlay`): from its first sound, at once.
        play: bool,
        /// Stopped over this long (ms; `SoundObjectFadeAndStop`), or at once.
        fade: Option<u16>,
    },
    /// `who` joins the party: it follows and fights for the first seat.
    Join { who: ObjectId },
    /// `who` leaves the party: it fights for itself again.
    Leave { who: ObjectId },
    /// `DestroyObject` on a thing: it leaves the world after `ms`, drawn
    /// going as `going` says (none: not drawn after).
    Destroy {
        who: ObjectId,
        ms: u32,
        going: Option<crate::snapshot::Going>,
    },
    /// `who`'s sheet as its equipment now dresses it (`script::items`),
    /// or as a script set its flags (`SetMinOneHP`).
    Sheet {
        who: ObjectId,
        sheet: crate::combat::Sheet,
    },
    /// `who`'s sheet with the items it may use as its holder's goods have
    /// them (`world::item_uses`), and its uses not yet taken from them.
    Usable {
        who: ObjectId,
        sheet: crate::combat::Sheet,
        used: [u8; crate::combat::items::KINDS],
    },
    /// `who` fights for standard faction `faction` from now
    /// (`ChangeToStandardFaction`).
    Faction { who: ObjectId, faction: u8 },
    /// `who`'s hit points set to `hp` (whole), at most its most
    /// (`SetMaxHitPoints` makes it whole at its new most).
    HitPoints { who: ObjectId, hp: u16 },
    /// `who` took a level, its sheet `before` it and `after`: its hit points
    /// whole at its new most, its Force points up by what their most rose
    /// (both games' `CSWSCreatureStats::LevelUp`; `script::levelling`).
    Levelled {
        who: ObjectId,
        before: crate::combat::Sheet,
        after: crate::combat::Sheet,
    },
    /// `who` leaves combat: no one to fight, out of it from now
    /// (`CancelCombat`).
    CancelCombat { who: ObjectId },
    /// `who` attacks a target in its reach with a blade or fists: in
    /// combat from now, as after a blow (`script::actions`, attack).
    InCombat { who: ObjectId },
    /// `who` in TSL's fake combat (`on`, unless it fights for real) or out
    /// of it (`SetFakeCombatState`, `combat::Fighter::fake_until`).
    FakeCombat { who: ObjectId, on: bool },
    /// `who` flourishes its weapon (`CreatureFlourishWeapon`): lit, in fake
    /// combat for 8 s.
    Flourish { who: ObjectId },
    /// `who`'s lightsaber lit or put out by a script
    /// (`SetLightsaberPowered`), held lit until a script lets go, or by a
    /// scene's blow, lit as in a fight (`combat::Sheath`).
    SaberPower { who: ObjectId, on: bool, held: bool },
    /// `who` used an item of `spells.2da` row `row` on itself that an item
    /// button takes (Use Item, an item talent; `script::items`): its fighter
    /// takes it as that button's press would (`combat::items::Pack::queued`).
    UseItem { who: ObjectId, row: u16 },
    /// A player swapped `who`'s weapon sets (`Action::SwitchWeapons`): in a
    /// fight its weapon waits `combat::SWAP_MS` after what it was doing.
    Swapped { who: ObjectId },
    /// A conversation began around `around`: its PC, or the body whose use
    /// started it; or the host joined one, around its owner (R5). Those
    /// `who` names, far off, come to it
    /// (`world::coop_talk::gather`).
    Gather { around: ObjectId, who: Gathered },
    /// `who` holds its hand out for `ms` from now: a script's cast
    /// (`Action::Cast`), with the hand every power here shows.
    Hand { who: ObjectId, ms: u32 },
    /// `who` pays `cost` (hundredths of FP, `engine::cast_cost`) as a cast
    /// landing does (`combat::effects::charge`: Force Body's share, and as
    /// much from its health, what it lacks forgiven).
    Pay { who: ObjectId, cost: i32 },
    /// `who`'s stealth mode turned on or off (`ActionUseSkill`).
    Stealth { who: ObjectId, on: bool },
    /// `who`'s body as scripts made it for good (`combat::Look`:
    /// `ChangeObjectAppearance`, `DuplicateHeadAppearance`).
    Look {
        who: ObjectId,
        look: crate::combat::Look,
    },
    /// A sound object moved to `to` (metres; `SoundObjectSetPosition`).
    Place { object: ObjectId, to: [f32; 3] },
    /// `who` out of stealth mode (a conversation began with it).
    Unstealth { who: ObjectId },
}

/// Who a conversation brings to it as it begins (`Change::Gather`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gathered {
    /// A player's own conversation, its listener seat `.0`'s body (R1):
    /// the companions following that seat that no seat drives, as KOTOR's
    /// party comes to its leader's, and that seat's own character (the
    /// host's, while it leads through a companion). A guest's brings
    /// nobody.
    Companions(u8),
    /// Every party member, the seats' bodies too: a party conversation.
    Everyone,
}

impl Change {
    /// `who` taken away at once, unfaded.
    pub fn destroy(who: ObjectId) -> Self {
        Change::Destroy {
            who,
            ms: 0,
            going: None,
        }
    }

    /// `who`, of `kind`, taken away now, fading as `DestroyObject` fades
    /// one.
    pub fn fade_away(who: ObjectId, kind: Option<ObjectKind>) -> Self {
        Change::Destroy {
            who,
            ms: 0,
            going: engine::destroy_going(kind, false, 0.0),
        }
    }
}

/// `StartNewModule`: the party goes to `module`, arriving at the door or
/// waypoint tagged `tag` (empty: the module's entry); `by` asked.
#[derive(Debug, Clone, PartialEq)]
pub struct NewModule {
    pub by: ObjectId,
    pub module: String,
    pub tag: String,
}

impl Out {
    /// `who`'s sheet as this step's scripts leave it: the last one they
    /// set, else `worn` (the sheet its items make it).
    pub(super) fn sheet(&self, who: ObjectId, worn: crate::combat::Sheet) -> crate::combat::Sheet {
        self.changes
            .iter()
            .rev()
            .find_map(|change| match change {
                Change::Sheet { who: of, sheet } if *of == who => Some(*sheet),
                _ => None,
            })
            .unwrap_or(worn)
    }
}

/// What scripts did, for reports (not state: nothing reads it back).
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Scripts run from their start, and deferred blocks run, by script.
    pub runs: BTreeMap<String, usize>,
    pub deferred: BTreeMap<String, usize>,
    /// Calls by routine: bound, and gaps.
    pub calls: BTreeMap<String, usize>,
    pub gaps: BTreeMap<String, usize>,
    /// Actions completed, by kind.
    pub actions: BTreeMap<&'static str, usize>,
    pub faults: Vec<String>,
    /// Runs stopped at the instruction limit, as KOTOR's machine stops
    /// them: the games' own endless loops, not ours.
    pub runaways: Vec<String>,
    /// Scripts named to run that the library does not have, by times
    /// named: nothing runs (the games ship hooks naming scripts they lack).
    pub missing: BTreeMap<String, usize>,
    pub instructions: usize,
    /// Conversations' lines, offers, picks and ends, and the journal's
    /// updates, in order (I18, I19).
    pub talk: Vec<dialogue::Line>,
    /// One line a run and a call: the first `TRACE_LINES`, then the last
    /// `TRACE_TAIL` (`tail`), those between dropped.
    pub trace: Vec<String>,
    pub tail: std::collections::VecDeque<Traced>,
    pub trace_dropped: usize,
    /// Every script jump, where it was asked and where it landed
    /// (`kjk dialogue check`).
    #[cfg(feature = "probes")]
    pub jumps: Vec<Jumped>,
    /// Every script move as it ended (`kjk dialogue check`).
    #[cfg(feature = "probes")]
    pub moves: Vec<Moved>,
    /// The objects a conversation's script gave actions to, and when (sim
    /// ms): the ones it directs (`kjk dialogue check`).
    #[cfg(feature = "probes")]
    pub directed: Vec<(ObjectId, u64)>,
}

/// A script's jump as it came out: when (sim ms), who, its mark (metres,
/// the soles), where the body stood (its soles), where it landed (its
/// soles; none: no free place, no jump), and the body nearest the mark that
/// took the place (none: the floor at the mark itself was not free).
#[cfg(feature = "probes")]
#[derive(Debug, Clone, PartialEq)]
pub struct Jumped {
    pub ms: u64,
    pub who: ObjectId,
    pub mark: [f32; 3],
    pub from: [f32; 3],
    /// Whether the walkmesh lies under the mark (one off it lands on the
    /// nearest ring place on it, as the executables' search puts it).
    pub on_mesh: bool,
    pub landed: Option<[f32; 3]>,
    pub displaced_by: Option<u32>,
}

/// A script move as it ended: who, when it began and ended (sim ms), how,
/// where the body stood then and where it was sent (its origin's, units),
/// and whether it was a party member while a conversation held the world
/// (it opens no door, S4).
#[cfg(feature = "probes")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moved {
    pub who: ObjectId,
    pub from_ms: u64,
    pub to_ms: u64,
    pub ended: Ended,
    pub at: [f32; 3],
    pub to: [f32; 3],
    pub held_party: bool,
}

/// How a script move ended: there; no way leading there; its walk given
/// up short of it; or, forced, jumped there at its timeout, its walk
/// held (`TimedOut`) or still carrying it on (`TimedOutWalking`: the
/// script's timeout is shorter than the walk, as KOTOR's walkways make a
/// long leg's, and the jump is the script's own plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Arrived,
    NoWay,
    GaveUp,
    TimedOut,
    TimedOutWalking,
}

impl Report {
    /// `script`'s program, or none: then it is counted missing.
    fn program<'a>(&mut self, library: &'a Library, script: &str) -> Option<&'a Program> {
        let program = library.program(script);
        if program.is_none() {
            self.line(format!("missing {script}: nothing runs"));
            *self.missing.entry(key(script)).or_insert(0) += 1;
        }
        program
    }

    fn line(&mut self, text: String) {
        self.keep(Traced::Line(text));
    }

    /// A line kept: among the first written out, else into the tail as it
    /// is (a call written out only when the trace is read).
    fn keep(&mut self, traced: Traced) {
        if self.trace.len() < TRACE_LINES {
            self.trace.push(traced.text());
        } else {
            self.tail.push_back(traced);
            if self.tail.len() > TRACE_TAIL {
                self.tail.pop_front();
                self.trace_dropped += 1;
            }
        }
    }

    /// The trace kept: its first lines, how many were dropped, its last.
    pub fn trace_lines(&self) -> Vec<String> {
        let mut lines = self.trace.clone();
        if self.trace_dropped > 0 {
            lines.push(format!("… {} lines dropped …", self.trace_dropped));
        }
        lines.extend(self.tail.iter().map(Traced::text));
        lines
    }
}

/// A line of the trace's tail (`Report::tail`): written, or a routine's
/// call kept as its parts, written out only when the trace is read
/// (`Report::trace_lines`). Most calls fall out of the tail unread, and
/// writing each out as it was made was much of a long run's trace.
#[derive(Debug, Clone)]
pub enum Traced {
    Line(String),
    Call(Box<CallLine>),
}

/// A routine's call for the trace (`Traced::Call`): when (sim ms), the
/// routine, its arguments (`None`: deferred code), whether it is a gap, and
/// what it gave (`None` for a routine that gives nothing).
#[derive(Debug, Clone)]
pub struct CallLine {
    pub ms: u64,
    pub routine: String,
    pub args: Vec<Option<vm::Value>>,
    pub gap: bool,
    pub value: Option<vm::Value>,
}

impl Traced {
    /// The line as the trace shows it.
    fn text(&self) -> String {
        match self {
            Traced::Line(text) => text.clone(),
            Traced::Call(call) => call.text(),
        }
    }
}

impl CallLine {
    /// The call as the trace shows it: when, the routine and its
    /// arguments, and what it gave.
    fn text(&self) -> String {
        use std::fmt::Write as _;
        let call = self;
        let mut line = format!("{:>8.2}s   {}(", call.ms as f64 / 1000.0, call.routine);
        for (n, arg) in call.args.iter().enumerate() {
            let comma = if n > 0 { ", " } else { "" };
            let _ = match arg {
                Some(vm::Value::String(text)) => write!(line, "{comma}{text:?}"),
                Some(vm::Value::Object(id)) => write!(line, "{comma}{}", name(*id)),
                Some(vm::Value::Int(v)) => write!(line, "{comma}{v}"),
                Some(vm::Value::Float(v)) => write!(line, "{comma}{v}"),
                Some(other) => write!(line, "{comma}{other:?}"),
                None => write!(line, "{comma}<code>"),
            };
        }
        line.push(')');
        if call.gap {
            line.push_str("  (gap)");
        }
        let _ = match &call.value {
            Some(vm::Value::Int(v)) => write!(line, " = {v}"),
            Some(vm::Value::Object(id)) => write!(line, " = {}", name(*id)),
            Some(vm::Value::String(text)) if !text.is_empty() => write!(line, " = {text:?}"),
            _ => Ok(()),
        };
        line
    }
}

/// An event's facts for its script (`Pending`'s, or none).
#[derive(Debug, Clone, Default)]
pub(super) struct Event {
    pub entering: Option<ObjectId>,
    pub number: i32,
    /// `ExecuteScript`'s `nScriptVar` (`GetRunScriptVar`).
    pub script_var: i32,
    /// A conversation script's parameters (TSL's `GetScriptParameter`).
    pub params: Option<Arc<fmt_odyssey::dlg::Call>>,
    /// The creature whose own AI this run is (`engine::own_ai`), carried
    /// into what it executes; deferred code is not followed.
    pub ai: Option<ObjectId>,
    /// The companion a guest drives whose own act raised the event
    /// (`Pending::leads`): this run, and what it executes, see it lead the
    /// party, as KOTOR's scripts see the character its player controls
    /// (`Engine::leading`); deferred code is not followed. The Endar
    /// Spire's first door opens for Trask leading (`k_pend_door01`), so
    /// for Trask a guest plays too.
    pub leads: Option<ObjectId>,
    /// The body whose use raised it (`Pending::asker`), carried into what
    /// it executes: a conversation it starts is that body's (R1).
    pub asker: Option<ObjectId>,
    /// The conversation whose script deferred the code running
    /// (`Delayed::talk`).
    pub talk: Option<u32>,
}

/// A step's scripts: the library, the world as they read it, and what
/// they keep and change.
struct Step<'a> {
    library: &'a Library,
    view: &'a View<'a>,
    state: &'a mut State,
    rng: &'a mut SimRng,
    report: &'a mut Report,
    out: &'a mut Out,
    iterators: Iterators,
    runs: usize,
    /// The seats whose Leave stopped their watching this step (R9a,
    /// `pazaak::asked_to_watch`): spent, it leaves no conversation.
    left_watching: Vec<u8>,
}

impl Step<'_> {
    /// `who` was an object and is gone (destroyed): what it had waiting,
    /// its delayed commands and the events sent it, goes with it, as in
    /// KOTOR (Korriban's academy, its students destroyed with their AI's
    /// commands still due). The module, the area, a minigame's things (ids
    /// from `MODULE` on), items and one made this step are no such object.
    fn gone(&self, who: ObjectId) -> bool {
        who.0 < MODULE.0
            && self.view.thing(who).is_none()
            && !self.state.items.contains_key(&who.0)
            && !self.out.made.iter().any(|made| made.id == who)
    }

    /// A player closed the party selection screen (`View::chosen`): the
    /// forced companions and those picked make the party, each on its
    /// creature here if it stands, else made beside the player; the others
    /// leave and go, as the screen sends them away. The screen's script,
    /// for the player, if it names one. A choice that would send away a
    /// companion another player is busy with is refused, the screen kept
    /// open (R11).
    fn choose_party(&mut self) -> Option<Pending> {
        let &(player, picks) = self.view.chosen.first()?;
        let selection = self.state.party.selection.take()?;
        // Unchanged, the screen keeps whom it opened with: the party.
        let picks = picks.map_or_else(|| self.state.party.members.clone(), |picks| picks.to_vec());
        let wanted = self.state.party.chosen(&selection, &picks);
        if let Some(busy) = self.busy_benched(&wanted) {
            self.state.party.selection = Some(selection);
            let said = crate::notice::Notice::Said(format!("{busy} is busy."));
            self.out.notices.push((Some(player), said));
            return None;
        }
        for npc in self.state.party.members.clone() {
            if !wanted.contains(&npc) {
                keep_npc(self.view, self.state, self.out, npc);
            }
            if !wanted.contains(&npc)
                && let Some(who) = self.state.party.leave(npc)
            {
                let who = ObjectId(who);
                self.out.changes.push(Change::Leave { who });
                self.out.changes.push(Change::destroy(who));
                puppet_leaves(self.view, self.state, self.out, npc);
            }
        }
        let (at, yaw) = self
            .view
            .thing(player)
            .map_or(([0.0; 3], 0.0), |thing| (thing.position, thing.yaw));
        for (n, npc) in wanted.iter().enumerate() {
            let Some(slot) = self.state.party.available.get(npc) else {
                continue;
            };
            // A downed member keeps its body: never a second from its
            // template.
            let standing = slot.object.map(ObjectId).filter(|id| {
                self.view.thing(*id).is_some_and(|t| {
                    t.hp.is_none_or(|(hp, _)| hp > 0) || t.character.is_some_and(|c| c.downed)
                })
            });
            // A member whose creature is gone joins again on the one made.
            if standing.is_none() {
                self.state.party.leave(*npc);
            }
            let who = match standing {
                Some(id) => Some(id),
                None => {
                    // A step to the player's left, then right.
                    let step = rules::scripts::moves::BESIDE_M;
                    let side = if n == 0 { step } else { -step };
                    make_npc(
                        self.library,
                        self.view,
                        self.state,
                        self.out,
                        *npc,
                        beside(at, yaw, side),
                        yaw,
                    )
                }
            };
            if let Some(who) = who
                && !self.state.party.has(*npc)
                && self.state.party.join(*npc, who.0)
            {
                self.state.takes_in(who.0);
                self.out.changes.push(Change::Join { who });
                influence::follow(self.library, self.view, self.state, self.out, who);
                puppet_joins(self.library, self.view, self.state, self.out, *npc, who);
            }
        }
        (!selection.exit.is_empty()).then(|| Pending::new(&selection.exit, player))
    }

    /// The name of a member leaving the party for `wanted` whom a seat
    /// drives while it stands in a conversation, trades or plays a match
    /// (R11), if any.
    fn busy_benched(&self, wanted: &[u8]) -> Option<String> {
        let (view, state) = (self.view, &*self.state);
        let busy = |body: ObjectId| {
            state.talk.holding(body).is_some()
                || state.trades.contains_key(&body.0)
                || state.plays_pazaak(body)
        };
        state
            .party
            .members
            .iter()
            .filter(|npc| !wanted.contains(npc))
            .filter_map(|npc| state.party.available.get(npc)?.object.map(ObjectId))
            .find(|body| view.seat_of(*body).is_some() && busy(*body))
            .map(|body| {
                let facts = view.facts(body);
                facts.map_or_else(String::new, |facts| facts.name.clone())
            })
    }

    /// Runs `script` for `caller` from its start (`None`) or deferred code;
    /// the int it returns, if it ran to its end with one.
    fn run(
        &mut self,
        script: &str,
        caller: ObjectId,
        event: Event,
        code: Option<&Deferred>,
    ) -> Option<i32> {
        self.run_as(script, caller, event, code, None)
    }

    /// `run`, as a cast's impact (`spell`) if one.
    fn run_as(
        &mut self,
        script: &str,
        caller: ObjectId,
        event: Event,
        code: Option<&Deferred>,
        spell: Option<&Cast>,
    ) -> Option<i32> {
        let program = self.report.program(self.library, script)?;
        self.runs += 1;
        let counts = if code.is_some() {
            &mut self.report.deferred
        } else {
            &mut self.report.runs
        };
        *counts.entry(script.to_string()).or_insert(0) += 1;
        let what = if code.is_some() { "deferred" } else { "run" };
        let mut engine = Engine {
            library: self.library,
            view: self.view,
            state: &mut *self.state,
            rng: &mut *self.rng,
            report: &mut *self.report,
            out: &mut *self.out,
            iterators: &mut self.iterators,
            spell,
            script,
            event,
            depth: 0,
        };
        engine.trace(&format!("{what} {script} for {}", name(caller)));
        let routines = self.library.routines();
        let outcome = match code {
            Some(deferred) => vm::resume(program, routines, caller, deferred, &mut engine),
            None => vm::run(program, routines, caller, &mut engine),
        };
        finish(self.report, script, outcome)
    }

    /// Moves each object's queue on (see `actions`), in id order.
    fn advance(&mut self) {
        let now = self.view.now_ms;
        let ids: Vec<u32> = self.state.actions.keys().copied().collect();
        for id in ids {
            let who = ObjectId(id);
            let Some(thing) = self.view.thing(who).cloned() else {
                // One made this step stands from its end: its queue waits.
                if !self.out.made.iter().any(|made| made.id == who) {
                    // Its lock goes with its queue (`enter_module`).
                    self.state.actions.remove(&id);
                    self.state.uncommandable.remove(&id);
                }
                continue;
            };
            // The dead act on nothing (their queue waits, should they rise).
            if thing.character.is_some() && thing.dead() {
                continue;
            }
            let player = thing.kind.is_none();
            for _ in 0..ACTIONS_PER_STEP {
                if self.runs >= RUNS_PER_STEP {
                    return;
                }
                let Some(front) = self.state.actions.get_mut(&id).and_then(|q| q.first_mut())
                else {
                    break;
                };
                let starting = front.started_ms.is_none();
                let started = *front.started_ms.get_or_insert(now);
                let action = front.action.clone();
                let pop = |state: &mut State| {
                    if let Some(queue) = state.actions.get_mut(&id) {
                        queue.remove(0);
                    }
                };
                if starting && matches!(action, Action::Move { .. }) {
                    self.out.changes.push(Change::Walk { who, anew: true });
                }
                let done = match &action {
                    // Code ends the object's turn this step: KOTOR starts
                    // one action a frame, so code that queues itself again
                    // (an idle loop) runs once a step, not on and on.
                    Action::Code { script, deferred } => {
                        pop(self.state);
                        self.run(script, who, Event::default(), Some(deferred));
                        *self.report.actions.entry("code").or_insert(0) += 1;
                        break;
                    }
                    // Everything queued goes with it (the executable's own
                    // clear, whether or not scripts may command it), then
                    // the swap; in combat with a target it attacks on.
                    Action::SwitchWeapons => {
                        self.state.actions.remove(&id);
                        self.out.changes.push(Change::Clear { who });
                        switch_weapons(self.library, self.view, self.state, self.out, who);
                        let character = thing.character.as_ref();
                        if let Some(target) = character
                            .filter(|character| character.in_combat)
                            .and_then(|character| character.target)
                        {
                            self.state.queue(
                                who,
                                Action::Attack {
                                    target,
                                    passive: false,
                                },
                            );
                        }
                        *self.report.actions.entry(action.kind()).or_insert(0) += 1;
                        break;
                    }
                    Action::Wait { ms } => now >= started + u64::from(*ms),
                    Action::Animate { animation, ms } => {
                        if starting {
                            self.out.changes.push(Change::Animate {
                                who,
                                play: (*animation, *ms),
                            });
                        }
                        now >= started + u64::from(*ms)
                    }
                    // A player's legs are its seat's, but in a conversation
                    // (a cinematic's) a script walks it.
                    Action::Move { .. }
                        if player && !self.state.talk.conversations.iter().any(|c| c.pc == who) =>
                    {
                        true
                    }
                    // While a conversation holds the world, a move to a place
                    // arrives on it (KOTOR's range 0, to the queue's floor,
                    // `ARRIVAL_FLOOR`), and a walk that can come no nearer is
                    // one KOTOR's walker gave up on bumping into bodies (S3),
                    // its stalls never counted.
                    Action::Move {
                        to,
                        run,
                        range,
                        force_ms,
                    } => match self.view.place(*to) {
                        None => true,
                        Some((at, _))
                            if flat(thing.position, at)
                                <= (self.arrival(to, *range))
                                    .max(rules::scripts::moves::ARRIVAL_FLOOR) =>
                        {
                            self.moved(&thing, started, Ended::Arrived, at);
                            true
                        }
                        Some((at, yaw))
                            if force_ms.is_some_and(|ms| now >= started + u64::from(ms)) =>
                        {
                            self.out.changes.push(Change::Jump { who, to: at, yaw });
                            let ended = if thing.character.as_ref().is_some_and(|c| c.walking) {
                                Ended::TimedOutWalking
                            } else {
                                Ended::TimedOut
                            };
                            self.moved(&thing, started, ended, at);
                            true
                        }
                        // Off the floor (thrown by a blow, falling): its
                        // clock waits for it to land, so the way there is
                        // judged from where it stands, as KOTOR's movers,
                        // never off the floor, plan theirs (Malak knocked
                        // off the Star Forge's ledge as a drain begins).
                        Some((at, _))
                            if force_ms.is_none()
                                && thing.character.as_ref().is_some_and(|c| c.airborne) =>
                        {
                            if let Some(front) =
                                self.state.actions.get_mut(&id).and_then(|q| q.first_mut())
                            {
                                front.started_ms = Some(now);
                            }
                            self.goal(id, at, self.arrival(to, *range), *run);
                            false
                        }
                        // No way leads there: the move fails, and what was
                        // queued after it goes on; `MOVE_GRACE_MS` on, so a
                        // door a script opens in the same step (a gate the
                        // scene opens for the party) is open by then.
                        Some((at, _))
                            if now >= started + MOVE_GRACE_MS
                                && now < started + MOVE_GRACE_MS + MOVE_CHECK_MS
                                && force_ms.is_none()
                                && !(self.view.leads)(thing.position, at) =>
                        {
                            self.moved(&thing, started, Ended::NoWay, at);
                            true
                        }
                        // A forced move tries on until its timeout, path-
                        // finding again (the NWN Lexicon's
                        // `ActionForceMoveToObject`): a walk given up is
                        // taken up afresh, so a Vulkar walking out of
                        // Javyar's Cantina past Mission at the bar does not
                        // stand there until the timeout jumps it out.
                        Some((at, _))
                            if force_ms.is_some()
                                && thing.character.as_ref().is_some_and(|c| c.stuck) =>
                        {
                            self.out.changes.push(Change::Walk { who, anew: false });
                            self.goal(id, at, self.arrival(to, *range), *run);
                            false
                        }
                        // As near as it can come (its walk stalled on and
                        // on: Malak's run to a captive Jedi's prison, inside
                        // its machine): done there, as KOTOR's mover gives up
                        // short of where it cannot reach.
                        Some((at, _))
                            if force_ms.is_none()
                                && now >= started + MOVE_GRACE_MS
                                && thing.character.as_ref().is_some_and(|c| c.stuck) =>
                        {
                            self.moved(&thing, started, Ended::GaveUp, at);
                            true
                        }
                        Some((at, _)) => {
                            self.goal(id, at, self.arrival(to, *range), *run);
                            false
                        }
                    },
                    Action::Jump { to } => {
                        if let Some((at, yaw)) = self.view.place(*to) {
                            self.out.changes.push(Change::Jump { who, to: at, yaw });
                        }
                        true
                    }
                    // A party member uses and opens nothing while a
                    // conversation holds the world (S4): the action ends,
                    // failed.
                    Action::Door { .. } | Action::UseObject { .. } | Action::Lock { .. }
                        if self.held_party(&thing) =>
                    {
                        true
                    }
                    &Action::Door { door, open } => {
                        self.within_reach(id, &thing, player, door, |step| {
                            step.out.changes.push(Change::Door {
                                door,
                                open,
                                by: who,
                            });
                        })
                    }
                    &Action::UseObject { object } => {
                        self.within_reach(id, &thing, player, object, |step| {
                            step.use_object(who, object);
                        })
                    }
                    &Action::Lock { object, locked } => {
                        self.within_reach(id, &thing, player, object, |step| {
                            step.lock(who, object, locked);
                        })
                    }
                    // Over once the door no longer may be broken (broken,
                    // unlocked); there, a blow a round, as KOTOR's attack.
                    Action::Bash { .. } if player => true,
                    &Action::Bash { door } => {
                        let bashable = self.view.thing(door).is_some_and(Thing::bashable);
                        if bashable
                            && self.within_reach(id, &thing, false, door, |_| {})
                            && now >= started + ROUND_MS
                        {
                            self.state.bashes.push((door.0, who.0));
                            if let Some(front) =
                                self.state.actions.get_mut(&id).and_then(|q| q.first_mut())
                            {
                                front.started_ms = Some(now);
                            }
                        }
                        !bashable
                    }
                    Action::RandomWalk { .. } if player => true,
                    Action::RandomWalk { from, to } => {
                        let stale = now >= started + WANDER_MS;
                        // Round where it stood as the walk began, as
                        // nwscript.nss has it ("a random location near its
                        // current location"), not where its walk has taken
                        // it since: a herd grazes where its spawn script
                        // set it walking rather than drifting off across
                        // the map (Tatooine's banthas) [verify in the game:
                        // whether each new point is near where it stands].
                        use rules::scripts::wander;
                        let from = from.unwrap_or(thing.position);
                        let point = match to {
                            Some(at) if !stale && flat(thing.position, *at) > wander::REACHED_M => {
                                *at
                            }
                            _ => {
                                let angle = self.rng.below(360) as f32;
                                let far = wander::LEAST_M
                                    + self.rng.below(100) as f32 / 100.0
                                        * (WANDER_M - wander::LEAST_M);
                                let (sin, cos) = crate::math::sin_cos_deg(angle);
                                let at =
                                    [from[0] + cos * far, from[1] + sin * far, thing.position[2]];
                                if let Some(front) =
                                    self.state.actions.get_mut(&id).and_then(|q| q.first_mut())
                                {
                                    front.action = Action::RandomWalk {
                                        from: Some(from),
                                        to: Some(at),
                                    };
                                    front.started_ms = Some(now);
                                }
                                at
                            }
                        };
                        self.goal(id, point, wander::ARRIVE_M, false);
                        false
                    }
                    Action::Attack { .. } if player => true,
                    Action::Attack { target, passive } => {
                        let alive = self
                            .view
                            .thing(*target)
                            .and_then(|thing| thing.hp)
                            .is_some_and(|(hp, _)| hp > 0);
                        // It lasts to the end of the object's round, as
                        // KOTOR's does; the round it began in, not the next
                        // ones a fight rolls on into (what was queued after
                        // it waits). With no round begun it closes in until
                        // it reaches its target, as KOTOR's attack walks to
                        // it (reone's follows until in range: Kashyyyk's
                        // katarns to Jolee), unless its walk can come no
                        // nearer (`stuck`). A cap of four rounds dropped a
                        // Star Forge freezer's attack on a player who kept
                        // moving: it stood idle, and the door it held stayed
                        // shut.
                        let end = self.state.rounds.get(&id).copied();
                        let over = match end {
                            Some(end) => now >= end.min(started + ROUND_MS),
                            None => thing.character.as_ref().is_some_and(|c| c.stuck),
                        };
                        if starting && !*passive {
                            self.attacking(id, None);
                        }
                        if alive && !over {
                            let intent = Intent {
                                target: *target,
                                passive: *passive,
                                talent: None,
                            };
                            self.out.intents.insert(id, intent);
                            // A blade or fists with the target within
                            // reach are in a round, and so in combat,
                            // whether or not a blow lands (reone runs a
                            // round for an attack in reach; a gun's every
                            // shot keeps it in combat already): a duel no
                            // blow lands in for a while (the Endar Spire's
                            // Jedi, a scene holding them apart) still has
                            // its rounds. From its second step: its mind
                            // first meets the target as a foe newly spotted
                            // (Awareness).
                            if !*passive && self.in_reach(&thing, *target) {
                                if !starting && !wields_gun(&thing) {
                                    self.out.changes.push(Change::InCombat { who });
                                }
                                self.assured(who, (&thing, None), *target, started);
                            }
                        }
                        !alive || over
                    }
                    Action::Talent { .. } if player => true,
                    Action::Talent { talent, target } => {
                        // To the end of its round, as an attack (time to
                        // come within the talent's reach).
                        let end = self.state.rounds.get(&id).copied();
                        let over = now >= end.unwrap_or(u64::MAX).min(started + ROUND_MS);
                        if starting {
                            self.attacking(id, Some(*talent));
                        }
                        if !over {
                            let intent = Intent {
                                target: *target,
                                passive: false,
                                talent: Some(*talent),
                            };
                            self.out.intents.insert(id, intent);
                            let feat = match talent {
                                Use::Feat(row) => Some(*row),
                                Use::Power(_) => None,
                            };
                            self.assured(who, (&thing, feat), *target, started);
                        }
                        over
                    }
                    Action::Converse {
                        with,
                        dialog,
                        asker,
                    } => {
                        let (with, dialog, asker) = (*with, dialog.clone(), *asker);
                        self.converse(who, with, &dialog, asker);
                        true
                    }
                    &Action::Hold { talk } => {
                        self.resume(who, talk);
                        true
                    }
                    Action::FollowOwner { .. } if player => true,
                    // Held while its owner is near (or it has none); farther,
                    // a run there and the follow again behind what waits,
                    // which start next step (one action a frame, as KOTOR).
                    &Action::FollowOwner { range } => {
                        let alive = thing.hp.is_none_or(|(hp, _)| hp > 0);
                        let party = &self.state.party;
                        if !alive || party.puppet_of(id, true).is_none() {
                            true
                        } else if let Some(owner) = party
                            .puppet_owner(id)
                            .and_then(|owner| self.view.thing(ObjectId(owner)))
                            .map(|owner| owner.position)
                            .filter(|at| flat(thing.position, *at) >= range)
                        {
                            let run = Action::Move {
                                to: Target::Point(owner, 0.0),
                                run: true,
                                range,
                                force_ms: None,
                            };
                            pop(self.state);
                            self.state.queue(who, run);
                            self.state.queue(who, action.clone());
                            *self.report.actions.entry(action.kind()).or_insert(0) += 1;
                            break;
                        } else {
                            false
                        }
                    }
                    Action::Instant { .. } => true,
                    Action::Surrender => {
                        engine::surrender(self.view, self.state, self.out, who, false);
                        true
                    }
                    &Action::Cast {
                        row,
                        target,
                        fake,
                        cheat,
                    } => self.cast(&thing, (row, target), (fake, cheat), (starting, started)),
                    &Action::Bark { strref } => {
                        dialogue::bark(self.library, self.state, self.report, now, (who, strref));
                        true
                    }
                };
                if !done {
                    break;
                }
                pop(self.state);
                *self.report.actions.entry(action.kind()).or_insert(0) += 1;
            }
            if self.state.actions.get(&id).is_some_and(Vec::is_empty) {
                self.state.actions.remove(&id);
            }
        }
    }

    /// A cast action's step (`Action::Cast`), `thing` casting `row` at
    /// `target`: it begins turned to its target with its hand out (the
    /// hand every power here shows; the row's own animations, hand and
    /// ground visuals and projectile are not drawn) for the row's conjure
    /// and cast time (`spells.2da` `conjtime` + `casttime`), done then. A
    /// real one is paid for then (none for a `cheat`'s) and its impact
    /// script runs at the target; a fake one does nothing more. A real one
    /// by a player (its powers are its seat's), by one not commandable, or
    /// of a power it does not know or cannot pay for (but a cheat's), does
    /// nothing.
    fn cast(
        &mut self,
        thing: &Thing,
        (row, target): (u16, Target),
        (fake, cheat): (bool, bool),
        (starting, started): (bool, u64),
    ) -> bool {
        let who = thing.id;
        let now = self.view.now_ms;
        let ms = self.library.spell(row).cast_ms;
        if starting {
            let able = thing.kind.is_some()
                && thing.character.is_some()
                && (cheat || engine::can_cast(self.view, thing, row));
            if !fake && (!able || self.state.uncommandable.contains(&who.0)) {
                return true;
            }
            if let Some((at, _)) = self
                .view
                .place(target)
                .filter(|(at, _)| *at != thing.position)
            {
                let yaw =
                    crate::math::atan2_deg(at[1] - thing.position[1], at[0] - thing.position[0]);
                self.out.changes.push(Change::Face { who, yaw });
            }
            self.out.changes.push(Change::Hand {
                who,
                ms: u32::from(ms),
            });
        }
        if now < started + u64::from(ms) {
            return false;
        }
        if !fake {
            if let Some(cost) = engine::cast_cost(self.view, thing, row).filter(|_| !cheat) {
                self.out.changes.push(Change::Pay { who, cost });
            }
            let (object, at) = match target {
                Target::Object(id) => (
                    id,
                    self.view.thing(id).map_or(thing.position, |t| t.position),
                ),
                Target::Point(at, _) => (ObjectId::INVALID, at),
            };
            let caught = Some(object)
                .filter(|id| *id != ObjectId::INVALID)
                .into_iter()
                .collect();
            self.impact(&Cast {
                caster: who,
                row,
                target: object,
                at,
                caught,
            });
        }
        true
    }

    /// `cast`'s impact: its power's impact script run by its caster, what
    /// the caster's last power reached noted first.
    fn impact(&mut self, cast: &Cast) {
        let last = &mut self.state.last;
        last.insert((cast.caster.0, last::SPELL_TARGET), cast.target.0);
        last.insert((cast.caster.0, last::POWER_LANDED), 0);
        if let Some(script) = self.library.impact(cast.row) {
            self.run_as(script, cast.caster, Event::default(), None, Some(cast));
        }
    }

    /// Notes `id`'s attack as it begins, for `GetLastAttackAction` and its
    /// kin: a Force power, or a blow with the combat feat it uses (none
    /// for a plain one).
    fn attacking(&mut self, id: u32, talent: Option<Use>) {
        let kept = &mut self.state.last;
        let action = match talent {
            Some(Use::Power(row)) => {
                kept.insert((id, last::FORCE_POWER), u32::from(row));
                last::ATTACK_POWER
            }
            Some(Use::Feat(row)) => {
                kept.insert((id, last::COMBAT_FEAT), u32::from(row));
                last::ATTACK_OBJECT
            }
            None => {
                kept.remove(&(id, last::COMBAT_FEAT));
                last::ATTACK_OBJECT
            }
        };
        kept.insert((id, last::ATTACK_ACTION), action);
    }

    /// An attack a script asked for while `who` has an assured hit
    /// (`EffectAssuredHit`, a scene's killing blow; nwscript.nss: it
    /// "guarantees that all attacks are successful"): once the target is in
    /// its weapon's reach, it is the round's attack, as K1's executable
    /// plays an assured round: the swing of its feat (`feat`, a talent its
    /// script uses) or of its round's set, its target answering the hit,
    /// the blow landing at the swing's `hit1` (`combat::rounds`); a
    /// gunner's goes out as its shot, its first shot clip, half a round on.
    /// It lands whether or not the swing meets the body (KOTOR's round
    /// decides it; the bodies here may not close, a gunner backing off).
    /// One a round, so once the attack (begun at `started`, sim ms) has
    /// struck its target it waits for the round to end (Taris's duel:
    /// Gerlon's one shot, Duncan then lying in the dead pose his surrender
    /// plays, not shot again to his feet). Its damage is the weapon's
    /// average where KOTOR rolls it [verify the Taris gang scene with a
    /// roll].
    fn assured(
        &mut self,
        who: ObjectId,
        (thing, feat): (&Thing, Option<u16>),
        target: ObjectId,
        started: u64,
    ) {
        use crate::combat::rounds::{Answer, HALF_ROUND_MS, NONE, Side};
        let Some(character) = thing.character else {
            return;
        };
        let sheet = character.sheet;
        let victim = self.view.thing(target).and_then(|victim| victim.character);
        let struck = victim
            .and_then(|victim| victim.struck)
            .is_some_and(|(by, at)| by == who && at >= started);
        // One attack a round (`combat::rounds`).
        let resting = self
            .state
            .swung
            .get(&who.0)
            .is_some_and(|at| self.view.now_ms < at + crate::combat::ROUND_MS);
        if sheet.flags & crate::combat::sheet::flags::ASSURED_HIT == 0
            || struck
            || resting
            || self.state.strikes.iter().any(|strike| strike.by == who)
        {
            return;
        }
        if !self.in_reach(thing, target) {
            return;
        }
        let Some(victim) = victim else {
            return;
        };
        let amount =
            (twice_average(&character.held()) / 2).max(rules::scripts::answers::ASSURED_LEAST);
        let beast = |id| {
            self.view
                .facts(id)
                .is_some_and(|facts| facts.strike.is_some())
        };
        let sides = (
            Side {
                sheet: &sheet,
                creature: beast(who),
            },
            Side {
                sheet: &victim.sheet,
                creature: beast(target),
            },
        );
        let table = self.library.rounds();
        let (animation, answer, hit_ms) = if character.gunner() {
            // A pistol in each hand shoots the two pistols' clip (wield 6:
            // as reone has them).
            let class = if sheet.pistol_pair() {
                sheet.wield_number()
            } else {
                sheet.right_class()
            };
            let row = table.row(&format!("b{class}a1")).unwrap_or(0);
            let answer = table.answer(row, Answer::Damage, sides.1);
            (row, answer, HALF_ROUND_MS)
        } else {
            let rng = &mut *self.rng;
            let Some(row) = table.swing(sides, feat, |count| 1 + rng.below(u32::from(count)) as u8)
            else {
                return;
            };
            let hit_ms = table.hit_ms(row).unwrap_or(HALF_ROUND_MS);
            (row, table.answer(row, Answer::Damage, sides.1), hit_ms)
        };
        self.state.strikes.push(effects::Strike {
            target,
            by: who,
            amount: effects::amount(amount),
            due_ms: self.view.now_ms + u64::from(hit_ms),
            assured: true,
            landed: true,
            animation,
            answer: answer.unwrap_or(NONE),
            hit_ms,
            // A gun with no shot clip of its class plays none.
            began: animation == 0,
            shots: Default::default(),
        });
    }

    /// Whether `target` stands within the reach of `thing`'s weapon: a
    /// gun's range (none known: any distance), `MELEE_REACH_M` for the
    /// rest.
    fn in_reach(&self, thing: &Thing, target: ObjectId) -> bool {
        let (Some(character), Some(victim)) = (thing.character, self.view.thing(target)) else {
            return false;
        };
        let weapon = character.held();
        let reach = match weapon.reach_m {
            _ if !weapon.delivery.ranged() => MELEE_REACH_M,
            0 => f32::MAX,
            metres => f32::from(metres),
        };
        flat(thing.position, victim.position) <= reach
    }

    /// Where `id`'s mind routes it: to stand at `at` (soles, metres),
    /// stopping within `range`.
    /// `act` once `thing` (object `id`) stands within `DOOR_REACH` of
    /// `object`, its route goal meanwhile: whether the action is done (at
    /// once for a player, or with nothing there).
    fn within_reach(
        &mut self,
        id: u32,
        thing: &Thing,
        player: bool,
        object: ObjectId,
        act: impl FnOnce(&mut Self),
    ) -> bool {
        match self.view.place(Target::Object(object)) {
            None => true,
            Some((at, _)) if flat(thing.position, at) <= DOOR_REACH => {
                act(self);
                true
            }
            // One that cannot walk (a trigger, a placeable) acts from where
            // it is: the Unknown World's floor tiles open the catacombs'
            // door with an ActionOpenDoor of their own.
            Some(_) if thing.character.is_none() => {
                act(self);
                true
            }
            Some(_) if player => true,
            Some((at, _)) => {
                self.goal(id, at, DOOR_REACH, false);
                false
            }
        }
    }

    /// Runs the events waiting (`State::pending`), in order, while the
    /// step has runs left.
    fn run_pending(&mut self) {
        while self.runs < RUNS_PER_STEP && !self.state.pending.is_empty() {
            let pending = self.state.pending.remove(0);
            if self.gone(pending.caller) {
                continue;
            }
            if pending.perception != 0
                && let Some(perceived) = pending.entering
            {
                let caller = pending.caller.0;
                let last = &mut self.state.last;
                last.insert((caller, last::PERCEIVED), perceived.0);
                last.insert((caller, last::PERCEPTION), u32::from(pending.perception));
            }
            let ai = pending.ai || engine::own_ai(self.view, pending.caller, &pending.script);
            let event = Event {
                entering: pending.entering,
                number: pending.number,
                ai: ai.then_some(pending.caller),
                leads: pending.leads,
                asker: pending.asker,
                ..Event::default()
            };
            self.run(&pending.script, pending.caller, event, None);
        }
    }

    /// Whether `thing` is a party member while a conversation holds the
    /// world (`held`).
    fn held_party(&self, thing: &Thing) -> bool {
        thing.character.is_some_and(|c| c.party) && self.held()
    }

    /// Whether a conversation holds the world this step (S1, `View::held`).
    fn held(&self) -> bool {
        self.view.held
    }

    /// How near (metres) a move to `to` with `range` arrives: on a place
    /// while the world is held (KOTOR's range 0, S3), else at its range.
    fn arrival(&self, to: &Target, range: f32) -> f32 {
        match to {
            Target::Point(..) if self.held() => 0.0,
            _ => range,
        }
    }

    /// Notes a move of `who` begun at `started` as ended now, `ended` (probes
    /// only).
    #[allow(unused_variables)]
    fn moved(&mut self, thing: &Thing, started: u64, ended: Ended, to: [f32; 3]) {
        #[cfg(feature = "probes")]
        {
            let who = thing.id;
            self.report.moves.push(Moved {
                who,
                from_ms: started,
                to_ms: self.view.now_ms,
                ended,
                at: thing.position.map(|v| v / collision::UNIT),
                to: standing(to),
                held_party: self.held_party(thing),
            });
        }
    }

    fn goal(&mut self, id: u32, at: [f32; 3], range: f32, run: bool) {
        self.out.goals.insert(
            id,
            Goal {
                point: standing(at),
                range: range / collision::UNIT,
                run,
            },
        );
    }
}

/// Whether `thing` holds a gun (`Character::gunner`).
fn wields_gun(thing: &Thing) -> bool {
    thing.character.is_some_and(|character| character.gunner())
}

/// How far above and below a place its floor is looked for (metres,
/// `View::floor`): a step and more.
pub(crate) const FLOOR_SEARCH_M: f32 = 1.0;

/// Where a character's origin would be (units) to stand at `at` (soles,
/// metres): a move's goal.
pub(crate) fn standing(at: [f32; 3]) -> [f32; 3] {
    let mut point = at.map(|v| v / collision::UNIT);
    point[2] -= crate::motion::SOLE_Z;
    point
}

pub(super) use crate::math::flat_distance as flat;

/// Runs what is due at `view.now_ms`: events waiting, then deferred code
/// by when it is due and the order it was deferred in, then each object's
/// action queue (`RUNS_PER_STEP` script runs at most). What they change
/// in the world goes to `out`.
pub fn run_due(
    library: &Library,
    state: &mut State,
    view: &View,
    casts: &[Cast],
    rng: &mut SimRng,
    report: &mut Report,
    out: &mut Out,
) {
    // Before any script runs, so that only deferred code holds values.
    state.compact();
    let mut step = Step {
        library,
        view,
        state,
        rng,
        report,
        out,
        iterators: Iterators::new(),
        runs: 0,
        left_watching: Vec::new(),
    };
    let exit = step.choose_party();
    pazaak::step(&mut step);
    // The attacks that begin now: the attacker plays the animation its
    // script named and its target the answer to it (`effects::blows`); a
    // shot's target is struck as its bolt lands (`world::fly`), not before.
    let shot = |strike: &effects::Strike| view.thing(strike.by).is_some_and(wields_gun);
    // A round not yet begun goes with its attacker's or its target's death:
    // the dead act on nothing, and an attack on the dead is over.
    let dead = |who: ObjectId| view.thing(who).is_some_and(Thing::dead);
    step.state
        .strikes
        .retain(|strike| strike.began || !(dead(strike.by) || dead(strike.target)));
    // A swing a round old no longer holds its attacker back.
    step.state
        .swung
        .retain(|_, at| *at + crate::combat::ROUND_MS > view.now_ms);
    for strike in step
        .state
        .strikes
        .iter_mut()
        .filter(|strike| !strike.began && strike.starts_ms() <= view.now_ms)
    {
        strike.began = true;
        if strike.animation != 0 {
            step.state.swung.insert(strike.by.0, strike.starts_ms());
        }
        // A gunner's clip fires its bolts as the table has it.
        if shot(strike)
            && strike.animation != 0
            && let Some(shots) = library.discharge(view.appearance(strike.by), strike.animation)
        {
            strike.shots = shots;
        }
        let answer = match strike.landed {
            true if shot(strike) => None,
            true => Some(effects::blows::HURT),
            false => Some(effects::blows::GUARD),
        };
        // Both are in a fight, the blades lit.
        for who in [strike.by, strike.target] {
            step.out.changes.push(Change::InCombat { who });
            step.out.changes.push(Change::SaberPower {
                who,
                on: true,
                held: false,
            });
        }
        // The target answers with the row its round chose, if any.
        let answered = answer
            .filter(|_| strike.answer < effects::blows::ROWS)
            .map(|kind| (strike.target, kind + strike.answer));
        for (who, play) in [(strike.by, effects::blows::SWING + strike.animation)]
            .into_iter()
            .chain(answered)
        {
            step.out.changes.push(Change::Animate {
                who,
                play: (play, (actions::ROUND_MS / 2) as u32),
            });
        }
    }
    // Blows that reach their hit now join the step's effects; one its
    // attacker has landed meanwhile by its own swing (an assured blow on
    // a body it reached) has had its hit. A shot's waits on its bolt a
    // moment more (`effects::SHOT_GRACE_MS`), then lands without it, its
    // target struck then.
    let (due, waiting) = std::mem::take(&mut step.state.strikes)
        .into_iter()
        .partition::<Vec<_>, _>(|strike| {
            let grace = if strike.landed && shot(strike) {
                effects::SHOT_GRACE_MS
            } else {
                0
            };
            strike.due_ms + grace <= view.now_ms
        });
    step.state.strikes = waiting;
    for strike in due.iter().filter(|strike| strike.landed && shot(strike)) {
        step.out.changes.push(Change::Struck {
            who: strike.target,
            row: strike.answer,
        });
    }
    let due: Vec<_> = due
        .into_iter()
        .filter(|strike| {
            if !strike.assured {
                return strike.landed;
            }
            let decided = strike.starts_ms();
            view.thing(strike.target)
                .and_then(|victim| victim.character)
                .and_then(|victim| victim.struck)
                .is_none_or(|(by, at)| by != strike.by || at < decided)
        })
        .collect();
    step.state
        .applied
        .extend(due.iter().map(effects::Strike::applied));
    // The step's casts first: their impacts (C10), each its power's impact
    // script run by the caster.
    for cast in casts {
        step.impact(cast);
    }
    step.run_pending();
    while step.runs < RUNS_PER_STEP {
        let Some(due) = step.state.next_due(view.now_ms) else {
            break;
        };
        if step.gone(due.caller) {
            continue;
        }
        let event = Event {
            talk: due.talk,
            ..Event::default()
        };
        step.run_as(
            &due.script,
            due.caller,
            event,
            Some(&due.deferred),
            due.spell.as_ref(),
        );
    }
    step.advance();
    dialogue::advance(&mut step);
    pazaak::leaving_module(&mut step);
    pazaak::engaged_stop_watching(&mut step);
    step.levels();
    step.state.dress_changed(library.items(), view, step.out);
    // A module a script started: the party leaves it as this step ends,
    // so its leaving's events run now, in the module they belong to.
    if step.out.new_module.is_some() {
        let events = leaving(view, &step.state.party);
        step.state.pending.extend(events);
        step.run_pending();
    }
    // The screen's script runs the next step, when those made stand.
    state.pending.extend(exit);
    // A planet picked on the galaxy map: the travel script asks for it.
    for &(player, planet) in view.travels {
        if state.galaxy.travels_to(planet) {
            state.galaxy.selected = Some(planet);
            state.pending.push(Pending::new(TRAVEL_SCRIPT, player));
        }
    }
}

/// The events of the party leaving the module, as KOTOR's leaving runs
/// them (as xoreos runs it): for each PC the area's `OnExit`,
/// then the module's `Mod_OnClientLeav`, the PC their exiting object
/// (`GetExitingObject`).
pub(crate) fn leaving(view: &View, party: &party::Party) -> Vec<Pending> {
    let pcs = view.pcs(party);
    [AREA, MODULE]
        .into_iter()
        .filter_map(|holder| Some((holder, view.hook(holder, Hook::Exit)?)))
        .flat_map(|(holder, script)| {
            pcs.iter().map(move |pc| Pending {
                entering: Some(*pc),
                ..Pending::new(&script, holder)
            })
        })
        .collect()
}

/// Where a companion made for the party stands (metres): `side` metres
/// to the left of one at `at` facing `yaw` (right for a
/// negative `side`).
pub(super) fn beside(at: [f32; 3], yaw: f32, side: f32) -> [f32; 3] {
    let (sin, cos) = crate::math::sin_cos_deg(yaw);
    [at[0] - sin * side, at[1] + cos * side, at[2]]
}

/// A creature of `template` made at `at` facing `yaw`, stocked with its
/// items: in the world from the end of the step (its spawn script runs
/// the next), its id the step's next.
pub(crate) fn make(
    library: &Library,
    view: &View,
    state: &mut State,
    out: &mut Out,
    template: &str,
    at: [f32; 3],
    yaw: f32,
) -> Option<ObjectId> {
    // A placeable stands where it is made (a creature's place is its body's).
    let facts = match library.creature(template) {
        Some(facts) => facts.clone(),
        None => {
            let mut facts = (**library.placeable(template)?).clone();
            (facts.position, facts.yaw) = (at, yaw);
            Arc::new(facts)
        }
    };
    Some(make_of(library, view, state, out, facts, at, yaw))
}

/// An object of `facts` made at `at` facing `yaw` (`make`).
fn make_of(
    library: &Library,
    view: &View,
    state: &mut State,
    out: &mut Out,
    facts: Arc<ObjectData>,
    at: [f32; 3],
    yaw: f32,
) -> ObjectId {
    let id = ObjectId(view.next_id + out.made.len() as u32);
    let creature = facts.kind == crate::module::ObjectKind::Creature;
    state.stock(library, id.0, &facts.inventory, creature);
    out.made.push(Made {
        id,
        facts,
        at,
        yaw,
        kept: None,
    });
    id
}

/// Companion `npc` made at `at` facing `yaw` (`SpawnAvailableNPC`, the
/// party selection's pick): as the party table kept it, when it did (its
/// items, career and XP moved on from the creature it was, its sheet and
/// points as they stood), else as its template.
fn make_npc(
    library: &Library,
    view: &View,
    state: &mut State,
    out: &mut Out,
    npc: u8,
    at: [f32; 3],
    yaw: f32,
) -> Option<ObjectId> {
    let slot = state.party.available.get(&npc)?.clone();
    // Of a template this module lacks, the one the table kept.
    let id = match library.creature(&slot.template) {
        Some(_) => make(library, view, state, out, &slot.template, at, yaw)?,
        None => {
            let facts = crate::pack::creature_of(
                slot.template_data.as_deref()?,
                crate::wire::Versions::THIS_BUILD,
            )
            .ok()?;
            make_of(library, view, state, out, Arc::new(facts), at, yaw)
        }
    };
    if let Some(kept) = slot.kept {
        // Its own items, not its template's again.
        for item in state.items.ids_held_by(id.0) {
            state.items.remove(&item);
        }
        for item in state.items.ids_held_by(kept.from) {
            if let Some(mut item) = state.items.get_mut(&item) {
                item.holder = id.0;
            }
        }
        if let Some(career) = state.careers.remove(&kept.from) {
            state.careers.insert(id.0, career);
        }
        if let Some(xp) = state.xp.remove(&kept.from) {
            state.xp.insert(id.0, xp);
        }
        if let Some(hoard) = state.hoards.remove(&kept.from) {
            state.hoards.insert(id.0, hoard);
        }
        if let Some(style) = state.party.ai_styles.remove(&kept.from) {
            state.party.ai_styles.insert(id.0, style);
        }
        if let Some(made) = out.made.last_mut() {
            made.kept = Some(kept);
        }
    }
    if let Some(slot) = state.party.available.get_mut(&npc) {
        slot.object = Some(id.0);
    }
    // TSL: made from the table, it takes its influence's alignment.
    influence::follow(library, view, state, out, id);
    Some(id)
}

/// Where `who` stands (metres): in the world, or where it was made this
/// step.
fn standing_at(view: &View, out: &Out, who: ObjectId) -> Option<([f32; 3], f32)> {
    match view.thing(who) {
        Some(thing) => Some((thing.position, thing.yaw)),
        None => out
            .made
            .iter()
            .find(|made| made.id == who)
            .map(|made| (made.at, made.yaw)),
    }
}

/// TSL: companion `npc` joined the party on `owner`, so the puppet given
/// to it (`AssignPUP`) comes along: its creature here moved beside it,
/// else made there, and joined (two puppets at most), as the executable's
/// `AddMember` spawns and adds it.
fn puppet_joins(
    library: &Library,
    view: &View,
    state: &mut State,
    out: &mut Out,
    npc: u8,
    owner: ObjectId,
) {
    let Some((pup, puppet)) = state
        .party
        .puppets
        .iter()
        .find(|(_, puppet)| puppet.owner == Some(npc) && !puppet.joined)
        .map(|(pup, puppet)| (*pup, puppet.clone()))
    else {
        return;
    };
    if state.party.puppet_creatures().count() >= party::PUPPETS_JOINED {
        return;
    }
    let Some((at, yaw)) = standing_at(view, out, owner) else {
        return;
    };
    let here = puppet
        .object
        .map(ObjectId)
        .filter(|who| view.thing(*who).is_some());
    let who = match here {
        Some(who) => {
            out.changes.push(Change::Jump { who, to: at, yaw });
            Some(who)
        }
        None => make(library, view, state, out, &puppet.template, at, yaw),
    };
    if let (Some(who), Some(slot)) = (who, state.party.puppets.get_mut(&pup)) {
        slot.object = Some(who.0);
        slot.joined = true;
        out.changes.push(Change::Join { who });
    }
}

/// TSL: companion `npc` left the party, so the puppet with it leaves too,
/// its creature fading away (the executable's `RemoveMember`).
fn puppet_leaves(view: &View, state: &mut State, out: &mut Out, npc: u8) {
    for puppet in state.party.puppets.values_mut() {
        if puppet.owner != Some(npc) || !puppet.joined {
            continue;
        }
        puppet.joined = false;
        if let Some(who) = puppet.object.take().map(ObjectId)
            && let Some(thing) = view.thing(who)
        {
            out.changes.push(Change::Leave { who });
            out.changes.push(Change::fade_away(who, thing.kind));
        }
    }
}

/// Whether `who` is in the party: a player's character or a companion
/// that joined.
pub(crate) fn in_party(view: &View, state: &State, who: ObjectId) -> bool {
    view.party.contains(&who) || state.party.creatures().any(|id| id == who.0)
}

/// TSL's forfeit condition `broken` (`party::forfeit`), if in force:
/// kept as the last broken (`GetLastForfeitViolation`) and told to the
/// area's user-defined script (event `forfeit::EVENT`), the act going on.
pub(crate) fn forfeit(view: &View, state: &mut State, broken: u32) {
    if state.party.forfeit & broken == 0 {
        return;
    }
    state.party.violation = broken;
    if let Some(script) = view.hook(AREA, Hook::UserDefined) {
        let mut pending = Pending::new(&script, AREA);
        pending.number = party::forfeit::EVENT;
        state.pending.push(pending);
    }
}

/// `who`'s weapon sets swapped (`State::switch_weapons`), each weapon
/// equipped into its new hand as TSL's executable equips it on a swap, so
/// a duel's rules see it (`forfeit_equip`).
pub(crate) fn switch_weapons(
    library: &Library,
    view: &View,
    state: &mut State,
    out: &mut Out,
    who: ObjectId,
) {
    for moved in state.switch_weapons(out, who.0) {
        forfeit_equip(library, view, state, who, moved);
    }
}

/// TSL's forfeit conditions a party member `who` equipping `item` into
/// `slot` breaks (`party::forfeit::equipping`).
pub(crate) fn forfeit_equip(
    library: &Library,
    view: &View,
    state: &mut State,
    who: ObjectId,
    (item, slot): (u32, u8),
) {
    if state.party.forfeit == 0 || !in_party(view, state, who) {
        return;
    }
    let broken = state
        .items
        .get(&item)
        .and_then(|held| state.item_facts(library.items(), &held.template))
        .and_then(|facts| {
            let delivery = facts.weapon.map(|weapon| weapon.delivery);
            party::forfeit::equipping(state.party.forfeit, slot, &facts.tag, delivery)
        });
    if let Some(broken) = broken {
        forfeit(view, state, broken);
    }
}

/// What the party table keeps of companion `npc` as its creature stands
/// now (`SaveNPCState`, a member leaving the party), if it stands here.
fn keep_npc(view: &View, state: &mut State, out: &Out, npc: u8) {
    if let Some(who) = state.party.available.get(&npc).and_then(|slot| slot.object) {
        keep_creature(view, state, out, npc, ObjectId(who));
    }
}

/// What the party table keeps of companion `npc` from creature `who` as it
/// stands now (`keep_npc`; TSL's `SaveNPCByObject`, any creature), if it
/// stands here and `npc` is available.
fn keep_creature(view: &View, state: &mut State, out: &Out, npc: u8, who: ObjectId) {
    let Some(slot) = state.party.available.get_mut(&npc) else {
        return;
    };
    let Some(thing) = view.thing(who) else {
        return;
    };
    let Some(c) = thing.character else {
        return;
    };
    slot.kept = Some(party::Kept {
        from: thing.id.0,
        sheet: out.sheet(thing.id, c.worn),
        hp: thing
            .hp
            .map_or(HUNDREDTHS, |(now, _)| now.max(1) * HUNDREDTHS),
        fp: c.fp,
    });
}

/// Counts a run's instructions, its fault or its stop at the limit; the
/// int it returned.
fn finish(
    report: &mut Report,
    script: &str,
    outcome: Result<vm::Finished, vm::Fault>,
) -> Option<i32> {
    match outcome {
        Ok(finished) => {
            report.instructions += finished.instructions;
            finished.returned
        }
        Err(fault @ vm::Fault::Budget { .. }) => {
            report.runaways.push(format!("{script}: {fault}"));
            None
        }
        // A stop the executables make too (`minigame::aborts`): no fault.
        Err(vm::Fault::Host { error, .. }) if error == minigame::ABORTED => None,
        Err(fault) => {
            report.faults.push(format!("{script}: {fault}"));
            None
        }
    }
}

/// An object's name in a trace.
fn name(id: ObjectId) -> String {
    match id {
        MODULE => "the module".into(),
        AREA => "the area".into(),
        ObjectId::INVALID => "nothing".into(),
        ObjectId(id) => format!("#{id}"),
    }
}
