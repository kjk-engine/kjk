//! A minigame in play: where each follower is on its track, its speed,
//! hit points and animations, the bullets in flight, and what the step's
//! hits were. It rides in the snapshot (`Snapshot::minigame`), so everyone
//! draws the same race, a save or recording carries it, and its hash
//! covers it.

use std::collections::BTreeMap;

use collision::trig;

use super::rules::{Hook, Kind, Rules, targets};
use crate::input::{Command, buttons};
use crate::math::{self, Vec3};
use crate::rng::SimRng;
use crate::snapshot::ObjectState;
use crate::wire::{DecodeError, Reader, Writer};

/// The most animations a thing keeps playing: the oldest go first. Room
/// for every name the scripts play (the swoop's timer digits, meter and
/// gears, about a hundred): a gear's hold (`gear2l`) must outlast the
/// digits played after it, or the base `gear0` under it shows again.
const ANIMS_KEPT: usize = 128;
/// How a swoop's sideways speed settles toward its push (per second): the
/// push over this is the speed it reaches [verify by feel].
const DRIFT_DAMPING: f32 = rules::minigames::swoop::DRIFT_DAMPING;
/// The most hits kept for the hooks that hear of them: one that runs a step
/// or more late still finds its own.
const FACTS_KEPT: usize = 64;
/// What pulls a TSL swoop's jump down (metres a second, a second: its
/// executable's constant).
pub const JUMP_GRAVITY: f32 = rules::minigames::swoop::JUMP_GRAVITY;
/// How far past touching a bump pushes the player out (metres: TSL's
/// executable adds it to the two radii).
const BUMP_ROOM: f32 = rules::minigames::bumps::ROOM;
/// The most times a step's bumps are pushed out, the first raising the
/// hooks (TSL's executable: again while a pass still finds one).
const BUMP_PASSES: usize = rules::minigames::bumps::PASSES;

/// An animation a script plays on a thing (`SWMG_PlayAnimation`): the views
/// play its clip on every one of the thing's models that has one.
#[derive(Debug, Clone, PartialEq)]
pub struct Anim {
    pub name: String,
    pub looping: bool,
    /// Over the others (the HUD's digits), not in place of them.
    pub overlay: bool,
    /// When it began (course ms).
    pub since: u64,
}

/// A follower in play (the player, an enemy).
#[derive(Debug, Clone, PartialEq)]
pub struct Mover {
    /// Seconds into its track.
    pub track: f32,
    /// Times through its track still to go; negative for ever.
    pub loops: i32,
    /// Across its track (a swoop: metres), or turned on it (a turret:
    /// degrees about x, y, z).
    pub offset: Vec3,
    /// How fast the offset moves (a swoop's drift, metres a second).
    pub drift: Vec3,
    pub speed: f32,
    pub min_speed: f32,
    pub max_speed: f32,
    pub accel: f32,
    pub tunnel_pos: Vec3,
    pub tunnel_neg: Vec3,
    pub hp: i32,
    pub max_hp: i32,
    pub radius: f32,
    pub invulnerable_until: u64,
    pub alive: bool,
    /// When it died (course ms); 0 while alive.
    pub died: u64,
    pub anims: Vec<Anim>,
    /// When each gun bank may fire next (course ms).
    pub shots: Vec<u64>,
    /// The things it touches (numbers), to tell a hit from a lingering one.
    pub touching: Vec<u16>,
}

impl Mover {
    /// Its offset held within its tunnel on each axis not infinite: the
    /// axes it was held on.
    fn keep_in_tunnel(&mut self, rules: &Rules) -> Vec<usize> {
        (0..3)
            .filter(|&axis| {
                let (low, high) = (self.tunnel_neg[axis], self.tunnel_pos[axis]);
                let out = self.offset[axis] < low || self.offset[axis] > high;
                if out && !rules.player.tunnel_infinite[axis] {
                    self.offset[axis] = self.offset[axis].clamp(low.min(high), high.max(low));
                    return true;
                }
                false
            })
            .collect()
    }
}

/// An obstacle in play.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Still {
    pub anims: Vec<Anim>,
    /// Taken off the course (`SWMG_DestroyMiniGameObject`): no longer hit
    /// nor drawn.
    pub gone: bool,
}

/// A bullet in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bullet {
    pub shooter: u16,
    pub bank: u8,
    /// The player's gun that fired it: 0 the first seat's, then the
    /// gunners' (`Course::gunners`); 0 for an enemy's.
    pub gunner: u8,
    pub from: Vec3,
    pub velocity: Vec3,
    pub born: u64,
    pub until: u64,
    pub damage: i32,
    pub target: u8,
}

impl Bullet {
    /// Where it is at `now` (course ms).
    pub fn at(&self, now: u64) -> Vec3 {
        let seconds = now.saturating_sub(self.born) as f32 / 1000.0;
        math::mad(self.from, seconds, self.velocity)
    }
}

/// What one hit was, for the `SWMG_GetLast…` routines of the hooks it ran.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Fact {
    /// The follower hit (a pad the player passed), or the obstacle.
    pub follower: Option<u16>,
    pub obstacle: Option<u16>,
    /// A bullet's damage, target and shooter.
    pub damage: i32,
    pub target: Option<u16>,
    pub shooter: Option<u16>,
    pub hp_change: i32,
}

/// A hook to run: whose (a thing's number), and which hit it hears of (a
/// fact's number from 1; 0 for none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raised {
    pub hook: Hook,
    /// Whose span (`minigame::id`): a run (`Course::run`; 0 the first
    /// seat's), or a gunner's lens.
    pub span: u8,
    pub who: u16,
    pub fact: i32,
}

/// A seat past the first at the turret (M11): its own aim and its own
/// copy of the player's gun banks, on the one hull.
#[derive(Debug, Clone, PartialEq)]
pub struct Gunner {
    pub seat: u8,
    /// The clips the player's heartbeat plays through its lens (its own
    /// compass and radar, M11), over the player's own.
    pub anims: Vec<Anim>,
    /// The globals that heartbeat keeps through its lens.
    pub overlay: Overlay,
    /// Its view's pitch and yaw when it began (`Course::aim_from`).
    pub aim_from: [i16; 2],
    /// Its turn on the hull (degrees about x, y, z), as the player's.
    pub offset: Vec3,
    pub fire_held: bool,
    /// When each of its banks may fire next (course ms).
    pub shots: Vec<u64>,
}

/// The globals a run's scripts set in a co-op race (M10), kept from the
/// other runs': the first run to finish has its committed as the race
/// leaves.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Overlay {
    pub numbers: BTreeMap<String, i32>,
    pub booleans: BTreeMap<String, bool>,
    pub strings: BTreeMap<String, String>,
}

/// A sound object as a co-op race's run's scripts set it (M10), over the
/// world's own: what each change set, and how many times they started it
/// (`ObjectState`'s sound fields). Each seat hears its own run's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Voice {
    pub object: u32,
    pub state: Option<u8>,
    pub volume: Option<u8>,
    pub pitch: Option<u16>,
    pub plays: u8,
    pub fade: Option<u16>,
}

/// The module the first run to finish started (M10), held until every run
/// has finished or `LEAVE_WAIT_MS` has passed.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaving {
    /// The run that finished first (`Course::run`), whose globals are
    /// committed as the race leaves.
    pub run: u8,
    /// The object that asked.
    pub by: u32,
    pub module: String,
    pub tag: String,
    /// The movies its `StartNewModule` plays, shown as the race leaves.
    pub movies: Vec<String>,
    /// When (course ms).
    pub since: u64,
}

/// How long a co-op race waits for the others once the first has finished
/// (ms).
pub const LEAVE_WAIT_MS: u64 = rules::minigames::coop::LEAVE_WAIT_MS;

#[derive(Debug, Clone, PartialEq)]
pub struct Course {
    pub time_ms: u64,
    /// Whether the things' create hooks have run.
    pub started: bool,
    /// The view's pitch and yaw when it began: a turret turns by how far
    /// the view turned since.
    pub aim_from: [i16; 2],
    pub fire_held: bool,
    /// Whether the jump key is down (a TSL swoop's `OnBrake`, on its press).
    pub jump_held: bool,
    pub lateral_accel: f32,
    /// A swoop's speed up its offset (`SWMG_SetJumpSpeed`, metres a second),
    /// pulled down by `JUMP_GRAVITY` in a TSL race (`Rules::tsl`).
    pub jump: f32,
    /// A bump pushed the player this step: its speed holds next step.
    pub bumped: bool,
    /// The speed blur's ratio; 0 off.
    pub blur: f32,
    pub clip: [f32; 2],
    /// By number: the player, the enemies.
    pub movers: Vec<Mover>,
    pub stills: Vec<Still>,
    pub bullets: Vec<Bullet>,
    /// The latest hits, by number (from 1, counting on across steps): the
    /// facts the hooks hear of.
    pub facts: Vec<(i32, Fact)>,
    pub next_fact: i32,
    /// The last of them, for a hook that hears of none.
    pub last: Fact,
    /// The seat whose run it is (`LEADER_SEAT`'s for the course itself).
    pub seat: u8,
    /// Its scripts' globals in a co-op race (M10).
    pub overlay: Overlay,
    /// When its script started the next module (course ms): it has
    /// finished.
    pub finished: Option<u64>,
    /// A co-op race's other runs, one per seat past the first, each a
    /// course of its own (M10); none in any other.
    pub runs: Vec<Course>,
    /// The turret's other seats (M11).
    pub gunners: Vec<Gunner>,
    /// The module a co-op race leaves for, held (M10).
    pub leaving: Option<Leaving>,
    /// The sound objects its scripts set in a co-op race (M10).
    pub voices: Vec<Voice>,
}

impl Course {
    /// The minigame `rules` describe, at its start.
    pub fn new(rules: &Rules) -> Course {
        let movers = std::iter::once(&rules.player)
            .chain(&rules.enemies)
            .map(|follower| Mover {
                track: 0.0,
                loops: follower.loops,
                offset: follower.start_offset,
                drift: [0.0; 3],
                speed: 0.0,
                min_speed: follower.min_speed,
                max_speed: follower.max_speed,
                accel: 0.0,
                tunnel_pos: follower.tunnel_pos,
                tunnel_neg: follower.tunnel_neg,
                hp: follower.hp,
                max_hp: follower.max_hp,
                radius: follower.radius,
                invulnerable_until: 0,
                alive: true,
                died: 0,
                anims: Vec::new(),
                shots: vec![0; follower.banks.len()],
                touching: Vec::new(),
            })
            .collect();
        Course {
            time_ms: 0,
            started: false,
            aim_from: [0; 2],
            fire_held: false,
            jump_held: false,
            lateral_accel: rules.lateral_accel,
            jump: 0.0,
            bumped: false,
            blur: 0.0,
            clip: rules.clip,
            movers,
            stills: vec![Still::default(); rules.obstacles.len()],
            bullets: Vec::new(),
            facts: Vec::new(),
            next_fact: 1,
            last: Fact::default(),
            seat: 0,
            overlay: Overlay::default(),
            finished: None,
            runs: Vec::new(),
            gunners: Vec::new(),
            leaving: None,
            voices: Vec::new(),
        }
    }

    /// Sound object `object`'s state as this run's scripts left it over
    /// the world's (`state`).
    pub fn voiced(&self, object: u32, mut state: ObjectState) -> ObjectState {
        if let Some(voice) = self.voices.iter().find(|voice| voice.object == object) {
            state.state = voice.state.unwrap_or(state.state);
            state.volume = voice.volume.unwrap_or(state.volume);
            state.pitch = voice.pitch.unwrap_or(state.pitch);
            state.plays = state.plays.wrapping_add(voice.plays);
            state.fade = voice.fade.unwrap_or(state.fade);
        }
        state
    }

    /// A change to sound object `object` its scripts made (`ObjectState`'s
    /// sound fields; `play`, a start).
    pub fn voice(&mut self, object: u32, change: Voice, play: bool) {
        let at = match self.voices.iter().position(|voice| voice.object == object) {
            Some(at) => at,
            None => {
                self.voices.push(Voice {
                    object,
                    state: None,
                    volume: None,
                    pitch: None,
                    plays: 0,
                    fade: None,
                });
                self.voices.len() - 1
            }
        };
        let voice = &mut self.voices[at];
        voice.state = change.state.or(voice.state);
        voice.volume = change.volume.or(voice.volume);
        voice.pitch = change.pitch.or(voice.pitch);
        voice.fade = change.fade.or(voice.fade);
        if play {
            voice.plays = voice.plays.wrapping_add(1);
        }
    }

    /// Run `k`: 0 the course itself, then its `runs`.
    pub fn run(&self, k: usize) -> Option<&Course> {
        match k {
            0 => Some(self),
            k => self.runs.get(k - 1),
        }
    }

    pub fn run_mut(&mut self, k: usize) -> Option<&mut Course> {
        match k {
            0 => Some(self),
            k => self.runs.get_mut(k - 1),
        }
    }

    /// The run `seat` plays: its own, else the first seat's (one who
    /// joined after the start watches it).
    pub fn run_of(&self, seat: u8) -> &Course {
        self.runs
            .iter()
            .find(|run| run.seat == seat)
            .unwrap_or(self)
    }

    /// The module a co-op race leaves for, once every run has finished or
    /// `LEAVE_WAIT_MS` has passed since the first did: taken (M10).
    pub fn leave_due(&mut self) -> Option<Leaving> {
        let since = self.leaving.as_ref()?.since;
        let all = std::iter::once(&*self)
            .chain(&self.runs)
            .all(|run| run.finished.is_some());
        if all || self.time_ms >= since + LEAVE_WAIT_MS {
            self.leaving.take()
        } else {
            None
        }
    }

    /// Whether it is a co-op race: runs past the first.
    pub fn racing_together(&self) -> bool {
        !self.runs.is_empty()
    }

    /// Whether it is a course of `rules` (one a save or a stale snapshot
    /// carries may not be): what everything reading both assumes.
    pub fn fits(&self, rules: &Rules) -> bool {
        self.movers.len() == rules.followers()
            && self.stills.len() == rules.obstacles.len()
            && std::iter::once(&rules.player)
                .chain(&rules.enemies)
                .zip(&self.movers)
                .all(|(follower, mover)| mover.shots.len() == follower.banks.len())
            && self
                .gunners
                .iter()
                .all(|gunner| gunner.shots.len() == rules.player.banks.len())
            && self.runs.len() < super::RUNS as usize
            && self.gunners.len() < usize::from(u8::MAX)
            && self
                .runs
                .iter()
                .all(|run| run.runs.is_empty() && run.fits(rules))
    }

    /// Where follower `n` is (metres): on its track, moved by its offset
    /// (a swoop's). TSL's executable hangs the models on the track's hook,
    /// so in a TSL race (`Rules::tsl`) the offset turns with the track (its
    /// tracks curve); K1's tracks run straight, the offset as it is
    /// [verify: K1's executable likely does the same].
    pub fn position(&self, rules: &Rules, n: usize) -> Option<Vec3> {
        let (follower, mover) = (rules.follower(n)?, self.movers.get(n)?);
        let on_track = follower.path.at(mover.track);
        Some(match rules.kind {
            Kind::Swoop if rules.tsl => math::add(
                on_track,
                rotate(follower.path.turn(mover.track), mover.offset),
            ),
            Kind::Swoop => math::add(on_track, mover.offset),
            Kind::Turret => on_track,
        })
    }

    /// Thing `n`'s place: a follower's, else an obstacle's.
    pub fn place(&self, rules: &Rules, n: usize) -> Option<Vec3> {
        self.position(rules, n).or_else(|| {
            rules
                .obstacles
                .get(n.checked_sub(rules.followers())?)
                .map(|obstacle| obstacle.position)
        })
    }

    /// Which way the player's guns fire: a swoop's +y (its banks shoot
    /// nothing), a turret's as its offset turns it (pitch about x, yaw about
    /// z, from +y) on its track's turn, as the view draws it.
    pub fn aim(&self, rules: &Rules) -> Vec3 {
        if self.movers.is_empty() {
            return [0.0, 1.0, 0.0];
        }
        match rules.kind {
            Kind::Swoop => [0.0, 1.0, 0.0],
            // Where the barrels meet.
            Kind::Turret => self.turned(rules, 0, rules.player.barrels().1),
        }
    }

    /// Where the player's bolts leave: its live banks' muzzles' middle, as
    /// the view turns them.
    pub fn muzzle(&self, rules: &Rules) -> Option<Vec3> {
        let at = self.position(rules, 0)?;
        Some(math::add(
            at,
            self.turned(rules, 0, rules.player.barrels().0),
        ))
    }

    /// Thing `n`'s animations to change: a follower's or an obstacle's.
    pub fn anims_mut(&mut self, rules: &Rules, n: usize) -> Option<&mut Vec<Anim>> {
        let followers = rules.followers();
        if n < followers {
            self.movers.get_mut(n).map(|mover| &mut mover.anims)
        } else {
            self.stills
                .get_mut(n - followers)
                .map(|still| &mut still.anims)
        }
    }

    /// How fast the player drifts across its track, as a share of the most
    /// it can at the lateral acceleration (−1 to 1; the view banks by it).
    pub fn drift_share(&self) -> f32 {
        let most = self.lateral_accel / DRIFT_DAMPING;
        match self.movers.first() {
            Some(mover) if most > 0.0 => (mover.drift[0] / most).clamp(-1.0, 1.0),
            _ => 0.0,
        }
    }

    /// Plays `anim` on thing `n` (`SWMG_PlayAnimation`): an overlay over
    /// the rest, else in place of the last one that was not.
    pub fn play(&mut self, rules: &Rules, n: usize, anim: Anim) {
        if let Some(anims) = self.anims_mut(rules, n) {
            play_into(rules, n, anims, anim);
        }
    }

    /// The animations playing on the player as gunner `gunner` (from 1)
    /// sees them: the player's, and those its lens played over them (M11);
    /// the player's own for 0.
    pub fn lens_anims(&self, rules: &Rules, gunner: usize) -> Vec<Anim> {
        let mut anims = self
            .movers
            .first()
            .map_or_else(Vec::new, |mover| mover.anims.clone());
        if let Some(lens) = gunner.checked_sub(1).and_then(|k| self.gunners.get(k)) {
            for anim in &lens.anims {
                play_into(rules, 0, &mut anims, anim.clone());
            }
        }
        anims
    }

    /// Stops `name` on thing `n` (`SWMG_RemoveAnimation`).
    pub fn stop(&mut self, rules: &Rules, n: usize, name: &str) {
        if let Some(anims) = self.anims_mut(rules, n) {
            anims.retain(|playing| !playing.name.eq_ignore_ascii_case(name));
        }
    }

    /// A new fact, its number.
    fn fact(&mut self, fact: Fact) -> i32 {
        let number = self.next_fact;
        self.next_fact = self.next_fact.wrapping_add(1).max(1);
        self.facts.push((number, fact));
        if self.facts.len() > FACTS_KEPT {
            self.facts.remove(0);
        }
        self.last = fact;
        number
    }

    /// The fact numbered `n`, else the last.
    pub fn fact_of(&self, n: i32) -> Fact {
        self.facts
            .iter()
            .find(|(number, _)| *number == n)
            .map_or(self.last, |(_, fact)| *fact)
    }

    /// Whether thing `n` has a script on `hook`.
    fn has(rules: &Rules, n: usize, hook: Hook) -> bool {
        rules.hooks(n).and_then(|hooks| hooks.get(hook)).is_some()
    }

    /// Thing `n`'s own `OnHitFollower`, the player having met it: its last
    /// follower hit the player (and, an obstacle, itself its `obstacle`).
    /// The hit is kept only for a script to hear (none changes nothing).
    fn hit_follower(
        &mut self,
        rules: &Rules,
        raised: &mut Vec<Raised>,
        n: usize,
        obstacle: Option<u16>,
    ) {
        if Course::has(rules, n, Hook::HitFollower) {
            let fact = self.fact(Fact {
                follower: Some(0),
                obstacle,
                ..Fact::default()
            });
            Course::raise(rules, raised, Hook::HitFollower, n, fact);
        }
    }

    /// `n`'s hook, if it has a script there (run 0's: `step` numbers a
    /// run's own).
    fn raise(rules: &Rules, raised: &mut Vec<Raised>, hook: Hook, n: usize, fact: i32) -> bool {
        let has = Course::has(rules, n, hook);
        if has {
            raised.push(Raised {
                hook,
                span: 0,
                who: n as u16,
                fact,
            });
        }
        has
    }

    /// A bullet (fact `fact`) hit follower `n`: its hit hook, else the
    /// default (`SWMG_OnBulletHit`): the damage.
    fn bullet_hit(&mut self, rules: &Rules, n: usize, fact: i32, raised: &mut Vec<Raised>) {
        if !Course::raise(rules, raised, Hook::HitBullet, n, fact) {
            self.damage(rules, n, fact, raised);
        }
    }

    /// Follower `n`'s hit points change by fact `fact`'s `hp_change`: its
    /// damage hook, else the default (`SWMG_OnDamage`).
    pub fn damage(&mut self, rules: &Rules, n: usize, fact: i32, raised: &mut Vec<Raised>) {
        if !Course::raise(rules, raised, Hook::Damage, n, fact) {
            self.on_damage(rules, n, fact, raised);
        }
    }

    /// The default damage (`SWMG_OnDamage`), as both executables have it:
    /// fact `fact`'s `hp_change` added to `n`'s hit points, held at its
    /// maximum when it adds; once they run out, its death hook, else the
    /// default death. Invulnerability is no part of it (only bumps and the
    /// scripts read it).
    pub fn on_damage(&mut self, rules: &Rules, n: usize, fact: i32, raised: &mut Vec<Raised>) {
        let change = self.fact_of(fact).hp_change;
        let Some(mover) = self.movers.get_mut(n).filter(|mover| mover.alive) else {
            return;
        };
        let was = mover.hp;
        mover.hp = mover.hp.saturating_add(change);
        if change >= 0 {
            mover.hp = mover.hp.min(mover.max_hp);
        } else if mover.hp <= 0 && was > 0 {
            // Dying as they run out, not again while none are left.
            self.died(rules, n, fact, raised);
        }
    }

    /// Follower `n`'s hit points changed by `change`
    /// (`SWMG_AdjustFollowerHitPoints`, and the bumps): heard as a hit,
    /// through its damage hook.
    pub fn adjust_hp(&mut self, rules: &Rules, n: usize, change: i32, raised: &mut Vec<Raised>) {
        let fact = self.fact(Fact {
            damage: change.saturating_neg(),
            hp_change: change,
            ..Fact::default()
        });
        self.damage(rules, n, fact, raised);
    }

    /// Follower `n` has no hit points left: its death hook, else the
    /// default death.
    pub fn died(&mut self, rules: &Rules, n: usize, fact: i32, raised: &mut Vec<Raised>) {
        if !Course::raise(rules, raised, Hook::Death, n, fact) {
            self.on_death(n);
        }
    }

    /// The default death (`SWMG_OnDeath`): it is gone from the course.
    pub fn on_death(&mut self, n: usize) {
        let now = self.time_ms;
        if let Some(mover) = self.movers.get_mut(n)
            && mover.alive
        {
            mover.alive = false;
            mover.died = now;
        }
    }

    /// Sets follower `n`'s hit points (`SWMG_SetFollowerHitPoints`), as both
    /// executables do: only to more than none (a race's crash script setting
    /// its swoop's to 0 leaves them), nothing heard, no death.
    pub fn set_hp(&mut self, n: usize, hp: i32) {
        if let Some(mover) = self.movers.get_mut(n).filter(|_| hp > 0) {
            mover.hp = hp;
        }
    }

    /// Takes obstacle `n` off the course (`SWMG_DestroyMiniGameObject`): no
    /// longer hit nor drawn. A follower stays (TSL's executable removes an
    /// obstacle only).
    pub fn destroy(&mut self, rules: &Rules, n: usize) {
        if let Some(still) = n
            .checked_sub(rules.followers())
            .and_then(|k| self.stills.get_mut(k))
        {
            still.gone = true;
        }
    }

    /// The player back at the start of its track, still
    /// (`SWMG_SetFollowerPosition`, which TSL's executable runs whatever
    /// vector it is given: QUIRKS.md Q-0259): its track's clip at 0, its
    /// offset and its drift none.
    pub fn restart(&mut self) {
        if let Some(mover) = self.movers.first_mut() {
            mover.track = 0.0;
            mover.offset = [0.0; 3];
            mover.drift = [0.0; 3];
        }
    }

    /// A push on the player (`SWMG_PlayerApplyForce`, TSL's executable): its
    /// drift across and the lateral acceleration both `force`'s x, its jump
    /// speed its z; its y is not read.
    pub fn apply_force(&mut self, force: Vec3) {
        if let Some(mover) = self.movers.first_mut() {
            mover.drift[0] = force[0];
        }
        self.lateral_accel = force[0];
        self.jump = force[2];
    }

    /// One step of `msec` ms at sim tick `tick`, each seat's command by
    /// seat (`commands`; the course's own seat steers its player): the hooks
    /// it and its runs raise, in order. Every other seat gets a run of its
    /// own in a race (M10; one coming late, until anyone has finished, its
    /// run from its own start) and a gun on the hull at the turret (M11);
    /// the fighters are scaled for the gunners there at the start
    /// (`Rules::per_player_hp`).
    pub fn step(
        &mut self,
        rules: &Rules,
        commands: &[(u8, Command)],
        msec: u32,
        tick: u64,
    ) -> Vec<Raised> {
        let others = commands.iter().filter(|(seat, _)| *seat != self.seat);
        match rules.kind {
            // One coming before anyone has finished races too, from its
            // own start.
            Kind::Swoop if self.leaving.is_none() => {
                for &(seat, _) in others {
                    if self.runs.len() + 1 < super::RUNS as usize
                        && self.runs.iter().all(|run| run.seat != seat)
                    {
                        self.runs.push(Course {
                            seat,
                            ..Course::new(rules)
                        });
                    }
                }
            }
            Kind::Swoop => {}
            Kind::Turret => {
                let before = self.gunners.len();
                for &(seat, command) in others {
                    if self.gunners.len() + 1 < usize::from(u8::MAX)
                        && self.gunners.iter().all(|gunner| gunner.seat != seat)
                    {
                        self.gunners.push(Gunner {
                            seat,
                            anims: Vec::new(),
                            overlay: Overlay::default(),
                            aim_from: [command.angles[0], command.angles[1]],
                            offset: rules.player.start_offset,
                            fire_held: false,
                            shots: vec![0; rules.player.banks.len()],
                        });
                    }
                }
                self.scale_enemies(rules, (before, self.gunners.len()));
            }
        }
        let own = |seat: u8| {
            commands
                .iter()
                .find(|(own, _)| *own == seat)
                .map(|(_, command)| *command)
        };
        let gunning: Vec<(usize, Command)> = self
            .gunners
            .iter()
            .enumerate()
            .filter_map(|(k, gunner)| Some((k, own(gunner.seat)?)))
            .collect();
        let mut raised = self.step_run(rules, own(self.seat).as_ref(), &gunning, msec, tick);
        // Each gunner's lens runs the player's heartbeat too: its own
        // compass from its own aim.
        let beating = raised
            .iter()
            .any(|r| r.hook == Hook::Heartbeat && r.span == 0 && r.who == 0);
        if beating {
            raised.extend((1..=self.gunners.len()).map(|gunner| Raised {
                hook: Hook::Heartbeat,
                span: super::lens(gunner) as u8,
                who: 0,
                fact: 0,
            }));
        }
        for (k, run) in self.runs.iter_mut().enumerate() {
            let command = own(run.seat);
            raised.extend(
                run.step_run(rules, command.as_ref(), &[], msec, tick)
                    .into_iter()
                    .map(|raised| Raised {
                        span: k as u8 + 1,
                        ..raised
                    }),
            );
        }
        raised
    }

    /// The fighters' hit points as the gunners went from `was` to `now`:
    /// each past the first adds `Rules::per_player_hp` of a fighter's own
    /// maximum, as an encounter's `per_player hp` (M11); one joining
    /// mid-fight adds its share to each fighter still flying, hurt or not.
    fn scale_enemies(&mut self, rules: &Rules, (was, now): (usize, usize)) {
        if now == was {
            return;
        }
        let share = |gunners: usize, base: i32| {
            (base as f32 * (1.0 + rules.per_player_hp * gunners as f32)).round() as i32
        };
        for (mover, enemy) in self.movers.iter_mut().skip(1).zip(&rules.enemies) {
            if !mover.alive || mover.hp <= 0 {
                continue;
            }
            let added = share(now, enemy.max_hp) - share(was, enemy.max_hp);
            mover.max_hp += added;
            mover.hp += added;
        }
    }

    /// One step of this run: its player steered by `command`, the turret's
    /// gunners by theirs (`gunning`: gunner, command).
    fn step_run(
        &mut self,
        rules: &Rules,
        command: Option<&Command>,
        gunning: &[(usize, Command)],
        msec: u32,
        tick: u64,
    ) -> Vec<Raised> {
        let mut raised = Vec::new();
        self.time_ms += u64::from(msec);
        let dt = msec as f32 / 1000.0;
        let command = command.copied().unwrap_or(Command::default());
        if !self.started {
            self.started = true;
            self.aim_from = [command.angles[0], command.angles[1]];
            for n in 0..rules.followers() + rules.obstacles.len() {
                Course::raise(rules, &mut raised, Hook::Create, n, 0);
            }
        }
        let fire = command.buttons & buttons::ATTACK != 0;
        if fire && !self.fire_held {
            Course::raise(rules, &mut raised, self.shift_hook(rules), 0, 0);
        }
        self.fire_held = fire;
        // TSL's jump key (its `MGshoot`) runs a swoop's `OnBrake`: the
        // races' jump.
        let jump = command.up > 0;
        if jump && !self.jump_held && rules.kind == Kind::Swoop {
            Course::raise(rules, &mut raised, Hook::Brake, 0, 0);
        }
        self.jump_held = jump;
        let before: Vec<Option<Vec3>> = (0..self.movers.len())
            .map(|n| self.position(rules, n))
            .collect();
        self.steer(rules, &command, dt);
        for &(k, command) in gunning {
            let gunner = &mut self.gunners[k];
            turn_turret(rules, &mut gunner.aim_from, &mut gunner.offset, &command);
        }
        self.move_followers(rules, dt, &mut raised);
        self.shoot(rules, fire, tick);
        self.gunners_shoot(rules, gunning);
        self.fly(rules, &mut raised);
        self.touch(rules, &before, &mut raised);
        if rules.do_bumping {
            for pass in 0..BUMP_PASSES {
                if !self.bump(rules, &before, pass == 0, &mut raised) {
                    break;
                }
            }
        }
        for n in 0..rules.followers() {
            if self.movers[n].alive {
                Course::raise(rules, &mut raised, Hook::Heartbeat, n, 0);
            }
        }
        raised
    }

    /// The hook the fire key's press raises on the player: a swoop's
    /// `OnAccelerate` when it has one (TSL's executable sends the swoop's
    /// fire press there: its races' gear shift), else its `OnFire` (K1's
    /// gear shift, the turret's).
    fn shift_hook(&self, rules: &Rules) -> Hook {
        match rules.kind {
            Kind::Swoop if rules.player.hooks.get(Hook::Accelerate).is_some() => Hook::Accelerate,
            _ => Hook::Fire,
        }
    }

    /// The player's offset as `command` moves it: a swoop drifts across its
    /// track and, in a TSL race, rises at its jump speed as `JUMP_GRAVITY`
    /// pulls it down (TSL's executable: the speed is not stopped on
    /// landing); a turret turns as the view does.
    fn steer(&mut self, rules: &Rules, command: &Command, dt: f32) {
        let (accel, jump) = (self.lateral_accel, self.jump);
        let Some(mover) = self.movers.first_mut() else {
            return;
        };
        if !mover.alive {
            return;
        }
        match rules.kind {
            Kind::Swoop => {
                let push = f32::from(command.right) / 127.0 * accel;
                mover.drift[0] = if rules.use_inertia {
                    mover.drift[0] + (push - DRIFT_DAMPING * mover.drift[0]) * dt
                } else {
                    push / DRIFT_DAMPING
                };
                mover.offset[0] += mover.drift[0] * dt;
                if rules.tsl {
                    mover.offset[2] += jump * dt;
                    self.jump += JUMP_GRAVITY * dt;
                }
                for axis in mover.keep_in_tunnel(rules) {
                    mover.drift[axis] = 0.0;
                }
            }
            Kind::Turret => {
                let tunnel = (mover.tunnel_neg, mover.tunnel_pos);
                turn_within(
                    rules,
                    tunnel,
                    &mut self.aim_from,
                    &mut mover.offset,
                    command,
                );
            }
        }
    }

    /// Each follower along its track: the player at its speed, the others
    /// at their track's own pace.
    fn move_followers(&mut self, rules: &Rules, dt: f32, raised: &mut Vec<Raised>) {
        for n in 0..self.movers.len() {
            let Some(follower) = rules.follower(n) else {
                continue;
            };
            let mover = &mut self.movers[n];
            if !mover.alive {
                continue;
            }
            let pace = if n == 0 {
                // Toward the speed range at the acceleration: up to the
                // maximum, or down to it when above; not the step after a
                // bump (TSL's executable).
                let step = mover.accel * dt;
                if std::mem::take(&mut self.bumped) {
                    // Held.
                } else if mover.speed > mover.max_speed {
                    mover.speed = (mover.speed - step).max(mover.max_speed);
                } else {
                    mover.speed = (mover.speed + step).min(mover.max_speed);
                }
                if rules.movement_per_sec > 0.0 {
                    mover.speed / rules.movement_per_sec
                } else {
                    0.0
                }
            } else {
                1.0
            };
            mover.track += pace * dt;
            let length = follower.path.length;
            if length > 0.0 && mover.track >= length {
                if mover.loops < 0 || mover.loops > 1 {
                    mover.track %= length;
                    if mover.loops > 1 {
                        mover.loops -= 1;
                    }
                    Course::raise(rules, raised, Hook::TrackLoop, n, 0);
                } else {
                    mover.track = length;
                }
            }
        }
    }

    /// The gun banks: the player's while fire is held, an enemy's at the
    /// player when it is near and ahead (strayed by a roll of the tick's).
    fn shoot(&mut self, rules: &Rules, fire: bool, tick: u64) {
        let now = self.time_ms;
        // Where enemies aim: the player's middle moved by its
        // `Target_Offset` along its track (the turret's 5 below the
        // gunner's eye) [verify].
        let player = self.position(rules, 0).map_or([0.0; 3], |at| {
            let turn = self.movers.first().map_or([0.0, 0.0, 0.0, 1.0], |mover| {
                rules.player.path.turn(mover.track)
            });
            math::add(at, rotate(turn, rules.player.target_offset))
        });
        for n in 0..self.movers.len() {
            let Some(follower) = rules.follower(n) else {
                continue;
            };
            if !self.movers[n].alive {
                continue;
            }
            let at = self.position(rules, n).unwrap_or([0.0; 3]);
            for (b, bank) in follower.banks.iter().enumerate() {
                if !bank.live() || self.movers[n].shots[b] > now {
                    continue;
                }
                let from = math::add(at, self.turned(rules, n, bank.muzzle));
                let direction = if n == 0 {
                    if !fire {
                        continue;
                    }
                    // Along its barrel (the turret's two meet on the
                    // crosshair's line).
                    let mut way = self.turned(rules, 0, bank.way);
                    math::normalize(&mut way);
                    way
                } else {
                    let mut toward = math::sub(player, at);
                    let distance = math::normalize(&mut toward);
                    if distance > bank.sensing || !self.ahead(rules, n, toward, bank.spread) {
                        continue;
                    }
                    let mut roll = SimRng::new(tick, (n as u64) << 8 | b as u64);
                    let stray = [0, 1, 2].map(|_| roll.range(-1.0, 1.0) * bank.inaccuracy);
                    let mut direction = math::add(toward, stray);
                    math::normalize(&mut direction);
                    direction
                };
                self.movers[n].shots[b] = now + u64::from(bank.rate_ms);
                self.bullets.push(Bullet {
                    shooter: n as u16,
                    bank: b as u8,
                    gunner: 0,
                    from,
                    velocity: math::scale(direction, bank.speed),
                    born: now,
                    until: now + u64::from(bank.lifespan_ms),
                    damage: bank.damage,
                    target: bank.target,
                });
            }
        }
    }

    /// `v` in follower `n`'s space turned as it is drawn: by its track, and
    /// the turret's by the view too (yaw about z, then pitch about x).
    fn turned(&self, rules: &Rules, n: usize, v: Vec3) -> Vec3 {
        let offset = self.movers.get(n).map_or([0.0; 3], |mover| mover.offset);
        self.turned_by(rules, n, offset, v)
    }

    /// `v` turned as `turned` does, the turret by `offset` (a gunner's).
    fn turned_by(&self, rules: &Rules, n: usize, offset: Vec3, v: Vec3) -> Vec3 {
        let (Some(follower), Some(mover)) = (rules.follower(n), self.movers.get(n)) else {
            return v;
        };
        let v = if rules.kind == Kind::Turret && n == 0 {
            let (sp, cp) = trig::sin_cos_deg(offset[0]);
            let (sy, cy) = trig::sin_cos_deg(offset[2]);
            let pitched = [v[0], v[1] * cp - v[2] * sp, v[1] * sp + v[2] * cp];
            [
                pitched[0] * cy - pitched[1] * sy,
                pitched[0] * sy + pitched[1] * cy,
                pitched[2],
            ]
        } else {
            v
        };
        rotate(follower.path.turn(mover.track), v)
    }

    /// The gunners' banks while their fire is held (`gunning`: gunner,
    /// command), each along its barrel as the gunner turns the hull's gun:
    /// the player's bolts, numbered by gunner.
    fn gunners_shoot(&mut self, rules: &Rules, gunning: &[(usize, Command)]) {
        let now = self.time_ms;
        if !self.movers.first().is_some_and(|mover| mover.alive) {
            return;
        }
        let at = self.position(rules, 0).unwrap_or([0.0; 3]);
        for &(k, command) in gunning {
            let fire = command.buttons & buttons::ATTACK != 0;
            self.gunners[k].fire_held = fire;
            if !fire {
                continue;
            }
            let offset = self.gunners[k].offset;
            for (b, bank) in rules.player.banks.iter().enumerate() {
                if !bank.live() || self.gunners[k].shots[b] > now {
                    continue;
                }
                let from = math::add(at, self.turned_by(rules, 0, offset, bank.muzzle));
                let mut way = self.turned_by(rules, 0, offset, bank.way);
                math::normalize(&mut way);
                self.gunners[k].shots[b] = now + u64::from(bank.rate_ms);
                self.bullets.push(Bullet {
                    shooter: 0,
                    bank: b as u8,
                    gunner: k as u8 + 1,
                    from,
                    velocity: math::scale(way, bank.speed),
                    born: now,
                    until: now + u64::from(bank.lifespan_ms),
                    damage: bank.damage,
                    target: bank.target,
                });
            }
        }
    }

    /// Which way gunner `k`'s barrels meet (`aim` for a gunner: 0 the
    /// first seat's).
    pub fn aim_of(&self, rules: &Rules, k: usize) -> Vec3 {
        match k.checked_sub(1).and_then(|k| self.gunners.get(k)) {
            Some(gunner) if !self.movers.is_empty() => {
                self.turned_by(rules, 0, gunner.offset, rules.player.barrels().1)
            }
            _ => self.aim(rules),
        }
    }

    /// Where gunner `k`'s bolts leave (`muzzle` for a gunner).
    pub fn muzzle_of(&self, rules: &Rules, k: usize) -> Option<Vec3> {
        match k.checked_sub(1).and_then(|k| self.gunners.get(k)) {
            Some(gunner) => Some(math::add(
                self.position(rules, 0)?,
                self.turned_by(rules, 0, gunner.offset, rules.player.barrels().0),
            )),
            None => self.muzzle(rules),
        }
    }

    /// Whether `toward` (a unit vector) is within enemy `n`'s firing cone
    /// about the way its track takes it.
    fn ahead(&self, rules: &Rules, n: usize, toward: Vec3, spread: [f32; 2]) -> bool {
        let (Some(follower), Some(mover)) = (rules.follower(n), self.movers.get(n)) else {
            return false;
        };
        let mut heading = math::sub(
            follower.path.at(mover.track + 0.05),
            follower.path.at(mover.track),
        );
        if math::normalize(&mut heading) <= f32::EPSILON {
            return true;
        }
        let across = trig::atan2_deg(
            heading[0] * toward[1] - heading[1] * toward[0],
            heading[0] * toward[0] + heading[1] * toward[1],
        )
        .abs();
        let flat = |v: Vec3| (v[0] * v[0] + v[1] * v[1]).sqrt();
        let up = (trig::atan2_deg(toward[2], flat(toward))
            - trig::atan2_deg(heading[2], flat(heading)))
        .abs();
        across <= spread[0] / 2.0 && up <= spread[1] / 2.0
    }

    /// Bullets fly: one that reaches a follower it may hit is spent there.
    fn fly(&mut self, rules: &Rules, raised: &mut Vec<Raised>) {
        let now = self.time_ms;
        let bullets = std::mem::take(&mut self.bullets);
        let mut flying = Vec::with_capacity(bullets.len());
        for bullet in bullets {
            if now >= bullet.until {
                continue;
            }
            let at = bullet.at(now);
            let hit = (0..self.movers.len()).find(|&n| {
                let may = if n == 0 {
                    bullet.target & targets::PLAYER != 0
                } else {
                    bullet.target & targets::ENEMIES != 0
                };
                may && n != usize::from(bullet.shooter)
                    && self.movers[n].alive
                    && self.movers[n].hp > 0
                    && self.position(rules, n).is_some_and(|there| {
                        math::length(math::sub(there, at)) <= self.movers[n].radius
                    })
            });
            match hit {
                Some(n) => {
                    let fact = self.fact(Fact {
                        damage: bullet.damage,
                        target: Some(n as u16),
                        shooter: Some(bullet.shooter),
                        hp_change: bullet.damage.saturating_neg(),
                        ..Fact::default()
                    });
                    self.bullet_hit(rules, n, fact, raised);
                }
                None => flying.push(bullet),
            }
        }
        self.bullets = flying;
    }

    /// The player meets enemies it may pass through and obstacles, each
    /// newly touched heard of: a pad by the player's `OnHitFollower`, then
    /// its own (each side's last follower hit the other: Nar Shaddaa's pads
    /// have only their own); an obstacle by its own `OnHitFollower` if it
    /// has one, else the player's `OnHitObstacle` (both executables: one or
    /// the other). K1's races meet them across the ground, the player's
    /// sphere on a pad's or an obstacle's box (heights left out [verify]);
    /// a TSL race (`Rules::tsl`) as its executable does, along the ways
    /// they came this step (`before`): a pad where the two spheres' ways
    /// come within their radii, an obstacle where the player's middle
    /// passes into its box (a jump clears them).
    fn touch(&mut self, rules: &Rules, before: &[Option<Vec3>], raised: &mut Vec<Raised>) {
        let Some(player) = self.position(rules, 0) else {
            return;
        };
        if !self.movers[0].alive {
            return;
        }
        let was = before.first().copied().flatten().unwrap_or(player);
        let radius = self.movers[0].radius;
        let followers = rules.followers();
        let mut touching = Vec::new();
        // How far the player's middle is from a box (its middle, half
        // sizes) across the ground.
        let off = |centre: Vec3, extent: [f32; 2]| {
            let [dx, dy, _] = math::sub(centre, player);
            let (x, y) = (
                (dx.abs() - extent[0]).max(0.0),
                (dy.abs() - extent[1]).max(0.0),
            );
            (x * x + y * y).sqrt()
        };
        for n in 1..followers + rules.obstacles.len() {
            let touches = if n < followers {
                let mover = &self.movers[n];
                // Only pads: the bumps are `bump`'s.
                if !mover.alive || !rules.enemies[n - 1].trigger {
                    continue;
                }
                let reach = radius + mover.radius;
                self.position(rules, n).is_some_and(|there| {
                    if rules.tsl {
                        let then = before.get(n).copied().flatten().unwrap_or(there);
                        closest_between((was, player), (then, there)) < reach
                    } else {
                        off(there, [0.0; 2]) <= reach
                    }
                })
            } else {
                let k = n - followers;
                let obstacle = &rules.obstacles[k];
                let meets = if rules.tsl {
                    passes_into((was, player), obstacle.centre, obstacle.extent)
                } else {
                    off(obstacle.centre, [obstacle.extent[0], obstacle.extent[1]]) <= radius
                };
                !self.stills[k].gone && meets
            };
            if touches {
                touching.push(n as u16);
            }
        }
        for &n in &touching {
            if self.movers[0].touching.contains(&n) {
                continue;
            }
            let n = usize::from(n);
            if n < followers {
                let fact = self.fact(Fact {
                    follower: Some(n as u16),
                    ..Fact::default()
                });
                Course::raise(rules, raised, Hook::HitFollower, 0, fact);
                self.hit_follower(rules, raised, n, None);
            } else if Course::has(rules, n, Hook::HitFollower) {
                self.hit_follower(rules, raised, n, Some(n as u16));
            } else {
                let fact = self.fact(Fact {
                    obstacle: Some(n as u16),
                    ..Fact::default()
                });
                Course::raise(rules, raised, Hook::HitObstacle, 0, fact);
                // The obstacle's own `hit` clip, if its model has one
                // [verify: neither executable plays it; K1's buoys have
                // no script that does].
                let since = self.time_ms;
                self.play(
                    rules,
                    n,
                    Anim {
                        name: "hit".into(),
                        looping: false,
                        overlay: false,
                        since,
                    },
                );
            }
        }
        self.movers[0].touching = touching;
    }

    /// One pass of the bumps (`DoBumping`, TSL's races: mines and traffic),
    /// as TSL's executable makes them: the player meets each enemy it may
    /// not pass through whose way this step came within their two radii of
    /// its own (`before`: where each was as the step began). On the first
    /// pass (`hooks`) an enemy out of its invulnerability takes the
    /// player's `Bump_Damage` through its damage hook and is invulnerable
    /// its period, then each hears of the other (the player's
    /// `OnHitFollower`, then the enemy's). Each pass pushes the player out
    /// along the line from each, to `BUMP_ROOM` past touching, by the
    /// pushes' mean (the world's, added to its track's offset: QUIRKS.md
    /// Q-0260), held in its tunnel; the player out of its
    /// invulnerability is invulnerable twice its period and, on the first
    /// pass, takes the most `Bump_Damage` of those it met and holds its
    /// speed next step (`bumped`). Whether it met any.
    fn bump(
        &mut self,
        rules: &Rules,
        before: &[Option<Vec3>],
        hooks: bool,
        raised: &mut Vec<Raised>,
    ) -> bool {
        let Some(now) = self.position(rules, 0).filter(|_| self.movers[0].alive) else {
            return false;
        };
        let was = before.first().copied().flatten().unwrap_or(now);
        let radius = self.movers[0].radius;
        let (mut push, mut met, mut worst) = ([0.0; 3], 0u32, None::<(usize, i32)>);
        for n in 1..rules.followers() {
            let (enemy, there) = (&rules.enemies[n - 1], self.position(rules, n));
            let Some(there) = there.filter(|_| !enemy.trigger && self.movers[n].alive) else {
                continue;
            };
            let then = before.get(n).copied().flatten().unwrap_or(there);
            let reach = radius + self.movers[n].radius;
            if closest_between((was, now), (then, there)) >= reach {
                continue;
            }
            if hooks {
                if self.movers[n].invulnerable_until <= self.time_ms {
                    let damage = rules.player.bump_damage;
                    let fact = self.fact(Fact {
                        damage,
                        target: Some(n as u16),
                        shooter: Some(0),
                        hp_change: damage.saturating_neg(),
                        ..Fact::default()
                    });
                    self.damage(rules, n, fact, raised);
                    self.movers[n].invulnerable_until = self.time_ms + u64::from(enemy.invince_ms);
                }
                let fact = self.fact(Fact {
                    follower: Some(n as u16),
                    ..Fact::default()
                });
                Course::raise(rules, raised, Hook::HitFollower, 0, fact);
                if worst.is_none_or(|(_, most)| enemy.bump_damage > most) {
                    worst = Some((n, enemy.bump_damage));
                }
                self.hit_follower(rules, raised, n, None);
            }
            let mut away = math::sub(now, there);
            math::normalize(&mut away);
            push = math::add(
                push,
                math::sub(math::mad(there, reach + BUMP_ROOM, away), now),
            );
            met += 1;
        }
        if met == 0 {
            return false;
        }
        let now_ms = self.time_ms;
        let player = &mut self.movers[0];
        player.offset = math::mad(player.offset, 1.0 / met as f32, push);
        player.keep_in_tunnel(rules);
        if player.invulnerable_until <= now_ms {
            player.invulnerable_until = now_ms
                + rules::minigames::bumps::INVULNERABLE_TIMES * u64::from(rules.player.invince_ms);
            if let Some((by, damage)) = worst {
                let fact = self.fact(Fact {
                    damage,
                    target: Some(0),
                    shooter: Some(by as u16),
                    hp_change: damage.saturating_neg(),
                    ..Fact::default()
                });
                self.damage(rules, 0, fact, raised);
                self.bumped = true;
            }
        }
        true
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u64(self.time_ms);
        out.u8(u8::from(self.started)
            | u8::from(self.fire_held) << 1
            | u8::from(self.jump_held) << 2
            | u8::from(self.bumped) << 3);
        out.u16(self.aim_from[0] as u16);
        out.u16(self.aim_from[1] as u16);
        out.f32(self.lateral_accel);
        out.f32(self.jump);
        out.f32(self.blur);
        out.f32(self.clip[0]);
        out.f32(self.clip[1]);
        out.u32(self.movers.len() as u32);
        for mover in &self.movers {
            out.f32(mover.track);
            out.u32(mover.loops as u32);
            vec3(out, mover.offset);
            vec3(out, mover.drift);
            for value in [mover.speed, mover.min_speed, mover.max_speed, mover.accel] {
                out.f32(value);
            }
            vec3(out, mover.tunnel_pos);
            vec3(out, mover.tunnel_neg);
            out.u32(mover.hp as u32);
            out.u32(mover.max_hp as u32);
            out.f32(mover.radius);
            out.u64(mover.invulnerable_until);
            out.u8(u8::from(mover.alive));
            out.u64(mover.died);
            encode_anims(out, &mover.anims);
            out.u32(mover.shots.len() as u32);
            for shot in &mover.shots {
                out.u64(*shot);
            }
            out.u32(mover.touching.len() as u32);
            for n in &mover.touching {
                out.u16(*n);
            }
        }
        out.u32(self.stills.len() as u32);
        for still in &self.stills {
            out.u8(u8::from(still.gone));
            encode_anims(out, &still.anims);
        }
        out.u32(self.bullets.len() as u32);
        for bullet in &self.bullets {
            out.u16(bullet.shooter);
            out.u8(bullet.bank);
            out.u8(bullet.gunner);
            vec3(out, bullet.from);
            vec3(out, bullet.velocity);
            out.u64(bullet.born);
            out.u64(bullet.until);
            out.u32(bullet.damage as u32);
            out.u8(bullet.target);
        }
        out.u32(self.facts.len() as u32);
        for (number, fact) in &self.facts {
            out.u32(*number as u32);
            encode_fact(out, fact);
        }
        out.u32(self.next_fact as u32);
        encode_fact(out, &self.last);
        out.u8(self.seat);
        encode_overlay(out, &self.overlay);
        out.u64(self.finished.map_or(0, |at| at + 1));
        out.u32(self.runs.len() as u32);
        for run in &self.runs {
            run.encode(out);
        }
        out.u32(self.gunners.len() as u32);
        for gunner in &self.gunners {
            out.u8(gunner.seat);
            out.u16(gunner.aim_from[0] as u16);
            out.u16(gunner.aim_from[1] as u16);
            vec3(out, gunner.offset);
            out.u8(u8::from(gunner.fire_held));
            out.u32(gunner.shots.len() as u32);
            for shot in &gunner.shots {
                out.u64(*shot);
            }
            encode_anims(out, &gunner.anims);
            encode_overlay(out, &gunner.overlay);
        }
        match &self.leaving {
            None => out.u8(0),
            Some(leaving) => {
                out.u8(1);
                out.u8(leaving.run);
                out.u32(leaving.by);
                out.string(&leaving.module);
                out.string(&leaving.tag);
                out.u32(leaving.movies.len() as u32);
                for movie in &leaving.movies {
                    out.string(movie);
                }
                out.u64(leaving.since);
            }
        }
        out.u32(self.voices.len() as u32);
        for voice in &self.voices {
            out.u32(voice.object);
            out.u8(u8::from(voice.state.is_some())
                | u8::from(voice.volume.is_some()) << 1
                | u8::from(voice.pitch.is_some()) << 2
                | u8::from(voice.fade.is_some()) << 3);
            out.u8(voice.state.unwrap_or(0));
            out.u8(voice.volume.unwrap_or(0));
            out.u16(voice.pitch.unwrap_or(0));
            out.u8(voice.plays);
            out.u16(voice.fade.unwrap_or(0));
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Course, DecodeError> {
        Course::decode_run(input, true)
    }

    /// A course, or one of its runs (`whole` false), which has none of its
    /// own.
    fn decode_run(input: &mut Reader, whole: bool) -> Result<Course, DecodeError> {
        let time_ms = input.u64()?;
        let flags = input.u8()?;
        let aim_from = [input.u16()? as i16, input.u16()? as i16];
        let lateral_accel = input.f32()?;
        let jump = input.f32()?;
        let blur = input.f32()?;
        let clip = [input.f32()?, input.f32()?];
        let mut movers = Vec::new();
        for _ in 0..input.u32()? {
            let track = input.f32()?;
            let loops = input.u32()? as i32;
            let offset = read_vec3(input)?;
            let drift = read_vec3(input)?;
            let [speed, min_speed, max_speed, accel] =
                [input.f32()?, input.f32()?, input.f32()?, input.f32()?];
            let tunnel_pos = read_vec3(input)?;
            let tunnel_neg = read_vec3(input)?;
            let hp = input.u32()? as i32;
            let max_hp = input.u32()? as i32;
            let radius = input.f32()?;
            let invulnerable_until = input.u64()?;
            let alive = input.u8()? != 0;
            let died = input.u64()?;
            let anims = decode_anims(input)?;
            let mut shots = Vec::new();
            for _ in 0..input.u32()? {
                shots.push(input.u64()?);
            }
            let mut touching = Vec::new();
            for _ in 0..input.u32()? {
                touching.push(input.u16()?);
            }
            movers.push(Mover {
                track,
                loops,
                offset,
                drift,
                speed,
                min_speed,
                max_speed,
                accel,
                tunnel_pos,
                tunnel_neg,
                hp,
                max_hp,
                radius,
                invulnerable_until,
                alive,
                died,
                anims,
                shots,
                touching,
            });
        }
        let mut stills = Vec::new();
        for _ in 0..input.u32()? {
            let gone = input.u8()? != 0;
            stills.push(Still {
                anims: decode_anims(input)?,
                gone,
            });
        }
        let mut bullets = Vec::new();
        for _ in 0..input.u32()? {
            bullets.push(Bullet {
                shooter: input.u16()?,
                bank: input.u8()?,
                gunner: input.u8()?,
                from: read_vec3(input)?,
                velocity: read_vec3(input)?,
                born: input.u64()?,
                until: input.u64()?,
                damage: input.u32()? as i32,
                target: input.u8()?,
            });
        }
        let mut facts = Vec::new();
        for _ in 0..input.u32()? {
            let number = input.u32()? as i32;
            facts.push((number, decode_fact(input)?));
        }
        let next_fact = input.u32()? as i32;
        let last = decode_fact(input)?;
        let seat = input.u8()?;
        let overlay = decode_overlay(input)?;
        let finished = input.u64()?.checked_sub(1);
        let mut runs = Vec::new();
        let at = input.at();
        let count = input.u32()?;
        if count > 0 && !whole {
            return Err(input.invalid("a run's runs", at));
        }
        for _ in 0..count {
            runs.push(Course::decode_run(input, false)?);
        }
        let mut gunners = Vec::new();
        for _ in 0..input.u32()? {
            let seat = input.u8()?;
            let aim_from = [input.u16()? as i16, input.u16()? as i16];
            let offset = read_vec3(input)?;
            let fire_held = input.u8()? != 0;
            let mut shots = Vec::new();
            for _ in 0..input.u32()? {
                shots.push(input.u64()?);
            }
            let anims = decode_anims(input)?;
            let overlay = decode_overlay(input)?;
            gunners.push(Gunner {
                seat,
                anims,
                overlay,
                aim_from,
                offset,
                fire_held,
                shots,
            });
        }
        let leaving = match input.u8()? {
            0 => None,
            _ => {
                let (run, by) = (input.u8()?, input.u32()?);
                let (module, tag) = (input.string()?, input.string()?);
                let mut movies = Vec::new();
                for _ in 0..input.u32()? {
                    movies.push(input.string()?);
                }
                Some(Leaving {
                    run,
                    by,
                    module,
                    tag,
                    movies,
                    since: input.u64()?,
                })
            }
        };
        let mut voices = Vec::new();
        for _ in 0..input.u32()? {
            let object = input.u32()?;
            let set = input.u8()?;
            let (state, volume, pitch) = (input.u8()?, input.u8()?, input.u16()?);
            let (plays, fade) = (input.u8()?, input.u16()?);
            voices.push(Voice {
                object,
                state: (set & 1 != 0).then_some(state),
                volume: (set & 2 != 0).then_some(volume),
                pitch: (set & 4 != 0).then_some(pitch),
                plays,
                fade: (set & 8 != 0).then_some(fade),
            });
        }
        Ok(Course {
            time_ms,
            started: flags & 1 != 0,
            aim_from,
            fire_held: flags & 2 != 0,
            jump_held: flags & 4 != 0,
            lateral_accel,
            jump,
            bumped: flags & 8 != 0,
            blur,
            clip,
            movers,
            stills,
            bullets,
            facts,
            next_fact,
            last,
            seat,
            overlay,
            finished,
            runs,
            gunners,
            leaving,
            voices,
        })
    }
}

fn encode_overlay(out: &mut Writer, overlay: &Overlay) {
    out.u32(overlay.numbers.len() as u32);
    for (name, value) in &overlay.numbers {
        out.string(name);
        out.u32(*value as u32);
    }
    out.u32(overlay.booleans.len() as u32);
    for (name, value) in &overlay.booleans {
        out.string(name);
        out.u8(u8::from(*value));
    }
    out.u32(overlay.strings.len() as u32);
    for (name, value) in &overlay.strings {
        out.string(name);
        out.string(value);
    }
}

fn decode_overlay(input: &mut Reader) -> Result<Overlay, DecodeError> {
    let mut overlay = Overlay::default();
    for _ in 0..input.u32()? {
        let name = input.string()?;
        overlay.numbers.insert(name, input.u32()? as i32);
    }
    for _ in 0..input.u32()? {
        let name = input.string()?;
        overlay.booleans.insert(name, input.u8()? != 0);
    }
    for _ in 0..input.u32()? {
        let name = input.string()?;
        overlay.strings.insert(name, input.string()?);
    }
    Ok(overlay)
}

/// A gunner's turret turn as its view turned since `aim_from`, within the
/// player's tunnel as the rules start it.
fn turn_turret(rules: &Rules, aim_from: &mut [i16; 2], offset: &mut Vec3, command: &Command) {
    let tunnel = (rules.player.tunnel_neg, rules.player.tunnel_pos);
    turn_within(rules, tunnel, aim_from, offset, command);
}

/// A turret's `offset` (pitch about x, yaw about z) by how far `command`'s
/// view turned since `aim_from`, its `Mouse` axes from the start offset,
/// held within `tunnel` (low, high): a view turned on past a limit takes it
/// from there, with no dead zone coming back.
fn turn_within(
    rules: &Rules,
    (low, high): (Vec3, Vec3),
    aim_from: &mut [i16; 2],
    offset: &mut Vec3,
    command: &Command,
) {
    // JA's pitch is positive looking down, its yaw positive turning left:
    // the offset's pitch is positive looking up.
    let turned = |now: i16, from: i16| math::short_to_degrees(now.wrapping_sub(from));
    let turns = [
        turned(command.angles[1], aim_from[1]),
        -turned(command.angles[0], aim_from[0]),
    ];
    for (k, turn) in turns.into_iter().enumerate() {
        let axis = usize::from(rules.mouse[k]).min(2);
        let sign = if rules.flip[k] { -1.0 } else { 1.0 };
        offset[axis] = rules.player.start_offset[axis] + sign * turn;
    }
    for axis in 0..3 {
        if rules.player.tunnel_infinite[axis] {
            continue;
        }
        let (low, high) = (low[axis], high[axis]);
        if offset[axis] >= low && offset[axis] <= high {
            continue;
        }
        offset[axis] = offset[axis].clamp(low.min(high), high.max(low));
        if let Some(k) = (0..2).find(|&k| usize::from(rules.mouse[k]).min(2) == axis) {
            let sign = if rules.flip[k] { -1.0 } else { 1.0 };
            let turn = (offset[axis] - rules.player.start_offset[axis]) * sign;
            if k == 0 {
                aim_from[1] = command.angles[1].wrapping_sub(math::degrees_to_short(turn));
            } else {
                aim_from[0] = command.angles[0].wrapping_sub(math::degrees_to_short(-turn));
            }
        }
    }
}

/// Plays `anim` on thing `n` among `anims` (`Course::play`).
pub(crate) fn play_into(rules: &Rules, n: usize, anims: &mut Vec<Anim>, anim: Anim) {
    // A looping clip asked for again while it plays goes on from where
    // it is, now over the rest: the swoop's heartbeat asks for its
    // `camshake` and `cDistL` every tick, and they shake and pull the
    // camera back only by running.
    if let Some(k) = anims.iter().position(|playing| {
        anim.looping
            && playing.looping
            && playing.overlay == anim.overlay
            && playing.name.eq_ignore_ascii_case(&anim.name)
    }) {
        let playing = anims.remove(k);
        anims.push(playing);
        return;
    }
    // One in place of the others hides those it covers on every model
    // that has them; a model without it keeps what it played (the
    // turret's hull bar under its compass).
    let covers = |playing: &Anim| rules.covers(n, &anim.name, &playing.name);
    anims.retain(|playing| {
        !playing.name.eq_ignore_ascii_case(&anim.name)
            && (anim.overlay || playing.overlay || !covers(playing))
    });
    anims.push(anim);
    if anims.len() > ANIMS_KEPT {
        // The oldest overlay goes, never the one under them.
        let oldest = anims
            .iter()
            .position(|playing| playing.overlay)
            .unwrap_or(0);
        anims.remove(oldest);
    }
}

/// Whether the segment from, to passes into (or lies in) the box of middle
/// `centre` and half sizes `extent`.
fn passes_into((from, to): (Vec3, Vec3), centre: Vec3, extent: Vec3) -> bool {
    let way = math::sub(to, from);
    let (mut enter, mut leave) = (0.0f32, 1.0f32);
    for axis in 0..3 {
        let (low, high) = (centre[axis] - extent[axis], centre[axis] + extent[axis]);
        if way[axis].abs() <= f32::EPSILON {
            if from[axis] < low || from[axis] > high {
                return false;
            }
            continue;
        }
        let (a, b) = (
            (low - from[axis]) / way[axis],
            (high - from[axis]) / way[axis],
        );
        enter = enter.max(a.min(b));
        leave = leave.min(a.max(b));
        if enter > leave {
            return false;
        }
    }
    true
}

/// How near two segments come (each from, to; metres).
fn closest_between((p0, p1): (Vec3, Vec3), (q0, q1): (Vec3, Vec3)) -> f32 {
    let (d1, d2, r) = (math::sub(p1, p0), math::sub(q1, q0), math::sub(p0, q0));
    let (a, e, f) = (math::dot(d1, d1), math::dot(d2, d2), math::dot(d2, r));
    let (s, t) = if a <= f32::EPSILON && e <= f32::EPSILON {
        (0.0, 0.0)
    } else if a <= f32::EPSILON {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = math::dot(d1, r);
        if e <= f32::EPSILON {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = math::dot(d1, d2);
            let denominator = a * e - b * b;
            let s = if denominator > f32::EPSILON {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t = (b * s + f) / e;
            if t < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s, t)
            }
        }
    };
    math::length(math::sub(math::mad(p0, s, d1), math::mad(q0, t, d2)))
}

/// `v` turned by `q` (x, y, z, w).
fn rotate(q: [f32; 4], v: Vec3) -> Vec3 {
    let axis = [q[0], q[1], q[2]];
    let t = math::scale(math::cross(axis, v), 2.0);
    math::add(math::add(v, math::scale(t, q[3])), math::cross(axis, t))
}

fn vec3(out: &mut Writer, v: Vec3) {
    for value in v {
        out.f32(value);
    }
}

fn read_vec3(input: &mut Reader) -> Result<Vec3, DecodeError> {
    Ok([input.f32()?, input.f32()?, input.f32()?])
}

fn encode_anims(out: &mut Writer, anims: &[Anim]) {
    out.u32(anims.len() as u32);
    for anim in anims {
        out.string(&anim.name);
        out.u8(u8::from(anim.looping) | u8::from(anim.overlay) << 1);
        out.u64(anim.since);
    }
}

fn decode_anims(input: &mut Reader) -> Result<Vec<Anim>, DecodeError> {
    let mut anims = Vec::new();
    for _ in 0..input.u32()? {
        let name = input.string()?;
        let flags = input.u8()?;
        anims.push(Anim {
            name,
            looping: flags & 1 != 0,
            overlay: flags & 2 != 0,
            since: input.u64()?,
        });
    }
    Ok(anims)
}

/// A number that may be none: one more than it, 0 for none.
fn maybe(out: &mut Writer, n: Option<u16>) {
    out.u16(n.map_or(0, |n| n + 1));
}

fn read_maybe(input: &mut Reader) -> Result<Option<u16>, DecodeError> {
    Ok(input.u16()?.checked_sub(1))
}

fn encode_fact(out: &mut Writer, fact: &Fact) {
    maybe(out, fact.follower);
    maybe(out, fact.obstacle);
    out.u32(fact.damage as u32);
    maybe(out, fact.target);
    maybe(out, fact.shooter);
    out.u32(fact.hp_change as u32);
}

fn decode_fact(input: &mut Reader) -> Result<Fact, DecodeError> {
    Ok(Fact {
        follower: read_maybe(input)?,
        obstacle: read_maybe(input)?,
        damage: input.u32()? as i32,
        target: read_maybe(input)?,
        shooter: read_maybe(input)?,
        hp_change: input.u32()? as i32,
    })
}
