//! A world made, restored from a snapshot, and taken as one: what every
//! change to the snapshot's format touches.

use super::*;

pub(super) type EntityRow = (
    &'static Id,
    Option<&'static Motion>,
    Option<&'static Player>,
    Option<&'static Avatar>,
    Option<&'static Wanderer>,
    Option<&'static Lifetime>,
    Option<&'static Object>,
    Option<&'static Placed>,
    Option<&'static Fighting>,
    Option<&'static PlayerSheet>,
    Option<&'static Bolt>,
    Option<&'static Benched>,
);

/// An entity's state as plain values at `time_ms`.
pub(super) fn entity_state(
    row: bevy_ecs::query::QueryItem<'_, '_, EntityRow>,
    time_ms: u64,
) -> EntityState {
    let (
        id,
        motion,
        player,
        avatar,
        wanderer,
        life,
        object,
        placed,
        fighting,
        sheet,
        bolt,
        benched,
    ) = row;
    let player = player.map(|player| player.0);
    let (position, velocity) = match (&player, motion, placed, bolt) {
        (Some(player), ..) => (scale(player.origin, UNIT), scale(player.velocity, UNIT)),
        (None, Some(motion), ..) => (motion.position, motion.velocity),
        (None, None, Some(placed), _) => (placed.0, [0.0; 3]),
        (None, None, None, Some(bolt)) => (
            scale(bolt.0.at(time_ms), UNIT),
            scale(bolt.0.velocity, UNIT),
        ),
        (None, None, None, None) => ([0.0; 3], [0.0; 3]),
    };
    EntityState {
        id: id.0,
        // A driven companion stays its object (its driver is
        // in its mind); a benched character its seat's avatar.
        kind: match (avatar.map(|a| a.0).or(benched.map(|b| b.0)), object, bolt) {
            (_, Some(object), _) => Kind::Object(object.kind, object.index),
            (Some(seat), None, _) => Kind::Avatar(seat),
            (None, None, Some(_)) => Kind::Missile,
            (None, None, None) => Kind::Wanderer,
        },
        position,
        velocity,
        player,
        retarget_ms: wanderer.map_or(0, |wanderer| wanderer.retarget_ms),
        life_ms: life.map(|life| life.left_ms),
        going: life.and_then(|life| life.going),
        object: object.map(|object| object.state),
        fighter: fighting.map(|fighting| fighting.0),
        sheet: sheet.map(|sheet| sheet.0),
        missile: bolt.map(|bolt| bolt.0),
    }
}

/// Deterministic ring-spawn offset in metres for `seat` out of `total_seats`.
/// Preserves the legacy linear X spacing (`seat * 2.0`) when `total_seats <= 4`
/// so 1–4 player worlds remain bit-identical.
pub(crate) fn spawn_offset_m(seat: u8, total_seats: u8) -> [f32; 2] {
    if total_seats <= 4 {
        return [f32::from(seat) * 2.0, 0.0];
    }
    let rings: [(u8, f32); 4] = [(8, 1.1), (16, 2.2), (20, 3.0), (20, 3.8)];
    let mut base = 0u8;
    for (cap, radius) in rings {
        let end = base.saturating_add(cap);
        if seat < end {
            let idx = seat - base;
            let count = cap.min(total_seats.saturating_sub(base)).max(1);
            let angle = std::f32::consts::TAU * f32::from(idx) / f32::from(count);
            let (sin, cos) = collision::trig::sin_cos(angle);
            return [radius * cos, radius * sin];
        }
        base = end;
    }
    let idx = seat.saturating_sub(base);
    let angle =
        std::f32::consts::TAU * f32::from(idx) / f32::from(total_seats.saturating_sub(base).max(1));
    let (sin, cos) = collision::trig::sin_cos(angle);
    [4.6 * cos, 4.6 * sin]
}

impl SimWorld {
    /// A new world: the players at `config.spawn`, or in a module's level
    /// at its entry, with the module's objects. Refused with more seats
    /// than `MAX_SEATS`, or when a module's entry has no room for them.
    pub fn new(config: WorldConfig, level: Level) -> Result<Self, InstallError> {
        if config.seats > MAX_SEATS {
            return Err(InstallError::Seat(Seat(config.seats - 1)));
        }
        let mut authority = SimRng::new(config.seed, 1);
        let mut entities = Vec::new();
        let mut next_id = 0;
        // Under `Guests::Companions` only seat 0 has a player of its own.
        let players = match config.guests {
            Guests::Avatars => config.seats,
            Guests::Companions => config.seats.min(1),
        };
        for seat in 0..players {
            let [x, y, z] = config.spawn;
            let [dx, dy] = spawn_offset_m(seat, config.seats);
            let soles = [x + dx, y + dy, z];
            let mut origin = scale(soles, 1.0 / UNIT);
            // Standing where a landing leaves the box: an eighth of a unit
            // above the floor (`CONTACT_GAP`), not touching it,
            // which traces as inside it.
            origin[2] += 0.125 - motion::SOLE_Z;
            let player = MotionState::standing(origin, config.yaw);
            let sheet = config.sheet;
            entities.push(EntityState {
                id: SimId(next_id),
                kind: Kind::Avatar(Seat(seat)),
                position: scale(player.origin, UNIT),
                velocity: [0.0; 3],
                player: Some(player),
                retarget_ms: 0,
                life_ms: None,
                going: None,
                object: None,
                fighter: Some(Fighter::of(&sheet)),
                sheet: Some(sheet),
                missile: None,
            });
            next_id += 1;
        }
        for _ in 0..config.wanderers {
            entities.push(EntityState {
                id: SimId(next_id),
                kind: Kind::Wanderer,
                position: [
                    authority.range(-ARENA, ARENA),
                    authority.range(-ARENA, ARENA),
                    0.0,
                ],
                velocity: [0.0; 3],
                player: None,
                retarget_ms: 0,
                life_ms: None,
                going: None,
                object: None,
                fighter: None,
                sheet: None,
                missile: None,
            });
            next_id += 1;
        }
        let start = Snapshot {
            tick: 0,
            time_ms: 0,
            place: None,
            entities,
            transition: None,
            stash: Stash::default(),
            talk: Told::default(),
            goods: Arc::default(),
            area_sound: crate::snapshot::AreaSound::default(),
            fade: None,
            world_fade_in: crate::snapshot::WorldFadeIn::Done,
            video_effect: None,
            spawned: Arc::default(),
            roster: None,
            progress: Arc::default(),
            personas: Arc::default(),
            lost: None,
            in_combat: false,
            paused_by: None,
            ready: None,
            kept: Vec::new(),
            leading: None,
            solo: false,
            minigame: None,
            pazaak: Vec::new(),
            explored: Arc::default(),
            visuals: Vec::new(),
            departing: Vec::new(),
            screens: Default::default(),
            remotes: Vec::new(),
            puppets: Vec::new(),
            standing: Default::default(),
            saber_rules: Default::default(),
            targeted_blocks: false,
            waiting: Vec::new(),
            damage_taken: Default::default(),
            cheats: Default::default(),
            shared: SimRng::new(config.seed, 0),
            authority,
            next_id,
            seats_held: config.seats,
        };
        let start = if level.module.is_some() {
            crate::install::assemble(&start, &level, None, None, &[], &Default::default(), None)?.0
        } else {
            start
        };
        let mut world = SimWorld::from_snapshot(&start, level)?;
        let carried = world.take_scripts();
        world.enter_module(carried);
        Ok(world)
    }

    /// A world in exactly the state `snapshot` describes, in `level`,
    /// which must be the level it was taken in.
    ///
    /// What the snapshot names is its module (`place`), checked here: a
    /// module's snapshot restores only in that module's level, a snapshot
    /// without one only in a level without one. Which module-less level
    /// (the arena, the course) the snapshot says nothing of; a recording
    /// or save carries that (`LevelId`), checked by whoever restores it.
    /// Refused when it holds more seats than `MAX_SEATS`, or a player or
    /// driver's seat is past them.
    pub fn from_snapshot(snapshot: &Snapshot, level: Level) -> Result<Self, InstallError> {
        let instances = validate_snapshot(snapshot, &level)?;
        let mut world = World::new();
        world.register_disabling_component::<Away>();
        world.init_resource::<MadeFaces>();
        populate_resources(&mut world, snapshot, &level, &instances);
        world.insert_resource(Static(level));
        let driving = driving_seats(snapshot);
        let game = world.resource::<Static>().0.game();
        let tsl = game == crate::combat::passive::Game::Tsl;
        let away = switch::leaves_area(game);
        for entity in &snapshot.entities {
            let mut spawned = world.spawn(Id(entity.id));
            sync_entity(&mut spawned, entity, &instances, &driving, tsl, away);
        }
        let schedule = build_schedule();
        let rows = world.query_filtered::<EntityRow, bevy_ecs::query::Allow<Away>>();
        Ok(SimWorld {
            world,
            schedule,
            rows,
        })
    }

    /// Rebuilds the world in place from `snapshot` in `level`, reusing the
    /// allocated ECS `World`, compiled `Schedule`, and query state.
    pub fn restore_from(&mut self, snapshot: &Snapshot, level: &Level) -> Result<(), InstallError> {
        let instances = validate_snapshot(snapshot, level)?;
        let same_level = self.world.resource::<Static>().0.id() == level.id()
            && Arc::ptr_eq(&self.world.resource::<Static>().0.placed, &level.placed);
        let preserve_faces = same_level
            && Arc::ptr_eq(
                self.world.resource::<Module>().instances.made_shared(),
                &snapshot.spawned,
            );
        if !same_level {
            *self.world.resource_mut::<Static>() = Static(level.clone());
        }
        if !preserve_faces {
            *self.world.resource_mut::<MadeFaces>() = MadeFaces::default();
        }
        populate_resources(&mut self.world, snapshot, level, &instances);

        let mut target_ids: Vec<SimId> = snapshot.entities.iter().map(|e| e.id).collect();
        target_ids.sort_unstable();
        let mut existing: Vec<(SimId, Entity, u8)> = {
            let mut q = self.world.query_filtered::<(
                Entity,
                &Id,
                Option<&Object>,
                Option<&Bolt>,
                Option<&Wanderer>,
            ), bevy_ecs::query::Allow<Away>>();
            q.iter(&self.world)
                .map(|(ent, id, obj, bolt, wand)| {
                    let tag = if obj.is_some() {
                        2
                    } else if bolt.is_some() {
                        3
                    } else if wand.is_some() {
                        1
                    } else {
                        0
                    };
                    (id.0, ent, tag)
                })
                .collect()
        };
        existing.sort_unstable_by_key(|(id, ..)| *id);

        let mut keep = Vec::with_capacity(existing.len());
        let mut remove = Vec::new();
        for &(id, ent, tag) in &existing {
            if target_ids.binary_search(&id).is_ok() {
                keep.push((id, ent, tag));
            } else {
                remove.push(ent);
            }
        }
        for ent in remove {
            self.world.despawn(ent);
        }

        let driving = driving_seats(snapshot);
        let game = self.world.resource::<Static>().0.game();
        let tsl = game == crate::combat::passive::Game::Tsl;
        let away = switch::leaves_area(game);
        for entity in &snapshot.entities {
            let new_tag = match entity.kind {
                Kind::Avatar(_) => 0,
                Kind::Wanderer => 1,
                Kind::Object(..) => 2,
                Kind::Missile => 3,
            };
            let matched = keep
                .binary_search_by_key(&entity.id, |(id, ..)| *id)
                .ok()
                .map(|i| keep[i]);
            let mut em = match matched {
                Some((_, ent, old_tag)) if old_tag == new_tag => self.world.entity_mut(ent),
                Some((_, ent, _)) => {
                    self.world.despawn(ent);
                    self.world.spawn(Id(entity.id))
                }
                None => self.world.spawn(Id(entity.id)),
            };
            sync_entity(&mut em, entity, &instances, &driving, tsl, away);
        }
        Ok(())
    }

    /// The state as plain values, entities ordered by id.
    pub fn snapshot(&mut self) -> Snapshot {
        let clock = self.world.resource::<Clock>();
        let (tick, time_ms) = (clock.tick, clock.time_ms);
        let streams = self.world.resource::<Streams>();
        let (shared, authority) = (streams.shared, streams.authority);
        let next_id = self.world.resource::<NextId>().0;
        let seats_held = self.world.resource::<SeatsHeld>().0;
        let module = self.world.resource::<Module>();
        let (area_sound, fade, world_fade_in, video_effect) = (
            module.sound.clone(),
            module.fade,
            module.world_fade_in,
            module.video_effect,
        );
        let (place, transition, stash, spawned, roster, explored, visuals) = (
            module.place.clone(),
            module.pending.clone(),
            module.stash.clone(),
            module.instances.made_shared().clone(),
            module.roster.clone(),
            module.explored.clone(),
            module.visuals.clone(),
        );
        let talk = self.world.resource::<Talking>().0.clone();
        let goods = self.world.resource::<Holding>().0.clone();
        let progress = self.world.resource::<Progressed>().0.clone();
        let personas = self.world.resource::<Personas>().0.clone();
        let minigame = self.world.resource::<minigame::Minigame>().0.clone();
        let pazaak = self.world.resource::<Tabled>().0.clone();
        let lost = self.world.resource::<falls::Lost>().0;
        let kept = self.world.resource::<drivers::Kept>().0.clone();
        let script_state = &self.world.resource::<scripts::Scripts>().state;
        let leading = lent(&script_state.party);
        let solo = script_state.party.solo;
        let puppets = script_state
            .party
            .puppet_creatures()
            .map(|(_, body)| SimId(body))
            .collect();
        let standing = script_state.reputation.clone();
        let module = self
            .world
            .resource::<Static>()
            .0
            .module()
            .map(|module| &**module);
        let waiting = module
            .filter(|module| crate::script::party::at_base(&module.name))
            .map_or(Vec::new(), |_| {
                script_state.party.waiting_creatures().map(SimId).collect()
            });
        let screens = script_state.screens(module);
        let barred = script_state.transit_barred(module);
        let in_combat = self.world.resource::<InCombat>().0;
        let party = self.world.resource::<party::PartyState>();
        let (paused_by, ready) = (party.paused_by, party.ready.clone());
        // The rows (references) put in id order before each (large) state
        // is made; ids are unique, so an unstable sort orders them one way.
        let mut rows: Vec<_> = self.rows.iter(&self.world).collect();
        rows.sort_unstable_by_key(|row| row.0.0);
        let entities: Vec<EntityState> = rows
            .into_iter()
            .map(|row| entity_state(row, time_ms))
            .collect();
        let mut snapshot = Snapshot {
            tick,
            time_ms,
            place,
            entities,
            transition,
            stash,
            talk,
            goods,
            area_sound,
            fade,
            world_fade_in,
            video_effect,
            spawned,
            roster,
            progress,
            personas,
            lost,
            in_combat,
            paused_by,
            ready,
            kept,
            leading,
            solo,
            minigame,
            pazaak,
            explored,
            visuals,
            departing: self.world.resource::<Leaving>().0.clone(),
            screens,
            remotes: self.world.resource::<remote::Remotes>().0.clone(),
            standing,
            puppets,
            waiting,
            saber_rules: self.world.resource::<super::Rules>().0,
            targeted_blocks: self.world.resource::<super::Rules>().1,
            damage_taken: self.world.resource::<super::DamageTaken>().0,
            cheats: self.world.resource::<super::CheatState>().0,
            shared,
            authority,
            next_id,
            seats_held,
        };
        snapshot.screens.transit.refused = snapshot.transit_refusal(barred);
        snapshot
    }
}

fn validate_snapshot(snapshot: &Snapshot, level: &Level) -> Result<Instances, InstallError> {
    let past = (snapshot.seats_held > MAX_SEATS)
        .then(|| Seat(snapshot.seats_held - 1))
        .or_else(|| {
            snapshot
                .entities
                .iter()
                .find_map(|entity| match entity.kind {
                    Kind::Avatar(seat) if seat.0 >= MAX_SEATS => Some(seat),
                    _ => entity
                        .fighter
                        .and_then(|fighter| fighter.mind.driver)
                        .filter(|seat| seat.0 >= MAX_SEATS),
                })
        });
    if let Some(seat) = past {
        return Err(InstallError::Seat(seat));
    }
    let place = level.place();
    let same = match (&snapshot.place, &place) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    if !same {
        let name = |place: &Option<Place>| {
            place
                .as_ref()
                .map_or_else(String::new, |place| place.module.clone())
        };
        return Err(InstallError::WrongLevel {
            snapshot: name(&snapshot.place),
            level: name(&place),
        });
    }
    let instances = Instances::in_snapshot(level, snapshot);
    for entity in &snapshot.entities {
        if let Kind::Object(kind, index) = entity.kind
            && (instances.kind(index) != Some(kind) || level.module.is_none())
        {
            return Err(InstallError::UnknownObject { id: entity.id });
        }
    }
    Ok(instances)
}

fn driving_seats(snapshot: &Snapshot) -> Vec<Seat> {
    snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Object(..)))
        .filter_map(|entity| entity.fighter?.mind.driver)
        .collect()
}

fn populate_resources(
    world: &mut World,
    snapshot: &Snapshot,
    level: &Level,
    instances: &Instances,
) {
    world.insert_resource(Clock {
        tick: snapshot.tick,
        time_ms: snapshot.time_ms,
        msec: 0,
        reason: StepReason::Authority,
    });
    world.insert_resource(Streams {
        shared: snapshot.shared,
        authority: snapshot.authority,
    });
    world.insert_resource(NextId(snapshot.next_id));
    world.insert_resource(SeatsHeld(snapshot.seats_held));
    world.insert_resource(super::Rules(snapshot.saber_rules, snapshot.targeted_blocks));
    world.insert_resource(super::DamageTaken(snapshot.damage_taken));
    world.insert_resource(super::CheatState(snapshot.cheats));
    world.insert_resource(Inputs::default());
    world.insert_resource(Bumped::default());
    world.insert_resource(GrenadeHolsterKeys::default());
    world.insert_resource(Driven::default());
    world.insert_resource(scene::Held::default());
    world.insert_resource(TorsoSent::default());
    world.insert_resource(history::History::starting(snapshot, instances));
    world.insert_resource(Hits::default());
    world.insert_resource(Heals::default());
    world.insert_resource(sabers::Blades::default());
    let mut scripts = scripts::Scripts::default();
    scripts
        .state
        .take_told(snapshot, level.module().map(|module| &**module));
    world.insert_resource(scripts);
    world.insert_resource(scripts::Opened::default());
    world.insert_resource(scripts::Blocked::default());
    world.insert_resource(scripts::Closed::default());
    world.insert_resource(scripts::Clicked::default());
    world.insert_resource(crate::notice::Notices::default());
    world.insert_resource(containers::Containers::default());
    world.insert_resource(locks::Locks::default());
    world.insert_resource(objects::Battering::default());
    world.insert_resource(objects::Battered::default());
    world.insert_resource(traps::Shot::default());
    world.insert_resource(minigame::Struck::default());
    world.insert_resource(covert::Presses::default());
    world.insert_resource(scripts::Used::default());
    world.insert_resource(Talking(snapshot.talk.clone()));
    world.insert_resource(Holding(snapshot.goods.clone()));
    world.insert_resource(Progressed(snapshot.progress.clone()));
    world.insert_resource(Personas(snapshot.personas.clone()));
    world.insert_resource(minigame::Minigame(snapshot.minigame.clone()));
    world.insert_resource(Tabled(snapshot.pazaak.clone()));
    world.insert_resource(falls::Lost(snapshot.lost));
    world.insert_resource(falls::Reviving::default());
    world.insert_resource(InCombat(snapshot.in_combat));
    world.insert_resource(party::Embattled::default());
    world.insert_resource(party::PartyState {
        paused_by: snapshot.paused_by,
        ready: snapshot.ready.clone(),
        through: false,
    });
    world.insert_resource(drivers::Kept(snapshot.kept.clone()));
    world.insert_resource(drivers::Phantoms::default());
    world.insert_resource(remote::Remotes(snapshot.remotes.clone()));
    world.insert_resource(remote::Launched::default());
    world.insert_resource(party::Asked::default());
    world.insert_resource(party::Refused::default());
    world.insert_resource(party::Leaving::default());
    world.insert_resource(scripts::SpellsAt::default());
    world.insert_resource(scripts::Casts::default());
    world.insert_resource(scripts::LetGo::default());
    world.insert_resource(scripts::Ordered::default());
    world.insert_resource(scripts::Spent::default());
    world.insert_resource(scripts::ItemTalents::default());
    world.insert_resource(scripts::FeatTalents::default());
    world.insert_resource(scripts::WeaponSwaps::default());
    world.insert_resource(scripts::Rounds::default());
    world.insert_resource(scripts::StatesEnded::default());
    world.insert_resource(super::shut_in::DoorShut::default());
    world.insert_resource(Module {
        place: snapshot.place.clone(),
        pending: snapshot.transition.clone(),
        stash: snapshot.stash.clone(),
        sound: snapshot.area_sound.clone(),
        fade: snapshot.fade,
        world_fade_in: snapshot.world_fade_in,
        video_effect: snapshot.video_effect,
        instances: instances.clone(),
        roster: snapshot.roster.clone(),
        reconciled: None,
        dropped_code: Vec::new(),
        stances_gone: Vec::new(),
        arrival_fallback: None,
        explored: snapshot.explored.clone(),
        visuals: snapshot.visuals.clone(),
    });
    world.insert_resource(Leaving(snapshot.departing.clone()));
}

fn sync_entity(
    em: &mut bevy_ecs::world::EntityWorldMut<'_>,
    entity: &EntityState,
    instances: &Instances,
    driving: &[Seat],
    tsl: bool,
    away: bool,
) {
    match entity.kind {
        Kind::Avatar(seat) => {
            let player = entity
                .player
                .unwrap_or_else(|| MotionState::standing(scale(entity.position, 1.0 / UNIT), 0.0));
            let mut sheet = entity.sheet.unwrap_or_else(Sheet::dev_soldier);
            if tsl {
                sheet.flags |= flags::TSL;
            }
            let fighter = entity.fighter.unwrap_or_else(|| Fighter::of(&sheet));
            em.insert((Player(player), Fighting(fighter), PlayerSheet(sheet)));
            if driving.contains(&seat) {
                em.remove::<Avatar>();
                em.insert(Benched(seat));
                if away {
                    em.insert(Away);
                } else {
                    em.remove::<Away>();
                }
            } else {
                em.remove::<(Benched, Away)>();
                em.insert(Avatar(seat));
            }
        }
        Kind::Wanderer => {
            em.insert((
                Wanderer {
                    retarget_ms: entity.retarget_ms,
                },
                Motion {
                    position: entity.position,
                    velocity: entity.velocity,
                },
            ));
        }
        Kind::Missile => {
            if let Some(missile) = entity.missile {
                em.insert(Bolt(missile));
            } else {
                em.remove::<Bolt>();
            }
        }
        Kind::Object(kind, index) => {
            let facts = instances
                .facts(index)
                .map(|facts| match instances.made_from(index) {
                    Some(_) if kind != ObjectKind::Creature => Arc::new(ObjectData {
                        position: entity.position,
                        ..(**facts).clone()
                    }),
                    _ => facts.clone(),
                });
            em.insert((
                Object {
                    kind,
                    index,
                    state: entity.object.unwrap_or(ObjectState::default()),
                    facts,
                },
                Placed(entity.position),
            ));
            if let (Some(player), Some(fighter)) = (entity.player, entity.fighter) {
                em.insert((Player(player), Fighting(fighter)));
                if let Some(seat) = fighter.mind.driver {
                    em.insert(Avatar(seat));
                } else {
                    em.remove::<Avatar>();
                }
            } else {
                em.remove::<(Player, Fighting, Avatar)>();
            }
            if let Some(sheet) = entity.sheet {
                em.insert(PlayerSheet(sheet));
            } else {
                em.remove::<PlayerSheet>();
            }
        }
    }
    if let Some(left_ms) = entity.life_ms {
        em.insert(Lifetime {
            left_ms,
            going: entity.going,
        });
    } else {
        em.remove::<Lifetime>();
    }
}

fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.set_executor(SingleThreadedExecutor::new());
    schedule.add_systems(
        (
            party::breaks,
            (
                drivers::assign.run_if(on_authority),
                minigame::steer,
                (
                    scene::hold,
                    remote::launch.run_if(on_authority),
                    hold_talkers,
                    restrict_seats,
                    ready_grenades,
                )
                    .chain(),
                (made_faces, minds::think.run_if(on_authority)).chain(),
                powers::force,
                (
                    stances::picks,
                    feat_styles::feat_styles,
                    move_actors,
                    remote::flight::fly,
                    remote::kit::kit.run_if(on_authority),
                )
                    .chain(),
                fire,
                sabers::sabers,
                (strikes::strikes, grapples::grapples).chain(),
                fly,
                (
                    hurt,
                    objects::objects,
                    party::combat,
                    falls::falls.run_if(on_authority),
                )
                    .chain(),
                (interact, party::ready).chain(),
                containers::containers,
                (
                    locks::locks.run_if(on_authority),
                    covert::covert.run_if(on_authority),
                )
                    .chain(),
                wander,
                spawn_wanderers.run_if(on_authority),
                (integrate, charting::explore.run_if(on_authority)).chain(),
                expire,
                history::remember.run_if(on_authority),
                (
                    (
                        scripts::scripts,
                        scene::hold,
                        scripts::apply,
                        locks::refused,
                        super::shut_in::free_shut_in,
                    )
                        .chain()
                        .run_if(on_authority),
                    bars::settle.run_if(on_authority),
                )
                    .chain(),
            )
                .chain()
                .run_if(party::running),
        )
            .chain(),
    );
    schedule
}
