//! A save's story: the player's progress keyed only by the games' own
//! identities (global names, quest tags, resrefs, GIT order), never by a
//! `SimId` or anything of kjk's own, so it outlives any change of the
//! engine. It is `story.json` in the save's folder, written with the world
//! in the same atomic write; a load the world cannot make rebuilds the story
//! from it (docs/superpowers/specs/2026-10-28-save-migrations-design.md, 4).
//!
//! Its `schema` only grows: fields are added, never removed or renamed, and
//! a reader takes what it knows and leaves the rest.

mod restore;
pub use restore::ItemLookup;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sim::combat::fighter::Life;
use sim::combat::sheet::{Rows, Sheet};
use sim::module::{ObjectKind, Placement};
use sim::reconcile::Origin;
use sim::script::State;
use sim::{EntityState, Kind, ObjectState, Save, Snapshot};

/// The version of `story.json` (`meta.json`'s `formats.story`).
pub const STORY_SCHEMA: u32 = 1;

/// How far the leader may stand from where the story put it after a
/// restore (`Story::restore` snaps to the nearest ground), metres.
pub const PLACE_SLACK_M: f32 = 2.0;

/// What a save's story holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Story {
    pub schema: u32,
    /// The game the save is of (`k1`, `tsl`), and the commit and Unix
    /// second that wrote it.
    pub game: String,
    pub build: String,
    pub saved: u64,
    pub place: StoryPlace,
    pub globals: StoryGlobals,
    /// The party table's companions and the players' characters.
    pub party: Vec<StoryCharacter>,
    /// The party's AI style (`PARTY_AISTYLE_*`).
    pub party_ai: i32,
    /// Who leads, as `items` names holders: `player<seat>` or `npc<number>`.
    pub leader: String,
    /// The items each holder has, by who holds them: `party` (the shared
    /// pool), `player<seat>`, `npc<number>` (a companion's `NPC_*`).
    pub items: BTreeMap<String, Vec<StoryItem>>,
    pub gold: i32,
    pub components: i32,
    pub chemicals: i32,
    /// The entry each quest has reached, the latest updated last.
    pub journal: Vec<(String, i32)>,
    pub galaxy: StoryGalaxy,
    /// What scripts changed of how factions stand: (faction, faction,
    /// adjustment).
    pub reputation: Vec<(u8, u8, i16)>,
    /// The modules `SetAreaUnescapable` marked.
    pub unescapable: Vec<String>,
    /// Whether each area's `DisableTransit` a script set, by module name.
    pub transit: BTreeMap<String, bool>,
    /// What is kept of each module visited, by lowercase name.
    pub visited: BTreeMap<String, StoryModule>,
}

/// Where the leader stands.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryPlace {
    pub module: String,
    /// Game space, metres.
    pub position: [f32; 3],
    /// Degrees from +X.
    pub facing: f32,
    /// The tag of the module's waypoint nearest to the leader, a place to
    /// arrive at when the position no longer fits.
    pub waypoint: String,
}

/// The scripts' globals, by name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryGlobals {
    pub numbers: BTreeMap<String, i32>,
    pub booleans: BTreeMap<String, bool>,
    pub strings: BTreeMap<String, String>,
    /// A place (metres) and a facing (degrees).
    pub places: BTreeMap<String, ([f32; 3], f32)>,
}

/// A player's character or a companion.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryCharacter {
    /// The seat of a player's character.
    pub seat: Option<u8>,
    /// The `NPC_*` number of a companion, and its template.
    pub npc: Option<u8>,
    pub template: String,
    pub member: bool,
    pub leader: bool,
    pub influence: Option<u8>,
    /// TSL's base alignment, where a shift moved it.
    pub base: Option<u8>,
    pub ai_style: Option<i32>,
    /// A player's name, appearance, gender and portrait (`personas`).
    pub name: String,
    pub appearance: u16,
    pub gender: u8,
    pub portrait: u16,
    /// The classes and levels taken in play: (`classes.2da` row, levels).
    pub classes: Vec<(u8, u8)>,
    pub level: u8,
    pub abilities: [u8; 6],
    pub skills: [u8; 8],
    /// `feat.2da` and `spells.2da` rows known.
    pub feats: Vec<u16>,
    pub powers: Vec<u16>,
    /// Hit and Force points now (hundredths) and at most.
    pub hp: i32,
    pub fp: i32,
    pub max_hp: u16,
    pub max_fp: u16,
    /// Good and evil, 0 (dark) to 100 (light).
    pub alignment: u8,
    pub xp: i32,
}

/// An item held: its template, and where and how.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryItem {
    pub template: String,
    /// The slot it is equipped in; none when carried.
    pub slot: Option<u8>,
    pub stack: u16,
    pub charges: u8,
    /// The upgrades fitted (`upgrade.2da` rows as bits), and TSL's fitted
    /// upgrade items by template.
    pub upgrades: u32,
    pub fitted: Vec<String>,
}

/// The galaxy map's planets (`planetary.2da` rows).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryGalaxy {
    pub available: Vec<u8>,
    pub selectable: Vec<u8>,
    pub selected: Option<u8>,
}

/// An object of a module as the games name it: its place in the module's
/// placements in GIT order, the template it was placed from, its kind
/// (`ObjectKind::name`) and its tag, which find it again in a module whose
/// placements moved (`sim::reconcile::renumber`).
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(default)]
pub struct OriginKey {
    pub index: u32,
    pub template: String,
    pub kind: String,
    pub tag: String,
}

impl OriginKey {
    /// The key of placement `index`, of `origin`.
    pub fn of(index: u32, origin: &Origin) -> OriginKey {
        OriginKey {
            index,
            template: origin.template.clone(),
            kind: origin.kind.name().to_string(),
            tag: origin.tag.clone(),
        }
    }

    /// The placement it names, as a roster keeps it; none for a kind no
    /// build knows.
    pub fn origin(&self) -> Option<Origin> {
        let kind = ObjectKind::ALL
            .into_iter()
            .find(|kind| kind.name() == self.kind)?;
        Some(Origin {
            kind,
            template: self.template.clone(),
            tag: self.tag.clone(),
        })
    }
}

/// What a module visited keeps. A save without a placement roster (from
/// before they were kept) gives `spawns_ran` alone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoryModule {
    /// Whether the module's creatures' spawn scripts have run.
    pub spawns_ran: bool,
    /// Placements destroyed, or creatures gone from their place.
    pub destroyed: Vec<OriginKey>,
    /// A door's or placeable's state (0 closed, 1 and 2 open) and lock
    /// (`ObjectState::lock`) where either moved from as placed.
    pub doors: Vec<(OriginKey, u8, u8)>,
    /// The locals of an object not destroyed: its object, 0 for booleans or
    /// 1 for numbers, the index and the value.
    pub locals: Vec<(OriginKey, u8, u32, i32)>,
}

impl Story {
    /// The story of `save`, which is in `level`. `build` and `saved` are
    /// the write's to stamp.
    pub fn of(save: &Save, level: &sim::Level) -> Story {
        let (snapshot, script) = (&save.snapshot, &save.script);
        let place = snapshot.place.as_ref();
        let module = place.map_or_else(|| level.id().name, |place| place.module.clone());
        Story {
            schema: STORY_SCHEMA,
            game: place.map_or_else(String::new, |place| place.game.clone()),
            place: leader_place(snapshot, level, module),
            globals: StoryGlobals {
                numbers: script.numbers.clone(),
                booleans: script.booleans.clone(),
                strings: script.strings.clone(),
                places: script.places.clone(),
            },
            party: characters(snapshot, script),
            party_ai: script.party.party_ai,
            leader: leader_label(snapshot, script),
            items: held_items(snapshot, script, level),
            gold: script.gold,
            components: script.components,
            chemicals: script.chemicals,
            journal: script.journal.clone(),
            galaxy: StoryGalaxy {
                available: script.galaxy.available.iter().copied().collect(),
                selectable: script.galaxy.selectable.iter().copied().collect(),
                selected: script.galaxy.selected,
            },
            reputation: script
                .reputation
                .iter()
                .map(|(&(a, b), &adjustment)| (a, b, adjustment))
                .collect(),
            unescapable: script.unescapable.iter().cloned().collect(),
            transit: script.transit.clone(),
            visited: visited(snapshot, script),
            ..Story::default()
        }
    }

    /// How `other`, a story written again after a restore of `self`, differs
    /// from it: the top-level fields that differ, and how far apart the
    /// leaders stand (metres). The build, the time and the leader's exact
    /// position are the write's, not the story's.
    pub fn compare(&self, other: &Story) -> (Vec<&'static str>, f32) {
        let [a, b] = [self.place.position, other.place.position];
        let off = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let [mut before, mut after] = [self.clone(), other.clone()];
        for story in [&mut before, &mut after] {
            (story.build, story.saved, story.place.position) = (String::new(), 0, [0.0; 3]);
        }
        (before.differing(&after), off)
    }

    /// The top-level fields in which `self` and `other` differ.
    pub fn differing(&self, other: &Story) -> Vec<&'static str> {
        let (a, b) = (self, other);
        let fields = [
            ("schema", a.schema == b.schema),
            ("game", a.game == b.game),
            ("place", a.place == b.place),
            ("globals", a.globals == b.globals),
            ("party", a.party == b.party),
            ("party_ai", a.party_ai == b.party_ai),
            ("leader", a.leader == b.leader),
            ("items", a.items == b.items),
            (
                "purse",
                (a.gold, a.components, a.chemicals) == (b.gold, b.components, b.chemicals),
            ),
            ("journal", a.journal == b.journal),
            ("galaxy", a.galaxy == b.galaxy),
            ("reputation", a.reputation == b.reputation),
            ("unescapable", a.unescapable == b.unescapable),
            ("transit", a.transit == b.transit),
            ("visited", a.visited == b.visited),
        ];
        fields
            .into_iter()
            .filter(|(_, same)| !same)
            .map(|(name, _)| name)
            .collect()
    }
}

/// Where the leader stands in `module`, and the waypoint nearest to it.
fn leader_place(snapshot: &Snapshot, level: &sim::Level, module: String) -> StoryPlace {
    let leader = snapshot
        .leading
        .and_then(|id| snapshot.get(id))
        .or_else(|| snapshot.body(sim::Seat(0)));
    let position = leader.map_or([0.0; 3], |leader| leader.position);
    let facing = leader
        .and_then(|leader| leader.player.as_ref())
        .map_or(0.0, |motion| motion.view[1]);
    let distance = |at: [f32; 3]| (at[0] - position[0]).powi(2) + (at[1] - position[1]).powi(2);
    let waypoint = level
        .module()
        .into_iter()
        .flat_map(|module| &module.placements)
        .filter_map(|placement| match placement {
            Placement::Own(facts) if facts.kind == ObjectKind::Waypoint => Some(facts),
            _ => None,
        })
        .min_by(|a, b| distance(a.position).total_cmp(&distance(b.position)))
        .map_or_else(String::new, |facts| facts.tag.clone());
    StoryPlace {
        module,
        position,
        facing,
        waypoint,
    }
}

/// The rows in `set`.
fn rows<const WORDS: usize>(set: &Rows<WORDS>) -> Vec<u16> {
    (0..Rows::<WORDS>::ROWS)
        .filter(|row| set.has(*row))
        .collect()
}

/// A character's sheet, fight and career as the story keeps them.
fn character(
    mut out: StoryCharacter,
    sheet: Option<&Sheet>,
    hp_fp: Option<(i32, i32)>,
    script: &State,
    id: Option<u32>,
) -> StoryCharacter {
    if let Some(sheet) = sheet {
        out.level = sheet.level;
        out.abilities = sheet.abilities;
        out.skills = sheet.skills;
        out.feats = rows(&sheet.feats);
        out.powers = rows(&sheet.powers);
        out.max_hp = sheet.max_hp;
        out.max_fp = sheet.max_fp;
        out.alignment = sheet.good_evil;
    }
    if let Some((hp, fp)) = hp_fp {
        (out.hp, out.fp) = (hp, fp);
    }
    if let Some(career) = id.and_then(|id| script.careers.get(&id)) {
        out.classes = career.clone();
    }
    out
}

/// The players' characters (by seat) and the party table's companions.
fn characters(snapshot: &Snapshot, script: &State) -> Vec<StoryCharacter> {
    let party = &script.party;
    let mut out = Vec::new();
    for entity in &snapshot.entities {
        let Kind::Avatar(seat) = entity.kind else {
            continue;
        };
        let persona = snapshot
            .personas
            .iter()
            .find(|persona| persona.seat == seat);
        let base = StoryCharacter {
            seat: Some(seat.0),
            member: true,
            leader: seat.0 == 0 && party.leader.is_none(),
            name: persona.map_or_else(String::new, |persona| persona.name.clone()),
            appearance: persona.map_or(0, |persona| persona.appearance),
            gender: persona.map_or(0, |persona| persona.gender),
            portrait: persona.map_or(0, |persona| persona.portrait),
            xp: script.xp.get(&sim::inventory::PARTY).copied().unwrap_or(0),
            ..StoryCharacter::default()
        };
        out.push(character(
            base,
            entity.sheet.as_ref(),
            entity
                .fighter
                .as_ref()
                .map(|fighter| (fighter.hp, fighter.fp)),
            script,
            Some(entity.id.0),
        ));
    }
    for (&number, npc) in &party.available {
        let entity = npc.object.and_then(|id| snapshot.get(sim::SimId(id)));
        let kept = npc.kept.as_ref();
        let id = holder(npc);
        let base = StoryCharacter {
            npc: Some(number),
            template: npc.template.clone(),
            member: party.members.contains(&number),
            leader: party.leader == Some(number),
            influence: party.influence.get(&number).copied(),
            base: party.bases.get(&number).copied(),
            ai_style: id.and_then(|id| party.ai_styles.get(&id).copied()),
            xp: id.and_then(|id| script.xp.get(&id).copied()).unwrap_or(0),
            ..StoryCharacter::default()
        };
        let sheet = entity
            .and_then(|entity| entity.sheet.as_ref())
            .or(kept.map(|kept| &kept.sheet));
        let hp_fp = entity
            .and_then(|entity| entity.fighter.as_ref())
            .map(|fighter| (fighter.hp, fighter.fp))
            .or(kept.map(|kept| (kept.hp, kept.fp)));
        out.push(character(base, sheet, hp_fp, script, id));
    }
    out
}

/// The creature id a companion's items, career and XP are under: its
/// creature, or the one that last had it when it is away from the party
/// (`Kept::from`).
fn holder(npc: &sim::script::party::Npc) -> Option<u32> {
    npc.object.or(npc.kept.as_ref().map(|kept| kept.from))
}

/// Who leads, as `items` names holders: `npc<number>` or `player<seat>`.
fn leader_label(snapshot: &Snapshot, script: &State) -> String {
    if let Some(number) = script.party.leader {
        return format!("npc{number}");
    }
    let seat = snapshot
        .leading
        .and_then(|id| snapshot.get(id))
        .and_then(|entity| match entity.kind {
            Kind::Avatar(seat) => Some(seat.0),
            _ => None,
        });
    format!("player{}", seat.unwrap_or(0))
}

/// What the party pool, each player and each companion has.
fn held_items(snapshot: &Snapshot, script: &State, level: &sim::Level) -> BTreeMap<String, Vec<StoryItem>> {
    let holders = std::iter::once((sim::inventory::PARTY, "party".to_string()))
        .chain(
            snapshot
                .entities
                .iter()
                .filter_map(|entity| match entity.kind {
                    Kind::Avatar(seat) => Some((entity.id.0, format!("player{}", seat.0))),
                    _ => None,
                }),
        )
        .chain(
            script
                .party
                .available
                .iter()
                .filter(|(number, npc)| {
                    // One neither in the party nor kept away has her
                    // creature in play (joined and left, or never joined):
                    // her items are listed when it holds more than her
                    // template's own, which come back with the template.
                    let carrying = || {
                        let party = sim::reconcile::Party {
                            members: &[],
                            items: &script.items,
                        };
                        npc.object
                            .and_then(|id| snapshot.get(sim::SimId(id)))
                            .is_some_and(|entity| {
                                party.stored_in(
                                    entity,
                                    sim::module::ObjectKind::Creature,
                                    &npc.template,
                                    level,
                                )
                            })
                    };
                    npc.kept.is_some() || script.party.members.contains(number) || carrying()
                })
                .filter_map(|(number, npc)| Some((holder(npc)?, format!("npc{number}")))),
        );
    holders
        .filter_map(|(holder, label)| {
            let items: Vec<StoryItem> = script
                .items
                .held_by(holder)
                .map(|(_, item)| StoryItem {
                    template: item.template.clone(),
                    slot: item.slot,
                    stack: item.stack,
                    charges: item.charges,
                    upgrades: item.upgrades,
                    fitted: item.fitted.clone(),
                })
                .collect();
            (!items.is_empty()).then_some((label, items))
        })
        .collect()
}

/// The modules visited: the one the party is in and each left behind.
fn visited(snapshot: &Snapshot, script: &State) -> BTreeMap<String, StoryModule> {
    // Each object of any module by its id, where the module's roster names it.
    let mut named: BTreeMap<u32, (String, OriginKey)> = BTreeMap::new();
    let mut out = BTreeMap::new();
    let here = snapshot.place.as_ref().map(|place| {
        (
            place.module.as_str(),
            snapshot.entities.as_slice(),
            snapshot.roster.as_deref(),
        )
    });
    let left = snapshot.stash.iter().map(|stashed| {
        (
            stashed.place.module.as_str(),
            stashed.entities.as_slice(),
            stashed.roster.as_deref(),
        )
    });
    for (module, entities, roster) in here.into_iter().chain(left) {
        let key = module.to_ascii_lowercase();
        let mut kept = StoryModule {
            spawns_ran: script.spawned.contains(&key),
            ..StoryModule::default()
        };
        if let Some(roster) = roster {
            fill_module(&mut kept, entities, roster, &key, &mut named);
        }
        out.insert(key, kept);
    }
    // Modules whose spawns ran and that nothing else is kept of.
    for module in &script.spawned {
        out.entry(module.clone()).or_insert(StoryModule {
            spawns_ran: true,
            ..StoryModule::default()
        });
    }
    for (&(object, kind, index), &value) in &script.locals {
        let (Some((module, origin)), Ok(index)) = (named.get(&object), u32::try_from(index)) else {
            continue;
        };
        // A destroyed object is gone with its locals: nothing is there to
        // hold them when the story is restored.
        if let Some(kept) = out
            .get_mut(module)
            .filter(|kept| !kept.destroyed.contains(origin))
        {
            kept.locals.push((origin.clone(), kind, index, value));
        }
    }
    out
}

/// `kept` given the destroyed objects and the doors and placeables moved
/// from as placed among `entities`, of a module whose placements were
/// `roster`; `named` gets each object's key.
fn fill_module(
    kept: &mut StoryModule,
    entities: &[EntityState],
    roster: &[Origin],
    module: &str,
    named: &mut BTreeMap<u32, (String, OriginKey)>,
) {
    let key = |index: u32| OriginKey::of(index, &roster[index as usize]);
    let mut present = vec![false; roster.len()];
    // Creatures lying dead (`Life::Dead`): their bodies are in play still.
    let mut dead = vec![false; roster.len()];
    for entity in entities {
        let Kind::Object(object_kind, number) = entity.kind else {
            continue;
        };
        let Some(origin) = roster.get(number as usize) else {
            continue;
        };
        present[number as usize] = true;
        dead[number as usize] = entity
            .fighter
            .as_ref()
            .is_some_and(|fighter| fighter.life == Life::Dead);
        named.insert(entity.id.0, (module.to_string(), key(number)));
        let Some(state) = entity.object else {
            continue;
        };
        if state.state == ObjectState::GONE.state {
            kept.destroyed.push(key(number));
        } else if matches!(origin.kind, ObjectKind::Door | ObjectKind::Placeable)
            && object_kind == origin.kind
            && (state.state != 0 || state.lock != 0)
        {
            kept.doors.push((key(number), state.state, state.lock));
        }
    }
    for (number, origin) in roster.iter().enumerate() {
        if origin.kind == ObjectKind::Creature && (!present[number] || dead[number]) {
            kept.destroyed.push(key(number as u32));
        }
    }
}
