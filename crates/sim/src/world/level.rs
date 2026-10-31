//! What the sim reads but never changes: the level (`Level`), its id and
//! digests.

use std::collections::BTreeMap;

use super::*;

/// What the sim reads but never changes: the level's collision, the
/// player skeleton's animation lengths and, for a KOTOR area, its
/// module's objects. Not in a snapshot: a world is rebuilt from a snapshot
/// in the same level (`digest` says which).
///
/// Its parts are set only by the constructors (`new`, `with_module`), so
/// the digests, computed once and kept, always describe them.
#[derive(Clone)]
pub struct Level {
    pub(super) collision: Arc<dyn CollisionWorld + Send + Sync>,
    pub(super) anims: Arc<AnimTable>,
    /// The module this level is the entry area of.
    pub(super) module: Option<Arc<ModuleData>>,
    /// Its placements' facts, by number: the first instances of its area
    /// (`crate::instance`).
    pub(super) placed: Arc<[Arc<ObjectData>]>,
    /// Where characters can walk and how to get there (I27), baked from
    /// a module's walkable faces.
    pub(super) nav: Option<Arc<NavMesh>>,
    /// What the level is (`arena`, `course`, a module's name).
    pub(super) name: String,
    /// Who in the party may hurt whom (I22 C17, a session option).
    pub(super) friendly_fire: FriendlyFire,
    /// The module's compiled scripts (I16), run on the authority.
    pub(super) scripts: Option<Arc<crate::script::Library>>,
    /// The area's minigame, if it is one (`crate::minigame`).
    pub(super) minigame: Option<Arc<crate::minigame::Rules>>,
    /// A combat arena's rules, if it is one (`arena`).
    pub(super) arena: Option<arena::Rules>,
    /// The party table's companions' templates its scripts lack, by name
    /// (`with_companions`): not the module's, so not in its digest.
    pub(super) companions: Arc<BTreeMap<String, Arc<ObjectData>>>,
    pub(super) digests: Arc<OnceLock<Digests>>,
}

/// Which level a recording or a save was made in: its name, for reports,
/// and `Level::digest`, which a level must have to replay or load it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelId {
    pub name: String,
    pub digest: [u8; 32],
}

impl LevelId {
    pub(crate) fn write(&self, out: &mut crate::wire::Writer) {
        out.string(&self.name);
        out.bytes(&self.digest);
    }

    pub(crate) fn read(
        input: &mut crate::wire::Reader,
    ) -> Result<LevelId, crate::wire::DecodeError> {
        Ok(LevelId {
            name: input.string()?,
            digest: input.array()?,
        })
    }

    /// Whether `level` is this one.
    pub fn check(&self, level: &Level) -> Result<(), InstallError> {
        let here = level.id();
        if here == *self {
            Ok(())
        } else {
            Err(InstallError::WrongLevel {
                snapshot: self.name.clone(),
                level: here.name,
            })
        }
    }
}

/// What names a level: two machines whose digests agree run the same one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digests {
    /// `ModuleData::digest`; zeros without a module.
    pub module: [u8; 32],
    /// `collision::Level::digest`; empty for a world that is not a built
    /// level (the placeholder's boxes).
    pub collision: String,
    /// The animation lengths movement times holds by.
    pub anims: [u8; 32],
    /// `NavMesh::digest`, if the level has one.
    pub nav: Option<[u8; 32]>,
}

impl Level {
    pub fn new(
        name: &str,
        collision: Arc<dyn CollisionWorld + Send + Sync>,
        anims: Arc<AnimTable>,
    ) -> Level {
        Level {
            collision,
            anims,
            module: None,
            placed: Arc::new([]),
            nav: None,
            name: name.to_string(),
            friendly_fire: FriendlyFire::Off,
            scripts: None,
            minigame: None,
            arena: None,
            companions: Arc::default(),
            digests: Arc::new(OnceLock::new()),
        }
    }

    /// This level with `companions`, the party table's kept templates
    /// (`script::party::Npc::template_data`) its scripts lack: a companion
    /// made available by another module's own template is made and carried
    /// in as itself (`template`), as KOTOR's table keeps the creature.
    pub fn with_companions(self, companions: BTreeMap<String, Arc<ObjectData>>) -> Level {
        Level {
            companions: Arc::new(companions),
            ..self
        }
    }

    /// The arena's rules, if the level is one (`with_arena`).
    pub fn arena_rules(&self) -> Option<&arena::Rules> {
        self.arena.as_ref()
    }

    /// This level as a combat arena under `rules`.
    pub fn with_arena(self, rules: arena::Rules) -> Level {
        Level {
            arena: Some(rules),
            digests: Arc::new(OnceLock::new()),
            ..self
        }
    }

    /// The area's minigame, if it is one (`with_minigame`).
    pub fn minigame(&self) -> Option<&Arc<crate::minigame::Rules>> {
        self.minigame.as_ref()
    }

    /// This level with its area's minigame.
    pub fn with_minigame(self, rules: crate::minigame::Rules) -> Level {
        Level {
            minigame: Some(Arc::new(rules)),
            digests: Arc::new(OnceLock::new()),
            ..self
        }
    }

    /// The level's scripts, if it has any (`with_scripts`).
    pub fn scripts(&self) -> Option<&Arc<crate::script::Library>> {
        self.scripts.as_ref()
    }

    /// This level with its module's compiled scripts (its own, or another
    /// level's shared).
    pub fn with_scripts(self, scripts: impl Into<Arc<crate::script::Library>>) -> Level {
        Level {
            scripts: Some(scripts.into()),
            digests: Arc::new(OnceLock::new()),
            ..self
        }
    }

    /// This level with `globals` declared in its module's catalogue
    /// (`ModuleData::globals`) beside the game's: a test's own scripts'
    /// globals, kept as the game's are. A module with no catalogue keeps
    /// any already.
    pub fn declaring(self, globals: impl IntoIterator<Item = (String, Global)>) -> Level {
        let Some(module) = self
            .module
            .as_ref()
            .filter(|module| !module.globals.is_empty())
        else {
            return self;
        };
        let mut module = (**module).clone();
        module.globals.extend(globals);
        Level {
            module: Some(Arc::new(module)),
            digests: Arc::new(OnceLock::new()),
            ..self
        }
    }

    /// This level with no scripts: nothing runs its events (a measure of
    /// walking alone, `kjk collision walk`).
    pub fn without_scripts(self) -> Level {
        Level {
            scripts: None,
            digests: Arc::new(OnceLock::new()),
            ..self
        }
    }

    /// Who in the party may hurt whom: off (JA's and KOTOR's default)
    /// unless the session chose otherwise (`with_friendly_fire`).
    pub fn friendly_fire(&self) -> FriendlyFire {
        self.friendly_fire
    }

    /// This level with the session's friendly fire (C17): players' shots,
    /// blades and powers hurt each other at `Full` (a duel, I25).
    pub fn with_friendly_fire(self, friendly_fire: FriendlyFire) -> Level {
        Level {
            friendly_fire,
            digests: Arc::new(OnceLock::new()),
            ..self
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The module whose resources the level is read through: the one its
    /// data came from (the arena runs on another's), else its own name.
    pub fn resources(&self) -> &str {
        self.module
            .as_ref()
            .map_or(&self.name, |module| module.name.as_str())
    }

    pub fn collision(&self) -> &Arc<dyn CollisionWorld + Send + Sync> {
        &self.collision
    }

    pub fn anims(&self) -> &Arc<AnimTable> {
        &self.anims
    }

    /// The module this level is the entry area of, if it is one.
    pub fn module(&self) -> Option<&Arc<ModuleData>> {
        self.module.as_ref()
    }

    /// The level's name and digest.
    pub fn id(&self) -> LevelId {
        LevelId {
            name: self.name.clone(),
            digest: self.digest(),
        }
    }

    /// Where characters can walk, if the level knows (`with_nav`).
    pub fn nav(&self) -> Option<&Arc<NavMesh>> {
        self.nav.as_ref()
    }

    /// Whether a character at `body` (metres, its movement box's origin) is
    /// in `trigger`, over the floors of this level (`ObjectData::contains`).
    pub fn in_trigger(&self, trigger: &ObjectData, body: [f32; 3]) -> bool {
        trigger.contains(body, self.nav.as_deref())
    }

    /// Whether a standing character at `body` (metres, its movement box's
    /// origin) runs into door `index`: in its doorway, wherever along its
    /// shape, as a walk into a door that leads out asks to leave
    /// (`in_doorway`).
    pub fn at_door(&self, index: u32, body: [f32; 3]) -> bool {
        in_doorway(self, index, scale(body, 1.0 / UNIT), motion::STAND_HEIGHT)
    }

    /// This level with its navigation mesh.
    pub fn with_nav(self, nav: impl Into<Arc<NavMesh>>) -> Level {
        Level {
            nav: Some(nav.into()),
            digests: Arc::new(OnceLock::new()),
            ..self
        }
    }

    /// This level as `module`'s entry area.
    pub fn with_module(self, module: ModuleData) -> Level {
        Level {
            placed: module
                .objects()
                .map(|facts| Arc::new(facts.into_owned()))
                .collect(),
            module: Some(Arc::new(module)),
            digests: Arc::new(OnceLock::new()),
            ..self
        }
    }

    /// The module's placements' facts, by number (none without a module).
    pub fn placed(&self) -> &Arc<[Arc<ObjectData>]> {
        &self.placed
    }

    /// The facts an instance of `template` made in play has: a creature's
    /// and a placeable's from the script library's templates; the dead's
    /// remains (`remains::bag_template`) from their `bodybag.2da` row; a
    /// mine laid in play (`traps::laid_template`) from its `traps.2da` row:
    /// its row's script and trap, no outline (its area is the square round
    /// where it stands, `traps::LAID_HALF_M`); its DCs, creator and faction
    /// are the scripts' state's.
    pub fn template(&self, kind: ObjectKind, template: &str) -> Option<Arc<ObjectData>> {
        match kind {
            ObjectKind::Creature => self
                .scripts()?
                .creature(template)
                .or_else(|| self.companions.get(template))
                .cloned(),
            ObjectKind::Placeable if remains::bag_row(template).is_some() => {
                remains::bag_facts(self.module()?, template).map(Arc::new)
            }
            ObjectKind::Placeable => self.scripts()?.placeable(template).cloned(),
            ObjectKind::Trigger => crate::traps::laid_facts(self.module()?, template).map(Arc::new),
            _ => None,
        }
    }

    /// The placeholder level: a 40 m square floor at z 0 walled in, with
    /// stairs to walk up and a ledge to fall from, and stand-in animation
    /// lengths.
    pub fn arena() -> Level {
        let solid = Solid::level;
        let mut world = BoxWorld::floor(ARENA, 0.0, 1.0)
            .with(solid([-ARENA - 1.0, -ARENA, 0.0], [-ARENA, ARENA, 3.0]))
            .with(solid([ARENA, -ARENA, 0.0], [ARENA + 1.0, ARENA, 3.0]))
            .with(solid([-ARENA, -ARENA - 1.0, 0.0], [ARENA, -ARENA, 3.0]))
            .with(solid([-ARENA, ARENA, 0.0], [ARENA, ARENA + 1.0, 3.0]));
        // Three steps of 8, 12 and 16 units, and a 1 m block.
        for (i, rise) in [8.0f32, 20.0, 36.0].into_iter().enumerate() {
            let x = 4.0 + i as f32 * 0.6;
            world = world.with(solid([x, -3.0, 0.0], [10.0, 3.0, rise * UNIT]));
        }
        world = world.with(solid([-10.0, -4.0, 0.0], [-6.0, 4.0, 1.0]));
        Level::new("arena", Arc::new(world), Arc::new(AnimTable::stand_in()))
    }

    /// Computed once per level (the collision's walks every brush).
    pub fn digests(&self) -> &Digests {
        self.digests.get_or_init(|| Digests {
            module: self
                .module
                .as_ref()
                .map_or([0; 32], |module| module.digest()),
            collision: self
                .collision
                .level()
                .map_or_else(String::new, collision::Level::digest),
            anims: self.anims.digest(),
            nav: self.nav.as_ref().map(|nav| nav.digest()),
        })
    }

    /// The digest's parts, each cut to 4 bytes, for the log: two machines
    /// whose `digest` differs tell by these which data they read
    /// differently (the module's, its collision or nav, the JA animations
    /// every level holds, the scripts).
    pub fn digest_parts(&self) -> String {
        let short =
            |bytes: &[u8]| -> String { bytes.iter().take(4).map(|b| format!("{b:02x}")).collect() };
        let digests = self.digests();
        let nav = digests
            .nav
            .map_or_else(|| "-".to_string(), |nav| short(&nav));
        let scripts = self
            .scripts
            .as_ref()
            .map_or_else(|| "-".to_string(), |scripts| short(&scripts.digest()));
        format!(
            "module {} collision {} anims {} nav {nav} scripts {scripts}",
            short(&digests.module),
            digests.collision.get(..8).unwrap_or("-"),
            short(&digests.anims),
        )
    }

    /// The name and the digests in one (the navigation mesh's only when
    /// there is one, so a level without one keeps its digest; the friendly
    /// fire only when it is not off).
    pub fn digest(&self) -> [u8; 32] {
        let digests = self.digests();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"KJKL");
        hasher.update(&(self.name.len() as u32).to_le_bytes());
        hasher.update(self.name.as_bytes());
        hasher.update(&digests.module);
        hasher.update(digests.collision.as_bytes());
        hasher.update(&digests.anims);
        if let Some(nav) = &digests.nav {
            hasher.update(b"nav");
            hasher.update(nav);
        }
        // Only another mode than the default names itself, so a level's
        // digest stays what it was without one.
        if self.friendly_fire != FriendlyFire::Off {
            hasher.update(b"friendly fire");
            hasher.update(&[self.friendly_fire as u8]);
        }
        if let Some(scripts) = &self.scripts {
            hasher.update(b"scripts");
            hasher.update(&scripts.digest());
        }
        if let Some(minigame) = &self.minigame {
            hasher.update(b"minigame");
            hasher.update(&minigame.digest());
        }
        if let Some(arena) = &self.arena {
            hasher.update(b"arena");
            arena.digest(&mut hasher);
        }
        *hasher.finalize().as_bytes()
    }

    /// The game whose rules its feats follow (`combat::passive`).
    pub fn game(&self) -> crate::combat::passive::Game {
        self.module
            .as_ref()
            .map_or_else(Default::default, |module| {
                crate::combat::passive::Game::named(&module.game)
            })
    }

    /// The module this level is, as a snapshot names it.
    pub fn place(&self) -> Option<Place> {
        self.module.as_ref().map(|module| Place {
            game: module.game.clone(),
            module: module.name.clone(),
            digest: self.digest(),
            players: module.players,
        })
    }

    /// The collision's entity states in `snapshot` (its doors' states), in
    /// the level's order, when the collision is a built level: what a view
    /// traces the level with (`with`) to see the doors the sim has.
    pub fn states_in(&self, snapshot: &Snapshot) -> Option<Vec<u8>> {
        self.states(snapshot.entities.iter().filter_map(|entity| {
            match (entity.kind, entity.object.as_ref()) {
                (Kind::Object(_, index), Some(object)) => Some((index, object)),
                _ => None,
            }
        }))
    }

    /// The collision's entity states for `objects` (a door's state; 0 for
    /// a placeable, whatever its look; `collision::GONE` for one no longer
    /// in play), in the
    /// level's order, when the collision is a built level.
    pub(crate) fn states<'a>(
        &self,
        objects: impl Iterator<Item = (u32, &'a ObjectState)>,
    ) -> Option<Vec<u8>> {
        let level = self.collision.level()?;
        let mut states = vec![collision::GONE; level.entities().count()];
        for (index, object) in objects {
            let Some(slot) = level.slot(index) else {
                continue;
            };
            // A placeable's walkmesh stands in every look it takes (open,
            // on, an animloop): only one gone from play lets bodies by.
            let placeable = self
                .placed
                .get(index as usize)
                .is_some_and(|facts| facts.kind == ObjectKind::Placeable);
            states[slot] = match object.state {
                collision::GONE => collision::GONE,
                _ if placeable => 0,
                state => state,
            };
        }
        Some(states)
    }

    /// Traces through the collision with `states` (from `states`) for the
    /// entities, or the collision as it is.
    pub fn with<R>(&self, states: Option<&[u8]>, run: impl FnOnce(&dyn CollisionWorld) -> R) -> R {
        match (self.collision.level(), states) {
            (Some(level), Some(states)) => run(&level.with_states(states)),
            _ => run(self.collision.as_ref()),
        }
    }

    /// Whether a standing body fits with its origin at `at` (units): the
    /// body move_body moves (its box, round across), against the level as built, its middle not over a
    /// placeable's footprint (inside the walls that ring it, which a box
    /// standing there does not touch; KOTOR's walker never stands there).
    pub fn fits(&self, at: math::Vec3) -> bool {
        self.fits_lifted(at, &[0.0])
    }

    /// `fits`, the box also tried each of `lifts` (units) over `at`, as
    /// move_body's ground trace holds a body on the highest of the floor under
    /// its box; the footprint looked for under `at` itself.
    fn fits_lifted(&self, at: math::Vec3, lifts: &[f32]) -> bool {
        let half = crate::motion::HALF_WIDTH * UNIT;
        let extents = Extents {
            mins: [-half, -half, crate::motion::SOLE_Z * UNIT],
            maxs: [half, half, crate::motion::STAND_HEIGHT * UNIT],
        };
        let at = scale(at, UNIT);
        let below = [
            at[0],
            at[1],
            at[2] + (crate::motion::SOLE_Z - crate::motion::STEP_HEIGHT) * UNIT,
        ];
        self.with(None, |world| {
            let clear = lifts.iter().any(|lift| {
                let up = [at[0], at[1], at[2] + lift * UNIT];
                !world
                    .sweep(
                        up,
                        up,
                        &collision::Shape::Upright(extents),
                        Contents::PLAYER_SOLID,
                    )
                    .start_solid
            });
            let under = world.trace(at, below, &Extents::POINT, Contents::NONWALK);
            clear && !(under.fraction < 1.0 && under.entity.is_some())
        })
    }

    /// Whether a standing body put down at `at` (an origin over the
    /// navigation mesh's floor, units) fits (`fits`), its box also tried a
    /// little higher (`rules::walking::walker::FLOOR_GIVE`), where the
    /// level's floor rises a few millimetres over the mesh's under a corner
    /// of the box. Beside Dantooine's Khoonda plaza, a floor 9 mm higher
    /// under one corner left no side of a citizen to go round by but the one
    /// behind a wall.
    pub fn fits_put_down(&self, at: math::Vec3) -> bool {
        self.fits_lifted(at, &[0.0, rules::walking::walker::FLOOR_GIVE])
    }

    /// Whether a standing body fits at `at` (units) among the level's own
    /// solids, doors and placeables aside, lifted a step (a sloped floor
    /// under it is no obstacle): where a route may turn.
    pub fn fits_level(&self, at: math::Vec3) -> bool {
        let at = [at[0], at[1], at[2] + crate::motion::STEP_HEIGHT];
        let Some(level) = self.collision.level() else {
            return self.fits(at);
        };
        let half = crate::motion::HALF_WIDTH * UNIT;
        let extents = Extents {
            mins: [-half, -half, crate::motion::SOLE_Z * UNIT],
            maxs: [half, half, crate::motion::STAND_HEIGHT * UNIT],
        };
        !level.level_overlaps(scale(at, UNIT), &extents, true, Contents::SOLID)
    }

    /// Whether nothing solid stands between `from` and `to` (units), doors
    /// as `states` has them (`states_in`): what minds and perception see.
    pub fn sees(&self, states: Option<&[u8]>, from: math::Vec3, to: math::Vec3) -> bool {
        unobstructed(&self.line(states, scale(from, UNIT), scale(to, UNIT)))
    }

    /// A line from `from` to `to` (metres) through what is solid, doors as
    /// `states` has them (`sees`, `target::in_sight`).
    pub fn line(
        &self,
        states: Option<&[u8]>,
        from: math::Vec3,
        to: math::Vec3,
    ) -> collision::Trace {
        self.with(states, |world| {
            world.trace(from, to, &Extents::POINT, Contents::SOLID)
        })
    }
}

/// Whether `trace` went all the way, from a start in the open.
pub fn unobstructed(trace: &collision::Trace) -> bool {
    trace.fraction >= 1.0 && !trace.start_solid
}
