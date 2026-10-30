//! What a module gives the sim: its area's placements (the GIT's
//! instances), in the module's order, the templates they are made from, and
//! where it is entered. A placement is its own facts (a door, a placeable,
//! a trigger: the GIT describes each whole), or a creature made as its
//! template is, standing where the GIT puts it (`Placement`); the area's
//! instances begin with them (`crate::instance`).
//!
//! Like the collision, this is static data, not in snapshots: every
//! machine builds the same from its own install, and the install
//! announcement (`Announcement`) names the digests that must match. A
//! snapshot's objects refer to it by number and carry only what changes
//! (a door's state, who stands in a trigger).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use collision::nav::NavMesh;

use crate::combat::Sheet;
use crate::wire::{DecodeError, Writer};

/// What an object is: the GIT's lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    Creature,
    Door,
    Placeable,
    Trigger,
    Waypoint,
    Sound,
    Encounter,
    Store,
}

impl ObjectKind {
    pub const ALL: [ObjectKind; 8] = [
        ObjectKind::Creature,
        ObjectKind::Door,
        ObjectKind::Placeable,
        ObjectKind::Trigger,
        ObjectKind::Waypoint,
        ObjectKind::Sound,
        ObjectKind::Encounter,
        ObjectKind::Store,
    ];

    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn from_index(index: u8) -> Option<ObjectKind> {
        ObjectKind::ALL.get(usize::from(index)).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            ObjectKind::Creature => "creature",
            ObjectKind::Door => "door",
            ObjectKind::Placeable => "placeable",
            ObjectKind::Trigger => "trigger",
            ObjectKind::Waypoint => "waypoint",
            ObjectKind::Sound => "sound",
            ObjectKind::Encounter => "encounter",
            ObjectKind::Store => "store",
        }
    }
}

/// Where a door or trigger leads (`LinkedToModule`, `LinkedTo`), both
/// lowercase. An empty module leads nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub module: String,
    /// The tag of the door or waypoint to arrive at; empty for the
    /// module's own entry.
    pub tag: String,
    /// `TransitionDestin`: the text shown for the destination (a strref).
    pub destination: Option<u32>,
}

/// Whether a door can be opened by a player; for a placeable, only whether
/// it is placed locked (`Locked`, else `Never`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    Yes,
    /// `Locked`: opening fails (the fail-to-open script's case).
    Locked,
    /// `Static`: part of the scenery; never selectable.
    Never,
}

/// Which ways a door opens (QUIRKS.md Q-0025): its first way only, or
/// both, its first (`opening1`, `<model>1.dwk`) swinging to one side of its
/// doorway and its second (`opening2`, `<model>2.dwk`) to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ways {
    #[default]
    One,
    /// Both, the first to its front: its own +Y, its facing turned a
    /// quarter left.
    FirstToFront,
    /// Both, the first to its back.
    FirstToBack,
}

/// A door's or placeable's lock beyond whether it is locked (`Opens`,
/// `snapshot::ObjectState::lock`): what unlocks it and what locks it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lock {
    /// `Lockable`: a creature may lock it again.
    pub lockable: bool,
    /// Security's difficulty to unlock it (`OpenLockDC`) and to lock it
    /// (`CloseLockDC`).
    pub open_dc: u8,
    pub close_dc: u8,
    /// The tag of the item that unlocks it, lowercase; empty for none.
    pub key: String,
    /// Only the key unlocks it: Security cannot.
    pub key_required: bool,
    /// The key is used up unlocking it.
    pub auto_remove_key: bool,
}

impl Lock {
    fn digest_into(&self, out: &mut Writer) {
        out.u8(u8::from(self.lockable));
        out.u8(self.open_dc);
        out.u8(self.close_dc);
        out.string(&self.key);
        out.u8(u8::from(self.key_required));
        out.u8(u8::from(self.auto_remove_key));
    }
}

/// What stands between a blow and a door's or placeable's hit points
/// (`Hardness`, `Plot`, `Min1HP`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Toughness {
    /// Whole points taken off every blow.
    pub hardness: u8,
    /// Nothing hurts it.
    pub plot: bool,
    /// Hurt down to 1 hit point and no further: it never breaks.
    pub min_one_hp: bool,
}

/// Whether blows may break `facts` open (KOTOR's Bash), `locked` and with
/// `hit_points` now: a door that is not scenery or a placeable, locked, not
/// `Plot`, with hit points left.
pub fn bashable(facts: &ObjectData, locked: bool, hit_points: Option<(i32, i32)>) -> bool {
    let scenery = facts.kind == ObjectKind::Door && facts.opens == Opens::Never;
    matches!(facts.kind, ObjectKind::Door | ObjectKind::Placeable)
        && !scenery
        && locked
        && !facts.toughness.plot
        && hit_points.is_some_and(|(now, _)| now > 0)
}

/// One object of the module, as the sim knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectData {
    pub kind: ObjectKind,
    /// Lowercase (tags are looked up without case).
    pub tag: String,
    /// The tag as its data writes it, when that has capitals; empty
    /// otherwise. `GetTag` answers with it for a trigger only, whose tag
    /// the games keep as written; the rest load theirs lowercase.
    pub cased_tag: String,
    pub template: String,
    /// Game space, metres.
    pub position: [f32; 3],
    /// Facing, degrees from +X.
    pub yaw: f32,
    /// A door's or trigger's destination.
    pub link: Option<Link>,
    /// A trigger's or encounter's outline, relative to `position`; a
    /// placeable template's collision footprint, the cells along the edge
    /// of what its walkmesh covers, each its least and greatest corners in
    /// its own frame, in pairs (what one made in play collides as,
    /// `world::move_actors`).
    pub geometry: Vec<[f32; 3]>,
    /// A placeable's action points: its walkmesh's two use positions
    /// (`<model>.pwk`), in its own frame (metres), where a character stands
    /// to use it; none for one whose first is the origin, as TSL's
    /// `CSWSPlaceable::GetNearestActionPoint` then takes the placeable's
    /// own position (`nearest_use_point`).
    pub use_points: Vec<[f32; 3]>,
    /// Doors: whether a player can open it.
    pub opens: Opens,
    /// A door's state at load (`AnimationState`: 0 closed, 1 and 2 open);
    /// 0 for the rest.
    pub state: u8,
    /// Which ways a door opens (`opening`); `One` for the rest.
    pub ways: Ways,
    /// Its event scripts (hook, script resref), from the template and the
    /// instance: recorded for the script VM (I16), not run.
    pub scripts: Vec<(String, String)>,
    /// `Conversation`: the DLG it speaks, lowercase; empty for none.
    pub conversation: String,
    /// A creature's character sheet (from its UTC, UTIs and the 2DAs), if
    /// it could be read; a creature without one fights as `Sheet::commoner`.
    pub sheet: Option<Sheet>,
    /// A creature's senses (its template's `PerceptionRange` row of
    /// `ranges.2da`), if they could be read; a creature without them
    /// perceives nothing (I17's perception, `world::scripts`).
    pub senses: Option<Senses>,
    /// A creature's classes (`classes.2da` rows) and levels in them, in
    /// its template's order (`GetLevelByClass`).
    pub classes: Vec<(u8, u8)>,
    /// A creature's `Gender` and `SubraceIndex` (`GetGender`, `GetSubRace`).
    pub gender: u8,
    pub subrace: u8,
    /// A creature's personal space (metres): its appearance's `perspace`
    /// in `appearance.2da` (`GetObjectPersonalSpace`); 0 for other kinds.
    pub personal_space: f32,
    /// How a creature without a humanoid swing attacks (`Strike`); `None`
    /// for one whose model JA's swings can drive, and for other kinds.
    pub strike: Option<Strike>,
    /// A creature's `ChallengeRating`: the XP its death gives the party
    /// (`script::xp`); 0 for other kinds.
    pub challenge: f32,
    /// A TSL creature's `MultiplierSet`: its `autobalance.2da` row (0 none),
    /// which its death's XP rates by (`CRMod`) instead of `challenge`.
    pub multiplier_set: u8,
    /// Its name as the game shows it (`GetName`): a creature's first and
    /// last names, another's localized name; empty for none.
    pub name: String,
    /// The items a creature or placeable starts with: its equipment and
    /// its inventory (I19).
    pub inventory: Vec<crate::inventory::Held>,
    /// A placeable players open (`HasInventory`): a container, holding
    /// `inventory`'s items and what scripts give it (`State::loot`).
    pub container: bool,
    /// A placeable players may use (`Useable`).
    pub useable: bool,
    /// A store's terms (its UTM); its stock is `inventory`.
    pub store: Option<crate::inventory::Terms>,
    /// A door's or placeable's lock, if its template gives it one (any
    /// field set).
    pub lock: Option<Lock>,
    /// A door's or placeable's hit points, current and most (`CurrentHP`,
    /// `HP`), as scripts read them (`GetMaxHitPoints`: Manaan's Bandon
    /// talker divides by its own); a creature template's (`CurrentHitPoints`,
    /// `HitPoints`), which its fighter starts as short of full as
    /// (`combat::Fighter::hurt_as`);
    /// `None` where the data has none.
    pub hit_points: Option<(i32, i32)>,
    /// What a door's or placeable's hit points stand against
    /// (`world::objects`); none for the rest.
    pub toughness: Toughness,
    /// A door's or placeable's `Faction` (a `repute.2da` row: Tatooine's
    /// Sand People are a placeable's whose reputation scripts adjust); 0
    /// for creatures (their sheet's) and where the data has none.
    pub faction: u8,
    /// A creature's body as `appearance.2da` has it, if its template names
    /// a row: what its AI asks of it (`GetAppearanceType`,
    /// `GetCreatureSize`, `GetCreatureMovmentType`).
    pub body: Option<Body>,
    /// A trigger's, door's or placeable's trap (`TrapFlag` set; a mine
    /// laid in play), if it is one (`crate::traps`).
    pub trap: Option<crate::traps::Trap>,
    /// A waypoint's note on the area's map (`HasMapNote`), if it has one.
    pub map_note: Option<MapNote>,
    /// A creature's `Interruptable` (true where its template does not
    /// say): a hurt listener's conversation with it is cut short
    /// (`script::dialogue`, R7). The games' scripted captives are not.
    pub interruptable: bool,
    /// A sound object's `PitchVariation` (its UTS;
    /// `SoundObjectGetPitchVariance`); 0 for the rest.
    pub pitch_variation: f32,
    /// An encounter's UTE (`crate::spawner`): what it spawns and when.
    pub encounter: Option<Box<crate::spawner::Encounter>>,
}

/// A waypoint's note on the area's map: its words as the game shows them,
/// and whether it shows as placed (`MapNoteEnabled`; `SetMapPinEnabled`
/// changes that, `ObjectState::state`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapNote {
    pub text: String,
    pub enabled: bool,
}

/// A creature's `appearance.2da` row and what the game reads off it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub appearance: u16,
    /// `sizecategory`: a `CREATURE_SIZE_*`.
    pub size: u8,
    /// Its `moverate`'s row in `creaturespeed.2da`: a `MOVEMENT_SPEED_*`.
    pub movement: u8,
    /// How fast it walks and runs (m/s): `walkrate` and `runrate` of the
    /// `creaturespeed.2da` row its template's `WalkRate` names, `movement`'s
    /// where that is 7 or absent.
    pub walk_mps: f32,
    pub run_mps: f32,
    /// How far its head turns from its body before the body turns too
    /// (degrees, `head_arc_h`; 0 where blank).
    pub head_arc_h: f32,
    /// What its death leaves (`world::remains`): the `bodybag.2da` row its
    /// remains are, how long after it the engine takes the body away
    /// (`destroyobjectdelay`, ms) and how long the body fades as it goes
    /// (`fadedelayondeath`, ms; `None` where blank: the engine's own).
    pub bag: u8,
    pub destroy_ms: u32,
    pub fade_ms: Option<u32>,
}

impl Body {
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u16(self.appearance);
        out.u8(self.size);
        out.u8(self.movement);
        out.f32(self.walk_mps);
        out.f32(self.run_mps);
        out.f32(self.head_arc_h);
        out.u8(self.bag);
        out.u32(self.destroy_ms);
        out.u32(self.fade_ms.unwrap_or(u32::MAX));
    }

    pub(crate) fn decode(input: &mut crate::wire::Reader) -> Result<Body, DecodeError> {
        Ok(Body {
            appearance: input.u16()?,
            size: input.u8()?,
            movement: input.u8()?,
            walk_mps: input.f32()?,
            run_mps: input.f32()?,
            head_arc_h: input.f32()?,
            bag: input.u8()?,
            destroy_ms: input.u32()?,
            fade_ms: Some(input.u32()?).filter(|ms| *ms != u32::MAX),
        })
    }
}

/// How far a creature sees and hears (metres): `ranges.2da`'s
/// `primaryrange` and `secondaryrange`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Senses {
    pub sight: f32,
    pub hearing: f32,
}

/// KOTOR's own attack, for a creature whose model has no humanoid skeleton
/// (`appearance.2da` `modeltype` S or L: beasts, droids and aliens with
/// skeletons of their own, which JA's swings cannot drive): an attack a
/// round with its creature weapon, landing at its attack animation's hit
/// moment on a target within its reach (`world::strikes`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strike {
    /// Its creature weapon (claws, bite or gore: `CWeapon_R`, `CWeapon_L`,
    /// `CWeapon_B`, the first it wears); none fights with its bare body
    /// (`Weapon::UNARMED`).
    pub weapon: Option<crate::combat::Weapon>,
    /// How far past its personal space its blows land (metres; the
    /// appearance's `hitdist`).
    pub hit_distance: f32,
    /// When in its attack the blow lands (ms): its model's melee attack
    /// animation's `hit` event (`m0a1`, else `g0a1`; half the animation
    /// without one, half the round without either).
    pub hit_ms: u16,
}

impl Strike {
    /// What an absent strike is written as.
    pub(crate) const BLANK: Strike = Strike {
        weapon: None,
        hit_distance: 0.0,
        hit_ms: 0,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        use crate::combat::{Weapon, sheet::encode_weapon};
        crate::wire::write_fixed(out, self.weapon.as_ref(), &Weapon::UNARMED, encode_weapon);
        out.f32(self.hit_distance);
        out.u16(self.hit_ms);
    }

    pub(crate) fn decode(input: &mut crate::wire::Reader) -> Result<Strike, DecodeError> {
        use crate::combat::{Weapon, sheet::decode_weapon};
        Ok(Strike {
            weapon: crate::wire::read_fixed(input, &Weapon::UNARMED, decode_weapon, "strike flag")?,
            hit_distance: input.f32()?,
            hit_ms: input.u16()?,
        })
    }
}

impl ObjectData {
    /// An object with nothing to it but its kind, tag and place: what the
    /// probes build their modules from.
    #[cfg(feature = "probes")]
    pub(crate) fn bare(kind: ObjectKind, tag: &str, position: [f32; 3]) -> ObjectData {
        ObjectData {
            kind,
            tag: tag.into(),
            ..ObjectData::unknown("", position, 0.0)
        }
    }

    /// Its facts in one fixed encoding, for a digest.
    /// The action point (`use_points`) nearest `from` (metres), in the
    /// world: as TSL's `CSWSPlaceable::GetNearestActionPoint` picks it for
    /// a use (`CSWSCreature::GetUseRange`), the second only when it is not
    /// the origin; `None` without any (the placeable's position is used).
    pub fn nearest_use_point(&self, from: [f32; 3]) -> Option<[f32; 3]> {
        // `yaw` is in degrees, turned as the collision's placements are.
        let (sin, cos) = crate::math::sin_cos_deg(self.yaw);
        let world = |p: &[f32; 3]| {
            [
                self.position[0] + p[0] * cos - p[1] * sin,
                self.position[1] + p[0] * sin + p[1] * cos,
                self.position[2] + p[2],
            ]
        };
        let far = |p: &[f32; 3]| {
            (p[0] - from[0]).powi(2) + (p[1] - from[1]).powi(2) + (p[2] - from[2]).powi(2)
        };
        self.use_points
            .iter()
            .map(world)
            .min_by(|a, b| far(a).total_cmp(&far(b)))
    }

    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u8(self.kind.index());
        out.string(&self.tag);
        out.string(&self.template);
        for value in self.position {
            out.f32(value);
        }
        out.f32(self.yaw);
        match &self.link {
            Some(link) => {
                out.u8(1);
                out.string(&link.module);
                out.string(&link.tag);
                out.u32(link.destination.unwrap_or(u32::MAX));
            }
            None => out.u8(0),
        }
        out.u32(self.geometry.len() as u32);
        for point in &self.geometry {
            for value in point {
                out.f32(*value);
            }
        }
        // Only a placeable with action points names them, so the rest keep
        // their digests.
        if !self.use_points.is_empty() {
            out.string("use points");
            out.u32(self.use_points.len() as u32);
            for value in self.use_points.iter().flatten() {
                out.f32(*value);
            }
        }
        out.u8(match self.opens {
            Opens::Yes => 0,
            Opens::Locked => 1,
            Opens::Never => 2,
        });
        out.u8(self.state);
        out.u32(self.scripts.len() as u32);
        for (hook, script) in &self.scripts {
            out.string(hook);
            out.string(script);
        }
        match &self.sheet {
            Some(sheet) => {
                out.u8(1);
                sheet.encode(out);
            }
            None => out.u8(0),
        }
        match &self.senses {
            Some(senses) => {
                out.u8(1);
                out.f32(senses.sight);
                out.f32(senses.hearing);
            }
            None => out.u8(0),
        }
        out.u32(self.classes.len() as u32);
        for (class, levels) in &self.classes {
            out.u8(*class);
            out.u8(*levels);
        }
        out.u8(self.gender);
        out.u8(self.subrace);
        out.string(&self.conversation);
        if !self.cased_tag.is_empty() {
            out.string("cased");
            out.string(&self.cased_tag);
        }
        // Only an object with a name, a personal space, a strike, items or
        // terms lists them, so a module without keeps its digest.
        if !self.name.is_empty() {
            out.string("name");
            out.string(&self.name);
        }
        if self.personal_space != 0.0 {
            out.string("space");
            out.f32(self.personal_space);
        }
        if let Some(strike) = &self.strike {
            out.string("strike");
            strike.encode(out);
        }
        if self.challenge != 0.0 {
            out.string("challenge");
            out.f32(self.challenge);
        }
        if self.multiplier_set != 0 {
            out.string("balance");
            out.u8(self.multiplier_set);
        }
        if !self.inventory.is_empty() {
            out.string("items");
            out.u32(self.inventory.len() as u32);
            for held in &self.inventory {
                out.string(&held.template);
                out.u16(held.stack);
                out.u8(held.slot.unwrap_or(u8::MAX));
                out.u8(held.flags());
            }
        }
        if self.container {
            out.string("container");
        }
        if self.useable {
            out.string("useable");
        }
        if let Some(terms) = &self.store {
            out.string("store");
            terms.encode(out);
        }
        if let Some(lock) = &self.lock {
            out.string("lock");
            lock.digest_into(out);
        }
        if self.ways != Ways::One {
            out.string("ways");
            out.u8(u8::from(self.ways == Ways::FirstToFront));
        }
        if self.faction != 0 {
            out.string("faction");
            out.u8(self.faction);
        }
        if self.toughness != Toughness::default() {
            let Toughness {
                hardness,
                plot,
                min_one_hp,
            } = self.toughness;
            out.string("toughness");
            out.u8(hardness);
            out.u8(u8::from(plot));
            out.u8(u8::from(min_one_hp));
        }
        if let Some(body) = &self.body {
            out.string("body");
            body.encode(out);
        }
        if let Some(note) = &self.map_note {
            out.string("map note");
            out.string(&note.text);
            out.u8(u8::from(note.enabled));
        }
        if let Some(trap) = &self.trap {
            out.string("trap");
            out.u8(trap.row);
            out.u8(u8::from(trap.detectable)
                | u8::from(trap.disarmable) << 1
                | u8::from(trap.flagged) << 2
                | u8::from(trap.one_shot) << 3
                | u8::from(trap.auto_remove_key) << 4);
            out.u8(trap.detect_dc);
            out.u8(trap.disarm_dc);
            out.string(&trap.key);
        }
        if !self.interruptable {
            out.string("uninterruptable");
        }
        if self.pitch_variation != 0.0 {
            out.string("pitch variation");
            out.f32(self.pitch_variation);
        }
        if let Some(encounter) = &self.encounter {
            out.string("encounter");
            encounter.digest_into(out);
        }
    }
}

/// A tag as its data writes it (`text`), when that has capitals; empty
/// otherwise (`ObjectData::cased_tag`).
pub fn cased(text: &str) -> String {
    let text = text.trim();
    if text.bytes().any(|byte| byte.is_ascii_uppercase()) {
        text.to_string()
    } else {
        String::new()
    }
}

/// A tag as `GetTag` gives it: its cased form, if it has one.
pub(crate) fn written<'a>(tag: &'a str, cased: &'a str) -> &'a str {
    if cased.is_empty() { tag } else { cased }
}

impl ObjectData {
    /// Its tag as its data writes it (a trigger's `GetTag`).
    pub fn written_tag(&self) -> &str {
        written(&self.tag, &self.cased_tag)
    }

    /// An object made from `template` (facts with no place) standing at
    /// `position` facing `yaw`.
    pub fn made_as(template: &ObjectData, position: [f32; 3], yaw: f32) -> ObjectData {
        ObjectData {
            position,
            yaw,
            ..template.clone()
        }
    }

    /// A creature of `template` the level has no facts for, standing at
    /// `position` facing `yaw`: no tag, hooks or sheet (it fights as a
    /// commoner).
    pub fn unknown(template: &str, position: [f32; 3], yaw: f32) -> ObjectData {
        ObjectData {
            kind: ObjectKind::Creature,
            tag: String::new(),
            cased_tag: String::new(),
            template: template.to_string(),
            position,
            yaw,
            link: None,
            geometry: Vec::new(),
            use_points: Vec::new(),
            opens: Opens::Never,
            state: 0,
            ways: Ways::One,
            scripts: Vec::new(),
            conversation: String::new(),
            sheet: None,
            senses: None,
            classes: Vec::new(),
            gender: 0,
            subrace: 0,
            personal_space: 0.0,
            strike: None,
            challenge: 0.0,
            multiplier_set: 0,
            name: String::new(),
            inventory: Vec::new(),
            container: false,
            useable: false,
            store: None,
            lock: None,
            hit_points: None,
            toughness: Toughness::default(),
            faction: 0,
            body: None,
            map_note: None,
            trap: None,
            interruptable: true,
            pitch_variation: 0.0,
            encounter: None,
        }
    }

    /// A creature tagged `tag`, of `template`, at `position` with `sheet`,
    /// and nothing else to it: the probes' creatures.
    pub fn creature(tag: &str, template: &str, position: [f32; 3], sheet: Sheet) -> ObjectData {
        ObjectData {
            tag: tag.to_string(),
            sheet: Some(sheet),
            ..ObjectData::unknown(template, position, 0.0)
        }
    }

    /// The state this door opens to for an opener standing at `by` (metres):
    /// away from it (QUIRKS.md Q-0025, fixed). A door that opens both ways
    /// takes its second when the opener stands on the side its first swings
    /// to, else its first; one that opens one way, or opened by no one (a
    /// script's own), takes its first.
    pub fn opening(&self, by: Option<[f32; 3]>) -> u8 {
        let front = match self.ways {
            Ways::One => return 1,
            Ways::FirstToFront => true,
            Ways::FirstToBack => false,
        };
        let Some([x, y, _]) = by else {
            return 1;
        };
        let (sin, cos) = crate::math::sin_cos_deg(self.yaw);
        let ahead = (y - self.position[1]) * cos - (x - self.position[0]) * sin;
        if (ahead > 0.0) == front { 2 } else { 1 }
    }

    /// Where this door or trigger leads, if it names a module. Its own
    /// module counts: reone installs a module again when a link names it,
    /// the objects as they are, the party arriving at the tag.
    pub fn exit(&self) -> Option<&Link> {
        self.link.as_ref().filter(|link| !link.module.is_empty())
    }

    /// Whether a character at `body` (metres: its movement box's origin, as
    /// snapshots and scripts have it, 0.61 m over its soles) is in this
    /// trigger (QUIRKS.md Q-0023 and Q-0024, fixed): over its outline
    /// (`inside_outline`), and on the floor the trigger lies on there. Of
    /// the walkable floors over that spot (`nav`), the trigger's is the one
    /// nearest its points' heights, the character's the highest at or under
    /// its soles (a step above them for a walkmesh over the drawn floor); a
    /// floor alone over the spot is both, however far above it the outline
    /// was drawn (the Dantooine ruins' star map) or the ground dips under it
    /// (Tatooine's dunes). Off the mesh it is the prism of the points'
    /// heights, `TRIGGER_REACH_M` under and over.
    pub fn contains(&self, body: [f32; 3], nav: Option<&NavMesh>) -> bool {
        let [x, y, z] = body;
        let z = z + crate::motion::SOLE_Z * collision::UNIT;
        if !inside_outline(&self.geometry, [x - self.position[0], y - self.position[1]]) {
            return false;
        }
        let (low, high) = self
            .geometry
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), p| {
                (low.min(p[2]), high.max(p[2]))
            });
        let (low, high) = (low + self.position[2], high + self.position[2]);
        let floors = nav.and_then(|nav| {
            let middle = [x, y, (low + high) / 2.0];
            let trigger = nav.height(middle, f32::MAX, f32::MAX)?;
            let standing = nav
                .height([x, y, z + STEP_M], f32::MAX, 0.0)
                .or_else(|| nav.height([x, y, z], f32::MAX, f32::MAX))?;
            Some((trigger, standing))
        });
        match floors {
            Some((trigger, standing)) => (trigger - standing).abs() <= STEP_M,
            None => z >= low - TRIGGER_REACH_M && z <= high + TRIGGER_REACH_M,
        }
    }
}

/// How far under and over a trigger's points one off the walkable floors
/// may stand and be in it (metres): a step's sloppiness in how high the
/// outline was drawn, and a jump, well short of the floor above or below.
pub const TRIGGER_REACH_M: f32 = rules::world::triggers::REACH_M;

/// A step's height (metres): floors closer than this are one, and a
/// walkmesh this far over the soles is still under them.
const STEP_M: f32 = rules::world::triggers::STEP_M;

/// Whether `at` is inside `outline` seen from above (both relative to
/// the trigger's place): a line from it crosses the outline's edges an
/// odd number of times. Any outline, convex or not (QUIRKS.md Q-0024).
pub fn inside_outline(outline: &[[f32; 3]], at: [f32; 2]) -> bool {
    let mut inside = false;
    for (k, a) in outline.iter().enumerate() {
        let b = outline[(k + 1) % outline.len()];
        if (a[1] > at[1]) != (b[1] > at[1])
            && at[0] < a[0] + (at[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            inside = !inside;
        }
    }
    inside
}

/// A place to stand: soles (metres) and facing (degrees from +X).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub position: [f32; 3],
    pub yaw: f32,
}

/// One object a module places (a GIT instance). Most are their own facts
/// (every door, placeable and trigger), so they are kept unboxed.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Placement {
    /// Its facts, all its own: what the GIT describes whole (a door, a
    /// placeable, a trigger, ...), and a creature that is not its template
    /// as it is (its GIT entry or an encounter changed it).
    Own(ObjectData),
    /// A creature made as its template is (`ModuleData::templates`), where
    /// the GIT puts it, facing its way.
    Template {
        template: String,
        position: [f32; 3],
        yaw: f32,
    },
}

/// A global's kind, as `globalcat.2da`'s `type` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Global {
    Number,
    Boolean,
    String,
    Location,
}

impl Global {
    /// `globalcat.2da`'s `type` (any case).
    pub fn named(name: &str) -> Option<Global> {
        match name.to_ascii_lowercase().as_str() {
            "number" => Some(Global::Number),
            "boolean" => Some(Global::Boolean),
            "string" => Some(Global::String),
            "location" => Some(Global::Location),
            _ => None,
        }
    }
}

/// A module's entry area as the sim runs it.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleData {
    /// `k1` or `tsl`.
    pub game: String,
    /// Lowercase resref.
    pub name: String,
    /// The IFO's `Mod_Name` as the game shows it (`GetModuleName`: K1's
    /// own text, TSL's talk-table line, e.g. `001EBO`); empty when unknown.
    pub title: String,
    pub area: String,
    /// The area's `Name` as the game shows it (`GetName` of the area).
    pub area_title: String,
    /// The IFO's entry (`Mod_Entry_*`).
    pub entry: Spot,
    /// In the GIT's order, by kind as `ObjectKind::ALL` lists them (an
    /// encounter's additions after); a placement's number is also its
    /// entity in the level's collision and its instance's number.
    pub placements: Vec<Placement>,
    /// The creature templates placements are made from, by lowercase
    /// resref: the script library's, shared (from the import step's store
    /// when there is one).
    pub templates: BTreeMap<String, Arc<ObjectData>>,
    /// How factions stand to each other (`repute.2da`).
    pub repute: Repute,
    /// What each Force power costs, by `spells.2da` row (`forcepoints`,
    /// whole FP); a row past the end cannot be cast.
    pub force_costs: Vec<u16>,
    /// The globals the game declares (`globalcat.2da`), by lowercase name:
    /// scripts keep only these, each of its kind, as KOTOR's catalogue
    /// does (a global it lacks never sets: Dantooine's `Dan_Deesra_Hunt`).
    /// Empty where there is no catalogue (a test's level): any keeps.
    pub globals: BTreeMap<String, Global>,
    /// The module's event scripts (IFO `Mod_On…`) and its entry area's
    /// (ARE `OnEnter`, ...): hook, script resref.
    pub module_scripts: Vec<(String, String)>,
    pub area_scripts: Vec<(String, String)>,
    /// The IFO's `Mod_MinPerHour`: real minutes to a game hour (the
    /// clock's, `GetTimeHour`); 0 where it does not say (2, KOTOR's).
    pub minutes_per_hour: u8,
    /// The players its encounters were planned for (`session::encounters`,
    /// I30 P21): the host and each guest driving a body when it was
    /// planned. 1 unless planned for more.
    pub players: u8,
    /// How the area's picture shows it and the grid its exploring is kept
    /// in (`fog`), where its ARE has a map.
    pub map: Option<crate::fog::Chart>,
    /// `traps.2da` by row: what each trap is (`crate::traps::TrapRow`).
    pub traps: Vec<crate::traps::TrapRow>,
    /// `bodybag.2da` by row: what the dead's remains are
    /// (`world::remains::BagRow`).
    pub bags: Vec<crate::world::remains::BagRow>,
    /// The area's stealth facts (its ARE).
    pub area_stealth: AreaStealth,
    /// The `ranges.2da` rows the rules read by their label.
    pub ranges: Ranges,
    /// The 2DAs encounters read (`crate::spawner::Tables`).
    pub encounter_tables: crate::spawner::Tables,
    /// The ARE's `DisableTransit`: the map page's return button refused
    /// here until a script says otherwise (`SetDisableTransit`).
    pub disable_transit: bool,
    /// TSL's Force facts (`ForceRules`): its powers' sides and Forms, the
    /// alignment's cost table, its rooms' Force ratings. Empty in K1.
    pub force: ForceRules,
}

/// TSL's Force facts its executable reads beyond a power's cost
/// (`context/artifacts/i35/combat-force.md` 2.2, 3): each `spells.2da`
/// row's side (`goodevil`) and `formmask`, `forceadjust.2da`'s cost
/// multipliers by alignment tenth (good, evil), and the area's rooms with
/// their Force ratings (`ForceRoom`). Empty for K1, whose costs are the
/// rows' own [verify: K1 loads `forceadjust.2da` too; whether its
/// executable adjusts by it is not mapped].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ForceRules {
    pub sides: Vec<crate::combat::sheet::Side>,
    pub masks: Vec<u8>,
    pub adjust: Vec<[f32; 2]>,
    pub rooms: Vec<ForceRoom>,
}

/// A room of the area as TSL's `GetRoomForceRating` finds it: its ARE
/// `ForceRating` and its walkmesh's walkable faces seen from above (x, y,
/// metres), and the box round them (least x, least y, most x, most y). A
/// place is in the first room, in the area's order, with a face under or
/// over it (the executable's vertical segment of ±1000 m).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ForceRoom {
    pub rating: i32,
    pub faces: Vec<[[f32; 2]; 3]>,
    bounds: [f32; 4],
}

impl ForceRoom {
    pub fn new(rating: i32, faces: Vec<[[f32; 2]; 3]>) -> ForceRoom {
        let bounds = faces.iter().flatten().fold(
            [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
            |[x0, y0, x1, y1], [x, y]| [x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)],
        );
        ForceRoom {
            rating,
            faces,
            bounds,
        }
    }

    /// Whether `at` (x, y) lies over one of its faces.
    fn holds(&self, [x, y]: [f32; 2]) -> bool {
        let [x0, y0, x1, y1] = self.bounds;
        (x0..=x1).contains(&x)
            && (y0..=y1).contains(&y)
            && self.faces.iter().any(|face| inside(face, [x, y]))
    }
}

impl ForceRules {
    /// The Force rating where `at` stands (metres): its room's, 0 outside
    /// every room.
    pub fn rating(&self, at: [f32; 3]) -> i32 {
        self.rooms
            .iter()
            .find(|room| room.holds([at[0], at[1]]))
            .map_or(0, |room| room.rating)
    }

    /// What power `row` of base cost `base` (whole FP) costs `caster`
    /// (`GetSpellForcePointCost`, `CSWSpell::GetAdjustedForcePoints`): for a
    /// party member, × `forceadjust.2da`'s multiplier for the power's side
    /// at its alignment's tenth, a multiplier above 1 eased
    /// `force_costs::CHARISMA_EASE` a point of its Charisma modifier down to
    /// 1; the room's Force rating (`rating`) over
    /// `force_costs::RATING_DIVISOR` off for the light (alignment
    /// `force_costs::LIGHT_AT_LEAST` or more), on for the dark (under
    /// `force_costs::DARK_UNDER`); at least 1; then Force Potency's or
    /// Mastery's (`forms::cost`). A power of no cost costs nothing.
    pub fn cost(
        &self,
        row: u16,
        base: u16,
        caster: &crate::combat::Sheet,
        party: bool,
        rating: i32,
    ) -> i32 {
        use crate::combat::sheet::{Ability, Side};
        use rules::world::force_costs;
        let mut cost = i32::from(base);
        if cost == 0 {
            return 0;
        }
        if party {
            let tenth = usize::from(caster.good_evil / 10);
            let column = match self.sides.get(usize::from(row)) {
                Some(Side::Light) => Some(0),
                Some(Side::Dark) => Some(1),
                _ => None,
            };
            let mut factor = column
                .and_then(|column| Some(self.adjust.get(tenth)?[column]))
                .unwrap_or(1.0);
            let charisma = caster.modifier(Ability::Charisma);
            if factor > 1.0 && charisma > 0 {
                factor = (factor + charisma as f32 * -force_costs::CHARISMA_EASE).max(1.0);
            }
            cost = (cost as f32 * factor) as i32;
        }
        match caster.good_evil {
            good if good >= force_costs::LIGHT_AT_LEAST => {
                cost -= rating / force_costs::RATING_DIVISOR
            }
            evil if evil < force_costs::DARK_UNDER => cost += rating / force_costs::RATING_DIVISOR,
            _ => {}
        }
        crate::combat::forms::cost(caster, cost.max(1))
    }
}

/// Whether `point` lies in `face` seen from above (its edge counts).
fn inside(face: &[[f32; 2]; 3], point: [f32; 2]) -> bool {
    let side = |a: [f32; 2], b: [f32; 2]| {
        (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])
    };
    let [a, b, c] = *face;
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// The rows of `ranges.2da` rules read by their label, `primaryrange`
/// (metres); K1's values where a level has no table (a test's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ranges {
    /// `PartyCombat`: how far a fight reaches the party (what keeps it in
    /// combat, `world::party`) and a creature in combat sees (`stealth`).
    pub party_combat_m: f32,
    /// `TrapRadius`: a trap's circle (`crate::traps`).
    pub trap_radius_m: f32,
}

impl Ranges {
    /// K1's `ranges.2da`.
    pub const K1: Ranges = Ranges {
        party_combat_m: rules::world::ranges::PARTY_COMBAT_M,
        trap_radius_m: rules::world::ranges::TRAP_RADIUS_M,
    };
}

impl Default for Ranges {
    fn default() -> Ranges {
        Ranges::K1
    }
}

/// An area's stealth facts (its ARE): `ModListenCheck`, added to every
/// listen check, and its stealth XP (`StealthXPMax`, `StealthXPCurrent`,
/// `StealthXPLoss`, `StealthXPEnabled`) as the area starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AreaStealth {
    pub mod_listen: i8,
    pub xp_max: i32,
    pub xp_current: i32,
    pub xp_loss: i32,
    pub xp_enabled: bool,
}

impl ModuleData {
    /// What power `row` costs `caster` (whole FP) where it stands at `at`
    /// (metres): its row's cost, as TSL's rules adjust it (`ForceRules::cost`,
    /// a party member's by alignment, every caster's by the room); `None`
    /// for a row past the table, which cannot be cast.
    pub fn power_cost(
        &self,
        row: u16,
        caster: &crate::combat::Sheet,
        party: bool,
        at: [f32; 3],
    ) -> Option<i32> {
        let base = *self.force_costs.get(usize::from(row))?;
        Some(if self.force.adjust.is_empty() {
            i32::from(base)
        } else {
            let rating = self.force.rating(at);
            self.force.cost(row, base, caster, party, rating)
        })
    }

    /// Placement `index`'s facts: its own, or its template's where it
    /// stands (a creature whose template the module lacks stands as one
    /// without facts of its own: a commoner, no hooks).
    pub fn object(&self, index: usize) -> Option<Cow<'_, ObjectData>> {
        Some(match self.placements.get(index)? {
            Placement::Own(facts) => Cow::Borrowed(facts),
            Placement::Template {
                template,
                position,
                yaw,
            } => Cow::Owned(match self.templates.get(template) {
                Some(facts) => ObjectData::made_as(facts, *position, *yaw),
                None => ObjectData::unknown(template, *position, *yaw),
            }),
        })
    }

    /// Every placement's facts, in order.
    pub fn objects(&self) -> impl Iterator<Item = Cow<'_, ObjectData>> {
        (0..self.placements.len()).filter_map(|index| self.object(index))
    }

    /// Placement `index`'s facts to change (an encounter's, a probe's): one
    /// made as its template is becomes its own first.
    pub fn object_mut(&mut self, index: usize) -> Option<&mut ObjectData> {
        if let Placement::Template { .. } = self.placements.get(index)? {
            let own = self.object(index)?.into_owned();
            self.placements[index] = Placement::Own(own);
        }
        match &mut self.placements[index] {
            Placement::Own(facts) => Some(facts),
            Placement::Template { .. } => None,
        }
    }

    /// `change` on each placement `which` picks, with its number (one made
    /// as its template is becomes its own: only those changed do); how many.
    pub fn change_each(
        &mut self,
        which: impl Fn(&ObjectData) -> bool,
        mut change: impl FnMut(usize, &mut ObjectData),
    ) -> usize {
        let picked: Vec<usize> = (0..self.placements.len())
            .filter(|&index| self.object(index).is_some_and(|object| which(&object)))
            .collect();
        for &index in &picked {
            if let Some(object) = self.object_mut(index) {
                change(index, object);
            }
        }
        picked.len()
    }

    /// The module's or the area's script for `hook` (`Mod_OnModLoad`,
    /// `OnEnter`), if it names one.
    pub fn hook(&self, hook: &str) -> Option<&str> {
        self.module_scripts
            .iter()
            .chain(&self.area_scripts)
            .find(|(label, _)| label == hook)
            .map(|(_, script)| script.as_str())
    }
}

/// How each standard faction regards each other one (`repute.2da`), 0 to
/// 100: hostile and friendly by `rules::world::factions`, neutral between.
/// Row `a`, column `b` is how `a` stands to `b`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Repute {
    pub rows: Vec<Vec<u8>>,
}

/// What scripts changed of how factions stand (`AdjustReputation`), by the
/// pair of `repute.2da` rows, the lower first: both ways, as a player's
/// standing with a faction is that faction's with the player (Tatooine's
/// Sand People, at peace with one in their robes, and the party with them).
pub type Adjustments = BTreeMap<(u8, u8), i16>;

/// No adjustments: the table as it is.
pub static UNADJUSTED: Adjustments = BTreeMap::new();

/// The key `a` and `b`'s adjustment is kept under.
pub fn pair(a: u8, b: u8) -> (u8, u8) {
    (a.min(b), a.max(b))
}

impl Repute {
    /// A probe's table of `repute.2da`'s seven rows: the two of `war`
    /// hostile to each other, each friendly to itself, the rest neutral.
    #[cfg(feature = "probes")]
    pub fn at_war(war: [u8; 2]) -> Repute {
        let war = war.map(usize::from);
        Repute {
            rows: (0..7)
                .map(|a| {
                    (0..7)
                        .map(|b| match (a, b) {
                            _ if a == b => 100,
                            _ if war.contains(&a) && war.contains(&b) => 0,
                            _ => 50,
                        })
                        .collect()
                })
                .collect(),
        }
    }

    /// How `a` stands to `b` in the table; a pair it lacks is friendly to
    /// itself and neutral otherwise.
    fn row(&self, a: u8, b: u8) -> u8 {
        match self
            .rows
            .get(usize::from(a))
            .and_then(|row| row.get(usize::from(b)))
        {
            Some(value) => *value,
            None if a == b => rules::world::factions::UNLISTED_OWN,
            None => rules::world::factions::UNLISTED_OTHER,
        }
    }

    /// The table as scripts' `adjusted` changes it.
    pub fn with<'a>(&'a self, adjusted: &'a Adjustments) -> Standing<'a> {
        Standing {
            table: self,
            adjusted,
        }
    }

    pub fn of(&self, a: u8, b: u8) -> u8 {
        self.with(&UNADJUSTED).of(a, b)
    }

    pub fn hostile(&self, a: u8, b: u8) -> bool {
        self.with(&UNADJUSTED).hostile(a, b)
    }

    pub fn friendly(&self, a: u8, b: u8) -> bool {
        self.with(&UNADJUSTED).friendly(a, b)
    }
}

/// How factions stand now: the module's table and what scripts changed.
#[derive(Debug, Clone, Copy)]
pub struct Standing<'a> {
    table: &'a Repute,
    adjusted: &'a Adjustments,
}

impl Standing<'_> {
    /// How `a` stands to `b`, 0 to 100.
    pub fn of(&self, a: u8, b: u8) -> u8 {
        let by = self.adjusted.get(&pair(a, b)).copied().unwrap_or(0);
        (i16::from(self.table.row(a, b)) + by).clamp(0, 100) as u8
    }

    pub fn hostile(&self, a: u8, b: u8) -> bool {
        self.of(a, b) <= rules::world::factions::HOSTILE_AT_MOST
    }

    /// KOTOR's `GetIsFriend`.
    pub fn friendly(&self, a: u8, b: u8) -> bool {
        self.of(a, b) >= rules::world::factions::FRIENDLY_AT_LEAST
    }
}

/// A character as `relation` reads it: who it is, its faction as scripts
/// left it, and whether it is in the party (a player or a companion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Side {
    pub id: u32,
    pub faction: u8,
    pub party: bool,
}

/// How `a` stands to `b`, the one rule every system asks (the damage
/// pipeline, shots, blades, powers, minds, the party's fight): the same
/// character; hostile by faction (KOTOR's `GetIsEnemy`, `repute.2da` as
/// scripts adjusted it); one side, both in the party or friends by faction
/// (`GetIsFriend`); else an outsider (`aimed` false: the caller knows whom
/// an attack was made at). Without a module's table only the same
/// character and the party are known. What a side's own attacks may do to
/// it is the friendly-fire mode's (`FriendlyFire::spares`).
pub fn relation(standing: Option<Standing<'_>>, a: Side, b: Side) -> crate::combat::Relation {
    use crate::combat::Relation;
    if a.id == b.id {
        Relation::Itself
    } else if standing.is_some_and(|standing| standing.hostile(a.faction, b.faction)) {
        Relation::Hostile
    } else if (a.party && b.party)
        || standing.is_some_and(|standing| standing.friendly(a.faction, b.faction))
    {
        Relation::Party
    } else {
        Relation::Outsider { aimed: false }
    }
}

impl ModuleData {
    /// Where arriving by `tag` puts a player: the first door or waypoint
    /// with that tag (xoreos's rule; reone takes the first object of any
    /// kind), else the module's entry. The spot is where the object
    /// stands; the install drops the player to the floor there.
    pub fn entry_for(&self, tag: Option<&str>) -> (Spot, Option<u32>) {
        let found = tag.filter(|tag| !tag.is_empty()).and_then(|tag| {
            self.objects().enumerate().find(|(_, object)| {
                matches!(object.kind, ObjectKind::Door | ObjectKind::Waypoint)
                    && object.tag.eq_ignore_ascii_case(tag)
            })
        });
        match found {
            Some((index, object)) => (
                Spot {
                    position: object.position,
                    yaw: object.yaw,
                },
                Some(index as u32),
            ),
            None => (self.entry, None),
        }
    }

    /// The objects of `kind`, counted.
    pub fn count(&self, kind: ObjectKind) -> usize {
        self.objects().filter(|o| o.kind == kind).count()
    }

    /// BLAKE3 of every fact above in one fixed encoding: two machines with
    /// the same digest run the same module.
    pub fn digest(&self) -> [u8; 32] {
        let mut out = Writer::default();
        out.bytes(b"KJKM");
        // 9: a creature made as its template is names it (its facts once,
        // with the templates).
        out.u32(9);
        out.string(&self.game);
        out.string(&self.name);
        out.string(&self.area);
        spot(&mut out, &self.entry);
        // Only a module with titles names them, so one without keeps its
        // digest.
        if !self.title.is_empty() || !self.area_title.is_empty() {
            out.string("title");
            out.string(&self.title);
            out.string(&self.area_title);
        }
        // Only a module planned for more than one player names the count,
        // so one planned for one keeps its digest.
        if self.players > 1 {
            out.string("players");
            out.u8(self.players);
        }
        out.u32(self.placements.len() as u32);
        for placement in &self.placements {
            match placement {
                Placement::Own(object) => object.digest_into(&mut out),
                // Past every kind's index.
                Placement::Template {
                    template,
                    position,
                    yaw,
                } => {
                    out.u8(u8::MAX);
                    out.string(template);
                    spot(
                        &mut out,
                        &Spot {
                            position: *position,
                            yaw: *yaw,
                        },
                    );
                }
            }
        }
        out.u32(self.repute.rows.len() as u32);
        for row in &self.repute.rows {
            out.section(row);
        }
        out.u32(self.force_costs.len() as u32);
        for cost in &self.force_costs {
            out.u16(*cost);
        }
        // Only a module with a trap or bag table or stealth facts names
        // them, so one without keeps its digest.
        if !self.traps.is_empty() {
            out.string("traps");
            out.u32(self.traps.len() as u32);
            for row in &self.traps {
                out.string(&row.label);
                out.string(&row.script);
                out.bytes(&[row.set_dc, row.detect_mod, row.disarm_mod]);
                out.u32(row.name);
                out.string(&row.kit);
                out.string(&row.model);
                out.string(&row.sound);
            }
        }
        if !self.bags.is_empty() {
            out.string("bags");
            out.u32(self.bags.len() as u32);
            for row in &self.bags {
                out.u16(row.appearance.unwrap_or(u16::MAX));
                out.string(&row.name);
                out.u8(u8::from(row.corpse));
            }
        }
        if self.area_stealth != AreaStealth::default() {
            let stealth = &self.area_stealth;
            out.string("stealth");
            out.i8(stealth.mod_listen);
            for value in [stealth.xp_max, stealth.xp_current, stealth.xp_loss] {
                out.u32(value as u32);
            }
            out.u8(u8::from(stealth.xp_enabled));
        }
        // Only a module with the encounters' tables names them, so one
        // without keeps its digest.
        if self.encounter_tables != crate::spawner::Tables::default() {
            out.string("encounter tables");
            self.encounter_tables.digest_into(&mut out);
        }
        if self.ranges != Ranges::K1 {
            out.string("ranges");
            out.f32(self.ranges.party_combat_m);
            out.f32(self.ranges.trap_radius_m);
        }
        if self.disable_transit {
            out.string("transit disabled");
        }
        // Only a module with a global catalogue names it, so one without
        // keeps its digest.
        if !self.globals.is_empty() {
            out.string("globals");
            out.u32(self.globals.len() as u32);
            for (name, kind) in &self.globals {
                out.string(name);
                out.u8(*kind as u8);
            }
        }
        // Only a module with hooks names them, so one without keeps its
        // digest.
        for (name, hooks) in [
            ("module", &self.module_scripts),
            ("area", &self.area_scripts),
        ] {
            if !hooks.is_empty() {
                out.string(name);
                out.u32(hooks.len() as u32);
                for (hook, script) in hooks {
                    out.string(hook);
                    out.string(script);
                }
            }
        }
        // Only a module whose hours are not KOTOR's two minutes names them,
        // so one with them keeps its digest.
        if !matches!(self.minutes_per_hour, 0 | 2) {
            out.string("minutes per hour");
            out.u8(self.minutes_per_hour);
        }
        // Only a module with a map names it, so one without keeps its
        // digest.
        if let Some(chart) = &self.map {
            out.string("map");
            chart.digest_into(&mut out);
        }
        // Only a module with templates lists them, so one without keeps its
        // digest.
        if !self.templates.is_empty() {
            out.string("templates");
            out.u32(self.templates.len() as u32);
            for (name, facts) in &self.templates {
                out.string(name);
                facts.digest_into(&mut out);
            }
        }
        // Only a module with TSL's Force facts names them, so one without
        // keeps its digest.
        if self.force != ForceRules::default() {
            let force = &self.force;
            out.string("force");
            out.u32(force.sides.len() as u32);
            for side in &force.sides {
                out.u8(*side as u8);
            }
            out.section(&force.masks);
            out.u32(force.adjust.len() as u32);
            for [good, evil] in &force.adjust {
                out.f32(*good);
                out.f32(*evil);
            }
            out.u32(force.rooms.len() as u32);
            for room in &force.rooms {
                out.u32(room.rating as u32);
                out.u32(room.faces.len() as u32);
                for corner in room.faces.iter().flatten() {
                    out.f32(corner[0]);
                    out.f32(corner[1]);
                }
            }
        }
        *blake3::hash(&out.0).as_bytes()
    }
}

fn spot(out: &mut Writer, spot: &Spot) {
    for value in spot.position {
        out.f32(value);
    }
    out.f32(spot.yaw);
}
