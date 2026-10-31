//! Installing a module: the commit of the install transaction. The host
//! resolves everything first (the module's facts, its collision: a
//! `Level`), and only then calls `SimWorld::install`, which cannot fail
//! halfway: it builds the whole next state, then swaps the world.
//!
//! What happens, as a pure function of the state before and the level
//! (`assemble`), so a client given the same announcement reaches the same
//! snapshot, bit for bit:
//! - the module being left keeps its objects in the stash (KOTOR keeps a
//!   module as it was left), replacing an older stay there;
//! - the module arrived in takes its objects from the stash when it was
//!   visited before, else fresh from its facts (new ids, in the facts'
//!   order);
//! - each seat's player carries over (its id and seat; the first seat's
//!   switch as the party table has it, `SimWorld::settle_switch`), standing at the
//!   arrival: the door or waypoint the transition names, else the module's
//!   entry, dropped to the floor there, the next seats on free spots
//!   around it; each looks the arrival's way;
//! - the party's companions (creatures following a seat) come along as
//!   objects made in the new module (`snapshot::Spawned`, from their
//!   templates), keeping their ids, fights and sheets, on the next free
//!   spots; the module left does not keep them;
//! - a trigger leading out that the arrival stands in already counts the
//!   player (and a guest's companion, as its seat) as inside, so arriving
//!   never fires it (nor bounces the party back); any other is entered;
//! - the first seat stands at the arrival's spot itself where the walkmesh
//!   there is floor, though a body's box does not fit (KOTOR's walker is a
//!   point: TSL's Peragus wakes the Exile in its kolto tank);
//! - wanderers stay behind (they belong to the placeholder world), the
//!   transition asked for is done, tick, time and random streams go on.

use std::fmt;

use collision::{CollisionWorld, Contents, Extents, UNIT};

use crate::combat::{Fighter, Sheet};
use crate::input::Seat;
use crate::instance::Instances;
use crate::math;
use crate::module::ObjectData;
use crate::module::{ModuleData, ObjectKind, Spot};
use crate::motion::{self, MotionState};
use crate::snapshot::{
    EntityState, Kind, ObjectState, Place, SimId, Snapshot, SnapshotHash, Stashed,
};
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};
use crate::world::{Digests, Level, SimWorld, in_doorway};

/// Why an install (or a restore) did not happen. Nothing changed.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallError {
    /// The level has no module to install.
    NoModule,
    /// A snapshot restored into a level other than its own: the modules
    /// they are ("" for none; the same name when their data differs).
    WrongLevel { snapshot: String, level: String },
    /// A snapshot's object the level's module does not have.
    UnknownObject { id: SimId },
    /// The module was left in a state taken with other data than the
    /// level's (another install, another build).
    StashChanged { module: String },
    /// No room to stand at or near the arrival (metres).
    NoRoom { module: String, at: [f32; 3] },
    /// A seat past the last a world holds (`MAX_SEATS`).
    Seat(Seat),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = |module: &str| {
            if module.is_empty() {
                "no module".to_string()
            } else {
                module.to_string()
            }
        };
        match self {
            InstallError::NoModule => f.write_str("the level has no module"),
            InstallError::WrongLevel { snapshot, level } => write!(
                f,
                "the state was made in {}, the level here is {} (or their data differ)",
                name(snapshot),
                name(level)
            ),
            InstallError::UnknownObject { id } => {
                write!(f, "entity {} is no object of the level's module", id.0)
            }
            InstallError::StashChanged { module } => {
                write!(f, "{module} was left with other module data than this")
            }
            InstallError::NoRoom { module, at } => write!(
                f,
                "{module}: no room to stand near ({:.2}, {:.2}, {:.2})",
                at[0], at[1], at[2]
            ),
            InstallError::Seat(seat) => write!(
                f,
                "seat {}: a world holds seats 0 to {}",
                seat.0,
                crate::world::MAX_SEATS - 1
            ),
        }
    }
}

impl std::error::Error for InstallError {}

/// A module stamped with other data than this build reads it with (its
/// `Place::digest`): a save's own, or one it left behind, taken on with
/// the level's digest (the save migrations design, part 3). kjk reading the
/// same game differently (collision, nav, anims, scripts) or the player's
/// files changing leaves the saved state as right as it was; another game,
/// module or player count is still refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restamp {
    pub module: String,
    pub from: [u8; 32],
    pub to: [u8; 32],
}

impl Restamp {
    /// Whether `stamped` is `here`, or is but for its digest.
    pub fn matches(stamped: &Place, here: &Place) -> bool {
        stamped.game == here.game
            && stamped.module == here.module
            && stamped.players == here.players
    }

    /// `stamped` taken on as `here`: none when it is `here` already or is
    /// another place.
    pub fn of(stamped: &Place, here: &Place) -> Option<Restamp> {
        (Restamp::matches(stamped, here) && stamped.digest != here.digest).then(|| Restamp {
            module: here.module.clone(),
            from: stamped.digest,
            to: here.digest,
        })
    }
}

impl fmt::Display for Restamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let short = |digest: &[u8; 32]| -> String {
            digest[..4]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        };
        write!(
            f,
            "{} data changed since the save ({} → {}), re-stamped",
            self.module,
            short(&self.from),
            short(&self.to)
        )
    }
}

impl SimWorld {
    /// What installing `level` re-stamps: the module's place left behind
    /// with other data, if it was (`assemble` takes it on).
    pub fn stale_stamp(&self, level: &Level) -> Option<Restamp> {
        let here = level.place()?;
        self.stash()
            .iter()
            .find(|stashed| stashed.place.module == here.module)
            .and_then(|stashed| Restamp::of(&stashed.place, &here))
    }

    /// Installs `level`'s module, arriving at the door or waypoint tagged
    /// `arrival` (else the module's entry): see the module documentation.
    /// The world is unchanged on an error.
    pub fn install(
        &mut self,
        level: Level,
        arrival: Option<&str>,
    ) -> Result<Snapshot, InstallError> {
        self.settle_switch();
        let party = &self.script_state().party;
        let level = with_party_templates(level, party);
        // The party table's members' creatures, each with its template: all
        // of them come along, following or not (KOTOR places the whole
        // party at a module's arrival).
        let members: Vec<(u32, String)> = party
            .members
            .iter()
            .filter_map(|npc| {
                let slot = party.available.get(npc)?;
                Some((slot.object?, slot.template.clone()))
            })
            .collect();
        let before = self.snapshot();
        let after = assemble(&before, &level, arrival, Some(self.level()), &members)?;
        let empty = crate::inventory::Catalog::new([]);
        let catalog = |level: &Level| level.scripts().map(|library| library.items().clone());
        let (last, next_catalog) = (catalog(self.level()), catalog(&level));
        let next = SimWorld::from_snapshot(&after, level)?;
        let mut carried = self.take_scripts();
        // What waits in the module left stays with it (its objects: those
        // that did not come along), to go on when the party is back.
        if let Some(place) = &before.place {
            let come: std::collections::BTreeSet<u32> =
                after.entities.iter().map(|entity| entity.id.0).collect();
            let stay: std::collections::BTreeSet<u32> = before
                .entities
                .iter()
                .filter(|entity| {
                    matches!(entity.kind, Kind::Object(..)) && !come.contains(&entity.id.0)
                })
                .map(|entity| entity.id.0)
                .collect();
            carried
                .state
                .leave_module(&place.module, &stay, before.time_ms);
        }
        carried.state.carry_known(
            last.as_ref().unwrap_or(&empty),
            next_catalog.as_ref().unwrap_or(&empty),
        );
        *self = next;
        self.enter_module(carried);
        Ok(after)
    }
}

/// `level` with the creature templates the party table kept for its
/// companions (`party::Npc::template_data`) that its library lacks: a
/// companion made available by a module's own template is made, and
/// carried in, as itself where another module does not ship it (KOTOR's
/// table keeps the creature).
pub(crate) fn with_party_templates(level: Level, party: &crate::script::party::Party) -> Level {
    let Some(library) = level.scripts() else {
        return level;
    };
    let kept: std::collections::BTreeMap<String, std::sync::Arc<crate::module::ObjectData>> = party
        .available
        .values()
        .filter(|npc| library.creature(&npc.template).is_none())
        .filter_map(|npc| {
            let facts = crate::pack::creature_of(
                npc.template_data.as_deref()?,
                crate::wire::Versions::THIS_BUILD,
            )
            .ok()?;
            Some((npc.template.clone(), std::sync::Arc::new(facts)))
        })
        .collect();
    level.with_companions(kept)
}

/// A companion of the party in `before` (a creature following a seat), with
/// its template and sheet from its instance in `old`, the level it was in.
fn companions(before: &Snapshot, old: Option<&Level>) -> Vec<(EntityState, String)> {
    let Some(old) = old else {
        return Vec::new();
    };
    let instances = Instances::in_snapshot(old, before);
    before
        .entities
        .iter()
        .filter(|entity| entity.fighter.is_some_and(|f| f.mind.leader.is_some()))
        .filter_map(|entity| {
            let Kind::Object(ObjectKind::Creature, index) = entity.kind else {
                return None;
            };
            let facts = instances.get(index)?;
            let entity = EntityState {
                sheet: entity.sheet.or(facts.sheet),
                ..*entity
            };
            Some((entity, facts.template.clone()))
        })
        .collect()
}

/// A party member carried into a module: the downed arrive standing at 1
/// HP (I30 P11; an arrival is out of combat), clear of every timed effect.
fn arrived(fighter: Option<Fighter>) -> Option<Fighter> {
    fighter.map(|mut fighter| {
        if fighter.downed() {
            fighter.rise(crate::falls::RISE_HP, 0);
        }
        fighter
    })
}

/// The state after installing `level`'s module over `before`, made in
/// `old` (none for a world's first module), the party table's `members`
/// (each one's creature and template) coming along whether they follow a
/// seat or not: one left behind in the module left, or in another's stash
/// (the dark side's Bastila at her Battle Meditation on the Star Forge,
/// whose lines the ceremony after it says), arrives with the party, as
/// KOTOR places the whole party at a module's arrival.
pub(crate) fn assemble(
    before: &Snapshot,
    level: &Level,
    arrival: Option<&str>,
    old: Option<&Level>,
    members: &[(u32, String)],
) -> Result<Snapshot, InstallError> {
    let module = level.module().cloned().ok_or(InstallError::NoModule)?;
    let place = level.place().ok_or(InstallError::NoModule)?;
    let mut stash = before.stash.to_vec();
    let mut next_id = before.next_id;
    // A minigame's area takes no companions: they wait in the module left
    // (its stash, below) and arrive with the party in the next. Peragus's
    // hangar turret ships its own Atton, out of the troopers' way, to speak
    // its scenes; the party's Atton carried in stood in the hangar, fought
    // the troopers as they came and fell in the turret's fire, and its last
    // scene (turend, his) never began.
    let minigame = level.minigame().is_some();
    let mut companions = if minigame {
        Vec::new()
    } else {
        companions(before, old)
    };
    for (id, template) in members.iter().filter(|_| !minigame) {
        if companions.iter().any(|(c, _)| c.id.0 == *id) {
            continue;
        }
        let here = before
            .entities
            .iter()
            .find(|entity| entity.id.0 == *id)
            .copied();
        let left = here.or_else(|| {
            stash.iter_mut().find_map(|stashed| {
                let at = stashed
                    .entities
                    .iter()
                    .position(|entity| entity.id.0 == *id)?;
                Some(stashed.entities.remove(at))
            })
        });
        if let Some(entity) = left.filter(|entity| entity.fighter.is_some_and(|f| f.alive())) {
            companions.push((entity, template.clone()));
        }
    }
    let following = |entity: &EntityState| companions.iter().any(|(c, _)| c.id == entity.id);
    if let Some(current) = &before.place {
        let objects = before
            .entities
            .iter()
            .filter(|entity| matches!(entity.kind, Kind::Object(..)) && !following(entity))
            .map(|entity| EntityState {
                object: entity.object.map(|object| ObjectState {
                    tenants: 0,
                    ..object
                }),
                ..*entity
            })
            .collect();
        stash.retain(|stashed| stashed.place.module != current.module);
        stash.push(Stashed {
            place: current.clone(),
            entities: objects,
            spawned: before.spawned.clone(),
        });
    }
    // The area as it was left, else its placements anew (new ids, in their
    // order).
    let stashed = stash
        .iter()
        .position(|stashed| stashed.place.module == place.module)
        .map(|at| stash.remove(at));
    // Left with other data than the level's: re-stamped when only the
    // digest moved (`Restamp`), as a save loads after an update.
    if stashed
        .as_ref()
        .is_some_and(|stashed| !Restamp::matches(&stashed.place, &place))
    {
        return Err(InstallError::StashChanged {
            module: place.module,
        });
    }
    let made = stashed.as_ref().map_or(&[][..], |stashed| &stashed.spawned);
    let mut instances = Instances::of(level, made);
    let fresh = stashed.is_none();
    let mut entities = match stashed {
        Some(stashed) => stashed.entities,
        None => instances
            .placements()
            .map(|(index, object)| {
                let id = SimId(next_id);
                next_id += 1;
                placed(id, index, object)
            })
            .collect(),
    };
    stash.sort_by(|a, b| a.place.module.cmp(&b.place.module));

    // The players, where they arrive.
    let states = level.states(entities.iter().filter_map(|entity| match entity.kind {
        Kind::Object(_, index) => entity.object.as_ref().map(|object| (index, object)),
        _ => None,
    }));
    if fresh {
        level.with(states.as_deref(), |world| {
            settle(level, world, &instances, &mut entities)
        });
    }
    let (spot, _) = module.entry_for(arrival);
    let mut avatars: Vec<&EntityState> = before
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Avatar(_)))
        .collect();
    avatars.sort_by_key(|entity| match entity.kind {
        Kind::Avatar(seat) => seat,
        _ => unreachable!("filtered to avatars"),
    });
    let count = avatars.len();
    // The first seat stands where the arrival puts it when the walkmesh
    // there is floor, though a body's box does not fit around it: KOTOR's
    // walker is a point (TSL's Peragus stands the Exile in its kolto tank,
    // and its waking scene moves the body out onto the floor). The other
    // seats stand clear around it.
    let own = (!avatars.is_empty())
        .then(|| {
            let fits = level.with(states.as_deref(), |world| {
                spots(world, &spot, 1, &[], None, 0).is_some()
            });
            let [x, y, z] = spot.position;
            let floor = level.nav()?.height([x, y, z], OWN_FLOOR_M, OWN_FLOOR_M)?;
            (!fits).then(|| [x / UNIT, y / UNIT, floor / UNIT - motion::SOLE_Z])
        })
        .flatten();
    let origins = level
        .with(states.as_deref(), |world| match own {
            Some(own) => spots(world, &spot, count - 1, &[(own, APART)], None, RINGS).map(|rest| {
                let mut all = vec![own];
                all.extend(rest);
                all
            }),
            None => spots(world, &spot, count, &[], None, RINGS),
        })
        .or_else(|| {
            // An arrival whose height is off its floor (Manaan's West
            // Central gives one 50 m below): KOTOR stands it on the
            // walkmesh under it, whatever height the waypoint says.
            let [x, y, z] = spot.position;
            let floor = level
                .nav()?
                .height([x, y, z], ARRIVAL_SEARCH_M, ARRIVAL_SEARCH_M)?;
            let lifted = Spot {
                position: [x, y, floor],
                ..spot
            };
            level.with(states.as_deref(), |world| {
                spots(world, &lifted, count, &[], None, RINGS)
            })
        });
    let origins = origins.ok_or(InstallError::NoRoom {
        module: module.name.clone(),
        at: spot.position,
    })?;
    // The companions follow, made anew in this module from their templates,
    // each where the safe-location search stands it within `PARTY` of the
    // arrival (`PARTY`), clear of the seats, the area's living creatures and
    // the companions before it; none found, on the last seat's place.
    let mut stood: Vec<([f32; 3], f32)> = origins
        .iter()
        .map(|origin| (*origin, crate::world::strikes::humanoid_space(0.0)))
        .chain(entities.iter().filter_map(|entity| {
            let player = entity.player?;
            let alive = entity.fighter.is_none_or(|fighter| fighter.alive());
            let own = instances
                .of_entity(entity)
                .map_or(0.0, |facts| facts.personal_space);
            alive.then_some((player.origin, crate::world::strikes::humanoid_space(own)))
        }))
        .collect();
    let soles = spot.position;
    for (companion, template) in &companions {
        let made = crate::snapshot::Spawned {
            kind: ObjectKind::Creature,
            template: template.to_string(),
            yaw: spot.yaw,
            holds: Vec::new(),
        };
        let index = instances.make(level, made);
        let space = crate::world::strikes::humanoid_space(
            instances
                .get(index)
                .map_or(0.0, |facts| facts.personal_space),
        );
        let origin = level
            .with(states.as_deref(), |world| {
                stand_among(level, world, soles, &stood, (space, PARTY))
            })
            .or_else(|| origins.last().copied())
            .ok_or(InstallError::NoRoom {
                module: module.name.clone(),
                at: spot.position,
            })?;
        stood.push((origin, space));
        let mut player = MotionState::standing(origin, spot.yaw);
        // One a guest drives turns from its seat's commands, as a player.
        if companion.fighter.is_some_and(|f| f.mind.driver.is_some())
            && let Some(old) = companion.player
        {
            player.command_angles = old.command_angles;
        }
        player.set_view([0.0, spot.yaw, 0.0]);
        entities.push(EntityState {
            kind: Kind::Object(ObjectKind::Creature, index),
            position: math::scale(origin, UNIT),
            velocity: [0.0; 3],
            player: Some(player),
            object: Some(ObjectState::default()),
            fighter: arrived(companion.fighter),
            ..*companion
        });
    }
    for (avatar, origin) in avatars.iter().zip(origins.iter().copied()) {
        let old = avatar.player.unwrap_or_else(|| {
            MotionState::standing(math::scale(avatar.position, 1.0 / UNIT), 0.0)
        });
        let mut player = MotionState::standing(origin, spot.yaw);
        player.command_angles = old.command_angles;
        player.set_view([0.0, spot.yaw, 0.0]);
        // The player's fight and sheet carry over.
        entities.push(EntityState {
            id: avatar.id,
            kind: avatar.kind,
            position: math::scale(origin, UNIT),
            velocity: [0.0; 3],
            player: Some(player),
            retarget_ms: 0,
            life_ms: None,
            going: None,
            object: None,
            fighter: arrived(avatar.fighter),
            sheet: avatar.sheet,
            missile: None,
        });
    }
    // A seat playing a creature (`world::switch`): that creature, the
    // scripts' PC, arrives where its seat's player does, and the seat's own
    // benched character stands in the creature's place: TSL's Atton, played
    // from the cantina back to the Ebon Hawk, stands on the waypoint whose
    // trigger asks whether the PC entered it (`k_301_attonwarn2`).
    let driven: Vec<(usize, usize)> = entities
        .iter()
        .enumerate()
        .filter_map(|(at, entity)| match entity.kind {
            Kind::Avatar(seat) => entities
                .iter()
                .position(|other| {
                    matches!(other.kind, Kind::Object(..))
                        && other.fighter.and_then(|f| f.mind.driver) == Some(seat)
                })
                .map(|creature| (at, creature)),
            _ => None,
        })
        .collect();
    for (avatar, creature) in driven {
        let (a, c) = (entities[avatar], entities[creature]);
        let swapped = |entity: EntityState, with: EntityState| {
            let player = entity.player.zip(with.player).map(|(mut player, with)| {
                player.origin = with.origin;
                player
            });
            EntityState {
                position: with.position,
                player,
                ..entity
            }
        };
        entities[avatar] = swapped(a, c);
        entities[creature] = swapped(c, a);
    }
    // Arriving inside a trigger that leads out, or in an open door's
    // doorway, is not entering it (no bounce back): for each seat's player,
    // and the companion a guest drives as its seat (`world::drivers`). Any
    // other trigger the party arrives in is entered, as reone's first move
    // in it enters it (TSL's Peragus wakes the Exile so: its `Awaken`
    // trigger lies on the module's entry, in the kolto tank).
    let seats: Vec<(u64, MotionState)> = entities
        .iter()
        .filter_map(|entity| {
            let seat = match entity.kind {
                Kind::Avatar(seat) => seat,
                _ => entity.fighter?.mind.driver?,
            };
            Some((crate::party::bit(seat), entity.player?))
        })
        .collect();
    for entity in &mut entities {
        let (Kind::Object(kind, index), Some(object)) = (entity.kind, &mut entity.object) else {
            continue;
        };
        let Some(facts) = instances.get(index) else {
            continue;
        };
        for (bit, player) in &seats {
            let inside = match kind {
                ObjectKind::Trigger => {
                    facts.exit().is_some()
                        && level.in_trigger(facts, crate::math::scale(player.origin, UNIT))
                }
                ObjectKind::Door => {
                    object.state != 0
                        && facts.exit().is_some()
                        && in_doorway(level, index, player.origin, player.maxs_z)
                }
                _ => false,
            };
            if inside {
                object.tenants |= bit;
            }
        }
    }
    entities.sort_by_key(|entity| entity.id);
    Ok(Snapshot {
        tick: before.tick,
        time_ms: before.time_ms,
        place: Some(place),
        entities,
        transition: None,
        stash: stash.into(),
        // Conversations end with the module left; custom tokens stay.
        talk: crate::script::dialogue::Told {
            tokens: before.talk.tokens.clone(),
            ..Default::default()
        },
        // The party keeps its gold and items; trades end (`enter_module`),
        // and what lies on the floor is the area left's.
        goods: crate::inventory::Goods {
            trades: Vec::new(),
            floor: Vec::new(),
            ..before.goods.clone()
        },
        // The new area's own sound until its scripts set any.
        area_sound: crate::snapshot::AreaSound::default(),
        // A module loads under black, lifted once its arrival's scripts have
        // run (`snapshot::WorldFadeIn`).
        fade: Some(crate::snapshot::Fade::level_load(before.time_ms)),
        world_fade_in: crate::snapshot::WorldFadeIn::AfterArrival,
        // On until a script disables it ("apply indefinitely").
        video_effect: before.video_effect,
        progress: before.progress.clone(),
        personas: before.personas.clone(),
        // A minigame's area starts its course; any other has none.
        minigame: level
            .minigame()
            .map(|rules| crate::minigame::Course::new(rules)),
        // Matches end with the module left (`State::leave_matches`, in
        // `enter_module`).
        pazaak: Vec::new(),
        // The party keeps what it explored, of every area.
        explored: before.explored.clone(),
        // The module's visuals stay with it.
        visuals: Vec::new(),
        // What was drawn going stays behind with the area left.
        departing: Vec::new(),
        // The party table's map carries on; the next snapshot tells the new
        // area's return button.
        screens: before.screens,
        // The droids are made again by the party in the new area.
        remotes: Vec::new(),
        // An arrival is never lost.
        lost: None,
        // A new area: no fight, no break, no proposal (`world::party`).
        in_combat: false,
        paused_by: None,
        ready: None,
        // The companions stay kept for their seats.
        kept: before.kept.clone(),
        // The party table names the leader's body again once the world
        // stands (`world::lent`).
        leading: None,
        spawned: instances.made().to_vec(),
        shared: before.shared,
        authority: before.authority,
        next_id,
        seats_held: before.seats_held,
    })
}

/// The module's creatures stood where KOTOR's area loader stands them, one
/// after another: each GIT place goes through the jump's safe-location
/// search (`jump_place`: on the walkmesh, clear of the creatures stood
/// before it by both personal spaces, a straight walk back to its place),
/// and stays as the GIT writes it where none is found (K1's loader, as TSL's
/// named one).
fn settle(
    level: &Level,
    world: &dyn CollisionWorld,
    instances: &Instances,
    entities: &mut [EntityState],
) {
    let spaces: Vec<f32> = instances
        .placements()
        .map(|(_, object)| crate::world::strikes::humanoid_space(object.personal_space))
        .collect();
    let space = |index: u32| {
        let own = spaces.get(index as usize).copied();
        own.unwrap_or_else(|| crate::world::strikes::humanoid_space(0.0))
    };
    let mut stood: Vec<([f32; 3], f32)> = Vec::new();
    for entity in entities.iter_mut() {
        let Kind::Object(ObjectKind::Creature, index) = entity.kind else {
            continue;
        };
        let Some(player) = entity.player.as_mut() else {
            continue;
        };
        let mine = space(index);
        let soles = {
            let [x, y, z] = math::scale(player.origin, UNIT);
            [x, y, z + motion::SOLE_Z * UNIT]
        };
        if let Some(place) = stand_among(level, world, soles, &stood, (mine, JUMP)) {
            player.origin = place;
            entity.position = math::scale(place, UNIT);
        }
        stood.push((player.origin, mine));
    }
}

/// Placement `index` (`object`) standing as the module puts it, as entity
/// `id`: a creature stands up as a character where the GIT puts it, facing
/// its way, as hurt as its template has it (short of its full by what its
/// `CurrentHitPoints` is under its most, never below 1).
fn placed(id: SimId, index: u32, object: &ObjectData) -> EntityState {
    let (player, fighter) = if object.kind == ObjectKind::Creature {
        let sheet = object.sheet.unwrap_or_else(Sheet::commoner);
        let mut origin = math::scale(object.position, 1.0 / UNIT);
        origin[2] += 0.125 - motion::SOLE_Z;
        (
            Some(MotionState::standing(origin, object.yaw)),
            Some(Fighter::of(&sheet).hurt_as(object.hit_points)),
        )
    } else {
        (None, None)
    };
    EntityState {
        id,
        kind: Kind::Object(object.kind, index),
        position: player.map_or(object.position, |player| math::scale(player.origin, UNIT)),
        velocity: [0.0; 3],
        player,
        retarget_ms: 0,
        life_ms: None,
        going: None,
        object: Some(ObjectState {
            state: object.state,
            lock: crate::snapshot::lock::PLACED,
            ..ObjectState::default()
        }),
        fighter,
        sheet: None,
        missile: None,
    }
}

/// How far down from a spot the floor is looked for (units), and how far
/// apart the players stand.
const FLOOR_BELOW: f32 = 64.0;
pub(crate) const APART: f32 = 36.0;
/// Rings of candidate spots around the arrival: this many, this far apart
/// (metres).
pub(crate) const RINGS: usize = 8;
const RING_STEP: f32 = 0.4;
/// How many heights the drop is tried from, a step apart: a step above
/// the spot first, then higher, for a spot buried under the floor.
const LIFTS: usize = 4;
/// How far up or down (metres) an arrival with no room where its waypoint
/// says is looked for on the walkmesh under it.
const ARRIVAL_SEARCH_M: f32 = 100.0;
/// How far from an arrival's height its own floor is looked for (metres):
/// the first seat stands on it though its box does not fit (`assemble`).
const OWN_FLOOR_M: f32 = 0.5;

/// `count` places to stand (box origins, units) at or near `spot`, none
/// within `APART` of another or within its clearance (units) of each of
/// those `taken`, and with `seen_from` (a
/// box origin, units) each in its sight; the first as near as it can be: the spot, then `rings` rings of spots around it,
/// each dropped from a step's height onto the floor below; a place whose
/// box starts in a solid, finds no floor or a slope too steep to stand on
/// is passed over. Where that leaves too few, the same spots are dropped
/// from a step higher, and so on (`LIFTS`): KOTOR stands an arrival on its
/// walkmesh, which may lie under the floor drawn there by more than a step
/// (K1's swoop track draws its floor half a metre above the walkmesh its
/// entry is on).
pub(crate) fn spots(
    world: &dyn CollisionWorld,
    spot: &Spot,
    count: usize,
    taken: &[([f32; 3], f32)],
    seen_from: Option<[f32; 3]>,
    rings: usize,
) -> Option<Vec<[f32; 3]>> {
    let box_extents = Extents {
        mins: [
            -motion::HALF_WIDTH * UNIT,
            -motion::HALF_WIDTH * UNIT,
            motion::SOLE_Z * UNIT,
        ],
        maxs: [
            motion::HALF_WIDTH * UNIT,
            motion::HALF_WIDTH * UNIT,
            motion::STAND_HEIGHT * UNIT,
        ],
    };
    let mut found: Vec<[f32; 3]> = Vec::new();
    let mut candidates = vec![spot.position];
    for ring in 1..=rings {
        let around = 8 * ring;
        let radius = ring as f32 * RING_STEP;
        for k in 0..around {
            let (sin, cos) = math::sin_cos_deg(spot.yaw + 360.0 * k as f32 / around as f32);
            let [x, y, z] = spot.position;
            candidates.push([x + radius * cos, y + radius * sin, z]);
        }
    }
    let drops = (1..=LIFTS).flat_map(|lift| candidates.iter().map(move |soles| (lift, soles)));
    for (lift, soles) in drops {
        if found.len() == count {
            break;
        }
        let mut origin = math::scale(*soles, 1.0 / UNIT);
        origin[2] -= motion::SOLE_Z;
        let from = [
            origin[0],
            origin[1],
            origin[2] + motion::STEP_HEIGHT * lift as f32,
        ];
        let to = [origin[0], origin[1], origin[2] - FLOOR_BELOW];
        let trace = world.trace(
            math::scale(from, UNIT),
            math::scale(to, UNIT),
            &box_extents,
            Contents::PLAYER_SOLID,
        );
        if trace.start_solid
            || trace.fraction >= 1.0
            || trace.plane.normal[2] < crate::motion::WALKABLE_NORMAL_Z
        {
            continue;
        }
        let stand = math::scale(trace.end, 1.0 / UNIT);
        let clear = found
            .iter()
            .map(|other| (other, APART))
            .chain(taken.iter().map(|(other, apart)| (other, *apart)))
            .all(|(other, apart)| {
                let (dx, dy) = (other[0] - stand[0], other[1] - stand[1]);
                dx * dx + dy * dy >= apart * apart
            });
        let seen = seen_from.is_none_or(|eye| {
            let sight = world.trace(
                math::scale(eye, UNIT),
                trace.end,
                &Extents::POINT,
                Contents::SOLID,
            );
            crate::world::unobstructed(&sight)
        });
        if clear && seen {
            found.push(stand);
        }
    }
    (found.len() == count).then_some(found)
}

/// How far a safe-location search looks around its mark (`jump_place`):
/// its rings' reach (metres), and whether a ring's place needs a straight
/// walk on the walkmesh back to the mark.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Reach {
    metres: f32,
    walks_back: bool,
}

/// A script's jump, a GIT creature's or a script-made one's place, with
/// the walk back.
pub(crate) const JUMP: Reach = Reach {
    metres: rules::scripts::jumps::SEARCH_M,
    walks_back: true,
};
/// The party's members around their leader as they arrive, with no walk
/// back.
pub(crate) const PARTY: Reach = Reach {
    metres: rules::scripts::jumps::PARTY_M,
    walks_back: false,
};
/// The half width (metres) of a search's first ring.
const JUMP_FIRST_RING_M: f32 = rules::scripts::jumps::RING_M;
/// How far apart (metres) a jump's straight walk back to its mark is
/// looked at, and how far over or under each point its walkmesh may lie.
const JUMP_LINE_STEP_M: f32 = 0.25;
const JUMP_LINE_DROP_M: f32 = 0.5;
/// Half the width (metres) of the column a jump's place is tested with:
/// KOTOR's walker is a point.
pub const WALKER_HALF_WIDTH_M: f32 = 0.01;
/// How far over or under a jump's mark (metres) its walkmesh is looked for:
/// the whole column, the executables taking the walkmesh's height under the
/// mark whatever the mark's own (Manaan's courtroom waypoints the
/// ambassadors walk lie 50 m under their floor, as West Central's arrival
/// does, `ARRIVAL_SEARCH_M`).
const JUMP_COLUMN_M: f32 = f32::INFINITY;
/// How many steps above and below a place's walkmesh its floor is looked
/// for (the collision's floor may lie off the walkmesh's height).
const JUMP_FLOOR_STEPS: f32 = 2.0;

/// Where the safe-location search (`jump_place`) stands a body of personal
/// space `space` (metres) put at `to` (soles, metres), clear of the bodies
/// `stood` (box origins, units, and their personal spaces, metres) by both
/// spaces: the area loader's and a script's made creatures', one after
/// another.
pub(crate) fn stand_among(
    level: &Level,
    world: &dyn CollisionWorld,
    to: [f32; 3],
    stood: &[([f32; 3], f32)],
    (space, reach): (f32, Reach),
) -> Option<[f32; 3]> {
    let taken: Vec<([f32; 3], f32)> = stood
        .iter()
        .map(|(origin, theirs)| (*origin, (space + theirs) / UNIT))
        .collect();
    jump_place(level, world, to, &taken, (space, reach))
}

/// Whether `level`'s walkmesh lies anywhere under or over `to` (soles,
/// metres): a jump's mark the safe-location search stands on.
#[cfg(feature = "probes")]
pub(crate) fn on_walkmesh(level: &Level, to: [f32; 3]) -> bool {
    level
        .nav()
        .is_none_or(|nav| nav.height(to, JUMP_COLUMN_M, JUMP_COLUMN_M).is_some())
}

/// Where a script's jump to `to` (soles, metres) puts its jumper (a box
/// origin, units), as the K1 and TSL executables' safe-location search
/// does. Each place tried stands on the walkmesh under its x and y, its
/// height the walkmesh's there (a mark's own height only chooses among
/// floors over one another): the mark itself when it is free, else the
/// first free place of square rings about it, the first 1 m from it each
/// way, each next wider by the jumper's personal space (`space`, metres),
/// out to `reach` (`JUMP`'s for a jump), each ring's edge tried every `space`
/// along it, its two rows across by x, then its two sides. Free is a body's
/// height of room in a column 2 cm wide over a walkable floor, clear of the
/// bodies `taken` (box origins and clearances, units); where the mark itself
/// is on the walkmesh and `reach` walks back, a ring's place must also have
/// a straight walk on the walkmesh to it, so no jump lands beyond a wall.
/// No free place, no jump: the jumper stays.
pub(crate) fn jump_place(
    level: &Level,
    world: &dyn CollisionWorld,
    to: [f32; 3],
    taken: &[([f32; 3], f32)],
    (space, reach): (f32, Reach),
) -> Option<[f32; 3]> {
    // The executables' test is a column 2 cm wide, a body high: KOTOR's
    // walker is a point, its bodies kept apart by their personal spaces.
    let half_width = WALKER_HALF_WIDTH_M;
    let box_extents = Extents {
        mins: [-half_width, -half_width, motion::SOLE_Z * UNIT],
        maxs: [half_width, half_width, motion::STAND_HEIGHT * UNIT],
    };
    let nav = level.nav();
    let height = |[x, y, z]: [f32; 3]| match nav {
        Some(nav) => nav.height([x, y, z], JUMP_COLUMN_M, JUMP_COLUMN_M),
        None => Some(z),
    };
    let origin_of = |soles: [f32; 3]| {
        let mut origin = math::scale(soles, 1.0 / UNIT);
        origin[2] -= motion::SOLE_Z;
        origin
    };
    let floored = |soles: [f32; 3]| -> Option<[f32; 3]> {
        let origin = origin_of([soles[0], soles[1], height(soles)?]);
        let reach = motion::STEP_HEIGHT * JUMP_FLOOR_STEPS;
        let from = [origin[0], origin[1], origin[2] + motion::STEP_HEIGHT];
        let below = [origin[0], origin[1], origin[2] - reach];
        let trace = world.trace(
            math::scale(from, UNIT),
            math::scale(below, UNIT),
            &box_extents,
            Contents::PLAYER_SOLID,
        );
        let stands = !trace.start_solid
            && trace.fraction < 1.0
            && trace.plane.normal[2] >= motion::WALKABLE_NORMAL_Z;
        let stand = math::scale(trace.end, 1.0 / UNIT);
        let clear = taken.iter().all(|(other, apart)| {
            let (dx, dy) = (other[0] - stand[0], other[1] - stand[1]);
            dx * dx + dy * dy >= apart * apart
        });
        (stands && clear).then_some(stand)
    };
    let mark = height(to).map(|z| [to[0], to[1], z]);
    if let Some(stand) = mark.and_then(floored) {
        return Some(stand);
    }
    // The rings' places look for their floor from the mark's (its own
    // height where the walkmesh has none under it).
    let centre = mark.unwrap_or(to);
    // A straight walk on the walkmesh from a place to the mark: every
    // `JUMP_LINE_STEP_M` along it over a face (the executables' line test is
    // the walkmesh's, a point's).
    let walks_to_mark = |stand: [f32; 3]| {
        let (Some(nav), Some(mark), true) = (nav, mark, reach.walks_back) else {
            return true;
        };
        let from = [
            stand[0] * UNIT,
            stand[1] * UNIT,
            (stand[2] + motion::SOLE_Z) * UNIT,
        ];
        let way = math::sub(mark, from);
        let steps = ((math::flat_length(way) / JUMP_LINE_STEP_M).ceil() as u32).max(1);
        (1..=steps).all(|k| {
            let at = math::mad(from, k as f32 / steps as f32, way);
            nav.locate(at, JUMP_LINE_DROP_M, JUMP_LINE_DROP_M).is_some()
        })
    };
    let step = space.max(0.1);
    let mut half = JUMP_FIRST_RING_M;
    while half <= reach.metres {
        let across = (2.0 * half / step).floor() as usize;
        let along = move |k: usize| -half + k as f32 * step;
        let rows = [half, -half]
            .into_iter()
            .flat_map(move |y| (0..=across).map(move |k| (along(k), y)));
        let sides = [-half, half]
            .into_iter()
            .flat_map(move |x| (1..across).map(move |k| (x, along(k))));
        for (dx, dy) in rows.chain(sides) {
            let Some(stand) = floored([centre[0] + dx, centre[1] + dy, centre[2]]) else {
                continue;
            };
            if walks_to_mark(stand) {
                return Some(stand);
            }
        }
        half += step;
    }
    None
}

const MAGIC: &[u8; 4] = b"KJKA";
/// 2: the seats installed with. 4: the players planned for (I30 P21).
const VERSION: u32 = 4 + crate::pending::INSTALL;

/// The host's word that it installed a module: everything a client needs
/// to install the same one from its own install and reach the same state,
/// and what must match. Plain values, one byte encoding (`KJKA` v2); I15
/// carries it over the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// `k1` or `tsl`.
    pub game: String,
    pub module: String,
    /// The door or waypoint arrived at; empty for the module's entry.
    pub arrival: String,
    /// Installed after this tick's step (before the next).
    pub tick: u64,
    /// The players installed with: a client plans its arrival for as many.
    pub seats: u8,
    /// The players the module was planned for (`ModuleData::players`, I30
    /// P21): a client plans its encounters for as many.
    pub players: u8,
    pub digests: Digests,
    /// The snapshot's hash right after the install.
    pub after: SnapshotHash,
}

/// What differs between an announcement and a client's own level or
/// state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    NoModule,
    Game {
        host: String,
        here: String,
    },
    Module {
        host: String,
        here: String,
    },
    /// The module's facts differ (other game data, another build).
    ModuleData,
    Collision {
        host: String,
        here: String,
    },
    Animations,
    /// The navigation meshes differ (or one side has none).
    Nav,
    /// The same level, but another state after installing it (the state
    /// before already differed).
    State {
        host: SnapshotHash,
        here: SnapshotHash,
    },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let short = |digest: &str| digest.chars().take(16).collect::<String>();
        match self {
            Mismatch::NoModule => f.write_str("the level here has no module"),
            Mismatch::Game { host, here } => write!(f, "game: host {host}, here {here}"),
            Mismatch::Module { host, here } => write!(f, "module: host {host}, here {here}"),
            Mismatch::ModuleData => f.write_str("the module's data differs"),
            Mismatch::Collision { host, here } => {
                write!(f, "collision: host {}, here {}", short(host), short(here))
            }
            Mismatch::Animations => f.write_str("the animation lengths differ"),
            Mismatch::Nav => f.write_str("the navigation meshes differ"),
            Mismatch::State { host, here } => write!(
                f,
                "state after the install: host {}, here {}",
                host.short(),
                here.short()
            ),
        }
    }
}

impl Announcement {
    /// What a host announces after installing `level` (arriving at
    /// `arrival`) after tick `after.tick`.
    pub fn new(level: &Level, arrival: Option<&str>, after: &Snapshot) -> Option<Announcement> {
        let module = level.module()?;
        Some(Announcement {
            game: module.game.clone(),
            module: module.name.clone(),
            arrival: arrival.unwrap_or("").to_string(),
            tick: after.tick,
            seats: after.seats(),
            players: module.players,
            digests: level.digests().clone(),
            after: after.hash(),
        })
    }

    /// The arrival, as `SimWorld::install` takes it.
    pub fn arrival(&self) -> Option<&str> {
        (!self.arrival.is_empty()).then_some(self.arrival.as_str())
    }

    /// Whether `level` (built here from this announcement) is the host's.
    pub fn check(&self, level: &Level) -> Result<(), Mismatch> {
        let module = level.module().ok_or(Mismatch::NoModule)?;
        if module.game != self.game {
            return Err(Mismatch::Game {
                host: self.game.clone(),
                here: module.game.clone(),
            });
        }
        if module.name != self.module {
            return Err(Mismatch::Module {
                host: self.module.clone(),
                here: module.name.clone(),
            });
        }
        let here = level.digests();
        if here.module != self.digests.module {
            return Err(Mismatch::ModuleData);
        }
        if here.collision != self.digests.collision {
            return Err(Mismatch::Collision {
                host: self.digests.collision.clone(),
                here: here.collision.clone(),
            });
        }
        if here.anims != self.digests.anims {
            return Err(Mismatch::Animations);
        }
        if here.nav != self.digests.nav {
            return Err(Mismatch::Nav);
        }
        Ok(())
    }

    /// Whether the state reached here after installing is the host's.
    pub fn check_after(&self, after: &Snapshot) -> Result<(), Mismatch> {
        let here = after.hash();
        if here == self.after {
            Ok(())
        } else {
            Err(Mismatch::State {
                host: self.after,
                here,
            })
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Writer::default();
        self.write(&mut out);
        out.0
    }

    pub(crate) fn write(&self, out: &mut Writer) {
        out.bytes(MAGIC);
        out.u32(VERSION);
        out.string(&self.game);
        out.string(&self.module);
        out.string(&self.arrival);
        out.u64(self.tick);
        out.u8(self.seats);
        out.u8(self.players);
        out.bytes(&self.digests.module);
        out.string(&self.digests.collision);
        out.bytes(&self.digests.anims);
        write_fixed(out, self.digests.nav.as_ref(), &[0; 32], |nav, out| {
            out.bytes(nav)
        });
        out.bytes(&self.after.0);
    }

    pub fn decode(bytes: &[u8]) -> Result<Announcement, DecodeError> {
        let mut input = Reader::new("announcement", bytes);
        let announcement = Announcement::read(&mut input)?;
        input.finish()?;
        Ok(announcement)
    }

    pub(crate) fn read(input: &mut Reader) -> Result<Announcement, DecodeError> {
        input.header(MAGIC, VERSION)?;
        Ok(Announcement {
            game: input.string()?,
            module: input.string()?,
            arrival: input.string()?,
            tick: input.u64()?,
            seats: input.u8()?,
            players: input.u8()?,
            digests: Digests {
                module: input.array()?,
                collision: input.string()?,
                anims: input.array()?,
                nav: read_fixed(input, &[0; 32], |input| input.array(), "nav flag")?,
            },
            after: SnapshotHash(input.array()?),
        })
    }
}

/// A module's facts, counted by kind (for reports).
pub fn counts(module: &ModuleData) -> Vec<(&'static str, usize)> {
    ObjectKind::ALL
        .iter()
        .map(|kind| (kind.name(), module.count(*kind)))
        .collect()
}
