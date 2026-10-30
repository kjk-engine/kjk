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
            let soles = [x + f32::from(seat) * 2.0, y, z];
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
            goods: crate::inventory::Goods::default(),
            area_sound: crate::snapshot::AreaSound::default(),
            fade: None,
            world_fade_in: crate::snapshot::WorldFadeIn::Done,
            video_effect: None,
            spawned: Vec::new(),
            progress: crate::snapshot::Progress::default(),
            personas: Vec::new(),
            lost: None,
            in_combat: false,
            paused_by: None,
            ready: None,
            kept: Vec::new(),
            leading: None,
            minigame: None,
            pazaak: Vec::new(),
            explored: Vec::new(),
            visuals: Vec::new(),
            departing: Vec::new(),
            screens: Default::default(),
            remotes: Vec::new(),
            shared: SimRng::new(config.seed, 0),
            authority,
            next_id,
            seats_held: config.seats,
        };
        let start = if level.module.is_some() {
            crate::install::assemble(&start, &level, None, None, &[])?
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
        let instances = Instances::in_snapshot(&level, snapshot);
        let mut world = World::new();
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
        world.init_resource::<Inputs>();
        world.init_resource::<Touched>();
        world.init_resource::<Bumped>();
        world.init_resource::<MadeFaces>();
        world.init_resource::<Driven>();
        world.init_resource::<scene::Held>();
        world.init_resource::<TorsoSent>();
        world.insert_resource(history::History::starting(snapshot));
        world.init_resource::<Hits>();
        world.init_resource::<Heals>();
        world.init_resource::<sabers::Blades>();
        world.init_resource::<scripts::Scripts>();
        world.init_resource::<scripts::Opened>();
        world.init_resource::<scripts::Blocked>();
        world.init_resource::<scripts::Closed>();
        world.init_resource::<scripts::Clicked>();
        world.init_resource::<crate::notice::Notices>();
        world.init_resource::<containers::Containers>();
        world.init_resource::<locks::Locks>();
        world.init_resource::<objects::Battering>();
        world.init_resource::<objects::Battered>();
        world.init_resource::<minigame::Struck>();
        world.init_resource::<covert::Presses>();
        world.init_resource::<scripts::Used>();
        world.insert_resource(Talking(snapshot.talk.clone()));
        world.insert_resource(Holding(snapshot.goods.clone()));
        world.insert_resource(Progressed(snapshot.progress.clone()));
        world.insert_resource(Personas(snapshot.personas.clone()));
        world.insert_resource(minigame::Minigame(snapshot.minigame.clone()));
        world.insert_resource(Tabled(snapshot.pazaak.clone()));
        world.insert_resource(falls::Lost(snapshot.lost));
        world.init_resource::<falls::Reviving>();
        world.insert_resource(InCombat(snapshot.in_combat));
        world.insert_resource(party::Embattled::default());
        world.insert_resource(party::PartyState {
            paused_by: snapshot.paused_by,
            ready: snapshot.ready.clone(),
        });
        world.insert_resource(drivers::Kept(snapshot.kept.clone()));
        world.init_resource::<drivers::Phantoms>();
        world.insert_resource(remote::Remotes(snapshot.remotes.clone()));
        world.init_resource::<remote::Launched>();
        world.init_resource::<party::Asked>();
        world.init_resource::<party::Refused>();
        world.init_resource::<party::Leaving>();
        world.init_resource::<scripts::SpellsAt>();
        world.init_resource::<scripts::Casts>();
        world.init_resource::<scripts::LetGo>();
        world.init_resource::<scripts::Ordered>();
        world.init_resource::<scripts::Spent>();
        world.init_resource::<scripts::ItemTalents>();
        world.init_resource::<scripts::Rounds>();
        world.init_resource::<scripts::StatesEnded>();
        world.insert_resource(Module {
            place: snapshot.place.clone(),
            pending: snapshot.transition.clone(),
            stash: snapshot.stash.clone(),
            sound: snapshot.area_sound,
            fade: snapshot.fade,
            world_fade_in: snapshot.world_fade_in,
            video_effect: snapshot.video_effect,
            instances: instances.clone(),
            explored: snapshot.explored.clone(),
            visuals: snapshot.visuals.clone(),
        });
        world.insert_resource(Leaving(snapshot.departing.clone()));
        for entity in &snapshot.entities {
            if let Kind::Object(kind, index) = entity.kind
                && (instances.kind(index) != Some(kind) || level.module.is_none())
            {
                return Err(InstallError::UnknownObject { id: entity.id });
            }
        }
        world.insert_resource(Static(level));
        // The seats playing a creature: their own characters are benched
        // (`switch`).
        let driving: Vec<Seat> = snapshot
            .entities
            .iter()
            .filter(|entity| matches!(entity.kind, Kind::Object(..)))
            .filter_map(|entity| entity.fighter?.mind.driver)
            .collect();
        let tsl = world.resource::<Static>().0.game() == crate::combat::passive::Game::Tsl;
        for entity in &snapshot.entities {
            let mut spawned = world.spawn(Id(entity.id));
            match entity.kind {
                Kind::Avatar(seat) => {
                    // Decoding guarantees a player, a fighter and a sheet;
                    // a hand-made snapshot without them stands at its
                    // position as the dev soldier.
                    let player = entity.player.unwrap_or_else(|| {
                        MotionState::standing(scale(entity.position, 1.0 / UNIT), 0.0)
                    });
                    let mut sheet = entity.sheet.unwrap_or_else(Sheet::dev_soldier);
                    // A player plays by its game's rules (`gear::Caps`), as
                    // the session marks the sheets it reads.
                    if tsl {
                        sheet.flags |= flags::TSL;
                    }
                    let fighter = entity.fighter.unwrap_or_else(|| Fighter::of(&sheet));
                    spawned.insert((Player(player), Fighting(fighter), PlayerSheet(sheet)));
                    if driving.contains(&seat) {
                        spawned.insert(Benched(seat));
                    } else {
                        spawned.insert(Avatar(seat));
                    }
                }
                Kind::Wanderer => {
                    spawned.insert((
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
                    // Decoding guarantees its flight; a hand-made snapshot
                    // without one has nothing to fly.
                    if let Some(missile) = entity.missile {
                        spawned.insert(Bolt(missile));
                    }
                }
                Kind::Object(kind, index) => {
                    // What play made stands where it was made (its
                    // template's facts stand nowhere); a creature's place
                    // is its body's.
                    let facts =
                        instances
                            .facts(index)
                            .map(|facts| match instances.made_from(index) {
                                Some(_) if kind != ObjectKind::Creature => Arc::new(ObjectData {
                                    position: entity.position,
                                    ..(**facts).clone()
                                }),
                                _ => facts.clone(),
                            });
                    spawned.insert((
                        Object {
                            kind,
                            index,
                            state: entity.object.unwrap_or(ObjectState::default()),
                            facts,
                        },
                        Placed(entity.position),
                    ));
                    // A creature is a character: it moves and fights.
                    if let (Some(player), Some(fighter)) = (entity.player, entity.fighter) {
                        spawned.insert((Player(player), Fighting(fighter)));
                        // A companion a guest drives is that seat's body
                        // too (I30 P2): seat-keyed systems move it.
                        if let Some(seat) = fighter.mind.driver {
                            spawned.insert(Avatar(seat));
                        }
                    }
                    if let Some(sheet) = entity.sheet {
                        spawned.insert(PlayerSheet(sheet));
                    }
                }
            }
            if let Some(left_ms) = entity.life_ms {
                spawned.insert(Lifetime {
                    left_ms,
                    going: entity.going,
                });
            }
        }

        let mut schedule = Schedule::default();
        schedule.set_executor(SingleThreadedExecutor::new());
        // A break first: while one holds, the rest of the step does not run
        // (`party::running`).
        schedule.add_systems(
            (
                party::breaks,
                (
                    drivers::assign.run_if(on_authority),
                    minigame::steer,
                    // A droid launches by the hold the step begins under.
                    (
                        scene::hold,
                        remote::launch.run_if(on_authority),
                        hold_talkers,
                    )
                        .chain(),
                    (made_faces, minds::think.run_if(on_authority)).chain(),
                    powers::force,
                    (
                        move_actors,
                        remote::flight::fly,
                        remote::kit::kit.run_if(on_authority),
                    )
                        .chain(),
                    fire,
                    sabers::sabers,
                    strikes::strikes,
                    fly,
                    // The fall reads the fight the hurts left.
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
                        scripts::scripts,
                        scene::hold,
                        scripts::apply,
                        locks::refused,
                    )
                        .chain()
                        .run_if(on_authority),
                )
                    .chain()
                    .run_if(party::running),
            )
                .chain(),
        );
        let rows = world.query::<EntityRow>();
        Ok(SimWorld {
            world,
            schedule,
            rows,
        })
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
            module.sound,
            module.fade,
            module.world_fade_in,
            module.video_effect,
        );
        let (place, transition, stash, spawned, explored, visuals) = (
            module.place.clone(),
            module.pending.clone(),
            module.stash.clone(),
            module.instances.made().to_vec(),
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
        let screens = script_state.screens(
            self.world
                .resource::<Static>()
                .0
                .module()
                .map(|module| &**module),
        );
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
        Snapshot {
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
            progress,
            personas,
            lost,
            in_combat,
            paused_by,
            ready,
            kept,
            leading,
            minigame,
            pazaak,
            explored,
            visuals,
            departing: self.world.resource::<Leaving>().0.clone(),
            screens,
            remotes: self.world.resource::<remote::Remotes>().0.clone(),
            shared,
            authority,
            next_id,
            seats_held,
        }
    }
}
