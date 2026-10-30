//! `--droids chaos` (the remote droid's chain proof, `docs/superpowers/
//! specs/2026-10-29-remote-droid-design.md`): seats 1 and 2 held as
//! phantom guests (`SimWorld::set_phantoms`) that never take a companion
//! and only fly droids, each step flying a seeded random way and pressing
//! every button of the kit, aimed at the creature nearest it. The run's
//! story must come out as a plain run's.

use sim::combat::mind;
use sim::{Command, Seat, Snapshot, TickInput, buttons};

/// The phantom seats.
const SEATS: [u8; 2] = [1, 2];
/// How many seats the world holds for them.
pub(super) const HELD: u8 = 3;
/// How often each changes its way (steps).
const WAY_STEPS: u64 = 125;
/// How far it looks for a creature to aim at (m).
const SIGHT_M: f32 = 30.0;

/// The phantoms' dice: a seeded xorshift of their own, no sim stream.
pub(super) struct Chaos {
    seed: u64,
    step: u64,
    ways: [(i8, i8, i8); 2],
}

impl Chaos {
    pub(super) fn new(seed: u64) -> Chaos {
        Chaos {
            seed: seed.max(1),
            step: 0,
            ways: [(0, 0, 0); 2],
        }
    }

    fn roll(&mut self) -> i8 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        [-127, 0, 127][(self.seed % 3) as usize]
    }

    /// Seats 1 and 2's commands into `input`, from what `last` shows of
    /// their droids; the world told they are phantoms.
    pub(super) fn drive(&mut self, world: &mut sim::SimWorld, last: &Snapshot, input: &mut TickInput) {
        world.set_phantoms(SEATS.iter().fold(0, |mask, seat| mask | 1 << seat));
        // A minigame gives each seat its own run or gun, no droid: the
        // phantoms sit it out, as the chain proves droids, not co-op races.
        if last.minigame.is_some() {
            return;
        }
        if self.step.is_multiple_of(WAY_STEPS) {
            for at in 0..SEATS.len() {
                self.ways[at] = (self.roll(), self.roll(), self.roll());
            }
        }
        self.step += 1;
        for (at, seat) in SEATS.iter().enumerate() {
            let (forward, right, up) = self.ways[at];
            let angles = last
                .remote(Seat(*seat))
                .and_then(|remote| aim(last, remote.at))
                .map_or([0; 3], |(pitch, yaw)| sim::moves::view_angles(pitch, yaw));
            input.insert(
                Seat(*seat),
                Command {
                    angles,
                    forward,
                    right,
                    up,
                    buttons: buttons::ATTACK | buttons::ALT_ATTACK | buttons::MEDPAC,
                    ..Command::default()
                },
            );
        }
    }
}

/// The pitch and yaw from `from` (m) to the chest of the living creature
/// nearest it, outside the party, within `SIGHT_M`.
fn aim(last: &Snapshot, from: [f32; 3]) -> Option<(f32, f32)> {
    let away = |at: [f32; 3]| sim::math::length(sim::math::sub(at, from));
    last.entities
        .iter()
        .filter(|entity| entity.fighter.is_some_and(|fighter| fighter.alive()))
        .filter(|entity| !last.in_party(entity))
        .map(|entity| [entity.position[0], entity.position[1], entity.position[2] + 1.0])
        .filter(|at| away(*at) <= SIGHT_M)
        .min_by(|a, b| away(*a).total_cmp(&away(*b)))
        .map(|chest| mind::aim(from, chest))
}

/// What a run keeps beside its steps: the chaos droids, if asked for, and
/// the story at each module left (`story`), which a chaos run's must equal
/// a plain run's.
pub(super) struct Watch {
    chaos: Option<Chaos>,
    story: Vec<serde_json::Value>,
    /// The module the last step was in.
    module: Option<String>,
    /// Steps a droid flew in, and the kit's presses that went off (a
    /// button's wait set anew).
    flew: u64,
    fired: u64,
}

impl Watch {
    pub(super) fn new(droids: bool) -> Watch {
        Watch {
            chaos: droids.then(|| Chaos::new(1)),
            story: Vec::new(),
            module: None,
            flew: 0,
            fired: 0,
        }
    }

    /// Before a step: the story as a module is left (an install comes
    /// between steps), and the phantoms' commands.
    pub(super) fn before(&mut self, world: &mut sim::SimWorld, last: &Snapshot, input: &mut TickInput) {
        let module = last.place.as_ref().map(|place| place.module.clone());
        if self.module.is_some() && module != self.module {
            let left = self.module.clone();
            self.story.push(story(world, left));
        }
        self.module = module;
        if let Some(chaos) = &mut self.chaos {
            chaos.drive(world, last, input);
        }
    }

    /// After a step: what the droids did in it.
    pub(super) fn after(&mut self, last: &Snapshot, now: &Snapshot) {
        self.flew += u64::from(!now.remotes.is_empty());
        for remote in &now.remotes {
            let before = last.remote(remote.seat).map_or([0; 3], |was| was.ready_ms);
            self.fired += (0..3).filter(|kit| remote.ready_ms[*kit] > before[*kit]).count() as u64;
        }
    }

    /// The story as the run ends, last, and what the droids did.
    pub(super) fn finish(mut self, world: &sim::SimWorld) -> serde_json::Value {
        let module = self.module.take();
        self.story.push(story(world, module));
        serde_json::json!({
            "story": self.story,
            "droid_steps": self.flew,
            "kit_presses": self.fired,
        })
    }
}

/// The story as scripts keep it: the globals, the journal and the party,
/// and the module it is told in. Objects' locals and XP are left out: the
/// fights' own bookkeeping, which droids change by design.
fn story(world: &sim::SimWorld, module: Option<String>) -> serde_json::Value {
    let state = world.script_state();
    serde_json::json!({
        "module": module,
        "numbers": state.numbers,
        "booleans": state.booleans,
        "strings": state.strings,
        "journal": state.journal,
        "party": state.party.members,
    })
}
