//! A story restored (docs/superpowers/specs/2026-10-28-save-migrations-design.md,
//! 4b): the last rung of a load, for a save the world cannot be made from
//! exactly. The saved module is installed fresh, as a new game arrives in
//! it (`SimWorld::new`: its placements anew, their spawn scripts and the
//! module's entry scripts waiting for the first step), then the story is
//! laid over it as sim's own values: the scripts' state
//! (`SimWorld::set_script_state`) and the snapshot the world is made in
//! (`SimWorld::from_snapshot`), as a save loads.
//!
//! Each module left is a stash entry the next arrival there reconciles
//! with its placements (`sim::reconcile`): its roster names, at their
//! places in the module's list, what was destroyed (it stays gone), the
//! doors and placeables moved (open or locked as they were) and the
//! objects with locals (kept under their ids), each standing where its
//! placement does; every other placement is placed anew and comes alive as
//! on a first visit.
//! What the story has that cannot come back is said in the lines returned.

use std::collections::BTreeMap;

use sim::combat::fighter::Fighter;
use sim::combat::sheet::{Rows, Sheet, factions};
use sim::inventory::{Catalog, ItemFacts, PARTY};
use sim::levels::Career;
use sim::module::{ObjectData, ObjectKind};
use sim::motion::{MotionState, UNIT};
use sim::reconcile::{Origin, Renumber, renumber};
use sim::script::State;
use sim::script::party::{Kept, Npc};
use sim::{
    EntityState, Guests, Kind, ObjectState, Place, Seat, SimId, SimWorld, Snapshot, Spawned,
    Stashed, WorldConfig,
};

use super::{OriginKey, Story, StoryCharacter, StoryModule};

/// How far from the leader (metres) the companions made for the party
/// stand, and how far apart (degrees) around behind it.
const COMPANION_M: f32 = 1.0;
const COMPANION_APART_DEG: f32 = 45.0;

/// A roster's place that names no placement, for a story without the
/// module's roster: kept so the destroyed after it keep their numbers. No
/// resref is `*`, so it matches none.
fn unnamed() -> Origin {
    Origin {
        kind: ObjectKind::Waypoint,
        template: "*".into(),
        tag: "*".into(),
    }
}

/// `kept`'s roster as a stash keeps it for a restore: each placement
/// marked to be placed anew (`sim::reconcile::ANEW`) until a fact names
/// it, one of a kind no build knows unnamed. Empty without one.
fn marked(kept: &StoryModule) -> Vec<Origin> {
    kept.roster
        .iter()
        .map(|placed| placed.origin().map_or_else(unnamed, |origin| origin.anew()))
        .collect()
}

/// The facts of an item template the module restored in lacks, from the
/// install's whole data (none where it has none); `&|_| None` for a host
/// without one.
pub type ItemLookup<'a> = &'a dyn Fn(&str) -> Option<ItemFacts>;

impl Story {
    /// The world this story restores in `level` (its module's, planned) for
    /// a session of `seats` (more when the story has players past them),
    /// and a line for each thing of the story it could not bring back. The
    /// save it came from is not touched: the next save is a normal one.
    pub fn restore(
        &self,
        level: &sim::Level,
        seats: u8,
        items: ItemLookup,
    ) -> Result<(SimWorld, Vec<String>), String> {
        let place = level.place().ok_or("the level has no module")?;
        if !place.module.eq_ignore_ascii_case(&self.place.module) {
            return Err(format!(
                "the story is in {}, the level is {}",
                self.place.module, place.module
            ));
        }
        let own = self.seats();
        let seats = seats.max(own);
        let config = WorldConfig {
            seats,
            guests: if own >= seats {
                Guests::Avatars
            } else {
                Guests::Companions
            },
            ..WorldConfig::one_seat()
        };
        let mut fresh = SimWorld::new(config, level.clone()).map_err(|error| error.to_string())?;
        let mut state = fresh.script_state().clone();
        // The story's items the module's catalog lacks, as the lookup has
        // them: known, as an item carried out of its module is.
        state.known = self
            .items
            .values()
            .flatten()
            .filter(|item| {
                level
                    .scripts()
                    .and_then(|library| library.items().get(&item.template))
                    .is_none()
            })
            .filter_map(|item| items(&item.template))
            .map(|facts| (facts.template.clone(), facts))
            .collect();
        let mut restore = Restore {
            story: self,
            level,
            catalog: level.scripts().map(|library| library.items()),
            snapshot: fresh.snapshot(),
            state,
            holders: BTreeMap::from([("party".to_string(), PARTY)]),
            members: Vec::new(),
            personas: Vec::new(),
            lost: Vec::new(),
        };
        restore.globals();
        restore.players();
        restore.companions();
        restore.items();
        restore.here(&place.module);
        restore.left(&place);
        restore.on_ground();
        let Restore {
            snapshot,
            state,
            personas,
            lost,
            ..
        } = restore;
        let mut world =
            SimWorld::from_snapshot(&snapshot, level.clone()).map_err(|error| error.to_string())?;
        world.set_script_state(state);
        for persona in personas {
            world.set_persona(persona);
        }
        Ok((world, lost))
    }
}

/// A module left as a stash entry knows it (`Restore::left_objects`):
/// the objects the story names, by their placements' numbers.
#[derive(Default)]
struct Left {
    entities: Vec<EntityState>,
    roster: Vec<Origin>,
}

impl Left {
    /// Placement `key` in the roster, as `origin`, the places before it
    /// unnamed.
    fn name(&mut self, key: &OriginKey, origin: Origin) {
        let at = key.index as usize;
        if self.roster.len() <= at {
            self.roster.resize(at + 1, unnamed());
        }
        self.roster[at] = origin;
    }

    /// The id of the object of placement `key`, if there is one.
    fn id_of(&self, key: &OriginKey) -> Option<u32> {
        self.entities
            .iter()
            .find(|entity| matches!(entity.kind, Kind::Object(_, number) if number == key.index))
            .map(|entity| entity.id.0)
    }

    /// Placement `key` (`origin`, not a creature) as object `id` in `state`,
    /// unless an object of it is there already.
    fn object(&mut self, id: u32, key: &OriginKey, origin: Origin, state: ObjectState) {
        if self.id_of(key).is_some() {
            return;
        }
        self.entities.push(EntityState {
            id: SimId(id),
            kind: Kind::Object(origin.kind, key.index),
            // Where its placement stands, once the module is there.
            position: [0.0; 3],
            velocity: [0.0; 3],
            player: None,
            retarget_ms: 0,
            life_ms: None,
            going: None,
            object: Some(state),
            fighter: None,
            sheet: None,
            missile: None,
        });
        self.name(key, origin);
    }
}

/// A restore under way: the fresh world's snapshot and scripts' state, the
/// story laid over them bit by bit.
struct Restore<'a> {
    story: &'a Story,
    level: &'a sim::Level,
    catalog: Option<&'a Catalog>,
    snapshot: Snapshot,
    state: State,
    /// The id each holder the story names has here (`Story::items`).
    holders: BTreeMap<String, u32>,
    /// The companions made for the party: creature and template.
    members: Vec<(u32, String)>,
    personas: Vec<sim::persona::Persona>,
    lost: Vec<String>,
}

impl Restore<'_> {
    /// A new id, as the world's next.
    fn next_id(&mut self) -> u32 {
        self.snapshot.take_id().0
    }

    /// The facts of the items `label` wears, by slot.
    fn worn(&self, label: &str) -> Vec<(u8, &ItemFacts)> {
        let Some(catalog) = self.catalog else {
            return Vec::new();
        };
        let items = self.story.items.get(label).into_iter().flatten();
        items
            .filter_map(|item| Some((item.slot?, self.state.item_facts(catalog, &item.template)?)))
            .collect()
    }

    /// `character`'s sheet: `base` (a template's, with what it wears as
    /// `before`, or a level-0 character's) grown from career `from` to the
    /// story's, with its scores, skills, feats, powers, points and alignment,
    /// dressed in what `label` wears.
    fn sheet(
        &self,
        base: Sheet,
        from: &Career,
        before: &[(u8, &ItemFacts)],
        character: &StoryCharacter,
        label: &str,
    ) -> Sheet {
        let classes = self.level.scripts().map(|library| library.classes());
        let mut sheet = match classes {
            Some(classes) if !character.classes.is_empty() => {
                classes.grown_to(&base, from, &character.classes)
            }
            _ => base,
        };
        sheet.feats = Rows::default();
        for &feat in &character.feats {
            sheet.feats.insert(feat);
            sim::combat::passive::learn(&mut sheet, feat, character.level);
        }
        sheet.powers = Rows::default();
        for &power in &character.powers {
            sheet.powers.insert(power);
        }
        sheet.abilities = character.abilities;
        sheet.skills = character.skills;
        sheet.level = character.level;
        sheet.max_hp = character.max_hp;
        sheet.max_fp = character.max_fp;
        sheet.good_evil = character.alignment;
        sim::inventory::rewear(sheet, before, &self.worn(label))
    }

    /// `character`'s fight: as its sheet makes it, its points as they were.
    fn fighter(sheet: &Sheet, character: &StoryCharacter) -> Fighter {
        let mut fighter = Fighter::of(sheet);
        (fighter.hp, fighter.fp) = (character.hp, character.fp);
        fighter
    }

    /// `character`'s career, XP and AI style under `holder`.
    fn career(&mut self, holder: u32, character: &StoryCharacter) {
        if !character.classes.is_empty() {
            self.state.careers.insert(holder, character.classes.clone());
        }
        if character.npc.is_some() && character.xp != 0 {
            self.state.xp.insert(holder, character.xp);
        }
        if let Some(style) = character.ai_style {
            self.state.party.ai_styles.insert(holder, style);
        }
    }

    /// The globals, purse, journal, galaxy, reputation, areas and the
    /// party's AI.
    fn globals(&mut self) {
        let (story, state) = (self.story, &mut self.state);
        state.numbers = story.globals.numbers.clone();
        state.booleans = story.globals.booleans.clone();
        state.strings = story.globals.strings.clone();
        state.places = story.globals.places.clone();
        state.gold = story.gold;
        state.components = story.components;
        state.chemicals = story.chemicals;
        state.journal = story.journal.clone();
        state.galaxy.available = story.galaxy.available.iter().copied().collect();
        state.galaxy.selectable = story.galaxy.selectable.iter().copied().collect();
        state.galaxy.selected = story.galaxy.selected;
        state.reputation = story.adjustments();
        state.unescapable = story.unescapable.iter().cloned().collect();
        state.transit = story.transit.clone();
        state.party.party_ai = story.party_ai;
    }

    /// The players' characters, on their seats' bodies: sheet, points,
    /// career, persona; the party's XP.
    fn players(&mut self) {
        let story = self.story;
        for character in &story.party {
            let Some(seat) = character.seat else {
                continue;
            };
            let label = format!("player{seat}");
            let Some(at) = self
                .snapshot
                .entities
                .iter()
                .position(|entity| entity.kind == Kind::Avatar(Seat(seat)))
            else {
                self.lost
                    .push(format!("{label}: no character of its own on its seat here"));
                continue;
            };
            let faction = self.snapshot.entities[at]
                .sheet
                .map_or(factions::PLAYER, |sheet| sheet.faction);
            let base = sim::creation::unmade(character.abilities, faction);
            let sheet = self.sheet(base, &Career::new(), &[], character, &label);
            let entity = &mut self.snapshot.entities[at];
            entity.sheet = Some(sheet);
            entity.fighter = Some(Self::fighter(&sheet, character));
            let id = entity.id.0;
            self.holders.insert(label, id);
            self.career(id, character);
            if seat == 0 && character.xp != 0 {
                self.state.xp.insert(PARTY, character.xp);
            }
            let persona = sim::persona::Persona {
                seat: Seat(seat),
                appearance: character.appearance,
                gender: character.gender,
                portrait: character.portrait,
                name: character.name.clone(),
            };
            let blank = sim::persona::Persona {
                seat: Seat(seat),
                appearance: 0,
                gender: 0,
                portrait: 0,
                name: String::new(),
            };
            if persona != blank {
                self.personas.push(persona);
            }
        }
    }

    /// Where the first seat stands and looks.
    fn leader_at(&self) -> Option<([f32; 3], f32)> {
        let leader = self
            .snapshot
            .entities
            .iter()
            .find(|entity| entity.kind == Kind::Avatar(Seat(0)))?;
        let player = leader.player?;
        Some((player.origin, player.view[1]))
    }

    /// The party table's companions: a member made in the party beside the
    /// leader, of its template, as the story has its sheet and points; one
    /// away kept by the table (`Kept`) as it stood, to be made when a
    /// script spawns it; one the story has no sheet of left to its
    /// template.
    fn companions(&mut self) {
        let story = self.story;
        let library = self.level.scripts();
        let made = story
            .party
            .iter()
            .filter(|character| character.member && character.npc.is_some())
            .count();
        let mut beside = 0;
        for character in &story.party {
            let Some(number) = character.npc else {
                continue;
            };
            let label = format!("npc{number}");
            let facts = library.and_then(|library| library.creature(&character.template));
            let before = facts.map_or_else(Vec::new, |facts| self.template_worn(facts));
            let base = facts
                .and_then(|facts| facts.sheet)
                .unwrap_or_else(Sheet::commoner);
            let from = facts.map_or_else(Career::new, |facts| facts.classes.clone());
            let sheet = self.sheet(base, &from, &before, character, &label);
            self.state.party.available.insert(
                number,
                Npc {
                    template: character.template.clone(),
                    object: None,
                    kept: None,
                    template_data: None,
                },
            );
            let room = self.state.party.members.len() < sim::script::party::FOLLOWERS;
            let holder = match (character.member, facts, room) {
                (true, Some(_), true) => {
                    let id = self.member(character, sheet, (beside, made));
                    beside += 1;
                    // The table's own rule (`Party::join`): its object, in
                    // the party, room checked above.
                    self.state.party.join(number, id);
                    Some(id)
                }
                (member, facts, _) => {
                    if member {
                        let why = match facts {
                            None => "this module has no template to make it of",
                            Some(_) => "the party is full",
                        };
                        self.lost.push(format!(
                            "{label} ({}): in the party, but {why}; kept away from the party",
                            character.template
                        ));
                    }
                    (character.level > 0).then(|| {
                        let from = self.next_id();
                        if let Some(npc) = self.state.party.available.get_mut(&number) {
                            npc.kept = Some(Kept {
                                from,
                                sheet,
                                hp: character.hp,
                                fp: character.fp,
                                worn: None,
                                look: sim::combat::Look::default(),
                            });
                        }
                        from
                    })
                }
            };
            if let Some(holder) = holder {
                self.holders.insert(label, holder);
                self.career(holder, character);
            }
            let party = &mut self.state.party;
            if let Some(influence) = character.influence {
                party.influence.insert(number, influence);
            }
            if let Some(base) = character.base {
                party.bases.insert(number, base);
            }
            if character.leader {
                party.leader = Some(number);
            }
        }
    }

    /// What `facts` wears as placed, by slot, as its sheet was read with it.
    fn template_worn<'a>(&'a self, facts: &ObjectData) -> Vec<(u8, &'a ItemFacts)> {
        let Some(catalog) = self.catalog else {
            return Vec::new();
        };
        facts
            .inventory
            .iter()
            .filter_map(|held| Some((held.slot?, catalog.get(&held.template)?)))
            .collect()
    }

    /// Companion `character` made in the module as play makes one
    /// (`Spawned`), following the first seat in the player's faction, the
    /// `k`-th of `n` around behind the leader; its id.
    fn member(&mut self, character: &StoryCharacter, sheet: Sheet, (k, n): (usize, usize)) -> u32 {
        let (leader, yaw) = self.leader_at().unwrap_or(([0.0; 3], 0.0));
        let id = self.next_id();
        let index = (self.level.placed().len() + self.snapshot.spawned.len()) as u32;
        self.snapshot.spawned.push(Spawned {
            kind: ObjectKind::Creature,
            template: character.template.clone(),
            yaw,
            holds: Vec::new(),
        });
        let turn = 180.0 + (k as f32 - (n as f32 - 1.0) / 2.0) * COMPANION_APART_DEG;
        let (sin, cos) = sim::math::sin_cos_deg(yaw + turn);
        let reach = COMPANION_M / UNIT;
        let origin = [leader[0] + cos * reach, leader[1] + sin * reach, leader[2]];
        let mut fighter = Self::fighter(&sheet, character);
        fighter.mind.leader = Some(Seat(0));
        fighter.faction = Some(factions::PLAYER);
        let mut player = MotionState::standing(origin, yaw);
        player.set_view([0.0, yaw, 0.0]);
        self.snapshot.entities.push(EntityState {
            id: SimId(id),
            kind: Kind::Object(ObjectKind::Creature, index),
            position: sim::math::scale(origin, UNIT),
            velocity: [0.0; 3],
            player: Some(player),
            retarget_ms: 0,
            life_ms: None,
            going: None,
            object: Some(ObjectState::default()),
            fighter: Some(fighter),
            sheet: Some(sheet),
            missile: None,
        });
        self.members.push((id, character.template.clone()));
        id
    }

    /// Each holder's items, as the story lists them.
    fn items(&mut self) {
        let story = self.story;
        let none = Catalog::default();
        let catalog = self.catalog.unwrap_or(&none);
        for (label, items) in &story.items {
            let Some(&holder) = self.holders.get(label) else {
                self.lost.push(format!(
                    "{label}: {} items, and no one here to hold them",
                    items.len()
                ));
                continue;
            };
            for item in items {
                let made =
                    self.state
                        .make_item(catalog, &item.template, holder, item.stack, item.slot);
                let Some(made) = made else {
                    self.lost.push(format!(
                        "{label}: item {}, not among this module's items or the install's",
                        item.template
                    ));
                    continue;
                };
                if let Some(mut kept) = self.state.items.get_mut(&made.0) {
                    kept.charges = item.charges;
                    kept.upgrades = item.upgrades;
                    kept.fitted = item.fitted.clone();
                }
            }
        }
    }

    /// The entity of the placement `key` names in the module here: where
    /// `moved` (the story's roster renumbered onto the level's placements)
    /// puts it, else, for a story without the roster, the level's placement
    /// at its number, of its template.
    fn placement(&self, key: &OriginKey, moved: Option<&Renumber>) -> Option<usize> {
        let placed = self.level.placed();
        let at = match moved {
            Some(moved) => (*moved.map.get(key.index as usize)?)?,
            None => key.index,
        };
        let facts = placed.get(at as usize)?;
        if !facts.template.eq_ignore_ascii_case(&key.template) {
            return None;
        }
        self.snapshot.entities.iter().position(|entity| {
            matches!(entity.kind, Kind::Object(kind, number) if number == at && kind == facts.kind)
        })
    }

    /// A line for `key`, of `module`, that names no placement here.
    fn unmatched(&mut self, module: &str, what: &str, key: &OriginKey) {
        self.lost.push(format!(
            "{module}: {what} {} {} (number {}): no such placement here",
            key.kind, key.template, key.index
        ));
    }

    /// The module arrived in, as the story left it: its destroyed gone (a
    /// creature's body and the items and scripts made for it with it), its
    /// doors and placeables open or locked as they were, its objects'
    /// locals.
    fn here(&mut self, module: &str) {
        let Some(kept) = self.story.visited.get(&module.to_ascii_lowercase()) else {
            return;
        };
        let roster: Vec<Origin> = kept
            .roster
            .iter()
            .map(|placed| placed.origin().unwrap_or_else(unnamed))
            .collect();
        let moved = (!roster.is_empty()).then(|| renumber(&roster, self.level.placed()));
        let moved = moved.as_ref();
        for key in &kept.destroyed {
            let Some(at) = self.placement(key, moved) else {
                self.unmatched(module, "destroyed", key);
                continue;
            };
            let entity = &mut self.snapshot.entities[at];
            if matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)) {
                let id = entity.id.0;
                self.snapshot.entities.remove(at);
                for item in self.state.items.ids_held_by(id) {
                    self.state.items.remove(&item);
                }
                self.state.pending.retain(|pending| pending.caller.0 != id);
            } else {
                entity.object = Some(ObjectState::GONE);
            }
        }
        for (key, state, lock) in &kept.doors {
            let Some(at) = self.placement(key, moved) else {
                self.unmatched(module, "door or placeable", key);
                continue;
            };
            let object = self.snapshot.entities[at].object.get_or_insert_default();
            (object.state, object.lock) = (*state, *lock);
        }
        for (key, kind, index, value) in &kept.locals {
            let (Some(at), Ok(index)) = (self.placement(key, moved), i32::try_from(*index)) else {
                self.unmatched(module, "local's object", key);
                continue;
            };
            let id = self.snapshot.entities[at].id.0;
            self.state.locals.insert((id, *kind, index), *value);
        }
    }

    /// Each module left, as a stash entry its next arrival reconciles
    /// (`sim::reconcile::apply`): its spawns ran, and its roster is the
    /// story's, each placement the story knows nothing of marked to be
    /// placed anew (`marked`), so placements alike are told apart by their
    /// order as a save's are; a story without the roster names, at their
    /// numbers, only the placements it knows something of. A
    /// creature destroyed is absent; any other object destroyed is gone; a
    /// door or placeable moved stands open or locked as it was; an object
    /// with locals has them under its id. Each of those objects stands
    /// where its placement does once there (the stash knows it only by its
    /// state); every other placement is placed anew.
    fn left(&mut self, here: &Place) {
        let story = self.story;
        let mut stash = self.snapshot.stash.to_vec();
        for (module, kept) in &story.visited {
            if module.eq_ignore_ascii_case(&here.module) {
                continue;
            }
            if !kept.spawns_ran {
                self.lost_facts(module, kept, "its spawns never ran");
                continue;
            }
            let (entities, roster) = self.left_objects(module, kept);
            stash.retain(|stashed| !stashed.place.module.eq_ignore_ascii_case(module));
            stash.push(Stashed {
                // Another digest than any data's: the next arrival takes the
                // module on and reconciles it (`sim::Restamp`).
                place: Place {
                    module: module.clone(),
                    digest: [0; 32],
                    ..here.clone()
                },
                entities,
                spawned: Vec::new(),
                roster: Some(roster),
            });
            self.state.spawned.insert(module.clone());
        }
        stash.sort_by(|a, b| a.place.module.cmp(&b.place.module));
        self.snapshot.stash = stash.into();
    }

    /// The stash entities and roster of `module`, left as `kept` says.
    fn left_objects(
        &mut self,
        module: &str,
        kept: &StoryModule,
    ) -> (Vec<EntityState>, Vec<Origin>) {
        let mut left = Left {
            roster: marked(kept),
            ..Left::default()
        };
        for key in &kept.destroyed {
            match key.origin() {
                Some(origin) if origin.kind == ObjectKind::Creature => left.name(key, origin),
                Some(origin) => {
                    let id = self.next_id();
                    left.object(id, key, origin, ObjectState::GONE);
                }
                None => self.unmatched(module, "destroyed", key),
            }
        }
        for (key, state, lock) in &kept.doors {
            let Some(origin) = key.origin() else {
                self.unmatched(module, "door or placeable", key);
                continue;
            };
            let state = ObjectState {
                state: *state,
                lock: *lock,
                ..ObjectState::default()
            };
            let id = self.next_id();
            left.object(id, key, origin, state);
        }
        let mut creatures = 0;
        for (key, kind, index, value) in &kept.locals {
            let (Some(origin), Ok(index)) = (key.origin(), i32::try_from(*index)) else {
                self.unmatched(module, "local's object", key);
                continue;
            };
            let id = match left.id_of(key) {
                Some(id) => id,
                None if origin.kind == ObjectKind::Creature => {
                    creatures += 1;
                    continue;
                }
                None => {
                    let id = self.next_id();
                    left.object(id, key, origin, ObjectState::default());
                    id
                }
            };
            self.state.locals.insert((id, *kind, index), *value);
        }
        if creatures > 0 {
            self.lost.push(format!(
                "{module}: {creatures} creatures' locals, their creatures placed anew on the next arrival"
            ));
        }
        (left.entities, left.roster)
    }

    /// A line for the facts of `module` lost, and why.
    fn lost_facts(&mut self, module: &str, kept: &StoryModule, why: &str) {
        let facts = kept.destroyed.len() + kept.doors.len() + kept.locals.len();
        if facts > 0 {
            self.lost
                .push(format!("{module}: {facts} facts lost, {why}"));
        }
    }

    /// The party where the story stood: the first seat at its place and
    /// facing, the other seats and the companions as they stand around it,
    /// all moved by as much, then put on the walkable mesh as a reconciled
    /// save is (`sim::reconcile::onto_ground`: the nearest walkable point
    /// within 2 m, else the module's nearest waypoint).
    fn on_ground(&mut self) {
        let Some((from, _)) = self.leader_at() else {
            return;
        };
        let to = sim::math::scale(self.story.place.position, 1.0 / UNIT);
        let shift = sim::math::sub(to, from);
        let facing = self.story.place.facing;
        let party: Vec<u32> = self
            .snapshot
            .entities
            .iter()
            .filter(|entity| matches!(entity.kind, Kind::Avatar(_)))
            .map(|entity| entity.id.0)
            .chain(self.members.iter().map(|(id, _)| *id))
            .collect();
        let (mut moved, rest): (Vec<EntityState>, Vec<EntityState>) =
            std::mem::take(&mut self.snapshot.entities)
                .into_iter()
                .partition(|entity| party.contains(&entity.id.0));
        for entity in &mut moved {
            let Some(player) = entity.player.as_mut() else {
                continue;
            };
            player.origin = sim::math::add(player.origin, shift);
            player.set_view([0.0, facing, 0.0]);
            entity.position = sim::math::scale(player.origin, UNIT);
        }
        let grounding = sim::reconcile::onto_ground(self.level, &mut moved, &self.members);
        self.lost.extend(grounding.lines);
        self.snapshot.entities = rest;
        self.snapshot.entities.extend(moved);
        self.snapshot.entities.sort_by_key(|entity| entity.id);
    }
}
