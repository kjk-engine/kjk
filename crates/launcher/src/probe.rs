//! Scripts made here (`script_ncs::probes::Asm`; nothing of the game's) run
//! in a module installed headless, for the end-to-end checks of what the
//! script engine carries out (`kjk script effects`, `kjk script items`).

use vfs::Discovery;

pub(crate) use sim::script::calls::{Call, by_tag, float, int, me, program, text};

/// A module installed for one player (the dev soldier) with `library` as
/// its scripts, stepped on the authority.
pub(crate) struct Probe {
    pub world: sim::SimWorld,
    /// The player.
    pub me: sim::SimId,
    tick: u64,
    installer: crate::lifecycle::Installer,
}

impl Probe {
    /// `library` makes the probe's scripts from the planned level (its
    /// module and its own library: the item templates, for its objects to
    /// be stocked and scripts to make).
    pub fn new(
        discovery: &Discovery,
        game: vfs::Game,
        module: &str,
        library: impl FnOnce(&sim::Level) -> Result<sim::script::Library, String>,
    ) -> Result<Probe, String> {
        // Its scripts' globals declared as the game's catalogue declares
        // its own: a global it does not list never sets.
        Probe::installed(discovery, game, module, |level| {
            let library = library(level)?;
            let declared = library.globals_written();
            Ok(level.clone().with_scripts(library).declaring(declared))
        })
    }

    /// The module as the game has it: its own scripts and templates.
    pub fn own(discovery: &Discovery, game: vfs::Game, module: &str) -> Result<Probe, String> {
        Probe::installed(discovery, game, module, |level| Ok(level.clone()))
    }

    fn installed(
        discovery: &Discovery,
        game: vfs::Game,
        module: &str,
        level: impl FnOnce(&sim::Level) -> Result<sim::Level, String>,
    ) -> Result<Probe, String> {
        let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
        let installer = crate::lifecycle::Installer::new(discovery, game, anims)?;
        let prepared = installer.prepare_one(module, None)?;
        let level = level(&prepared.plan.level)?;
        let config = sim::WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: sim::combat::Sheet::dev_soldier(),
            guests: sim::Guests::Avatars,
        };
        let mut world = sim::SimWorld::new(config, level).map_err(|e| e.to_string())?;
        let me = world
            .snapshot()
            .entities
            .iter()
            .find(|e| matches!(e.kind, sim::Kind::Avatar(_)))
            .map(|e| e.id)
            .ok_or("no player")?;
        Ok(Probe {
            world,
            me,
            tick: 0,
            installer,
        })
    }

    /// The probe with the module's creatures tagged in `tags` in the party
    /// by hand, as `--follow` puts them (`stage::with_followers`).
    pub fn follow(self, tags: &[String]) -> Result<Probe, String> {
        let world = crate::stage::with_followers(self.world, tags, Some(&self.installer))?;
        Ok(Probe { world, ..self })
    }

    /// Goes to `module` (its own scripts), arriving at its entry.
    pub fn install(&mut self, module: &str) -> Result<sim::Snapshot, String> {
        let request = self.installer.request(module, None, 1);
        let prepared = self.installer.prepare(&request, false)?;
        prepared
            .plan
            .commit(&mut self.world)
            .map_err(|e| e.to_string())?;
        Ok(self.world.snapshot())
    }

    /// The player stood at (`x`, `y`) metres on its own floor, the world and
    /// its scripts' state otherwise as they are.
    pub fn place(&mut self, x: f32, y: f32) -> Result<(), String> {
        self.edit_me(|me| {
            let player = me.player.as_mut().ok_or("no player")?;
            player.origin[0] = x / collision::UNIT;
            player.origin[1] = y / collision::UNIT;
            Ok(())
        })
    }

    /// The player's entity changed by `edit`, the world and its scripts'
    /// state otherwise as they are.
    pub fn edit_me(
        &mut self,
        edit: impl FnOnce(&mut sim::EntityState) -> Result<(), String>,
    ) -> Result<(), String> {
        self.edit(self.me, edit)
    }

    /// Entity `id` changed by `edit`, the world and its scripts' state
    /// otherwise as they are.
    pub fn edit(
        &mut self,
        id: sim::SimId,
        edit: impl FnOnce(&mut sim::EntityState) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut snapshot = self.world.snapshot();
        let state = self.world.script_state().clone();
        let entity = snapshot
            .entities
            .iter_mut()
            .find(|entity| entity.id == id)
            .ok_or(format!("no entity {}", id.0))?;
        edit(entity)?;
        let level = self.world.level().clone();
        self.world = sim::SimWorld::from_snapshot(&snapshot, level).map_err(|e| e.to_string())?;
        self.world.set_script_state(state);
        Ok(())
    }

    /// One step with the player's command `command`: its snapshot, and
    /// what it told the player.
    pub fn press(&mut self, command: sim::Command) -> (sim::Snapshot, Vec<sim::notice::Notice>) {
        let mut input = sim::TickInput::default();
        input.insert(sim::Seat(0), command);
        self.tick(&input)
    }

    /// `ticks` steps; the last one's snapshot.
    pub fn step(&mut self, ticks: u64) -> sim::Snapshot {
        self.step_told(ticks).0
    }

    /// `ticks` steps; the last one's snapshot, and what every step told the
    /// player.
    pub fn step_told(&mut self, ticks: u64) -> (sim::Snapshot, Vec<sim::notice::Notice>) {
        let mut told = Vec::new();
        let mut last = None;
        for _ in 0..ticks {
            let (snapshot, notices) = self.tick(&sim::TickInput::default());
            told.extend(notices);
            last = Some(snapshot);
        }
        (last.expect("stepped"), told)
    }

    /// One step with the player asking `action`: its snapshot, and what it
    /// told the player.
    pub fn act(
        &mut self,
        action: sim::action::Action,
    ) -> (sim::Snapshot, Vec<sim::notice::Notice>) {
        let mut input = sim::TickInput::default();
        input.act(sim::Seat(0), action);
        self.tick(&input)
    }

    fn tick(&mut self, input: &sim::TickInput) -> (sim::Snapshot, Vec<sim::notice::Notice>) {
        self.tick += 1;
        let snapshot = sim::step(
            &mut self.world,
            self.tick,
            input,
            sim::TickRate::default().msec(self.tick),
            sim::StepReason::Authority,
        );
        let told = self
            .world
            .take_notices()
            .into_iter()
            .map(|(_, notice)| notice)
            .collect();
        (snapshot, told)
    }

    /// One step with `input` (a seat's actions); its snapshot.
    pub fn step_with(&mut self, input: sim::TickInput) -> sim::Snapshot {
        self.tick(&input).0
    }

    /// `script` to run for `caller` at the end of the next step, and
    /// nothing else waiting.
    pub fn queue(&mut self, script: &str, caller: sim::SimId) {
        let mut state = self.world.script_state().clone();
        state.pending.clear();
        state.pending.push(sim::script::Pending::new(
            script,
            script_ncs::vm::ObjectId(caller.0),
        ));
        self.world.set_script_state(state);
    }

    /// The faults of the probe scripts (named `kjk_…`).
    pub fn faults(&self) -> Vec<String> {
        self.world
            .script_report()
            .faults
            .iter()
            .filter(|f| f.starts_with("kjk_"))
            .cloned()
            .collect()
    }
}
